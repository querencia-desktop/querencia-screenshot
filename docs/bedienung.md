# Querencia Screenshot – Bedienung, Aufnahme, Sprachen, Aufbau

Ausführliche Angaben, aus dem README ausgelagert (20.10.2026).
Einstieg und Stand: [README](../README.md).

## Bedienung

| Aufruf | Verhalten |
|---|---|
| `querencia-screenshot` (Druck) | Auswahlfenster „Was möchten Sie aufnehmen?“: Gesamten Bildschirm aufnehmen, Fenster einrahmen, Bereich auswählen |
| `--window` (Alt+Druck) | Fensterwahl: erkannte Fenster werden beim Darüberfahren hervorgehoben, Klick nimmt das Fenster samt Titelleiste und Rahmen auf, Ziehen außerhalb erkannter Fenster rahmt frei ein, Esc bricht ab; aktives Vollbildfenster exakt; ohne aktives Fenster ganzer Bildschirm |
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
| Neue Aufnahme / Fenster schließen | Strg+N / Strg+W oder Strg+Q |
| laufende Zeichnung abbrechen | Esc |
| Farbpalette öffnen (Pfeiltasten, Eingabe-/Leertaste, Esc) | F |

Farben: eine Farbschaltfläche in der Werkzeugleiste zeigt die aktuelle Farbe
und klappt die Palette auf – acht Grundfarben in zwei Reihen (Rot =
Standard, Orange, Gelb, Grün, Blau, Violett, Schwarz, Weiß) und
„Benutzerdefinierte Farbe …“ (kleines Fenster mit dem GTK-Farbwähler,
Eingabe „#RRGGBB“). Die Farbe gilt für neue Markierungen aller Werkzeuge;
vorhandene behalten ihre Farbe. Stärken Dünn, Mittel (Standard), Dick. „Alle Markierungen entfernen“ fragt nach und ist rückgängig
machbar. Ungespeicherte Markierungen – auch ein angefangener, noch nicht mit
Eingabe übernommener Text – lösen beim Schließen (Titelleiste,
Datei → Schließen, Strg+W, Strg+Q) eine Abfrage aus: „Ohne Speichern
schließen“, „Abbrechen“, „Speichern …“ (wie Querencia Editor). Strg+Q schließt
nur das eigene Editorfenster, nicht andere offene Aufnahmen; im
Auswahlfenster schließen Esc, Strg+W und Strg+Q.

Die Aufnahme bleibt unverändert; Markierungen sind eigene Objekte in
Originalpixeln. Erst Speichern und Kopieren erzeugen daraus ein neues PNG in
Originalgröße (`render::compose`).

## Aufnahme

| Art | Umsetzung |
|---|---|
| Bereich | `slurp` liefert `x,y wxh`, `grim -g` nimmt nur diesen Bereich auf |
| Fenster (Alt+Druck, „Fenster einrahmen“) | Vollbild → `grim -o`; sonst Bildschirm mit `grim` einfrieren, labwc-Fensterrahmen im Bild erkennen (`window_pick`), Rechtecke an `slurp` (stdin) zum Hervorheben/Anklicken; Ausschnitt aus dem eingefrorenen Bild. Beginnt ein Zug innerhalb eines erkannten Fensters, liefert slurp dieses Fenster; freies Einrahmen von Fensterteilen über „Bereich auswählen“ |
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

## Sprachen

Seit 0.2.7 sind die Texte der Oberfläche englisch im Code (msgid) und deutsch
in `po/de.po`, über GNU gettext wie bei GNOME und Debian (Muster: Querencia
Editor 0.2.0). Der deutsche Wortlaut ist der bisherige. Übersetzt sind
Auswahlfenster, Editor (Menüs, Werkzeugleiste, Statuszeile, Rückfragen),
Speicher- und Ordnerdialog, Fehlermeldungen und die Desktop-Datei
(`data/org.querencia.Screenshot.desktop.in` → `…desktop` mit `[de]`). Nicht
übersetzt: der Programmname im Fenstertitel, die Kommandozeilenhilfe
(`--help`, vor dem GTK-Start), reine Protokollzeilen auf stdout/stderr,
Dateinamen (`Screenshot-…png`, `…-markiert.png`). Statusmeldungen des Editors
und Namen von Farben und Werkzeugen stehen im Protokoll in der Sprache der
Oberfläche.

- Sprachwahl über `LANGUAGE`, `LC_ALL`, `LC_MESSAGES`, `LANG`; ohne deutschen
  Katalog erscheint Englisch. `LANGUAGE` wirkt nicht bei der Locale `C`.
- Kataloge `<lang>/LC_MESSAGES/querencia-screenshot.mo`: zuerst
  `<Ordner der Binary>/share/locale`, dann `<Ordner der Binary>/../share/locale`,
  zuletzt `/usr/share/locale`; `QUERENCIA_SCREENSHOT_LOCALEDIR` setzt den Ordner.
- Kürzel im Menü in der Sprache der Oberfläche („Strg+N“ / „Ctrl+N“): GTK
  beschriftet sie über seinen eigenen Katalog (gtk40), der auf Querencia
  fehlt; das Programm ersetzt die Tastennamen deshalb beim Öffnen eines Menüs
  über den eigenen Katalog (Kontext `keyboard label`, seit 0.2.8).
- Zugriffstasten je Sprache: deutsch wie bisher (Alt+N/O/Z/A/S im
  Speicherdialog), englisch nach GNOME 2 (Alt+N/F/L/C/S).
- po-Pflege mit `werkzeug/i18n.py` (`pot`, `update`, `desktop`, `mo <ziel>`,
  `pruefen`), wie im Editor; `.mo` inhaltsgleich mit GNU `msgfmt`.

## Struktur

```
src/main.rs        GtkApplication, Kommandozeile
src/i18n.rs        gettext: Textdomäne, Katalogsuche, Platzhalter, Plural
src/cli.rs         Optionen
src/ui.rs          Auswahlfenster, Stil, Symbolthema
src/instant.rs     Sofortaufnahme, Speicherdialog
src/editor.rs      Editor: Menüs, Werkzeugleiste, Zeichenfläche, Speichern, Kopieren
src/annotation.rs  Markierungen, Rückgängig/Wiederholen (ohne GTK)
src/render.rs      Zeichnen (cairo) und Zusammenführen zu PNG
src/toplevel.rs    Fenster und Ausgaben (wlr-foreign-toplevel, wlr-output-management)
src/window_pick.rs Fensterwahl: Rahmenerkennung, Rechtecke für slurp
src/capture.rs     Bereichsauswahl (slurp) und Aufnahme (grim)
src/geometry.rs    Region: Parsen/Validieren von `x,y wxh`
src/runner.rs      CommandRunner (System + Test-Attrappe), keine Shell
src/screenshot.rs  Png: Signaturprüfung, Größenlimit, Abmessungen
src/storage.rs     Zielordner, Dateinamen, privates Speichern ohne Überschreiben
src/clipboard.rs   wl-copy mit image/png
src/style.css      helle, kompakte, klassische Optik
po/                querencia-screenshot.pot, de.po, LINGUAS
werkzeug/i18n.py   po-Pflege (pot, update, desktop, mo, pruefen)
data/              Desktop-Vorlage (.in, englisch), erzeugte Desktop-Datei, MetaInfo
```

Symbole: GNOME Human, wenn installiert und nur GTKs Standardthema eingestellt
ist (wie Panel und Desktop); Werkzeugsymbole sind gezeichnet. Die MetaInfo für
Softwarekataloge liegt in `data/org.querencia.Screenshot.metainfo.xml`.
Änderungen: [CHANGELOG.md](../CHANGELOG.md).
