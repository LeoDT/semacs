"""Offline fixture / mock-window checks; never opens a desktop or user Git state."""

import contextlib
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import magit_commit_manual as manual
from test_magit_status_manual import FakeChild


class FixtureTests(unittest.TestCase):
    def setUp(self):
        root = Path(__file__).resolve().parents[1] / ".pi/tmp"
        root.mkdir(parents=True, exist_ok=True)
        temporary = tempfile.TemporaryDirectory(prefix="f3-commit-tools-", dir=root)
        self.addCleanup(temporary.cleanup)
        self.directory = Path(temporary.name) / "commit"
        self.frozen = manual.prepare(self.directory)
        self.repo = self.directory / "repo"

    def verify(self, phase):
        return manual.verify(self.directory, phase, self.frozen)

    def message(self, text):
        (self.repo / ".git/COMMIT_EDITMSG").write_text(text)

    def test_base_cancel_active_commit_checkpoints_and_full_index_oracles(self):
        self.verify("base")
        self.message(manual.CANCEL_SUBJECT + "\n\n# comment\n")
        self.verify("canceled")
        self.message("# fresh editor template; unsaved edits must not reach disk\n")
        bridge = self.directory / "runtime/.semacs-editor-fixture"
        bridge.mkdir()
        self.assertEqual(len(self.verify("active")["editor_bridges"]), 1)
        bridge.rmdir()
        manual.git(self.repo, "commit", "-qm", manual.SAVE_SUBJECT)
        result = self.verify("committed")
        self.assertTrue(result["complete_tree"] and result["complete_index"])
        for name, data in manual.WORKTREE_FILES.items():
            self.assertEqual((self.repo / name).read_bytes(), data)

    def test_unchanged_head_does_not_count_as_success_even_with_matching_message(self):
        self.message(manual.SAVE_SUBJECT + "\n")
        with self.assertRaisesRegex(RuntimeError, "HEAD tree mismatch"):
            self.verify("committed")

    def test_wrong_parent_or_message_and_extra_refs_are_rejected(self):
        manual.git(self.repo, "commit", "-qm", manual.SAVE_SUBJECT)
        tree = manual.git(self.repo, "rev-parse", "HEAD^{tree}").decode().strip()
        head = manual.git(self.repo, "rev-parse", "HEAD").decode().strip()
        manual.git(self.repo, "update-ref", "refs/heads/extra", head)
        with self.assertRaisesRegex(RuntimeError, "unexpected refs"):
            self.verify("committed")
        manual.git(self.repo, "update-ref", "-d", "refs/heads/extra")
        for arguments in (("-m", manual.SAVE_SUBJECT), ("-p", head, "-m", "wrong message")):
            wrong = manual.git(self.repo, "commit-tree", tree, *arguments).decode().strip()
            manual.git(self.repo, "update-ref", "HEAD", wrong)
            with self.assertRaisesRegex(RuntimeError, "parent / message mismatch"):
                self.verify("committed")

    def test_index_message_or_bridge_failures_are_not_repaired(self):
        original = (self.repo / "20-control.txt").read_bytes()
        (self.repo / "20-control.txt").write_bytes(b"wrong staged content\n")
        manual.git(self.repo, "add", "--", "20-control.txt")
        (self.repo / "20-control.txt").write_bytes(original)
        before = (self.repo / ".git/index").read_bytes()
        with self.assertRaisesRegex(RuntimeError, "complete index"):
            self.verify("base")
        self.assertEqual((self.repo / ".git/index").read_bytes(), before)
        manual.git(self.repo, "reset", "--quiet", "HEAD", "--", "20-control.txt")
        self.message("incorrect cancellation\n")
        with self.assertRaisesRegex(RuntimeError, "message checkpoint"):
            self.verify("canceled")
        self.message(manual.CANCEL_SUBJECT + "\n")
        (self.directory / "runtime/.semacs-editor-leftover").mkdir()
        with self.assertRaisesRegex(RuntimeError, "bridge lifecycle"):
            self.verify("canceled")

    def test_fixture_configuration_worktree_metadata_and_symlinks_are_checked(self):
        for name in ("fixture.json", "config/semacs/init.js", "repo/20-control.txt", "repo/.git/config"):
            path = self.directory / name
            before = path.read_bytes()
            path.write_bytes(before + b"changed\n")
            with self.subTest(name=name), self.assertRaises(RuntimeError):
                self.verify("base")
            self.assertEqual(path.read_bytes(), before + b"changed\n")
            path.write_bytes(before)
        lock = self.repo / ".git/index.lock"
        lock.write_bytes(b"keep failure evidence")
        with self.assertRaisesRegex(RuntimeError, "index.lock"):
            self.verify("base")
        lock.unlink()
        metadata = self.repo / ".git"
        metadata.rename(self.repo / "hidden-git")
        with self.assertRaisesRegex(RuntimeError, "missing or linked"):
            self.verify("base")
        (self.repo / "hidden-git").rename(metadata)
        (self.repo / "linked").symlink_to(self.directory / "config")
        with self.assertRaisesRegex(RuntimeError, "linked fixture"):
            self.verify("base")

    def test_fixture_isolates_daily_git_overrides_and_default_ignore_attributes(self):
        daily = self.directory.parent / "daily"
        (daily / "git").mkdir(parents=True)
        (daily / "git/ignore").write_text("*.txt\n")
        (daily / "git/attributes").write_text("*.txt working-tree-encoding=UTF-16\n")
        with patch.dict(os.environ, {"XDG_CONFIG_HOME": str(daily), "GIT_DIR": "/daily", "GIT_INDEX_FILE": "/daily/index"}):
            other = self.directory.parent / "isolated"
            frozen = manual.prepare(other)
            manual.verify(other, "base", frozen)
        self.assertEqual((daily / "git/ignore").read_text(), "*.txt\n")
        with self.assertRaises(ValueError):
            self.verify("unknown")
        with self.assertRaises(FileExistsError):
            manual.prepare(self.directory)


