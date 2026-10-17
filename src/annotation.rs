// SPDX-License-Identifier: MIT

//! Markierungen auf einer Aufnahme (QS-0d): Werkzeuge, Farben, Stärken und
//! die Liste der Markierungen mit Rückgängig/Wiederholen.
//!
//! Alle Koordinaten sind Pixel der **Originalaufnahme**; die Anzeige rechnet
//! nur um. Die Aufnahme selbst wird nie verändert – erst beim Speichern oder
//! Kopieren werden Bild und Markierungen zusammengeführt (`render`).

/// Kleinste Ausdehnung (Originalpixel), ab der eine Form übernommen wird;
/// ein bloßer Klick erzeugt keine leere Ellipse.
pub const MIN_EXTENT: f64 = 3.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Pen,
    Ellipse,
    Rectangle,
    Arrow,
    Text,
}

impl Tool {
    pub const ALL: [Tool; 5] = [
        Tool::Pen,
        Tool::Ellipse,
        Tool::Rectangle,
        Tool::Arrow,
        Tool::Text,
    ];

    /// Name im Aktionszustand (`win.tool`).
    pub fn id(self) -> &'static str {
        match self {
            Self::Pen => "stift",
            Self::Ellipse => "ellipse",
            Self::Rectangle => "rechteck",
            Self::Arrow => "pfeil",
            Self::Text => "text",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tool| tool.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Pen => "Stift",
            Self::Ellipse => "Kreis/Ellipse",
            Self::Rectangle => "Rechteck",
            Self::Arrow => "Pfeil",
            Self::Text => "Text",
        }
    }

    /// Einzeltaste zur Werkzeugwahl.
    pub fn key(self) -> &'static str {
        match self {
            Self::Pen => "p",
            Self::Ellipse => "e",
            Self::Rectangle => "r",
            Self::Arrow => "a",
            Self::Text => "t",
        }
    }

    pub fn tooltip(self) -> &'static str {
        match self {
            Self::Pen => "Stift (P): mit gedrückter Maustaste frei zeichnen",
            Self::Ellipse => {
                "Kreis/Ellipse (E): Maus ziehen, um etwas einzukreisen – mit Umschalt ein runder Kreis"
            }
            Self::Rectangle => "Rechteck (R): Maus ziehen – mit Umschalt ein Quadrat",
            Self::Arrow => "Pfeil (A): vom Anfang zur Spitze ziehen",
            Self::Text => {
                "Text (T): auf das Bild klicken und Text eingeben, Eingabetaste übernimmt"
            }
        }
    }
}

/// Markierungsfarbe: eine der acht Grundfarben der Palette oder eine
/// benutzerdefinierte Farbe aus dem Farbdialog (RGB, 0…1, deckend).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    r: f64,
    g: f64,
    b: f64,
}

/// Eine Grundfarbe der Palette: Farbe, Kennung (Aktionsziel), Name.
pub struct Named {
    pub color: Color,
    pub id: &'static str,
    pub label: &'static str,
}

impl Color {
    // Kräftige, gut sichtbare Töne; Rot exakt #e60000 wie bisher.
    pub const RED: Color = Color::new(0.90, 0.0, 0.0);
    pub const ORANGE: Color = Color::new(1.0, 0.50, 0.0);
    pub const YELLOW: Color = Color::new(1.0, 0.85, 0.0);
    pub const GREEN: Color = Color::new(0.0, 0.65, 0.0);
    pub const BLUE: Color = Color::new(0.0, 0.35, 0.90);
    pub const VIOLET: Color = Color::new(0.55, 0.10, 0.70);
    pub const BLACK: Color = Color::new(0.0, 0.0, 0.0);
    pub const WHITE: Color = Color::new(1.0, 1.0, 1.0);

