#!/bin/sh
# jobs: macos
# rule: The last record of docs/baseline/r1-gate-main.txt names the tree of the last commit that changed scripts/r1-gate or the bounds file, the self-tests of the R1 gate pass, each rejects at least one failing fixture, and each check script passes its self-test under dash in the weekly run, when the range of the push or the pull request changes a file under scripts/r1-gate or this script, and when that range cannot be read.
# lane: alone
# limit: 1800
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
  [ -z "${2:-}" ] || fixture_parent="${fixture_parent}parent $2
"
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

event_value() {
  values=$(grep -o -E "\"$1\": *\"[^\"]*\"" "$GITHUB_EVENT_PATH" | sed 's/^[^:]*: *"//; s/"$//')
  case $values in
    '' | *"$nl"*) return 1 ;;
  esac
  printf '%s\n' "$values"
}

unreadable() {
  printf 'range: unreadable: %s\n' "$1"
  dash_reason='range unreadable'
}

push_base() {
  [ -f "${GITHUB_EVENT_PATH:-}" ] || { unreadable 'GITHUB_EVENT_PATH names no file'; return 1; }
  before=$(event_value before) || { unreadable 'the push event holds no one value of before'; return 1; }
  base=$before
  case $before in
    *[!0-9a-f]*) ;;
    *) git -C "$CI_D_REPO" rev-parse -q --verify "$before^{commit}" >/dev/null && return 0 ;;
  esac
  default=$(event_value default_branch) || { unreadable "before $before names no commit here, and the push event holds no one value of default_branch"; return 1; }
  ! git -C "$CI_D_REPO" merge-base --is-ancestor HEAD "refs/remotes/origin/$default" || { unreadable "before $before names no commit here, and HEAD is on refs/remotes/origin/$default, so the merge base gives no range"; return 1; }
  base=$(git -C "$CI_D_REPO" merge-base HEAD "refs/remotes/origin/$default") || { unreadable "before $before names no commit here, and HEAD has no merge base with refs/remotes/origin/$default"; return 1; }
  printf 'range: before %s names no commit here, so the base is the merge base with refs/remotes/origin/%s\n' "$before" "$default"
}

dash_decision() {
  dash_reason=
  case ${GITHUB_EVENT_NAME:-} in
    schedule) dash_reason='weekly run'; return ;;
    push) push_base || return ;;
    pull_request)
      git -C "$CI_D_REPO" rev-parse -q --verify 'HEAD^2^{commit}' >/dev/null || { unreadable 'the pull request checkout is no merge commit'; return; }
      base=$(git -C "$CI_D_REPO" rev-parse 'HEAD^1') || { unreadable 'git rev-parse HEAD^1 failed'; return; }
      ;;
    '') unreadable 'GITHUB_EVENT_NAME is not set'; return ;;
    *) unreadable "the event $GITHUB_EVENT_NAME has no range"; return ;;
  esac
  head=$(git -C "$CI_D_REPO" rev-parse HEAD) || { unreadable 'git rev-parse HEAD failed'; return; }
  changed=$(git -C "$CI_D_REPO" diff --name-only "$base" "$head" -- scripts/r1-gate "scripts/r1/ci.d/$name.sh") || { unreadable "git diff could not read $base..$head"; return; }
  [ -n "$changed" ] || return
  printf '%s\n' "$changed" | sed 's/^/gate file changed: /'
  dash_reason='gate file changed'
}

