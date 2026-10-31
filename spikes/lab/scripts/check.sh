#!/usr/bin/env bash
# Run a lab case and compare its normalized log against cases/<case>.expected.
#
#   scripts/check.sh <case> [<case> ...]   # verify
#   scripts/check.sh --all                 # verify every case
#   UPDATE=1 scripts/check.sh <case>       # rewrite the expectation
#
# The log is filtered down to the lines that carry behaviour (keystroke
# resolution, commands, echo area, text changes, undo replay). Harness noise
# (window activation flapping caused by the virtual keyboard, the high-frequency
# accepts_text_input polling, screenshots, pids) is dropped so the comparison is
# deterministic.
set -uo pipefail

DIR=$(cd "$(dirname "$0")/.." && pwd)
mkdir -p "$DIR/log"
FILTER='^(KEY|RESOLVE|CMD|CMDQ|ECHO|CHANGE|DUMP|DUMP2|OWNEDIT|OWNUNDO|REDO|REPLAY|MINI|PREEDIT|PREFIX|TARGET|=== focused)'

normalize() {
  grep -E "$FILTER" "$1" \
    | grep -v 'accepts_text_input' \
    | sed -E 's/pid=[0-9]+//'
}

run_one() {
  local case="$1"
  if [ ! -f "$DIR/cases/$case.sh" ]; then
    echo "FAIL $case: no such case"
    return 2
  fi
  if ! "$DIR/scripts/run.sh" "$case" >/dev/null 2>"$DIR/log/$case.err"; then
    echo "FAIL $case: harness error"
    tail -5 "$DIR/log/$case.err" | sed 's/^/    /'
    return 2
  fi
  local actual="$DIR/log/$case.normalized"
  normalize "$DIR/log/$case.log" >"$actual"
  local expected="$DIR/cases/$case.expected"

  if [ "${UPDATE:-0}" = "1" ]; then
    cp "$actual" "$expected"
    echo "UPDATED $case ($(wc -l <"$expected") lines)"
    return 0
  fi
  if [ ! -f "$expected" ]; then
    echo "FAIL $case: missing expectation (run UPDATE=1 scripts/check.sh $case)"
    return 2
  fi
  if diff -u "$expected" "$actual" >"$DIR/log/$case.diff"; then
    echo "ok   $case"
    return 0
  fi
  echo "FAIL $case: log differs"
  sed 's/^/    /' "$DIR/log/$case.diff" | head -40
  return 1
}

cargo build --manifest-path "$DIR/Cargo.toml" >/dev/null 2>&1 || {
  echo "FAIL: cargo build failed"
  exit 2
}

cases=()
for arg in "$@"; do
  if [ "$arg" = "--all" ]; then
    while IFS= read -r path; do
      cases+=("$(basename "$path" .sh)")
    done < <(ls "$DIR"/cases/*.sh)
  else
    cases+=("$arg")
  fi
done
if [ ${#cases[@]} -eq 0 ]; then
  echo "usage: scripts/check.sh <case>... | --all"
  exit 2
fi

failed=0
for case in "${cases[@]}"; do
  run_one "$case" || failed=$((failed + 1))
done
if [ "$failed" -ne 0 ]; then
  echo "$failed case(s) failed"
  exit 1
fi
echo "all ${#cases[@]} case(s) passed"
