import csv
import importlib.util
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location("perf_native", ROOT / "scripts/perf_native.py")
runner = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(runner)


def rows():
    def row(phase, step):
        return {
            "phase": phase, "step": str(step), "key": "enter", "bytes": "8388608",
            "point": "0", "mark": "", "mark_active": "false", "action_cpu_ms": "0.1",
            "first_submit_ms": "1", "ready_submit_ms": "2", "first_frame_submit_ms": "1",
            "draw_cpu_max_ms": "1", "frames": "1", "blank_frames": "0",
            "schedule_lag_ms": "0", "observed_elapsed_ms": "3", "status": "ready",
            "width_px": "838", "height_px": "1306",
        }
    result = [row("open", 0)] + [row("chain", i) for i in range(29)]
    for i in range(20):
        value = row("sustained", i)
        value.update(key="x", point=str(i + 1), bytes=str(8388608 + i + 1))
        result.append(value)
    return result


def report(values, software=False):
    output = io.StringIO()
    output.write(f"# graphics=is_software_emulated: {str(software).lower()}\n")
    output.write("# mode=m1; duration_seconds=1; interval_ms=50; command_steps=29;\n")
    writer = csv.DictWriter(output, fieldnames=list(values[0]))
    writer.writeheader()
    writer.writerows(values)
    verdict = "not-applicable" if software else "true"
    output.write(f"# completed=true; budget_passed={verdict};\n")
    return output.getvalue()


def native_rows(ime=False):
    counts = (
        {"open": 1, "preedit": 2, "cancel": 1, "commit": 1,
         "scroll": 1, "resize": 1, "font": 1, "click": 1} if ime else
        {"open": 1, "scroll": 30, "resize": 1, "font": 1, "forced": 30,
         "idle": 1, "click": 1, "insert": 1, "delete": 1, "newline": 1,
         "join": 1, "burst-insert": 1, "burst-delete": 1}
    )
    result = []
    for phase, count in counts.items():
        for _ in range(count):
            value = {
                "phase": phase, "ready_submit_ms": "2", "first_submit_ms": "1",
                "first_frame_submit_ms": "1", "visible_rows": "1", "first_visible_rows": "1",
                "action_cpu_ms": "0.1", "draw_cpu_max_ms": "1", "blank_frames": "0",
                "frames": "1", "width_px": "838", "height_px": "1306",
                "observed_elapsed_ms": "3", "boundary": "action-to-submit",
            }
            if ime and phase in ("preedit", "cancel", "commit"):
                value["boundary"] = "arm-to-submit"
            if phase == "idle":
                value.update(frames="0", first_submit_ms="", ready_submit_ms="", first_frame_submit_ms="")
            result.append(value)
    return result


def csv_report(values, software=False):
    output = io.StringIO()
    output.write(f"# is_software_emulated: {str(software).lower()}\n")
    writer = csv.DictWriter(output, fieldnames=list(values[0]))
    writer.writeheader()
    writer.writerows(values)
    output.write("# completed=true;\n")
    return output.getvalue()


class NativeRunnerTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        (ROOT / ".pi/tmp").mkdir(parents=True, exist_ok=True)

    def report(self, text, software=False):
        with tempfile.TemporaryDirectory(dir=ROOT / ".pi/tmp") as directory:
            path = Path(directory) / "report.csv"
            path.write_text(text)
            return runner.read_report(path, software, True)

    def valid_report(self):
        return csv_report(native_rows(ime=True))

    def test_requires_completion_hardware_and_full_matrix(self):
        self.assertEqual(len(self.report(self.valid_report())), 9)
        for invalid in (
            self.valid_report().replace("# completed=true;", ""),
            self.valid_report().replace("false", "true"),
            csv_report([r for r in native_rows(True) if r["phase"] != "cancel"]),
            csv_report([dict(r, visible_rows="0") if r["phase"] == "commit" else r for r in native_rows(True)]),
        ):
            with self.subTest(invalid=invalid), self.assertRaises(RuntimeError):
                self.report(invalid)

    def test_refuses_input_when_focus_is_not_the_probe(self):
        desktop = runner.Desktop.__new__(runner.Desktop)
        desktop.target = Mock(return_value={"address": "probe"})
        desktop.active = Mock(return_value={"address": "other"})
        with self.assertRaisesRegex(RuntimeError, "refusing to inject"):
            desktop.keys("token", "ni")

    def test_explicit_software_report_is_not_hardware(self):
        software = self.valid_report().replace("false", "true")
        self.assertEqual(len(self.report(software, software=True)), 9)


