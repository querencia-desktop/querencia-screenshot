#!/bin/bash
# Querencia Screenshot 0.2.10: ein isolierter Lauf der Prüfung hell/dunkel
# (QD-109), Aufbau wie Terminal 0.3.1 (tools/dunkel-pruefung): eigenes HOME,
# XDG_RUNTIME_DIR, Bus und labwc (isoliert.py), Farbschema über die gsettings
# des Profils, die portal-ersatz.py wie das echte Settings-Portal an GTK
# meldet. GTK-Thema: die Live-Ablage Querencia-Klassisch (nur gelesen, als
# Link); Symbolthemen aus einem einmaligen Schnappschuss. Das Profil muss neu
# sein (die Prüfung löscht nichts).
#   lauf.sh <ablage-binary> <name> hell|dunkel de|en [BxH] [zeitlimit]
# Fensterlage fest: Auswahlfenster bei 0,0, Editor bei 560,0 (1600x1000).
# Ergebnis und Fotos: <gui-test>/<name>/out/
set -euo pipefail
HIER=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
BIN=$(realpath -e "${1:?Binary}"); NAME=${2:?Name}; SCHEMA=${3:?hell|dunkel}; SPRACHE=${4:?de|en}
RES=${5:-1600x1000}; T=${6:-300}
case $SCHEMA in hell | dunkel) ;; *) echo "Schema hell|dunkel" >&2; exit 2 ;; esac
case $SPRACHE in de | en) ;; *) echo "Sprache de|en" >&2; exit 2 ;; esac
case $NAME in '' | .* | */* | *[!A-Za-z0-9._-]*) echo "unzulässiger Name: $NAME" >&2; exit 2 ;; esac
G=$HOME/projects/querencia-screenshot-0210-pruefung/gui-test
P=$G/$NAME
[ ! -e "$P" ] || { echo "Profil $P gibt es schon (die Prüfung löscht nichts)" >&2; exit 2; }
mkdir -p "$G"
QB=$HOME/projects/querencia-terminal-gui-test/bin
[ -x "$QB/qeingabe" ] && [ -x "$QB/qausgabe" ] || { echo "qeingabe/qausgabe fehlen in $QB" >&2; exit 2; }
S=$G/schnappschuss-symbole
if [ ! -f "$S/zeitpunkt.txt" ]; then
    mkdir -p "$S"
    cp -a $HOME/.local/share/icons/Querencia $HOME/.local/share/icons/hicolor "$S/"
    date '+%F %T' >"$S/zeitpunkt.txt"
fi
THEMA=$(readlink -f $HOME/.local/share/themes/Querencia-Klassisch)
[ -f "$THEMA/gtk-4.0/gtk-dark.css" ] || { echo "Thema ohne Dunkelfassung: $THEMA" >&2; exit 2; }
mkdir -p "$P"/{config/labwc,config/gtk-4.0,data/themes,data/icons,state,logs,out,home/Bilder}
cat >"$P/config/labwc/rc.xml" <<X
<?xml version="1.0"?>
<labwc_config>
  <core><decoration>server</decoration></core>
  <theme><name>Querencia-Klassisch</name></theme>
  <placement><policy>automatic</policy></placement>
  <windowRules>
    <windowRule identifier="org.querencia.Screenshot" title="Querencia Screenshot"><action name="MoveTo" x="0" y="0"/></windowRule>
    <windowRule identifier="org.querencia.Screenshot" title="Querencia Screenshot – *"><action name="MoveTo" x="560" y="0"/></windowRule>
  </windowRules>
</labwc_config>
X
ln -s "$THEMA" "$P/data/themes/Querencia-Klassisch"
cp -a "$S/Querencia" "$S/hicolor" "$P/data/icons/"
schrift=$(gsettings get org.gnome.desktop.interface font-name | tr -d "'")
printf '[Settings]\ngtk-font-name=%s\ngtk-icon-theme-name=Querencia\ngtk-theme-name=Querencia-Klassisch\n' "$schrift" >"$P/config/gtk-4.0/settings.ini"
printf 'XDG_PICTURES_DIR="$HOME/Bilder"\n' >"$P/config/user-dirs.dirs"
# Testbild (fest): Farbflächen und Verlauf, 640x400
python3 -I - "$P/home/testbild.png" <<'PY'
import sys, gi
gi.require_version("GdkPixbuf", "2.0")
from gi.repository import GdkPixbuf, GLib
w, h = 640, 400
d = bytearray()
for y in range(h):
    for x in range(w):
        if y < 120:
            d += bytes(((x * 255) // w, (y * 2) % 256, 160))
        elif x < 320:
            d += bytes((240, 119, 70))
        else:
            d += bytes((250, 250, 250)) if (x // 20 + y // 20) % 2 else bytes((60, 60, 60))
pb = GdkPixbuf.Pixbuf.new_from_bytes(GLib.Bytes.new(bytes(d)), GdkPixbuf.Colorspace.RGB, False, 8, w, h, w * 3)
pb.savev(sys.argv[1], "png", [], [])
PY
cat >"$P/env.sh" <<X
W=$HIER
Q=$QB/qeingabe
QA=$QB/qausgabe
OUT=$P/out
RES=$RES
SCHEMA=$SCHEMA
SPRACHE=$SPRACHE
BIN=$BIN
SCHRIFT='$schrift'
TESTBILD=$P/home/testbild.png
export GSK_RENDERER=cairo GDK_DISABLE=gl,vulkan LANG=en_US.UTF-8 LANGUAGE=$SPRACHE
X
{
    echo "Binary $BIN $(sha256sum "$BIN" | cut -d' ' -f1)"
    echo "Thema $THEMA; Symbole $(cat "$S/zeitpunkt.txt"); Schema $SCHEMA; Sprache $SPRACHE; Ausgang $RES"
} >"$P/out/umgebung.txt"
QTD_PORTAL=1 python3 -I "$HIER/isoliert.py" "$P" "$HIER/${QSD_SZENARIO:-szenario.sh}" "$T" || true
cat "$P/out/ergebnis.txt" 2>/dev/null || echo "kein Ergebnis" >&2
