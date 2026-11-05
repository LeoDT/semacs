#!/usr/bin/env python3
"""Opt-in native benchmark runner; only --ime injects real desktop input."""

import argparse
import csv
import hashlib
import json
import math
import os
from pathlib import Path
import queue
import re
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
        if line.startswith("STOP "):
            raise RuntimeError(line)
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


def validate_m1_report(content, rows, software, seconds, interval_ms):
    settings = re.search(
        r"# mode=m1; duration_seconds=(\d+); interval_ms=(\d+); command_steps=(\d+);", content,
    )
    if not settings or tuple(map(int, settings.groups()[:2])) != (seconds, interval_ms):
        raise RuntimeError("M1 report settings do not match the requested workload")
    chain_steps = int(settings.group(3))
    count = seconds * 1000 // interval_ms
    expected = [("open", 0)] + [("chain", i) for i in range(chain_steps)] + [
        ("sustained", i) for i in range(count)
    ]
    if chain_steps < 1 or [(r["phase"], int(r["step"])) for r in rows] != expected:
        raise RuntimeError("incomplete or reordered M1 phase matrix")
    budget_status = "not-applicable" if software else "true"
    if f"# completed=true; budget_passed={budget_status};" not in content:
        raise RuntimeError("M1 report is missing the expected budget verdict")
    original_bytes = int(rows[0]["bytes"])
    for row in rows:
        phase, status = row["phase"], row["status"]
        values = {}
        for key in (
            "action_cpu_ms", "first_submit_ms", "ready_submit_ms", "first_frame_submit_ms",
            "draw_cpu_max_ms", "schedule_lag_ms", "observed_elapsed_ms",
        ):
            value = row[key]
            if not value and key in ("first_submit_ms", "ready_submit_ms", "first_frame_submit_ms"):
                values[key] = None
            elif not value or not math.isfinite(float(value)) or float(value) < 0:
                raise RuntimeError(f"invalid M1 metric {key}: {row}")
            else:
                values[key] = float(value)
        first, ready = values["first_submit_ms"], values["ready_submit_ms"]
        if status == "ready":
            valid = first is not None and ready is not None and ready >= first
        elif status == "superseded-before-ready":
            valid = phase == "sustained" and first is not None and ready is None
        elif status == "superseded-before-first":
            valid = software and phase == "sustained" and first is None and ready is None
        else:
            valid = False
        if not valid or (phase != "sustained" and status != "ready"):
            raise RuntimeError(f"invalid M1 observation coverage: {row}")
        if int(row["blank_frames"]) != 0 or float(row["width_px"]) <= 0 or float(row["height_px"]) <= 0:
            raise RuntimeError(f"missing M1 body: {row}")
        if first is not None and (values["first_frame_submit_ms"] is None or values["first_frame_submit_ms"] > first):
            raise RuntimeError(f"invalid M1 first-frame boundary: {row}")
        if phase == "sustained":
            index = int(row["step"])
            remainder = (index + 1) % 64
            pending = remainder if remainder <= 32 else 64 - remainder
            key = "x" if index % 64 < 32 else "backspace"
            if int(row["bytes"]) != original_bytes + pending or int(row["point"]) != pending or row["key"] != key:
                raise RuntimeError(f"incorrect M1 sustained workload: {row}")
        if not software:
            first_limit, ready_limit = (250, 500) if phase == "open" else (16, 300)
            for key, limit in (
                ("action_cpu_ms", 2), ("draw_cpu_max_ms", 16),
                ("first_submit_ms", first_limit), ("ready_submit_ms", ready_limit),
            ):
                if values[key] is not None and values[key] > limit:
                    raise RuntimeError(f"STOP: M1 {phase} step {row['step']}: {key}={values[key]} > {limit}")
    if rows[-1]["status"] != "ready":
        raise RuntimeError("M1 final surviving key did not reach full layout readiness")


def metric(row, key, optional=False):
    value = row.get(key, "")
    if optional and value == "":
        return None
    try:
        number = float(value)
    except (TypeError, ValueError) as error:
        raise RuntimeError(f"invalid metric {key}: {row}") from error
    if not math.isfinite(number) or number < 0:
        raise RuntimeError(f"invalid metric {key}: {row}")
    return number


def limit_errors(row, values, limits, software):
    return [] if software else [
        f"STOP: {row['phase']}: {key}={values[key]} > {limit}"
        for key, limit in limits.items() if values[key] > limit
    ]


def check_limits(row, values, limits, software):
    errors = limit_errors(row, values, limits, software)
    if errors:
        raise RuntimeError(errors[0])


def validate_native_metrics(rows, software, ime, diagnostic=False):
    errors = []

    def check(row, values, limits):
        found = limit_errors(row, values, limits, software)
        if found and not diagnostic:
            raise RuntimeError(found[0])
        errors.extend(found)

    for row in rows:
        phase = row["phase"]
        values = {key: metric(row, key) for key in (
            "action_cpu_ms", "draw_cpu_max_ms", "blank_frames", "frames",
            "width_px", "height_px", "observed_elapsed_ms",
        )}
        if values["blank_frames"] or not values["width_px"] or not values["height_px"]:
            raise RuntimeError(f"missing body: {row}")
        limits = {"draw_cpu_max_ms": 16}
        if phase == "idle":
            if values["frames"] != 0:
                if not diagnostic:
                    raise RuntimeError(f"idle produced draw: {row}")
                errors.append("idle: unexpected draw")
            check(row, values, limits)
            continue
        for key in ("first_submit_ms", "ready_submit_ms", "first_frame_submit_ms"):
            values[key] = metric(row, key)
        if not values["frames"] or not (
            values["first_frame_submit_ms"] <= values["first_submit_ms"] <= values["ready_submit_ms"]
        ):
            raise RuntimeError(f"invalid submission boundaries: {row}")
        if phase == "open":
            limits.update(first_submit_ms=250, ready_submit_ms=500)
            if metric(row, "first_visible_rows") == 0:
                raise RuntimeError(f"blank opening frame: {row}")
        elif phase in ("insert", "delete", "newline", "join"):
            limits.update(action_cpu_ms=2, first_submit_ms=16, ready_submit_ms=300)
        elif phase in ("burst-insert", "burst-delete"):
            limits.update(action_cpu_ms=50, first_submit_ms=50, ready_submit_ms=300)
        elif phase == "scroll":
            limits["first_submit_ms"] = 4
        elif phase == "forced":
            limits["first_submit_ms"] = 2
        elif phase in ("resize", "font"):
            limits.update(first_frame_submit_ms=50, ready_submit_ms=300)
        elif ime and phase in ("preedit", "cancel", "commit"):
            if row.get("boundary") != "arm-to-submit":
                raise RuntimeError(f"IME boundary is not arm-to-submit: {row}")
            limits.update(first_submit_ms=75, ready_submit_ms=300)
        check(row, values, limits)

    return errors


