#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Shows that checks 2, 3 and 5 of tools/agreement.sh can fail. Check 5: two
# files that differ in one byte must give exit 2 and name the byte and its
# header field, and two equal files must pass. Check 3: two gcc programs
# that print the same version but differ in one byte must give exit 2, and
# one program against its copy must pass. Needs bash, python3, cmp and sed.
set -euo pipefail

here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
a="$tmp/sealed by rimg.bin"
b="$tmp/sealed by c.bin"

python3 -I -c 'import sys; open(sys.argv[1], "wb").write(bytes(range(256)) * 2)' "$a"
cp "$a" "$b"
bash "$here/agreement.sh" compare-sealed "$a" "$b" >/dev/null ||
    { echo "test_agreement: equal files did not pass" >&2; exit 1; }

# One changed byte at each boundary must name its field. $1 is the flags
# byte 0x2c of both files, $2 the byte to change, $3 the field. The change
# keeps bit 0, so the META block stays present or absent.
expect_field() {
    python3 -I -c '
import sys
d = bytearray(range(256)) * 2
d[0x2c] = int(sys.argv[3], 0)
open(sys.argv[1], "wb").write(d)
d[int(sys.argv[2], 0)] ^= 0x80
open(sys.argv[4], "wb").write(d)' "$a" "$2" "$1" "$b"
    set +e
    msg=$(bash "$here/agreement.sh" compare-sealed "$a" "$b" 2>&1)
    code=$?
    set -e
    [ "$code" -eq 2 ] || { echo "test_agreement: $2: exit $code, not 2: $msg" >&2; exit 1; }
    want="agreement: FAILED: 5. sealers: rimg.py and the C sealer differ, first difference at byte $2, $3"
    [ "$msg" = "$want" ] || { echo "test_agreement: $2: got: $msg" >&2; exit 1; }
}
# IMAGE_FORMAT.md: the first and last byte of each field.
for c in "0x0 branch stub" "0x1f branch stub" "0x20 magic" "0x27 magic" \
    "0x28 hdr_version" "0x29 hdr_version" "0x2a hdr_size" "0x2b hdr_size" \
    "0x2c flags" "0x2f flags" "0x30 load_addr" "0x33 load_addr" \
    "0x34 file_size" "0x37 file_size" "0x38 mem_size" "0x3b mem_size" \
    "0x3c crc32" "0x3f crc32" "0x40 abi_version" "0x43 abi_version" \
    "0x44 image_version" "0x47 image_version" "0x48 subtype" "0x4b subtype" \
    "0x4c name" "0x5b name" "0x5c reserved" "0x5f reserved" \
    "0x60 META title" "0x9f META title" "0xa0 META icon" "0x11f META icon" \
    "0x120 code and data"; do
    expect_field 0x01 "${c%% *}" "${c#* }"
done
# No META block: code and data start at 0x60.
expect_field 0x00 0x5f reserved
expect_field 0x00 0x60 "code and data"

# Check 3: two stand-in gcc programs with the same version text.
g1="$tmp/make bin/arm-none-eabi-gcc"
g2="$tmp/robco bin/arm-none-eabi-gcc"
mkdir -p "${g1%/*}" "${g2%/*}"
printf '#!/bin/sh\necho 15.2.1\n' > "$g1"
cp "$g1" "$g2"
chmod +x "$g1" "$g2"
bash "$here/agreement.sh" compare-gcc "$g1" "$g2" >/dev/null ||
    { echo "test_agreement: one gcc program did not pass against its copy" >&2; exit 1; }
printf '\n' >> "$g2"
[ "$("$g1")" = "$("$g2")" ] || { echo "test_agreement: the version texts differ" >&2; exit 1; }
set +e
msg=$(bash "$here/agreement.sh" compare-gcc "$g1" "$g2" 2>&1)
code=$?
set -e
[ "$code" -eq 2 ] || { echo "test_agreement: gcc: exit $code, not 2: $msg" >&2; exit 1; }
case $msg in
    "agreement: FAILED: 3. gcc: the gcc program Make ran has SHA-256 "*) ;;
    *) echo "test_agreement: gcc: got: $msg" >&2; exit 1 ;;
esac
# Check 2: the ABI comes from the Makefile, never from the caller. A stand-in
# project: a pipos/abi.h with the SDK's default 1.4 and a pipos-app.mk
# that passes ABI as -DPIPOS_ABI_TARGET, as the SDK's does.
cc=$(command -v cc || command -v gcc || command -v arm-none-eabi-gcc) ||
    { echo "test_agreement: no C compiler" >&2; exit 1; }
# make splits paths at white space, so this one holds none.
proj="$tmp/project"
mkdir -p "$proj/include/pipos"
printf '#ifndef PIPOS_ABI_TARGET\n#define PIPOS_ABI_TARGET 0x00010004\n#endif\n' > "$proj/include/pipos/abi.h"
cat > "$proj/app.mk" <<EOF
PIP_CC := $cc
PIP_CFLAGS := -O2 -MMD -I\$(CURDIR)/include \$(if \$(ABI),-DPIPOS_ABI_TARGET=0x0001000\$(word 2,\$(subst ., ,\$(ABI))))
EOF
printf 'ABI := 1.2\ninclude app.mk\n' > "$proj/Makefile"
# A caller's ABI in the environment must not hide the Makefile's.
got=$(ABI=1.3 bash "$here/agreement.sh" make-abi "$proj")
[ "$got" = "1.2 1.2" ] || { echo "test_agreement: make-abi with ABI := 1.2 gave $got" >&2; exit 1; }
printf 'include app.mk\n' > "$proj/Makefile"
got=$(bash "$here/agreement.sh" make-abi "$proj")
[ "$got" = "1.4 default" ] || { echo "test_agreement: make-abi with no ABI gave $got" >&2; exit 1; }
# Three ABIs: each pair that differs fails and is named.
python3 -I -c '
import sys
for path, v in zip(sys.argv[1:], (0x00010004, 0x00010002)):
    d = bytearray(0x120); d[0x40:0x44] = v.to_bytes(4, "little"); open(path, "wb").write(d)' "$a" "$b"
bash "$here/agreement.sh" compare-abi 1.4 "$a" "$a" >/dev/null ||
    { echo "test_agreement: three equal ABIs did not pass" >&2; exit 1; }
expect_abi() {
    set +e
    msg=$(bash "$here/agreement.sh" compare-abi "$1" "$2" "$3" 2>&1)
    code=$?
    set -e
    [ "$code" -eq 2 ] && [ "$msg" = "agreement: FAILED: 2. abi: $4" ] ||
        { echo "test_agreement: abi: exit $code: $msg" >&2; exit 1; }
}
expect_abi 1.2 "$a" "$a" "Make builds with ABI 1.2, the header of the Make tape holds 1.4"
expect_abi 1.4 "$a" "$b" "Make builds with ABI 1.4, the header of the robco build tape holds 1.2"
expect_abi 1.4 "$b" "$a" "Make builds with ABI 1.4, the header of the Make tape holds 1.2"
echo "test_agreement: ok"
