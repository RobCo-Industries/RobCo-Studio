#!/usr/bin/env bash
# Write the licence texts, the record of what was packed and the NOTICE
# into a package that pack.sh has just packed, then check that every packed
# library has its licence. Follows the checklist in docs/THIRD_PARTY.md,
# steps 1, 2, 4 and 6. The release adds the sources, step 3.
# Usage: notices.sh <linux|macos|windows> <out-dir>
# Reads <out-dir>/build/ to find where each packed library came from.
set -euo pipefail

os=$1
out=$(cd "$2" && pwd)
build=$out/build
dist=$out/qemu
lic=$dist/licenses
record=$lic/packed-libraries.txt
ver=10.1.2

# The component a packed file belongs to, named as in docs/THIRD_PARTY.md.
component() {
  case $1 in
    libglib-2.0*) echo glib ;;
    libpcre2-8*) echo pcre2 ;;
    libpcre.*) echo pcre ;;
    libpixman-1*) echo pixman ;;
    libfdt*) echo libfdt ;;
    libz.* | libz-* | zlib1.dll) echo zlib ;;
    libatomic*) echo libatomic ;;
    libintl*) echo intl ;;
    libiconv*) echo iconv ;;
    libwinpthread*) echo winpthread ;;
    *) echo "unknown packed library: $1" >&2; return 1 ;;
  esac
}

# Its licence, as docs/THIRD_PARTY.md reads it from the source.
licence() {
  case $1 in
    glib | intl) echo "LGPL-2.1-or-later" ;;
    pcre) echo "BSD-3-Clause" ;;
    pcre2) echo "BSD-3-Clause WITH PCRE2-exception" ;;
    pixman) echo "MIT" ;;
    libfdt) echo "GPL-2.0-or-later OR BSD-2-Clause" ;;
    zlib) echo "Zlib" ;;
    libatomic) echo "GPL-3.0-or-later WITH GCC-exception-3.1" ;;
    iconv) echo "LGPL, version as stated in its source" ;;
    winpthread) echo "MIT AND BSD-3-Clause" ;;
  esac
}

