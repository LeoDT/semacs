import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import sys

ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location("m2_manual", ROOT / "scripts/m2_manual.py")
manual = importlib.util.module_from_spec(SPEC)
with patch.object(sys, "path", [str(ROOT / "scripts"), *sys.path]):
    SPEC.loader.exec_module(manual)


class ManualFixtureTests(unittest.TestCase):
    def setUp(self):
        (ROOT / ".pi/tmp").mkdir(parents=True, exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(dir=ROOT / ".pi/tmp")
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name) / "new"
        manual.prepare(self.directory)

    def test_refuses_reuse_and_preserves_bom_crlf_no_final_newline(self):
        self.assertEqual((self.directory / "alpha.txt").read_bytes(), b"\xef\xbb\xbfalpha\r\nsecond")
        with self.assertRaises(FileExistsError):
            manual.prepare(self.directory)

    def test_machine_bytes_do_not_imply_user_observations_passed(self):
        for name, data in manual.EXPECTED.items():
            (self.directory / name).write_bytes(data)
        with self.assertRaisesRegex(RuntimeError, "not confirmed"):
            manual.verify(self.directory)
        self.assertFalse(json.loads((self.directory / "result.json").read_text())["completed"])

    def test_all_bytes_and_observations_are_required(self):
        for name, data in manual.EXPECTED.items():
            (self.directory / name).write_bytes(data)
        (self.directory / "observations.tsv").write_text(
            "".join(f"{name}\tyes\n" for name in manual.OBSERVATIONS)
        )
        self.assertTrue(manual.verify(self.directory)["completed"])
        (self.directory / "overwrite.txt").write_bytes(b"wrong")
        with self.assertRaisesRegex(RuntimeError, "disk-byte mismatch"):
            manual.verify(self.directory)


if __name__ == "__main__":
    unittest.main()