M2_PHASES = (
    ["boot"] + ["open"] * 3 + ["switch", "insert", "save-edit", "save"]
    + ["switch"] * 6 + ["complete", "cancel-completion"] + ["kill"] * 3
    + ["open", "insert", "save", "kill"] * 3
)


def validate_m2_report(content, rows, software, open_diagnostic=False):
    phases = M2_PHASES[:2] if open_diagnostic else M2_PHASES
    cycles = 0 if open_diagnostic else 3
    if f"# mode=m2; entries=2000; cycles={cycles}; steps={len(phases)};" not in content:
        raise RuntimeError("unexpected M2 workload settings")
    if [(r["phase"], r["step"]) for r in rows] != [(p, str(i)) for i, p in enumerate(phases)]:
        raise RuntimeError("incomplete or reordered M2 matrix")
    verdict = "not-applicable" if software else "true"
    if f"# completed=true; existing_budget_passed={verdict};" not in content:
        raise RuntimeError("missing M2 existing-budget verdict")
    file_buffers = 0
    message_buffers = 0
    for row in rows:
        phase = row["phase"]
        values = {key: metric(row, key) for key in (
            "action_cpu_ms", "draw_cpu_max_ms", "frames", "blank_frames",
            "width_px", "height_px", "buffers", "scratch_buffers", "file_buffers", "message_buffers",
            "retained_closed_views", "observed_elapsed_ms",
        )}
        if row["status"] != "complete" or values["blank_frames"]:
            raise RuntimeError(f"invalid M2 coverage: {row}")
        if not values["width_px"] or not values["height_px"]:
            raise RuntimeError(f"missing M2 body: {row}")
        if phase == "open":
            file_buffers += 1
        elif phase == "kill":
            file_buffers -= 1
        if phase in ("save-edit", "save"):
            message_buffers = 1
        if (values["scratch_buffers"] != 1 or values["file_buffers"] != file_buffers
                or values["message_buffers"] != message_buffers
                or values["buffers"] != 1 + file_buffers + message_buffers
                or (phase == "kill" and values["retained_closed_views"])):
            raise RuntimeError(f"incorrect M2 lifecycle: {row}")
        limits = {"draw_cpu_max_ms": 16}
        if phase in ("save", "complete", "cancel-completion"):
            if row["first_submit_ms"] or row["ready_submit_ms"]:
                raise RuntimeError(f"UI-only observation incorrectly claims submission: {row}")
        else:
            values["first_submit_ms"] = metric(row, "first_submit_ms")
            values["ready_submit_ms"] = metric(row, "ready_submit_ms")
            if not values["frames"] or values["ready_submit_ms"] < values["first_submit_ms"]:
                raise RuntimeError(f"invalid M2 submissions: {row}")
        if phase in ("save", "save-edit"):
            metric(row, "save_observed_ms")
        if phase == "save-edit":
            metric(row, "save_action_cpu_ms")
            if row.get("save_ack_before_edit") not in ("true", "false"):
                raise RuntimeError(f"missing M2 save acknowledgement coverage: {row}")
        if phase in ("complete", "cancel-completion"):
            metric(row, "result_observed_ms")
        if phase in ("insert", "save-edit"):
            limits.update(action_cpu_ms=2, first_submit_ms=16, ready_submit_ms=300)
        metric(row, "rss_kib", optional=True)
        check_limits(row, values, limits, software)
    if file_buffers != int(open_diagnostic) or message_buffers != int(not open_diagnostic):
        raise RuntimeError("M2 buffer classes did not match the requested workload")


def verify_m2_files(directory, open_diagnostic=False):
    original = (directory / "original.txt").read_bytes()
    # Initial x/y save plus three reopen/insert/save/close cycles.
    expected = [original] * 3 if open_diagnostic else [b"xxxxy" + original, original, original]
    for index, data in enumerate(expected):
        if (directory / f"input-{index}.txt").read_bytes() != data:
            raise RuntimeError(f"M2 independent disk-byte mismatch: input-{index}.txt")
    if sorted(p.name for p in (directory / "completion").iterdir()) != [
        f"entry-{index:04}.txt" for index in range(2000)
    ]:
        raise RuntimeError("M2 completion fixture changed")


