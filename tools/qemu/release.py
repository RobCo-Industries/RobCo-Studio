#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Gather the files of a QEMU release and write the list of their sizes and
SHA-256. Standard library only, no network.

  release.py scan <package-dir> [--forbid TEXT] [--forbid-env NAME] [--forbid-host]
      Fails when a file of the package holds one of the texts: the paths,
      user and host names of the machine that built it. --forbid-env reads
      a text from the environment, where no shell can rewrite it, and
      --forbid-host adds this machine's host name. Each path is searched
      with either slash; a path with a drive letter in each of its
      spellings, C:\\x, C:/x, /c/x and /cygdrive/c/x, with the drive in
      either case and without regard to case. Paths also compare without
      case on macOS. Each spelling and the host name are searched in UTF-8
      and in UTF-16 little endian. Each hit is followed by up to five lines
      "  at byte N: <text>", the printable string around it.

  release.py gather --in DIR --out DIR --repo DIR [--tag TAG] [--base-url URL]
      Reads the workflow's artifacts from --in:
        qemu-<os>/        the package of pack.sh, for os linux, macos, windows
        sources-<os>/     the library sources of sources.sh
        report-<os>/report.txt   the pack report, for the configure line
        qemu-source/qemu-<version>.tar.xz
      Before packing a host, each file of bin/ and lib/ must be an ELF,
      Mach-O or PE program of the machine the host name states: x64 is
      x86_64, arm64 is arm64.
      Writes into --out, for each host of HOSTS:
        qemu-<version>-<host>.tar.gz, or .zip on Windows: the package
            under the folder qemu-<version>-<host>/, which is the root
        qemu-<version>-<host>-notices.tar.gz: its NOTICE and licenses/
        qemu-<version>-<host>-library-sources.tar: its library sources
      and once:
        qemu-<version>.tar.xz: QEMU's source archive, unchanged
        qemu-<version>-recipe.tar.gz: tools/qemu, the workflow, the terms
            page and the configure line of each host
        qemu-<version>-files.json: every file above with its size and
            SHA-256, and per host the archive object the index names
        qemu-<version>-SHA256SUMS: the same hashes, for sha256sum -c
      With --tag, the tag must be qemu-<version>-r<n> for the version
      packed. With --base-url, each url is that URL and the file name;
      without, the url is the file name, as a local index has it.

  release.py verify <folder>
      Fails unless the list names exactly the files of the folder, with
      their sizes and SHA-256, SHA256SUMS agrees, each archive holds
      bin/, NOTICE and licenses/ under its root, and each library-sources
      archive holds exactly the files its SOURCES.txt lists, with their
      SHA-256.
