// SPDX-License-Identifier: MIT

//! Sofortaufnahme (`--screen`, `--window`, `--area`): ohne vorherige
//! Moduswahl aufnehmen und das Ergebnis im Editor öffnen (QS-0d; bis QS-1
//! direkt im Speicherdialog). Vor der Aufnahme ist kein eigenes Fenster
//! sichtbar, also steht nichts von Querencia Screenshot im Bild.
//!
//! Dazu der Dialog „Bildschirmfoto speichern“ (Vorschau, Name, Ordner,
//! Kopieren, Abbrechen), den der Editor zum Speichern des fertigen Bildes
//! öffnet.

use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
    time::Instant,
};

use gtk::{gdk, gio, glib, prelude::*};

use crate::{
    capture::{self, CaptureError, Scope},
    cli::Target,
    clipboard, editor,
    geometry::Region,
    runner::{self, CommandRunner},
    screenshot::Png,
    storage,
    toplevel::{self, Desktop, WindowPlan},
    ui,
    window_pick::{self, Found, FrameStyle, Image, Kind},
};

/// Eine fertige Aufnahme und ein Satz, was sie zeigt.
#[derive(Debug)]
pub struct Shot {
    pub png: Png,
    pub description: String,
}

/// Nimmt `target` auf. `Ok(None)`: der Benutzer hat die Auswahl
/// abgebrochen; es entsteht keine Datei. `desktop` wird nur für `--window`
/// gefragt.
pub fn perform(
    runner: &dyn CommandRunner,
    target: Target,
    desktop: impl FnOnce() -> Desktop,
) -> Result<Option<Shot>, CaptureError> {
    perform_with_style(runner, target, desktop, window_pick::load_style)
}

fn perform_with_style(
    runner: &dyn CommandRunner,
    target: Target,
    desktop: impl FnOnce() -> Desktop,
    style: impl FnOnce() -> FrameStyle,
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
        Target::Window => {
            let desktop = desktop();
            match toplevel::plan(desktop.query.clone()) {
                WindowPlan::Output { output, window } => {
                    let png = capture::capture(runner, &Scope::Output(output.clone()))?;
                    Ok(Some(Shot {
                        png,
                        description: format!(
                            "Fenster „{}“ (Vollbild auf {output})",
                            window.label()
                        ),
                    }))
                }
                WindowPlan::Frame { window } => pick_window(runner, &desktop, window, style),
                WindowPlan::Everything => whole("Kein aktives Fenster – ganzer Bildschirm"),
            }
        }
    }
}

/// Ausgaben in Bildkoordinaten des eingefrorenen Vollbilds. grim legt alle
/// Ausgaben ab der kleinsten Layout-Ecke in ein Bild, in der größten Skala.
struct Layout {
    origin: (i32, i32),
    /// Bildpixel je logischem Pixel; `None`, wenn das Bild nicht zum
    /// Layout passt.
    scale: Option<i32>,
    outputs: Vec<window_pick::Output>,
}

impl Layout {
    fn new(outputs: &[toplevel::OutputInfo], width: i32, height: i32) -> Self {
        if outputs.is_empty() {
            return Self {
                origin: (0, 0),
                scale: Some(1),
                outputs: vec![window_pick::Output {
                    name: "Bild".to_owned(),
                    area: Region {
                        x: 0,
                        y: 0,
                        width: width.max(0) as u32,
                        height: height.max(0) as u32,
                    },
                    usable_height: None,
                    has_maximized: false,
                }],
            };
        }
        let min_x = outputs.iter().map(|o| o.x).min().unwrap_or(0);
        let min_y = outputs.iter().map(|o| o.y).min().unwrap_or(0);
        let max_x = outputs.iter().map(|o| o.x + o.width).max().unwrap_or(0);
        let max_y = outputs.iter().map(|o| o.y + o.height).max().unwrap_or(0);
        let (lw, lh) = (max_x - min_x, max_y - min_y);
        let scale = (lw > 0 && lh > 0 && width % lw == 0 && height == lh * (width / lw))
            .then_some(width / lw)
            .filter(|s| *s >= 1);
        let s = scale.unwrap_or(1);
        Self {
            origin: (min_x, min_y),
            scale,
            outputs: outputs
                .iter()
                .map(|o| window_pick::Output {
                    name: o.name.clone(),
                    area: Region {
                        x: (o.x - min_x) * s,
                        y: (o.y - min_y) * s,
                        width: (o.width * s).max(0) as u32,
                        height: (o.height * s).max(0) as u32,
                    },
                    usable_height: o.usable.map(|(_, h)| (h * s).max(0) as u32),
                    has_maximized: o.has_maximized,
                })
                .collect(),
        }
    }

