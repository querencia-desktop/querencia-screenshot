# Eine einzige, dauerhafte qeingabe-Sitzung (virtuelle Tastatur bleibt
# „eingesteckt“). Jeder neue qeingabe-Prozess steckt sonst eine Tastatur
# an und ab; GTK 4.22 stürzt nach einem solchen Wechsel bei geschlossenem
# Dialog ab (isoliert belegt, Bericht Abschnitt „Befund Tastatur“).
#   . eingabe.sh ; q "key a" "chord ctrl s" …   (wartet, bis alles ausgeführt ist)
coproc QE { "$Q"; }
q() {
    local c
    for c in "$@"; do printf '%s\n' "$c" >&"${QE[1]}"; done
    printf 'mark SYNC\n' >&"${QE[1]}"
    local line
    while IFS= read -r -t 30 line <&"${QE[0]}"; do
        [[ $line == *SYNC* ]] && return 0
    done
    echo "qeingabe antwortet nicht" >&2; return 1
}
