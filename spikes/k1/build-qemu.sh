#!/usr/bin/env bash
# Spike K1: build QEMU 10.1.2 for arm and aarch64, then pack it.
# Usage: build-qemu.sh <linux|macos|windows> <out-dir>
# Runs in CI only. Writes <out-dir>/qemu/ and <out-dir>/report.txt.
set -euxo pipefail

os=$1
out=$(mkdir -p "$2" && cd "$2" && pwd)
ver=10.1.2
# The pin RobCo-Core's ci/Dockerfile checks. QEMU itself publishes a GPG
# signature, not a SHA-256.
sha=9d75f331c1a5cb9b6eb8fd9f64f563ec2eab346c822cb97f8b35cd82d3f11479
work=$(mktemp -d)
report=$out/report.txt
dist=$out/qemu

cd "$work"
curl -fsSLo qemu.tar.xz "https://download.qemu.org/qemu-${ver}.tar.xz"
if command -v sha256sum >/dev/null; then
  echo "${sha}  qemu.tar.xz" | sha256sum -c -
else
  echo "${sha}  qemu.tar.xz" | shasum -a 256 -c -
fi
# Firmware sources and CI tooling hold symlinks Windows cannot make.
# The build needs neither.
tar xf qemu.tar.xz --exclude="qemu-${ver}/roms" --exclude="qemu-${ver}/tests/lcitool"
cd "qemu-${ver}"

configure=(./configure
  --target-list=arm-softmmu,aarch64-softmmu
  --without-default-features
  --enable-fdt=system
  --enable-pixman
  --disable-docs
  --disable-tools
  --disable-guest-agent
  --disable-slirp
  --disable-werror)

jobs=$(getconf _NPROCESSORS_ONLN 2>/dev/null || sysctl -n hw.ncpu)
start=$(date +%s)
"${configure[@]}"
make -j"$jobs"
secs=$(( $(date +%s) - start ))

exe=""
[ "$os" = windows ] && exe=.exe
mkdir -p "$dist/bin" "$dist/lib"
cp "build/qemu-system-arm$exe" "build/qemu-system-aarch64$exe" "$dist/bin/"
cp "$work/qemu.tar.xz" "$out/qemu-${ver}.tar.xz"

{
  echo "os: $os"
  echo "configure: ${configure[*]}"
  echo "build seconds (configure + make -j$jobs): $secs"
  echo "binary sizes:"
  ls -l "$dist/bin"
  echo "runtime libraries before packing:"
  case $os in
    linux) ldd "$dist/bin/qemu-system-arm" ;;
    macos) otool -L "$dist/bin/qemu-system-arm" ;;
    windows) ldd "$dist/bin/qemu-system-arm.exe" ;;
  esac
} >"$report"

# Drop debug symbols. Release users do not need them.
unstripped=$(du -sk "$dist/bin" | cut -f1)
case $os in
  macos) strip -x "$dist"/bin/* ;;
  *) strip "$dist"/bin/* ;;
esac
echo "binaries before strip (du -sk, KiB): $unstripped" >>"$report"

# Pack the libraries that do not come with the system next to the binaries.
case $os in
  linux)
    # Not packed: the spike records what it would take. See docs/spikes/K1.md.
    ;;
  macos)
    dylibbundler -of -b -x "$dist/bin/qemu-system-arm" -x "$dist/bin/qemu-system-aarch64" \
      -d "$dist/lib" -p @executable_path/../lib/
    for f in "$dist"/lib/*.dylib "$dist"/bin/*; do codesign --force -s - "$f"; done
    { echo "runtime libraries after packing:"; otool -L "$dist/bin/qemu-system-arm"; } >>"$report"
    ;;
  windows)
    for b in "$dist"/bin/*.exe; do
      ldd "$b" | awk '$3 ~ /^\/ucrt64\// {print $3}'
    done | sort -u | while read -r dll; do cp "$dll" "$dist/bin/"; done
    { echo "runtime libraries after packing:"; ldd "$dist/bin/qemu-system-arm.exe"; } >>"$report"
    ;;
esac

{
  echo "packed files:"
  ls -lR "$dist"
  echo "packed size (du -sk, KiB): $(du -sk "$dist" | cut -f1)"
} >>"$report"
cat "$report"
