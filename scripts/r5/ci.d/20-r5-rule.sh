#!/bin/sh
# jobs: macos windows
# rule: Each of the 30 tests of the rule's closed list (contracts.md L3.9, L11.1) that its crate holds reads ok in cargo test --locked, every test marked built is there, and with --all every one of them is there; the check refuses its own file when it differs from HEAD or carries an index flag, holds the list and the workspace's crates before its first test run, and refuses a tree that one of its cargo commands changed (a tracked file, HEAD, an untracked file or an index flag); it does not hold code that runs in the job before its reads (a test binary, a tool earlier on the PATH, code before main in a test file, a target with its harness off), as the routing run's SG-30 says.
set -u
LC_ALL=C
export LC_ALL
name=20-r5-rule
crates='halvo-learn halvo-schema halvo-learn-store halvo-learn-gate'
script_path=scripts/r5/ci.d/20-r5-rule.sh
lfx=$(printf '\nx')
nl=${lfx%x}
crx=$(printf '\rx')
cr=${crx%x}

finish() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

closed_list() {
  cat <<'EOF'
halvo-learn refusal_unverified_run built
halvo-learn refusal_no_source built
halvo-learn refusal_value_in_label built
halvo-learn refusal_rejected_again built
halvo-learn refusal_compile_run_edit built
halvo-learn refusal_transient_keeps built
halvo-learn refusal_edits_full built
halvo-learn refusal_second_pending built
halvo-learn refusal_failed_after_recovered built
halvo-learn refusal_failure_of_other_step built
halvo-learn refusal_steps_apart built
halvo-learn-store refusal_page_text_instruction built
halvo-learn-store refusal_class_change built
halvo-learn-store refusal_step_too_long built
halvo-learn-store refusal_reverted_not_served built
halvo-learn-store refusal_replay_put_not_stored built
halvo-learn-store refusal_revoke_keeps_row built
halvo-learn-store refusal_cross_workflow_source built
halvo-schema refusal_edit_extra_key built
halvo-learn-gate refusal_delete_leaves_others pending
halvo-learn-gate refusal_delete_takes_revoked pending
halvo-learn-gate refusal_step_writes_no_edit pending
halvo-learn edit_table built
halvo-learn routine_sample built
halvo-learn formal_traces pending
halvo-learn-gate constants_match_targets pending
halvo-schema kinds_have_workflow_key built
halvo-schema r5_golden_records built
halvo-schema r5_golden_requests built
halvo-schema the_edit_kind_is_kept_of_origin_own_and_holds_the_fields_of_its_row_in_a_closed_table built
EOF
}

names_of() {
  found=
  rest=$closed
  while [ -n "$rest" ]; do
    one=${rest%%"$nl"*}
    case $rest in
      *"$nl"*) rest=${rest#*"$nl"} ;;
      *) rest= ;;
    esac
    if [ "${one%% *}" = "$1" ]; then
      mark=${one##* }
      test=${one#* }
      found="$found ${test%% *}/$mark"
    fi
  done
  printf '%s\n' "${found# }"
}

read_results() {
  awk -v crate="$1" -v names="$2" -v present="$3" '
    BEGIN {
      n = split(names, list, " ")
      for (i = 1; i <= n; i++) {
        split(list[i], pair, "/")
        list[i] = pair[1]
        mark[pair[1]] = pair[2]
        state[pair[1]] = "pending"
      }
    }
    present == "yes" {
      sub(/\r$/, "")
      if ($0 !~ /^test [^ ]+ \.\.\. /) next
      path = $2
      sub(/^.*::/, "", path)
      if (!(path in state)) next
      result = substr($0, index($0, " ... ") + 5)
      if (result != "ok") state[path] = "miss"
      else if (state[path] == "pending") state[path] = "ok"
    }
    END {
      for (i = 1; i <= n; i++) {
        test = list[i]
        if (state[test] == "pending" && mark[test] != "pending") state[test] = "miss"
        print crate, test, state[test]
      }
    }
  '
}

status_now() {
  git -c core.fsmonitor=false -c core.untrackedCache=false status --porcelain=v2 --branch --untracked-files=all
}

files_now() {
  git ls-files -v
}

tree_start() {
  status_before=$(status_now 2>/dev/null) || status_before=
  files_before=$(files_now 2>/dev/null) || files_before=
  if [ -z "$status_before" ] || [ -z "$files_before" ]; then
    printf 'tree head=unreadable\n'
    finish tests
  fi
  own=no
  case "$nl$status_before$nl" in
    *" $script_path$nl"*) own=yes ;;
  esac
  case "$nl$files_before$nl" in
    *"${nl}H $script_path$nl"*) ;;
    *) own=yes ;;
  esac
  if [ "$own" = yes ]; then
    printf 'list same=no\n'
    finish list
  fi
  flagged=$(printf '%s\n' "$files_before" | awk '$1 ~ /^([a-z]|S)$/ { n++ } END { print n + 0 }') || finish read
  if [ "$flagged" -gt 0 ]; then
    printf 'tree flagged=%s\n' "$flagged"
    finish tests
  fi
}

