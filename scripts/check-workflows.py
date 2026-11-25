#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Fail when a workflow file holds the same key twice in one mapping.

A YAML parser keeps the last of two equal keys without a word, so a merge
that joins two jobs of one name drops one of them. Git merges such a file
without a conflict. This check reads the block-style YAML that workflows
use, line by line, with no parser to install.

Run it from the repository root. It exits 1 when a key repeats.
"""

import pathlib
import re
import sys

KEY = re.compile(r"""^(\s*)(- +)?("[^"]*"|'[^']*'|[^\s#'"{\[-][^:#]*?)\s*:(\s|$)""")
BLOCK = re.compile(r":\s*[|>][-+0-9]*\s*(#.*)?$")


def check(path):
    errors = []
    # Each entry: (indent of the mapping, keys seen in it).
    stack = []
    block_indent = None
    for n, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if not raw.strip() or raw.lstrip().startswith("#"):
            continue
        indent = len(raw) - len(raw.lstrip())
        if block_indent is not None:
            if indent > block_indent:
                continue
            block_indent = None
        m = KEY.match(raw)
        if not m:
            continue
        key_indent = indent + (len(m.group(2)) if m.group(2) else 0)
        # A list item opens a new mapping, even at the indent of the last one.
        while stack and (stack[-1][0] > key_indent or (m.group(2) and stack[-1][0] >= key_indent)):
            stack.pop()
        if not stack or stack[-1][0] < key_indent:
            stack.append((key_indent, {}))
        key = m.group(3).strip("'\"")
        seen = stack[-1][1]
        if key in seen:
            errors.append(f"{path}:{n}: key {key!r} repeats the one at line {seen[key]}")
        else:
            seen[key] = n
        if BLOCK.search(raw):
            block_indent = key_indent
    return errors


def main():
    files = sorted(pathlib.Path(".github/workflows").glob("*.y*ml"))
    if not files:
        print("::error::no workflow files under .github/workflows")
        return 1
    errors = [e for f in files for e in check(f)]
    for e in errors:
        print(f"::error::{e}")
    print(f"checked {len(files)} workflow files, {len(errors)} repeated keys")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
