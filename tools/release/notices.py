#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Write the third-party notice file of the robco program.

Usage: notices.py --target TRIPLE --out FILE [--metadata FILE --tree FILE]

The crates come from the locked dependency tree of the release target:
`cargo tree --locked --offline` names the packages compiled into robco,
and `cargo metadata` gives each one's licence and source folder. Build
and test dependencies and procedural macros are not in the program. Each crate's licence texts are copied from its source folder,
which cargo has already fetched for the build, so no network is used.
The C that robco links from RobCo-Core, and FatFs inside it, comes last.

With --metadata and --tree the script reads those files in place of
running cargo, for the tests.

Exits 1 when a crate in the program names no licence.
"""

import argparse
import json
import pathlib
import re
import subprocess
import sys

ROOT = "robco"
TEXT = re.compile(r"^(licen[cs]e|copying|notice|unlicense)([-_.].*)?$", re.I)

# The C compiled into the program by studio-sys, relative to the RobCo-Core
# submodule: name, licence and licence file.
C_PARTS = [
    ("RobCo-Core", "MIT", "LICENSE"),
    ("FatFs", "BSD-1-Clause", "third_party/fatfs/LICENSE.txt"),
]


def metadata(target):
    out = subprocess.run(
        ["cargo", "metadata", "--locked", "--offline", "--format-version", "1",
         "--filter-platform", target],
        check=True, capture_output=True, text=True)
    return json.loads(out.stdout)


def tree(target):
    """The packages in the program, as `cargo tree` lists them: robco and
    its normal dependencies, with the features robco alone turns on, and
    without procedural macros, which run in the compiler only."""
    out = subprocess.run(
        ["cargo", "tree", "--locked", "--offline", "-p", ROOT, "-e", "normal,no-proc-macro",
         "--target", target, "--prefix", "none", "-f", "{p}"],
        check=True, capture_output=True, text=True)
    return out.stdout


def shipped(meta, tree_text):
    """The third-party packages of the tree, with their metadata. Workspace
    packages are robco itself, under GPL-3.0-or-later."""
    names = set()
    for line in tree_text.splitlines():
        m = re.match(r"^(\S+) v(\S+)", line)
        if m:
            names.add((m.group(1), m.group(2)))
    if not any(n == ROOT for n, _ in names):
        raise SystemExit(f"error: {ROOT} is not in the dependency tree")
    members = set(meta["workspace_members"])
    found = {(p["name"], p["version"]): p for p in meta["packages"] if p["id"] not in members}
    member_names = {p["name"] for p in meta["packages"] if p["id"] in members}
    missing = sorted(k for k in names if k not in found and k[0] not in member_names)
    if missing:
        raise SystemExit(f"error: not in cargo metadata: {missing}")
    return sorted((found[k] for k in names if k in found), key=lambda p: (p["name"], p["version"]))


def texts(folder):
    """The licence files at the top of a crate's folder, sorted."""
    return sorted(f for f in folder.iterdir() if f.is_file() and TEXT.match(f.name))


def section(title, licence, files, out):
    out.append("=" * 72)
    out.append(title)
    out.append(f"Licence: {licence}")
    if not files:
        out.append("The source carries no licence file. The licence above is the one it declares.")
    for f in files:
        out.append("")
        out.append(f"--- {f.name} ---")
        out.append(f.read_text(encoding="utf-8", errors="replace").rstrip("\n"))
    out.append("")


def build(meta, tree_text, core):
    out = [
        "Third-party notices for robco",
        "",
        "robco is free software under GPL-3.0-or-later. It contains the",
        "following works of others, each under its own licence, listed below",
        "with the licence texts each work ships.",
        "",
    ]
    errors = []
    crates = shipped(meta, tree_text)
    for p in crates:
        lic = p.get("license")
        if not lic:
            errors.append(f"FAIL: {p['name']} {p['version']} names no licence")
            lic = "none declared"
        folder = pathlib.Path(p["manifest_path"]).parent
        src = p.get("repository") or ""
        title = f"Rust crate {p['name']} {p['version']}" + (f", {src}" if src else "")
        section(title, lic, texts(folder), out)
    for name, lic, rel in C_PARTS:
        f = core / rel
        if not f.is_file():
            errors.append(f"FAIL: {name}: no licence file at {rel} in RobCo-Core")
            continue
        section(f"C library {name}, from RobCo-Core", lic, [f], out)
    return "\n".join(out) + "\n", len(crates), errors


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--target", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--metadata")
    ap.add_argument("--tree")
    ap.add_argument("--core", default="external/robco-core")
    a = ap.parse_args(argv)
    meta = json.loads(pathlib.Path(a.metadata).read_text()) if a.metadata else metadata(a.target)
    tree_text = pathlib.Path(a.tree).read_text() if a.tree else tree(a.target)
    text, n, errors = build(meta, tree_text, pathlib.Path(a.core))
    for e in errors:
        print(e, file=sys.stderr)
    if errors:
        return 1
    pathlib.Path(a.out).write_text(text, encoding="utf-8", newline="\n")
    print(f"{a.out}: {n} crates and {len(C_PARTS)} C libraries")
    return 0


if __name__ == "__main__":
    sys.exit(main())
