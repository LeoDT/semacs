#!/usr/bin/env python3
"""Generate a deterministic text sample for the semacs performance baseline.

The sample mixes ASCII, CJK, accented/Greek/Cyrillic text, emoji and periodic
very long lines. It is written as UTF-8 with LF endings unless --crlf or --bom
is given. The same seed and arguments produce the same bytes.
"""

import argparse
from pathlib import Path

LONG_EVERY = 5_000
LONG_CHUNK = "0123456789abcdef" * 1_200


def line_for(index: int) -> str:
    if index and index % LONG_EVERY == 0:
        return f"LONG {index} {LONG_CHUNK}"
    match index % 4:
        case 0:
            return f'fn line_{index}() {{ println!("hello {index}"); }}'
        case 1:
            return f"中文测试：变量 {index} 的值是 {index:08x}。"
        case 2:
            return f"Café naïve résumé — Ελληνικά — русский — {index}"
        case _:
            return f"emoji 😀 🚀 🧪 and combining e\u0301 — {index}"


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("output", type=Path)
    parser.add_argument("--bytes", type=int, default=8 * 1024 * 1024)
    parser.add_argument("--crlf", action="store_true")
    parser.add_argument("--bom", action="store_true")
    args = parser.parse_args()

    newline = "\r\n" if args.crlf else "\n"
    data = bytearray(b"\xef\xbb\xbf" if args.bom else b"")
    index = 0
    while len(data) < args.bytes:
        # Count only the body, not the one-time BOM.
        body_len = len(data) - (3 if args.bom else 0)
        if body_len >= args.bytes:
            break
        data.extend(line_for(index).encode("utf-8"))
        data.extend(newline.encode("ascii"))
        index += 1

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(data)
    print(f"{args.output}: {len(data)} bytes, {index} generated lines")


if __name__ == "__main__":
    main()
