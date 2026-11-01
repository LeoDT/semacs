#!/usr/bin/env python3
"""Disposable fixtures and independent byte checks for manual Wayland acceptance."""

import argparse
import hashlib
import json
from pathlib import Path

OBSERVATIONS = (
    "keyboard-file-loop", "body-ime", "minibuffer-ime",
    "os-close-cancel", "os-close-clean",
)
SEEDS = {
    "alpha.txt": b"\xef\xbb\xbfalpha\r\nsecond",
    "beta.txt": b"beta\n",
    "overwrite.txt": b"outside\n",
    "close.txt": b"close\n",
}
EXPECTED = {
    "alpha.txt": b"\xef\xbb\xbfX" + "你".encode() + b"alpha\r\nsecond",
    "beta.txt": b"Ybeta\n",
    "overwrite.txt": b"\xef\xbb\xbfX" + "你".encode() + b"alpha\r\nsecond",
    "close.txt": b"Zclose\n",
}


def prepare(directory):
    directory.mkdir(parents=True, exist_ok=False)
    for name, data in SEEDS.items():
        (directory / name).write_bytes(data)
    (directory / "fixture.json").write_text(json.dumps({
        "purpose": "disposable manual Wayland acceptance; NOT performance data",
        "seed_sha256": {name: hashlib.sha256(data).hexdigest() for name, data in SEEDS.items()},
    }, indent=2) + "\n")


def verify(directory):
    errors = []
    checks = {}
    for name, expected in EXPECTED.items():
        path = directory / name
        actual = path.read_bytes() if path.is_file() else None
        checks[name] = actual == expected
        if not checks[name]:
            errors.append(f"disk-byte mismatch: {name}")
    observations = {}
    path = directory / "observations.tsv"
    if path.is_file():
        for line in path.read_text().splitlines():
            parts = line.split("\t")
            if len(parts) == 2:
                observations[parts[0]] = parts[1]
    for name in OBSERVATIONS:
        if observations.get(name) != "yes":
            errors.append(f"manual observation not confirmed: {name}")
    result = {
        "completed": not errors, "disk_bytes": checks, "observations": observations,
        "errors": errors, "evidence": "user-observed real desktop plus independent disk bytes",
        "performance": "unmeasured",
    }
    (directory / "result.json").write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
    if errors:
        raise RuntimeError("; ".join(errors))
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("prepare", "verify"))
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    directory = args.directory.resolve()
    if args.action == "prepare":
        prepare(directory)
        print(directory)
    else:
        verify(directory)
        print(f"manual observations and disk bytes verified: {directory / 'result.json'}")


if __name__ == "__main__":
    main()
