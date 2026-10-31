import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock

ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location("perf_native", ROOT / "scripts/perf_native.py")
RUNNER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RUNNER)


class NativeRunnerTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        (ROOT / ".pi/tmp").mkdir(parents=True, exist_ok=True)

    def report(self, text, software=False):
        with tempfile.TemporaryDirectory(dir=ROOT / ".pi/tmp") as directory:
            path = Path(directory) / "report.csv"
            path.write_text(text)
            return RUNNER.read_report(path, software, True)

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
        desktop = RUNNER.Desktop.__new__(RUNNER.Desktop)
        desktop.target = Mock(return_value={"address": "probe"})
        desktop.active = Mock(return_value={"address": "other"})
        with self.assertRaisesRegex(RuntimeError, "refusing to inject"):
            desktop.keys("token", "ni")

    def test_explicit_software_report_is_not_hardware(self):
        software = self.valid_report().replace("false", "true")
        self.assertEqual(len(self.report(software, software=True)), 9)


if __name__ == "__main__":
    unittest.main()
