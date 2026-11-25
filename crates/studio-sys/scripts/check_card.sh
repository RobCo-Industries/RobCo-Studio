#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Reads a card written by the studio-sys card test with mtools, never mounting it.
#   scripts/check_card.sh <card.img>
# Lists the volume, then copies every file of <card>.tree out of the image
# and compares it byte for byte. Exits non-zero on any difference.
set -euo pipefail

img=$1
tree=${img%.img}.tree
part="$img@@$((2048 * 512))"
export MTOOLS_SKIP_CHECK=1

[ -d "$tree" ] || { echo "check_card: no $tree" >&2; exit 1; }
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT

echo "== minfo"
minfo -i "$part" ::
echo "== mdir -/"
mdir -i "$part" -/ ::

n=0
while IFS= read -r -d '' f; do
    rel=${f#"$tree"/}
    mcopy -n -i "$part" "::/$rel" "$out/got" 2>/dev/null || { echo "check_card: missing $rel" >&2; exit 1; }
    cmp "$f" "$out/got" || { echo "check_card: $rel differs" >&2; exit 1; }
    rm -f "$out/got"
    echo "same bytes: $rel ($(wc -c < "$f" | tr -d ' ') bytes)"
    n=$((n + 1))
done < <(find "$tree" -type f -print0)

[ "$n" -gt 0 ] || { echo "check_card: no files compared" >&2; exit 1; }
echo "check_card: $n files read back by mtools with the same bytes"