class M1ReportTests(unittest.TestCase):
    def validate(self, values, software=False):
        runner.validate_m1_report(report(values, software), values, software, 1, 50)

    def test_complete_hardware_report(self):
        self.validate(rows())

    def test_superseded_ready_is_missing_not_zero_and_final_ready_is_required(self):
        values = rows()
        values[-2].update(status="superseded-before-ready", ready_submit_ms="")
        self.validate(values)
        values[-2]["ready_submit_ms"] = "0"
        with self.assertRaisesRegex(RuntimeError, "coverage"):
            self.validate(values)
        values = rows()
        values[-1].update(status="superseded-before-ready", ready_submit_ms="")
        with self.assertRaisesRegex(RuntimeError, "final surviving"):
            self.validate(values)

    def test_missing_first_is_not_a_hardware_pass(self):
        values = rows()
        values[-2].update(status="superseded-before-first", first_submit_ms="", ready_submit_ms="")
        with self.assertRaisesRegex(RuntimeError, "coverage"):
            self.validate(values)
        self.validate(values, software=True)

    def test_nonfinite_negative_and_missing_metrics_are_rejected(self):
        for metric in ("action_cpu_ms", "draw_cpu_max_ms", "schedule_lag_ms", "first_submit_ms"):
            for invalid in ("nan", "inf", "-1"):
                with self.subTest(metric=metric, value=invalid):
                    values = rows()
                    values[1][metric] = invalid
                    with self.assertRaisesRegex(RuntimeError, "invalid M1 metric"):
                        self.validate(values)

    def test_phase_order_and_workload_are_checked(self):
        values = rows()
        for invalid in (values[:-1], [values[1], values[0], *values[2:]]):
            with self.assertRaisesRegex(RuntimeError, "phase matrix"):
                self.validate(invalid)
        for metric, invalid in (("key", "backspace"), ("point", "0"), ("bytes", "8388608")):
            values = rows()
            values[-1][metric] = invalid
            with self.assertRaisesRegex(RuntimeError, "sustained workload"):
                self.validate(values)

    def test_hardware_budgets_are_independently_rechecked(self):
        for metric, invalid in (("action_cpu_ms", "2.001"), ("draw_cpu_max_ms", "16.001"),
                                ("first_submit_ms", "16.001"), ("ready_submit_ms", "300.001")):
            values = rows()
            values[1][metric] = invalid
            if metric == "first_submit_ms":
                values[1]["ready_submit_ms"] = "17"
            with self.assertRaisesRegex(RuntimeError, "STOP"):
                self.validate(values)
            self.validate(values, software=True)

    def test_software_verdict_is_not_hardware_acceptance(self):
        values = rows()
        with self.assertRaisesRegex(RuntimeError, "budget verdict"):
            runner.validate_m1_report(report(values, True), values, False, 1, 50)
        with self.assertRaisesRegex(RuntimeError, "settings"):
            runner.validate_m1_report(report(values), values, False, 60, 50)

    def test_empty_body_and_wrong_first_frame_are_rejected(self):
        for metric, invalid in (("blank_frames", "1"), ("width_px", "0"),
                                ("first_frame_submit_ms", "3")):
            values = rows()
            values[1][metric] = invalid
            with self.assertRaises(RuntimeError):
                self.validate(values)