measure() {
  found=$(record_defect "$CI_D_REPO") || fail "$found"
  printf 'record: the last gate line of docs/baseline/r1-gate-main.txt names the tree of the last commit that changed scripts/r1-gate or the bounds file\n'
  gate=$CI_D_REPO/scripts/r1-gate
  dash_decision
  if [ -z "$dash_reason" ]; then
    printf 'dash pass: not run: no gate file changed in %s..%s\n' "$base" "$head"
  else
    printf 'dash pass: run (%s)\n' "$dash_reason"
    if ! command -v dash >/dev/null 2>&1; then
      printf 'SKIP %s (dash is not installed)\n' "$name"
      exit 77
    fi
  fi
  self_tested 'run.sh --self-test' '' sh "$gate/run.sh" --self-test
  [ -n "$dash_reason" ] || { printf 'PASS %s\n' "$name"; return; }
  for script in "$gate"/check-*.sh; do
    [ -f "$script" ] || fail "scripts/r1-gate holds no check-*.sh"
    group=${script##*/check-}
    self_tested "dash ${script##*/} --self-test" "${group%.sh}" dash "$script" --self-test
  done
  printf 'PASS %s\n' "$name"
}

check() {
  out=$(
    unset GITHUB_EVENT_NAME GITHUB_EVENT_PATH
    [ -z "$event" ] || export GITHUB_EVENT_NAME="$event"
    [ -z "$event_path" ] || export GITHUB_EVENT_PATH="$event_path"
    PATH=$path CI_D_REPO=$work/$1 "$shell" "$0" 2>&1
  )
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
  event=
  event_path=
}

ranged() {
  gate "$1" "${5:-$pass}" store '[[ a == a ]] && printf "PASS self-test:store (1 cases)\n"'
  mkdir -p "$work/$1/scripts/r1/ci.d" || exit 2
  printf 'check\n' >"$work/$1/scripts/r1/ci.d/$name.sh"
  fixture_commit "$work/$1"
  from=$(git -C "$work/$1" rev-parse HEAD) || exit 2
  case $2 in
    other) printf 'notes\n' >"$work/$1/README.md"; fixture_commit "$work/$1" ;;
    gate) printf '# a change\n' >>"$work/$1/scripts/r1-gate/check-store.sh"; fixture_record "$work/$1" ;;
    check) printf '# a change\n' >>"$work/$1/scripts/r1/ci.d/$name.sh"; fixture_commit "$work/$1" ;;
  esac
  to=$(git -C "$work/$1" rev-parse HEAD) || exit 2
  event=${3:-}
  [ -z "${4:-}" ] || {
    event_path=$work/$1.json
    printf '%s\n' "$4" | sed "s/@from@/$from/g; s/@to@/$to/g" >"$event_path"
  }
}

merged() {
  git -C "$work/$1" update-ref HEAD "$from" || exit 2
  fixture_commit "$work/$1" "$to"
  to=$(git -C "$work/$1" rev-parse HEAD) || exit 2
}

