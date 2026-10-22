#!/bin/bash
# Querencia Screenshot 0.2.10, Prüfung hell/dunkel: läuft IN der isolierten
# Sitzung (lauf.sh). Auswahlfenster und Editor (Testbild über --open); Satz a
# im Startschema, Wechsel per gsettings im laufenden Programm (Satz b),
# zurück (Satz c = a).
set -u
. "$HMP04_PROFILE/env.sh"
LOG=$OUT/szenario.log
exec >>"$LOG" 2>&1
B=${RES%x*}; H=${RES#*x}
ERG=$OUT/ergebnis.txt
: >"$ERG"
ergebnis() { local id=$1; shift; if "$@"; then echo "PASS $id" >>"$ERG"; else echo "FAIL $id ($*)" >>"$ERG"; fi; }
notiz() { echo "INFO $*" >>"$ERG"; }
schema_setzen() {
    case $1 in dunkel) gsettings set org.gnome.desktop.interface color-scheme prefer-dark ;;
    *) gsettings set org.gnome.desktop.interface color-scheme default ;; esac
    echo "$(date +%T.%N | cut -c1-12) color-scheme $(gsettings get org.gnome.desktop.interface color-scheme)"
}
gsettings set org.gnome.desktop.interface font-name "$SCHRIFT"
gsettings set org.gnome.desktop.interface gtk-theme Querencia-Klassisch
gsettings set org.gnome.desktop.interface icon-theme Querencia
gsettings set org.gnome.desktop.interface cursor-blink false
schema_setzen "$SCHEMA"
python3 -I "$W/portal-ersatz.py" "$HMP04_PROFILE/logs/portal-bereit" >>"$OUT/portal.log" 2>&1 &
for i in $(seq 50); do [ -f "$HMP04_PROFILE/logs/portal-bereit" ] && break; sleep 0.1; done
"$QA" "$RES"; sleep 1
. "$W/eingabe.sh"
q "extent $B $H"
foto() { grim "$OUT/$1.png"; echo "$(date +%T.%N | cut -c1-12) Foto $1"; }
weg() { q "move $((B - 2)) $((H - 2))" "wait 600"; }
# Editor bei 560,0: Lage seiner Teile (gemessen im Erkundungslauf)
EX=560
# Englisch: kürzere Knöpfe „New/Save/Copy“, alles rechts davon 67 px weiter links
case $SPRACHE in en) DATEI_X=$((EX + 24)); DX=-67 ;; *) DATEI_X=$((EX + 31)); DX=0 ;; esac
MENUE_Y=38; LEISTE_Y=70
PALETTE_X=$((EX + 693 + DX)); STAERKE_X=$((EX + 747 + DX)); TEXT_X=$((EX + 640 + DX)); STIFT_X=$((EX + 479 + DX))
BILD_X=$((EX + 300)); BILD_Y=300
"$BIN" >>"$OUT/programm.log" 2>&1 &
APID=$!
q "wait 3000"
"$BIN" --open "$TESTBILD" >>"$OUT/programm.log" 2>&1 &
q "wait 3000"
ergebnis "Auswahlfenster und Editor in einem Prozess" kill -0 "$APID"
satz() {
    local p=$1
    echo "--- Satz $p ($(gsettings get org.gnome.desktop.interface color-scheme))"
    weg; foto "$p-01-fenster"
    q "move 470 100" "wait 800"; foto "$p-02-auswahl-ueber"; weg
    q "move $DATEI_X $MENUE_Y" "wait 400" "click" "wait 1000"; weg; foto "$p-03-menue-datei"
    q "key Escape" "wait 400" "key Escape" "wait 800"
    q "move $PALETTE_X $LEISTE_Y" "wait 400" "click" "wait 1200"; weg; foto "$p-04-palette"
    q "key Escape" "wait 800"
    q "move $STAERKE_X $LEISTE_Y" "wait 400" "click" "wait 1200"; weg; foto "$p-05-staerke"
    q "key Escape" "wait 800"
    q "move $TEXT_X $LEISTE_Y" "wait 400" "click" "wait 600" "move $BILD_X $BILD_Y" "wait 400" "click" "wait 1200"; weg; foto "$p-06-text"
    q "key Escape" "wait 800"
    # zurück zum Stift (Taste P bzw. Werkzeug 1), dann Ruhebild
    q "move $STIFT_X $LEISTE_Y" "wait 400" "click" "wait 600"; weg; foto "$p-07-ende"
}
satz a
case $SCHEMA in hell) ANDERS=dunkel ;; *) ANDERS=hell ;; esac
schema_setzen "$ANDERS"; q "wait 3000"
satz b
schema_setzen "$SCHEMA"; q "wait 3000"
satz c
ergebnis "Programm läuft bis zum Ende" kill -0 "$APID"
kill "$APID" 2>/dev/null; wait "$APID" 2>/dev/null
# GTK-Meldungen ohne die erwarteten Portal-Hinweisen (der Ersatz bietet nur
# Settings an)
grep -i -E "gtk-(warning|critical)|gdk-(warning|critical)" "$OUT/programm.log" | grep -v -c "Cannot get portal" >"$OUT/gtk-meldungen.txt"
touch "$HMP04_PROFILE/logs/FERTIG"; sleep 30
