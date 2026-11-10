#!/bin/sh
# Checks the S5 rule line against the plan's unit. Touches no key store; runs anywhere.
set -u
# shellcheck source=scratch/r7-s5/s5-verdict.sh
. "$(dirname "$0")/../s5-verdict.sh"
failed=0

# check <expected return> <expected start of the line> <arguments of s5_verdict>
check() {
  want_code=$1
  want=$2
  shift 2
  got_code=0
  got=$(s5_verdict "$@") || got_code=$?
  case "$got" in
    "$want"*)
      if [ "$got_code" = "$want_code" ]; then
        echo "ok    $got"
        return
      fi
      ;;
  esac
  echo "FAIL  wanted return $want_code and '$want...', got return $got_code and '$got'"
  failed=1
}

check 0 "S5 RULE r: MET (" r 0 cdhash:aaa cdhash:bbb
check 0 "S5 RULE r: MISSED (rebuilt build 2" r 10 cdhash:aaa cdhash:bbb
check 0 "S5 RULE r: MISSED (rebuilt build 2" r 12 cdhash:aaa cdhash:bbb
check 1 "S5 RULE r: DOES NOT COUNT (" r 11 cdhash:aaa cdhash:bbb
# The plan: a read that succeeds counts as MET only when the log shows two different code
# hashes. With equal hashes the leg prints MISSED and gives that reason.
check 0 "S5 RULE r: MISSED (the log does not show two different code hashes" r 0 cdhash:aaa cdhash:aaa
check 0 "S5 RULE r: MISSED (the log does not show two different code hashes" r 0 "" cdhash:bbb
check 0 "S5 RULE r: MISSED (" r 10 cdhash:aaa cdhash:aaa
exit "$failed"