    /// Bild → Layout (nur Skala 1 wird für erkannte Fenster benutzt).
    fn to_layout(&self, area: Region) -> Region {
        Region {
            x: area.x + self.origin.0,
            y: area.y + self.origin.1,
            ..area
        }
    }

    /// Layout → Bild.
    fn to_image(&self, area: Region) -> Region {
        let s = self.scale.unwrap_or(1);
        Region {
            x: (area.x - self.origin.0) * s,
            y: (area.y - self.origin.1) * s,
            width: area.width * s as u32,
            height: area.height * s as u32,
        }
    }
}

/// Fensterwahl (Alt+Druck, „Fenster einrahmen“): Vollbild einfrieren,
/// labwc-Rahmen erkennen, slurp mit den Rechtecken (Klick wählt, Ziehen
/// rahmt frei ein, Esc bricht ab), Ausschnitt aus dem eingefrorenen Bild.
fn pick_window(
    runner: &dyn CommandRunner,
    desktop: &Desktop,
    active: Option<toplevel::ActiveWindow>,
    style: impl FnOnce() -> FrameStyle,
) -> Result<Option<Shot>, CaptureError> {
    let manual = |region: Region| match &active {
        Some(window) => format!(
            "Fenster „{}“, mit der Maus eingerahmt ({})",
            window.label(),
            region.grim_argument()
        ),
        None => format!(
            "Fenster, mit der Maus eingerahmt ({})",
            region.grim_argument()
        ),
    };
    let full = capture::capture(runner, &Scope::Everything)?;
    let started = Instant::now();
    let image = decode(&full)?;
    let layout = Layout::new(&desktop.outputs, image.width, image.height);
    if layout.scale != Some(1) {
        // Skala ≠ 1 ist im isolierten Test nicht prüfbar: nur Einrahmen,
        // wie bisher mit grim -g.
        eprintln!(
            "querencia-screenshot: Fensterwahl: Bild {}×{} passt nicht zu Skala 1 – nur Einrahmen",
            image.width, image.height
        );
        let Some(region) = capture::select_region(runner)? else {
            return Ok(None);
        };
        let png = capture::capture(runner, &Scope::Region(region))?;
        return Ok(Some(Shot {
            png,
            description: manual(region),
        }));
    }
    let style = style();
    let found = window_pick::detect(&image, &style, &layout.outputs);
    let boxes: Vec<(Region, String)> = window_pick::slurp_boxes(&image, &style, &found)
        .into_iter()
        .map(|(area, index)| (layout.to_layout(area), format!("f{index}")))
        .collect();
    let count = |kind: Kind| found.iter().filter(|f| f.kind == kind).count();
    eprintln!(
        "querencia-screenshot: Fensterwahl: {} Fenster erkannt ({} vollständig, {} teilverdeckt, {} maximiert; {} Rechtecke für slurp; {} ms; {} Fenster gemeldet, {} Ausgabe(n); Rahmenfarben: {})",
        found.len(),
        count(Kind::Complete),
        count(Kind::Partial),
        count(Kind::Maximized),
        boxes.len(),
        started.elapsed().as_millis(),
        desktop.windows,
        layout.outputs.len(),
        style.source
    );
    for (index, window) in found.iter().enumerate() {
        let shown = Found {
            area: layout.to_layout(window.area),
            ..*window
        };
        eprintln!(
            "querencia-screenshot: Fensterwahl: f{index} {}",
            shown.describe()
        );
    }
    let Some(picked) = capture::pick_region(runner, &boxes)? else {
        return Ok(None);
    };
    let chosen = picked
        .label
        .as_deref()
        .and_then(|label| label.strip_prefix('f'))
        .and_then(|index| index.parse::<usize>().ok())
        .and_then(|index| found.get(index));
    let (area, description) = match chosen {
        Some(window) => {
            let area = layout.to_layout(window.area);
            let title = match (&active, window.active) {
                (Some(active), true) => format!(" „{}“", active.label()),
                _ => String::new(),
            };
            let note = match window.kind {
                Kind::Complete => "",
                Kind::Partial => ", teilweise verdeckt – darüberliegende Fenster sind mit im Bild",
                Kind::Maximized => ", maximiert",
            };
            (
                area,
                format!("Fenster{title} (gewählt {}{note})", area.grim_argument()),
            )
        }
        None => (picked.region, manual(picked.region)),
    };
    let Some(part) = image.crop(layout.to_image(area)) else {
        return Err(CaptureError::Geometry(
            crate::geometry::GeometryError::Degenerate(area),
        ));
    };
    let png = encode(&part)?;
    Ok(Some(Shot { png, description }))
}

