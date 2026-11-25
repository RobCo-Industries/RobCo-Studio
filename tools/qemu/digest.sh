#!/usr/bin/env bash
# Print the SHA-256 of the bytes of a library that packing keeps unchanged.
# Usage: digest.sh <linux|macos|windows> <file>
# Linux and Windows: the whole file. pack.sh compares it right after the
# copy, before patchelf changes the run path on Linux.
# macOS: the machine code only, the __TEXT,__text section as otool -t
# prints it, without its first line, the file name. dylibbundler rewrites
# install names and codesign replaces the signature, so the whole file
# differs from its source. Neither touches the machine code.
set -euo pipefail

sha() {
  if command -v sha256sum >/dev/null; then sha256sum | cut -d' ' -f1; else shasum -a 256 | cut -d' ' -f1; fi
}

case $1 in
  macos)
    text=$(otool -t "$2" | tail -n +2)
    [ -n "$text" ] || { echo "no machine code in $2" >&2; exit 1; }
    printf '%s\n' "$text" | sha
    ;;
  linux | windows) sha <"$2" ;;
  *) echo "unknown system: $1" >&2; exit 2 ;;
esac
