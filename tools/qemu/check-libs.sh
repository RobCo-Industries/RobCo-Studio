#!/usr/bin/env bash
# Show where the packed QEMU programs load their libraries from, and fail
# when one comes from outside the package and is not part of the system.
#
# Usage:
#   check-libs.sh --system <soname>        exit 0 if glibc provides it
#   check-libs.sh linux <package> [bin]    ldd every program in bin
#   check-libs.sh macos <package> [bin]    otool -L every program and library
# bin defaults to <package>/bin. Passing another folder, such as the
# unpacked build, shows that the check fails on programs that are not packed.
set -euo pipefail

# glibc and the kernel's vDSO: the only libraries Linux may take from the
# system.
glibc="ld-linux-x86-64.so.2 libc.so.6 libm.so.6 libpthread.so.0 libdl.so.2 librt.so.1 libresolv.so.2 libutil.so.1"
# The libraries the Linux build needs beyond glibc. Each must load from
# the package. On Ubuntu 24.04 they are glib, pcre2, pixman, libfdt, zlib
# and libatomic. On 22.04 glib uses the older pcre and nothing needs
# libatomic, so the check names every one it knows and requires the four
# both share, plus every library the package holds.
known="libglib-2.0.so.0 libpcre.so.3 libpcre2-8.so.0 libpixman-1.so.0 libfdt.so.1 libz.so.1 libatomic.so.1"
required="libglib-2.0.so.0 libpixman-1.so.0 libfdt.so.1 libz.so.1"

if [ "${1:-}" = --system ]; then
  for so in $glibc; do [ "$2" = "$so" ] && exit 0; done
  exit 1
fi

os=$1
pkg=$(cd "$2" && pwd -P)
bin=$(cd "${3:-$pkg/bin}" && pwd -P)
lib=$pkg/lib
bad=0

case $os in
  linux)
    echo "Libraries the package may carry, as this system has them (ldconfig -p):"
    for so in $known; do
      if ldconfig -p | awk -v s="$so" '$1 == s {f = 1} END {exit !f}'; then echo "  $so: present"; else echo "  $so: absent"; fi
    done
    for b in "$bin"/qemu-system-*; do
      echo "ldd $(basename "$b"):"
      t=$(mktemp)
      ldd "$b" | tee "$t" | sed 's/^/  /'
      while read -r so arrow path _; do
        [ "$arrow" = "=>" ] || continue
        if [ "$path" = "not" ]; then
          echo "FAIL: $so not found, neither in the package nor on the system"; bad=1; continue
        fi
        if "$0" --system "$so"; then continue; fi
        dir=$(dirname "$(readlink -f "$path")")
        if [ "$dir" != "$lib" ]; then
          echo "FAIL: $so loads from $path, outside the package"; bad=1
        fi
      done <"$t"
      for so in $required $(cd "$lib" && ls); do
        if ! grep -qF "$so => " "$t"; then
          echo "FAIL: $(basename "$b") does not load $so"; bad=1
        fi
      done
      rm -f "$t"
    done
    ;;
  macos)
    for f in "$bin"/qemu-system-* "$lib"/*.dylib; do
      [ -f "$f" ] || continue
      echo "otool -L $(basename "$f"):"
      otool -L "$f" | tail -n +2 | sed 's/^/  /'
      while read -r dep _; do
        case $dep in
          /usr/lib/* | /System/Library/*) ;;
          @executable_path/../lib/*)
            if [ ! -f "$lib/${dep#@executable_path/../lib/}" ]; then
              echo "FAIL: $dep is not in the package"; bad=1
            fi
            ;;
          *) echo "FAIL: $(basename "$f") loads $dep, outside the package"; bad=1 ;;
        esac
      done < <(otool -L "$f" | tail -n +2)
    done
    ;;
  *)
    echo "unknown system: $os" >&2
    exit 2
    ;;
esac

if [ "$bad" -ne 0 ]; then
  echo "Verdict: fail, a library is missing or loads from outside the package."
  exit 1
fi
echo "Verdict: pass, every library loads from the package or the system's C library."
