#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Check the pins CI enforces.

1. `rust-toolchain.toml` names one exact stable release, and every workspace
   manifest states the same `rust-version`, directly or through the workspace.
2. The RobCo-Core submodule at `external/robco-core` is declared in
   `.gitmodules`, is checked out at the commit HEAD records, and that commit
   is on RobCo-Core's `main`. A missing submodule fails.

Run it from the repository root, with the submodule checked out. It exits 1
when any rule is broken.
"""

import os
import pathlib
import re
import subprocess
import sys
import tomllib

ROOT = pathlib.Path.cwd()
CORE_REMOTE = "origin"
CORE_PATH = "external/robco-core"
CORE_BRANCH = "main"


def load(path):
    with open(path, "rb") as f:
        return tomllib.load(f)


def check_toolchain(errors):
    channel = load(ROOT / "rust-toolchain.toml").get("toolchain", {}).get("channel")
    if not isinstance(channel, str) or not re.fullmatch(r"\d+\.\d+\.\d+", channel):
        errors.append(f"rust-toolchain.toml: channel must be an exact release such as 1.2.3, found {channel!r}")
        return
    root = load(ROOT / "Cargo.toml")
    ws = root.get("workspace", {})
    ws_version = ws.get("package", {}).get("rust-version")
    if ws_version != channel:
        errors.append(f"Cargo.toml: workspace rust-version is {ws_version!r}, rust-toolchain.toml says {channel!r}")
    members = ws.get("members", [])
    if not members:
        errors.append("Cargo.toml: the workspace lists no members")
    for member in members:
        manifest = ROOT / member / "Cargo.toml"
        if not manifest.is_file():
            errors.append(f"{member}: no Cargo.toml")
            continue
        value = load(manifest).get("package", {}).get("rust-version")
        if value == {"workspace": True}:
            continue
        if value != channel:
            errors.append(f"{member}/Cargo.toml: rust-version is {value!r}, rust-toolchain.toml says {channel!r}")
    print(f"toolchain: {channel}, checked {len(members)} workspace members")


# The script reads a real repository, so it keeps the repository's own
# configuration. It never prompts for credentials or a passphrase.
GIT_ENV = {**os.environ, "GIT_TERMINAL_PROMPT": "0", "GIT_SSH_COMMAND": "ssh -o BatchMode=yes"}


def git(*args, cwd=ROOT):
    return subprocess.run(["git", *args], cwd=cwd, capture_output=True, text=True, env=GIT_ENV)


def check_core_submodule(errors):
    declared = git("config", "-f", ".gitmodules", "--get-regexp", r"^submodule\..*\.path$")
    paths = declared.stdout.split()[1::2] if declared.returncode == 0 else []
    if CORE_PATH not in paths:
        errors.append(f".gitmodules does not declare a submodule at {CORE_PATH}")
        return
    tree = git("ls-tree", "HEAD", CORE_PATH).stdout.split()
    if len(tree) < 3 or tree[1] != "commit":
        errors.append(f"{CORE_PATH} is declared in .gitmodules but HEAD holds no submodule commit there")
        return
    pinned = tree[2]
    head = git("rev-parse", "HEAD", cwd=ROOT / CORE_PATH) if (ROOT / CORE_PATH / ".git").exists() else None
    if head is None or head.returncode != 0:
        errors.append(f"{CORE_PATH} is not checked out. Run `git submodule update --init`.")
        return
    if head.stdout.strip() != pinned:
        errors.append(f"{CORE_PATH}: checked out {head.stdout.strip()}, HEAD records {pinned}")
        return
    sub = ROOT / CORE_PATH
    # A shallow clone, as CI's checkout makes, cannot tell an ancestor:
    # fetch the whole history of main.
    shallow = git("rev-parse", "--is-shallow-repository", cwd=sub).stdout.strip() == "true"
    depth = ["--unshallow"] if shallow else []
    # A url.<base>.insteadOf rule would send the fetch elsewhere, such as to
    # SSH under the contributor's own identity. Do not fetch then.
    declared_url = git("config", "--get", f"remote.{CORE_REMOTE}.url", cwd=sub).stdout.strip()
    used_url = git("remote", "get-url", CORE_REMOTE, cwd=sub).stdout.strip()
    if declared_url != used_url:
        errors.append(
            f"{CORE_PATH}: could not tell whether {pinned} is on {CORE_BRANCH}: the fetch did not run, "
            f"a url rewrite rule turns {declared_url} into {used_url}. CI runs this check."
        )
        return
    fetch = git("fetch", "--quiet", *depth, CORE_REMOTE, CORE_BRANCH, cwd=sub)
    if fetch.returncode != 0:
        errors.append(f"{CORE_PATH}: could not tell whether {pinned} is on {CORE_BRANCH}: the fetch failed: {fetch.stderr.strip()}")
        return
    if git("rev-parse", "--is-shallow-repository", cwd=sub).stdout.strip() != "false":
        errors.append(f"{CORE_PATH}: could not tell whether {pinned} is on {CORE_BRANCH}: the history is still shallow")
        return
    on_main = git("merge-base", "--is-ancestor", pinned, "FETCH_HEAD", cwd=sub)
    if on_main.returncode == 1:
        errors.append(f"{CORE_PATH}: pinned commit {pinned} is not on RobCo-Core {CORE_BRANCH}")
    elif on_main.returncode != 0:
        errors.append(f"{CORE_PATH}: could not tell whether {pinned} is on {CORE_BRANCH}: {on_main.stderr.strip()}")
    else:
        print(f"robco-core: {pinned} is on {CORE_BRANCH}")


def main():
    errors = []
    check_toolchain(errors)
    check_core_submodule(errors)
    for e in errors:
        print(f"::error::{e}")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