class DriverTests(unittest.TestCase):
    def setUp(self):
        temporary = ROOT / ".pi/tmp"
        temporary.mkdir(parents=True, exist_ok=True)
        self.directory = tempfile.TemporaryDirectory(prefix="native-runner-test-", dir=temporary)
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)

    def test_failed_probe_stops_matrix_and_restores_desktop(self):
        fixture = self.root / "fixture"
        fixture.write_bytes(b"x" * (8 * 1024 * 1024))
        binary = self.root / "probe"
        binary.touch()
        output = self.root / "report"
        argv = ["perf_native.py", str(fixture), str(output), "--consent", "--m1",
                "--sample", "mixed", "--sample", "single-ascii", "--runs", "3",
                "--binary", str(binary)]
        with patch.object(sys, "argv", argv), patch.object(runner, "Desktop") as desktop, \
             patch.object(runner, "command", return_value="font"), \
             patch.object(runner, "run_probe", side_effect=RuntimeError("STOP: budget")) as probe:
            with self.assertRaisesRegex(RuntimeError, "STOP"):
                runner.main()
            probe.assert_called_once()
            desktop.return_value.restore.assert_called_once()
        metadata = json.loads((output / "environment.json").read_text())
        self.assertFalse(metadata["completed"])
        self.assertTrue(metadata["restored"])
        self.assertEqual(metadata["error"], "STOP: budget")

    def test_modes_and_pacing_require_explicit_consent_and_correct_mode(self):
        for flags in ([], ["--consent", "--m1", "--ime"], ["--consent", "--m1", "--m2"],
                      ["--consent", "--interval-ms", "50"],
                      ["--consent", "--m1", "--duration-seconds", "0"],
                      ["--consent", "--m2-open-diagnostic"],
                      ["--consent", "--m2-open-diagnostic", "--runs", "2"],
                      ["--consent", "--m2-open-diagnostic", "--runs", "1", "--sample", "single-ascii"],
                      ["--consent", "--m2-open-diagnostic", "--m2", "--runs", "1"]):
            output = self.root / "report"
            with patch.object(sys, "argv", ["perf_native.py", "unused", str(output), *flags]), \
                 patch("sys.stderr", new=io.StringIO()):
                with self.assertRaises(SystemExit) as result:
                    runner.main()
            self.assertEqual(result.exception.code, 2)
            self.assertFalse(output.exists())

    def test_failed_m2_probe_stops_matrix_and_restores_desktop(self):
        fixture = self.root / "fixture"
        fixture.write_bytes(b"x" * (8 * 1024 * 1024))
        binary = self.root / "probe"
        binary.touch()
        output = self.root / "report"
        argv = ["perf_native.py", str(fixture), str(output), "--consent", "--m2",
                "--sample", "mixed", "--runs", "3", "--binary", str(binary)]
        with patch.object(sys, "argv", argv), patch.object(runner, "Desktop") as desktop, \
             patch.object(runner, "command", return_value="font"), \
             patch.object(runner, "run_probe", side_effect=RuntimeError("STOP: M2")) as probe:
            with self.assertRaisesRegex(RuntimeError, "STOP: M2"):
                runner.main()
            probe.assert_called_once()
            desktop.return_value.restore.assert_called_once()
        metadata = json.loads((output / "environment.json").read_text())
        self.assertTrue(metadata["m2"])
        self.assertFalse(metadata["completed"])
        self.assertTrue(metadata["restored"])

    def test_probe_failure_surfaces_stop_reason_and_keeps_logs(self):
        from argparse import Namespace
        script = self.root / "fake-probe.py"
        script.write_text("print('STOP first_submit_ms=17 > 16', flush=True)\nraise SystemExit(2)\n")
        args = Namespace(output=self.root, binary=Path(sys.executable), input=script,
                         software=False, ime=False, m1=True, duration_seconds=1, interval_ms=50)
        with self.assertRaisesRegex(RuntimeError, "STOP first_submit_ms"):
            runner.run_probe(args, None, "mixed", 1)
        self.assertIn("STOP", (self.root / "mixed-1.stdout.log").read_text())

    def test_m1_report_file_round_trip(self):
        path = self.root / "report.csv"
        path.write_text(report(rows()))
        self.assertEqual(len(runner.read_report(path, False, False, (1, 50))), 50)

    def test_legacy_baseline_report_remains_supported(self):
        content = csv_report(native_rows())
        path = self.root / "legacy.csv"
        path.write_text(content)
        self.assertEqual(len(runner.read_report(path, False, False)), 71)