/// PNG → RGBA (über GDK, ohne Anzeige).
fn decode(png: &Png) -> Result<Image, CaptureError> {
    let texture = gdk::Texture::from_bytes(&glib::Bytes::from(png.bytes())).map_err(|error| {
        CaptureError::Failed {
            status: -1,
            stderr: format!("Bild nicht lesbar: {error}"),
        }
    })?;
    let (width, height) = (texture.width(), texture.height());
    let mut downloader = gdk::TextureDownloader::new(&texture);
    downloader.set_format(gdk::MemoryFormat::R8g8b8a8);
    let (bytes, stride) = downloader.download_bytes();
    let row = width as usize * 4;
    let mut rgba = Vec::with_capacity(row * height as usize);
    for y in 0..height as usize {
        rgba.extend_from_slice(&bytes[y * stride..y * stride + row]);
    }
    Ok(Image {
        width,
        height,
        rgba,
    })
}

/// RGBA → PNG.
fn encode(image: &Image) -> Result<Png, CaptureError> {
    let texture = gdk::MemoryTexture::new(
        image.width,
        image.height,
        gdk::MemoryFormat::R8g8b8a8,
        &glib::Bytes::from(&image.rgba),
        image.width as usize * 4,
    );
    Png::new(texture.save_to_png_bytes().to_vec()).map_err(CaptureError::Image)
}

/// Startet die Aufnahme beim Aktivieren der Anwendung und zeigt danach den
/// Speicherdialog oder eine Fehlermeldung.
pub fn start(application: &gtk::Application, target: Target) {
    // Ohne Fenster würde GApplication sofort enden.
    let hold = application.hold();
    let application = application.clone();
    glib::spawn_future_local(async move {
        let outcome = gio::spawn_blocking(move || {
            perform(&runner::SystemRunner, target, toplevel::query_desktop)
        })
        .await;
        match outcome {
            Ok(Ok(Some(shot))) => {
                eprintln!(
                    "querencia-screenshot: aufgenommen: {} ({} Byte)",
                    shot.description,
                    shot.png.size_bytes()
                );
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
            Ok(Ok(None)) => println!("querencia-screenshot: abgebrochen – nichts gespeichert"),
            Ok(Err(error)) => show_error(&application, &error.to_string()),
            Err(_) => show_error(&application, "Die Aufnahme wurde unerwartet beendet."),
        }
        drop(hold);
    });
}

fn titled_window(
    application: &gtk::Application,
    title: &str,
    parent: Option<&gtk::Window>,
) -> gtk::ApplicationWindow {
    let window = gtk::ApplicationWindow::builder()
        .application(application)
        .title(title)
        .resizable(false)
        .build();
    if let Some(parent) = parent {
        window.set_transient_for(Some(parent));
        window.set_modal(true);
    }
    window.add_css_class("qs-window");
    // Kein set_icon_name, siehe „Fenstersymbol“ in ui.rs.
    // Keine eigene Titelleiste, siehe „Fensterdekoration“ in ui.rs.
    window
}

/// Eine Rückfrage als klassisches modales Fenster über `parent` – mit der
/// labwc-Dekoration wie alle anderen Fenster (statt GtkAlertDialog, den GTK
/// ohne Titelleiste und abgerundet zeichnet). `buttons` mit Zugriffstasten
/// („_Abbrechen“); Esc, Schließen-Knopf und `cancel` antworten `cancel`, die
/// Eingabetaste löst `default` aus. `on_answer` wird genau einmal gerufen.
pub struct Question<'a> {
    pub title: &'a str,
    pub message: &'a str,
    pub detail: &'a str,
    pub icon: &'a [&'a str],
    pub buttons: &'a [&'a str],
    pub cancel: usize,
    pub default: usize,
}

