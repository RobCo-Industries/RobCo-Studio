#!/usr/bin/env bash
# Pack the QEMU build of build.sh so it runs on a machine without the
# build's libraries.
# Usage: pack.sh <linux|macos|windows> <out-dir>
# Reads <out-dir>/build/. Writes <out-dir>/qemu/, <out-dir>/report.txt and
# <out-dir>/sources.txt: for each packed library, a line with its packed
# name, the real path it was copied from and digest.sh's hash of its bytes,
# tab separated. notices.sh asks the packager about that exact path.
#
# The package:
#   bin/        qemu-system-arm, qemu-system-aarch64, and on Windows the DLLs
#   lib/        Linux and macOS: the libraries the system does not provide
#   licenses/   licence texts named after each component, and
#               packed-libraries.txt, the record of what was packed
#   NOTICE      what the package is and what it carries
# notices.sh writes licenses/ and NOTICE, as docs/THIRD_PARTY.md asks.
set -euxo pipefail

os=$1
out=$(cd "$2" && pwd)
build=$out/build
dist=$out/qemu
report=$out/report.txt
sources=$out/sources.txt
here=$(cd "$(dirname "$0")" && pwd)
exe=""
[ "$os" = windows ] && exe=.exe

rm -rf "$dist"
: >"$sources"
mkdir -p "$dist/bin" "$dist/lib"
for t in arm aarch64; do cp "$build/qemu-system-$t$exe" "$dist/bin/"; done
chmod 755 "$dist"/bin/*

{
  cat "$build/build.txt"
  echo "programs before strip (du -sk, KiB): $(du -sk "$dist/bin" | cut -f1)"
  echo "runtime libraries before packing:"
  case $os in
    macos) otool -L "$dist/bin/qemu-system-arm" ;;
    *) ldd "$dist/bin/qemu-system-arm$exe" ;;
  esac
} >"$report"

# Records that packed library $1 was copied from the real file $2, once the
# bytes packing keeps of $3, the packed copy, equal those of $2.
keep() {
  local d
  d=$("$here/digest.sh" "$os" "$3")
  [ "$d" = "$("$here/digest.sh" "$os" "$2")" ] || { echo "packed $1 differs from its source $2" >&2; return 1; }
  printf '%s\t%s\t%s\n' "$1" "$2" "$d" >>"$sources"
}

# Drop debug symbols. Release users do not need them.
case $os in
  macos) strip -x "$dist"/bin/* ;;
  *) strip "$dist"/bin/* ;;
esac

case $os in
  linux)
    # Copy every library that is not part of glibc next to the programs,
    # under its soname, then point the programs at it with a relative run
    # path. glibc stays a system library: the build host's glibc is the
    # oldest the package runs on.
    for b in "$dist"/bin/*; do
      ldd "$b" | awk '$2 == "=>" {print $1, $3}'
    done | sort -u | while read -r so path; do
      if "$here/check-libs.sh" --system "$so"; then continue; fi
      if [ "$path" = "not" ]; then echo "missing library: $so" >&2; exit 1; fi
      real=$(readlink -f "$path")
      cp "$real" "$dist/lib/$so"
      keep "$so" "$real" "$dist/lib/$so"
    done
    chmod 644 "$dist"/lib/*
    # The programs find lib/ through their own run path. Each library finds
    # its neighbours, pcre2 for glib, through its own: a DT_RUNPATH covers
    # only the direct needs of the file that holds it.
    for b in "$dist"/bin/*; do patchelf --set-rpath '$ORIGIN/../lib' "$b"; done
    for l in "$dist"/lib/*; do patchelf --set-rpath '$ORIGIN' "$l"; done
    "$dist/bin/qemu-system-arm" --version
    {
      echo "run paths:"
      for f in "$dist"/bin/* "$dist"/lib/*; do echo "$(basename "$f"): $(patchelf --print-rpath "$f")"; done
      echo "runtime libraries after packing:"
      ldd "$dist/bin/qemu-system-arm"
    } >>"$report"
    ;;
  macos)
    # Where each library dylibbundler copies comes from: the install names
    # the programs load, and those each library loads, outside the system,
    # resolved to the real file. dylibbundler copies each under the last
    # part of its install name.
    deps=$out/macos-deps.txt
    : >"$deps"
    todo=("$dist"/bin/*)
    while [ ${#todo[@]} -gt 0 ]; do
      f=${todo[0]}
      todo=("${todo[@]:1}")
      for d in $(otool -L "$f" | tail -n +2 | awk '{print $1}'); do
        case $d in
          /usr/lib/* | /System/*) continue ;;
          /*) ;;
          *) echo "cannot resolve install name $d of $f" >&2; exit 1 ;;
        esac
        r=$(python3 -c 'import os,sys; print(os.path.realpath(sys.argv[1]))' "$d")
        [ -f "$r" ] || { echo "install name $d of $f is not a file" >&2; exit 1; }
        line=$(printf '%s\t%s' "$(basename "$d")" "$r")
        if grep -qxF -- "$line" "$deps"; then continue; fi
        echo "$line" >>"$deps"
        todo+=("$r")
      done
    done
    twice=$(cut -f1 "$deps" | sort | uniq -d)
    [ -z "$twice" ] || { echo "two files load as $twice" >&2; exit 1; }
    dylibbundler -of -b -x "$dist/bin/qemu-system-arm" -x "$dist/bin/qemu-system-aarch64" \
      -d "$dist/lib" -p @executable_path/../lib/
    # dylibbundler can add the same run path twice. dyld 26 aborts on that.
    rp=@executable_path/../lib/
    for b in "$dist"/bin/*; do
      n=$(otool -l "$b" | grep -c "path $rp " || [ $? = 1 ])
      while [ "$n" -gt 1 ]; do install_name_tool -delete_rpath "$rp" "$b"; n=$((n - 1)); done
    done
    # make install signs with the HVF entitlement. TCG needs none, so an
    # ad hoc signature is enough.
    for f in "$dist"/lib/*.dylib "$dist"/bin/*; do codesign --force -s - "$f"; done
    for l in "$dist"/lib/*; do
      n=$(basename "$l")
      r=$(awk -F'\t' -v n="$n" '$1 == n {print $2}' "$deps")
      [ -n "$r" ] || { echo "no source path known for packed $n" >&2; exit 1; }
      keep "$n" "$r" "$l"
    done
    "$dist/bin/qemu-system-arm" --version
    { echo "runtime libraries after packing:"; otool -L "$dist/bin/qemu-system-arm"; } >>"$report"
    ;;
  windows)
    rmdir "$dist/lib"
    for b in "$dist"/bin/*.exe; do
      ldd "$b" | awk '$3 ~ /^\/ucrt64\// {print $3}'
    done | sort -u | while read -r dll; do
      n=$(basename "$dll")
      real=$(realpath "$dll")
      cp "$real" "$dist/bin/$n"
      keep "$n" "$real" "$dist/bin/$n"
    done
    { echo "runtime libraries after packing:"; ldd "$dist/bin/qemu-system-arm.exe"; } >>"$report"
    ;;
  *)
    echo "unknown system: $os" >&2
    exit 2
    ;;
esac

"$here/notices.sh" "$os" "$out" | tee -a "$report"

{
  echo "packed files:"
  ls -lR "$dist"
  echo "packed size (du -sk, KiB): $(du -sk "$dist" | cut -f1)"
} >>"$report"
cat "$report"
