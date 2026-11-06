import copy
import csv
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import perf_status


class FixtureTests(unittest.TestCase):
    def setUp(self):
        (perf_status.ROOT / ".pi/tmp").mkdir(parents=True, exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(dir=perf_status.ROOT / ".pi/tmp")
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name) / "fixture"
        self.digest = perf_status.prepare_fixture(self.directory, 8)

    def test_known_groups_overlap_and_complete_index_oracle(self):
        manifest = perf_status.verify_fixture(self.directory, self.digest)
        self.assertEqual((manifest["records"], manifest["projected_leaves"]), (8, 10))
        base = (self.directory / "base.expected").read_text()
        stage = (self.directory / "stage.expected").read_text()
        for expected in ("Untracked files (2)", "Unstaged changes (4)",
                         "Staged changes (4)", "Unmerged paths (0)"):
            self.assertIn(expected, base)
        self.assertEqual(base.count("both-0000.txt"), 2)
        self.assertIn("Unstaged changes (3)", stage)
        self.assertIn("Staged changes (5)", stage)
        self.assertNotIn(f" .M {perf_status.TARGET}", stage)
        repo = self.directory / "repo"
        perf_status.git(repo, "add", "--", perf_status.TARGET)
        self.assertEqual(perf_status.git(repo, "ls-files", "--stage", "-z"),
                         (self.directory / "stage.index").read_bytes())
        with self.assertRaisesRegex(RuntimeError, "index entries"):
            perf_status.verify_fixture(self.directory, self.digest)
        perf_status.git(repo, "reset", "--quiet", "HEAD", "--", perf_status.TARGET)
        perf_status.verify_fixture(self.directory, self.digest)

    def test_tampered_protected_files_manifest_lock_and_index_are_rejected(self):
        for relative in ("repo/both-0000.txt", "repo/.git/config", "repo/.git/refs/heads/main",
                         "base.expected", "stage.index", "head", "manifest.json"):
            with self.subTest(relative=relative):
                path = self.directory / relative
                old = path.read_bytes()
                path.write_bytes(old + b"tampered")
                with self.assertRaises(RuntimeError):
                    perf_status.verify_fixture(self.directory, self.digest)
                path.write_bytes(old)
        lock = self.directory / "repo/.git/index.lock"
        lock.write_bytes(b"must not disappear")
        with self.assertRaisesRegex(RuntimeError, "protected"):
            perf_status.verify_fixture(self.directory, self.digest)

    def test_existing_directory_and_out_of_range_samples_are_refused(self):
        with self.assertRaises(FileExistsError):
            perf_status.prepare_fixture(self.directory, 8)
        for count in (0, 7, 9, 4100):
            with self.assertRaises(ValueError):
                perf_status.prepare_fixture(self.directory, count)
        self.assertEqual(perf_status.SIZES["limit"], 4096)

    def test_git_overrides_do_not_redirect_fixture_oracle(self):
        with patch.dict(perf_status.os.environ, {"GIT_DIR": "/invalid", "GIT_INDEX_FILE": "/invalid"}):
            perf_status.verify_fixture(self.directory, self.digest)


class ReportTests(unittest.TestCase):
    def setUp(self):
        (perf_status.ROOT / ".pi/tmp").mkdir(parents=True, exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(dir=perf_status.ROOT / ".pi/tmp")
        self.addCleanup(self.temp.cleanup)
        self.report = Path(self.temp.name) / "report.csv"
        self.rows = [dict(phase=phase, views=2 if 4 <= i <= 9 else 1, action_cpu_ms="0.1",
                          first_ms="1", selected_ready_ms="2", all_ready_ms="3",
                          probe_draw_max_ms="1", frames="1", min_visible_rows="10", bytes="1000",
                          projection_rows="100", write_records=1 if i == 8 else 2 if i >= 9 else 0,
                          verified="true") for i, phase in enumerate(perf_status.ACTIONS)]

    def write(self, rows=None, completed=True):
        rows = self.rows if rows is None else rows
        with self.report.open("w") as output:
            writer = csv.DictWriter(output, fieldnames=self.rows[0])
            writer.writeheader()
            writer.writerows(rows)
            if completed:
                output.write("# completed=true; baseline_only=true\n")

    def test_complete_sequence_and_large_baseline_timings_have_no_new_budget(self):
        rows = copy.deepcopy(self.rows)
        rows[0].update(first_ms="400", selected_ready_ms="500", all_ready_ms="600")
        self.write(rows)
        self.assertEqual(len(perf_status.read_report(self.report)), len(perf_status.ACTIONS))

    def test_truncation_wrong_order_and_missing_footer_fail(self):
        for rows, completed in ((self.rows[:-1], True), (self.rows[::-1], True), (self.rows, False)):
            self.write(rows, completed)
            with self.assertRaises(RuntimeError):
                perf_status.read_report(self.report)

    def test_malformed_metrics_geometry_records_and_verification_fail(self):
        for key, value in (("all_ready_ms", "nan"), ("first_ms", "inf"), ("first_ms", "-1"),
                           ("all_ready_ms", "0"), ("verified", "false"), ("views", "2"),
                           ("min_visible_rows", "0"), ("projection_rows", "0"),
                           ("write_records", "1"), ("frames", "0")):
            with self.subTest(key=key, value=value):
                rows = copy.deepcopy(self.rows)
                rows[0][key] = value
                self.write(rows)
                with self.assertRaises(RuntimeError):
                    perf_status.read_report(self.report)


if __name__ == "__main__":
    unittest.main()
