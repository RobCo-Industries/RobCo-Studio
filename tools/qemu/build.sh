#!/usr/bin/env bash
# Build QEMU 10.1.2 for arm and aarch64 with the suite's configure line.
# Usage: build.sh <linux|macos|windows> <out-dir>
# Runs in CI only. Writes <out-dir>/build/: the two programs, not stripped,
# QEMU's licence files, the source archive and build.txt.
# CI caches <out-dir>/build keyed on this file, which pins the source hash,
# and on prefix-map.sh.
# Packing lives in pack.sh so that a change to it does not rebuild.
set -euxo pipefail

here=$(cd "$(dirname "$0")" && pwd)
os=$1
out=$(mkdir -p "$2/build" && cd "$2/build" && pwd)
ver=10.1.2
# The pin RobCo-Core's ci/Dockerfile checks. QEMU itself publishes a GPG
# signature, not a SHA-256. Check the signature by hand when this moves.
sha=9d75f331c1a5cb9b6eb8fd9f64f563ec2eab346c822cb97f8b35cd82d3f11479
work=$(mktemp -d)

cd "$work"
curl -fsSLo qemu.tar.xz "https://download.qemu.org/qemu-${ver}.tar.xz"
if command -v sha256sum >/dev/null; then
  echo "${sha}  qemu.tar.xz" | sha256sum -c -
else
  echo "${sha}  qemu.tar.xz" | shasum -a 256 -c -
fi
# Firmware sources and CI tooling hold symlinks Windows cannot make.
# The build needs neither.
tar xf qemu.tar.xz --exclude="qemu-${ver}/roms" --exclude="qemu-${ver}/tests/lcitool/libvirt-ci"
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

# No path of this machine in the programs: map the build folder away in
# __FILE__ and debug information. One --extra-cflags holds every flag.
cflags=$(bash "$here/prefix-map.sh" "$os" "$work")
if [ "$os" = macos ]; then
  # Homebrew's libfdt has no pkg-config file that meson finds.
  brew=$(brew --prefix)
  cflags="$cflags -I$brew/include"
  configure+=(--extra-ldflags="-L$brew/lib")
fi
configure+=(--extra-cflags="$cflags")

jobs=$(getconf _NPROCESSORS_ONLN 2>/dev/null || sysctl -n hw.ncpu)
start=$(date +%s)
"${configure[@]}"
# Only the two emulators: the unit tests do not link on MinGW in 10.1.2
# (test-vmstate, qemu_ftruncate64) and Studio ships none of them.
exe=""; sfx=""
[ "$os" = windows ] && exe=.exe
[ "$os" = macos ] && sfx=-unsigned
ninja -C build -j"$jobs" "qemu-system-arm$sfx$exe" "qemu-system-aarch64$sfx$exe"
secs=$(( $(date +%s) - start ))

# On macOS the build leaves "-unsigned" programs. pack.sh signs them.
for t in arm aarch64; do cp "build/qemu-system-$t$sfx$exe" "$out/qemu-system-$t$exe"; done
cp COPYING LICENSE "$out/"
cp "$work/qemu.tar.xz" "$out/qemu-${ver}.tar.xz"

{
  echo "os: $os"
  # The release checks that no packed file holds this folder's path.
  echo "work: $work"
  # The release publishes this line, so it names the folder <work>.
  line=${configure[*]}
  if [ "$os" = windows ]; then line=${line//"$(cygpath -m "$work")"/<work>}; fi
  echo "configure: ${line//"$work"/<work>}"
  echo "build seconds (configure + ninja -j$jobs, two emulators): $secs"
  ls -l "$out"
} >"$out/build.txt"
cat "$out/build.txt"
