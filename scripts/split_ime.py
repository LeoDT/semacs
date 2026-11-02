#!/usr/bin/env python3
"""Auto-asserted desktop IME check; the user types only the prompted IME sequences."""

import argparse
import csv
import hashlib
import io
import json
import os
from pathlib import Path
import queue
import shutil
import subprocess
import threading
import time
import uuid

from perf_native import Desktop, wait_line

ROOT = Path(__file__).resolve().parent.parent
INITIAL = b"alpha\nsecond\n"
FINAL = "你你alpha\nsecond\n".encode()
PHASES = (
    "open", "split", "right-preedit", "right-cancel", "right-preedit", "right-commit",
    "left", "left-preedit", "left-cancel", "left-preedit", "left-commit",
    "minibuffer", "mini-preedit", "mini-cancel", "mini-preedit", "mini-commit", "restore", "save",
)


def validate_report(content):
    if "# completed=true; performance=unmeasured; candidate_visual=unreviewed" not in content:
        raise RuntimeError("probe did not complete all integration assertions")
    if "# hardware=true;" not in content:
        raise RuntimeError("missing hardware renderer identity")
    rows = list(csv.DictReader(io.StringIO("\n".join(
        line for line in content.splitlines() if not line.startswith("#")
    ))))
    if tuple(row["phase"] for row in rows) != PHASES:
        raise RuntimeError("incomplete or reordered phase matrix")
    for index, row in enumerate(rows):
        expected = INITIAL if index < 5 else ("你".encode() + INITIAL if index < 10 else FINAL)
        first_point = 0 if index < 10 else 3
        second_point = 0 if index < 5 else (3 if index < 10 else 6)
        mini = "你".encode().hex() if index == 15 else ""
        if (row["step"] != str(index) or row["assertions_passed"] != "true"
                or row["body_hex"] != expected.hex() or row["mini_hex"] != mini
                or row["first_point"] != str(first_point)
                or row["second_point"] != str(second_point)):
            raise RuntimeError(f"invalid state in phase {index}: {row}")
    return rows


def ensure_focus(desktop, token):
    target = desktop.target(token)
    if desktop.active().get("address") != target["address"]:
        raise RuntimeError("refusing desktop action: the integration probe is not focused")
    return target


def screenshot(desktop, token, path):
    target = ensure_focus(desktop, token)
    monitors = json.loads(desktop.hypr("monitors", "-j"))
    matches = [monitor for monitor in monitors if monitor["id"] == target.get("monitor")]
    if len(matches) != 1:
        raise RuntimeError("cannot identify the probe monitor for candidate capture")
    subprocess.run(
        ["grim", "-o", matches[0]["name"], str(path)],
        check=True, timeout=10,
    )


def drive(process, desktop, lines, output, token):
    deadline = time.monotonic() + 30
    while True:
        if process.poll() is not None:
            raise RuntimeError(f"probe exited {process.returncode} during startup")
        try:
            target = desktop.target(token)
            break
        except RuntimeError:
            if time.monotonic() >= deadline:
                raise TimeoutError("probe did not map a unique window")
            time.sleep(0.1)
    desktop.focus(target["address"])
    time.sleep(0.2)
    wait_line(lines, "READY", process)
    for index, phase in enumerate(PHASES[1:], 1):
        ensure_focus(desktop, token)
        process.stdin.write(phase + "\n")
        process.stdin.flush()
        wait_line(lines, "ARMED " + phase, process)
        wait_line(lines, "DONE " + phase, process)
        wait_line(lines, "READY", process)
        if phase.endswith("preedit"):
            time.sleep(0.15)
            screenshot(desktop, token, output / f"{index:02d}-{phase}.png")
    process.stdin.write("done\n")
    process.stdin.flush()
    wait_line(lines, "COMPLETED", process)


def restore_focus(desktop):
    windows = json.loads(desktop.hypr("clients", "-j"))
    if not desktop.original or desktop.original not in {window["address"] for window in windows}:
        raise RuntimeError("original window is gone; focus restore skipped, input method untouched")
    desktop.focus(desktop.original)
    if desktop.active().get("address") != desktop.original:
        raise RuntimeError("original window did not regain focus; input method untouched")


