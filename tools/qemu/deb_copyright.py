"""Read a Debian copyright file for the licence texts it does not hold.

Prints each /usr/share/common-licenses/ file the copyright file points to,
one per line. In a machine-readable copyright file, fails when a licence is
named by its short name with neither its text nor such a pointer.

Usage: python3 deb_copyright.py <copyright-file>
"""

import re
import sys

COMMON = "/usr/share/common-licenses/"
POINTER = re.compile(re.escape(COMMON) + r"([A-Za-z0-9._+-]+)")
SPLIT = re.compile(r"\s+or\s+|\s+and\s+|\s*,\s*", re.IGNORECASE)


def pointers(text):
    """The common-licenses files the text names, in order, once each."""
    out = []
    for m in POINTER.finditer(text):
        name = m.group(1).rstrip(".,;:)")
        if name and name not in out:
            out.append(name)
    return out


def base(name):
    """A licence short name without its version suffix words."""
    n = name.strip().lower().rstrip("+")
    for suffix in ("-or-later", "-only"):
        if n.endswith(suffix):
            n = n[: -len(suffix)]
    return n


def same(name, pointer):
    a, b = base(name), base(pointer)
    return a == b or a == b + ".0" or b == a + ".0"


def names(value):
    """The short names in a License field's first line."""
    value = re.split(r"\s+with\s+", value, flags=re.IGNORECASE)[0]
    return [n.strip() for n in SPLIT.split(value) if n.strip()]


def undefined(text):
    """Short names used with no text and no pointer, in a DEP-5 file."""
    if not re.search(r"^Format:.*(copyright-format|dep5)", text, re.MULTILINE):
        return []
    used, defined = [], set()
    for para in re.split(r"\n\s*\n", text):
        lines = para.splitlines()
        for i, line in enumerate(lines):
            if not line.startswith("License:"):
                continue
            found = names(line[len("License:"):])
            body = []
            for cont in lines[i + 1:]:
                if not cont[:1].isspace():
                    break
                if cont.strip() not in ("", "."):
                    body.append(cont)
            if body:
                defined.update(n.lower() for n in found)
            used.extend(n for n in found if n not in used)
    known = pointers(text)
    return [n for n in used
            if n.lower() not in defined and not any(same(n, p) for p in known)]


def main(argv):
    with open(argv[1], encoding="utf-8", errors="replace") as f:
        text = f.read()
    missing = undefined(text)
    for name in pointers(text):
        print(COMMON + name)
    if missing:
        print(f"{argv[1]} names a licence with no text and no pointer: "
              + ", ".join(missing), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
