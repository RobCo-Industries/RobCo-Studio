#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The agreement check: one holotape built by Pip-OS's Makefile and by
# `robco build` must give the same sealed tape, and one raw image sealed by
# RobCo-Core's rimg.py and by the C sealer must give the same bytes.
#
# It builds sdk/examples/tape of a Pip-OS checkout by Make, writes a
# robco.toml from the Makefile's own variables, builds the same folder with
# `robco build` from a pipos package and a compiler archive, then runs the
# checks of RobCo-Core's STUDIO_PROJECT.md, "A project that also builds
# with a Makefile", in their order:
#
#   1. The sources of the result line equal the words of SRCS.
#   2. The abi of the result line equals the Makefile's ABI.
#   3. The gcc of the result line equals the release of the compiler Make ran.
#   4. The two tapes have the same SHA-256.
#
# then 5: rimg.py and the C sealer seal one raw image to the same bytes.
#
# Inputs, all from the environment:
#
#   PIPOS_DIR    a Pip-OS checkout, with its submodule, at the commit the
#                package was built from
#   PIPOS_ZIP    that pipos package zip
#   AGREE_GCC_ARCHIVE  the compiler archive the package names, for this host
#   AGREE_GCC_ROOT     the folder inside it that holds bin/, empty for the top
#   CROSS        the compiler prefix Make uses, such as .../bin/arm-none-eabi-
#   AGREE_GCC_RELEASE  the release of that compiler, such as 15.2.1-1.1
#   ABI          the target ABI both builds state, such as 1.4
#   The AGREE_ names keep off gcc, which reads GCC_ROOT from the
#   environment and then finds no stddef.h.
#   OUT          a folder that does not exist yet; everything goes there
#   MAKE_EXTRA   optional words added to the make command line. Only to
#                show that the check fails, such as CFLAGS_EXTRA=-O1.
#
# It writes OUT/hashes.txt before any check runs, so a failed run still
# shows its hashes. Exit 0: everything agrees. Exit 2: a check failed.
# Exit 1: the run could not get as far as the checks.
#
# Relative paths are fine: the script turns PIPOS_DIR, PIPOS_ZIP,
# AGREE_GCC_ARCHIVE and a CROSS that holds a slash into absolute paths before
# it starts, since make and cargo run the builds from other folders. OUT,
# PIPOS_DIR and CROSS cannot hold white space, since make reads them.
#
# Run it from any folder. It needs bash 3.2 or later, cargo,
# make, python3, cmp, sed, and the compiler CROSS names. It uses only cd and
# pwd to resolve paths, never readlink -f or realpath.
#
# `tools/agreement.sh compare-sealed A B` runs only check 5's comparison on
# two files, with the same exit codes. tools/test_agreement.sh uses it.
set -euo pipefail

die() { echo "agreement: $*" >&2; exit 1; }
fail() { echo "agreement: FAILED: $*" >&2; exit 2; }

# The first byte that differs, and the header field it falls in.
first_diff() {
    local at
    at=$(cmp "$1" "$2" 2>/dev/null | sed -En "s/.* (byte|char) ([0-9]+).*/\2/p" || true)
    if [ -z "$at" ]; then
        echo "one file is a prefix of the other: sizes $(($(wc -c <"$1"))) and $(($(wc -c <"$2")))"
        return
    fi
    at=$((at - 1))
    local field="code or data after the header"
    if [ "$at" -lt 32 ]; then field="branch stub"
    elif [ "$at" -lt 92 ]; then
        case $(((at - 32) / 4)) in
            0 | 1) field="magic" ;; 2) field="header_size" ;; 3) field="flags" ;;
            4) field="load_addr" ;; 5) field="file_size" ;; 6) field="mem_size" ;;
            7) field="crc32" ;; 8) field="abi_version" ;; 9) field="image_version" ;;
            10) field="subtype" ;; *) field="name" ;;
        esac
    elif [ "$at" -lt 96 ]; then field="META block start"
    elif [ "$at" -lt 160 ]; then field="META title"
    elif [ "$at" -lt 288 ]; then field="META icon"
    fi
    printf 'first difference at byte 0x%x, %s\n' "$at" "$field"
}

# Check 5: the file sealed by rimg.py ($1) and by the C sealer ($2).
compare_sealed() {
    cmp -s "$1" "$2" ||
        fail "5. sealers: rimg.py and the C sealer differ, $(first_diff "$1" "$2")"
}

if [ "${1-}" = compare-sealed ]; then
    [ "$#" -eq 3 ] || die "usage: $0 compare-sealed A B"
    [ -f "$2" ] && [ -f "$3" ] || die "compare-sealed takes two files"
    compare_sealed "$2" "$3"
    echo "agreement: the sealed files are equal"
    exit 0
fi

