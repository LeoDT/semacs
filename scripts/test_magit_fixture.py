"""Offline checks for disposable Magit fixtures and their isolated child launch."""

import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import magit_fixture
import m2_manual


class MagitFixtureTests(unittest.TestCase):
    def setUp(self):
        root = Path(__file__).resolve().parents[1] / ".pi/tmp"
        root.mkdir(parents=True, exist_ok=True)
        temporary = tempfile.TemporaryDirectory(prefix="magit-fixture-tools-", dir=root)
        self.addCleanup(temporary.cleanup)
        self.directory = Path(temporary.name) / "files"

    def prepare(self):
        m2_manual.prepare(self.directory, "magit")
        (self.directory / "observations.tsv").write_text(
            "".join(f"{name}\tyes\n" for name in magit_fixture.OBSERVATIONS)
        )

    def test_head_index_worktree_are_distinct_and_verify_without_writes(self):
        self.prepare()
        before = (self.directory / ".git/index").read_bytes()
        result = m2_manual.verify(self.directory, "magit")
        self.assertTrue(result["completed"])
        self.assertTrue(all(result["git_state"].values()))
        self.assertTrue(all(result["disk_bytes"].values()))
        for revision, marker in (("HEAD", b"HEAD-OLD"), ("", b"INDEX-NEW")):
            self.assertTrue(magit_fixture.git(self.directory, "cat-file", "blob", f"{revision}:00-main.txt").startswith(marker))
        self.assertTrue((self.directory / "00-main.txt").read_bytes().startswith(b"WORKTREE-NEW"))
        self.assertGreater(len((self.directory / "10-long.txt").read_bytes()), 8192)
        self.assertEqual((self.directory / ".git/index").read_bytes(), before)
        self.assertEqual(result["ime"], "unmeasured")
        self.assertEqual(result["gpu"], "unmeasured")
        self.assertFalse(result["screenshots"])

    def test_staging_changes_cannot_pass_even_when_worktree_is_restored(self):
        self.prepare()
        magit_fixture.git(self.directory, "add", "--", "00-main.txt")
        with self.assertRaisesRegex(RuntimeError, "Git fixture mismatch"):
            m2_manual.verify(self.directory, "magit")
        report = json.loads((self.directory / "result.json").read_text())
        self.assertTrue(report["disk_bytes"]["00-main.txt"])
        self.assertFalse(report["git_state"]["index:00-main.txt"])
        self.assertFalse(report["completed"])

    def test_missing_metadata_does_not_fall_back_to_parent_repository(self):
        self.prepare()
        (self.directory / ".git").rename(self.directory / "hidden-git")
        with self.assertRaisesRegex(RuntimeError, "fixture .git is missing"):
            m2_manual.verify(self.directory, "magit")
        self.assertFalse(json.loads((self.directory / "result.json").read_text())["completed"])

    def test_modified_git_config_is_preserved_and_rejected(self):
        self.prepare()
        magit_fixture.git(self.directory, "config", "--local", "core.quotePath", "false")
        config = (self.directory / ".git/config").read_bytes()
        with self.assertRaisesRegex(RuntimeError, "Git fixture mismatch: config_bytes"):
            m2_manual.verify(self.directory, "magit")
        self.assertEqual((self.directory / ".git/config").read_bytes(), config)

    def test_missing_observation_or_configuration_change_stops(self):
        self.prepare()
        (self.directory / "observations.tsv").write_text("magit-horizontal\tno\n")
        (self.directory / "config/semacs/init.js").write_bytes(b"changed")
        with self.assertRaisesRegex(RuntimeError, "disk-byte mismatch"):
            m2_manual.verify(self.directory, "magit")
        self.assertEqual((self.directory / "config/semacs/init.js").read_bytes(), b"changed")
        report = json.loads((self.directory / "result.json").read_text())
        self.assertFalse(report["completed"])
        self.assertIn("manual observation not confirmed: magit-horizontal", report["errors"])

    def test_child_environment_does_not_inherit_daily_git_or_config(self):
        self.prepare()
        fake = self.directory / "fake-semacs"
        capture = self.directory / "child.json"
        fake.write_text("#!/usr/bin/env python3\nimport json,os,sys\nfrom pathlib import Path\n"
                        "Path(os.environ['CAPTURE']).write_text(json.dumps({'env':dict(os.environ),'args':sys.argv[1:],'cwd':os.getcwd()}))\n")
        fake.chmod(0o755)
        daily = {"GIT_DIR": "/daily/repo/.git", "GIT_INDEX_FILE": "/daily/index", "GIT_CONFIG_COUNT": "1",
                 "GIT_CONFIG_KEY_0": "core.worktree", "GIT_CONFIG_VALUE_0": "/daily/worktree", "XDG_CONFIG_HOME": "/daily/config", "CAPTURE": str(capture)}
        with patch.dict(os.environ, daily):
            self.assertEqual(magit_fixture.launch(self.directory, fake), 0)
            self.assertEqual(os.environ["XDG_CONFIG_HOME"], "/daily/config")
            self.assertEqual(os.environ["GIT_DIR"], daily["GIT_DIR"])
        result = json.loads(capture.read_text())
        self.assertEqual(result["cwd"], str(self.directory))
        self.assertEqual(result["args"], [str(self.directory / "00-main.txt")])
        self.assertEqual(result["env"]["XDG_CONFIG_HOME"], str(self.directory / "config"))
        self.assertEqual(result["env"]["GIT_CONFIG_GLOBAL"], os.devnull)
        for name in daily:
            if name.startswith("GIT_"):
                self.assertNotIn(name, result["env"])

    def test_cli_requires_consent_and_preserves_failing_child_status(self):
        source = Path(magit_fixture.__file__)
        result = subprocess.run(["python3", "-B", str(source), str(self.directory), "--binary", "/missing"], capture_output=True)
        self.assertEqual(result.returncode, 2)
        self.assertIn(b"consent", result.stderr)
        self.prepare()
        fake = self.directory / "fake-failure"
        fake.write_text("#!/bin/sh\nexit 7\n"); fake.chmod(0o755)
        result = subprocess.run(["python3", "-B", str(source), str(self.directory), "--binary", str(fake), "--consent"], capture_output=True)
        self.assertEqual(result.returncode, 7)

    def test_changed_head_is_rejected_even_with_identical_blobs(self):
        self.prepare()
        head = magit_fixture.git(self.directory, "rev-parse", "HEAD").decode().strip()
        tree = magit_fixture.git(self.directory, "rev-parse", "HEAD^{tree}").decode().strip()
        commit = magit_fixture.git(self.directory, "commit-tree", tree, "-p", head, "-m", "unexpected commit").decode().strip()
        magit_fixture.git(self.directory, "update-ref", "HEAD", commit)
        with self.assertRaisesRegex(RuntimeError, "Git fixture mismatch: head_identity"):
            m2_manual.verify(self.directory, "magit")
        report = json.loads((self.directory / "result.json").read_text())
        self.assertTrue(report["git_state"]["HEAD:00-main.txt"])
        self.assertFalse(report["git_state"]["head_identity"])

    def test_prelaunch_file_change_prevents_even_a_mock_child_from_starting(self):
        self.prepare()
        marker = self.directory / "launched"
        fake = self.directory / "fake-marker"
        fake.write_text(f"#!/bin/sh\ntouch '{marker}'\n"); fake.chmod(0o755)
        (self.directory / "00-main.txt").write_bytes(b"changed before launch")
        result = subprocess.run(["python3", "-B", magit_fixture.__file__, str(self.directory), "--binary", str(fake), "--consent"], capture_output=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b"file/configuration changed", result.stderr)
        self.assertFalse(marker.exists())

    def test_timed_out_git_is_reportable_and_new_fixture_cannot_overwrite(self):
        self.prepare()
        with patch.object(magit_fixture.subprocess, "run", side_effect=subprocess.TimeoutExpired("git", 15)):
            with self.assertRaisesRegex(RuntimeError, "15 second limit"):
                m2_manual.verify(self.directory, "magit")
        self.assertFalse(json.loads((self.directory / "result.json").read_text())["completed"])
        with self.assertRaises(FileExistsError):
            m2_manual.prepare(self.directory, "magit")


if __name__ == "__main__":
    unittest.main()
