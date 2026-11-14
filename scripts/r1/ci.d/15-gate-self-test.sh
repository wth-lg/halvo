#!/bin/sh
# jobs: macos
# rule: The last record of docs/baseline/r1-gate-main.txt names the tree of the last commit that changed scripts/r1-gate or the bounds file, the self-tests of the R1 gate pass, each rejects at least one failing fixture, and each check script passes its self-test under dash.
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
  zero=$(printf '%s\n' "$out" | sed -n '/^PASS self-test:[^ ]* (0 cases/{p;q;}')
  [ -z "$zero" ] || fail "$label printed a PASS line with 0 cases: $zero"
  none=$(printf '%s\n' "$out" | grep -m 1 -E '^PASS self-test:[^ ]* \((.*[ ,])?0 failing fixtures rejected\)')
  [ -z "$none" ] || fail "$label rejected 0 failing fixtures: $none"
  case $group:$nl$out$nl in
    :*"${nl}PASS self-test:"*) ;;
    *"${nl}PASS self-test:$group ("* | *"${nl}PASS self-test:$group$nl"*) ;;
    *) fail "$label printed no PASS self-test${group:+:$group} line" ;;
  esac
}

record_defect() {
  record_tree=$(sed -n 's/^gate head=[0-9a-f]* tree=\([0-9a-f]*\) .*/\1/p' "$1/docs/baseline/r1-gate-main.txt" 2>/dev/null | tail -n 1)
  [ -n "$record_tree" ] || { printf 'docs/baseline/r1-gate-main.txt holds no line gate head=... tree=...\n'; return 1; }
  record_commit=$(git -C "$1" log -1 --format=%H -- scripts/r1-gate docs/baseline/r1-gate-bounds.txt) || { printf 'git log could not read the history of scripts/r1-gate\n'; return 1; }
  [ -n "$record_commit" ] || { printf 'no commit changed scripts/r1-gate or the bounds file\n'; return 1; }
  record_want=$(GATE_ROOT=$1 sh -c '. "$0"; tree_id "$GATE_ROOT" "$1"' "$1/scripts/r1-gate/lib.sh" "$record_commit") || { printf 'tree_id of lib.sh could not read %s\n' "$record_commit"; return 1; }
  record_bounds=$(sed -n 's/^gate head=[0-9a-f]* tree=[0-9a-f]* bounds=\([0-9a-f]*\) .*/\1/p' "$1/docs/baseline/r1-gate-main.txt" 2>/dev/null | tail -n 1)
  record_blob=$(git -C "$1" rev-parse -q --verify "$record_commit:docs/baseline/r1-gate-bounds.txt") || record_blob=none
  [ "$record_bounds" = "$record_blob" ] || { printf 'the last record of docs/baseline/r1-gate-main.txt names bounds %s, and the bounds file of the last commit that changed scripts/r1-gate or the bounds file is %s: make the record again\n' "${record_bounds:-absent}" "$record_blob"; return 1; }
  [ "$record_tree" = "$record_want" ] && return 0
  record_short=$(printf '%s' "$record_commit" | cut -c1-12)
  printf 'the last record of docs/baseline/r1-gate-main.txt names tree %s, and %s, the last commit that changed scripts/r1-gate or the bounds file, has tree %s: make the record again\n' "$record_tree" "$record_short" "$record_want"
  return 1
}

fixture_commit() {
  git -C "$1" add -A >/dev/null 2>&1 || exit 2
  fixture_tree=$(git -C "$1" write-tree) || exit 2
  fixture_parent=''
  if git -C "$1" rev-parse -q --verify HEAD >/dev/null 2>&1; then fixture_parent="parent $(git -C "$1" rev-parse HEAD)
"; fi
  fixture_new=$(printf 'tree %s\n%sauthor gate fixture <fixture@example.invalid> 1 +0000\ncommitter gate fixture <fixture@example.invalid> 1 +0000\n\nfixture\n' "$fixture_tree" "$fixture_parent" | git -C "$1" hash-object -t commit -w --stdin) || exit 2
  git -C "$1" update-ref HEAD "$fixture_new" || exit 2
}

