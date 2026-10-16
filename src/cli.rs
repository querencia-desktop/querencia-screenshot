// SPDX-License-Identifier: MIT

//! Kommandozeile. Ohne Optionen startet das bisherige Auswahlfenster; mit
//! einer Aufnahmeoption wird sofort aufgenommen und danach der Dialog
//! „Bildschirmfoto speichern“ gezeigt (GNOME 2: Druck, Alt+Druck).

use std::ffi::OsString;

pub const USAGE: &str = "\
Aufruf: querencia-screenshot [OPTION]

Ohne Option öffnet sich das Fenster mit der Wahl des Aufnahmemodus.

  -s, --screen   sofort den ganzen Bildschirm aufnehmen (Druck)
  -w, --window   sofort das aktive Fenster aufnehmen (Alt+Druck)
  -a, --area     sofort einen Bereich mit der Maus aufziehen
  -h, --help     diese Hilfe anzeigen
      --version  Version anzeigen

Nach einer Sofortaufnahme erscheint „Bildschirmfoto speichern“ mit Vorschau,
Dateiname, Ordner, Kopieren in die Zwischenablage und Abbrechen.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Screen,
    Window,
    Area,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Gui,
    Capture(Target),
    Help,
    Version,
}

/// Liest die Argumente ohne den Programmnamen. Genau eine Option ist
/// erlaubt; alles andere ist ein Fehler mit deutscher Meldung.
pub fn parse<I: IntoIterator<Item = OsString>>(args: I) -> Result<Command, String> {
    let mut command = Command::Gui;
    for arg in args {
        let Some(text) = arg.to_str() else {
            return Err(format!("ungültiges Argument: {}", arg.to_string_lossy()));
        };
        let next = match text {
            "-s" | "--screen" => Command::Capture(Target::Screen),
            "-w" | "--window" => Command::Capture(Target::Window),
            "-a" | "--area" => Command::Capture(Target::Area),
            "-h" | "--help" => Command::Help,
            "--version" => Command::Version,
            other => return Err(format!("unbekannte Option: {other}")),
        };
        if command != Command::Gui {
            return Err("nur eine Option auf einmal".to_owned());
        }
        command = next;
    }
    Ok(command)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn without_options_the_previous_gui_starts() {
        assert_eq!(parse(args(&[])), Ok(Command::Gui));
    }

    #[test]
    fn capture_options_have_short_and_long_forms() {
        for (list, target) in [
            (&["--screen"], Target::Screen),
            (&["-s"], Target::Screen),
            (&["--window"], Target::Window),
            (&["-w"], Target::Window),
            (&["--area"], Target::Area),
            (&["-a"], Target::Area),
        ] {
            assert_eq!(parse(args(list)), Ok(Command::Capture(target)), "{list:?}");
        }
        assert_eq!(parse(args(&["--help"])), Ok(Command::Help));
        assert_eq!(parse(args(&["--version"])), Ok(Command::Version));
    }

    #[test]
    fn unknown_doubled_and_invalid_arguments_are_refused() {
        assert_eq!(
            parse(args(&["--fenster"])),
            Err("unbekannte Option: --fenster".into())
        );
        assert!(parse(args(&["--screen", "--window"])).is_err());
        assert!(parse(args(&["--screen", "--screen"])).is_err());
        assert!(parse(args(&["datei.png"])).is_err());
        use std::os::unix::ffi::OsStringExt;
        assert!(parse(vec![OsString::from_vec(vec![0xff, 0xfe])]).is_err());
    }
}
