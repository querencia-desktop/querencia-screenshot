// SPDX-License-Identifier: MIT

//! Bildeditor nach der Aufnahme (QS-0d): große Vorschau, klassische
//! Menüleiste und Werkzeugleiste, Markierungen (Stift, Kreis/Ellipse,
//! Rechteck, Pfeil, Text) in acht Grundfarben (aufklappbare Palette) oder
//! einer benutzerdefinierten Farbe und drei Stärken, Rückgängig und
//! Wiederholen, Speichern und Kopieren des fertigen Bildes.
//!
//! Die Aufnahme bleibt unverändert (`original`); Markierungen sind eigene
//! Objekte in Originalkoordinaten (`annotation::Marks`). Erst Speichern und
//! Kopieren rechnen beides zu einem neuen PNG zusammen (`render::compose`).
//! Menü, Werkzeugleiste und Tastenkürzel lösen dieselben Fensteraktionen
//! (`win.*`) aus.

use std::{
    cell::{Cell, RefCell},
    path::{Path, PathBuf},
    rc::{Rc, Weak},
};

use gtk::{cairo, gdk, gio, glib, prelude::*};

use crate::{
    annotation::{Color, Mark, Marks, Point, Shape, Tool, Width, constrain},
    clipboard, instant, render,
    runner::{self, CommandRunner as _},
    screenshot::Png,
    storage, ui,
};

/// Rand um das Bild in der Anzeige (Bildschirmpixel).
const MARGIN: f64 = 12.0;

/// Ab wann ein neuer Stiftpunkt aufgenommen wird (Originalpixel).
const PEN_STEP: f64 = 1.5;

/// Eine Aufnahme, die bearbeitet werden soll.
pub struct Source {
    pub png: Png,
    pub description: String,
    /// Dateiname, den „Speichern“ vorschlägt (sonst Zeitstempel).
    pub suggested_name: Option<String>,
    /// Ordner, den „Speichern“ vorschlägt (sonst Bilder/Screenshots).
    pub suggested_folder: Option<PathBuf>,
}

/// Abbildung Originalpixel ↔ Anzeige.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub scale: f64,
    pub ox: f64,
    pub oy: f64,
}

impl View {
    /// Einpassen (nie vergrößern) oder Originalgröße, zentriert.
    pub fn compute(fit: bool, area: (f64, f64), image: (f64, f64)) -> Self {
        let (aw, ah) = area;
        let (iw, ih) = image;
        let scale = if fit {
            ((aw - 2.0 * MARGIN) / iw)
                .min((ah - 2.0 * MARGIN) / ih)
                .clamp(0.01, 1.0)
        } else {
            1.0
        };
        Self {
            scale,
            ox: ((aw - iw * scale) / 2.0).max(MARGIN).floor(),
            oy: ((ah - ih * scale) / 2.0).max(MARGIN).floor(),
        }
    }

    pub fn to_image(self, x: f64, y: f64, image: (f64, f64)) -> Point {
        Point::new(
            ((x - self.ox) / self.scale).clamp(0.0, image.0),
            ((y - self.oy) / self.scale).clamp(0.0, image.1),
        )
    }

    pub fn to_area(self, point: Point) -> (f64, f64) {
        (
            point.x * self.scale + self.ox,
            point.y * self.scale + self.oy,
        )
    }
}

struct Editor {
    window: gtk::ApplicationWindow,
    area: gtk::DrawingArea,
    scroller: gtk::ScrolledWindow,
    status: gtk::Label,
    drag: gtk::GestureDrag,
    /// Texteingabe direkt über der Zeichenfläche, an der Klickstelle
    /// (keine eigene Wayland-Oberfläche wie bei einem Popover).
    text_box: gtk::Box,
    entry: gtk::Entry,
    original: Png,
    description: String,
    suggested_name: Option<String>,
    suggested_folder: Option<PathBuf>,
    base: cairo::ImageSurface,
    /// Verkleinerte Fassung des Bildes für die aktuelle Skalierung.
    scaled: RefCell<Option<(f64, cairo::ImageSurface)>>,
    marks: RefCell<Marks>,
    current: RefCell<Option<Mark>>,
    drag_start: Cell<Option<Point>>,
    text_at: Cell<Option<Point>>,
    tool: Cell<Tool>,
    color: Cell<Color>,
    /// Farbschaltfläche der Werkzeugleiste (gesetzt, sobald sie gebaut ist).
    palette: RefCell<Option<Palette>>,
    width: Cell<Width>,
    fit: Cell<bool>,
    busy: Cell<bool>,
    force_close: Cell<bool>,
    close_after_save: Cell<bool>,
    message: RefCell<String>,
}

impl Editor {
    fn image_size(&self) -> (f64, f64) {
        (f64::from(self.base.width()), f64::from(self.base.height()))
    }

    fn view(&self) -> View {
        View::compute(
            self.fit.get(),
            (f64::from(self.area.width()), f64::from(self.area.height())),
            self.image_size(),
        )
    }

    fn action(&self, name: &str) -> Option<gio::SimpleAction> {
        self.window
            .lookup_action(name)
            .and_then(|action| action.downcast::<gio::SimpleAction>().ok())
    }

    fn set_enabled(&self, name: &str, enabled: bool) {
        if let Some(action) = self.action(name) {
            action.set_enabled(enabled);
        }
    }

    /// Menü- und Schaltflächenzustand nach jeder Änderung.
    fn refresh(&self) {
        let text_mode = self.text_box.is_visible();
        let marks = self.marks.borrow();
        for name in [
            "new",
            "save",
            "copy",
            "close",
            "tool",
            "color",
            "palette",
            "color-custom",
            "width",
            "fit",
        ] {
            self.set_enabled(name, !text_mode && !self.busy.get());
        }
        if let Some(palette) = self.palette.borrow().as_ref() {
            let color = self.color.get();
            palette.button.set_sensitive(!text_mode && !self.busy.get());
            palette.button.set_tooltip_text(Some(&format!(
                "Farbe: {} – Farbpalette öffnen (F)",
                color.label()
            )));
            palette.current.queue_draw();
            palette.custom_swatch.set_visible(color.is_custom());
            palette.custom_swatch.queue_draw();
        }
        self.set_enabled("undo", !text_mode && marks.can_undo());
        self.set_enabled("redo", !text_mode && marks.can_redo());
        self.set_enabled("clear", !text_mode && !marks.items().is_empty());
        drop(marks);
        self.update_status();
        self.area.queue_draw();
    }

    fn update_status(&self) {
        let (iw, ih) = self.image_size();
        let view = self.view();
        let hint = match self.tool.get() {
            Tool::Text => "auf das Bild klicken und Text eingeben",
            Tool::Pen => "mit gedrückter Maustaste auf dem Bild zeichnen",
            _ => "mit gedrückter Maustaste auf dem Bild ziehen",
        };
        let count = self.marks.borrow().items().len();
        let mut text = format!(
            "{} × {} Pixel · {:.0} % · {}: {} ({}, {}) · {} Markierung{}",
            iw,
            ih,
            view.scale * 100.0,
            self.tool.get().label(),
            hint,
            self.color.get().label(),
            self.width.get().label(),
            count,
            if count == 1 { "" } else { "en" }
        );
        let message = self.message.borrow();
        if !message.is_empty() {
            text = format!("{message} – {text}");
        }
        self.status.set_text(&text);
    }

