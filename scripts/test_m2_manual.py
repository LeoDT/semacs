"""Offline self-checks for fixture/report tools; these do not launch semacs."""

import json
from pathlib import Path
import tempfile
import unittest

import m2_manual


class ManualFixtureTests(unittest.TestCase):
    def setUp(self):
        temporary = Path(__file__).resolve().parents[1] / ".pi" / "tmp"
        temporary.mkdir(parents=True, exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(prefix="manual-tools-", dir=temporary)
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name) / "files"

    def completed_fixture(self, profile):
        m2_manual.prepare(self.directory, profile)
        _, expected, observations = m2_manual.PROFILES[profile]
        for name, data in expected.items():
            (self.directory / name).write_bytes(data)
        (self.directory / "observations.tsv").write_text(
            "".join(f"{name}\tyes\n" for name in observations)
        )

    def test_all_profiles_verify_their_own_bytes_and_observations(self):
        for profile in m2_manual.PROFILES:
            with self.subTest(profile=profile):
                self.directory = Path(self.temporary.name) / profile
                self.completed_fixture(profile)
                result = m2_manual.verify(self.directory, profile)
                self.assertTrue(result["completed"])
                self.assertEqual(result["performance"], "unmeasured")
                self.assertEqual(result["profile"], profile)

    def test_existing_fixture_is_never_overwritten(self):
        m2_manual.prepare(self.directory, "candidates")
        target = self.directory / "source.txt"
        target.write_bytes(b"preserve failure evidence")
        with self.assertRaises(FileExistsError):
            m2_manual.prepare(self.directory, "candidates")
        self.assertEqual(target.read_bytes(), b"preserve failure evidence")

    def test_missing_or_failed_observations_leave_a_failure_report(self):
        self.completed_fixture("candidates")
        (self.directory / "observations.tsv").write_text("candidate-command\tno\n")
        with self.assertRaisesRegex(RuntimeError, "manual observation not confirmed"):
            m2_manual.verify(self.directory, "candidates")
        result = json.loads((self.directory / "result.json").read_text())
        self.assertFalse(result["completed"])
        self.assertTrue(all(result["disk_bytes"].values()))
        self.assertEqual(result["observations"]["candidate-command"], "no")

    def test_disk_mismatch_is_not_overridden_by_yes_observations(self):
        self.completed_fixture("candidates")
        (self.directory / "alpha-existing.txt").write_bytes(b"accidentally overwritten")
        with self.assertRaisesRegex(RuntimeError, "disk-byte mismatch: alpha-existing.txt"):
            m2_manual.verify(self.directory, "candidates")
        result = json.loads((self.directory / "result.json").read_text())
        self.assertFalse(result["completed"])
        self.assertFalse(result["disk_bytes"]["alpha-existing.txt"])

    def test_missing_new_path_fails_verification(self):
        self.completed_fixture("candidates")
        (self.directory / "al").unlink()
        with self.assertRaisesRegex(RuntimeError, "disk-byte mismatch: al"):
            m2_manual.verify(self.directory, "candidates")

    def test_message_fixture_has_isolated_configuration_and_untruncated_markers(self):
        m2_manual.prepare(self.directory, "messages")
        config = self.directory / "config/semacs/init.js"
        self.assertEqual(config.read_bytes(), m2_manual.MESSAGES_CONFIG)
        self.assertIn(json.dumps(m2_manual.MESSAGE_LONG).encode(), config.read_bytes())
        self.assertIn(json.dumps(m2_manual.MESSAGE_MULTILINE).encode(), config.read_bytes())
        self.assertLess(len(m2_manual.MESSAGE_LONG), 900)
        self.assertLess(len(m2_manual.MESSAGE_MULTILINE), 900)
        self.assertGreater(m2_manual.MESSAGE_MULTILINE.count("\n"), 10)
        self.assertEqual((self.directory / "body.txt").read_bytes(), m2_manual.MESSAGES_SEEDS["body.txt"])

    def test_message_success_records_only_observed_scope(self):
        self.completed_fixture("messages")
        result = m2_manual.verify(self.directory, "messages")
        self.assertTrue(result["completed"])
        self.assertEqual(result["ime"], "unmeasured")
        self.assertEqual(result["gpu"], "unmeasured")
        self.assertFalse(result["screenshots"])

    def test_message_no_and_missing_observations_cannot_pass(self):
        self.completed_fixture("messages")
        (self.directory / "observations.tsv").write_text("message-long\tno\n")
        with self.assertRaisesRegex(RuntimeError, "manual observation not confirmed"):
            m2_manual.verify(self.directory, "messages")
        self.assertFalse(json.loads((self.directory / "result.json").read_text())["completed"])

    def test_altered_message_config_is_preserved_as_failure_evidence(self):
        self.completed_fixture("messages")
        config = self.directory / "config/semacs/init.js"
        config.write_bytes(b"preserve modified configuration")
        with self.assertRaisesRegex(RuntimeError, "disk-byte mismatch: config/semacs/init.js"):
            m2_manual.verify(self.directory, "messages")
        self.assertEqual(config.read_bytes(), b"preserve modified configuration")

    def test_message_final_body_must_match_independent_bytes(self):
        self.completed_fixture("messages")
        (self.directory / "body.txt").write_bytes(m2_manual.MESSAGES_SEEDS["body.txt"])
        with self.assertRaisesRegex(RuntimeError, "disk-byte mismatch: body.txt"):
            m2_manual.verify(self.directory, "messages")

    def test_fixture_symlink_is_not_accepted_as_verified_bytes(self):
        self.completed_fixture("messages")
        outside = Path(self.temporary.name) / "outside.txt"
        outside.write_bytes(m2_manual.MESSAGES_EXPECTED["body.txt"])
        target = self.directory / "body.txt"
        target.unlink()
        target.symlink_to(outside)
        with self.assertRaisesRegex(RuntimeError, "disk-byte mismatch: body.txt"):
            m2_manual.verify(self.directory, "messages")
        self.assertEqual(outside.read_bytes(), m2_manual.MESSAGES_EXPECTED["body.txt"])

    def test_wrong_profile_cannot_pass(self):
        self.completed_fixture("m2")
        with self.assertRaisesRegex(RuntimeError, "fixture profile does not match"):
            m2_manual.verify(self.directory, "candidates")


if __name__ == "__main__":
    unittest.main()