pub fn ask(
    parent: &gtk::ApplicationWindow,
    question: Question,
    on_answer: impl Fn(usize) + 'static,
) {
    let Some(application) = parent.application() else {
        on_answer(question.cancel);
        return;
    };
    let parent_window: gtk::Window = parent.clone().upcast();
    let window = titled_window(&application, question.title, Some(&parent_window));
    let root = gtk::Box::new(gtk::Orientation::Vertical, 12);
    root.set_margin_top(12);
    root.set_margin_bottom(12);
    root.set_margin_start(12);
    root.set_margin_end(12);
    let body = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    if let Some(image) = ui::theme_icon(question.icon) {
        image.set_pixel_size(48);
        image.set_valign(gtk::Align::Start);
        body.append(&image);
    }
    let texts = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let message = gtk::Label::new(Some(question.message));
    message.set_xalign(0.0);
    message.add_css_class("qs-heading");
    let detail = gtk::Label::new(Some(question.detail));
    detail.set_xalign(0.0);
    detail.set_wrap(true);
    detail.set_max_width_chars(52);
    texts.append(&message);
    texts.append(&detail);
    body.append(&texts);
    root.append(&body);
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    row.set_halign(gtk::Align::End);
    root.append(&row);
    window.set_child(Some(&root));

    let answered = Rc::new(std::cell::Cell::new(false));
    let on_answer = Rc::new(on_answer);
    let finish = {
        let answered = answered.clone();
        let window = window.clone();
        let on_answer = on_answer.clone();
        // `close`: Fenster selbst schließen (Knopf, Esc); beim Schließen über
        // die Titelleiste schließt es sich ohnehin.
        move |index: usize, close: bool| {
            if answered.replace(true) {
                return;
            }
            if close {
                window.close();
            }
            on_answer(index);
        }
    };
    let finish = Rc::new(finish);
    let mut default_button = None;
    for (index, label) in question.buttons.iter().enumerate() {
        let button = gtk::Button::with_mnemonic(label);
        button.add_css_class("qs-button");
        let finish = finish.clone();
        button.connect_clicked(move |_| finish(index, true));
        if index == question.default {
            default_button = Some(button.clone());
        }
        row.append(&button);
    }
    {
        let finish = finish.clone();
        let cancel = question.cancel;
        window.connect_close_request(move |_| {
            finish(cancel, false);
            glib::Propagation::Proceed
        });
    }
    close_on_escape(&window);
    window.set_default_widget(default_button.as_ref());
    window.present();
    if let Some(button) = default_button {
        button.grab_focus();
    }
}

