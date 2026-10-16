// SPDX-License-Identifier: MIT

//! Sofortaufnahme wie GNOME 2: Druck nimmt den Bildschirm auf, Alt+Druck das
//! aktive Fenster, ohne vorher nach dem Modus zu fragen. Danach erscheint
//! „Bildschirmfoto speichern“ mit Vorschau, Name, Ordner, Kopieren und
//! Abbrechen. Vor der Aufnahme ist kein eigenes Fenster sichtbar, also steht
//! nichts von Querencia Screenshot im Bild.

use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
};

use gtk::{gdk, gio, glib, prelude::*};

use crate::{
    capture::{self, CaptureError, Scope},
    cli::Target,
    clipboard,
    runner::{self, CommandRunner},
    screenshot::Png,
    storage,
    toplevel::{self, WindowPlan, WindowQuery},
    ui,
};

/// Eine fertige Aufnahme und ein Satz, was sie zeigt.
#[derive(Debug)]
pub struct Shot {
    pub png: Png,
    pub description: String,
}

/// Nimmt `target` auf, ohne GTK. `Ok(None)`: der Benutzer hat die
/// Bereichsauswahl abgebrochen; es entsteht keine Datei.
pub fn perform(
    runner: &dyn CommandRunner,
    target: Target,
    query: impl FnOnce() -> WindowQuery,
) -> Result<Option<Shot>, CaptureError> {
    let framed = |description: String| -> Result<Option<Shot>, CaptureError> {
        let Some(region) = capture::select_region(runner)? else {
            return Ok(None);
        };
        let png = capture::capture(runner, &Scope::Region(region))?;
        Ok(Some(Shot { png, description }))
    };
    let whole = |description: &str| -> Result<Option<Shot>, CaptureError> {
        let png = capture::capture(runner, &Scope::Everything)?;
        Ok(Some(Shot {
            png,
            description: description.to_owned(),
        }))
    };
    match target {
        Target::Screen => whole("Bildschirm"),
        Target::Area => framed("Bereich".to_owned()),
        Target::Window => match toplevel::plan(query()) {
            WindowPlan::Output { output, window } => {
                let png = capture::capture(runner, &Scope::Output(output.clone()))?;
                Ok(Some(Shot {
                    png,
                    description: format!("Fenster „{}“ (Vollbild auf {output})", window.label()),
                }))
            }
            WindowPlan::Frame {
                window: Some(window),
            } => framed(format!(
                "Fenster „{}“, mit der Maus eingerahmt (labwc meldet keine Fenstergrenzen)",
                window.label()
            )),
            WindowPlan::Frame { window: None } => framed(
                "Fenster, mit der Maus eingerahmt (aktives Fenster nicht ermittelbar)".to_owned(),
            ),
            WindowPlan::Everything => whole("Kein aktives Fenster – ganzer Bildschirm"),
        },
    }
}

/// Startet die Aufnahme beim Aktivieren der Anwendung und zeigt danach den
/// Speicherdialog oder eine Fehlermeldung.
pub fn start(application: &gtk::Application, target: Target) {
    // Ohne Fenster würde GApplication sofort enden.
    let hold = application.hold();
    let application = application.clone();
    glib::spawn_future_local(async move {
        let outcome = gio::spawn_blocking(move || {
            perform(&runner::SystemRunner, target, toplevel::query_active)
        })
        .await;
        match outcome {
            Ok(Ok(Some(shot))) => {
                eprintln!(
                    "querencia-screenshot: aufgenommen: {} ({} Byte)",
                    shot.description,
                    shot.png.size_bytes()
                );
                show_save_dialog(&application, shot);
            }
            Ok(Ok(None)) => println!("querencia-screenshot: abgebrochen – nichts gespeichert"),
            Ok(Err(error)) => show_error(&application, &error.to_string()),
            Err(_) => show_error(&application, "Die Aufnahme wurde unerwartet beendet."),
        }
        drop(hold);
    });
}

fn titled_window(application: &gtk::Application, title: &str) -> gtk::ApplicationWindow {
    let window = gtk::ApplicationWindow::builder()
        .application(application)
        .title(title)
        .resizable(false)
        .build();
    window.add_css_class("qs-window");
    window.set_icon_name(Some("applets-screenshooter"));
    let titlebar = gtk::HeaderBar::new();
    titlebar.set_show_title_buttons(true);
    titlebar.set_decoration_layout(Some(":close"));
    titlebar.add_css_class("qs-titlebar");
    window.set_titlebar(Some(&titlebar));
    window
}

