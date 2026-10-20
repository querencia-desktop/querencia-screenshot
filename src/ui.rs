// SPDX-License-Identifier: GPL-3.0-or-later

//! Das Auswahlfenster (Druck, QS-0d): „Was möchten Sie aufnehmen?“ mit
//! drei Aufnahmearten – ganzer Bildschirm, Fenster einrahmen, Bereich
//! auswählen. Nach der Aufnahme öffnet sich der Editor (`editor`).
//!
//! Die Aufnahme beginnt erst, wenn das Auswahlfenster vom Compositor
//! tatsächlich entfernt wurde (`unmap`) und ein weiteres Bild gezeichnet
//! ist; so steht es nie im Bild.

use std::{cell::RefCell, rc::Rc, time::Duration};

use gtk::{gdk, gio, glib, graphene, gsk, prelude::*};

use crate::{
    cli::Target,
    editor,
    i18n::{gettext, gettext_f},
    instant, runner, toplevel,
};

/// Zeit nach dem Unmap, die dem Compositor für sein nächstes Bild bleibt;
/// Ergänzung zum Unmap-Signal, nicht dessen Ersatz.
const COMPOSITOR_GRACE: Duration = Duration::from_millis(150);

/// Fläche der Vorschau im Speicherdialog. Die Aufnahme wird nur zur Anzeige
/// hineinskaliert; gespeichert und kopiert werden immer die vollständigen
/// PNG-Daten.
const PREVIEW_WIDTH: i32 = 480;
const PREVIEW_HEIGHT: i32 = 270;

// Fensterdekoration: Kein Fenster setzt eine eigene Titelleiste
// (`set_titlebar`/HeaderBar). GTK 4 fordert dann über xdg-decoration die
// Dekoration des Compositors an, und labwc zeichnet Titel, Minimieren,
// Maximieren und Schließen genau wie bei Querencia Terminal und Files – eine
// Leiste, keine nachgebauten Knöpfe (QS-0d-Deko; bis dahin GTK-CSD mit
// eigener HeaderBar).
//
// Fenstersymbol: Fenster bekommen **kein** eigenes Symbol (`set_icon_name`). GTK 4.22
// schickt es über `xdg_toplevel_icon_v1` an labwc 0.9.8; schloss sich ein
// Fenster mit Symbol (Speicherdialog, Auswahlfenster), während ein anderes
// desselben Prozesses offen blieb (Editor), stürzte das Programm beim
// Freigeben der Symbolpuffer reproduzierbar ab (QS-0d, gdb +
// WAYLAND_DEBUG). Panel und Fensterliste finden das Symbol ohnehin über die
// App-ID `org.querencia.Screenshot` und deren Desktop-Datei.

/// CSS-Klasse, an der das Auswahlfenster wiedererkannt wird.
const CHOOSER_CLASS: &str = "qs-chooser";

struct Choice {
    target: Target,
    icons: &'static [&'static str],
}

const CHOICES: [Choice; 3] = [
    Choice {
        target: Target::Screen,
        icons: &["video-display", "computer", "applets-screenshooter"],
    },
    Choice {
        target: Target::Window,
        icons: &["window-new", "preferences-system-windows", "view-restore"],
    },
    Choice {
        target: Target::Area,
        icons: &["select-rectangular", "edit-select-all", "object-select"],
    },
];

impl Choice {
    /// Beschriftung mit Zugriffstaste und Erklärung darunter (übersetzt).
    fn texts(&self) -> (String, String) {
        // TRANSLATORS: Auswahlfenster „What would you like to capture?“; die
        // Zugriffstasten der drei Beschriftungen müssen verschieden sein.
        match self.target {
            Target::Screen => (
                gettext("Grab the whole _screen"),
                gettext("Everything that can be seen on the screen (all screens in one picture)."),
            ),
            Target::Window => (
                gettext("_Frame a window"),
                gettext(
                    "Click a window: detected windows are highlighted when the pointer \
                     moves over them. Frame other windows with the mouse. \
                     A full-screen window is captured directly.",
                ),
            ),
            Target::Area => (
                gettext("Select an _area"),
                gettext("Drag a rectangle while holding the mouse button. Esc cancels."),
            ),
        }
    }
}

struct State {
    window: gtk::ApplicationWindow,
    choices: gtk::Box,
    status: gtk::Label,
    busy: RefCell<bool>,
}

/// Symbolthema des GNOME-2-Aussehens, wie bei Panel und Desktop nur für
/// diesen Prozess.
const CLASSIC_ICON_THEME: &str = "gnome-human";

