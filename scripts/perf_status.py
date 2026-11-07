"""Isolated F2 status / JS index-write headless baselines; never opens a desktop."""

import argparse
import csv
import hashlib
import json
import math
import os
from pathlib import Path
import subprocess

from magit_fixture import git, isolated_environment

ROOT = Path(__file__).resolve().parents[1]
SIZES = {"small": 64, "limit": 4096}
ACTIONS = ("open", "navigate", "leaf-toggle", "scroll", "split", "collapse",
           "refresh", "expand", "stage", "unstage", "unsplit")
TARGET = "unstaged-0000.txt"


def sha(data):
    return hashlib.sha256(data).hexdigest()


def body(count, staged):
    quarter = count // 4
    names = lambda prefix: [f"{prefix}-{i:04}.txt" for i in range(quarter)]
    both = [("MM", name) for name in names("both")]
    unstaged = [(".M", name) for name in names("unstaged") if not staged or name != TARGET]
    indexed = [("M.", name) for name in names("staged")]
    if staged:
        indexed.append(("M.", TARGET))
    groups = (("Untracked files", [("??", name) for name in names("untracked")]),
              ("Unstaged changes", both + unstaged),
              ("Staged changes", both + indexed), ("Unmerged paths", []))
    return "".join(f"{title} ({len(entries)})\n" +
                   "".join(f" {xy} {name}\n" for xy, name in entries)
                   for title, entries in groups).encode()


def protected_files(directory):
    files = {}
    for path in sorted(directory.rglob("*")):
        relative = path.relative_to(directory)
        if path.is_symlink():
            raise RuntimeError(f"unexpected fixture symlink: {relative}")
        if not path.is_file():
            continue
        if relative.parts[:2] == ("repo", ".git"):
            if relative.parts[2] in ("objects", "index"):
                continue
        if relative == Path("manifest.json"):
            continue
        files[str(relative)] = sha(path.read_bytes())
    return files


def prepare_fixture(directory, count):
    if count < 8 or count > 4096 or count % 4:
        raise ValueError("fixture count must be a multiple of four in 8..4096")
    directory.mkdir(parents=True, exist_ok=False)
    repo = directory / "repo"
    repo.mkdir()
    git(repo, "init", "--quiet", "--template=", "--initial-branch=main")
    for key, value in (("core.autocrlf", "false"), ("core.filemode", "false"),
                       ("core.untrackedCache", "false"), ("core.splitIndex", "false"),
                       ("commit.gpgsign", "false")):
        git(repo, "config", key, value)
    quarter = count // 4
    for prefix in ("both", "staged", "unstaged"):
        for i in range(quarter):
            (repo / f"{prefix}-{i:04}.txt").write_bytes(b"HEAD base\n")
    git(repo, "add", ".")
    git(repo, "commit", "--quiet", "-m", "baseline")
    for prefix in ("both", "staged"):
        for i in range(quarter):
            (repo / f"{prefix}-{i:04}.txt").write_bytes(b"INDEX intermediate\n")
    git(repo, "add", ".")
    for prefix in ("both", "unstaged", "untracked"):
        for i in range(quarter):
            (repo / f"{prefix}-{i:04}.txt").write_bytes(b"WORKTREE current\n")
    (directory / "base.index").write_bytes(git(repo, "ls-files", "--stage", "-z"))
    git(repo, "add", "--", TARGET)
    (directory / "stage.index").write_bytes(git(repo, "ls-files", "--stage", "-z"))
    git(repo, "reset", "--quiet", "HEAD", "--", TARGET)
    for state in ("base", "stage"):
        (directory / f"{state}.expected").write_bytes(body(count, state == "stage"))
    (directory / "head").write_bytes(git(repo, "rev-parse", "HEAD"))
    manifest = {"records": count, "projected_leaves": count + quarter,
                "protected_files": protected_files(directory)}
    encoded = (json.dumps(manifest, indent=2, sort_keys=True) + "\n").encode()
    (directory / "manifest.json").write_bytes(encoded)
    return sha(encoded)


def verify_fixture(directory, digest):
    encoded = (directory / "manifest.json").read_bytes()
    if sha(encoded) != digest:
        raise RuntimeError("fixture manifest changed")
    manifest = json.loads(encoded)
    if protected_files(directory) != manifest["protected_files"]:
        raise RuntimeError("protected fixture bytes changed")
    if git(directory / "repo", "ls-files", "--stage", "-z") != (directory / "base.index").read_bytes():
        raise RuntimeError("index entries differ from baseline")
    return manifest


def source_identity():
    result = subprocess.run(["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
                            cwd=ROOT, env=isolated_environment(), capture_output=True, check=True)
    files = {}
    for raw in result.stdout.split(b"\0"):
        if raw:
            name = os.fsdecode(raw)
            path = ROOT / name
            files[name] = sha(path.read_bytes()) if path.is_file() else "missing"
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT,
                                   env=isolated_environment()).decode().strip()
    return {"head": head, "files": files}