"""

import argparse
import gzip
import hashlib
import io
import json
import os
import pathlib
import re
import socket
import sys
import tarfile
import zipfile

# The workflow's system name to the host name of the index, and the kind
# of archive. The host name states the architecture; ARCH says which
# program format and machine that is, and check_arch holds every packed
# program to it.
HOSTS = {
    "linux": ("linux-x64", "tar.gz"),
    "macos": ("macos-arm64", "tar.gz"),
    "windows": ("windows-x64", "zip"),
}
ARCH = {
    "linux-x64": ("ELF", "x86_64"),
    "macos-arm64": ("Mach-O", "arm64"),
    "windows-x64": ("PE", "x86_64"),
}
ELF_MACHINE = {0x03: "i386", 0x28: "arm", 0x3E: "x86_64", 0xB7: "arm64"}
MACHO_CPU = {7: "i386", 0x01000007: "x86_64", 12: "arm", 0x0100000C: "arm64"}
PE_MACHINE = {0x014C: "i386", 0x01C4: "arm", 0x8664: "x86_64", 0xAA64: "arm64"}
TAG = re.compile(r"^qemu-(\d+\.\d+\.\d+)-r([1-9][0-9]*)$")
NOTICE_HEAD = re.compile(r"^QEMU (\d+\.\d+\.\d+) for Arm")
NOTICE_SHA = re.compile(r"SHA-256 ([0-9a-f]{64})\.")
EPOCH = 315532800  # 1980-01-01, the earliest time a zip entry can hold
RECIPE = ["tools/qemu", ".github/workflows/qemu.yml", "docs/THIRD_PARTY.md"]


class Refused(Exception):
    """A rule of the release is broken. The message says which."""


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


# A Windows path in any of its spellings: C:\\x, C:/x, /c/x, /cygdrive/c/x.
DRIVE = re.compile(r"^(?:([A-Za-z]):(?=[\\/]|$)|/cygdrive/([A-Za-z])(?=/|$)|/([A-Za-z])(?=/|$))(.*)$", re.S)
# Whether this system's file names compare without case: Windows and the
# default macOS file system do, Linux does not.
CASELESS = sys.platform in ("win32", "cygwin", "darwin")


def variants(text):
    """The spellings of a path that a file may hold, as (spelling, caseless)
    pairs. A path with a drive letter is spelt C:\\x, C:/x, C:\\\\x as a C or
    JSON string holds it, and /c/x, which /cygdrive/c/x contains. It
    compares without case, so the drive letter may be in either case. Any
    other path is spelt with either slash."""
    m = DRIVE.match(text)
    if not m:
        return [(t, False) for t in [text] + sorted({text.replace("/", "\\"), text.replace("\\", "/")} - {text})]
    d = next(g for g in m.groups()[:3] if g)
    rest = m.group(4).replace("\\", "/")
    win = f"{d}:{rest}"
    out = {win, win.replace("/", "\\"), win.replace("/", "\\\\"), f"/{d}{rest}"}
    out.discard(text)
    return [(t, True) for t in [text] + sorted(out)]


def needles(forbid, caseless=CASELESS, hosts=()):
    """The byte strings to search for: (group, label, bytes, caseless) for each
    spelling of each forbidden path, in UTF-8 and in UTF-16 little endian,
    and each host name as it is, in both. A caseless needle is lower case
    and is searched in the lower-cased file. Refuses a text shorter than
    4 characters: it would match everywhere and mean nothing."""
    out, seen = [], set()

    def add(group, label, text, nocase):
        for enc, tag in (("utf-8", ""), ("utf-16-le", " in UTF-16")):
            b = text.encode(enc)
            if nocase:
                b = b.lower()
            if (b, nocase) not in seen:
                seen.add((b, nocase))
                out.append((group, label + tag, b, nocase))

    for t in list(forbid) + list(hosts):
        if len(t) < 4:
            raise Refused(f"forbidden text {t!r} is shorter than 4 characters")
    for i, t in enumerate(forbid):
        for v, drive in variants(t):
            add(i, v, v, drive or caseless)
    for i, h in enumerate(hosts, len(forbid)):
        add(i, h, h, False)
    return out


def files_of(root):
    """Every file under root, sorted by its relative name. Refuses links and
    anything that is neither a file nor a folder."""
    root = pathlib.Path(root)
    out = []
    for p in sorted(root.rglob("*")):
        rel = p.relative_to(root).as_posix()
        if p.is_symlink():
            raise Refused(f"{rel} is a symbolic link")
        if p.is_file():
            out.append((rel, p))
        elif not p.is_dir():
            raise Refused(f"{rel} is not a regular file")
    return out


def context(path, needle, nocase=False, limit=5, width=100):
    """Where needle, a byte string, sits in a file: up to limit (offset,
    string) pairs, the string being the run of printable characters around
    each occurrence, as strings -t d shows it, cut to width characters on
    each side. A needle with zero bytes is read as UTF-16."""
    data = pathlib.Path(path).read_bytes()
    if isinstance(needle, str):
        needle = needle.encode()
    hay = data.lower() if nocase else data
    step = 2 if b"\x00" in needle else 1

    def printable(at):
        if at < 0 or at + step > len(data) or not 0x20 <= data[at] < 0x7F:
            return False
        return step == 1 or data[at + 1] == 0

    out, at = [], hay.find(needle)
    while at >= 0 and len(out) < limit:
        lo, hi = at, at + len(needle)
        while (at - lo) // step < width and printable(lo - step):
            lo -= step
        while (hi - at - len(needle)) // step < width and printable(hi):
            hi += step
        enc = "utf-16-le" if step == 2 else "ascii"
        out.append((at, data[lo:hi].decode(enc, "replace")))
        at = hay.find(needle, at + 1)
    return out


def scan(package, forbid, caseless=CASELESS, hosts=()):
    """The (file, label, needle, caseless) hits: in each file, the first
    spelling found of each forbidden text."""
    ns = needles(forbid, caseless, hosts)
    hits = []
    for rel, p in files_of(package):
        data = p.read_bytes()
        low = data.lower() if any(n[3] for n in ns) else data
        found = set()
        for group, label, n, nocase in ns:
            if group not in found and n in (low if nocase else data):
                found.add(group)
                hits.append((rel, label, n, nocase))
    return hits


def arch_of(data):
    """The (format, machines) a program's header states, machines being a
    sorted tuple: one name, or the slices of a universal Mach-O. A machine
    code not known here is named by its number. Refuses a file whose header
    cannot be read."""
    def num(at, size, order):
        if len(data) < at + size:
            raise ValueError("the header is cut short")
        return int.from_bytes(data[at:at + size], order)

    try:
        if data[:4] == b"\x7fELF":
            order = {1: "little", 2: "big"}.get(num(5, 1, "little"))
            if order is None:
                raise ValueError("the ELF byte order is unknown")
            m = num(18, 2, order)
            return "ELF", (ELF_MACHINE.get(m, hex(m)),)
        if data[:4] in (b"\xcf\xfa\xed\xfe", b"\xce\xfa\xed\xfe"):
            c = num(4, 4, "little")
            return "Mach-O", (MACHO_CPU.get(c, hex(c)),)
        if data[:4] == b"\xca\xfe\xba\xbe":
            n = num(4, 4, "big")
            if not 0 < n < 16:
                raise ValueError("not a universal Mach-O")
            cpus = [num(8 + 20 * i, 4, "big") for i in range(n)]
            return "Mach-O", tuple(sorted(MACHO_CPU.get(c, hex(c)) for c in cpus))
        if data[:2] == b"MZ":
            pe = num(0x3C, 4, "little")
            if data[pe:pe + 4] != b"PE\0\0":
                raise ValueError("no PE signature")
            m = num(pe + 4, 2, "little")
            return "PE", (PE_MACHINE.get(m, hex(m)),)
    except ValueError as e:
        raise Refused(str(e)) from None
    raise Refused("it is not an ELF, Mach-O or PE program")


def check_arch(programs, host, where):
    """Checks that each (name, bytes) of programs, the files of bin/ and
    lib/, is a program of the format and machine the host name states. A
    universal Mach-O passes when it holds that machine."""
    fmt, machine = ARCH[host]
    for name, data in programs:
        try:
            found_fmt, found = arch_of(data)
        except Refused as e:
            raise Refused(f"{where}/{name}: cannot read its architecture, {e}; "
                          f"{host} expects {fmt} {machine}") from None
        if found_fmt != fmt or machine not in found:
            raise Refused(f"{where}/{name} is {found_fmt} {'+'.join(found)}, "
                          f"but {host} expects {fmt} {machine}")


def is_program(rel):
    return rel.startswith("bin/") or rel.startswith("lib/")


def mode_of(rel):
    return 0o755 if rel.startswith("bin/") else 0o644


def tar_bytes(entries, compress):
    """A tar archive of (name, bytes, mode), with fixed owners and times so
    that the same input gives the same bytes."""
    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode="w", format=tarfile.PAX_FORMAT) as tar:
        dirs = set()
        for name, _, _ in entries:
            parts = name.split("/")[:-1]
            for i in range(1, len(parts) + 1):
                dirs.add("/".join(parts[:i]))
        for d in sorted(dirs):
            info = tarfile.TarInfo(d)
            info.type, info.mode, info.mtime = tarfile.DIRTYPE, 0o755, EPOCH
            tar.addfile(info)
        for name, data, mode in entries:
            info = tarfile.TarInfo(name)
            info.size, info.mode, info.mtime = len(data), mode, EPOCH
            tar.addfile(info, io.BytesIO(data))
    if not compress:
        return raw.getvalue()
    gz = io.BytesIO()
    with gzip.GzipFile(fileobj=gz, mode="wb", mtime=EPOCH, filename="") as f:
        f.write(raw.getvalue())
    return gz.getvalue()


def zip_bytes(entries):
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED) as z:
        for name, data, mode in entries:
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.create_system = 3
            info.external_attr = (0o100000 | mode) << 16
            z.writestr(info, data)
    return buf.getvalue()


def tree(src, root, only=None, mode=mode_of):
    """(name under root, bytes, mode) for each file of src."""
    out = []
    for rel, p in files_of(src):
        if only and not any(rel == o or rel.startswith(o + "/") for o in only):
            continue
        out.append((f"{root}/{rel}", p.read_bytes(), mode(rel)))
    return out


def package_version(package):
    """The QEMU version and source SHA-256 the package's NOTICE states, after
    checking the package holds bin/, NOTICE and licenses/."""
    package = pathlib.Path(package)
    for need in ("bin", "licenses"):
        if not (package / need).is_dir() or not any((package / need).iterdir()):
            raise Refused(f"{package} has no {need}/")
    notice = package / "NOTICE"
    if not notice.is_file():
        raise Refused(f"{package} has no NOTICE")
    text = notice.read_text(encoding="utf-8")
    head, sha = NOTICE_HEAD.search(text), NOTICE_SHA.search(text)
    if not head or not sha:
        raise Refused(f"{notice} names no QEMU version and source SHA-256")
    return head.group(1), sha.group(1)


def configure_line(report):
    for line in pathlib.Path(report).read_text(encoding="utf-8").splitlines():
        if line.startswith("configure: "):
            return line + "\n"
    raise Refused(f"{report} holds no configure line")


def check_sources(listing, files, where):
    """Checks that SOURCES.txt, as text, lists exactly the files, a dict of
    name to bytes, each with its SHA-256."""
    listed = {}
    for line in listing.splitlines():
        if not line or line.startswith("#"):
            continue
        parts = line.split(" | ")
        if len(parts) != 4:
            raise Refused(f"{where}/SOURCES.txt has a line that is not package | version | file | sha256: {line!r}")
        listed[parts[2]] = parts[3]
    present = set(files)
    if set(listed) != present:
        raise Refused(f"{where}/SOURCES.txt does not list exactly its files: "
                      f"unlisted {sorted(present - set(listed))}, missing {sorted(set(listed) - present)}")
    if not present:
        raise Refused(f"{where} holds no library source")
    for name in sorted(present):
        if hashlib.sha256(files[name]).hexdigest() != listed[name]:
            raise Refused(f"{where}/{name} does not match its SHA-256 in SOURCES.txt")


def library_sources(path):
    """Checks a library-sources archive against the SOURCES.txt inside it."""
    root = path.name[:-len(".tar")]
    files, listing = {}, None
    with tarfile.open(path, "r:") as t:
        for m in t.getmembers():
            if m.isdir():
                continue
            if not m.isfile() or not m.name.startswith(root + "/") or ".." in m.name.split("/"):
                raise Refused(f"{path.name} holds {m.name}, which is not a file under {root}/")
            data = t.extractfile(m).read()
            rel = m.name[len(root) + 1:]
            if rel == "SOURCES.txt":
                listing = data.decode("utf-8")
            else:
                files[rel] = data
    if listing is None:
        raise Refused(f"{path.name} holds no {root}/SOURCES.txt")
    check_sources(listing, files, path.name)


def gather(src, out, repo, tag=None, base_url=None):
    src, out, repo = pathlib.Path(src), pathlib.Path(out), pathlib.Path(repo)
    out.mkdir(parents=True, exist_ok=True)
    if any(out.iterdir()):
        raise Refused(f"{out} is not empty")

    found = {os_: package_version(src / f"qemu-{os_}") for os_ in HOSTS}
    if len(set(found.values())) != 1:
        raise Refused(f"the packages state different QEMU builds: {found}")
    version, qsha = next(iter(found.values()))
    if tag is not None:
        m = TAG.match(tag)
        if not m:
            raise Refused(f"tag {tag!r} is not of the form qemu-<version>-r<n>")
        if m.group(1) != version:
            raise Refused(f"tag {tag} names QEMU {m.group(1)}, but {version} was packed")

    qsrc = src / "qemu-source" / f"qemu-{version}.tar.xz"
    if not qsrc.is_file():
        raise Refused(f"no QEMU source archive {qsrc}")
    if sha256(qsrc) != qsha:
        raise Refused(f"{qsrc.name} is not the archive the packages were built from")

    for os_, (host, _) in HOSTS.items():
        package = src / f"qemu-{os_}"
        check_arch([(rel, p.read_bytes()) for rel, p in files_of(package) if is_program(rel)],
                   host, str(package))

    written = {}

    def put(name, data):
        (out / name).write_bytes(data)
        written[name] = out / name

    archives = {}
    recipe_extra = []
    for os_, (host, kind) in HOSTS.items():
        package = src / f"qemu-{os_}"
        root = f"qemu-{version}-{host}"
        entries = tree(package, root)
        name = f"{root}.{kind}"
        put(name, zip_bytes(entries) if kind == "zip" else tar_bytes(entries, True))
        archives[host] = (name, root)
        put(f"{root}-notices.tar.gz",
            tar_bytes(tree(package, f"{root}-notices", only=["NOTICE", "licenses"]), True))
        sources = src / f"sources-{os_}"
        if not (sources / "SOURCES.txt").is_file():
            raise Refused(f"{sources} holds no SOURCES.txt")
        check_sources((sources / "SOURCES.txt").read_text(encoding="utf-8"),
                      {rel: p.read_bytes() for rel, p in files_of(sources) if rel != "SOURCES.txt"},
                      str(sources))
        put(f"{root}-library-sources.tar",
            tar_bytes(tree(sources, f"{root}-library-sources", mode=lambda _: 0o644), False))
        recipe_extra.append((f"qemu-{version}-recipe/configure-{host}.txt",
                             configure_line(src / f"report-{os_}" / "report.txt").encode(), 0o644))

    put(qsrc.name, qsrc.read_bytes())
    recipe = []
    for part in RECIPE:
        p = repo / part
        if p.is_dir():
            recipe += tree(p, f"qemu-{version}-recipe/{part}",
                           mode=lambda rel: 0o755 if rel.endswith(".sh") or rel.endswith(".py") else 0o644)
        elif p.is_file():
            recipe.append((f"qemu-{version}-recipe/{part}", p.read_bytes(), 0o644))
        else:
            raise Refused(f"the recipe needs {part}, which is not in {repo}")
    put(f"qemu-{version}-recipe.tar.gz", tar_bytes(recipe + recipe_extra, True))

    def url(name):
        return (base_url.rstrip("/") + "/" + name) if base_url else name

    files = [{"name": n, "url": url(n), "size": p.stat().st_size, "sha256": sha256(p)}
             for n, p in sorted(written.items())]
    by_name = {f["name"]: f for f in files}
    listing = {
        "name": "qemu",
        "version": version,
        "tag": tag,
        "archives": {host: {"url": url(n), "size": by_name[n]["size"],
                            "sha256": by_name[n]["sha256"], "root": root}
                     for host, (n, root) in archives.items()},
        "files": files,
    }
    lst = f"qemu-{version}-files.json"
    put(lst, (json.dumps(listing, indent=2) + "\n").encode())
    sums = "".join(f"{sha256(p)}  {n}\n" for n, p in sorted(written.items()))
    (out / f"qemu-{version}-SHA256SUMS").write_text(sums, encoding="utf-8")
    return version


def members(path, read=None):
    """The member names of a tool archive, told apart by its first bytes as
    the index's reader does. With read, a function of the name, also the
    bytes of each file member it is true for, as a list of (name, bytes)."""
    head = path.read_bytes()[:4]
    if head == b"PK\x03\x04":
        with zipfile.ZipFile(path) as z:
            names = z.namelist()
            data = [(n, z.read(n)) for n in names if read and not n.endswith("/") and read(n)]
    elif head[:2] == b"\x1f\x8b":
        with tarfile.open(path, "r:gz") as t:
            names = t.getnames()
            data = [(m.name, t.extractfile(m).read()) for m in t.getmembers()
                    if read and m.isfile() and read(m.name)]
    else:
        raise Refused(f"{path.name} is neither a zip nor a gzip archive")
    return (names, data) if read else names


def verify(folder):
    """Checks a gathered release: the list names exactly the files beside it
    with their sizes and hashes, SHA256SUMS agrees, and each archive holds
    bin/ with both programs, NOTICE and licenses/ at its root."""
    folder = pathlib.Path(folder)
    lists = sorted(folder.glob("qemu-*-files.json"))
    if len(lists) != 1:
        raise Refused(f"{folder} holds {len(lists)} lists, not one")
    listing = json.loads(lists[0].read_text(encoding="utf-8"))
    version = listing["version"]
    sums_name = f"qemu-{version}-SHA256SUMS"
    present = {p.name for p in folder.iterdir()} - {lists[0].name, sums_name}
    listed = {f["name"] for f in listing["files"]}
    if present != listed:
        raise Refused(f"the list does not name exactly the files: unlisted {sorted(present - listed)}, "
                      f"missing {sorted(listed - present)}")
    for f in listing["files"]:
        p = folder / f["name"]
        if p.stat().st_size != f["size"] or sha256(p) != f["sha256"]:
            raise Refused(f"{f['name']} does not match its size and SHA-256 in the list")
    want = "".join(f"{sha256(folder / n)}  {n}\n" for n in sorted(present | {lists[0].name}))
    if (folder / sums_name).read_text(encoding="utf-8") != want:
        raise Refused(f"{sums_name} does not match the files")
    if set(listing["archives"]) != {h for h, _ in HOSTS.values()}:
        raise Refused(f"the list has archives for {sorted(listing['archives'])}")
    by_url = {f["url"]: f for f in listing["files"]}
    for host, a in listing["archives"].items():
        f = by_url.get(a["url"])
        if not f or f["size"] != a["size"] or f["sha256"] != a["sha256"]:
            raise Refused(f"the archive object of {host} does not match a listed file")
        r = a["root"] + "/" if a["root"] else ""
        all_names, programs = members(folder / f["name"],
                                      read=lambda n: n.startswith(r) and is_program(n[len(r):]))
        names = set(all_names)
        check_arch([(n[len(r):], d) for n, d in programs], host, f["name"])
        exe = ".exe" if host.startswith("windows") else ""
        for need in (f"bin/qemu-system-arm{exe}", f"bin/qemu-system-aarch64{exe}", "NOTICE",
                     "licenses/qemu-COPYING", "licenses/packed-libraries.txt"):
            if r + need not in names:
                raise Refused(f"{f['name']} holds no {r}{need}")
        outside = [n for n in names
                   if not (n.startswith(r) or n.rstrip("/") == a["root"]) or ".." in n.split("/")]
        if outside:
            raise Refused(f"{f['name']} holds entries outside its root: {outside[:3]}")
    for host, _ in HOSTS.values():
        name = f"qemu-{version}-{host}-library-sources.tar"
        if name not in listed:
            raise Refused(f"the list names no {name}")
        library_sources(folder / name)
    return version


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("scan")
    s.add_argument("package")
    s.add_argument("--forbid", action="append", default=[])
    s.add_argument("--forbid-env", action="append", default=[], metavar="NAME")
    s.add_argument("--forbid-host", action="store_true")
    g = sub.add_parser("gather")
    g.add_argument("--in", dest="src", required=True)
    g.add_argument("--out", required=True)
    g.add_argument("--repo", required=True)
    g.add_argument("--tag")
    g.add_argument("--base-url")
    v = sub.add_parser("verify")
    v.add_argument("folder")
    a = ap.parse_args(argv)
    try:
        if a.cmd == "verify":
            print(f"The release of QEMU {verify(a.folder)} matches its list.")
        elif a.cmd == "scan":
            forbid = list(a.forbid)
            for name in a.forbid_env:
                if not os.environ.get(name):
                    raise Refused(f"the environment variable {name} is not set")
                forbid.append(os.environ[name])
            hosts = [socket.gethostname()] if a.forbid_host else []
            if not forbid and not hosts:
                raise Refused("nothing to search for")
            hits = scan(a.package, forbid, hosts=hosts)
            for rel, label, n, nocase in hits:
                print(f"FAIL: {rel} holds {label}")
                for at, around in context(pathlib.Path(a.package) / rel, n, nocase):
                    print(f"  at byte {at}: {around}")
            if hits:
                return 1
            print(f"No file of {a.package} holds a path or name of this machine.")
        else:
            v = gather(a.src, a.out, a.repo, a.tag, a.base_url)
            for p in sorted(pathlib.Path(a.out).iterdir()):
                print(f"{p.stat().st_size:>12}  {p.name}")
            print(f"Gathered the release of QEMU {v}.")
    except Refused as e:
        print(f"FAIL: {e}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