def preparation_summary(path, frames, slowest, verdict):
    source = path.with_suffix(".preparation.csv").read_text()
    if verdict not in source:
        raise RuntimeError("incomplete / lossy diagnostic preparation sidecar")
    spans = list(csv.DictReader(line for line in source.splitlines() if not line.startswith("#")))
    if not spans:
        raise RuntimeError("no preparation spans")
    phases = {"prepare", "ensure-index", "font-init", "wrap-preview", "shape-visible",
              "shape-row", "shape-line", "measure-grapheme", "wrap-worker", "shape-worker", "initial-worker"}
    for index, row in enumerate(spans):
        start, end = float(row["start_ms"]), float(row["end_ms"])
        if (row["id"] != str(index) or row["lane"] not in ("foreground", "wrap-worker", "shape-worker", "initial-worker")
                or row["phase"] not in phases or row["complete"] not in ("true", "false")
                or not math.isfinite(start) or not math.isfinite(end) or end < start
                or abs(end - start - metric(row, "total_ms")) > .00001
                or not row["bytes"].isdigit()):
            raise RuntimeError(f"invalid preparation span: {row}")
        if row["parent"]:
            parent = int(row["parent"])
            if not 0 <= parent < index:
                raise RuntimeError(f"invalid preparation parent: {row}")
            parent = spans[parent]
            if (parent["lane"] != row["lane"] or float(parent["start_ms"]) > start
                    or float(parent["end_ms"]) < end):
                raise RuntimeError(f"preparation span escapes parent: {row}")
        if row["frame"]:
            frame = int(row["frame"])
            if not 0 <= frame < len(frames):
                raise RuntimeError(f"invalid preparation frame: {row}")
            frame = frames[frame]
            if (row["lane"] != "foreground" or start < float(frame["draw_start_ms"])
                    or end > float(frame["draw_end_ms"])):
                raise RuntimeError(f"preparation span escapes frame: {row}")
    selected = [] if slowest is None else [row for row in spans if row["frame"] == slowest["frame"]]
    workers = [] if slowest is None else [row for row in spans
        if row["phase"] in ("wrap-worker", "shape-worker", "initial-worker")
        and float(row["start_ms"]) < float(slowest["draw_end_ms"])
        and float(row["end_ms"]) > float(slowest["draw_start_ms"])]
    totals = {}
    for row in selected:
        stats = totals.setdefault(row["phase"], {"count": 0, "total_ms": 0, "max_ms": 0})
        duration = metric(row, "total_ms")
        stats["count"] += 1
        stats["total_ms"] += duration
        stats["max_ms"] = max(stats["max_ms"], duration)
    return {
        "spans_in_slowest_frame": selected, "phases_in_slowest_frame": totals,
        "workers_overlapping_slowest_frame": workers,
        "initial_preparation_workers": [row for row in spans if row["phase"] == "initial-worker"],
        "interpretation": "nested wall-clock totals overlap; worker overlap does not prove lock contention; incomplete worker end is cutoff, not completion",
    }


def read_open_diagnostic(path, software, succeeded):
    content = path.read_text()
    renderer = "is_software_emulated: " + str(software).lower()
    if (renderer not in content or "# open_diagnostic=true; acceptance=false" not in content
            or "# mode=m2; entries=2000; cycles=0; steps=2;" not in content
            or f"# completed={str(succeeded).lower()};" not in content):
        raise RuntimeError("unexpected renderer, mode or verdict in open diagnostic")
    rows = list(csv.DictReader(line for line in content.splitlines() if not line.startswith("#")))
    if succeeded:
        validate_m2_report(content, rows, software, open_diagnostic=True)
    elif not rows or [r["phase"] for r in rows] not in (["boot"], ["boot", "open"]) or rows[-1]["status"] != "failed":
        raise RuntimeError("invalid partial diagnostic coverage")
    traces = {}
    verdict = f"# completed_open={str(succeeded).lower()}; lost_entries=0; acceptance=false"
    for name in ("frames", "views"):
        source = path.with_suffix(f".{name}.csv").read_text()
        if verdict not in source:
            raise RuntimeError(f"incomplete / lossy diagnostic {name} sidecar")
        traces[name] = list(csv.DictReader(line for line in source.splitlines() if not line.startswith("#")))
    frames = traces["frames"]
    if not frames:
        raise RuntimeError("no diagnostic frames")
    measured = {}
    for index, row in enumerate(frames):
        if row["frame"] != str(index) or row["phase"] not in ("boot", "open") or row["stage"] not in ("setup", "measured"):
            raise RuntimeError(f"invalid diagnostic frame identity: {row}")
        if row["step"] != ("0" if row["phase"] == "boot" else "1"):
            raise RuntimeError(f"invalid diagnostic step: {row}")
        values = {key: metric(row, key) for key in (
            "draw_cpu_ms", "request_layout_ms", "prepaint_ms", "paint_ms", "draw_other_ms",
            "submit_cpu_ms", "draw_to_submit_gap_ms",
        )}
        phases = sum(values[key] for key in ("request_layout_ms", "prepaint_ms", "paint_ms", "draw_other_ms"))
        if abs(values["draw_cpu_ms"] - phases) > .00001:
            raise RuntimeError(f"diagnostic phases do not account for draw: {row}")
        if row["target_match"] not in ("", "true", "false"):
            raise RuntimeError(f"invalid diagnostic target observation: {row}")
        if row["stage"] == "measured":
            measured.setdefault(row["phase"], []).append(row)
    for row in traces["views"]:
        index = int(row["frame"])
        if not 0 <= index < len(frames) or row["view_phase"] not in ("request_layout", "prepaint", "paint"):
            raise RuntimeError(f"invalid slow-view frame association: {row}")
        frame = frames[index]
        if (row["step"], row["phase"]) != (frame["step"], frame["phase"]):
            raise RuntimeError(f"invalid slow-view step association: {row}")
        if metric(row, "self_ms") > metric(row, "total_ms") or metric(row, "total_ms") > metric(frame, "draw_cpu_ms") + .00001:
            raise RuntimeError(f"invalid slow-view durations: {row}")
    for row in rows:
        phase_frames = measured.get(row["phase"], [])
        if len(phase_frames) != int(row["frames"]):
            raise RuntimeError("diagnostic and aggregate frame counts differ")
        maximum = max((metric(frame, "draw_cpu_ms") for frame in phase_frames), default=0)
        if abs(maximum - metric(row, "draw_cpu_max_ms")) > .000501:
            raise RuntimeError("diagnostic and aggregate max draw differ")
    opens = measured.get("open", [])
    slowest = max(opens, key=lambda row: metric(row, "draw_cpu_ms")) if opens else None
    slow_views = [] if slowest is None else sorted(
        (row for row in traces["views"] if row["frame"] == slowest["frame"]),
        key=lambda row: metric(row, "self_ms"), reverse=True,
    )
    summary = {
        "diagnostic_only": True, "acceptance": False, "process_succeeded": succeeded,
        "software_diagnostic": software, "frames": len(frames),
        "slowest_open_frame": slowest, "slow_views_in_that_frame": slow_views,
        "view_timing_floor_ms": .5, "gpu_execution": "unmeasured",
        "compositor_presentation": "unmeasured",
    }
    summary["preparation"] = (preparation_summary(path, frames, slowest, verdict)
                              if "# preparation_trace_version=1" in path.with_suffix(".frames.csv").read_text()
                              else None)
    path.with_suffix(".diagnostic.json").write_text(json.dumps(summary, indent=2) + "\n")
    return rows


