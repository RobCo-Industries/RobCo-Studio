#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Usage: TAG=<tag> VERSION=<cargo version> check-tag.sh
#
# Exits 0 when TAG is exactly robco-<VERSION>-pre.<n>, where <n> is a
# number without a leading zero, such as robco-0.1.0-pre.1. Exits 1 for
# any other tag. Both values come through the environment, never through
# the command line of a workflow, and are compared as strings.
set -euo pipefail

tag=${TAG-}
version=${VERSION-}
fail() { echo "check-tag.sh: $*" >&2; exit 1; }

[[ $version =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] \
  || fail "the version '$version' is not X.Y.Z"
prefix="robco-$version-pre."
[ "${tag:0:${#prefix}}" = "$prefix" ] || fail "the tag '$tag' does not start with '$prefix'"
n=${tag:${#prefix}}
[[ $n =~ ^[1-9][0-9]{0,3}$ ]] || fail "the tag '$tag' does not end with a pre-release number 1 to 9999"
echo "tag $tag: robco $version, pre-release $n"
