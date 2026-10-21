# Querencia Screenshot

Bildschirmfotos aufnehmen, markieren und speichern für alle, die Querencia
Desktop benutzen. Teil von
[Querencia Desktop](https://github.com/querencia-desktop/querencia) – einer
klassischen Desktopumgebung im GNOME-2-Stil für Wayland (labwc, Rust, GTK 4).

*English summary:* A screenshot tool in the spirit of GNOME 2's
gnome-screenshot with a small markup editor (ellipse, rectangle, arrow, pen,
text), built on grim and slurp. Part of Querencia Desktop, a classic
GNOME-2-style desktop for Wayland. Alpha software; documentation is in German,
the user interface is available in English and German.

> **Alpha.** Die Fensterwahl erkennt Fenster an ihren labwc-Rahmen im Bild
> (labwc meldet keine Fenstergrenzen). Keine Sicherheits- oder
> Datenverlustgarantie.

## Zweck

- Aufnahme des ganzen Bildschirms, eines Fensters (Alt+Druck; Fensterwahl per
  Klick, Vollbildfenster exakt) oder eines Bereichs; Auswahlfenster „Was möchten Sie
  aufnehmen?“ oder sofort per `--screen`, `--window`, `--area`.
- Editor nach jeder Aufnahme: Kreis/Ellipse, Rechteck, Pfeil, Stift, Text in
  acht Grundfarben oder eigener Farbe, drei Stärken, Rückgängig/Wiederholen;
  Speichern als neues PNG in Originalgröße (nie überschreiben, Dateien
  `0600`) oder Kopieren in die Zwischenablage.
- Oberfläche auf Englisch und Deutsch über GNU gettext (englische msgid,
  `po/de.po`); Auswahl über `LANGUAGE`/`LANG`.
- Bewusst **nicht** Teil dieses Programms: Bildbearbeitung über Markierungen
  hinaus, Hochladen, Videoaufnahme, Verzögerungstimer.

Anwendungskennung (App-ID): `org.querencia.Screenshot` · Binary:
`querencia-screenshot` · Debian-Paket: `querencia-screenshot` (Paketierung in `querencia-packaging`)

Ausführlich (Tasten, Aufnahmewege, Speichern, Sprachen, Aufbau):
[docs/bedienung.md](docs/bedienung.md).

## Stand

| | |
|---|---|
| Version | 0.2.9 (Tag `screenshot/0.2.9`); seit 0.2.8 Fensterwahl per Klick, Menükürzel in der Sprache der Oberfläche, Rückfrage bei angefangenem Text |
| Letzte Prüfung | 21.10.2026: Unit-Tests; isolierte GUI-Prüfung (Stand 0.2.8: 66 Fälle, 691 Prüfpunkte, 0 Fehler) |
| Bekannte Lücken | Fensterwahl erkennt nur Fenster mit labwc-Rahmen; Kommandozeilenhilfe und Protokollzeilen nicht übersetzt; die isolierte GUI-Prüfung liegt noch nicht in diesem Repo |
| Getestet auf | Ubuntu 26.04, labwc 0.9.8, GTK 4.22, Rust 1.93 |
| Nicht getestet | andere Compositoren, HiDPI, Mehrmonitor |

Änderungen: [CHANGELOG.md](CHANGELOG.md).

## Bauen

Voraussetzungen (Ubuntu 26.04):

```bash
sudo apt install build-essential pkg-config rustc cargo libgtk-4-dev
sudo apt install grim slurp wl-clipboard   # zur Laufzeit: grim Pflicht, slurp und wl-copy empfohlen
```

```bash
cargo build --release --locked
```

Das Ergebnis liegt unter `target/release/querencia-screenshot`. Kataloge:
`python3 werkzeug/i18n.py mo <ziel>`; ohne `de.mo` zeigt das Programm
Englisch. Fehlende Laufzeitprogramme meldet es; nichts wird automatisch
installiert.

## Prüfen

```bash
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
python3 werkzeug/i18n.py pruefen
```

Diese Befehle laufen ohne Bildschirm; die ersten drei sind auch die CI
(`.github/workflows/ci.yml`). Die Tests starten keine Aufnahme (externe
Programme über `runner::CommandRunner` ersetzt, `render`-Tests prüfen
Pixel). Die **GUI-Prüfung** läuft in einer abgeschotteten Sitzung (eigener
labwc ohne Bildschirm, eigenes `HOME` und `XDG_RUNTIME_DIR`), niemals in der
laufenden Desktopsitzung; ihr Werkzeug liegt noch außerhalb dieses Repos. Für
Tests und Katalog im Einzelnen:
[docs/bauen-und-pruefen.md](docs/bauen-und-pruefen.md). Für eine Abnahme von
Hand in der eigenen Sitzung:
[docs/qs-0a-pruefanleitung.md](docs/qs-0a-pruefanleitung.md). Arbeitsweise:
[CONTRIBUTING.md](CONTRIBUTING.md).

## Vorbild und Abgrenzung

Vorbild für Bedienung und Aussehen ist **gnome-screenshot aus GNOME 2.30**,
für den Markierungseditor einfache Snipping-Werkzeuge. Querencia übernimmt
davon die Bedienideen (Aufnahmearten, Kürzel, Ablauf), **keinen Quellcode,
keine Texte, keine Grafiken**. Das Projekt ist mit GNOME und dessen Autoren
weder verbunden noch von ihnen unterstützt.

Abweichungen vom Vorbild und Dinge, die bewusst fehlen:

- GTK 4 statt GTK 2; Aufnahme über `grim`/`slurp` (Wayland) statt X11.
- Eigener Markierungseditor (gnome-screenshot hat keinen).

Fremdbestandteile: nur Rust-Abhängigkeiten (gtk4-rs, wayland-client und
Umfeld) unter MIT, MIT OR Apache-2.0 und verwandten freien Lizenzen; Liste
mit `cargo metadata --locked`. Dynamisch gelinkt: GTK 4, libwayland und das
gettext der glibc. Zur Laufzeit aufgerufen (nicht gelinkt): `grim`, `slurp`,
`wl-copy`.

## Lizenz

Programm: GPL-3.0-or-later, siehe [LICENSE](LICENSE) (bis 0.2.4 MIT, seit
0.2.6 wie alle Querencia-Programme GPL). Copyright 2026 Querencia Desktop
contributors; Urheberhinweis in [COPYRIGHT](COPYRIGHT). Die
MetaInfo-Daten stehen unter FSFAP.

Name und Logo „Querencia“ sind keine freien Inhalte: [MARKE.md](MARKE.md).

## Fehler melden

Fehler, Wünsche und Fragen zu Querencia gehen an die gemeinsame Meldestelle:
<https://github.com/querencia-desktop/querencia/issues/new/choose>.
Sicherheitslücken bitte nicht öffentlich melden, siehe [SECURITY.md](SECURITY.md).
