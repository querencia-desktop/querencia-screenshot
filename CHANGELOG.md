# Änderungsprotokoll

Format nach [Keep a Changelog](https://keepachangelog.com/de/1.1.0/), Versionen
nach [Semantic Versioning](https://semver.org/lang/de/). Solange die Hauptversion
0 ist, kann sich alles ändern. Jede Zeile nennt, was Nutzer merken.

## [0.2.9] – 2026-10-21

Ab 0.2.8 (Tag `screenshot/0.2.8`). Behebt QD-076 für Screenshot.

### Behoben
- Das Auswahlfenster „Was möchten Sie aufnehmen?“ hat nach einer mit Esc
  abgebrochenen (oder fehlgeschlagenen) Aufnahme wieder seine Titelleiste.
  Bisher kam es ohne Titelleiste zurück: labwc 0.9.8 verknüpft die
  Fensterdekoration nur beim ersten Zeigen eines Fensters (QD-076). Das
  Fenster wird jetzt nach dem Abbruch neu gebaut statt wieder gezeigt, mit
  der Meldung „Abgebrochen – keine Aufnahme.“ wie bisher.
- Druck während einer laufenden Aufnahme holt das verborgene Auswahlfenster
  nicht mehr vorzeitig zurück (es stand sonst im Bild); es kommt wie gewohnt
  nach dem Ende der Aufnahme.

## [0.2.8] – 2026-10-20

0.2.7 + Fensterwahl aus 0.2.5 (Zweig `fensterwahl`, ADR-004 „so
übernehmen“: beide Benutzerpfade, Ziehverhalten Option 1, kein Umschalter).

### Hinzugefügt
- **Fensterwahl** (Alt+Druck und „Fenster einrahmen“): Der Bildschirm wird
  eingefroren, Fenster mit labwc-Rahmen werden im Bild erkannt und beim
  Darüberfahren hervorgehoben; ein Klick nimmt das Fenster samt Titelleiste
  und Rahmen auf. Außerhalb erkannter Fenster wird mit gedrückter Maustaste
  frei eingerahmt, Esc bricht ab. Ein Zug, der in einem erkannten Fenster
  beginnt, wählt dieses Fenster; Fensterteile über „Bereich auswählen“.

### Geändert
- Kürzel im Menü in der Sprache der Oberfläche: „Strg+N“ statt „Ctrl+N“ im
  Deutschen (GTKs eigener Katalog fehlt auf Querencia).
- Ein angefangener, noch nicht mit Eingabe übernommener Text zählt beim
  Schließen als ungespeicherte Änderung (Rückfrage); „Speichern …“ übernimmt
  ihn vorher ins Bild.
- Strg+W und Strg+Q schließen den Editor auch während der Texteingabe
  (bisher behielt das Eingabefeld die Kürzel, nur das X schloss).
- MetaInfo: Version 0.2.8 mit Beschreibung.

## [0.2.7] – 2026-10-20

Ab `main` (0.2.6). Die Fensterwahl aus 0.2.5 (Zweig `fensterwahl`) ist
weiterhin nicht enthalten.

### Hinzugefügt
- **Englische Oberfläche** über GNU gettext (ENTSCHEIDUNGEN Nr. 16):
  Texte englisch im Code, Deutsch aus `po/de.po` mit dem bisherigen Wortlaut.
  Die Sprache folgt `LANGUAGE`/`LANG`; ohne deutschen Katalog erscheint
  Englisch. Wortschatz nach GNOME 2 (gnome-screenshot, gedit 2.30).
- Desktop-Datei im Repo: `data/org.querencia.Screenshot.desktop` (aus der
  englischen Vorlage `.desktop.in` erzeugt, mit `Name[de]` usw.).
- `werkzeug/i18n.py` für die po-Pflege (wie im Editor).

### Geändert
- Zugriffstasten im Englischen nach GNOME 2 (Speicherdialog Alt+N/F/L/C/S,
  Menüs File/Edit/Tools/View); im Deutschen unverändert.
- Lizenzkopf (Copyright „Querencia Desktop contributors“).
- MetaInfo: Version 0.2.7, Homepage auf das geplante Repo in der
  Organisation `querencia-desktop` statt des privaten `classic2`.

## [0.2.6] – 2026-10-19

Ab `main` (0.2.4); die Fensterwahl aus 0.2.5 (Zweig `fensterwahl`) ist nicht
enthalten.

### Geändert
- **Lizenz: GPL-3.0-or-later statt MIT** (`LICENSE`, `Cargo.toml`,
  SPDX-Köpfe der Quelldateien, MetaInfo `project_license`), einheitlich mit
  den übrigen Querencia-Programmen. Copyright „Querencia Desktop
  contributors“.
- Rückfrage bei ungespeicherten Markierungen mit den Knöpfen in der
  Reihenfolge von Querencia Editor: „Ohne Speichern schließen“, „Abbrechen“,
  „Speichern …“ (Zugriffstasten unverändert Alt+O, Alt+A, Alt+S).

### Hinzugefügt
- Strg+Q schließt das Editorfenster wie Strg+W und das Titelleisten-X, mit
  derselben Rückfrage bei ungespeicherten Markierungen (QREL-05, Fall P.4).
  Andere offene Editorfenster bleiben offen.
- Strg+W und Strg+Q schließen auch das Auswahlfenster „Was möchten Sie
  aufnehmen?“ (bisher nur Esc und „Abbrechen“).
- MetaInfo `data/org.querencia.Screenshot.metainfo.xml` im Repo.

## [0.2.5] – 2026-10-18 (nur Zweig `fensterwahl`, nicht veröffentlicht)

Fensterwahl mit Rahmenerkennung (Alt+Druck, „Fenster einrahmen“); wartet auf
eine Produktentscheidung.

## [0.2.4] – 2026-10-18

- Stift als Standardwerkzeug, Strichstärken-Regler 1–20 px (Taste S).

## [0.2.3] – 2026-10-17

- Ordnerwahl im Speichern-Dialog mit GTK-Ordnerdialog im eigenen Prozess
  (Tastaturfokus, Strg+L) statt Portal.

## [0.2.2] – 2026-10-17

- Farbpalette mit Popover (8 Grundfarben) und eigenem Farbdialog,
  kompaktere Werkzeugleiste.

## [0.2.1] – 2026-10-17

- Fensterdekoration von labwc statt GTK-Kopfleiste.

## [0.2.0] – 2026-10-17

- Auswahlfenster, Editor mit Markierungswerkzeugen, Speichern,
  Zwischenablage (QS-0d).