    fn say(&self, message: &str) {
        println!("querencia-screenshot: {message}");
        self.message.replace(message.to_owned());
        self.update_status();
    }

    fn new_mark(&self, shape: Shape) -> Mark {
        Mark {
            shape,
            color: self.color.get(),
            width: self.width.get(),
        }
    }

    fn compose(&self) -> Result<Png, render::RenderError> {
        render::compose(&self.original, self.marks.borrow().items())
    }
}

/// Öffnet einen Editor für `source`.
pub fn open(application: &gtk::Application, source: Source) {
    let base = match render::surface_from_png(&source.png) {
        Ok(base) => base,
        Err(error) => {
            instant::show_error(application, &error.to_string());
            return;
        }
    };
    let window = gtk::ApplicationWindow::builder()
        .application(application)
        .title("Querencia Screenshot – Bearbeiten")
        .build();
    window.add_css_class("qs-window");
    window.add_css_class("qs-editor");
    // Kein set_icon_name, siehe „Fenstersymbol“ in ui.rs.
    // Keine eigene Titelleiste, siehe „Fensterdekoration“ in ui.rs.

    let area = gtk::DrawingArea::new();
    area.set_hexpand(true);
    area.set_vexpand(true);
    area.set_cursor_from_name(Some("crosshair"));
    area.set_focusable(true);
    let scroller = gtk::ScrolledWindow::new();
    scroller.set_child(Some(&area));
    scroller.set_hexpand(true);
    scroller.set_vexpand(true);
    scroller.add_css_class("qs-canvas");
    let status = gtk::Label::new(None);
    status.set_xalign(0.0);
    status.set_ellipsize(gtk::pango::EllipsizeMode::End);
    status.add_css_class("qs-statusbar");

    let entry = gtk::Entry::new();
    entry.set_width_chars(28);
    entry.set_placeholder_text(Some("Text eingeben"));
    let text_hint = gtk::Label::new(Some("Eingabetaste übernimmt, Esc verwirft"));
    text_hint.add_css_class("qs-hint");
    let text_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text_box.add_css_class("qs-textbox");
    text_box.append(&entry);
    text_box.append(&text_hint);
    text_box.set_halign(gtk::Align::Start);
    text_box.set_valign(gtk::Align::Start);
    text_box.set_visible(false);
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&scroller));
    overlay.add_overlay(&text_box);

    let drag = gtk::GestureDrag::new();
    drag.set_button(gdk::BUTTON_PRIMARY);
    area.add_controller(drag.clone());

    let editor = Rc::new(Editor {
        window: window.clone(),
        area: area.clone(),
        scroller: scroller.clone(),
        status: status.clone(),
        drag: drag.clone(),
        text_box: text_box.clone(),
        entry: entry.clone(),
        original: source.png,
        description: source.description,
        suggested_name: source.suggested_name,
        suggested_folder: source.suggested_folder,
        base,
        scaled: RefCell::new(None),
        marks: RefCell::new(Marks::default()),
        current: RefCell::new(None),
        drag_start: Cell::new(None),
        text_at: Cell::new(None),
        tool: Cell::new(Tool::Ellipse),
        color: Cell::new(Color::RED),
        palette: RefCell::new(None),
        width: Cell::new(Width::Medium),
        fit: Cell::new(true),
        busy: Cell::new(false),
        force_close: Cell::new(false),
        close_after_save: Cell::new(false),
        message: RefCell::new(String::new()),
    });

    install_actions(application, &editor);
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.append(&menu_bar());
    root.append(&toolbar(&editor));
    root.append(&overlay);
    root.append(&status);
    window.set_child(Some(&root));

    connect_canvas(&editor);
    connect_text(&editor);
    connect_keys(&editor);
    connect_close(&editor);

    // Fenstergröße: Bild in Originalgröße, höchstens 85 % des Bildschirms.
    let (iw, ih) = editor.image_size();
    let (mw, mh) = monitor_size(&window).unwrap_or((1280.0, 800.0));
    let chrome_height = 130.0;
    let width = (iw + 2.0 * MARGIN).clamp(900.0, (mw * 0.85).max(900.0));
    let height = (ih + 2.0 * MARGIN + chrome_height).clamp(400.0, (mh * 0.85).max(400.0));
    window.set_default_size(width as i32, height as i32);

    // Die letzte Referenz auf den Editor hält das Fenster bis zum Ende.
    let keep = RefCell::new(Some(editor.clone()));
    window.connect_destroy(move |_| {
        keep.borrow_mut().take();
    });

    editor.refresh();
    window.present();
    area.grab_focus();
    println!(
        "querencia-screenshot: Editor geöffnet: {} ({} × {} Pixel)",
        editor.description, iw, ih
    );
}

fn monitor_size(window: &gtk::ApplicationWindow) -> Option<(f64, f64)> {
    let display = gtk::prelude::WidgetExt::display(window);
    let monitors = display.monitors();
    let monitor = monitors.item(0)?.downcast::<gdk::Monitor>().ok()?;
    let geometry = monitor.geometry();
    Some((f64::from(geometry.width()), f64::from(geometry.height())))
}

// ---------------------------------------------------------------- Aktionen

fn upgrade(weak: &Weak<Editor>) -> Option<Rc<Editor>> {
    weak.upgrade()
}

fn install_actions(application: &gtk::Application, editor: &Rc<Editor>) {
    let window = &editor.window;
    let simple = |name: &str, run: fn(&Rc<Editor>)| {
        let action = gio::SimpleAction::new(name, None);
        let weak = Rc::downgrade(editor);
        action.connect_activate(move |_, _| {
            if let Some(editor) = upgrade(&weak) {
                run(&editor);
            }
        });
        window.add_action(&action);
    };
    simple("new", |editor| {
        if let Some(application) = editor.window.application() {
            ui::present(&application);
        }
    });
    simple("save", save);
    simple("copy", copy);
    simple("close", |editor| editor.window.close());
    simple("undo", |editor| {
        if editor.marks.borrow_mut().undo() {
            editor.say("Rückgängig gemacht");
        }
        editor.refresh();
    });
    simple("redo", |editor| {
        if editor.marks.borrow_mut().redo() {
            editor.say("Wiederholt");
        }
        editor.refresh();
    });
    simple("clear", confirm_clear);

    let stateful = |name: &str, initial: &str, apply: fn(&Editor, &str) -> bool| {
        let action = gio::SimpleAction::new_stateful(
            name,
            Some(glib::VariantTy::STRING),
            &initial.to_variant(),
        );
        let weak = Rc::downgrade(editor);
        action.connect_change_state(move |action, value| {
            let (Some(editor), Some(value)) = (upgrade(&weak), value) else {
                return;
            };
            if let Some(id) = value.str()
                && apply(&editor, id)
            {
                action.set_state(value);
                editor.message.replace(String::new());
                editor.refresh();
            }
        });
        window.add_action(&action);
    };
    stateful("tool", Tool::Ellipse.id(), |editor, id| {
        Tool::from_id(id)
            .map(|tool| editor.tool.set(tool))
            .is_some()
    });
    stateful("color", &Color::RED.id(), |editor, id| {
        let Some(color) = Color::from_id(id) else {
            return false;
        };
        editor.color.set(color);
        println!("querencia-screenshot: Farbe: {}", color.label());
        true
    });
    simple("palette", |editor| {
        if let Some(palette) = editor.palette.borrow().as_ref() {
            palette.button.popup();
        }
    });
    simple("color-custom", choose_custom_color);
    stateful("width", Width::Medium.id(), |editor, id| {
        Width::from_id(id)
            .map(|width| editor.width.set(width))
            .is_some()
    });

    let fit = gio::SimpleAction::new_stateful("fit", None, &true.to_variant());
    let weak = Rc::downgrade(editor);
    fit.connect_change_state(move |action, value| {
        let (Some(editor), Some(value)) = (upgrade(&weak), value) else {
            return;
        };
        let fit = value.get::<bool>().unwrap_or(true);
        action.set_state(value);
        editor.fit.set(fit);
        apply_zoom(&editor);
    });
    window.add_action(&fit);

    for (action, accels) in [
        ("win.new", &["<Control>n"][..]),
        ("win.save", &["<Control>s"]),
        ("win.copy", &["<Control>c"]),
        ("win.close", &["<Control>w"]),
        ("win.undo", &["<Control>z"]),
        ("win.redo", &["<Control><Shift>z", "<Control>y"]),
        ("win.palette", &["f"]),
    ] {
        application.set_accels_for_action(action, accels);
    }
    for tool in Tool::ALL {
        application.set_accels_for_action(&format!("win.tool::{}", tool.id()), &[tool.key()]);
    }
}

