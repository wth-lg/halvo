#!/bin/sh
# jobs: macos windows
# rule: The tests of halvo-cap pass, and measure-peak catches its control child at 268435456 bytes: the Job refuses it on Windows, the parent stops it within 2000 ms on macOS.
# lane: alone
set -u
name=11-cap
limit=268435456
nl='
'

judge_test() {
  [ "$1" -eq 0 ] || printf 'cargo test -p halvo-cap exited %s' "$1"
}

judge_control() {
  code=$1
  out=$2
  last=${out##*"$nl"}
  case $nl$out$nl in
    *"${nl}control allocate_bytes=$((limit + 16777216)) "*) has_control=yes ;;
    *) has_control=no ;;
  esac
  if [ "$code" -ne 0 ]; then
    printf 'measure-peak --limit %s --control exited %s, last line: %s' "$limit" "$code" "${last:-none}"
  elif [ "$has_control" = no ]; then
    printf 'measure-peak --limit %s --control printed no control line with allocate_bytes=%s' "$limit" "$((limit + 16777216))"
  elif [ "$last" != 'checks control=ok' ]; then
    printf 'measure-peak --limit %s --control exited 0 with the last line: %s' "$limit" "${last:-none}"
  fi
}

enter_repository() {
  if [ -z "${CI_D_REPO:-}" ] || ! cd "$CI_D_REPO"; then
    printf 'FAIL %s (cannot enter the repository that CI_D_REPO names)\n' "$name"
    return 1
  fi
}

measure() {
  enter_repository || exit 1
  cargo test -p halvo-cap --locked
  cause=$(judge_test "$?")
  [ -z "$cause" ] || { printf 'FAIL %s (%s)\n' "$name" "$cause"; exit 1; }
  out=$(cargo run --quiet --locked -p halvo-cap --bin measure-peak -- --limit "$limit" --control)
  code=$?
  printf '%s\n' "$out"
  cause=$(judge_control "$code" "$out")
  [ -z "$cause" ] || { printf 'FAIL %s (%s)\n' "$name" "$cause"; exit 1; }
  printf 'PASS %s\n' "$name"
}

head_macos='measure-peak platform=macos mode=poll limit_bytes=268435456 control=yes'
head_windows='measure-peak platform=windows mode=job limit_bytes=268435456 control=yes'

repository_verdict() {
  entered=$(
    if [ "$1" = unset ]; then unset CI_D_REPO; else CI_D_REPO=$1; fi
    enter_repository 2>/dev/null
  )
  entered_status=$?
  if [ "$entered_status" -eq 0 ] && [ -z "$entered" ]; then
    printf PASS
  elif [ "$entered_status" -ne 0 ] && [ "$entered" = "FAIL $name (cannot enter the repository that CI_D_REPO names)" ]; then
    printf FAIL
  else
    printf 'status-%s-with-%s-lines' "$entered_status" "$(printf '%s' "$entered" | grep -c .)"
  fi
}

