# SPDX-License-Identifier: GPL-3.0-or-later
"""Tests of the robco release tools: the tag check, the SHA-256 list and
the notice file. Run: python3 -m unittest discover -s tools/release"""

import hashlib
import os
import pathlib
import subprocess
import sys
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import notices  # noqa: E402

# Only what bash needs: the tests never pass the caller's environment.
BASE_ENV = {"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "LC_ALL": "C"}


def run(script, *args, env=None, cwd=None):
    return subprocess.run(["bash", str(HERE / script), *args], capture_output=True,
                          text=True, env={**BASE_ENV, **(env or {})}, cwd=cwd)


class TagTest(unittest.TestCase):
    def check(self, tag, version="0.1.0"):
        return run("check-tag.sh", env={"TAG": tag, "VERSION": version}).returncode

    def test_good_tags(self):
        for tag in ["robco-0.1.0-pre.1", "robco-0.1.0-pre.12", "robco-0.1.0-pre.9999"]:
            self.assertEqual(self.check(tag), 0, tag)

    def test_bad_tags(self):
        for tag in ["", "robco-0.1.0", "robco-0.1.0-pre.", "robco-0.1.0-pre.0",
                    "robco-0.1.0-pre.01", "robco-0.1.0-pre.1a", "robco-0.1.0-pre.10000",
                    "robco-0.2.0-pre.1", "robco-0.1.0-rc.1", "v0.1.0", "qemu-10.1.2-r1",
                    "robco-0.1.0-pre.1\n", "robco-0.1.0-pre.1 ", "xrobco-0.1.0-pre.1",
                    "robco-0.1.0-pre.1;true", "robco-0.1.0-pre.$(id)", "robco-0.1.0-pre.-1"]:
            self.assertEqual(self.check(tag), 1, repr(tag))

    def test_bad_versions(self):
        for v in ["", "0.1", "0.1.0-pre", "01.1.0", "0.1.0\n"]:
            self.assertEqual(self.check(f"robco-{v}-pre.1", v), 1, repr(v))


class SumsTest(unittest.TestCase):
    def test_list_format(self):
        with tempfile.TemporaryDirectory() as d:
            files = {"robco-linux-x64": b"a", "robco-windows-x64.exe": b"bb",
                     "robco-macos-arm64": b"", "THIRD_PARTY_NOTICES.txt": b"n\n"}
            for name, data in files.items():
                pathlib.Path(d, name).write_bytes(data)
            os.mkdir(os.path.join(d, "sub"))
            r = run("sums.sh", d, "SHA256SUMS")
            self.assertEqual(r.returncode, 0, r.stderr)
            want = "".join(f"{hashlib.sha256(files[n]).hexdigest()}  {n}\n"
                           for n in sorted(files))
            self.assertEqual(pathlib.Path(d, "SHA256SUMS").read_text(), want)
            # Again: the list leaves itself out.
            self.assertEqual(run("sums.sh", d, "SHA256SUMS").returncode, 0)
            self.assertEqual(pathlib.Path(d, "SHA256SUMS").read_text(), want)

    def test_refusals(self):
        with tempfile.TemporaryDirectory() as d:
            self.assertEqual(run("sums.sh", d, "SHA256SUMS").returncode, 1)
            pathlib.Path(d, "a b").write_bytes(b"x")
            self.assertEqual(run("sums.sh", d, "SHA256SUMS").returncode, 1)
            self.assertEqual(run("sums.sh", d, "x/SHA256SUMS").returncode, 2)


class NoticesTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.d = pathlib.Path(self.tmp.name)
        core = self.d / "core"
        (core / "third_party/fatfs").mkdir(parents=True)
        (core / "LICENSE").write_text("core licence\n")
        (core / "third_party/fatfs/LICENSE.txt").write_text("fatfs licence\n")
        self.core = core
        self.reg = self.d / "src"
        for name, files in [("alpha", ["LICENSE-MIT", "README.md"]), ("beta", ["COPYING"]),
                            ("macro", ["LICENSE"]), ("devonly", [])]:
            folder = self.reg / "index.example-0" / f"{name}-1.0.0"
            folder.mkdir(parents=True)
            for f in files:
                (folder / f).write_text(f"{name} {f}\n")

    def tearDown(self):
        self.tmp.cleanup()

    def build(self, tree):
        return notices.build(tree, self.core, self.reg)

    ROBCO = "robco v1.0.0 (/w/crates/robco)\tGPL-3.0-or-later\thttps://example.org/robco\n"
    ALPHA = "alpha v1.0.0\tMIT\thttps://example.org/alpha\n"
    BETA = "beta v1.0.0\tApache-2.0\t\n"

    def test_only_the_tree_with_its_texts(self):
        text, n, errors = self.build(self.ROBCO + self.ALPHA + self.BETA
                                     + "alpha v1.0.0 (*)\tMIT\thttps://example.org/alpha\n"
                                     + "studio-core v0.0.0 (/w/crates/studio-core)\tGPL-3.0-or-later\t\n")
        self.assertEqual(errors, [])
        self.assertEqual(n, 2)
        self.assertIn("Rust crate alpha 1.0.0, https://example.org/alpha\nLicence: MIT", text)
        self.assertIn("--- LICENSE-MIT ---\nalpha LICENSE-MIT", text)
        self.assertNotIn("README", text)
        self.assertIn("Rust crate beta 1.0.0\nLicence: Apache-2.0", text)
        self.assertIn("beta COPYING", text)
        self.assertNotIn("macro", text)
        self.assertNotIn("devonly", text)
        self.assertNotIn("studio-core", text)
        self.assertIn("C library RobCo-Core", text)
        self.assertIn("fatfs licence", text)
        self.assertLess(text.index("crate alpha"), text.index("crate beta"))

    def test_a_crate_without_a_licence_fails(self):
        _, _, errors = self.build(self.ROBCO + "devonly v1.0.0\t\t\n")
        self.assertEqual(errors, ["FAIL: devonly 1.0.0 names no licence"])

    def test_a_missing_c_licence_fails(self):
        (self.core / "third_party/fatfs/LICENSE.txt").unlink()
        _, _, errors = self.build(self.ROBCO)
        self.assertEqual(len(errors), 1)
        self.assertIn("FatFs", errors[0])

    def test_a_crate_without_source_fails(self):
        _, _, errors = self.build(self.ROBCO + "ghost v9.9.9\tMIT\t\n")
        self.assertEqual(len(errors), 1)
        self.assertIn("ghost 9.9.9: no source", errors[0])

    def test_a_crate_from_another_registry_fails(self):
        with self.assertRaises(SystemExit):
            self.build(self.ROBCO + "alpha v1.0.0 (registry+https://x.example)\tMIT\t\n")

    def test_a_tree_without_robco_fails(self):
        with self.assertRaises(SystemExit):
            self.build(self.ALPHA)

    def test_command_line(self):
        tree, out = self.d / "t.txt", self.d / "N.txt"
        tree.write_text(self.ROBCO + self.ALPHA)
        code = notices.main(["--target", "x", "--out", str(out), "--tree", str(tree),
                             "--registry", str(self.reg), "--core", str(self.core)])
        self.assertEqual(code, 0)
        self.assertTrue(out.read_text().startswith("Third-party notices for robco\n"))


if __name__ == "__main__":
    unittest.main()
