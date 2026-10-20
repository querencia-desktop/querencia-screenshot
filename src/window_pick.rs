// SPDX-License-Identifier: MIT

//! Fensterwahl für `--window` (Alt+Druck) und „Fenster einrahmen“ (QS-024,
//! Prototyp).
//!
//! labwc 0.9.8 meldet keine Fenstergeometrie und bietet keine Aufnahme
//! einzelner Fenster an (siehe `toplevel`). Deshalb wird der ganze
//! Bildschirm mit grim eingefroren und im Bild nach den Fensterrahmen von
//! labwc gesucht: 1 px Rahmen in der Rahmenfarbe des Themes (aktiv/inaktiv),
//! oben gerundete Ecken (cornerRadius), direkt unter der Oberkante die
//! Titelleiste. Die gefundenen Rechtecke bekommt slurp auf stdin: Darüber-
//! fahren hebt ein Fenster hervor, Klick wählt es, Ziehen rahmt frei ein.
//!
//! Lieber kein Rechteck als ein falsches: Jede Hypothese muss als
//! geschlossenes Rechteck in *einer* Rahmenfarbe belegt sein. Lücken im
//! Rahmen (verdeckte Teile) sind nur erlaubt, wenn sie an einer kreuzenden
//! Rahmenlinie eines anderen Fensters enden; dazu Mindestgröße, Titelleiste
//! unter der Oberkante, stimmige Ecken und Lage innerhalb der Ausgaben.
//! Maximierte Fenster (ohne Rahmen) nur, wenn foreign-toplevel ein
//! maximiertes Fenster auf der Ausgabe meldet: Titelleiste über die ganze
//! Ausgabenbreite, unten die freie Fläche (Layer-Shell) bzw. der
//! Ausgabenrand.

use std::path::{Path, PathBuf};

use crate::geometry::Region;

/// Farbabweichung je Kanal, die noch als Rahmenfarbe gilt.
const BORDER_TOLERANCE: i32 = 2;
/// Farbabweichung je Kanal für Titelleistenfarben (Verlauf, Rundung).
const TITLE_TOLERANCE: i32 = 8;
/// Kürzeste Rahmenlinie, die als Kante zählt.
const MIN_RUN: i32 = 10;
/// Kleinstes Fenster samt Rahmen (Knöpfe der Titelleiste sind kleiner).
const MIN_WIDTH: i32 = 40;
const MIN_HEIGHT: i32 = 40;
/// So viele Zeilen unter der Oberkante müssen nach Titelleiste aussehen.
const TITLE_MIN: i32 = 16;
/// Höchstens so viele Stücke je verdecktem Fenster für slurp.
const MAX_PIECES: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// `#rrggbb` oder `#rrggbbaa` (Alpha wird ignoriert).
    pub fn parse(text: &str) -> Option<Self> {
        let hex = text.trim().strip_prefix('#')?;
        if hex.len() != 6 && hex.len() != 8 {
            return None;
        }
        let channel = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
        Some(Self(channel(0)?, channel(2)?, channel(4)?))
    }

    fn near(self, other: Self, tolerance: i32) -> bool {
        (i32::from(self.0) - i32::from(other.0)).abs() <= tolerance
            && (i32::from(self.1) - i32::from(other.1)).abs() <= tolerance
            && (i32::from(self.2) - i32::from(other.2)).abs() <= tolerance
    }

    /// Liegt die Farbe (mit Toleranz) im Quader zwischen `a` und `b`?
    fn between(self, a: Self, b: Self, tolerance: i32) -> bool {
        let inside = |v: u8, x: u8, y: u8| {
            let (low, high) = (i32::from(x.min(y)), i32::from(x.max(y)));
            (low - tolerance..=high + tolerance).contains(&i32::from(v))
        };
        inside(self.0, a.0, b.0) && inside(self.1, a.1, b.1) && inside(self.2, a.2, b.2)
    }
}

/// Was einen labwc-Fensterrahmen ausmacht (aus der themerc).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameStyle {
    pub active_border: Rgb,
    pub inactive_border: Rgb,
    /// Titelleiste: Farbe und Verlaufsende (bei „Solid“ gleich).
    pub active_title: (Rgb, Rgb),
    pub inactive_title: (Rgb, Rgb),
    pub corner_radius: i32,
    /// Woher die Werte stammen, für das Protokoll.
    pub source: String,
}

impl Default for FrameStyle {
    /// Querencia-Klassisch (Variante B), wie im isolierten labwc gemessen.
    fn default() -> Self {
        Self {
            active_border: Rgb(0x7a, 0x75, 0x70),
            inactive_border: Rgb(0xaa, 0xa6, 0xa2),
            active_title: (Rgb(0xee, 0xec, 0xe9), Rgb(0xd8, 0xd4, 0xd0)),
            inactive_title: (Rgb(0xf2, 0xf1, 0xef), Rgb(0xf2, 0xf1, 0xef)),
            corner_radius: 8,
            source: "Standardwerte Querencia-Klassisch".to_owned(),
        }
    }
}

impl FrameStyle {
    /// Übernimmt die bekannten Schlüssel einer themerc über `self`.
    /// `override_only`: themerc-override, fehlende Schlüssel lassen den
    /// bisherigen Wert (auch den Verlauf) unverändert. In einer vollständigen
    /// themerc ist die Titelleiste ohne „Gradient“ einfarbig, wie bei labwc.
    pub fn with_themerc(mut self, text: &str, override_only: bool) -> Self {
        let color = |key: &str| text_after_key(text, key).as_deref().and_then(Rgb::parse);
        let gradient = |key: &str| {
            text_after_key(text, key).map(|v| v.to_ascii_lowercase().contains("gradient"))
        };
        if let Some(c) = color("window.active.border.color") {
            self.active_border = c;
        }
        if let Some(c) = color("window.inactive.border.color") {
            self.inactive_border = c;
        }
        let title = |old: (Rgb, Rgb), prefix: &str| {
            let from = color(&format!("{prefix}.color")).unwrap_or(old.0);
            let to = color(&format!("{prefix}.colorTo"));
            match gradient(prefix) {
                Some(true) => (from, to.unwrap_or(old.1)),
                Some(false) => (from, from),
                None if override_only => (from, to.unwrap_or(old.1)),
                None => (from, from),
            }
        };
        self.active_title = title(self.active_title, "window.active.title.bg");
        self.inactive_title = title(self.inactive_title, "window.inactive.title.bg");
        self
    }

    fn title_like(&self, class: Class, color: Rgb) -> bool {
        let (a, b) = match class {
            Class::Active => self.active_title,
            _ => self.inactive_title,
        };
        color.between(a, b, TITLE_TOLERANCE)
    }
}

