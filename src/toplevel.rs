// SPDX-License-Identifier: MIT

//! Das aktive Fenster für `--window` (Alt+Druck).
//!
//! labwc 0.9.8 meldet Fenster über `zwlr_foreign_toplevel_manager_v1`:
//! Titel, App-ID, Zustand (aktiviert, maximiert, Vollbild) und die Ausgaben,
//! auf denen ein Fenster liegt – aber **keine Fenstergeometrie**. Eine
//! Aufnahme eines einzelnen Fensters (`ext_foreign_toplevel_image_capture_
//! source_manager_v1`) bietet labwc 0.9.8 ebenfalls nicht an. Exakt
//! aufnehmen lässt sich ein Fenster deshalb nur, wenn es im Vollbild genau
//! eine Ausgabe füllt; sonst wählt der Benutzer das Fenster im eingefrorenen
//! Bild (`window_pick`) oder rahmt es ein (siehe `plan`).
//!
//! Für die Fensterwahl liefert `query_desktop` zusätzlich die Ausgaben mit
//! Lage und Größe (wlr-output-management, nur gelesen – wl_output meldet
//! unter wlroots keine Lage), welche Ausgaben ein maximiertes Fenster zeigen
//! und deren freie Fläche (eine nie sichtbare Layer-Shell-Fläche mit allen
//! vier Ankern bekommt sie als Größe gemeldet).

use std::collections::HashMap;

use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle, WEnum,
    backend::ObjectId,
    event_created_child,
    protocol::{wl_compositor, wl_output, wl_registry, wl_surface},
};
use wayland_protocols_wlr::{
    foreign_toplevel::v1::client::{
        zwlr_foreign_toplevel_handle_v1::{self, ZwlrForeignToplevelHandleV1},
        zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1},
    },
    layer_shell::v1::client::{
        zwlr_layer_shell_v1::{self, ZwlrLayerShellV1},
        zwlr_layer_surface_v1::{self, ZwlrLayerSurfaceV1},
    },
    output_management::v1::client::{
        zwlr_output_head_v1::{self, ZwlrOutputHeadV1},
        zwlr_output_manager_v1::{self, ZwlrOutputManagerV1},
        zwlr_output_mode_v1::{self, ZwlrOutputModeV1},
    },
};

/// Was über das aktive Fenster bekannt ist.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ActiveWindow {
    pub title: String,
    pub app_id: String,
    pub fullscreen: bool,
    pub maximized: bool,
    /// Namen der Ausgaben (wl_output v4 `name`), auf denen das Fenster liegt.
    pub outputs: Vec<String>,
}

impl ActiveWindow {
    /// Kurzer Name für Meldungen: Titel, sonst App-ID.
    pub fn label(&self) -> String {
        let name = if self.title.trim().is_empty() {
            self.app_id.trim()
        } else {
            self.title.trim()
        };
        if name.is_empty() {
            return "ohne Titel".to_owned();
        }
        let short: String = name.chars().take(60).collect();
        if short.len() < name.len() {
            format!("{short}…")
        } else {
            short
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WindowQuery {
    Active(ActiveWindow),
    /// Der Compositor meldet Fenster, aber keines ist aktiv (z. B. der
    /// Schreibtisch hat den Fokus).
    NoActiveWindow,
    /// Abfrage nicht möglich (keine Wayland-Verbindung, Protokoll fehlt).
    Unsupported(String),
}

/// Wie `--window` aufnimmt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WindowPlan {
    /// Vollbildfenster auf genau einer Ausgabe: diese Ausgabe ist exakt
    /// das Fenster.
    Output {
        output: String,
        window: ActiveWindow,
    },
    /// Fenstergrenzen unbekannt: der Benutzer rahmt das Fenster ein.
    Frame { window: Option<ActiveWindow> },
    /// Kein aktives Fenster: wie GNOME 2 den ganzen Bildschirm.
    Everything,
}

pub fn plan(query: WindowQuery) -> WindowPlan {
    match query {
        WindowQuery::Active(window) if window.fullscreen && window.outputs.len() == 1 => {
            WindowPlan::Output {
                output: window.outputs[0].clone(),
                window,
            }
        }
        WindowQuery::Active(window) => WindowPlan::Frame {
            window: Some(window),
        },
        WindowQuery::NoActiveWindow => WindowPlan::Everything,
        WindowQuery::Unsupported(_) => WindowPlan::Frame { window: None },
    }
}

/// Eine Ausgabe in Layout-Koordinaten (logische Pixel).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OutputInfo {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    /// Größe der freien Fläche ohne Leisten (Layer-Shell), falls erfragt.
    pub usable: Option<(i32, i32)>,
    /// Hier liegt ein maximiertes, nicht minimiertes Fenster.
    pub has_maximized: bool,
}

/// Was die Fensterwahl vom Compositor weiß.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Desktop {
    pub query: WindowQuery,
    pub outputs: Vec<OutputInfo>,
    /// Anzahl gemeldeter (nicht geschlossener) Fenster.
    pub windows: usize,
}