# The packed library files.
packed() {
  case $os in
    windows) (cd "$dist/bin" && ls ./*.dll | sed 's|^\./||') ;;
    *) (cd "$dist/lib" && ls) ;;
  esac
}

# Where the build loaded a packed file from, on Linux and Windows.
origin() {
  ldd "$build"/qemu-system-arm* "$build"/qemu-system-aarch64* \
    | awk -v f="$1" '$1 == f && $2 == "=>" {print $3; exit}'
}

rm -rf "$lic" "$dist/NOTICE"
mkdir -p "$lic"
# Step 1: QEMU's own licence files, from the source tree it was built from.
cp "$build/COPYING" "$lic/qemu-COPYING"
cp "$build/LICENSE" "$lic/qemu-LICENSE"

{
  echo "# What was packed, recorded when it was packed."
  echo "# file | component | packager | package | version | details"
} >"$record"
notice_libs=""

for f in $(packed); do
  c=$(component "$f")
  case $os in
    linux)
      # dpkg knows the path under /lib or /usr/lib, as installed. ldd may
      # print the other one, since /lib links to /usr/lib.
      p=$(origin "$f")
      real=$(readlink -f "$p")
      pkg=""
      for try in "$(dirname "$p")/$(basename "$real")" "$real" "/lib${real#/usr/lib}"; do
        pkg=$(dpkg -S "$try" 2>/dev/null | head -1 | cut -d: -f1) && [ -n "$pkg" ] && break
      done
      [ -n "$pkg" ] || { echo "no dpkg package owns $p" >&2; exit 1; }
      v=$(dpkg-query -W -f='${Version}' "$pkg")
      src=$(dpkg-query -W -f='${source:Package} ${source:Version}' "$pkg")
      echo "$f | $c | Ubuntu $(. /etc/os-release && echo "$VERSION_ID") | $pkg | $v | source package $src, from $p" >>"$record"
      # The packager's copyright file, which holds the licence text.
      cp "/usr/share/doc/$pkg/copyright" "$lic/$c-copyright"
      ;;
    macos)
      cellar=$(brew --cellar)
      p=$(find "$cellar" -name "$f" -not -type d | head -1)
      [ -n "$p" ] || { echo "no Homebrew keg holds $f" >&2; exit 1; }
      rel=${p#"$cellar"/}
      formula=${rel%%/*}
      rest=${rel#*/}
      v=${rest%%/*}
      keg=$cellar/$formula/$v
      head=$(python3 -c 'import json,sys; print((json.load(open(sys.argv[1])).get("source") or {}).get("tap_git_head") or "unknown")' "$keg/INSTALL_RECEIPT.json")
      echo "$f | $c | Homebrew | $formula | $v | homebrew-core tap_git_head $head, from $p" >>"$record"
      cp "$keg/INSTALL_RECEIPT.json" "$lic/$c-INSTALL_RECEIPT.json"
      for l in "$keg"/COPYING* "$keg"/LICENSE* "$keg"/LICENCE* "$keg"/COPYRIGHT*; do
        [ -f "$l" ] && cp "$l" "$lic/$c-$(basename "$l")"
      done
      ;;
    windows)
      p=$(origin "$f")
      pkg=$(pacman -Qqo "$p")
      v=$(pacman -Q "$pkg" | cut -d' ' -f2)
      echo "$f | $c | MSYS2 | $pkg | $v | from $p" >>"$record"
      pacman -Qi "$pkg" >"$lic/$c-pacman-Qi.txt"
      for l in $(pacman -Qlq "$pkg" | grep '/share/licenses/.*[^/]$' || true); do
        cp "$l" "$lic/$c-$(basename "$l")"
      done
      ;;
  esac
  case " $notice_libs " in *" $c "*) ;; *) notice_libs="$notice_libs $c" ;; esac
done

# Step 2: the NOTICE at the top of the package.
{
  echo "QEMU $ver for Arm, as built by RobCo Studio."
  echo
  echo "QEMU is released under the GNU General Public License, version 2."
  echo "Its licence texts are licenses/qemu-COPYING and licenses/qemu-LICENSE."
  echo
  echo "This package also carries these libraries, each under its own licence."
  echo "Their licence texts are in licenses/, named after each library."
  echo "licenses/packed-libraries.txt records the exact package each file came from."
  echo
  for c in $notice_libs; do
    v=$(awk -F' [|] ' -v c="$c" '$2 == c {print $5; exit}' "$record")
    echo "- $c $v: $(licence "$c")"
  done
  echo
  echo "Sources: the release that publishes this package carries QEMU's source"
  echo "archive, the build and packing scripts, and the sources of each library"
  echo "above. This build was not published, so it names no release address."
} >"$dist/NOTICE"

# Step 6: every packed library has a licence file and a record line.
bad=0
for f in qemu-COPYING qemu-LICENSE packed-libraries.txt; do
  [ -s "$lic/$f" ] || { echo "FAIL: licenses/$f is missing"; bad=1; }
done
[ -s "$dist/NOTICE" ] || { echo "FAIL: NOTICE is missing"; bad=1; }
for f in $(packed); do
  c=$(component "$f")
  n=$(cd "$lic" && ls | grep -E "^$c-" | grep -vE -- '-(INSTALL_RECEIPT\.json|pacman-Qi\.txt)$' | wc -l)
  [ "$n" -gt 0 ] || { echo "FAIL: no licence file for $f ($c)"; bad=1; }
  grep -q "^$f | " "$record" || { echo "FAIL: $f is not in the record"; bad=1; }
done

echo "NOTICE:"; cat "$dist/NOTICE"
echo "Record:"; cat "$record"
echo "licenses/:"; ls -l "$lic"
[ "$bad" -eq 0 ] || exit 1
echo "Every packed library has a licence file and a record line."
