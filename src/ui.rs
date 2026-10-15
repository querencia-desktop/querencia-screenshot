// SPDX-License-Identifier: MIT

//! Das Auswahlfenster: Aufnahmemodi, Ergebnisvorschau, Speichern, Kopieren.
//! Die Aufnahme selbst läuft in `capture`, nachdem das Fenster vom
//! Compositor tatsächlich entfernt wurde (`unmap`).

use std::{cell::RefCell, rc::Rc, time::Duration};

use gtk::{gdk, gio, glib, prelude::*};

use crate::{
    capture::{self, CaptureError, Scope},
    clipboard,
    runner::{self, CommandRunner as _},
    screenshot, storage,
};

/// Zeit nach dem Unmap, die dem Compositor für sein nächstes Bild bleibt;
/// Ergänzung zum Unmap-Signal, nicht dessen Ersatz.
const COMPOSITOR_GRACE: Duration = Duration::from_millis(150);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Rectangle,
    /// Ehrlich benannt: labwc liefert keine Fenstergeometrie, der Benutzer
    /// rahmt das Fenster mit slurp ein.
    WindowByRectangle,
    Screen,
}

impl Mode {
    fn label(self) -> &'static str {
        match self {
            Self::Rectangle => "Rechteck",
            Self::WindowByRectangle => "Fenster (per Rechteck)",
            Self::Screen => "Bildschirm",
        }
    }

    fn icons(self) -> &'static [&'static str] {
        match self {
            Self::Rectangle => &["select-rectangular", "edit-select-all", "object-select"],
            Self::WindowByRectangle => {
                &["window-new", "preferences-system-windows", "view-restore"]
            }
            Self::Screen => &["video-display", "computer", "applets-screenshooter"],
        }
    }
}

struct State {
    window: gtk::ApplicationWindow,
    modes: gtk::Box,
    result: gtk::Box,
    picture: gtk::Picture,
    details: gtk::Label,
    status: gtk::Label,
    capture: RefCell<Option<screenshot::Png>>,
    busy: RefCell<bool>,
}

