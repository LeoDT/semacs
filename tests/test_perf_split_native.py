import csv
import importlib.util
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location("split_perf_runner", ROOT / "scripts/perf_native.py")
runner = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(runner)


def rows():
    result = []
    for index, phase in enumerate(runner.SPLIT_PHASES):
        views = "1" if index == 0 else "2"
        row = {
            "phase": phase, "views": views, "selected_view": "20" if index in (1, 2) else "10",
            "action_cpu_ms": "0.1", "first_frame_submit_ms": "0.5",
            "first_submit_ms": "1", "ready_submit_ms": "2", "all_ready_submit_ms": "350",
            "draw_cpu_max_ms": "1", "frames": "2", "missing_observations": "0",
            "blank_frames": "0", "idle_ticks": "0", "min_visible_rows": "1",
            "fully_indexed_views": views, "bytes": "8388608", "width_px": "450",
            "height_px": "576", "status": "ready", "shared_text_verified": "true",
            "observed_elapsed_ms": "360",
        }
        if phase == "idle":
            row.update(status="idle", frames="0", idle_ticks="30",
                       first_frame_submit_ms="", first_submit_ms="", ready_submit_ms="",
                       all_ready_submit_ms="", draw_cpu_max_ms="0")
        result.append(row)
    return result


def report(values, direction="right", sample="mixed", software=False):
    out = io.StringIO()
    out.write(f"# mode=split; direction={direction}; sample={sample}; release=true; "
              f"graphics=is_software_emulated: {str(software).lower()}\n")
    writer = csv.DictWriter(out, fieldnames=list(values[0]))
    writer.writeheader()
    writer.writerows(values)
    verdict = "not-applicable" if software else "true"
    out.write(f"# completed=true; selected_budget_passed={verdict}; all_view_budget=not-established;\n")
    return out.getvalue()


