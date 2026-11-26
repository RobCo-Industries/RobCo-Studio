#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Usage: sums.sh <dir> <list-name>
#
# Writes <dir>/<list-name>: one line per regular file of <dir>, sorted by
# name in byte order, in the form of sha256sum and shasum -a 256:
# 64 lowercase hex digits, two spaces, the file name. The list leaves
# itself out. Then checks the list against the files.
set -euo pipefail
export LC_ALL=C

dir=${1:?usage: sums.sh <dir> <list-name>}
list=${2:?usage: sums.sh <dir> <list-name>}
case $list in */*|"") echo "sums.sh: the list name has no folder" >&2; exit 2 ;; esac

if command -v sha256sum >/dev/null; then sum=(sha256sum); else sum=(shasum -a 256); fi

cd "$dir"
rm -f -- "$list"
files=()
while IFS= read -r f; do files+=("$f"); done < <(find . -maxdepth 1 -type f ! -name "$list" | sed 's|^\./||' | sort)
if [ "${#files[@]}" -eq 0 ]; then echo "sums.sh: no file in $dir" >&2; exit 1; fi
for f in "${files[@]}"; do
  case $f in *[[:space:]]*|*\\*) echo "sums.sh: file name with a space or backslash: $f" >&2; exit 1 ;; esac
done
"${sum[@]}" -- "${files[@]}" > "$list.tmp"
mv "$list.tmp" "$list"
if grep -qvE '^[0-9a-f]{64}  [^ /]+$' "$list"; then
  echo "sums.sh: a line of $list is not '<sha256>  <name>'" >&2; exit 1
fi
"${sum[@]}" -c "$list" >/dev/null
cat "$list"
