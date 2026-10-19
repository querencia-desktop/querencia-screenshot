// SPDX-License-Identifier: GPL-3.0-or-later

//! Das aktive Fenster für `--window` (Alt+Druck).
//!
//! labwc 0.9.8 meldet Fenster über `zwlr_foreign_toplevel_manager_v1`:
//! Titel, App-ID, Zustand (aktiviert, maximiert, Vollbild) und die Ausgaben,
//! auf denen ein Fenster liegt – aber **keine Fenstergeometrie**. Eine
//! Aufnahme eines einzelnen Fensters (`ext_foreign_toplevel_image_capture_
//! source_manager_v1`) bietet labwc 0.9.8 ebenfalls nicht an. Exakt
//! aufnehmen lässt sich ein Fenster deshalb nur, wenn es im Vollbild genau
//! eine Ausgabe füllt; sonst rahmt der Benutzer es ein (siehe `plan`).

use std::collections::HashMap;

use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle,
    backend::ObjectId,
    event_created_child,
    protocol::{wl_output, wl_registry},
};
use wayland_protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1::{self, ZwlrForeignToplevelHandleV1},
    zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1},
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

// Werte aus wlr-foreign-toplevel-management-unstable-v1, `enum state`.
const STATE_MAXIMIZED: u32 = 0;
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
struct Collector {
    manager: Option<ZwlrForeignToplevelManagerV1>,
    outputs: HashMap<ObjectId, (wl_output::WlOutput, Option<String>)>,
    toplevels: Vec<(ObjectId, Toplevel)>,
}

impl Collector {
    fn toplevel(&mut self, id: &ObjectId) -> Option<&mut Toplevel> {
        self.toplevels
            .iter_mut()
            .find(|(known, _)| known == id)
            .map(|(_, toplevel)| toplevel)
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

/// Fragt den Compositor (WAYLAND_DISPLAY) nach dem aktiven Fenster. Liest
/// nur; es wird kein Fenster verändert.
pub fn query_active() -> WindowQuery {
    let connection = match Connection::connect_to_env() {
        Ok(connection) => connection,
        Err(error) => {
            return WindowQuery::Unsupported(format!("keine Wayland-Verbindung: {error}"));
        }
    };
    let mut queue = connection.new_event_queue();
    let handle = queue.handle();
    connection.display().get_registry(&handle, ());
    let mut collector = Collector::default();
    // 1. Globale Objekte, 2. Ausgabenamen und Fensterliste, 3. Fensterzustände.
    for _ in 0..3 {
        if let Err(error) = queue.roundtrip(&mut collector) {
            return WindowQuery::Unsupported(format!("Wayland-Abfrage fehlgeschlagen: {error}"));
        }
    }
    let Some(manager) = collector.manager.take() else {
        return WindowQuery::Unsupported(
            "der Compositor bietet zwlr_foreign_toplevel_manager_v1 nicht an".to_owned(),
        );
    };
    let result = match collector.active() {
        Some(window) => WindowQuery::Active(window),
        None => WindowQuery::NoActiveWindow,
    };
    manager.stop();
    let _ = connection.flush();
    result
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
