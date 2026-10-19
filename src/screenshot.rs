// SPDX-License-Identifier: GPL-3.0-or-later

//! Aufgenommene Bilddaten: immer PNG, geprüft und größenbegrenzt.

use std::fmt;

pub const MIME_TYPE: &str = "image/png";
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
/// Oberhalb dieser Größe wird eine Aufnahme verworfen statt weitergereicht.
pub const MAX_BYTES: usize = 48 * 1024 * 1024;

#[derive(Clone, PartialEq, Eq)]
pub struct Png {
    bytes: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PngError {
    Empty,
    NotPng,
    TooLarge(usize),
}

impl fmt::Display for PngError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "Die Aufnahme ist leer"),
            Self::NotPng => write!(f, "Die Aufnahme ist kein PNG-Bild"),
            Self::TooLarge(bytes) => write!(
                f,
                "Die Aufnahme ist zu groß ({} MiB)",
                bytes / (1024 * 1024)
            ),
        }
    }
}

impl Png {
    pub fn new(bytes: Vec<u8>) -> Result<Self, PngError> {
        if bytes.is_empty() {
            return Err(PngError::Empty);
        }
        if bytes.len() > MAX_BYTES {
            return Err(PngError::TooLarge(bytes.len()));
        }
        if !bytes.starts_with(&PNG_SIGNATURE) {
            return Err(PngError::NotPng);
        }
        Ok(Self { bytes })
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn size_bytes(&self) -> usize {
        self.bytes.len()
    }

    /// Breite und Höhe aus dem IHDR-Kopf, falls vorhanden.
    pub fn dimensions(&self) -> Option<(u32, u32)> {
        let header = self.bytes.get(8..24)?;
        if &header[4..8] != b"IHDR" {
            return None;
        }
        let width = u32::from_be_bytes(header[8..12].try_into().ok()?);
        let height = u32::from_be_bytes(header[12..16].try_into().ok()?);
        Some((width, height))
    }
}

impl fmt::Debug for Png {
    // Niemals Bilddaten ausgeben.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Png({} Byte)", self.bytes.len())
    }
}

#[cfg(test)]
pub fn sample(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = PNG_SIGNATURE.to_vec();
    bytes.extend_from_slice(&13u32.to_be_bytes());
    bytes.extend_from_slice(b"IHDR");
    bytes.extend_from_slice(&width.to_be_bytes());
    bytes.extend_from_slice(&height.to_be_bytes());
    bytes.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_png_and_reads_its_dimensions() {
        let png = Png::new(sample(640, 480)).expect("png");
        assert_eq!(png.dimensions(), Some((640, 480)));
        assert_eq!(
            format!("{png:?}"),
            format!("Png({} Byte)", png.size_bytes())
        );
    }

    #[test]
    fn rejects_empty_foreign_and_oversized_data() {
        assert_eq!(Png::new(Vec::new()).err(), Some(PngError::Empty));
        assert_eq!(
            Png::new(b"P6\n1 1\n255\n".to_vec()).err(),
            Some(PngError::NotPng)
        );
        let huge = vec![0u8; MAX_BYTES + 1];
        assert_eq!(
            Png::new(huge).err(),
            Some(PngError::TooLarge(MAX_BYTES + 1))
        );
    }

    #[test]
    fn signature_without_header_has_no_dimensions() {
        let png = Png::new(PNG_SIGNATURE.to_vec()).expect("bare signature is still png");
        assert_eq!(png.dimensions(), None);
    }
}