class SplitNativeRunnerTests(unittest.TestCase):
    def setUp(self):
        (ROOT / ".pi/tmp").mkdir(parents=True, exist_ok=True)
        self.tmp = tempfile.TemporaryDirectory(dir=ROOT / ".pi/tmp")
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def read(self, values=None, content=None, software=False, direction="right", sample="mixed"):
        path = self.root / "report.csv"
        path.write_text(content if content is not None else report(
            values if values is not None else rows(), direction, sample, software))
        return runner.read_report(path, software, False, split=direction, sample=sample)

    def test_aggregate_above_300_is_not_a_selected_budget_failure(self):
        for direction in ("right", "below"):
            self.assertEqual(len(self.read(direction=direction)), 73)

    def test_selected_budgets_are_not_relaxed(self):
        cases = (
            ("insert", "action_cpu_ms", "2.01"), ("delete", "first_submit_ms", "16.01"),
            ("newline", "ready_submit_ms", "300.01"), ("join", "draw_cpu_max_ms", "16.01"),
            ("burst-insert", "action_cpu_ms", "50.01"),
            ("burst-delete", "first_submit_ms", "50.01"),
            ("peer-scroll", "first_submit_ms", "4.01"), ("scroll", "first_submit_ms", "4.01"),
            ("forced", "first_submit_ms", "2.01"),
            ("resize", "first_frame_submit_ms", "50.01"), ("font", "ready_submit_ms", "300.01"),
            ("boot", "first_submit_ms", "250.01"), ("boot", "ready_submit_ms", "500.01"),
        )
        for phase, key, value in cases:
            with self.subTest(phase=phase, metric=key):
                values = rows()
                row = next(r for r in values if r["phase"] == phase)
                row[key] = value
                row["first_submit_ms"] = str(max(float(row["first_frame_submit_ms"]), float(row["first_submit_ms"])))
                row["ready_submit_ms"] = str(max(float(row["first_submit_ms"]), float(row["ready_submit_ms"])))
                row["all_ready_submit_ms"] = str(max(float(row["ready_submit_ms"]), 350))
                row["observed_elapsed_ms"] = str(float(row["all_ready_submit_ms"]) + 1)
                with self.assertRaisesRegex(RuntimeError, "STOP"):
                    self.read(values)

    def test_software_diagnostic_skips_only_numeric_budgets(self):
        values = rows()
        values[4]["action_cpu_ms"] = "999"
        self.read(values, software=True)
        values[4]["missing_observations"] = "1"
        with self.assertRaises(RuntimeError):
            self.read(values, software=True)
        with self.assertRaisesRegex(RuntimeError, "renderer"):
            self.read(content=report(rows(), software=True))

    def test_incomplete_or_reordered_matrix_cannot_pass(self):
        for values in (rows()[:-1], rows()[1:], rows() + [rows()[0]], rows()[::-1]):
            with self.assertRaisesRegex(RuntimeError, "reordered"):
                self.read(values)

    def test_identity_release_verdict_and_completion_are_checked(self):
        source = report(rows())
        for old, new in (
            ("direction=right", "direction=below"), ("sample=mixed", "sample=single-ascii"),
            ("sample=mixed; ", ""), ("sample=mixed", "sample=NOT-A-SAMPLE"),
            ("release=true", "release=false"), ("completed=true", "completed=false"),
            ("selected_budget_passed=true", "selected_budget_passed=false"),
            ("all_view_budget=not-established", "all_view_budget=passed"),
        ):
            with self.subTest(new=new), self.assertRaises(RuntimeError):
                self.read(content=source.replace(old, new))

    def test_view_selection_and_body_verification_cannot_be_omitted(self):
        for key, value in (
            ("views", "1"), ("selected_view", "20"), ("fully_indexed_views", "1"),
            ("shared_text_verified", "false"), ("min_visible_rows", "0"),
            ("missing_observations", "1"), ("blank_frames", "1"),
            ("width_px", "0"), ("height_px", "0"), ("bytes", "4"),
            ("status", "failed"), ("frames", "0.5"),
        ):
            with self.subTest(key=key), self.assertRaises(RuntimeError):
                values = rows()
                values[4][key] = value
                self.read(values)

    def test_ready_cannot_be_synthesized_from_missing_or_nonfinite_values(self):
        for key, value in (
            ("all_ready_submit_ms", ""), ("all_ready_submit_ms", "1"),
            ("first_frame_submit_ms", "2"), ("ready_submit_ms", "nan"),
            ("action_cpu_ms", "inf"), ("frames", "-1"),
            ("observed_elapsed_ms", "3"), ("ready_submit_ms", "0"),
        ):
            with self.subTest(key=key), self.assertRaises(RuntimeError):
                values = rows()
                values[4][key] = value
                self.read(values)

    def test_idle_is_thirty_no_draw_ticks_not_zero_ms_rendering(self):
        for key, value in (
            ("frames", "1"), ("idle_ticks", "29"), ("all_ready_submit_ms", "0"),
            ("first_submit_ms", "0"), ("status", "ready"),
        ):
            with self.subTest(key=key), self.assertRaises(RuntimeError):
                values = rows()
                values[-1][key] = value
                self.read(values)

    def main_args(self):
        fixture = self.root / "sample.txt"
        fixture.write_bytes(b"x" * (8 * 1024 * 1024))
        binary = self.root / "binary"
        binary.touch()
        output = self.root / "output"
        return ["perf_native.py", str(fixture), str(output), "--consent", "--split", "below",
                "--binary", str(binary)], output

    def test_main_defaults_to_two_representative_shapes_three_runs(self):
        argv, output = self.main_args()
        with patch.object(sys, "argv", argv), patch.object(runner, "Desktop") as desktop, \
             patch.object(runner, "command", return_value="font"), \
             patch.object(runner, "run_probe", return_value=rows()) as probe:
            runner.main()
        self.assertEqual([call.args[2:] for call in probe.call_args_list],
                         [(s, r) for s in ("mixed", "single-ascii") for r in (1, 2, 3)])
        self.assertTrue(all(call.args[0].split == "below" for call in probe.call_args_list))
        desktop.return_value.restore.assert_called_once()
        metadata = json.loads((output / "environment.json").read_text())
        self.assertTrue(metadata["completed"])
        self.assertFalse(metadata["acceptance"])
        self.assertEqual(metadata["all_view_ready_budget"], "not-established")
        summary = json.loads((output / "summary.json").read_text())
        self.assertTrue(any(r["metric"] == "all_ready_submit_ms" and r["max"] == 350 for r in summary))

    def test_main_preserves_stop_and_never_retries_or_continues(self):
        argv, output = self.main_args()
        with patch.object(sys, "argv", argv), patch.object(runner, "Desktop") as desktop, \
             patch.object(runner, "command", return_value="font"), \
             patch.object(runner, "run_probe", side_effect=RuntimeError("STOP insert: over budget")) as probe:
            with self.assertRaisesRegex(RuntimeError, "STOP insert"):
                runner.main()
        probe.assert_called_once()
        desktop.return_value.restore.assert_called_once()
        self.assertIn("STOP insert", (output / "STOP.txt").read_text())
        self.assertFalse(json.loads((output / "environment.json").read_text())["completed"])
        self.assertFalse((output / "summary.json").exists())

    def test_probe_invocation_routes_split_without_os_key_injection(self):
        from argparse import Namespace
        script = self.root / "fake_probe.py"
        script.write_text(
            "from pathlib import Path\nimport sys\n"
            "assert sys.argv[3:] == ['--split', 'right']\n"
            f"Path(sys.argv[2]).write_text({report(rows())!r})\n"
        )
        args = Namespace(output=self.root, binary=Path(sys.executable), input=script,
                         software=False, ime=False, m1=False, split="right")
        with patch.object(runner, "drive_ime") as ime:
            self.assertEqual(len(runner.run_probe(args, None, "mixed", 1)), 73)
            ime.assert_not_called()

    def test_nonzero_probe_keeps_partial_report_and_stop_reason(self):
        from argparse import Namespace
        script = self.root / "failed_probe.py"
        script.write_text(
            "from pathlib import Path\nimport sys\n"
            "Path(sys.argv[2]).write_text('# completed=false;\\n')\n"
            "print('STOP insert: missing peer', flush=True)\nraise SystemExit(2)\n"
        )
        args = Namespace(output=self.root, binary=Path(sys.executable), input=script,
                         software=False, ime=False, m1=False, split="right")
        with self.assertRaisesRegex(RuntimeError, "STOP insert: missing peer"):
            runner.run_probe(args, None, "mixed", 1)
        self.assertIn("completed=false", (self.root / "mixed-1.csv").read_text())


if __name__ == "__main__":
    unittest.main()
