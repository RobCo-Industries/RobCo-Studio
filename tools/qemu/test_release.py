"""Unit tests for release.py, on small fake packages. No network.
Run with: python3 -m unittest discover -s tools/qemu -v"""

import contextlib
import hashlib
import io
import json
import pathlib
import shutil
import sys
import tarfile
import tempfile
import unittest
import unittest.mock
import zipfile

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import release  # noqa: E402

QSRC = b"qemu source archive"
QSHA = hashlib.sha256(QSRC).hexdigest()


def elf(machine):
    """The first bytes of a little-endian 64-bit ELF program."""
    return b"\x7fELF\x02\x01\x01" + bytes(11) + machine.to_bytes(2, "little") + bytes(10)


def macho(cpu):
    return b"\xcf\xfa\xed\xfe" + cpu.to_bytes(4, "little") + bytes(8)


def fat(*cpus):
    return (b"\xca\xfe\xba\xbe" + len(cpus).to_bytes(4, "big")
            + b"".join(c.to_bytes(4, "big") + bytes(16) for c in cpus))


def pe(machine):
    return b"MZ" + bytes(0x3A) + (0x40).to_bytes(4, "little") + b"PE\0\0" + machine.to_bytes(2, "little")


X64_ELF, ARM64_ELF = elf(0x3E), elf(0xB7)
ARM64_MACHO, X64_MACHO = macho(0x0100000C), macho(0x01000007)
X64_PE, ARM64_PE = pe(0x8664), pe(0xAA64)
RIGHT = {"linux": X64_ELF, "macos": ARM64_MACHO, "windows": X64_PE}


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
                (p / "bin" / f"qemu-system-{t}{exe}").write_bytes(RIGHT[os_] + os_.encode())
            (p / "licenses/qemu-COPYING").write_text("GPL\n")
            (p / "licenses/packed-libraries.txt").write_text("# record\n")
            (p / "NOTICE").write_text(f"QEMU 10.1.2 for Arm, as built by RobCo Studio from qemu-10.1.2.tar.xz,\n"
                                      f"SHA-256 {QSHA}.\n")
            s = self.src / f"sources-{os_}"
            (s / "glib").mkdir(parents=True)
            (s / "glib/glib.dsc").write_text("dsc\n")
            (s / "SOURCES.txt").write_text("# head\nglib | 1 | glib/glib.dsc | "
                                     + hashlib.sha256(b"dsc\n").hexdigest() + "\n")
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
        self.refused("missing ['glib/glib.dsc']")

    def test_sources_unlisted(self):
        (self.src / "sources-macos/glib/extra.patch").write_text("p\n")
        self.refused("unlisted ['glib/extra.patch']")

    def test_sources_changed_after_listed(self):
        (self.src / "sources-windows/glib/glib.dsc").write_text("changed\n")
        self.refused("glib/glib.dsc does not match its SHA-256 in SOURCES.txt")

    def rewrite_sources_tar(self, change):
        """Changes the linux library-sources archive, then fixes the outer
        list and SHA256SUMS so that only the inner check can catch it."""
        p = self.out / "qemu-10.1.2-linux-x64-library-sources.tar"
        with tarfile.open(p) as t:
            entries = {m.name: t.extractfile(m).read() for m in t.getmembers() if m.isfile()}
        change(entries, "qemu-10.1.2-linux-x64-library-sources/")
        p.write_bytes(release.tar_bytes([(n, d, 0o644) for n, d in sorted(entries.items())], False))
        listing_p = self.out / "qemu-10.1.2-files.json"
        listing = json.loads(listing_p.read_text())
        for f in listing["files"]:
            if f["name"] == p.name:
                f["sha256"], f["size"] = release.sha256(p), p.stat().st_size
        listing_p.write_text(json.dumps(listing))
        sums = "".join(f"{release.sha256(q)}  {q.name}\n" for q in sorted(self.out.iterdir())
                       if not q.name.endswith("SHA256SUMS"))
        (self.out / "qemu-10.1.2-SHA256SUMS").write_text(sums)

    def verify_refused(self, needle):
        with self.assertRaises(release.Refused) as e:
            release.verify(self.out)
        self.assertIn(needle, str(e.exception))

    def test_verify_source_changed_in_archive(self):
        self.gather()
        self.rewrite_sources_tar(lambda e, r: e.update({r + "glib/glib.dsc": b"changed\n"}))
        self.verify_refused("glib/glib.dsc does not match its SHA-256 in SOURCES.txt")

    def test_verify_source_unlisted_in_archive(self):
        self.gather()
        self.rewrite_sources_tar(lambda e, r: e.update({r + "glib/extra.patch": b"p\n"}))
        self.verify_refused("unlisted ['glib/extra.patch']")

    def test_verify_source_missing_in_archive(self):
        self.gather()
        self.rewrite_sources_tar(lambda e, r: e.pop(r + "glib/glib.dsc"))
        self.verify_refused("missing ['glib/glib.dsc']")

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

    def put_program(self, os_, name, data):
        (self.src / f"qemu-{os_}" / name).parent.mkdir(exist_ok=True)
        (self.src / f"qemu-{os_}" / name).write_bytes(data)

    def test_wrong_architecture_on_each_host(self):
        cases = [("linux", "bin/qemu-system-arm", ARM64_ELF, "ELF arm64", "linux-x64 expects ELF x86_64"),
                 ("macos", "bin/qemu-system-aarch64", X64_MACHO, "Mach-O x86_64",
                  "macos-arm64 expects Mach-O arm64"),
                 ("windows", "bin/qemu-system-arm.exe", ARM64_PE, "PE arm64", "windows-x64 expects PE x86_64")]
        for os_, name, data, found, want in cases:
            with self.subTest(os_):
                self.setUp()
                self.put_program(os_, name, data)
                with self.assertRaises(release.Refused) as e:
                    self.gather()
                msg = str(e.exception)
                self.assertIn(f"qemu-{os_}/{name} is {found}", msg)
                self.assertIn(want, msg)
                self.assertFalse(any(self.out.iterdir()))

    def test_wrong_format(self):
        self.put_program("macos", "bin/qemu-system-arm", elf(0xB7))
        self.refused("bin/qemu-system-arm is ELF arm64, but macos-arm64 expects Mach-O arm64")

    def test_library_checked_too(self):
        self.put_program("linux", "lib/libglib-2.0.so.0", ARM64_ELF)
        self.refused("lib/libglib-2.0.so.0 is ELF arm64")

    def test_truncated_program(self):
        self.put_program("windows", "bin/qemu-system-arm.exe", X64_PE[:0x44])
        self.refused("bin/qemu-system-arm.exe: cannot read its architecture, the header is cut short")

    def test_not_a_program(self):
        self.put_program("linux", "bin/qemu-system-arm", b"#!/bin/sh\n")
        self.refused("bin/qemu-system-arm: cannot read its architecture, it is not an ELF, Mach-O or PE")

    def test_universal_macho(self):
        self.assertEqual(release.arch_of(fat(0x01000007, 0x0100000C)), ("Mach-O", ("arm64", "x86_64")))
        self.put_program("macos", "lib/libz.1.dylib", fat(0x01000007, 0x0100000C))
        self.gather()
        self.setUp()
        self.put_program("macos", "lib/libz.1.dylib", fat(0x01000007))
        self.refused("lib/libz.1.dylib is Mach-O x86_64, but macos-arm64 expects Mach-O arm64")

    def test_arch_of_right_cases(self):
        self.assertEqual(release.arch_of(X64_ELF), ("ELF", ("x86_64",)))
        self.assertEqual(release.arch_of(ARM64_MACHO), ("Mach-O", ("arm64",)))
        self.assertEqual(release.arch_of(X64_PE), ("PE", ("x86_64",)))
        big = b"\x7fELF\x02\x02\x01" + bytes(11) + b"\x00\xb7"
        self.assertEqual(release.arch_of(big), ("ELF", ("arm64",)))

    def reseal(self, p, host):
        """Fixes the list and SHA256SUMS after archive p changed."""
        listing_p = self.out / "qemu-10.1.2-files.json"
        listing = json.loads(listing_p.read_text())
        sha, size = release.sha256(p), p.stat().st_size
        for f in listing["files"]:
            if f["name"] == p.name:
                f["sha256"], f["size"] = sha, size
        listing["archives"][host].update(sha256=sha, size=size)
        listing_p.write_text(json.dumps(listing))
        sums = "".join(f"{release.sha256(q)}  {q.name}\n" for q in sorted(self.out.iterdir())
                       if not q.name.endswith("SHA256SUMS"))
        (self.out / "qemu-10.1.2-SHA256SUMS").write_text(sums)

    def test_verify_wrong_architecture_in_tar(self):
        self.gather()
        p = self.out / "qemu-10.1.2-macos-arm64.tar.gz"
        with tarfile.open(p) as t:
            entries = {m.name: t.extractfile(m).read() for m in t.getmembers() if m.isfile()}
        entries["qemu-10.1.2-macos-arm64/bin/qemu-system-arm"] = X64_MACHO
        p.write_bytes(release.tar_bytes([(n, d, 0o755) for n, d in sorted(entries.items())], True))
        self.reseal(p, "macos-arm64")
        self.verify_refused("qemu-10.1.2-macos-arm64.tar.gz/bin/qemu-system-arm is Mach-O x86_64, "
                            "but macos-arm64 expects Mach-O arm64")

    def test_verify_wrong_architecture_in_zip(self):
        self.gather()
        p = self.out / "qemu-10.1.2-windows-x64.zip"
        with zipfile.ZipFile(p) as z:
            kept = [(i, z.read(i)) for i in z.infolist()]
        with zipfile.ZipFile(p, "w") as z:
            for i, d in kept:
                z.writestr(i, ARM64_PE if i.filename.endswith("qemu-system-aarch64.exe") else d)
        self.reseal(p, "windows-x64")
        self.verify_refused("bin/qemu-system-aarch64.exe is PE arm64, but windows-x64 expects PE x86_64")

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


