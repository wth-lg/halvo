#!/bin/sh
# jobs: macos windows
# rule: The 19 refusal cases of T7.4, T8.4 and T9.4 and the tests formal_traces, no_grant_in_release and sequences_same pass, route-gate spend-sim for the seed 1 prints its seven lines with no overrun, no reserve above the cap and every admission within the room that it read, and its export equals docs/r6/gateway/sequences.jsonl of the commit, which the check reads before its first cargo command; a cargo command that leaves the tree changed fails the check.
set -u
name=50-r6-spend
case $0 in
  /*) self=$0 ;;
  *) self=$(pwd)/$0 ;;
esac
sequences=docs/r6/gateway/sequences.jsonl
refusal_cases='no_hold over_room second_dispatch ruling_none no_cap not_active zero_amount final_state admit_ruling admit_no_cap admit_over_room admit_sum admit_overflow leave_ruling leave_local leave_ceiling leave_part leave_origin leave_consent'
required_tests='formal_traces no_grant_in_release sequences_same'
sim_cases='near_cap fan_out cap_lowered stream_cut error_burst second_device'
want_last='spend cases=6 overruns=0 above_cap=0 admit_sum=ok result=pass'

finish() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

enter_repo() {
  if [ -n "${CI_D_REPO:-}" ]; then
    repo=$CI_D_REPO
  else
    repo=$(git rev-parse --show-toplevel 2>/dev/null) || repo=
  fi
  if [ -z "$repo" ] || ! cd "$repo" 2>/dev/null; then
    finish setup
  fi
}

tree_state() {
  git -c core.fsmonitor=false -c core.untrackedCache=false status --porcelain=v2 --untracked-files=all --ignored=matching &&
    git ls-files -v
}

tree_same() {
  tree_now=$(tree_state 2>&1) || tree_now="unreadable $tree_now"
  [ "$tree_now" = "$tree_before" ] && return 0
  printf 'tree changed=%s\n' "$1" >>"$artifacts/tree.txt"
  finish "$2"
}

tree_clean() {
  R6_TREE=$tree_before awk '
    BEGIN {
      n = split(ENVIRON["R6_TREE"], line, "\n")
      for (i = 1; i <= n; i++) {
        if (line[i] ~ /^(1|2|u) /) bad = 1
        if (line[i] ~ /^[A-Za-z] / && line[i] !~ /^H /) bad = 1
      }
      exit bad
    }'
}

count_refusals() {
  awk -v cases="$refusal_cases" '
    { sub(/\r$/, "") }
    /^test [^ ]+ \.\.\. ok$/ {
      path = substr($0, 6, length($0) - 12)
      n = split(path, part, "::")
      ok[part[n]] = 1
    }
    END {
      count = split(cases, list, " ")
      for (i = 1; i <= count; i++) {
        if (("refusal_" list[i]) in ok) found++
        else print "refusal missing=refusal_" list[i] >"/dev/stderr"
      }
      print found + 0
    }' "$1"
}

count_required() {
  awk -v names="$required_tests" '
    { sub(/\r$/, "") }
    /^ *Doc-tests / { target = "" }
    /^ *Running / {
      target = $0
      sub(/^ *Running /, "", target)
      sub(/ \(.*$/, "", target)
      gsub(/\\/, "/", target)
    }
    /^test [^ ]+ \.\.\. ok$/ {
      path = substr($0, 6, length($0) - 12)
      if (target == "tests/" path ".rs") ok[path] = 1
    }
    END {
      count = split(names, list, " ")
      for (i = 1; i <= count; i++) {
        if (list[i] in ok) found++
        else print "required missing=" list[i] >"/dev/stderr"
      }
      print found + 0
    }' "$1"
}

sim_holds() {
  awk -v cases="$sim_cases" -v want="$want_last" '
    { sub(/\r$/, ""); line[NR] = $0 }
    END {
      count = split(cases, name, " ")
      if (NR != count + 1) exit 1
      k = split("tasks reserves admitted refused_over_cap expired overruns above_cap stopped_mid_run", key, " ")
      for (i = 1; i <= count; i++) {
        if (split(line[i], word, " ") != k + 2) exit 1
        if (word[1] != "spend" || word[2] != "case=" name[i]) exit 1
        for (j = 1; j <= k; j++) if (word[j + 2] !~ ("^" key[j] "=(0|[1-9][0-9]*)$")) exit 1
        if (word[8] != "overruns=0" || word[9] != "above_cap=0") exit 1
      }
      exit (line[NR] != want)
    }' "$1"
}

measure() {
  enter_repo
  artifacts=${CI_D_ARTIFACTS:-target/ci-artifacts/$name}
  if ! mkdir -p "$artifacts" 2>/dev/null || ! scratch=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX" 2>/dev/null); then
    finish setup
  fi
  CARGO_TERM_QUIET=false
  CARGO_TERM_VERBOSE=false
  CARGO_TERM_COLOR=never
  export CARGO_TERM_QUIET CARGO_TERM_VERBOSE CARGO_TERM_COLOR
  : >"$artifacts/tree.txt"
  cause=

  tree_before=$(tree_state 2>&1) || tree_before=
  if [ -z "$tree_before" ] || ! tree_clean; then
    printf 'tree changed=before\n' >>"$artifacts/tree.txt"
    finish tests
  fi
  committed=$scratch/committed.jsonl
  if git -c core.autocrlf=false cat-file blob "HEAD:$sequences" >"$committed" 2>"$artifacts/committed.err"; then
    have=yes
  else
    have=no
  fi

  cargo test --locked -p halvo-route -p halvo-route-gate >"$artifacts/cargo-test.txt" 2>&1 </dev/null
  test_code=$?
  tree_same after-tests tests
  refusals=$(count_refusals "$artifacts/cargo-test.txt" 2>"$artifacts/refusals.txt") || refusals=0
  required=$(count_required "$artifacts/cargo-test.txt" 2>"$artifacts/required.txt") || required=0
  printf 'refusals expected=19 ok=%s\n' "$refusals"
  printf 'tests required=3 ok=%s\n' "$required"
  if [ "$test_code" -ne 0 ] || [ "$required" != 3 ]; then
    cause=tests
  elif [ "$refusals" != 19 ]; then
    cause=refusals
  fi

  fresh=$scratch/sequences.jsonl
  cargo run -q --locked -p halvo-route-gate --bin route-gate -- spend-sim --seed 1 --export "$fresh" \
    >"$artifacts/spend-sim.out" 2>"$artifacts/spend-sim.err" </dev/null
  sim_code=$?
  tree_same after-sim sim
  grep '^spend ' "$artifacts/spend-sim.out"
  if [ "$sim_code" -ne 0 ] || ! sim_holds "$artifacts/spend-sim.out"; then
    cause=${cause:-sim}
  fi

  lines=0
  same=no
  if [ "$have" = yes ]; then
    lines=$(grep -c '' "$committed") || lines=0
    if [ -f "$fresh" ] && cmp -s "$fresh" "$committed"; then
      same=yes
    fi
  fi
  printf 'sequences lines=%s same=%s\n' "$lines" "$same"
  [ "$same" = yes ] || cause=${cause:-sequences}

  [ -z "$cause" ] || finish "$cause"
  printf 'PASS %s\n' "$name"
}

note() {
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$3"
  if [ "$3" != "$2" ]; then
    wrong="$wrong $1"
  elif [ "$2" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

reset_stubs() {
  STUB_TEST_OUT=$work/test-good.txt
  STUB_TEST_EXIT=0
  STUB_SIM_OUT=$work/sim-good.txt
  STUB_SIM_EXIT=0
  STUB_FRESH=$work/committed.jsonl
  STUB_COMMITTED=$work/committed.jsonl
  STUB_CHANGE=
  STUB_REPO=$work/repo
  cp "$work/clean-status.txt" "$work/tree/status" || exit 2
  cp "$work/clean-flags.txt" "$work/tree/flags" || exit 2
}

check() {
  out=$(
    PATH=$work/bin:$PATH
    CI_D_REPO=$STUB_REPO
    CI_D_ARTIFACTS=$work/artifacts/$1
    STUB_TREE=$work/tree
    export PATH CI_D_REPO CI_D_ARTIFACTS STUB_TREE STUB_TEST_OUT STUB_TEST_EXIT STUB_SIM_OUT STUB_SIM_EXIT
    export STUB_FRESH STUB_COMMITTED STUB_CHANGE
    sh "$self" 2>&1
  )
  code=$?
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  case_name=$1
  want=$2
  shift 2
  for text in "$@"; do
    case $out in
      *"$text"*) ;;
      *) got=$got-without-the-text ;;
    esac
  done
  note "$case_name" "$want" "$got"
  reset_stubs
}

write_stubs() {
  cat >"$work/bin/cargo" <<'EOF'
#!/bin/sh
case $* in
  'test --locked -p halvo-route -p halvo-route-gate')
    cat "$STUB_TEST_OUT"
    [ "$STUB_CHANGE" != tests ] || printf '1 .M N... 100644 100644 100644 0 0 docs/r6/gateway/sequences.jsonl\n' >>"$STUB_TREE/status"
    exit "$STUB_TEST_EXIT"
    ;;
  'run -q --locked -p halvo-route-gate --bin route-gate -- spend-sim --seed 1 --export '*)
    shift 12
    cat "$STUB_SIM_OUT"
    [ ! -f "$STUB_FRESH" ] || cp "$STUB_FRESH" "$1"
    [ "$STUB_CHANGE" != sim ] || printf '? docs/r6/new.txt\n' >>"$STUB_TREE/status"
    exit "$STUB_SIM_EXIT"
    ;;
esac
printf 'stub cargo: got the arguments: %s\n' "$*"
exit 97
EOF
  cat >"$work/bin/git" <<'EOF'
#!/bin/sh
case $* in
  '-c core.fsmonitor=false -c core.untrackedCache=false status --porcelain=v2 --untracked-files=all --ignored=matching')
    cat "$STUB_TREE/status"
    ;;
  'ls-files -v')
    cat "$STUB_TREE/flags"
    ;;
  '-c core.autocrlf=false cat-file blob HEAD:docs/r6/gateway/sequences.jsonl')
    [ -f "$STUB_COMMITTED" ] || exit 128
    cat "$STUB_COMMITTED"
    ;;
  *)
    printf 'stub git: got the arguments: %s\n' "$*"
    exit 97
    ;;
esac
EOF
  chmod +x "$work/bin/cargo" "$work/bin/git"
}

write_fixtures() {
  {
    printf '     Running unittests src/lib.rs (target/debug/deps/halvo_route-0123456789abcdef)\n'
    for case_word in $refusal_cases; do
      case $case_word in
        admit_* | leave_*) printf 'test %s::tests::refusal_%s ... ok\n' "${case_word%%_*}" "$case_word" ;;
      esac
    done
    printf 'test admit::tests::not_refusal_no_cap ... ok\n'
    printf '     Running tests/formal_traces.rs (target/debug/deps/formal_traces-0123456789abcdef)\n'
    printf 'test formal_traces ... ok\n'
    printf '     Running tests/no_grant_in_release.rs (target/debug/deps/no_grant_in_release-0123456789abcdef)\n'
    printf 'test no_grant_in_release ... ok\n'
    printf '     Running tests/refusal_spend.rs (target/debug/deps/refusal_spend-0123456789abcdef)\n'
    for case_word in $refusal_cases; do
      case $case_word in
        admit_* | leave_*) ;;
        *) printf 'test refusal_%s ... ok\n' "$case_word" ;;
      esac
    done
    printf '     Running tests/sequences_same.rs (target/debug/deps/sequences_same-0123456789abcdef)\n'
    printf 'test sequences_same ... ok\n'
    printf '   Doc-tests halvo_route\n'
    printf 'test result: ok. 0 passed; 0 failed\n'
  } >"$work/test-good.txt" || exit 2
  {
    for case_word in $sim_cases; do
      printf 'spend case=%s tasks=2 reserves=6 admitted=0 refused_over_cap=1 expired=0 overruns=0 above_cap=0 stopped_mid_run=0\n' "$case_word"
    done
    printf '%s\n' "$want_last"
  } >"$work/sim-good.txt" || exit 2
  printf '# branch.oid 0123456789abcdef0123456789abcdef01234567\n! target/\n' >"$work/clean-status.txt" || exit 2
  printf 'H %s\nH scripts/r6/ci.d/%s.sh\n' "$sequences" "$name" >"$work/clean-flags.txt" || exit 2
}

variant() {
  sed "$1" "$2" >"$3" || exit 2
  cmp -s "$2" "$3" && exit 2
}

self_test() {
  enter_repo
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  mkdir -p "$work/bin" "$work/repo" "$work/artifacts" "$work/tree" || exit 2
  git -c core.autocrlf=false cat-file blob "HEAD:$sequences" >"$work/committed.jsonl" || exit 2
  [ -s "$work/committed.jsonl" ] || exit 2
  variant '1s/"answer":"ok"/"answer":"state"/' "$work/committed.jsonl" "$work/one-answer.jsonl"
  committed_lines=$(grep -c '' "$work/committed.jsonl") || exit 2
  write_stubs
  write_fixtures
  rejected=0
  wrong=
  reset_stubs

  check all-hold PASS 'refusals expected=19 ok=19
tests required=3 ok=3
spend case=near_cap ' "$want_last
sequences lines=$committed_lines same=yes
PASS $name"
  awk '{ printf "%s\r\n", $0 }' "$work/test-good.txt" >"$work/test-crlf.txt" || exit 2
  awk '{ printf "%s\r\n", $0 }' "$work/sim-good.txt" >"$work/sim-crlf.txt" || exit 2
  STUB_TEST_OUT=$work/test-crlf.txt
  STUB_SIM_OUT=$work/sim-crlf.txt
  check carriage-returns PASS 'refusals expected=19 ok=19
tests required=3 ok=3' "PASS $name"
  sed 's/^\(  *Running \)tests\/\([a-z_]*\)\.rs (target\/debug\//\1tests\\\2.rs (target\\debug\\/' "$work/test-good.txt" >"$work/test-backslash.txt" || exit 2
  STUB_TEST_OUT=$work/test-backslash.txt
  check windows-paths PASS 'tests required=3 ok=3' "PASS $name"

  STUB_TEST_EXIT=101
  check cargo-test-fails FAIL "refusals expected=19 ok=19" "FAIL $name (tests)"
  variant '/refusal_admit_sum /d' "$work/test-good.txt" "$work/test-no-sum.txt"
  STUB_TEST_OUT=$work/test-no-sum.txt
  check one-refusal-missing FAIL 'refusals expected=19 ok=18' "FAIL $name (refusals)"
  variant 's/^test refusal_no_cap \.\.\. ok$/test refusal_no_cap ... FAILED/' "$work/test-good.txt" "$work/test-failed.txt"
  STUB_TEST_OUT=$work/test-failed.txt
  check one-refusal-failed FAIL 'refusals expected=19 ok=18' "FAIL $name (refusals)"
  variant 's/^test refusal_no_hold \.\.\. ok$/test not_refusal_no_hold ... ok/' "$work/test-good.txt" "$work/test-prefix.txt"
  STUB_TEST_OUT=$work/test-prefix.txt
  check a-longer-name-is-not-the-case FAIL 'refusals expected=19 ok=18' "FAIL $name (refusals)"
  variant '/^test sequences_same /d' "$work/test-good.txt" "$work/test-no-sequences.txt"
  STUB_TEST_OUT=$work/test-no-sequences.txt
  check required-missing FAIL 'tests required=3 ok=2' "FAIL $name (tests)"
  awk '/^test formal_traces /{ next } { print } /^test refusal_no_hold /{ print "test formal_traces ... ok" }' \
    "$work/test-good.txt" >"$work/test-other-target.txt" || exit 2
  STUB_TEST_OUT=$work/test-other-target.txt
  check required-under-another-target FAIL 'tests required=3 ok=2' "FAIL $name (tests)"
  variant 's/^test no_grant_in_release \.\.\. ok$/test tests::no_grant_in_release ... ok/' "$work/test-good.txt" "$work/test-module.txt"
  STUB_TEST_OUT=$work/test-module.txt
  check required-in-a-module FAIL 'tests required=3 ok=2' "FAIL $name (tests)"

  variant 's/result=pass$/result=fail/' "$work/sim-good.txt" "$work/sim-fail.txt"
  STUB_SIM_OUT=$work/sim-fail.txt
  STUB_SIM_EXIT=1
  check sim-fails FAIL 'admit_sum=ok result=fail' "FAIL $name (sim)"
  STUB_SIM_EXIT=1
  check sim-exit-1-with-a-pass-line FAIL "$want_last" "FAIL $name (sim)"
  variant '2s/ overruns=0 / overruns=1 /' "$work/sim-good.txt" "$work/sim-overrun.txt"
  STUB_SIM_OUT=$work/sim-overrun.txt
  check a-case-overruns FAIL "FAIL $name (sim)"
  variant '3s/ above_cap=0 / above_cap=2 /' "$work/sim-good.txt" "$work/sim-above.txt"
  STUB_SIM_OUT=$work/sim-above.txt
  check a-case-above-the-cap FAIL "FAIL $name (sim)"
  variant '4d' "$work/sim-good.txt" "$work/sim-six.txt"
  STUB_SIM_OUT=$work/sim-six.txt
  check six-lines FAIL "FAIL $name (sim)"
  variant '1s/case=near_cap/case=fan_out/' "$work/sim-good.txt" "$work/sim-order.txt"
  STUB_SIM_OUT=$work/sim-order.txt
  check cases-out-of-order FAIL "FAIL $name (sim)"
  variant '5s/ expired=0 / expired=x /' "$work/sim-good.txt" "$work/sim-word.txt"
  STUB_SIM_OUT=$work/sim-word.txt
  check a-count-that-is-no-number FAIL "FAIL $name (sim)"
  variant '6s/ stopped_mid_run=0$/ stopped_mid_run=0 extra=1/' "$work/sim-good.txt" "$work/sim-extra.txt"
  STUB_SIM_OUT=$work/sim-extra.txt
  check a-line-with-another-key FAIL "FAIL $name (sim)"
  variant '7s/admit_sum=ok/admit_sum=miss/' "$work/sim-good.txt" "$work/sim-miss.txt"
  STUB_SIM_OUT=$work/sim-miss.txt
  check admit-sum-miss FAIL "FAIL $name (sim)"
  STUB_SIM_OUT=$work/test-good.txt
  STUB_SIM_EXIT=2
  STUB_FRESH=$work/none.jsonl
  check sim-refused FAIL "sequences lines=$committed_lines same=no" "FAIL $name (sim)"

  STUB_COMMITTED=$work/one-answer.jsonl
  check one-answer-changed FAIL "sequences lines=$committed_lines same=no" "FAIL $name (sequences)"
  STUB_FRESH=$work/one-answer.jsonl
  check fresh-export-differs FAIL "sequences lines=$committed_lines same=no" "FAIL $name (sequences)"
  STUB_COMMITTED=$work/none.jsonl
  check not-committed FAIL 'sequences lines=0 same=no' "FAIL $name (sequences)"
  STUB_FRESH=$work/none.jsonl
  check not-exported FAIL "sequences lines=$committed_lines same=no" "FAIL $name (sequences)"

  STUB_CHANGE=tests
  check tests-change-the-tree FAIL "FAIL $name (tests)"
  STUB_CHANGE=sim
  check sim-changes-the-tree FAIL "FAIL $name (sim)"
  printf '1 .M N... 100644 100644 100644 0 0 %s\n' "$sequences" >>"$work/tree/status"
  check changed-before FAIL "FAIL $name (tests)"
  printf 'h %s\n' "$sequences" >>"$work/tree/flags"
  check index-flag-before FAIL "FAIL $name (tests)"
  STUB_TEST_EXIT=101
  STUB_SIM_OUT=$work/sim-fail.txt
  STUB_SIM_EXIT=1
  STUB_COMMITTED=$work/one-answer.jsonl
  check causes-name-the-first FAIL "FAIL $name (tests)"
  STUB_TEST_OUT=$work/test-no-sum.txt
  STUB_COMMITTED=$work/one-answer.jsonl
  check refusals-before-sequences FAIL "FAIL $name (refusals)"
  STUB_REPO=$work/no-such-directory
  check no-repository FAIL "FAIL $name (setup)"

  usage=$(sh "$self" --all 2>&1)
  code=$?
  if [ "$code:$usage" = "1:FAIL $name (usage)" ]; then
    note unknown-argument FAIL FAIL
  else
    note unknown-argument FAIL "exit-$code"
  fi

  cargo run -q --locked -p halvo-route-gate --bin route-gate -- spend-sim --seed 1 --export "$work/real.jsonl" \
    >"$work/real.out" 2>"$work/real.err" </dev/null
  if cmp -s "$work/real.jsonl" "$work/committed.jsonl"; then
    note real-export-of-the-commit PASS PASS
  else
    note real-export-of-the-commit PASS FAIL
  fi
  if cmp -s "$work/real.jsonl" "$work/one-answer.jsonl"; then
    note real-export-against-one-answer-changed FAIL PASS
  else
    note real-export-against-one-answer-changed FAIL FAIL
  fi

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  '') measure ;;
  *) finish usage ;;
esac
