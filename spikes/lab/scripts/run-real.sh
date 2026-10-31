#!/usr/bin/env bash
# Run one case ON THE USER'S REAL HYPRLAND SESSION. This opens a window, takes
# focus and injects keys; it is opt-in and must only be used with consent.
#
#   SPIKE_REAL=1 scripts/run-real.sh <case>
#
# Case scripts live in real-cases/ and use `keys` (wtype) / `vk` (full keymap
# virtual keyboard) / `shot` (grim). cases-real/<case>.env may set SPIKE_IME=1
# (activate fcitx5 pinyin) or 0 (leave it inactive).
#
# Safety: every input batch checks hyprctl's focused window and aborts if the
# lab lost focus, so keys are never sent to another window. fcitx5's current
# input method and active state are restored on exit.
set -uo pipefail

if [ "${SPIKE_REAL:-0}" != "1" ]; then
  echo "refusing to touch the real session: set SPIKE_REAL=1 (see spikes/README.md)" >&2
  exit 2
fi

DIR=$(cd "$(dirname "$0")/.." && pwd)
CASE=${1:?usage: run-real.sh <case>}
LOG="$DIR/log/real-$CASE.log"
APP_LOG="$DIR/log/real-$CASE.app.log"
SHOT="$DIR/log/real-$CASE.png"
mkdir -p "$DIR/log"
: > "$LOG"; : > "$APP_LOG"

if [ -f "$DIR/real-cases/$CASE.env" ]; then
  # shellcheck source=/dev/null
  . "$DIR/real-cases/$CASE.env"
fi

export XDG_RUNTIME_DIR=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}
SIG=${HYPRLAND_INSTANCE_SIGNATURE:-$(basename "$(ls -d "$XDG_RUNTIME_DIR"/hypr/*/ 2>/dev/null | head -1)" 2>/dev/null || true)}
if [ -z "$SIG" ] || ! HYPRLAND_INSTANCE_SIGNATURE=$SIG hyprctl version >/dev/null 2>&1; then
  echo "FAIL: cannot reach Hyprland (HYPRLAND_INSTANCE_SIGNATURE=$SIG)" >&2
  exit 1
fi
export HYPRLAND_INSTANCE_SIGNATURE=$SIG
if [ -z "${WAYLAND_DISPLAY:-}" ]; then
  WAYLAND_DISPLAY=$(ls "$XDG_RUNTIME_DIR" | grep -o '^wayland-[0-9]*' | head -1)
fi
export WAYLAND_DISPLAY
[ -n "$WAYLAND_DISPLAY" ] || { echo "FAIL: no wayland socket in $XDG_RUNTIME_DIR" >&2; exit 1; }
if [ -S "$XDG_RUNTIME_DIR/bus" ]; then
  export DBUS_SESSION_BUS_ADDRESS=${DBUS_SESSION_BUS_ADDRESS:-unix:path=$XDG_RUNTIME_DIR/bus}
fi
export LIBGL_ALWAYS_SOFTWARE=${LIBGL_ALWAYS_SOFTWARE:-1}

addresses() { hyprctl -j clients | python3 -c 'import json,sys;print("\n".join(c["address"] for c in json.load(sys.stdin)))'; }
active_addr() { hyprctl -j activewindow | python3 -c 'import json,sys;print(json.load(sys.stdin).get("address",""))'; }

SAVED_IM=$(fcitx5-remote -n 2>/dev/null || true)
SAVED_STATE=$(fcitx5-remote 2>/dev/null || true)
restore_ime() {
  [ -n "${SAVED_IM:-}" ] && dbus-send --session --dest=org.fcitx.Fcitx5 /controller \
    org.fcitx.Fcitx.Controller1.SetCurrentIM "string:$SAVED_IM" >/dev/null 2>&1
  case "${SAVED_STATE:-}" in
    2) fcitx5-remote -o >/dev/null 2>&1 ;;
    *) fcitx5-remote -c >/dev/null 2>&1 ;;
  esac
}

BEFORE=$(addresses)
SPIKE_LOG="$LOG" SPIKE_MODE="${SPIKE_MODE:-log}" SPIKE_INITIAL="${SPIKE_INITIAL:-}" \
  "$DIR/target/debug/spike" > "$APP_LOG" 2>&1 &
APP_PID=$!
ADDR=""
cleanup() {
  [ -n "${APP_PID:-}" ] && kill "$APP_PID" 2>/dev/null
  restore_ime
  wait 2>/dev/null
}
trap cleanup EXIT

for _ in $(seq 400); do grep -q '=== focused' "$LOG" 2>/dev/null && break; sleep 0.05; done
if ! grep -q '=== focused' "$LOG" 2>/dev/null; then
  echo "FAIL: lab never focused"; tail -20 "$APP_LOG"; exit 1
fi
for _ in $(seq 60); do
  ADDR=$(comm -13 <(echo "$BEFORE" | sort) <(addresses | sort) | head -1)
  [ -n "$ADDR" ] && break; sleep 0.05
done
[ -n "$ADDR" ] || { echo "FAIL: lab window not found"; exit 1; }
hyprctl dispatch focuswindow "address:$ADDR" >/dev/null
sleep 0.6
if [ "$(active_addr)" != "$ADDR" ]; then
  echo "FAIL: lab is not the focused window; refusing to type" >&2
  exit 1
fi

case "${SPIKE_IME:-0}" in
  1) fcitx5-remote -o >/dev/null 2>&1; sleep 0.4
     echo "=== ime state: $(fcitx5-remote) $(fcitx5-remote -n)" >>"$LOG" ;;
  0) fcitx5-remote -c >/dev/null 2>&1; sleep 0.2 ;;
esac

guard() { [ "$(active_addr)" = "$ADDR" ] || { echo "ABORT: lab lost focus, not sending keys" >&2; exit 1; }; }
keys() { guard; wtype -s 200 -d 40 "$@" 2>>"$DIR/log/real-$CASE.wtype.log"; sleep 0.3; }
VKBD="$DIR/scripts/vkbd/build/vkbd"
[ -x "$VKBD" ] || "$DIR/scripts/vkbd/build.sh" >/dev/null 2>>"$APP_LOG"
vk() { guard; "$VKBD" "$@" 2>>"$DIR/log/real-$CASE.vkbd.log"; sleep 0.3; }
shot() { grim "$SHOT" 2>/dev/null && echo "screenshot: $SHOT"; }

# shellcheck source=/dev/null
source "$DIR/real-cases/$CASE.sh"

sleep 0.3
[ "${KEEP_SHOT:-1}" = "1" ] && shot
kill "$APP_PID" 2>/dev/null; APP_PID=""; sleep 0.3
echo "---- log: $LOG"
cat "$LOG"
