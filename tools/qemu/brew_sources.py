#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Fetch the formula file, source archive and patches of one Homebrew keg,
each checked against a SHA-256 that Homebrew itself states. Called by
sources.sh on macOS.

  brew_sources.py <brew-info.json> <keg-version> <dest-dir>

<brew-info.json> is the output of brew info --json=v2 --formula. Homebrew
installs from its API and keeps no copy of the core tap, so brew cat cannot
read the formula. Instead:

- the formula file comes from the public homebrew-core repository at the
  tap commit brew info names, at the path it names, and must have the
  SHA-256 of ruby_source_checksum;
- the full version, revision included, that brew info and the fetched
  formula state must be the keg's;
- the source archive comes from the stable URL and must have its checksum;
- each patch comes from its URL, or from the tap at the same commit, and
  must have the SHA-256 brew info states for it.

Runs no brew command, so it never turns on Homebrew's developer mode. Every
download goes through curl; the unit tests put a fake curl first on PATH.
"""

import hashlib
import json
import pathlib
import re
import subprocess
import sys
import urllib.parse

RAW = "https://raw.githubusercontent.com/Homebrew/homebrew-core"


class Refused(Exception):
    """A source cannot be fetched as packed. The message says why."""


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def fetch(url, dest, want, what):
    """Downloads url to dest and checks its SHA-256 against want."""
    if not want:
        raise Refused(f"Homebrew states no SHA-256 for {what}")
    r = subprocess.run(["curl", "-fsSLo", str(dest), url], check=False)
    if r.returncode != 0:
        raise Refused(f"cannot download {what} from {url}")
    got = sha256(dest)
    if got != want:
        dest.unlink()
        raise Refused(f"{what} from {url} has SHA-256 {got}, but Homebrew states {want}")


def stated_revision(rb):
    """The revision the formula file states, 0 when it states none. Only a
    line indented once counts: a resource block is indented twice."""
    m = re.search(r"^  revision (\d+)\s*$", rb, re.M)
    return int(m.group(1)) if m else 0


def full_version(stable, revision):
    return stable + (f"_{revision}" if revision else "")


def run(info_path, keg, dest):
    dest = pathlib.Path(dest)
    f = json.loads(pathlib.Path(info_path).read_text(encoding="utf-8"))["formulae"][0]
    name = f["name"]
    stable = (f.get("versions") or {}).get("stable")
    if not stable:
        raise Refused(f"brew info gave no version for {name}")
    fver = full_version(stable, f.get("revision") or 0)
    if fver != keg:
        raise Refused(f"the formula of {name} is {fver} now, but the keg packed is {keg}")
    if f.get("tap") != "homebrew/core":
        raise Refused(f"{name} comes from tap {f.get('tap')}, not homebrew/core")
    head, path = f.get("tap_git_head"), f.get("ruby_source_path")
    if not head or not path:
        raise Refused(f"brew info names no tap commit or formula path for {name}")

    rb = dest / f"{name}.rb"
    fetch(f"{RAW}/{head}/{path}", rb, (f.get("ruby_source_checksum") or {}).get("sha256"),
          f"the formula file {path} at {head}")
    text = rb.read_text(encoding="utf-8")
    url = f["urls"]["stable"]
    if url.get("using"):
        raise Refused(f"the source of {name} is fetched with {url['using']}, not as an archive")
    # The formula file must state the version and revision brew info gave:
    # the archive URL and its checksum, and the revision.
    rver = full_version(stable, stated_revision(text))
    if f'url "{url["url"]}"' not in text or f'sha256 "{url.get("checksum")}"' not in text:
        raise Refused(f"the fetched formula of {name} does not state the source {url['url']} of {fver}")
    if rver != keg:
        raise Refused(f"the fetched formula of {name} states {rver}, but the keg packed is {keg}")

    archive = urllib.parse.urlparse(url["url"]).path.rsplit("/", 1)[-1]
    if stable not in archive:
        raise Refused(f"source {archive} of {name} is not version {keg}")
    fetch(url["url"], dest / archive, url.get("checksum"), f"the source archive of {name}")

    sums = f.get("local_patch_checksums") or {}
    for p in f.get("patches") or []:
        if p.get("url"):
            fetch(p["url"], dest / p["url"].rsplit("/", 1)[-1], p.get("sha256"), f"a patch of {name}")
        elif p.get("file"):
            fetch(f"{RAW}/{head}/{p['file']}", dest / p["file"].rsplit("/", 1)[-1], sums.get(p["file"]),
                  f"the patch {p['file']} of {name}")
        else:
            raise Refused(f"a patch of {name} has neither a URL nor a file: {json.dumps(p)}")


def main(argv):
    if len(argv) != 3:
        print(__doc__, file=sys.stderr)
        return 2
    try:
        run(*argv)
    except Refused as e:
        print(e, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
