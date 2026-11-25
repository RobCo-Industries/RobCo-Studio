# SPDX-License-Identifier: GPL-3.0-or-later
"""Tests of check-pins.py against local repositories.

Run from the repository root: `python3 -m unittest scripts/test_check_pins.py`.
The submodule is a shallow clone, as CI's checkout makes it.
"""

import os
import pathlib
import subprocess
import sys
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parent / "check-pins.py"
GIT = ["git", "-c", "protocol.file.allow=always"]
HOME = tempfile.TemporaryDirectory()


def closed_env():
    """A git environment that owes nothing to the machine's configuration."""
    home = HOME.name
    return {
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "HOME": home,
        "XDG_CONFIG_HOME": home,
        "LANG": "C",
        "LC_ALL": "C",
        "GIT_CONFIG_GLOBAL": os.devnull,
        "GIT_CONFIG_SYSTEM": os.devnull,
        "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_AUTHOR_NAME": "Test Author",
        "GIT_AUTHOR_EMAIL": "author@example.invalid",
        "GIT_COMMITTER_NAME": "Test Committer",
        "GIT_COMMITTER_EMAIL": "committer@example.invalid",
        "GIT_TERMINAL_PROMPT": "0",
        "GIT_TEMPLATE_DIR": "",
        "GIT_CONFIG_COUNT": "4",
        "GIT_CONFIG_KEY_0": "commit.gpgsign",
        "GIT_CONFIG_VALUE_0": "false",
        "GIT_CONFIG_KEY_1": "tag.gpgsign",
        "GIT_CONFIG_VALUE_1": "false",
        "GIT_CONFIG_KEY_2": "core.hooksPath",
        "GIT_CONFIG_VALUE_2": os.devnull,
        "GIT_CONFIG_KEY_3": "init.defaultBranch",
        "GIT_CONFIG_VALUE_3": "main",
    }


def git(cwd, *args):
    return subprocess.run(
        [*GIT, *args], cwd=cwd, check=True, capture_output=True, text=True, env=closed_env()
    ).stdout.strip()


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
        r = subprocess.run(
            [sys.executable, str(SCRIPT)], cwd=self.studio, capture_output=True, text=True, env=closed_env()
        )
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

    def test_url_rewrite_stops_the_fetch(self):
        self.pin(self.behind, url="https://example.invalid/robco-core")
        sub = self.studio / "external/robco-core"
        git(sub, "config", f"url.{self.core.as_uri()}.insteadOf", "https://example.invalid/robco-core")
        rc, out = self.run_check()
        self.assertEqual(rc, 1)
        self.assertIn("a url rewrite rule turns", out)
        self.assertNotIn(f"robco-core: {self.behind} is on", out)

    def test_missing_submodule_fails(self):
        git(self.studio, "add", "rust-toolchain.toml", "Cargo.toml", "crates/x/Cargo.toml")
        git(self.studio, "commit", "-qm", "no submodule")
        rc, out = self.run_check()
        self.assertEqual(rc, 1)
        self.assertIn(".gitmodules does not declare", out)


if __name__ == "__main__":
    unittest.main()
