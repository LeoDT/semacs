#!/usr/bin/env python3
"""Drive a real Emacs 31.1 command loop in a pty and dump state after each command.

Reference side of the fidelity matrix in docs/emacs-fidelity.md section 5:
real command loop (`emacs -Q -nw`), fundamental-mode, transient mark on,
keystrokes injected as terminal input.

Usage: drive.py <case> [<case> ...]
Output: .pi/tmp/emacs-reference/emacs-<case>.jsonl (repository-relative) plus a per-step index map on stdout.

Mouse cases send xterm SGR sequences as terminal input after enabling
`xterm-mouse-mode`; their cases declare a fixed 24x80 pty and `point: start` so
clicked rows and scrolling line up with the dumped window.
"""
import base64
import fcntl
import json
import os
import pty
import select
import struct
import subprocess
import sys
import termios
import time

HERE = os.path.dirname(os.path.abspath(__file__))
LOG = os.path.abspath(os.path.join(HERE, "..", "..", ".pi", "tmp", "emacs-reference"))

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
C_W = b"\x17"

def mouse(button, col, row, release=False):
    """xterm SGR mouse report; button 0 is left, 32 is motion with left held."""
    return f"\x1b[<{button};{col};{row}{'m' if release else 'M'}".encode()


def click(col, row):
    return mouse(0, col, row) + mouse(0, col, row, release=True)


MOUSE_FIXTURE = "".join(f"l{i:02d} aaaa\n" for i in range(100))
SHORT_FIXTURE = "".join(f"l{i:02d} aaaa\n" for i in range(3))

