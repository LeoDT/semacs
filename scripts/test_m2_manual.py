"""Offline fixture/report checks; no real semacs or desktop access."""

import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
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

    def test_default_profiles_seed_and_verify_empty_configuration(self):
        for profile in ("m2", "candidates"):
            with self.subTest(profile=profile):
                self.directory = Path(self.temporary.name) / profile
                self.completed_fixture(profile)
                config = self.directory / "config/semacs/init.js"
                self.assertEqual(config.read_bytes(), b"")
                manifest = json.loads((self.directory / "fixture.json").read_text())
                self.assertIn("config/semacs/init.js", manifest["seed_sha256"])
                result = m2_manual.verify(self.directory, profile)
                self.assertTrue(result["disk_bytes"]["config/semacs/init.js"])
                self.assertNotEqual(result.get("ime"), "unmeasured")

    def test_font_profiles_inherit_the_bundled_default_family(self):
        for profile in ("messages", "themes", "syntax", "languages", "flicker", "styles"):
            with self.subTest(profile=profile):
                config = m2_manual.PROFILES[profile][0]["config/semacs/init.js"]
                if config:
                    self.assertIn(b"semacs.setFont({size:", config)
                self.assertNotIn(b"family:", config)

    def test_default_profile_missing_modified_or_linked_config_is_preserved_and_rejected(self):
        outside = Path(self.temporary.name) / "daily-init.js"
        outside.write_bytes(b"daily configuration must not change")
        for profile in ("m2", "candidates"):
            for change in ("missing", "modified", "symlink"):
                with self.subTest(profile=profile, change=change):
                    self.directory = Path(self.temporary.name) / f"{profile}-{change}"
                    self.completed_fixture(profile)
                    config = self.directory / "config/semacs/init.js"
                    config.unlink()
                    if change == "modified":
                        config.write_bytes(b"changed fixture configuration")
                    elif change == "symlink":
                        config.symlink_to(outside)
                    with self.assertRaisesRegex(RuntimeError, "disk-byte mismatch: config/semacs/init.js"):
                        m2_manual.verify(self.directory, profile)
                    self.assertEqual(outside.read_bytes(), b"daily configuration must not change")
                    if change == "modified":
                        self.assertEqual(config.read_bytes(), b"changed fixture configuration")

    @unittest.skipUnless(shutil.which("fish"), "fish is required to exercise child launch commands")
    def test_manual_and_candidate_launch_commands_isolate_only_the_child_environment(self):
        root = Path(self.temporary.name) / "mock-root"
        binary = root / "target/release/semacs"
        binary.parent.mkdir(parents=True)
        binary.write_text("""#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
config = Path(os.environ["XDG_CONFIG_HOME"]) / "semacs/init.js"
Path(os.environ["CAPTURE"]).write_text(json.dumps({
    "config": str(config), "bytes": config.read_text(), "file": sys.argv[1],
}))
""")
        binary.chmod(0o755)
        daily = Path(self.temporary.name) / "daily-config"
        (daily / "semacs").mkdir(parents=True)
        (daily / "semacs/init.js").write_bytes(b"daily configuration must not be loaded")
        source = (Path(__file__).parent / "acceptance.fish").read_text().splitlines()
        original = os.environ.get("XDG_CONFIG_HOME")
        for profile, filename in (("m2", "alpha.txt"), ("m2", "close.txt"), ("candidates", "source.txt"), ("styles", "body.rs")):
            with self.subTest(profile=profile, filename=filename):
                output = Path(self.temporary.name) / filename
                m2_manual.prepare(output / "files", profile)
                template = "$body" if profile == "styles" else filename
                commands = [line for line in source if f'"$output/files/{template}" >' in line]
                self.assertEqual(len(commands), 1)
                capture = output / "capture.json"
                environment = {
                    **os.environ, "XDG_CONFIG_HOME": str(daily),
                    "CAPTURE": str(capture), "PYTHONDONTWRITEBYTECODE": "1",
                }
                script = (
                    f"set -l root {shlex.quote(str(root))}\n"
                    f"set -l output {shlex.quote(str(output))}\n"
                    f"set -l body {shlex.quote(filename)}\n"
                    f"{commands[0]}\nor exit $status\n"
                    'printf "%s" "$XDG_CONFIG_HOME" > "$output/parent-env.txt"\n'
                )
                subprocess.run(["fish", "--no-config", "-c", script], env=environment,
                               check=True, capture_output=True, timeout=10)
                result = json.loads(capture.read_text())
                self.assertEqual(result["config"], str(output / "files/config/semacs/init.js"))
                expected_config = m2_manual.PROFILES[profile][0]["config/semacs/init.js"].decode()
                self.assertEqual(result["bytes"], expected_config)
                self.assertEqual(result["file"], str(output / "files" / filename))
                self.assertEqual((output / "parent-env.txt").read_text(), str(daily))
                self.assertEqual((daily / "semacs/init.js").read_bytes(), b"daily configuration must not be loaded")
                self.assertEqual(os.environ.get("XDG_CONFIG_HOME"), original)

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


    def test_languages_fixture_tracks_only_the_two_committed_ime_characters(self):
        self.completed_fixture("languages")
        result = m2_manual.verify(self.directory, "languages")
        self.assertTrue(result["completed"])
        self.assertIn("user-confirmed", result["ime"])
        self.assertEqual(result["ime_latency"], "unmeasured")
        self.assertEqual(result["gpu"], "unmeasured")
        self.assertFalse(result["screenshots"])
        self.assertEqual((self.directory / "body.md").read_bytes().count("你".encode()), 1)
        self.assertEqual((self.directory / "body.html").read_bytes().count("你".encode()), 1)
        self.assertEqual((self.directory / "config/semacs/init.js").read_bytes(), m2_manual.LANGUAGES_CONFIG)

    def test_languages_unobserved_ime_is_not_reported_as_confirmed(self):
        self.completed_fixture("languages")
        path = self.directory / "observations.tsv"
        path.write_text(path.read_text().replace("languages-html-ime\tyes", "languages-html-ime\tno"))
        with self.assertRaisesRegex(RuntimeError, "manual observation not confirmed"):
            m2_manual.verify(self.directory, "languages")
        result = json.loads((self.directory / "result.json").read_text())
        self.assertFalse(result["completed"])
        self.assertEqual(result["ime"], "unconfirmed")

    def test_languages_incorrect_commit_or_config_is_preserved_and_rejected(self):
        for name, data in (
            ("body.md", m2_manual.LANGUAGES_MARKDOWN),
            ("body.html", m2_manual.LANGUAGES_HTML.replace(b"// HTML:", "// HTML:你你ni".encode())),
            ("config/semacs/init.js", b"changed config"),
        ):
            with self.subTest(name=name):
                self.directory = Path(self.temporary.name) / name.replace("/", "-")
                self.completed_fixture("languages")
                path = self.directory / name
                path.write_bytes(data)
                with self.assertRaisesRegex(RuntimeError, "disk-byte mismatch"):
                    m2_manual.verify(self.directory, "languages")
                self.assertEqual(path.read_bytes(), data)

    def test_languages_instructions_address_embedded_comments_and_both_files(self):
        lines = m2_manual.LANGUAGES_MARKDOWN.decode().splitlines()
        self.assertEqual(lines[:4], ["\x60\x60\x60html", "<script>", "// MD:", "const n = call(42);"])
        self.assertEqual(m2_manual.LANGUAGES_HTML.decode().splitlines()[1], "// HTML:")
        self.assertEqual(m2_manual.LANGUAGES_EXPECTED["body.md"],
                         m2_manual.LANGUAGES_MARKDOWN.replace(b"// MD:", "// MD:你".encode()))
        self.assertIn("body.html", m2_manual.LANGUAGES_INSTRUCTIONS)
        self.assertIn("C-? redo", m2_manual.LANGUAGES_INSTRUCTIONS)
        self.assertEqual(len(m2_manual.LANGUAGES_OBSERVATIONS), 5)

    def test_flicker_uses_large_isolated_fixtures_without_ime_claims(self):
        self.completed_fixture("flicker")
        result = m2_manual.verify(self.directory, "flicker")
        for name in ("body.md", "body.html"):
            data = (self.directory / name).read_bytes()
            self.assertTrue(256 * 1024 <= len(data) < 257 * 1024)
            self.assertIn(b"// FLICKER:\nconst n = call(42);", data)
        self.assertEqual((self.directory / "body.md").read_text().splitlines()[2], "// FLICKER:")
        self.assertEqual((self.directory / "body.html").read_text().splitlines()[1], "// FLICKER:")
        self.assertEqual((self.directory / "config/semacs/init.js").read_bytes(), b"")
        self.assertEqual(result["ime"], "unmeasured")
        self.assertEqual(result["ime_latency"], "unmeasured")
        self.assertEqual(result["gpu"], "unmeasured")
        self.assertFalse(result["screenshots"])
        self.assertEqual(len(m2_manual.FLICKER_OBSERVATIONS), 3)

    def test_flicker_unobserved_or_unrestored_bytes_stop_without_repair(self):
        for name, data in (
            ("body.md", b"extra x"),
            ("body.html", b"extra x"),
            ("config/semacs/init.js", b"changed"),
            ("observations.tsv", b"flicker-markdown\tyes\n"),
        ):
            with self.subTest(name=name):
                self.directory = Path(self.temporary.name) / name.replace("/", "-")
                self.completed_fixture("flicker")
                path = self.directory / name
                path.write_bytes(data)
                with self.assertRaises(RuntimeError):
                    m2_manual.verify(self.directory, "flicker")
                self.assertEqual(path.read_bytes(), data)
                self.assertFalse(json.loads((self.directory / "result.json").read_text())["completed"])

    def test_styles_verify_only_the_two_ime_commits_and_record_unmeasured_limits(self):
        self.completed_fixture("styles")
        result = m2_manual.verify(self.directory, "styles")
        self.assertTrue(result["completed"])
        self.assertEqual(result["ime"], "user-confirmed styled Rust/Markdown body preedit/cancel/commit")
        self.assertEqual(result["ime_latency"], "unmeasured")
        self.assertEqual(result["performance"], "unmeasured")
        self.assertEqual(result["gpu"], "unmeasured")
        self.assertFalse(result["screenshots"])
        self.assertEqual((self.directory / "body.rs").read_bytes(),
                         m2_manual.STYLES_RUST.replace(b"// E3-RUST:", "// E3-RUST:你".encode(), 1))
        self.assertEqual((self.directory / "body.md").read_bytes(),
                         m2_manual.STYLES_MARKDOWN.replace(b"# E3-MD:", "# E3-MD:你".encode(), 1))
        self.assertEqual(m2_manual.STYLES_RUST.decode().splitlines()[0], "// E3-RUST:")
        self.assertEqual(m2_manual.STYLES_MARKDOWN.decode().splitlines()[0], "# E3-MD:")
        self.assertIn('weight: "normal"', m2_manual.STYLES_CONFIG.decode())
        self.assertIn("italic: false", m2_manual.STYLES_CONFIG.decode())
        self.assertIn("C-? redo", m2_manual.STYLES_INSTRUCTIONS)
        self.assertEqual(len(m2_manual.STYLES_OBSERVATIONS), 4)
        self.assertNotIn("强调只换颜色", m2_manual.LANGUAGES_INSTRUCTIONS)

    def test_styles_failed_ime_observation_is_not_reported_as_confirmed(self):
        self.completed_fixture("styles")
        observations = self.directory / "observations.tsv"
        observations.write_text(observations.read_text().replace("styles-rust-ime\tyes", "styles-rust-ime\tno"))
        with self.assertRaisesRegex(RuntimeError, "manual observation not confirmed"):
            m2_manual.verify(self.directory, "styles")
        result = json.loads((self.directory / "result.json").read_text())
        self.assertFalse(result["completed"])
        self.assertEqual(result["ime"], "unconfirmed")

    def test_styles_incorrect_commits_and_changed_configuration_stop_without_repair(self):
        for name, data in (
            ("body.rs", m2_manual.STYLES_RUST),
            ("body.md", m2_manual.STYLES_MARKDOWN.replace(b"# E3-MD:", "# E3-MD:你你ni".encode())),
            ("config/semacs/init.js", b"preserve modified configuration"),
        ):
            with self.subTest(name=name):
                self.directory = Path(self.temporary.name) / name.replace("/", "-")
                self.completed_fixture("styles")
                target = self.directory / name
                target.write_bytes(data)
                with self.assertRaisesRegex(RuntimeError, "disk-byte mismatch"):
                    m2_manual.verify(self.directory, "styles")
                self.assertEqual(target.read_bytes(), data)

    def test_styles_performance_entry_is_representative_and_stops_between_invocations(self):
        source = (Path(__file__).parent / "acceptance.fish").read_text()
        block = source.split('else if test "$mode" = styles-perf\n')[1].split('else if test "$mode" = syntax-perf\n')[0]
        commands = [line.strip() for line in block.splitlines() if "scripts/perf_native.py" in line]
        self.assertEqual(len(commands), 2)
        self.assertIn("--sample mixed --runs 3", commands[0])
        self.assertIn('env XDG_CONFIG_HOME="$output/config"', commands[0])
        self.assertIn("--syntax --sample markdown --runs 3", commands[1])
        self.assertIn('--syntax-manifest-sha256 "$syntax_manifest"', commands[1])
        self.assertNotIn("--sample html", block)
        for command in commands:
            following = block.split(command, 1)[1].lstrip()
            self.assertTrue(following.startswith("or fail "))
        self.assertIn('test ! -L "$output/config/semacs/init.js"', block)

    def test_wrong_profile_cannot_pass(self):
        self.completed_fixture("m2")
        with self.assertRaisesRegex(RuntimeError, "fixture profile does not match"):
            m2_manual.verify(self.directory, "candidates")


if __name__ == "__main__":
    unittest.main()
