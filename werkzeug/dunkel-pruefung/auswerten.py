#!/usr/bin/env python3
"""Querencia Screenshot 0.2.10: Auswertung der Prüfung hell/dunkel (QD-109).

Liest die Fotos eines Laufs von lauf.sh (Auswahlfenster bei 0,0, Editor bei
560,0, Ausgang 1600x1000) und prüft
- Farbproben (Fenstergrund, Wahlknopf, Überfahren, Menüleiste, Werkzeugleiste,
  Statuszeile, Menü, Farb- und Stärken-Popover, Textfeld) gegen die Werte
  hell bzw. dunkel; Satz a/c im Startschema, b im anderen;
- die Tinte der gezeichneten Symbole (Stift, Klapppfeile): hell #303030,
  dunkel die Schriftfarbe des Themas #dfdbd2;
- den grauen Grund hinter dem Bild (#8c8c8c) unverändert;
- Satz c pixelgleich zu Satz a (ohne die Fokusmarken in FOKUS), keine
  GTK-Meldungen außer Portal-Hinweisen.

Aufruf:  auswerten.py <lauf-ordner> [<vergleichs-lauf> …]
"""

import os
import sys

import gi

gi.require_version("GdkPixbuf", "2.0")
from gi.repository import GdkPixbuf  # noqa: E402

# gemessene Sollwerte (Thema Querencia-Klassisch; Mischungen aus style.css)
SOLL = {
    "hell": {"grund": "f2f1f0", "knopf": "e7e7e6", "ueber": "f1f1f0", "leiste": "efeeed",
             "menue": "f5f4f2", "grau": "8c8c8c", "tinte": "303030"},
    "dunkel": {"grund": "3c3b37", "knopf": "464440", "ueber": "504f4a", "leiste": "3f3e3a",
               "menue": "353430", "grau": "8c8c8c", "tinte": "dfdbd2"},
}
# (foto, was, x, y, art, verschiebt-sich-im-englischen)
PROBEN = [
    ("01-fenster", "Auswahlfenster Grund", 450, 40, "grund", False),
    ("01-fenster", "Wahlknopf", 500, 80, "knopf", False),
    ("02-auswahl-ueber", "Wahlknopf überfahren", 500, 80, "ueber", False),
    ("01-fenster", "Editor Menüleiste", 1160, 38, "leiste", False),
    ("01-fenster", "Editor Werkzeugleiste", 1450, 60, "leiste", False),
    ("01-fenster", "Editor Statuszeile", 1456, 569, "leiste", False),
    ("01-fenster", "Knopf „Neu“", 570, 80, "knopf", False),
    ("01-fenster", "grauer Grund hinter dem Bild", 600, 300, "grau", False),
    ("03-menue-datei", "Datei-Menü", 575, 100, "menue", False),
    ("04-palette", "Farb-Popover", {"de": 1290, "en": 1255}, 160, "menue", False),
    ("05-staerke", "Stärken-Popover", {"de": 1330, "en": 1263}, 105, "menue", False),
    ("06-text", "Textfeld über dem Bild", 1157, 380, "menue", False),
]
# Kästen, in denen die Tinte vorkommen muss (Stift-Symbol, Klapppfeil Farbe/Stärke)
TINTE = [
    ("Stift-Symbol", (1028, 60, 1052, 82)),
    ("Klapppfeil Farbe", (1262, 64, 1272, 78)),
    ("Klapppfeil Stärke", (1316, 64, 1330, 78)),
]
DX = {"de": 0, "en": -67}
# Fokusmarken (gepunkteter Rahmen bzw. Fokusring), die GTK je nach
# vorheriger Eingabe zeigt oder nicht: beim Vergleich a/c ausgenommen und als
# Hinweis gezählt (Farbfläche „Rot“ im Farb-Popover, Regler im Stärken-Popover)
FOKUS = {
    "04-palette": {"de": (1130, 88, 1182, 124), "en": (1109, 88, 1161, 124)},
    "05-staerke": {"de": (1176, 112, 1386, 160), "en": (1106, 112, 1320, 160)},
}


def bild(pfad):
    pb = GdkPixbuf.Pixbuf.new_from_file(pfad)
    return pb.get_width(), pb.get_height(), pb.get_n_channels(), pb.get_rowstride(), pb.get_pixels()


def pixel(b, x, y):
    _, _, n, rs, d = b
    o = y * rs + x * n
    return "%02x%02x%02x" % (d[o], d[o + 1], d[o + 2])


def zaehlen(b, kasten, farbe):
    x0, y0, x1, y1 = kasten
    return sum(1 for y in range(y0, y1) for x in range(x0, x1) if pixel(b, x, y) == farbe)