pub fn load_styles() {
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

pub fn present(application: &gtk::Application) {
    if let Some(window) = application.active_window() {
        window.present();
        return;
    }
    let window = gtk::ApplicationWindow::builder()
        .application(application)
        .title("Querencia Screenshot")
        .resizable(false)
        .build();
    window.add_css_class("qs-window");
    window.set_icon_name(Some("applets-screenshooter"));

    let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
    root.set_margin_top(10);
    root.set_margin_bottom(10);
    root.set_margin_start(12);
    root.set_margin_end(12);

    let heading = gtk::Label::new(Some("Aufnahmemodus wählen:"));
    heading.set_xalign(0.0);
    root.append(&heading);

    let modes = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    root.append(&modes);

    let hint = gtk::Label::new(Some(
        "Fensteraufnahme: labwc meldet keine Fenstergrenzen; das Fenster wird mit der Maus eingerahmt.",
    ));
    hint.add_css_class("qs-hint");
    hint.set_xalign(0.0);
    hint.set_wrap(true);
    hint.set_max_width_chars(60);
    root.append(&hint);

    let result = gtk::Box::new(gtk::Orientation::Vertical, 6);
    result.add_css_class("qs-result");
    result.set_visible(false);
    let picture = gtk::Picture::new();
    picture.set_can_shrink(true);
    picture.set_size_request(320, 180);
    picture.add_css_class("qs-preview");
    result.append(&picture);
    let details = gtk::Label::new(None);
    details.set_xalign(0.0);
    result.append(&details);
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    result.append(&actions);
    root.append(&result);

    let status = gtk::Label::new(None);
    status.add_css_class("qs-status");
    status.set_xalign(0.0);
    status.set_wrap(true);
    status.set_max_width_chars(60);
    status.set_selectable(true);
    root.append(&status);

    let bottom = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    bottom.set_halign(gtk::Align::End);
    let quit = button("Beenden", &["application-exit", "window-close"]);
    bottom.append(&quit);
    root.append(&bottom);

    window.set_child(Some(&root));

    let state = Rc::new(State {
        window: window.clone(),
        modes: modes.clone(),
        result: result.clone(),
        picture,
        details,
        status,
        capture: RefCell::new(None),
        busy: RefCell::new(false),
    });

    for mode in [Mode::Rectangle, Mode::WindowByRectangle, Mode::Screen] {
        let control = button(mode.label(), mode.icons());
        let state = state.clone();
        control.connect_clicked(move |_| begin_capture(&state, mode));
        modes.append(&control);
    }

    let save = button("Speichern", &["document-save"]);
    let copy = button("Kopieren", &["edit-copy"]);
    let discard = button("Verwerfen", &["edit-delete", "process-stop"]);
    {
        let state = state.clone();
        save.connect_clicked(move |_| save_capture(&state));
    }
    {
        let state = state.clone();
        copy.connect_clicked(move |_| copy_capture(&state));
    }
    {
        let state = state.clone();
        discard.connect_clicked(move |_| discard_capture(&state));
    }
    actions.append(&save);
    actions.append(&copy);
    actions.append(&discard);

    {
        let window = window.clone();
        quit.connect_clicked(move |_| window.close());
    }

    // Escape: Ergebnis verwerfen oder das Werkzeug schließen.
    let keys = gtk::EventControllerKey::new();
    {
        let state = state.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Escape {
                if state.capture.borrow().is_some() {
                    discard_capture(&state);
                } else {
                    state.window.close();
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
    }
    window.add_controller(keys);

    let missing = capture::missing_tools(&runner::SystemRunner);
    if !missing.is_empty() {
        set_status(
            &state,
            &format!(
                "Fehlende Programme: {}. Rechteck- und Fenstermodus brauchen slurp, Kopieren braucht wl-copy (Paket wl-clipboard).",
                missing.join(", ")
            ),
        );
    }

    window.present();
}

fn button(label: &str, icon_names: &[&str]) -> gtk::Button {
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

/// Das erste im aktiven Symbolthema vorhandene Symbol, 24 px; ohne Treffer
/// bleibt die Schaltfläche eine reine Textschaltfläche.
fn theme_icon(names: &[&str]) -> Option<gtk::Image> {
    let display = gdk::Display::default()?;
    let theme = gtk::IconTheme::for_display(&display);
    let name = names.iter().find(|name| theme.has_icon(name))?;
    let image = gtk::Image::from_icon_name(name);
    image.set_pixel_size(24);
    Some(image)
}

fn set_status(state: &State, text: &str) {
    state.status.set_text(text);
}

fn begin_capture(state: &Rc<State>, mode: Mode) {
    if *state.busy.borrow() {
        return;
    }
    *state.busy.borrow_mut() = true;
    state.modes.set_sensitive(false);
    set_status(state, "");
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
        glib::timeout_add_local_once(COMPOSITOR_GRACE, move || run_capture(state, mode));
    });
    handler.borrow_mut().replace(id);
    state.window.set_visible(false);
}

fn run_capture(state: Rc<State>, mode: Mode) {
    glib::spawn_future_local(async move {
        let outcome = gio::spawn_blocking(move || {
            let runner = runner::SystemRunner;
            let scope = match mode {
                Mode::Rectangle | Mode::WindowByRectangle => match capture::select_region(&runner)?
                {
                    Some(region) => Scope::Region(region),
                    None => return Ok(None),
                },
                Mode::Screen => Scope::Everything,
            };
            capture::capture(&runner, &scope).map(Some)
        })
        .await;
        // Ein Panic im Aufnahme-Thread wird wie ein Abbruch mit Meldung behandelt.
        finish_capture(&state, outcome.ok());
    });
}

fn finish_capture(
    state: &Rc<State>,
    outcome: Option<Result<Option<screenshot::Png>, CaptureError>>,
) {
    *state.busy.borrow_mut() = false;
    state.modes.set_sensitive(true);
    state.window.set_visible(true);
    state.window.present();
    match outcome {
        Some(Ok(Some(png))) => show_capture(state, png),
        Some(Ok(None)) => set_status(state, "Abgebrochen – keine Aufnahme."),
        Some(Err(error)) => set_status(state, &error.to_string()),
        None => set_status(state, "Die Aufnahme wurde unerwartet beendet."),
    }
}

fn show_capture(state: &Rc<State>, png: screenshot::Png) {
    let bytes = glib::Bytes::from(png.bytes());
    match gdk::Texture::from_bytes(&bytes) {
        Ok(texture) => state.picture.set_paintable(Some(&texture)),
        Err(_) => state.picture.set_paintable(None::<&gdk::Paintable>),
    }
    let size = png
        .dimensions()
        .map(|(w, h)| format!("{w} × {h} Pixel"))
        .unwrap_or_else(|| "unbekannte Größe".to_owned());
    state.details.set_text(&format!(
        "Aufnahme: {size}, {} KiB",
        png.size_bytes() / 1024
    ));
    state.capture.replace(Some(png));
    state.result.set_visible(true);
    set_status(
        state,
        "Aufnahme bereit – speichern, kopieren oder verwerfen.",
    );
}

fn discard_capture(state: &Rc<State>) {
    state.capture.replace(None);
    state.picture.set_paintable(None::<&gdk::Paintable>);
    state.result.set_visible(false);
    set_status(state, "Aufnahme verworfen.");
}

fn save_capture(state: &Rc<State>) {
    let Some(png) = state.capture.borrow().clone() else {
        return;
    };
    let directory = storage::default_directory();
    match storage::save(&directory, &storage::file_name_now(), &png) {
        Ok(path) => set_status(state, &format!("Gespeichert: {}", path.display())),
        Err(error) => set_status(state, &error.to_string()),
    }
}

fn copy_capture(state: &Rc<State>) {
    let Some(png) = state.capture.borrow().clone() else {
        return;
    };
    let runner = runner::SystemRunner;
    if runner.available(clipboard::WL_COPY) {
        match clipboard::copy_png(&runner, &png) {
            Ok(()) => set_status(state, "In die Zwischenablage kopiert (image/png)."),
            Err(error) => set_status(state, &error.to_string()),
        }
        return;
    }
    // Ohne wl-copy bietet GTK die Daten selbst an – nur solange dieses
    // Fenster offen ist.
    let bytes = glib::Bytes::from(png.bytes());
    match gdk::Texture::from_bytes(&bytes) {
        Ok(texture) => {
            state.window.clipboard().set_texture(&texture);
            set_status(
                state,
                "Kopiert über GTK (wl-copy fehlt): gilt nur, solange Querencia Screenshot geöffnet bleibt.",
            );
        }
        Err(_) => set_status(
            state,
            "Kopieren nicht möglich: wl-copy fehlt und GTK konnte das Bild nicht laden.",
        ),
    }
}
