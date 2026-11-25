#!/usr/bin/env python3
"""Spike K2: build Culvert by direct commands, without Make.

Mirrors Pip-OS sdk/pipos-app.mk for KIND=holotape: the same flags, the
same input order, the same seal. Prints the SHA-256 of every object, of
libpip.a, of the ELF and of the sealed CULVERT.HTP.

    build.py --deitrix DIR --gcc-bin DIR --out DIR [--prefix-map] [--ar-d]
                              [--no-ident] [--strip-elf]
    build.py --hash-only DIR    hash the outputs of a make build

Commands run in holotapes/culvert with relative paths, as make does.
"""

import argparse
import hashlib
import os
import subprocess
import sys

SDK = "../../external/pip-os/sdk"
CORE = SDK + "/../external/robco-core"
SRCS = ["main.c", "game.c", "logic.c", "draw.c"]
LIB_SDK = ["crc32.c", "heap.c", "ini.c", "printf.c", "selftest.c",
           "string.c", "svc.c", "syscall.c"]  # sorted, as $(sort $(wildcard))
LIB_CORE = ["k_string.c", "k_printf.c", "alloc.c", "ini.c", "crc32.c"]

ARCH = ["-mcpu=arm1176jzf-s", "-marm", "-mfloat-abi=soft",
        "-mgeneral-regs-only", "-mno-unaligned-access"]
BASE = ARCH + ["-std=gnu11", "-ffreestanding", "-nostdlib",
               "-ffunction-sections", "-fdata-sections",
               "-fno-tree-loop-distribute-patterns",
               "-O2", "-g", "-Wall", "-Wextra", "-MMD", "-MP",
               "-I" + SDK + "/include"]
RENAMES = ["-D%s=pip__robco_%s" % (f, f) for f in
           ["crc32", "crc32_update", "ini_parse", "ini_get", "ini_get_int",
            "ini_get_bool", "ini_strerror"]]
LIBFLAGS = BASE + ["-I" + CORE + "/include"] + RENAMES


def sha(path):
    with open(path, "rb") as f:
        return hashlib.sha256(f.read()).hexdigest()


def run(cmd, cwd):
    print("+ " + " ".join(cmd), file=sys.stderr)
    subprocess.run(cmd, cwd=cwd, check=True, stdout=sys.stderr)


def hash_tree(out):
    rows = []
    for root, _, files in os.walk(out):
        for name in files:
            if name.endswith((".o", ".a", ".elf", ".HTP")):
                p = os.path.join(root, name)
                rel = os.path.relpath(p, out).replace(os.sep, "/")
                rows.append((rel, sha(p)))
    for rel, h in sorted(rows):
        print("%s  %s" % (h, rel))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--deitrix")
    ap.add_argument("--gcc-bin")
    ap.add_argument("--out")
    ap.add_argument("--prefix-map", action="store_true",
                    help="add -ffile-prefix-map=<cwd>=. to every compile")
    ap.add_argument("--ar-d", action="store_true",
                    help="build libpip.a with ar rcsD: zero dates and ids")
    ap.add_argument("--no-ident", action="store_true",
                    help="add -fno-ident: no .comment naming the host arch")
    ap.add_argument("--strip-elf", action="store_true",
                    help="also write CULVERT.nodebug.elf, without debug info")
    ap.add_argument("--hash-only")
    a = ap.parse_args()
    if a.hash_only:
        hash_tree(a.hash_only)
        return

    cwd = os.path.join(os.path.abspath(a.deitrix), "holotapes", "culvert")
    out = os.path.abspath(a.out)
    exe = ".exe" if os.name == "nt" else ""
    gcc = os.path.join(a.gcc_bin, "arm-none-eabi-gcc" + exe)
    ar = os.path.join(a.gcc_bin, "arm-none-eabi-ar" + exe)
    objcopy = os.path.join(a.gcc_bin, "arm-none-eabi-objcopy" + exe)
    extra = []
    if a.prefix_map:
        # gcc sees cwd as typed by the OS; map both spellings on Windows.
        # The compiler's own include dir also lands in the debug info.
        tool = os.path.dirname(os.path.abspath(a.gcc_bin))
        pairs = [(cwd, "."), (tool, "/toolchain")]
        extra = []
        for src, dst in pairs:
            extra.append("-ffile-prefix-map=%s=%s" % (src, dst))
            if os.name == "nt":
                extra.append("-ffile-prefix-map=%s=%s"
                             % (src.replace("\\", "/"), dst))
    if a.no_ident:
        extra.append("-fno-ident")
    cflags = BASE + extra
    libflags = LIBFLAGS + extra
    lib = os.path.join(out, "libpip")
    os.makedirs(os.path.join(out, "obj"), exist_ok=True)
    os.makedirs(lib, exist_ok=True)

    # libpip.a, members in the Makefile's order: sdk/lib sorted, then core.
    members = []
    for s in LIB_SDK:
        o = os.path.join(lib, "pip_" + s[:-2] + ".o")
        run([gcc] + libflags + ["-c", "-o", o, SDK + "/lib/" + s], cwd)
        members.append(o)
    for s in LIB_CORE:
        o = os.path.join(lib, "robco_" + s[:-2] + ".o")
        run([gcc] + libflags + ["-c", "-o", o, CORE + "/src/lib/" + s], cwd)
        members.append(o)
    libpip = os.path.join(lib, "libpip.a")
    if os.path.exists(libpip):
        os.remove(libpip)
    run([ar, "rcsD" if a.ar_d else "rcs", libpip] + members, cwd)

    crt0 = os.path.join(out, "crt0_holotape.o")
    run([gcc] + cflags + ['-DPIP_IMG_MAGIC="ROBCO78"', "-DPIP_IMG_SUBTYPE=0",
                          "-c", "-o", crt0, SDK + "/crt0.S"], cwd)
    objs = []
    for s in SRCS:
        o = os.path.join(out, "obj", s + ".o")
        run([gcc] + cflags + ["-c", "-o", o, s], cwd)
        objs.append(o)

    elf = os.path.join(out, "CULVERT.elf")
    run([gcc] + ARCH + ["-nostdlib", "-T", SDK + "/holotape.ld",
                        "-Wl,--gc-sections", "-Wl,--no-warn-rwx-segments",
                        "-Wl,-Map," + os.path.join(out, "CULVERT.map"),
                        "-o", elf, crt0] + objs + [libpip, "-lgcc"], cwd)

    if a.strip_elf:
        # libgcc.a in the Windows archive carries debug info, the others not.
        run([objcopy, "--strip-debug", elf,
             os.path.join(out, "CULVERT.nodebug.elf")], cwd)

    htp = os.path.join(out, "CULVERT.HTP")
    tmp = htp + ".tmp"
    run([objcopy, "-O", "binary", elf, tmp], cwd)
    rimg = os.path.join(cwd, CORE, "tools", "rimg.py")
    run([sys.executable, rimg, "seal", tmp, "--name", "CULVERT",
         "--image-version", "1.0.0", "--title", "Culvert",
         "--icon", "icon.pbm"], cwd)
    run([sys.executable, rimg, "verify", tmp, "--magic", "ROBCO78"], cwd)
    os.replace(tmp, htp)
    hash_tree(out)


if __name__ == "__main__":
    main()
