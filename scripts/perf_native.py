#!/usr/bin/env python3
"""Opt-in native benchmark runner; only --ime injects real desktop input."""

import argparse
import csv
import hashlib
import json
import os
from pathlib import Path
import queue
import shutil
import subprocess
import threading
import time
import uuid

SAMPLES = (
    "mixed", "short-ascii", "short-unicode",
    "long-ascii", "single-ascii", "single-unicode",
)
ROOT = Path(__file__).resolve().parent.parent


def command(*args):
    return subprocess.check_output(args, text=True, timeout=10).strip()


class Desktop:
    def __init__(self, signature, ime):
        self.signature = signature
        self.ime = ime
        self.original = self.active().get("address") if signature else None
        self.state = command("fcitx5-remote") if ime else None
        self.method = command("fcitx5-remote", "-n") if ime else None
        if ime and (not self.original or self.state not in ("1", "2") or not self.method):
            raise RuntimeError("cannot safely capture the original focus / fcitx5 state")

    def hypr(self, *args):
        return command("hyprctl", "-i", self.signature, *args)

    def active(self):
        return json.loads(self.hypr("activewindow", "-j"))

    def target(self, token):
        windows = [
            w for w in json.loads(self.hypr("clients", "-j"))
            if w.get("class") == "semacs-perf-native-" + token
            and w.get("mapped")
        ]
        if len(windows) != 1:
            raise RuntimeError(f"expected one mapped probe for token {token}, got {windows}")
        return windows[0]

    def focus(self, address):
        self.hypr("dispatch", "hl.dsp.focus({window=" + json.dumps("address:" + address) + "})")

    def keys(self, token, *args):
        target = self.target(token)
        if self.active().get("address") != target["address"]:
            raise RuntimeError("refusing to inject input: the probe is not focused")
        subprocess.run(["wtype", *args], check=True, timeout=10)

    def restore(self):
        if self.original:
            addresses = {w["address"] for w in json.loads(self.hypr("clients", "-j"))}
            if self.original in addresses:
                self.focus(self.original)
                time.sleep(0.15)
            if self.ime:
                command("fcitx5-remote", "-s", self.method)
                if self.state in ("1", "2"):
                    command("fcitx5-remote", "-o" if self.state == "2" else "-c")
                if command("fcitx5-remote", "-n") != self.method:
                    raise RuntimeError("failed to restore original input method")
                if command("fcitx5-remote") != self.state:
                    raise RuntimeError("failed to restore original IME activity")


def wait_line(lines, prefix, process):
    deadline = time.monotonic() + 45
    while time.monotonic() < deadline:
        try:
            line = lines.get(timeout=0.2)
        except queue.Empty:
            if process.poll() is not None:
                raise RuntimeError(f"probe exited {process.returncode} while waiting for {prefix}")
            continue
        if line.startswith(prefix):
            return line
    raise TimeoutError(f"waiting for {prefix}; see probe logs")


def drive_ime(process, desktop, lines, stem, token):
    wait_line(lines, "READY", process)
    target = desktop.target(token)
    desktop.focus(target["address"])
    time.sleep(0.2)
    command("fcitx5-remote", "-s", "pinyin")
    command("fcitx5-remote", "-o")
    for index, (phase, keys) in enumerate((
        ("preedit", ("-d", "1", "ni")),
        ("cancel", ("-k", "Escape")),
        ("scroll", None), ("resize", None), ("font", None), ("click", None),
        ("preedit", ("-d", "1", "ni")),
        ("commit", ("-k", "space")),
    )):
        process.stdin.write(phase + "\n")
        process.stdin.flush()
        wait_line(lines, "ARMED " + phase, process)
        if keys:
            desktop.keys(token, *keys)
        wait_line(lines, "DONE " + phase, process)
        wait_line(lines, "READY", process)
        if phase == "preedit" and shutil.which("grim"):
            target = desktop.target(token)
            x, y = target["at"]
            width, height = target["size"]
            subprocess.run([
                "grim", "-g", f"{x},{y} {width}x{height}",
                str(stem.with_suffix(f".preedit-{index}.png")),
            ], check=True, timeout=10)
    process.stdin.write("done\n")
    process.stdin.flush()


def read_report(path, software, ime):
    content = path.read_text()
    if "# completed=true;" not in content:
        raise RuntimeError(f"incomplete report {path}")
    renderer = "is_software_emulated: true" if software else "is_software_emulated: false"
    if renderer not in content:
        raise RuntimeError(f"unexpected renderer class in {path}")
    rows = list(csv.DictReader(line for line in content.splitlines() if not line.startswith("#")))
    expected = (
        {"open": 1, "preedit": 2, "cancel": 1, "commit": 1,
         "scroll": 1, "resize": 1, "font": 1, "click": 1} if ime else
        {"open": 1, "scroll": 30, "resize": 1, "font": 1, "forced": 30,
         "idle": 1, "click": 1, "insert": 1, "delete": 1, "newline": 1,
         "join": 1, "burst-insert": 1, "burst-delete": 1}
    )
    counts = {phase: sum(row["phase"] == phase for row in rows) for phase in expected}
    if counts != expected or len(rows) != sum(expected.values()):
        raise RuntimeError(f"wrong phase matrix in {path}: {counts}")
    for row in rows:
        if row["phase"] != "idle" and (
            not row["ready_submit_ms"] or int(row["visible_rows"]) == 0
        ):
            raise RuntimeError(f"missing body / completion in {path}: {row}")
    return rows


