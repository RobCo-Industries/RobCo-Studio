#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Write and check the reduced Homebrew install receipt a package carries.

  brew_receipt.py reduce <INSTALL_RECEIPT.json> <formula> <keg-version> <out>
      Writes to <out> the few fields of the keg's receipt that record what
      was packed, with a fixed key order. The full receipt names folders of
      the machine that installed it, such as Homebrew's cache under the
      home folder, so it is never packed.

  brew_receipt.py check <file> <formula> <keg-version> <details>
      Fails unless <file> holds exactly the fields reduce writes, with the
      formula and keg version of the record line, the tap_git_head its
      details field names, and no absolute path in any value.

The fields, and why each stays:
  formula, keg_version   the keg packed, which the record line also names
  tap, tap_git_head      the tap and the homebrew-core commit at install,
                         where the formula file of the recipe lives
  stable_version         the upstream version the formula built
  poured_from_bottle     whether Homebrew's bottle, not this machine,
                         built the library
  homebrew_version       the Homebrew that installed it
Standard library only.
"""

import json
import pathlib
import re
import sys

KEYS = ["formula", "keg_version", "tap", "tap_git_head", "stable_version",
        "poured_from_bottle", "homebrew_version"]
HEAD = re.compile(r"homebrew-core tap_git_head (\S+?),")
ABSOLUTE = re.compile(r"^(?:/|~|[A-Za-z]:[\\/])")


class Refused(Exception):
    """The reduced receipt breaks a rule. The message says which."""


def reduce(receipt, formula, version):
    """The reduced record of a full receipt, as JSON text."""
    r = json.loads(pathlib.Path(receipt).read_text(encoding="utf-8"))
    src = r.get("source") or {}
    rec = {
        "formula": formula,
        "keg_version": version,
        "tap": src.get("tap"),
        "tap_git_head": src.get("tap_git_head"),
        "stable_version": (src.get("versions") or {}).get("stable"),
        "poured_from_bottle": r.get("poured_from_bottle"),
        "homebrew_version": r.get("homebrew_version"),
    }
    check_values(rec)
    return json.dumps(rec, indent=2) + "\n"


def check_values(rec):
    for k, v in rec.items():
        if v is not None and not isinstance(v, (str, bool)):
            raise Refused(f"{k} is a {type(v).__name__}, not a string or a flag")
        if isinstance(v, str) and ABSOLUTE.match(v):
            raise Refused(f"{k} is a path of the machine: {v}")


def check(path, formula, version, details):
    """Refuses unless path holds the reduced receipt of this record line."""
    try:
        rec = json.loads(pathlib.Path(path).read_text(encoding="utf-8"))
    except (OSError, ValueError) as e:
        raise Refused(f"{path} is not a readable receipt: {e}") from e
    if not isinstance(rec, dict) or list(rec) != KEYS:
        raise Refused(f"{path} holds the fields {list(rec) if isinstance(rec, dict) else rec}, not {KEYS}")
    check_values(rec)
    if (rec["formula"], rec["keg_version"]) != (formula, version):
        raise Refused(f"{path} names {rec['formula']} {rec['keg_version']}, the record {formula} {version}")
    m = HEAD.search(details)
    head = m.group(1) if m else None
    if (rec["tap_git_head"] or "unknown") != head:
        raise Refused(f"{path} names tap_git_head {rec['tap_git_head']}, the record {head}")


def main(argv):
    try:
        if len(argv) == 5 and argv[0] == "reduce":
            text = reduce(argv[1], argv[2], argv[3])
            pathlib.Path(argv[4]).write_text(text, encoding="utf-8")
        elif len(argv) == 5 and argv[0] == "check":
            check(*argv[1:])
        else:
            print(__doc__, file=sys.stderr)
            return 2
    except Refused as e:
        print(f"FAIL: {e}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