pub fn show_error(application: &gtk::Application, message: &str) {
    eprintln!("querencia-screenshot: Fehler: {message}");
    let window = titled_window(application, "Bildschirmfoto fehlgeschlagen", None);
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

/// Rückmeldung nach erfolgreichem Speichern, mit dem Pfad.
pub type OnSaved = Box<dyn Fn(&Path)>;

/// Was „Bildschirmfoto speichern“ speichern soll.
pub struct SaveRequest {
    pub png: Png,
    pub description: String,
    /// Fenster, über dem der Dialog modal erscheint.
    pub parent: Option<gtk::Window>,
    pub suggested_name: Option<String>,
    pub suggested_folder: Option<PathBuf>,
    /// Nach erfolgreichem Speichern, mit dem Pfad.
    pub on_saved: Option<OnSaved>,
    /// Beim Schließen des Dialogs (gespeichert oder nicht).
    pub on_closed: Option<Box<dyn Fn()>>,
}

struct Dialog {
    on_saved: Option<OnSaved>,
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

pub fn show_save_dialog(application: &gtk::Application, request: SaveRequest) {
    let SaveRequest {
        png,
        description,
        parent,
        suggested_name: wanted_name,
        suggested_folder,
        on_saved,
        on_closed,
    } = request;
    let shot = Shot { png, description };
    let window = titled_window(application, "Bildschirmfoto speichern", parent.as_ref());
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
    let directory = suggested_folder.unwrap_or_else(storage::default_directory);
    let name = gtk::Entry::new();
    name.set_text(&wanted_name.unwrap_or_else(|| suggested_name(&directory)));
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
        on_saved,
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
    window.connect_close_request(move |_| {
        println!("querencia-screenshot: Dialog geschlossen");
        if let Some(on_closed) = &on_closed {
            on_closed();
        }
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
            if let Some(on_saved) = &dialog.on_saved {
                on_saved(&path);
            }
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

/// „Ordner wählen“: GTK-Ordnerdialog im eigenen Prozess, modal über
/// „Bildschirmfoto speichern“.
///
/// Nicht `gtk::FileDialog` (QREL-04b, F-01): Der schickt die Wahl an
/// xdg-desktop-portal-gtk. Dessen Fenster gehört einem anderen Prozess und
/// hängt nur über xdg-foreign an unserem *modalen* Speicherfenster; labwc
/// 0.9.8 gibt den Tastaturfokus in so einer Fensterfamilie dem modalen
/// Dialog (`desktop_focus_view` → `view_get_modal_dialog`), also unserem
/// Speicherfenster – das Portalfenster bekam nie Tasten (kein Strg+L, keine
/// Pfadeingabe, kein Esc). Ein eigener modaler Dialog ist selbst der modale
/// Dialog seiner Familie und erhält den Fokus; dazu labwc-Titelleiste statt
/// GTK-Kopfleiste und deutsche Knöpfe. GtkFileChooserDialog ist seit GTK
/// 4.10 als veraltet markiert, aber in GTK 4.22 vorhanden.
#[allow(deprecated)]
fn choose_folder(dialog: &Rc<Dialog>) {
    use gtk::prelude::{DialogExt as _, FileChooserExt as _};
    // use_header_bar(0): Knöpfe unten wie in GNOME 2, Titelleiste von
    // labwc (keine GTK-Kopfleiste, siehe „Fensterdekoration“ in ui.rs).
    let chooser = gtk::FileChooserDialog::builder()
        .title("Ordner für das Bildschirmfoto wählen")
        .transient_for(&dialog.window)
        .modal(true)
        .use_header_bar(0)
        .action(gtk::FileChooserAction::SelectFolder)
        .default_width(760)
        .default_height(500)
        .build();
    chooser.add_button("_Abbrechen", gtk::ResponseType::Cancel);
    chooser.add_button("_Auswählen", gtk::ResponseType::Accept);
    chooser.set_default_response(gtk::ResponseType::Accept);
    chooser.add_css_class("qs-window");
    // Kein set_icon_name, siehe „Fenstersymbol“ in ui.rs.
    let current = dialog.folder.borrow().clone();
    let start = current
        .ancestors()
        .find(|path| path.is_dir())
        .map(Path::to_path_buf)
        .unwrap_or_else(glib::home_dir);
    if let Err(error) = chooser.set_current_folder(Some(&gio::File::for_path(&start))) {
        println!(
            "querencia-screenshot: Startordner {}: {error}",
            start.display()
        );
    }
    let dialog = dialog.clone();
    chooser.connect_response(move |chooser, response| {
        let chosen = (response == gtk::ResponseType::Accept)
            .then(|| chooser.file())
            .flatten();
        chooser.destroy();
        let Some(file) = chosen else {
            println!("querencia-screenshot: Ordnerwahl abgebrochen");
            return;
        };
        let Some(path) = file.path() else {
            dialog
                .status
                .set_text("Nur lokale Ordner werden unterstützt.");
            return;
        };
        println!("querencia-screenshot: Ordner gewählt: {}", path.display());
        dialog.folder_label.set_text(&path.display().to_string());
        dialog
            .status
            .set_text(directory_problem(&path).as_deref().unwrap_or(""));
        dialog.folder.replace(path);
    });
    chooser.present();
    println!("querencia-screenshot: Ordnerwahl geöffnet");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        capture::{GRIM, SLURP},
        runner::{Output, RunError, fake::FakeRunner},
        screenshot::sample,
        toplevel::{ActiveWindow, WindowQuery},
    };

    fn exit(status: i32, stdout: &[u8], stderr: &str) -> Result<Output, RunError> {
        Ok(Output {
            status,
            stdout: stdout.to_vec(),
            stderr: stderr.to_owned(),
        })
    }

    fn never() -> Desktop {
        panic!("Fensterabfrage nur für --window")
    }

    fn style() -> FrameStyle {
        FrameStyle::default()
    }

    fn editor(fullscreen: bool) -> Desktop {
        Desktop::without_outputs(WindowQuery::Active(ActiveWindow {
            title: "Brief.txt – Editor".into(),
            app_id: "editor".into(),
            fullscreen,
            maximized: false,
            outputs: vec!["HEADLESS-2".into()],
        }))
    }

    /// Ein echtes Vollbild-PNG 1280×720 mit einem aktiven Fenster bei
    /// 100,80 (500×366) und einem inaktiven bei 700,40 (402×328).
    fn screen_png() -> Vec<u8> {
        use window_pick::tests::{BACKGROUND, window};
        let mut image = Image::filled(1280, 720, BACKGROUND);
        window(&mut image, 700, 40, 402, 328, false);
        window(&mut image, 100, 80, 500, 366, true);
        encode(&image).expect("png").bytes().to_vec()
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
        let shot = perform_with_style(&runner, Target::Window, || editor(true), style)
            .expect("ok")
            .expect("shot");
        assert_eq!(
            runner.calls.borrow()[0].args,
            vec!["-t", "png", "-l", "1", "-o", "HEADLESS-2", "-"]
        );
        assert!(shot.description.contains("Vollbild auf HEADLESS-2"));
    }

    #[test]
    fn a_detected_window_is_offered_and_cut_from_the_frozen_image() {
        let runner = FakeRunner::default()
            .respond(GRIM, exit(0, &screen_png(), ""))
            .respond(SLURP, exit(0, b"100,80 500x366;f1\n", ""));
        let shot = perform_with_style(&runner, Target::Window, || editor(false), style)
            .expect("ok")
            .expect("shot");
        assert_eq!(shot.png.dimensions(), Some((500, 366)));
        assert_eq!(
            shot.description,
            "Fenster „Brief.txt – Editor“ (gewählt 100,80 500x366)"
        );
        // Linke obere Ecke des Ausschnitts: Rahmen links (aktiv) unter der
        // Rundung, rechts unten der Rahmen.
        let (r, g, b) = crate::render::tests::pixel(&shot.png, 0, 20);
        assert_eq!((r, g, b), (0x7a, 0x75, 0x70));
        assert_eq!(
            crate::render::tests::pixel(&shot.png, 499, 365),
            (0x7a, 0x75, 0x70)
        );
        let calls = runner.calls.borrow();
        assert_eq!(
            calls.len(),
            2,
            "ein grim (eingefroren), ein slurp, kein zweites grim"
        );
        assert_eq!(calls[0].args, vec!["-t", "png", "-l", "1", "-"]);
        assert_eq!(calls[1].program, SLURP);
        let boxes = String::from_utf8(calls[1].stdin.clone().expect("stdin")).expect("utf8");
        assert_eq!(boxes, "700,40 402x328 f0\n100,80 500x366 f1\n");
    }

    #[test]
    fn an_inactive_window_is_named_without_the_active_title() {
        let runner = FakeRunner::default()
            .respond(GRIM, exit(0, &screen_png(), ""))
            .respond(SLURP, exit(0, b"700,40 402x328;f0\n", ""));
        let shot = perform_with_style(&runner, Target::Window, || editor(false), style)
            .expect("ok")
            .expect("shot");
        assert_eq!(shot.png.dimensions(), Some((402, 328)));
        assert_eq!(shot.description, "Fenster (gewählt 700,40 402x328)");
    }

    #[test]
    fn dragging_still_frames_freely_and_says_so() {
        let runner = FakeRunner::default()
            .respond(GRIM, exit(0, &screen_png(), ""))
            .respond(SLURP, exit(0, b"10,20 300x200;\n", ""));
        let shot = perform_with_style(&runner, Target::Window, || editor(false), style)
            .expect("ok")
            .expect("shot");
        assert_eq!(shot.png.dimensions(), Some((300, 200)));
        assert_eq!(
            shot.description,
            "Fenster „Brief.txt – Editor“, mit der Maus eingerahmt (10,20 300x200)"
        );
        assert_eq!(runner.calls.borrow().len(), 2, "aus dem eingefrorenen Bild");
    }

    #[test]
    fn cancelling_the_frame_creates_nothing() {
        let runner = FakeRunner::default()
            .respond(GRIM, exit(0, &screen_png(), ""))
            .respond(SLURP, exit(1, b"", "selection cancelled\n"));
        assert!(
            perform_with_style(&runner, Target::Window, || editor(false), style)
                .expect("ok")
                .is_none()
        );
        assert_eq!(runner.calls.borrow().len(), 2, "kein zweites grim");
        let runner = FakeRunner::default().respond(SLURP, exit(1, b"", "selection cancelled\n"));
        assert!(perform(&runner, Target::Area, never).expect("ok").is_none());
        assert_eq!(runner.calls.borrow().len(), 1, "grim nicht gestartet");
    }

    /// Echte Bildschirmfotos aus dem isolierten labwc prüfen (nur von Hand):
    /// `QS_FENSTERBILDER=a.png:b.png cargo test echte -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn echte_bildschirmfotos() {
        let paths = std::env::var("QS_FENSTERBILDER").unwrap_or_default();
        for path in paths.split(':').filter(|p| !p.is_empty()) {
            let png = Png::new(std::fs::read(path).expect("lesen")).expect("png");
            let image = decode(&png).expect("decode");
            let outputs: Vec<window_pick::Output> = (0..image.width / 1280)
                .map(|i| window_pick::Output {
                    has_maximized: std::env::var("QS_MAXIMIERT").is_ok(),
                    usable_height: std::env::var("QS_MAXIMIERT")
                        .ok()
                        .and_then(|h| h.parse().ok()),
                    ..window_pick::tests::output("A", i * 1280, 0, 1280, 720)
                })
                .collect();
            let started = Instant::now();
            let found = window_pick::detect(&image, &style(), &outputs);
            let boxes = window_pick::slurp_boxes(&image, &style(), &found);
            println!("{path}: {} ms", started.elapsed().as_millis());
            for f in &found {
                println!("  {}", f.describe());
            }
            for (b, i) in &boxes {
                println!("  box f{i} {}", b.grim_argument());
            }
        }
    }

    #[test]
    fn layout_maps_two_outputs_and_negative_origins_into_the_image() {
        let outputs = vec![
            toplevel::OutputInfo {
                name: "L".into(),
                x: -1280,
                y: 0,
                width: 1280,
                height: 720,
                usable: Some((1280, 690)),
                has_maximized: true,
            },
            toplevel::OutputInfo {
                name: "R".into(),
                x: 0,
                y: 0,
                width: 1280,
                height: 720,
                ..Default::default()
            },
        ];
        let layout = Layout::new(&outputs, 2560, 720);
        assert_eq!(layout.scale, Some(1));
        assert_eq!(layout.outputs[0].area.x, 0);
        assert_eq!(layout.outputs[1].area.x, 1280);
        assert_eq!(layout.outputs[0].usable_height, Some(690));
        let area = Region {
            x: 1300,
            y: 10,
            width: 5,
            height: 5,
        };
        assert_eq!(layout.to_layout(area).x, 20);
        assert_eq!(layout.to_image(layout.to_layout(area)), area);
        assert_eq!(Layout::new(&outputs, 5120, 1440).scale, Some(2));
        assert_eq!(Layout::new(&outputs, 2000, 720).scale, None);
    }

    #[test]
    fn no_active_window_means_whole_screen_like_gnome_2() {
        let runner = FakeRunner::default().respond(GRIM, exit(0, &sample(2, 2), ""));
        let shot = perform(&runner, Target::Window, || {
            Desktop::without_outputs(WindowQuery::NoActiveWindow)
        })
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