fn show_error(application: &gtk::Application, message: &str) {
    eprintln!("querencia-screenshot: Fehler: {message}");
    let window = titled_window(application, "Bildschirmfoto fehlgeschlagen");
    let root = gtk::Box::new(gtk::Orientation::Vertical, 10);
    root.set_margin_top(12);
    root.set_margin_bottom(12);
    root.set_margin_start(12);
    root.set_margin_end(12);
    let text = gtk::Label::new(Some(message));
    text.set_wrap(true);
    text.set_max_width_chars(60);
    text.set_selectable(true);
    text.set_xalign(0.0);
    root.append(&text);
    let close = ui::button("Schließen", &["window-close"]);
    close.set_halign(gtk::Align::End);
    {
        let window = window.clone();
        close.connect_clicked(move |_| window.close());
    }
    root.append(&close);
    window.set_child(Some(&root));
    window.set_default_widget(Some(&close));
    close_on_escape(&window);
    window.present();
}

fn close_on_escape(window: &gtk::ApplicationWindow) {
    let keys = gtk::EventControllerKey::new();
    let target = window.clone();
    keys.connect_key_pressed(move |_, key, _, _| {
        if key == gdk::Key::Escape {
            target.close();
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    window.add_controller(keys);
}

/// Warum in `directory` voraussichtlich nicht gespeichert werden kann; die
/// Prüfung schreibt nichts. Ein fehlender Ordner ist kein Problem, solange
/// der nächste vorhandene Elternordner beschreibbar ist.
pub fn directory_problem(directory: &Path) -> Option<String> {
    let writable = |path: &Path| {
        gio::File::for_path(path)
            .query_info(
                "access::can-write",
                gio::FileQueryInfoFlags::NONE,
                gio::Cancellable::NONE,
            )
            .map(|info| info.boolean("access::can-write"))
            .unwrap_or(false)
    };
    if std::fs::symlink_metadata(directory).is_ok() {
        if !directory.is_dir() {
            return Some(format!(
                "„{}“ ist kein Ordner – bitte einen anderen Ordner wählen.",
                directory.display()
            ));
        }
        if !writable(directory) {
            return Some(format!(
                "Keine Schreibberechtigung für „{}“ – bitte einen anderen Ordner wählen.",
                directory.display()
            ));
        }
        return None;
    }
    let parent = directory
        .ancestors()
        .skip(1)
        .find(|path| std::fs::symlink_metadata(path).is_ok());
    match parent {
        Some(parent) if parent.is_dir() && writable(parent) => None,
        _ => Some(format!(
            "„{}“ fehlt und kann nicht angelegt werden – bitte einen anderen Ordner wählen.",
            directory.display()
        )),
    }
}

/// Vorschlag für den Dateinamen: Zeitstempel, bei Kollision mit Zähler.
fn suggested_name(directory: &Path) -> String {
    let name = storage::file_name_now();
    if directory.is_dir()
        && let Ok(path) = storage::unique_path(directory, &name)
        && let Some(file) = path.file_name()
    {
        return file.to_string_lossy().into_owned();
    }
    name
}

struct Dialog {
    window: gtk::ApplicationWindow,
    name: gtk::Entry,
    folder_label: gtk::Label,
    folder: RefCell<PathBuf>,
    status: gtk::Label,
    cancel_label: gtk::Label,
    actions: gtk::Box,
    png: Png,
    busy: RefCell<bool>,
}

fn show_save_dialog(application: &gtk::Application, shot: Shot) {
    let window = titled_window(application, "Bildschirmfoto speichern");
    let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
    root.set_margin_top(10);
    root.set_margin_bottom(10);
    root.set_margin_start(12);
    root.set_margin_end(12);

    let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
    frame.add_css_class("qs-preview");
    frame.set_halign(gtk::Align::Center);
    // Die Vorschau ist schon auf höchstens 480 × 270 verkleinert; ohne
    // can_shrink ist das zugleich Mindest- und Wunschgröße. GtkFixed gibt dem
    // Bild genau diese Größe; in einer Box würde GtkPicture bei breitem
    // Dialog mehr Höhe anfordern (Höhe aus Breite).
    let picture = gtk::Picture::new();
    picture.set_can_shrink(false);
    let texture = gdk::Texture::from_bytes(&glib::Bytes::from(shot.png.bytes())).ok();
    picture.set_paintable(texture.as_ref().and_then(ui::preview_paintable).as_ref());
    let fixed = gtk::Fixed::new();
    fixed.put(&picture, 0.0, 0.0);
    frame.append(&fixed);
    root.append(&frame);

    let size = shot
        .png
        .dimensions()
        .map(|(w, h)| format!("{w} × {h} Pixel"))
        .unwrap_or_else(|| "unbekannte Größe".to_owned());
    let details = gtk::Label::new(Some(&format!("{} · {size}", shot.description)));
    details.set_xalign(0.0);
    details.set_wrap(true);
    details.set_max_width_chars(60);
    details.add_css_class("qs-hint");
    root.append(&details);

    let grid = gtk::Grid::new();
    grid.set_row_spacing(6);
    grid.set_column_spacing(8);
    let name_title = gtk::Label::with_mnemonic("_Name:");
    name_title.set_xalign(1.0);
    let directory = storage::default_directory();
    let name = gtk::Entry::new();
    name.set_text(&suggested_name(&directory));
    name.set_hexpand(true);
    name.set_width_chars(36);
    name_title.set_mnemonic_widget(Some(&name));
    grid.attach(&name_title, 0, 0, 1, 1);
    grid.attach(&name, 1, 0, 1, 1);
    let folder_title = gtk::Label::with_mnemonic("In _Ordner speichern:");
    folder_title.set_xalign(1.0);
    let folder_label = gtk::Label::new(Some(&directory.display().to_string()));
    folder_label.set_ellipsize(gtk::pango::EllipsizeMode::Start);
    folder_label.set_max_width_chars(36);
    folder_label.set_xalign(0.0);
    folder_label.set_hexpand(true);
    let folder_content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    folder_content.append(&gtk::Image::from_icon_name("folder"));
    folder_content.append(&folder_label);
    let folder_button = gtk::Button::new();
    folder_button.set_child(Some(&folder_content));
    folder_button.add_css_class("qs-button");
    folder_button.set_tooltip_text(Some("Anderen Ordner wählen …"));
    folder_title.set_mnemonic_widget(Some(&folder_button));
    grid.attach(&folder_title, 0, 1, 1, 1);
    grid.attach(&folder_button, 1, 1, 1, 1);
    root.append(&grid);

    let status = gtk::Label::new(None);
    status.add_css_class("qs-status");
    status.set_xalign(0.0);
    status.set_wrap(true);
    status.set_max_width_chars(60);
    status.set_selectable(true);
    root.append(&status);

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let copy = ui::button("In die _Zwischenablage kopieren", &["edit-copy"]);
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    let cancel_label = gtk::Label::with_mnemonic("_Abbrechen");
    let cancel = gtk::Button::new();
    cancel.set_child(Some(&cancel_label));
    cancel.add_css_class("qs-button");
    let save = ui::button("_Speichern", &["document-save"]);
    for button in [&copy, &save] {
        mnemonic(button);
    }
    actions.append(&copy);
    actions.append(&spacer);
    actions.append(&cancel);
    actions.append(&save);
    root.append(&actions);
    window.set_child(Some(&root));
    window.set_default_widget(Some(&save));

    let dialog = Rc::new(Dialog {
        window: window.clone(),
        name: name.clone(),
        folder_label,
        folder: RefCell::new(directory.clone()),
        status,
        cancel_label,
        actions,
        png: shot.png,
        busy: RefCell::new(false),
    });
    if let Some(problem) = directory_problem(&directory) {
        dialog.status.set_text(&problem);
    }

    {
        let dialog = dialog.clone();
        save.connect_clicked(move |_| save_now(&dialog));
    }
    {
        let dialog = dialog.clone();
        name.connect_activate(move |_| save_now(&dialog));
    }
    {
        let dialog = dialog.clone();
        copy.connect_clicked(move |_| copy_now(&dialog));
    }
    {
        let dialog = dialog.clone();
        folder_button.connect_clicked(move |_| choose_folder(&dialog));
    }
    {
        let window = window.clone();
        cancel.connect_clicked(move |_| window.close());
    }
    window.connect_close_request(|_| {
        println!("querencia-screenshot: Dialog geschlossen");
        glib::Propagation::Proceed
    });
    close_on_escape(&window);
    window.present();
    // Wie GNOME 2: der Name ohne Endung ist markiert und kann sofort
    // überschrieben werden.
    name.grab_focus();
    let stem = name.text().trim_end_matches(".png").chars().count() as i32;
    name.select_region(0, stem);
}

fn mnemonic(button: &gtk::Button) {
    let Some(content) = button.child() else {
        return;
    };
    let mut child = content.first_child();
    while let Some(widget) = child {
        if let Ok(label) = widget.clone().downcast::<gtk::Label>() {
            label.set_use_underline(true);
            label.set_mnemonic_widget(Some(button));
        }
        child = widget.next_sibling();
    }
}

fn save_now(dialog: &Rc<Dialog>) {
    if *dialog.busy.borrow() {
        return;
    }
    let folder = dialog.folder.borrow().clone();
    match storage::save_exact(&folder, &dialog.name.text(), &dialog.png) {
        Ok(path) => {
            println!("querencia-screenshot: Gespeichert: {}", path.display());
            dialog.window.close();
        }
        Err(error) => {
            eprintln!("querencia-screenshot: nicht gespeichert: {error}");
            dialog.status.set_text(&error.to_string());
        }
    }
}

fn copy_now(dialog: &Rc<Dialog>) {
    if *dialog.busy.borrow() {
        return;
    }
    let png = dialog.png.clone();
    if !runner::SystemRunner.available(clipboard::WL_COPY) {
        match gdk::Texture::from_bytes(&glib::Bytes::from(png.bytes())) {
            Ok(texture) => {
                dialog.window.clipboard().set_texture(&texture);
                dialog.status.set_text(
                    "Kopiert über GTK (wl-copy fehlt): gilt nur, solange dieses Fenster offen bleibt.",
                );
            }
            Err(_) => dialog
                .status
                .set_text("Kopieren nicht möglich: wl-copy fehlt."),
        }
        return;
    }
    *dialog.busy.borrow_mut() = true;
    dialog.actions.set_sensitive(false);
    dialog.status.set_text("Kopiere in die Zwischenablage …");
    let dialog = dialog.clone();
    glib::spawn_future_local(async move {
        let outcome =
            gio::spawn_blocking(move || clipboard::copy_png(&runner::SystemRunner, &png)).await;
        *dialog.busy.borrow_mut() = false;
        dialog.actions.set_sensitive(true);
        match outcome {
            Ok(Ok(())) => {
                println!("querencia-screenshot: in die Zwischenablage kopiert");
                dialog
                    .status
                    .set_text("In die Zwischenablage kopiert (image/png).");
                // Die Daten hält wl-copy; das Fenster darf jetzt zu.
                dialog.cancel_label.set_text_with_mnemonic("_Schließen");
            }
            Ok(Err(error)) => dialog.status.set_text(&error.to_string()),
            Err(_) => dialog.status.set_text("Kopieren wurde unerwartet beendet."),
        }
    });
}

fn choose_folder(dialog: &Rc<Dialog>) {
    let chooser = gtk::FileDialog::builder()
        .title("Ordner für das Bildschirmfoto wählen")
        .accept_label("Auswählen")
        .modal(true)
        .build();
    let current = dialog.folder.borrow().clone();
    let start = current
        .ancestors()
        .find(|path| path.is_dir())
        .map(Path::to_path_buf)
        .unwrap_or_else(glib::home_dir);
    chooser.set_initial_folder(Some(&gio::File::for_path(start)));
    let dialog = dialog.clone();
    let parent = dialog.window.clone();
    chooser.select_folder(Some(&parent), gio::Cancellable::NONE, move |result| {
        let Ok(file) = result else {
            return;
        };
        let Some(path) = file.path() else {
            dialog
                .status
                .set_text("Nur lokale Ordner werden unterstützt.");
            return;
        };
        dialog.folder_label.set_text(&path.display().to_string());
        dialog
            .status
            .set_text(directory_problem(&path).as_deref().unwrap_or(""));
        dialog.folder.replace(path);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        capture::{GRIM, SLURP},
        runner::{Output, RunError, fake::FakeRunner},
        screenshot::sample,
        toplevel::ActiveWindow,
    };

    fn exit(status: i32, stdout: &[u8], stderr: &str) -> Result<Output, RunError> {
        Ok(Output {
            status,
            stdout: stdout.to_vec(),
            stderr: stderr.to_owned(),
        })
    }

    fn never() -> WindowQuery {
        panic!("Fensterabfrage nur für --window")
    }

    fn editor(fullscreen: bool) -> WindowQuery {
        WindowQuery::Active(ActiveWindow {
            title: "Brief.txt – Editor".into(),
            app_id: "editor".into(),
            fullscreen,
            maximized: false,
            outputs: vec!["HEADLESS-2".into()],
        })
    }

    #[test]
    fn screen_captures_everything_at_once_without_asking() {
        let runner = FakeRunner::default().respond(GRIM, exit(0, &sample(1920, 1080), ""));
        let shot = perform(&runner, Target::Screen, never)
            .expect("ok")
            .expect("shot");
        assert_eq!(shot.png.dimensions(), Some((1920, 1080)));
        assert_eq!(shot.description, "Bildschirm");
        let calls = runner.calls.borrow();
        assert_eq!(calls.len(), 1, "kein slurp, keine Rückfrage");
        assert_eq!(calls[0].args, vec!["-t", "png", "-l", "1", "-"]);
    }

    #[test]
    fn fullscreen_window_is_its_output_exactly() {
        let runner = FakeRunner::default().respond(GRIM, exit(0, &sample(1280, 720), ""));
        let shot = perform(&runner, Target::Window, || editor(true))
            .expect("ok")
            .expect("shot");
        assert_eq!(
            runner.calls.borrow()[0].args,
            vec!["-t", "png", "-l", "1", "-o", "HEADLESS-2", "-"]
        );
        assert!(shot.description.contains("Vollbild auf HEADLESS-2"));
    }

    #[test]
    fn ordinary_window_is_framed_by_the_user_and_says_so() {
        let runner = FakeRunner::default()
            .respond(SLURP, exit(0, b"10,20 300x200\n", ""))
            .respond(GRIM, exit(0, &sample(300, 200), ""));
        let shot = perform(&runner, Target::Window, || editor(false))
            .expect("ok")
            .expect("shot");
        assert!(shot.description.contains("„Brief.txt – Editor“"));
        assert!(shot.description.contains("eingerahmt"));
        assert_eq!(
            runner.calls.borrow()[1].args,
            vec!["-t", "png", "-l", "1", "-g", "10,20 300x200", "-"]
        );
    }

    #[test]
    fn cancelling_the_frame_creates_nothing() {
        let runner = FakeRunner::default().respond(SLURP, exit(1, b"", "selection cancelled\n"));
        assert!(
            perform(&runner, Target::Window, || editor(false))
                .expect("ok")
                .is_none()
        );
        let runner = FakeRunner::default().respond(SLURP, exit(1, b"", "selection cancelled\n"));
        assert!(perform(&runner, Target::Area, never).expect("ok").is_none());
        assert_eq!(runner.calls.borrow().len(), 1, "grim nicht gestartet");
    }

    #[test]
    fn no_active_window_means_whole_screen_like_gnome_2() {
        let runner = FakeRunner::default().respond(GRIM, exit(0, &sample(2, 2), ""));
        let shot = perform(&runner, Target::Window, || WindowQuery::NoActiveWindow)
            .expect("ok")
            .expect("shot");
        assert!(shot.description.starts_with("Kein aktives Fenster"));
    }

    #[test]
    fn capture_errors_are_reported() {
        let runner = FakeRunner::default().respond(
            GRIM,
            exit(
                1,
                b"",
                "compositor doesn't support wlr-screencopy-unstable-v1\n",
            ),
        );
        let error = perform(&runner, Target::Screen, never).expect_err("error");
        assert!(error.to_string().contains("wlr-screencopy"));
        let runner = FakeRunner::default().respond(GRIM, Err(RunError::Missing(GRIM.into())));
        assert_eq!(
            perform(&runner, Target::Screen, never)
                .expect_err("missing")
                .to_string(),
            "Programm „grim“ ist nicht installiert"
        );
    }

    #[test]
    fn directory_problems_are_found_without_writing() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!(
            "querencia-screenshot-test-problem-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("root");
        assert_eq!(directory_problem(&root), None);
        assert_eq!(
            directory_problem(&root.join("neu").join("Screenshots")),
            None
        );
        let file = root.join("Screenshots");
        std::fs::write(&file, b"").expect("file");
        assert!(
            directory_problem(&file)
                .expect("problem")
                .contains("ist kein Ordner")
        );
        assert!(
            directory_problem(&file.join("unter"))
                .expect("problem")
                .contains("kann nicht angelegt werden")
        );
        let locked = root.join("gesperrt");
        std::fs::create_dir(&locked).expect("dir");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o500)).expect("chmod");
        assert!(
            directory_problem(&locked)
                .expect("problem")
                .starts_with("Keine Schreibberechtigung")
        );
        assert!(directory_problem(&locked.join("Screenshots")).is_some());
        assert_eq!(
            std::fs::read_dir(&locked).expect("list").count(),
            0,
            "nichts angelegt"
        );
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).expect("chmod");
        let _ = std::fs::remove_dir_all(&root);
    }
}
