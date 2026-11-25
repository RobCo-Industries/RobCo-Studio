#!/usr/bin/env bash
# Write the licence texts, the record of what was packed and the NOTICE
# into a package that pack.sh has just packed, then check that every packed
# library has its licence. Follows the checklist in docs/THIRD_PARTY.md,
# steps 1, 2, 4 and 6. The release adds the sources, step 3.
# Usage: [QEMU_RELEASE_URL=<url>] notices.sh <linux|macos|windows> <out-dir>
#        notices.sh --check <linux|macos|windows> <out-dir>
# Reads <out-dir>/sources.txt, written by pack.sh, for the real path each
# packed library was copied from, and asks the packager which package owns
# that exact path. Reads QEMU's version from the packed program and its
# licence files from the source archive in <out-dir>/build/.
# --check runs only the final check on a package already written.
# QEMU_RELEASE_URL, set by the workflow on a release tag, is the release
# page the NOTICE names as the place of the sources.
#
# The record, licenses/packed-libraries.txt, has this format. Lines that
# start with # are comments. Every other line describes one packed library,
# and each packed library has exactly one line, in the order ls lists the
# library folder. A line has seven fields joined by " | ": file, component,
# packager, package, version, details, and "licence texts: " followed by
# the licence ids, one space apart. No field holds " | ". The line ends
# with the last id, no trailing space, and every line ends with a newline.
# A file name never holds a space: the packers do not produce one, and a
# name split at a space would be two unknown libraries, which fails the
# check. The script writes each line once, after it has fetched every
# licence text of that library, and never edits it.
set -euxo pipefail

check_only=0
if [ "$1" = --check ]; then check_only=1; shift; fi
here=$(cd "$(dirname "$0")" && pwd)
os=$1
out=$(cd "$2" && pwd)
build=$out/build
dist=$out/qemu
lic=$dist/licenses
record=$lic/packed-libraries.txt
sources=$out/sources.txt
exe=""
[ "$os" = windows ] && exe=.exe

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

# The packages, per packager, that the licence table above was read from.
# A library from any other package, renamed or split off, stops the packing
# until someone reads that package's licence and adds it here.
packages() {
  case $os:$1 in
    linux:glib) echo libglib2.0-0 ;;
    linux:pcre) echo libpcre3 ;;
    linux:pcre2) echo libpcre2-8-0 ;;
    linux:pixman) echo libpixman-1-0 ;;
    linux:libfdt) echo libfdt1 ;;
    linux:zlib) echo zlib1g ;;
    linux:libatomic) echo libatomic1 ;;
    macos:libfdt) echo dtc ;;
    macos:intl) echo gettext ;;
    macos:iconv) echo libiconv ;;
    macos:*) echo "$1" ;;
    windows:glib) echo mingw-w64-ucrt-x86_64-glib2 ;;
    windows:libfdt) echo mingw-w64-ucrt-x86_64-dtc ;;
    windows:intl) echo mingw-w64-ucrt-x86_64-gettext-runtime ;;
    windows:iconv) echo mingw-w64-ucrt-x86_64-libiconv ;;
    windows:winpthread) echo mingw-w64-ucrt-x86_64-libwinpthread ;;
    windows:libatomic) echo mingw-w64-ucrt-x86_64-gcc-libs ;;
    windows:*) echo "mingw-w64-ucrt-x86_64-$1" ;;
  esac
}

# Fails and prints why, naming the library, when package $3 is not one the licence table
# of component $2 was read from.
known_package() {
  case " $(packages "$2") " in
    *" $3 "*) return 0 ;;
    *) echo "$1 ($2) comes from package $3, but its licence was read from $(packages "$2"); read the licence of $3 and update notices.sh"; return 1 ;;
  esac
}

# The licence texts that apply to a component, by SPDX-style short name.
# The package must hold the full text of each.
texts() {
  case $1 in
    glib | intl | iconv) echo "LGPL-2.1" ;;
    pcre) echo "BSD-3-Clause" ;;
    pcre2) echo "BSD-3-Clause PCRE2-exception" ;;
    pixman) echo "MIT" ;;
    libfdt) echo "GPL-2.0 BSD-2-Clause" ;;
    zlib) echo "Zlib" ;;
    libatomic) echo "GPL-3.0 GCC-exception-3.1" ;;
    winpthread) echo "MIT BSD-3-Clause" ;;
  esac
}

