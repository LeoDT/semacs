"""Offline fixtures / mock-child checks; never launches the editor or a desktop."""

import contextlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import magit_status_manual as manual


class FixtureTests(unittest.TestCase):
    def setUp(self):
        root = Path(__file__).resolve().parents[1] / ".pi/tmp"
        root.mkdir(parents=True, exist_ok=True)
        temporary = tempfile.TemporaryDirectory(prefix="f2-manual-tools-", dir=root)
        self.addCleanup(temporary.cleanup)
        self.directory = Path(temporary.name) / "status"
        self.digest = manual.prepare(self.directory)
        self.repo = self.directory / "repo"

    def test_base_stage_base_complete_index_oracles_and_worktree_immutability(self):
        before = (self.repo / ".git/index").read_bytes()
        self.assertTrue(manual.verify(self.directory, "base", self.digest)["complete_index"])
        self.assertEqual((self.repo / ".git/index").read_bytes(), before)
        manual.git(self.repo, "add", "--", manual.TARGET)
        self.assertTrue(manual.verify(self.directory, "stage", self.digest)["complete_index"])
        with self.assertRaisesRegex(RuntimeError, "complete index mismatch"):
            manual.verify(self.directory, "base", self.digest)
        manual.git(self.repo, "reset", "--quiet", "HEAD", "--", manual.TARGET)
        manual.verify(self.directory, "base", self.digest)
        for name, expected in manual.WORKTREE_FILES.items():
            self.assertEqual((self.repo / name).read_bytes(), expected)

    def test_final_base_cannot_substitute_for_stage_checkpoint(self):
        manual.git(self.repo, "add", "--", manual.TARGET)
        manual.git(self.repo, "reset", "--quiet", "HEAD", "--", manual.TARGET)
        with self.assertRaisesRegex(RuntimeError, "stage checkpoint"):
            manual.verify(self.directory, "stage", self.digest)

    def test_unrelated_index_entry_change_is_rejected_without_repair(self):
        path = self.repo / "10-staged.txt"
        path.write_bytes(b"wrong intermediate\n")
        manual.git(self.repo, "add", "--", path.name)
        path.write_bytes(manual.WORKTREE_FILES[path.name])
        index = (self.repo / ".git/index").read_bytes()
        with self.assertRaisesRegex(RuntimeError, "complete index mismatch"):
            manual.verify(self.directory, "base", self.digest)
        self.assertEqual((self.repo / ".git/index").read_bytes(), index)

    def test_worktree_config_manifest_and_index_lock_changes_are_rejected(self):
        for name in ("repo/20-control.txt", "config/semacs/init.js", "repo/.git/config", "fixture.json"):
            with self.subTest(name=name):
                path = self.directory / name
                old = path.read_bytes()
                path.write_bytes(old + b"changed")
                with self.assertRaises(RuntimeError):
                    manual.verify(self.directory, "base", self.digest)
                self.assertEqual(path.read_bytes(), old + b"changed")
                path.write_bytes(old)
        lock = self.repo / ".git/index.lock"
        lock.write_bytes(b"do not delete")
        with self.assertRaisesRegex(RuntimeError, "index.lock"):
            manual.verify(self.directory, "base", self.digest)
        self.assertEqual(lock.read_bytes(), b"do not delete")

    def test_changed_ref_even_with_same_tree_is_rejected(self):
        tree = manual.git(self.repo, "rev-parse", "HEAD^{tree}").decode().strip()
        commit = manual.git(self.repo, "commit-tree", tree, "-m", "different identity").decode().strip()
        manual.git(self.repo, "update-ref", "refs/heads/unexpected", commit)
        with self.assertRaisesRegex(RuntimeError, "HEAD / refs"):
            manual.verify(self.directory, "base", self.digest)

    def test_missing_metadata_never_discovers_parent_and_links_are_rejected(self):
        gitdir = self.repo / ".git"
        gitdir.rename(self.repo / "hidden-git")
        with self.assertRaisesRegex(RuntimeError, "missing or linked"):
            manual.verify(self.directory, "base", self.digest)
        (self.repo / "hidden-git").rename(gitdir)
        config = self.directory / "config"
        saved = self.directory / "saved-config"
        config.rename(saved)
        config.symlink_to(saved, target_is_directory=True)
        with self.assertRaisesRegex(RuntimeError, "missing or linked"):
            manual.verify(self.directory, "base", self.digest)

    def test_unknown_checkpoint_and_existing_directory_are_refused(self):
        with self.assertRaises(ValueError):
            manual.verify(self.directory, "unknown", self.digest)
        with self.assertRaises(FileExistsError):
            manual.prepare(self.directory)

    def test_default_user_ignore_and_attributes_are_excluded(self):
        for variant in ("explicit-xdg", "home-fallback"):
            with self.subTest(variant=variant):
                parent = self.directory.parent / variant
                home = parent / "home"
                xdg = parent / "xdg" if variant == "explicit-xdg" else home / ".config"
                (xdg / "git").mkdir(parents=True)
                (xdg / "git/ignore").write_text("*.txt\n")
                (xdg / "git/attributes").write_text("*.txt working-tree-encoding=UTF-16\n")
                with patch.dict(os.environ, {"HOME": str(home)}):
                    if variant == "explicit-xdg":
                        os.environ["XDG_CONFIG_HOME"] = str(xdg)
                    else:
                        os.environ.pop("XDG_CONFIG_HOME", None)
                    directory = parent / "status"
                    frozen = manual.prepare(directory)
                    manual.verify(directory, "base", frozen)
                    manual.git(directory / "repo", "add", "--", manual.TARGET)
                    manual.verify(directory, "stage", frozen)
                    for key in ("core.excludesFile", "core.attributesFile"):
                        self.assertEqual(manual.git(directory / "repo", "config", "--get", key).strip(),
                                         os.fsencode(os.devnull))
                self.assertEqual((xdg / "git/ignore").read_text(), "*.txt\n")
                self.assertEqual((xdg / "git/attributes").read_text(), "*.txt working-tree-encoding=UTF-16\n")

    def test_fixture_git_and_verification_ignore_inherited_overrides(self):
        with patch.dict(os.environ, {"GIT_DIR": "/daily", "GIT_INDEX_FILE": "/daily/index",
                                    "GIT_CONFIG_COUNT": "1", "GIT_CONFIG_KEY_0": "core.worktree",
                                    "GIT_CONFIG_VALUE_0": "/daily/worktree"}):
            manual.verify(self.directory, "base", self.digest)