def run(output, signature):
    output.mkdir(parents=True, exist_ok=False)
    fixture = output / "input.txt"
    with fixture.open("xb") as file:
        file.write(INITIAL)
    binary = ROOT / "target/release/examples/native_split_ime"
    result = {
        "completed": False,
        "scope": "native split/body/minibuffer IME integration",
        "performance": "unmeasured",
        "candidate_visual": "unreviewed; inspect saved screenshots",
        "screenshot_scope": "entire probe monitor; may include other visible windows",
        "input": "user IME keys only; no global injection or scripted input-method changes",
        "fixture_initial_sha256": hashlib.sha256(INITIAL).hexdigest(),
    }
    desktop = None
    error = None
    try:
        result["probe_sha256"] = hashlib.sha256(binary.read_bytes()).hexdigest()
        desktop = Desktop(signature, ime=False)
        if not desktop.original:
            raise RuntimeError("cannot capture the original window focus before starting the probe")
        environment = os.environ.copy()
        token = uuid.uuid4().hex
        environment["SEMACS_PERF_TOKEN"] = token
        result["token"] = token
        result["environment"] = {
            name: os.environ.get(name) for name in
            ("WAYLAND_DISPLAY", "XDG_SESSION_TYPE", "XDG_CURRENT_DESKTOP", "LANG")
        }
        with (output / "stderr.log").open("x") as stderr, (output / "stdout.log").open("x") as stdout:
            process = subprocess.Popen(
                [str(binary), str(fixture), str(output / "report.csv")],
                stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=stderr,
                text=True, bufsize=1, env=environment,
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
                drive(process, desktop, lines, output, token)
                status = process.wait(timeout=15)
                if status:
                    raise RuntimeError(f"probe exited {status}; see stdout.log / stderr.log")
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
        rows = validate_report((output / "report.csv").read_text())
        if fixture.read_bytes() != FINAL:
            raise RuntimeError("independent saved fixture bytes do not match the two body commits")
        result["phases"] = len(rows)
        result["fixture_final_sha256"] = hashlib.sha256(fixture.read_bytes()).hexdigest()
        result["screenshots"] = sorted(path.name for path in output.glob("*.png"))
        if len(result["screenshots"]) != 6:
            raise RuntimeError("expected six preedit screenshots")
        result["completed"] = True
    except Exception as failure:
        error = str(failure)
    finally:
        if desktop is not None:
            try:
                restore_focus(desktop)
                result["focus_restored"] = True
            except Exception as failure:
                result["focus_restored"] = False
                error = f"{error + '; ' if error else ''}focus restore failed: {failure}"
        if error:
            result.update(completed=False, error=error)
            with (output / "STOP.txt").open("x") as file:
                file.write(error + "\n")
        with (output / "result.json").open("x") as file:
            json.dump(result, file, indent=2)
            file.write("\n")
    if error:
        raise RuntimeError(error)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--consent", action="store_true")
    parser.add_argument("--hyprland-instance", default=os.environ.get("HYPRLAND_INSTANCE_SIGNATURE"))
    args = parser.parse_args()
    if not args.consent:
        parser.error("explicit --consent is required before desktop access")
    if not args.hyprland_instance or not os.environ.get("WAYLAND_DISPLAY"):
        parser.error("requires the actual Wayland/Hyprland session")
    for binary in ("hyprctl", "grim"):
        if not shutil.which(binary):
            parser.error(f"missing {binary}")
    output = args.output.resolve()
    try:
        run(output, args.hyprland_instance)
    except Exception as failure:
        parser.exit(1, f"STOP: {failure}; preserve {output}, do not retry\n")
    print(f"Integration assertions and disk bytes passed: {output}")
    print("Review the saved candidate screenshots; native performance remains unmeasured.")


if __name__ == "__main__":
    main()
