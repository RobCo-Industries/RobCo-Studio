#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Shows that check 5 of tools/agreement.sh can fail: two files that differ
# in one byte must give exit 2 and name the byte and its header field, and
# two equal files must pass. Needs bash, python3, cmp and sed.
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
echo "test_agreement: ok"