fn apply_zoom(editor: &Rc<Editor>) {
    let (iw, ih) = editor.image_size();
    if editor.fit.get() {
        editor.area.set_content_width(0);
        editor.area.set_content_height(0);
        editor
            .scroller
            .set_policy(gtk::PolicyType::Never, gtk::PolicyType::Never);
    } else {
        editor.area.set_content_width((iw + 2.0 * MARGIN) as i32);
        editor.area.set_content_height((ih + 2.0 * MARGIN) as i32);
        editor
            .scroller
            .set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
    }
    editor.scaled.replace(None);
    editor.refresh();
}

fn menu_bar() -> gtk::PopoverMenuBar {
    let menu = gio::Menu::new();
    let file = gio::Menu::new();
    file.append(Some("_Neue Aufnahme"), Some("win.new"));
    let section = gio::Menu::new();
    section.append(Some("_Speichern …"), Some("win.save"));
    section.append(Some("In die _Zwischenablage kopieren"), Some("win.copy"));
    file.append_section(None, &section);
    let section = gio::Menu::new();
    section.append(Some("S_chließen"), Some("win.close"));
    file.append_section(None, &section);
    menu.append_submenu(Some("_Datei"), &file);

    let edit = gio::Menu::new();
    edit.append(Some("_Rückgängig"), Some("win.undo"));
    edit.append(Some("_Wiederholen"), Some("win.redo"));
    let section = gio::Menu::new();
    section.append(Some("_Alle Markierungen entfernen …"), Some("win.clear"));
    edit.append_section(None, &section);
    menu.append_submenu(Some("_Bearbeiten"), &edit);

    let tools = gio::Menu::new();
    let section = gio::Menu::new();
    for tool in Tool::ALL {
        section.append(
            Some(tool.label()),
            Some(&format!("win.tool::{}", tool.id())),
        );
    }
    tools.append_section(None, &section);
    let colors = gio::Menu::new();
    for named in &Color::PALETTE {
        colors.append(Some(named.label), Some(&format!("win.color::{}", named.id)));
    }
    colors.append(
        Some("_Benutzerdefinierte Farbe …"),
        Some("win.color-custom"),
    );
    tools.append_section(Some("Farbe"), &colors);
    let widths = gio::Menu::new();
    for width in Width::ALL {
        widths.append(
            Some(width.label()),
            Some(&format!("win.width::{}", width.id())),
        );
    }
    tools.append_section(Some("Strichstärke"), &widths);
    menu.append_submenu(Some("_Werkzeuge"), &tools);

    let view = gio::Menu::new();
    view.append(Some("Ins Fenster _einpassen"), Some("win.fit"));
    menu.append_submenu(Some("_Ansicht"), &view);

    let bar = gtk::PopoverMenuBar::from_model(Some(&menu));
    bar.add_css_class("qs-menubar");
    bar
}

// ---------------------------------------------------------- Werkzeugleiste

/// Kleines Symbol, mit cairo gezeichnet (unabhängig vom Symbolthema).
fn drawn_icon(size: i32, paint: impl Fn(&cairo::Context, f64) + 'static) -> gtk::DrawingArea {
    let icon = gtk::DrawingArea::new();
    icon.set_content_width(size);
    icon.set_content_height(size);
    icon.set_draw_func(move |_, cr, w, h| paint(cr, f64::from(w.min(h))));
    icon
}

