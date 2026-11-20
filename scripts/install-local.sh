#!/usr/bin/env bash
set -euo pipefail

: "${HOME:?HOME must be set}"
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
binary=$(
    cargo build --locked --release --bin semacs --message-format=json-render-diagnostics |
        python3 -c '
import json
import sys

binary = None
for line in sys.stdin:
    message = json.loads(line)
    if message["reason"] == "compiler-artifact" and message["target"]["name"] == "semacs":
        binary = message["executable"] or binary
if binary is None:
    sys.exit("Cargo did not report a semacs executable")
print(binary)
'
)

bin_dir="$HOME/.local/bin"
mkdir -p -- "$bin_dir"
staged=$(mktemp -- "$bin_dir/.semacs.XXXXXX")
trap 'rm -f -- "$staged"' EXIT
install -m 755 -- "$binary" "$staged"
mv -fT -- "$staged" "$bin_dir/semacs"
printf 'Installed %s\n' "$bin_dir/semacs"
