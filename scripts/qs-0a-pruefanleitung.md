# QS-0a – Manuelle GUI-Abnahme: Anleitung

Prüfobjekt: Querencia Screenshot, Entwicklungsbuild `target/debug/querencia-screenshot`
aus `~/projects/querencia-screenshot-dev` (Rust 1.98.1).

## Regeln

- Nur der Besitzer in der laufenden Querencia-Sitzung (labwc, Wayland); kein `sudo`,
  keine Installation durch das Werkzeug, keine Änderung globaler Einstellungen.
- Das Startskript legt `~/qs-0a-gui-test/` an (Marker `.qs-0a-gui-test`) und leitet
  `XDG_*` dorthin um: Screenshots landen unter `~/qs-0a-gui-test/Bilder/Screenshots/`,
  das Symbolthema ist `gnome-human` (Profil-`settings.ini`, nur für diesen Start).
- Vorhandene Dateien werden nie überschrieben; das Werkzeug zählt bei Namenskollision
  hoch (`-2`, `-3`, …) und legt Dateien mit Rechten `0600` an.
- Aufgenommene Bilder bleiben lokal; nichts wird hochgeladen.

## Voraussetzungen

| Programm | Zweck | Stand auf der VM (2026-10-15) |
|---|---|---|
| `grim` | Aufnahme | vorhanden (1.4.0) |
| `slurp` | Rechteckauswahl (Modi „Rechteck“, „Fenster (per Rechteck)“) | **fehlt** – Paket `slurp` |
| `wl-copy` | Zwischenablage `image/png` | **fehlt** – Paket `wl-clipboard` (ohne wl-copy kopiert GTK selbst, gültig nur solange das Fenster offen ist) |
| `satty` | optional, spätere Annotation | fehlt, nicht benötigt |

Fehlende Programme meldet das Werkzeug beim Start in der Statuszeile; der Modus
„Bildschirm“ funktioniert mit grim allein.

## Start

```bash
~/projects/querencia-screenshot-dev/scripts/qs-0a-gui-start.sh
```
Log: `~/qs-0a-gui-test/logs/querencia-screenshot-<Zeitstempel>.log` (enthält keine Bilddaten).

## Prüfablauf (Ergebnis je Punkt OK / FEHLER / N.A. notieren)

1. Anwendung starten – Fenster „Querencia Screenshot“, hell, kompakt.
2. Auswahlfenster zeigt die Knöpfe Rechteck · Fenster (per Rechteck) · Bildschirm, darunter Hinweis
   zur Fensteraufnahme; ggf. Statuszeile „Fehlende Programme: …“.
3. „Rechteck“ klicken – das Fenster verschwindet, slurp startet (Fadenkreuz).
4. Bereich mit der Maus aufziehen – das Fenster erscheint wieder mit Vorschaubild und Größe.
5. „Speichern“ – Statuszeile „Gespeichert: …/Bilder/Screenshots/Screenshot-JJJJ-MM-TT-HHMMSS.png“.
6. Datei in Querencia Files/Bildbetrachter öffnen: Inhalt entspricht dem Bereich.
7. „Kopieren“ – Statuszeile „In die Zwischenablage kopiert (image/png)“ (bzw. GTK-Hinweis ohne wl-copy).
8. In eine GTK-Anwendung einfügen (z. B. Bild-Upload-Feld oder ein Editor mit Bildunterstützung).
9. „Bildschirm“ – gesamter Bildschirm (alle Ausgaben) wird aufgenommen, Vorschau erscheint.
10. „Rechteck“, dann Escape in slurp – Fenster erscheint mit „Abgebrochen – keine Aufnahme.“, keine Datei entsteht.
11. „Fenster (per Rechteck)“ – wie Rechteck; Hinweistext bestätigt, dass keine Fenstergrenzen erkannt werden.
12. In den gespeicherten Bildern aus 5 und 9 darf das Querencia-Screenshot-Fenster **nicht** sichtbar sein.
13. Escape im Hauptfenster: mit Aufnahme → verwerfen; ohne Aufnahme → Fenster schließt.
14. Zweimal hintereinander speichern – zweite Datei bekommt `-2`.

Bekannte Grenzen: keine Fenstererkennung unter labwc (nur Rechteck); Aufnahme mit mehreren
Monitoren erfasst bei „Bildschirm“ alle Ausgaben in einem Bild (grim-Standard); Mauszeiger ist nicht
enthalten (grim ohne `-c`).

Alle Punkte gelten bis zur Abnahme als **ungetestet**.
