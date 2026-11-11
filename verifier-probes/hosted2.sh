#!/bin/sh
# sh verifier-probes/hosted2.sh JOB: changed copies of the egress check on a hosted runner, one rule
# each, for the changes that cannot run on the development Mac (a probe's packet could leave) or
# that concern the wfp mode. For each: the self-test's lines and exit status; where the self-test
# passes, also test.sh for the mode and the per-push file through the driver. Always exits 0.
set -u
job=$1
cd "$GITHUB_WORKSPACE" || exit 0
case $job in
  macos) py=python3; mode=sandbox; part=sandbox; ids='S1 S2' ;;
  *) py=python; mode=wfp; part=wfp; ids='W1 W2 W3 W4 W5 W6 W7' ;;
esac
mkdir -p target/verifier-probes
sh scripts/egress/probe.sh none
printf 'probe build exit=%s\n' "$?"

self_test() {
  sh scripts/egress/check.sh --mode "$mode" --self-test >"target/verifier-probes/$1.out" 2>"target/verifier-probes/$1.err"
  code=$?
  sed "s/^/  $1: /" "target/verifier-probes/$1.out"
  grep '^REFUSED' "target/verifier-probes/$1.err" | sed "s/^/  $1: /" | head -n 3
  printf 'mutant %s self-test exit=%s\n' "$1" "$code"
  return "$code"
}

printf '\n== unchanged\n'
self_test unchanged
if [ "$job" = windows ]; then
  powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File verifier-probes/state.ps1 -Label unchanged
else
  printf '\n== launchctl submit outside the check, a job that makes no connection\n'
  /bin/launchctl submit -l r2-verifier-plain -- /bin/sh -c 'sleep 20'
  printf 'launchctl submit outside the check exit=%s\n' "$?"
  /bin/launchctl list | grep -c r2-verifier-plain
  /bin/launchctl remove r2-verifier-plain
  printf 'launchctl remove exit=%s\n' "$?"
fi

for id in $ids; do
  printf '\n== mutant %s\n' "$id"
  "$py" verifier-probes/mutants.py "$id" || continue
  git diff --stat -- scripts/egress | tail -n 1
  if self_test "$id"; then
    sh scripts/egress/test.sh "$part" >"target/verifier-probes/$id.test" 2>&1
    printf 'mutant %s test.sh %s exit=%s\n' "$id" "$part" "$?"
    grep '^not ok' "target/verifier-probes/$id.test" | head -n 6 | sed "s/^/  $id: /"
    tail -n 1 "target/verifier-probes/$id.test" | sed "s/^/  $id: /"
    sh scripts/ci/run-ci-d.sh "$job" 10-egress >"target/verifier-probes/$id.driver" 2>&1
    printf 'mutant %s driver 10-egress exit=%s\n' "$id" "$?"
    grep -e '^checks ' -e 'verdict=' -e '^FAIL' -e '^PASS' "target/verifier-probes/$id.driver" | head -n 8 | sed "s/^/  $id: /"
  fi
  if [ "$job" = windows ]; then
    powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File verifier-probes/state.ps1 -Label "after-$id" | grep 'firewall-rules-of-the-check'
  fi
  git checkout -q -- scripts/egress
done
printf '\nverifier mutants done\n'
exit 0