class SessionTests(unittest.TestCase):
    def setUp(self):
        root = Path(__file__).resolve().parents[1] / ".pi/tmp"
        root.mkdir(parents=True, exist_ok=True)
        temporary = tempfile.TemporaryDirectory(prefix="f3-commit-session-", dir=root)
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.directory = self.root / "commit"
        self.binary = self.root / "fake-binary"
        self.binary.write_bytes(b"not executable; never launched")
        self.child = FakeChild()
        self.launches = []
        self.prompts = []
        self.output = io.StringIO()

    def popen(self, args, **options):
        self.launches.append((args, options))
        return self.child

    def ask(self, prompt):
        self.prompts.append(prompt)
        repo = self.directory / "repo"
        message = repo / ".git/COMMIT_EDITMSG"
        if prompt.startswith("cancel-commit"):
            message.write_text(manual.CANCEL_SUBJECT + "\n\n# instructions\n")
        elif prompt.startswith("decline-os-close"):
            message.write_text("# fresh file; draft remains in memory\n")
            (self.directory / "runtime/.semacs-editor-mock").mkdir()
        elif prompt.startswith("save-commit"):
            manual.git(repo, "commit", "-qm", manual.SAVE_SUBJECT)
            shutil.rmtree(self.directory / "runtime/.semacs-editor-mock")
        elif prompt.startswith("请记录"):
            return "1100x700; readable"
        return "yes"

    def run_session(self, ask=None):
        api = SimpleNamespace(Popen=self.popen, DEVNULL=subprocess.DEVNULL, TimeoutExpired=subprocess.TimeoutExpired)
        with patch.object(manual, "subprocess", api), contextlib.redirect_stdout(self.output):
            return manual.run(self.directory, self.binary, self.ask if ask is None else ask)

    def test_one_window_all_intermediate_checkpoints_and_no_native_claims(self):
        result = self.run_session()
        self.assertTrue(result["completed"], result)
        self.assertEqual(len(self.launches), 1)
        self.assertEqual(len(result["observations"]), 4)
        self.assertEqual([item["phase"] for item in result["checkpoints"]],
                         ["base", "base", "canceled", "active", "committed", "committed"])
        self.assertFalse(result["forced_cleanup"] or result["screenshots"] or result["key_injection"])
        for key in ("performance", "ime", "gpu", "presentation"):
            self.assertEqual(result[key], "unmeasured")
        self.assertEqual(result["window_notes"], "1100x700; readable")
        self.assertEqual(json.loads((self.directory / "result.json").read_text()), result)
        self.assertEqual(self.child.signals, [])

    def test_cancel_noop_stops_even_when_user_answers_yes(self):
        result = self.run_session(lambda prompt: self.prompts.append(prompt) or "yes")
        self.assertFalse(result["completed"])
        self.assertEqual(len(self.prompts), 2)
        self.assertNotIn("3. 活动提交", self.output.getvalue())
        self.assertTrue((self.directory / "STOP.txt").is_file())

    def test_no_eof_child_death_and_binary_or_config_change_stop_without_retry(self):
        for failure in ("no", "eof", "child", "binary", "config"):
            with self.subTest(failure=failure):
                self.directory = self.root / failure
                self.child = FakeChild()
                def ask(_):
                    if failure == "eof":
                        raise EOFError()
                    if failure == "child":
                        self.child.code = 7
                    elif failure == "binary":
                        self.binary.write_bytes(b"different binary")
                    elif failure == "config":
                        (self.directory / "config/semacs/init.js").write_bytes(b"different config")
                    return "no" if failure == "no" else "yes"
                result = self.run_session(ask)
                self.assertFalse(result["completed"])
                self.assertEqual(len(result["checkpoints"]), 1)
                self.assertEqual(len(result["observations"]), 1)

    def test_child_environment_has_local_config_runtime_and_no_daily_git_overrides(self):
        with patch.dict(os.environ, {"XDG_CONFIG_HOME": "/daily/config", "GIT_DIR": "/daily", "GIT_INDEX_FILE": "/daily/index"}):
            self.assertTrue(self.run_session()["completed"])
            self.assertEqual(os.environ["XDG_CONFIG_HOME"], "/daily/config")
        args, options = self.launches[0]
        self.assertEqual(args, [str(self.binary), str(self.directory / "repo" / manual.TARGET)])
        self.assertEqual(options["cwd"], self.directory / "repo")
        self.assertEqual(options["env"]["XDG_CONFIG_HOME"], str(self.directory / "config"))
        self.assertEqual(options["env"]["TMPDIR"], str(self.directory / "runtime"))
        self.assertEqual(options["env"]["GIT_CONFIG_GLOBAL"], os.devnull)
        self.assertNotIn("GIT_DIR", options["env"])
        self.assertNotIn("GIT_INDEX_FILE", options["env"])

    def test_abnormal_final_close_preserves_failure_and_skips_after_close_checkpoint(self):
        def wait(timeout):
            self.child.code = 7
            return 7
        self.child.wait = wait
        result = self.run_session()
        self.assertFalse(result["completed"])
        self.assertIn("exited abnormally: 7", result["errors"][0])
        self.assertEqual(result["returncode"], 7)
        self.assertEqual(result["checkpoints"][-1]["phase"], "committed")
        self.assertEqual(len(result["checkpoints"]), 5)

    def test_failed_batch_allows_normal_close_then_marks_forced_cleanup(self):
        self.child.timeout_once = True
        result = self.run_session(lambda _: "no")
        self.assertFalse(result["completed"])
        self.assertTrue(result["forced_cleanup"])
        self.assertEqual(self.child.waits, [manual.CLOSE_TIMEOUT, 5])
        self.assertEqual(self.child.signals, ["terminate"])
        self.assertEqual(len(self.launches), 1)

    def test_cli_consent_fresh_directory_and_fish_routing(self):
        result = subprocess.run([sys.executable, "-B", manual.__file__, str(self.directory), "--binary", str(self.binary)], capture_output=True)
        self.assertEqual(result.returncode, 2)
        self.assertIn(b"consent", result.stderr)
        self.assertFalse(self.directory.exists())
        self.directory.mkdir()
        with self.assertRaises(FileExistsError):
            self.run_session()
        linked = self.root / "linked"
        linked.symlink_to(self.root / "missing", target_is_directory=True)
        with self.assertRaises(FileExistsError):
            manual.run(linked, self.binary)
        source = (Path(manual.__file__).parent / "acceptance.fish").read_text()
        self.assertIn('magit-status magit-commit\n    cargo build --release --bin semacs --target-dir', source)
        block = source.split('else if test "$mode" = magit-commit\n')[1].split('else if test "$mode" = magit-status\n')[0]
        self.assertEqual(block.count("scripts/magit_commit_manual.py"), 1)
        self.assertIn('--binary "$root/target/release/semacs" --consent\n    or fail ', block)
        self.assertIn('git diff --no-ext-diff --binary HEAD > "$output/source.patch"', source)


if __name__ == "__main__":
    unittest.main()