impl Desktop {
    /// Nur das aktive Fenster, ohne Ausgaben (Tests, Rückfall).
    pub fn without_outputs(query: WindowQuery) -> Self {
        Self {
            query,
            outputs: Vec::new(),
            windows: 0,
        }
    }
}

// Werte aus wlr-foreign-toplevel-management-unstable-v1, `enum state`.
const STATE_MAXIMIZED: u32 = 0;
const STATE_MINIMIZED: u32 = 1;
const STATE_ACTIVATED: u32 = 2;
const STATE_FULLSCREEN: u32 = 3;

#[derive(Default)]
struct Toplevel {
    title: String,
    app_id: String,
    states: Vec<u32>,
    outputs: Vec<ObjectId>,
    closed: bool,
}

#[derive(Default)]
struct Head {
    name: String,
    enabled: bool,
    position: (i32, i32),
    current_mode: Option<ObjectId>,
    scale: f64,
    /// 90°/270° gedreht: Breite und Höhe tauschen.
    rotated: bool,
}

#[derive(Default)]
struct Collector {
    manager: Option<ZwlrForeignToplevelManagerV1>,
    outputs: HashMap<ObjectId, (wl_output::WlOutput, Option<String>)>,
    toplevels: Vec<(ObjectId, Toplevel)>,
    output_manager: Option<ZwlrOutputManagerV1>,
    heads: Vec<(ObjectId, Head)>,
    modes: HashMap<ObjectId, (i32, i32)>,
    compositor: Option<wl_compositor::WlCompositor>,
    layer_shell: Option<ZwlrLayerShellV1>,
    usable: HashMap<String, (i32, i32)>,
}

impl Collector {
    fn toplevel(&mut self, id: &ObjectId) -> Option<&mut Toplevel> {
        self.toplevels
            .iter_mut()
            .find(|(known, _)| known == id)
            .map(|(_, toplevel)| toplevel)
    }

    fn output_name(&self, id: &ObjectId) -> Option<String> {
        self.outputs.get(id).and_then(|(_, name)| name.clone())
    }

    /// Ausgaben (eingeschaltet, mit Modus) in logischen Pixeln.
    fn output_infos(&self) -> Vec<OutputInfo> {
        let maximized: Vec<String> = self
            .toplevels
            .iter()
            .map(|(_, toplevel)| toplevel)
            .filter(|t| {
                !t.closed
                    && t.states.contains(&STATE_MAXIMIZED)
                    && !t.states.contains(&STATE_MINIMIZED)
                    && !t.states.contains(&STATE_FULLSCREEN)
            })
            .flat_map(|t| t.outputs.iter().filter_map(|id| self.output_name(id)))
            .collect();
        self.heads
            .iter()
            .map(|(_, head)| head)
            .filter(|head| head.enabled && !head.name.is_empty())
            .filter_map(|head| {
                let &(w, h) = self.modes.get(head.current_mode.as_ref()?)?;
                let scale = if head.scale > 0.0 { head.scale } else { 1.0 };
                let (w, h) = if head.rotated { (h, w) } else { (w, h) };
                Some(OutputInfo {
                    name: head.name.clone(),
                    x: head.position.0,
                    y: head.position.1,
                    width: (f64::from(w) / scale).round() as i32,
                    height: (f64::from(h) / scale).round() as i32,
                    usable: self.usable.get(&head.name).copied(),
                    has_maximized: maximized.contains(&head.name),
                })
            })
            .collect()
    }

