#!/usr/bin/env bash
# QS-0a GUI-Abnahme: startet Querencia Screenshot aus dem Entwicklungsbuild
# mit eigenem Log- und Ausgabeordner. Nur in der grafischen Sitzung ausführen.
# Keine globale Konfiguration wird verändert; Exitcode = Exitcode der App.
set -euo pipefail

ROOT="$HOME/qs-0a-gui-test"
REPO="${QS_REPO:-$HOME/projects/querencia-screenshot-dev}"
BIN="$REPO/target/debug/querencia-screenshot"
MARKER=".qs-0a-gui-test"
STAMP="$(date +%Y%m%d-%H%M%S)"

die() { echo "ABBRUCH: $*" >&2; exit 1; }

[[ -n "${HOME:-}" && -d "$HOME" && ! -L "$HOME" ]] || die "HOME ($HOME) ist kein reguläres Verzeichnis"
[[ ! -L "$ROOT" ]] || die "$ROOT ist ein symbolischer Link"
if [[ ! -e "$ROOT" ]]; then
  mkdir "$ROOT"
  printf 'QS-0a GUI-Testverzeichnis (Querencia Screenshot); angelegt %s\n' "$(date -Is)" > "$ROOT/$MARKER"
fi
[[ -d "$ROOT" && -f "$ROOT/$MARKER" && ! -L "$ROOT/$MARKER" ]] || die "$ROOT ist nicht als QS-0a-Testverzeichnis markiert ($MARKER fehlt)"
ROOT_REAL="$(realpath -e "$ROOT")"
[[ "$ROOT_REAL" == "$(realpath -e "$HOME")/qs-0a-gui-test" ]] || die "physischer Pfad $ROOT_REAL weicht ab"
links="$(find "$ROOT" -type l -print 2>/dev/null | head -5 || true)"
[[ -z "$links" ]] || die "symbolische Links im Testverzeichnis:"$'\n'"$links"
mkdir -p "$ROOT/logs" "$ROOT/xdg/config" "$ROOT/xdg/state" "$ROOT/xdg/cache" "$ROOT/xdg/data" "$ROOT/Bilder/Screenshots"
LOG="$ROOT/logs/querencia-screenshot-$STAMP.log"

[[ -x "$BIN" ]] || die "$BIN fehlt – erst bauen: (cd $REPO && ~/.cargo/bin/cargo +1.98.1 build)"
[[ -n "${WAYLAND_DISPLAY:-}" ]] || die "keine Wayland-Sitzung (WAYLAND_DISPLAY leer)"
if pids="$(pgrep -u "$(id -u)" -x querencia-screenshot)"; then
  die "querencia-screenshot läuft bereits (PID ${pids//$'\n'/, })"
fi

# Isolation: Screenshots landen unter $ROOT/Bilder/Screenshots (XDG-Bilderordner
# des Profils), Einstellungen/Caches im Profil; das Symbolthema wie bei QF-0d.
export XDG_CONFIG_HOME="$ROOT_REAL/xdg/config"
export XDG_STATE_HOME="$ROOT_REAL/xdg/state"
export XDG_CACHE_HOME="$ROOT_REAL/xdg/cache"
export XDG_DATA_HOME="$ROOT_REAL/xdg/data"
mkdir -p "$XDG_CONFIG_HOME/gtk-4.0"
[[ -f "$XDG_CONFIG_HOME/user-dirs.dirs" ]] || printf 'XDG_PICTURES_DIR="%s/Bilder"\n' "$ROOT_REAL" > "$XDG_CONFIG_HOME/user-dirs.dirs"
[[ -f "$XDG_CONFIG_HOME/gtk-4.0/settings.ini" ]] || printf '[Settings]\ngtk-icon-theme-name = %s\n' "${QS_ICON_THEME:-gnome-human}" > "$XDG_CONFIG_HOME/gtk-4.0/settings.ini"

{
  echo "=== QS-0a GUI-Abnahme $STAMP"
  echo "binary: $BIN ($(sha256sum "$BIN" | cut -c1-16)…)"
  echo "session: WAYLAND_DISPLAY=$WAYLAND_DISPLAY XDG_SESSION_TYPE=${XDG_SESSION_TYPE:-}"
  echo "tools: grim=$(command -v grim || echo FEHLT) slurp=$(command -v slurp || echo FEHLT) wl-copy=$(command -v wl-copy || echo FEHLT) satty=$(command -v satty || echo FEHLT)"
  echo "icon-theme (Profil): $(grep -s gtk-icon-theme-name "$XDG_CONFIG_HOME/gtk-4.0/settings.ini" || echo keine)"
  echo "pictures: $(grep -s XDG_PICTURES_DIR "$XDG_CONFIG_HOME/user-dirs.dirs" || echo keine) -> Screenshots unter $ROOT_REAL/Bilder/Screenshots"
  echo "=== start $(date -Is)"
} | tee "$LOG"
echo "Log: $LOG"

"$BIN" >>"$LOG" 2>&1 &
PID=$!
echo "PID $PID"
trap 'kill -INT "$PID" 2>/dev/null || true' INT TERM
STATUS=0
wait "$PID" || STATUS=$?
trap - INT TERM
{
  echo "=== exit $STATUS $(date -Is)"
  echo "screenshots: $(ls -1 "$ROOT_REAL/Bilder/Screenshots" 2>/dev/null | wc -l) Datei(en)"
} | tee -a "$LOG"
exit "$STATUS"
