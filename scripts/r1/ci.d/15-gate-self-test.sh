#!/bin/sh
# jobs: macos
# rule: The self-tests of the R1 gate pass, and each check script passes its self-test under dash.
set -u
name=15-gate-self-test

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

nl='
'

self_tested() {
  label=$1
  shift
  out=$("$@" 2>&1)
  code=$?
  printf '%s\n' "$out"
  [ "$code" -eq 0 ] || fail "$label exited $code"
  case $nl$out in
    *"${nl}PASS self-test:"*) ;;
    *) fail "$label printed no PASS self-test line" ;;
  esac
}

measure() {
  gate=$CI_D_REPO/scripts/r1-gate
  if ! command -v dash >/dev/null 2>&1; then
    printf 'SKIP %s (dash is not installed)\n' "$name"
    exit 77
  fi
  self_tested 'run.sh --self-test' sh "$gate/run.sh" --self-test
  for script in "$gate"/check-*.sh; do
    [ -f "$script" ] || fail "scripts/r1-gate holds no check-*.sh"
    self_tested "dash ${script##*/} --self-test" dash "$script" --self-test
  done
  printf 'PASS %s\n' "$name"
}

check() {
  out=$(
    CI_D_REPO=$work/$1
    export CI_D_REPO
    measure 2>&1
  )
  code=$?
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  case $out in
    *"${3:-}"*) ;;
    *) got=$got-without-the-text ;;
  esac
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$got"
  if [ "$got" != "$2" ]; then
    wrong="$wrong $1"
  elif [ "$2" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

gate() {
  mkdir -p "$work/$1/scripts/r1-gate" || exit 2
  printf '%s\n' "$2" >"$work/$1/scripts/r1-gate/run.sh"
  [ $# -lt 3 ] || printf '%s\n' "$3" >"$work/$1/scripts/r1-gate/check-store.sh"
  [ $# -lt 4 ] || printf '%s\n' "$4" >"$work/$1/scripts/r1-gate/check-timed-run.sh"
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  pass='printf "PASS self-test:store\n"'
  planned='printf "FAIL self-test:store (planned)\n"; exit 1'
  rejected=0
  wrong=

  gate gate-passes "$pass" "$pass" "$pass"
  check gate-passes PASS
  gate run-fails "$planned" "$pass"
  check run-fails FAIL 'run.sh --self-test exited 1)'
  gate run-prints-no-pass-line 'exit 0' "$pass"
  check run-prints-no-pass-line FAIL 'run.sh --self-test printed no PASS self-test line)'
  gate no-check-script "$pass"
  check no-check-script FAIL 'scripts/r1-gate holds no check-*.sh)'
  gate check-fails "$pass" "$planned"
  check check-fails FAIL 'dash check-store.sh --self-test exited 1)'
  gate check-prints-no-pass-line "$pass" 'exit 0'
  check check-prints-no-pass-line FAIL 'dash check-store.sh --self-test printed no PASS self-test line)'
  gate second-check-fails "$pass" "$pass" "$planned"
  check second-check-fails FAIL 'dash check-timed-run.sh --self-test exited 1)'
  gate check-needs-bash "$pass" '[[ a == a ]] && printf "PASS self-test:store\n"'
  check check-needs-bash FAIL 'dash check-store.sh --self-test exited 127)'

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
