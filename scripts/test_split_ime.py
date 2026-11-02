"""Offline integration-driver checks. Never access the desktop."""

import csv
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

import split_ime


def report():
    output = io.StringIO()
    output.write("# hardware=true; performance=unmeasured\n")
    writer = csv.DictWriter(output, fieldnames=(
        "step", "phase", "first_point", "second_point", "body_hex", "mini_hex", "assertions_passed",
    ))
    writer.writeheader()
    for index, phase in enumerate(split_ime.PHASES):
        body = split_ime.INITIAL if index < 5 else (
            "你".encode() + split_ime.INITIAL if index < 10 else split_ime.FINAL
        )
        writer.writerow({
            "step": index, "phase": phase, "first_point": 0 if index < 10 else 3,
            "second_point": 0 if index < 5 else (3 if index < 10 else 6),
            "body_hex": body.hex(), "mini_hex": "你".encode().hex() if index == 15 else "",
            "assertions_passed": "true",
        })
    output.write("# completed=true; performance=unmeasured; candidate_visual=unreviewed\n")
    return output.getvalue()


class SplitImeTests(unittest.TestCase):
    def test_full_report_is_required_and_checked_independently(self):
        valid = report()
        self.assertEqual(len(split_ime.validate_report(valid)), 18)
        for invalid in (
            valid.replace("# completed=true;", "# completed=false;"),
            valid.replace("# hardware=true;", "# hardware=false;"),
            valid.replace("right-cancel", "unexpected"),
            valid.replace(split_ime.FINAL.hex(), split_ime.INITIAL.hex()),
            valid.replace(",true", ",false", 1),
            "\n".join(line for line in valid.splitlines() if not line.startswith("16,")),
        ):
            with self.subTest(invalid=invalid), self.assertRaises(RuntimeError):
                split_ime.validate_report(invalid)

    def test_focus_loss_refuses_actions_and_screenshots(self):
        desktop = Mock()
        desktop.target.return_value = {"address": "probe"}
        desktop.active.return_value = {"address": "other"}
        with patch.object(split_ime.subprocess, "run") as execute:
            with self.assertRaisesRegex(RuntimeError, "not focused"):
                split_ime.screenshot(desktop, "token", Path("unused.png"))
            execute.assert_not_called()

    def test_existing_output_is_preserved_without_desktop_access(self):
        root = Path(__file__).resolve().parents[1] / ".pi/tmp"
        root.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=root) as temporary:
            output = Path(temporary) / "existing"
            output.mkdir()
            sentinel = output / "evidence"
            sentinel.write_text("keep failure")
            with patch.object(split_ime, "Desktop") as desktop:
                with self.assertRaises(FileExistsError):
                    split_ime.run(output, "unused")
                desktop.assert_not_called()
            self.assertEqual(sentinel.read_text(), "keep failure")

    def fake_probe(self, root):
        binary = root / "target/release/examples/native_split_ime"
        binary.parent.mkdir(parents=True)
        binary.write_bytes(b"offline fake probe")
        process = Mock()
        process.stdout = io.StringIO("")
        process.stdin = io.StringIO()
        process.wait.return_value = 0
        process.poll.return_value = 0
        return process

    def test_failed_observation_stops_probe_and_restores_focus(self):
        root = Path(__file__).resolve().parents[1] / ".pi/tmp"
        root.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=root) as temporary:
            temporary = Path(temporary)
            process = self.fake_probe(temporary)
            process.poll.return_value = None
            desktop = Mock()
            desktop.original = "original"
            desktop.hypr.return_value = '[{"address": "original"}]'
            desktop.active.return_value = {"address": "original"}
            output = temporary / "run"
            with patch.object(split_ime, "ROOT", temporary), \
                    patch.object(split_ime, "Desktop", return_value=desktop), \
                    patch.object(split_ime.subprocess, "Popen", return_value=process), \
                    patch.object(split_ime, "drive", side_effect=RuntimeError("lost focus")):
                with self.assertRaisesRegex(RuntimeError, "lost focus"):
                    split_ime.run(output, "unused")
            process.terminate.assert_called_once()
            desktop.focus.assert_called_once_with("original")
            result = json.loads((output / "result.json").read_text())
            self.assertFalse(result["completed"])
            self.assertTrue(result["focus_restored"])

    def test_restore_failure_cannot_be_reported_as_success(self):
        root = Path(__file__).resolve().parents[1] / ".pi/tmp"
        root.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=root) as temporary:
            temporary = Path(temporary)
            process = self.fake_probe(temporary)
            desktop = Mock()
            desktop.original = "original"
            desktop.hypr.return_value = "[]"
            output = temporary / "run"

            def finish(*_):
                (output / "report.csv").write_text(report())
                (output / "input.txt").write_bytes(split_ime.FINAL)
                for index in range(6):
                    (output / f"{index}.png").write_bytes(b"offline placeholder")

            with patch.object(split_ime, "ROOT", temporary), \
                    patch.object(split_ime, "Desktop", return_value=desktop), \
                    patch.object(split_ime.subprocess, "Popen", return_value=process), \
                    patch.object(split_ime, "drive", side_effect=finish):
                with self.assertRaisesRegex(RuntimeError, "focus restore failed"):
                    split_ime.run(output, "unused")
            result = json.loads((output / "result.json").read_text())
            self.assertFalse(result["completed"])
            self.assertFalse(result["focus_restored"])
            desktop.focus.assert_not_called()

    def test_protocol_never_injects_keys_or_changes_the_input_method(self):
        process = Mock()
        process.stdin = io.StringIO()
        process.poll.return_value = None
        desktop = Mock()
        desktop.target.return_value = {"address": "probe", "monitor": 1}
        desktop.active.return_value = {"address": "probe"}
        with patch.object(split_ime, "wait_line") as wait, \
                patch.object(split_ime, "screenshot") as capture, \
                patch.object(split_ime.time, "sleep"), \
                patch.object(split_ime.subprocess, "run") as execute:
            split_ime.drive(process, desktop, Mock(), Path("unused"), "token")
        self.assertEqual(process.stdin.getvalue().splitlines(), list(split_ime.PHASES[1:]) + ["done"])
        self.assertEqual(capture.call_count, 6)
        expected_waits = ["READY"]
        for phase in split_ime.PHASES[1:]:
            expected_waits.extend(["ARMED " + phase, "DONE " + phase, "READY"])
        expected_waits.append("COMPLETED")
        self.assertEqual([call.args[1] for call in wait.call_args_list], expected_waits)
        desktop.keys.assert_not_called()
        execute.assert_not_called()

    def test_capture_covers_the_probe_monitor_instead_of_clipping_to_its_window(self):
        desktop = Mock()
        desktop.target.return_value = {"address": "probe", "monitor": 2}
        desktop.active.return_value = {"address": "probe"}
        desktop.hypr.return_value = '[{"id": 1, "name": "other"}, {"id": 2, "name": "probe-output"}]'
        with patch.object(split_ime.subprocess, "run") as execute:
            split_ime.screenshot(desktop, "token", Path("capture.png"))
        self.assertEqual(execute.call_args.args[0], ["grim", "-o", "probe-output", "capture.png"])

    def test_startup_failure_leaves_a_result_and_does_not_claim_completion(self):
        root = Path(__file__).resolve().parents[1] / ".pi/tmp"
        root.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=root) as temporary:
            output = Path(temporary) / "new"
            with patch.object(Path, "read_bytes", side_effect=RuntimeError("probe unavailable")):
                with self.assertRaisesRegex(RuntimeError, "probe unavailable"):
                    split_ime.run(output, "unused")
            result = json.loads((output / "result.json").read_text())
            self.assertFalse(result["completed"])
            self.assertEqual(result["performance"], "unmeasured")
            self.assertIn("probe unavailable", (output / "STOP.txt").read_text())


if __name__ == "__main__":
    unittest.main()
