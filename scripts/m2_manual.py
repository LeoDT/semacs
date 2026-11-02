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


CANDIDATE_SEEDS = {
    "source.txt": b"alpha second\nnext line\n",
    "alpha-existing.txt": b"outside\n",
    "overwrite.txt": b"outside\n",
    "sub/child-document.txt": b"child\n",
    **{
        f"sub/long-candidate-{index:02}-" + "abcdefghijklmnopqrstuvwxyz-" * 3 + "name.txt": b"long\n"
        for index in range(12)
    },
}
CANDIDATE_EXPECTED = {
    **CANDIDATE_SEEDS,
    "alpha": b"NEW-A!",
    "al": b"NEW-B",
    "overwrite.txt": "你Xalpha second\nnext line\n".encode(),
}
CANDIDATE_OBSERVATIONS = (
    "candidate-command", "candidate-files", "candidate-buffers",
    "candidate-layout-font", "candidate-minibuffer-ime", "candidate-body-ime",
)
PROFILES = {
    "m2": (SEEDS, EXPECTED, OBSERVATIONS),
    "candidates": (CANDIDATE_SEEDS, CANDIDATE_EXPECTED, CANDIDATE_OBSERVATIONS),
}


def prepare(directory, profile="m2"):
    seeds, _, _ = PROFILES[profile]
    directory.mkdir(parents=True, exist_ok=False)
    for name, data in seeds.items():
        path = directory / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
    (directory / "fixture.json").write_text(json.dumps({
        "purpose": "disposable manual Wayland acceptance; NOT performance data",
        "profile": profile,
        "seed_sha256": {name: hashlib.sha256(data).hexdigest() for name, data in seeds.items()},
    }, indent=2) + "\n")


def verify(directory, profile="m2"):
    _, expected_files, required_observations = PROFILES[profile]
    errors = []
    manifest = json.loads((directory / "fixture.json").read_text())
    if manifest.get("profile", "m2") != profile:
        errors.append("fixture profile does not match verification profile")
    checks = {}
    for name, expected in expected_files.items():
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
    for name in required_observations:
        if observations.get(name) != "yes":
            errors.append(f"manual observation not confirmed: {name}")
    result = {
        "completed": not errors, "profile": profile,
        "disk_bytes": checks, "observations": observations,
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
    parser.add_argument("--profile", choices=tuple(PROFILES), default="m2")
    args = parser.parse_args()
    directory = args.directory.resolve()
    if args.action == "prepare":
        prepare(directory, args.profile)
        print(directory)
    else:
        verify(directory, args.profile)
        print(f"manual observations and disk bytes verified: {directory / 'result.json'}")


if __name__ == "__main__":
    main()