check() {
  case=$1
  want=$2
  shift 2
  if [ "$1" = repository ]; then
    got=$(repository_verdict "$2")
  else
    if [ "$1" = test ]; then
      cause=$(judge_test "$2")
    else
      cause=$(judge_control "$2" "$3")
    fi
    if [ -z "$cause" ]; then got=PASS; else got=FAIL; fi
  fi
  printf 'case %s/%s want=%s got=%s\n' "$name" "$case" "$want" "$got"
  if [ "$got" != "$want" ]; then
    wrong="$wrong $case"
  elif [ "$want" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

check_measure() {
  case=$1
  want=$2
  repository=$3
  shift 3
  measured=$(
    test_status=$1
    run_status=$2
    run_output=$3
    cargo() {
      if [ "$1" = test ]; then
        return "$test_status"
      fi
      printf '%s\n' "$run_output"
      return "$run_status"
    }
    CI_D_REPO=$repository
    measure 2>/dev/null
  )
  measured_status=$?
  last=${measured##*"$nl"}
  if [ "$want" = "PASS $name" ]; then want_status=0; else want_status=1; fi
  if [ "$last" = "$want" ] && [ "$measured_status" -eq "$want_status" ]; then
    got=$want
  else
    got="other: status $measured_status, $last"
  fi
  printf 'case %s/%s want=%s got=%s\n' "$name" "$case" "$want" "$got"
  if [ "$got" != "$want" ]; then
    wrong="$wrong $case"
  elif [ "$want" != "PASS $name" ]; then
    rejected=$((rejected + 1))
  fi
}

self_test() {
  rejected=0
  wrong=
  check repository-entered PASS repository "$PWD"
  check no-repository FAIL repository "${CI_D_ARTIFACTS:-$PWD}/no-such-repository"
  check repository-unset FAIL repository unset
  check repository-empty FAIL repository ""
  check tests-pass PASS test 0
  check tests-fail FAIL test 101
  check stopped-on-macos PASS control 0 "$head_macos${nl}control allocate_bytes=285212672 outcome=stopped stop_ms=212 peak_bytes=287309824${nl}checks control=ok"
  check refused-on-windows PASS control 0 "$head_windows${nl}control allocate_bytes=285212672 outcome=refused stop_ms=- peak_bytes=268283904${nl}checks control=ok"
  check stopped-too-late FAIL control 1 "$head_macos${nl}control allocate_bytes=285212672 outcome=stopped stop_ms=2250 peak_bytes=287309824${nl}checks control=miss"
  check completed FAIL control 1 "$head_windows${nl}control allocate_bytes=285212672 outcome=completed stop_ms=- peak_bytes=285999104${nl}checks control=miss"
  check refused-cap-unsupported FAIL control 2 ""
  check ok-status-with-a-miss FAIL control 0 "$head_macos${nl}control allocate_bytes=285212672 outcome=failed stop_ms=- peak_bytes=0${nl}checks control=miss"
  check miss-status-with-an-ok-line FAIL control 1 "$head_macos${nl}control allocate_bytes=285212672 outcome=stopped stop_ms=212 peak_bytes=287309824${nl}checks control=ok"
  check control-at-another-limit FAIL control 0 "measure-peak platform=macos mode=poll limit_bytes=1048576 control=yes${nl}control allocate_bytes=17825792 outcome=stopped stop_ms=0 peak_bytes=19136512${nl}checks control=ok"
  check no-control-line FAIL control 0 "$head_macos${nl}checks control=ok"
  check no-checks-line FAIL control 0 "$head_macos${nl}control allocate_bytes=285212672 outcome=stopped stop_ms=212 peak_bytes=287309824"
  check_measure measure-passes "PASS $name" "$PWD" 0 0 "$head_macos${nl}control allocate_bytes=285212672 outcome=stopped stop_ms=212 peak_bytes=287309824${nl}checks control=ok"
  check_measure measure-tests-fail "FAIL $name (cargo test -p halvo-cap exited 101)" "$PWD" 101 0 "$head_macos${nl}control allocate_bytes=285212672 outcome=stopped stop_ms=212 peak_bytes=287309824${nl}checks control=ok"
  check_measure measure-control-misses "FAIL $name (measure-peak --limit 268435456 --control exited 1, last line: checks control=miss)" "$PWD" 0 1 "$head_macos${nl}control allocate_bytes=285212672 outcome=completed stop_ms=- peak_bytes=285999104${nl}checks control=miss"
  check_measure measure-no-repository "FAIL $name (cannot enter the repository that CI_D_REPO names)" "${CI_D_ARTIFACTS:-$PWD}/no-such-repository" 0 0 "$head_macos${nl}control allocate_bytes=285212672 outcome=stopped stop_ms=212 peak_bytes=287309824${nl}checks control=ok"
  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  '') measure ;;
  *) exit 64 ;;
esac