tree_same() {
  status_after=$(status_now 2>/dev/null) || status_after=unreadable
  files_after=$(files_now 2>/dev/null) || files_after=unreadable
  if [ "$status_after" = "$status_before" ] && [ "$files_after" = "$files_before" ]; then
    return 0
  fi
  printf '%s\n%s\n' "$status_before" "$files_before" >"$dir/tree-before.txt" || finish artifacts
  printf '%s\n%s\n' "$status_after" "$files_after" >"$dir/tree-after-$1.txt" || finish artifacts
  differ=$(awk 'NR == FNR { was[$0] = 1; next } !($0 in was) { n++ } END { print n + 0 }' \
    "$dir/tree-before.txt" "$dir/tree-after-$1.txt") || differ=unreadable
  printf 'tree changed=after-%s lines=%s\n' "$1" "$differ"
  finish tests
}

measure() {
  dir=$1
  want_all=$2
  mkdir -p "$dir" || finish artifacts
  tree_start
  closed=$(closed_list) || finish read
  meta=$(cargo metadata --format-version 1 --no-deps --locked 2>"$dir/metadata.err") || finish metadata
  tree_same metadata
  printf '%s\n' "$meta" >"$dir/metadata.json" || finish artifacts
  workspace=
  for crate in $crates; do
    case $meta in
      *"{\"name\":\"$crate\",\"version\":\""*) workspace="$workspace $crate" ;;
    esac
  done
  cargo_failed=no
  results=
  for crate in $crates; do
    present=no
    case " $workspace " in
      *" $crate "*) present=yes ;;
    esac
    out=
    if [ "$present" = yes ]; then
      out=$(cargo test --locked -p "$crate" 2>&1) || cargo_failed=yes
      tree_same test
    fi
    printf '%s\n' "$out" >"$dir/test-$crate.txt" || finish artifacts
    names=$(names_of "$crate") || finish read
    lines=$(printf '%s\n' "$out" | read_results "$crate" "$names" "$present") || finish read
    results="$results$lines$nl"
  done
  printf '%s' "$results" >"$dir/results.txt" || finish artifacts

  ok=0
  pending=0
  miss=0
  : >"$dir/pending.txt" || finish artifacts
  rest=$results
  while [ -n "$rest" ]; do
    one=${rest%%"$nl"*}
    rest=${rest#*"$nl"}
    state=${one##* }
    case $state in
      ok) ok=$((ok + 1)) ;;
      pending)
        pending=$((pending + 1))
        test=${one#* }
        printf '%s\n' "${test% *}" >>"$dir/pending.txt" || finish artifacts
        ;;
      *) miss=$((miss + 1)) ;;
    esac
  done
  total=$((ok + pending + miss))
  [ "$total" -eq 30 ] || finish read
  printf 'rule tests=%s ok=%s pending=%s miss=%s\n' "$total" "$ok" "$pending" "$miss"
  [ "$miss" -eq 0 ] || finish miss
  [ "$cargo_failed" = no ] || finish cargo
  if [ "$want_all" = yes ] && [ "$pending" -gt 0 ]; then
    finish pending
  fi
  printf 'PASS %s\n' "$name"
}

