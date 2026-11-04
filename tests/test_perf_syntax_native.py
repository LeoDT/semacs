"""Offline E2 report/driver checks; never launch semacs or access the desktop."""

import argparse
import csv
import importlib.util
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, ROOT / path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


runner = module("syntax_runner", "scripts/perf_native.py")
samples = module("syntax_samples", "scripts/perf_syntax_sample.py")


def rows(fixture):
    phases = ["open", "insert", "delete", "newline", "join", "burst-insert", "burst-delete"] + ["forced"] * 30 + ["idle"]
    result = []
    for index, phase in enumerate(phases):
        delta = [0, 1, 0, 0, -1, 31, -1][min(index, 6)]
        version = f"TextVersion({[0, 1, 2, 3, 4, 36, 68][min(index, 6)]})"
        value = dict.fromkeys(runner.SYNTAX_FIELDS, "")
        value.update(
            phase=phase, step=str(index), bytes=str(len(fixture) + delta),
            point=str(runner.syntax_input_offset(fixture, "markdown") + delta),
            target_buffer="BufferId(1)", target_version=version,
            painted_buffer="BufferId(1)", painted_version=version,
            syntax_pending_before_action="true" if index <= 6 else "false",
            syntax_pending_frames="1" if index < 6 else "0",
            first_frame_submit_ms="1", first_submit_ms="1", ready_submit_ms="2",
            syntax_and_layout_submit_ms="2" if index >= 6 else "",
            action_cpu_ms="0.1", draw_cpu_max_ms="1", frames="1",
            missing_observations="0", blank_frames="0", idle_ticks="0",
            visible_rows="20", indexed_bytes=str(len(fixture) + delta),
            width_px="944", height_px="576", first_visible_rows="20",
            text_verified="true", syntax_status="ready" if index >= 6 else "not-observed",
            observed_elapsed_ms="3",
        )
        if index >= 6:
            value.update(syntax_buffer="BufferId(1)", syntax_version=version)
        if phase == "idle":
            value.update(
                first_frame_submit_ms="", first_submit_ms="", ready_submit_ms="",
                syntax_and_layout_submit_ms="", syntax_status="idle",
                action_cpu_ms="0", draw_cpu_max_ms="0", frames="0",
                idle_ticks="30", first_visible_rows="0",
            )
        result.append(value)
    return result


def report(values, fixture, sample="markdown", software=False):
    output = io.StringIO()
    output.write(f"# mode=syntax; sample={sample}; release=true; fixture_bytes={len(fixture)}; input_offset={runner.syntax_input_offset(fixture, sample)}; graphics=is_software_emulated: {str(software).lower()}\n")
    writer = csv.DictWriter(output, fieldnames=runner.SYNTAX_FIELDS)
    writer.writeheader()
    writer.writerows(values)
    pending = sum(row["syntax_pending_before_action"] == "true" for row in values[1:7])
    output.write(f"# completed=true; input_while_syntax_pending={pending}; layout_budget_separate=true; syntax_ready_budget=not-established; gpu_execution_ms=unmeasured; compositor_presentation_ms=unmeasured\n")
    return output.getvalue()


