# Querencia Screenshot

Klassisches Screenshot-Werkzeug für Querencia Desktop (Wayland/labwc), Rust + GTK4.
Bedienung nach dem Vorbild einfacher Snipping-Werkzeuge: Modus wählen, Bereich
aufziehen, speichern oder in die Zwischenablage kopieren.

## Aufnahmemodi

| Modus | Umsetzung |
|---|---|
| Rechteck | `slurp` liefert `x,y wxh`, `grim -g` nimmt nur diesen Bereich auf |
| Fenster (per Rechteck) | wie Rechteck – labwc stellt keine Fenstergeometrie bereit, der Benutzer rahmt das Fenster ein |
| Bildschirm | `grim` nimmt alle Ausgaben in ein Bild auf |

Vor jeder Aufnahme wird das eigene Fenster verborgen; die Aufnahme beginnt erst nach
dem `unmap`-Signal plus einer kurzen Karenz für das nächste Compositor-Bild.

## Speichern und Zwischenablage

- Ziel: `$XDG_PICTURES_DIR/Screenshots`, sonst `~/Screenshots`; Ordner `0700`, Dateien `0600`.
- Name: `Screenshot-JJJJ-MM-TT-HHMMSS.png`; bei Kollision `-2`, `-3`, … (nie überschreiben,
  `create_new`).
- Zwischenablage: `wl-copy --type image/png` mit den PNG-Bytes auf stdin; ohne wl-copy
  bietet GTK die Daten selbst an, solange das Fenster offen ist.

## Voraussetzungen

`grim` (Pflicht), `slurp` (Rechteck/Fenster), `wl-clipboard` (Kopieren), optional `satty`.
Fehlende Programme werden in der Statuszeile gemeldet; nichts wird automatisch installiert.

## Bauen und prüfen

```bash
cargo fmt --all --check
cargo check --all-targets --locked
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo build --locked
```

Die Tests brauchen kein Display und starten keine Aufnahme; externe Programme werden
über `runner::CommandRunner` ersetzt. GUI-Abnahme: `scripts/qs-0a-pruefanleitung.md`,
Start über `scripts/qs-0a-gui-start.sh`.

## Struktur

```
src/main.rs       GtkApplication, Modulverbund
src/ui.rs         Auswahlfenster, Ergebnisansicht, Escape, Statuszeile
src/capture.rs    Bereichsauswahl (slurp) und Aufnahme (grim)
src/geometry.rs   Region: Parsen/Validieren von `x,y wxh`
src/runner.rs     CommandRunner (System + Test-Attrappe), keine Shell
src/screenshot.rs Png: Signaturprüfung, Größenlimit, Abmessungen
src/storage.rs    Zielordner, Dateinamen, kollisionsfreies privates Speichern
src/clipboard.rs  wl-copy mit image/png
src/style.css     helle, kompakte Optik
```

Lizenz: MIT (siehe `LICENSE`). Symbole stammen zur Laufzeit aus dem GTK-Symbolthema
der Sitzung (bevorzugt GNOME Human); es werden keine Symboldateien mitgeliefert.
