#!/bin/sh
# One round of the loop of round 1 of U3.4 on a scratch branch: each test that the unit adds or changes runs by its name
# with --exact from the test binaries that cargo built for halvo-agent-store, and a round passes only when the
# result line of each binary shows the expected count passed and none failed. Prints counts alone.
set -u
dir="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/u34-loop"
list="$dir/binaries.txt"
mkdir -p "$dir" || exit 1
if [ ! -s "$list" ]; then
  cargo test -p halvo-agent-store --locked --no-run --message-format=json >"$dir/build.json" 2>"$dir/build.err" || {
    echo "loop-u34: build failed"
    exit 1
  }
  sed -n 's/.*"target":{[^}]*"name":"\([a-z_]*\)"[^}]*"test":true}.*"executable":"\([^"]*\)".*/\1 \2/p' "$dir/build.json" |
    sed 's#\\\\#/#g' >"$list.new" || exit 1
  mv "$list.new" "$list" || exit 1
fi
round=0
check() {
  target=$1
  shift
  want=$#
  bin=$(awk -v t="$target" '$1 == t { print $2; exit }' "$list") || bin=
  if [ -z "$bin" ]; then
    echo "loop-u34: no test binary for $target"
    exit 1
  fi
  "$bin" --exact "$@" >"$dir/out.txt" 2>&1
  if ! grep -q "^test result: ok\. $want passed; 0 failed; 0 ignored; 0 measured; [0-9]* filtered out" "$dir/out.txt"; then
    echo "loop-u34: $target did not pass $want tests"
    tail -n 30 "$dir/out.txt"
    exit 1
  fi
  round=$((round + want))
}
check replies \
  a_repeat_of_a_begun_row_a_day_later_with_the_day_of_its_at_is_answered_as_the_first \
  after_a_run_s_end_a_repeat_of_the_newest_row_is_answered_as_the_first_and_a_new_row_is_refused
if [ "$round" -ne 2 ]; then
  echo "loop-u34: $round tests passed, not 2"
  exit 1
fi
echo "loop-u34: 2 tests in 1 binaries passed"