# The absolute form of an existing path, by cd and pwd only. $1 names the
# variable, $2 is file or dir.
abs_path() {
    local name=$1 kind=$2 p=${!1} d
    case $kind in
        file) [ -f "$p" ] || die "$name is not a file: $p" ;;
        dir) [ -d "$p" ] || die "$name is not a folder: $p" ;;
    esac
    if [ "$kind" = dir ]; then
        (CDPATH='' cd -- "$p" && pwd -P) || die "$name: cannot enter $p"
        return
    fi
    case $p in */*) d=${p%/*} ;; *) d=. ;; esac
    d=$(CDPATH='' cd -- "${d:-/}" && pwd -P) || die "$name: cannot enter the folder of $p"
    printf '%s/%s\n' "${d%/}" "${p##*/}"
}

for v in PIPOS_DIR PIPOS_ZIP AGREE_GCC_ARCHIVE CROSS AGREE_GCC_RELEASE ABI OUT; do
    [ -n "${!v:-}" ] || die "$v is not set"
done
AGREE_GCC_ROOT=${AGREE_GCC_ROOT-}
MAKE_EXTRA=${MAKE_EXTRA-}
PIPOS_DIR=$(abs_path PIPOS_DIR dir)
PIPOS_ZIP=$(abs_path PIPOS_ZIP file)
AGREE_GCC_ARCHIVE=$(abs_path AGREE_GCC_ARCHIVE file)
# A CROSS with a slash is a path; make runs in another folder. A bare prefix
# is looked up in PATH and stays as it is.
case $CROSS in
    */*)
        cross_dir=$(CDPATH='' cd -- "${CROSS%/*}/" && pwd -P) ||
            die "CROSS is not in a folder that exists: $CROSS"
        CROSS="$cross_dir/${CROSS##*/}"
        ;;
esac
[ -x "${CROSS}gcc" ] || command -v "${CROSS}gcc" >/dev/null || die "CROSS names no compiler: ${CROSS}gcc"
[ ! -e "$OUT" ] || die "$OUT exists; give a new folder"
# make splits its paths at white space, so the ones it reads cannot hold it.
for v in OUT PIPOS_DIR CROSS; do
    case ${!v} in *[[:space:]]*) die "$v holds white space, which make cannot use: ${!v}" ;; esac
done
mkdir -p "$OUT"
OUT=$(CDPATH='' cd -- "$OUT" && pwd -P)
# Every path is absolute now: work from the root of RobCo-Studio.
CDPATH='' cd -- "$(dirname -- "$0")/.." || die "cannot enter the root of RobCo-Studio"
[ -f Cargo.toml ] && [ -f external/robco-core/tools/rimg.py ] ||
    die "$(pwd -P) is not RobCo-Studio with its submodule"

sha() {
    if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi |
        cut -d' ' -f1
}

PROJ="$PIPOS_DIR/sdk/examples/tape"
[ -f "$PROJ/Makefile" ] || die "$PROJ holds no Makefile"

# The Makefile's variables, as make expands them. A second makefile prints
# them; the example's own Makefile is read first, as `make` would.
cat > "$OUT/print.mk" <<'EOF'
agreement-print:
	@printf '%s\n' '$(NAME)' '$(TITLE)' '$(VERSION)' '$(ICON)' '$(KIND)' '$(SUBTYPE)' '$(SRCS)' '$(IMAGE)'
EOF
# shellcheck disable=SC2086 # MAKE_EXTRA is a list of words.
make_args=(-C "$PROJ" -f Makefile -f "$OUT/print.mk" BUILD="$OUT/make" ABI="$ABI"
    CROSS="$CROSS" PYTHON=python3 $MAKE_EXTRA)
vars=()
while IFS= read -r line; do vars+=("$line"); done < <(make -s "${make_args[@]}" agreement-print)
[ "${#vars[@]}" -eq 8 ] || die "make printed ${#vars[@]} values, not 8"
name=${vars[0]} title=${vars[1]} version=${vars[2]} icon=${vars[3]}
kind=${vars[4]} subtype=${vars[5]} srcs=${vars[6]} image=${vars[7]}
[ "$kind" = holotape ] || die "KIND is $kind; only holotape is compared"
case $subtype in
    "" | 0 | game) subtype=game ;;
    1 | utility) subtype=utility ;;
    *) die "SUBTYPE $subtype has no robco.toml value" ;;
esac
case "$name$title$version$icon$srcs" in
    *'"'* | *\\*) die "a Makefile value holds a quote or a back slash" ;;
esac

echo "agreement: Make build of sdk/examples/tape"
make "${make_args[@]}" all >"$OUT/make.log" 2>&1 || { cat "$OUT/make.log" >&2; die "make failed"; }
make_tape="$OUT/make/$image"
[ -f "$make_tape" ] || die "make wrote no $image"

