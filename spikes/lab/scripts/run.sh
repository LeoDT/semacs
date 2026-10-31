#!/usr/bin/env bash
# Run one spike case inside a private headless sway compositor.
#
#   scripts/run.sh <case>            # case lives in cases/<case>.sh
#   scripts/check.sh <case>          # run it and diff against cases/<case>.expected
#
# Optional per-case environment lives in cases/<case>.env and is sourced before
# the lab starts (e.g. SPIKE_INITIAL, SPIKE_MODE, SPIKE_READONLY).
#
# Everything happens on a dedicated Wayland socket: nothing appears on the
# user's Hyprland session.
set -uo pipefail

DIR=$(cd "$(dirname "$0")/.." && pwd)
CASE=${1:?usage: run.sh <case>}
LOG="$DIR/log/$CASE.log"
APP_LOG="$DIR/log/$CASE.app.log"
SHOT="$DIR/log/$CASE.png"
mkdir -p "$DIR/log"
: > "$LOG"; : > "$APP_LOG"

if [ -f "$DIR/cases/$CASE.env" ]; then
  # shellcheck source=/dev/null
  . "$DIR/cases/$CASE.env"
fi

export XDG_RUNTIME_DIR=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}
before=$(ls "$XDG_RUNTIME_DIR" 2>/dev/null | grep -o '^wayland-[0-9]*' | sort -u)

WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=pixman \
  sway -c "$DIR/scripts/sway.conf" > "$DIR/log/sway-$CASE.log" 2>&1 &
SWAY_PID=$!
APP_PID=""
cleanup() { [ -n "$APP_PID" ] && kill "$APP_PID" 2>/dev/null; kill "$SWAY_PID" 2>/dev/null; wait 2>/dev/null; }
trap cleanup EXIT

WL=""
for _ in $(seq 200); do
  WL=$(comm -13 <(echo "$before") <(ls "$XDG_RUNTIME_DIR" | grep -o '^wayland-[0-9]*' | sort -u) 2>/dev/null | head -1)
  [ -n "$WL" ] && break
  sleep 0.05
done
if [ -z "$WL" ]; then echo "FAIL: no new wayland socket (see log/sway-$CASE.log)"; tail -5 "$DIR/log/sway-$CASE.log"; exit 1; fi
export WAYLAND_DISPLAY=$WL
export SWAYSOCK="$XDG_RUNTIME_DIR/sway-ipc.$(id -u).$SWAY_PID.sock"
for _ in $(seq 200); do [ -S "$SWAYSOCK" ] && break; sleep 0.05; done

WAYLAND_DEBUG="${WAYLAND_DEBUG:-}" \
SPIKE_LOG="$LOG" SPIKE_MODE="${SPIKE_MODE:-consume}" SPIKE_INITIAL="${SPIKE_INITIAL:-}" \
  "$DIR/target/debug/spike" > "$APP_LOG" 2>&1 &
APP_PID=$!

# Wait for the lab to report focus; that means the window exists and is mapped.
for _ in $(seq 400); do
  grep -q '=== focused' "$LOG" 2>/dev/null && break
  sleep 0.05
done
if ! grep -q '=== focused' "$LOG" 2>/dev/null; then
  echo "FAIL: lab never focused (wayland=$WL)"; tail -20 "$APP_LOG"; exit 1
fi
sleep 0.4

# -s gives the compositor time to hand keyboard focus to the client after the
# virtual keyboard is created; without it the first key press is routed to the
# old (nil) focus and silently dropped.
keys() { wtype -s 200 -d 40 "$@" 2>>"$DIR/log/$CASE.wtype.log"; sleep 0.2; }
# Real-keymap virtual keyboard: Shift levels, dead keys and AltGr, which wtype's
# synthetic one-level keymap cannot produce. Build it once before the run.
VKBD="$DIR/scripts/vkbd/build/vkbd"
[ -x "$VKBD" ] || "$DIR/scripts/vkbd/build.sh" >/dev/null 2>>"$APP_LOG" || true
vk() { "$VKBD" "$@" 2>>"$DIR/log/$CASE.vkbd.log"; sleep 0.3; }
VPTR="$DIR/scripts/vptr/build/vptr"
[ -x "$VPTR" ] || "$DIR/scripts/vptr/build.sh" >/dev/null 2>>"$APP_LOG" || true
# Absolute pointer motion/click/drag inside the headless output (1280x800).
mouse() { "$VPTR" "$@" 2>>"$DIR/log/$CASE.vptr.log"; sleep 0.3; }
shot() { grim "$SHOT" 2>/dev/null && echo "screenshot: $SHOT"; }

# shellcheck source=/dev/null
source "$DIR/cases/$CASE.sh"

sleep 0.3
[ "${KEEP_SHOT:-1}" = "1" ] && shot

kill "$APP_PID" 2>/dev/null; APP_PID=""
sleep 0.2
echo "---- log: $LOG"
cat "$LOG"
