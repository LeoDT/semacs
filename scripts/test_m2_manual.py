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

    def test_theme_fixture_is_isolated_and_only_final_body_edit_is_expected(self):
        m2_manual.prepare(self.directory, "themes")
        self.assertEqual(
            (self.directory / "config/semacs/init.js").read_bytes(), m2_manual.THEMES_CONFIG
        )
        self.assertIn(b'semacs.loadTheme("e0-night")', m2_manual.THEMES_CONFIG)
        self.assertIn("中文 🙂 e\u0301".encode(), (self.directory / "body.txt").read_bytes())
        self.assertEqual(
            m2_manual.THEMES_EXPECTED["body.txt"], b"X" + m2_manual.THEMES_SEEDS["body.txt"]
        )

    def test_theme_missing_observation_or_changed_config_fails_and_keeps_evidence(self):
        self.completed_fixture("themes")
        (self.directory / "observations.tsv").write_text("theme-switch\tno\n")
        config = self.directory / "config/semacs/init.js"
        config.write_bytes(b"changed configuration")
        with self.assertRaisesRegex(RuntimeError, "disk-byte mismatch"):
            m2_manual.verify(self.directory, "themes")
        result = json.loads((self.directory / "result.json").read_text())
        self.assertFalse(result["completed"])
        self.assertEqual(result["observations"]["theme-switch"], "no")
        self.assertEqual(result["ime"], "unmeasured")
        self.assertEqual(result["gpu"], "unmeasured")
        self.assertFalse(result["screenshots"])
        self.assertEqual(config.read_bytes(), b"changed configuration")

    def test_syntax_fixture_checks_language_roundtrip_and_configuration(self):
        self.completed_fixture("syntax")
        result = m2_manual.verify(self.directory, "syntax")
        self.assertTrue(result["completed"])
        self.assertEqual((self.directory / "body.rs").read_bytes(), m2_manual.SYNTAX_BODY)
        self.assertEqual((self.directory / "plain.txt").read_bytes(), m2_manual.SYNTAX_BODY)
        self.assertEqual((self.directory / "language.txt").read_bytes(), m2_manual.SYNTAX_BODY)
        self.assertEqual((self.directory / "restored.rs").read_bytes(), b"// done\n" + m2_manual.SYNTAX_BODY)
        self.assertEqual(result["ime"], "unmeasured")
        self.assertEqual(result["gpu"], "unmeasured")
        self.assertFalse(result["screenshots"])
        (self.directory / "restored.rs").write_bytes(m2_manual.SYNTAX_BODY)
        with self.assertRaisesRegex(RuntimeError, "disk-byte mismatch: restored.rs"):
            m2_manual.verify(self.directory, "syntax")

    def test_syntax_no_observation_or_altered_config_cannot_pass(self):
        self.completed_fixture("syntax")
        (self.directory / "observations.tsv").write_text("syntax-start-split\tno\n")
        config = self.directory / "config/semacs/init.js"
        config.write_bytes(b"preserve changed syntax config")
        with self.assertRaises(RuntimeError):
            m2_manual.verify(self.directory, "syntax")
        result = json.loads((self.directory / "result.json").read_text())
        self.assertFalse(result["completed"])
        self.assertFalse(result["disk_bytes"]["config/semacs/init.js"])
        self.assertEqual(config.read_bytes(), b"preserve changed syntax config")

    def test_wrong_profile_cannot_pass(self):
        self.completed_fixture("m2")
        with self.assertRaisesRegex(RuntimeError, "fixture profile does not match"):
            m2_manual.verify(self.directory, "candidates")


if __name__ == "__main__":
    unittest.main()