fn tool_icon(tool: Tool) -> gtk::DrawingArea {
    drawn_icon(20, move |cr, s| {
        cr.set_source_rgb(0.19, 0.19, 0.19);
        cr.set_line_width(2.0);
        cr.set_line_cap(cairo::LineCap::Round);
        cr.set_line_join(cairo::LineJoin::Round);
        match tool {
            Tool::Pen => {
                cr.move_to(s * 0.15, s * 0.75);
                cr.curve_to(s * 0.3, s * 0.2, s * 0.5, s * 0.95, s * 0.65, s * 0.45);
                cr.line_to(s * 0.85, s * 0.2);
                cr.stroke().ok();
            }
            Tool::Ellipse => {
                cr.save().ok();
                cr.translate(s * 0.5, s * 0.5);
                cr.scale(s * 0.42, s * 0.32);
                cr.arc(0.0, 0.0, 1.0, 0.0, std::f64::consts::TAU);
                cr.restore().ok();
                cr.set_source_rgb(0.9, 0.0, 0.0);
                cr.stroke().ok();
            }
            Tool::Rectangle => {
                cr.rectangle(s * 0.12, s * 0.25, s * 0.76, s * 0.5);
                cr.stroke().ok();
            }
            Tool::Arrow => {
                cr.move_to(s * 0.15, s * 0.85);
                cr.line_to(s * 0.72, s * 0.28);
                cr.stroke().ok();
                cr.move_to(s * 0.88, s * 0.12);
                cr.line_to(s * 0.52, s * 0.22);
                cr.line_to(s * 0.78, s * 0.48);
                cr.close_path();
                cr.fill().ok();
            }
            Tool::Text => {
                cr.select_font_face("Serif", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
                cr.set_font_size(s * 0.9);
                cr.move_to(s * 0.2, s * 0.85);
                cr.show_text("T").ok();
            }
        }
    })
}

/// Farbfeld mit dezentem dunklem Rand.
fn paint_swatch(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, color: Color) {
    let (r, g, b) = color.rgb();
    cr.rectangle(x + 0.5, y + 0.5, w - 1.0, h - 1.0);
    cr.set_source_rgb(r, g, b);
    cr.fill_preserve().ok();
    cr.set_source_rgb(0.31, 0.31, 0.31);
    cr.set_line_width(1.0);
    cr.stroke().ok();
}

/// Farbschaltfläche der Werkzeugleiste: zeigt die aktuelle Farbe, klappt
/// die Palette (Popover) auf.
struct Palette {
    button: gtk::MenuButton,
    popover: gtk::Popover,
    /// Farbfeld auf der Schaltfläche.
    current: gtk::DrawingArea,
    /// Farbfeld vor „Benutzerdefinierte Farbe …“, nur bei eigener Farbe.
    custom_swatch: gtk::DrawingArea,
}

fn current_color(weak: &Weak<Editor>) -> Color {
    upgrade(weak).map_or(Color::RED, |editor| editor.color.get())
}

/// Eine Grundfarbe im Palettenraster; die gewählte ist eingedrückt,
/// orange umrandet und mit einem Häkchen versehen.
fn palette_swatch(name: &'static str, label: &'static str, color: Color) -> gtk::ToggleButton {
    let toggle = gtk::ToggleButton::new();
    let area = gtk::DrawingArea::new();
    area.set_content_width(26);
    area.set_content_height(18);
    let weak_toggle = toggle.downgrade();
    area.set_draw_func(move |_, cr, w, h| {
        let (w, h) = (f64::from(w), f64::from(h));
        paint_swatch(cr, 0.0, 0.0, w, h, color);
        if weak_toggle
            .upgrade()
            .is_some_and(|toggle| toggle.is_active())
        {
            let mark = if color.is_light() { 0.0 } else { 1.0 };
            cr.set_source_rgb(mark, mark, mark);
            cr.set_line_width(2.0);
            cr.set_line_cap(cairo::LineCap::Round);
            cr.set_line_join(cairo::LineJoin::Round);
            cr.move_to(w * 0.30, h * 0.52);
            cr.line_to(w * 0.45, h * 0.75);
            cr.line_to(w * 0.72, h * 0.25);
            cr.stroke().ok();
        }
    });
    toggle.set_child(Some(&area));
    toggle.connect_toggled(move |_| area.queue_draw());
    toggle.set_tooltip_text(Some(label));
    toggle.update_property(&[gtk::accessible::Property::Label(label)]);
    toggle.set_action_name(Some("win.color"));
    toggle.set_action_target_value(Some(&name.to_variant()));
    toggle.add_css_class("qs-swatch");
    toggle
}

fn palette_button(editor: &Rc<Editor>) -> gtk::MenuButton {
    let weak = Rc::downgrade(editor);
    let current = gtk::DrawingArea::new();
    current.set_content_width(32);
    current.set_content_height(18);
    current.set_draw_func(move |_, cr, w, h| {
        let (w, h) = (f64::from(w), f64::from(h));
        paint_swatch(cr, 0.0, 0.0, w - 10.0, h, current_color(&weak));
        // Klapppfeil wie bei klassischen Auswahlknöpfen
        cr.set_source_rgb(0.19, 0.19, 0.19);
        cr.move_to(w - 7.0, h / 2.0 - 2.0);
        cr.line_to(w - 1.0, h / 2.0 - 2.0);
        cr.line_to(w - 4.0, h / 2.0 + 2.0);
        cr.close_path();
        cr.fill().ok();
    });

    let popover = gtk::Popover::new();
    let grid = gtk::Grid::new();
    grid.set_row_spacing(3);
    grid.set_column_spacing(3);
    for (i, named) in Color::PALETTE.iter().enumerate() {
        let toggle = palette_swatch(named.id, named.label, named.color);
        let weak_popover = popover.downgrade();
        toggle.connect_clicked(move |_| {
            if let Some(popover) = weak_popover.upgrade() {
                popover.popdown();
            }
        });
        grid.attach(&toggle, (i % 4) as i32, (i / 4) as i32, 1, 1);
    }

    let weak = Rc::downgrade(editor);
    let custom_swatch = gtk::DrawingArea::new();
    custom_swatch.set_content_width(16);
    custom_swatch.set_content_height(12);
    custom_swatch.set_valign(gtk::Align::Center);
    custom_swatch.set_draw_func(move |_, cr, w, h| {
        paint_swatch(
            cr,
            0.0,
            0.0,
            f64::from(w),
            f64::from(h),
            current_color(&weak),
        );
    });
    custom_swatch.set_visible(false);
    let custom_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    custom_box.append(&custom_swatch);
    custom_box.append(&gtk::Label::with_mnemonic("_Benutzerdefinierte Farbe …"));
    let custom = gtk::Button::new();
    custom.set_child(Some(&custom_box));
    custom.set_tooltip_text(Some("Eine beliebige Farbe im Farbdialog wählen"));
    custom.set_action_name(Some("win.color-custom"));
    custom.add_css_class("qs-custom");

    let content = gtk::Box::new(gtk::Orientation::Vertical, 4);
    content.append(&grid);
    content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    content.append(&custom);
    popover.set_child(Some(&content));
    popover.set_has_arrow(false);
    popover.set_position(gtk::PositionType::Bottom);
    popover.add_css_class("qs-palette");
    // Tastatur: beim Öffnen steht der Fokus auf der gewählten Farbe
    // (sonst auf der ersten); Pfeiltasten wandern im Raster, Leertaste oder
    // Eingabetaste wählen, Esc schließt.
    popover.connect_map(move |_| {
        let mut child = grid.first_child();
        let mut first = None;
        while let Some(widget) = child {
            if let Ok(toggle) = widget.clone().downcast::<gtk::ToggleButton>() {
                if toggle.is_active() {
                    toggle.grab_focus();
                    return;
                }
                first.get_or_insert(toggle);
            }
            child = widget.next_sibling();
        }
        if let Some(toggle) = first {
            toggle.grab_focus();
        }
    });

    let button = gtk::MenuButton::new();
    button.set_child(Some(&current));
    button.set_popover(Some(&popover));
    button.set_focus_on_click(false);
    button.add_css_class("qs-palette-button");
    button.update_property(&[gtk::accessible::Property::Label("Farbe")]);
    editor.palette.replace(Some(Palette {
        button: button.clone(),
        popover,
        current,
        custom_swatch,
    }));
    button
}

/// „Benutzerdefinierte Farbe …“: kleines Fenster mit dem GTK-Farbwähler
/// (Farbfeld, Farbton, Eingabe „#RRGGBB“), vorbelegt mit der aktuellen
/// Farbe. Die Grundfarben bleiben in der kompakten Palette.
///
/// Nicht `gtk::ColorDialog`: der zeichnet eine eigene GTK-Kopfleiste (gegen
/// die labwc-Fensterdekoration, siehe ui.rs) und spricht ohne installierte
/// GTK-Übersetzungen Englisch („Cancel“, „Select“). Hier: labwc-Titelleiste,
/// deutsche Knöpfe unten wie in GNOME 2.
fn choose_custom_color(editor: &Rc<Editor>) {
    if let Some(palette) = editor.palette.borrow().as_ref() {
        palette.popover.popdown();
    }
    let window = gtk::Window::builder()
        .title("Benutzerdefinierte Farbe")
        .transient_for(&editor.window)
        .modal(true)
        .resizable(false)
        .build();
    window.add_css_class("qs-window");
    window.add_css_class("qs-color-window");
    // Kein set_icon_name, siehe „Fenstersymbol“ in ui.rs.

    // GtkColorChooserWidget ist seit GTK 4.10 als veraltet markiert, aber
    // vorhanden; sein Ersatz `gtk::ColorDialog` passt hier nicht (oben).
    #[allow(deprecated)]
    let chooser = {
        use gtk::prelude::ColorChooserExt as _;
        let chooser = gtk::ColorChooserWidget::new();
        chooser.set_use_alpha(false);
        let (r, g, b) = editor.color.get().rgb();
        chooser.set_rgba(&gdk::RGBA::new(r as f32, g as f32, b as f32, 1.0));
        chooser.set_show_editor(true);
        chooser
    };
    let hint = gtk::Label::new(Some(
        "Farbton und Farbfeld wählen oder „#RRGGBB“ eingeben.",
    ));
    hint.set_xalign(0.0);
    hint.add_css_class("qs-hint");
    let cancel = ui::button("Abbrechen", &["process-stop", "window-close"]);
    let apply = ui::button("Übernehmen", &["dialog-ok", "dialog-apply", "gtk-apply"]);
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    buttons.set_halign(gtk::Align::End);
    buttons.append(&cancel);
    buttons.append(&apply);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    content.set_margin_top(10);
    content.set_margin_bottom(10);
    content.set_margin_start(10);
    content.set_margin_end(10);
    content.append(&hint);
    content.append(&chooser);
    content.append(&buttons);
    window.set_child(Some(&content));
    window.set_default_widget(Some(&apply));

    // finish(Some(farbe)) übernimmt, finish(None) bricht ab; beides schließt.
    let weak = Rc::downgrade(editor);
    let finish: Rc<dyn Fn(Option<Color>)> = Rc::new({
        let window = window.downgrade();
        move |choice: Option<Color>| {
            let Some(window) = window.upgrade() else {
                return;
            };
            match (choice, upgrade(&weak)) {
                (Some(color), Some(editor)) => {
                    if WidgetExt::activate_action(
                        &editor.window,
                        "win.color",
                        Some(&color.id().to_variant()),
                    )
                    .is_ok()
                    {
                        editor.say(&format!("Farbe gewählt: {}", color.label()));
                    }
                }
                _ => println!("querencia-screenshot: Farbdialog abgebrochen"),
            }
            window.destroy();
        }
    });
    let chosen = {
        let chooser = chooser.clone();
        move || {
            #[allow(deprecated)]
            let rgba = gtk::prelude::ColorChooserExt::rgba(&chooser);
            Color::from_unit(rgba.red(), rgba.green(), rgba.blue())
        }
    };
    let done = finish.clone();
    cancel.connect_clicked(move |_| done(None));
    let (done, color) = (finish.clone(), chosen.clone());
    apply.connect_clicked(move |_| done(Some(color())));
    // Doppelklick auf ein Farbfeld übernimmt wie in GNOME 2.
    let (done, color) = (finish.clone(), chosen.clone());
    #[allow(deprecated)]
    gtk::prelude::ColorChooserExt::connect_color_activated(&chooser, move |_, _| {
        done(Some(color()))
    });
    let done = finish.clone();
    window.connect_close_request(move |_| {
        done(None);
        glib::Propagation::Stop
    });
    // Tastatur (vor den Kindern, sonst schlucken Regler und Eingabefeld die
    // Tasten): Esc bricht ab. Eingabetaste übernimmt – im Eingabefeld den
    // getippten Wert „#RRGGBB“, auf Regler und Farbfeld die gezeigte Farbe;
    // auf einem Knopf löst sie wie gewohnt diesen Knopf aus.
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let window_weak = window.downgrade();
    keys.connect_key_pressed(move |_, key, _, _| {
        if key == gdk::Key::Escape {
            finish(None);
            return glib::Propagation::Stop;
        }
        if !matches!(key, gdk::Key::Return | gdk::Key::KP_Enter) {
            return glib::Propagation::Proceed;
        }
        let focus = window_weak.upgrade().and_then(|window| GtkWindowExt::focus(&window));
        match focus {
            Some(widget) if widget.is::<gtk::Button>() => glib::Propagation::Proceed,
            Some(widget) if widget.is::<gtk::Text>() => {
                let typed = widget
                    .downcast::<gtk::Text>()
                    .map(|text| text.text().trim().to_owned())
                    .unwrap_or_default();
                let typed = if typed.starts_with('#') { typed } else { format!("#{typed}") };
                match Color::from_id(&typed) {
                    Some(color) => {
                        finish(Some(color));
                        glib::Propagation::Stop
                    }
                    // Kein gültiger Wert: das Eingabefeld meldet es selbst.
                    None => glib::Propagation::Proceed,
                }
            }
            _ => {
                finish(Some(chosen()));
                glib::Propagation::Stop
            }
        }
    });
    window.add_controller(keys);
    window.present();
    println!("querencia-screenshot: Farbdialog geöffnet");
}

fn width_icon(width: Width) -> gtk::DrawingArea {
    drawn_icon(20, move |cr, s| {
        cr.set_source_rgb(0.19, 0.19, 0.19);
        cr.set_line_width(width.pixels() * 0.75);
        cr.set_line_cap(cairo::LineCap::Round);
        cr.move_to(s * 0.15, s * 0.5);
        cr.line_to(s * 0.85, s * 0.5);
        cr.stroke().ok();
    })
}

/// Ersatzsymbole, falls das Symbolthema keines liefert.
#[derive(Clone, Copy)]
enum Fallback {
    Undo,
    Redo,
    Clear,
}

fn fallback_icon(kind: Fallback) -> gtk::DrawingArea {
    drawn_icon(20, move |cr, s| {
        cr.set_source_rgb(0.19, 0.19, 0.19);
        cr.set_line_width(2.0);
        cr.set_line_cap(cairo::LineCap::Round);
        match kind {
            Fallback::Undo | Fallback::Redo => {
                if matches!(kind, Fallback::Redo) {
                    cr.translate(s, 0.0);
                    cr.scale(-1.0, 1.0);
                }
                cr.arc(s * 0.55, s * 0.6, s * 0.28, std::f64::consts::PI, 0.0);
                cr.stroke().ok();
                cr.move_to(s * 0.12, s * 0.45);
                cr.line_to(s * 0.27, s * 0.75);
                cr.line_to(s * 0.42, s * 0.45);
                cr.close_path();
                cr.fill().ok();
            }
            Fallback::Clear => {
                cr.rectangle(s * 0.25, s * 0.3, s * 0.5, s * 0.6);
                cr.stroke().ok();
                cr.move_to(s * 0.15, s * 0.3);
                cr.line_to(s * 0.85, s * 0.3);
                cr.move_to(s * 0.4, s * 0.15);
                cr.line_to(s * 0.6, s * 0.15);
                cr.stroke().ok();
            }
        }
    })
}

fn action_button(
    label: Option<&str>,
    icons: &[&str],
    fallback: Option<Fallback>,
    tooltip: &str,
    action: &str,
) -> gtk::Button {
    let button = match label {
        Some(label) => ui::button(label, icons),
        None => {
            let button = gtk::Button::new();
            match (ui::theme_icon(icons), fallback) {
                (Some(image), _) => button.set_child(Some(&image)),
                (None, Some(kind)) => button.set_child(Some(&fallback_icon(kind))),
                (None, None) => button.set_label("?"),
            }
            button.add_css_class("qs-button");
            button
        }
    };
    button.set_tooltip_text(Some(tooltip));
    button.set_action_name(Some(action));
    button.set_focus_on_click(false);
    button
}

fn radio(icon: &gtk::DrawingArea, tooltip: &str, action: &str, target: &str) -> gtk::ToggleButton {
    let toggle = gtk::ToggleButton::new();
    toggle.set_child(Some(icon));
    toggle.set_tooltip_text(Some(tooltip));
    toggle.set_action_name(Some(action));
    toggle.set_action_target_value(Some(&target.to_variant()));
    toggle.set_focus_on_click(false);
    toggle.add_css_class("qs-tool");
    toggle
}

fn separator() -> gtk::Separator {
    let separator = gtk::Separator::new(gtk::Orientation::Vertical);
    separator.set_margin_start(4);
    separator.set_margin_end(4);
    separator
}

fn toolbar(editor: &Rc<Editor>) -> gtk::Box {
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    bar.add_css_class("qs-toolbar");
    bar.append(&action_button(
        Some("Neu"),
        &["applets-screenshooter", "camera-photo", "document-new"],
        None,
        "Neue Aufnahme (Strg+N)",
        "win.new",
    ));
    bar.append(&action_button(
        Some("Speichern"),
        &["document-save"],
        None,
        "Bild mit Markierungen als PNG speichern (Strg+S)",
        "win.save",
    ));
    // Kurze Beschriftung, damit die Werkzeugleiste auch auf 1024 Pixel
    // breiten Bildschirmen ganz hineinpasst; der Hinweis nennt das Ziel.
    bar.append(&action_button(
        Some("Kopieren"),
        &["edit-copy"],
        None,
        "Bild mit Markierungen in die Zwischenablage kopieren (Strg+C)",
        "win.copy",
    ));
    bar.append(&separator());
    bar.append(&action_button(
        None,
        &["edit-undo"],
        Some(Fallback::Undo),
        "Rückgängig: letzte Markierung entfernen (Strg+Z)",
        "win.undo",
    ));
    bar.append(&action_button(
        None,
        &["edit-redo"],
        Some(Fallback::Redo),
        "Wiederholen (Strg+Umschalt+Z)",
        "win.redo",
    ));
    bar.append(&action_button(
        None,
        &["edit-clear-all", "edit-clear", "edit-delete"],
        Some(Fallback::Clear),
        "Alle Markierungen entfernen – das Bildschirmfoto bleibt",
        "win.clear",
    ));
    bar.append(&separator());
    for tool in Tool::ALL {
        bar.append(&radio(
            &tool_icon(tool),
            tool.tooltip(),
            "win.tool",
            tool.id(),
        ));
    }
    bar.append(&separator());
    bar.append(&palette_button(editor));
    bar.append(&separator());
    for width in Width::ALL {
        bar.append(&radio(
            &width_icon(width),
            &format!("Strichstärke: {}", width.label()),
            "win.width",
            width.id(),
        ));
    }
    bar.append(&separator());
    let fit = gtk::ToggleButton::with_label("Einpassen");
    fit.set_tooltip_text(Some(
        "Bild ins Fenster einpassen; ausgeschaltet: Originalgröße mit Bildlaufleisten",
    ));
    fit.set_action_name(Some("win.fit"));
    fit.set_focus_on_click(false);
    fit.add_css_class("qs-tool");
    bar.append(&fit);
    bar
}

// ------------------------------------------------------------- Zeichnen

fn connect_canvas(editor: &Rc<Editor>) {
    let weak = Rc::downgrade(editor);
    editor.area.set_draw_func(move |_, cr, width, height| {
        if let Some(editor) = upgrade(&weak) {
            draw(&editor, cr, f64::from(width), f64::from(height));
        }
    });
    let weak = Rc::downgrade(editor);
    editor.area.connect_resize(move |_, _, _| {
        if let Some(editor) = upgrade(&weak) {
            editor.scaled.replace(None);
            editor.update_status();
        }
    });

    let weak = Rc::downgrade(editor);
    editor.drag.connect_drag_begin(move |gesture, x, y| {
        let Some(editor) = upgrade(&weak) else {
            return;
        };
        if editor.text_box.is_visible() {
            // Klick neben die Texteingabe: Text übernehmen, sonst nichts.
            gesture.set_state(gtk::EventSequenceState::Denied);
            finish_text(&editor, true);
            return;
        }
        editor.area.grab_focus();
        let point = editor.view().to_image(x, y, editor.image_size());
        if editor.tool.get() == Tool::Text {
            gesture.set_state(gtk::EventSequenceState::Denied);
            begin_text(&editor, point);
            return;
        }
        editor.message.replace(String::new());
        editor.drag_start.set(Some(point));
        let shape = match editor.tool.get() {
            Tool::Pen => Shape::Pen(vec![point]),
            Tool::Ellipse => Shape::Ellipse(point, point),
            Tool::Rectangle => Shape::Rectangle(point, point),
            Tool::Arrow => Shape::Arrow(point, point),
            Tool::Text => return,
        };
        editor.current.replace(Some(editor.new_mark(shape)));
        editor.area.queue_draw();
    });

    let weak = Rc::downgrade(editor);
    editor.drag.connect_drag_update(move |gesture, dx, dy| {
        let Some(editor) = upgrade(&weak) else {
            return;
        };
        let (Some(start), Some((sx, sy))) = (editor.drag_start.get(), gesture.start_point()) else {
            return;
        };
        let end = editor
            .view()
            .to_image(sx + dx, sy + dy, editor.image_size());
        let square = gesture
            .current_event_state()
            .contains(gdk::ModifierType::SHIFT_MASK);
        if let Some(mark) = editor.current.borrow_mut().as_mut() {
            match &mut mark.shape {
                Shape::Pen(points) => {
                    let far_enough = points
                        .last()
                        .is_none_or(|last| (last.x - end.x).hypot(last.y - end.y) >= PEN_STEP);
                    if far_enough {
                        points.push(end);
                    }
                }
                Shape::Ellipse(_, b) | Shape::Rectangle(_, b) => {
                    *b = constrain(start, end, square);
                }
                Shape::Arrow(_, b) => *b = end,
                Shape::Text(..) => {}
            }
        }
        editor.area.queue_draw();
    });

    let weak = Rc::downgrade(editor);
    editor.drag.connect_drag_end(move |_, _, _| {
        let Some(editor) = upgrade(&weak) else {
            return;
        };
        editor.drag_start.set(None);
        let Some(mark) = editor.current.take() else {
            return;
        };
        if editor.marks.borrow_mut().add(mark.clone()) {
            println!("querencia-screenshot: Markierung: {}", describe(&mark));
        }
        editor.refresh();
    });
    let weak = Rc::downgrade(editor);
    editor.drag.connect_cancel(move |_, _| {
        if let Some(editor) = upgrade(&weak) {
            editor.drag_start.set(None);
            editor.current.replace(None);
            editor.area.queue_draw();
        }
    });
}

/// Kurzbeschreibung einer Markierung für das Protokoll (Originalpixel).
fn describe(mark: &Mark) -> String {
    let what = match &mark.shape {
        Shape::Pen(points) => format!("Stift, {} Punkte", points.len()),
        Shape::Ellipse(a, b) => format!(
            "Ellipse {:.0},{:.0}–{:.0},{:.0}",
            a.x.min(b.x),
            a.y.min(b.y),
            a.x.max(b.x),
            a.y.max(b.y)
        ),
        Shape::Rectangle(a, b) => format!(
            "Rechteck {:.0},{:.0}–{:.0},{:.0}",
            a.x.min(b.x),
            a.y.min(b.y),
            a.x.max(b.x),
            a.y.max(b.y)
        ),
        Shape::Arrow(a, b) => format!("Pfeil {:.0},{:.0}→{:.0},{:.0}", a.x, a.y, b.x, b.y),
        Shape::Text(at, text) => format!(
            "Text bei {:.0},{:.0}: {} Zeichen",
            at.x,
            at.y,
            text.chars().count()
        ),
    };
    format!("{what} ({}, {})", mark.color.label(), mark.width.label())
}

fn draw(editor: &Editor, cr: &cairo::Context, width: f64, height: f64) {
    cr.set_source_rgb(0.55, 0.55, 0.55);
    cr.paint().ok();
    let view = View::compute(editor.fit.get(), (width, height), editor.image_size());
    let (iw, ih) = editor.image_size();
    // Schatten und Rahmen wie ein Blatt auf grauem Grund.
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.35);
    cr.rectangle(
        view.ox + 2.0,
        view.oy + 2.0,
        iw * view.scale,
        ih * view.scale,
    );
    cr.fill().ok();

    let scaled = scaled_base(editor, view.scale);
    let source = scaled.as_ref().unwrap_or(&editor.base);
    cr.save().ok();
    cr.translate(view.ox, view.oy);
    if scaled.is_none() {
        cr.scale(view.scale, view.scale);
    }
    if cr.set_source_surface(source, 0.0, 0.0).is_ok() {
        cr.paint().ok();
    }
    cr.restore().ok();

    cr.save().ok();
    cr.translate(view.ox, view.oy);
    cr.scale(view.scale, view.scale);
    cr.rectangle(0.0, 0.0, iw, ih);
    cr.clip();
    render::draw_marks(cr, editor.marks.borrow().items());
    if let Some(mark) = editor.current.borrow().as_ref() {
        render::draw_mark(cr, mark);
    }
    if let Some(at) = editor.text_at.get() {
        let text = editor.entry.text().to_string();
        let mark = editor.new_mark(Shape::Text(at, text));
        // Einfügemarke: wo der Text erscheinen wird.
        let (tw, th) = render::text_size(cr, &mark);
        cr.set_source_rgba(0.2, 0.4, 0.9, 0.9);
        cr.set_line_width(1.0 / view.scale);
        cr.set_dash(&[4.0 / view.scale, 3.0 / view.scale], 0.0);
        cr.rectangle(at.x, at.y, tw.max(8.0), th);
        cr.stroke().ok();
        cr.set_dash(&[], 0.0);
        render::draw_mark(cr, &mark);
    }
    cr.restore().ok();
}

