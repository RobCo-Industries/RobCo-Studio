"""Unit tests for sources.sh, on fake packagers first on PATH. No network.
Run with: python3 -m unittest discover -s tools/qemu -v"""

import hashlib
import io
import json
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


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


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
    def msys2(self, base="mingw-w64-gcc", pkgbuild=True, many=0):
        pkg, ver = "mingw-w64-ucrt-x86_64-gcc-libs", "15.2.0-8"
        db = self.root / "pacman/local" / f"{pkg}-{ver}"
        db.mkdir(parents=True)
        (db / "desc").write_text(f"%NAME%\n{pkg}\n\n%BASE%\n{base}\n\n%VERSION%\n{ver}\n")
        write_exe(self.bin / "pacman", f"printf 'Name            : {pkg}\\nVersion         : {ver}\\n'\n")
        served = self.root / "served"
        served.mkdir()
        names = ["mingw-w64-gcc/PKGBUILD", "mingw-w64-gcc/0001.patch"] if pkgbuild else ["x/README"]
        names += [f"mingw-w64-gcc/src/{'d' * 60}/file-{i:06d}.c" for i in range(many)]
        tar_with(served / f"mingw-w64-gcc-{ver}.src.tar.zst", names)
        if many:
            # MSYS2's tar lists through zstd, which fails when its reader
            # closes the pipe early. macOS's tar does not, so a fake one
            # that writes the listing as zstd does stands in for it.
            write_exe(self.bin / "tar", f"""
[ "$1" = tf ] || exit 2
exec python3 -c '
import sys, tarfile
try:
    for n in tarfile.open(sys.argv[1]).getnames():
        sys.stdout.write(n + "\\n")
    sys.stdout.flush()
except BrokenPipeError:
    sys.stderr.write("zstd: error 70 : Write error : cannot write block : Broken pipe\\n")
    sys.stderr.close()
    sys.exit(1)
' "$2"
""")
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

    def test_windows_long_listing_read_whole(self):
        # PKGBUILD comes first and the listing is far larger than a pipe
        # holds: a reader that stops at the first match breaks tar's pipe.
        self.msys2(many=4000)
        r = self.run_sources("windows")
        self.assertEqual(r.returncode, 0, r.stderr)

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

    # macOS: a fake brew that answers --cellar and info only, as Homebrew
    # does with no tap on disk, and a fake curl that serves files by URL.
    def homebrew(self, archive="glib-2.86.0.tar.xz", revision=1, rb_revision=None, tamper=None):
        cellar = self.root / "Cellar"
        keg = cellar / "glib/2.86.0_1"
        keg.mkdir(parents=True)
        (keg / "INSTALL_RECEIPT.json").write_text("{}\n")
        served = self.root / "served"
        served.mkdir()
        head = "c0ffee" * 6 + "abcd"
        src_url = f"https://example.org/glib/{archive}"
        src = served / archive
        src.write_bytes(b"src\n" * 70000)
        patch_url = "https://raw.githubusercontent.com/Homebrew/patches/1/glib/fix.diff"
        (served / "fix.diff").write_text("patch\n")
        local = served / "hardcoded-paths.diff"
        local.write_text("local patch\n")
        rb_rev = revision if rb_revision is None else rb_revision
        rb = (f'class Glib < Formula\n  url "{src_url}"\n  sha256 "{sha(src)}"\n'
              + (f"  revision {rb_rev}\n" if rb_rev else "") + "end\n")
        (served / "glib.rb").write_text(rb)
        urls = {f"https://raw.githubusercontent.com/Homebrew/homebrew-core/{head}/Formula/g/glib.rb": "glib.rb",
                src_url: archive, patch_url: "fix.diff",
                f"https://raw.githubusercontent.com/Homebrew/homebrew-core/{head}/Patches/glib/hardcoded-paths.diff":
                    "hardcoded-paths.diff"}
        info = {"formulae": [{
            "name": "glib", "tap": "homebrew/core", "tap_git_head": head,
            "ruby_source_path": "Formula/g/glib.rb", "ruby_source_checksum": {"sha256": sha(served / "glib.rb")},
            "versions": {"stable": "2.86.0"}, "revision": revision,
            "urls": {"stable": {"url": src_url, "using": None, "checksum": sha(src)}},
            "patches": [{"strip": "p1", "url": patch_url, "sha256": sha(served / "fix.diff")},
                        {"strip": "p1", "file": "Patches/glib/hardcoded-paths.diff"}],
            "local_patch_checksums": {"Patches/glib/hardcoded-paths.diff": sha(local)}}]}
        if tamper:
            tamper(served, info["formulae"][0])
        (self.root / "info.json").write_text(json.dumps(info))
        write_exe(self.bin / "brew", f"""
case "$1" in
  --cellar) echo "{cellar}" ;;
  info) cat "{self.root}/info.json" ;;
  *) echo "Error: $1 is a developer command, or needs the tap on disk" >&2; exit 1 ;;
esac
""")
        table = "".join(f"  '{u}') f='{n}' ;;\n" for u, n in urls.items())
        write_exe(self.bin / "curl", f"""
case "${{@: -1}}" in
{table}  *) echo "curl: (22) 404 for ${{@: -1}}" >&2; exit 22 ;;
esac
cp "{served}/$f" "$2"
""")
        self.write_record(["libglib-2.0.0.dylib | glib | Homebrew | glib | 2.86.0_1 | x | licence texts: LGPL-2.1"])

    def test_macos_fetches_formula_archive_and_patches_without_a_tap(self):
        self.homebrew()
        r = self.run_sources("macos")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.listed(), ["glib/brew-info.json", "glib/fix.diff", "glib/glib-2.86.0.tar.xz",
                                         "glib/glib.rb", "glib/hardcoded-paths.diff", "glib/homebrew-receipt.json"])

    def test_macos_formula_file_not_the_one_stated(self):
        def edit(served, f):
            (served / "glib.rb").write_text((served / "glib.rb").read_text() + "# changed\n")
        self.homebrew(tamper=edit)
        r = self.run_sources("macos")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("the formula file Formula/g/glib.rb at", r.stderr)
        self.assertIn("but Homebrew states", r.stderr)

    def test_macos_archive_not_the_one_stated(self):
        self.homebrew(tamper=lambda served, f: (served / "glib-2.86.0.tar.xz").write_text("other\n"))
        r = self.run_sources("macos")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("the source archive of glib", r.stderr)

    def test_macos_local_patch_not_the_one_stated(self):
        self.homebrew(tamper=lambda served, f: (served / "hardcoded-paths.diff").write_text("other\n"))
        r = self.run_sources("macos")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("the patch Patches/glib/hardcoded-paths.diff of glib", r.stderr)

    def test_macos_patch_without_stated_hash(self):
        self.homebrew(tamper=lambda served, f: f["patches"][0].pop("sha256"))
        r = self.run_sources("macos")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("Homebrew states no SHA-256 for a patch of glib", r.stderr)

    def test_macos_archive_of_another_version(self):
        self.homebrew(archive="glib-2.87.0.tar.xz")
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

    def test_macos_fetched_formula_states_another_revision(self):
        self.homebrew(rb_revision=2)
        r = self.run_sources("macos")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("the fetched formula of glib states 2.86.0_2, but the keg packed is 2.86.0_1", r.stderr)

if __name__ == "__main__":
    unittest.main()
