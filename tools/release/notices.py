#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Write the third-party notice file of the robco program.

Usage: notices.py --target TRIPLE --out FILE [--tree FILE] [--registry DIR]

The crates come from the locked dependency tree of the release target:
`cargo tree --locked --offline -p robco` names the packages compiled into
robco, each with the licence its own Cargo.toml declares. Build and test
dependencies and procedural macros are not in the program. cargo tree
reads only the lock file, the registry index and the manifests of robco's
own tree for that target, which `cargo build -p robco` has fetched, so no
crate of another workspace member is needed. Each crate's licence texts
are copied from its unpacked source under the cargo home, in
registry/src/<index>/<name>-<version>. No network is used.
The C that robco links from RobCo-Core, and FatFs inside it, comes last.

With --tree the script reads that file in place of running cargo, and
with --registry it looks for sources in DIR/<index>/, for the tests.

Exits 1 when a crate in the program names no licence or has no source.
"""

import argparse
import os
import pathlib
import re
import subprocess
import sys

ROOT = "robco"
TEXT = re.compile(r"^(licen[cs]e|copying|notice|unlicense)([-_.].*)?$", re.I)
# One line of the tree: name, version, an optional "(source)", then the
# licence and the repository, split by tabs.
LINE = re.compile(r"^(\S+) v(\S+)(?: \((.*?)\))?(?: \(\*\))?\t(.*)\t(.*)$")

# The C compiled into the program by studio-sys, relative to the RobCo-Core
# submodule: name, licence and licence file.
C_PARTS = [
    ("RobCo-Core", "MIT", "LICENSE"),
    ("FatFs", "BSD-1-Clause", "third_party/fatfs/LICENSE.txt"),
]


def tree(target):
    """The packages in the program, as `cargo tree` lists them: robco and
    its normal dependencies, with the features robco alone turns on, and
    without procedural macros, which run in the compiler only."""
    out = subprocess.run(
        ["cargo", "tree", "--locked", "--offline", "-p", ROOT, "-e", "normal,no-proc-macro",
         "--target", target, "--prefix", "none", "-f", "{p}\t{l}\t{r}"],
        check=True, capture_output=True, text=True)
    return out.stdout


def registry():
    home = os.environ.get("CARGO_HOME") or str(pathlib.Path.home() / ".cargo")
    return pathlib.Path(home) / "registry" / "src"


def shipped(tree_text, reg):
    """The third-party packages of the tree, sorted, as (name, version,
    licence, repository, source folder or None). A package with a path
    source is a workspace member, robco itself, under GPL-3.0-or-later."""
    seen, crates, root = set(), [], False
    for line in tree_text.splitlines():
        if not line.strip():
            continue
        m = LINE.match(line)
        if not m:
            raise SystemExit(f"error: cannot read the tree line {line!r}")
        name, version, source, lic, repo = m.groups()
        if name == ROOT:
            root = True
        if source is not None:
            if source.startswith(("registry+", "git+", "sparse+")):
                raise SystemExit(f"error: {name} {version} comes from {source}, not crates.io")
            continue
        if (name, version) in seen:
            continue
        seen.add((name, version))
        found = sorted(d for d in reg.glob(f"*/{name}-{version}") if d.is_dir())
        crates.append((name, version, lic, repo, found[0] if found else None))
    if not root:
        raise SystemExit(f"error: {ROOT} is not in the dependency tree")
    return sorted(crates)


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


def build(tree_text, core, reg):
    out = [
        "Third-party notices for robco",
        "",
        "robco is free software under GPL-3.0-or-later. It contains the",
        "following works of others, each under its own licence, listed below",
        "with the licence texts each work ships.",
        "",
    ]
    errors = []
    crates = shipped(tree_text, reg)
    for name, version, lic, repo, folder in crates:
        if not lic:
            errors.append(f"FAIL: {name} {version} names no licence")
            lic = "none declared"
        if folder is None:
            errors.append(f"FAIL: {name} {version}: no source under {reg}")
            continue
        title = f"Rust crate {name} {version}" + (f", {repo}" if repo else "")
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
    ap.add_argument("--registry")
    ap.add_argument("--tree")
    ap.add_argument("--core", default="external/robco-core")
    a = ap.parse_args(argv)
    tree_text = pathlib.Path(a.tree).read_text() if a.tree else tree(a.target)
    reg = pathlib.Path(a.registry) if a.registry else registry()
    text, n, errors = build(tree_text, pathlib.Path(a.core), reg)
    for e in errors:
        print(e, file=sys.stderr)
    if errors:
        return 1
    pathlib.Path(a.out).write_text(text, encoding="utf-8", newline="\n")
    print(f"{a.out}: {n} crates and {len(C_PARTS)} C libraries")
    return 0


if __name__ == "__main__":
    sys.exit(main())