class NativeBudgetTests(unittest.TestCase):
    def test_each_existing_budget_is_checked(self):
        for phase, key, value in (
            ("open", "first_submit_ms", "251"), ("open", "ready_submit_ms", "501"),
            ("insert", "action_cpu_ms", "2.1"), ("delete", "first_submit_ms", "17"),
            ("newline", "ready_submit_ms", "301"), ("join", "draw_cpu_max_ms", "17"),
            ("burst-insert", "action_cpu_ms", "51"), ("burst-delete", "first_submit_ms", "51"),
            ("scroll", "first_submit_ms", "5"), ("forced", "first_submit_ms", "3"),
            ("resize", "first_frame_submit_ms", "51"), ("font", "ready_submit_ms", "301"),
        ):
            with self.subTest(phase=phase, key=key):
                values = native_rows()
                row = next(r for r in values if r["phase"] == phase)
                row[key] = value
                row["first_submit_ms"] = str(max(float(row["first_submit_ms"]), float(row["first_frame_submit_ms"])))
                row["ready_submit_ms"] = str(max(float(row["ready_submit_ms"]), float(row["first_submit_ms"])))
                with self.assertRaisesRegex(RuntimeError, "STOP"):
                    runner.validate_native_metrics(values, False, False)
                runner.validate_native_metrics(values, True, False)

    def test_ime_arm_budget_and_boundary(self):
        values = native_rows(True)
        row = next(r for r in values if r["phase"] == "preedit")
        row.update(first_submit_ms="76", ready_submit_ms="80")
        with self.assertRaisesRegex(RuntimeError, "STOP"):
            runner.validate_native_metrics(values, False, True)
        row.update(first_submit_ms="1", ready_submit_ms="2", boundary="action-to-submit")
        with self.assertRaisesRegex(RuntimeError, "boundary"):
            runner.validate_native_metrics(values, False, True)

    def test_invalid_metrics_and_idle_draw_are_not_passes(self):
        for key, value in (("frames", "1"), ("action_cpu_ms", "nan"), ("width_px", "0")):
            values = native_rows()
            next(r for r in values if r["phase"] == "idle")[key] = value
            with self.assertRaises(RuntimeError):
                runner.validate_native_metrics(values, False, False)


def m2_rows():
    result = []
    buffers = 1
    for index, phase in enumerate(runner.M2_PHASES):
        buffers += (phase == "open") - (phase == "kill")
        value = {
            "step": str(index), "phase": phase, "status": "complete",
            "action_cpu_ms": "0.1", "save_action_cpu_ms": "", "save_ack_before_edit": "",
            "first_submit_ms": "1", "ready_submit_ms": "2",
            "save_observed_ms": "", "result_observed_ms": "",
            "draw_cpu_max_ms": "1", "frames": "1", "blank_frames": "0",
            "width_px": "838", "height_px": "1306", "buffers": str(buffers),
            "retained_closed_views": "0", "rss_kib": "1000", "observed_elapsed_ms": "3",
        }
        if phase in ("save", "complete", "cancel-completion"):
            value.update(first_submit_ms="", ready_submit_ms="")
        if phase in ("save", "save-edit"):
            value["save_observed_ms"] = "3"
        if phase in ("complete", "cancel-completion"):
            value["result_observed_ms"] = "3"
        if phase == "save-edit":
            value.update(save_action_cpu_ms="0.1", save_ack_before_edit="false")
        result.append(value)
    return result