def run_probe(args, desktop, sample, run):
    stem = args.output / f"{sample}-{run}"
    report = stem.with_suffix(".csv")
    invocation = [str(args.binary), str(args.input), sample, str(report)]
    if args.software:
        invocation.append("--allow-software")
    if args.ime:
        invocation.append("--external")
    environment = os.environ.copy()
    token = uuid.uuid4().hex
    environment["SEMACS_PERF_TOKEN"] = token
    if args.software:
        environment["LIBGL_ALWAYS_SOFTWARE"] = "1"
    with stem.with_suffix(".stderr.log").open("x") as stderr, stem.with_suffix(".stdout.log").open("x") as stdout:
        process = subprocess.Popen(
            invocation, env=environment, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=stderr, text=True, bufsize=1,
        )
        lines = queue.Queue()

        def reader():
            for line in process.stdout:
                stdout.write(line)
                stdout.flush()
                lines.put(line.rstrip())

        thread = threading.Thread(target=reader, daemon=True)
        thread.start()
        try:
            if args.ime:
                drive_ime(process, desktop, lines, stem, token)
            status = process.wait(timeout=150)
            if status != 0:
                raise RuntimeError(f"probe exited {status}; see {stem}.stderr.log")
            return read_report(report, args.software, args.ime)
        finally:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)
            thread.join(timeout=5)
            process.stdout.close()
            process.stdin.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", type=Path, help="deterministic mixed fixture")
    parser.add_argument("output", type=Path, help="NEW directory; refuses to reuse")
    parser.add_argument("--consent", action="store_true", help="authorize independent native test windows")
    parser.add_argument("--software", action="store_true", help="diagnostic only, never hardware acceptance")
    parser.add_argument("--ime", action="store_true", help="real Wayland/fcitx5 Pinyin; requires Hyprland/wtype")
    parser.add_argument("--sample", choices=SAMPLES, action="append")
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--hyprland-instance", default=os.environ.get("HYPRLAND_INSTANCE_SIGNATURE"))
    parser.add_argument("--binary", type=Path, default=ROOT / "target/release/examples/perf_native")
    args = parser.parse_args()
    if not args.consent:
        parser.error("--consent is required before opening desktop test windows")
    if args.runs < 1:
        parser.error("--runs must be positive")
    if args.ime and (not args.hyprland_instance or not os.environ.get("WAYLAND_DISPLAY")):
        parser.error("--ime requires the actual Wayland/Hyprland session environment")
    args.input = args.input.resolve(strict=True)
    if args.input.stat().st_size < 8 * 1024 * 1024:
        parser.error("R1 mixed fixture must be at least 8 MiB; use scripts/perf_sample.py")
    args.binary = args.binary.resolve(strict=True)
    args.output.mkdir(parents=True, exist_ok=False)
    digest = hashlib.sha256(args.input.read_bytes()).hexdigest()
    metadata = {
        "uname": list(os.uname()), "software_diagnostic": args.software,
        "ime": args.ime, "runs": args.runs, "samples": args.sample or list(SAMPLES),
        "fixture_sha256": digest,
        "fontconfig_monospace": command("fc-match", "-f", "%{family}\n%{file}\n", "monospace"),
        "fontconfig_bold": command("fc-match", "-f", "%{family}\n%{file}\n", "monospace:style=Bold"),
        "completed": False, "restored": False,
    }
    desktop = Desktop(args.hyprland_instance, args.ime)
    values = {}
    try:
        for sample in metadata["samples"]:
            for run in range(1, args.runs + 1):
                rows = run_probe(args, desktop, sample, run)
                for row in rows:
                    for key in (
                        "first_submit_ms", "ready_submit_ms", "draw_cpu_max_ms",
                        "action_cpu_ms", "blank_frames", "first_frame_submit_ms",
                    ):
                        if row[key]:
                            values.setdefault((sample, row["phase"], key), []).append(float(row[key]))
                print(f"{sample} {run}/{args.runs}: complete", flush=True)
        metadata["completed"] = True
    finally:
        try:
            desktop.restore()
            if hashlib.sha256(args.input.read_bytes()).hexdigest() != digest:
                raise RuntimeError("fixture changed during native measurements")
            metadata["restored"] = True
        finally:
            (args.output / "environment.json").write_text(json.dumps(metadata, ensure_ascii=False, indent=2) + "\n")
    summary = [
        {"sample": sample, "phase": phase, "metric": metric,
         "n": len(data), "min": min(data), "max": max(data)}
        for (sample, phase, metric), data in sorted(values.items())
    ]
    (args.output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")


if __name__ == "__main__":
    main()
