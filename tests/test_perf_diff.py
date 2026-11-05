import copy
import csv
import io
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import perf_diff


class FixtureTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(dir=perf_diff.ROOT / ".pi/tmp")
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name) / "fixture"
        self.digest = perf_diff.prepare_fixture(self.directory, "small")

    def test_patch_scale_content_and_both_comparisons(self):
        manifest = perf_diff.verify_fixture(self.directory, self.digest)
        for kind, old, new in (("staged", "HEAD", "INDEX"), ("unstaged", "INDEX", "WORKTREE")):
            metrics = manifest["metrics"][kind]
            self.assertEqual((metrics["files"], metrics["hunks"]), (16, 48))
            self.assertLess(metrics["patch_lines"], 100000)
            body = (self.directory / f"{kind}.expected").read_text()
            self.assertIn(f"-{old} file=", body)
            self.assertIn(f"+{new} file=", body)
            self.assertIn("中文", body)
            self.assertGreater(max(map(len, body.splitlines())), 4000)

    def test_frozen_manifest_cannot_be_replaced(self):
        path = self.directory / "manifest.json"
        path.write_text(path.read_text() + "\n")
        with self.assertRaisesRegex(RuntimeError, "manifest changed"):
            perf_diff.verify_fixture(self.directory, self.digest)

    def test_source_oracle_index_and_git_config_changes_are_rejected(self):
        for relative in ("repo/file-0000.txt", "staged.expected", "repo/.git/index", "repo/.git/config"):
            with self.subTest(relative=relative):
                path = self.directory / relative
                old = path.read_bytes()
                path.write_bytes(old + b"tampered")
                with self.assertRaisesRegex(RuntimeError, "bytes changed"):
                    perf_diff.verify_fixture(self.directory, self.digest)
                path.write_bytes(old)

    def test_reuses_neither_fixture_nor_output_directory(self):
        with self.assertRaises(FileExistsError):
            perf_diff.prepare_fixture(self.directory, "small")

    def test_git_environment_is_isolated(self):
        from unittest.mock import patch
        with patch.dict(perf_diff.os.environ, {"GIT_DIR": "/invalid", "GIT_INDEX_FILE": "/invalid"}):
            perf_diff.verify_fixture(self.directory, self.digest)


class SourceIdentityTests(unittest.TestCase):
    def test_head_relative_archive_includes_staged_worktree_and_untracked_bytes(self):
        from unittest.mock import patch
        with tempfile.TemporaryDirectory(dir=perf_diff.ROOT / ".pi/tmp") as temporary:
            repo = Path(temporary) / "repo"
            repo.mkdir()
            perf_diff.git(repo, "init", "--quiet", "--template=")
            tracked = repo / "tracked"
            tracked.write_text("base\nbase\n")
            perf_diff.git(repo, "add", ".")
            perf_diff.git(repo, "commit", "--quiet", "-m", "base")
            tracked.write_text("staged\nbase\n")
            perf_diff.git(repo, "add", ".")
            tracked.write_text("staged\nworktree\n")
            (repo / "untracked").write_text("untracked bytes\n")
            output = Path(temporary) / "archive"
            output.mkdir()
            with patch.object(perf_diff, "ROOT", repo):
                perf_diff.source_identity(output)
            archive = (output / "source.patch").read_text()
            for value in ("+staged", "+worktree", "+untracked bytes"):
                self.assertIn(value, archive)


class ReportTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(dir=perf_diff.ROOT / ".pi/tmp")
        self.addCleanup(self.temp.cleanup)
        self.report = Path(self.temp.name) / "report.csv"

    def rows(self, native):
        rows = []
        for k, kind in enumerate(perf_diff.KINDS):
            for i, phase in enumerate(perf_diff.ACTIONS):
                if not native and phase == "idle":
                    continue
                views = 2 if 6 <= i < 12 else 1
                idle = phase == "idle"
                rows.append(dict(
                    kind=kind, phase=phase, step=str(k * len(perf_diff.ACTIONS) + i), views=str(views),
                    action_cpu_ms="0.1", first_ms="" if idle else "1",
                    selected_ready_ms="" if idle else "2", all_ready_ms="" if idle else "3",
                    draw_cpu_max_ms="0" if idle else "1", frames="0" if idle else "1",
                    blank_frames="0", missing_observations="0", idle_ticks="30" if idle else "0",
                    min_visible_rows="0" if idle else "10", fully_indexed_views="0" if idle else str(views),
                    bytes="1000", projection_rows="100", width_px="464" if views == 2 else "944", height_px="576",
                    target_display=f"Some(DisplayId {{ committed: SnapshotId {{ buffer: BufferId({k + 1}), version: TextVersion({1 + int(i >= 9)}) }}, session: SessionToken {{ target: BufferId({k + 1}), serial: 1 }}, revision: 0 }})",
                    text_verified="true", projection_verified="true", status="ready", observed_elapsed_ms="4",
                    selected_view="1", target_point="10", before_point="0", before_scroll_y="0", budget_violations=""))
        return rows

    def write(self, native=True, rows=None, footer="# completed=true;", diagnostic=False, budget_passed=True):
        rows = self.rows(native) if rows is None else rows
        output = io.StringIO()
        output.write(f"# mode={'native' if native else 'headless'}-diff; release=true;\n")
        if native:
            output.write("# hardware=true; graphics=test GPU\n")
            output.write(f"# diagnostic={str(diagnostic).lower()}; acceptance={str(not diagnostic).lower()}\n")
        writer = csv.DictWriter(output, fieldnames=rows[0].keys())
        writer.writeheader()
        writer.writerows(rows)
        if native:
            output.write(f"# budget_passed={str(budget_passed).lower()}\n")
        output.write(footer + "\n")
        self.report.write_text(output.getvalue())

    def test_modes_have_distinct_idle_coverage(self):
        for native in (False, True):
            self.write(native)
            rows = perf_diff.read_report(self.report, native)
            self.assertEqual(len(rows), 26 if native else 24)

    def test_missing_reordered_and_wrong_step_rejected(self):
        original = self.rows(True)
        for mutation in range(3):
            rows = copy.deepcopy(original)
            if mutation == 0:
                rows.pop()
            elif mutation == 1:
                rows[0], rows[1] = rows[1], rows[0]
            else:
                rows[0]["step"] = "13"
            self.write(rows=rows)
            with self.assertRaisesRegex(RuntimeError, "workload"):
                perf_diff.read_report(self.report, True)

    def test_correctness_finite_values_and_coverage_fail_closed(self):
        for key, value in (
            ("views", "2"), ("blank_frames", "1"), ("missing_observations", "1"),
            ("text_verified", "false"), ("projection_verified", "false"), ("min_visible_rows", "0"),
            ("fully_indexed_views", "0"), ("action_cpu_ms", "nan"), ("all_ready_ms", "inf"),
            ("first_ms", "-1"), ("status", "failed"), ("frames", "0"), ("bytes", "0"),
            ("projection_rows", "0"), ("width_px", "0"), ("height_px", "0"), ("target_display", ""),
        ):
            with self.subTest(key=key):
                rows = self.rows(True)
                rows[0][key] = value
                self.write(rows=rows)
                with self.assertRaises(RuntimeError):
                    perf_diff.read_report(self.report, True)

    def test_native_draw_scroll_and_idle_budgets(self):
        for phase, key, value in (("open", "draw_cpu_max_ms", "16.001"),
                                  ("scroll", "first_ms", "4.001"),
                                  ("idle", "frames", "1"), ("idle", "idle_ticks", "29"),
                                  ("idle", "first_ms", "0")):
            rows = self.rows(True)
            row = next(row for row in rows if row["phase"] == phase)
            row[key] = value
            self.write(rows=rows)
            with self.assertRaises(RuntimeError):
                perf_diff.read_report(self.report, True)

    def test_new_diff_latency_has_no_invented_budget(self):
        rows = self.rows(True)
        rows[0].update(first_ms="1000", selected_ready_ms="2000", all_ready_ms="3000",
                       observed_elapsed_ms="3001")
        self.write(rows=rows)
        perf_diff.read_report(self.report, True)

    def test_debug_build_and_stale_refresh_are_rejected(self):
        self.write()
        self.report.write_text(self.report.read_text().replace("release=true", "release=false"))
        with self.assertRaises(RuntimeError):
            perf_diff.read_report(self.report, True)
        rows = self.rows(True)
        for row in rows:
            if row["phase"] == "refresh":
                row["target_display"] = row["target_display"].replace("TextVersion(2)", "TextVersion(1)")
        self.write(rows=rows)
        with self.assertRaisesRegex(RuntimeError, "stale refresh"):
            perf_diff.read_report(self.report, True)

    def test_partial_and_failed_footer_cannot_pass(self):
        for footer in ("", "# completed=false;", "# stop_reason=failed\n# completed=true;"):
            self.write(footer=footer)
            with self.assertRaises(RuntimeError):
                perf_diff.read_report(self.report, True)

    def frame_log(self, diagnostic=True, slow_setup=True, idle_draw=False):
        rows = self.rows(True)
        frames = []
        clock = 0.0

        def append(step, kind, phase, stage, duration, body, start=None):
            nonlocal clock
            draw_start = clock
            clock += duration
            reasons = []
            if duration > 16:
                reasons.append(f"draw_cpu_ms={duration:.6f} >16")
            if stage == "action" and phase == "idle":
                reasons.append("idle_draw")
            frames.append(dict(frame=str(len(frames)), step=str(step), kind=kind, phase=phase, stage=stage,
                               action_start_ms="" if start is None else str(start),
                               draw_start_ms=str(draw_start), draw_end_ms=str(clock), draw_cpu_ms=str(duration),
                               request_layout_ms=str(duration / 4), prepaint_ms=str(duration / 4),
                               paint_ms=str(duration / 4), draw_other_ms=str(duration / 4),
                               submit_cpu_ms="0", submit_end_ms=str(clock), views=body["views"],
                               selected_view="1", min_visible_rows="10", fully_indexed_views=body["views"],
                               bytes="1000", width_px=body["width_px"], height_px=body["height_px"],
                               target_display=body["target_display"], compatible_views=body["views"],
                               selected_compatible="true", selected_ready="true", shared_snapshot="true",
                               target_kind=body["kind"], projection_rows=body["projection_rows"],
                               collapsed=str(phase in ("collapse", "refresh")).lower(), point=body["target_point"],
                               scroll_y="120", budget_violations=";".join(reasons)))

        append(-1, "none", "window-open", "setup", 20 if slow_setup else 1, rows[0])
        for row in rows:
            start = clock
            if row["phase"] == "idle":
                if idle_draw:
                    row.update(frames="1", min_visible_rows="10", fully_indexed_views="2", draw_cpu_max_ms="1")
                    append(int(row["step"]), row["kind"], row["phase"], "action", 1, row, start)
            else:
                row["frames"] = "3"
                for _ in range(3):
                    append(int(row["step"]), row["kind"], row["phase"], "action", 1, row, start)
            reasons = perf_diff.row_violations(row)
            row["budget_violations"] = ";".join(reasons)
            row["status"] = "over-budget" if reasons else "ready"
            clock += 1
        passed = not slow_setup and not idle_draw
        self.write(rows=rows, diagnostic=diagnostic, budget_passed=passed)
        path = self.report.with_suffix(".frames.csv")
        with path.open("w") as output:
            writer = csv.DictWriter(output, fieldnames=frames[0].keys())
            writer.writeheader()
            writer.writerows(frames)
            output.write("# completed=true\n")
        return rows, path

    def test_diagnostic_continues_but_records_draw_scroll_and_idle_failures(self):
        rows = self.rows(True)
        for row in rows:
            if row["phase"] == "open":
                row.update(draw_cpu_max_ms="20")
            if row["phase"] == "scroll":
                row.update(first_ms="8", selected_ready_ms="9", all_ready_ms="10", observed_elapsed_ms="11")
            if row["phase"] == "idle":
                row.update(frames="1", draw_cpu_max_ms="1", min_visible_rows="10", fully_indexed_views="2")
            row["budget_violations"] = ";".join(perf_diff.row_violations(row))
            row["status"] = "over-budget" if row["budget_violations"] else "ready"
        self.write(rows=rows, diagnostic=True, budget_passed=False)
        self.assertEqual(len(perf_diff.read_report(self.report, True, True)), 26)
        with self.assertRaisesRegex(RuntimeError, "cannot be read as acceptance"):
            perf_diff.read_report(self.report, True)
        rows[0]["budget_violations"] = ""
        self.write(rows=rows, diagnostic=True, budget_passed=False)
        with self.assertRaisesRegex(RuntimeError, "budget verdict"):
            perf_diff.read_report(self.report, True, True)

    def test_setup_frame_and_idle_overruns_are_backed_by_raw_evidence(self):
        rows, path = self.frame_log(idle_draw=True)
        perf_diff.read_report(self.report, True, True)
        verdict = perf_diff.read_frames(path, rows, True)
        self.assertFalse(verdict["budget_passed"])
        self.assertTrue(any(v["phase"] == "window-open" for v in verdict["violations"]))
        self.assertTrue(any(v["phase"] == "idle" for v in verdict["violations"]))

    def test_setup_only_failure_and_all_passing_diagnostic_are_non_acceptance(self):
        for slow in (True, False):
            rows, path = self.frame_log(slow_setup=slow)
            perf_diff.read_report(self.report, True, True)
            verdict = perf_diff.read_frames(path, rows, True)
            self.assertEqual(verdict["budget_passed"], not slow)
            with self.assertRaises(RuntimeError):
                perf_diff.read_report(self.report, True)

    def test_diagnostic_frame_counts_phases_and_completion_are_fail_closed(self):
        for old, new in (("20.0", "nan"), ("window-open", "unknown"),
                         ("# completed=true", "# completed=false")):
            rows, path = self.frame_log()
            path.write_text(path.read_text().replace(old, new))
            with self.assertRaises((RuntimeError, ValueError)):
                perf_diff.read_frames(path, rows, True)
        rows, path = self.frame_log()
        rows[0]["frames"] = "2"
        with self.assertRaisesRegex(RuntimeError, "frame counts"):
            perf_diff.read_frames(path, rows, True)

    def test_completion_requires_actual_target_ready_and_monotonic_frames(self):
        for mutation in ("old-refresh", "not-ready", "bad-setup", "reordered"):
            rows, path = self.frame_log()
            with path.open() as source:
                frames = list(csv.DictReader(line for line in source if not line.startswith("#")))
            if mutation == "old-refresh":
                for frame in frames:
                    if frame["phase"] == "refresh":
                        frame["target_display"] = frame["target_display"].replace("TextVersion(2)", "TextVersion(1)")
            elif mutation == "not-ready":
                for frame in frames:
                    if frame["stage"] == "action":
                        frame.update(fully_indexed_views="0", selected_ready="false")
            elif mutation == "bad-setup":
                frames[0].update(compatible_views="0", selected_compatible="false", fully_indexed_views="0")
            else:
                frames[1:7] = frames[4:7] + frames[1:4]
                for index, frame in enumerate(frames):
                    frame["frame"] = str(index)
            with path.open("w") as output:
                writer = csv.DictWriter(output, fieldnames=frames[0].keys())
                writer.writeheader()
                writer.writerows(frames)
                output.write("# completed=true\n")
            with self.assertRaises(RuntimeError, msg=mutation):
                perf_diff.read_frames(path, rows, True)

    def test_diagnostic_never_continues_after_correctness_failure(self):
        for key, value in (("text_verified", "false"), ("projection_verified", "false"),
                           ("blank_frames", "1"), ("missing_observations", "1"),
                           ("fully_indexed_views", "0"), ("target_display", "")):
            rows = self.rows(True)
            rows[0][key] = value
            self.write(rows=rows, diagnostic=True)
            with self.assertRaises(RuntimeError):
                perf_diff.read_report(self.report, True, True)
        self.write(diagnostic=True)
        self.report.write_text(self.report.read_text().replace("acceptance=false", "acceptance=true"))
        with self.assertRaises(RuntimeError):
            perf_diff.read_report(self.report, True, True)

    def test_missing_span_completion_or_coverage_rejected(self):
        path = self.report.with_suffix(".spans.csv")
        path.write_text("kind,phase,span,start_ms,duration_ms,complete\n"
                        "staged,open,diff-git,0,,false\n")
        with self.assertRaises(RuntimeError):
            perf_diff.read_spans(path)
        path.write_text("kind,phase,span,start_ms,duration_ms,complete\n")
        with self.assertRaisesRegex(RuntimeError, "missing"):
            perf_diff.read_spans(path)