class M2ReportTests(unittest.TestCase):
    def validate(self, values, software=False):
        verdict = "not-applicable" if software else "true"
        content = ("# mode=m2; entries=2000; cycles=3; steps=31;\n"
                   f"# completed=true; existing_budget_passed={verdict};\n")
        runner.validate_m2_report(content, values, software)

    def test_complete_report_and_new_latency_metrics_do_not_invent_budgets(self):
        values = m2_rows()
        values[1].update(action_cpu_ms="20", first_submit_ms="1000", ready_submit_ms="2000")
        self.validate(values)

    def test_ui_observation_need_not_invent_a_draw_or_submission(self):
        values = m2_rows()
        for row in values:
            if row["phase"] in ("save", "complete", "cancel-completion"):
                row.update(frames="0", draw_cpu_max_ms="0")
        self.validate(values)

    def test_matrix_lifecycle_and_observation_boundaries_are_required(self):
        for key, invalid in (("step", "2"), ("status", "failed"), ("buffers", "20"),
                             ("blank_frames", "1"), ("width_px", "0"), ("rss_kib", "nan")):
            values = m2_rows()
            values[1][key] = invalid
            with self.subTest(key=key), self.assertRaises(RuntimeError):
                self.validate(values)
        values = m2_rows()
        next(r for r in values if r["phase"] == "kill")["retained_closed_views"] = "1"
        with self.assertRaisesRegex(RuntimeError, "lifecycle"):
            self.validate(values)
        values = m2_rows()
        next(r for r in values if r["phase"] == "complete")["first_submit_ms"] = "1"
        with self.assertRaisesRegex(RuntimeError, "incorrectly claims"):
            self.validate(values)

    def test_concurrent_input_keeps_existing_budget(self):
        for key, value in (("action_cpu_ms", "3"), ("first_submit_ms", "17"),
                           ("ready_submit_ms", "301"), ("draw_cpu_max_ms", "17")):
            values = m2_rows()
            row = next(r for r in values if r["phase"] == "save-edit")
            row[key] = value
            row["ready_submit_ms"] = str(max(float(row["ready_submit_ms"]), float(row["first_submit_ms"])))
            with self.assertRaisesRegex(RuntimeError, "STOP"):
                self.validate(values)
            self.validate(values, True)

    def test_independent_disk_verification(self):
        with tempfile.TemporaryDirectory(dir=ROOT / ".pi/tmp") as root:
            directory = Path(root)
            (directory / "original.txt").write_bytes(b"original")
            for index, prefix in enumerate((b"xxxxy", b"", b"")):
                (directory / f"input-{index}.txt").write_bytes(prefix + b"original")
            (directory / "completion").mkdir()
            for index in range(2000):
                (directory / "completion" / f"entry-{index:04}.txt").touch()
            runner.verify_m2_files(directory)
            (directory / "input-0.txt").write_bytes(b"wrong")
            with self.assertRaisesRegex(RuntimeError, "disk-byte"):
                runner.verify_m2_files(directory)


def open_diagnostic_files(succeeded):
    values = m2_rows()[:2]
    main = io.StringIO()
    main.write("# is_software_emulated: false\n# mode=m2; entries=2000; cycles=0; steps=2;\n")
    main.write("# open_diagnostic=true; acceptance=false\n")
    if not succeeded:
        values[-1].update(status="failed", draw_cpu_max_ms="42", ready_submit_ms="")
    writer = csv.DictWriter(main, fieldnames=list(values[0]))
    writer.writeheader()
    writer.writerows(values)
    main.write(f"# completed={str(succeeded).lower()}; existing_budget_passed={str(succeeded).lower()};\n")
    frames = []
    for index, phase in enumerate(("boot", "open")):
        slow = index == 1 and not succeeded
        frames.append({
            "frame": str(index), "step": str(index), "phase": phase, "stage": "measured",
            "draw_start_ms": "10", "draw_end_ms": "52" if slow else "11",
            "action_to_draw_ms": "10", "draw_cpu_ms": "42" if slow else "1",
            "request_layout_ms": "1" if slow else ".1", "prepaint_ms": "30" if slow else ".5",
            "paint_ms": "9" if slow else ".2", "draw_other_ms": "2" if slow else ".2",
            "submit_cpu_ms": "2", "draw_to_submit_gap_ms": "1", "action_to_submit_ms": "55",
            "invalidations": "1", "target_match": "true", "bytes": "8388608", "indexed_bytes": "4096",
            "visible_rows": "30", "scanned_bytes": "4096", "layout_ready": "false" if slow else "true",
            "compatible": "true", "marked": "false", "width_px": "838", "height_px": "1306",
        })
    sidecars = {}
    views = [{
        "frame": "1", "step": "1", "phase": "open", "view": 'view,"name',
        "view_phase": "prepaint", "start_ms": "10", "end_ms": "40" if not succeeded else "10.5",
        "total_ms": "30" if not succeeded else ".5", "self_ms": "29" if not succeeded else ".5",
    }]
    preparation = []
    for parent, lane, phase, start, end, complete in (
            ("", "foreground", "prepare", 10, 40 if not succeeded else 10.5, "true"),
            ("0", "foreground", "ensure-index", 10, 10.1, "true"),
            ("1", "foreground", "wrap-preview", 10, 10.1, "true"),
            ("2", "foreground", "measure-grapheme", 10.01, 10.05, "true"),
            ("0", "foreground", "shape-visible", 10.1, 40 if not succeeded else 10.5, "true"),
            ("4", "foreground", "shape-line", 10.2, 39 if not succeeded else 10.4, "true"),
            ("", "wrap-worker", "wrap-worker", 10.15, 53, "false")):
        preparation.append({
            "id": str(len(preparation)), "parent": parent, "frame": "1" if lane == "foreground" else "",
            "lane": lane, "phase": phase, "bytes": "3", "start_ms": str(start), "end_ms": str(end),
            "complete": complete, "total_ms": f"{end - start:.6f}",
        })
    for name, rows in (("frames", frames), ("views", views), ("preparation", preparation)):
        output = io.StringIO()
        if name == "frames":
            output.write("# preparation_trace_version=1\n")
        writer = csv.DictWriter(output, fieldnames=list(rows[0]))
        writer.writeheader()
        writer.writerows(rows)
        output.write(f"# completed_open={str(succeeded).lower()}; lost_entries=0; acceptance=false\n")
        sidecars[name] = output.getvalue()
    return main.getvalue(), sidecars


