#!/usr/bin/env bash
# Spike K5 benchmark, one command:
#   spikes/k5/bench.sh ROBCO_CORE_DIR [RESULT_FILE]
# Builds the animated guest at 640x480 and 320x240 and the firmware stub
# under spikes/k5/.cache, builds the poller, then runs one QEMU at a time:
# screendump on each board and size, then VNC on each board at 640x480.
# Each QEMU lives at most 20 s. Output goes to stdout and RESULT_FILE.
# K5_BOARDS, K5_SIZES and K5_MODES narrow the runs. K5_QEMU_DIR picks a
# QEMU build directory. Prints no path, user or host name.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
core="$(cd "${1:?usage: bench.sh ROBCO_CORE_DIR [RESULT_FILE]}" && pwd)"
out="${2:-/dev/null}"
boards="${K5_BOARDS:-raspi0 raspi2b raspi3b}"
sizes="${K5_SIZES:-640x480 320x240}"
modes="${K5_MODES:-screendump vnc}"
cache="$here/.cache"
mkdir -p "$cache/fwstub" "$cache/tmp"

cross="${CROSS:-arm-none-eabi-}"
"${cross}gcc" -mcpu=arm1176jzf-s -marm -nostdlib -Wl,-Ttext=0x0 -Wl,--build-id=none \
  -Wl,--no-warn-rwx-segments -o "$cache/fwstub/fwstub.elf" "$core/tests/fwstub/fwstub.S"
"${cross}objcopy" -O binary "$cache/fwstub/fwstub.elf" "$cache/fwstub/fwstub.bin"
for s in $sizes; do
  make -s -C "$here/guest" CI=1 ROBCO_CORE="$core" BUILD="$cache/guest-$s" K5_W="${s%x*}" K5_H="${s#*x}" >/dev/null
done
(cd "$here" && cargo build -j 3 --release --locked -q)
bin="$here/target/release/k5"
qdir=()
if [ -n "${K5_QEMU_DIR:-}" ]; then qdir=(--qemu-dir "$K5_QEMU_DIR"); fi

status=0
{
  echo "# spike K5 benchmark, $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "# host: $(uname -s) $(uname -m), cpus: $(getconf _NPROCESSORS_ONLN)"
  echo "# load averages before: $(uptime | sed 's/.*load average[s]*: //')"
  echo "# robco-core: $(git -C "$core" rev-parse --short=12 HEAD 2>/dev/null || echo unknown)"
} | tee "$out"
for mode in $modes; do
  for b in $boards; do
    for s in $sizes; do
      if [ "$mode" = vnc ] && [ "$s" != 640x480 ]; then continue; fi
      echo "## $mode $b $s, load averages: $(uptime | sed 's/.*load average[s]*: //')" | tee -a "$out"
      if ! "$bin" "$mode" --board "$b" --size "$s" --guest "$cache/guest-$s/k5.img" \
          --fwstub "$cache/fwstub/fwstub.bin" --tmp "$cache/tmp" ${qdir[@]+"${qdir[@]}"} | tee -a "$out"; then
        status=1
      fi
    done
  done
done
echo "# load averages after: $(uptime | sed 's/.*load average[s]*: //')" | tee -a "$out"
exit $status
