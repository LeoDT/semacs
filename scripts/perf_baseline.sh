#!/usr/bin/env bash
# Reproducible file-layer baseline: generate the sample once, then time
# fs::read + files::decode in a release build.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
SAMPLE=${1:-"$ROOT/.pi/tmp/perf-sample.txt"}
RUNS=${RUNS:-7}

if [ ! -f "$SAMPLE" ]; then
  echo "missing sample: $SAMPLE" >&2
  echo "generate it first, e.g. python3 scripts/perf_sample.py $SAMPLE" >&2
  exit 1
fi

cargo build --release --example perf_load --manifest-path "$ROOT/Cargo.toml" >/dev/null
BIN="$ROOT/target/release/examples/perf_load"
for _ in $(seq "$RUNS"); do
  "$BIN" "$SAMPLE"
done
