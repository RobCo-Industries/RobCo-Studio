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


if __name__ == "__main__":
    unittest.main()
