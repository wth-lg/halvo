#!/bin/sh
# jobs: macos
# rule: The self-tests of the R1 gate pass, and each check script passes its self-test under dash.
set -u
name=15-gate-self-test
nl='
'

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

self_tested() {
  label=$1
  group=$2
  shift 2
  out=$("$@" 2>&1)
  code=$?
  printf '%s\n' "$out"
  [ "$code" -eq 0 ] || fail "$label exited $code"
  first=$(printf '%s\n' "$out" | sed -n '/^FAIL/{p;q;}')
  [ -z "$first" ] || fail "$label printed: $first"
  case $group:$nl$out$nl in
    :*"${nl}PASS self-test:"*) ;;
    *"${nl}PASS self-test:$group ("* | *"${nl}PASS self-test:$group$nl"*) ;;
    *) fail "$label printed no PASS self-test${group:+:$group} line" ;;
  esac
}

measure() {
  gate=$CI_D_REPO/scripts/r1-gate
  if ! command -v dash >/dev/null 2>&1; then
    printf 'SKIP %s (dash is not installed)\n' "$name"
    exit 77
  fi
  self_tested 'run.sh --self-test' '' sh "$gate/run.sh" --self-test
  for script in "$gate"/check-*.sh; do
    [ -f "$script" ] || fail "scripts/r1-gate holds no check-*.sh"
    group=${script##*/check-}
    self_tested "dash ${script##*/} --self-test" "${group%.sh}" dash "$script" --self-test
  done
  printf 'PASS %s\n' "$name"
}

check() {
  out=$(PATH=$path CI_D_REPO=$work/$1 "$shell" "$0" 2>&1)
  code=$?
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    "77:SKIP $name ("*")") got=SKIP ;;
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
  path=$PATH
}

gate() {
  directory=$work/$1/scripts/r1-gate
  mkdir -p "$directory" || exit 2
  printf '%s\n' "$2" | sed 's/@group@/store/g' >"$directory/run.sh"
  shift 2
  while [ $# -ge 2 ]; do
    printf '%s\n' "$2" | sed "s/@group@/$1/g" >"$directory/check-$1.sh"
    shift 2
  done
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  mkdir -p "$work/no-tools"
  shell=$(command -v sh) || exit 2
  path=$PATH
  pass='[ "$*" = --self-test ] || exit 3; printf "PASS self-test:@group@ (1 cases)\n"'
  fail_then_pass='printf "FAIL self-test:@group@ (case x: wrong verdict)\nPASS self-test:@group@ (1 cases)\n"'
  planned='printf "FAIL self-test:store (planned)\n"; exit 1'
  rejected=0
  wrong=

  gate gate-passes "$pass" store "$pass" timed-run "$pass"
  check gate-passes PASS
  gate run-fails "$planned" store "$pass"
  check run-fails FAIL 'run.sh --self-test exited 1)'
  gate run-prints-no-pass-line 'exit 0' store "$pass"
  check run-prints-no-pass-line FAIL 'run.sh --self-test printed no PASS self-test line)'
  gate pass-words-inside-a-line 'printf "the line PASS self-test:store is missing\n"' store "$pass"
  check pass-words-inside-a-line FAIL 'run.sh --self-test printed no PASS self-test line)'
  gate no-check-script "$pass"
  check no-check-script FAIL 'scripts/r1-gate holds no check-*.sh)'
  gate check-fails "$pass" store "$planned"
  check check-fails FAIL 'dash check-store.sh --self-test exited 1)'
  gate check-prints-no-pass-line "$pass" store 'exit 0'
  check check-prints-no-pass-line FAIL 'dash check-store.sh --self-test printed no PASS self-test:store line)'
  gate second-check-fails "$pass" store "$pass" timed-run "$planned"
  check second-check-fails FAIL 'dash check-timed-run.sh --self-test exited 1)'
  gate check-with-another-initial-fails "$pass" store "$pass" cost "$planned"
  check check-with-another-initial-fails FAIL 'dash check-cost.sh --self-test exited 1)'
  gate check-needs-bash "$pass" store '[[ a == a ]] && printf "PASS self-test:store\n"'
  check check-needs-bash FAIL 'dash check-store.sh --self-test exited 127)'
  gate run-prints-a-fail-line "$fail_then_pass" store "$pass"
  check run-prints-a-fail-line FAIL 'run.sh --self-test printed: FAIL self-test:store (case x: wrong verdict))'
  gate check-prints-a-fail-line "$pass" store "$fail_then_pass"
  check check-prints-a-fail-line FAIL 'dash check-store.sh --self-test printed: FAIL self-test:store (case x: wrong verdict))'
  gate check-passes-as-another "$pass" store '[ "$*" = --self-test ] || exit 3; printf "PASS self-test:deletion (1 cases)\n"'
  check check-passes-as-another FAIL 'dash check-store.sh --self-test printed no PASS self-test:store line)'
  gate check-group-is-a-prefix "$pass" store '[ "$*" = --self-test ] || exit 3; printf "PASS self-test:store-other (1 cases)\n"'
  check check-group-is-a-prefix FAIL 'dash check-store.sh --self-test printed no PASS self-test:store line)'
  gate dash-missing "$pass" store "$pass"
  path=$work/no-tools
  check dash-missing SKIP 'SKIP 15-gate-self-test (dash is not installed)'

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