    fn active(&self) -> Option<ActiveWindow> {
        self.toplevels
            .iter()
            .rev()
            .map(|(_, toplevel)| toplevel)
            .find(|toplevel| !toplevel.closed && toplevel.states.contains(&STATE_ACTIVATED))
            .map(|toplevel| ActiveWindow {
                title: toplevel.title.clone(),
                app_id: toplevel.app_id.clone(),
                fullscreen: toplevel.states.contains(&STATE_FULLSCREEN),
                maximized: toplevel.states.contains(&STATE_MAXIMIZED),
                outputs: toplevel
                    .outputs
                    .iter()
                    .filter_map(|id| self.outputs.get(id).and_then(|(_, name)| name.clone()))
                    .collect(),
            })
    }
}

/// Der Zustand kommt als Feld aus u32 in Maschinenbyte-Reihenfolge.
fn decode_states(raw: &[u8]) -> Vec<u32> {
    raw.chunks_exact(4)
        .filter_map(|chunk| chunk.try_into().ok().map(u32::from_ne_bytes))
        .collect()
}

impl Dispatch<wl_registry::WlRegistry, ()> for Collector {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        queue: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            if interface == ZwlrForeignToplevelManagerV1::interface().name {
                state.manager = Some(registry.bind(name, version.min(3), queue, ()));
            } else if interface == ZwlrOutputManagerV1::interface().name {
                state.output_manager = Some(registry.bind(name, version.min(2), queue, ()));
            } else if interface == wl_compositor::WlCompositor::interface().name {
                state.compositor = Some(registry.bind(name, version.min(4), queue, ()));
            } else if interface == ZwlrLayerShellV1::interface().name {
                state.layer_shell = Some(registry.bind(name, version.min(3), queue, ()));
            } else if interface == wl_output::WlOutput::interface().name && version >= 4 {
                let output: wl_output::WlOutput = registry.bind(name, 4, queue, ());
                state.outputs.insert(output.id(), (output, None));
            }
        }
    }
}