SPLIT_PHASES = (
    ["boot", "split-and-select", "peer-scroll", "select-first", "insert", "delete",
     "newline", "join", "burst-insert", "burst-delete"]
    + ["scroll"] * 30 + ["resize", "font"] + ["forced"] * 30 + ["idle"]
)


def validate_split_report(content, rows, software, direction, sample):
    if (sample not in SAMPLES or f"# mode=split; direction={direction}; sample={sample}; release=true; graphics=" not in content):
        raise RuntimeError("split report mode / direction / release mismatch")
    verdict = "not-applicable" if software else "true"
    if (f"selected_budget_passed={verdict}; all_view_budget=not-established;" not in content
            or [row["phase"] for row in rows] != SPLIT_PHASES):
        raise RuntimeError("incomplete or reordered split report / verdict")
    selected = [row.get("selected_view") for row in rows]
    if (not all(selected) or selected[0] != selected[3]
            or selected[1] == selected[0] or selected[2] != selected[1]
            or any(value != selected[0] for value in selected[3:])):
        raise RuntimeError("split view selection identity mismatch")
    for index, row in enumerate(rows):
        phase = row["phase"]
        values = {key: metric(row, key) for key in (
            "views", "action_cpu_ms", "draw_cpu_max_ms", "frames", "missing_observations",
            "blank_frames", "idle_ticks", "min_visible_rows", "fully_indexed_views",
            "bytes", "width_px", "height_px", "observed_elapsed_ms",
        )}
        views = 1 if index == 0 else 2
        if (values["views"] != views or values["fully_indexed_views"] != views
                or values["missing_observations"] or values["blank_frames"]
                or values["min_visible_rows"] < 1 or values["bytes"] < 8 * 1024 * 1024
                or not values["width_px"] or not values["height_px"]
                or row.get("shared_text_verified") != "true"):
            raise RuntimeError(f"split body / shared text verification failed: {row}")
        limits = {"draw_cpu_max_ms": 16}
        if phase == "idle":
            if (row.get("status") != "idle" or values["frames"] != 0
                    or values["idle_ticks"] != 30 or any(row.get(key) != "" for key in (
                        "first_frame_submit_ms", "first_submit_ms", "ready_submit_ms", "all_ready_submit_ms"))):
                raise RuntimeError(f"invalid split idle observation: {row}")
        else:
            for key in ("first_frame_submit_ms", "first_submit_ms", "ready_submit_ms", "all_ready_submit_ms"):
                values[key] = metric(row, key)
            if (row.get("status") != "ready" or values["frames"] < 1
                    or values["frames"] != int(values["frames"]) or values["idle_ticks"] != 0
                    or not (values["first_frame_submit_ms"] <= values["first_submit_ms"]
                            <= values["ready_submit_ms"] <= values["all_ready_submit_ms"]
                            <= values["observed_elapsed_ms"])):
                raise RuntimeError(f"invalid split submission boundaries: {row}")
            if phase == "boot":
                limits.update(first_submit_ms=250, ready_submit_ms=500)
            elif phase in ("insert", "delete", "newline", "join"):
                limits.update(action_cpu_ms=2, first_submit_ms=16, ready_submit_ms=300)
            elif phase in ("burst-insert", "burst-delete"):
                limits.update(action_cpu_ms=50, first_submit_ms=50, ready_submit_ms=300)
            elif phase in ("peer-scroll", "scroll"):
                limits["first_submit_ms"] = 4
            elif phase == "forced":
                limits["first_submit_ms"] = 2
            elif phase in ("resize", "font"):
                limits.update(first_frame_submit_ms=50, ready_submit_ms=300)
        check_limits(row, values, limits, software)



SYNTAX_SAMPLES = ("markdown", "html")
SYNTAX_PHASES = ["open", "insert", "delete", "newline", "join", "burst-insert", "burst-delete"] + ["forced"] * 30 + ["idle"]
SYNTAX_FIELDS = (
    "phase,step,bytes,point,target_buffer,target_version,painted_buffer,painted_version,"
    "syntax_buffer,syntax_version,syntax_pending_before_action,syntax_pending_frames,"
    "first_frame_submit_ms,first_submit_ms,ready_submit_ms,syntax_and_layout_submit_ms,"
    "action_cpu_ms,draw_cpu_max_ms,frames,missing_observations,blank_frames,idle_ticks,"
    "visible_rows,indexed_bytes,width_px,height_px,first_visible_rows,text_verified,syntax_status,observed_elapsed_ms"
).split(",")


def syntax_input_offset(data, sample):
    prefix = b"\x60\x60\x60html\n" if sample == "markdown" else b""
    marker = prefix + b"<script>const n = call(42);</script><style>"
    try:
        return data.index(marker) + len(prefix + b"<script>const n = ")
    except ValueError as error:
        raise RuntimeError("missing embedded script input marker") from error


def verify_syntax_batch(directory, expected_sha256):
    manifest = directory / "syntax-fixtures.json"
    if manifest.is_symlink() or not manifest.is_file():
        raise RuntimeError("missing regular syntax batch manifest")
    raw = manifest.read_bytes()
    if hashlib.sha256(raw).hexdigest() != expected_sha256:
        raise RuntimeError("syntax batch manifest identity changed")
    metadata = json.loads(raw)
    if (not isinstance(metadata, dict) or set(metadata) != {"schema", "files"}
            or metadata["schema"] != 1 or not isinstance(metadata["files"], dict)
            or set(metadata["files"]) != {"markdown.md", "html.html"}):
        raise RuntimeError("unexpected syntax batch fixture manifest")
    for name, digest in metadata["files"].items():
        path = directory / name
        if (not isinstance(digest, str) or re.fullmatch(r"[0-9a-f]{64}", digest) is None
                or path.is_symlink() or not path.is_file()
                or hashlib.sha256(path.read_bytes()).hexdigest() != digest):
            raise RuntimeError(f"syntax batch fixture identity changed: {name}")
    return metadata["files"]


