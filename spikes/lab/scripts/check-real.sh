#!/usr/bin/env bash
# Compare a real-session case against real-cases/<case>.expected.
#   SPIKE_REAL=1 scripts/check-real.sh <case> | --all
#   UPDATE=1 SPIKE_REAL=1 scripts/check-real.sh <case>
#
# These expectations are machine-specific (Hyprland + fcitx5 pinyin + layout).
# Preedit update noise is dropped; commit / cancel / text / undo stay asserted.
set -uo pipefail
DIR=$(cd "$(dirname "$0")/.." && pwd)
mkdir -p "$DIR/log"
FILTER='^(KEY|RESOLVE|CMD|CMDQ|ECHO|CHANGE|DUMP|DUMP2|BUFFERS|SYNC|REFUSE|CANCEL|FAKE-PREEDIT|OWNEDIT|OWNUNDO|REDO|REPLAY|MINI|PREEDIT|PREFIX|TARGET|COMPOSE|CLOSE|ACTIVATION|=== focused|=== ime)'

normalize() {
  grep -E "$FILTER" "$1" \
    | grep -v 'accepts_text_input' \
    | grep -v 'marked_text_range' \
    | grep -v 'replace_and_mark_text_in_range' \
    | sed -E 's/pid=[0-9]+//'
}

run_one() {
  local case="$1"
  if [ ! -f "$DIR/real-cases/$case.sh" ]; then echo "FAIL $case: no such case"; return 2; fi
  if ! "$DIR/scripts/run-real.sh" "$case" >/dev/null 2>"$DIR/log/real-$case.err"; then
    echo "FAIL $case: harness error"; tail -5 "$DIR/log/real-$case.err" | sed 's/^/    /'; return 2
  fi
  local actual="$DIR/log/real-$case.normalized"
  normalize "$DIR/log/real-$case.log" >"$actual"
  local expected="$DIR/real-cases/$case.expected"
  if [ "${UPDATE:-0}" = "1" ]; then cp "$actual" "$expected"; echo "UPDATED $case ($(wc -l <"$expected") lines)"; return 0; fi
  if [ ! -f "$expected" ]; then echo "FAIL $case: missing expectation"; return 2; fi
  if diff -u "$expected" "$actual" >"$DIR/log/real-$case.diff"; then echo "ok   $case"; return 0; fi
  echo "FAIL $case: log differs"; sed 's/^/    /' "$DIR/log/real-$case.diff" | head -40; return 1
}

cargo build --manifest-path "$DIR/Cargo.toml" >/dev/null 2>&1 || { echo "FAIL: cargo build failed"; exit 2; }
"$DIR/scripts/vkbd/build.sh" >/dev/null 2>&1 || { echo "FAIL: vkbd build failed"; exit 2; }

cases=()
for arg in "$@"; do
  if [ "$arg" = "--all" ]; then
    while IFS= read -r path; do cases+=("$(basename "$path" .sh)"); done < <(ls "$DIR"/real-cases/*.sh)
  else cases+=("$arg"); fi
done
[ ${#cases[@]} -eq 0 ] && { echo "usage: check-real.sh <case>... | --all"; exit 2; }
failed=0
for case in "${cases[@]}"; do run_one "$case" || failed=$((failed+1)); done
[ "$failed" -ne 0 ] && { echo "$failed case(s) failed"; exit 1; }
echo "all ${#cases[@]} real case(s) passed"
