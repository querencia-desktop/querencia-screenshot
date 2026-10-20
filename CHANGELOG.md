# Änderungsprotokoll

Format nach [Keep a Changelog](https://keepachangelog.com/de/1.1.0/), Versionen
nach [Semantic Versioning](https://semver.org/lang/de/). Solange die Hauptversion
0 ist, kann sich alles ändern. Jede Zeile nennt, was Nutzer merken.

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
- Lizenzkopf in `LICENSE` (Copyright „Querencia Desktop contributors“,
  ENTSCHEIDUNGEN Nr. 14 Q5).
- MetaInfo: Version 0.2.7, Homepage auf das geplante Repo in der
  Organisation `querencia-desktop` statt des privaten `classic2`.

## [0.2.6] – 2026-10-19

Ab `main` (0.2.4); die Fensterwahl aus 0.2.5 (Zweig `fensterwahl`) ist nicht
enthalten.

### Geändert
- **Lizenz: GPL-3.0-or-later statt MIT** (`LICENSE`, `Cargo.toml`,
  SPDX-Köpfe der Quelldateien, MetaInfo `project_license`), einheitlich mit
  den übrigen Querencia-Programmen. Copyright „Querencia Desktop
  contributors“ im README.
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