class OpenDiagnosticTests(unittest.TestCase):
    def setUp(self):
        (ROOT / ".pi/tmp").mkdir(parents=True, exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(dir=ROOT / ".pi/tmp")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)

    def fixture(self, succeeded):
        content, sidecars = open_diagnostic_files(succeeded)
        path = self.root / "mixed-1.csv"
        path.write_text(content)
        for name, data in sidecars.items():
            path.with_suffix(f".{name}.csv").write_text(data)
        return path

    def test_success_is_only_a_two_step_diagnostic_not_full_acceptance(self):
        path = self.fixture(True)
        self.assertEqual(len(runner.read_open_diagnostic(path, False, True)), 2)
        with self.assertRaisesRegex(RuntimeError, "settings"):
            runner.read_report(path, False, False, m2=True)
        summary = json.loads(path.with_suffix(".diagnostic.json").read_text())
        self.assertFalse(summary["acceptance"])
        self.assertTrue(summary["diagnostic_only"])

    def test_budget_failure_preserves_slowest_frame_and_unmeasured_ready(self):
        path = self.fixture(False)
        rows = runner.read_open_diagnostic(path, False, False)
        self.assertEqual(rows[-1]["ready_submit_ms"], "")
        summary = json.loads(path.with_suffix(".diagnostic.json").read_text())
        self.assertFalse(summary["process_succeeded"])
        self.assertEqual(summary["slowest_open_frame"]["prepaint_ms"], "30")
        self.assertEqual(summary["slow_views_in_that_frame"][0]["view"], 'view,"name')
        self.assertEqual(summary["slow_views_in_that_frame"][0]["self_ms"], "29")
        preparation = summary["preparation"]
        self.assertEqual(preparation["phases_in_slowest_frame"]["measure-grapheme"]["count"], 1)
        self.assertAlmostEqual(preparation["phases_in_slowest_frame"]["shape-line"]["total_ms"], 28.8)
        self.assertEqual(preparation["workers_overlapping_slowest_frame"][0]["complete"], "false")

    def test_legacy_diagnostic_stays_readable_without_inventing_subphase_evidence(self):
        path = self.fixture(False)
        frames = path.with_suffix(".frames.csv")
        frames.write_text(frames.read_text().replace("# preparation_trace_version=1\n", ""))
        path.with_suffix(".preparation.csv").unlink()
        runner.read_open_diagnostic(path, False, False)
        self.assertIsNone(json.loads(path.with_suffix(".diagnostic.json").read_text())["preparation"])

    def test_invalid_preparation_parent_frame_and_duration_are_rejected(self):
        for field, value in (("parent", "3"), ("frame", "99"), ("end_ms", "100"),
                             ("total_ms", "nan"), ("lane", "unknown")):
            with self.subTest(field=field):
                path = self.fixture(False)
                source = path.with_suffix(".preparation.csv")
                rows = list(csv.DictReader(line for line in source.read_text().splitlines()
                                           if not line.startswith("#")))
                rows[3][field] = value
                output = io.StringIO()
                writer = csv.DictWriter(output, fieldnames=list(rows[0]))
                writer.writeheader()
                writer.writerows(rows)
                source.write_text(output.getvalue() + "# completed_open=false; lost_entries=0; acceptance=false\n")
                with self.assertRaises(RuntimeError):
                    runner.read_open_diagnostic(path, False, False)

    def test_initial_worker_is_reported_even_when_it_precedes_the_target_draw(self):
        path = self.fixture(False)
        source = path.with_suffix(".preparation.csv")
        data = source.read_text().replace(
            "# completed_open=false;",
            "7,,,initial-worker,initial-worker,8388648,0,9,true,9\n# completed_open=false;")
        source.write_text(data)
        runner.read_open_diagnostic(path, False, False)
        preparation = json.loads(path.with_suffix(".diagnostic.json").read_text())["preparation"]
        self.assertEqual(preparation["initial_preparation_workers"][0]["total_ms"], "9")
        self.assertNotIn("initial-worker", preparation["phases_in_slowest_frame"])

    def test_new_trace_requires_its_preparation_sidecar(self):
        path = self.fixture(False)
        path.with_suffix(".preparation.csv").unlink()
        with self.assertRaises(FileNotFoundError):
            runner.read_open_diagnostic(path, False, False)

    def test_missing_lossy_or_inconsistent_sidecars_are_rejected(self):
        for name in ("frames", "views", "preparation"):
            path = self.fixture(False)
            source = path.with_suffix(f".{name}.csv")
            source.write_text(source.read_text().replace("lost_entries=0", "lost_entries=1"))
            with self.assertRaisesRegex(RuntimeError, "lossy"):
                runner.read_open_diagnostic(path, False, False)
        path = self.fixture(False)
        source = path.with_suffix(".frames.csv")
        source.write_text(source.read_text().replace(",30,9,2,", ",31,9,2,"))
        with self.assertRaisesRegex(RuntimeError, "account for draw"):
            runner.read_open_diagnostic(path, False, False)
        path = self.fixture(False)
        path.write_text(path.read_text().replace(",42,", ",41,"))
        with self.assertRaisesRegex(RuntimeError, "max draw differ"):
            runner.read_open_diagnostic(path, False, False)

    def test_failed_subprocess_still_writes_diagnostic_summary_and_surfaces_stop(self):
        from argparse import Namespace
        content, sidecars = open_diagnostic_files(False)
        script = self.root / "fake-probe.py"
        script.write_text(
            "from pathlib import Path\nimport sys\np = Path(sys.argv[2])\n"
            f"p.write_text({content!r})\n"
            + "".join(f"p.with_suffix('.{name}.csv').write_text({data!r})\n" for name, data in sidecars.items())
            + "print('STOP open: draw_cpu_max_ms=42 > 16', flush=True)\nraise SystemExit(2)\n"
        )
        args = Namespace(output=self.root, binary=Path(sys.executable), input=script,
                         software=False, ime=False, m1=False, m2=True, m2_open_diagnostic=True)
        with self.assertRaisesRegex(RuntimeError, "STOP open: draw_cpu_max_ms"):
            runner.run_probe(args, None, "mixed", 1)
        self.assertTrue((self.root / "mixed-1.diagnostic.json").is_file())

    def test_single_diagnostic_main_never_starts_the_remaining_matrix(self):
        fixture = self.root / "fixture"
        fixture.write_bytes(b"x" * (8 * 1024 * 1024))
        binary = self.root / "probe"
        binary.touch()
        output = self.root / "report"
        argv = ["perf_native.py", str(fixture), str(output), "--consent", "--m2-open-diagnostic",
                "--runs", "1", "--binary", str(binary)]
        with patch.object(sys, "argv", argv), patch.object(runner, "Desktop") as desktop, \
             patch.object(runner, "command", return_value="font"), \
             patch.object(runner, "run_probe", return_value=m2_rows()[:2]) as probe:
            runner.main()
            probe.assert_called_once()
            self.assertEqual(probe.call_args.args[2:], ("mixed", 1))
            desktop.return_value.restore.assert_called_once()
        metadata = json.loads((output / "environment.json").read_text())
        self.assertTrue(metadata["completed"])
        self.assertTrue(metadata["m2_open_diagnostic"])
        self.assertFalse(metadata["acceptance"])


if __name__ == "__main__":
    unittest.main()
