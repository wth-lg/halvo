#!/bin/sh
# jobs: macos
# rule: On every run scripts/r1-gate/mutants.expected holds no name twice and the last record of docs/baseline/r1-gate-main.txt names the tree of the last commit that changed scripts/r1-gate, its list of names mutants.expected aside, or the bounds file; the self-tests of the R1 gate, sh scripts/r1-gate/run.sh --self-test and each check script under dash, run only in the weekly run, when the range of the push or the pull request changes a path of this script's list, and when that range cannot be read, and then they pass, print each group's PASS line once and reject at least one failing fixture each; an ordinary push or pull request runs neither pass.
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
  marked=$(printf '%s\n' "$out" | awk '/^PASS self-test:/ && (index($0, "[override:") || index($0, "[dirty tree;")) { print; exit }')
  [ -z "$marked" ] || fail "$label printed a PASS line with an override or a dirty tree: $marked"
  case $group:$nl$out$nl in
    :*"${nl}PASS self-test:"*) ;;
    *"${nl}PASS self-test:$group ("* | *"${nl}PASS self-test:$group$nl"*) ;;
    *) fail "$label printed no PASS self-test${group:+:$group} line" ;;
  esac
}

isolated_git() {
  names=$(env | sed -n 's/^\(GIT_[A-Za-z0-9_]*\)=.*/\1/p')
  for each in $names; do unset "$each"; done
  GIT_CONFIG_NOSYSTEM=1
  GIT_CONFIG_GLOBAL=/dev/null
  GIT_NO_REPLACE_OBJECTS=1
  GIT_GRAFT_FILE=/dev/null/no-grafts
  export GIT_CONFIG_NOSYSTEM GIT_CONFIG_GLOBAL GIT_NO_REPLACE_OBJECTS GIT_GRAFT_FILE
}

range_git() {
  (
    isolated_git
    git -C "$CI_D_REPO" "$@"
  )
}

record_defect() (
  isolated_git
  record_tree=$(sed -n 's/^gate head=[0-9a-f]* tree=\([0-9a-f]*\) .*/\1/p' "$1/docs/baseline/r1-gate-main.txt" 2>/dev/null | tail -n 1)
  [ -n "$record_tree" ] || { printf 'docs/baseline/r1-gate-main.txt holds no line gate head=... tree=...\n'; return 1; }
  record_commit=$(git -C "$1" log -1 --format=%H -- scripts/r1-gate docs/baseline/r1-gate-bounds.txt ':(icase,exclude)scripts/r1-gate/mutants.expected') || { printf 'git log could not read the history of scripts/r1-gate\n'; return 1; }
  [ -n "$record_commit" ] || { printf 'no commit changed scripts/r1-gate or the bounds file\n'; return 1; }
  record_want=$(GATE_ROOT=$1 sh -c '. "$0"; tree_id "$GATE_ROOT" "$1"' "$1/scripts/r1-gate/lib.sh" "$record_commit") || { printf 'tree_id of lib.sh could not read %s\n' "$record_commit"; return 1; }
  record_bounds=$(sed -n 's/^gate head=[0-9a-f]* tree=[0-9a-f]* bounds=\([0-9a-f]*\) .*/\1/p' "$1/docs/baseline/r1-gate-main.txt" 2>/dev/null | tail -n 1)
  record_blob=$(git -C "$1" rev-parse -q --verify "$record_commit:docs/baseline/r1-gate-bounds.txt") || record_blob=none
  [ "$record_bounds" = "$record_blob" ] || { printf 'the last record of docs/baseline/r1-gate-main.txt names bounds %s, and the bounds file of the last commit that changed scripts/r1-gate or the bounds file is %s: make the record again\n' "${record_bounds:-absent}" "$record_blob"; return 1; }
  [ "$record_tree" = "$record_want" ] && return 0
  record_short=$(printf '%s' "$record_commit" | cut -c1-12)
  printf 'the last record of docs/baseline/r1-gate-main.txt names tree %s, and %s, the last commit that changed scripts/r1-gate or the bounds file, has tree %s: make the record again\n' "$record_tree" "$record_short" "$record_want"
  return 1
)

names_twice() {
  [ -f "$1/scripts/r1-gate/mutants.expected" ] || return 0
  awk '$0 != "" && seen[$0]++ == 1 { printf "%s%s", sep, $0; sep = " " }' "$1/scripts/r1-gate/mutants.expected"
}

fixture_commit() {
  [ "${3:-}" = index ] || git -C "$1" add -A >/dev/null 2>&1 || exit 2
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
  fixture_commit "$1" '' "${2:-}"
  recorded=$(GATE_ROOT=$1 sh -c '. "$0"; tree_id "$GATE_ROOT" HEAD' "$1/scripts/r1-gate/lib.sh") || exit 2
  recorded_blob=$(git -C "$1" rev-parse HEAD:docs/baseline/r1-gate-bounds.txt) || exit 2
  printf 'gate head=0 tree=%s bounds=%s state=clean input=none machine=host\n' "$recorded" "$recorded_blob" >"$1/docs/baseline/r1-gate-main.txt"
  [ "${2:-}" != index ] || git -C "$1" add -- docs/baseline/r1-gate-main.txt >/dev/null 2>&1 || exit 2
  fixture_commit "$1" '' "${2:-}"
}

