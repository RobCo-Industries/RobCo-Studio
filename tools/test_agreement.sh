#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Shows that checks 3 and 5 of tools/agreement.sh can fail. Check 5: two
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

# Byte 0x28 is the first byte of header_size.
python3 -I -c 'import sys; d = bytearray(open(sys.argv[1], "rb").read()); d[0x28] ^= 1; open(sys.argv[1], "wb").write(d)' "$b"
set +e
msg=$(bash "$here/agreement.sh" compare-sealed "$a" "$b" 2>&1)
code=$?
set -e
[ "$code" -eq 2 ] || { echo "test_agreement: exit $code, not 2: $msg" >&2; exit 1; }
want="agreement: FAILED: 5. sealers: rimg.py and the C sealer differ, first difference at byte 0x28, header_size"
[ "$msg" = "$want" ] || { echo "test_agreement: got: $msg" >&2; exit 1; }

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
echo "test_agreement: ok"
