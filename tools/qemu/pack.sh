#!/usr/bin/env bash
# Pack the QEMU build of build.sh so it runs on a machine without the
# build's libraries.
# Usage: pack.sh <linux|macos|windows> <out-dir>
# Reads <out-dir>/build/. Writes <out-dir>/qemu/ and <out-dir>/report.txt.
#
# The package:
#   bin/        qemu-system-arm, qemu-system-aarch64, and on Windows the DLLs
#   lib/        Linux and macOS: the libraries the system does not provide
#   licences/   licence texts and notices; for now QEMU's COPYING and LICENSE
set -euxo pipefail

os=$1
out=$(cd "$2" && pwd)
build=$out/build
dist=$out/qemu
report=$out/report.txt
here=$(cd "$(dirname "$0")" && pwd)
exe=""
[ "$os" = windows ] && exe=.exe

rm -rf "$dist"
mkdir -p "$dist/bin" "$dist/lib" "$dist/licences/qemu"
for t in arm aarch64; do cp "$build/qemu-system-$t$exe" "$dist/bin/"; done
chmod 755 "$dist"/bin/*
cp "$build/COPYING" "$build/LICENSE" "$dist/licences/qemu/"

{
  cat "$build/build.txt"
  echo "programs before strip (du -sk, KiB): $(du -sk "$dist/bin" | cut -f1)"
  echo "runtime libraries before packing:"
  case $os in
    macos) otool -L "$dist/bin/qemu-system-arm" ;;
    *) ldd "$dist/bin/qemu-system-arm$exe" ;;
  esac
} >"$report"

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
      cp -L "$path" "$dist/lib/$so"
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
    "$dist/bin/qemu-system-arm" --version
    { echo "runtime libraries after packing:"; otool -L "$dist/bin/qemu-system-arm"; } >>"$report"
    ;;
  windows)
    rmdir "$dist/lib"
    for b in "$dist"/bin/*.exe; do
      ldd "$b" | awk '$3 ~ /^\/ucrt64\// {print $3}'
    done | sort -u | while read -r dll; do cp "$dll" "$dist/bin/"; done
    { echo "runtime libraries after packing:"; ldd "$dist/bin/qemu-system-arm.exe"; } >>"$report"
    ;;
  *)
    echo "unknown system: $os" >&2
    exit 2
    ;;
esac

{
  echo "packed files:"
  ls -lR "$dist"
  echo "packed size (du -sk, KiB): $(du -sk "$dist" | cut -f1)"
} >>"$report"
cat "$report"