unread() {
  want="range: unreadable: $1${nl}dash pass: run (range unreadable)"
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
  event=
  event_path=
  zero=0000000000000000000000000000000000000000

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
  run_changed="gate file changed: scripts/r1-gate/check-store.sh${nl}dash pass: run (gate file changed)"
  new_branch="{\"before\": \"$zero\", \"repository\": {\"default_branch\": \"trunk\"}}"
  ranged push-no-gate-file-changed other push '{"before": "@from@", "after": "@to@"}'
  check push-no-gate-file-changed PASS "dash pass: not run: no gate file changed in $from..$to"
  ranged push-gate-file-changed gate push '{"before": "@from@", "after": "@to@"}'
  check push-gate-file-changed FAIL "$run_changed"
  ranged push-check-changed check push '{"before": "@from@", "after": "@to@"}'
  check push-check-changed FAIL "gate file changed: scripts/r1/ci.d/$name.sh${nl}dash pass: run (gate file changed)"
  ranged new-branch-no-gate-file-changed other push "$new_branch"
  git -C "$work/new-branch-no-gate-file-changed" update-ref refs/remotes/origin/trunk "$from" || exit 2
  check new-branch-no-gate-file-changed PASS "range: before $zero names no commit here, so the base is the merge base with refs/remotes/origin/trunk${nl}dash pass: not run: no gate file changed in $from..$to"
  ranged new-branch-gate-file-changed gate push "$new_branch"
  git -C "$work/new-branch-gate-file-changed" update-ref refs/remotes/origin/trunk "$from" || exit 2
  check new-branch-gate-file-changed FAIL "$run_changed"
  ranged force-push-to-the-default-branch gate push "{\"before\": \"1111111111111111111111111111111111111111\", \"repository\": {\"default_branch\": \"trunk\"}}"
  git -C "$work/force-push-to-the-default-branch" update-ref refs/remotes/origin/trunk "$to" || exit 2
  unread 'before 1111111111111111111111111111111111111111 names no commit here, and HEAD is on refs/remotes/origin/trunk, so the merge base gives no range'
  check force-push-to-the-default-branch FAIL "$want"
  ranged new-branch-without-default-branch other push "{\"before\": \"$zero\"}"
  git -C "$work/new-branch-without-default-branch" update-ref refs/remotes/origin/trunk "$from" || exit 2
  unread "before $zero names no commit here, and the push event holds no one value of default_branch"
  check new-branch-without-default-branch FAIL "$want"
  ranged new-branch-without-origin-ref other push "$new_branch"
  unread "before $zero names no commit here, and HEAD has no merge base with refs/remotes/origin/trunk"
  check new-branch-without-origin-ref FAIL "$want"
  ranged push-before-not-hex other push '{"before": "HEAD~1"}'
  unread 'before HEAD~1 names no commit here, and the push event holds no one value of default_branch'
  check push-before-not-hex FAIL "$want"
  ranged push-before-twice other push '{"before": "@from@", "x": {"before": "@from@"}}'
  unread 'the push event holds no one value of before'
  check push-before-twice FAIL "$want"
  ranged push-without-before other push '{"after": "@to@", "repository": {"default_branch": "trunk"}}'
  git -C "$work/push-without-before" update-ref refs/remotes/origin/trunk "$from" || exit 2
  unread 'the push event holds no one value of before'
  check push-without-before FAIL "$want"
  ranged push-without-event-file other push ""
  unread 'GITHUB_EVENT_PATH names no file'
  check push-without-event-file FAIL "$want"
  ranged push-diff-fails other push ""
  broken=$(printf 'tree 1111111111111111111111111111111111111111\nauthor gate fixture <fixture@example.invalid> 1 +0000\ncommitter gate fixture <fixture@example.invalid> 1 +0000\n\nfixture\n' | git -C "$work/push-diff-fails" hash-object -t commit -w --stdin) || exit 2
  event_path=$work/push-diff-fails.json
  printf '{"before": "%s"}\n' "$broken" >"$event_path"
  unread "git diff could not read $broken..$to"
  check push-diff-fails FAIL "$want"
  ranged pull-request-no-gate-file-changed other pull_request
  merged pull-request-no-gate-file-changed
  check pull-request-no-gate-file-changed PASS "dash pass: not run: no gate file changed in $from..$to"
  ranged pull-request-gate-file-changed gate pull_request
  merged pull-request-gate-file-changed
  check pull-request-gate-file-changed FAIL "$run_changed"
  ranged pull-request-head-not-a-merge other pull_request
  unread 'the pull request checkout is no merge commit'
  check pull-request-head-not-a-merge FAIL "$want"
  ranged weekly-run other schedule
  check weekly-run FAIL "record: the last gate line of docs/baseline/r1-gate-main.txt names the tree of the last commit that changed scripts/r1-gate or the bounds file${nl}dash pass: run (weekly run)"
  ranged no-event other
  unread 'GITHUB_EVENT_NAME is not set'
  check no-event FAIL "$want"
  ranged event-without-a-range other workflow_dispatch
  unread 'the event workflow_dispatch has no range'
  check event-without-a-range FAIL "$want"
  ranged run-fails-dash-not-run other push '{"before": "@from@"}' "$planned"
  check run-fails-dash-not-run FAIL 'run.sh --self-test exited 1)'
  gate dash-missing "$pass" store "$pass"
  for tool in sh git sed tail cut awk grep; do
    tool_path=$(command -v "$tool") || exit 2
    ln -s "$tool_path" "$work/no-tools/$tool" || exit 2
  done
  path=$work/no-tools
  check dash-missing SKIP 'SKIP 15-gate-self-test (dash is not installed)'
  ranged dash-missing-not-run other push '{"before": "@from@"}'
  path=$work/no-tools
  check dash-missing-not-run PASS "dash pass: not run: no gate file changed in $from..$to"

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