/// Wert eines themerc-Schlüssels (letzte Zeile gewinnt, wie bei labwc);
/// „#“ gehört hier zur Farbe, Kommentare stehen am Zeilenanfang.
fn text_after_key(text: &str, wanted: &str) -> Option<String> {
    text.lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .filter_map(|line| line.split_once(':'))
        .filter(|(key, _)| key.trim() == wanted)
        .map(|(_, value)| {
            // Ein Kommentar nach dem Wert beginnt mit „ #“ hinter der Farbe.
            let value = value.trim();
            match value.strip_prefix('#') {
                Some(rest) => format!("#{}", rest.split_whitespace().next().unwrap_or_default()),
                None => value.split_whitespace().collect::<Vec<_>>().join(" "),
            }
        })
        .next_back()
}

/// Liest die Rahmenfarben des laufenden labwc: Konfigurationsordner aus
/// `-C` des labwc-Prozesses (`LABWC_PID`), sonst `$XDG_CONFIG_HOME/labwc`;
/// Theme-Name und cornerRadius aus dessen rc.xml; themerc aus den
/// Theme-Ordnern wie labwc, dazu themerc-override. Fehlt etwas, gelten die
/// Werte von Querencia-Klassisch.
pub fn load_style() -> FrameStyle {
    let config = labwc_config_dir();
    let rc = config
        .as_ref()
        .and_then(|dir| std::fs::read_to_string(dir.join("rc.xml")).ok())
        .unwrap_or_default();
    let mut style = FrameStyle::default();
    if let Some(radius) = theme_setting(&rc, "cornerRadius").and_then(|v| v.parse().ok()) {
        style.corner_radius = radius;
    }
    let Some(name) = theme_setting(&rc, "name") else {
        style.source = format!("{} (kein Theme in rc.xml gefunden)", style.source);
        return style;
    };
    let Some(themerc) = theme_dirs()
        .into_iter()
        .map(|dir| dir.join(&name).join("labwc/themerc"))
        .find(|path| path.is_file())
    else {
        style.source = format!("{} (Theme „{name}“ nicht gefunden)", style.source);
        return style;
    };
    if let Ok(text) = std::fs::read_to_string(&themerc) {
        style = style.with_themerc(&text, false);
        style.source = themerc.display().to_string();
    }
    if let Some(dir) = config {
        let override_path = dir.join("themerc-override");
        if let Ok(text) = std::fs::read_to_string(&override_path) {
            style = style.with_themerc(&text, true);
            style.source = format!("{} + {}", style.source, override_path.display());
        }
    }
    style
}

fn labwc_config_dir() -> Option<PathBuf> {
    let from_process = std::env::var("LABWC_PID")
        .ok()
        .and_then(|pid| std::fs::read(format!("/proc/{}/cmdline", pid.trim())).ok())
        .and_then(|cmdline| {
            let args: Vec<String> = cmdline
                .split(|byte| *byte == 0)
                .map(|arg| String::from_utf8_lossy(arg).into_owned())
                .collect();
            args.iter()
                .position(|arg| arg == "-C" || arg == "--config-dir")
                .and_then(|i| args.get(i + 1))
                .map(PathBuf::from)
        });
    from_process.or_else(|| {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".config")))?;
        Some(base.join("labwc"))
    })
}

fn theme_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut dirs = Vec::new();
    match std::env::var_os("XDG_DATA_HOME") {
        Some(data) if !data.is_empty() => dirs.push(PathBuf::from(data).join("themes")),
        _ => dirs.extend(home.iter().map(|h| h.join(".local/share/themes"))),
    }
    dirs.extend(home.iter().map(|h| h.join(".themes")));
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_owned());
    dirs.extend(
        data_dirs
            .split(':')
            .filter(|d| !d.is_empty())
            .map(|d| Path::new(d).join("themes")),
    );
    dirs.push(PathBuf::from("/opt/share/themes"));
    dirs
}

/// `<theme><name>…</name></theme>` bzw. `<cornerRadius>` aus rc.xml, ohne
/// XML-Kommentare.
fn theme_setting(rc: &str, tag: &str) -> Option<String> {
    let mut text = String::with_capacity(rc.len());
    let mut rest = rc;
    while let Some(start) = rest.find("<!--") {
        text.push_str(&rest[..start]);
        rest = rest[start..]
            .find("-->")
            .map(|end| &rest[start + end + 3..])
            .unwrap_or("");
    }
    text.push_str(rest);
    let theme = text.split("<theme>").nth(1)?.split("</theme>").next()?;
    let value = theme
        .split(&format!("<{tag}>"))
        .nth(1)?
        .split(&format!("</{tag}>"))
        .next()?
        .trim();
    (!value.is_empty()).then(|| value.to_owned())
}

/// Ein RGBA-Bild (8 Bit je Kanal, ohne Vormultiplikation).
#[derive(Clone, PartialEq, Eq)]
pub struct Image {
    pub width: i32,
    pub height: i32,
    pub rgba: Vec<u8>,
}

impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Image({}×{})", self.width, self.height)
    }
}

impl Image {
    #[cfg(test)]
    pub fn filled(width: i32, height: i32, color: Rgb) -> Self {
        let mut rgba = Vec::with_capacity((width * height * 4).max(0) as usize);
        for _ in 0..width * height {
            rgba.extend_from_slice(&[color.0, color.1, color.2, 255]);
        }
        Self {
            width,
            height,
            rgba,
        }
    }

    pub fn rgb(&self, x: i32, y: i32) -> Rgb {
        let i = ((y * self.width + x) * 4) as usize;
        Rgb(self.rgba[i], self.rgba[i + 1], self.rgba[i + 2])
    }

    #[cfg(test)]
    pub fn set(&mut self, x: i32, y: i32, color: Rgb) {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return;
        }
        let i = ((y * self.width + x) * 4) as usize;
        self.rgba[i..i + 4].copy_from_slice(&[color.0, color.1, color.2, 255]);
    }

    /// Ausschnitt (wird auf das Bild begrenzt); `None`, wenn nichts übrig bleibt.
    pub fn crop(&self, area: Region) -> Option<Image> {
        let x0 = area.x.max(0);
        let y0 = area.y.max(0);
        let x1 = (area.x + area.width as i32).min(self.width);
        let y1 = (area.y + area.height as i32).min(self.height);
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        let mut rgba = Vec::with_capacity(((x1 - x0) * (y1 - y0) * 4) as usize);
        for y in y0..y1 {
            let start = ((y * self.width + x0) * 4) as usize;
            let end = ((y * self.width + x1) * 4) as usize;
            rgba.extend_from_slice(&self.rgba[start..end]);
        }
        Some(Image {
            width: x1 - x0,
            height: y1 - y0,
            rgba,
        })
    }
}