record_case() {
  repo=$work/record-$1
  mkdir -p "$repo/scripts/r1-gate" || exit 2
  printf 'name-one\n' >"$repo/scripts/r1-gate/mutants.expected"
  fixture_record "$repo"
  case $1 in
    stale) printf '# a later change\n' >>"$repo/scripts/r1-gate/lib.sh"; fixture_commit "$repo" ;;
    bounds-changed) printf 'x=2\n' >"$repo/docs/baseline/r1-gate-bounds.txt"; fixture_commit "$repo" ;;
    other-file-changed) printf 'notes\n' >"$repo/README.md"; fixture_commit "$repo" ;;
    list-changed) printf 'name-two\n' >>"$repo/scripts/r1-gate/mutants.expected"; fixture_commit "$repo" ;;
    names-added-with-their-rows)
      mkdir -p "$repo/crates/x/src" || exit 2
      printf 'row\n' >"$repo/crates/x/src/mutants.rows"
      printf 'name-two\n' >>"$repo/scripts/r1-gate/mutants.expected"
      fixture_commit "$repo" ;;
    list-edited-after-the-record)
      printf '# the change that the record measured\n' >>"$repo/scripts/r1-gate/lib.sh"
      git -C "$repo" add -A >/dev/null 2>&1 || exit 2
      measured=$(git -C "$repo" write-tree) || exit 2
      recorded=$(GATE_ROOT=$repo sh -c '. "$0"; tree_id "$GATE_ROOT" "$1"' "$repo/scripts/r1-gate/lib.sh" "$measured") || exit 2
      printf 'gate head=0 tree=%s bounds=%s state=clean input=none machine=host\n' "$recorded" "$recorded_blob" >>"$repo/docs/baseline/r1-gate-main.txt"
      printf 'name-two\n' >>"$repo/scripts/r1-gate/mutants.expected"
      fixture_commit "$repo" ;;
    list-and-lib-changed)
      printf 'name-two\n' >>"$repo/scripts/r1-gate/mutants.expected"
      printf '# a later change\n' >>"$repo/scripts/r1-gate/lib.sh"
      fixture_commit "$repo" ;;
    list-in-another-case)
      blob=$(printf 'name-two\n' | git -C "$repo" hash-object -w --stdin) || exit 2
      git -C "$repo" update-index --add --cacheinfo "100644,$blob,scripts/r1-gate/Mutants.expected" || exit 2
      fixture_commit "$repo" '' index ;;
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

in_gate_list() {
  "$@" ':(icase)scripts/r1-gate' ':(icase,exclude)scripts/r1-gate/mutants.expected' ':(icase)scripts/r1/ci.d/15-gate-self-test.sh' ':(icase)scripts/ci/run-ci-d.sh' ':(icase)scripts/ci/self-test-alone.txt' ':(icase)docs/baseline/r1-gate-bounds.txt' ':(icase).github/workflows/ci.yml' ':(icase).gitattributes' ':(icase)scripts/.gitattributes' ':(icase)docs/.gitattributes' ':(icase)docs/baseline/.gitattributes'
}

unreadable() {
  printf 'range: unreadable: %s\n' "$1"
  gate_reason="range unreadable: $1"
}

push_base() {
  [ -f "${GITHUB_EVENT_PATH:-}" ] || { unreadable 'GITHUB_EVENT_PATH names no file'; return 1; }
  before=$(event_value before) || { unreadable 'the push event holds no one value of before'; return 1; }
  base=$before
  case $before in
    *[!0-9a-f]*) ;;
    *) range_git rev-parse -q --verify "$before^{commit}" >/dev/null && return 0 ;;
  esac
  default=$(event_value default_branch) || { unreadable "before $before names no commit here, and the push event holds no one value of default_branch"; return 1; }
  ! range_git merge-base --is-ancestor HEAD "refs/remotes/origin/$default" || { unreadable "before $before names no commit here, and HEAD is on refs/remotes/origin/$default, so the merge base gives no range"; return 1; }
  base=$(range_git merge-base HEAD "refs/remotes/origin/$default") || { unreadable "before $before names no commit here, and HEAD has no merge base with refs/remotes/origin/$default"; return 1; }
  printf 'range: before %s names no commit here, so the base is the merge base with refs/remotes/origin/%s\n' "$before" "$default"
}

gate_decision() {
  gate_reason=
  case ${GITHUB_EVENT_NAME:-} in
    schedule) gate_reason='weekly run'; return ;;
    push) push_base || return ;;
    pull_request)
      range_git rev-parse -q --verify 'HEAD^2^{commit}' >/dev/null || { unreadable 'the pull request checkout is no merge commit'; return; }
      base=$(range_git rev-parse 'HEAD^1') || { unreadable 'git rev-parse HEAD^1 failed'; return; }
      ;;
    '') unreadable 'GITHUB_EVENT_NAME is not set'; return ;;
    *) unreadable "the event $GITHUB_EVENT_NAME has no range"; return ;;
  esac
  head=$(range_git rev-parse HEAD) || { unreadable 'git rev-parse HEAD failed'; return; }
  changed=$(in_gate_list range_git diff --name-only --ignore-submodules=none -O/dev/null "$base" "$head" --) || { unreadable "git diff could not read $base..$head"; return; }
  [ -n "$changed" ] || return
  printf '%s\n' "$changed" | sed 's/^/gate file changed: /'
  gate_reason="gate file changed: ${changed%%"$nl"*}"
}

each_once() {
  for each in $3; do
    lines=$(printf '%s\n' "$2" | awk -v g="self-test:$each" '/^PASS self-test:/ && $2 == g')
    seen=$(printf '%s' "$lines" | awk 'END { print NR }')
    [ "$seen" -ne 0 ] || fail "$1 printed no PASS self-test:$each line"
    [ "$seen" -eq 1 ] || fail "$1 printed PASS self-test:$each $seen times"
    rejected=$(printf '%s\n' "$lines" | sed -n 's/.*[( ]\([0-9][0-9]*\) failing fixtures rejected)$/\1/p')
    [ "${rejected:-0}" -gt 0 ] || fail "$1 printed PASS self-test:$each with no count above 0 of failing fixtures rejected: $lines"
  done
}

pass_counts() {
  printf '%s\n' "$1" | awk '/^PASS self-test:/ {
      n++
      if (match($0, /[0-9]+ cases/)) c += substr($0, RSTART, RLENGTH - 6)
      if (match($0, /[0-9]+ failing fixtures rejected/)) f += substr($0, RSTART, RLENGTH - 26)
    }
    END { printf "%d groups, %d cases, %d failing fixtures rejected", n, c, f }'
}

