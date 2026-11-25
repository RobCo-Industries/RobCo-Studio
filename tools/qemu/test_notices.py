"""Unit tests for notices.sh --check, on a small fake Linux package.
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
    BSD text out, so the script fetches it from the source. Fake brew, pacman,
    ldd and curl stand first on PATH: nothing reaches the network."""

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

    def run_pack(self, os_name):
        env = {"PATH": f"{self.bin}:" + os.environ.get("PATH", "/usr/bin:/bin")}
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

    def test_homebrew_keg_without_a_text(self):
        cellar = self.root / "Cellar"
        for formula, v, lib, licence in (("dtc", "1.8.1", "libfdt.1.dylib", GPL2),
                                          ("zlib", "1.3", "libz.1.dylib", ZLIB)):
            keg = cellar / formula / v
            (keg / "lib").mkdir(parents=True)
            (keg / "lib" / lib).write_text("macho")
            (keg / "INSTALL_RECEIPT.json").write_text("{}")
            (keg / "COPYING").write_text(licence)
            dist_lib = self.out / "qemu" / "lib"
            dist_lib.mkdir(parents=True, exist_ok=True)
            (dist_lib / lib).write_text("macho")
        write_exe(self.bin / "brew", f"""case $1 in
  --cellar) echo {cellar} ;;
  fetch) ;;
  --cache) echo {self.upstream} ;;
  *) exit 2 ;;
esac
""")
        lines = self.run_pack("macos")
        self.assert_one_line_each(lines, [("libfdt.1.dylib", "GPL-2.0 BSD-2-Clause"),
                                          ("libz.1.dylib", "Zlib")])
        self.assertIn("licence texts from the source archive dtc-1.8.1.tar.gz", lines[0])
        self.assertNotIn("source archive", lines[1])

    def test_msys2_package_without_a_text(self):
        ucrt = self.root / "ucrt64"
        (ucrt / "bin").mkdir(parents=True)
        dist_bin = self.out / "qemu" / "bin"
        dist_bin.mkdir(parents=True)
        lic = ucrt / "share" / "licenses"
        lic.mkdir(parents=True)
        (lic / "dtc-COPYING").write_text(GPL2)
        (lic / "zlib-LICENSE").write_text(ZLIB)
        for dll in ("libfdt-1.dll", "zlib1.dll"):
            (dist_bin / dll).write_text("pe")
        # MSYS2's source package holds the upstream archive.
        srcdir = self.root / "src" / "mingw-w64-dtc"
        srcdir.mkdir(parents=True)
        shutil.copy(self.upstream, srcdir / "dtc-1.8.1.tar.gz")
        srcpkg = self.root / "srcpkg.tar"
        make_tar(srcpkg, self.root / "src", "mingw-w64-dtc")
        write_exe(self.bin / "ldd", f"""echo "	libfdt-1.dll => {ucrt}/bin/libfdt-1.dll (0x1)"
echo "	zlib1.dll => {ucrt}/bin/zlib1.dll (0x2)"
""")
        write_exe(self.bin / "pacman", f"""pkg() {{ case $1 in *libfdt*|*dtc) echo dtc ;; *) echo zlib ;; esac; }}
case $1 in
  -Qqo) echo "mingw-w64-ucrt-x86_64-$(pkg "$2")" ;;
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
        lines = self.run_pack("windows")
        self.assert_one_line_each(lines, [("libfdt-1.dll", "GPL-2.0 BSD-2-Clause"),
                                          ("zlib1.dll", "Zlib")])
        self.assertIn("licence texts from the source package mingw-w64-dtc-1.8.1-2.src.tar.zst", lines[0])
        self.assertNotIn("source package", lines[1])


if __name__ == "__main__":
    unittest.main()