/// Eine Ausgabe in Bildkoordinaten.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    pub name: String,
    pub area: Region,
    /// Höhe der freien Fläche (ohne Leisten), falls bekannt.
    pub usable_height: Option<u32>,
    /// foreign-toplevel meldet hier ein maximiertes, nicht minimiertes Fenster.
    pub has_maximized: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Rahmen ringsum sichtbar.
    Complete,
    /// Teile des Rahmens sind von anderen Fenstern verdeckt; das Bild zeigt
    /// dort das verdeckende Fenster.
    Partial,
    /// Maximiert: ohne Rahmen, Titelleiste über die Ausgabenbreite.
    Maximized,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Found {
    pub area: Region,
    pub active: bool,
    pub kind: Kind,
}

impl Found {
    pub fn describe(&self) -> String {
        format!(
            "{},{} {}x{} {} {}",
            self.area.x,
            self.area.y,
            self.area.width,
            self.area.height,
            if self.active { "aktiv" } else { "inaktiv" },
            match self.kind {
                Kind::Complete => "vollständig",
                Kind::Partial => "teilverdeckt",
                Kind::Maximized => "maximiert",
            }
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    None = 0,
    Active = 1,
    Inactive = 2,
}

/// Rechteck mit eingeschlossenen Kanten (x0..=x1, y0..=y1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Frame {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}

impl Frame {
    fn region(self) -> Region {
        Region {
            x: self.x0,
            y: self.y0,
            width: (self.x1 - self.x0 + 1) as u32,
            height: (self.y1 - self.y0 + 1) as u32,
        }
    }
}

struct Analysis<'a> {
    image: &'a Image,
    style: &'a FrameStyle,
    classes: Vec<u8>,
}

/// Maximale Läufe einer Klasse je Zeile und je Spalte.
struct Runs {
    rows: Vec<Vec<(i32, i32)>>,
    cols: Vec<Vec<(i32, i32)>>,
}

#[derive(Default)]
struct Corners {
    tl: Vec<(i32, i32)>,
    tr: Vec<(i32, i32)>,
    bl: Vec<(i32, i32)>,
    br: Vec<(i32, i32)>,
}

impl<'a> Analysis<'a> {
    fn new(image: &'a Image, style: &'a FrameStyle) -> Self {
        let mut classes = vec![0u8; (image.width * image.height).max(0) as usize];
        for y in 0..image.height {
            for x in 0..image.width {
                let color = image.rgb(x, y);
                classes[(y * image.width + x) as usize] =
                    if color.near(style.active_border, BORDER_TOLERANCE) {
                        Class::Active as u8
                    } else if color.near(style.inactive_border, BORDER_TOLERANCE) {
                        Class::Inactive as u8
                    } else {
                        Class::None as u8
                    };
            }
        }
        Self {
            image,
            style,
            classes,
        }
    }

    fn class(&self, x: i32, y: i32) -> u8 {
        if x < 0 || y < 0 || x >= self.image.width || y >= self.image.height {
            return Class::None as u8;
        }
        self.classes[(y * self.image.width + x) as usize]
    }

    fn is(&self, x: i32, y: i32, class: Class) -> bool {
        self.class(x, y) == class as u8
    }

    fn is_border(&self, x: i32, y: i32) -> bool {
        self.class(x, y) != Class::None as u8
    }

    fn runs(&self, class: Class) -> Runs {
        let (w, h) = (self.image.width, self.image.height);
        let mut rows = vec![Vec::new(); h.max(0) as usize];
        let mut cols = vec![Vec::new(); w.max(0) as usize];
        for y in 0..h {
            let mut x = 0;
            while x < w {
                if self.is(x, y, class) {
                    let start = x;
                    while x < w && self.is(x, y, class) {
                        x += 1;
                    }
                    if x - start >= MIN_RUN {
                        rows[y as usize].push((start, x - 1));
                    }
                } else {
                    x += 1;
                }
            }
        }
        for x in 0..w {
            let mut y = 0;
            while y < h {
                if self.is(x, y, class) {
                    let start = y;
                    while y < h && self.is(x, y, class) {
                        y += 1;
                    }
                    if y - start >= MIN_RUN {
                        cols[x as usize].push((start, y - 1));
                    }
                } else {
                    y += 1;
                }
            }
        }
        Runs { rows, cols }
    }

    fn corners(&self, runs: &Runs) -> Corners {
        let r = self.style.corner_radius.max(0);
        let w = self.image.width;
        let mut corners = Corners::default();
        // Obere Ecken: gerundet, Oberkante beginnt d Pixel rechts der
        // linken Kante, die linke Kante d Pixel unter der Oberkante.
        let top_side = |column: i32, y: i32, d: i32| -> bool {
            runs.cols.get(column as usize).is_some_and(|list| {
                list.iter().any(|&(s, _)| {
                    let down = s - y;
                    (0..=r + 2).contains(&down) && (down - d).abs() <= 2
                })
            })
        };
        for (y, list) in runs.rows.iter().enumerate() {
            let y = y as i32;
            for &(a, b) in list {
                if let Some(d) = (0..=r + 2).find(|&d| a - d >= 0 && top_side(a - d, y, d)) {
                    corners.tl.push((a - d, y));
                }
                if let Some(d) = (0..=r + 2).find(|&d| b + d < w && top_side(b + d, y, d)) {
                    corners.tr.push((b + d, y));
                }
            }
        }
        // Untere Ecken: eckig, Unterkante und Seitenkante treffen sich.
        for (x, list) in runs.cols.iter().enumerate() {
            let x = x as i32;
            for &(_, e) in list {
                let Some(row) = runs.rows.get(e as usize) else {
                    continue;
                };
                if row.iter().any(|&(a, _)| a == x) {
                    corners.bl.push((x, e));
                }
                if row.iter().any(|&(_, b)| b == x) {
                    corners.br.push((x, e));
                }
            }
        }
        corners
    }

    /// Kreuzt bei `(x, y)` eine Rahmenlinie (beliebiger Farbe) quer zur
    /// Seite? `horizontal`: die Seite ist waagrecht, gesucht ist eine
    /// senkrechte Linie. Liegt dort die gerundete Ecke des verdeckenden
    /// Fensters, beginnt dessen Kante erst bis zu cornerRadius weiter; darum
    /// wird so weit gesucht.
    fn crossing(&self, x: i32, y: i32, horizontal: bool) -> bool {
        let reach = self.style.corner_radius.max(0) + 3;
        let count = (-reach..=reach)
            .filter(|&k| k != 0)
            .filter(|&k| {
                if horizontal {
                    self.is_border(x, y + k)
                } else {
                    self.is_border(x + k, y)
                }
            })
            .count();
        count >= 3
    }