def abweichung(a, b, maske=(0, 0, -1, -1)):
    """Zahl und Rahmen der abweichenden Pixel außerhalb der Maske, dazu die
    Zahl in der Maske."""
    wa, ha, na, ra, da = a
    wb, hb, nb, rb, db = b
    if (wa, ha) != (wb, hb):
        return -1, None, 0
    zahl, maskiert, x0, y0, x1, y1 = 0, 0, wa, ha, -1, -1
    mx0, my0, mx1, my1 = maske
    for y in range(ha):
        za = da[y * ra:y * ra + wa * na]
        zb = db[y * rb:y * rb + wb * nb]
        if za == zb:
            continue
        for x in range(wa):
            if za[x * na:x * na + 3] != zb[x * nb:x * nb + 3]:
                if mx0 <= x <= mx1 and my0 <= y <= my1:
                    maskiert += 1
                    continue
                zahl += 1
                x0, y0, x1, y1 = min(x0, x), min(y0, y), max(x1, x), max(y1, y)
    return zahl, (x0, y0, x1, y1) if zahl else None, maskiert


def umgebung(lauf):
    werte = {}
    with open(os.path.join(lauf, "out", "umgebung.txt")) as f:
        for teil in f.read().replace(";", "\n").split("\n"):
            teil = teil.strip()
            for k in ("Schema", "Sprache"):
                if teil.startswith(k + " "):
                    werte[k] = teil.split()[1]
    return werte


def main():
    lauf = sys.argv[1]
    out = os.path.join(lauf, "out")
    u = umgebung(lauf)
    start, dx = u["Schema"], DX[u["Sprache"]]
    anders = "dunkel" if start == "hell" else "hell"
    ergebnisse = []

    def ergebnis(ok, text):
        ergebnisse.append(("PASS" if ok else "FAIL") + " " + text)

    for satz, schema in (("a", start), ("b", anders), ("c", start)):
        for foto, was, x, y, art, schiebt in PROBEN:
            x = x[u["Sprache"]] if isinstance(x, dict) else (x + dx if schiebt else x)
            ist = pixel(bild(os.path.join(out, f"{satz}-{foto}.png")), x, y)
            soll = SOLL[schema][art]
            ergebnis(ist == soll, f"{satz} ({schema}) {was} bei {x},{y}: #{ist} (soll #{soll})")
        b = bild(os.path.join(out, f"{satz}-01-fenster.png"))
        for was, (x0, y0, x1, y1) in TINTE:
            kasten = (x0 + dx, y0, x1 + dx, y1)
            n = zaehlen(b, kasten, SOLL[schema]["tinte"])
            ergebnis(n >= 4, f"{satz} ({schema}) {was}: {n} Pixel in Tinte #{SOLL[schema]['tinte']}")
    for name in sorted(os.listdir(out)):
        if name.startswith("a-") and name.endswith(".png"):
            c = name.replace("a-", "c-", 1)
            foto = name[2:-4]
            maske = FOKUS.get(foto, {}).get(u["Sprache"], (0, 0, -1, -1))
            zahl, rahmen, m = abweichung(bild(os.path.join(out, name)), bild(os.path.join(out, c)), maske)
            ergebnis(zahl == 0, f"zurückgeschaltet {c} = {name}" + ("" if zahl == 0 else f" ({zahl} Pixel, {rahmen})")
                     + (f" [Fokusmarke: {m} Pixel]" if m else ""))
    with open(os.path.join(out, "gtk-meldungen.txt")) as f:
        n = int(f.read().strip() or "0")
    ergebnis(n == 0, f"GTK-Meldungen außer Portal-Hinweisen: {n}")
    for vergleich in sys.argv[2:]:
        vout = os.path.join(vergleich, "out")
        for name in sorted(os.listdir(out)):
            if not (name.startswith("a-") and name.endswith(".png")) or not os.path.exists(os.path.join(vout, name)):
                continue
            zahl, rahmen, _ = abweichung(bild(os.path.join(out, name)), bild(os.path.join(vout, name)))
            ergebnisse.append(f"VERGLEICH {name} gegen {os.path.basename(vergleich)}: "
                              + ("pixelgleich" if zahl == 0 else f"{zahl} Pixel anders, Rahmen {rahmen}"))
    text = "\n".join(ergebnisse) + "\n"
    with open(os.path.join(out, "auswertung.txt"), "w") as f:
        f.write(text)
    print(text, end="")
    return 0 if not any(z.startswith("FAIL") for z in ergebnisse) else 1


if __name__ == "__main__":
    sys.exit(main())
