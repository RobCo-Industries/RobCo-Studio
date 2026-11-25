# SPDX-License-Identifier: GPL-3.0-or-later
"""Tests of check-pins.py against local repositories.

Run from the repository root: `python3 -m unittest scripts/test_check_pins.py`.
The submodule is a shallow clone, as CI's checkout makes it.
"""

import pathlib
import subprocess
import sys
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parent / "check-pins.py"
GIT = ["git", "-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "protocol.file.allow=always"]


def git(cwd, *args):
    return subprocess.run([*GIT, *args], cwd=cwd, check=True, capture_output=True, text=True).stdout.strip()


def commit(repo, name):
    (repo / name).write_text(name)
    git(repo, "add", name)
    git(repo, "commit", "-qm", name)
    return git(repo, "rev-parse", "HEAD")


class CheckPins(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        base = pathlib.Path(self.tmp.name)
        self.core = base / "core"
        self.core.mkdir()
        git(self.core, "init", "-q", "-b", "main")
        git(self.core, "config", "uploadpack.allowAnySHA1InWant", "true")
        commit(self.core, "a")
        self.behind = commit(self.core, "b")
        commit(self.core, "c")
        git(self.core, "checkout", "-q", "-b", "side", self.behind)
        self.side = commit(self.core, "s")
        git(self.core, "checkout", "-q", "main")
        self.studio = base / "studio"
        (self.studio / "crates/x").mkdir(parents=True)
        (self.studio / "rust-toolchain.toml").write_text('[toolchain]\nchannel = "1.2.3"\n')
        (self.studio / "Cargo.toml").write_text(
            '[workspace]\nmembers = ["crates/x"]\n[workspace.package]\nrust-version = "1.2.3"\n'
        )
        (self.studio / "crates/x/Cargo.toml").write_text("[package]\nrust-version.workspace = true\n")
        git(self.studio, "init", "-q", "-b", "main")

    def tearDown(self):
        self.tmp.cleanup()

    def pin(self, sha, url=None):
        sub = self.studio / "external/robco-core"
        git(self.studio, "clone", "-q", "--depth", "1", self.core.as_uri(), str(sub))
        git(sub, "fetch", "-q", "--depth", "1", "origin", sha)
        git(sub, "checkout", "-q", sha)
        if url:
            git(sub, "remote", "set-url", "origin", url)
        (self.studio / ".gitmodules").write_text(
            f'[submodule "external/robco-core"]\n\tpath = external/robco-core\n\turl = {self.core.as_uri()}\n'
        )
        git(self.studio, "add", ".gitmodules", "rust-toolchain.toml", "Cargo.toml", "crates/x/Cargo.toml")
        git(self.studio, "update-index", "--add", "--cacheinfo", f"160000,{sha},external/robco-core")
        git(self.studio, "commit", "-qm", "pin")

    def run_check(self):
        r = subprocess.run([sys.executable, str(SCRIPT)], cwd=self.studio, capture_output=True, text=True)
        return r.returncode, r.stdout

    def test_pin_behind_main_in_a_shallow_clone_passes(self):
        self.pin(self.behind)
        rc, out = self.run_check()
        self.assertEqual(rc, 0, out)
        self.assertIn(f"{self.behind} is on main", out)

    def test_pin_off_main_fails(self):
        self.pin(self.side)
        rc, out = self.run_check()
        self.assertEqual(rc, 1)
        self.assertIn(f"pinned commit {self.side} is not on RobCo-Core main", out)

    def test_fetch_that_cannot_run_fails_as_unknown(self):
        self.pin(self.behind, url=(self.core.parent / "gone").as_uri())
        rc, out = self.run_check()
        self.assertEqual(rc, 1)
        self.assertIn("could not tell", out)
        self.assertNotIn("is not on", out)

    def test_missing_submodule_fails(self):
        git(self.studio, "add", "rust-toolchain.toml", "Cargo.toml", "crates/x/Cargo.toml")
        git(self.studio, "commit", "-qm", "no submodule")
        rc, out = self.run_check()
        self.assertEqual(rc, 1)
        self.assertIn(".gitmodules does not declare", out)


if __name__ == "__main__":
    unittest.main()
