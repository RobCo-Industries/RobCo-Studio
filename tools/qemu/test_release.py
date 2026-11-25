"""Unit tests for release.py, on small fake packages. No network.
Run with: python3 -m unittest discover -s tools/qemu -v"""

import hashlib
import json
import pathlib
import shutil
import sys
import tarfile
import tempfile
import unittest
import zipfile

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import release  # noqa: E402

QSRC = b"qemu source archive"
QSHA = hashlib.sha256(QSRC).hexdigest()


class Release(unittest.TestCase):
    def setUp(self):
        self.tmp = pathlib.Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp)
        self.src = self.tmp / "in"
        self.out = self.tmp / "out"
        self.repo = self.tmp / "repo"
        for part in ("tools/qemu", ".github/workflows", "docs"):
            (self.repo / part).mkdir(parents=True)
        (self.repo / "tools/qemu/build.sh").write_text("build\n")
        (self.repo / ".github/workflows/qemu.yml").write_text("name: qemu\n")
        (self.repo / "docs/THIRD_PARTY.md").write_text("terms\n")
        for os_ in release.HOSTS:
            exe = ".exe" if os_ == "windows" else ""
            p = self.src / f"qemu-{os_}"
            (p / "bin").mkdir(parents=True)
            (p / "licenses").mkdir()
            for t in ("arm", "aarch64"):
                (p / "bin" / f"qemu-system-{t}{exe}").write_bytes(b"\x7fELF" + os_.encode())
            (p / "licenses/qemu-COPYING").write_text("GPL\n")
            (p / "licenses/packed-libraries.txt").write_text("# record\n")
            (p / "NOTICE").write_text(f"QEMU 10.1.2 for Arm, as built by RobCo Studio from qemu-10.1.2.tar.xz,\n"
                                      f"SHA-256 {QSHA}.\n")
            s = self.src / f"sources-{os_}"
            (s / "glib").mkdir(parents=True)
            (s / "glib/glib.dsc").write_text("dsc\n")
            (s / "SOURCES.txt").write_text("# head\nglib | 1 | glib/glib.dsc | x\n")
            r = self.src / f"report-{os_}"
            r.mkdir()
            (r / "report.txt").write_text("os: x\nwork: /tmp/w\nconfigure: ./configure --x\n")
        (self.src / "qemu-source").mkdir()
        (self.src / "qemu-source/qemu-10.1.2.tar.xz").write_bytes(QSRC)

    def gather(self, **kw):
        return release.gather(self.src, self.out, self.repo, **kw)

    def refused(self, needle, **kw):
        with self.assertRaises(release.Refused) as e:
            self.gather(**kw)
        self.assertIn(needle, str(e.exception))

    def test_gathers_and_verifies(self):
        self.assertEqual(self.gather(tag="qemu-10.1.2-r1"), "10.1.2")
        self.assertEqual(release.verify(self.out), "10.1.2")
        names = sorted(p.name for p in self.out.iterdir())
        self.assertIn("qemu-10.1.2-linux-x64.tar.gz", names)
        self.assertIn("qemu-10.1.2-macos-arm64.tar.gz", names)
        self.assertIn("qemu-10.1.2-windows-x64.zip", names)
        self.assertIn("qemu-10.1.2.tar.xz", names)
        self.assertIn("qemu-10.1.2-recipe.tar.gz", names)
        for h in ("linux-x64", "macos-arm64", "windows-x64"):
            self.assertIn(f"qemu-10.1.2-{h}-notices.tar.gz", names)
            self.assertIn(f"qemu-10.1.2-{h}-library-sources.tar", names)
        listing = json.loads((self.out / "qemu-10.1.2-files.json").read_text())
        self.assertEqual(listing["archives"]["linux-x64"]["root"], "qemu-10.1.2-linux-x64")
        self.assertEqual(listing["archives"]["windows-x64"]["url"], "qemu-10.1.2-windows-x64.zip")
        self.assertEqual(len(listing["files"]), len(names) - 2)

    def test_program_is_executable_in_tar(self):
        self.gather()
        with tarfile.open(self.out / "qemu-10.1.2-linux-x64.tar.gz") as t:
            m = t.getmember("qemu-10.1.2-linux-x64/bin/qemu-system-arm")
            self.assertEqual(m.mode, 0o755)
            self.assertEqual(t.getmember("qemu-10.1.2-linux-x64/NOTICE").mode, 0o644)

    def test_same_input_same_bytes(self):
        self.gather()
        first = {p.name: p.read_bytes() for p in self.out.iterdir()}
        shutil.rmtree(self.out)
        self.gather()
        self.assertEqual(first, {p.name: p.read_bytes() for p in self.out.iterdir()})

    def test_base_url(self):
        self.gather(base_url="https://example.invalid/r/")
        listing = json.loads((self.out / "qemu-10.1.2-files.json").read_text())
        self.assertEqual(listing["archives"]["macos-arm64"]["url"],
                         "https://example.invalid/r/qemu-10.1.2-macos-arm64.tar.gz")
        release.verify(self.out)

    def test_tag_of_wrong_form(self):
        self.refused("is not of the form", tag="qemu-10.1.2")
        self.refused("is not of the form", tag="v10.1.2-r1")

    def test_tag_of_other_version(self):
        self.refused("names QEMU 10.1.3", tag="qemu-10.1.3-r1")

    def test_packages_disagree(self):
        n = self.src / "qemu-macos/NOTICE"
        n.write_text(n.read_text().replace("10.1.2 for", "10.1.1 for"))
        self.refused("different QEMU builds")

    def test_source_archive_not_the_one_built(self):
        (self.src / "qemu-source/qemu-10.1.2.tar.xz").write_bytes(b"other")
        self.refused("is not the archive the packages were built from")

    def test_package_without_notice(self):
        (self.src / "qemu-linux/NOTICE").unlink()
        self.refused("has no NOTICE")

    def test_package_without_licenses(self):
        shutil.rmtree(self.src / "qemu-windows/licenses")
        self.refused("has no licenses/")

    def test_sources_missing(self):
        shutil.rmtree(self.src / "sources-linux/glib")
        self.refused("does not list exactly its files")

    def test_sources_unlisted(self):
        (self.src / "sources-macos/glib/extra.patch").write_text("p\n")
        self.refused("does not list exactly its files")

    def test_link_refused(self):
        (self.src / "qemu-linux/lib").mkdir()
        (self.src / "qemu-linux/lib/libz.so").symlink_to("/etc/hosts")
        self.refused("symbolic link")

    def test_no_configure_line(self):
        (self.src / "report-linux/report.txt").write_text("os: linux\n")
        self.refused("no configure line")

    def test_verify_extra_file(self):
        self.gather()
        (self.out / "stray.txt").write_text("x")
        with self.assertRaises(release.Refused):
            release.verify(self.out)

    def test_verify_changed_file(self):
        self.gather()
        (self.out / "qemu-10.1.2.tar.xz").write_bytes(b"changed")
        with self.assertRaises(release.Refused):
            release.verify(self.out)

    def test_verify_archive_without_notice(self):
        self.gather()
        p = self.out / "qemu-10.1.2-windows-x64.zip"
        with zipfile.ZipFile(p) as z:
            kept = [(i, z.read(i)) for i in z.infolist() if not i.filename.endswith("/NOTICE")]
        with zipfile.ZipFile(p, "w") as z:
            for i, d in kept:
                z.writestr(i, d)
        listing_p = self.out / "qemu-10.1.2-files.json"
        listing = json.loads(listing_p.read_text())
        sha, size = release.sha256(p), p.stat().st_size
        for f in listing["files"]:
            if f["name"] == p.name:
                f["sha256"], f["size"] = sha, size
        listing["archives"]["windows-x64"].update(sha256=sha, size=size)
        listing_p.write_text(json.dumps(listing))
        sums = "".join(f"{release.sha256(q)}  {q.name}\n" for q in sorted(self.out.iterdir())
                       if not q.name.endswith("SHA256SUMS"))
        (self.out / "qemu-10.1.2-SHA256SUMS").write_text(sums)
        with self.assertRaises(release.Refused) as e:
            release.verify(self.out)
        self.assertIn("holds no qemu-10.1.2-windows-x64/NOTICE", str(e.exception))


