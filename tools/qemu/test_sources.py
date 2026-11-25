"""Unit tests for sources.sh, on fake packagers first on PATH. No network.
Run with: python3 -m unittest discover -s tools/qemu -v"""

import io
import os
import pathlib
import shutil
import subprocess
import tarfile
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent


def write_exe(path, body):
    path.write_text("#!/usr/bin/env bash\nset -eu\n" + body)
    path.chmod(0o755)


def tar_with(path, names):
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w") as t:
        for n in names:
            info = tarfile.TarInfo(n)
            info.size = 1
            t.addfile(info, io.BytesIO(b"x"))
    path.write_bytes(buf.getvalue())


class Sources(unittest.TestCase):
    def setUp(self):
        self.root = pathlib.Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.root)
        self.out = self.root / "out"
        lic = self.out / "qemu/licenses"
        lic.mkdir(parents=True)
        self.record = lic / "packed-libraries.txt"
        self.bin = self.root / "fakebin"
        self.bin.mkdir()

    def write_record(self, lines):
        self.record.write_text("# head\n# file | component | packager | package | version | details | licence texts\n"
                               + "".join(l + "\n" for l in lines))

    def run_sources(self, os_name):
        env = {"PATH": f"{self.bin}:" + os.environ.get("PATH", "/usr/bin:/bin"),
               "PACMAN_DB": str(self.root / "pacman")}
        return subprocess.run(["bash", str(HERE / "sources.sh"), os_name, str(self.out)],
                              capture_output=True, text=True, env=env, check=False)

    def listed(self):
        lines = (self.out / "sources/SOURCES.txt").read_text().splitlines()
        return sorted(l.split(" | ")[2] for l in lines if not l.startswith("#"))

    # Linux: dpkg-query answers from a table, apt-get source writes the
    # three files of a source package, unless told the version is gone.
    def ubuntu(self, installed="1.2-1"):
        write_exe(self.bin / "dpkg-query", f"""
case "$*" in
  *'${{Version}}'*) echo '{installed}' ;;
  *'${{source:Package}}'*) echo zlib ;;
  *'${{source:Version}}'*) echo '1:1.2-1' ;;
esac
""")
        write_exe(self.bin / "apt-get", """
[ "$1 $2 $3" = "source --download-only zlib=1:1.2-1" ] || { echo "E: no $3" >&2; exit 100; }
[ -f "${GONE:-/nonexistent}" ] && { echo "E: Unable to find a source package" >&2; exit 100; }
touch zlib_1.2-1.dsc zlib_1.2.orig.tar.gz zlib_1.2-1.debian.tar.xz
""")
        self.write_record(["libz.so.1 | zlib | Ubuntu 22.04 | zlib1g | 1.2-1 | source package zlib 1:1.2-1, from /lib | licence texts: Zlib"])

    def test_linux_fetches_the_source_package(self):
        self.ubuntu()
        r = self.run_sources("linux")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.listed(), ["zlib1g/zlib_1.2-1.debian.tar.xz", "zlib1g/zlib_1.2-1.dsc",
                                         "zlib1g/zlib_1.2.orig.tar.gz"])

    def test_linux_source_no_longer_served(self):
        self.ubuntu()
        (self.root / "gone").touch()
        write_exe(self.bin / "apt-get", "echo 'E: Unable to find a source package' >&2; exit 100\n")
        r = self.run_sources("linux")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("Unable to find a source package", r.stderr)

    def test_linux_installed_version_moved(self):
        self.ubuntu(installed="1.2-2")
        r = self.run_sources("linux")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("zlib1g is 1.2-2 now, but 1.2-1 was packed", r.stderr)

    def test_linux_no_dsc(self):
        self.ubuntu()
        write_exe(self.bin / "apt-get", "touch zlib_1.2.orig.tar.gz\n")
        r = self.run_sources("linux")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("no .dsc for zlib 1:1.2-1", r.stderr)

    def test_empty_record(self):
        self.write_record([])
        r = self.run_sources("linux")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("names no packed library", r.stderr)

    # Windows: pacman answers, the local database names the package base,
    # and curl serves the source package from a folder.
    def msys2(self, base="mingw-w64-gcc", pkgbuild=True):
        pkg, ver = "mingw-w64-ucrt-x86_64-gcc-libs", "15.2.0-8"
        db = self.root / "pacman/local" / f"{pkg}-{ver}"
        db.mkdir(parents=True)
        (db / "desc").write_text(f"%NAME%\n{pkg}\n\n%BASE%\n{base}\n\n%VERSION%\n{ver}\n")
        write_exe(self.bin / "pacman", f"printf 'Name            : {pkg}\\nVersion         : {ver}\\n'\n")
        served = self.root / "served"
        served.mkdir()
        names = ["mingw-w64-gcc/PKGBUILD", "mingw-w64-gcc/0001.patch"] if pkgbuild else ["x/README"]
        tar_with(served / f"mingw-w64-gcc-{ver}.src.tar.zst", names)
        write_exe(self.bin / "curl", f"""
f=$(basename "${{@: -1}}")
[ -f "{served}/$f" ] || {{ echo "curl: (22) 404 for $f" >&2; exit 22; }}
cp "{served}/$f" "$2"
""")
        self.write_record([f"libatomic-1.dll | libatomic | MSYS2 | {pkg} | {ver} | from /ucrt64 | licence texts: GPL-3.0 GCC-exception-3.1"])

    def test_windows_fetches_by_package_base(self):
        self.msys2()
        r = self.run_sources("windows")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.listed(), ["mingw-w64-ucrt-x86_64-gcc-libs/mingw-w64-gcc-15.2.0-8.src.tar.zst",
                                         "mingw-w64-ucrt-x86_64-gcc-libs/pacman-Qi.txt"])

    def test_windows_source_package_gone(self):
        self.msys2(base="mingw-w64-gcc-gone")
        r = self.run_sources("windows")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("404", r.stderr)

    def test_windows_source_package_without_pkgbuild(self):
        self.msys2(pkgbuild=False)
        r = self.run_sources("windows")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("holds no PKGBUILD", r.stderr)

    # macOS: a fake brew with one keg and a cache.
    def homebrew(self, tarball="glib-2.86.0.tar.xz", revision=1):
        cellar = self.root / "Cellar"
        keg = cellar / "glib/2.86.0_1"
        keg.mkdir(parents=True)
        (keg / "INSTALL_RECEIPT.json").write_text("{}\n")
        cache = self.root / "cache"
        cache.mkdir()
        (cache / tarball).write_text("src\n")
        patch = cache / ("a" * 64 + "--glib-fix.patch")
        patch.write_text("patch\n")
        write_exe(self.bin / "brew", f"""
case "$1" in
  --cellar) echo "{cellar}" ;;
  --cache) echo "{cache}/{tarball}" ;;
  info) echo '{{"formulae": [{{"versions": {{"stable": "2.86.0"}}, "revision": {revision}}}]}}' ;;
  cat) echo 'class Glib < Formula; end' ;;
  fetch) echo "Already downloaded: {cache}/{tarball}"; echo "Downloaded to: {patch}" ;;
esac
""")
        self.write_record(["libglib-2.0.0.dylib | glib | Homebrew | glib | 2.86.0_1 | x | licence texts: LGPL-2.1"])

    def test_macos_keeps_formula_archive_and_patch(self):
        self.homebrew()
        r = self.run_sources("macos")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.listed(), ["glib/INSTALL_RECEIPT.json", "glib/brew-info.json",
                                         "glib/glib-2.86.0.tar.xz", "glib/glib-fix.patch", "glib/glib.rb"])

    def test_macos_archive_of_another_version(self):
        self.homebrew(tarball="glib-2.87.0.tar.xz")
        r = self.run_sources("macos")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("is not version 2.86.0_1", r.stderr)

    def test_macos_formula_revision_moved(self):
        self.homebrew(revision=2)
        r = self.run_sources("macos")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("the formula of glib is 2.86.0_2 now, but the keg packed is 2.86.0_1", r.stderr)

    def test_macos_formula_without_revision(self):
        self.homebrew(revision=0)
        r = self.run_sources("macos")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("is 2.86.0 now, but the keg packed is 2.86.0_1", r.stderr)


if __name__ == "__main__":
    unittest.main()