measure() {
  twice=$(names_twice "$CI_D_REPO") || fail 'awk could not read scripts/r1-gate/mutants.expected'
  [ -z "$twice" ] || fail "scripts/r1-gate/mutants.expected holds a name more than once: $twice; keep one line of each"
  printf 'names: no name of scripts/r1-gate/mutants.expected stands twice\n'
  found=$(record_defect "$CI_D_REPO") || fail "$found"
  printf 'record: the last gate line of docs/baseline/r1-gate-main.txt names the tree of the last commit that changed scripts/r1-gate or the bounds file\n'
  gate=$CI_D_REPO/scripts/r1-gate
  gate_decision
  if [ -z "$gate_reason" ]; then
    printf "gate self-tests: not run: no path of the gate's list changed in %s..%s\n" "$base" "$head"
    list=$(in_gate_list echo)
    printf 'gate self-tests: first pass 0 groups; dash pass 0 groups; the list: %s\n' "$list"
    printf 'PASS %s\n' "$name"
    return
  fi
  printf 'gate self-tests: run (%s)\n' "$gate_reason"
  if ! command -v dash >/dev/null 2>&1; then
    printf 'SKIP %s (dash is not installed)\n' "$name"
    exit 77
  fi
  groups=
  for script in "$gate"/check-*.sh; do
    [ -f "$script" ] || continue
    group=${script##*/check-}
    groups="$groups ${group%.sh}"
  done
  self_tested 'first pass: run.sh --self-test' '' sh "$gate/run.sh" --self-test
  each_once 'first pass' "$out" "$groups run helper tree mutation"
  first_counts=$(pass_counts "$out")
  dash_out=
  for script in "$gate"/check-*.sh; do
    [ -f "$script" ] || fail "scripts/r1-gate holds no check-*.sh"
    group=${script##*/check-}
    self_tested "dash pass: dash ${script##*/} --self-test" "${group%.sh}" dash "$script" --self-test
    dash_out=$dash_out$nl$out
  done
  each_once 'dash pass' "$dash_out" "$groups"
  dash_counts=$(pass_counts "$dash_out")
  printf 'gate self-tests: first pass %s; dash pass %s\n' "$first_counts" "$dash_counts"
  printf 'PASS %s\n' "$name"
}

outside_reads() {
  reads=$(git -C "$1" grep -n -I -o -E '\$\{?(GATE_ROOT|CI_D_REPO)\}?/[A-Za-z0-9_./-]*' -- scripts/r1-gate)
  [ $? -le 1 ] || return 2
  calls=$(git -C "$1" grep -n -I -E 'git -C "?\$\{?GATE_ROOT' -- scripts/r1-gate)
  [ $? -le 1 ] || return 2
  list=$(in_gate_list echo)
  printf 'R %s\nC %s\n' "$reads" "$calls" | awk -v list="$list" '
    BEGIN {
      n = split(list, spec, " ")
      for (i = 1; i <= n; i++) {
        sub(/^:\([a-z,]*\)/, "", spec[i])
        listed[tolower(spec[i])] = 1
      }
    }
    function outside(p,   q) {
      q = tolower(p)
      sub(/\/+$/, "", q)
      if (q == "" || q == "scripts/r1-gate" || index(q, "scripts/r1-gate/") == 1 || index(q, "target/") == 1) return 0
      if (q == "changed-during-the-run") return 0
      return !(q in listed)
    }
    $1 == "R" && match($0, /\$[{]?(GATE_ROOT|CI_D_REPO)[}]?\//) {
      p = substr($0, RSTART + RLENGTH)
      if (outside(p)) print substr($0, 3)
      next
    }
    $1 == "C" && NF > 1 {
      for (i = 2; i <= NF; i++) {
        w = $i
        gsub(/["\047]/, "", w)
        sub(/^[A-Za-z0-9_.^~{}-]*:/, "", w)
        if (w ~ /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.\/-]*$/ && outside(w)) print substr($0, 3) ": " w
      }
    }'
}

check() {
  out=$(
    unset GITHUB_EVENT_NAME GITHUB_EVENT_PATH
    [ -z "$event" ] || export GITHUB_EVENT_NAME="$event"
    [ -z "$event_path" ] || export GITHUB_EVENT_PATH="$event_path"
    for exported in $caller; do export "${exported?}"; done
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
  verdict "$1" "$2" "$got"
  path=$PATH
  event=
  event_path=
  caller=
}

verdict() {
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$3"
  if [ "$3" != "$2" ]; then
    wrong="$wrong $1"
  elif [ "$2" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

ranged() {
  gate "$1" "${5:-$run_pass}" store "${6:-[[ a == a ]] && printf \"PASS self-test:store (1 cases)\\n\"}"
  mkdir -p "$work/$1/scripts/r1/ci.d" || exit 2
  printf 'check\n' >"$work/$1/scripts/r1/ci.d/$name.sh"
  [ -z "$stale" ] || printf '# after the record\n' >>"$work/$1/scripts/r1-gate/run.sh"
  fixture_commit "$work/$1"
  from=$(git -C "$work/$1" rev-parse HEAD) || exit 2
  case $2 in
    gitlink)
      git -C "$work/$1" update-index --add --cacheinfo "160000,$from,scripts/r1-gate/sub" || exit 2
      fixture_record "$work/$1" index ;;
    gitlink-ignored)
      printf '[submodule "sub"]\n\tpath = scripts/r1-gate/sub\n\turl = ./sub\n\tignore = all\n' >"$work/$1/.gitmodules"
      git -C "$work/$1" add -- .gitmodules >/dev/null 2>&1 || exit 2
      git -C "$work/$1" update-index --add --cacheinfo "160000,$from,scripts/r1-gate/sub" || exit 2
      fixture_record "$work/$1" index ;;
    other) printf 'notes\n' >"$work/$1/README.md"; fixture_commit "$work/$1" ;;
    gate) printf '# a change\n' >>"$work/$1/scripts/r1-gate/check-store.sh"; fixture_record "$work/$1" ;;
    check) printf '# a change\n' >>"$work/$1/scripts/r1/ci.d/$name.sh"; fixture_commit "$work/$1" ;;
    expected)
      printf 'row\n' >>"$work/$1/scripts/r1-gate/mutants.expected"
      record_line=$(tail -n 1 "$work/$1/docs/baseline/r1-gate-main.txt") || exit 2
      printf '%s\n' "$record_line" >>"$work/$1/docs/baseline/r1-gate-main.txt"
      fixture_commit "$work/$1" ;;
    two)
      mkdir -p "$work/$1/scripts/ci" "$work/$1/.github/workflows" || exit 2
      printf '# a change\n' >>"$work/$1/scripts/ci/run-ci-d.sh"
      printf '# a change\n' >>"$work/$1/.github/workflows/ci.yml"
      fixture_commit "$work/$1" ;;
    renamed-out)
      mkdir -p "$work/$1/scripts/old" || exit 2
      mv "$work/$1/scripts/r1-gate/check-store.sh" "$work/$1/scripts/old/check-store.sh" || exit 2
      fixture_record "$work/$1" ;;
    other-case)
      blob=$(printf 'x\n' | git -C "$work/$1" hash-object -w --stdin) || exit 2
      git -C "$work/$1" update-index --add --cacheinfo "100644,$blob,scripts/R1-gate/check-x.sh" || exit 2
      fixture_commit "$work/$1" '' index ;;
    docs/baseline/r1-gate-bounds.txt) printf 'x=2\n' >"$work/$1/$2"; fixture_record "$work/$1" ;;
    *)
      mkdir -p "$(dirname "$work/$1/$2")" || exit 2
      printf '# a change\n' >>"$work/$1/$2"
      fixture_commit "$work/$1" ;;
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
  want="range: unreadable: $1${nl}gate self-tests: run (range unreadable: $1)"
}

