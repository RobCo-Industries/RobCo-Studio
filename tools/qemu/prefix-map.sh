#!/usr/bin/env bash
# Print the compiler flags that keep the build folder out of the programs.
# Usage: prefix-map.sh <linux|macos|windows> <work-dir>
# Each spelling of <work-dir> the compiler may see is mapped to ".": the
# path as given, its physical path, as macOS's /var is /private/var, and on
# Windows its C:/ form, which MinGW's compiler sees. -ffile-prefix-map
# covers __FILE__ in assertions and the paths of debug information.
# build.sh calls it; the workflow keys the build cache on both files.
set -euo pipefail

os=$1
work=$2
[ -d "$work" ] || { echo "no folder $work" >&2; exit 1; }
spellings=("$work" "$(cd "$work" && pwd -P)")
if [ "$os" = windows ]; then spellings+=("$(cygpath -m "$work")"); fi
flags=()
for s in "${spellings[@]}"; do
  case " ${flags[*]-} " in *" -ffile-prefix-map=$s=. "*) continue ;; esac
  flags+=("-ffile-prefix-map=$s=.")
done
echo "${flags[*]}"
