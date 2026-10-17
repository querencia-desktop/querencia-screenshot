// SPDX-License-Identifier: MIT

//! Zeichnen der Markierungen mit cairo – dieselbe Funktion für die Anzeige
//! im Editor und für das fertige Bild. `compose` führt Originalaufnahme und
//! Markierungen in Originalgröße zu einem neuen PNG zusammen; die
//! Originaldaten bleiben dabei unverändert.

use std::fmt;

use gtk::{cairo, gdk, glib, prelude::*};

use crate::{
    annotation::{Mark, Shape, arrow_head},
    screenshot::{Png, PngError},
};

const FONT_FAMILY: &str = "Sans";

#[derive(Debug)]
pub enum RenderError {
    Decode(String),
    Cairo(String),
    Image(PngError),
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Decode(error) => write!(f, "Bild kann nicht gelesen werden: {error}"),
            Self::Cairo(error) => write!(f, "Zeichnen fehlgeschlagen: {error}"),
            Self::Image(error) => write!(f, "{error}"),
        }
    }
}

impl From<cairo::Error> for RenderError {
    fn from(error: cairo::Error) -> Self {
        Self::Cairo(error.to_string())
    }
}

/// Die Originalaufnahme als cairo-Fläche (ARGB32, vormultipliert – das ist
/// GDKs Standardformat beim Herunterladen, Byte-Reihenfolge BGRA).
pub fn surface_from_png(png: &Png) -> Result<cairo::ImageSurface, RenderError> {
    let texture = gdk::Texture::from_bytes(&glib::Bytes::from(png.bytes()))
        .map_err(|error| RenderError::Decode(error.to_string()))?;
    surface_from_texture(&texture)
}

pub fn surface_from_texture(texture: &gdk::Texture) -> Result<cairo::ImageSurface, RenderError> {
    let (width, height) = (texture.width(), texture.height());
    let stride = cairo::Format::ARgb32
        .stride_for_width(width as u32)
        .map_err(RenderError::from)?;
    let mut data = vec![0u8; stride as usize * height as usize];
    texture.download(&mut data, stride as usize);
    cairo::ImageSurface::create_for_data(data, cairo::Format::ARgb32, width, height, stride)
        .map_err(RenderError::from)
}

/// Zeichnet `marks` in Originalkoordinaten in `cr` (die Anzeige setzt vorher
/// Skalierung und Verschiebung).
pub fn draw_marks(cr: &cairo::Context, marks: &[Mark]) {
    for mark in marks {
        draw_mark(cr, mark);
    }
}