def read_report(path):
    lines = path.read_text().splitlines()
    if "# completed=true; baseline_only=true" not in lines:
        raise RuntimeError("incomplete status report")
    rows = list(csv.DictReader(line for line in lines if not line.startswith("#")))
    if tuple(row["phase"] for row in rows) != ACTIONS:
        raise RuntimeError("wrong status action sequence")
    for i, row in enumerate(rows):
        views = 2 if 4 <= i <= 9 else 1
        if (row["verified"] != "true" or int(row["views"]) != views
                or int(row["projection_rows"]) <= 0 or int(row["bytes"]) <= 0
                or int(row["frames"]) <= 0 or int(row["min_visible_rows"]) <= 0):
            raise RuntimeError("invalid status body / projection verification")
        values = {}
        for key in ("action_cpu_ms", "first_ms", "selected_ready_ms", "all_ready_ms", "probe_draw_max_ms"):
            values[key] = float(row[key])
            if not math.isfinite(values[key]) or values[key] < 0:
                raise RuntimeError(f"invalid status metric: {key}")
        if not values["action_cpu_ms"] <= values["first_ms"] <= values["selected_ready_ms"] <= values["all_ready_ms"]:
            raise RuntimeError("status metric endpoints out of order")
        if int(row["write_records"]) != (1 if i == 8 else 2 if i >= 9 else 0):
            raise RuntimeError("missing / duplicate core write record")
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path, help="NEW output directory")
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    metadata = {"completed": False, "acceptance": False, "latency_budget": "not-established",
                "platform": "TestPlatform / TestTextSystem", "runs": 3,
                "new_process_each_run": True, "explicit_font_warmup": False,
                "observation": "after run_until_parked + draw; scheduler may coalesce readiness",
                "probe_draw": "explicit draw calls only; excludes scheduler-internal paints",
                "preparation": "M-x command entry, point positioning and single-status-window normalization after open outside phase timers",
                "cache": "OS page cache uncontrolled; fixture setup precedes measurement",
                "native_draw_scroll_idle": "unmeasured", "gpu_presentation": "unmeasured",
                "uname": list(os.uname())}
    try:
        identity = source_identity()
        (output / "source-identity.json").write_text(json.dumps(identity, indent=2) + "\n")
        metadata["source_identity_sha256"] = sha((output / "source-identity.json").read_bytes())
        with (output / "build.log").open("x") as log:
            subprocess.run(["cargo", "build", "--release", "--example", "perf_status",
                            "--target-dir", str(ROOT / "target")], cwd=ROOT,
                           stdout=log, stderr=log, check=True)
        binary = ROOT / "target/release/examples/perf_status"
        binary_digest = sha(binary.read_bytes())
        metadata["binary_sha256"] = binary_digest
        environment = isolated_environment()
        environment["XDG_CONFIG_HOME"] = str(output / "config")
        config = output / "config/semacs/init.js"
        config.parent.mkdir(parents=True)
        config.write_bytes(b"")
        metadata["fixtures"] = {}
        summary = []
        for size, count in SIZES.items():
            for run in range(1, 4):
                stem = f"{size}-{run}"
                fixture = output / stem
                digest = prepare_fixture(fixture, count)
                metadata["fixtures"][stem] = digest
                verify_fixture(fixture, digest)
                if source_identity() != identity or sha(binary.read_bytes()) != binary_digest:
                    raise RuntimeError("source / binary changed during batch")
                with (output / f"{stem}.stdout").open("x") as stdout, (output / f"{stem}.stderr").open("x") as stderr:
                    subprocess.run([str(binary), str(fixture), str(output / f"{stem}.csv")],
                                   cwd=ROOT, env=environment, stdout=stdout, stderr=stderr,
                                   timeout=180, check=True)
                verify_fixture(fixture, digest)
                if (source_identity() != identity or sha(binary.read_bytes()) != binary_digest
                        or config.is_symlink() or config.read_bytes() != b""):
                    raise RuntimeError("source / binary / isolated config changed")
                summary.extend(dict(row, sample=size, run=run) for row in read_report(output / f"{stem}.csv"))
                (output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
        metadata["completed"] = True
    except (RuntimeError, OSError, ValueError, KeyError, csv.Error, subprocess.SubprocessError) as error:
        metadata["stop_reason"] = str(error)
        (output / "STOP.txt").write_text(str(error) + "\n")
        raise SystemExit(f"STOP: {error}; keep {output}")
    finally:
        (output / "environment.json").write_text(json.dumps(metadata, indent=2) + "\n")
    print(f"COMPLETED {output}; headless status / writes are baseline only, not native budget acceptance")


if __name__ == "__main__":
    main()