fixture_record() {
  mkdir -p "$1/scripts/r1-gate" "$1/docs/baseline" || exit 2
  git init -q "$1" >/dev/null 2>&1 || exit 2
  [ -f "$1/scripts/r1-gate/lib.sh" ] || cp "$CI_D_REPO/scripts/r1-gate/lib.sh" "$1/scripts/r1-gate/lib.sh" || exit 2
  [ -f "$1/docs/baseline/r1-gate-bounds.txt" ] || printf 'x=1\n' >"$1/docs/baseline/r1-gate-bounds.txt"
  fixture_commit "$1"
  recorded=$(GATE_ROOT=$1 sh -c '. "$0"; tree_id "$GATE_ROOT" HEAD' "$1/scripts/r1-gate/lib.sh") || exit 2
  recorded_blob=$(git -C "$1" rev-parse HEAD:docs/baseline/r1-gate-bounds.txt) || exit 2
  printf 'gate head=0 tree=%s bounds=%s state=clean input=none machine=host\n' "$recorded" "$recorded_blob" >"$1/docs/baseline/r1-gate-main.txt"
  fixture_commit "$1"
}

record_case() {
  repo=$work/record-$1
  fixture_record "$repo"
  case $1 in
    stale) printf '# a later change\n' >>"$repo/scripts/r1-gate/lib.sh"; fixture_commit "$repo" ;;
    bounds-changed) printf 'x=2\n' >"$repo/docs/baseline/r1-gate-bounds.txt"; fixture_commit "$repo" ;;
    other-file-changed) printf 'notes\n' >"$repo/README.md"; fixture_commit "$repo" ;;
  esac
  got=PASS
  record_defect "$repo" >/dev/null || got=FAIL
  printf 'case %s/record-%s want=%s got=%s\n' "$name" "$1" "$2" "$got"
  if [ "$got" != "$2" ]; then wrong="$wrong record-$1"; elif [ "$2" = FAIL ]; then rejected=$((rejected + 1)); fi
}

measure() {
  found=$(record_defect "$CI_D_REPO") || fail "$found"
  printf 'record: the last gate line of docs/baseline/r1-gate-main.txt names the tree of the last commit that changed scripts/r1-gate or the bounds file\n'
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
  fixture_record "${directory%/scripts/r1-gate}"
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
  gate check-passes-with-0-cases "$pass" store '[ "$*" = --self-test ] || exit 3; printf "PASS self-test:store (0 cases)\n"'
  check check-passes-with-0-cases FAIL 'dash check-store.sh --self-test printed a PASS line with 0 cases: PASS self-test:store (0 cases))'
  gate check-rejects-no-fixture "$pass" store '[ "$*" = --self-test ] || exit 3; printf "PASS self-test:store (3 cases, 0 failing fixtures rejected)\n"'
  check check-rejects-no-fixture FAIL 'dash check-store.sh --self-test rejected 0 failing fixtures: PASS self-test:store (3 cases, 0 failing fixtures rejected))'
  gate run-rejects-no-fixture 'printf "PASS self-test:run (5 cases, 0 failing fixtures rejected)\n"' store "$pass"
  check run-rejects-no-fixture FAIL 'run.sh --self-test rejected 0 failing fixtures: PASS self-test:run (5 cases, 0 failing fixtures rejected))'
  gate bare-zero-rejected "$pass" store '[ "$*" = --self-test ] || exit 3; printf "PASS self-test:store (0 failing fixtures rejected)\n"'
  check bare-zero-rejected FAIL 'dash check-store.sh --self-test rejected 0 failing fixtures: PASS self-test:store (0 failing fixtures rejected))'
  gate ten-fixtures-rejected "$pass" store '[ "$*" = --self-test ] || exit 3; printf "PASS self-test:store (12 cases, 10 failing fixtures rejected)\n"'
  check ten-fixtures-rejected PASS
  mkdir -p "$work/check-script-renamed/scripts" "$work/check-script-renamed/docs" || exit 2
  cp -R "$CI_D_REPO/scripts/r1-gate" "$work/check-script-renamed/scripts/" || exit 2
  cp -R "$CI_D_REPO/docs/baseline" "$work/check-script-renamed/docs/" || exit 2
  mv "$work/check-script-renamed/scripts/r1-gate/check-store.sh" "$work/check-script-renamed/scripts/r1-gate/check-store-old.sh" || exit 2
  fixture_record "$work/check-script-renamed"
  check check-script-renamed FAIL 'FAIL self-test:run (check-store.sh is missing from'
  gate gate-with-a-stale-record "$pass" store "$pass"
  printf '# a change after the record\n' >>"$work/gate-with-a-stale-record/scripts/r1-gate/run.sh"
  fixture_commit "$work/gate-with-a-stale-record"
  check gate-with-a-stale-record FAIL ': make the record again)'
  record_case matches PASS
  record_case other-file-changed PASS
  record_case stale FAIL
  record_case bounds-changed FAIL
  gate dash-missing "$pass" store "$pass"
  for tool in sh git sed tail cut awk; do
    tool_path=$(command -v "$tool") || exit 2
    ln -s "$tool_path" "$work/no-tools/$tool" || exit 2
  done
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
