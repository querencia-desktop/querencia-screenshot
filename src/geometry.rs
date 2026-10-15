// SPDX-License-Identifier: MIT

//! Bildschirmbereiche, wie slurp sie ausgibt und grim sie erwartet
//! (`x,y wxh` in logischen Pixeln).

use std::fmt;

/// Größte zulässige Kantenlänge eines Bereichs; schützt grim vor absurden
/// Werten aus einer manipulierten Auswahl.
pub const MAX_EDGE: u32 = 32_768;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub enum GeometryError {
    /// slurp hat nichts geliefert (Abbruch durch den Benutzer).
    Empty,
    /// Die Ausgabe folgt nicht dem Muster `x,y wxh`.
    Malformed(String),
    /// Breite oder Höhe sind null oder zu groß.
    Degenerate(Region),
}

impl fmt::Display for GeometryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "Keine Auswahl"),
            Self::Malformed(text) => write!(f, "Ungültige Geometrie: {text:?}"),
            Self::Degenerate(region) => write!(
                f,
                "Ungültige Größe: {}×{} Pixel",
                region.width, region.height
            ),
        }
    }
}

impl Region {
    /// Liest die Standardausgabe von slurp (`x,y wxh`, optional mit
    /// Zeilenumbruch) und prüft sie.
    pub fn parse(text: &str) -> Result<Self, GeometryError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(GeometryError::Empty);
        }
        let malformed = || GeometryError::Malformed(text.chars().take(64).collect());
        let (origin, size) = text.split_once(' ').ok_or_else(malformed)?;
        let (x, y) = origin.split_once(',').ok_or_else(malformed)?;
        let (width, height) = size.split_once('x').ok_or_else(malformed)?;
        let region = Self {
            x: x.parse().map_err(|_| malformed())?,
            y: y.parse().map_err(|_| malformed())?,
            width: width.parse().map_err(|_| malformed())?,
            height: height.parse().map_err(|_| malformed())?,
        };
        region.validated()
    }

    pub fn validated(self) -> Result<Self, GeometryError> {
        if self.width == 0 || self.height == 0 || self.width > MAX_EDGE || self.height > MAX_EDGE {
            return Err(GeometryError::Degenerate(self));
        }
        Ok(self)
    }

    /// Das Argument für `grim -g`.
    pub fn grim_argument(&self) -> String {
        format!("{},{} {}x{}", self.x, self.y, self.width, self.height)
    }
}

impl fmt::Display for Region {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}×{} Pixel bei ({}, {})",
            self.width, self.height, self.x, self.y
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_slurp_output_with_and_without_newline() {
        let region = Region {
            x: 10,
            y: 20,
            width: 300,
            height: 150,
        };
        assert_eq!(Region::parse("10,20 300x150"), Ok(region));
        assert_eq!(Region::parse("10,20 300x150\n"), Ok(region));
        assert_eq!(
            Region::parse("-5,-7 1x1"),
            Ok(Region {
                x: -5,
                y: -7,
                width: 1,
                height: 1
            })
        );
        assert_eq!(region.grim_argument(), "10,20 300x150");
    }

    #[test]
    fn empty_output_is_a_cancellation_not_a_format_error() {
        assert_eq!(Region::parse(""), Err(GeometryError::Empty));
        assert_eq!(Region::parse("  \n"), Err(GeometryError::Empty));
    }

    #[test]
    fn malformed_output_is_rejected_with_a_bounded_excerpt() {
        for text in [
            "10,20",
            "10;20 300x150",
            "a,b cxd",
            "10,20 300x",
            "10,20 300x150 extra",
        ] {
            match Region::parse(text) {
                Err(GeometryError::Malformed(excerpt)) => assert!(excerpt.len() <= 64),
                other => panic!("{text:?}: {other:?}"),
            }
        }
        let long = format!("{} 1x1", "9".repeat(200));
        match Region::parse(&long) {
            Err(GeometryError::Malformed(excerpt)) => assert_eq!(excerpt.len(), 64),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn zero_or_oversized_regions_are_degenerate() {
        assert!(matches!(
            Region::parse("0,0 0x10"),
            Err(GeometryError::Degenerate(_))
        ));
        assert!(matches!(
            Region::parse("0,0 10x0"),
            Err(GeometryError::Degenerate(_))
        ));
        assert!(matches!(
            Region::parse("0,0 40000x10"),
            Err(GeometryError::Degenerate(_))
        ));
        assert!(Region::parse("0,0 32768x32768").is_ok());
    }
}