    /// Prüft eine Seite: sichtbare Pixel in `class`; Lücken müssen an einer
    /// kreuzenden Rahmenlinie enden (oder am Seitenende). Liefert die Zahl
    /// sichtbarer Pixel oder `None`, wenn die Seite nicht stimmt.
    fn side(&self, class: Class, fixed: i32, from: i32, to: i32, horizontal: bool) -> Option<Side> {
        let at = |p: i32| {
            if horizontal { (p, fixed) } else { (fixed, p) }
        };
        let visible: Vec<bool> = (from..=to)
            .map(|p| {
                let (x, y) = at(p);
                self.is(x, y, class)
            })
            .collect();
        let shown = visible.iter().filter(|v| **v).count();
        let longest = visible.split(|v| !*v).map(<[bool]>::len).max().unwrap_or(0);
        if longest < 8 {
            return None;
        }
        let mut gaps = false;
        let mut i = 0usize;
        while i < visible.len() {
            if visible[i] {
                i += 1;
                continue;
            }
            let g0 = i;
            while i < visible.len() && !visible[i] {
                i += 1;
            }
            let g1 = i - 1;
            gaps = true;
            let crossing_at = |index: usize| {
                let (x, y) = at(from + index as i32);
                self.crossing(x, y, horizontal)
            };
            if g0 > 0 && !(crossing_at(g0) || crossing_at(g0 - 1)) {
                return None;
            }
            if g1 + 1 < visible.len() && !(crossing_at(g1) || crossing_at(g1 + 1)) {
                return None;
            }
        }
        Some(Side {
            visible,
            shown,
            gaps,
        })
    }

    fn validate(&self, frame: Frame, class: Class, outputs: &[Output]) -> Option<Found> {
        let Frame { x0, y0, x1, y1 } = frame;
        let r = self.style.corner_radius.max(0);
        if x0 < 0 || y0 < 0 || x1 >= self.image.width || y1 >= self.image.height {
            return None;
        }
        if x1 - x0 + 1 < MIN_WIDTH.max(2 * r + MIN_RUN)
            || y1 - y0 + 1 < MIN_HEIGHT.max(TITLE_MIN + 4)
        {
            return None;
        }
        // Innerhalb der Ausgaben: jede Ecke liegt auf einer Ausgabe.
        let on_output = |x: i32, y: i32| {
            outputs.iter().any(|o| {
                x >= o.area.x
                    && y >= o.area.y
                    && x < o.area.x + o.area.width as i32
                    && y < o.area.y + o.area.height as i32
            })
        };
        let corners_on_outputs =
            on_output(x0, y0) && on_output(x1, y0) && on_output(x0, y1) && on_output(x1, y1);
        if !outputs.is_empty() && !corners_on_outputs {
            return None;
        }
        let top = self.side(class, y0, x0 + r, x1 - r, true)?;
        let bottom = self.side(class, y1, x0, x1, true)?;
        let left = self.side(class, x0, y0 + r, y1, false)?;
        let right = self.side(class, x1, y0 + r, y1, false)?;
        // Stimmige Ecken: an jeder Ecke sind beide Seiten sichtbar oder
        // beide verdeckt.
        let first = |s: &Side| s.visible.first().copied().unwrap_or(false);
        let last = |s: &Side| s.visible.last().copied().unwrap_or(false);
        let near_last = |s: &Side| s.visible.iter().rev().nth(1).copied().unwrap_or(false);
        let near_first = |s: &Side| s.visible.get(1).copied().unwrap_or(false);
        if first(&top) != first(&left)
            || last(&top) != first(&right)
            || near_first(&bottom) != near_last(&left)
            || near_last(&bottom) != near_last(&right)
        {
            return None;
        }
        // Geschlossenes Rechteck: an sichtbaren Ecken laufen die Kanten
        // nicht darüber hinaus (sonst ist es eine Linie eines größeren
        // Fensters, z. B. Knopfumrisse unter dessen Oberkante).
        let overshoot = |x: i32, y: i32, dx: i32, dy: i32| {
            // Von (x, y) aus in Richtung (dx, dy) über die Ecke hinaus.
            let (mut px, mut py) = (x, y);
            while self.is(px, py, class) {
                px += dx;
                py += dy;
            }
            match (dx, dy) {
                (-1, 0) => px + 1 < x0,
                (1, 0) => px - 1 > x1,
                (0, -1) => py + 1 < y0,
                _ => py - 1 > y1,
            }
        };
        if (first(&top) && (overshoot(x0 + r, y0, -1, 0) || overshoot(x0, y0 + r, 0, -1)))
            || (last(&top) && (overshoot(x1 - r, y0, 1, 0) || overshoot(x1, y0 + r, 0, -1)))
            || (near_first(&bottom)
                && (overshoot(x0 + 1, y1, -1, 0) || overshoot(x0, y1 - 1, 0, 1)))
            || (near_last(&bottom) && (overshoot(x1 - 1, y1, 1, 0) || overshoot(x1, y1 - 1, 0, 1)))
        {
            return None;
        }
        // Titelleiste direkt unter den sichtbaren Teilen der Oberkante.
        let title_cells: Vec<bool> = (x0 + r..=x1 - r)
            .zip(&top.visible)
            .filter(|(_, v)| **v)
            .map(|(x, _)| self.style.title_like(class, self.image.rgb(x, y0 + 1)))
            .collect();
        let titled = title_cells.iter().filter(|t| **t).count();
        if titled < 8 || titled * 5 < title_cells.len() * 4 {
            return None;
        }
        // Titelleistenhöhe: in den Randstreifen sichtbarer oberer Ecken
        // (links/rechts vom Fenstersymbol bzw. den Knöpfen) mindestens
        // TITLE_MIN Zeilen Titelleistenfarbe.
        let column_titled = |x: i32| {
            (y0 + r + 1..=y0 + TITLE_MIN)
                .all(|y| self.style.title_like(class, self.image.rgb(x, y)))
        };
        if first(&top) && !column_titled(x0 + 3) {
            return None;
        }
        if last(&top) && !column_titled(x1 - 3) {
            return None;
        }
        let total =
            top.visible.len() + bottom.visible.len() + left.visible.len() + right.visible.len();
        let shown = top.shown + bottom.shown + left.shown + right.shown;
        // Mindestens ein Viertel des Rahmens muss zu sehen sein.
        if shown * 4 < total {
            return None;
        }
        let gaps = top.gaps || bottom.gaps || left.gaps || right.gaps;
        Some(Found {
            area: frame.region(),
            active: class == Class::Active,
            kind: if gaps { Kind::Partial } else { Kind::Complete },
        })
    }