# The manifest, from the Makefile's values. Sources keep SRCS's order.
src_list=""
for s in $srcs; do src_list="$src_list${src_list:+, }\"$s\""; done
mkdir -p "$OUT/project"
for s in $srcs $icon; do
    mkdir -p "$OUT/project/$(dirname "$s")"
    cp "$PROJ/$s" "$OUT/project/$s"
done
{
    printf 'format = 1\n\n[project]\n'
    printf 'name = "%s"\ntitle = "%s"\nversion = "%s"\n' "$name" "$title" "$version"
    printf 'kind = "holotape"\nsubtype = "%s"\n' "$subtype"
    [ -z "$icon" ] || printf 'icon = "%s"\n' "$icon"
    printf '\n[target]\nplatform = "pipos"\nabi = "%s"\n' "$ABI"
    printf '\n[build]\nsources = [%s]\n' "$src_list"
} > "$OUT/project/robco.toml"

echo "agreement: robco build of the same folder"
mkdir -p "$OUT/studio"
ROBCO_TEST_PIPOS="$PIPOS_ZIP" ROBCO_TEST_GCC="$AGREE_GCC_ARCHIVE" ROBCO_TEST_GCC_ROOT="$AGREE_GCC_ROOT" \
    ROBCO_TEST_PROJECT="$OUT/project" ROBCO_TEST_PROJECT_OUT="$OUT/studio" \
    cargo test --locked -q -p robco --test build -- --exact build_the_project_the_environment_names \
    >"$OUT/studio.log" 2>&1 || { cat "$OUT/studio.log" >&2; die "robco build failed"; }
studio_tape="$OUT/studio/$image"
[ -f "$studio_tape" ] && [ -f "$OUT/studio/result.json" ] ||
    { cat "$OUT/studio.log" >&2; die "the test wrote no $image or result.json"; }

echo "agreement: one raw image, sealed by rimg.py and by the C sealer"
mkdir -p "$OUT/seal"
raw="$OUT/seal/raw.bin"
"${CROSS}objcopy" -O binary "$OUT/make/${image%.*}.elf" "$raw"
seal_args=(--name "$name" --image-version "$version" --title "$title")
[ -z "$icon" ] || seal_args+=(--icon "$PROJ/$icon")
python3 -I external/robco-core/tools/rimg.py seal "$raw" -o "$OUT/seal/rimg.bin" "${seal_args[@]}"
ROBCO_TEST_SEAL_RAW="$raw" ROBCO_TEST_SEAL_OUT="$OUT/seal/c.bin" \
    ROBCO_TEST_SEAL_NAME="$name" ROBCO_TEST_SEAL_VERSION="$version" \
    ROBCO_TEST_SEAL_TITLE="$title" ROBCO_TEST_SEAL_ICON="${icon:+$PROJ/$icon}" \
    cargo test --locked -q -p studio-sys --test seal_file >"$OUT/seal.log" 2>&1 ||
    { cat "$OUT/seal.log" >&2; die "the C sealer failed"; }
[ -f "$OUT/seal/c.bin" ] || die "the C sealer wrote nothing"

# Names only, never a path: the file is published.
{
    echo "$(sha "$make_tape")  make/$image"
    echo "$(sha "$studio_tape")  robco/$image"
    echo "$(sha "$OUT/seal/rimg.bin")  sealed-by-rimg.py"
    echo "$(sha "$OUT/seal/c.bin")  sealed-by-c"
} > "$OUT/hashes.txt"
cat "$OUT/hashes.txt"

result=$(cat "$OUT/studio/result.json")
field() { python3 -I -c 'import json,sys; print(json.dumps(json.loads(sys.argv[1])[sys.argv[2]]))' "$result" "$1"; }
want_srcs=$(python3 -I -c 'import json,sys; print(json.dumps(sys.argv[1].split()))' "$srcs")
got_srcs=$(field sources)
[ "$got_srcs" = "$want_srcs" ] ||
    fail "1. sources: robco build has $got_srcs, SRCS has $want_srcs"
[ "$(field abi)" = "\"$ABI\"" ] || fail "2. abi: robco build has $(field abi), the Makefile has $ABI"
dump=$("${CROSS}gcc" -dumpversion)
case $AGREE_GCC_RELEASE in
    "$dump" | "$dump"-*) ;;
    *) fail "3. gcc: the compiler Make ran says $dump, not release $AGREE_GCC_RELEASE" ;;
esac
[ "$(field gcc)" = "\"$AGREE_GCC_RELEASE\"" ] ||
    fail "3. gcc: robco build used $(field gcc), Make ran $AGREE_GCC_RELEASE"
cmp -s "$make_tape" "$studio_tape" ||
    fail "4. tapes: $image by Make and by robco build differ, $(first_diff "$make_tape" "$studio_tape")"
compare_sealed "$OUT/seal/rimg.bin" "$OUT/seal/c.bin"
echo "agreement: the tapes and the sealed files are equal"