# Phrases only the full text of a licence holds: its title line and a line
# from near its end. A file that names a licence and points elsewhere holds
# neither. Compared ignoring case, line breaks and C comment stars.
markers() {
  case $1 in
    LGPL-2.1) printf '%s\n' "GNU LESSER GENERAL PUBLIC LICENSE" "Version 2.1, February 1999" "That's all there is to it!" ;;
    GPL-2.0) printf '%s\n' "GNU GENERAL PUBLIC LICENSE" "Version 2, June 1991" "Public License instead of this License." ;;
    GPL-3.0) printf '%s\n' "GNU GENERAL PUBLIC LICENSE" "Version 3, 29 June 2007" "why-not-lgpl.html" ;;
    GCC-exception-3.1) printf '%s\n' "GCC RUNTIME LIBRARY EXCEPTION" "Version 3.1, 31 March 2009" "Eligible Compilation Process" ;;
    BSD-2-Clause) printf '%s\n' "Redistribution and use in source and binary forms" "EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE" ;;
    BSD-3-Clause) printf '%s\n' "Redistribution and use in source and binary forms" "Neither the name of" "EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE" ;;
    PCRE2-exception) printf '%s\n' "EXEMPTION FOR BINARY LIBRARY-LIKE PACKAGES" ;;
    MIT) printf '%s\n' "Permission is hereby granted" "WITHOUT WARRANTY OF ANY KIND" ;;
    Zlib) printf '%s\n' "Permission is granted to anyone to use this software for any purpose" "This notice may not be removed or altered from any source distribution." ;;
    *) echo "no full-text markers for licence $1" >&2; return 1 ;;
  esac
}

# Succeeds when one of the files holds the full text of licence $1.
# Written for macOS's bash 3.2. The text is read whole first: grep -q
# stopping early would break the pipe and pipefail would call it a miss.
has_text() {
  local id=$1 f m ok n want
  shift
  want=$(markers "$id") || return 1
  for f in "$@"; do
    [ -f "$f" ] || continue
    n=$(tr '\t\r\n*' '    ' <"$f" | tr -s ' ')
    ok=1
    while IFS= read -r m; do
      grep -qiF -- "$m" <<<"$n" || { ok=0; break; }
    done <<<"$want"
    [ "$ok" = 1 ] && return 0
  done
  return 1
}

# Downloads URL $1 to file $2. The only network access besides brew fetch;
# the unit tests put a fake curl first on PATH.
fetch() {
  curl -fsSLo "$2" "$1"
}