/// Das Bild in der Anzeigegröße, einmal je Skalierung berechnet (flüssiges
/// Zeichnen auch bei großen Aufnahmen). Bei 100 % das Original.
fn scaled_base(editor: &Editor, scale: f64) -> Option<cairo::ImageSurface> {
    if (scale - 1.0).abs() < f64::EPSILON {
        return None;
    }
    if let Some((cached, surface)) = editor.scaled.borrow().as_ref()
        && (cached - scale).abs() < f64::EPSILON
    {
        return Some(surface.clone());
    }
    let (iw, ih) = editor.image_size();
    let (w, h) = (
        ((iw * scale).round() as i32).max(1),
        ((ih * scale).round() as i32).max(1),
    );
    let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, w, h).ok()?;
    {
        let cr = cairo::Context::new(&surface).ok()?;
        cr.scale(f64::from(w) / iw, f64::from(h) / ih);
        cr.set_source_surface(&editor.base, 0.0, 0.0).ok()?;
        cr.source().set_filter(cairo::Filter::Good);
        cr.paint().ok()?;
    }
    editor.scaled.replace(Some((scale, surface.clone())));
    Some(surface)
}

// ----------------------------------------------------------------- Text

fn begin_text(editor: &Rc<Editor>, at: Point) {
    println!(
        "querencia-screenshot: Texteingabe bei {:.0},{:.0}",
        at.x, at.y
    );
    editor.text_at.set(Some(at));
    editor.entry.set_text("");
    // Eingabefeld knapp unter der Einfügestelle, in Koordinaten der
    // sichtbaren Fläche (bei Originalgröße um den Bildlauf verschoben).
    let view = editor.view();
    let (x, y) = view.to_area(at);
    let line = editor.width.get().font_size() * view.scale * 1.3;
    let (dx, dy) = (
        editor.scroller.hadjustment().value(),
        editor.scroller.vadjustment().value(),
    );
    let visible_width = f64::from(editor.scroller.width());
    let left = (x - dx).clamp(0.0, (visible_width - 260.0).max(0.0));
    editor.text_box.set_margin_start(left as i32);
    editor
        .text_box
        .set_margin_top((y - dy + line + 4.0).max(0.0) as i32);
    editor.text_box.set_visible(true);
    editor.entry.grab_focus();
    editor.say("Text eingeben – Eingabetaste übernimmt, Esc verwirft");
    editor.refresh();
}

