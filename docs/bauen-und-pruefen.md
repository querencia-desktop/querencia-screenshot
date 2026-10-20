# Querencia Screenshot – Bauen und Prüfen

Aus dem README ausgelagert (DOKU-1, 20.10.2026). GUI-Prüfung nur isoliert
(eigener labwc ohne Bildschirm, eigenes `HOME` und `XDG_RUNTIME_DIR`), nie in
der laufenden Sitzung; Prüfanleitung der frühen Fassung:
[qs-0a-pruefanleitung.md](qs-0a-pruefanleitung.md).

```bash
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo build --release --locked
python3 werkzeug/i18n.py pruefen     # Katalog vollständig, .pot aktuell
```

Die Tests brauchen kein Display und starten keine Aufnahme; externe Programme werden
über `runner::CommandRunner` ersetzt; `render`-Tests prüfen exportierte Pixel.
Ohne gebundenen Katalog laufen sie englisch; `german_catalog_is_complete`
prüft, dass jede msgid in `po/de.po` übersetzt ist.