class DiagnosticDriverTests(unittest.TestCase):
    def invoke(self, fail_at=None):
        import json
        from contextlib import ExitStack
        from unittest.mock import patch

        with tempfile.TemporaryDirectory(dir=perf_diff.ROOT / ".pi/tmp") as temporary:
            root = Path(temporary)
            binary = root / "target/release/examples/perf_native"
            binary.parent.mkdir(parents=True)
            binary.write_bytes(b"offline mock probe; never executed")
            output = root / "output"
            snapshot = {"git-head.txt": b"head\n", "git-status.txt": b"dirty\n", "source.patch": b"patch\n"}
            completed = ([{"kind": "staged", "phase": "open"}],
                         {"budget_passed": False, "violations": [{"phase": "window-open"}]})
            results = [copy.deepcopy(completed) for _ in range(3)]
            if fail_at is not None:
                results[fail_at] = RuntimeError("injected hard failure")
            with ExitStack() as stack:
                stack.enter_context(patch.object(perf_diff, "ROOT", root))
                stack.enter_context(patch.object(perf_diff, "source_snapshot", return_value=snapshot))
                stack.enter_context(patch.object(perf_diff, "prepare_fixture", return_value="frozen"))
                stack.enter_context(patch.object(perf_diff, "verify_fixture"))
                stack.enter_context(patch.object(perf_diff.subprocess, "run"))
                stack.enter_context(patch.object(perf_diff.Path, "is_dir", return_value=True))
                stack.enter_context(patch.dict(perf_diff.os.environ, {"WAYLAND_DISPLAY": "offline-test"}))
                stack.enter_context(patch.object(sys, "argv", ["perf_diff.py", str(output), "--native", "--diagnostic", "--consent"]))
                probe = stack.enter_context(patch.object(perf_diff, "run_probe", side_effect=results))
                if fail_at is None:
                    perf_diff.main()
                else:
                    with self.assertRaises(SystemExit):
                        perf_diff.main()
                metadata = json.loads((output / "environment.json").read_text())
                self.assertEqual(probe.call_count, 3 if fail_at is None else fail_at + 1)
                self.assertTrue(all(call.args[-1] for call in probe.call_args_list))
                return metadata, (output / "summary.json").exists()

    def test_all_three_windows_continue_despite_budget_failures(self):
        metadata, summary = self.invoke()
        self.assertTrue(metadata["completed"])
        self.assertFalse(metadata["acceptance"])
        self.assertFalse(metadata["budget_passed"])
        self.assertEqual(len(metadata["run_results"]), 3)
        self.assertTrue(summary)

    def test_hard_failure_stops_later_windows_and_keeps_prior_summary(self):
        metadata, summary = self.invoke(fail_at=1)
        self.assertFalse(metadata["completed"])
        self.assertFalse(metadata["acceptance"])
        self.assertEqual(len(metadata["run_results"]), 1)
        self.assertIn("injected hard failure", metadata["stop_reason"])
        self.assertTrue(summary)


if __name__ == "__main__":
    unittest.main()
