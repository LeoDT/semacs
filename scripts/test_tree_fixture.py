"""Offline checks for the F6 manual fixture and child launch; never opens a desktop."""

import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import m2_manual
import tree_fixture


class TreeFixtureTests(unittest.TestCase):
    def setUp(self):
        temporary = Path(__file__).resolve().parents[1] / ".pi/tmp"
        temporary.mkdir(parents=True, exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(prefix="tree-host-tools-", dir=temporary)
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name) / "files"

    def prepare(self):
        m2_manual.prepare(self.directory, "tree")
        (self.directory / "observations.tsv").write_text(
            "".join(f"{name}\tyes\n" for name in tree_fixture.OBSERVATIONS)
        )

    def test_unchanged_fixture_and_user_observations_do_not_claim_numeric_timing(self):
        self.prepare()
        result = m2_manual.verify(self.directory, "tree")
        self.assertTrue(result["completed"])
        self.assertTrue(result["fixture_paths"])
        self.assertTrue(all(result["disk_bytes"].values()))
        self.assertEqual(result["performance"], "unmeasured")
        self.assertEqual(result["ime"], "unmeasured")
        self.assertEqual(result["gpu"], "unmeasured")
        self.assertIn("unmeasured", result["follow_delay_ms"])
        self.assertFalse(result["screenshots"])
        self.assertFalse(result["key_injection"])
        self.assertEqual((self.directory / "config/semacs/init.js").read_bytes(), b"")
        self.assertTrue((self.directory / "project/.git").is_dir())
        self.assertEqual((self.directory / "instructions.txt").read_text(), tree_fixture.INSTRUCTIONS)

    def test_missing_or_no_observation_cannot_pass(self):
        for answer in ("no", "unobserved", ""):
            with self.subTest(answer=answer):
                self.directory = Path(self.temporary.name) / (answer or "missing")
                self.prepare()
                (self.directory / "observations.tsv").write_text(f"tree-side-window\t{answer}\n")
                with self.assertRaisesRegex(RuntimeError, "manual observation not confirmed"):
                    m2_manual.verify(self.directory, "tree")
                result = json.loads((self.directory / "result.json").read_text())
                self.assertFalse(result["completed"])

    def test_changed_bytes_and_extra_paths_are_preserved_as_failure_evidence(self):
        for name in ("project/00-main.txt", "project/.git/marker", "config/semacs/init.js", "project/extra", "project/new-directory"):
            with self.subTest(name=name):
                self.directory = Path(self.temporary.name) / name.replace("/", "-")
                self.prepare()
                path = self.directory / name
                if name.endswith("new-directory"):
                    path.mkdir()
                else:
                    path.write_bytes(b"preserve this failure evidence")
                with self.assertRaises(RuntimeError):
                    m2_manual.verify(self.directory, "tree")
                if path.is_file():
                    self.assertEqual(path.read_bytes(), b"preserve this failure evidence")
                result = json.loads((self.directory / "result.json").read_text())
                self.assertFalse(result["completed"])

    def test_linked_parent_is_rejected_before_reading_any_fixture_file_bytes(self):
        self.prepare()
        outside = Path(self.temporary.name) / "daily"
        outside.mkdir()
        (outside / "init.js").write_bytes(b"daily config must not be read or changed")
        target = self.directory / "config/semacs"
        shutil.rmtree(target)
        target.symlink_to(outside, target_is_directory=True)
        with patch.object(Path, "read_bytes", side_effect=AssertionError("must not read bytes")):
            with self.assertRaisesRegex(RuntimeError, "linked fixture path"):
                m2_manual.verify(self.directory, "tree")
        self.assertEqual((outside / "init.js").read_bytes(), b"daily config must not be read or changed")
        self.assertTrue(target.is_symlink())

    def test_linked_manifest_is_rejected_without_reading_it(self):
        self.prepare()
        outside = Path(self.temporary.name) / "daily-manifest"
        outside.write_bytes(b"not a fixture")
        path = self.directory / "fixture.json"
        path.unlink()
        path.symlink_to(outside)
        with self.assertRaisesRegex(RuntimeError, "linked fixture manifest"):
            m2_manual.verify(self.directory, "tree")
        self.assertEqual(outside.read_bytes(), b"not a fixture")

    def test_existing_tree_fixture_is_never_overwritten(self):
        self.prepare()
        path = self.directory / "project/00-main.txt"
        path.write_bytes(b"preserve")
        with self.assertRaises(FileExistsError):
            m2_manual.prepare(self.directory, "tree")
        self.assertEqual(path.read_bytes(), b"preserve")

    @unittest.skipUnless(shutil.which("fish"), "fish is required to exercise the child launch command")
    def test_tree_launch_isolates_configuration_and_history_only_in_the_child(self):
        output = Path(self.temporary.name) / "output"
        m2_manual.prepare(output / "files", "tree")
        root = Path(self.temporary.name) / "mock-root"
        binary = root / "target/release/semacs"
        binary.parent.mkdir(parents=True)
        binary.write_text('''#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
config = Path(os.environ["XDG_CONFIG_HOME"]) / "semacs/init.js"
state = Path(os.environ["XDG_STATE_HOME"])
state.mkdir(parents=True)
(state / "child-history-marker").write_bytes(b"isolated child")
Path(os.environ["CAPTURE"]).write_text(json.dumps({
    "config": str(config), "bytes": config.read_text(), "file": sys.argv[1], "state": str(state),
}))
''')
        binary.chmod(0o755)
        daily = Path(self.temporary.name) / "daily"
        daily.mkdir()
        (daily / "fish").mkdir()
        (daily / "sentinel").write_bytes(b"daily config and history must not change")
        source = (Path(__file__).parent / "acceptance.fish").read_text()
        commands = [line for line in source.splitlines() if '"$output/files/project/00-main.txt" >' in line]
        self.assertEqual(len(commands), 1)
        environment = {
            **os.environ, "XDG_CONFIG_HOME": str(daily), "XDG_STATE_HOME": str(daily),
            "CAPTURE": str(output / "capture.json"), "PYTHONDONTWRITEBYTECODE": "1",
        }
        script = (
            f"set -l root {shlex.quote(str(root))}\n"
            f"set -l output {shlex.quote(str(output))}\n"
            "set -l mode tree\n"
            f"{commands[0]}\nor exit $status\n"
            'printf "%s\\n%s\\n" "$XDG_CONFIG_HOME" "$XDG_STATE_HOME" > "$output/parent-env.txt"\n'
        )
        subprocess.run(["fish", "--no-config", "-c", script], env=environment,
                       check=True, capture_output=True, timeout=10)
        result = json.loads((output / "capture.json").read_text())
        self.assertEqual(result["config"], str(output / "files/config/semacs/init.js"))
        self.assertEqual(result["bytes"], "")
        self.assertEqual(result["state"], str(output / "state"))
        self.assertEqual(result["file"], str(output / "files/project/00-main.txt"))
        self.assertEqual((output / "parent-env.txt").read_text().splitlines(), [str(daily), str(daily)])
        self.assertEqual(set(daily.iterdir()), {daily / "sentinel", daily / "fish"})
        self.assertEqual(list((daily / "fish").iterdir()), [])
        self.assertEqual((daily / "sentinel").read_bytes(), b"daily config and history must not change")

    def test_fish_routes_tree_to_one_manual_window_and_stops_on_failed_observations(self):
        source = (Path(__file__).parent / "acceptance.fish").read_text()
        build = source.split('else if contains -- "$mode" candidates scripting', 1)[1].split('else if test "$mode" = split-ime', 1)[0]
        self.assertIn(" tree\n", build)
        self.assertIn("cargo build --release --bin semacs --target-dir", build)
        self.assertNotIn("perf_native", build)
        block = source.split('else if contains -- "$mode" themes syntax languages flicker styles magit tree\n', 1)[1].split('else if test "$mode" = messages', 1)[0]
        self.assertEqual(block.count('"$output/files/project/00-main.txt"'), 1)
        self.assertIn("set checks " + " ".join(tree_fixture.OBSERVATIONS), block)
        self.assertIn('if test "$answer" != yes', block)
        self.assertIn('fail "theme/syntax observation not passed', block)
        self.assertIn('XDG_STATE_HOME="$output/state"', block)
        self.assertIn("Exact 0.2-second", block)
        self.assertNotIn("--software", block)


if __name__ == "__main__":
    unittest.main()
