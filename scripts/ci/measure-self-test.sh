#!/bin/sh
# usage: sh scripts/ci/measure-self-test.sh <job> <n|default>
# Runs the driver's self-test step with CI_D_SELF_TEST_JOBS=n (or unset), keeps its
# standard output and standard error under target/ci-artifacts/measure/, prints both,
# and prints one line: measure: job, jobs, exit status and whole seconds.
set -u
job=$1
n=$2
out=target/ci-artifacts/measure
mkdir -p "$out"
if [ "$n" != default ]; then
  CI_D_SELF_TEST_JOBS=$n
  export CI_D_SELF_TEST_JOBS
fi
start=$(date +%s)
sh scripts/ci/run-ci-d.sh "$job" --self-test >"$out/self-test-$n.out" 2>"$out/self-test-$n.err"
code=$?
end=$(date +%s)
cat "$out/self-test-$n.out"
cat "$out/self-test-$n.err" >&2
printf 'measure: job=%s jobs=%s exit=%s seconds=%s\n' "$job" "$n" "$code" "$((end - start))"
exit "$code"