# The packed library files.
packed() {
  case $os in
    windows) (cd "$dist/bin" && ls ./*.dll | sed 's|^\./||') ;;
    *) (cd "$dist/lib" && ls) ;;
  esac
}

# The real path pack.sh copied packed file $1 from, once its bytes still
# match: the source's now, and on macOS and Windows the packed copy's too.
# On Linux patchelf has changed the copy's run path since pack.sh compared
# it. Fails, naming the library, when pack.sh recorded no single source.
origin() {
  local n p d
  [ -f "$sources" ] || { echo "no $sources: pack.sh records where each library came from" >&2; return 1; }
  n=$(awk -F'\t' -v f="$1" '$1 == f' "$sources" | wc -l | tr -d ' ')
  [ "$n" = 1 ] || { echo "$1 has $n source lines in $sources, not one" >&2; return 1; }
  p=$(awk -F'\t' -v f="$1" '$1 == f {print $2}' "$sources")
  d=$(awk -F'\t' -v f="$1" '$1 == f {print $3}' "$sources")
  [ -f "$p" ] || { echo "$1 was copied from $p, which is gone" >&2; return 1; }
  [ "$("$here/digest.sh" "$os" "$p")" = "$d" ] || { echo "$1: its source $p changed since it was packed" >&2; return 1; }
  if [ "$os" != linux ] && [ "$("$here/digest.sh" "$os" "$(packed_path "$1")")" != "$d" ]; then
    echo "$1: the packed file is not the bytes of $p" >&2
    return 1
  fi
  echo "$p"
}

packed_path() {
  case $os in windows) echo "$dist/bin/$1" ;; *) echo "$dist/lib/$1" ;; esac
}

# Step 6. The list of libraries comes from the files really in the package,
# and which licences apply comes from texts() above, not from the record:
# the record is checked against both. Each packed library has exactly one
# record line, its licence ids equal texts(), licenses/ holds the full text
# of each and the library's own licence or notice file. A record line for a
# file not in the package fails, and so does a file this script does not
# know: a new dependency stops the packing until someone states its licence.
check() {
  local bad=0 f c t want own files lines n pk msg
  for f in qemu-COPYING qemu-LICENSE packed-libraries.txt; do
    [ -s "$lic/$f" ] || { echo "FAIL: licenses/$f is missing"; bad=1; }
  done
  has_text GPL-2.0 "$lic/qemu-COPYING" || { echo "FAIL: licenses/qemu-COPYING is not the full GPL-2.0 text"; bad=1; }
  [ -s "$dist/NOTICE" ] || { echo "FAIL: NOTICE is missing"; bad=1; }
  files=$(packed) || { echo "FAIL: no library folder in the package"; return 1; }
  lines=""
  [ -f "$record" ] && lines=$(awk -F' [|] ' '$7 ~ /^licence texts: / {sub(/^licence texts: /, "", $7); print $1 "\t" $7}' "$record")
  for f in $files; do
    c=$(component "$f" 2>/dev/null) || { echo "FAIL: $f is a packed library this script does not know; state its licence in notices.sh"; bad=1; continue; }
    n=$(printf '%s\n' "$lines" | awk -F'\t' -v f="$f" '$1 == f' | wc -l | tr -d ' ')
    [ "$n" -le 1 ] || { echo "FAIL: $f ($c) has $n record lines naming licence texts, not one"; bad=1; }
    # awk reads the whole list: an early exit would break printf's pipe.
    t=$(printf '%s\n' "$lines" | awk -F'\t' -v f="$f" '$1 == f && !n++ {print $2}')
    [ -n "$t" ] || { echo "FAIL: $f ($c) has no record line naming its licence texts"; bad=1; }
    pk=$(awk -F' [|] ' -v f="$f" '$1 == f {print $4; exit}' "$record" 2>/dev/null || true)
    if [ -n "$t" ] && ! msg=$(known_package "$f" "$c" "$pk"); then echo "FAIL: $msg"; bad=1; fi
    want=$(texts "$c")
    [ "$(echo $t)" = "$want" ] || { echo "FAIL: the record names licence texts '$t' for $f ($c), but $want apply"; bad=1; }
    for id in $want; do
      has_text "$id" "$lic/$c"-* "$lic"/text-* || { echo "FAIL: no full $id text in licenses/ for $f ($c)"; bad=1; }
    done
    own=0
    for g in "$lic/$c"-*; do
      case $g in *-INSTALL_RECEIPT.json | *-pacman-Qi.txt) continue ;; esac
      [ -s "$g" ] && own=1
    done
    [ "$own" = 1 ] || { echo "FAIL: no licence or notice file of its own in licenses/ for $f ($c)"; bad=1; }
  done
  for f in $(printf '%s\n' "$lines" | cut -f1); do
    case "
$files
" in *"
$f
"*) ;; *) echo "FAIL: the record names $f, which is not in the package"; bad=1 ;; esac
  done
  return "$bad"
}

if [ "$check_only" = 1 ]; then
  if check; then echo "Every packed library has the full text of each of its licences."; exit 0; fi
  exit 1
fi

rm -rf "$lic" "$dist/NOTICE"
mkdir -p "$lic"
# Step 1: QEMU's own licence files. The version is the packed program's
# own answer. build.sh keeps the source archive it checked the hash of; it
# must hold that version, and the licence files come from it.
ver=$("$dist/bin/qemu-system-arm$exe" --version | tr -d '\r' | sed -n '1s/^QEMU emulator version \([^ ]*\).*/\1/p')
[ -n "$ver" ] || { echo "the packed qemu-system-arm names no version" >&2; exit 1; }
qsrc=$build/qemu-$ver.tar.xz
[ -f "$qsrc" ] || { echo "no source archive $qsrc for the packed QEMU $ver" >&2; exit 1; }
t=$(mktemp -d)
tar xJf "$qsrc" -C "$t" "qemu-$ver/VERSION" "qemu-$ver/COPYING" "qemu-$ver/LICENSE"
[ "$(tr -d '\r\n' <"$t/qemu-$ver/VERSION")" = "$ver" ] || { echo "$qsrc is not the source of QEMU $ver" >&2; exit 1; }
cp "$t/qemu-$ver/COPYING" "$lic/qemu-COPYING"
cp "$t/qemu-$ver/LICENSE" "$lic/qemu-LICENSE"
rm -rf "$t"
qsha=$("$here/digest.sh" linux "$qsrc")

{
  echo "# What was packed, recorded when it was packed."
  echo "# file | component | packager | package | version | details | licence texts"
} >"$record"
notice_libs=""

for f in $(packed); do
  c=$(component "$f")
  p=$(origin "$f")
  case $os in
    linux)
      # dpkg knows the file under /lib or /usr/lib, as installed, and /lib
      # links to /usr/lib: ask for the resolved path under either name.
      pkg=""
      for try in "$p" "/lib${p#/usr/lib}"; do
        [ "$try" -ef "$p" ] || continue
        pkg=$(dpkg -S "$try" 2>/dev/null | sed -n '1s/:.*//p' || true)
        if [ -n "$pkg" ]; then break; fi
      done
      [ -n "$pkg" ] || { echo "no dpkg package owns $p, the source of $f" >&2; exit 1; }
      known_package "$f" "$c" "$pkg" >&2
      v=$(dpkg-query -W -f='${Version}' "$pkg")
      src=$(dpkg-query -W -f='${source:Package} ${source:Version}' "$pkg")
      entry="$f | $c | Ubuntu $(. /etc/os-release && echo "$VERSION_ID") | $pkg | $v | source package $src, from $p"
      # The packager's copyright file, the one that package installs. It
      # may name a licence and point to its text under
      # /usr/share/common-licenses/, which is not packed: copy each text it
      # points to, once, as text-<name>.
      cr=/usr/share/doc/$pkg/copyright
      [ "$(dpkg -S "$cr" 2>/dev/null | sed -n '1s/:.*//p' || true)" = "$pkg" ] || { echo "$pkg does not install $cr" >&2; exit 1; }
      cp "$cr" "$lic/$c-copyright"
      ptrs=$(python3 "$here/deb_copyright.py" "$cr")
      # A pointer may be a link, such as LGPL to LGPL-3: name the copy after
      # the text it really is. The LGPL 3 is a supplement to the GPL 3.
      for t in $ptrs; do
        [ -f "$t" ] || { echo "$pkg copyright points to $t, which does not exist" >&2; exit 1; }
        t=$(readlink -f "$t")
        case $(basename "$t") in LGPL-3) ptrs="$ptrs /usr/share/common-licenses/GPL-3" ;; esac
      done
      for t in $ptrs; do
        t=$(readlink -f "$t")
        [ -f "$lic/text-$(basename "$t")" ] || cp "$t" "$lic/text-$(basename "$t")"
      done
      # A licence the record names but the copyright file does not point
      # to, as glib's file points to the newest LGPL while glib is under
      # the LGPL 2.1 or later: take its text from the same folder.
      for id in $(texts "$c"); do
        has_text "$id" "$lic/$c"-* "$lic"/text-* && continue
        case $id in
          LGPL-2.1) t=LGPL-2.1 ;; GPL-2.0) t=GPL-2 ;; GPL-3.0) t=GPL-3 ;; BSD-3-Clause) t=BSD ;;
          *) continue ;;
        esac
        [ -f "$lic/text-$t" ] || cp "/usr/share/common-licenses/$t" "$lic/text-$t"
      done
      ;;
    macos)
      # The keg is the Cellar folder the resolved source path lies in.
      cellar=$(brew --cellar)
      case $p in "$cellar"/*/*/*) ;; *) echo "$p, the source of $f, is in no Homebrew keg" >&2; exit 1 ;; esac
      rel=${p#"$cellar"/}
      formula=${rel%%/*}
      rest=${rel#*/}
      v=${rest%%/*}
      keg=$cellar/$formula/$v
      [ -f "$keg/INSTALL_RECEIPT.json" ] || { echo "$keg, the source of $f, has no install receipt" >&2; exit 1; }
      known_package "$f" "$c" "$formula" >&2
      head=$(python3 -c 'import json,sys; print((json.load(open(sys.argv[1])).get("source") or {}).get("tap_git_head") or "unknown")' "$keg/INSTALL_RECEIPT.json")
      entry="$f | $c | Homebrew | $formula | $v | homebrew-core tap_git_head $head, from $p"
      cp "$keg/INSTALL_RECEIPT.json" "$lic/$c-INSTALL_RECEIPT.json"
      found=0
      for l in "$keg"/COPYING* "$keg"/LICENSE* "$keg"/LICENCE* "$keg"/COPYRIGHT*; do
        if [ -f "$l" ]; then cp "$l" "$lic/$c-$(basename "$l")"; found=1; fi
      done
      need=0
      for id in $(texts "$c"); do
        has_text "$id" "$lic/$c"-* || need=1
      done
      if [ "$found" = 0 ] || [ "$need" = 1 ]; then
        # Some kegs, such as glib and dtc, keep no licence file, and some,
        # such as gettext for intl, keep one that is not the library's.
        # Take the texts from the formula's own source archive, checked to
        # be this version.
        brew fetch --build-from-source --formula "$formula" >&2
        tarball=$(brew --cache --build-from-source --formula "$formula")
        case $(basename "$tarball") in
          *"${v%_*}"*) ;;
          *) echo "source $tarball is not version $v" >&2; exit 1 ;;
        esac
        t=$(mktemp -d)
        tar xf "$tarball" -C "$t"
        for l in "$t"/*/COPYING* "$t"/*/LICENSE* "$t"/*/LICENCE* "$t"/*/README.license "$t"/*/BSD-2-Clause "$t"/*/GPL; do
          if [ -f "$l" ]; then cp "$l" "$lic/$c-$(basename "$l")"; fi
        done
        # A text still missing may sit deeper, as gettext keeps the LGPL
        # of libintl: take the first licence file that holds it in full.
        for id in $(texts "$c"); do
          has_text "$id" "$lic/$c"-* && continue
          while IFS= read -r l; do
            if has_text "$id" "$l"; then
              rel=${l#"$t"/*/}
              cp "$l" "$lic/$c-${rel//\//-}"
              break
            fi
          done < <(find "$t" -maxdepth 4 -type f \( -name 'COPYING*' -o -name 'LICENSE*' -o -name 'LICENCE*' \) | sort)
        done
        rm -rf "$t"
        entry="$entry; licence texts from the source archive $(basename "$tarball")"
      fi
      ;;
    windows)
      pkg=$(pacman -Qqo "$p") || { echo "no MSYS2 package owns $p, the source of $f" >&2; exit 1; }
      known_package "$f" "$c" "$pkg" >&2
      v=$(pacman -Q "$pkg" | cut -d' ' -f2)
      entry="$f | $c | MSYS2 | $pkg | $v | from $p"
      pacman -Qi "$pkg" >"$lic/$c-pacman-Qi.txt"
      for l in $(pacman -Qlq "$pkg" | grep '/share/licenses/.*[^/]$' || true); do
        cp "$l" "$lic/$c-$(basename "$l")"
      done
      # A text MSYS2 does not install, such as libfdt's BSD licence: take it
      # from MSYS2's source package of this exact version, and from the
      # upstream archive inside it.
      need=""
      for id in $(texts "$c"); do
        has_text "$id" "$lic/$c"-* || need="$need $id"
      done
      if [ -n "$need" ]; then
        srcpkg="mingw-w64-${pkg#mingw-w64-ucrt-x86_64-}-$v.src.tar.zst"
        t=$(mktemp -d)
        fetch "https://repo.msys2.org/mingw/sources/$srcpkg" "$t/$srcpkg"
        tar xf "$t/$srcpkg" -C "$t"
        for a in "$t"/*/*.tar.*; do
          case $a in *.sig | *.asc) continue ;; esac
          [ -f "$a" ] && tar xf "$a" -C "$(dirname "$a")"
        done
        for id in $need; do
          while IFS= read -r l; do
            if has_text "$id" "$l"; then cp "$l" "$lic/$c-$(basename "$l")"; break; fi
          done < <(find "$t" -maxdepth 5 -type f \( -name 'COPYING*' -o -name 'LICENSE*' -o -name 'LICENCE*' -o -name 'BSD-2-Clause' -o -name 'README.license' \) | sort)
        done
        rm -rf "$t"
        entry="$entry; licence texts from the source package $srcpkg"
      fi
      ;;
  esac
  # The library's one record line, written now that its texts are in place.
  echo "$entry | licence texts: $(texts "$c")" >>"$record"
  case " $notice_libs " in *" $c "*) ;; *) notice_libs="$notice_libs $c" ;; esac
done

# Step 2: the NOTICE at the top of the package.
{
  echo "QEMU $ver for Arm, as built by RobCo Studio from qemu-$ver.tar.xz,"
  echo "SHA-256 $qsha."
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
  if [ -n "${QEMU_RELEASE_URL:-}" ]; then
    echo "above, at $QEMU_RELEASE_URL"
  else
    echo "above. This build was not published, so it names no release address."
  fi
} >"$dist/NOTICE"

echo "NOTICE:"; cat "$dist/NOTICE"
echo "Record:"; cat "$record"
echo "licenses/:"; ls -l "$lic"
check
echo "Every packed library has the full text of each of its licences."
