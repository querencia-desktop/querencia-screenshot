# Querencia Screenshot

Klassisches Screenshot-Werkzeug für Querencia Desktop (Wayland/labwc), Rust + GTK4.
Bedienung nach dem Vorbild einfacher Snipping-Werkzeuge: Aufnahmeart wählen,
aufnehmen, im Editor markieren (Kreis, Rechteck, Pfeil, Stift, Text), speichern
oder in die Zwischenablage kopieren.

## Bedienung (QS-0d)

| Aufruf | Verhalten |
|---|---|
| `querencia-screenshot` (Druck) | Auswahlfenster „Was möchten Sie aufnehmen?“: Gesamten Bildschirm aufnehmen, Fenster einrahmen, Bereich auswählen |
| `--window` (Alt+Druck) | aktives Fenster: Vollbildfenster exakt, sonst einrahmen (labwc meldet keine Fenstergrenzen); ohne aktives Fenster ganzer Bildschirm |
| `--screen`, `--area` | sofort Bildschirm bzw. Bereich |
| `--open DATEI` | vorhandenes PNG bearbeiten; gespeichert wird unter neuem Namen, nie über das Original |

Nach jeder Aufnahme öffnet sich der **Editor**: Menüleiste (Datei, Bearbeiten,
Werkzeuge, Ansicht), Werkzeugleiste mit Tooltips, Bild eingepasst oder in
Originalgröße, Statuszeile mit Bedienhinweis.

| Werkzeug / Aktion | Taste |
|---|---|
| Kreis/Ellipse (Standard, rot, nur Umrandung; Umschalt = Kreis) | E |
| Rechteck (Umschalt = Quadrat) | R |
| Pfeil | A |
| Stift | P |
| Text (klicken, tippen, Eingabetaste; Esc verwirft) | T |
| Rückgängig / Wiederholen | Strg+Z / Strg+Umschalt+Z, Strg+Y |
| Speichern … / In Zwischenablage kopieren | Strg+S / Strg+C |
| Neue Aufnahme / Schließen | Strg+N / Strg+W |
| laufende Zeichnung abbrechen | Esc |
| Farbpalette öffnen (Pfeiltasten, Eingabe-/Leertaste, Esc) | F |

Farben: eine Farbschaltfläche in der Werkzeugleiste zeigt die aktuelle Farbe
und klappt die Palette auf – acht Grundfarben in zwei Reihen (Rot =
Standard, Orange, Gelb, Grün, Blau, Violett, Schwarz, Weiß) und
„Benutzerdefinierte Farbe …“ (kleines Fenster mit dem GTK-Farbwähler,
Eingabe „#RRGGBB“). Die Farbe gilt für neue Markierungen aller Werkzeuge;
vorhandene behalten ihre Farbe. Stärken Dünn, Mittel (Standard), Dick. „Alle Markierungen entfernen“ fragt nach und ist rückgängig
machbar. Ungespeicherte Markierungen lösen beim Schließen eine Abfrage aus.

Die Aufnahme bleibt unverändert; Markierungen sind eigene Objekte in
Originalpixeln. Erst Speichern und Kopieren erzeugen daraus ein neues PNG in
Originalgröße (`render::compose`).

## Aufnahme

| Art | Umsetzung |
|---|---|
| Bereich / Fenster einrahmen | `slurp` liefert `x,y wxh`, `grim -g` nimmt nur diesen Bereich auf |
| Fenster (Alt+Druck) | aktives Fenster über `zwlr_foreign_toplevel_manager_v1`; Vollbild → `grim -o`, sonst einrahmen |
| Bildschirm | `grim` nimmt alle Ausgaben in ein Bild auf |

Vor der Aufnahme wird das Auswahlfenster verborgen; die Aufnahme beginnt erst
nach dem `unmap`-Signal plus einer kurzen Karenz für das nächste Compositor-Bild.

## Speichern und Zwischenablage

- Dialog „Bildschirmfoto speichern“: Vorschau, Name, Ordner. Ziel: `$XDG_PICTURES_DIR/Screenshots`,
  sonst `~/Screenshots`; Ordner `0700`, Dateien `0600`; nie überschreiben.
- Zwischenablage: `wl-copy --type image/png` mit den PNG-Bytes auf stdin (ohne auf den
  Hintergrundprozess von wl-copy zu warten); ohne wl-copy bietet GTK die Daten selbst an,
  solange das Fenster offen ist.

## Voraussetzungen

`grim` (Pflicht), `slurp` (Fenster/Bereich), `wl-clipboard` (Kopieren).
Fehlende Programme werden gemeldet; nichts wird automatisch installiert.

## Bauen und prüfen

```bash
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo build --release --locked
```

Die Tests brauchen kein Display und starten keine Aufnahme; externe Programme werden
über `runner::CommandRunner` ersetzt; `render`-Tests prüfen exportierte Pixel.

## Struktur

```
src/main.rs        GtkApplication, Kommandozeile
src/cli.rs         Optionen
src/ui.rs          Auswahlfenster, Stil, Symbolthema
src/instant.rs     Sofortaufnahme, Speicherdialog
src/editor.rs      Editor: Menüs, Werkzeugleiste, Zeichenfläche, Speichern, Kopieren
src/annotation.rs  Markierungen, Rückgängig/Wiederholen (ohne GTK)
src/render.rs      Zeichnen (cairo) und Zusammenführen zu PNG
src/toplevel.rs    aktives Fenster (wlr-foreign-toplevel)
src/capture.rs     Bereichsauswahl (slurp) und Aufnahme (grim)
src/geometry.rs    Region: Parsen/Validieren von `x,y wxh`
src/runner.rs      CommandRunner (System + Test-Attrappe), keine Shell
src/screenshot.rs  Png: Signaturprüfung, Größenlimit, Abmessungen
src/storage.rs     Zielordner, Dateinamen, privates Speichern ohne Überschreiben
src/clipboard.rs   wl-copy mit image/png
src/style.css      helle, kompakte, klassische Optik
```

Lizenz: MIT (siehe `LICENSE`). Symbole: GNOME Human, wenn installiert und nur GTKs
Standardthema eingestellt ist (wie Panel und Desktop); Werkzeugsymbole sind gezeichnet.