def syntax_fixture(path, sample):
    extension = {"markdown": ".md", "html": ".html"}.get(sample)
    data = path.read_bytes()
    marker = b"<script>const n = call(42);</script><style>"
    if (extension is None or path.suffix != extension
            or not 256 * 1024 <= len(data) < 257 * 1024 or marker not in data
            or (sample == "markdown" and b"\x60\x60\x60html\n" + marker not in data)):
        raise RuntimeError("expected representative 256 KiB Markdown/HTML syntax fixture")
    data.decode("utf-8")
    syntax_input_offset(data, sample)
    return data


def count(row, key):
    value = row.get(key, "")
    if not isinstance(value, str) or re.fullmatch(r"[0-9]+", value) is None:
        raise RuntimeError(f"invalid integer {key}: {row}")
    return int(value)


def validate_syntax_report(content, rows, software, sample, fixture):
    offset = syntax_input_offset(fixture, sample)
    header = f"# mode=syntax; sample={sample}; release=true; fixture_bytes={len(fixture)}; input_offset={offset}; graphics="
    if not any(line.startswith(header) for line in content.splitlines()):
        raise RuntimeError("syntax mode / sample / release / fixture identity mismatch")
    if [row.get("phase") for row in rows] != SYNTAX_PHASES:
        raise RuntimeError("incomplete or reordered syntax phase matrix")
    if any(set(row) != set(SYNTAX_FIELDS) for row in rows):
        raise RuntimeError("unexpected syntax report columns")
    target_buffer = rows[0]["target_buffer"]
    if re.fullmatch(r"BufferId\([1-9][0-9]*\)", target_buffer) is None:
        raise RuntimeError("missing syntax buffer identity")
    deltas = [0, 1, 0, 0, -1, 31, -1]
    versions = [0, 1, 2, 3, 4, 36, 68]
    pending_inputs = 0
    for index, row in enumerate(rows):
        delta = deltas[min(index, 6)]
        version = f"TextVersion({versions[min(index, 6)]})"
        if (count(row, "step") != index or count(row, "bytes") != len(fixture) + delta
                or count(row, "point") != offset + delta
                or row["target_buffer"] != target_buffer or row["target_version"] != version
                or row["painted_buffer"] != target_buffer or row["painted_version"] != version
                or row["text_verified"] != "true"):
            raise RuntimeError(f"syntax target / workload / text verification mismatch: {row}")
        if row["syntax_pending_before_action"] not in ("true", "false"):
            raise RuntimeError(f"missing syntax pending state: {row}")
        pending = row["syntax_pending_before_action"] == "true"
        if index >= 7 and pending:
            raise RuntimeError("stable draw / idle started before syntax settled")
        if index > 0 and rows[index - 1]["syntax_status"] == "ready" and pending:
            raise RuntimeError("prior syntax was already ready; pending input claim is contradictory")
        if 1 <= index <= 6 and pending:
            pending_inputs += 1
        frames = count(row, "frames")
        pending_frames = count(row, "syntax_pending_frames")
        if (count(row, "missing_observations") or count(row, "blank_frames")
                or pending_frames > frames or count(row, "visible_rows") == 0
                or count(row, "indexed_bytes") != len(fixture) + delta):
            raise RuntimeError(f"missing syntax body / frame coverage: {row}")
        combined = metric(row, "syntax_and_layout_submit_ms", optional=True)
        if index == len(rows) - 1:
            if (row["syntax_status"] != "idle" or frames != 0 or count(row, "idle_ticks") != 30
                    or pending_frames != 0 or combined is not None
                    or any(row[key] for key in ("first_frame_submit_ms", "first_submit_ms", "ready_submit_ms"))):
                raise RuntimeError("syntax idle coverage mismatch")
        else:
            if (count(row, "idle_ticks") or not count(row, "first_visible_rows")
                    or metric(row, "ready_submit_ms") > metric(row, "observed_elapsed_ms")):
                raise RuntimeError("syntax first visibility / idle ticks / timing mismatch")
            if row["syntax_status"] == "ready":
                if (pending_frames >= frames or combined is None
                        or not metric(row, "ready_submit_ms") <= combined <= metric(row, "observed_elapsed_ms")):
                    raise RuntimeError("invalid syntax/layout timing boundary")
            elif row["syntax_status"] == "not-observed" and index < 6:
                if combined is not None or pending_frames == 0:
                    raise RuntimeError("unobserved syntax cannot have a ready time")
            else:
                raise RuntimeError("final syntax completion missing")
        if row["syntax_status"] in ("ready", "idle"):
            if row["syntax_buffer"] != target_buffer or row["syntax_version"] != version:
                raise RuntimeError("syntax snapshot does not match current committed target")
        elif row["syntax_buffer"] or row["syntax_version"]:
            raise RuntimeError("unobserved syntax must not invent a snapshot")
    validate_native_metrics(rows, software, False)
    footer = f"# completed=true; input_while_syntax_pending={pending_inputs}; layout_budget_separate=true; syntax_ready_budget=not-established; gpu_execution_ms=unmeasured; compositor_presentation_ms=unmeasured"
    if not pending_inputs or footer not in content.splitlines():
        raise RuntimeError("coverage missing: input while prior syntax pending / completion verdict")


DISPLAY_PHASES = (
    ["open"] + ["scroll"] * 30 + ["resize", "font"] + ["forced"] * 30
    + ["idle", "click", "insert", "delete", "newline", "join", "burst-insert", "burst-delete"]
)
DISPLAY_DIAGNOSTIC_HEADER = "# mode=display-diagnostic; acceptance=false; stage_timeout_ms=2000"


def diagnostic_findings(content):
    return [(int(row), reason) for row, reason in re.findall(
        r"^# budget_violation=row (\d+): (.+)$", content, re.MULTILINE)]


