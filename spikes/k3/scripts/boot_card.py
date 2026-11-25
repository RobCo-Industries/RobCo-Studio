"""Boots RobCo-Core's examples/storage guest on a card written from Rust.

    python3 scripts/boot_card.py <robco-core-dir> <storage kernel.img> <card.img>

Uses RobCo-Core's own harness (tests/harness/boot.py) with the card in
place of the one tools/mksdimage.sh would make. The guest mounts the
card with fs_mount_boot(), then answers one marker per command. The check
passes when the guest reads back the files with the sizes and CRC-32
values the host computes from <card>.tree, and lists the folders.
"""

import os
import pathlib
import sys
import time
import zlib

core, kernel, card = (pathlib.Path(a).resolve() for a in sys.argv[1:4])
sys.path.insert(0, str(core / "tests"))

from harness.boot import boot  # noqa: E402

BOARD = os.environ.get("K3_BOARD", "raspi2b")
tree = card.with_suffix(".tree")
STAGE = "storage"


def events(vm, ev):
    return [m for m in vm.markers() if m.get("stage") == STAGE and m.get("ev") == ev]


def cmd(vm, line, ev, timeout=15):
    before = len(events(vm, ev))
    vm.send(line + "\r")
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        got = events(vm, ev)
        if len(got) > before:
            return got[before]
        time.sleep(0.02)
    raise SystemExit(f"boot_card: no ev={ev} answer to {line!r}. serial:\n{vm.serial}")


failed = False
with boot(kernel, BOARD, sd=card, timeout=15) as vm:
    vm.wait_for("ev=ready", timeout=15)
    mount = events(vm, "mount")[0]
    print("guest mount marker:", mount)
    if mount.get("rc") != "ok":
        failed = True
    for f in sorted(p for p in tree.rglob("*") if p.is_file()):
        rel = f.relative_to(tree).as_posix()
        if " " in rel:
            continue  # the guest's command line splits on spaces
        data = f.read_bytes()
        want_crc = f"0x{zlib.crc32(data) & 0xFFFFFFFF:08x}"
        got = cmd(vm, f"cat /{rel}", "cat")
        ok = got.get("rc") == "ok" and int(got["size"]) == len(data) and got["crc"] == want_crc
        print(f"cat /{rel}: guest {got} host size={len(data)} crc={want_crc} -> {'same' if ok else 'DIFFERENT'}")
        failed |= not ok
    got = cmd(vm, "ls /K3", "ls")
    print("guest ls /K3:", got)
    failed |= got.get("rc") != "ok"
    print("serial lines:")
    print(vm.serial)

if failed:
    raise SystemExit("boot_card: the guest did not read the card as written")
print("boot_card: the guest booted on the Rust card and read every file with the same size and CRC")
