#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Check the pins CI enforces.

1. `rust-toolchain.toml` names one exact stable release, and every workspace
   manifest states the same `rust-version`, directly or through the workspace.
2. The RobCo-Core submodule at `external/robco-core` points at a commit on
   RobCo-Core's `main`. While the repository has no such submodule, the check
   says so and passes.

Run it from the repository root. It exits 1 on the first broken rule set.
"""

import pathlib
import re
import subprocess
import sys
import tomllib

ROOT = pathlib.Path.cwd()
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


def git(*args, cwd=ROOT):
    return subprocess.run(["git", *args], cwd=cwd, capture_output=True, text=True)


def check_core_submodule(errors):
    declared = git("config", "-f", ".gitmodules", "--get-regexp", r"^submodule\..*\.path$")
    paths = declared.stdout.split()[1::2] if declared.returncode == 0 else []
    tree = git("ls-tree", "HEAD", CORE_PATH).stdout.split()
    if CORE_PATH not in paths and not tree and not (ROOT / CORE_PATH).exists():
        print(f"robco-core: no submodule yet at {CORE_PATH}, nothing to check")
        return
    if CORE_PATH not in paths:
        errors.append(f"{CORE_PATH} exists but .gitmodules does not declare it")
        return
    if len(tree) < 3 or tree[1] != "commit":
        errors.append(f"{CORE_PATH} is declared in .gitmodules but HEAD holds no submodule commit there")
        return
    pinned = tree[2]
    init = git("submodule", "update", "--init", CORE_PATH)
    if init.returncode != 0:
        errors.append(f"{CORE_PATH}: could not check out the pinned commit: {init.stderr.strip()}")
        return
    sub = ROOT / CORE_PATH
    fetch = git("fetch", "--quiet", "origin", CORE_BRANCH, cwd=sub)
    if fetch.returncode != 0:
        errors.append(f"{CORE_PATH}: could not fetch {CORE_BRANCH}: {fetch.stderr.strip()}")
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