/// Beendet die Texteingabe: übernehmen (`commit`) oder verwerfen.
fn finish_text(editor: &Rc<Editor>, commit: bool) {
    let Some(at) = editor.text_at.take() else {
        return;
    };
    let text = editor.entry.text().to_string();
    editor.text_box.set_visible(false);
    editor.entry.set_text("");
    if commit {
        let mark = editor.new_mark(Shape::Text(at, text));
        if editor.marks.borrow_mut().add(mark.clone()) {
            println!("querencia-screenshot: Markierung: {}", describe(&mark));
        }
        editor.message.replace(String::new());
    } else {
        editor.say("Text verworfen");
    }
    editor.refresh();
    editor.area.grab_focus();
}

fn connect_text(editor: &Rc<Editor>) {
    let weak = Rc::downgrade(editor);
    editor.entry.connect_changed(move |_| {
        if let Some(editor) = upgrade(&weak) {
            editor.area.queue_draw();
        }
    });
    let weak = Rc::downgrade(editor);
    editor.entry.connect_activate(move |_| {
        if let Some(editor) = upgrade(&weak) {
            finish_text(&editor, true);
        }
    });
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak = Rc::downgrade(editor);
    keys.connect_key_pressed(move |_, key, _, _| {
        if key == gdk::Key::Escape
            && let Some(editor) = upgrade(&weak)
        {
            finish_text(&editor, false);
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    editor.entry.add_controller(keys);
}

// ------------------------------------------------------- Tasten, Schließen

fn connect_keys(editor: &Rc<Editor>) {
    let keys = gtk::EventControllerKey::new();
    let weak = Rc::downgrade(editor);
    keys.connect_key_pressed(move |_, key, _, _| {
        let Some(editor) = upgrade(&weak) else {
            return glib::Propagation::Proceed;
        };
        if key == gdk::Key::Escape && editor.current.borrow().is_some() {
            // Laufende Zeichnung abbrechen; nichts wird übernommen.
            editor.current.replace(None);
            editor.drag_start.set(None);
            editor.drag.set_state(gtk::EventSequenceState::Denied);
            editor.say("Abgebrochen");
            editor.area.queue_draw();
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    editor.window.add_controller(keys);
}

fn connect_close(editor: &Rc<Editor>) {
    let weak = Rc::downgrade(editor);
    editor.window.connect_close_request(move |window| {
        let Some(editor) = upgrade(&weak) else {
            return glib::Propagation::Proceed;
        };
        if editor.force_close.get() || !editor.marks.borrow().has_unsaved_marks() {
            println!("querencia-screenshot: Editor geschlossen");
            return glib::Propagation::Proceed;
        }
        let weak = Rc::downgrade(&editor);
        instant::ask(
            window,
            instant::Question {
                title: "Querencia Screenshot",
                message: "Änderungen nicht gespeichert",
                detail: "Die Markierungen wurden seit dem letzten Speichern geändert oder noch nie gespeichert. Beim Schließen gehen diese Änderungen verloren.",
                icon: &["dialog-warning"],
                buttons: &["_Abbrechen", "_Ohne Speichern schließen", "_Speichern …"],
                cancel: 0,
                default: 2,
            },
            move |answer| {
                let Some(editor) = upgrade(&weak) else {
                    return;
                };
                match answer {
                    1 => {
                        println!("querencia-screenshot: ohne Speichern geschlossen");
                        editor.force_close.set(true);
                        editor.window.close();
                    }
                    2 => {
                        editor.close_after_save.set(true);
                        save(&editor);
                    }
                    _ => editor.say("Schließen abgebrochen"),
                }
            },
        );
        glib::Propagation::Stop
    });
}

fn confirm_clear(editor: &Rc<Editor>) {
    if editor.marks.borrow().items().is_empty() {
        return;
    }
    let weak = Rc::downgrade(editor);
    instant::ask(
        &editor.window,
        instant::Question {
            title: "Querencia Screenshot",
            message: "Alle Markierungen entfernen?",
            detail: "Das Bildschirmfoto selbst bleibt erhalten. Mit Strg+Z lassen sich die Markierungen zurückholen.",
            icon: &["dialog-question", "dialog-warning"],
            buttons: &["_Abbrechen", "_Entfernen"],
            cancel: 0,
            default: 1,
        },
        move |answer| {
            let Some(editor) = upgrade(&weak) else {
                return;
            };
            if answer == 1 && editor.marks.borrow_mut().clear() {
                editor.say("Alle Markierungen entfernt");
            }
            editor.refresh();
        },
    );
}

// ------------------------------------------------- Speichern, Kopieren

fn save(editor: &Rc<Editor>) {
    let png = match editor.compose() {
        Ok(png) => png,
        Err(error) => {
            editor.say(&format!("Nicht gespeichert: {error}"));
            return;
        }
    };
    let count = editor.marks.borrow().items().len();
    let description = if count == 0 {
        editor.description.clone()
    } else {
        format!(
            "{} mit {count} Markierung{}",
            editor.description,
            if count == 1 { "" } else { "en" }
        )
    };
    let weak = Rc::downgrade(editor);
    let snapshot = editor.marks.borrow().items().to_vec();
    let Some(application) = editor.window.application() else {
        return;
    };
    instant::show_save_dialog(
        &application,
        instant::SaveRequest {
            png,
            description,
            parent: Some(editor.window.clone().upcast()),
            suggested_name: editor.suggested_name.clone(),
            suggested_folder: editor.suggested_folder.clone(),
            on_saved: Some(Box::new(move |path: &Path| {
                let Some(editor) = upgrade(&weak) else {
                    return;
                };
                // Gespeichert ist genau der Stand, aus dem das Bild erzeugt
                // wurde (der Dialog ist modal, der Stand also unverändert).
                editor.marks.borrow_mut().mark_saved_as(snapshot.clone());
                // Die Zeile „Gespeichert: …“ im Protokoll schreibt der Dialog.
                editor
                    .message
                    .replace(format!("Gespeichert: {}", path.display()));
                editor.update_status();
                if editor.close_after_save.get() {
                    editor.force_close.set(true);
                    editor.window.close();
                }
            })),
            on_closed: Some(Box::new({
                let weak = Rc::downgrade(editor);
                move || {
                    if let Some(editor) = upgrade(&weak) {
                        editor.close_after_save.set(false);
                    }
                }
            })),
        },
    );
}

fn copy(editor: &Rc<Editor>) {
    if editor.busy.get() {
        return;
    }
    let png = match editor.compose() {
        Ok(png) => png,
        Err(error) => {
            editor.say(&format!("Nicht kopiert: {error}"));
            return;
        }
    };
    let count = editor.marks.borrow().items().len();
    if !runner::SystemRunner.available(clipboard::WL_COPY) {
        match gdk::Texture::from_bytes(&glib::Bytes::from(png.bytes())) {
            Ok(texture) => {
                editor.window.clipboard().set_texture(&texture);
                editor.say(
                    "Kopiert über GTK (wl-copy fehlt) – gilt nur, solange der Editor offen ist",
                );
            }
            Err(_) => editor.say("Kopieren nicht möglich: wl-copy fehlt"),
        }
        return;
    }
    editor.busy.set(true);
    editor.refresh();
    editor.say("Kopiere in die Zwischenablage …");
    let weak = Rc::downgrade(editor);
    glib::spawn_future_local(async move {
        let outcome =
            gio::spawn_blocking(move || clipboard::copy_png(&runner::SystemRunner, &png)).await;
        let Some(editor) = upgrade(&weak) else {
            return;
        };
        editor.busy.set(false);
        match outcome {
            Ok(Ok(())) => editor.say(&format!(
                "In die Zwischenablage kopiert (image/png, {count} Markierung{})",
                if count == 1 { "" } else { "en" }
            )),
            Ok(Err(error)) => editor.say(&error.to_string()),
            Err(_) => editor.say("Kopieren wurde unerwartet beendet"),
        }
        editor.refresh();
    });
}

/// Vorschlag für einen Dateinamen beim Öffnen einer vorhandenen Datei: der
/// alte Name mit „-markiert“, damit das Original nie überschrieben wird.
pub fn marked_name(path: &Path) -> String {
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Bild".to_owned());
    let candidate = format!("{stem}-markiert.png");
    match path.parent() {
        Some(folder) if folder.is_dir() => storage::unique_path(folder, &candidate)
            .ok()
            .and_then(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or(candidate),
        _ => candidate,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_view_scales_down_centres_and_maps_back_exactly() {
        let image = (2560.0, 1600.0);
        let view = View::compute(true, (1000.0, 700.0), image);
        assert!((view.scale - (976.0 / 2560.0)).abs() < 1e-9, "{view:?}");
        let (x, y) = view.to_area(Point::new(1280.0, 800.0));
        assert!(
            (x - 500.0).abs() <= 1.0 && (y - 350.0).abs() <= 1.0,
            "Mitte bleibt Mitte"
        );
        let back = view.to_image(x, y, image);
        assert!((back.x - 1280.0).abs() < 1e-9 && (back.y - 800.0).abs() < 1e-9);
    }

    #[test]
    fn small_images_are_not_enlarged_and_points_are_clamped() {
        let image = (300.0, 200.0);
        let view = View::compute(true, (1000.0, 700.0), image);
        assert_eq!(view.scale, 1.0);
        assert_eq!((view.ox, view.oy), (350.0, 250.0));
        assert_eq!(view.to_image(0.0, 0.0, image), Point::new(0.0, 0.0));
        assert_eq!(
            view.to_image(9999.0, 9999.0, image),
            Point::new(300.0, 200.0)
        );
    }

    #[test]
    fn original_size_keeps_one_to_one_pixels() {
        let image = (2560.0, 1600.0);
        let view = View::compute(false, (1000.0, 700.0), image);
        assert_eq!((view.scale, view.ox, view.oy), (1.0, MARGIN, MARGIN));
        assert_eq!(view.to_image(112.0, 62.0, image), Point::new(100.0, 50.0));
    }

    #[test]
    fn marked_names_never_point_at_the_original() {
        let root = std::env::temp_dir().join(format!("qs0d-name-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("root");
        let original = root.join("Fehler – Größe.png");
        std::fs::write(&original, b"x").expect("file");
        assert_eq!(marked_name(&original), "Fehler – Größe-markiert.png");
        std::fs::write(root.join("Fehler – Größe-markiert.png"), b"x").expect("taken");
        assert_eq!(marked_name(&original), "Fehler – Größe-markiert-2.png");
        let _ = std::fs::remove_dir_all(&root);
    }
}