def validate_display_diagnostic(content, rows, software):
    if DISPLAY_DIAGNOSTIC_HEADER not in content.splitlines():
        raise RuntimeError("missing display diagnostic identity")
    if [row["phase"] for row in rows] != DISPLAY_PHASES:
        raise RuntimeError("incomplete or reordered display diagnostic")
    findings = diagnostic_findings(content)
    counts = [0] * len(rows)
    for index, reason in findings:
        if not 1 <= index <= len(rows) or not reason.startswith(rows[index - 1]["phase"] + ":"):
            raise RuntimeError("invalid diagnostic violation row")
        counts[index - 1] += 1
    for row, count in zip(rows, counts):
        expected = "over-budget" if count else "ok"
        if row.get("status") != expected:
            raise RuntimeError("diagnostic status / violation mismatch")
        errors = validate_native_metrics([row], software, False, diagnostic=True)
        if errors and not count:
            raise RuntimeError("diagnostic omitted budget failure")
        if row["phase"] != "idle" and metric(row, "visible_rows") == 0:
            raise RuntimeError("diagnostic missing body")
    verdict = "not-applicable" if software else ("false" if findings else "true")
    footer = f"# completed=true; budget_passed={verdict}; acceptance=false; budget_violation_count={len(findings)}; gpu_execution_ms=unmeasured; compositor_presentation_ms=unmeasured"
    if footer not in content.splitlines():
        raise RuntimeError("invalid diagnostic completion / budget verdict")


def wait_display_diagnostic(process, lines):
    deadline = time.monotonic() + 5
    stage = 0
    active = False
    while True:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            label = DISPLAY_PHASES[stage] if stage < len(DISPLAY_PHASES) else "shutdown"
            raise RuntimeError(f"STOP display diagnostic watchdog: {label}; keeping partial report and logs")
        try:
            line = lines.get(timeout=min(remaining, 0.1))
        except queue.Empty:
            if process.poll() is not None:
                return process.returncode
            continue
        if line.startswith("STAGE "):
            if active or stage >= len(DISPLAY_PHASES) or line != "STAGE " + DISPLAY_PHASES[stage]:
                raise RuntimeError("invalid diagnostic stage progress")
            active = True
            deadline = time.monotonic() + 2
        elif line.startswith("STAGE_DONE "):
            if not active or line != "STAGE_DONE " + DISPLAY_PHASES[stage]:
                raise RuntimeError("invalid diagnostic stage completion")
            active = False
            stage += 1
            deadline = time.monotonic() + 2
        elif line.startswith("STOP "):
            return process.wait(timeout=2)


def read_report(path, software, ime, m1=None, m2=False, split=None, sample=None, syntax=False, fixture=None, display_diagnostic=False):
    content = path.read_text()
    if "# mode=display-diagnostic;" in content and not display_diagnostic:
        raise RuntimeError("display diagnostic is not an acceptance report")
    stops = [line.removeprefix("# stop_reason=") for line in content.splitlines()
             if line.startswith("# stop_reason=")]
    if stops:
        raise RuntimeError(f"STOP: {stops[-1]}; partial report {path}")
    if "# completed=true;" not in content:
        raise RuntimeError(f"incomplete report {path}")
    renderer = "is_software_emulated: true" if software else "is_software_emulated: false"
    if renderer not in content:
        raise RuntimeError(f"unexpected renderer class in {path}")
    reader = csv.DictReader(line for line in content.splitlines() if not line.startswith("#"))
    rows = list(reader)
    if display_diagnostic:
        if ime or m1 is not None or m2 or split is not None or syntax:
            raise RuntimeError("display diagnostic cannot use another workload")
        validate_display_diagnostic(content, rows, software)
        return rows
    if syntax:
        if reader.fieldnames != SYNTAX_FIELDS:
            raise RuntimeError("unexpected syntax report columns")
        validate_syntax_report(content, rows, software, sample, fixture)
        return rows
    if split is not None:
        validate_split_report(content, rows, software, split, sample)
        return rows
    if m1 is not None:
        validate_m1_report(content, rows, software, *m1)
        return rows
    if m2:
        validate_m2_report(content, rows, software)
        return rows
    expected = (
        {"open": 1, "preedit": 2, "cancel": 1, "commit": 1,
         "scroll": 1, "resize": 1, "font": 1, "click": 1} if ime else
        {"open": 1, "scroll": 30, "resize": 1, "font": 1, "forced": 30,
         "idle": 1, "click": 1, "insert": 1, "delete": 1, "newline": 1,
         "join": 1, "burst-insert": 1, "burst-delete": 1}
    )
    if any(row.get("status", "ok") != "ok" for row in rows):
        raise RuntimeError(f"failed stage in {path}")
    counts = {phase: sum(row["phase"] == phase for row in rows) for phase in expected}
    if counts != expected or len(rows) != sum(expected.values()):
        raise RuntimeError(f"wrong phase matrix in {path}: {counts}")
    for row in rows:
        if row["phase"] != "idle" and (
            not row["ready_submit_ms"] or int(row["visible_rows"]) == 0
        ):
            raise RuntimeError(f"missing body / completion in {path}: {row}")
    validate_native_metrics(rows, software, ime)
    return rows


