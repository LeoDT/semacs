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
        return (
            "# is_software_emulated: false\n"
            "phase,ready_submit_ms,visible_rows\n"
            "open,10,1\npreedit,2,1\ncancel,2,1\nscroll,2,1\nresize,2,1\n"
            "font,2,1\nclick,2,1\npreedit,2,1\ncommit,2,1\n"
            "# completed=true;\n"
        )

    def test_requires_completion_hardware_and_full_matrix(self):
        self.assertEqual(len(self.report(self.valid_report())), 9)
        for invalid in (
            self.valid_report().replace("# completed=true;", ""),
            self.valid_report().replace("false", "true"),
            self.valid_report().replace("cancel,2,1\n", ""),
            self.valid_report().replace("commit,2,1", "commit,,0"),
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
        for flags in ([], ["--consent", "--m1", "--ime"],
                      ["--consent", "--interval-ms", "50"],
                      ["--consent", "--m1", "--duration-seconds", "0"]):
            output = self.root / "report"
            with patch.object(sys, "argv", ["perf_native.py", "unused", str(output), *flags]), \
                 patch("sys.stderr", new=io.StringIO()):
                with self.assertRaises(SystemExit) as result:
                    runner.main()
            self.assertEqual(result.exception.code, 2)
            self.assertFalse(output.exists())

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
        counts = {"open": 1, "scroll": 30, "resize": 1, "font": 1, "forced": 30,
                  "idle": 1, "click": 1, "insert": 1, "delete": 1, "newline": 1,
                  "join": 1, "burst-insert": 1, "burst-delete": 1}
        content = "# is_software_emulated: false\nphase,ready_submit_ms,visible_rows\n"
        for phase, count in counts.items():
            content += f"{phase},1,1\n" * count
        content += "# completed=true;\n"
        path = self.root / "legacy.csv"
        path.write_text(content)
        self.assertEqual(len(runner.read_report(path, False, False)), 71)


if __name__ == "__main__":
    unittest.main()
