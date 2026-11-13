#!/bin/sh
# jobs: macos windows
# rule: The tests of halvo-route and halvo-route-gate pass, and each of the 19 refusal cases of T7.4, T8.4 and T9.4 and of the tests formal_traces, no_grant_in_release and sequences_same counts only under its own crate, target and whole test path, when that target's list holds it, its list of ignored tests does not, and a run of the bound tests alone exits 0 and prints its ok line under its target with that target's summary; route-gate spend-sim for the seed 1 prints its seven lines with no overrun, no reserve above the cap and every admission within the room that it read, and its export has the SHA-256 of docs/r6/gateway/sequences.jsonl of the commit, which the check reads with the file's line count before its first cargo command; a cargo command that leaves the tree changed fails the check; it does not hold code that runs before main in a bound test file or in a target with its harness off, nor the job's environment, such as a runner for test binaries or a cargo earlier on the PATH. It holds that each bound test is listed, is not ignored, ran and passed under its crate, its target and its whole path, and it does not hold what the body of that test asserts, so a bound test that keeps its name with another body counts.
set -u
name=50-r6-spend
case $0 in
  /*) self=$0 ;;
  *) self=$(pwd)/$0 ;;
esac
sequences=docs/r6/gateway/sequences.jsonl
bound_tests='halvo-route tests/refusal_spend.rs refusal_no_hold case
halvo-route tests/refusal_spend.rs refusal_over_room case
halvo-route tests/refusal_spend.rs refusal_second_dispatch case
halvo-route tests/refusal_spend.rs refusal_ruling_none case
halvo-route tests/refusal_spend.rs refusal_no_cap case
halvo-route tests/refusal_spend.rs refusal_not_active case
halvo-route tests/refusal_spend.rs refusal_zero_amount case
halvo-route tests/refusal_spend.rs refusal_final_state case
halvo-route src/lib.rs admit::tests::refusal_admit_ruling case
halvo-route src/lib.rs admit::tests::refusal_admit_no_cap case
halvo-route src/lib.rs admit::tests::refusal_admit_over_room case
halvo-route src/lib.rs admit::tests::refusal_admit_sum case
halvo-route src/lib.rs admit::tests::refusal_admit_overflow case
halvo-route src/lib.rs leave::tests::refusal_leave_ruling case
halvo-route src/lib.rs leave::tests::refusal_leave_local case
halvo-route src/lib.rs leave::tests::refusal_leave_ceiling case
halvo-route src/lib.rs leave::tests::refusal_leave_part case
halvo-route src/lib.rs leave::tests::refusal_leave_origin case
halvo-route src/lib.rs leave::tests::refusal_leave_consent case
halvo-route tests/formal_traces.rs formal_traces test
halvo-route tests/no_grant_in_release.rs no_grant_in_release test
halvo-route-gate tests/sequences_same.rs sequences_same test'
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
  git -c core.fsmonitor=false -c core.untrackedCache=false status --porcelain=v2 --branch --untracked-files=all --ignored=matching &&
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

bound_of() {
  R6_BOUND=$bound_tests awk -v package="$1" '
    BEGIN {
      n = split(ENVIRON["R6_BOUND"], line, "\n")
      for (i = 1; i <= n; i++) {
        split(line[i], field, " ")
        if (field[1] == package) print line[i]
      }
    }'
}

read_bound() {
  awk -v crate="$1" -v marker="$2" -v list_code="$list_code" -v ignored_code="$ignored_code" -v run_code="$run_code" '
    function target_of(line,    paren, bin) {
      sub(/^ *Running /, "", line)
      gsub(/\\/, "/", line)
      paren = index(line, " (")
      if (paren == 0 || line !~ /\)$/) return ""
      bin = substr(line, paren + 2, length(line) - paren - 2)
      sub(/.*\//, "", bin)
      sub(/\.exe$/, "", bin)
      sub(/-[0-9a-f]+$/, "", bin)
      return bin " " substr(line, 1, paren - 1)
    }
    { sub(/\r$/, "") }
    $0 == marker {
      section++
      target = ""
      next
    }
    section < 3 {
      if ($0 ~ /^ *Doc-tests /) {
        target = ""
      } else if ($0 ~ /^ *Running /) {
        target = target_of($0)
      } else {
        if (section == 0 && $0 ~ /: test$/) listed[target " " substr($0, 1, length($0) - 6)] = 1
        if (section == 1 && $0 ~ /: test$/) ignored[target " " substr($0, 1, length($0) - 6)] = 1
        if (section == 2 && $0 ~ /^test [^ ]+ \.\.\. ok$/) passed[target " " substr($0, 6, length($0) - 12)] = 1
        if (section == 2 && $0 ~ /^test result: ok\. /) summary[target] = 1
      }
      next
    }
    {
      if ($2 == "src/lib.rs") {
        bin = crate
        gsub(/-/, "_", bin)
        key = bin " unittests src/lib.rs"
      } else {
        bin = $2
        sub(/^tests\//, "", bin)
        sub(/\.rs$/, "", bin)
        key = bin " " $2
      }
      miss = ""
      if (list_code != 0 || !((key " " $3) in listed)) miss = miss ",listed"
      if (ignored_code != 0 || ((key " " $3) in ignored)) miss = miss ",ignored"
      if (run_code != 0 || !(key in summary) || !((key " " $3) in passed)) miss = miss ",ran"
      if (miss == "") print $4 " ok " $1 " " $2 " " $3
      else print $4 " miss=" substr(miss, 2) " " $1 " " $2 " " $3
    }
  '
}

run_bound() {
  package=$1
  wanted=$(bound_of "$package")
  sources=$(printf '%s\n' "$wanted" | awk '!seen[$2]++ { print $2 }')
  set --
  while read -r source; do
    case $source in
      src/lib.rs) set -- "$@" --lib ;;
      *)
        stem=${source#tests/}
        set -- "$@" --test "${stem%.rs}"
        ;;
    esac
  done <<EOF
$sources
EOF
  list_out=$(cargo test --locked -p "$package" "$@" -- --list 2>&1 </dev/null)
  list_code=$?
  tree_same "after-list-$package" tests
  ignored_out=$(cargo test --locked -p "$package" "$@" -- --list --ignored 2>&1 </dev/null)
  ignored_code=$?
  tree_same "after-ignored-$package" tests
  set -- "$@" -- --exact
  while read -r _ _ test_name _; do
    set -- "$@" "$test_name"
  done <<EOF
$wanted
EOF
  exact_out=$(cargo test --locked -p "$package" "$@" 2>&1 </dev/null)
  run_code=$?
  tree_same "after-exact-$package" tests
  marker='== r6 bound reader =='
  while :; do
    case "$list_out $ignored_out $exact_out" in
      *"$marker"*) marker="$marker=" ;;
      *) break ;;
    esac
  done
  found=$(printf '%s\n%s\n%s\n%s\n%s\n%s\n%s\n' "$list_out" "$marker" "$ignored_out" "$marker" "$exact_out" "$marker" "$wanted" |
    read_bound "$package" "$marker")
  printf '%s\n' "$list_out" >"$artifacts/list-$package.txt"
  printf '%s\n' "$ignored_out" >"$artifacts/ignored-$package.txt"
  printf '%s\n' "$exact_out" >"$artifacts/exact-$package.txt"
  bound_lines="$bound_lines$found
"
}

sum_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum | awk '{ print $1 }'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 | awk '{ print $1 }'
  else
    return 1
  fi
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
  : >"$artifacts/bound.txt"
  bound_lines=
  cause=

  tree_before=$(tree_state 2>&1) || tree_before=
  if [ -z "$tree_before" ] || ! tree_clean; then
    printf 'tree changed=before\n' >>"$artifacts/tree.txt"
    finish tests
  fi
  committed_sum=
  committed_lines=0
  if git -c core.autocrlf=false cat-file -e "HEAD:$sequences" 2>"$artifacts/committed.err"; then
    committed_sum=$(git -c core.autocrlf=false cat-file blob "HEAD:$sequences" | sum_of) || committed_sum=
    committed_lines=$(git -c core.autocrlf=false cat-file blob "HEAD:$sequences" | grep -c '') || committed_lines=0
  fi

  cargo test --locked -p halvo-route -p halvo-route-gate >"$artifacts/cargo-test.txt" 2>&1 </dev/null
  test_code=$?
  tree_same after-tests tests
  for package in halvo-route halvo-route-gate; do
    run_bound "$package"
  done
  refusals=$(printf '%s' "$bound_lines" | grep -c '^case ok ') || refusals=0
  required=$(printf '%s' "$bound_lines" | grep -c '^test ok ') || required=0
  printf '%s' "$bound_lines" >"$artifacts/bound.txt"
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

  same=no
  if [ -n "$committed_sum" ] && [ -f "$fresh" ]; then
    fresh_sum=$(sum_of <"$fresh") || fresh_sum=
    [ "$fresh_sum" != "$committed_sum" ] || same=yes
  fi
  printf 'sequences lines=%s same=%s\n' "$committed_lines" "$same"
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
  STUB_DIR=$work/stub
  STUB_TEST_OUT=$work/test-good.txt
  STUB_TEST_EXIT=0
  STUB_LIST_EXIT=0
  STUB_IGNORED_EXIT=0
  STUB_EXACT_EXIT=0
  STUB_SIM_OUT=$work/sim-good.txt
  STUB_SIM_EXIT=0
  STUB_FRESH=$work/committed.jsonl
  STUB_COMMITTED=$work/committed.jsonl
  STUB_CHANGE=
  STUB_CHANGED=$work/one-answer.jsonl
  STUB_TERM=
  STUB_REPO=$work/repo
  cp "$work/clean-status.txt" "$work/tree/status" || exit 2
  cp "$work/clean-flags.txt" "$work/tree/flags" || exit 2
}

stub_case() {
  STUB_DIR=$work/stub-$1
  mkdir "$STUB_DIR" || exit 2
  cp "$work/stub/"* "$STUB_DIR/" || exit 2
}

check() {
  out=$(
    PATH=$work/bin:$PATH
    CI_D_REPO=$STUB_REPO
    CI_D_ARTIFACTS=$work/artifacts/$1
    STUB_TREE=$work/tree
    export PATH CI_D_REPO CI_D_ARTIFACTS STUB_TREE STUB_DIR STUB_TEST_OUT STUB_TEST_EXIT STUB_SIM_OUT STUB_SIM_EXIT
    export STUB_LIST_EXIT STUB_IGNORED_EXIT STUB_EXACT_EXIT STUB_FRESH STUB_COMMITTED STUB_CHANGE STUB_CHANGED
    if [ -n "$STUB_TERM" ]; then
      CARGO_TERM_QUIET=true
      CARGO_TERM_VERBOSE=true
      CARGO_TERM_COLOR=always
      export CARGO_TERM_QUIET CARGO_TERM_VERBOSE CARGO_TERM_COLOR
    fi
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
route='test --locked -p halvo-route --test refusal_spend --lib --test formal_traces --test no_grant_in_release'
route_names='refusal_no_hold refusal_over_room refusal_second_dispatch refusal_ruling_none refusal_no_cap refusal_not_active refusal_zero_amount refusal_final_state admit::tests::refusal_admit_ruling admit::tests::refusal_admit_no_cap admit::tests::refusal_admit_over_room admit::tests::refusal_admit_sum admit::tests::refusal_admit_overflow leave::tests::refusal_leave_ruling leave::tests::refusal_leave_local leave::tests::refusal_leave_ceiling leave::tests::refusal_leave_part leave::tests::refusal_leave_origin leave::tests::refusal_leave_consent formal_traces no_grant_in_release'
gate='test --locked -p halvo-route-gate --test sequences_same'
if [ "${CARGO_TERM_QUIET-}" != false ] || [ "${CARGO_TERM_VERBOSE-}" != false ] || [ "${CARGO_TERM_COLOR-}" != never ]; then
  printf 'stub cargo: quiet=%s verbose=%s color=%s\n' "${CARGO_TERM_QUIET-}" "${CARGO_TERM_VERBOSE-}" "${CARGO_TERM_COLOR-}"
  exit 96
fi
case $* in
  'test --locked -p halvo-route -p halvo-route-gate')
    cat "$STUB_TEST_OUT"
    case $STUB_CHANGE in
      tests) printf '1 .M N... 100644 100644 100644 0 0 docs/r6/gateway/sequences.jsonl\n' >>"$STUB_TREE/status" ;;
      flags) printf 'h docs/r6/gateway/sequences.jsonl\n' >>"$STUB_TREE/flags" ;;
      head) printf '# branch.oid fedcba9876543210fedcba9876543210fedcba98\n! target/\n' >"$STUB_TREE/status" ;;
      blob) cp "$STUB_CHANGED" "$STUB_COMMITTED" ;;
      bound) printf 'case ok halvo-route tests/refusal_spend.rs refusal_no_hold\ntest ok halvo-route-gate tests/sequences_same.rs sequences_same\n' >>"$CI_D_ARTIFACTS/bound.txt" ;;
      copy)
        for copy in "${TMPDIR:-/tmp}"/50-r6-spend.*/*; do
          [ ! -f "$copy" ] || cp "$STUB_CHANGED" "$copy"
        done
        ;;
    esac
    exit "$STUB_TEST_EXIT"
    ;;
  "$route -- --list")
    cat "$STUB_DIR/list-halvo-route.txt"
    [ "$STUB_CHANGE" != list ] || printf '? crates/halvo-route/tests/new.rs\n' >>"$STUB_TREE/status"
    exit "$STUB_LIST_EXIT"
    ;;
  "$route -- --list --ignored")
    cat "$STUB_DIR/ignored-halvo-route.txt"
    exit "$STUB_IGNORED_EXIT"
    ;;
  "$route -- --exact $route_names")
    cat "$STUB_DIR/exact-halvo-route.txt"
    [ "$STUB_CHANGE" != exact ] || printf '1 .M N... 100644 100644 100644 0 0 scripts/r6/required-tests.txt\n' >>"$STUB_TREE/status"
    [ "$STUB_CHANGE" != ignored-file ] ||
      printf '     Running tests/refusal_spend.rs (target/debug/deps/refusal_spend-0123456789abcdef)\n' >"$CI_D_ARTIFACTS/ignored-halvo-route.txt"
    exit "$STUB_EXACT_EXIT"
    ;;
  "$gate -- --list")
    cat "$STUB_DIR/list-halvo-route-gate.txt"
    exit 0
    ;;
  "$gate -- --list --ignored")
    cat "$STUB_DIR/ignored-halvo-route-gate.txt"
    exit 0
    ;;
  "$gate -- --exact sequences_same")
    cat "$STUB_DIR/exact-halvo-route-gate.txt"
    exit 0
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
  '-c core.fsmonitor=false -c core.untrackedCache=false status --porcelain=v2 --branch --untracked-files=all --ignored=matching')
    cat "$STUB_TREE/status"
    ;;
  'ls-files -v')
    cat "$STUB_TREE/flags"
    ;;
  '-c core.autocrlf=false cat-file -e HEAD:docs/r6/gateway/sequences.jsonl')
    [ -f "$STUB_COMMITTED" ] || exit 128
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
  for package in halvo-route halvo-route-gate; do
    bound_of "$package" | awk -v package="$package" -v dir="$work/stub" '
      {
        if ($2 == "src/lib.rs") {
          bin = package
          gsub(/-/, "_", bin)
          printed = "unittests src/lib.rs"
        } else {
          bin = $2
          sub(/^tests\//, "", bin)
          sub(/\.rs$/, "", bin)
          printed = $2
        }
        if (!(printed in order)) {
          order[printed] = ++n
          head[n] = "     Running " printed " (target/debug/deps/" bin "-0123456789abcdef)"
        }
        k = order[printed]
        names[k] = names[k] $3 ": test\n"
        oks[k] = oks[k] "test " $3 " ... ok\n"
        count[k]++
      }
      END {
        for (k = 1; k <= n; k++) {
          printf "%s\n%s\n%d tests, 0 benchmarks\n", head[k], names[k], count[k] >(dir "/list-" package ".txt")
          printf "%s\n\n0 tests, 0 benchmarks\n", head[k] >(dir "/ignored-" package ".txt")
          printf "%s\n\nrunning %d tests\n%s\ntest result: ok. %d passed; 0 failed\n\n", head[k], count[k], oks[k], count[k] >(dir "/exact-" package ".txt")
        }
      }' || exit 2
  done
  cat "$work/stub/exact-halvo-route.txt" "$work/stub/exact-halvo-route-gate.txt" >"$work/test-good.txt" || exit 2
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

bound_cases() {
  stub_case carriage-returns
  for file in "$work/stub/"*; do
    awk '{ printf "%s\r\n", $0 }' "$file" >"$STUB_DIR/${file##*/}" || exit 2
  done
  check carriage-returns PASS 'refusals expected=19 ok=19
tests required=3 ok=3' "PASS $name"

  stub_case windows-paths
  for file in "$work/stub/"*; do
    awk '/^ *Running / { n = split($0, part, "/"); $0 = part[1]; for (i = 2; i <= n; i++) $0 = $0 "\\" part[i]; sub(/\)$/, ".exe)") } { print }' \
      "$file" >"$STUB_DIR/${file##*/}" || exit 2
  done
  check windows-paths PASS 'refusals expected=19 ok=19
tests required=3 ok=3' "PASS $name"

  STUB_TEST_EXIT=101
  check cargo-test-fails FAIL "refusals expected=19 ok=19" "FAIL $name (tests)"

  stub_case ignored-test-whose-ok-line-another-writes
  printf '     Running tests/sequences_same.rs (target/debug/deps/sequences_same-0123456789abcdef)\nsequences_same: test\n' \
    >"$STUB_DIR/ignored-halvo-route-gate.txt" || exit 2
  check ignored-test-whose-ok-line-another-writes FAIL 'tests required=3 ok=2' "FAIL $name (tests)"

  stub_case ignored-case-with-stand-ins-elsewhere
  printf '     Running tests/refusal_spend.rs (target/debug/deps/refusal_spend-0123456789abcdef)\nrefusal_no_hold: test\n' \
    >"$STUB_DIR/ignored-halvo-route.txt" || exit 2
  variant 's/^test refusal_no_hold \.\.\. ok$/test refusal_no_hold ... ignored/' "$work/stub/exact-halvo-route.txt" "$STUB_DIR/exact-halvo-route.txt"
  {
    cat "$work/test-good.txt"
    printf '     Running tests/prices_data.rs (target/debug/deps/prices_data-0123456789abcdef)\ntest refusal_no_hold ... ok\n'
    printf '     Running unittests src/lib.rs (target/debug/deps/halvo_route_gate-0123456789abcdef)\ntest spendsim::tests::refusal_no_hold ... ok\ntest refusal_no_hold ... ok\n'
  } >"$work/test-stand-ins.txt" || exit 2
  STUB_TEST_OUT=$work/test-stand-ins.txt
  check ignored-case-with-stand-ins-elsewhere FAIL 'refusals expected=19 ok=18' "FAIL $name (refusals)"

  stub_case a-child-prints-a-target-line
  printf '     Running tests/formal_traces.rs (target/debug/deps/formal_traces-0123456789abcdef)\nformal_traces: test\n' \
    >"$STUB_DIR/ignored-halvo-route.txt" || exit 2
  variant 's/^test formal_traces \.\.\. ok$/test formal_traces ... ignored/' "$work/stub/exact-halvo-route.txt" "$STUB_DIR/exact-halvo-route.txt"
  {
    cat "$work/test-good.txt"
    printf '     Running tests/pick_rules.rs (target/debug/deps/pick_rules-0123456789abcdef)\n'
    printf '     Running tests/formal_traces.rs (target/debug/deps/formal_traces-0)\ntest formal_traces ... ok\n'
  } >"$work/test-child.txt" || exit 2
  STUB_TEST_OUT=$work/test-child.txt
  check a-child-prints-a-target-line FAIL 'tests required=3 ok=2' "FAIL $name (tests)"

  stub_case bound-run-without-its-ok-line
  variant '/^test refusal_no_cap \.\.\. ok$/d' "$work/stub/exact-halvo-route.txt" "$STUB_DIR/exact-halvo-route.txt"
  check bound-run-without-its-ok-line FAIL 'refusals expected=19 ok=18' "FAIL $name (refusals)"

  stub_case not-in-its-list
  variant '/^admit::tests::refusal_admit_sum: test$/d' "$work/stub/list-halvo-route.txt" "$STUB_DIR/list-halvo-route.txt"
  check not-in-its-list FAIL 'refusals expected=19 ok=18' "FAIL $name (refusals)"

  stub_case under-another-bound-target
  for kind in list exact; do
    awk -v kind="$kind" '
      kind == "list" && $0 == "refusal_no_hold: test" { next }
      kind == "exact" && $0 == "test refusal_no_hold ... ok" { next }
      { print }
      /Running tests\/formal_traces\.rs / { print (kind == "list" ? "refusal_no_hold: test" : "test refusal_no_hold ... ok") }
    ' "$work/stub/$kind-halvo-route.txt" >"$STUB_DIR/$kind-halvo-route.txt" || exit 2
  done
  check under-another-bound-target FAIL 'refusals expected=19 ok=18' "FAIL $name (refusals)"

  stub_case required-in-a-module
  for kind in list exact; do
    sed 's/^no_grant_in_release: test$/tests::no_grant_in_release: test/; s/^test no_grant_in_release \.\.\. ok$/test tests::no_grant_in_release ... ok/' \
      "$work/stub/$kind-halvo-route.txt" >"$STUB_DIR/$kind-halvo-route.txt" || exit 2
  done
  check required-in-a-module FAIL 'tests required=3 ok=2' "FAIL $name (tests)"

  stub_case summary-missing
  awk '/Running tests\/refusal_spend\.rs / { inside = 1 } /Running unittests / { inside = 0 } inside && /^test result: / { next } { print }' \
    "$work/stub/exact-halvo-route.txt" >"$STUB_DIR/exact-halvo-route.txt" || exit 2
  check summary-missing FAIL 'refusals expected=19 ok=11' "FAIL $name (refusals)"

  STUB_EXACT_EXIT=101
  check bound-run-fails FAIL 'refusals expected=19 ok=0
tests required=3 ok=1' "FAIL $name (tests)"
  STUB_LIST_EXIT=101
  check list-run-fails FAIL 'refusals expected=19 ok=0' "FAIL $name (tests)"
  STUB_IGNORED_EXIT=101
  check ignored-run-fails FAIL 'refusals expected=19 ok=0' "FAIL $name (tests)"
  STUB_CHANGE=list
  check list-run-changes-the-tree FAIL "FAIL $name (tests)"

  stub_case a-test-writes-the-counted-lines
  printf '     Running tests/refusal_spend.rs (target/debug/deps/refusal_spend-0123456789abcdef)\nrefusal_no_hold: test\n' \
    >"$STUB_DIR/ignored-halvo-route.txt" || exit 2
  variant 's/^test refusal_no_hold \.\.\. ok$/test refusal_no_hold ... ignored/' "$work/stub/exact-halvo-route.txt" "$STUB_DIR/exact-halvo-route.txt"
  printf '     Running tests/sequences_same.rs (target/debug/deps/sequences_same-0123456789abcdef)\nsequences_same: test\n' \
    >"$STUB_DIR/ignored-halvo-route-gate.txt" || exit 2
  variant 's/^test sequences_same \.\.\. ok$/test sequences_same ... ignored/' "$work/stub/exact-halvo-route-gate.txt" "$STUB_DIR/exact-halvo-route-gate.txt"
  STUB_CHANGE=bound
  check a-test-writes-the-counted-lines FAIL 'refusals expected=19 ok=18
tests required=3 ok=2' "FAIL $name (tests)"

  stub_case a-bound-test-rewrites-the-ignored-list
  printf '     Running tests/refusal_spend.rs (target/debug/deps/refusal_spend-0123456789abcdef)\nrefusal_no_hold: test\n' \
    >"$STUB_DIR/ignored-halvo-route.txt" || exit 2
  STUB_CHANGE=ignored-file
  check a-bound-test-rewrites-the-ignored-list FAIL 'refusals expected=19 ok=18' "FAIL $name (refusals)"
  STUB_CHANGE=exact
  check bound-run-changes-the-tree FAIL "FAIL $name (tests)"
}

self_test() {
  enter_repo
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name-self-test.XXXXXX") || exit 2
  mkdir -p "$work/bin" "$work/repo" "$work/artifacts" "$work/tree" "$work/stub" || exit 2
  git -c core.autocrlf=false cat-file blob "HEAD:$sequences" >"$work/committed.jsonl" || exit 2
  [ -s "$work/committed.jsonl" ] || exit 2
  variant '1s/"answer":"ok"/"answer":"state"/' "$work/committed.jsonl" "$work/one-answer.jsonl"
  committed_lines=$(grep -c '' "$work/committed.jsonl") || exit 2
  awk 'NR == 1 { first = $0; next } NR == 2 { print; print first; next } { print }' "$work/committed.jsonl" >"$work/swapped.jsonl" || exit 2
  cmp -s "$work/swapped.jsonl" "$work/committed.jsonl" && exit 2
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
  bound_cases

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
  variant '2s/^spend /spent /' "$work/sim-good.txt" "$work/sim-first-word.txt"
  STUB_SIM_OUT=$work/sim-first-word.txt
  check a-case-line-of-another-first-word FAIL "FAIL $name (sim)"
  variant '7s/cases=6/cases=5/' "$work/sim-good.txt" "$work/sim-five.txt"
  STUB_SIM_OUT=$work/sim-five.txt
  check summary-of-five-cases-with-exit-0 FAIL "FAIL $name (sim)"
  STUB_SIM_OUT=$work/sim-fail.txt
  check summary-fail-with-exit-0 FAIL "FAIL $name (sim)"
  variant '7s/ overruns=0 / overruns=2 /' "$work/sim-good.txt" "$work/sim-total.txt"
  STUB_SIM_OUT=$work/sim-total.txt
  check summary-overruns-with-exit-0 FAIL "FAIL $name (sim)"
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
  STUB_FRESH=$work/swapped.jsonl
  check two-moves-in-another-order FAIL "sequences lines=$committed_lines same=no" "FAIL $name (sequences)"
  STUB_FRESH=$work/none.jsonl
  check not-exported FAIL "sequences lines=$committed_lines same=no" "FAIL $name (sequences)"

  STUB_CHANGE=tests
  check tests-change-the-tree FAIL "FAIL $name (tests)"
  STUB_CHANGE=sim
  check sim-changes-the-tree FAIL "FAIL $name (sim)"
  STUB_CHANGE=flags
  check tests-set-an-index-flag FAIL "FAIL $name (tests)"
  STUB_CHANGE='head'
  check tests-move-head FAIL "FAIL $name (tests)"
  STUB_FRESH=$work/one-answer.jsonl
  STUB_CHANGE=copy
  check tests-rewrite-the-checks-own-copy FAIL "sequences lines=$committed_lines same=no" "FAIL $name (sequences)"
  cp "$work/committed.jsonl" "$work/blob.jsonl" || exit 2
  STUB_COMMITTED=$work/blob.jsonl
  STUB_CHANGE=blob
  check committed-file-read-before-the-tests PASS "sequences lines=$committed_lines same=yes" "PASS $name"
  printf '2 R. N... 100644 100644 100644 0 0 R100 docs/r6/new.jsonl\tdocs/r6/gateway/sequences.jsonl\n' >>"$work/tree/status"
  check renamed-before FAIL "FAIL $name (tests)"
  printf '1 .M N... 100644 100644 100644 0 0 %s\n' "$sequences" >>"$work/tree/status"
  check changed-before FAIL "FAIL $name (tests)"
  printf 'h %s\n' "$sequences" >>"$work/tree/flags"
  check index-flag-before FAIL "FAIL $name (tests)"
  printf 'u UU N... 100644 100644 100644 100644 0 0 0 %s\n' "$sequences" >>"$work/tree/status"
  check unmerged-before FAIL "FAIL $name (tests)"
  : >"$work/tree/status"
  : >"$work/tree/flags"
  check empty-tree-state-before FAIL "FAIL $name (tests)"
  STUB_TERM=1
  check job-sets-cargo-terms PASS 'refusals expected=19 ok=19' "PASS $name"
  STUB_TEST_EXIT=101
  STUB_SIM_OUT=$work/sim-fail.txt
  STUB_SIM_EXIT=1
  STUB_COMMITTED=$work/one-answer.jsonl
  check causes-name-the-first FAIL "FAIL $name (tests)"
  stub_case refusals-before-sequences
  variant '/^test refusal_no_cap \.\.\. ok$/d' "$work/stub/exact-halvo-route.txt" "$STUB_DIR/exact-halvo-route.txt"
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