    /// Die acht Grundfarben in Palettenreihenfolge (zwei Reihen zu vier).
    pub const PALETTE: [Named; 8] = [
        Named {
            color: Color::RED,
            id: "rot",
            label: "Rot",
        },
        Named {
            color: Color::ORANGE,
            id: "orange",
            label: "Orange",
        },
        Named {
            color: Color::YELLOW,
            id: "gelb",
            label: "Gelb",
        },
        Named {
            color: Color::GREEN,
            id: "gruen",
            label: "Grün",
        },
        Named {
            color: Color::BLUE,
            id: "blau",
            label: "Blau",
        },
        Named {
            color: Color::VIOLET,
            id: "violett",
            label: "Violett",
        },
        Named {
            color: Color::BLACK,
            id: "schwarz",
            label: "Schwarz",
        },
        Named {
            color: Color::WHITE,
            id: "weiss",
            label: "Weiß",
        },
    ];

    pub const fn new(r: f64, g: f64, b: f64) -> Self {
        Self { r, g, b }
    }

    /// Benutzerdefinierte Farbe aus 8-Bit-Werten; trifft sie eine
    /// Grundfarbe, ist es genau diese.
    pub fn from_rgb8(r: u8, g: u8, b: u8) -> Self {
        let color = Self::new(
            f64::from(r) / 255.0,
            f64::from(g) / 255.0,
            f64::from(b) / 255.0,
        );
        Self::PALETTE
            .iter()
            .find(|named| named.color.rgb8() == (r, g, b))
            .map_or(color, |named| named.color)
    }

    /// Aus Farbdialog-Werten (0…1, auch außerhalb), auf 8 Bit gerundet.
    pub fn from_unit(r: f32, g: f32, b: f32) -> Self {
        let byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
        Self::from_rgb8(byte(r), byte(g), byte(b))
    }

    pub fn rgb8(self) -> (u8, u8, u8) {
        let byte = |c: f64| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
        (byte(self.r), byte(self.g), byte(self.b))
    }

