#!/usr/bin/env python3
"""Deterministic 256 KiB syntax fixtures; no desktop access."""

import argparse
import hashlib
import json
from pathlib import Path

BYTES = 256 * 1024
UNITS = {
    "markdown": '# Heading 中🙂\n\n**bold** and [link](https://example.com).\n\n' +
                '\x60\x60\x60html\n<script>const n = call(42);</script><style>.box { width: 12px; }</style>\n\x60\x60\x60\n\n',
    "html": '<div class="box">中🙂</div><script>const n = call(42);</script><style>.box { width: 12px; }</style>\n',
}
EXTENSIONS = {"markdown": "md", "html": "html"}


def sample(name):
    unit = UNITS[name].encode()
    return unit * ((BYTES + len(unit) - 1) // len(unit))


def prepare(directory):
    directory.mkdir(parents=True, exist_ok=False)
    digests = {}
    for name, extension in EXTENSIONS.items():
        data = sample(name)
        filename = f"{name}.{extension}"
        with (directory / filename).open("xb") as output:
            output.write(data)
        digests[filename] = hashlib.sha256(data).hexdigest()
    manifest = (json.dumps({"schema": 1, "files": digests}, sort_keys=True, indent=2) + "\n").encode()
    with (directory / "syntax-fixtures.json").open("xb") as output:
        output.write(manifest)
    return hashlib.sha256(manifest).hexdigest()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    print(prepare(parser.parse_args().directory))