def where(hits):
    return [(h[0], h[1]) for h in hits]


class Scan(unittest.TestCase):
    def setUp(self):
        self.pkg = pathlib.Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.pkg)
        (self.pkg / "bin").mkdir()
        (self.pkg / "bin/qemu").write_bytes(b"\x00../hw/arm/raspi.c\x00")

    def put(self, data):
        (self.pkg / "bin/qemu").write_bytes(data)

    def test_clean(self):
        self.assertEqual(release.scan(self.pkg, ["/home/builder", "host-123"]), [])

    def test_path_found(self):
        self.put(b"\x00/home/builder/qemu/hw.c\x00")
        self.assertEqual(where(release.scan(self.pkg, ["/home/builder"], caseless=False)),
                         [("bin/qemu", "/home/builder")])

    def test_unix_path_case_follows_the_system(self):
        self.put(b"\x00/HOME/Builder/x.c\x00")
        self.assertEqual(release.scan(self.pkg, ["/home/builder"], caseless=False), [])
        self.assertEqual(len(release.scan(self.pkg, ["/home/builder"], caseless=True)), 1)

    def test_each_spelling_of_a_windows_path_is_found(self):
        # Whichever spelling the caller passes, each spelling in a file is found.
        given = ["C:\\Users\\builder", "C:/Users/builder", "/c/Users/builder", "/cygdrive/c/Users/builder"]
        planted = [b"C:\\Users\\builder\\x", b"C:/Users/builder/x", b"/c/Users/builder/x",
                   b"/cygdrive/c/Users/builder/x", b'"C:\\\\Users\\\\builder\\\\x"']
        for g in given:
            for p in planted:
                with self.subTest(given=g, planted=p):
                    self.put(b"\x00" + p + b"\x00")
                    self.assertEqual([h[0] for h in release.scan(self.pkg, [g], caseless=False)],
                                     ["bin/qemu"])

    def test_drive_letter_and_name_compare_without_case(self):
        self.put(b"\x00c:\\USERS\\builder\\x.c\x00")
        self.assertEqual([h[0] for h in release.scan(self.pkg, ["C:\\Users\\Builder"], caseless=False)],
                         ["bin/qemu"])

    def test_utf16_found_with_its_context(self):
        self.put(b"\x01\x02" + "at C:\\Users\\builder\\a.c".encode("utf-16-le") + b"\xff\xff")
        hits = release.scan(self.pkg, ["/c/Users/builder"], caseless=False)
        self.assertEqual(len(hits), 1)
        rel, label, n, nocase = hits[0]
        self.assertTrue(label.endswith(" in UTF-16"))
        self.assertEqual(release.context(self.pkg / rel, n, nocase), [(8, "at C:\\Users\\builder\\a.c")])

    def test_host_name_found_in_both_encodings(self):
        (self.pkg / "NOTICE").write_text("built on host-123\n")
        self.assertEqual(where(release.scan(self.pkg, [], hosts=["host-123"])), [("NOTICE", "host-123")])
        (self.pkg / "NOTICE").write_bytes("host-123".encode("utf-16-le"))
        self.assertEqual(where(release.scan(self.pkg, [], hosts=["host-123"])),
                         [("NOTICE", "host-123 in UTF-16")])

    def test_short_text_refused(self):
        for bad in (["/a"], ["C:\\"]):
            with self.subTest(bad=bad), self.assertRaises(release.Refused):
                release.scan(self.pkg, bad)
        with self.assertRaises(release.Refused):
            release.scan(self.pkg, [], hosts=["ab"])

    def test_each_user_form_found_in_both_encodings(self):
        forms = ["/home/builder/x", "\\home\\builder\\x", "/Users/builder/x", "\\Users\\builder\\x",
                 "C:\\Users\\builder\\x", "ssh builder@host"]
        for form in forms:
            for enc in ("utf-8", "utf-16-le"):
                with self.subTest(form=form, enc=enc):
                    self.put(b"\x00" + form.encode(enc) + b"\x00")
                    self.assertEqual([h[0] for h in release.scan(self.pkg, [], caseless=False, users=["builder"])],
                                     ["bin/qemu"])

    def test_user_forms_compare_without_case_where_the_system_does(self):
        self.put(b"\x00c:\\USERS\\Builder\\x\x00")
        self.assertEqual(release.scan(self.pkg, [], caseless=False, users=["builder"]), [])
        self.assertEqual(len(release.scan(self.pkg, [], caseless=True, users=["builder"])), 1)

    def test_bare_user_name_not_found(self):
        self.put(b"\x00builder\x00" + "builder".encode("utf-16-le"))
        for caseless in (False, True):
            self.assertEqual(release.scan(self.pkg, [], caseless=caseless, users=["builder"]), [])

    def test_short_user_name_refused(self):
        with self.assertRaises(release.Refused):
            release.scan(self.pkg, [], users=["bob"])

    def test_runner_word_passes_until_its_home_is_planted(self):
        (self.pkg / "README").write_text("the runner starts each test runner in turn\n")
        (self.pkg / "fmt.json").write_text('{"runner": "tcg", "runners": ["a"]}\n')
        self.assertEqual(release.scan(self.pkg, [], users=["runner"]), [])
        (self.pkg / "planted").write_text("/home/runner/x\n")
        self.assertEqual([h[0] for h in release.scan(self.pkg, [], users=["runner"])], ["planted"])

    def test_main_reads_the_user_from_the_system(self):
        (self.pkg / "NOTICE").write_text("by builder@host\n")
        with unittest.mock.patch.object(release, "this_user", return_value="builder"):
            out = io.StringIO()
            with contextlib.redirect_stdout(out):
                self.assertEqual(release.main(["scan", str(self.pkg), "--forbid-user"]), 1)
        self.assertIn("FAIL: NOTICE holds builder@\n", out.getvalue())

    def test_this_user_names_the_account(self):
        self.assertTrue(release.this_user())

    def test_context_shows_the_string_around_each_hit(self):
        self.put(b"\x01\x02/home/builder/qemu-10.1.2/build/x.h\x00\xff"
                 b"abc\x00assert /home/builder/y.c\x00")
        self.assertEqual(release.context(self.pkg / "bin/qemu", b"/home/builder"),
                         [(2, "/home/builder/qemu-10.1.2/build/x.h"), (50, "assert /home/builder/y.c")])

    def test_main_prints_the_context(self):
        self.put(b"\x00x /home/builder/a.c\x00")
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            self.assertEqual(release.main(["scan", str(self.pkg), "--forbid", "/home/builder"]), 1)
        self.assertIn("FAIL: bin/qemu holds /home/builder\n  at byte 3: x /home/builder/a.c\n", out.getvalue())

    def test_main_reads_the_environment(self):
        self.put(b"\x00x /c/Users/builder/a.c\x00")
        with unittest.mock.patch.dict(release.os.environ, {"SCAN_TEST_HOME": "C:\\Users\\builder"}):
            out = io.StringIO()
            with contextlib.redirect_stdout(out):
                self.assertEqual(release.main(["scan", str(self.pkg), "--forbid-env", "SCAN_TEST_HOME"]), 1)
        self.assertIn("FAIL: bin/qemu holds ", out.getvalue())
        self.assertIn("  at byte 3: x /c/Users/builder/a.c\n", out.getvalue())
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(release.main(["scan", str(self.pkg), "--forbid-env", "SCAN_TEST_UNSET_X"]), 1)

    def test_main_searches_the_host_name(self):
        (self.pkg / "NOTICE").write_text("built on host-123\n")
        with unittest.mock.patch.object(release.socket, "gethostname", return_value="host-123"):
            out = io.StringIO()
            with contextlib.redirect_stdout(out):
                self.assertEqual(release.main(["scan", str(self.pkg), "--forbid-host"]), 1)
        self.assertIn("FAIL: NOTICE holds host-123\n", out.getvalue())

    def test_main_exit_code(self):
        (self.pkg / "NOTICE").write_text("/home/builder\n")
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(release.main(["scan", str(self.pkg), "--forbid", "/home/builder"]), 1)
            self.assertEqual(release.main(["scan", str(self.pkg), "--forbid", "/home/other"]), 0)


if __name__ == "__main__":
    unittest.main()