not_run() {
  list=$(in_gate_list echo)
  want="gate self-tests: not run: no path of the gate's list changed in $from..$to${nl}gate self-tests: first pass 0 groups; dash pass 0 groups; the list: $list${nl}PASS $name"
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

scan_case() {
  repo=$work/scan-$1
  mkdir -p "$repo/scripts/r1-gate" || exit 2
  git init -q "$repo" >/dev/null 2>&1 || exit 2
  printf '%s\n' "$3" | tr @ \$ >"$repo/scripts/r1-gate/check-x.sh"
  git -C "$repo" add -A >/dev/null 2>&1 || exit 2
  found=$(outside_reads "$repo")
  got=PASS
  [ -z "$found" ] || got=FAIL
  verdict "scan-$1" "$2" "$got"
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  mkdir -p "$work/no-tools"
  shell=$(command -v sh) || exit 2
  path=$PATH
  pass='[ "$*" = --self-test ] || exit 3; printf "PASS self-test:@group@ (1 cases, 1 failing fixtures rejected)\n"'
  run_groups="[ \"\$*\" = --self-test ] || exit 3; for f in \"\${0%/*}\"/check-*.sh; do [ -f \"\$f\" ] || continue; g=\${f##*/check-}; printf \"PASS self-test:%s (2 cases, 1 failing fixtures rejected)\\n\" \"\${g%.sh}\"; done; for g in"
  run_pass="$run_groups run helper tree mutation; do printf \"PASS self-test:%s (2 cases, 1 failing fixtures rejected)\\n\" \"\$g\"; done"
  fail_then_pass='printf "FAIL self-test:@group@ (case x: wrong verdict)\nPASS self-test:@group@ (1 cases)\n"'
  planned='printf "FAIL self-test:store (planned)\n"; exit 1'
  rejected=0
  wrong=
  event=
  event_path=
  caller=
  stale=
  zero=0000000000000000000000000000000000000000

  gate gate-passes "$run_pass" store "$pass" timed-run "$pass"
  check gate-passes PASS "gate self-tests: first pass 6 groups, 12 cases, 6 failing fixtures rejected; dash pass 2 groups, 2 cases, 2 failing fixtures rejected${nl}PASS $name"
  gate run-fails "$planned" store "$pass"
  check run-fails FAIL 'first pass: run.sh --self-test exited 1)'
  gate run-prints-no-pass-line 'exit 0' store "$pass"
  check run-prints-no-pass-line FAIL 'run.sh --self-test printed no PASS self-test line)'
  gate pass-words-inside-a-line 'printf "the line PASS self-test:store is missing\n"' store "$pass"
  check pass-words-inside-a-line FAIL 'run.sh --self-test printed no PASS self-test line)'
  gate no-check-script "$run_pass"
  check no-check-script FAIL 'scripts/r1-gate holds no check-*.sh)'
  gate check-fails "$run_pass" store "$planned"
  check check-fails FAIL 'dash pass: dash check-store.sh --self-test exited 1)'
  gate check-prints-no-pass-line "$run_pass" store 'exit 0'
  check check-prints-no-pass-line FAIL 'dash check-store.sh --self-test printed no PASS self-test:store line)'
  gate second-check-fails "$run_pass" store "$pass" timed-run "$planned"
  check second-check-fails FAIL 'dash check-timed-run.sh --self-test exited 1)'
  gate check-with-another-initial-fails "$run_pass" store "$pass" cost "$planned"
  check check-with-another-initial-fails FAIL 'dash check-cost.sh --self-test exited 1)'
  gate check-needs-bash "$run_pass" store '[[ a == a ]] && printf "PASS self-test:store\n"'
  check check-needs-bash FAIL 'dash check-store.sh --self-test exited 127)'
  gate run-prints-a-fail-line "$fail_then_pass" store "$pass"
  check run-prints-a-fail-line FAIL 'run.sh --self-test printed: FAIL self-test:store (case x: wrong verdict))'
  gate check-prints-a-fail-line "$run_pass" store "$fail_then_pass"
  check check-prints-a-fail-line FAIL 'dash check-store.sh --self-test printed: FAIL self-test:store (case x: wrong verdict))'
  gate check-passes-as-another "$run_pass" store '[ "$*" = --self-test ] || exit 3; printf "PASS self-test:deletion (1 cases)\n"'
  check check-passes-as-another FAIL 'dash check-store.sh --self-test printed no PASS self-test:store line)'
  gate check-group-is-a-prefix "$run_pass" store '[ "$*" = --self-test ] || exit 3; printf "PASS self-test:store-other (1 cases)\n"'
  check check-group-is-a-prefix FAIL 'dash check-store.sh --self-test printed no PASS self-test:store line)'
  gate check-passes-with-0-cases "$run_pass" store '[ "$*" = --self-test ] || exit 3; printf "PASS self-test:store (0 cases)\n"'
  check check-passes-with-0-cases FAIL 'dash check-store.sh --self-test printed a PASS line with 0 cases: PASS self-test:store (0 cases))'
  gate check-rejects-no-fixture "$run_pass" store '[ "$*" = --self-test ] || exit 3; printf "PASS self-test:store (3 cases, 0 failing fixtures rejected)\n"'
  check check-rejects-no-fixture FAIL 'dash check-store.sh --self-test rejected 0 failing fixtures: PASS self-test:store (3 cases, 0 failing fixtures rejected))'
  gate run-rejects-no-fixture 'printf "PASS self-test:run (5 cases, 0 failing fixtures rejected)\n"' store "$pass"
  check run-rejects-no-fixture FAIL 'run.sh --self-test rejected 0 failing fixtures: PASS self-test:run (5 cases, 0 failing fixtures rejected))'
  gate bare-zero-rejected "$run_pass" store '[ "$*" = --self-test ] || exit 3; printf "PASS self-test:store (0 failing fixtures rejected)\n"'
  check bare-zero-rejected FAIL 'dash check-store.sh --self-test rejected 0 failing fixtures: PASS self-test:store (0 failing fixtures rejected))'
  gate ten-fixtures-rejected "$run_pass" store '[ "$*" = --self-test ] || exit 3; printf "PASS self-test:store (12 cases, 10 failing fixtures rejected)\n"'
  check ten-fixtures-rejected PASS 'gate self-tests: first pass 5 groups, 10 cases, 5 failing fixtures rejected; dash pass 1 groups, 12 cases, 10 failing fixtures rejected'
  mkdir -p "$work/check-script-renamed/scripts" "$work/check-script-renamed/docs" || exit 2
  cp -R "$CI_D_REPO/scripts/r1-gate" "$work/check-script-renamed/scripts/" || exit 2
  cp -R "$CI_D_REPO/docs/baseline" "$work/check-script-renamed/docs/" || exit 2
  mv "$work/check-script-renamed/scripts/r1-gate/check-store.sh" "$work/check-script-renamed/scripts/r1-gate/check-store-old.sh" || exit 2
  fixture_record "$work/check-script-renamed"
  check check-script-renamed FAIL 'FAIL self-test:run (check-store.sh is missing from'
  gate gate-with-a-stale-record "$run_pass" store "$pass"
  printf '# a change after the record\n' >>"$work/gate-with-a-stale-record/scripts/r1-gate/run.sh"
  fixture_commit "$work/gate-with-a-stale-record"
  check gate-with-a-stale-record FAIL ': make the record again)'
  record_case matches PASS
  record_case other-file-changed PASS
  record_case stale FAIL
  record_case bounds-changed FAIL
  record_case list-changed PASS
  record_case names-added-with-their-rows PASS
  record_case list-edited-after-the-record PASS
  record_case list-and-lib-changed FAIL
  record_case list-in-another-case PASS
  gate list-holds-a-name-twice "$run_pass" store "$pass"
  printf 'name-one\n\nname-two\nname-one\n\nname-three\nname-two\nname-one\n' >"$work/list-holds-a-name-twice/scripts/r1-gate/mutants.expected"
  fixture_commit "$work/list-holds-a-name-twice"
  check list-holds-a-name-twice FAIL 'FAIL 15-gate-self-test (scripts/r1-gate/mutants.expected holds a name more than once: name-one name-two; keep one line of each)'
  gate list-holds-each-name-once "$run_pass" store "$pass"
  printf 'name-one\n\nname-two\n\nname-three\n' >"$work/list-holds-each-name-once/scripts/r1-gate/mutants.expected"
  fixture_commit "$work/list-holds-each-name-once"
  check list-holds-each-name-once PASS "names: no name of scripts/r1-gate/mutants.expected stands twice${nl}record: the last gate line of docs/baseline/r1-gate-main.txt"
  gate list-cannot-be-read "$run_pass" store "$pass"
  printf 'name-one\n' >"$work/list-cannot-be-read/scripts/r1-gate/mutants.expected"
  fixture_commit "$work/list-cannot-be-read"
  chmod 000 "$work/list-cannot-be-read/scripts/r1-gate/mutants.expected" || exit 2
  check list-cannot-be-read FAIL 'FAIL 15-gate-self-test (awk could not read scripts/r1-gate/mutants.expected)'
  run_changed="gate file changed: scripts/r1-gate/check-store.sh${nl}gate self-tests: run (gate file changed: scripts/r1-gate/check-store.sh)"
  new_branch="{\"before\": \"$zero\", \"repository\": {\"default_branch\": \"trunk\"}}"
  ranged push-no-gate-file-changed other push '{"before": "@from@", "after": "@to@"}' "$planned"
  not_run
  check push-no-gate-file-changed PASS "$want"
  ranged push-gate-file-changed gate push '{"before": "@from@", "after": "@to@"}'
  check push-gate-file-changed FAIL "$run_changed"
  ranged push-gate-file-changed-counts gate push '{"before": "@from@"}' "$run_pass" "$pass"
  check push-gate-file-changed-counts PASS "$run_changed${nl}PASS self-test:store (2 cases, 1 failing fixtures rejected)${nl}PASS self-test:run (2 cases, 1 failing fixtures rejected)${nl}PASS self-test:helper (2 cases, 1 failing fixtures rejected)${nl}PASS self-test:tree (2 cases, 1 failing fixtures rejected)${nl}PASS self-test:mutation (2 cases, 1 failing fixtures rejected)${nl}PASS self-test:store (1 cases, 1 failing fixtures rejected)${nl}gate self-tests: first pass 5 groups, 10 cases, 5 failing fixtures rejected; dash pass 1 groups, 1 cases, 1 failing fixtures rejected${nl}PASS $name"
  ranged push-check-changed check push '{"before": "@from@", "after": "@to@"}'
  check push-check-changed FAIL "gate file changed: scripts/r1/ci.d/$name.sh${nl}gate self-tests: run (gate file changed: scripts/r1/ci.d/$name.sh)"
  for listed in scripts/ci/run-ci-d.sh scripts/ci/self-test-alone.txt docs/baseline/r1-gate-bounds.txt .github/workflows/ci.yml .gitattributes scripts/.gitattributes docs/.gitattributes docs/baseline/.gitattributes; do
    listed_case=listed$(printf '%s' "$listed" | tr '/.' '--')
    ranged "$listed_case" "$listed" push '{"before": "@from@"}' "$planned"
    check "$listed_case" FAIL "gate self-tests: run (gate file changed: $listed)${nl}FAIL self-test:store (planned)${nl}FAIL $name (first pass: run.sh --self-test exited 1)"
  done
  ranged two-listed-paths two push '{"before": "@from@"}' "$planned"
  check two-listed-paths FAIL "gate file changed: .github/workflows/ci.yml${nl}gate file changed: scripts/ci/run-ci-d.sh${nl}gate self-tests: run (gate file changed: .github/workflows/ci.yml)"
  ranged only-mutants-expected-and-the-record expected push '{"before": "@from@"}' "$planned"
  not_run
  check only-mutants-expected-and-the-record PASS "$want"
  ranged renamed-out-of-the-gate renamed-out push '{"before": "@from@"}'
  check renamed-out-of-the-gate FAIL 'gate self-tests: run (gate file changed: scripts/r1-gate/check-store.sh)'
  ranged gate-in-another-case other-case push '{"before": "@from@"}'
  check gate-in-another-case FAIL 'gate self-tests: run (gate file changed: scripts/R1-gate/check-x.sh)'
  ranged new-branch-no-gate-file-changed other push "$new_branch" "$planned"
  git -C "$work/new-branch-no-gate-file-changed" update-ref refs/remotes/origin/trunk "$from" || exit 2
  not_run
  check new-branch-no-gate-file-changed PASS "range: before $zero names no commit here, so the base is the merge base with refs/remotes/origin/trunk${nl}$want"
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
  ranged push-diff-fails other push "" "$planned"
  broken=$(printf 'tree 1111111111111111111111111111111111111111\nauthor gate fixture <fixture@example.invalid> 1 +0000\ncommitter gate fixture <fixture@example.invalid> 1 +0000\n\nfixture\n' | git -C "$work/push-diff-fails" hash-object -t commit -w --stdin) || exit 2
  event_path=$work/push-diff-fails.json
  printf '{"before": "%s"}\n' "$broken" >"$event_path"
  unread "git diff could not read $broken..$to"
  check push-diff-fails FAIL "$want${nl}FAIL self-test:store (planned)${nl}FAIL $name (first pass: run.sh --self-test exited 1)"
  ranged pull-request-no-gate-file-changed other pull_request "" "$planned"
  merged pull-request-no-gate-file-changed
  not_run
  check pull-request-no-gate-file-changed PASS "$want"
  ranged pull-request-gate-file-changed gate pull_request
  merged pull-request-gate-file-changed
  check pull-request-gate-file-changed FAIL "$run_changed"
  ranged pull-request-head-not-a-merge other pull_request
  unread 'the pull request checkout is no merge commit'
  check pull-request-head-not-a-merge FAIL "$want"
  ranged weekly-run other schedule "" "$planned"
  check weekly-run FAIL "record: the last gate line of docs/baseline/r1-gate-main.txt names the tree of the last commit that changed scripts/r1-gate or the bounds file${nl}gate self-tests: run (weekly run)${nl}FAIL self-test:store (planned)${nl}FAIL $name (first pass: run.sh --self-test exited 1)"
  ranged weekly-run-dash-pass other schedule
  check weekly-run-dash-pass FAIL "gate self-tests: run (weekly run)${nl}PASS self-test:store (2 cases"
  ranged no-event other "" "" "$planned"
  unread 'GITHUB_EVENT_NAME is not set'
  check no-event FAIL "$want${nl}FAIL self-test:store (planned)${nl}FAIL $name (first pass: run.sh --self-test exited 1)"
  ranged event-without-a-range other workflow_dispatch
  unread 'the event workflow_dispatch has no range'
  check event-without-a-range FAIL "$want"
  ranged run-fails-not-run other push '{"before": "@from@"}' "$planned"
  not_run
  check run-fails-not-run PASS "$want"
  for assignment in GATE_ROOT=/nonexistent GATE_OVERRIDE_X=1 GATE_TREE_STATE=clean; do
    caller_case=caller-${assignment%%=*}
    ranged "$caller_case" gate push '{"before": "@from@"}' "env | grep ^GATE_; $run_pass"
    caller=$assignment
    check "$caller_case" FAIL "$run_changed${nl}$assignment"
    ranged "$caller_case-not-run" other push '{"before": "@from@"}' "$planned"
    not_run
    caller=$assignment
    check "$caller_case-not-run" PASS "$want"
  done
  gate override-line '[ "$*" = --self-test ] || exit 3; printf "PASS self-test:store (2 cases, 1 failing fixtures rejected) [override: x=2]\n"' store "$pass"
  check override-line FAIL 'first pass: run.sh --self-test printed a PASS line with an override or a dirty tree: PASS self-test:store (2 cases, 1 failing fixtures rejected) [override: x=2])'
  gate dirty-tree-line "$run_pass" store '[ "$*" = --self-test ] || exit 3; printf "PASS self-test:store (1 cases) [dirty tree; x]\n"'
  check dirty-tree-line FAIL 'dash pass: dash check-store.sh --self-test printed a PASS line with an override or a dirty tree: PASS self-test:store (1 cases) [dirty tree; x])'
  gate first-pass-lacks-a-group "$run_groups run helper mutation; do printf \"PASS self-test:%s (2 cases, 1 failing fixtures rejected)\\n\" \"\$g\"; done" store "$pass"
  check first-pass-lacks-a-group FAIL 'FAIL 15-gate-self-test (first pass printed no PASS self-test:tree line)'
  gate first-pass-group-twice "$run_groups run helper helper tree mutation; do printf \"PASS self-test:%s (2 cases, 1 failing fixtures rejected)\\n\" \"\$g\"; done" store "$pass"
  check first-pass-group-twice FAIL 'FAIL 15-gate-self-test (first pass printed PASS self-test:helper 2 times)'
  gate first-pass-lacks-a-check-group "[ \"\$*\" = --self-test ] || exit 3; for g in run helper tree mutation; do printf \"PASS self-test:%s (2 cases, 1 failing fixtures rejected)\\n\" \"\$g\"; done" store "$pass"
  check first-pass-lacks-a-check-group FAIL 'FAIL 15-gate-self-test (first pass printed no PASS self-test:store line)'
  gate dash-pass-group-twice "$run_pass" store '[ "$*" = --self-test ] || exit 3; printf "PASS self-test:store (1 cases)\nPASS self-test:store (1 cases)\n"'
  check dash-pass-group-twice FAIL 'FAIL 15-gate-self-test (dash pass printed PASS self-test:store 2 times)'
  got=PASS
  gate_list=$(in_gate_list printf '%s\n')
  [ "$gate_list" = ":(icase)scripts/r1-gate${nl}:(icase,exclude)scripts/r1-gate/mutants.expected${nl}:(icase)scripts/r1/ci.d/$name.sh${nl}:(icase)scripts/ci/run-ci-d.sh${nl}:(icase)scripts/ci/self-test-alone.txt${nl}:(icase)docs/baseline/r1-gate-bounds.txt${nl}:(icase).github/workflows/ci.yml${nl}:(icase).gitattributes${nl}:(icase)scripts/.gitattributes${nl}:(icase)docs/.gitattributes${nl}:(icase)docs/baseline/.gitattributes" ] || got=FAIL
  verdict the-gate-list PASS "$got"
  found=$(outside_reads "$CI_D_REPO")
  got=PASS
  [ -z "$found" ] || { got=FAIL; printf '%s\n' "$found"; }
  verdict gate-reads-in-the-list PASS "$got"
  scan_case read-outside FAIL 'cat "@GATE_ROOT/docs/other.txt"'
  scan_case repo-read-outside FAIL 'cat "@{CI_D_REPO}/Cargo.lock"'
  scan_case git-read-outside FAIL 'git -C "@GATE_ROOT" show HEAD:crates/x/src/lib.rs'
  scan_case stub-file-of-run PASS ': > "@GATE_ROOT/changed-during-the-run"'
  scan_case another-stub-file FAIL ': > "@GATE_ROOT/changed-during-the-walk"'
  scan_case reads-in-the-list PASS 'cat "@GATE_ROOT/docs/baseline/r1-gate-bounds.txt" "@GATE_ROOT/target/r1-gate/x" "@GATE_ROOT/scripts/r1-gate/lib.sh"'
  stale=yes
  ranged stale-record-push-not-run other push '{"before": "@from@"}' "$planned"
  stale=
  check stale-record-push-not-run FAIL ': make the record again)'
  stale=yes
  ranged stale-record-pull-request-not-run other pull_request "" "$planned"
  stale=
  merged stale-record-pull-request-not-run
  check stale-record-pull-request-not-run FAIL ': make the record again)'
  gate dash-group-without-a-fixture-count "$run_pass" store '[ "$*" = --self-test ] || exit 3; printf "PASS self-test:store (4 cases)\n"'
  check dash-group-without-a-fixture-count FAIL 'FAIL 15-gate-self-test (dash pass printed PASS self-test:store with no count above 0 of failing fixtures rejected: PASS self-test:store (4 cases))'
  gate first-pass-group-with-00-fixtures "$run_groups run helper mutation; do printf \"PASS self-test:%s (2 cases, 1 failing fixtures rejected)\\n\" \"\$g\"; done; printf \"PASS self-test:tree (2 cases, 00 failing fixtures rejected)\\n\"" store "$pass"
  check first-pass-group-with-00-fixtures FAIL 'FAIL 15-gate-self-test (first pass printed PASS self-test:tree with no count above 0 of failing fixtures rejected: PASS self-test:tree (2 cases, 00 failing fixtures rejected))'
  gate first-pass-indented-group-line "$run_groups run helper mutation; do printf \"PASS self-test:%s (2 cases, 1 failing fixtures rejected)\\n\" \"\$g\"; done; printf \"  PASS self-test:tree (2 cases, 1 failing fixtures rejected)\\n\"" store "$pass"
  check first-pass-indented-group-line FAIL 'FAIL 15-gate-self-test (first pass printed no PASS self-test:tree line)'
  ranged caller-git-literal-pathspecs gate push '{"before": "@from@"}' "$planned"
  caller=GIT_LITERAL_PATHSPECS=1
  check caller-git-literal-pathspecs FAIL 'gate self-tests: run (gate file changed: scripts/r1-gate/check-store.sh)'
  submodule_changed="gate file changed: scripts/r1-gate/sub${nl}gate self-tests: run (gate file changed: scripts/r1-gate/sub)"
  ranged caller-config-count-ignore-submodules gitlink push '{"before": "@from@"}' "$planned"
  caller='GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=diff.ignoreSubmodules GIT_CONFIG_VALUE_0=all'
  check caller-config-count-ignore-submodules FAIL "$submodule_changed"
  ranged caller-config-parameters-ignore-submodules gitlink push '{"before": "@from@"}' "$planned"
  caller="GIT_CONFIG_PARAMETERS='diff.ignoreSubmodules'='all'"
  check caller-config-parameters-ignore-submodules FAIL "$submodule_changed"
  ranged caller-work-tree-gitmodules gitlink push '{"before": "@from@"}' "$planned"
  mkdir -p "$work/caller-work-tree-elsewhere" || exit 2
  printf '[submodule "sub"]\n\tpath = scripts/r1-gate/sub\n\turl = ./sub\n\tignore = all\n' >"$work/caller-work-tree-elsewhere/.gitmodules"
  caller=GIT_WORK_TREE=$work/caller-work-tree-elsewhere
  check caller-work-tree-gitmodules FAIL "$submodule_changed"
  ranged checkout-gitmodules-ignore-all gitlink-ignored push '{"before": "@from@"}' "$planned"
  check checkout-gitmodules-ignore-all FAIL "$submodule_changed"
  ranged checkout-config-ignore-submodules gitlink push '{"before": "@from@"}' "$planned"
  git -C "$work/checkout-config-ignore-submodules" config diff.ignoreSubmodules all || exit 2
  check checkout-config-ignore-submodules FAIL "$submodule_changed"
  ranged caller-replace-ref-base check push '{"before": "@from@"}' "$planned"
  git -C "$work/caller-replace-ref-base" update-ref "refs/caller/$to" "$from" || exit 2
  caller=GIT_REPLACE_REF_BASE=refs/caller/
  check caller-replace-ref-base FAIL "gate self-tests: run (gate file changed: scripts/r1/ci.d/$name.sh)"
  ranged caller-replace-ref-base-ci-yml .github/workflows/ci.yml push '{"before": "@from@"}' "$planned"
  git -C "$work/caller-replace-ref-base-ci-yml" update-ref "refs/caller/$to" "$from" || exit 2
  caller=GIT_REPLACE_REF_BASE=refs/caller/
  check caller-replace-ref-base-ci-yml FAIL 'gate self-tests: run (gate file changed: .github/workflows/ci.yml)'
  ranged checkout-replace-ref check push '{"before": "@from@"}' "$planned"
  git -C "$work/checkout-replace-ref" update-ref "refs/replace/$to" "$from" || exit 2
  check checkout-replace-ref FAIL "gate self-tests: run (gate file changed: scripts/r1/ci.d/$name.sh)"
  ranged checkout-graft-pull-request gate pull_request "" "$planned"
  branch_tip=$to
  merged checkout-graft-pull-request
  mkdir -p "$work/checkout-graft-pull-request/.git/info" || exit 2
  printf '%s %s %s\n' "$to" "$branch_tip" "$from" >"$work/checkout-graft-pull-request/.git/info/grafts"
  check checkout-graft-pull-request FAIL 'gate self-tests: run (gate file changed: scripts/r1-gate/check-store.sh)'
  ranged checkout-order-file two push '{"before": "@from@"}' "$planned"
  printf 'scripts/ci/run-ci-d.sh\n' >"$work/checkout-order-file.order"
  git -C "$work/checkout-order-file" config diff.orderFile "$work/checkout-order-file.order" || exit 2
  check checkout-order-file FAIL "gate file changed: .github/workflows/ci.yml${nl}gate file changed: scripts/ci/run-ci-d.sh${nl}gate self-tests: run (gate file changed: .github/workflows/ci.yml)"
  ranged caller-home-gitconfig gitlink push '{"before": "@from@"}' "$planned"
  mkdir -p "$work/caller-home" || exit 2
  printf '[diff]\n\tignoreSubmodules = all\n' >"$work/caller-home/.gitconfig"
  caller=HOME=$work/caller-home
  check caller-home-gitconfig FAIL "$submodule_changed"
  gate dash-group-with-only-a-fixture-count "$run_pass" store '[ "$*" = --self-test ] || exit 3; printf "PASS self-test:store (5 failing fixtures rejected)\n"'
  check dash-group-with-only-a-fixture-count PASS 'dash pass 1 groups, 0 cases, 5 failing fixtures rejected'
  gate first-pass-tab-after-pass "$run_groups run helper mutation; do printf \"PASS self-test:%s (2 cases, 1 failing fixtures rejected)\\n\" \"\$g\"; done; printf \"PASS\\tself-test:tree (2 cases, 1 failing fixtures rejected)\\n\"" store "$pass"
  check first-pass-tab-after-pass FAIL 'FAIL 15-gate-self-test (first pass printed no PASS self-test:tree line)'
  ranged caller-git-dir-repository other push '{"before": "@from@"}'
  ranged caller-git-dir gate push '{"before": "@from@"}' "$planned"
  caller=GIT_DIR=$work/caller-git-dir-repository/.git
  check caller-git-dir FAIL 'gate self-tests: run (gate file changed: scripts/r1-gate/check-store.sh)'
  ranged caller-object-directory gate push '{"before": "@from@"}' "$planned"
  mkdir -p "$work/caller-object-directory-empty" || exit 2
  caller=GIT_OBJECT_DIRECTORY=$work/caller-object-directory-empty
  check caller-object-directory FAIL 'gate self-tests: run (gate file changed: scripts/r1-gate/check-store.sh)'
  gate dash-missing "$run_pass" store "$pass"
  for tool in sh git sed tail cut awk grep; do
    tool_path=$(command -v "$tool") || exit 2
    ln -s "$tool_path" "$work/no-tools/$tool" || exit 2
  done
  path=$work/no-tools
  check dash-missing SKIP 'SKIP 15-gate-self-test (dash is not installed)'
  ranged dash-missing-not-run other push '{"before": "@from@"}'
  path=$work/no-tools
  not_run
  check dash-missing-not-run PASS "$want"

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
