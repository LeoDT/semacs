#!/usr/bin/env python3
"""Drive a real Emacs 31.1 command loop in a pty and dump state after each command.

Reference side of the fidelity matrix in docs/emacs-fidelity.md section 5:
real command loop (`emacs -Q -nw`), fundamental-mode, transient mark on,
keystrokes injected as terminal input.

Usage: drive.py <case> [<case> ...]
Output: ../log/emacs-<case>.jsonl plus a per-step index map on stdout.
"""
import base64
import json
import os
import pty
import select
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
LOG = os.path.abspath(os.path.join(HERE, "..", "log"))

C_D = b"\x04"
C_F = b"\x06"
C_A = b"\x01"
C_E = b"\x05"
C_K = b"\x0b"
C_Y = b"\x19"
C_G = b"\x07"
C_SLASH = b"\x1f"   # C-/ and C-_ are the same event
DEL = b"\x7f"       # terminal DEL -> delete-backward-char
RET = b"\r"
ESC = b"\x1b"
C_M__ = b"\x1b\x1f"  # C-M-_ (redo)
C_SPC = b"\x00"
C_X = b"\x18"

# Each case: (initial text, [(label, keys), ...])
CASES = {
    "undo-typing": ("", [("type abc", b"abc"), ("undo1", C_SLASH)]),
    "undo-del": ("", [("type abc", b"abc"), ("del x3", DEL * 3),
                      ("undo1", C_SLASH), ("undo2", C_SLASH)]),
    "undo-cd": ("abc", [("C-a", C_A), ("C-d x3", C_D * 3),
                        ("undo1", C_SLASH), ("undo2", C_SLASH)]),
    "undo-move": ("", [("type abc", b"abc"), ("C-f", C_F), ("type def", b"def"),
                       ("undo1", C_SLASH), ("undo2", C_SLASH)]),
    "undo-kill": ("abc", [("C-e", C_E), ("C-k x2", C_K * 2),
                          ("undo1", C_SLASH), ("undo2", C_SLASH)]),
    "undo-yank": ("abc", [("C-a", C_A), ("C-k", C_K), ("C-y x2", C_Y * 2),
                          ("undo1", C_SLASH), ("undo2", C_SLASH)]),
    "undo-ret": ("", [("type abc", b"abc"), ("RET", RET), ("type def", b"def"),
                      ("undo1", C_SLASH), ("undo2", C_SLASH)]),
    "undo-empty": ("", [("undo1", C_SLASH), ("undo2", C_SLASH)]),
    "undo-then-redo": ("", [("type abc", b"abc"), ("undo", C_SLASH), ("redo", C_M__)]),
    "mark-quit": ("hello world", [("C-a", C_A), ("C-SPC", C_SPC), ("C-f x2", C_F * 2),
                                  ("C-g", C_G)]),    "prefix-quit": ("", [("type ab", b"ab"), ("C-x", b"\x18"), ("C-g", C_G),
                          ("type z", b"z")]),
    "undefined": ("", [("type ab", b"ab"), ("C-x", b"\x18"), ("M-a", b"\x1ba"),
                        ("type z", b"z")]),
    "prefix-quit-mark": ("hello", [("C-e", C_E), ("C-SPC", C_SPC), ("C-a", C_A),
                                    ("C-x C-g", C_X + C_G)]),
    "dirty": ("", [("type abc", b"abc"), ("save point", b"\x03m"),
                   ("type d", b"d"), ("undo", C_SLASH)]),
    "dirty-past-save": ("", [("type abc", b"abc"), ("save point", b"\x03m"),
                             ("undo", C_SLASH), ("type x", b"x")]),
    "readonly": ("abc", [("readonly", b"\x03r"), ("type z", b"z"), ("C-a", C_A)]),
    "kill2": ("ab\ncd", [("C-a", C_A), ("C-k x2", C_K * 2), ("undo1", C_SLASH), ("undo2", C_SLASH)]),
    "kill-yank": ("ab", [("C-a", C_A), ("C-k", C_K), ("C-y", C_Y), ("undo1", C_SLASH),
                          ("undo2", C_SLASH)]),
    "undo-empty-msg": ("", [("undo1", C_SLASH)]),
    "mark-quit2": ("hello world", [("C-e", C_E), ("C-SPC", C_SPC), ("C-a", C_A),
                                   ("C-g", C_G), ("yank after quit", C_Y)]),
    "del-then-undo-then-type": ("abc", [("C-a", C_A), ("delete to end", C_K),
                                        ("undo", C_SLASH), ("type z", b"z"),
                                        ("undo", C_SLASH)]),
}


def drain(fd, seconds):
    end = time.time() + seconds
    while time.time() < end:
        r, _, _ = select.select([fd], [], [], 0.05)
        if r:
            try:
                os.read(fd, 65536)
            except OSError:
                return


def read_lines(path):
    if not os.path.exists(path):
        return []
    with open(path) as f:
        return [line for line in f.read().splitlines() if line.strip()]


def run_case(case, init_wait=1.5, settle=0.5):
    initial, steps = CASES[case]
    os.makedirs(LOG, exist_ok=True)
    state = os.path.join(LOG, f"emacs-{case}.jsonl")
    if os.path.exists(state):
        os.remove(state)

    os.environ.update({"TERM": "xterm-256color", "SPIKE_STATE": state, "SPIKE_INITIAL": initial})
    pid, master = pty.fork()
    if pid == 0:  # child
        try:
            os.execvpe(
                "emacs",
                ["emacs", "-Q", "-nw", "-l", os.path.join(HERE, "baseline.el")],
                os.environ,
            )
        finally:
            os._exit(1)
    drain(master, 0.2)
    deadline = time.time() + 15
    while time.time() < deadline and not read_lines(state):
        drain(master, 0.1)
    drain(master, init_wait)

    marks = []
    for label, keys in steps:
        before = len(read_lines(state))
        os.write(master, keys)
        drain(master, settle)
        after = len(read_lines(state))
        marks.append({"step": label, "keys": keys.decode("latin-1").encode("unicode_escape").decode(),
                      "dumps": [before, after]})
    drain(master, 0.3)
    os.kill(pid, 9)
    try:
        os.waitpid(pid, 0)
    except ChildProcessError:
        pass
    return state, marks


def main():
    for case in sys.argv[1:]:
        state, marks = run_case(case)
        print(f"### {case}")
        lines = read_lines(state)
        for i, line in enumerate(lines):
            d = json.loads(line)
            raw = base64.b64decode(d["text"]).decode("utf-8", "replace")
            print(f"  [{i}] text={raw!r} point={d['point']} mark={d['mark']} active={d['active']} modified={d.get('modified')} msg={d.get('msg','')!r}")
        for m in marks:
            print(f"  step {m['keys']!r:24} -> dumps {m['dumps'][0]}..{m['dumps'][1]}")
        print()


if __name__ == "__main__":
    main()
