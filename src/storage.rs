// SPDX-License-Identifier: MIT

//! Speichern von Aufnahmen: Zielordner, Dateinamen ohne Kollision, private
//! Dateirechte, nie überschreiben.

use std::{
    fmt,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use crate::screenshot::Png;

const FOLDER: &str = "Screenshots";
const PREFIX: &str = "Screenshot";

#[derive(Debug)]
pub enum SaveError {
    CreateDirectory(PathBuf, std::io::Error),
    NotADirectory(PathBuf),
    /// Auch nach vielen Suffixen existierte jeder Kandidat bereits.
    NoFreeName(PathBuf),
    Write(PathBuf, std::io::Error),
    /// Vom Benutzer gewählter Name ist schon vergeben (nie überschreiben).
    Exists(PathBuf),
    InvalidName(String),
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CreateDirectory(path, error) => {
                write!(
                    f,
                    "Ordner „{}“ kann nicht angelegt werden: {error}",
                    path.display()
                )
            }
            Self::NotADirectory(path) => write!(f, "„{}“ ist kein Ordner", path.display()),
            Self::NoFreeName(path) => write!(f, "Kein freier Dateiname in „{}“", path.display()),
            Self::Write(path, error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                write!(
                    f,
                    "Keine Schreibberechtigung für „{}“ – bitte einen anderen Ordner wählen",
                    path.display()
                )
            }
            Self::Write(path, error) => write!(
                f,
                "„{}“ kann nicht geschrieben werden: {error}",
                path.display()
            ),
            Self::Exists(path) => write!(
                f,
                "„{}“ existiert bereits – bitte einen anderen Namen wählen",
                path.display()
            ),
            Self::InvalidName(reason) => write!(f, "Ungültiger Dateiname: {reason}"),
        }
    }
}

/// Der Screenshots-Ordner unterhalb des Bilderordners (XDG), sonst
/// `~/Screenshots`; der Ordner wird erst beim Speichern angelegt.
pub fn directory_for(pictures: Option<PathBuf>, home: &Path) -> PathBuf {
    match pictures {
        Some(pictures) if pictures != home => pictures.join(FOLDER),
        _ => home.join(FOLDER),
    }
}

pub fn default_directory() -> PathBuf {
    let home = gtk::glib::home_dir();
    let pictures = gtk::glib::user_special_dir(gtk::glib::UserDirectory::Pictures);
    directory_for(pictures, &home)
}

/// `Screenshot-JJJJ-MM-TT-HHMMSS.png`
pub fn file_name(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> String {
    format!("{PREFIX}-{year:04}-{month:02}-{day:02}-{hour:02}{minute:02}{second:02}.png")
}

pub fn file_name_now() -> String {
    let now = gtk::glib::DateTime::now_local();
    match now {
        Ok(now) => file_name(
            now.year(),
            now.month() as u32,
            now.day_of_month() as u32,
            now.hour() as u32,
            now.minute() as u32,
            now.seconds() as u32,
        ),
        Err(_) => format!("{PREFIX}.png"),
    }
}

/// Der erste nicht vorhandene Pfad: `name.png`, `name-2.png`, `name-3.png` …
pub fn unique_path(directory: &Path, name: &str) -> Result<PathBuf, SaveError> {
    let stem = name.strip_suffix(".png").unwrap_or(name);
    let candidates =
        std::iter::once(format!("{stem}.png")).chain((2..=999).map(|n| format!("{stem}-{n}.png")));
    for candidate in candidates {
        let path = directory.join(candidate);
        if !path.exists() && fs::symlink_metadata(&path).is_err() {
            return Ok(path);
        }
    }
    Err(SaveError::NoFreeName(directory.to_path_buf()))
}

/// Prüft einen vom Benutzer eingegebenen Dateinamen und ergänzt `.png`.
/// Unicode ist erlaubt; verboten sind Pfadtrenner, Steuerzeichen, „.“ und
/// „..“ sowie Namen, die das Dateisystem nicht fassen kann.
pub fn validate_name(input: &str) -> Result<String, SaveError> {
    let name = input.trim();
    let invalid = |reason: &str| Err(SaveError::InvalidName(reason.to_owned()));
    if name.is_empty() {
        return invalid("der Name ist leer");
    }
    if name.contains('/') {
        return invalid("„/“ ist im Namen nicht erlaubt");
    }
    if name.chars().any(char::is_control) {
        return invalid("Steuerzeichen sind im Namen nicht erlaubt");
    }
    if name == "." || name == ".." {
        return invalid("„.“ und „..“ sind keine Dateinamen");
    }
    let name = if name.to_lowercase().ends_with(".png") {
        name.to_owned()
    } else {
        format!("{name}.png")
    };
    if name.len() > 255 {
        return invalid("der Name ist zu lang (höchstens 255 Byte)");
    }
    Ok(name)
}

/// Speichert unter genau `name` in `directory` (Ordner bei Bedarf 0700,
/// Datei 0600, `create_new`): Ist der Name vergeben, wird nichts
/// überschrieben, sondern `SaveError::Exists` gemeldet.
pub fn save_exact(directory: &Path, name: &str, png: &Png) -> Result<PathBuf, SaveError> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    let name = validate_name(name)?;
    if fs::symlink_metadata(directory).is_err() {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(directory)
            .map_err(|error| SaveError::CreateDirectory(directory.to_path_buf(), error))?;
    }
    if !directory.is_dir() {
        return Err(SaveError::NotADirectory(directory.to_path_buf()));
    }
    let path = directory.join(&name);
    let mut file = match OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(SaveError::Exists(path));
        }
        Err(error) => return Err(SaveError::Write(path, error)),
    };
    if let Err(error) = file.write_all(png.bytes()).and_then(|()| file.sync_all()) {
        // Keine halbe Datei zurücklassen.
        drop(file);
        let _ = fs::remove_file(&path);
        return Err(SaveError::Write(path, error));
    }
    Ok(path)
}

