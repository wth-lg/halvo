#!/bin/sh
# usage: sh tools/r4-trials/floor/selftest.sh [DIR]
# Runs the floor self-test and prints its output, then fails unless its last count line says
# K of K with K above 0 (count.mjs), whatever the exit status of the runner was.
# Exit: the runner's status when the count line holds, 1 otherwise.
here=$(cd "$(dirname "$0")" && pwd) || exit 1
out=$(node "$here/selftest.mjs" "$@")
code=$?
printf '%s\n' "$out"
if ! node "$here/count.mjs" "$out"; then
  exit 1
fi
exit "$code"