    /// Alle belegten Fensterrahmen einer Farbe.
    fn frames(&self, class: Class, outputs: &[Output]) -> Vec<Found> {
        let runs = self.runs(class);
        let corners = self.corners(&runs);
        let mut seeds: Vec<Vec<Frame>> = Vec::new();
        // Diagonale Eckpaare legen das Rechteck fest.
        for &(x0, y0) in &corners.tl {
            for &(x1, y1) in &corners.br {
                if x1 > x0 && y1 > y0 {
                    seeds.push(vec![Frame { x0, y0, x1, y1 }]);
                }
            }
        }
        for &(x1, y0) in &corners.tr {
            for &(x0, y1) in &corners.bl {
                if x1 > x0 && y1 > y0 {
                    seeds.push(vec![Frame { x0, y0, x1, y1 }]);
                }
            }
        }
        // Benachbarte Ecken + eine Kante gegenüber: die fehlende Koordinate
        // kommt von einem Lauf, der ganz zwischen den beiden Ecken liegt.
        for &(x0, y1) in &corners.bl {
            for &(x1, _) in corners.br.iter().filter(|&&(x, y)| y == y1 && x > x0) {
                let options = (0..y1)
                    .flat_map(|y0| {
                        runs.rows[y0 as usize]
                            .iter()
                            .filter(move |&&(a, b)| a > x0 && b < x1)
                            .map(move |_| Frame { x0, y0, x1, y1 })
                    })
                    .collect();
                seeds.push(options);
            }
        }
        for &(x0, y0) in &corners.tl {
            for &(x1, _) in corners.tr.iter().filter(|&&(x, y)| y == y0 && x > x0) {
                let options = (y0 + 1..self.image.height)
                    .flat_map(|y1| {
                        runs.rows[y1 as usize]
                            .iter()
                            .filter(move |&&(a, b)| a >= x0 && b <= x1)
                            .map(move |_| Frame { x0, y0, x1, y1 })
                    })
                    .collect();
                seeds.push(options);
            }
        }
        for &(x0, y0) in &corners.tl {
            for &(_, y1) in corners.bl.iter().filter(|&&(x, y)| x == x0 && y > y0) {
                let options = (x0 + 1..self.image.width)
                    .flat_map(|x1| {
                        runs.cols[x1 as usize]
                            .iter()
                            .filter(move |&&(s, e)| s >= y0 && e <= y1)
                            .map(move |_| Frame { x0, y0, x1, y1 })
                    })
                    .collect();
                seeds.push(options);
            }
        }
        for &(x1, y0) in &corners.tr {
            for &(_, y1) in corners.br.iter().filter(|&&(x, y)| x == x1 && y > y0) {
                let options = (0..x1)
                    .flat_map(|x0| {
                        runs.cols[x0 as usize]
                            .iter()
                            .filter(move |&&(s, e)| s >= y0 && e <= y1)
                            .map(move |_| Frame { x0, y0, x1, y1 })
                    })
                    .collect();
                seeds.push(options);
            }
        }
        let mut found: Vec<Found> = Vec::new();
        for options in seeds {
            let mut valid: Vec<Found> = Vec::new();
            for frame in options {
                if let Some(hit) = self.validate(frame, class, outputs)
                    && !valid.contains(&hit)
                {
                    valid.push(hit);
                }
            }
            // Mehrdeutig (mehrere Rechtecke aus denselben Ecken): verwerfen.
            if valid.len() == 1 && !found.contains(&valid[0]) {
                found.push(valid[0]);
            }
        }
        // Zwei verschiedene Rechtecke mit drei gleichen Kanten: eines davon
        // ist falsch, aber welches? Beide verwerfen.
        let edges = |f: &Found| {
            let a = f.area;
            [
                a.x,
                a.y,
                a.x + a.width as i32 - 1,
                a.y + a.height as i32 - 1,
            ]
        };
        let doubtful: Vec<bool> = found
            .iter()
            .map(|f| {
                found.iter().any(|g| {
                    g != f && {
                        let (e, h) = (edges(f), edges(g));
                        e.iter().zip(h.iter()).filter(|(a, b)| a == b).count() >= 3
                    }
                })
            })
            .collect();
        found
            .into_iter()
            .zip(doubtful)
            .filter(|(_, doubt)| !doubt)
            .map(|(f, _)| f)
            .collect()
    }

