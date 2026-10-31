#!/usr/bin/env bash
# Measure how long the release binary takes to map its window on a private
# headless sway compositor. Nothing touches the user's desktop session.
#
# Usage: scripts/perf_app_startup.sh [SAMPLE] [RUNS]
set -uo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
SAMPLE=${1:-"$ROOT/.pi/tmp/perf-sample.txt"}
RUNS=${2:-3}
LOG="$ROOT/.pi/tmp/perf-app"
mkdir -p "$LOG"

[ -f "$SAMPLE" ] || python3 "$ROOT/scripts/perf_sample.py" "$SAMPLE"
cargo build --release --bin semacs --manifest-path "$ROOT/Cargo.toml" >/dev/null

export XDG_RUNTIME_DIR=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}
before=$(ls "$XDG_RUNTIME_DIR" 2>/dev/null | grep -o '^wayland-[0-9]*' | sort -u)

WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=pixman \
  sway -c "$ROOT/scripts/sway-headless.conf" >"$LOG/sway.log" 2>&1 &
SWAY_PID=$!
APP_PID=""
cleanup() {
  [ -n "$APP_PID" ] && kill "$APP_PID" 2>/dev/null
  kill "$SWAY_PID" 2>/dev/null
  wait 2>/dev/null
}
trap cleanup EXIT

WL=""
for _ in $(seq 200); do
  WL=$(comm -13 <(echo "$before") <(ls "$XDG_RUNTIME_DIR" | grep -o '^wayland-[0-9]*' | sort -u) 2>/dev/null | head -1)
  [ -n "$WL" ] && break
  sleep 0.05
done
if [ -z "$WL" ]; then
  echo "FAIL: no new Wayland socket" >&2
  tail -10 "$LOG/sway.log" >&2
  exit 1
fi
export WAYLAND_DISPLAY=$WL
export SWAYSOCK="$XDG_RUNTIME_DIR/sway-ipc.$(id -u).$SWAY_PID.sock"
for _ in $(seq 200); do [ -S "$SWAYSOCK" ] && break; sleep 0.05; done

title=$(basename "$SAMPLE")
for run in $(seq "$RUNS"); do
  start=$(date +%s%N)
  "$ROOT/target/release/semacs" "$SAMPLE" >"$LOG/run-$run.log" 2>&1 &
  APP_PID=$!
  mapped=0
  for _ in $(seq 1000); do
    if swaymsg -t get_tree 2>/dev/null | grep -q "$title"; then
      mapped=1
      break
    fi
    sleep 0.01
  done
  end=$(date +%s%N)
  if [ "$mapped" -ne 1 ]; then
    echo "run=$run FAIL: window did not map" >&2
    tail -20 "$LOG/run-$run.log" >&2
    exit 1
  fi
  printf 'run=%s window_map_ms=%.3f\n' "$run" "$(awk -v a="$start" -v b="$end" 'BEGIN { print (b-a)/1000000 }')"
  if [ "$run" -eq 1 ] && command -v grim >/dev/null; then
    grim "$LOG/screenshot.png" 2>/dev/null && echo "screenshot=$LOG/screenshot.png"
  fi
  kill "$APP_PID" 2>/dev/null
  wait "$APP_PID" 2>/dev/null
  APP_PID=""
  sleep 0.2
done