impl Dispatch<wl_output::WlOutput, ()> for Collector {
    fn event(
        state: &mut Self,
        output: &wl_output::WlOutput,
        event: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_output::Event::Name { name } = event
            && let Some((_, known)) = state.outputs.get_mut(&output.id())
        {
            *known = Some(name);
        }
    }
}

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for Collector {
    fn event(
        state: &mut Self,
        _: &ZwlrForeignToplevelManagerV1,
        event: zwlr_foreign_toplevel_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } = event {
            state.toplevels.push((toplevel.id(), Toplevel::default()));
        }
    }

    event_created_child!(Collector, ZwlrForeignToplevelManagerV1, [
        zwlr_foreign_toplevel_manager_v1::EVT_TOPLEVEL_OPCODE => (ZwlrForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for Collector {
    fn event(
        state: &mut Self,
        handle: &ZwlrForeignToplevelHandleV1,
        event: zwlr_foreign_toplevel_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use zwlr_foreign_toplevel_handle_v1::Event;
        let Some(toplevel) = state.toplevel(&handle.id()) else {
            return;
        };
        match event {
            Event::Title { title } => toplevel.title = title,
            Event::AppId { app_id } => toplevel.app_id = app_id,
            Event::State { state } => toplevel.states = decode_states(&state),
            Event::OutputEnter { output } => toplevel.outputs.push(output.id()),
            Event::OutputLeave { output } => toplevel.outputs.retain(|id| *id != output.id()),
            Event::Closed => toplevel.closed = true,
            _ => {}
        }
    }
}

impl Dispatch<ZwlrOutputManagerV1, ()> for Collector {
    fn event(
        state: &mut Self,
        _: &ZwlrOutputManagerV1,
        event: zwlr_output_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwlr_output_manager_v1::Event::Head { head } = event {
            state.heads.push((
                head.id(),
                Head {
                    scale: 1.0,
                    ..Head::default()
                },
            ));
        }
    }

    event_created_child!(Collector, ZwlrOutputManagerV1, [
        zwlr_output_manager_v1::EVT_HEAD_OPCODE => (ZwlrOutputHeadV1, ()),
    ]);
}

impl Dispatch<ZwlrOutputHeadV1, ()> for Collector {
    fn event(
        state: &mut Self,
        head: &ZwlrOutputHeadV1,
        event: zwlr_output_head_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use zwlr_output_head_v1::Event;
        let Some((_, known)) = state.heads.iter_mut().find(|(id, _)| *id == head.id()) else {
            return;
        };
        match event {
            Event::Name { name } => known.name = name,
            Event::Enabled { enabled } => known.enabled = enabled != 0,
            Event::CurrentMode { mode } => known.current_mode = Some(mode.id()),
            Event::Position { x, y } => known.position = (x, y),
            Event::Scale { scale } => known.scale = scale,
            Event::Transform { transform } => {
                known.rotated = matches!(
                    transform,
                    WEnum::Value(
                        wl_output::Transform::_90
                            | wl_output::Transform::_270
                            | wl_output::Transform::Flipped90
                            | wl_output::Transform::Flipped270
                    )
                );
            }
            _ => {}
        }
    }

    event_created_child!(Collector, ZwlrOutputHeadV1, [
        zwlr_output_head_v1::EVT_MODE_OPCODE => (ZwlrOutputModeV1, ()),
    ]);
}

impl Dispatch<ZwlrOutputModeV1, ()> for Collector {
    fn event(
        state: &mut Self,
        mode: &ZwlrOutputModeV1,
        event: zwlr_output_mode_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwlr_output_mode_v1::Event::Size { width, height } = event {
            state.modes.insert(mode.id(), (width, height));
        }
    }
}

impl Dispatch<wl_compositor::WlCompositor, ()> for Collector {
    fn event(
        _: &mut Self,
        _: &wl_compositor::WlCompositor,
        _: wl_compositor::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_surface::WlSurface, ()> for Collector {
    fn event(
        _: &mut Self,
        _: &wl_surface::WlSurface,
        _: wl_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrLayerShellV1, ()> for Collector {
    fn event(
        _: &mut Self,
        _: &ZwlrLayerShellV1,
        _: zwlr_layer_shell_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrLayerSurfaceV1, String> for Collector {
    fn event(
        state: &mut Self,
        _: &ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        output: &String,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwlr_layer_surface_v1::Event::Configure { width, height, .. } = event {
            state
                .usable
                .insert(output.clone(), (width as i32, height as i32));
        }
    }
}

/// Aktives Fenster, Ausgaben und maximierte Fenster für die Fensterwahl.
/// Liest nur; kein Fenster und keine Ausgabe wird verändert. Die
/// Layer-Shell-Flächen für die freie Fläche bekommen nie einen Puffer, sind
/// also nie zu sehen, und werden sofort wieder zerstört.
pub fn query_desktop() -> Desktop {
    let unsupported = |message: String| Desktop::without_outputs(WindowQuery::Unsupported(message));
    let connection = match Connection::connect_to_env() {
        Ok(connection) => connection,
        Err(error) => return unsupported(format!("keine Wayland-Verbindung: {error}")),
    };
    let mut queue = connection.new_event_queue();
    let handle = queue.handle();
    connection.display().get_registry(&handle, ());
    let mut collector = Collector::default();
    // 1. Globale Objekte, 2. Ausgabenamen, Fensterliste, Köpfe,
    // 3. Fensterzustände und Modi.
    for _ in 0..3 {
        if let Err(error) = queue.roundtrip(&mut collector) {
            return unsupported(format!("Wayland-Abfrage fehlgeschlagen: {error}"));
        }
    }
    let Some(manager) = collector.manager.take() else {
        return unsupported(
            "der Compositor bietet zwlr_foreign_toplevel_manager_v1 nicht an".to_owned(),
        );
    };
    let query = match collector.active() {
        Some(window) => WindowQuery::Active(window),
        None => WindowQuery::NoActiveWindow,
    };
    // Freie Fläche nur, wo ein maximiertes Fenster liegt.
    let wanted: Vec<String> = collector
        .output_infos()
        .into_iter()
        .filter(|o| o.has_maximized)
        .map(|o| o.name)
        .collect();
    if let (Some(compositor), Some(layer_shell)) = (&collector.compositor, &collector.layer_shell)
        && !wanted.is_empty()
    {
        let mut probes = Vec::new();
        for (output, name) in collector.outputs.values() {
            let Some(name) = name.clone().filter(|n| wanted.contains(n)) else {
                continue;
            };
            let surface = compositor.create_surface(&handle, ());
            let layer = layer_shell.get_layer_surface(
                &surface,
                Some(output),
                zwlr_layer_shell_v1::Layer::Background,
                "querencia-screenshot-freie-flaeche".to_owned(),
                &handle,
                name,
            );
            layer.set_anchor(zwlr_layer_surface_v1::Anchor::all());
            layer.set_size(0, 0);
            layer.set_exclusive_zone(0);
            surface.commit();
            probes.push((layer, surface));
        }
        let _ = queue.roundtrip(&mut collector);
        for (layer, surface) in probes {
            layer.destroy();
            surface.destroy();
        }
    }
    let outputs = collector.output_infos();
    let windows = collector
        .toplevels
        .iter()
        .filter(|(_, t)| !t.closed)
        .count();
    manager.stop();
    if let Some(output_manager) = collector.output_manager.take() {
        output_manager.stop();
    }
    let _ = connection.flush();
    Desktop {
        query,
        outputs,
        windows,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(fullscreen: bool, outputs: &[&str]) -> ActiveWindow {
        ActiveWindow {
            title: "Dokument – Texteditor".into(),
            app_id: "org.example.Editor".into(),
            fullscreen,
            maximized: false,
            outputs: outputs.iter().map(|name| (*name).to_owned()).collect(),
        }
    }

    #[test]
    fn only_a_fullscreen_window_on_one_output_is_captured_exactly() {
        assert_eq!(
            plan(WindowQuery::Active(window(true, &["HEADLESS-1"]))),
            WindowPlan::Output {
                output: "HEADLESS-1".into(),
                window: window(true, &["HEADLESS-1"])
            }
        );
        for (fullscreen, outputs) in [
            (false, &["HEADLESS-1"][..]),
            (true, &[][..]),
            (true, &["HEADLESS-1", "HEADLESS-2"][..]),
        ] {
            assert_eq!(
                plan(WindowQuery::Active(window(fullscreen, outputs))),
                WindowPlan::Frame {
                    window: Some(window(fullscreen, outputs))
                },
                "{fullscreen} {outputs:?}: keine erfundene Fenstergrenze"
            );
        }
    }

    #[test]
    fn without_active_window_the_whole_screen_is_taken_like_gnome_2() {
        assert_eq!(plan(WindowQuery::NoActiveWindow), WindowPlan::Everything);
        assert_eq!(
            plan(WindowQuery::Unsupported("x".into())),
            WindowPlan::Frame { window: None }
        );
    }

    #[test]
    fn states_are_native_endian_u32_values() {
        let raw: Vec<u8> = [STATE_ACTIVATED, STATE_FULLSCREEN]
            .iter()
            .flat_map(|value| value.to_ne_bytes())
            .collect();
        assert_eq!(decode_states(&raw), vec![STATE_ACTIVATED, STATE_FULLSCREEN]);
        assert_eq!(decode_states(&[1, 2, 3]), Vec::<u32>::new());
    }

    #[test]
    fn labels_prefer_the_title_and_are_bounded() {
        assert_eq!(window(false, &[]).label(), "Dokument – Texteditor");
        let untitled = ActiveWindow {
            app_id: "foot".into(),
            ..ActiveWindow::default()
        };
        assert_eq!(untitled.label(), "foot");
        assert_eq!(ActiveWindow::default().label(), "ohne Titel");
        let long = ActiveWindow {
            title: "ä".repeat(100),
            ..ActiveWindow::default()
        };
        assert_eq!(long.label(), format!("{}…", "ä".repeat(60)));
    }
}
