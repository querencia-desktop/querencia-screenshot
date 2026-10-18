// SPDX-License-Identifier: MIT

//! Kommandozeile. Ohne Optionen öffnet sich das Auswahlfenster (Druck); mit
//! einer Aufnahmeoption wird sofort aufgenommen (Alt+Druck: `--window`).
//! Danach öffnet sich der Editor zum Markieren, Speichern und Kopieren.

use std::{ffi::OsString, path::PathBuf};

pub const USAGE: &str = "\
Aufruf: querencia-screenshot [OPTION]

Ohne Option öffnet sich das Auswahlfenster „Was möchten Sie aufnehmen?“.

  -s, --screen        sofort den ganzen Bildschirm aufnehmen
  -w, --window        sofort ein Fenster wählen: erkannte Fenster werden beim
                      Darüberfahren hervorgehoben, Klick nimmt das Fenster
                      samt Titelleiste und Rahmen auf; außerhalb erkannter
                      Fenster mit gedrückter Maustaste frei einrahmen;
                      Esc bricht ab. Ein Vollbildfenster wird direkt
                      aufgenommen, ohne aktives Fenster der ganze Bildschirm.
  -a, --area          sofort einen Bereich mit der Maus aufziehen
  -o, --open DATEI    ein vorhandenes PNG-Bild im Editor öffnen
  -h, --help          diese Hilfe anzeigen
      --version       Version anzeigen

Nach der Aufnahme öffnet sich der Editor: Markierungen (Stift, Kreis/Ellipse,
Rechteck, Pfeil, Text), Speichern als PNG und Kopieren in die Zwischenablage.
Eine geöffnete Datei wird nie überschrieben.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Screen,
    Window,
    Area,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Gui,
    Capture(Target),
    Open(PathBuf),
    Help,
    Version,
}

/// Liest die Argumente ohne den Programmnamen. Genau eine Option ist
/// erlaubt; alles andere ist ein Fehler mit deutscher Meldung.
pub fn parse<I: IntoIterator<Item = OsString>>(args: I) -> Result<Command, String> {
    let mut command = Command::Gui;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let Some(text) = arg.to_str() else {
            return Err(format!("ungültiges Argument: {}", arg.to_string_lossy()));
        };
        let next = match text {
            "-s" | "--screen" => Command::Capture(Target::Screen),
            "-w" | "--window" => Command::Capture(Target::Window),
            "-a" | "--area" => Command::Capture(Target::Area),
            "-h" | "--help" => Command::Help,
            "--version" => Command::Version,
            "-o" | "--open" => match args.next() {
                Some(path) if !path.is_empty() => Command::Open(PathBuf::from(path)),
                _ => return Err(format!("{text} braucht einen Dateinamen")),
            },
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
        assert_eq!(
            parse(args(&["--open", "Größe.png"])),
            Ok(Command::Open(PathBuf::from("Größe.png")))
        );
        assert_eq!(
            parse(args(&["-o", "--screen"])),
            Ok(Command::Open(PathBuf::from("--screen"))),
            "der Wert nach -o ist immer ein Dateiname"
        );
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
        assert!(parse(args(&["--open"])).is_err(), "Datei fehlt");
        assert!(parse(args(&["--open", "a.png", "--screen"])).is_err());
        use std::os::unix::ffi::OsStringExt;
        assert!(parse(vec![OsString::from_vec(vec![0xff, 0xfe])]).is_err());
    }
}
