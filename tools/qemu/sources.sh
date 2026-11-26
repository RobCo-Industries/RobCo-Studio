#!/usr/bin/env bash
# Fetch and keep the sources of every library packed in a QEMU package,
# at the exact version packed. Step 3 of the checklist in
# docs/THIRD_PARTY.md. Runs in the same job as pack.sh, right after it:
# a packager may stop serving an old version, so a release never fetches
# them later.
# Usage: sources.sh <linux|macos|windows> <out-dir>
# Reads <out-dir>/qemu/licenses/packed-libraries.txt, written by
# notices.sh. Writes <out-dir>/sources/<package>/ for each packed package
# and <out-dir>/sources/SOURCES.txt. Fails when any source cannot be
# fetched, or when the installed package is no longer the version packed.
#
# What each packager gives, per package:
#   linux    apt-get source --download-only: the .dsc, the upstream
#            archive and the debian archive with the rules and patches.
#            Needs deb-src lines in apt's sources.
#   macos    the formula file from homebrew-core at the tap commit brew
#            info names, the upstream archive and its patches, each
#            checked against the SHA-256 Homebrew states, the keg's
#            reduced install receipt and brew info --json=v2. No brew command
#            that turns on developer mode.
#   windows  MSYS2's source package of the exact version, which holds
#            the PKGBUILD, its patches and the upstream archive, and
#            pacman -Qi.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
os=$1
out=$(cd "$2" && pwd)
record=$out/qemu/licenses/packed-libraries.txt
dest=$out/sources
list=$dest/SOURCES.txt

[ -f "$record" ] || { echo "no $record: run pack.sh first" >&2; exit 1; }

sha() {
  if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

# Downloads URL $1 to file $2. The unit tests put a fake curl first on PATH.
fetch() {
  curl -fsSLo "$2" "$1"
}

rm -rf "$dest"
mkdir -p "$dest"
{
  echo "# The sources of each library packed, fetched when it was packed."
  echo "# package | version | file | sha256"
} >"$list"

# One line per packed package and version: fields 4 and 5 of the record.
pairs=$(awk -F' [|] ' '!/^#/ && NF >= 7 {print $4 "\t" $5}' "$record" | sort -u)
[ -n "$pairs" ] || { echo "$record names no packed library" >&2; exit 1; }

while IFS=$'\t' read -r pkg ver; do
  d=$dest/$pkg
  mkdir -p "$d"
  case $os in
    linux)
      now=$(dpkg-query -W -f='${Version}' "$pkg")
      [ "$now" = "$ver" ] || { echo "$pkg is $now now, but $ver was packed" >&2; exit 1; }
      src=$(dpkg-query -W -f='${source:Package}' "$pkg")
      srcver=$(dpkg-query -W -f='${source:Version}' "$pkg")
      (cd "$d" && apt-get source --download-only "$src=$srcver") >&2
      ls "$d"/*.dsc >/dev/null 2>&1 || { echo "no .dsc for $src $srcver" >&2; exit 1; }
      ;;
    macos)
      keg=$(brew --cellar)/$pkg/$ver
      [ -f "$keg/INSTALL_RECEIPT.json" ] || { echo "$keg, the keg packed, is gone" >&2; exit 1; }
      # The reduced receipt: the full one names folders of this machine.
      python3 "$here/brew_receipt.py" reduce "$keg/INSTALL_RECEIPT.json" "$pkg" "$ver" "$d/homebrew-receipt.json"
      brew info --json=v2 --formula "$pkg" >"$d/brew-info.json"
      # Homebrew installs from its API and keeps no tap on disk, so brew
      # cat cannot read the formula. brew_sources.py fetches it, the source
      # archive and the patches, each checked against a hash brew info
      # states, and fails unless the formula's full version is the keg's.
      python3 "$here/brew_sources.py" "$d/brew-info.json" "$ver" "$d"
      ;;
    windows)
      desc=$(pacman -Qi "$pkg")
      now=$(printf '%s\n' "$desc" | sed -n 's/^Version *: //p' | tr -d '\r')
      [ "$now" = "$ver" ] || { echo "$pkg is $now now, but $ver was packed" >&2; exit 1; }
      printf '%s\n' "$desc" >"$d/pacman-Qi.txt"
      # The source package is named after the package base, which differs
      # from the package name for split packages such as gcc-libs.
      dbdesc=${PACMAN_DB:-/var/lib/pacman}/local/$pkg-$ver/desc
      [ -f "$dbdesc" ] || { echo "no pacman record $dbdesc" >&2; exit 1; }
      base=$(awk 'f {print; exit} /^%BASE%$/ {f=1}' "$dbdesc" | tr -d '\r')
      [ -n "$base" ] || { echo "$dbdesc names no package base" >&2; exit 1; }
      s=$base-$ver.src.tar.zst
      fetch "https://repo.msys2.org/mingw/sources/$s" "$d/$s"
      # The listing is read whole before it is searched: a reader that stops
      # early breaks zstd's pipe, and pipefail counts that as a failure.
      names=$(tar tf "$d/$s") || { echo "cannot list $s" >&2; exit 1; }
      grep -q '/PKGBUILD$' <<<"$names" || { echo "$s holds no PKGBUILD" >&2; exit 1; }
      ;;
    *) echo "unknown system: $os" >&2; exit 2 ;;
  esac
  n=0
  for f in "$d"/*; do
    [ -f "$f" ] || continue
    echo "$pkg | $ver | $pkg/$(basename "$f") | $(sha "$f")" >>"$list"
    n=$((n + 1))
  done
  [ "$n" -gt 0 ] || { echo "nothing fetched for $pkg $ver" >&2; exit 1; }
done <<<"$pairs"

cat "$list"
