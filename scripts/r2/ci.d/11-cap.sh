#!/bin/sh
# jobs: macos windows
# rule: The tests of halvo-cap pass, and measure-peak catches its control child at 268435456 bytes: the Job refuses it on Windows, the parent stops it within 2000 ms on macOS.
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
    *"${nl}control allocate_bytes="*) has_control=yes ;;
    *) has_control=no ;;
  esac
  if [ "$code" -ne 0 ]; then
    printf 'measure-peak --limit %s --control exited %s, last line: %s' "$limit" "$code" "${last:-none}"
  elif [ "$has_control" = no ]; then
    printf 'measure-peak --limit %s --control printed no control line' "$limit"
  elif [ "$last" != 'checks control=ok' ]; then
    printf 'measure-peak --limit %s --control exited 0 with the last line: %s' "$limit" "${last:-none}"
  fi
}

measure() {
  cd "$CI_D_REPO" || exit 2
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

head_macos="measure-peak platform=macos mode=poll limit_bytes=$limit control=yes"
head_windows="measure-peak platform=windows mode=job limit_bytes=$limit control=yes"

check() {
  case=$1
  want=$2
  shift 2
  if [ "$1" = test ]; then
    cause=$(judge_test "$2")
  else
    cause=$(judge_control "$2" "$3")
  fi
  if [ -z "$cause" ]; then got=PASS; else got=FAIL; fi
  printf 'case %s/%s want=%s got=%s\n' "$name" "$case" "$want" "$got"
  if [ "$got" != "$want" ]; then
    wrong="$wrong $case"
  elif [ "$want" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

self_test() {
  rejected=0
  wrong=
  check tests-pass PASS test 0
  check tests-fail FAIL test 101
  check stopped-on-macos PASS control 0 "$head_macos${nl}control allocate_bytes=285212672 outcome=stopped stop_ms=212 peak_bytes=287309824${nl}checks control=ok"
  check refused-on-windows PASS control 0 "$head_windows${nl}control allocate_bytes=285212672 outcome=refused stop_ms=- peak_bytes=268283904${nl}checks control=ok"
  check stopped-too-late FAIL control 1 "$head_macos${nl}control allocate_bytes=285212672 outcome=stopped stop_ms=2250 peak_bytes=287309824${nl}checks control=miss"
  check completed FAIL control 1 "$head_windows${nl}control allocate_bytes=285212672 outcome=completed stop_ms=- peak_bytes=285999104${nl}checks control=miss"
  check refused-cap-unsupported FAIL control 2 ""
  check ok-status-with-a-miss FAIL control 0 "$head_macos${nl}control allocate_bytes=285212672 outcome=failed stop_ms=- peak_bytes=0${nl}checks control=miss"
  check no-control-line FAIL control 0 "$head_macos${nl}checks control=ok"
  check no-checks-line FAIL control 0 "$head_macos${nl}control allocate_bytes=285212672 outcome=stopped stop_ms=212 peak_bytes=287309824"
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
