"""Unit tests for notices.sh, on small fake packages and fake packagers.
Run with: python3 -m unittest discover -s tools/qemu -v"""

import os
import pathlib
import shutil
import subprocess
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent

GPL2 = ("GNU GENERAL PUBLIC LICENSE\nVersion 2, June 1991\n...\n"
        "Public License instead of this License.\n")
BSD2 = ("Redistribution and use in source and binary forms\n...\n"
        "EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE\n")
ZLIB = ("Permission is granted to anyone to use this software for any purpose\n"
        "This notice may not be removed or altered from any source distribution.\n")
POINTER = "On Debian systems, see /usr/share/common-licenses/GPL-2.\n"

LIBFDT = "libfdt.so.1 | libfdt | Ubuntu 22.04 | libfdt1 | 1.6 | from /lib | licence texts: GPL-2.0 BSD-2-Clause"
ZLIB_LINE = "libz.so.1 | zlib | Ubuntu 22.04 | zlib1g | 1.2 | from /lib | licence texts: Zlib"


class Check(unittest.TestCase):
    def setUp(self):
        self.out = pathlib.Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.out)
        dist = self.out / "qemu"
        self.lib = dist / "lib"
        self.lic = dist / "licenses"
        self.lib.mkdir(parents=True)
        self.lic.mkdir()
        (dist / "NOTICE").write_text("QEMU\n")
        (self.lic / "qemu-COPYING").write_text(GPL2)
        (self.lic / "qemu-LICENSE").write_text("QEMU licence summary\n")
        (self.lib / "libfdt.so.1").write_text("elf")
        (self.lib / "libz.so.1").write_text("elf")
        (self.lic / "libfdt-copyright").write_text("libfdt copyright\n" + BSD2)
        (self.lic / "text-GPL-2").write_text(GPL2)
        (self.lic / "zlib-copyright").write_text(ZLIB)
        self.write_record([LIBFDT, ZLIB_LINE])

    def write_record(self, lines):
        (self.lic / "packed-libraries.txt").write_text(
            "# header\n" + "".join(line + "\n" for line in lines))

    def run_check(self):
        env = {"PATH": os.environ.get("PATH", "/usr/bin:/bin")}
        r = subprocess.run(["bash", str(HERE / "notices.sh"), "--check", "linux", str(self.out)],
                           capture_output=True, text=True, env=env, check=False)
        return r.returncode, [l for l in r.stdout.splitlines() if l.startswith("FAIL")]

    def assert_fails(self, *needles):
        code, fails = self.run_check()
        self.assertEqual(code, 1)
        for needle in needles:
            self.assertTrue(any(needle in f for f in fails), (needle, fails))

    def test_good_package_passes(self):
        self.assertEqual(self.run_check(), (0, []))

    def test_licence_id_left_out_and_its_text_deleted(self):
        self.write_record([LIBFDT.replace(" BSD-2-Clause", ""), ZLIB_LINE])
        (self.lic / "libfdt-copyright").write_text("libfdt copyright\n")
        self.assert_fails("names licence texts 'GPL-2.0' for libfdt.so.1 (libfdt)",
                          "no full BSD-2-Clause text in licenses/ for libfdt.so.1 (libfdt)")

    def test_record_line_removed(self):
        self.write_record([ZLIB_LINE])
        self.assert_fails("libfdt.so.1 (libfdt) has no record line")

    def test_extra_record_line(self):
        self.write_record([LIBFDT, ZLIB_LINE, ZLIB_LINE.replace("libz.so.1", "libpixman-1.so.0")])
        self.assert_fails("the record names libpixman-1.so.0, which is not in the package")

    def test_duplicate_record_line(self):
        self.write_record([LIBFDT, ZLIB_LINE, ZLIB_LINE])
        self.assert_fails("libz.so.1 (zlib) has 2 record lines")

    def test_unknown_library(self):
        (self.lib / "libnew.so.3").write_text("elf")
        self.assert_fails("libnew.so.3 is a packed library this script does not know")

    def test_own_licence_file_removed(self):
        (self.lic / "zlib-copyright").unlink()
        self.assert_fails("no licence or notice file of its own in licenses/ for libz.so.1 (zlib)")

    def test_text_replaced_by_pointer(self):
        (self.lic / "text-GPL-2").write_text(POINTER)
        self.assert_fails("no full GPL-2.0 text in licenses/ for libfdt.so.1 (libfdt)")

    def test_qemu_copying_pointer(self):
        (self.lic / "qemu-COPYING").write_text(POINTER)
        self.assert_fails("qemu-COPYING is not the full GPL-2.0 text")

    def test_record_names_a_package_the_licence_was_not_read_from(self):
        self.write_record([LIBFDT, ZLIB_LINE.replace("| zlib1g |", "| zlib-ng1 |")])
        self.assert_fails("FAIL: libz.so.1 (zlib) comes from package zlib-ng1")

    def test_notice_removed(self):
        (self.out / "qemu" / "NOTICE").unlink()
        self.assert_fails("NOTICE is missing")