    /// Die Grundfarbe, falls es eine ist.
    pub fn named(self) -> Option<&'static Named> {
        Self::PALETTE.iter().find(|named| named.color == self)
    }

    /// „#RRGGBB“
    pub fn hex(self) -> String {
        let (r, g, b) = self.rgb8();
        format!("#{r:02X}{g:02X}{b:02X}")
    }

    /// Kennung für die Aktion `win.color`: Name der Grundfarbe oder „#RRGGBB“.
    pub fn id(self) -> String {
        self.named()
            .map_or_else(|| self.hex(), |named| named.id.to_owned())
    }

    pub fn from_id(id: &str) -> Option<Self> {
        if let Some(named) = Self::PALETTE.iter().find(|named| named.id == id) {
            return Some(named.color);
        }
        let hex = id.strip_prefix('#')?;
        if hex.len() != 6 || !hex.bytes().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let part = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
        Some(Self::from_rgb8(part(0)?, part(2)?, part(4)?))
    }

    pub fn label(self) -> String {
        self.named().map_or_else(
            || format!("eigene Farbe {}", self.hex()),
            |named| named.label.to_owned(),
        )
    }

    pub fn is_custom(self) -> bool {
        self.named().is_none()
    }

    /// RGB, 0…1.
    pub fn rgb(self) -> (f64, f64, f64) {
        (self.r, self.g, self.b)
    }

    /// Helle Farbe (Gelb, Weiß, helle eigene Farben)?
    pub fn is_light(self) -> bool {
        0.2126 * self.r + 0.7152 * self.g + 0.0722 * self.b > 0.6
    }

    /// Hintergrundfarbe für den Textrand, damit Text auf jedem Bild lesbar
    /// bleibt: hell um dunkle Farben, dunkel um helle (Gelb, Weiß).
    pub fn halo(self) -> (f64, f64, f64) {
        if self.is_light() {
            (0.0, 0.0, 0.0)
        } else {
            (1.0, 1.0, 1.0)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Width {
    Thin,
    Medium,
    Thick,
}

impl Width {
    pub const ALL: [Width; 3] = [Width::Thin, Width::Medium, Width::Thick];

    pub fn id(self) -> &'static str {
        match self {
            Self::Thin => "duenn",
            Self::Medium => "mittel",
            Self::Thick => "dick",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|width| width.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Thin => "Dünn",
            Self::Medium => "Mittel",
            Self::Thick => "Dick",
        }
    }

    /// Linienbreite in Originalpixeln.
    pub fn pixels(self) -> f64 {
        match self {
            Self::Thin => 2.0,
            Self::Medium => 4.0,
            Self::Thick => 8.0,
        }
    }

    /// Schriftgröße für Text in Originalpixeln.
    pub fn font_size(self) -> f64 {
        match self {
            Self::Thin => 16.0,
            Self::Medium => 24.0,
            Self::Thick => 36.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    fn distance(self, other: Point) -> f64 {
        (self.x - other.x).hypot(self.y - other.y)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Pen(Vec<Point>),
    /// Umschließendes Rechteck der Ellipse (zwei beliebige Ecken).
    Ellipse(Point, Point),
    Rectangle(Point, Point),
    /// Anfang und Spitze.
    Arrow(Point, Point),
    /// Linke obere Ecke und Text.
    Text(Point, String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    pub shape: Shape,
    pub color: Color,
    pub width: Width,
}

/// Endpunkt eines Ziehvorgangs; mit `square` wird das Rechteck zum Quadrat
/// (Ellipse zum Kreis), Kantenlänge = die größere Ausdehnung, Richtung wie
/// gezogen.
pub fn constrain(start: Point, end: Point, square: bool) -> Point {
    if !square {
        return end;
    }
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let side = dx.abs().max(dy.abs());
    Point::new(start.x + side.copysign(dx), start.y + side.copysign(dy))
}

impl Mark {
    /// Ist die Markierung groß genug, um übernommen zu werden?
    pub fn is_meaningful(&self) -> bool {
        match &self.shape {
            Shape::Pen(points) => !points.is_empty(),
            Shape::Ellipse(a, b) | Shape::Rectangle(a, b) => {
                (a.x - b.x).abs() >= MIN_EXTENT || (a.y - b.y).abs() >= MIN_EXTENT
            }
            Shape::Arrow(a, b) => a.distance(*b) >= MIN_EXTENT,
            Shape::Text(_, text) => !text.trim().is_empty(),
        }
    }
}

/// Pfeilspitze: drei Punkte (Spitze, linke und rechte Ecke). Die Spitze
/// wächst mit der Strichstärke, ist aber nie länger als der halbe Pfeil.
pub fn arrow_head(from: Point, to: Point, width: f64) -> [Point; 3] {
    let length = from.distance(to).max(f64::EPSILON);
    let head = (width * 5.0 + 12.0).min(length * 0.5).max(width * 2.0);
    let (ux, uy) = ((to.x - from.x) / length, (to.y - from.y) / length);
    let half = head * 0.5;
    let base = Point::new(to.x - ux * head, to.y - uy * head);
    [
        to,
        Point::new(base.x - uy * half, base.y + ux * half),
        Point::new(base.x + uy * half, base.y - ux * half),
    ]
}

#[derive(Clone, Debug, PartialEq)]
enum Op {
    Add(Mark),
    /// Alle Markierungen entfernt; gemerkt wird, was vorher da war.
    Clear(Vec<Mark>),
}

/// Die Markierungen eines Bildes mit Rückgängig/Wiederholen.
#[derive(Debug, Default)]
pub struct Marks {
    items: Vec<Mark>,
    undo: Vec<Op>,
    redo: Vec<Op>,
    /// Die Markierungen der zuletzt gespeicherten Datei (am Anfang keine:
    /// die Aufnahme selbst ist noch nirgends markiert).
    saved: Vec<Mark>,
}

impl Marks {
    pub fn items(&self) -> &[Mark] {
        &self.items
    }

    pub fn add(&mut self, mark: Mark) -> bool {
        if !mark.is_meaningful() {
            return false;
        }
        self.items.push(mark.clone());
        self.undo.push(Op::Add(mark));
        self.redo.clear();
        true
    }

    /// Entfernt alle Markierungen (rückgängig machbar). Ohne Markierungen
    /// passiert nichts.
    pub fn clear(&mut self) -> bool {
        if self.items.is_empty() {
            return false;
        }
        let previous = std::mem::take(&mut self.items);
        self.undo.push(Op::Clear(previous));
        self.redo.clear();
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        let Some(op) = self.undo.pop() else {
            return false;
        };
        match op {
            Op::Add(mark) => {
                self.items.pop();
                self.redo.push(Op::Add(mark));
            }
            Op::Clear(previous) => {
                self.items = previous.clone();
                self.redo.push(Op::Clear(previous));
            }
        }
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(op) = self.redo.pop() else {
            return false;
        };
        match op {
            Op::Add(mark) => {
                self.items.push(mark.clone());
                self.undo.push(Op::Add(mark));
            }
            Op::Clear(previous) => {
                self.items.clear();
                self.undo.push(Op::Clear(previous));
            }
        }
        true
    }

    /// Gespeichert: der aktuelle Stand ist in einer Datei (für Tests; der
    /// Editor nennt mit `mark_saved_as` den tatsächlich geschriebenen Stand).
    #[cfg(test)]
    pub fn mark_saved(&mut self) {
        self.saved = self.items.clone();
    }

    /// Gespeichert wurde `written` – der Stand, aus dem die Datei erzeugt
    /// wurde (er kann vom aktuellen abweichen, wenn sich seither etwas
    /// geändert hat).
    pub fn mark_saved_as(&mut self, written: Vec<Mark>) {
        self.saved = written;
    }

    /// Weichen die Markierungen vom zuletzt gespeicherten Stand ab? Das gilt
    /// in beide Richtungen: neue Markierungen ebenso wie entfernte, die schon
    /// in der Datei sind. Rückgängig/Wiederholen zurück auf den gespeicherten
    /// Stand ist wieder „gespeichert“. Ohne je gespeichert zu haben, zählen
    /// nur vorhandene Markierungen.
    pub fn has_unsaved_marks(&self) -> bool {
        self.items != self.saved
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ellipse(x: f64) -> Mark {
        Mark {
            shape: Shape::Ellipse(Point::new(x, 10.0), Point::new(x + 50.0, 40.0)),
            color: Color::RED,
            width: Width::Medium,
        }
    }

    #[test]
    fn ids_round_trip_and_red_medium_ellipse_exist() {
        for tool in Tool::ALL {
            assert_eq!(Tool::from_id(tool.id()), Some(tool));
        }
        for named in Color::PALETTE {
            assert_eq!(Color::from_id(&named.color.id()), Some(named.color));
            assert_eq!(named.color.id(), named.id);
        }
        for width in Width::ALL {
            assert_eq!(Width::from_id(width.id()), Some(width));
        }
        assert_eq!(Color::RED.rgb(), (0.90, 0.0, 0.0));
        assert!(Width::Thin.pixels() < Width::Medium.pixels());
        assert!(Width::Medium.pixels() < Width::Thick.pixels());
        assert_eq!(Tool::from_id("unbekannt"), None);
    }

    #[test]
    fn palette_has_eight_distinct_colors_and_custom_colors_round_trip() {
        assert_eq!(Color::PALETTE.len(), 8);
        for (i, a) in Color::PALETTE.iter().enumerate() {
            assert!(!a.color.is_custom());
            for b in &Color::PALETTE[i + 1..] {
                assert_ne!(a.color.rgb8(), b.color.rgb8());
                assert_ne!(a.id, b.id);
            }
        }
        // Rot bleibt die Standardfarbe mit genau dem bisherigen Ton.
        assert_eq!(Color::RED.hex(), "#E60000");
        assert_eq!(Color::from_id("rot"), Some(Color::RED));
        // Eigene Farbe: Kennung „#RRGGBB“ hin und zurück.
        let custom = Color::from_rgb8(0x33, 0x66, 0xcc);
        assert!(custom.is_custom());
        assert_eq!(custom.id(), "#3366CC");
        assert_eq!(Color::from_id("#3366cc"), Some(custom));
        assert_eq!(custom.label(), "eigene Farbe #3366CC");
        // Eine eigene Farbe, die genau einer Grundfarbe entspricht, ist diese.
        assert_eq!(Color::from_id("#E60000"), Some(Color::RED));
        assert_eq!(Color::from_unit(230.0 / 255.0, 0.0, 0.0), Color::RED);
        assert_eq!(Color::from_unit(2.0, -1.0, 0.5).rgb8(), (255, 0, 128));
        for bad in ["", "#", "#12345", "#1234567", "#GG0000", "3366cc", "lila"] {
            assert_eq!(Color::from_id(bad), None, "{bad}");
        }
        // Textrand: dunkel um helle Farben, hell um dunkle.
        assert_eq!(Color::YELLOW.halo(), (0.0, 0.0, 0.0));
        assert_eq!(Color::WHITE.halo(), (0.0, 0.0, 0.0));
        for dark in [
            Color::RED,
            Color::GREEN,
            Color::BLUE,
            Color::BLACK,
            Color::VIOLET,
            Color::ORANGE,
        ] {
            assert_eq!(dark.halo(), (1.0, 1.0, 1.0));
        }
    }

    #[test]
    fn undo_and_redo_step_through_several_marks() {
        let mut marks = Marks::default();
        assert!(!marks.can_undo());
        for x in [0.0, 100.0, 200.0] {
            assert!(marks.add(ellipse(x)));
        }
        assert_eq!(marks.items().len(), 3);
        assert!(marks.undo());
        assert!(marks.undo());
        assert_eq!(marks.items(), &[ellipse(0.0)]);
        assert!(marks.redo());
        assert_eq!(marks.items(), &[ellipse(0.0), ellipse(100.0)]);
        assert!(marks.undo() && marks.undo());
        assert!(marks.items().is_empty());
        assert!(!marks.undo(), "nichts mehr rückgängig zu machen");
        assert!(marks.redo() && marks.redo() && marks.redo());
        assert_eq!(marks.items().len(), 3);
        assert!(!marks.redo());
    }

    #[test]
    fn a_new_mark_discards_the_redo_history() {
        let mut marks = Marks::default();
        marks.add(ellipse(0.0));
        marks.add(ellipse(100.0));
        marks.undo();
        marks.add(ellipse(300.0));
        assert!(!marks.can_redo());
        assert_eq!(marks.items(), &[ellipse(0.0), ellipse(300.0)]);
    }

    #[test]
    fn clear_all_is_undoable_and_keeps_nothing_else() {
        let mut marks = Marks::default();
        assert!(!marks.clear(), "ohne Markierungen nichts zu entfernen");
        marks.add(ellipse(0.0));
        marks.add(ellipse(100.0));
        assert!(marks.clear());
        assert!(marks.items().is_empty());
        assert!(marks.undo());
        assert_eq!(marks.items(), &[ellipse(0.0), ellipse(100.0)]);
        assert!(marks.redo());
        assert!(marks.items().is_empty());
        assert!(marks.undo());
        assert!(marks.undo(), "danach die einzelnen Markierungen");
        assert_eq!(marks.items(), &[ellipse(0.0)]);
    }

    #[test]
    fn unsaved_state_follows_changes_after_saving() {
        let mut marks = Marks::default();
        assert!(!marks.has_unsaved_marks(), "nur das Bild: keine Abfrage");
        marks.add(ellipse(0.0));
        assert!(marks.has_unsaved_marks());
        marks.undo();
        assert!(
            !marks.has_unsaved_marks(),
            "nie gespeichert, nichts mehr da"
        );
        marks.redo();
        marks.mark_saved();
        assert!(!marks.has_unsaved_marks());
        marks.add(ellipse(100.0));
        assert!(marks.has_unsaved_marks());
        marks.undo();
        assert!(
            !marks.has_unsaved_marks(),
            "zurück auf den gespeicherten Stand"
        );
        marks.redo();
        assert!(marks.has_unsaved_marks(), "wieder anders als gespeichert");
    }

    #[test]
    fn removing_saved_marks_is_an_unsaved_change() {
        let mut marks = Marks::default();
        marks.add(ellipse(0.0));
        marks.add(ellipse(100.0));
        marks.mark_saved();
        assert!(marks.clear());
        assert!(
            marks.has_unsaved_marks(),
            "gespeicherte Markierungen entfernt"
        );
        marks.undo();
        assert!(!marks.has_unsaved_marks(), "Entfernen rückgängig gemacht");
        marks.redo();
        assert!(marks.has_unsaved_marks(), "Entfernen wiederholt");
        marks.undo();
        marks.undo();
        assert!(
            marks.has_unsaved_marks(),
            "eine gespeicherte Markierung zurückgenommen"
        );
        marks.redo();
        assert!(!marks.has_unsaved_marks());
    }

    #[test]
    fn saving_empty_after_clear_makes_the_empty_state_saved() {
        let mut marks = Marks::default();
        marks.add(ellipse(0.0));
        marks.mark_saved();
        marks.clear();
        marks.mark_saved();
        assert!(!marks.has_unsaved_marks(), "leeres Bild gespeichert");
        marks.undo();
        assert!(
            marks.has_unsaved_marks(),
            "Markierung zurückgeholt, Datei leer"
        );
    }

    #[test]
    fn saved_snapshot_is_what_was_written_not_what_is_now() {
        let mut marks = Marks::default();
        marks.add(ellipse(0.0));
        let written = marks.items().to_vec();
        marks.add(ellipse(100.0));
        marks.mark_saved_as(written);
        assert!(
            marks.has_unsaved_marks(),
            "die zweite ist nicht in der Datei"
        );
        marks.undo();
        assert!(!marks.has_unsaved_marks());
    }

    #[test]
    fn clicks_without_drag_and_empty_text_are_not_marks() {
        let mut marks = Marks::default();
        let dot = Point::new(5.0, 5.0);
        for shape in [
            Shape::Ellipse(dot, Point::new(6.0, 6.0)),
            Shape::Rectangle(dot, dot),
            Shape::Arrow(dot, Point::new(6.0, 5.0)),
            Shape::Text(dot, "   ".into()),
            Shape::Pen(Vec::new()),
        ] {
            assert!(
                !marks.add(Mark {
                    shape: shape.clone(),
                    color: Color::RED,
                    width: Width::Thin
                }),
                "{shape:?}"
            );
        }
        assert!(marks.items().is_empty() && !marks.can_undo());
        assert!(marks.add(Mark {
            shape: Shape::Pen(vec![dot]),
            color: Color::RED,
            width: Width::Thin
        }));
    }

    #[test]
    fn shift_makes_squares_in_the_dragged_direction() {
        let start = Point::new(100.0, 100.0);
        assert_eq!(
            constrain(start, Point::new(130.0, 110.0), true),
            Point::new(130.0, 130.0)
        );
        assert_eq!(
            constrain(start, Point::new(80.0, 40.0), true),
            Point::new(40.0, 40.0)
        );
        assert_eq!(
            constrain(start, Point::new(80.0, 40.0), false),
            Point::new(80.0, 40.0)
        );
    }

    #[test]
    fn arrow_head_points_at_the_end_and_is_symmetric() {
        let [tip, left, right] = arrow_head(Point::new(0.0, 0.0), Point::new(100.0, 0.0), 4.0);
        assert_eq!(tip, Point::new(100.0, 0.0));
        assert!((left.x - 68.0).abs() < 1e-9 && (right.x - 68.0).abs() < 1e-9);
        assert!((left.y + right.y).abs() < 1e-9, "symmetrisch zur Linie");
        assert!((left.y - right.y).abs() > 20.0, "breit genug");
        let [_, l, r] = arrow_head(Point::new(0.0, 0.0), Point::new(10.0, 0.0), 4.0);
        assert!(
            l.x >= 0.0 && r.x >= 0.0,
            "kurzer Pfeil: Spitze bleibt im Pfeil"
        );
    }
}
