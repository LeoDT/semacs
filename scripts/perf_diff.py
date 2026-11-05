"""Isolated F1 diff CPU baselines; native windows require separate consent."""

import argparse
import csv
import hashlib
import json
import os
from pathlib import Path
import queue
import re
import subprocess
import threading
import time

from magit_fixture import git, isolated_environment

ROOT = Path(__file__).resolve().parents[1]
SIZES = {"small": 16, "large": 128}
KINDS = ("staged", "unstaged")
ACTIONS = ("open", "locate", "collapse", "expand", "navigate", "scroll",
           "split", "locate", "collapse", "refresh", "expand", "idle", "unsplit")
DIFF_ARGS = ("--no-ext-diff", "--no-textconv", "--no-color", "--no-relative",
             "--no-prefix", "--diff-algorithm=myers", "--no-indent-heuristic",
             "--word-diff=none")


def sha(data):
    return hashlib.sha256(data).hexdigest()


def document(file, version):
    lines = []
    for row in range(96):
        tag = ("HEAD", "INDEX", "WORKTREE")[version] if row in (8, 40, 72) else "context"
        width = 4096 if row == 72 else 384
        prefix = f"{tag} file={file:04} row={row:03} 中文 alpha "
        lines.append(prefix + "0123456789abcdef" * ((width - len(prefix.encode())) // 16) + "\n")
    return "".join(lines).encode()


def diff_output(repo, kind, *args):
    return git(repo, "diff", *DIFF_ARGS, *(("--cached",) if kind == "staged" else ()), *args, "--")


def expected_body(repo, kind):
    stat = diff_output(repo, kind, "--numstat").decode().splitlines()
    result = [f"{len(stat)} files changed\n"]
    for entry in stat:
        added, deleted, name = entry.split("\t")
        result.append(f" {name} | +{added} -{deleted}\n")
    patch = diff_output(repo, kind, "--patch").decode()
    for block in patch.split("diff --git ")[1:]:
        lines = block.splitlines(keepends=True)
        match = re.fullmatch(r"(file-\d{4}\.txt) \1\n", lines[0])
        if not match:
            raise RuntimeError("unexpected fixture diff path")
        result.append(f"M {match[1]}\n")
        first = next(i for i, line in enumerate(lines) if line.startswith("@@ "))
        result.extend(lines[first:])
    return "".join(result).encode()


def tree_identity(directory):
    paths = sorted(directory.rglob("*"))
    if directory.is_symlink() or any(path.is_symlink() for path in paths):
        raise RuntimeError("fixture contains a symlink")
    return {str(path.relative_to(directory)): sha(path.read_bytes())
            for path in paths if path.is_file()}


def prepare_fixture(directory, size):
    directory.mkdir(parents=True, exist_ok=False)
    repo = directory / "repo"
    repo.mkdir()
    git(repo, "init", "--quiet", "--initial-branch=perf-diff", "--object-format=sha1", "--template=")
    for version in range(3):
        for file in range(SIZES[size]):
            (repo / f"file-{file:04}.txt").write_bytes(document(file, version))
        if version < 2:
            git(repo, "add", "--", ".")
        if version == 0:
            git(repo, "commit", "--quiet", "-m", "deterministic diff baseline")
    metrics = {}
    for kind in KINDS:
        patch = diff_output(repo, kind, "--patch")
        # Limits describe actual Git patch output, not repository/source size.
        target = (256 if size == "small" else 2048) * 1024
        if not target * 0.9 <= len(patch) <= target * 1.2:
            raise RuntimeError(f"unexpected {size} patch size: {len(patch)}")
        if len(patch.splitlines()) >= 100000 or len(patch) >= 16 * 1024 * 1024:
            raise RuntimeError("fixture exceeds production resource bounds")
        (directory / f"{kind}.patch").write_bytes(patch)
        body = expected_body(repo, kind)
        (directory / f"{kind}.expected").write_bytes(body)
        lines = body.decode().splitlines()
        headings = [i for i, line in enumerate(lines) if line.startswith("@@ ")]
        (directory / f"{kind}.projection").write_text(f"{headings[0] + 1} {headings[1]}\n")
        metrics[kind] = {"patch_bytes": len(patch), "patch_lines": len(patch.splitlines()),
                         "files": SIZES[size], "hunks": SIZES[size] * 3}
    manifest = {"size": size, "metrics": metrics, "files": tree_identity(directory)}
    encoded = (json.dumps(manifest, indent=2, sort_keys=True) + "\n").encode()
    (directory / "manifest.json").write_bytes(encoded)
    return sha(encoded)


def verify_fixture(directory, digest):
    encoded = (directory / "manifest.json").read_bytes()
    if sha(encoded) != digest:
        raise RuntimeError("fixture manifest changed")
    manifest = json.loads(encoded)
    actual = tree_identity(directory)
    actual.pop("manifest.json", None)
    if actual != manifest["files"]:
        raise RuntimeError("fixture/HEAD/index/config/oracle bytes changed")
    for kind in KINDS:
        if diff_output(directory / "repo", kind, "--patch") != (directory / f"{kind}.patch").read_bytes():
            raise RuntimeError("Git comparison differs from frozen patch")
    return manifest


def source_snapshot():
    def capture(*args):
        return subprocess.check_output(args, cwd=ROOT, env=isolated_environment())
    head = capture("git", "rev-parse", "HEAD")
    status = capture("git", "status", "--short")
    patch = capture("git", "diff", "--no-ext-diff", "--binary", "HEAD")
    for name in capture("git", "ls-files", "--others", "--exclude-standard", "-z").split(b"\0"):
        if not name:
            continue
        result = subprocess.run(["git", "diff", "--no-ext-diff", "--binary", "--no-index",
                                 "--", "/dev/null", os.fsdecode(name)], cwd=ROOT, capture_output=True,
                                env=isolated_environment())
        if result.returncode not in (0, 1):
            raise RuntimeError("cannot archive untracked source")
        patch += result.stdout
    return {"git-head.txt": head, "git-status.txt": status, "source.patch": patch}


def source_identity(output):
    snapshot = source_snapshot()
    for name, data in snapshot.items():
        (output / name).write_bytes(data)
    return {name: sha(data) for name, data in snapshot.items()}


def number(row, key):
    value = float(row[key])
    if not 0 <= value < float("inf"):
        raise RuntimeError(f"invalid {key}: {row[key]}")
    return value


def row_violations(row):
    violations = []
    draw = number(row, "draw_cpu_max_ms")
    if draw > 16:
        violations.append(f"draw_cpu_max_ms={draw:.6f} >16")
    if row["phase"] == "scroll" and number(row, "first_ms") > 4:
        violations.append(f"first_ms={number(row, 'first_ms'):.6f} >4")
    if row["phase"] == "idle" and int(row["frames"]) > 0:
        violations.append(f"idle_frames={int(row['frames'])} >0")
    return violations


def display_identity(value):
    identity = re.fullmatch(
        r"Some\(DisplayId \{ committed: SnapshotId \{ buffer: BufferId\((\d+)\), version: TextVersion\((\d+)\) \}, session: SessionToken \{ target: BufferId\((\d+)\), serial: (\d+) \}, revision: (\d+) \}\)", value)
    if not identity or identity[1] != identity[3] or min(int(identity[i]) for i in (1, 4)) <= 0:
        raise RuntimeError("invalid target display identity")
    return identity[1], int(identity[2])


def read_report(path, native, diagnostic=False):
    content = path.read_text()
    if not content.rstrip().splitlines()[-1].startswith("# completed=true;") or "# stop_reason=" in content:
        raise RuntimeError("incomplete diff report")
    if f"# mode={'native' if native else 'headless'}-diff; release=true;" not in content:
        raise RuntimeError("wrong diff report mode")
    if native and "# hardware=true; graphics=" not in content:
        raise RuntimeError("missing hardware renderer identity")
    if diagnostic:
        if not native or "# diagnostic=true; acceptance=false" not in content:
            raise RuntimeError("diagnostic must be explicit and non-acceptance")
    elif "# diagnostic=true" in content:
        raise RuntimeError("diagnostic report cannot be read as acceptance")
    rows = list(csv.DictReader(line for line in content.splitlines() if not line.startswith("#")))
    expected = [(kind, action, str(k * len(ACTIONS) + i)) for k, kind in enumerate(KINDS)
                for i, action in enumerate(ACTIONS) if native or action != "idle"]
    if [(row["kind"], row["phase"], row["step"]) for row in rows] != expected:
        raise RuntimeError("incomplete/reordered diff workload")
    identities = {}
    for row in rows:
        views = 2 if ACTIONS.index("split") <= int(row["step"]) % len(ACTIONS) < ACTIONS.index("unsplit") else 1
        violations = row_violations(row) if native else []
        if int(row["views"]) != views or row["status"] != ("over-budget" if violations else "ready"):
            raise RuntimeError("view/status mismatch")
        if row["budget_violations"] != ";".join(violations):
            raise RuntimeError("missing or incorrect budget verdict")
        if violations and not diagnostic:
            raise RuntimeError(f"STOP: {';'.join(violations)}")
        if any(int(row[key]) for key in ("blank_frames", "missing_observations")):
            raise RuntimeError("blank/missing frame")
        for key in ("action_cpu_ms", "draw_cpu_max_ms", "observed_elapsed_ms"):
            number(row, key)
        if int(row["frames"]) < 0:
            raise RuntimeError("negative frame count")
        if row["phase"] == "idle":
            if int(row["idle_ticks"]) != 30:
                raise RuntimeError("incomplete idle coverage")
            if any(row[key] for key in ("first_ms", "selected_ready_ms", "all_ready_ms")):
                raise RuntimeError("idle cannot claim latency")
        else:
            values = [number(row, key) for key in ("first_ms", "selected_ready_ms", "all_ready_ms")]
            if not values[0] <= values[1] <= values[2] <= number(row, "observed_elapsed_ms"):
                raise RuntimeError("unordered completion times")
            if int(row["frames"]) < 1:
                raise RuntimeError("missing body frames")
        if row["phase"] != "idle" or int(row["frames"]):
            if (int(row["min_visible_rows"]) <= 0 or int(row["fully_indexed_views"]) != views
                    or int(row["bytes"]) <= 0 or int(row["projection_rows"]) < int(row["min_visible_rows"])
                    or number(row, "width_px") <= 0 or number(row, "height_px") <= 0):
                raise RuntimeError("body/index/geometry/frame coverage missing")
            buffer, version = display_identity(row["target_display"])
            if version <= 0:
                raise RuntimeError("diff text version missing")
            kind = row["kind"]
            current = (buffer, version, int(row["bytes"]))
            if kind in identities:
                before = identities[kind]
                if (current[0] != before[0] or current[2] != before[2]
                        or current[1] != before[1] + int(row["phase"] == "refresh")):
                    raise RuntimeError("stale refresh or changed comparison/text identity")
            identities[kind] = current
        if not row["selected_view"] or min(number(row, key) for key in ("target_point", "before_point", "before_scroll_y")) < 0:
            raise RuntimeError("missing action target state")
        if row["text_verified"] != "true" or row["projection_verified"] != "true":
            raise RuntimeError("identity/geometry not verified")
    return rows


def read_frames(path, rows, diagnostic):
    content = path.read_text()
    if not content.rstrip().endswith("# completed=true"):
        raise RuntimeError("incomplete frame sidecar")
    frames = list(csv.DictReader(line for line in content.splitlines() if not line.startswith("#")))
    if not frames or (frames[0]["stage"], frames[0]["phase"]) != ("setup", "window-open"):
        raise RuntimeError("missing window-open frame")
    actions = {row["step"]: [] for row in rows}
    violations = []
    last_step, last_draw, last_submit = -1, -1.0, -1.0
    for index, frame in enumerate(frames):
        if frame["frame"] != str(index) or frame["stage"] not in ("setup", "action"):
            raise RuntimeError("invalid frame identity/stage")
        values = {key: number(frame, key) for key in (
            "draw_start_ms", "draw_end_ms", "draw_cpu_ms", "request_layout_ms", "prepaint_ms",
            "paint_ms", "draw_other_ms", "submit_cpu_ms", "submit_end_ms", "width_px", "height_px")}
        if (abs(values["draw_end_ms"] - values["draw_start_ms"] - values["draw_cpu_ms"]) > 0.00001
                or abs(sum(values[key] for key in ("request_layout_ms", "prepaint_ms", "paint_ms", "draw_other_ms")) - values["draw_cpu_ms"]) > 0.00001
                or values["submit_end_ms"] < values["draw_end_ms"]
                or values["submit_cpu_ms"] > values["submit_end_ms"] - values["draw_end_ms"] + 0.00001):
            raise RuntimeError("inconsistent frame timings")
        step = int(frame["step"])
        if step < last_step or values["draw_start_ms"] < last_draw or values["submit_end_ms"] < last_submit:
            raise RuntimeError("nonmonotonic frame time/stage order")
        last_step, last_draw, last_submit = step, values["draw_start_ms"], values["submit_end_ms"]
        if step == -1:
            if (frame["stage"], frame["phase"], frame["kind"]) != ("setup", "window-open", "none"):
                raise RuntimeError("invalid initial setup scope")
        elif str(step) not in actions or frame["kind"] != rows[step]["kind"]:
            raise RuntimeError("invalid frame step/kind")
        display_identity(frame["target_display"])
        if (int(frame["views"]) not in (1, 2) or not frame["selected_view"]
                or int(frame["min_visible_rows"]) <= 0 or int(frame["bytes"]) <= 0
                or not 0 <= int(frame["fully_indexed_views"]) <= int(frame["views"])
                or values["width_px"] <= 0 or values["height_px"] <= 0):
            raise RuntimeError("missing frame body/geometry evidence")
        for key in ("selected_compatible", "selected_ready", "shared_snapshot", "collapsed"):
            if frame[key] not in ("true", "false"):
                raise RuntimeError("invalid frame state")
        compatible, ready, views = (int(frame[key]) for key in ("compatible_views", "fully_indexed_views", "views"))
        if not 0 <= ready <= compatible <= views or int(frame["projection_rows"]) < int(frame["min_visible_rows"]):
            raise RuntimeError("invalid frame projection/ready counts")
        if (ready == views and (frame["selected_ready"] != "true" or frame["selected_compatible"] != "true")
                or ready == 0 and frame["selected_ready"] == frame["selected_compatible"] == "true"):
            raise RuntimeError("inconsistent selected/all ready evidence")
        if frame["stage"] == "setup" and compatible != views:
            raise RuntimeError("incompatible setup identity/projection")
        number(frame, "point")
        number(frame, "scroll_y")
        reasons = []
        if values["draw_cpu_ms"] > 16:
            reasons.append(f"draw_cpu_ms={values['draw_cpu_ms']:.6f} >16")
        if frame["stage"] == "action":
            step = frame["step"]
            if step not in actions or (frame["kind"], frame["phase"]) != (rows[int(step)]["kind"], rows[int(step)]["phase"]):
                raise RuntimeError("wrong action frame")
            if frame["views"] != rows[int(step)]["views"]:
                raise RuntimeError("wrong frame view count")
            number(frame, "action_start_ms")
            if values["draw_start_ms"] < number(frame, "action_start_ms"):
                raise RuntimeError("pre-action frame claimed as target")
            actions[step].append(frame)
            if frame["phase"] == "idle":
                reasons.append("idle_draw")
        else:
            if frame["phase"] not in ("window-open", "command-prepare", "between-actions", "before-action") or frame["action_start_ms"]:
                raise RuntimeError("invalid setup scope")
        if frame["budget_violations"] != ";".join(reasons):
            raise RuntimeError("incorrect frame budget verdict")
        if reasons:
            violations.append(dict(frame=index, step=frame["step"], kind=frame["kind"],
                                   phase=frame["phase"], reasons=reasons))
    for row in rows:
        observed = actions[row["step"]]
        if len(observed) != int(row["frames"]):
            raise RuntimeError("aggregate/sidecar frame counts differ")
        maximum = max((number(f, "draw_cpu_ms") for f in observed), default=0)
        if abs(maximum - number(row, "draw_cpu_max_ms")) > 0.00001:
            raise RuntimeError("aggregate/sidecar draw maximum differs")
        if row["phase"] != "idle":
            starts = {f["action_start_ms"] for f in observed}
            if len(starts) != 1:
                raise RuntimeError("inconsistent action start")
            def target_matches(frame):
                return (display_identity(frame["target_display"]) == display_identity(row["target_display"])
                        and frame["selected_view"] == row["selected_view"]
                        and frame["target_kind"] == row["kind"] and frame["bytes"] == row["bytes"]
                        and frame["projection_rows"] == row["projection_rows"]
                        and frame["selected_compatible"] == "true"
                        and frame["collapsed"] == str(row["phase"] in ("collapse", "refresh")).lower()
                        and int(frame["point"]) == int(row["target_point"])
                        and (row["phase"] != "navigate" or int(frame["point"]) != int(row["before_point"]))
                        and (row["phase"] != "scroll" or number(frame, "scroll_y") > number(row, "before_scroll_y")))

            for key in ("first_ms", "selected_ready_ms", "all_ready_ms"):
                candidates = [frame for frame in observed if
                              abs(number(row, key) - (number(frame, "submit_end_ms") - number(frame, "action_start_ms"))) <= 0.00001]
                if not any(target_matches(frame)
                           and (key == "first_ms" or frame["selected_ready"] == "true")
                           and (key != "all_ready_ms" or (frame["fully_indexed_views"] == row["views"]
                                and frame["compatible_views"] == row["views"] and frame["shared_snapshot"] == "true"))
                           for frame in candidates):
                    raise RuntimeError("completion not backed by exact target/ready submitted frame")
        if row["budget_violations"]:
            violations.append(dict(step=row["step"], kind=row["kind"], phase=row["phase"],
                                   reasons=row["budget_violations"].split(";")))
    passed = not violations
    main_content = path.with_name(path.name.replace(".frames.csv", ".csv")).read_text()
    if f"# budget_passed={str(passed).lower()}\n" not in main_content:
        raise RuntimeError("overall budget verdict differs from evidence")
    if not diagnostic and not passed:
        raise RuntimeError("STOP: native frame budget exceeded")
    return {"budget_passed": passed, "violations": violations, "frames": len(frames)}


def read_spans(path):
    with path.open() as source:
        rows = list(csv.DictReader(source))
    for row in rows:
        number(row, "start_ms")
        if row["complete"] != "true":
            raise RuntimeError("unfinished preparation span")
        number(row, "duration_ms")
    for kind in KINDS:
        for phase in ("open", "refresh"):
            samples = [r for r in rows if (r["kind"], r["phase"]) == (kind, phase)]
            for name in ("diff-git", "diff-parse", "diff-render", "diff-install-and-invalidate",
                         "section-projection"):
                if not any(r["span"] == name for r in samples):
                    raise RuntimeError(f"missing {kind}/{phase}/{name} span")
    return rows


def run_probe(binary, fixture, stem, native, environment, diagnostic=False):
    command = ([str(binary), "--diff", str(fixture), str(stem.with_suffix(".csv"))] if native else
               [str(binary), str(fixture), str(stem.with_suffix(".csv"))])
    if diagnostic:
        command.append("--diagnostic")
    with stem.with_suffix(".stdout.log").open("x") as stdout, stem.with_suffix(".stderr.log").open("x") as stderr:
        process = subprocess.Popen(command, cwd=ROOT, env=environment, stdout=subprocess.PIPE,
                                   stderr=stderr, text=True, bufsize=1)
        lines = queue.Queue()

        def reader():
            for line in process.stdout:
                stdout.write(line)
                stdout.flush()
                lines.put(line.rstrip())

        thread = threading.Thread(target=reader, daemon=True)
        thread.start()
        deadline = time.monotonic() + 35
        try:
            while process.poll() is None:
                try:
                    line = lines.get(timeout=0.1)
                    if line.startswith(("STAGE ", "SETUP ")):
                        deadline = time.monotonic() + 35
                except queue.Empty:
                    pass
                if time.monotonic() > deadline:
                    raise RuntimeError("probe stage watchdog expired (resource bound, not performance budget)")
            thread.join(timeout=5)
            if process.returncode:
                raise RuntimeError(f"probe exited {process.returncode}; keep partial reports and logs")
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
    rows = read_report(stem.with_suffix(".csv"), native, diagnostic)
    read_spans(stem.with_suffix(".spans.csv"))
    result = read_frames(stem.with_suffix(".frames.csv"), rows, diagnostic) if native else {"budget_passed": None}
    return rows, result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path, help="NEW directory")
    parser.add_argument("--native", action="store_true", help="three sequential hardware windows")
    parser.add_argument("--consent", action="store_true")
    parser.add_argument("--diagnostic", action="store_true", help="native only: collect all actions/windows despite budget failures; never acceptance")
    args = parser.parse_args()
    if args.diagnostic and not args.native:
        parser.error("--diagnostic requires --native")
    if args.native and not args.consent:
        parser.error("native windows require --consent AND prior approval of this batch")
    if args.native and (not os.environ.get("WAYLAND_DISPLAY") or not Path("/dev/dri").is_dir()):
        parser.error("native requires the actual hardware Wayland host; no software fallback")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    metadata = {"native": args.native, "diagnostic": args.diagnostic,
                "acceptance": args.native and not args.diagnostic, "budget_passed": None,
                "run_results": [], "completed": False, "runs": 3, "uname": list(os.uname()),
                "diff_latency_budget": "not-established", "cache": "OS page cache uncontrolled",
                "gpu_execution": "unmeasured", "screen_presentation": "unmeasured"}
    try:
        metadata["source_identity"] = source_identity(output)
        metadata["source_sha256"] = sha((output / "source.patch").read_bytes())
        example = "perf_native" if args.native else "perf_diff"
        with (output / "build.log").open("x") as log:
            subprocess.run(["cargo", "build", "--release", "--features", "native-profiler",
                            "--example", example, "--target-dir", str(ROOT / "target")],
                           cwd=ROOT, stdout=log, stderr=log, check=True)
        binary = ROOT / f"target/release/examples/{example}"
        metadata["binary_sha256"] = sha(binary.read_bytes())
        metadata["driver_sha256"] = sha(Path(__file__).read_bytes())
        config = output / "config/semacs/init.js"
        config.parent.mkdir(parents=True)
        config.write_bytes(b"")
        environment = isolated_environment()
        environment["XDG_CONFIG_HOME"] = str(output / "config")
        metadata["environment"] = {key: environment.get(key) for key in
                                   ("WAYLAND_DISPLAY", "DISPLAY", "XDG_SESSION_TYPE", "XDG_CONFIG_HOME",
                                    "LIBGL_ALWAYS_SOFTWARE", "WGPU_BACKEND")}
        sizes = ("small",) if args.native else tuple(SIZES)
        frozen = {size: prepare_fixture(output / size, size) for size in sizes}
        metadata["fixtures"] = frozen
        summary = []
        for size in sizes:
            for run in range(1, 4):
                for name, digest in frozen.items():
                    verify_fixture(output / name, digest)
                if {name: sha(data) for name, data in source_snapshot().items()} != metadata["source_identity"]:
                    raise RuntimeError("source/HEAD/status changed during batch")
                if sha(binary.read_bytes()) != metadata["binary_sha256"] or config.is_symlink() or config.read_bytes() != b"":
                    raise RuntimeError("binary/config changed")
                rows, result = run_probe(binary, output / size, output / f"{size}-{run}", args.native, environment, args.diagnostic)
                for name, digest in frozen.items():
                    verify_fixture(output / name, digest)
                if (config.is_symlink() or config.read_bytes() != b""
                        or sha(binary.read_bytes()) != metadata["binary_sha256"]
                        or {name: sha(data) for name, data in source_snapshot().items()} != metadata["source_identity"]):
                    raise RuntimeError("isolated config/binary/source changed")
                summary.extend(dict(row, sample=size, run=run) for row in rows)
                metadata["run_results"].append(dict(result, sample=size, run=run))
                (output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
        if args.native:
            metadata["budget_passed"] = all(result["budget_passed"] for result in metadata["run_results"])
        metadata["completed"] = True
    except (RuntimeError, OSError, ValueError, KeyError, TypeError, csv.Error, subprocess.SubprocessError) as error:
        metadata["stop_reason"] = str(error)
        (output / "STOP.txt").write_text(str(error) + "\n")
        raise SystemExit(f"STOP: {error}; keep {output}")
    finally:
        (output / "environment.json").write_text(json.dumps(metadata, indent=2) + "\n")
    print(f"COMPLETED {output}; diagnostic={args.diagnostic}; budget_passed={metadata['budget_passed']}; diff latency is baseline only, not complete performance acceptance")


if __name__ == "__main__":
    main()
