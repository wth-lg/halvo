#!/bin/sh
# usage: sh tools/r4-trials/floor/run.sh
# One cell of the floor trial (contracts.md I11.4) on a hosted machine or a guest: builds the
# trial app and runs cell.mjs, which prints the cell line. Refuses with not_hosted where neither
# CI nor R4_GUEST is set. Exit: 0 when it printed its cell, 1 when the cell could not be made,
# 2 on a refusal.
set -u

if [ "$#" -ne 0 ]; then
  echo "REFUSED usage: sh tools/r4-trials/floor/run.sh takes no argument" >&2
  exit 2
fi
if [ -z "${CI:-}" ] && [ -z "${R4_GUEST:-}" ]; then
  echo "REFUSED not_hosted: neither CI nor R4_GUEST is set" >&2
  exit 2
fi

here=$(cd "$(dirname "$0")" && pwd) || exit 1
root=$(cd "$here/../../.." && pwd) || exit 1
machine=hosted-mac
exe=
case "$(uname -s)" in
  MINGW* | MSYS* | CYGWIN*)
    machine=hosted-win
    exe=.exe
    ;;
esac
if [ -n "${R4_GUEST:-}" ]; then
  machine=guest-mac14
fi

before=$(git -C "$root" status --porcelain --untracked-files=no 2>/dev/null)
if ! cargo build --release --locked --manifest-path "$root/tools/r4-trials/app/Cargo.toml" >&2; then
  echo "fail trial=floor machine=$machine step=build"
  exit 1
fi
after=$(git -C "$root" status --porcelain --untracked-files=no 2>/dev/null)
if [ "$before" != "$after" ]; then
  echo "fail trial=floor machine=$machine step=tree"
  exit 1
fi

target=${CARGO_TARGET_DIR:-$root/tools/r4-trials/app/target}
exec node "$here/cell.mjs" --bin "$target/release/r4-trial-app$exe"
