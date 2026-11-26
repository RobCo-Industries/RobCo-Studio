"""Unit tests for prefix-map.sh. Compiles one small file with the host's C
compiler, as build.sh compiles QEMU: by absolute path, with debug
information and an assertion. No network.
Run with: python3 -m unittest discover -s tools/qemu -v"""

import os
import pathlib
import shutil
import subprocess
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
CC = shutil.which("cc") or shutil.which("gcc") or shutil.which("clang")
SOURCE = """#include <assert.h>
const char *where(void) { return __FILE__; }
int check(int x) { assert(x > 0); return x; }
"""


def flags(os_name, work, path=None):
    env = {"PATH": path or os.environ.get("PATH", "/usr/bin:/bin")}
    r = subprocess.run(["bash", str(HERE / "prefix-map.sh"), os_name, str(work)],
                       capture_output=True, text=True, env=env, check=True)
    return r.stdout.split()


class PrefixMap(unittest.TestCase):
    def setUp(self):
        self.root = pathlib.Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.root)
        self.work = self.root / "tmp.build123"
        (self.work / "src").mkdir(parents=True)
        (self.work / "src/a.c").write_text(SOURCE)

    def compiled(self, extra):
        obj = self.root / "a.o"
        # In the build folder, the source named by its absolute path, as
        # meson names generated and some included files.
        subprocess.run([CC, "-g", "-O0", *extra, "-c", str(self.work / "src/a.c"), "-o", str(obj)],
                       cwd=self.work, check=True, capture_output=True)
        return obj.read_bytes()

    def spellings(self):
        return [str(self.work).encode(), str(self.work.resolve()).encode()]

    @unittest.skipUnless(CC, "no C compiler")
    def test_without_the_map_the_object_holds_the_folder(self):
        data = self.compiled([])
        self.assertTrue(any(s in data for s in self.spellings()))

    @unittest.skipUnless(CC, "no C compiler")
    def test_with_the_map_the_object_holds_no_spelling_of_the_folder(self):
        data = self.compiled(flags("linux", self.work))
        for s in self.spellings():
            self.assertNotIn(s, data)

    def test_physical_path_is_mapped(self):
        link = self.root / "link"
        link.symlink_to(self.work)
        self.assertEqual(flags("linux", link), [f"-ffile-prefix-map={link}=.",
                                                f"-ffile-prefix-map={self.work.resolve()}=."])

    def test_windows_maps_the_form_the_compiler_sees(self):
        fake = self.root / "fakebin"
        fake.mkdir()
        (fake / "cygpath").write_text("#!/usr/bin/env bash\necho 'D:/a/_temp/msys64/tmp/tmp.build123'\n")
        (fake / "cygpath").chmod(0o755)
        got = flags("windows", self.work, f"{fake}:" + os.environ.get("PATH", "/usr/bin:/bin"))
        self.assertIn("-ffile-prefix-map=D:/a/_temp/msys64/tmp/tmp.build123=.", got)

    def test_missing_folder(self):
        r = subprocess.run(["bash", str(HERE / "prefix-map.sh"), "linux", str(self.root / "gone")],
                           capture_output=True, text=True, check=False)
        self.assertNotEqual(r.returncode, 0)


if __name__ == "__main__":
    unittest.main()