def write_exe(path, body):
    path.write_text("#!/usr/bin/env bash\n" + body)
    path.chmod(0o755)


def make_tar(dest, root, member):
    subprocess.run(["tar", "cf", str(dest), "-C", str(root), member], check=True)


class Pack(unittest.TestCase):
    """Packing mode on a fake Homebrew or MSYS2 whose package leaves libfdt's
    BSD text out, so the script fetches it from the source. Fake brew, otool,
    pacman and curl stand first on PATH: nothing reaches the network. The
    fake otool prints the whole file as its machine code."""

    def setUp(self):
        self.root = pathlib.Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.root)
        self.out = self.root / "out"
        build = self.out / "build"
        build.mkdir(parents=True)
        (build / "COPYING").write_text(GPL2)
        (build / "LICENSE").write_text("QEMU licence summary\n")
        self.bin = self.root / "fakebin"
        self.bin.mkdir()
        # The upstream dtc archive: the BSD text the packager left out.
        up = self.root / "up" / "dtc-1.8.1"
        up.mkdir(parents=True)
        (up / "BSD-2-Clause").write_text(BSD2)
        self.upstream = self.root / "dtc-1.8.1.tar.gz"
        make_tar(self.upstream, self.root / "up", "dtc-1.8.1")
        # QEMU's source archive, as build.sh keeps it.
        q = self.root / "q" / "qemu-10.1.2"
        q.mkdir(parents=True)
        (q / "VERSION").write_text("10.1.2\n")
        (q / "COPYING").write_text(GPL2)
        (q / "LICENSE").write_text("QEMU licence summary\n")
        subprocess.run(["tar", "cJf", str(build / "qemu-10.1.2.tar.xz"), "-C", str(self.root / "q"),
                        "qemu-10.1.2"], check=True)
        write_exe(self.bin / "otool", """[ "$1" = -t ] || exit 2
echo "$2:"; cat "$2"
""")
        self.sources = []

    def env(self):
        env = {"PATH": f"{self.bin}:" + os.environ.get("PATH", "/usr/bin:/bin")}
        if "QEMU_RELEASE_URL" in os.environ:
            env["QEMU_RELEASE_URL"] = os.environ["QEMU_RELEASE_URL"]
        return env

    def fake_qemu(self, name):
        dist_bin = self.out / "qemu" / "bin"
        dist_bin.mkdir(parents=True, exist_ok=True)
        write_exe(dist_bin / name, 'echo "QEMU emulator version 10.1.2"\n')

    def packed_from(self, os_name, name, source):
        """Record, as pack.sh does, that packed file name came from source."""
        d = subprocess.run(["bash", str(HERE / "digest.sh"), os_name, str(source)], env=self.env(),
                           capture_output=True, text=True, check=True).stdout.strip()
        self.sources.append(f"{name}\t{source}\t{d}\n")
        (self.out / "sources.txt").write_text("".join(self.sources))

    def pack_fails(self, os_name):
        r = subprocess.run(["bash", str(HERE / "notices.sh"), os_name, str(self.out)],
                           capture_output=True, text=True, env=self.env(), check=False)
        self.assertNotEqual(r.returncode, 0, r.stdout[-2000:])
        return r.stderr

    def run_pack(self, os_name):
        env = self.env()
        r = subprocess.run(["bash", str(HERE / "notices.sh"), os_name, str(self.out)],
                           capture_output=True, text=True, env=env, check=False)
        self.assertEqual(r.returncode, 0, r.stdout[-2000:] + r.stderr[-2000:])
        r = subprocess.run(["bash", str(HERE / "notices.sh"), "--check", os_name, str(self.out)],
                           capture_output=True, text=True, env=env, check=False)
        self.assertEqual(r.returncode, 0, r.stdout[-2000:])
        text = (self.out / "qemu" / "licenses" / "packed-libraries.txt").read_text()
        self.assertTrue(text.endswith("\n"))
        return [l for l in text.splitlines() if not l.startswith("#")]

    def assert_one_line_each(self, lines, want):
        self.assertEqual([l.split(" | ")[0] for l in lines], [f for f, _ in want])
        for line, (_, ids) in zip(lines, want):
            self.assertEqual(len(line.split(" | ")), 7, line)
            self.assertTrue(line.endswith("licence texts: " + ids), line)

    def homebrew(self, zlib_from):
        """A Cellar with dtc 1.8.1 and two kegs of zlib, 1.3 and 1.3.1, whose
        libz differ. opt/zlib points at the newer. The package holds the libz
        of keg zlib_from, and sources.txt names it."""
        self.cellar = self.root / "Cellar"
        dist_lib = self.out / "qemu" / "lib"
        dist_lib.mkdir(parents=True)
        self.fake_qemu("qemu-system-arm")
        self.kegs = {}
        for formula, v, lib, licence in (("dtc", "1.8.1", "libfdt.1.dylib", GPL2),
                                          ("zlib", "1.3", "libz.1.dylib", ZLIB),
                                          ("zlib", "1.3.1", "libz.1.dylib", ZLIB)):
            keg = self.cellar / formula / v
            (keg / "lib").mkdir(parents=True)
            (keg / "lib" / lib).write_text(f"macho {formula} {v}")
            (keg / "INSTALL_RECEIPT.json").write_text("{}")
            (keg / "COPYING").write_text(licence)
            self.kegs[(formula, v)] = keg / "lib" / lib
        opt = self.root / "opt"
        opt.mkdir()
        (opt / "zlib").symlink_to(self.cellar / "zlib" / "1.3.1")
        shutil.copy(self.kegs[("dtc", "1.8.1")], dist_lib / "libfdt.1.dylib")
        self.packed_from("macos", "libfdt.1.dylib", self.kegs[("dtc", "1.8.1")])
        shutil.copy(self.kegs[("zlib", zlib_from)], dist_lib / "libz.1.dylib")
        self.packed_from("macos", "libz.1.dylib", self.kegs[("zlib", zlib_from)])
        write_exe(self.bin / "brew", f"""case $1 in
  --cellar) echo {self.cellar} ;;
  fetch) ;;
  --cache) echo {self.upstream} ;;
  *) exit 2 ;;
esac
""")

    def test_homebrew_keg_without_a_text(self):
        self.homebrew("1.3.1")
        lines = self.run_pack("macos")
        self.assert_one_line_each(lines, [("libfdt.1.dylib", "GPL-2.0 BSD-2-Clause"),
                                          ("libz.1.dylib", "Zlib")])
        self.assertIn("licence texts from the source archive dtc-1.8.1.tar.gz", lines[0])
        self.assertNotIn("source archive", lines[1])
        notice = (self.out / "qemu" / "NOTICE").read_text()
        self.assertIn("QEMU 10.1.2 for Arm, as built by RobCo Studio from qemu-10.1.2.tar.xz", notice)
        self.assertIn("This build was not published", notice)

    def test_release_notice_names_the_release(self):
        self.homebrew("1.3.1")
        os.environ["QEMU_RELEASE_URL"] = "https://example.invalid/releases/tag/qemu-10.1.2-r1"
        self.addCleanup(os.environ.pop, "QEMU_RELEASE_URL")
        self.run_pack("macos")
        notice = (self.out / "qemu" / "NOTICE").read_text()
        self.assertIn("above, at https://example.invalid/releases/tag/qemu-10.1.2-r1", notice)
        self.assertNotIn("not published", notice)

    def test_homebrew_two_kegs_packed_from_the_newer(self):
        self.homebrew("1.3.1")
        z = self.run_pack("macos")[1].split(" | ")
        self.assertEqual(z[3:5], ["zlib", "1.3.1"])
        self.assertIn(f"from {self.cellar}/zlib/1.3.1/lib/libz.1.dylib", z[5])

    def test_homebrew_two_kegs_packed_from_the_older(self):
        self.homebrew("1.3")
        z = self.run_pack("macos")[1].split(" | ")
        self.assertEqual(z[3:5], ["zlib", "1.3"])
        self.assertIn(f"from {self.cellar}/zlib/1.3/lib/libz.1.dylib", z[5])

    def test_homebrew_record_names_a_keg_the_bytes_did_not_come_from(self):
        self.homebrew("1.3.1")
        self.sources[1] = self.sources[1].replace("/1.3.1/", "/1.3/")
        (self.out / "sources.txt").write_text("".join(self.sources))
        self.assertIn("libz.1.dylib: its source", self.pack_fails("macos"))

    def test_homebrew_packed_bytes_differ_from_the_source(self):
        self.homebrew("1.3.1")
        (self.out / "qemu" / "lib" / "libz.1.dylib").write_text("macho zlib 1.3")
        self.assertIn("libz.1.dylib: the packed file is not the bytes of", self.pack_fails("macos"))

    def test_homebrew_source_in_no_keg(self):
        self.homebrew("1.3.1")
        stray = self.root / "elsewhere" / "libz.1.dylib"
        stray.parent.mkdir()
        shutil.copy(self.kegs[("zlib", "1.3.1")], stray)
        self.sources[1] = ""
        self.packed_from("macos", "libz.1.dylib", stray)
        self.assertIn(f"{stray}, the source of libz.1.dylib, is in no Homebrew keg", self.pack_fails("macos"))

    def test_homebrew_no_source_recorded(self):
        self.homebrew("1.3.1")
        (self.out / "sources.txt").write_text(self.sources[0])
        self.assertIn("libz.1.dylib has 0 source lines", self.pack_fails("macos"))

    def test_homebrew_formula_renamed(self):
        self.homebrew("1.3.1")
        (self.cellar / "zlib").rename(self.cellar / "zlib-ng-compat")
        self.sources[1] = ""
        self.packed_from("macos", "libz.1.dylib", self.cellar / "zlib-ng-compat" / "1.3.1" / "lib" / "libz.1.dylib")
        self.assertIn("libz.1.dylib (zlib) comes from package zlib-ng-compat", self.pack_fails("macos"))

    def test_qemu_archive_of_another_version(self):
        self.homebrew("1.3.1")
        write_exe(self.out / "qemu" / "bin" / "qemu-system-arm", 'echo "QEMU emulator version 10.2.0"\n')
        self.assertIn("no source archive", self.pack_fails("macos"))

    def msys2(self):
        ucrt = self.root / "ucrt64"
        (ucrt / "bin").mkdir(parents=True)
        dist_bin = self.out / "qemu" / "bin"
        self.fake_qemu("qemu-system-arm.exe")
        lic = ucrt / "share" / "licenses"
        lic.mkdir(parents=True)
        (lic / "dtc-COPYING").write_text(GPL2)
        (lic / "zlib-LICENSE").write_text(ZLIB)
        for dll in ("libfdt-1.dll", "zlib1.dll"):
            (ucrt / "bin" / dll).write_text(f"pe {dll}")
            shutil.copy(ucrt / "bin" / dll, dist_bin / dll)
            self.packed_from("windows", dll, ucrt / "bin" / dll)
        # MSYS2's source package holds the upstream archive.
        srcdir = self.root / "src" / "mingw-w64-dtc"
        srcdir.mkdir(parents=True)
        shutil.copy(self.upstream, srcdir / "dtc-1.8.1.tar.gz")
        srcpkg = self.root / "srcpkg.tar"
        make_tar(srcpkg, self.root / "src", "mingw-w64-dtc")
        write_exe(self.bin / "pacman", f"""pkg() {{ case $1 in *libfdt*|*dtc) echo dtc ;; *) echo zlib ;; esac; }}
case $1 in
  -Qqo) case $2 in {ucrt}/*) echo "mingw-w64-ucrt-x86_64-$(pkg "$2")" ;; *) echo "error: No package owns $2" >&2; exit 1 ;; esac ;;
  -Q) case $(pkg "$2") in dtc) echo "$2 1.8.1-2" ;; *) echo "$2 1.3-1" ;; esac ;;
  -Qi) echo "Name : $2" ;;
  -Qlq) case $(pkg "$2") in dtc) echo {lic}/dtc-COPYING ;; *) echo {lic}/zlib-LICENSE ;; esac ;;
  *) exit 2 ;;
esac
""")
        write_exe(self.bin / "curl", f"""case $3 in
  */mingw-w64-dtc-1.8.1-2.src.tar.zst) cp {srcpkg} "$2" ;;
  *) echo "unexpected fetch $3" >&2; exit 22 ;;
esac
""")
        self.ucrt = ucrt

    def test_msys2_package_without_a_text(self):
        self.msys2()
        lines = self.run_pack("windows")
        self.assert_one_line_each(lines, [("libfdt-1.dll", "GPL-2.0 BSD-2-Clause"),
                                          ("zlib1.dll", "Zlib")])
        self.assertIn("licence texts from the source package mingw-w64-dtc-1.8.1-2.src.tar.zst", lines[0])
        self.assertNotIn("source package", lines[1])

    def test_msys2_source_no_package_owns(self):
        self.msys2()
        stray = self.root / "stray" / "zlib1.dll"
        stray.parent.mkdir()
        shutil.copy(self.ucrt / "bin" / "zlib1.dll", stray)
        self.sources[1] = ""
        self.packed_from("windows", "zlib1.dll", stray)
        self.assertIn(f"no MSYS2 package owns {stray}, the source of zlib1.dll", self.pack_fails("windows"))


if __name__ == "__main__":
    unittest.main()