/// Legt den Ordner an (0700), wählt einen freien Namen und schreibt die
/// Datei mit `create_new` (0600): weder eine vorhandene Datei noch ein
/// Symlink wird überschrieben.
pub fn save(directory: &Path, name: &str, png: &Png) -> Result<PathBuf, SaveError> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    if !directory.exists() {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(directory)
            .map_err(|error| SaveError::CreateDirectory(directory.to_path_buf(), error))?;
    }
    if !directory.is_dir() {
        return Err(SaveError::NotADirectory(directory.to_path_buf()));
    }
    for _ in 0..3 {
        let path = unique_path(directory, name)?;
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
        {
            Ok(mut file) => {
                file.write_all(png.bytes())
                    .and_then(|()| file.sync_all())
                    .map_err(|error| SaveError::Write(path.clone(), error))?;
                return Ok(path);
            }
            // Zwischen Namenswahl und Anlegen hat jemand den Namen belegt.
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(SaveError::Write(path, error)),
        }
    }
    Err(SaveError::NoFreeName(directory.to_path_buf()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screenshot::sample;

    fn scratch(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "querencia-screenshot-test-{label}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn file_names_are_sortable_timestamps() {
        assert_eq!(
            file_name(2026, 10, 15, 13, 5, 1),
            "Screenshot-2026-10-15-130501.png"
        );
        assert!(file_name_now().starts_with("Screenshot-"));
        assert!(file_name_now().ends_with(".png"));
    }

    #[test]
    fn directory_prefers_the_pictures_folder_and_falls_back_to_home() {
        let home = Path::new("/home/test");
        assert_eq!(
            directory_for(Some(PathBuf::from("/home/test/Bilder")), home),
            PathBuf::from("/home/test/Bilder/Screenshots")
        );
        assert_eq!(
            directory_for(None, home),
            PathBuf::from("/home/test/Screenshots")
        );
        // GLib meldet HOME, wenn kein Bilderordner konfiguriert ist.
        assert_eq!(
            directory_for(Some(PathBuf::from("/home/test")), home),
            PathBuf::from("/home/test/Screenshots")
        );
    }

    #[test]
    fn collisions_get_a_numeric_suffix_and_symlinks_count_as_taken() {
        let dir = scratch("collision");
        fs::create_dir_all(&dir).expect("scratch");
        assert_eq!(
            unique_path(&dir, "Screenshot-x.png").expect("free"),
            dir.join("Screenshot-x.png")
        );
        fs::write(dir.join("Screenshot-x.png"), b"taken").expect("occupy");
        assert_eq!(
            unique_path(&dir, "Screenshot-x.png").expect("free"),
            dir.join("Screenshot-x-2.png")
        );
        std::os::unix::fs::symlink("/nonexistent", dir.join("Screenshot-x-2.png"))
            .expect("dangling link");
        assert_eq!(
            unique_path(&dir, "Screenshot-x.png").expect("free"),
            dir.join("Screenshot-x-3.png")
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_creates_the_folder_privately_and_never_overwrites() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("save").join("Bilder").join("Screenshots");
        let png = Png::new(sample(2, 2)).expect("png");
        let first = save(&dir, "Screenshot-a.png", &png).expect("first");
        assert_eq!(first, dir.join("Screenshot-a.png"));
        assert_eq!(fs::read(&first).expect("read"), sample(2, 2));
        assert_eq!(
            fs::metadata(&first).expect("meta").permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(&dir).expect("dir").permissions().mode() & 0o777,
            0o700
        );
        let second = save(&dir, "Screenshot-a.png", &png).expect("second");
        assert_eq!(second, dir.join("Screenshot-a-2.png"));
        assert_eq!(
            fs::read(&first).expect("read"),
            sample(2, 2),
            "erste Datei unverändert"
        );
        let _ = fs::remove_dir_all(dir.parent().and_then(Path::parent).expect("scratch root"));
    }

    #[test]
    fn user_names_accept_unicode_and_get_the_png_suffix() {
        assert_eq!(
            validate_name("  Übersicht – Größe 10 € „Test“ ").expect("ok"),
            "Übersicht – Größe 10 € „Test“.png"
        );
        assert_eq!(validate_name("Bild.PNG").expect("ok"), "Bild.PNG");
        assert_eq!(validate_name("a.png").expect("ok"), "a.png");
        assert_eq!(validate_name("Bericht.jpg").expect("ok"), "Bericht.jpg.png");
        for bad in ["", "   ", "a/b", "../x", ".", "..", "zeile\nzwei", "tab\tx"] {
            assert!(
                matches!(validate_name(bad), Err(SaveError::InvalidName(_))),
                "{bad:?}"
            );
        }
        assert!(validate_name(&"ä".repeat(125)).is_ok(), "250 + 4 Byte");
        assert!(validate_name(&"ä".repeat(126)).is_err(), "252 + 4 Byte");
    }

    #[test]
    fn exact_save_never_overwrites_and_keeps_unicode_names() {
        use std::os::unix::fs::PermissionsExt;
        let root = scratch("exact");
        let dir = root.join("Bilder").join("Screenshots");
        let png = Png::new(sample(3, 2)).expect("png");
        let path = save_exact(&dir, "Größe – Ü", &png).expect("saved");
        assert_eq!(path, dir.join("Größe – Ü.png"));
        assert_eq!(fs::read(&path).expect("read"), sample(3, 2));
        assert_eq!(
            fs::metadata(&path).expect("meta").permissions().mode() & 0o777,
            0o600
        );
        let other = Png::new(sample(9, 9)).expect("png");
        assert!(matches!(
            save_exact(&dir, "Größe – Ü.png", &other),
            Err(SaveError::Exists(_))
        ));
        assert_eq!(fs::read(&path).expect("read"), sample(3, 2), "unverändert");
        std::os::unix::fs::symlink("/nonexistent", dir.join("link.png")).expect("link");
        assert!(matches!(
            save_exact(&dir, "link", &other),
            Err(SaveError::Exists(_))
        ));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn exact_save_reports_missing_permission_and_files_in_the_way() {
        use std::os::unix::fs::PermissionsExt;
        let root = scratch("exact-refuse");
        fs::create_dir_all(&root).expect("scratch");
        let png = Png::new(sample(1, 1)).expect("png");
        let file = root.join("Screenshots");
        fs::write(&file, b"").expect("file");
        let error = save_exact(&file, "a", &png).expect_err("file");
        assert!(matches!(error, SaveError::NotADirectory(_)));
        assert!(error.to_string().contains("ist kein Ordner"));
        let locked = root.join("gesperrt");
        fs::create_dir(&locked).expect("dir");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o500)).expect("chmod");
        let error = save_exact(&locked, "a", &png).expect_err("readonly");
        assert!(
            error.to_string().starts_with("Keine Schreibberechtigung"),
            "{error}"
        );
        assert!(fs::read_dir(&locked).expect("list").next().is_none());
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).expect("chmod");
        let error = save_exact(&locked.join("x").join("y"), "a", &png);
        assert!(error.is_ok(), "nach Freigabe anlegbar");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn save_refuses_a_file_in_place_of_the_folder_and_unwritable_targets() {
        let root = scratch("refuse");
        fs::create_dir_all(&root).expect("scratch");
        let not_a_dir = root.join("file");
        fs::write(&not_a_dir, b"x").expect("file");
        let png = Png::new(sample(1, 1)).expect("png");
        assert!(matches!(
            save(&not_a_dir, "Screenshot.png", &png),
            Err(SaveError::NotADirectory(_))
        ));
        assert!(matches!(
            save(&not_a_dir.join("below"), "Screenshot.png", &png),
            Err(SaveError::CreateDirectory(..))
        ));
        let _ = fs::remove_dir_all(&root);
    }
}