stub_metadata() {
  printf '{"packages":['
  sep=
  for member in $fx_members; do
    printf '%s{"name":"%s","version":"0.0.0","id":"path+file:///w/crates/%s#0.0.0","dependencies":[{"name":"halvo-learn-gate","source":null,"req":"*"}]}' "$sep" "$member" "$member"
    sep=,
  done
  printf '],"workspace_members":[]}\n'
}

stub_test() {
  names=$(names_of "$1") || return 98
  eol=
  [ "$fx_crlf" = yes ] && eol=$cr
  printf '   Compiling %s v0.0.0%s\n' "$1" "$eol"
  [ "$fx_build" = ok ] || return 101
  printf 'running 9 tests%s\n' "$eol"
  printf 'test src/lib.rs - edit_step (line 5) ... ok%s\n' "$eol"
  code=0
  if [ "$fx_other" = ok ]; then
    printf 'test %s_other ... ok%s\n' "$1" "$eol"
  else
    printf 'test %s_other ... FAILED%s\n' "$1" "$eol"
    code=101
  fi
  for test in $names; do
    test=${test%/*}
    case " $fx_missing " in
      *" $test "*) continue ;;
    esac
    result=ok
    case " $fx_failed " in
      *" $test "*)
        result=FAILED
        code=101
        ;;
    esac
    case " $fx_ignored " in
      *" $test "*) result=ignored ;;
    esac
    case " $fx_renamed " in
      *" $test "*) test=${test}_rows ;;
    esac
    case " $fx_twice " in
      *" $test "*)
        printf 'test %s ... ok%s\n' "$test" "$eol"
        result=FAILED
        code=101
        ;;
    esac
    printf 'test %s%s ... %s%s\n' "$fx_prefix" "$test" "$result" "$eol"
  done
  return "$code"
}

stub_change() {
  [ "$1" = "$fx_change_at" ] || return 0
  case $fx_change_file in
    oid) printf '%s\n' "$fx_change_line" >"$work/oid" ;;
    metadata) printf '{}\n' >"$dir/metadata.json" ;;
    *) printf '%s\n' "$fx_change_line" >>"$work/$fx_change_file" ;;
  esac
}

stub_git() {
  case $* in
    '-c core.fsmonitor=false -c core.untrackedCache=false status --porcelain=v2 --branch --untracked-files=all')
      [ "$fx_git" = ok ] || return 128
      printf '# branch.oid %s\n' "$(cat "$work/oid")"
      cat "$work/status"
      ;;
    'ls-files -v')
      [ "$fx_git" = ok ] || return 128
      printf '%s\n' "$fx_own_line"
      cat "$work/flags"
      ;;
    *) return 97 ;;
  esac
}

stub_cargo() {
  case $* in
    'metadata --format-version 1 --no-deps --locked')
      stub_change metadata
      [ "$fx_metadata" = ok ] || return 101
      stub_metadata
      ;;
    'test --locked -p '*)
      stub_change "$4"
      case " $fx_members " in
        *" $4 "*) stub_test "$4" ;;
        *) return 97 ;;
      esac
      ;;
    *) return 97 ;;
  esac
}

fixture() {
  case_name=$1
  fx_members='halvo-learn halvo-schema halvo-learn-store'
  fx_metadata=ok
  fx_build=ok
  fx_crlf=no
  fx_prefix=
  fx_missing='formal_traces'
  fx_failed=
  fx_ignored=
  fx_renamed=
  fx_twice=
  fx_other=ok
  fx_git=ok
  fx_own_line="H $script_path"
  fx_change_at=
  fx_change_file=status
  fx_change_line=
  case_all=no
  printf '%s\n' 1111111111111111111111111111111111111111 >"$work/oid" || exit 2
  : >"$work/status" || exit 2
  : >"$work/flags" || exit 2
}

check() {
  want=$1
  text=$2
  out=$(
    cargo() {
      stub_cargo "$@"
    }
    git() {
      stub_git "$@"
    }
    measure "$self_art/$case_name" "$case_all"
  )
  code=$?
  last=${out##*"$nl"}
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")")
      got=${last#"FAIL $name ("}
      got=FAIL-${got%)}
      ;;
    *) got=exit-$code ;;
  esac
  case $out in
    *"$text"*) ;;
    *) got=$got-without-the-text ;;
  esac
  printf 'case %s/%s want=%s got=%s\n' "$name" "$case_name" "$want" "$got"
  if [ "$got" != "$want" ]; then
    wrong="$wrong $case_name"
  elif [ "$want" != PASS ]; then
    rejected=$((rejected + 1))
  fi
}

self_test() {
  self_art=$art/self-test
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  trap 'rm -rf "$work"' EXIT
  trap 'exit 130' INT TERM HUP
  rejected=0
  wrong=
  all_crates='halvo-learn halvo-schema halvo-learn-store halvo-learn-gate'

  fixture unit-landed
  check PASS 'rule tests=30 ok=25 pending=5 miss=0'

  fixture failed-test
  fx_failed=refusal_no_source
  check FAIL-miss 'rule tests=30 ok=24 pending=5 miss=1'

  fixture ignored-test
  fx_ignored=edit_table
  check FAIL-miss 'rule tests=30 ok=24 pending=5 miss=1'

  fixture one-line-ok-one-failed
  fx_twice=routine_sample
  check FAIL-miss 'rule tests=30 ok=24 pending=5 miss=1'

  fixture built-test-line-missing
  fx_missing='formal_traces refusal_cross_workflow_source'
  check FAIL-miss 'rule tests=30 ok=24 pending=5 miss=1'

  fixture pin-test-line-missing
  fx_missing='formal_traces the_edit_kind_is_kept_of_origin_own_and_holds_the_fields_of_its_row_in_a_closed_table'
  check FAIL-miss 'rule tests=30 ok=24 pending=5 miss=1'

  fixture built-crate-absent
  fx_members='halvo-learn halvo-schema'
  check FAIL-miss 'rule tests=30 ok=18 pending=5 miss=7'

  fixture pending-test-failed
  fx_members=$all_crates
  fx_missing=
  fx_failed=constants_match_targets
  check FAIL-miss 'rule tests=30 ok=29 pending=0 miss=1'

  fixture all-with-one-test-line-missing
  fx_members=$all_crates
  fx_missing=constants_match_targets
  case_all=yes
  check FAIL-pending 'rule tests=30 ok=29 pending=1 miss=0'

  fixture all-present
  fx_members=$all_crates
  fx_missing=
  case_all=yes
  check PASS 'rule tests=30 ok=30 pending=0 miss=0'

  fixture one-test-line-missing-without-all
  fx_members=$all_crates
  fx_missing=constants_match_targets
  check PASS 'rule tests=30 ok=29 pending=1 miss=0'

  fixture build-fails
  fx_build=failed
  check FAIL-miss 'rule tests=30 ok=0 pending=5 miss=25'

  fixture other-test-fails
  fx_other=failed
  check FAIL-cargo 'rule tests=30 ok=25 pending=5 miss=0'

  fixture metadata-fails
  fx_metadata=failed
  check FAIL-metadata "FAIL $name (metadata)"

  fixture module-path
  fx_prefix=tests::
  check PASS 'rule tests=30 ok=25 pending=5 miss=0'

  fixture longer-name
  fx_renamed=edit_table
  check FAIL-miss 'rule tests=30 ok=24 pending=5 miss=1'

  fixture longer-name-of-a-pending-test
  fx_members=$all_crates
  fx_missing=
  fx_renamed=constants_match_targets
  check PASS 'rule tests=30 ok=29 pending=1 miss=0'

  fixture longer-name-with-all
  fx_members=$all_crates
  fx_missing=
  fx_renamed=constants_match_targets
  case_all=yes
  check FAIL-pending 'rule tests=30 ok=29 pending=1 miss=0'

  fixture carriage-returns
  fx_crlf=yes
  check PASS 'rule tests=30 ok=25 pending=5 miss=0'

  fixture crate-absent
  fx_members=halvo-schema
  check FAIL-miss 'rule tests=30 ok=5 pending=5 miss=20'

  fixture changed-and-untracked-files-before-the-run
  printf '1 .M N... 100644 100644 100644 0000 0000 crates/halvo-learn/src/lib.rs\n? notes.txt\n' >"$work/status"
  check PASS 'rule tests=30 ok=25 pending=5 miss=0'

  fixture test-run-changes-a-tracked-file
  fx_change_at=halvo-schema
  fx_change_line='1 .M N... 100644 100644 100644 0000 0000 crates/halvo-schema/src/lib.rs'
  check FAIL-tests 'tree changed=after-test lines=1'

  fixture list-differs-in-the-work-tree
  printf '1 .M N... 100644 100644 100644 0000 0000 %s\n' "$script_path" >"$work/status"
  check FAIL-list 'list same=no'

  fixture list-assumed-unchanged
  fx_own_line="h $script_path"
  check FAIL-list 'list same=no'

  fixture tracked-file-assumed-unchanged
  printf 'h crates/halvo-learn/src/lib.rs\n' >"$work/flags"
  check FAIL-tests 'tree flagged=1'

  fixture tracked-file-skipped
  printf 'S crates/halvo-learn/src/lib.rs\n' >"$work/flags"
  check FAIL-tests 'tree flagged=1'

  fixture skip-worktree-set-by-a-test
  fx_change_at=halvo-learn
  fx_change_file=flags
  fx_change_line='S crates/halvo-learn/src/lib.rs'
  check FAIL-tests 'tree changed=after-test lines=1'

  fixture head-moved-by-a-test
  fx_change_at=halvo-learn-store
  fx_change_file=oid
  fx_change_line=2222222222222222222222222222222222222222
  check FAIL-tests 'tree changed=after-test lines=1'

  fixture untracked-file-written-by-a-test
  fx_change_at=halvo-learn
  fx_change_line='? .cargo/config.toml'
  check FAIL-tests 'tree changed=after-test lines=1'

  fixture metadata-run-changes-the-tree
  fx_change_at=metadata
  fx_change_line='? .cargo/config.toml'
  check FAIL-tests 'tree changed=after-metadata lines=1'

  fixture metadata-file-rewritten-by-a-test
  fx_change_at=halvo-learn
  fx_change_file=metadata
  check PASS 'rule tests=30 ok=25 pending=5 miss=0'

  fixture git-unreadable
  fx_git=no
  check FAIL-tests 'tree head=unreadable'

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

repo=${CI_D_REPO:-}
if [ -z "$repo" ]; then
  here=$(dirname "$0") || exit 2
  repo=$(cd "$here/../../.." && pwd) || exit 2
fi
cd "$repo" || exit 2
art=${CI_D_ARTIFACTS:-$repo/target/ci-artifacts/$name}
case ${1:-} in
  --self-test)
    [ $# -eq 1 ] || exit 64
    self_test
    ;;
  --all)
    [ $# -eq 1 ] || exit 64
    measure "$art" yes
    ;;
  '')
    [ $# -le 1 ] || exit 64
    measure "$art" no
    ;;
  *) exit 64 ;;
esac
