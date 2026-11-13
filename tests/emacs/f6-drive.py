#!/usr/bin/env python3
"""Drive real treemacs in a pty and dump state after each step.

Companion to f6-treemacs.el: batch ERT cannot exercise redisplay, so
`window-configuration-change-hook' (side window parameters) and idle timers
(`treemacs-follow-mode') are observed here, in a real `emacs -Q -nw` command
loop.

Usage: f6-drive.py [case ...]
Output: .pi/tmp/emacs-reference/f6-treemacs-<case>.jsonl plus a per-step index
on stdout. Reference evidence only, not a semacs pass record.
"""
import fcntl
import json
import os
import pty
import select
import shutil
import struct
import sys
import tempfile
import termios
import time

HERE = os.path.dirname(os.path.abspath(__file__))
LOG = os.path.abspath(os.path.join(HERE, "..", "..", ".pi", "tmp", "emacs-reference"))
ELPA = os.path.expanduser("~/.emacs.d/elpa")

# Fixed identities; the same list is documented in tests/emacs/README.md.
LIBRARIES = [
    "treemacs-20251226.1307",
    "dash-20260221.1346",
    "s-20260522.135",
    "f-20241003.1131",
    "ace-window-20220911.358",
    "avy-20241101.1357",
    "pfuture-20220913.1401",
    "hydra-20250316.1254",
    "lv-20200507.1518",
    "ht-20230703.558",
    "cfrs-20250729.1422",
]

C_G = b"\x07"
C_X = b"\x18"
C_C = b"\x03"
RET = b"\r"

# (label, keys) steps. `C-x 1` = delete-other-windows, `C-x o` = other-window,
# `M-x ... RET` runs the named command in the real loop.
CASES = {
    "tree-lifecycle": [
        ("dump initial", b""),
        ("C-x o to file", C_X + b"o"),
        ("C-x 1 from file keeps tree", C_X + b"1"),
        ("C-x o back into tree", C_X + b"o"),
        ("C-x 1 inside tree", C_X + b"1"),
        ("q buries tree", b"q"),
        ("M-x treemacs RET shows it again", b"\x1bxtreemacs" + RET),
        ("C-x o to file again", C_X + b"o"),
        ("C-x 1 after reopen keeps tree", C_X + b"1"),
        ("C-x o into tree again", C_X + b"o"),
        ("Q kills the tree buffer", b"Q"),
        ("M-x treemacs RET recreates", b"\x1bxtreemacs" + RET),
        ("t h hides dotfiles", b"th"),
        ("t h shows dotfiles again", b"th"),
        ("t i hides gitignored", b"ti"),
    ],
    "tree-follow": [
        ("dump initial", b""),
        # Follow mode is on by default: visiting a nested file should reveal it
        # in the tree after `treemacs-file-follow-delay' idle time.
        ("C-x o to file", C_X + b"o"),
        ("C-x C-f inner.txt", "find-b-dir-inner"),
        ("dump after idle", b""),
    ],
}


CONSOLE = []


def drain(fd, seconds):
    end = time.time() + seconds
    while time.time() < end:
        r, _, _ = select.select([fd], [], [], 0.05)
        if r:
            try:
                CONSOLE.append(os.read(fd, 65536))
            except OSError:
                return


def dumps(path):
    if not os.path.exists(path):
        return []
    with open(path) as handle:
        return [line for line in handle.read().splitlines() if line.strip()]


def run_case(case):
    steps = CASES[case]
    os.makedirs(LOG, exist_ok=True)
    root = tempfile.mkdtemp(prefix="f6-treemacs-loop-")
    state = os.path.join(LOG, f"f6-treemacs-{case}.jsonl")
    if os.path.exists(state):
        os.remove(state)

    os.environ.update({
        "TERM": "xterm-256color",
        "EMACS_REFERENCE_STATE": state,
        "EMACS_REFERENCE_ROOT": root,
        "EMACS_REFERENCE_PERSIST": os.path.join(LOG, f"f6-treemacs-{case}-persist"),
    })
    argv = ["emacs", "-Q", "-nw"]
    for library in LIBRARIES:
        argv += ["-L", os.path.join(ELPA, library)]
    argv += ["-l", os.path.join(HERE, "f6-treemacs-loop.el")]

    pid, master = pty.fork()
    if pid == 0:  # child
        try:
            fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 100, 0, 0))
            os.execvpe(argv[0], argv, os.environ)
        finally:
            os._exit(1)

    # f6-treemacs-loop.el dumps once after setup; wait for that record.
    drain(master, 0.3)
    deadline = time.time() + 30
    while time.time() < deadline and not dumps(state):
        drain(master, 0.2)
    drain(master, 0.5)

    marks = []
    for label, keys in steps:
        if keys == "find-b-dir-inner":
            keys = C_X + b"\x06" + os.path.join(root, "b-dir", "inner.txt").encode() + RET
        if keys:
            os.write(master, keys)
            drain(master, 1.2)
        before = len(dumps(state))
        os.write(master, C_C + b"d")  # reference/dump
        waited = 0.0
        while waited < 10 and len(dumps(state)) <= before:
            drain(master, 0.2)
            waited += 0.2
        current = len(dumps(state))
        marks.append({"step": label, "index": current - 1 if current > before else -1})

    drain(master, 0.3)
    os.kill(pid, 9)
    try:
        os.waitpid(pid, 0)
    except ChildProcessError:
        pass
    return state, marks, root


def main():
    for case in sys.argv[1:] or list(CASES):
        CONSOLE.clear()
        state, marks, root = run_case(case)
        print(f"### {case} (fixture {root})")
        records = dumps(state)
        for mark in marks:
            index = mark["index"]
            if index < 0 or index >= len(records):
                print(f"  step {mark['step']!r}: MISSING dump ({len(records)} records)")
                continue
            record = json.loads(records[index])
            windows = " ".join(
                f"{w['buffer']}{'*' if w['selected'] else ''}"
                f"[side={w['side']} slot={w['slot']} ndow={w['no-delete']} w={w['width']}]"
                for w in record["windows"])
            tree = (record.get("tree") or "").replace("\n", " | ")
            print(f"  step {mark['step']!r}")
            print(f"    selected={record['selected']!r} visibility={record['visibility']}")
            print(f"    mode={record['mode']}")
            print(f"    windows={windows}")
            print(f"    tree-line={record.get('tree-line')!r}")
            print(f"    tree={tree!r}")
        console = b"".join(CONSOLE).decode("utf-8", "replace")
        if os.environ.get("F6_CONSOLE") or not records:
            path = os.path.join(LOG, f"f6-console-{case}.log")
            with open(path, "w") as handle:
                handle.write(console)
            print(f"  console: {path}")
        shutil.rmtree(root, ignore_errors=True)
        for suffix in ("", "-error"):
            path = os.path.join(LOG, f"f6-treemacs-{case}-persist{suffix}")
            if os.path.exists(path):
                os.remove(path)
        print()


if __name__ == "__main__":
    main()
