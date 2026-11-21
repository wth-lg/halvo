#!/bin/sh
# usage: sh .scratch/u26b/red-step.sh <id> <target file> <copy file> <full test name>...
# A red step of the U2.6b scratch run (never merged): puts the copy in the place of the target
# file, runs the named lib tests of halvo-web by --exact, puts the target back with git checkout,
# and prints the test lines, the rows that read otherwise, the panics and the result line. The step
# is read by libtest's result line, never by an exit code: it exits 0.
set +e
id=$1
target=$2
copy=$3
shift 3
out="${RUNNER_TEMP:-/tmp}/red-$id.txt"
cp "$copy" "$target"
cargo test -p halvo-web --locked --lib -- --exact --nocapture --test-threads=1 "$@" >"$out" 2>&1
code=$?
git checkout -- "$target"
grep -E '^test |^test result|rows read otherwise|rows ended otherwise|panicked at|^assertion|^  left:|^ right:|fence|^rc scene' "$out"
echo "red-step id=$id cargo_exit=$code"
echo "red-step id=$id tree_lines_after_restore=$(git status --porcelain | wc -l | tr -d ' ')"
exit 0