# Each case: (initial text, [(label, keys), ...], optional options).
# Options: {"size": (rows, cols), "point": "start", "mouse": True}.
CASES = {
    "m1-minibuffer-motion": ("body", [("open reference input", b"\x03t"),
        ("type abc", b"abc"), ("M-<", ESC + b"<"), ("C-a", C_A),
        ("C-b x2", b"\x02" * 2), ("C-a in prompt", C_A),
        ("C-k in prompt", C_K), ("M->", ESC + b">"),
        ("undo", C_SLASH), ("C-g", C_G)]),
    "m1-minibuffer-history": ("body", [("C-a", C_A), ("C-k", C_K),
        ("type A", b"A"), ("open reference input", b"\x03t"),
        ("C-y", C_Y), ("C-a", C_A), ("C-SPC", C_SPC),
        ("C-f x2", C_F * 2), ("C-w", C_W), ("undo", C_SLASH),
        ("undo", C_SLASH), ("C-g", C_G), ("undo body", C_SLASH)]),
    "m1-minibuffer-prompt": ("body", [("open reference input", b"\x03t"),
        ("type abc", b"abc"), ("C-a", C_A), ("C-b", b"\x02"),
        ("C-f", C_F), ("DEL", DEL), ("C-g", C_G)]),
    "m1-copy-kill-yank": ("ab\ncd", [
        ("M-<", ESC + b"<"), ("C-SPC", C_SPC), ("C-f x2", C_F * 2),
        ("M-w", ESC + b"w"), ("C-k x2", C_K * 2), ("C-y", C_Y)]),
    "m1-readonly-kill": ("abc\ndef", [
        ("M-<", ESC + b"<"), ("readonly", b"\x03r"), ("C-k", C_K),
        ("C-k again", C_K)]),
    "m1-kill-copy-direction": ("abc\ndef", [
        ("M-<", ESC + b"<"), ("C-SPC", C_SPC), ("C-f x2", C_F * 2),
        ("C-k", C_K), ("M-w", ESC + b"w"), ("M-w again", ESC + b"w")]),
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
    "prefix-quit-mark-twice": ("hello", [("C-e", C_E), ("C-SPC", C_SPC), ("C-a", C_A),
                                          ("C-x C-g", C_X + C_G), ("C-x C-g again", C_X + C_G)]),
    "dirty": ("", [("type abc", b"abc"), ("save point", b"\x03m"),
                   ("type d", b"d"), ("undo", C_SLASH)]),
    "dirty-past-save": ("", [("type abc", b"abc"), ("save point", b"\x03m"),
                             ("undo", C_SLASH), ("type x", b"x")]),
    "readonly": ("abc", [("readonly", b"\x03r"), ("type z", b"z"), ("C-a", C_A)]),
    "kill2": ("ab\ncd", [("C-a", C_A), ("C-k x2", C_K * 2), ("undo1", C_SLASH), ("undo2", C_SLASH)]),
    "kill-yank": ("ab", [("C-a", C_A), ("C-k", C_K), ("C-y", C_Y), ("undo1", C_SLASH),
                          ("undo2", C_SLASH)]),
    "undo-empty-msg": ("", [("undo1", C_SLASH)]),
    "m1-redo-backward-delete": ("abcDEF", [("C-a C-f x3", C_A + C_F * 3),
        ("DEL x2", DEL * 2), ("undo", C_SLASH), ("redo", C_M__)]),
    "m1-redo-forward-delete": ("abcDEF", [("C-a C-f", C_A + C_F),
        ("C-d x2", C_D * 2), ("undo", C_SLASH), ("redo", C_M__)]),
    "m1-redo-after-motion": ("tail", [("C-a", C_A), ("type abc", b"abc"), ("undo", C_SLASH),
        ("C-f", C_F), ("redo", C_M__)]),
    "m1-save-twice": ("", [("type ab", b"ab"), ("save", b"\x03m"),
        ("type cd", b"cd"), ("save", b"\x03m"), ("undo", C_SLASH),
        ("redo", C_M__)]),
    "m1-save-crossing": ("", [("type ab", b"ab"), ("RET", RET),
        ("type cd", b"cd"), ("save", b"\x03m"), ("undo", C_SLASH),
        ("undo", C_SLASH), ("redo", C_M__), ("redo", C_M__)]),
    "mark-quit2": ("hello world", [("C-e", C_E), ("C-SPC", C_SPC), ("C-a", C_A),
                                   ("C-g", C_G), ("yank after quit", C_Y)]),
    "quit-no-mark": ("", [("C-g", C_G), ("C-g again", C_G)]),
    "cq-nomark": ("hello", [("C-x C-g", C_X + C_G)]),
    "cq-mark-inactive": ("hello", [("C-SPC", C_SPC), ("C-g", C_G), ("C-x C-g", C_X + C_G)]),
    "cq-mark-active": ("hello", [("C-SPC", C_SPC), ("C-x C-g", C_X + C_G)]),
    "cq-quit-first": ("hello", [("C-g", C_G), ("C-x C-g", C_X + C_G)]),
    "cq-after-insert": ("hello", [("type ab", b"ab"), ("C-x C-g", C_X + C_G)]),
    "cq-after-insert-move": ("hello", [("type ab", b"ab"), ("C-a", C_A), ("C-x C-g", C_X + C_G)]),
    "cq-after-move": ("hello", [("C-a", C_A), ("C-x C-g", C_X + C_G)]),
    "cq-after-quit-then-insert": ("hello", [("C-g", C_G), ("type ab", b"ab"), ("C-x C-g", C_X + C_G)]),
    "cq-sep-hello": ("hello", [("C-x", C_X), ("C-g", C_G)]),
    "cq-combined-empty": ("", [("C-x C-g", C_X + C_G)]),
    "cq-sep-empty": ("", [("C-x", C_X), ("C-g", C_G)]),
    "m3-mouse-click-drag": (MOUSE_FIXTURE, [
        ("click line 03 col 6", click(6, 3)),
        ("click line 06 col 6", click(6, 6)),
        ("press 5,9 / move 9,8 / move 9,11 / release",
         mouse(0, 5, 9) + mouse(32, 9, 8) + mouse(32, 9, 11) + mouse(0, 9, 11, release=True)),
    ], {"size": (24, 80), "point": "start", "mouse": True}),
    "m3-mouse-autoscroll": (MOUSE_FIXTURE, [
        ("press 5,4", mouse(0, 5, 4)),
        ("move below rows", mouse(32, 5, 22)),
        ("move further below", mouse(32, 5, 30)),
        ("move further below again", mouse(32, 5, 30)),
        ("release below", mouse(0, 5, 30, release=True)),
        ("press in text", mouse(0, 4, 3)),
        ("move right of the pty", mouse(32, 200, 10)),
        ("move right again", mouse(32, 200, 10)),
        ("release right", mouse(0, 200, 10, release=True)),
    ], {"size": (24, 80), "point": "start", "mouse": True}),
    "m3-mouse-below-eob": (SHORT_FIXTURE, [
        ("press 5,10 / release below eob",
         mouse(0, 5, 10) + mouse(0, 5, 10, release=True)),
        ("press 5,20 / release below eob",
         mouse(0, 5, 20) + mouse(0, 5, 20, release=True)),
        ("press 40,3 / release past line end",
         mouse(0, 40, 3) + mouse(0, 40, 3, release=True)),
        ("press 5,10 / move 5,2 / release",
         mouse(0, 5, 10) + mouse(32, 5, 2) + mouse(0, 5, 2, release=True)),
        ("press 5,2 / move 5,12 / release",
         mouse(0, 5, 2) + mouse(32, 5, 12) + mouse(0, 5, 12, release=True)),
    ], {"size": (24, 80), "point": "start", "mouse": True}),
    "m3-read-find-file": ("body", [("C-x C-f", C_X + b"\x06"),
        ("RET", RET),
        ("C-g", C_G),
    ], {"size": (24, 80)}),
    "m3-read-write-file": ("body", [
        ("C-x C-w", C_X + C_W),
        ("C-g", C_G),
    ], {"size": (24, 80)}),
    "buffers": ("aa", [("C-e", C_E), ("C-SPC", C_SPC), ("C-a", C_A),
                        ("C-x b foo RET", C_X + b"bfoo\r"),
                        ("type bb", b"bb"), ("C-SPC", C_SPC), ("C-a", C_A),
                        ("C-x b *reference* RET", C_X + b"b*reference*\r"),
                        ("type c", b"c"), ("undo", C_SLASH),
                        ("C-x b foo RET", C_X + b"bfoo\r"),
                        ("undo in foo", C_SLASH)]),
    "minibuffer-quit": ("", [("C-x C-f", C_X + b"\x06"), ("type abc", b"abc"), ("C-g", C_G)]),
    "m2-mx-complete": ("body", [("M-x", ESC + b"x"), ("type for", b"for"),
        ("TAB", b"\t"), ("TAB again", b"\t"), ("C-g", C_G)]),
    "m2-mx-exit": ("body", [("M-x", ESC + b"x"), ("type forw", b"forw"),
        ("RET", RET), ("type z", b"z")]),
    "m2-mx-unique": ("body", [("M-x", ESC + b"x"), ("type kill-regi", b"kill-regi"),
        ("RET", RET), ("C-g", C_G)]),
    "m2-mx-unknown": ("body", [("M-x", ESC + b"x"), ("type nosuchcmd", b"nosuchcmd"),
        ("RET", RET), ("C-g", C_G)]),
    "m2-mx-empty": ("body", [("M-x", ESC + b"x"), ("RET", RET), ("C-g", C_G)]),
    "m2-mx-cancel": ("body", [("type ab", b"ab"), ("M-x", ESC + b"x"),
        ("type abc", b"abc"), ("C-g", C_G), ("type z", b"z")]),
    "quit-then-region": ("hello", [("C-g", C_G), ("C-e", C_E), ("C-SPC", C_SPC),
                                    ("C-a", C_A), ("C-g", C_G)]),
    "del-then-undo-then-type": ("abc", [("C-a", C_A), ("delete to end", C_K),
                                        ("undo", C_SLASH), ("type z", b"z"),
                                        ("undo", C_SLASH)]),
    "redo-point": ("abc", [("type zz", b"zz"), ("undo", C_SLASH), ("redo", C_M__)]),
    # mark is a marker: text changes move it (insertion type nil, deleted text
    # collapses it to the deletion start), and undo restores it when it is still
    # at that start. Active state is deactivated by edits, not by position.
    "mark-insert": ("abcdef", [("C-a C-f x2", C_A + C_F * 2), ("C-SPC", C_SPC),
                                ("insert at mark", b"X"), ("C-a", C_A),
                                ("insert before mark", b"Y"), ("C-e", C_E),
                                ("insert after mark", b"Z")]),
    "mark-insert-undo": ("abcdef", [("C-a C-f x2", C_A + C_F * 2), ("C-SPC", C_SPC),
                                     ("C-a", C_A), ("insert before mark", b"X"),
                                     ("undo", C_SLASH), ("redo", C_M__)]),
    "mark-delete-undo": ("abcdef", [("C-a C-f x3", C_A + C_F * 3), ("C-SPC", C_SPC),
                                     ("C-a", C_A), ("C-k", C_K),
                                     ("undo", C_SLASH), ("redo", C_M__)]),
    "mark-region-undo": ("abcdef", [("C-a C-f x2", C_A + C_F * 2), ("C-SPC", C_SPC),
                                     ("C-a", C_A), ("C-w", C_W),
                                     ("undo", C_SLASH)]),
    "mark-undo-moved": ("abcdef", [("C-a C-f x3", C_A + C_F * 3), ("C-SPC", C_SPC),
                                    ("C-a", C_A), ("C-d x3", C_D * 3),
                                    ("C-e", C_E), ("C-SPC", C_SPC), ("C-g", C_G),
                                    ("undo", C_SLASH)]),
    "mark-buffers": ("abcdef", [("C-a C-f x3", C_A + C_F * 3), ("C-SPC", C_SPC),
                                 ("C-x b foo RET", C_X + b"bfoo\r"),
                                 ("type xx", b"xx"), ("C-a C-f", C_A + C_F),
                                 ("C-SPC", C_SPC), ("type yy", b"yy"),
                                 ("C-x b *reference* RET", C_X + b"b*reference*\r"),
                                 ("C-a", C_A), ("insert before A mark", b"Q"),
                                 ("C-x b foo RET", C_X + b"bfoo\r"),
                                 ("undo in foo", C_SLASH)]),
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
    entry = CASES[case]
    initial, steps = entry[0], entry[1]
    options = entry[2] if len(entry) > 2 else {}
    os.makedirs(LOG, exist_ok=True)
    state = os.path.join(LOG, f"emacs-{case}.jsonl")
    if os.path.exists(state):
        os.remove(state)

    os.environ.update({
        "TERM": "xterm-256color",
        "EMACS_REFERENCE_STATE": state,
        "EMACS_REFERENCE_INITIAL": initial,
        "EMACS_REFERENCE_POINT": options.get("point", "end"),
        "EMACS_REFERENCE_MOUSE": "1" if options.get("mouse") else "0",
    })
    pid, master = pty.fork()
    if pid == 0:  # child
        try:
            size = options.get("size")
            if size:
                rows, cols = size
                fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
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
            print(f"  [{i}] text={raw!r} point={d['point']} mark={d['mark']} active={d['active']} modified={d.get('modified')} start={d.get('start')} end={d.get('end')} msg={d.get('msg','')!r}")
            if d.get("prompt_end") is not None:
                end = d["prompt_end"]
                print(f"      prompt_end={end} input={raw[end - 1:]!r}")
            if "kill_ring" in d:
                ring = [base64.b64decode(text).decode("utf-8") for text in d["kill_ring"]]
                print(f"      kill_ring={ring!r}")
        for m in marks:
            print(f"  step {m['keys']!r:24} -> dumps {m['dumps'][0]}..{m['dumps'][1]}")
        print()


if __name__ == "__main__":
    main()
