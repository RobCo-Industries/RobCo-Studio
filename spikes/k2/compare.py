#!/usr/bin/env python3
"""Spike K2: compare the hash lists from the CI jobs.

    compare.py DIR    DIR holds hashes-<os>/<variant>.txt files

Fails when:
- a cured build differs between two systems in any file but the full
  ELF, which carries libgcc's debug info,
- a CULVERT.HTP differs anywhere among the xPack builds,
- the make build and the raw direct build on Ubuntu differ in the ELF
  or the HTP.
Everything else is printed for the record.
"""

import os
import sys


def load(path):
    rows = {}
    with open(path) as f:
        for line in f:
            h, name = line.split()
            rows[name] = h
    return rows


def main():
    root = sys.argv[1]
    data = {}
    for d in sorted(os.listdir(root)):
        for f in sorted(os.listdir(os.path.join(root, d))):
            if f.endswith(".txt"):
                key = d.replace("hashes-", "") + "/" + f[:-4]
                data[key] = load(os.path.join(root, d, f))
    bad = []

    def diff(keys, label, strict, only=None):
        names = sorted(set().union(*(data[k] for k in keys)))
        for n in names:
            if only and n not in only:
                continue
            vals = {k: data[k].get(n) for k in keys}
            if len(set(vals.values())) > 1:
                print("DIFF %s %s: %s" % (label, n, vals))
                if strict:
                    bad.append((label, n))
            else:
                print("same %s %s" % (label, n))

    systems = sorted({k.split("/")[0] for k in data})
    print("systems:", systems)
    cured = [s + "/cured" for s in systems]
    # The full ELF keeps libgcc's debug info, which differs on Windows.
    diff(cured, "cured", False, only={"CULVERT.elf"})
    names = set().union(*(data[k] for k in cured)) - {"CULVERT.elf"}
    diff(cured, "cured", True, only=names)
    diff([s + "/raw" for s in systems], "raw", False)
    xpack = [k for k in data if not k.endswith("/distro")]
    diff(xpack, "all-xpack", True, only={"CULVERT.HTP"})
    if "ubuntu/make" in data:
        diff(["ubuntu/make", "ubuntu/raw"], "make-vs-direct", False)
        diff(["ubuntu/make", "ubuntu/raw"], "make-vs-direct", True,
             only={"CULVERT.elf", "CULVERT.HTP"})
    else:
        bad.append(("make", "missing"))
    if "ubuntu/distro" in data:
        diff(["ubuntu/distro", "ubuntu/cured"], "distro-vs-xpack", False,
             only={"CULVERT.elf", "CULVERT.HTP"})
    if bad:
        print("FAILED:", bad)
        sys.exit(1)
    print("OK")


if __name__ == "__main__":
    main()
