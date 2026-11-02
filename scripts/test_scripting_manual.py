"""Offline checks for the scripting smoke driver; never launch a desktop app."""

import contextlib
import io
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import scripting_manual as smoke


class FakeProcess:
    def __init__(self, index):
        self.pid = 100 + index
        self.returncode = None
        self.terminated = False
        self.killed = False
        self.close_code = 0
        self.close_timeout = False
        self.ignore_terminate = False

    def poll(self):
        return self.returncode

    def wait(self, timeout):
        if (self.close_timeout and timeout == smoke.CLOSE_TIMEOUT) or (self.ignore_terminate and not self.killed):
            raise subprocess.TimeoutExpired("fixture", timeout)
        if self.returncode is None:
            self.returncode = self.close_code
        return self.returncode

    def terminate(self):
        self.terminated = True
        if not self.ignore_terminate:
            self.returncode = -15

    def kill(self):
        self.killed = True
        self.returncode = -9


class ScriptingSmokeTests(unittest.TestCase):
    def setUp(self):
        root = Path(__file__).resolve().parents[1] / ".pi/tmp"
        root.mkdir(parents=True, exist_ok=True)
        temporary = tempfile.TemporaryDirectory(prefix="scripting-driver-", dir=root)
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.directory = self.root / "new-smoke"
        self.binary = self.root / "fake-sem acs"
        self.binary.write_bytes(b"offline binary identity only; never executable")
        self.processes = []
        self.launches = []
        self.answers = 0

    def launch(self, argv, **kwargs):
        self.assertEqual(argv[0], str(self.binary))
        self.assertEqual(kwargs["env"]["XDG_CONFIG_HOME"], str(self.directory / "config"))
        self.assertEqual(kwargs["stdin"], subprocess.DEVNULL)
        self.assertEqual(kwargs["cwd"], self.directory)
        self.launches.append((argv, kwargs))
        process = FakeProcess(len(self.processes))
        self.processes.append(process)
        return process

    def successful_answer(self, prompt):
        name = smoke.OBSERVATIONS[self.answers]
        self.answers += 1
        if name == "command-routes":
            (self.directory / "files/commands.txt").write_bytes(b"ALPHA beta\n")
        elif name == "reload-success":
            self.assertEqual(
                (self.directory / "config/semacs/init.js").read_bytes(),
                smoke.CONFIGURATIONS["reloaded"],
            )
            (self.directory / "files/commands.txt").write_bytes(b"alpha beta\n")
        elif name == "reload-failure":
            self.assertEqual(
                (self.directory / "config/semacs/init.js").read_bytes(),
                smoke.CONFIGURATIONS["invalid"],
            )
            (self.directory / "files/commands.txt").write_bytes(b"xalpha beta\n")
        elif name == "fallback-editing":
            (self.directory / "files/fallback.txt").write_bytes(b"Zfallback\n")
        return "yes"

    def run_driver(self, ask=None):
        with patch.object(smoke.subprocess, "Popen", side_effect=self.launch), contextlib.redirect_stdout(io.StringIO()):
            return smoke.run(self.directory, self.binary, self.successful_answer if ask is None else ask)

    def test_full_flow_uses_two_owned_children_and_records_independent_checks(self):
        with patch.dict(os.environ, {"XDG_CONFIG_HOME": "/must-not-read-real-config"}):
            result = self.run_driver()
            self.assertEqual(os.environ["XDG_CONFIG_HOME"], "/must-not-read-real-config")
        self.assertTrue(result["completed"])
        self.assertEqual(result["errors"], [])
        self.assertEqual(set(result["observations"]), set(smoke.OBSERVATIONS))
        self.assertEqual(len(result["processes"]), 2)
        self.assertTrue(all(record["returncode"] == 0 and not record["cleanup"] for record in result["processes"]))
        self.assertEqual(result["performance"], "unmeasured")
        self.assertIn("no per-frame", result["startup_visual"])
        self.assertEqual(result["binary_sha256"], smoke.digest(self.binary.read_bytes()))
        self.assertTrue(all(
            check["matched"] for stage in result["disk_bytes"].values() for check in stage.values()
        ))
        for phase, data in smoke.CONFIGURATIONS.items():
            self.assertEqual((self.directory / "configurations" / f"{phase}.js").read_bytes(), data)
        self.assertEqual(
            (self.directory / "config/semacs/init.js").read_bytes(), smoke.CONFIGURATIONS["invalid"]
        )
        self.assertFalse((self.directory / "STOP.txt").exists())
        self.assertTrue(all(kwargs["stdout"].closed and kwargs["stderr"].closed for _, kwargs in self.launches))

    def test_existing_directory_is_preserved_before_any_launch(self):
        self.directory.mkdir()
        sentinel = self.directory / "result.json"
        sentinel.write_text("keep prior evidence")
        with patch.object(smoke.subprocess, "Popen") as launch:
            with self.assertRaises(FileExistsError):
                smoke.run(self.directory, self.binary)
        launch.assert_not_called()
        self.assertEqual(sentinel.read_text(), "keep prior evidence")

    def test_no_observation_stops_immediately_and_preserves_configuration(self):
        result = self.run_driver(lambda _: "no")
        self.assertFalse(result["completed"])
        self.assertEqual(result["observations"], {"startup-config": "no"})
        self.assertEqual(len(self.processes), 1)
        self.assertTrue(self.processes[0].terminated)
        self.assertFalse(self.processes[0].killed)
        self.assertEqual(
            (self.directory / "config/semacs/init.js").read_bytes(), smoke.CONFIGURATIONS["initial"]
        )
        self.assertTrue((self.directory / "STOP.txt").is_file())
        self.assertFalse(json.loads((self.directory / "result.json").read_text())["completed"])

    def test_eof_is_unobserved_not_a_pass(self):
        def eof(_):
            raise EOFError
        result = self.run_driver(eof)
        self.assertFalse(result["completed"])
        self.assertEqual(result["observations"], {"startup-config": "unobserved"})
        self.assertTrue(self.processes[0].terminated)

    def test_yes_does_not_override_wrong_disk_bytes(self):
        result = self.run_driver(lambda _: "yes")
        self.assertFalse(result["completed"])
        self.assertIn("disk-byte mismatch", result["errors"][0])
        self.assertFalse(result["disk_bytes"]["commands"]["commands.txt"]["matched"])
        self.assertEqual(len(self.processes), 1)

    def test_unexpected_config_change_is_not_repaired_or_overwritten(self):
        def answer(prompt):
            reply = self.successful_answer(prompt)
            if self.answers == 2:
                (self.directory / "config/semacs/init.js").write_bytes(b"preserve failure evidence")
            return reply
        result = self.run_driver(answer)
        self.assertFalse(result["completed"])
        self.assertIn("configuration changed unexpectedly", result["errors"][0])
        self.assertEqual(
            (self.directory / "config/semacs/init.js").read_bytes(), b"preserve failure evidence"
        )
        self.assertEqual(len(self.processes), 1)

    def test_fixture_symlink_is_rejected_without_reading_or_changing_its_target(self):
        outside = self.root / "outside.txt"
        outside.write_bytes(b"outside")
        def answer(prompt):
            reply = self.successful_answer(prompt)
            if self.answers == 2:
                file = self.directory / "files/commands.txt"
                file.unlink()
                file.symlink_to(outside)
            return reply
        result = self.run_driver(answer)
        self.assertFalse(result["completed"])
        self.assertIsNone(result["disk_bytes"]["commands"]["commands.txt"]["actual_hex"])
        self.assertEqual(outside.read_bytes(), b"outside")

    def test_early_clean_exit_cannot_pass_an_observation(self):
        def answer(_):
            self.processes[0].returncode = 0
            return "yes"
        result = self.run_driver(answer)
        self.assertFalse(result["completed"])
        self.assertIn("exited before", result["errors"][0])
        self.assertEqual(len(self.processes), 1)
        self.assertFalse(self.processes[0].terminated)

    def test_nonzero_exit_prevents_second_window(self):
        def answer(prompt):
            reply = self.successful_answer(prompt)
            if self.answers == 4:
                self.processes[0].close_code = 7
            return reply
        result = self.run_driver(answer)
        self.assertFalse(result["completed"])
        self.assertIn("exited abnormally: 7", result["errors"][0])
        self.assertEqual(len(self.processes), 1)
        self.assertEqual(result["processes"][0]["returncode"], 7)

    def test_close_timeout_stops_without_launching_or_retrying(self):
        def answer(prompt):
            reply = self.successful_answer(prompt)
            if self.answers == 4:
                self.processes[0].close_timeout = True
            return reply
        result = self.run_driver(answer)
        self.assertFalse(result["completed"])
        self.assertEqual(len(self.processes), 1)
        self.assertTrue(self.processes[0].terminated)

    def test_launch_failure_keeps_report_and_closes_log_handles(self):
        with patch.object(smoke.subprocess, "Popen", side_effect=OSError("launch failed")), contextlib.redirect_stdout(io.StringIO()):
            result = smoke.run(self.directory, self.binary)
        self.assertFalse(result["completed"])
        self.assertIn("launch failed", result["errors"][0])
        self.assertEqual(result["processes"], [])
        self.assertTrue((self.directory / "result.json").is_file())

    def test_missing_binary_fails_before_any_launch(self):
        with patch.object(smoke.subprocess, "Popen") as launch, contextlib.redirect_stdout(io.StringIO()):
            result = smoke.run(self.directory, self.root / "missing")
        self.assertFalse(result["completed"])
        launch.assert_not_called()
        self.assertTrue((self.directory / "STOP.txt").is_file())

    def test_cleanup_kills_only_the_owned_child_if_termination_times_out(self):
        def answer(_):
            self.processes[0].ignore_terminate = True
            return "no"
        result = self.run_driver(answer)
        self.assertFalse(result["completed"])
        self.assertEqual(len(self.processes), 1)
        self.assertTrue(self.processes[0].terminated)
        self.assertTrue(self.processes[0].killed)
        self.assertEqual(result["processes"][0]["returncode"], -9)

    def test_keyboard_interrupt_stops_without_advancing_configuration(self):
        def interrupt(_):
            raise KeyboardInterrupt
        result = self.run_driver(interrupt)
        self.assertFalse(result["completed"])
        self.assertEqual(result["errors"], ["KeyboardInterrupt"])
        self.assertEqual(len(self.processes), 1)
        self.assertTrue(self.processes[0].terminated)

    def test_symlinked_configuration_is_not_followed_or_replaced(self):
        outside = self.root / "outside.js"
        outside.write_bytes(b"preserve outside configuration")
        def answer(prompt):
            reply = self.successful_answer(prompt)
            if self.answers == 2:
                config = self.directory / "config/semacs/init.js"
                config.unlink()
                config.symlink_to(outside)
            return reply
        result = self.run_driver(answer)
        self.assertFalse(result["completed"])
        self.assertTrue((self.directory / "config/semacs/init.js").is_symlink())
        self.assertEqual(outside.read_bytes(), b"preserve outside configuration")

    def test_cli_requires_explicit_consent_before_creating_output(self):
        argv = ["scripting_manual.py", str(self.directory), "--binary", str(self.binary)]
        with patch.object(smoke.sys, "argv", argv), contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit) as error:
                smoke.main()
        self.assertEqual(error.exception.code, 2)
        self.assertFalse(self.directory.exists())


if __name__ == "__main__":
    unittest.main()
