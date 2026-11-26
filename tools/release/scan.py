#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Fail when a release program names a folder of the machine that built it.

Usage: scan.py PROGRAM... [--workspace DIR] [--forbid TEXT]

Searches each program for the builder's home folder, the workspace folder
and the cargo home, which is CARGO_HOME or .cargo in the home folder. The
rules are those of `tools/qemu/release.py scan`, whose code this reuses:
each path with either slash, a path with a drive letter in each of its
spellings and without regard to case, every path without case on macOS
and Windows, each in UTF-8 and in UTF-16 little endian. Each hit is
followed by the printable string around it and its offset.

Exits 1 on a hit, 0 when no program names a folder.
"""

import argparse
import os
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "qemu"))
import release  # noqa: E402


def forbidden(workspace, home=None, cargo_home=None):
    """The folders to search for: the home folder, the workspace and the
    cargo home, without duplicates."""
    home = home or str(pathlib.Path.home())
    cargo_home = cargo_home or os.environ.get("CARGO_HOME") or str(pathlib.Path(home) / ".cargo")
    out = []
    for p in (home, str(workspace), cargo_home):
        p = p.rstrip("/\\")
        if p and p not in out:
            out.append(p)
    return out


def scan(programs, forbid):
    """The (program, label, needle, caseless) hits, the first spelling found
    of each forbidden folder in each program."""
    ns = release.needles(forbid)
    hits = []
    for prog in programs:
        data = pathlib.Path(prog).read_bytes()
        low = data.lower()
        found = set()
        for group, label, n, nocase in ns:
            if group not in found and n in (low if nocase else data):
                found.add(group)
                hits.append((prog, label, n, nocase))
    return hits


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("programs", nargs="+")
    ap.add_argument("--workspace", default=str(HERE.parent.parent))
    ap.add_argument("--forbid", action="append", default=[])
    a = ap.parse_args(argv)
    try:
        forbid = forbidden(a.workspace) + a.forbid
        hits = scan(a.programs, forbid)
    except release.Refused as e:
        print(f"FAIL: {e}")
        return 1
    for prog, label, n, nocase in hits:
        print(f"FAIL: {prog} holds {label}")
        for at, around in release.context(prog, n, nocase):
            print(f"  at byte {at}: {around}")
    if hits:
        return 1
    print(f"No program names {', '.join(forbid)}.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