    /// Maximiertes Fenster auf `output` (ohne Rahmen, eckig): die Zeile, an
    /// der die Titelleiste beginnt – sie und die Zeile darunter haben über
    /// (fast) die ganze Ausgabenbreite Titelleistenfarbe, die Zeile darüber
    /// nicht (oder es ist der Ausgabenrand). Ist die freie Fläche bekannt,
    /// liegt der Anfang zwischen Ausgabenoberkante und Leistenhöhe; gesucht
    /// wird dann von unten (eine helle Leiste oben wird übersprungen).
    fn maximized(&self, output: &Output) -> Option<Found> {
        let area = output.area;
        let (ox, oy, ow, oh) = (area.x, area.y, area.width as i32, area.height as i32);
        if ox < 0 || oy < 0 || ox + ow > self.image.width || oy + oh > self.image.height {
            return None;
        }
        let title_like = |color: Rgb| {
            self.style.title_like(Class::Active, color)
                || self.style.title_like(Class::Inactive, color)
        };
        let row_title = |y: i32| {
            y >= oy
                && y < oy + oh
                && (ox..ox + ow)
                    .filter(|&x| title_like(self.image.rgb(x, y)))
                    .count() as i32
                    * 100
                    >= ow * 95
        };
        let starts_here = |y: i32| {
            row_title(y)
                && row_title(y + 1)
                && (y == oy || !row_title(y - 1))
                && (y..y + TITLE_MIN)
                    .all(|row| row < oy + oh && title_like(self.image.rgb(ox + 3, row)))
        };
        let start = match output.usable_height {
            Some(usable) if (usable as i32) < oh => (oy..=oy + oh - usable as i32)
                .rev()
                .find(|&y| starts_here(y)),
            _ => (oy..oy + oh / 2).find(|&y| starts_here(y)),
        }?;
        let height = output
            .usable_height
            .map(|h| (h as i32).min(oy + oh - start))
            .unwrap_or(oy + oh - start);
        if height < MIN_HEIGHT {
            return None;
        }
        // Aktiv oder nicht: welche Titelfarbe der Anfangszeile näher liegt.
        let color = self.image.rgb(ox + 3, start);
        let distance = |c: Rgb| {
            (i32::from(c.0) - i32::from(color.0)).abs()
                + (i32::from(c.1) - i32::from(color.1)).abs()
                + (i32::from(c.2) - i32::from(color.2)).abs()
        };
        let active = distance(self.style.active_title.0) < distance(self.style.inactive_title.0);
        Some(Found {
            area: Region {
                x: ox,
                y: start,
                width: ow as u32,
                height: height as u32,
            },
            active,
            kind: Kind::Maximized,
        })
    }
}

#[derive(Debug)]
struct Side {
    visible: Vec<bool>,
    shown: usize,
    gaps: bool,
}

/// Sucht labwc-Fensterrahmen in `image` (Bildkoordinaten).
pub fn detect(image: &Image, style: &FrameStyle, outputs: &[Output]) -> Vec<Found> {
    if image.width <= 0
        || image.height <= 0
        || image.rgba.len() < (image.width * image.height * 4) as usize
    {
        return Vec::new();
    }
    let analysis = Analysis::new(image, style);
    let mut found = analysis.frames(Class::Active, outputs);
    found.extend(analysis.frames(Class::Inactive, outputs));
    // Ein aktives Fenster gibt es höchstens einmal; mehrere aktive Rahmen
    // (etwa Bilder von Fenstern im Fenster) bleiben, der Benutzer wählt.
    for output in outputs.iter().filter(|o| o.has_maximized) {
        if let Some(hit) = analysis.maximized(output) {
            found.push(hit);
        }
    }
    found.sort_by_key(|f| (f.area.y, f.area.x));
    found
}

/// Wie viele Pixel der Rahmenkante von `f` innerhalb von `other` sichtbar
/// sind (in der Rahmenfarbe von `f`).
fn border_inside(analysis: &Analysis, f: &Found, other: &Region) -> usize {
    if f.kind == Kind::Maximized {
        return 0;
    }
    let class = if f.active {
        Class::Active
    } else {
        Class::Inactive
    };
    let a = f.area;
    let (x0, y0) = (a.x, a.y);
    let (x1, y1) = (a.x + a.width as i32 - 1, a.y + a.height as i32 - 1);
    let inside = |x: i32, y: i32| {
        x > other.x
            && y > other.y
            && x < other.x + other.width as i32 - 1
            && y < other.y + other.height as i32 - 1
    };
    let mut count = 0;
    for x in x0..=x1 {
        for y in [y0, y1] {
            if inside(x, y) && analysis.is(x, y, class) {
                count += 1;
            }
        }
    }
    for y in y0..=y1 {
        for x in [x0, x1] {
            if inside(x, y) && analysis.is(x, y, class) {
                count += 1;
            }
        }
    }
    count
}

fn intersects(a: &Region, b: &Region) -> bool {
    a.x < b.x + b.width as i32
        && b.x < a.x + a.width as i32
        && a.y < b.y + b.height as i32
        && b.y < a.y + a.height as i32
}

fn area(r: &Region) -> u64 {
    u64::from(r.width) * u64::from(r.height)
}

/// `a` ohne `b`: bis zu vier Rechtecke.
fn subtract(a: Region, b: &Region) -> Vec<Region> {
    if !intersects(&a, b) {
        return vec![a];
    }
    let (ax1, ay1) = (a.x + a.width as i32, a.y + a.height as i32);
    let (bx1, by1) = (b.x + b.width as i32, b.y + b.height as i32);
    let mut pieces = Vec::new();
    let mut push = |x: i32, y: i32, x1: i32, y1: i32| {
        if x1 > x && y1 > y {
            pieces.push(Region {
                x,
                y,
                width: (x1 - x) as u32,
                height: (y1 - y) as u32,
            });
        }
    };
    push(a.x, a.y, ax1, b.y.max(a.y)); // oben
    push(a.x, by1.min(ay1), ax1, ay1); // unten
    let (my0, my1) = (b.y.max(a.y), by1.min(ay1));
    push(a.x, my0, b.x.max(a.x), my1); // links
    push(bx1.min(ax1), my0, ax1, my1); // rechts
    pieces
}

/// Die Rechtecke für slurp (Bildkoordinaten) mit dem Index des Fensters.
///
/// slurp hebt beim Darüberfahren das *kleinste* Rechteck unter dem Zeiger
/// hervor, ohne Stapelreihenfolge. Liegt ein Fenster hinter einem gleich
/// großen oder größeren Fenster, bekäme es sonst den Zeiger auch dort, wo
/// das vordere zu sehen ist. Solche Fenster werden deshalb nur mit ihrem
/// unverdeckten Teil (in Stücken) angeboten; gewählt wird trotzdem das
/// ganze Fenster. Vorn ist, wessen Rahmen in der Überschneidung zu sehen ist.
pub fn slurp_boxes(image: &Image, style: &FrameStyle, found: &[Found]) -> Vec<(Region, usize)> {
    let analysis = Analysis::new(image, style);
    let mut boxes = Vec::new();
    for (index, window) in found.iter().enumerate() {
        let in_front: Vec<&Found> = found
            .iter()
            .enumerate()
            .filter(|(other, f)| {
                *other != index
                    && intersects(&f.area, &window.area)
                    && border_inside(&analysis, f, &window.area)
                        > border_inside(&analysis, window, &f.area)
            })
            .map(|(_, f)| f)
            .collect();
        if in_front.iter().all(|f| area(&f.area) < area(&window.area)) {
            boxes.push((window.area, index));
            continue;
        }
        let mut pieces = vec![window.area];
        for front in in_front {
            pieces = pieces
                .into_iter()
                .flat_map(|piece| subtract(piece, &front.area))
                .filter(|piece| piece.width >= 4 && piece.height >= 4)
                .collect();
        }
        pieces.truncate(MAX_PIECES);
        boxes.extend(pieces.into_iter().map(|piece| (piece, index)));
    }
    boxes
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub const BACKGROUND: Rgb = Rgb(0, 0, 0);
    const CONTENT: Rgb = Rgb(0x24, 0x24, 0x24);

    fn style() -> FrameStyle {
        FrameStyle::default()
    }

    /// Zeichnet ein labwc-Fenster wie im isolierten labwc gemessen:
    /// Rahmen 1 px, obere Ecken mit Radius 8 (Kantenpixel geglättet, also
    /// nicht in Rahmenfarbe), Titelleiste 26 px (aktiv mit Verlauf),
    /// Knöpfe rechts mit Umriss in Rahmenfarbe, darunter der Inhalt.
    pub fn window(image: &mut Image, x0: i32, y0: i32, width: i32, height: i32, active: bool) {
        let s = style();
        let border = if active {
            s.active_border
        } else {
            s.inactive_border
        };
        let (x1, y1) = (x0 + width - 1, y0 + height - 1);
        let mix = |a: Rgb, b: Rgb, t: f32| {
            let m = |p: u8, q: u8| (f32::from(p) * (1.0 - t) + f32::from(q) * t).round() as u8;
            Rgb(m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
        };
        for y in y0..=y1 {
            for x in x0..=x1 {
                let row = y - y0;
                let color = if row == 0 || y == y1 || x == x0 || x == x1 {
                    border
                } else if row <= 26 {
                    let (from, to) = if active {
                        s.active_title
                    } else {
                        s.inactive_title
                    };
                    mix(from, to, (row - 1) as f32 / 25.0)
                } else {
                    CONTENT
                };
                image.set(x, y, color);
            }
        }
        // Gerundete obere Ecken: außerhalb des Viertelkreises Hintergrund,
        // auf dem Bogen eine Mischfarbe.
        for dy in 0..8 {
            for dx in 0..8 {
                let (cx, cy) = (7.5 - dx as f32, 7.5 - dy as f32);
                let distance = (cx * cx + cy * cy).sqrt();
                let color = if distance > 8.0 {
                    Some(BACKGROUND)
                } else if distance > 6.5 {
                    Some(mix(border, BACKGROUND, 0.4))
                } else {
                    None
                };
                if let Some(color) = color {
                    image.set(x0 + dx, y0 + dy, color);
                    image.set(x1 - dx, y0 + dy, color);
                }
            }
        }
        // Drei Knöpfe 24×22 mit Umriss in Rahmenfarbe.
        for k in 0..3 {
            let bx = x1 - 9 - 24 - k * 26;
            for i in 0..24 {
                image.set(bx + i, y0 + 3, border);
                image.set(bx + i, y0 + 24, border);
            }
            for j in 0..22 {
                image.set(bx, y0 + 3 + j, border);
                image.set(bx + 23, y0 + 3 + j, border);
            }
        }
    }

    pub fn output(name: &str, x: i32, y: i32, width: u32, height: u32) -> Output {
        Output {
            name: name.to_owned(),
            area: Region {
                x,
                y,
                width,
                height,
            },
            usable_height: None,
            has_maximized: false,
        }
    }

    fn region(x: i32, y: i32, width: u32, height: u32) -> Region {
        Region {
            x,
            y,
            width,
            height,
        }
    }

    fn areas(found: &[Found]) -> Vec<(Region, bool, Kind)> {
        found.iter().map(|f| (f.area, f.active, f.kind)).collect()
    }

    fn screen() -> (Image, Vec<Output>) {
        (
            Image::filled(1280, 720, BACKGROUND),
            vec![output("HEADLESS-1", 0, 0, 1280, 720)],
        )
    }

    #[test]
    fn a_single_window_is_found_exactly() {
        let (mut image, outputs) = screen();
        window(&mut image, 100, 80, 500, 366, true);
        assert_eq!(
            areas(&detect(&image, &style(), &outputs)),
            vec![(region(100, 80, 500, 366), true, Kind::Complete)]
        );
    }

    #[test]
    fn two_windows_side_by_side_and_their_activity() {
        let (mut image, outputs) = screen();
        window(&mut image, 40, 60, 500, 330, false);
        window(&mut image, 640, 100, 520, 400, true);
        assert_eq!(
            areas(&detect(&image, &style(), &outputs)),
            vec![
                (region(40, 60, 500, 330), false, Kind::Complete),
                (region(640, 100, 520, 400), true, Kind::Complete),
            ]
        );
    }

    #[test]
    fn overlapping_windows_front_complete_back_partial() {
        let (mut image, outputs) = screen();
        // Hinten (inaktiv), vorn (aktiv) überdeckt die linke obere Ecke.
        window(&mut image, 400, 250, 602, 405, false);
        window(&mut image, 100, 80, 500, 366, true);
        let found = detect(&image, &style(), &outputs);
        assert_eq!(
            areas(&found),
            vec![
                (region(100, 80, 500, 366), true, Kind::Complete),
                (region(400, 250, 602, 405), false, Kind::Partial),
            ]
        );
        // slurp: das vordere Fenster ganz, das hintere ist größer und
        // bleibt deshalb ebenfalls ein Rechteck (slurp nimmt das kleinste).
        let boxes = slurp_boxes(&image, &style(), &found);
        assert_eq!(
            boxes,
            vec![
                (region(100, 80, 500, 366), 0),
                (region(400, 250, 602, 405), 1)
            ]
        );
    }

    #[test]
    fn a_smaller_window_behind_is_offered_only_where_it_is_visible() {
        let (mut image, outputs) = screen();
        window(&mut image, 500, 300, 400, 300, false);
        window(&mut image, 100, 80, 600, 400, true);
        let found = detect(&image, &style(), &outputs);
        assert_eq!(found.len(), 2, "{found:?}");
        let back = found
            .iter()
            .position(|f| f.area == region(500, 300, 400, 300))
            .expect("hinteres Fenster");
        assert_eq!(found[back].kind, Kind::Partial);
        let boxes = slurp_boxes(&image, &style(), &found);
        let front_area = region(100, 80, 600, 400);
        for (piece, index) in boxes.iter().filter(|(_, i)| *i == back) {
            assert!(!intersects(piece, &front_area), "{piece:?} von {index}");
        }
        let covered: u64 = boxes
            .iter()
            .filter(|(_, i)| *i == back)
            .map(|(p, _)| area(p))
            .sum();
        assert_eq!(
            covered,
            400 * 300 - 200 * 180,
            "genau der sichtbare Teil des hinteren Fensters"
        );
    }

    #[test]
    fn windows_of_the_same_colour_are_not_mixed_up() {
        // Nachbau des Fehlschlags des Python-Prototyps: „Fenster Zwei“ links
        // von Eins, rechts von Vier verdeckt; Vier hat dieselbe Rahmenfarbe.
        let (mut image, outputs) = screen();
        window(&mut image, 400, 250, 602, 405, false); // Zwei
        window(&mut image, 700, 40, 402, 328, false); // Vier
        window(&mut image, 100, 80, 500, 366, true); // Eins
        let found = detect(&image, &style(), &outputs);
        for f in &found {
            assert!(
                f.area.width >= MIN_WIDTH as u32 && f.area.height >= MIN_HEIGHT as u32,
                "{f:?}"
            );
        }
        let expected = [
            region(100, 80, 500, 366),
            region(700, 40, 402, 328),
            region(400, 250, 602, 405),
        ];
        for f in &found {
            assert!(expected.contains(&f.area), "falsches Rechteck {f:?}");
        }
        assert!(found.iter().any(|f| f.area == expected[0]));
        assert!(found.iter().any(|f| f.area == expected[1]));
    }

    #[test]
    fn a_window_without_labwc_border_is_not_guessed() {
        // CSD-artig: eigene Titelleiste ohne labwc-Rahmen, runde Ecken.
        let (mut image, outputs) = screen();
        for y in 100..400 {
            for x in 200..700 {
                image.set(
                    x,
                    y,
                    if y < 146 {
                        Rgb(0xeb, 0xeb, 0xeb)
                    } else {
                        Rgb(0xfa, 0xfa, 0xfa)
                    },
                );
            }
        }
        assert!(detect(&image, &style(), &outputs).is_empty());
    }

    #[test]
    fn a_window_at_the_output_edge_is_found() {
        let (mut image, outputs) = screen();
        window(&mut image, 0, 0, 640, 360, true);
        window(&mut image, 780, 400, 500, 320, false);
        assert_eq!(
            areas(&detect(&image, &style(), &outputs)),
            vec![
                (region(0, 0, 640, 360), true, Kind::Complete),
                (region(780, 400, 500, 320), false, Kind::Complete),
            ]
        );
    }

    #[test]
    fn a_window_partly_off_screen_is_dropped() {
        let (mut image, outputs) = screen();
        // Rechte Kante und Unterkante liegen außerhalb des Bildes.
        window(&mut image, 1000, 500, 600, 400, true);
        assert!(detect(&image, &style(), &outputs).is_empty());
    }

    #[test]
    fn two_outputs_and_a_window_across_the_boundary() {
        let mut image = Image::filled(2560, 720, BACKGROUND);
        let outputs = vec![
            output("HEADLESS-2", 0, 0, 1280, 720),
            output("HEADLESS-1", 1280, 0, 1280, 720),
        ];
        window(&mut image, 1400, 100, 500, 327, false);
        window(&mut image, 1000, 420, 602, 275, true);
        assert_eq!(
            areas(&detect(&image, &style(), &outputs)),
            vec![
                (region(1400, 100, 500, 327), false, Kind::Partial),
                (region(1000, 420, 602, 275), true, Kind::Complete),
            ]
        );
    }

    #[test]
    fn a_window_in_a_gap_between_outputs_is_refused() {
        // Zwei Ausgaben unterschiedlicher Höhe: unten rechts ist keine Ausgabe.
        let mut image = Image::filled(2560, 1080, BACKGROUND);
        let outputs = vec![
            output("A", 0, 0, 1280, 1080),
            output("B", 1280, 0, 1280, 720),
        ];
        window(&mut image, 1500, 600, 400, 300, true);
        assert!(detect(&image, &style(), &outputs).is_empty());
    }

    #[test]
    fn maximized_window_needs_the_toplevel_hint_and_uses_the_free_area() {
        let (mut image, mut outputs) = screen();
        // Leiste oben (30 px), darunter maximiertes Fenster ohne Rahmen.
        let s = style();
        for y in 0..30 {
            for x in 0..1280 {
                image.set(x, y, Rgb(0x30, 0x30, 0x60));
            }
        }
        for y in 30..720 {
            for x in 0..1280 {
                image.set(x, y, if y < 56 { s.inactive_title.0 } else { CONTENT });
            }
        }
        assert!(
            detect(&image, &s, &outputs).is_empty(),
            "ohne Meldung von foreign-toplevel kein Raten"
        );
        outputs[0].has_maximized = true;
        assert_eq!(
            areas(&detect(&image, &s, &outputs)),
            vec![(region(0, 30, 1280, 690), false, Kind::Maximized)]
        );
        outputs[0].usable_height = Some(660);
        assert_eq!(
            detect(&image, &s, &outputs)[0].area,
            region(0, 30, 1280, 660),
            "Leiste auch unten: freie Fläche"
        );
        // Ein normales Fenster vor dem maximierten wird zusätzlich gefunden
        // und liegt für slurp vorn (kleiner).
        window(&mut image, 300, 200, 400, 300, true);
        let found = detect(&image, &s, &outputs);
        assert_eq!(found.len(), 2, "{found:?}");
        let boxes = slurp_boxes(&image, &s, &found);
        assert_eq!(boxes.len(), 2, "beide ganz: {boxes:?}");
    }

    #[test]
    fn themerc_colours_override_the_defaults() {
        let text = "# Kommentar\nwindow.active.border.color: #112233\n\
                    window.inactive.border.color: #445566 # Notiz\n\
                    window.active.title.bg: Solid\n\
                    window.active.title.bg.color: #aabbcc\n\
                    window.active.title.bg.colorTo: #000000\n";
        let style = FrameStyle::default().with_themerc(text, false);
        assert_eq!(style.active_border, Rgb(0x11, 0x22, 0x33));
        assert_eq!(style.inactive_border, Rgb(0x44, 0x55, 0x66));
        assert_eq!(
            style.active_title,
            (Rgb(0xaa, 0xbb, 0xcc), Rgb(0xaa, 0xbb, 0xcc)),
            "Solid: kein Verlauf"
        );
        assert_eq!(style.inactive_title, FrameStyle::default().inactive_title);
        assert_eq!(Rgb::parse("#7a7570ff"), Some(Rgb(0x7a, 0x75, 0x70)));
        assert_eq!(Rgb::parse("7a7570"), None);
    }

    #[test]
    fn rc_xml_theme_name_ignores_comments() {
        let rc = "<labwc_config><!-- <theme><name>Alt</name></theme> -->\
                  <theme>\n  <name>Querencia-Klassisch</name><cornerRadius>6</cornerRadius>\n</theme></labwc_config>";
        assert_eq!(
            theme_setting(rc, "name").as_deref(),
            Some("Querencia-Klassisch")
        );
        assert_eq!(theme_setting(rc, "cornerRadius").as_deref(), Some("6"));
        assert_eq!(theme_setting("<labwc_config/>", "name"), None);
    }

    #[test]
    fn crop_is_clipped_to_the_image() {
        let mut image = Image::filled(10, 10, BACKGROUND);
        image.set(3, 4, Rgb(1, 2, 3));
        let part = image.crop(region(3, 4, 20, 2)).expect("crop");
        assert_eq!((part.width, part.height), (7, 2));
        assert_eq!(part.rgb(0, 0), Rgb(1, 2, 3));
        assert!(image.crop(region(20, 20, 5, 5)).is_none());
    }
}