/// Das Symbolthema, auf das umgeschaltet wird: GNOME Human, wenn es
/// installiert ist und nur GTKs eingebauter Standard eingestellt ist (gleiche
/// Regel wie `querencia-panel`). Keine Datei, keine andere Anwendung ändert
/// sich.
fn classic_icon_theme(current: &str, installed: impl Fn(&str) -> bool) -> Option<&'static str> {
    let built_in_default = matches!(current, "" | "Adwaita" | "hicolor");
    (built_in_default && installed(CLASSIC_ICON_THEME)).then_some(CLASSIC_ICON_THEME)
}

fn use_classic_icon_theme() {
    let Some(settings) = gtk::Settings::default() else {
        return;
    };
    let current = settings
        .gtk_icon_theme_name()
        .map(|name| name.to_string())
        .unwrap_or_default();
    let mut data_dirs = vec![glib::user_data_dir()];
    data_dirs.extend(glib::system_data_dirs());
    let installed = |name: &str| {
        data_dirs
            .iter()
            .any(|dir| dir.join("icons").join(name).join("index.theme").is_file())
    };
    if let Some(theme) = classic_icon_theme(&current, installed) {
        settings.set_gtk_icon_theme_name(Some(theme));
    }
}

pub fn load_styles() {
    use_classic_icon_theme();
    let provider = gtk::CssProvider::new();
    provider.load_from_string(include_str!("style.css"));
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

/// Zeigt das Auswahlfenster; ein schon offenes kommt nach vorn. Offene
/// Editoren bleiben davon unberührt.
pub fn present(application: &gtk::Application) {
    if let Some(window) = application
        .windows()
        .into_iter()
        .find(|window| window.has_css_class(CHOOSER_CLASS))
    {
        window.present();
        return;
    }
    let window = gtk::ApplicationWindow::builder()
        .application(application)
        .title("Querencia Screenshot")
        .resizable(false)
        .build();
    window.add_css_class("qs-window");
    window.add_css_class(CHOOSER_CLASS);
    // Kein set_icon_name, siehe „Fenstersymbol“ in ui.rs.
    // Keine eigene Titelleiste, siehe „Fensterdekoration“ in ui.rs.

    let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
    root.set_margin_top(10);
    root.set_margin_bottom(10);
    root.set_margin_start(12);
    root.set_margin_end(12);
    let heading = gtk::Label::new(Some(&gettext("What would you like to capture?")));
    heading.set_xalign(0.0);
    heading.add_css_class("qs-heading");
    root.append(&heading);

    let choices = gtk::Box::new(gtk::Orientation::Vertical, 6);
    root.append(&choices);
    let status = gtk::Label::new(Some(&gettext(
        "After the capture the editor opens for annotating, saving and copying.",
    )));
    status.add_css_class("qs-status");
    status.set_xalign(0.0);
    status.set_wrap(true);
    status.set_max_width_chars(56);
    status.set_selectable(true);
    root.append(&status);

    let bottom = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    bottom.set_halign(gtk::Align::End);
    let cancel = button(&gettext("Cancel"), &["process-stop", "window-close"]);
    bottom.append(&cancel);
    root.append(&bottom);
    window.set_child(Some(&root));

    let state = Rc::new(State {
        window: window.clone(),
        choices: choices.clone(),
        status,
        busy: RefCell::new(false),
    });

    let mut first = None;
    for choice in &CHOICES {
        let control = choice_button(choice);
        let state = state.clone();
        let target = choice.target;
        control.connect_clicked(move |_| begin_capture(&state, target));
        choices.append(&control);
        first.get_or_insert(control);
    }
    {
        let window = window.clone();
        cancel.connect_clicked(move |_| window.close());
    }
    let keys = gtk::EventControllerKey::new();
    {
        let window = window.clone();
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            if closes_chooser(key, modifiers) {
                window.close();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
    }
    window.add_controller(keys);

    let missing = crate::capture::missing_tools(&runner::SystemRunner);
    if !missing.is_empty() {
        state.status.set_text(&gettext_f(
            "Missing programs: {programs}. Window and area need slurp, copying needs wl-copy (package wl-clipboard).",
            &[("programs", &missing.join(", "))],
        ));
    }
    window.present();
    if let Some(first) = first {
        first.grab_focus();
    }
}

/// Esc, Strg+W und Strg+Q schließen das Auswahlfenster (wie den Editor).
fn closes_chooser(key: gdk::Key, modifiers: gdk::ModifierType) -> bool {
    key == gdk::Key::Escape || close_shortcut(key, modifiers)
}

/// Strg+W oder Strg+Q (Feststell- und NumLock egal, keine weitere
/// Zusatztaste) – die Schließkürzel von Editor und Auswahlfenster.
pub fn close_shortcut(key: gdk::Key, modifiers: gdk::ModifierType) -> bool {
    let control = modifiers
        & (gdk::ModifierType::CONTROL_MASK
            | gdk::ModifierType::SHIFT_MASK
            | gdk::ModifierType::ALT_MASK
            | gdk::ModifierType::SUPER_MASK)
        == gdk::ModifierType::CONTROL_MASK;
    control && matches!(key.to_lower(), gdk::Key::w | gdk::Key::q)
}

/// Große Schaltfläche: Symbol, fette Beschriftung mit Zugriffstaste und eine
/// kurze Erklärung darunter.
fn choice_button(choice: &Choice) -> gtk::Button {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    if let Some(image) = theme_icon(choice.icons) {
        image.set_pixel_size(32);
        image.set_valign(gtk::Align::Start);
        content.append(&image);
    }
    let texts = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let (title, explanation) = choice.texts();
    let label = gtk::Label::with_mnemonic(&title);
    label.set_xalign(0.0);
    label.add_css_class("qs-choice-title");
    let detail = gtk::Label::new(Some(&explanation));
    detail.set_xalign(0.0);
    detail.set_wrap(true);
    detail.set_max_width_chars(52);
    detail.add_css_class("qs-hint");
    texts.append(&label);
    texts.append(&detail);
    content.append(&texts);
    let button = gtk::Button::new();
    button.set_child(Some(&content));
    button.add_css_class("qs-button");
    button.add_css_class("qs-choice");
    label.set_mnemonic_widget(Some(&button));
    button
}

/// Im Auswahlfenster hat der Benutzer „Fenster einrahmen“ ausdrücklich
/// gewählt: Ohne aktives Fenster wird trotzdem eingerahmt, nicht (wie bei
/// Alt+Druck nach GNOME 2) der ganze Bildschirm genommen.
fn chooser_desktop() -> toplevel::Desktop {
    let desktop = toplevel::query_desktop();
    toplevel::Desktop {
        query: window_query_for_chooser(desktop.query.clone()),
        ..desktop
    }
}

fn window_query_for_chooser(query: toplevel::WindowQuery) -> toplevel::WindowQuery {
    match query {
        toplevel::WindowQuery::NoActiveWindow => {
            toplevel::WindowQuery::Unsupported("kein aktives Fenster".to_owned())
        }
        other => other,
    }
}

fn begin_capture(state: &Rc<State>, target: Target) {
    if *state.busy.borrow() {
        return;
    }
    *state.busy.borrow_mut() = true;
    state.choices.set_sensitive(false);
    // Erst wenn das Fenster tatsächlich nicht mehr gemappt ist, darf die
    // Aufnahme beginnen – sonst stünde es im Bild.
    let handler: Rc<RefCell<Option<glib::SignalHandlerId>>> = Rc::new(RefCell::new(None));
    let armed = handler.clone();
    let state_for_unmap = state.clone();
    let id = state.window.connect_unmap(move |window| {
        if let Some(id) = armed.borrow_mut().take() {
            window.disconnect(id);
        }
        let state = state_for_unmap.clone();
        glib::timeout_add_local_once(COMPOSITOR_GRACE, move || run_capture(state, target));
    });
    handler.borrow_mut().replace(id);
    state.window.set_visible(false);
}

fn run_capture(state: Rc<State>, target: Target) {
    glib::spawn_future_local(async move {
        let outcome = gio::spawn_blocking(move || {
            instant::perform(&runner::SystemRunner, target, chooser_desktop)
        })
        .await;
        *state.busy.borrow_mut() = false;
        state.choices.set_sensitive(true);
        let message = match outcome {
            Ok(Ok(Some(shot))) => {
                eprintln!(
                    "querencia-screenshot: aufgenommen: {} ({} Byte)",
                    shot.description,
                    shot.png.size_bytes()
                );
                if let Some(application) = state.window.application() {
                    editor::open(
                        &application,
                        editor::Source {
                            png: shot.png,
                            description: shot.description,
                            suggested_name: None,
                            suggested_folder: None,
                        },
                    );
                }
                // Das Auswahlfenster hat seine Aufgabe erfüllt.
                state.window.close();
                return;
            }
            Ok(Ok(None)) => {
                println!("querencia-screenshot: abgebrochen – nichts aufgenommen");
                gettext("Cancelled – nothing captured.")
            }
            Ok(Err(error)) => {
                eprintln!("querencia-screenshot: Fehler: {error}");
                error.to_string()
            }
            Err(_) => gettext("The capture ended unexpectedly."),
        };
        state.window.set_visible(true);
        state.window.present();
        state.status.set_text(&message);
    });
}

pub(crate) fn button(label: &str, icon_names: &[&str]) -> gtk::Button {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    if let Some(image) = theme_icon(icon_names) {
        content.append(&image);
    }
    content.append(&gtk::Label::new(Some(label)));
    let button = gtk::Button::new();
    button.set_child(Some(&content));
    button.add_css_class("qs-button");
    button
}
pub(crate) fn theme_icon(names: &[&str]) -> Option<gtk::Image> {
    let display = gdk::Display::default()?;
    let theme = gtk::IconTheme::for_display(&display);
    let name = names.iter().find(|name| theme.has_icon(name))?;
    let image = gtk::Image::from_icon_name(name);
    image.set_pixel_size(24);
    Some(image)
}
pub(crate) fn preview_paintable(texture: &gdk::Texture) -> Option<gdk::Paintable> {
    let (width, height) = preview_size(texture.width(), texture.height());
    let snapshot = gtk::Snapshot::new();
    snapshot.append_scaled_texture(
        texture,
        gsk::ScalingFilter::Trilinear,
        &graphene::Rect::new(0.0, 0.0, width as f32, height as f32),
    );
    snapshot.to_paintable(Some(&graphene::Size::new(width as f32, height as f32)))
}

/// Größe, mit der ein Bild von `width` × `height` in die Vorschaufläche
/// passt: Seitenverhältnis bleibt, nichts wird abgeschnitten oder
/// vergrößert, mindestens 1 × 1.
fn preview_size(width: i32, height: i32) -> (i32, i32) {
    if width <= 0 || height <= 0 {
        return (1, 1);
    }
    let scale = (f64::from(PREVIEW_WIDTH) / f64::from(width))
        .min(f64::from(PREVIEW_HEIGHT) / f64::from(height))
        .min(1.0);
    let fit = |edge: i32| (f64::from(edge) * scale).round().max(1.0) as i32;
    (fit(width), fit(height))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chosen_window_capture_always_frames_without_an_active_window() {
        use toplevel::{WindowPlan, WindowQuery, plan};
        assert_eq!(
            plan(window_query_for_chooser(WindowQuery::NoActiveWindow)),
            WindowPlan::Frame { window: None }
        );
        assert_eq!(
            plan(WindowQuery::NoActiveWindow),
            WindowPlan::Everything,
            "Alt+Druck"
        );
    }

    #[test]
    fn escape_ctrl_w_and_ctrl_q_close_the_chooser() {
        use gdk::{Key, ModifierType as M};
        assert!(closes_chooser(Key::Escape, M::empty()));
        assert!(closes_chooser(Key::q, M::CONTROL_MASK));
        assert!(
            closes_chooser(Key::W, M::CONTROL_MASK | M::LOCK_MASK),
            "Feststelltaste"
        );
        assert!(!closes_chooser(Key::q, M::empty()));
        assert!(!closes_chooser(Key::q, M::CONTROL_MASK | M::SHIFT_MASK));
        assert!(!closes_chooser(Key::b, M::CONTROL_MASK));
    }

    #[test]
    fn classic_icons_only_replace_the_built_in_default() {
        let yes = |_: &str| true;
        assert_eq!(classic_icon_theme("Adwaita", yes), Some("gnome-human"));
        assert_eq!(classic_icon_theme("", yes), Some("gnome-human"));
        assert_eq!(classic_icon_theme("Yaru", yes), None, "Benutzerwahl bleibt");
        assert_eq!(
            classic_icon_theme("Adwaita", |_| false),
            None,
            "nicht installiert"
        );
    }

    #[test]
    fn preview_fits_inside_the_fixed_area_without_upscaling() {
        assert_eq!(preview_size(1920, 1080), (480, 270), "Vollbild 16:9");
        assert_eq!(preview_size(3840, 2160), (480, 270));
        assert_eq!(preview_size(1000, 100), (480, 48), "breit: Breite begrenzt");
        assert_eq!(preview_size(100, 1000), (27, 270), "hoch: Höhe begrenzt");
        assert_eq!(preview_size(100, 50), (100, 50), "klein bleibt klein");
        assert_eq!(preview_size(480, 270), (480, 270));
        assert_eq!(preview_size(0, 0), (1, 1));
        assert_eq!(preview_size(1, 5000), (1, 270), "nie 0 Pixel");
    }

    #[test]
    fn preview_keeps_the_aspect_ratio_within_rounding() {
        for (w, h) in [
            (1920, 1080),
            (1366, 768),
            (2560, 1440),
            (800, 1200),
            (1234, 77),
        ] {
            let (pw, ph) = preview_size(w, h);
            assert!(
                pw <= PREVIEW_WIDTH && ph <= PREVIEW_HEIGHT,
                "{w}×{h} → {pw}×{ph}"
            );
            let original = f64::from(w) / f64::from(h);
            let preview = f64::from(pw) / f64::from(ph);
            assert!(
                (original - preview).abs() / original < 0.02,
                "{w}×{h} → {pw}×{ph}: Seitenverhältnis {original:.3} vs {preview:.3}"
            );
        }
    }
}