class FakeChild:
    pid = 12345

    def __init__(self):
        self.code = None
        self.waits = []
        self.signals = []
        self.timeout_once = False

    def poll(self):
        return self.code

    def wait(self, timeout):
        self.waits.append(timeout)
        if self.timeout_once:
            self.timeout_once = False
            raise subprocess.TimeoutExpired("fake-child", timeout)
        self.code = 0
        return self.code

    def terminate(self):
        self.signals.append("terminate")

    def kill(self):
        self.signals.append("kill")


class SessionTests(unittest.TestCase):
    def setUp(self):
        root = Path(__file__).resolve().parents[1] / ".pi/tmp"
        root.mkdir(parents=True, exist_ok=True)
        temporary = tempfile.TemporaryDirectory(prefix="f2-manual-session-", dir=root)
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.directory = self.root / "status"
        self.binary = self.root / "fake-binary"
        self.binary.write_bytes(b"never executed")
        self.child = FakeChild()
        self.launches = []
        self.prompts = []
        self.stdout = io.StringIO()

    def popen(self, args, **kwargs):
        self.launches.append((args, kwargs))
        return self.child

    def ask(self, prompt):
        self.prompts.append(prompt)
        repo = self.directory / "repo"
        if prompt.startswith("status-stage-stale"):
            manual.git(repo, "add", "--", manual.TARGET)
        elif prompt.startswith("status-unstage-refresh"):
            manual.git(repo, "reset", "--quiet", "HEAD", "--", manual.TARGET)
        elif prompt.startswith("请记录"):
            return "1100x700; readable"
        return "yes"

    def run_session(self, ask=None):
        process_api = SimpleNamespace(Popen=self.popen, DEVNULL=subprocess.DEVNULL,
                                      TimeoutExpired=subprocess.TimeoutExpired)
        with patch.object(manual, "subprocess", process_api), contextlib.redirect_stdout(self.stdout):
            return manual.run(self.directory, self.binary, self.ask if ask is None else ask)

    def test_one_child_with_all_intermediate_checks_and_no_native_claims(self):
        result = self.run_session()
        self.assertTrue(result["completed"], result)
        self.assertEqual(len(self.launches), 1)
        self.assertEqual(len(result["observations"]), 3)
        self.assertEqual([check["phase"] for check in result["checkpoints"]], ["base", "base", "stage", "base", "base"])
        self.assertEqual(result["performance"], "unmeasured")
        self.assertEqual(result["ime"], "unmeasured")
        self.assertFalse(result["screenshots"] or result["key_injection"] or result["forced_cleanup"])
        self.assertEqual(result["window_notes"], "1100x700; readable")
        self.assertEqual(self.child.signals, [])
        self.assertEqual(json.loads((self.directory / "result.json").read_text()), result)

    def test_stage_noop_stops_before_unstage_even_when_answered_yes(self):
        result = self.run_session(lambda prompt: self.prompts.append(prompt) or "yes")
        self.assertFalse(result["completed"])
        self.assertIn("complete index mismatch at stage", result["errors"][0])
        self.assertEqual(len(self.prompts), 2)
        self.assertNotIn("3. 默认 u", self.stdout.getvalue())
        self.assertTrue((self.directory / "STOP.txt").is_file())
        self.assertEqual(self.child.signals, [])

    def test_no_or_eof_stops_before_any_following_group(self):
        def eof(_):
            raise EOFError()
        for ask, label in ((lambda _: "no", "no"), (eof, "unobserved")):
            with self.subTest(label=label):
                self.directory = self.root / label
                self.child = FakeChild()
                result = self.run_session(ask)
                self.assertFalse(result["completed"])
                self.assertEqual(result["observations"], {"status-display-focus": label})
                self.assertEqual(len(result["checkpoints"]), 1)

    def test_child_failure_binary_change_and_config_change_stop(self):
        for failure in ("child", "binary", "config"):
            with self.subTest(failure=failure):
                self.directory = self.root / failure
                self.child = FakeChild()
                def ask(_):
                    if failure == "child":
                        self.child.code = 7
                    elif failure == "binary":
                        self.binary.write_bytes(b"different binary")
                    else:
                        (self.directory / "config/semacs/init.js").write_bytes(b"different config")
                    return "yes"
                result = self.run_session(ask)
                self.assertFalse(result["completed"])
                self.assertEqual(len(result["observations"]), 1)
                self.assertEqual(len(result["checkpoints"]), 1)

    def test_isolated_child_environment_and_explicit_fixture_path(self):
        daily = {"XDG_CONFIG_HOME": "/daily/config", "GIT_DIR": "/daily/repo/.git", "GIT_INDEX_FILE": "/daily/index"}
        with patch.dict(os.environ, daily):
            result = self.run_session()
            self.assertTrue(result["completed"])
            self.assertEqual(os.environ["XDG_CONFIG_HOME"], daily["XDG_CONFIG_HOME"])
        args, options = self.launches[0]
        self.assertEqual(args, [str(self.binary), str(self.directory / "repo" / manual.TARGET)])
        self.assertEqual(options["cwd"], self.directory / "repo")
        environment = options["env"]
        self.assertEqual(environment["XDG_CONFIG_HOME"], str(self.directory / "config"))
        self.assertEqual(environment["GIT_CONFIG_GLOBAL"], os.devnull)
        self.assertNotIn("GIT_DIR", environment)
        self.assertNotIn("GIT_INDEX_FILE", environment)
        self.assertEqual(options["stdin"], subprocess.DEVNULL)

    def test_cleanup_allows_normal_close_before_forcing_and_never_retries(self):
        self.child.timeout_once = True
        result = self.run_session(lambda _: "no")
        self.assertFalse(result["completed"])
        self.assertTrue(result["forced_cleanup"])
        self.assertEqual(self.child.waits, [manual.CLOSE_TIMEOUT, 5])
        self.assertEqual(self.child.signals, ["terminate"])
        self.assertEqual(len(self.launches), 1)

    def test_cli_requires_consent_and_existing_or_linked_output_is_refused(self):
        result = subprocess.run([sys.executable, "-B", manual.__file__, str(self.directory),
                                 "--binary", str(self.binary)], capture_output=True)
        self.assertEqual(result.returncode, 2)
        self.assertIn(b"consent", result.stderr)
        self.assertFalse(self.directory.exists())
        self.directory.mkdir()
        with self.assertRaises(FileExistsError):
            self.run_session()
        linked = self.root / "linked"
        linked.symlink_to(self.root / "nonexistent", target_is_directory=True)
        with self.assertRaises(FileExistsError):
            manual.run(linked, self.binary)

    def test_fish_entry_builds_only_the_application_and_propagates_failure(self):
        source = (Path(manual.__file__).parent / "acceptance.fish").read_text()
        self.assertIn('styles magit magit-status magit-commit\n    cargo build --release --bin semacs --target-dir', source)
        block = source.split('else if test "$mode" = magit-status\n')[1].split('else if contains -- "$mode" themes')[0]
        self.assertEqual(block.count("scripts/magit_status_manual.py"), 1)
        self.assertIn('--binary "$root/target/release/semacs" --consent\n    or fail ', block)
        self.assertIn('git diff --no-ext-diff --binary HEAD > "$output/source.patch"', source)


if __name__ == "__main__":
    unittest.main()