class Scan(unittest.TestCase):
    def setUp(self):
        self.pkg = pathlib.Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.pkg)
        (self.pkg / "bin").mkdir()
        (self.pkg / "bin/qemu").write_bytes(b"\x00../hw/arm/raspi.c\x00")

    def test_clean(self):
        self.assertEqual(release.scan(self.pkg, ["/home/builder", "host-123"]), [])

    def test_path_found(self):
        (self.pkg / "bin/qemu").write_bytes(b"\x00/home/builder/qemu/hw.c\x00")
        self.assertEqual(release.scan(self.pkg, ["/home/builder"]), [("bin/qemu", "/home/builder")])

    def test_msys_path_found_as_windows_path(self):
        (self.pkg / "bin/qemu").write_bytes(b"\x00c:\\users\\builder\\x.c\x00")
        hits = release.scan(self.pkg, ["/c/Users/builder"])
        self.assertEqual([h[0] for h in hits], ["bin/qemu"])

    def test_host_name_found(self):
        (self.pkg / "NOTICE").write_text("built on host-123\n")
        self.assertEqual(release.scan(self.pkg, ["host-123"]), [("NOTICE", "host-123")])

    def test_short_text_refused(self):
        with self.assertRaises(release.Refused):
            release.scan(self.pkg, ["/a"])

    def test_main_exit_code(self):
        (self.pkg / "NOTICE").write_text("/home/builder\n")
        self.assertEqual(release.main(["scan", str(self.pkg), "--forbid", "/home/builder"]), 1)
        self.assertEqual(release.main(["scan", str(self.pkg), "--forbid", "/home/other"]), 0)


if __name__ == "__main__":
    unittest.main()