pub fn draw_mark(cr: &cairo::Context, mark: &Mark) {
    let (r, g, b) = mark.color.rgb();
    let width = mark.width.pixels();
    cr.save().ok();
    cr.set_source_rgb(r, g, b);
    cr.set_line_width(width);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);
    match &mark.shape {
        Shape::Pen(points) => {
            if let [only] = points.as_slice() {
                // Ein einzelner Punkt: runder Tupfer in Strichstärke.
                cr.arc(only.x, only.y, width / 2.0, 0.0, std::f64::consts::TAU);
                cr.fill().ok();
            } else if let Some(first) = points.first() {
                cr.move_to(first.x, first.y);
                for point in &points[1..] {
                    cr.line_to(point.x, point.y);
                }
                cr.stroke().ok();
            }
        }
        Shape::Ellipse(a, b) => {
            let (cx, cy) = ((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
            let (rx, ry) = (
                ((a.x - b.x) / 2.0).abs().max(0.5),
                ((a.y - b.y) / 2.0).abs().max(0.5),
            );
            // Pfad im skalierten Raum, Strich danach ungestreckt: gleich
            // breite Linie rundherum, nur Umrandung (keine Füllung).
            cr.save().ok();
            cr.translate(cx, cy);
            cr.scale(rx, ry);
            cr.new_path();
            cr.arc(0.0, 0.0, 1.0, 0.0, std::f64::consts::TAU);
            cr.restore().ok();
            cr.stroke().ok();
        }
        Shape::Rectangle(a, b) => {
            cr.set_line_join(cairo::LineJoin::Miter);
            cr.rectangle(
                a.x.min(b.x),
                a.y.min(b.y),
                (a.x - b.x).abs(),
                (a.y - b.y).abs(),
            );
            cr.stroke().ok();
        }
        Shape::Arrow(from, to) => {
            let [tip, left, right] = arrow_head(*from, *to, width);
            // Linie nur bis zur Mitte der Spitze, damit das runde Linienende
            // nicht über die Spitze hinausragt.
            let base = ((left.x + right.x) / 2.0, (left.y + right.y) / 2.0);
            cr.move_to(from.x, from.y);
            cr.line_to(base.0, base.1);
            cr.stroke().ok();
            cr.move_to(tip.x, tip.y);
            cr.line_to(left.x, left.y);
            cr.line_to(right.x, right.y);
            cr.close_path();
            cr.fill().ok();
        }
        Shape::Text(at, text) => {
            cr.select_font_face(
                FONT_FAMILY,
                cairo::FontSlant::Normal,
                cairo::FontWeight::Bold,
            );
            cr.set_font_size(mark.width.font_size());
            let ascent = cr
                .font_extents()
                .map(|extents| extents.ascent())
                .unwrap_or(0.0);
            let line_height = mark.width.font_size() * 1.25;
            for (index, line) in text.lines().enumerate() {
                cr.new_path();
                cr.move_to(at.x, at.y + ascent + line_height * index as f64);
                cr.text_path(line);
                // Schmaler Rand in Kontrastfarbe, dann die Schrift selbst.
                let (hr, hg, hb) = mark.color.halo();
                cr.set_source_rgb(hr, hg, hb);
                cr.set_line_width((mark.width.font_size() / 8.0).max(2.0));
                cr.stroke_preserve().ok();
                cr.set_source_rgb(r, g, b);
                cr.fill().ok();
            }
        }
    }
    cr.restore().ok();
}

/// Ungefähre Ausdehnung eines Textes (Breite, Höhe) in Originalpixeln – für
/// die Vorschau der Texteingabe.
pub fn text_size(cr: &cairo::Context, mark: &Mark) -> (f64, f64) {
    let Shape::Text(_, text) = &mark.shape else {
        return (0.0, 0.0);
    };
    cr.save().ok();
    cr.select_font_face(
        FONT_FAMILY,
        cairo::FontSlant::Normal,
        cairo::FontWeight::Bold,
    );
    cr.set_font_size(mark.width.font_size());
    let width = text
        .lines()
        .filter_map(|line| cr.text_extents(line).ok())
        .map(|extents| extents.x_advance())
        .fold(0.0, f64::max);
    cr.restore().ok();
    let lines = text.lines().count().max(1) as f64;
    (width, mark.width.font_size() * 1.25 * lines)
}

/// Originalaufnahme + Markierungen → neues PNG in Originalgröße.
pub fn compose(original: &Png, marks: &[Mark]) -> Result<Png, RenderError> {
    let base = surface_from_png(original)?;
    let (width, height) = (base.width(), base.height());
    let target = cairo::ImageSurface::create(cairo::Format::ARgb32, width, height)?;
    {
        let cr = cairo::Context::new(&target)?;
        cr.set_source_surface(&base, 0.0, 0.0)?;
        cr.set_operator(cairo::Operator::Source);
        cr.paint()?;
        cr.set_operator(cairo::Operator::Over);
        draw_marks(&cr, marks);
        cr.status()?;
    }
    target.flush();
    let stride = target.stride() as usize;
    let data = target
        .take_data()
        .map_err(|error| RenderError::Cairo(error.to_string()))?;
    let bytes = glib::Bytes::from(&data[..]);
    let texture = gdk::MemoryTexture::new(
        width,
        height,
        gdk::MemoryFormat::B8g8r8a8Premultiplied,
        &bytes,
        stride,
    );
    let png = texture.save_to_png_bytes();
    Png::new(png.to_vec()).map_err(RenderError::Image)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::annotation::{Color, Point, Width};

    /// Ein echtes PNG (nicht nur ein Kopf): einfarbig hellgrau mit einem
    /// magentafarbenen 4×4-Quadrat bei (`mx`, `my`).
    pub fn test_png(width: i32, height: i32, mx: i32, my: i32) -> Png {
        let stride = width as usize * 4;
        let mut data = vec![0u8; stride * height as usize];
        for y in 0..height as usize {
            for x in 0..width as usize {
                let i = y * stride + x * 4;
                let magenta =
                    (mx..mx + 4).contains(&(x as i32)) && (my..my + 4).contains(&(y as i32));
                let (r, g, b) = if magenta {
                    (255, 0, 255)
                } else {
                    (200, 200, 200)
                };
                data[i..i + 4].copy_from_slice(&[r, g, b, 255]);
            }
        }
        let texture = gdk::MemoryTexture::new(
            width,
            height,
            gdk::MemoryFormat::R8g8b8a8,
            &glib::Bytes::from(&data),
            stride,
        );
        Png::new(texture.save_to_png_bytes().to_vec()).expect("png")
    }

    /// RGB eines Pixels eines PNG.
    pub fn pixel(png: &Png, x: i32, y: i32) -> (u8, u8, u8) {
        let texture = gdk::Texture::from_bytes(&glib::Bytes::from(png.bytes())).expect("decode");
        let mut downloader = gdk::TextureDownloader::new(&texture);
        downloader.set_format(gdk::MemoryFormat::R8g8b8a8);
        let (data, stride) = downloader.download_bytes();
        let i = y as usize * stride + x as usize * 4;
        (data[i], data[i + 1], data[i + 2])
    }

    fn is_red((r, g, b): (u8, u8, u8)) -> bool {
        r > 200 && g < 60 && b < 60
    }

    fn mark(shape: Shape) -> Mark {
        Mark {
            shape,
            color: Color::RED,
            width: Width::MEDIUM,
        }
    }

    #[test]
    fn compose_keeps_size_and_original_and_adds_red_ellipse_outline_only() {
        let original = test_png(400, 300, 198, 148);
        let before = original.bytes().to_vec();
        let ellipse = mark(Shape::Ellipse(
            Point::new(100.0, 100.0),
            Point::new(300.0, 200.0),
        ));
        let result = compose(&original, &[ellipse]).expect("compose");
        assert_eq!(result.dimensions(), Some((400, 300)));
        assert_eq!(original.bytes(), before.as_slice(), "Original unverändert");
        // Rand der Ellipse: links (100,150), oben (200,100), rechts, unten
        for (x, y) in [(100, 150), (200, 100), (300, 150), (200, 200)] {
            assert!(
                is_red(pixel(&result, x, y)),
                "Rand bei {x},{y}: {:?}",
                pixel(&result, x, y)
            );
        }
        // innen nicht gefüllt: Bildinhalt (Magenta-Marke) bleibt sichtbar
        assert_eq!(pixel(&result, 199, 149), (255, 0, 255));
        assert_eq!(pixel(&result, 150, 150), (200, 200, 200));
        // außerhalb unverändert
        assert_eq!(pixel(&result, 20, 20), (200, 200, 200));
    }

    #[test]
    fn rectangle_arrow_pen_and_text_reach_the_exported_image() {
        let original = test_png(400, 300, 0, 0);
        let marks = [
            mark(Shape::Rectangle(
                Point::new(20.0, 20.0),
                Point::new(120.0, 80.0),
            )),
            mark(Shape::Arrow(
                Point::new(200.0, 250.0),
                Point::new(350.0, 250.0),
            )),
            Mark {
                shape: Shape::Pen(vec![Point::new(20.0, 150.0), Point::new(120.0, 150.0)]),
                color: Color::BLUE,
                width: Width::THICK,
            },
            Mark {
                shape: Shape::Text(
                    Point::new(200.0, 20.0),
                    "Hier stimmt etwas nicht – Größe".into(),
                ),
                color: Color::BLACK,
                width: Width::MEDIUM,
            },
        ];
        let result = compose(&original, &marks).expect("compose");
        assert!(is_red(pixel(&result, 20, 50)), "Rechteck links");
        assert!(is_red(pixel(&result, 70, 80)), "Rechteck unten");
        assert_eq!(
            pixel(&result, 70, 50),
            (200, 200, 200),
            "Rechteck ohne Füllung"
        );
        assert!(is_red(pixel(&result, 250, 250)), "Pfeilschaft");
        assert!(is_red(pixel(&result, 345, 250)), "Pfeilspitze");
        assert!(
            is_red(pixel(&result, 335, 255)),
            "Spitze ist breiter als der Schaft"
        );
        let (r, g, b) = pixel(&result, 70, 150);
        assert!(
            b > 180 && r < 40 && g < 120,
            "blauer dicker Strich: {r},{g},{b}"
        );
        let dark = (200..400)
            .flat_map(|x| (20..50).map(move |y| (x, y)))
            .filter(|&(x, y)| pixel_sum(&result, x, y) < 100)
            .count();
        assert!(dark > 200, "Text sichtbar ({dark} dunkle Pixel)");
    }

    fn pixel_sum(png: &Png, x: i32, y: i32) -> u32 {
        let (r, g, b) = pixel(png, x, y);
        u32::from(r) + u32::from(g) + u32::from(b)
    }

    #[test]
    fn without_marks_the_image_content_is_identical() {
        let original = test_png(64, 32, 10, 10);
        let result = compose(&original, &[]).expect("compose");
        for (x, y) in [(0, 0), (11, 11), (63, 31)] {
            assert_eq!(pixel(&result, x, y), pixel(&original, x, y));
        }
    }
}