class SyntaxTestCase(unittest.TestCase):
    def setUp(self):
        temporary = ROOT / ".pi/tmp"
        temporary.mkdir(parents=True, exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(prefix="syntax-native-", dir=temporary)
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.fixture = samples.sample("markdown")

    def validate(self, values=None, content=None, software=False):
        values = rows(self.fixture) if values is None else values
        path = self.root / "report.csv"
        path.write_text(report(values, self.fixture, software=software) if content is None else content)
        return runner.read_report(path, software, False, sample="markdown", syntax=True, fixture=self.fixture)


class SyntaxReportTests(SyntaxTestCase):
    def test_exact_language_enabled_matrix_and_independent_ready_boundaries(self):
        self.assertEqual(len(self.validate()), 38)
        values = rows(self.fixture)
        values[6].update(syntax_and_layout_submit_ms="2000", observed_elapsed_ms="2001")
        self.validate(values)
        self.assertEqual(values[6]["ready_submit_ms"], "2")

    def test_ret_cleans_space_and_join_does_not_restore_it(self):
        values = rows(self.fixture)
        offset = runner.syntax_input_offset(self.fixture, "markdown")
        self.assertEqual([int(row["bytes"]) - len(self.fixture) for row in values[:7]],
                         [0, 1, 0, 0, -1, 31, -1])
        self.assertEqual([int(row["point"]) - offset for row in values[:7]],
                         [0, 1, 0, 0, -1, 31, -1])
        for index, delta in ((3, 1), (4, 0), (5, 32), (6, 0), (37, 0)):
            invalid = rows(self.fixture)
            invalid[index].update(bytes=str(len(self.fixture) + delta),
                                  point=str(offset + delta), indexed_bytes=str(len(self.fixture) + delta))
            with self.subTest(index=index), self.assertRaises(RuntimeError):
                self.validate(invalid)

    def test_missing_or_reordered_phases_and_duplicate_columns_fail(self):
        values = rows(self.fixture)
        for invalid in (values[:-1], values[1:] + values[:1]):
            with self.assertRaises(RuntimeError):
                self.validate(invalid)
        content = report(values, self.fixture)
        for invalid in (
            content.replace("# completed=true;", "# completed=false;"),
            content.replace("sample=markdown", "sample=html"),
            content.replace("release=true", "release=false"),
            content.replace("phase,step,", "phase,phase,step,"),
            content.replace("fixture_bytes=", "different_bytes="),
            content.replace("is_software_emulated: false", "is_software_emulated: true"),
        ):
            with self.subTest(invalid=invalid[:100]), self.assertRaises(RuntimeError):
                self.validate(content=invalid)

    def test_wrong_snapshot_text_point_or_syntax_cannot_pass(self):
        for index, key, value in (
            (2, "target_version", "TextVersion(1)"),
            (2, "target_buffer", "BufferId(9)"),
            (2, "painted_version", "TextVersion(1)"),
            (6, "syntax_buffer", "BufferId(9)"),
            (6, "syntax_version", "TextVersion(4)"),
            (6, "syntax_status", "not-observed"),
            (6, "text_verified", "false"),
            (3, "bytes", str(len(self.fixture) + 1)),
            (4, "point", str(runner.syntax_input_offset(self.fixture, "markdown"))),
            (6, "bytes", str(len(self.fixture))),
            (3, "point", "0"),
            (0, "target_buffer", ""),
            (0, "syntax_buffer", "BufferId(1)"),
        ):
            with self.subTest(index=index, key=key):
                values = rows(self.fixture)
                values[index][key] = value
                with self.assertRaises(RuntimeError):
                    self.validate(values)

    def test_frame_coverage_and_idle_are_fail_closed_even_in_software(self):
        for index, key, value in (
            (0, "missing_observations", "1"), (0, "first_visible_rows", "0"),
            (0, "blank_frames", "1"), (6, "indexed_bytes", "1"),
            (1, "syntax_pending_frames", "2"), (1, "frames", "1.5"),
            (37, "frames", "1"), (37, "idle_ticks", "29"),
            (37, "first_submit_ms", "1"), (7, "syntax_pending_before_action", "true"),
        ):
            values = rows(self.fixture)
            values[index][key] = value
            with self.subTest(index=index, key=key), self.assertRaises(RuntimeError):
                self.validate(values, software=True)

    def test_unobserved_pending_input_is_a_coverage_failure_not_a_pass(self):
        values = rows(self.fixture)
        for row in values[1:7]:
            row["syntax_pending_before_action"] = "false"
        with self.assertRaisesRegex(RuntimeError, "coverage missing"):
            self.validate(values)
        with self.assertRaises(RuntimeError):
            self.validate(content=report(rows(self.fixture), self.fixture).replace("input_while_syntax_pending=6", "input_while_syntax_pending=1"))

    def test_nonfinite_negative_and_contradictory_timing_is_rejected(self):
        for key in ("action_cpu_ms", "draw_cpu_max_ms", "ready_submit_ms", "observed_elapsed_ms"):
            for invalid in ("nan", "inf", "-1", ""):
                values = rows(self.fixture)
                values[2][key] = invalid
                with self.subTest(key=key, invalid=invalid), self.assertRaises(RuntimeError):
                    self.validate(values)
        for key, value in (("syntax_and_layout_submit_ms", "nan"), ("syntax_and_layout_submit_ms", "1"),
                           ("syntax_and_layout_submit_ms", "4"), ("syntax_and_layout_submit_ms", "")):
            values = rows(self.fixture)
            values[6][key] = value
            with self.assertRaises(RuntimeError):
                self.validate(values)

    def test_existing_cpu_and_layout_limits_are_rechecked(self):
        for key, invalid in (("action_cpu_ms", "2.001"), ("first_submit_ms", "16.001"),
                             ("ready_submit_ms", "300.001"), ("draw_cpu_max_ms", "16.001")):
            values = rows(self.fixture)
            values[2][key] = invalid
            values[2]["observed_elapsed_ms"] = "400"
            if key == "first_submit_ms":
                values[2]["ready_submit_ms"] = "17"
            with self.subTest(key=key), self.assertRaisesRegex(RuntimeError, "STOP"):
                self.validate(values)
        values = rows(self.fixture)
        values[5].update(action_cpu_ms="50.001", observed_elapsed_ms="51")
        with self.assertRaisesRegex(RuntimeError, "STOP"):
            self.validate(values)

    def test_pending_coverage_cannot_contradict_prior_completion_or_frame_counts(self):
        values = rows(self.fixture)
        for row in values[:6]:
            row.update(syntax_status="ready", syntax_buffer=row["target_buffer"],
                       syntax_version=row["target_version"], syntax_and_layout_submit_ms="2",
                       syntax_pending_frames="0")
        with self.assertRaisesRegex(RuntimeError, "already ready"):
            self.validate(values)
        values = rows(self.fixture)
        values[6]["syntax_pending_frames"] = "1"
        with self.assertRaisesRegex(RuntimeError, "timing boundary"):
            self.validate(values)

    def test_input_offset_uses_script_context_not_an_earlier_same_named_plain_token(self):
        for sample in ("markdown", "html"):
            fixture = b"call(42)\n" + samples.sample(sample)
            path = self.root / ("sample.md" if sample == "markdown" else "sample.html")
            path.write_bytes(fixture)
            self.assertEqual(runner.syntax_fixture(path, sample), fixture)
            offset = runner.syntax_input_offset(fixture, sample)
            self.assertGreater(offset, fixture.index(b"call(42)"))
            self.assertEqual(fixture[offset - len(b"<script>const n = "):offset], b"<script>const n = ")

    def test_fixture_generation_is_deterministic_bounded_and_refuses_reuse(self):
        directory = self.root / "files"
        samples.prepare(directory)
        for sample, extension in samples.EXTENSIONS.items():
            path = directory / f"{sample}.{extension}"
            self.assertEqual(runner.syntax_fixture(path, sample), samples.sample(sample))
            self.assertTrue(256 * 1024 <= path.stat().st_size < 257 * 1024)
        with self.assertRaises(FileExistsError):
            samples.prepare(directory)
        bad = directory / "plain.txt"
        bad.write_bytes(self.fixture)
        with self.assertRaises(RuntimeError):
            runner.syntax_fixture(bad, "markdown")


class SyntaxDriverTests(SyntaxTestCase):
    def argv(self, fixture, output, binary, flags=()):
        return ["perf_native.py", str(fixture), str(output), "--consent", "--syntax",
                "--sample", "markdown", "--runs", "3", "--binary", str(binary),
                "--syntax-manifest-sha256", self.manifest_sha, *flags]

    def setup_driver(self):
        files = self.root / "files"
        self.manifest_sha = samples.prepare(files)
        fixture = files / "markdown.md"
        binary = self.root / "fake-probe"
        binary.touch()
        return fixture, binary

    def test_three_runs_use_new_directories_isolated_config_and_stop_on_first_failure(self):
        fixture, binary = self.setup_driver()
        for failed in (False, True):
            output = self.root / ("failed" if failed else "passed")
            with patch.object(sys, "argv", self.argv(fixture, output, binary)), \
                 patch.object(runner, "Desktop") as desktop, \
                 patch.object(runner, "command", return_value="offline identity"), \
                 patch.object(runner, "run_probe", return_value=rows(self.fixture)) as probe, \
                 patch("sys.stdout", new=io.StringIO()):
                if failed:
                    probe.side_effect = RuntimeError("STOP: coverage missing")
                    with self.assertRaises(RuntimeError):
                        runner.main()
                else:
                    runner.main()
                self.assertEqual(probe.call_count, 1 if failed else 3)
                desktop.assert_called_once_with(unittest.mock.ANY, False)
                desktop.return_value.restore.assert_called_once()
            result = json.loads((output / "environment.json").read_text())
            self.assertEqual(result["completed"], not failed)
            self.assertTrue(result["syntax"])
            self.assertFalse(result["acceptance"])
            self.assertEqual(result["syntax_ready_budget"], "not-established")
            self.assertEqual((output / "config/semacs/init.js").read_bytes(), b"")
            self.assertEqual(fixture.read_bytes(), self.fixture)
            self.assertEqual((output / "STOP.txt").exists(), failed)

    def test_mode_restrictions_are_checked_before_opening_any_window(self):
        for flags in (["--syntax", "--sample", "mixed"], ["--sample", "markdown"],
                      ["--syntax"], ["--syntax", "--sample", "html", "--sample", "markdown"],
                      ["--syntax", "--m1"], ["--syntax", "--ime"]):
            output = self.root / "unused"
            with patch.object(sys, "argv", ["perf_native.py", "unused", str(output), "--consent", *flags]), \
                 patch.object(runner, "Desktop") as desktop, patch("sys.stderr", new=io.StringIO()):
                with self.assertRaises(SystemExit):
                    runner.main()
                desktop.assert_not_called()
                self.assertFalse(output.exists())

    def test_fake_child_proves_environment_isolation_and_actual_report_validation(self):
        fixture, binary = self.setup_driver()
        output = self.root / "child"
        config = output / "config/semacs/init.js"
        config.parent.mkdir(parents=True)
        config.write_bytes(b"")
        binary.write_text("#!/usr/bin/env python3\nimport json, os, sys\nfrom pathlib import Path\n"
                          "output = Path(sys.argv[3])\n"
                          f"output.write_text({report(rows(self.fixture), self.fixture)!r})\n"
                          "config = Path(os.environ['XDG_CONFIG_HOME']) / 'semacs/init.js'\n"
                          "output.with_suffix('.env.json').write_text(json.dumps({'config': str(config), 'bytes': config.read_text(), 'args': sys.argv[4:]}))\n")
        binary.chmod(0o755)
        args = argparse.Namespace(output=output, input=fixture, binary=binary, software=False, ime=False, m1=False, syntax=True)
        before = os.environ.get("XDG_CONFIG_HOME")
        values = runner.run_probe(args, None, "markdown", 1)
        self.assertEqual(len(values), 38)
        observed = json.loads((output / "markdown-1.env.json").read_text())
        self.assertEqual(observed, {"config": str(config), "bytes": "", "args": ["--syntax"]})
        self.assertEqual(os.environ.get("XDG_CONFIG_HOME"), before)

    def test_other_sample_changes_abort_before_the_next_window_and_stay_preserved(self):
        fixture, binary = self.setup_driver()
        other = fixture.parent / "html.html"
        changed = other.read_bytes().replace(b"const n", b"const x", 1)
        output = self.root / "other-changed"
        def probe(*_):
            other.write_bytes(changed)
            return rows(self.fixture)
        with patch.object(sys, "argv", self.argv(fixture, output, binary)), \
             patch.object(runner, "Desktop"), patch.object(runner, "command", return_value="offline"), \
             patch.object(runner, "run_probe", side_effect=probe) as called:
            with self.assertRaisesRegex(RuntimeError, "html.html"):
                runner.main()
            self.assertEqual(called.call_count, 1)
        self.assertEqual(other.read_bytes(), changed)
        self.assertFalse(json.loads((output / "environment.json").read_text())["completed"])
        self.assertTrue((output / "STOP.txt").exists())
        with patch.object(sys, "argv", [
            "perf_native.py", str(other), str(self.root / "html-second-process"),
            "--consent", "--syntax", "--sample", "html", "--binary", str(binary),
            "--syntax-manifest-sha256", self.manifest_sha,
        ]), patch.object(runner, "Desktop") as desktop, patch("sys.stderr", new=io.StringIO()):
            with self.assertRaises(SystemExit):
                runner.main()
            desktop.assert_not_called()
        self.assertFalse((self.root / "html-second-process").exists())

    def test_frozen_manifest_cannot_be_rebased_together_with_an_altered_fixture(self):
        fixture, _ = self.setup_driver()
        manifest = fixture.parent / "syntax-fixtures.json"
        frozen = self.manifest_sha
        metadata = json.loads(manifest.read_text())
        import hashlib
        other = fixture.parent / "html.html"
        other.write_bytes(other.read_bytes().replace(b"const n", b"const x", 1))
        metadata["files"]["html.html"] = hashlib.sha256(other.read_bytes()).hexdigest()
        manifest.write_text(json.dumps(metadata))
        with self.assertRaisesRegex(RuntimeError, "manifest identity changed"):
            runner.verify_syntax_batch(fixture.parent, frozen)

    def test_restore_or_configuration_failure_cannot_leave_completed_true(self):
        fixture, binary = self.setup_driver()
        for fault in ("restore", "config", "fixture"):
            output = self.root / fault
            with patch.object(sys, "argv", self.argv(fixture, output, binary)), \
                 patch.object(runner, "Desktop") as desktop, patch.object(runner, "command", return_value="offline"):
                if fault == "restore":
                    desktop.return_value.restore.side_effect = RuntimeError("restore failed")
                def probe(*_):
                    if fault == "config":
                        (output / "config/semacs/init.js").write_bytes(b"changed config")
                    if fault == "fixture":
                        fixture.write_bytes(b"preserved corrupted fixture")
                    return rows(self.fixture)
                with patch.object(runner, "run_probe", side_effect=probe) as called, patch("sys.stdout", new=io.StringIO()):
                    with self.assertRaises(RuntimeError):
                        runner.main()
                    self.assertEqual(called.call_count, 3 if fault == "restore" else 1)
            result = json.loads((output / "environment.json").read_text())
            self.assertFalse(result["completed"])
            self.assertTrue((output / "STOP.txt").exists())


if __name__ == "__main__":
    unittest.main()