def run_probe(args, desktop, sample, run):
    stem = args.output / f"{sample}-{run}"
    report = stem.with_suffix(".csv")
    invocation = [str(args.binary), str(args.input), sample, str(report)]
    if getattr(args, "display_diagnostic", False):
        invocation.append("--display-diagnostic")
    if args.software:
        invocation.append("--allow-software")
    if args.ime:
        invocation.append("--external")
    if getattr(args, "syntax", False):
        if not hasattr(args, "syntax_fixture"):
            args.syntax_fixture = syntax_fixture(args.input, sample)
        invocation.append("--syntax")
    if getattr(args, "m2", False):
        invocation.extend(["--m2-directory", str(stem.with_suffix(".files"))])
    if getattr(args, "m2_open_diagnostic", False):
        invocation.append("--m2-open-diagnostic")
    if getattr(args, "split", None):
        invocation.extend(["--split", args.split])
    if args.m1:
        invocation.extend(["--m1", "--duration-seconds", str(args.duration_seconds),
                           "--interval-ms", str(args.interval_ms)])
    environment = os.environ.copy()
    token = uuid.uuid4().hex
    environment["SEMACS_PERF_TOKEN"] = token
    if getattr(args, "syntax", False) or getattr(args, "display_diagnostic", False):
        environment["XDG_CONFIG_HOME"] = str(args.output / "config")
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
            status = (wait_display_diagnostic(process, lines) if getattr(args, "display_diagnostic", False)
                      else process.wait(timeout=args.duration_seconds + 120 if args.m1 else 150))
            if getattr(args, "m2_open_diagnostic", False):
                diagnostic_rows = read_open_diagnostic(report, args.software, status == 0)
            if status != 0:
                thread.join(timeout=5)
                stops = [line for line in stem.with_suffix(".stdout.log").read_text().splitlines()
                         if line.startswith("STOP ")]
                reason = stops[-1] if stops else f"probe exited {status}"
                raise RuntimeError(f"{reason}; see {stem}.stdout.log and {stem}.stderr.log")
            if getattr(args, "m2_open_diagnostic", False):
                verify_m2_files(stem.with_suffix(".files"), open_diagnostic=True)
                return diagnostic_rows
            pacing = (args.duration_seconds, args.interval_ms) if args.m1 else None
            rows = read_report(report, args.software, args.ime, pacing, getattr(args, "m2", False), getattr(args, "split", None), sample,
                               getattr(args, "syntax", False),
                               getattr(args, "syntax_fixture", None) if getattr(args, "syntax", False) else None,
                               display_diagnostic=getattr(args, "display_diagnostic", False))
            if getattr(args, "m2", False):
                verify_m2_files(stem.with_suffix(".files"))
            return rows
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
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--split", choices=("right", "below"), help="shared-buffer dual-view CPU/submission measurements; all-view ready is baseline only")
    mode.add_argument("--ime", action="store_true", help="real Wayland/fcitx5 Pinyin; requires Hyprland/wtype")
    mode.add_argument("--m2-open-diagnostic", action="store_true", help="one mixed-file open only; buffered frame / slow-view diagnostic, NOT acceptance")
    mode.add_argument("--m2", action="store_true", help="M2 file / save / switch / completion / lifecycle baseline")
    mode.add_argument("--syntax", action="store_true", help="syntax-enabled Markdown/HTML input/draw baseline; layout and syntax ready are separate")
    mode.add_argument("--display-diagnostic", action="store_true", help="one full mixed display workload; record budget failures and continue; NOT acceptance")
    mode.add_argument("--m1", action="store_true", help="M1 command chain + paced native sustained input; stop on first hardware budget failure")
    parser.add_argument("--duration-seconds", type=int, help="M1 sustained duration, 1..600; default 60")
    parser.add_argument("--interval-ms", type=int, help="M1 input cadence, 20..1000 ms; default 50")
    parser.add_argument("--sample", choices=SAMPLES + SYNTAX_SAMPLES, action="append")
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--syntax-manifest-sha256", help="frozen manifest digest shared by both syntax sample invocations")
    parser.add_argument("--hyprland-instance", default=os.environ.get("HYPRLAND_INSTANCE_SIGNATURE"))
    parser.add_argument("--binary", type=Path, default=ROOT / "target/release/examples/perf_native")
    args = parser.parse_args()
    if not args.consent:
        parser.error("--consent is required before opening desktop test windows")
    if args.runs < 1:
        parser.error("--runs must be positive")
    if args.display_diagnostic:
        if args.runs != 1 or args.sample not in (None, ["mixed"]):
            parser.error("display diagnostic requires --runs 1 and only --sample mixed")
        args.sample = ["mixed"]
    if args.m2_open_diagnostic:
        if args.runs != 1 or args.sample not in (None, ["mixed"]):
            parser.error("open diagnostic requires --runs 1 and only --sample mixed")
        args.sample = ["mixed"]
        args.m2 = True
    if not args.m1 and (args.duration_seconds is not None or args.interval_ms is not None):
        parser.error("pacing options require --m1")
    if args.m1:
        args.duration_seconds = 60 if args.duration_seconds is None else args.duration_seconds
        args.interval_ms = 50 if args.interval_ms is None else args.interval_ms
        if not 1 <= args.duration_seconds <= 600 or not 20 <= args.interval_ms <= 1000:
            parser.error("M1 requires duration 1..600 seconds and interval 20..1000 ms")
    if args.ime and (not args.hyprland_instance or not os.environ.get("WAYLAND_DISPLAY")):
        parser.error("--ime requires the actual Wayland/Hyprland session environment")
    if args.split and args.sample is None:
        args.sample = ["mixed", "single-ascii"]
    if args.syntax:
        if args.sample is None or len(args.sample) != 1 or args.sample[0] not in SYNTAX_SAMPLES:
            parser.error("--syntax requires exactly one --sample markdown|html")
        if re.fullmatch(r"[0-9a-f]{64}", args.syntax_manifest_sha256 or "") is None:
            parser.error("--syntax requires the frozen --syntax-manifest-sha256")
    elif args.syntax_manifest_sha256 is not None:
        parser.error("--syntax-manifest-sha256 requires --syntax")
    elif any(sample not in SAMPLES for sample in (args.sample or [])):
        parser.error("syntax samples require --syntax")
    args.input = args.input.resolve(strict=True)
    if args.syntax:
        try:
            extension = {"markdown": "md", "html": "html"}[args.sample[0]]
            if args.input.name != f"{args.sample[0]}.{extension}":
                raise RuntimeError("syntax input must be the selected generated batch fixture")
            args.syntax_batch = verify_syntax_batch(args.input.parent, args.syntax_manifest_sha256)
            args.syntax_fixture = syntax_fixture(args.input, args.sample[0])
        except (RuntimeError, ValueError, OSError) as error:
            parser.error(str(error))
    elif args.input.stat().st_size < 8 * 1024 * 1024:
        parser.error("R1 mixed fixture must be at least 8 MiB; use scripts/perf_sample.py")
    args.binary = args.binary.resolve(strict=True)
    args.output = args.output.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    digest = hashlib.sha256(args.input.read_bytes()).hexdigest()
    config = args.output / "config/semacs/init.js"
    if args.syntax or args.display_diagnostic:
        config.parent.mkdir(parents=True)
        with config.open("xb") as file:
            file.write(b"")
    metadata = {
        "uname": list(os.uname()), "software_diagnostic": args.software,
        "ime": args.ime, "m1": args.m1, "m2": args.m2, "split": args.split, "syntax": args.syntax,
        "syntax_ready_budget": "not-established" if args.syntax else None,
        "syntax_batch_manifest_sha256": args.syntax_manifest_sha256,
        "syntax_batch_fixtures": args.syntax_batch if args.syntax else None,
        "syntax_fixture_bytes": args.input.stat().st_size if args.syntax else None,
        "isolated_config_sha256": hashlib.sha256(b"").hexdigest() if args.syntax or args.display_diagnostic else None,
        "all_view_ready_budget": "not-established" if args.split else None,
        "m2_open_diagnostic": args.m2_open_diagnostic,
        "display_diagnostic": args.display_diagnostic,
        "stage_timeout_ms": 2000 if args.display_diagnostic else None,
        "acceptance": not (args.m2_open_diagnostic or args.split or args.syntax or args.display_diagnostic),
        "measurement_scope": "selected-view-budgets-and-all-view-baseline" if args.split else None,
        "duration_seconds": args.duration_seconds,
        "interval_ms": args.interval_ms, "runs": args.runs, "samples": args.sample or list(SAMPLES),
        "fixture_sha256": digest,
        "git_head": command("git", "-C", str(ROOT), "rev-parse", "HEAD"),
        "git_status": command("git", "-C", str(ROOT), "status", "--short"),
        "binary_sha256": hashlib.sha256(args.binary.read_bytes()).hexdigest(),
        "driver_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        "fontconfig_monospace": command("fc-match", "-f", "%{family}\n%{file}\n", "monospace"),
        "fontconfig_bold": command("fc-match", "-f", "%{family}\n%{file}\n", "monospace:style=Bold"),
        "completed": False, "restored": False,
    }
    desktop = None
    values = {}
    try:
        desktop = Desktop(args.hyprland_instance, args.ime)
        for sample in metadata["samples"]:
            for run in range(1, args.runs + 1):
                if args.syntax:
                    verify_syntax_batch(args.input.parent, args.syntax_manifest_sha256)
                rows = run_probe(args, desktop, sample, run)
                if args.syntax:
                    verify_syntax_batch(args.input.parent, args.syntax_manifest_sha256)
                    if args.input.read_bytes() != args.syntax_fixture:
                        raise RuntimeError("syntax fixture changed; refusing subsequent probes")
                    if config.is_symlink() or config.read_bytes() != b"":
                        raise RuntimeError("isolated syntax configuration changed; refusing subsequent probes")
                if args.display_diagnostic:
                    content = (args.output / f"{sample}-{run}.csv").read_text()
                    metadata["budget_violations"] = diagnostic_findings(content)
                    metadata["budget_passed"] = ("not-applicable" if args.software else
                                                 not metadata["budget_violations"])
                if args.m2 and not args.m2_open_diagnostic:
                    metadata.setdefault("m2_coverage", []).append({
                        "sample": sample, "run": run,
                        "save_ack_before_edit": next(row["save_ack_before_edit"] for row in rows if row["phase"] == "save-edit"),
                        "closed_views_retained_at_end": rows[-1]["retained_closed_views"],
                        "scratch_buffers_at_end": rows[-1]["scratch_buffers"],
                        "file_buffers_at_end": rows[-1]["file_buffers"],
                        "message_buffers_at_end": rows[-1]["message_buffers"],
                        "disk_bytes_verified": True,
                    })
                if args.m1:
                    sustained = [row for row in rows if row["phase"] == "sustained"]
                    metadata.setdefault("m1_coverage", []).append({
                        "sample": sample, "run": run, "keys": len(sustained),
                        "ready": sum(row["status"] == "ready" for row in sustained),
                        "superseded_before_ready": sum(row["status"] == "superseded-before-ready" for row in sustained),
                        "superseded_before_first": sum(row["status"] == "superseded-before-first" for row in sustained),
                    })
                for row in rows:
                    for key in (
                        "first_submit_ms", "ready_submit_ms", "draw_cpu_max_ms",
                        "action_cpu_ms", "blank_frames", "first_frame_submit_ms", "schedule_lag_ms",
                        "all_ready_submit_ms", "syntax_and_layout_submit_ms", "syntax_pending_frames",
                        "save_action_cpu_ms", "save_observed_ms", "result_observed_ms",
                        "rss_kib", "retained_closed_views",
                    ):
                        if row.get(key):
                            values.setdefault((sample, row["phase"], key), []).append(float(row[key]))
                label = "diagnostic complete (not acceptance)" if args.display_diagnostic else "complete"
                print(f"{sample} {run}/{args.runs}: {label}", flush=True)
        metadata["completed"] = True
    except Exception as error:
        metadata["error"] = str(error)
        (args.output / "STOP.txt").write_text(str(error) + "\n")
        raise
    finally:
        try:
            if desktop is not None:
                desktop.restore()
            if hashlib.sha256(args.input.read_bytes()).hexdigest() != digest:
                raise RuntimeError("fixture changed during native measurements")
            if args.syntax:
                verify_syntax_batch(args.input.parent, args.syntax_manifest_sha256)
                if config.is_symlink() or config.read_bytes() != b"":
                    raise RuntimeError("isolated syntax configuration changed")
            if args.display_diagnostic:
                if config.is_symlink() or config.read_bytes() != b"":
                    raise RuntimeError("isolated display diagnostic configuration changed")
                partial = args.output / "mixed-1.csv"
                if partial.exists():
                    metadata["budget_violations"] = diagnostic_findings(partial.read_text())
                if not metadata["completed"]:
                    metadata["budget_passed"] = (False if metadata.get("budget_violations") and not args.software
                                                 else "unmeasured")
            metadata["restored"] = desktop is not None
        except Exception as error:
            metadata["completed"] = False
            metadata["error"] = f"{metadata.get('error', '')}; {error}".lstrip("; ")
            (args.output / "STOP.txt").write_text(metadata["error"] + "\n")
            raise
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
