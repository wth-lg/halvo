#!/bin/sh
# usage: sh repeat-interrupt.sh <driver file> <INT|TERM> <count>
# Repeats the driver's own case step-interrupted-by-<signal> <count> times
# against the driver file: a fixture repository with two self-tests that each
# start a sleep and record their own and the sleep's process id, the driver
# started through perl with INT and QUIT at their defaults (as the case starts
# it), the signal sent to the started driver once both self-tests have
# started, then the driver's exit status, its line that names the signal and
# the recorded processes left. One verdict line per repetition, the counts at
# the end; exit 0 when every repetition gave the right verdict.
set -u
driver=$1
signal=$2
count=$3
case $signal in
  INT) want=130 ;;
  TERM) want=143 ;;
  *) echo "usage: sh repeat-interrupt.sh <driver file> <INT|TERM> <count>"; exit 64 ;;
esac
case $driver in
  /*) ;;
  *) driver=$(pwd)/$driver ;;
esac
line="FAIL run-ci-d (the step was interrupted by $signal; every self-test that it started is ended)"
case_perl="setpgrp(0, 0); \$SIG{QUIT} = 'DEFAULT'; \$SIG{INT} = 'DEFAULT'; exec { \$ARGV[0] } @ARGV; exit 127;"
base=${RUNNER_TEMP:-${TMPDIR:-/tmp}}
root=$(mktemp -d "$base/repeat-interrupt.XXXXXX") || exit 2
repo=$root/repo
mkdir -p "$repo/scripts/r1/ci.d" || exit 2
git -C "$repo" init -q || exit 2
printf 'version = 4\n' >"$repo/Cargo.lock"
body='[ "${1:-}" = --self-test ] || exit 3; me=${0##*/}; me=${me%.sh}; sleep 20 & printf '"'"'%s\n%s\n'"'"' "$$" "$!" >>"$CI_D_REPO/pids.$REPEAT"; : >"$CI_D_REPO/started.$me.$REPEAT"; wait; printf "PASS self-test:%s (1 failing fixtures rejected)\n" "$me"'
for name in 10-a 20-b; do
  printf '#!/bin/sh\n# jobs: macos windows\n# rule: A fixture rule.\n%s\n' "$body" >"$repo/scripts/r1/ci.d/$name.sh"
done
git -C "$repo" add Cargo.lock scripts || exit 2
tree=$(git -C "$repo" write-tree) || exit 2
commit=$(printf 'tree %s\nauthor fixture <fixture@example.invalid> 1 +0000\ncommitter fixture <fixture@example.invalid> 1 +0000\n\nfixture\n' "$tree" | git -C "$repo" hash-object -t commit -w --stdin) || exit 2
git -C "$repo" update-ref HEAD "$commit" || exit 2
right=0
wrong=0
i=0
while [ "$i" -lt "$count" ]; do
  i=$((i + 1))
  out=$root/out.$i
  t0=$(date +%s)
  (
    cd "$repo" || exit 99
    unset CI_D_SCRIPTS CI_D_SCRIPT_LIMIT_S RUN_CI_D_WORKER
    REPEAT=$i
    CI_D_SELF_TEST_JOBS=2
    TMPDIR=$root
    export REPEAT CI_D_SELF_TEST_JOBS TMPDIR
    printf 'a line on standard input\n' | perl -e "$case_perl" sh "$driver" macos --self-test 10-a 20-b >"$out" 2>&1 &
    pid=$!
    n=0
    while { [ ! -e "$repo/started.10-a.$i" ] || [ ! -e "$repo/started.20-b.$i" ]; } && [ "$n" -lt 300 ]; do
      sleep 0.1
      n=$((n + 1))
    done
    before=0
    if [ -f "$repo/pids.$i" ]; then
      while IFS= read -r p; do
        [ -z "$p" ] || ! kill -0 "$p" 2>/dev/null || before=$((before + 1))
      done <"$repo/pids.$i"
    fi
    printf '%s\n' "$before" >"$root/before.$i"
    kill -s "$signal" "$pid"
    wait "$pid"
  )
  code=$?
  t1=$(date +%s)
  left=0
  n=0
  while :; do
    left=0
    if [ -f "$repo/pids.$i" ]; then
      while IFS= read -r p; do
        [ -z "$p" ] || ! kill -0 "$p" 2>/dev/null || left=$((left + 1))
      done <"$repo/pids.$i"
    fi
    [ "$left" -gt 0 ] && [ "$n" -lt 30 ] || break
    sleep 0.1
    n=$((n + 1))
  done
  before=
  read -r before <"$root/before.$i"
  said=no
  ! grep -qxF -e "$line" "$out" || said=yes
  if [ "$code" = "$want" ] && [ "$said" = yes ] && [ "$left" -eq 0 ] && [ "$before" = 4 ]; then
    verdict=right
    right=$((right + 1))
  else
    verdict=wrong
    wrong=$((wrong + 1))
  fi
  printf 'repeat step-interrupted-by-%s %s: %s exit=%s line=%s before=%s after=%s seconds=%s\n' "$signal" "$i" "$verdict" "$code" "$said" "$before" "$left" "$((t1 - t0))"
done
printf 'counts step-interrupted-by-%s with %s: %s right, %s wrong, of %s\n' "$signal" "${driver##*/}" "$right" "$wrong" "$count"
[ "$wrong" -eq 0 ]
