# Querencia Screenshot

Klassisches Screenshot-Werkzeug für Querencia Desktop (Wayland/labwc), Rust + GTK4.
Bedienung nach dem Vorbild einfacher Snipping-Werkzeuge: Aufnahmeart wählen,
aufnehmen, im Editor markieren (Kreis, Rechteck, Pfeil, Stift, Text), speichern
oder in die Zwischenablage kopieren.

> **Alpha.** Fensterwahl nur für Fenster mit labwc-Rahmen; bei Skalierung
> ungleich 1 nur Einrahmen. Keine Sicherheits- oder Datenverlustgarantie.

## Stand

| | |
|---|---|
| Version | 0.2.8 (Tag `screenshot/0.2.8`): 0.2.7 (Deutsch und Englisch über gettext) plus Fensterwahl per Klick, Menükürzel in der Sprache der Oberfläche, Rückfrage bei angefangenem Text |
| Auf der Entwicklungs-VM | live seit 20.10.2026 |
| Letzte Prüfung | 20.10.2026: 94 Unit-Tests; isolierte GUI-Prüfung 66 Fälle (691 Prüfpunkte, 0 Fehler) |
| Bekannte Lücken | Kommandozeilenhilfe und Protokollzeilen nicht übersetzt; GTK-eigene Dialoge ohne gtk40-Katalog englisch |
| Getestet auf | Ubuntu 26.04, labwc 0.9.8, GTK 4.22, Rust 1.93.1 |

Änderungen je Version: [CHANGELOG.md](CHANGELOG.md). Bedienung, Tasten,
Aufnahmearten, Sprachen und Aufbau: [docs/bedienung.md](docs/bedienung.md).

## Bauen und Prüfen

`cargo build --release --locked`, danach `cargo fmt --all --check`, `cargo clippy --all-targets --locked -- -D warnings`, `cargo test --locked` und `python3 werkzeug/i18n.py pruefen`.
Einzelheiten zu Tests und Katalog: [docs/bauen-und-pruefen.md](docs/bauen-und-pruefen.md).

## Lizenz

Programm: GPL-3.0-or-later (GNU General Public License, Version 3 oder neuer),
siehe [LICENSE](LICENSE) (reiner Lizenztext). Copyright 2026 Querencia
Desktop contributors; Urheberhinweis in [COPYRIGHT](COPYRIGHT).
Bis 0.2.4 stand das Programm unter MIT; seit 0.2.6 gilt wie für alle
Querencia-Programme die GPL. Es enthält keinen fremden Quellcode; die
Rust-Abhängigkeiten stehen unter eigenen, freizügigen Lizenzen (überwiegend
MIT/Apache-2.0).
Die MetaInfo-Daten stehen unter FSFAP.

Name und Logo „Querencia“ sind keine freien Inhalte.

