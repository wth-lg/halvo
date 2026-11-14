#!/bin/sh
# jobs: macos
# rule: Every stall phase of tests/macos_stall_fixture.rs runs in its own process against the fixture and passes, each listed known fault fails by its own assertion of a recorded key, the measured lines and the loop are named and run in steps of their own, 12-test ran the tests of the file that drive stand-ins or tables (the launch path, one fixture for the line at the product's constants, the stamps command, the verdict and case tables and the on-time with the keys' fates) and passed them and ran no other test of the file, and neither a build without debug assertions that passes another bound of the hold nor a print of a held key compiles.
set -u
name=51-macos-stall
test_limit_s=${MS_TEST_LIMIT_S:-900}
fixture_source=crates/halvo-capture/tests/macos_stall_fixture/fixture.m
leak='recorded keys beside the markers in rounds that counted'
LC_ALL=C
export LC_ALL

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

expected_tests() {
  cat <<'EOF'
the_probe_reports_what_the_stall_phases_rest_on not-run
a_key_typed_ahead_into_a_secure_field_is_not_recorded phase
a_key_typed_ahead_of_a_click_into_a_secure_field_is_not_recorded phase
a_key_typed_ahead_of_a_focus_move_by_the_program_is_not_recorded phase
a_key_typed_on_a_return_to_a_secure_field_is_not_recorded three-way
the_up_of_the_key_that_leaves_a_secure_field_is_not_recorded phase
the_control_records_the_keys_typed_in_the_plain_field_and_none_of_the_secure_field phase
at_the_bound_of_the_product_a_stall_records_nothing_and_the_control_records_its_keys three-way
the_gap_rule_through_the_real_timer_drops_every_key_when_every_tick_is_a_gap_and_none_when_none_is phase
known_fault_a_key_typed_ahead_of_a_stall_longer_than_the_bound_is_not_recorded known-fault
measured_a_password_typed_whole_ahead_at_machine_speed measured
measured_a_password_typed_whole_ahead_at_a_persons_pace measured
short_window_a_the_click_round_as_it_was_at_wait_150_ms measured
short_window_a_the_click_round_as_it_was_at_wait_2000_ms measured
short_window_b_the_leave_asked_after_n_ms measured
the_mended_click_phase_counts_62_rounds_at_wait_2000_ms loop
the_launch_path_fails_with_a_line_that_names_the_wait_that_ran_out twelve
the_line_at_the_products_constants_starts_one_fixture_for_its_phase_and_its_control twelve
the_fixture_gives_out_its_stamps_on_a_command_through_the_launch_path twelve
the_verdict_of_a_mended_round_holds_each_rule_on_a_table_of_entries twelve
the_case_of_a_short_window_round_holds_each_rule_on_a_table_of_entries twelve
the_on_time_and_the_fates_of_the_keys_are_read_from_the_raw_entries twelve
a_reading_of_on_outside_every_span_is_counted_from_the_rounds_first_stamp twelve
EOF
}

notes() {
  if [ -n "${MS_NOTES_FILE:-}" ]; then
    tr -d '\r' <"$MS_NOTES_FILE"
    return
  fi
  cat <<'EOF'
measured_a_password_typed_whole_ahead_at_machine_speed in pushes 6 and 7 the keys were held under a reading of off and dropped by a reading that fell into the on-time while the program handled them; the on-time was not printed. D2 stays an open fault (MA-12 D, ruling MA-19)
short_window_a_the_click_round_as_it_was_at_wait_150_ms measures D2 and asserts nothing; it holds D2 in place of a listed test for as long as it exists (ruling MA-19 (7))
short_window_a_the_click_round_as_it_was_at_wait_2000_ms D2 stays an open fault. Its mechanism has been seen once on the hosted machine at the test's gap of 200 ms: an on-time shorter than the spacing of two readings that are both in time releases the keys. At the product's gap of 30 ms no hosted round shows it. The cause of push 7's recorded click round is not shown (ruling MA-20 (1), (7))
short_window_b_the_leave_asked_after_n_ms names the leave that the fixture was asked to make after n ms; the set on-times were not reached, because the leave ran behind the queued events, so each on-time is as measured, and the sample under 40 ms is thin (8 of 599 rounds in push 8) (ruling MA-20 (6))
EOF
}

history_note() {
  cat <<'EOF'
MS note pushes 5 to 7: in the Tab, click, focus-move, leaving-key and whole-password phases the test neither kept nor measured the on-time; each round that read 0 shows that a reading fell inside it, and no more; 1 of 105 counted click rounds recorded its 8 letters (ruling MA-19 (7))
EOF
}

proof_lines() {
  cat <<'EOF'
hold_bound_in_a_release_build E0599
held_key_printed E0277
seam_in_a_release_build E0599
EOF
}

seam_manifests() {
  git grep -l -F -e stall-seam -- 'Cargo.toml' '*/Cargo.toml' >"$work/seam" 2>/dev/null
  case $? in 0 | 1) ;; *) fail "git grep could not read the manifests" ;; esac
  named=$(tr '\n' ' ' <"$work/seam")
  [ "$named" = "crates/halvo-capture/Cargo.toml " ] ||
    fail "the feature stall-seam must stand in crates/halvo-capture/Cargo.toml alone; manifests that name it: ${named:-none}"
  printf 'seam: the feature stall-seam stands in crates/halvo-capture/Cargo.toml alone\n'
}

tracked_changes() {
  git status --porcelain --untracked-files=no
}

judge() {
  verdict=FAIL
  cause=
  out=$(tr -d '\r' <"$3")
  summary=$(printf '%s\n' "$out" | sed -n 's/^test result: //p' | tail -n 1)
  case $2 in
    phase | three-way)
      if [ "$1" -ne 0 ]; then
        cause="exit $1"
        return 0
      fi
      case $summary in
        'ok. 1 passed; 0 failed;'*) verdict=pass ;;
        *) cause='exit 0, and not one test ran and passed' ;;
      esac
      return 0
      ;;
  esac
  if [ "$1" -eq 0 ]; then
    case $summary in
      'ok. 1 passed; 0 failed;'*)
        verdict=FIXED-BUT-LISTED
        cause='the listed known fault passed'
        ;;
      *) cause='exit 0, and not one test ran and passed' ;;
    esac
    return 0
  fi
  case $1:$summary in
    '101:FAILED. 0 passed; 1 failed;'*) ;;
    *)
      cause="exit $1, and not the exit 101 of one failed test"
      return 0
      ;;
  esac
  case $out in
    *"$leak"*) verdict='known-fault' ;;
    *) cause='the listed known fault failed without its assertion of a recorded key' ;;
  esac
}

run_test() {
  HALVO_STALL_FIXTURE=$fixture perl -e 'alarm shift; exec @ARGV or exit 127' "$test_limit_s" \
    "$exe" --ignored --exact --nocapture --test-threads=1 "$1" </dev/null >"$2" 2>&1
}

proof_build() {
  build_list=$1
  build_log=$2
  set --
  for proof in $build_list; do
    set -- "$@" --cfg "halvo_proof=\"$proof\""
  done
  cargo rustc --locked -p halvo-capture --lib --profile check --message-format short -- -C debug-assertions=off "$@" >"$build_log" 2>&1
}

twelve_test() {
  list=$CI_D_REPO/target/ci-artifacts/12-test/tests.txt
  if [ ! -f "$list" ]; then
    printf '12-test: no list of tests in this job\n'
    return 0
  fi
  : >"$work/twelve-ran" || fail "the list of 12-test cannot be read"
  awk -v ran_file="$work/twelve-ran" '
    /^ *Running / { inside = index($0, "tests/macos_stall_fixture.rs") > 0; next }
    /^ *Doc-tests / { inside = 0; next }
    inside && /^test / {
      if ($0 ~ / \.\.\. ignored(,|$)/) ignored++
      else { ran++; print $2, $NF > ran_file }
    }
    END { printf "%d %d\n", ignored, ran }' "$list" >"$work/twelve" || fail "the list of 12-test cannot be read"
  read -r ignored ran <"$work/twelve" || fail "the list of 12-test cannot be read"
  sed -n 's/ twelve$//p' "$work/expected" | sort >"$work/twelve-want" || fail "the expected list cannot be read"
  cut -d ' ' -f 1 "$work/twelve-ran" | sort >"$work/twelve-got" || fail "the list of 12-test cannot be read"
  printf '12-test: tests of macos_stall_fixture ignored=%s ran=%s\n' "$ignored" "$ran"
  other=$(comm -13 "$work/twelve-want" "$work/twelve-got" | tr '\n' ' ')
  [ -z "$other" ] || fail "12-test ran tests of tests/macos_stall_fixture.rs that are not the launch-path test: ${other% }"
  absent=$(comm -23 "$work/twelve-want" "$work/twelve-got" | tr '\n' ' ')
  [ -z "$absent" ] || fail "12-test did not run the launch-path test of tests/macos_stall_fixture.rs: ${absent% }"
  failed_here=$(grep -v ' ok$' "$work/twelve-ran" | cut -d ' ' -f 1 | tr '\n' ' ')
  [ -z "$failed_here" ] || fail "12-test ran the launch-path test and it did not pass: ${failed_here% }"
  [ "$ignored" -gt 0 ] || fail "12-test listed no test of tests/macos_stall_fixture.rs as ignored"
}

measure() {
  cd "$CI_D_REPO" || exit 2
  artifacts=${CI_D_ARTIFACTS:-$CI_D_REPO/target/ci-artifacts/$name}
  mkdir -p "$artifacts" || fail "the artifacts directory cannot be made"
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail "mktemp failed"
  if [ -n "${MS_EXPECTED_FILE:-}" ]; then
    tr -d '\r' <"$MS_EXPECTED_FILE" >"$work/expected" || fail "the expected list cannot be read"
  else
    expected_tests >"$work/expected"
  fi
  proof_lines >"$work/proofs"
  before=$(tracked_changes) || fail "git status failed before the run"

  twelve_test
  seam_manifests
  printf 'session reader of the phases: %s\n' "${HALVO_STALL_SESSION:-real}"

  proofs=$(cut -d ' ' -f 1 "$work/proofs" | tr '\n' ' ')
  twins=$(cut -d ' ' -f 1 "$work/proofs" | sed 's/$/_twin/' | tr '\n' ' ')
  proof_build "$twins" "$work/twins.txt"
  status=$?
  [ "$status" -eq 0 ] || fail "the twins of the proofs did not build: cargo rustc exited $status"
  proof_build "$proofs" "$work/proofs.txt"
  status=$?
  [ "$status" -ne 0 ] || fail "the proofs built: a build without debug assertions passed another bound or printed a held key"
  cut -d ' ' -f 2 "$work/proofs" >"$work/proof-codes" || fail "the list of proofs cannot be read"
  while read -r proof code; do
    wanted=$(grep -c -x -e "$code" "$work/proof-codes")
    found=$(grep -c "error\[$code\]" "$work/proofs.txt")
    [ "$found" -ge "$wanted" ] || fail "the proof $proof did not fail with $code ($found errors $code for $wanted proofs)"
    printf 'proof %s: refused with %s\n' "$proof" "$code"
  done <"$work/proofs"

  fixture=$work/stall-fixture
  clang -fobjc-arc -Wall -Wextra -Werror -O1 -framework AppKit -framework Carbon -framework CoreGraphics \
    -o "$fixture" "$fixture_source" >"$work/fixture.txt" 2>&1 ||
    fail "the fixture did not compile: clang exited $?"

  cargo test --locked -p halvo-capture --test macos_stall_fixture --no-run --message-format json >"$work/build.json" ||
    fail "the test build failed: cargo test --no-run exited $?"
  exe=$(sed -n 's/.*"name":"macos_stall_fixture".*"executable":"\([^"]*\)".*/\1/p' "$work/build.json")
  [ -n "$exe" ] || fail "cargo named no executable for the test target macos_stall_fixture"
  case $exe in
    *"
"*) fail "cargo named more than one executable for the test target macos_stall_fixture" ;;
  esac
  [ -f "$exe" ] || fail "the test executable that cargo named does not exist"

  "$exe" --list >"$work/list.txt" 2>&1 || fail "the test executable did not list its tests: exited $?"
  sed -n 's/: test$//p' "$work/list.txt" | sort >"$work/listed" || fail "the test list cannot be read"
  cut -d ' ' -f 1 "$work/expected" | sort >"$work/expected-names" || fail "the expected list cannot be read"
  missing=$(comm -23 "$work/expected-names" "$work/listed" | tr '\n' ' ')
  [ -z "$missing" ] || fail "the test executable does not list these tests of this script's list: ${missing% }"
  extra=$(comm -13 "$work/expected-names" "$work/listed" | tr '\n' ' ')
  [ -z "$extra" ] || fail "the test executable lists tests that this script's list does not hold: ${extra% }"

  : >"$artifacts/ms.txt" || fail "ms.txt cannot be written"
  passed=0
  known=0
  fixed=0
  failed=0
  not_driven=0
  not_kept=0
  index=0
  while read -r test kind; do
    case $kind in
      not-run | twelve) continue ;;
      measured | loop)
        line="MS $test kind=$kind run=in_a_step_of_its_own"
        printf '%s\n' "$line" | tee -a "$artifacts/ms.txt" || fail "ms.txt cannot be written"
        note=$(notes | sed -n "s/^$test //p")
        [ -z "$note" ] || printf 'MS note %s: %s\n' "$test" "$note" | tee -a "$artifacts/ms.txt" || fail "ms.txt cannot be written"
        continue
        ;;
    esac
    index=$((index + 1))
    log=$work/test-$index.log
    started=$(date +%s)
    run_test "$test" "$log"
    code=$?
    seconds=$(($(date +%s) - started))
    tr -d '\r' <"$log" | grep -E "^$test (session_reader=|gap_ms=|fixture=|round=|control=|verdict=|notdriven |notcounted |late_click |entries |drops |side=)" | sed 's/^/    | /'
    judge "$code" "$kind" "$log"
    three=
    if [ "$kind" = three-way ] && [ "$verdict" = pass ]; then
      three=$(tr -d '\r' <"$log" | sed -n -E "s/^$test verdict=(held|notdriven|notkept) .*/\1/p" | tail -n 1)
      case $three in
        held) ;;
        notdriven) not_driven=$((not_driven + 1)) ;;
        notkept) not_kept=$((not_kept + 1)) ;;
        *)
          verdict=FAIL
          cause='the three-way phase printed no verdict line of held, notdriven or notkept'
          ;;
      esac
    fi
    case $verdict in
      pass) passed=$((passed + 1)) ;;
      known-fault) known=$((known + 1)) ;;
      FIXED-BUT-LISTED) fixed=$((fixed + 1)) ;;
      *) failed=$((failed + 1)) ;;
    esac
    [ -z "$cause" ] || printf '%s: %s: %s\n' "$name" "$test" "$cause"
    line="MS $test result=$verdict${three:+ verdict=$three} seconds=$seconds"
    printf '%s\n' "$line"
    printf '%s\n' "$line" >>"$artifacts/ms.txt" || fail "ms.txt cannot be written"
    note=$(notes | sed -n "s/^$test //p")
    [ -z "$note" ] || printf 'MS note %s: %s\n' "$test" "$note" | tee -a "$artifacts/ms.txt" || fail "ms.txt cannot be written"
  done <"$work/expected"

  after=$(tracked_changes) || fail "git status failed after the run"
  [ "$after" = "$before" ] || fail "a tracked file differs from HEAD after the run"
  history_note | tee -a "$artifacts/ms.txt" || fail "ms.txt cannot be written"
  line="MS total=$index pass=$passed known_fault=$known notdriven=$not_driven notkept=$not_kept fail=$((failed + fixed))"
  printf '%s\n' "$line"
  printf '%s\n' "$line" >>"$artifacts/ms.txt" || fail "ms.txt cannot be written"
  [ "$fixed" -eq 0 ] || fail "$fixed listed known faults passed (FIXED-BUT-LISTED): take each out of this script's list"
  [ "$failed" -eq 0 ] || fail "$failed tests failed"
  printf 'PASS %s\n' "$name"
}

check() {
  case_name=$1
  want=$2
  want_text=$3
  spec_file=$work/spec-$case_name
  printf '%s\n' "$4" >"$spec_file"
  out=$(
    PATH=$work/bin:$PATH
    CI_D_REPO=$work/repo
    CI_D_ARTIFACTS=$work/artifacts-$case_name
    STUB_SPEC=$spec_file
    STUB_EXE=$work/bin/stall-stub
    MS_EXPECTED_FILE=$work/expected-small
    MS_TEST_LIMIT_S=${limit:-30}
    MS_NOTES_FILE=$work/notes-small
    export PATH CI_D_REPO CI_D_ARTIFACTS STUB_SPEC STUB_EXE MS_EXPECTED_FILE MS_TEST_LIMIT_S MS_NOTES_FILE
    sh "$0" 2>&1
  )
  code=$?
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  case $out in
    *"$want_text"*) ;;
    *) got=$got-without-the-text ;;
  esac
  printf 'case %s/%s want=%s got=%s\n' "$name" "$case_name" "$want" "$got"
  if [ "$got" != "$want" ]; then
    printf '%s\n' "$out" | tail -n 3 | sed 's/^/    | /'
    wrong="$wrong $case_name"
  elif [ "$want" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
  limit=
}

write_stubs() {
  cat >"$work/bin/cargo" <<'EOF'
#!/bin/sh
spec() { grep -q "^$1\$" "$STUB_SPEC"; }
case $* in
  'rustc --locked -p halvo-capture --lib --profile check --message-format short -- -C debug-assertions=off --cfg halvo_proof="hold_bound_in_a_release_build_twin" --cfg halvo_proof="held_key_printed_twin" --cfg halvo_proof="seam_in_a_release_build_twin"')
    spec twins-fail && exit 101
    exit 0
    ;;
  'rustc --locked -p halvo-capture --lib --profile check --message-format short -- -C debug-assertions=off --cfg halvo_proof="hold_bound_in_a_release_build" --cfg halvo_proof="held_key_printed" --cfg halvo_proof="seam_in_a_release_build"')
    spec proofs-build && exit 0
    spec proof-other-code && { printf 'error[E0308]: mismatched types\nerror[E0277]: no Debug\n'; exit 101; }
    spec proof-one-e0599 && { printf 'error[E0599]: no function named `with_hold_bound`\nerror[E0277]: `HeldKey` does not implement `Debug`\n'; exit 101; }
    printf 'error[E0599]: no function named `with_hold_bound`\nerror[E0277]: `HeldKey` does not implement `Debug`\nerror[E0599]: no function named `with_seam`\n'
    exit 101
    ;;
  'test --locked -p halvo-capture --test macos_stall_fixture --no-run --message-format json')
    spec test-build-fails && exit 101
    printf '{"reason":"compiler-artifact","target":{"kind":["test"],"crate_types":["bin"],"name":"macos_stall_fixture","test":true},"executable":"%s","fresh":true}\n' "$STUB_EXE"
    exit 0
    ;;
esac
exit 97
EOF
  cat >"$work/bin/clang" <<'EOF'
#!/bin/sh
grep -q '^fixture-fails$' "$STUB_SPEC" && exit 1
for arg; do
  case $prev in -o) printf '#!/bin/sh\nexit 0\n' >"$arg" && chmod +x "$arg" ;; esac
  prev=$arg
done
exit 0
EOF
  cat >"$work/bin/stall-stub" <<'EOF'
#!/bin/sh
spec() { grep -q "^$1\$" "$STUB_SPEC"; }
if [ "$*" = --list ]; then
  printf 'the_probe: test\nphase_one: test\nphase_two: test\nknown_one: test\nthree_one: test\nlaunch_one: test\nmeasured_one: test\nloop_one: test\n'
  spec extra-test && printf 'unlisted: test\n'
  exit 0
fi
test=$5
[ -x "$HALVO_STALL_FIXTURE" ] || exit 98
case $test in
  measured_* | loop_*)
    printf "thread 'x' panicked at y:\n%s: ran in the check\ntest result: FAILED. 0 passed; 1 failed; 0 ignored\n" "$test"
    exit 101
    ;;
esac
printf '%s round=0 wait_ms=150 bound_ms=1000 counted=1 ahead=0 markers=2 plain=2 secure=8\n' "$test"
spec "touch-$test" && printf 'changed\n' >>"$CI_D_REPO/tracked.txt"
spec "hang-$test" && sleep 5
spec "fail-$test" && { printf "thread 'x' panicked at y:\n%s: rounds that did not count: wait 150 ms\ntest result: FAILED. 0 passed; 1 failed; 0 ignored\n" "$test"; exit 101; }
spec "leak-$test" && { printf "thread 'x' panicked at y:\n%s: recorded keys beside the markers in rounds that counted: wait 150 ms: 3\ntest result: FAILED. 0 passed; 1 failed; 0 ignored\n" "$test"; exit 101; }
spec "empty-$test" && { printf 'test result: ok. 0 passed; 0 failed; 1 ignored\n'; exit 0; }
case $test in
  three_*)
    if spec "silent-$test"; then :
    elif spec "notdriven-$test"; then printf '%s verdict=notdriven held=3 failed=0 notdriven=2\n' "$test"
    elif spec "notkept-$test"; then printf '%s verdict=notkept widest_gap_us=61000 gap_drops=2\n' "$test"
    else printf '%s verdict=held held=5 failed=0 notdriven=0\n' "$test"; fi
    printf 'test result: ok. 1 passed; 0 failed; 0 ignored\n'
    exit 0
    ;;
  known_*)
    spec "pass-$test" && { printf 'test result: ok. 1 passed; 0 failed; 0 ignored\n'; exit 0; }
    printf "thread 'x' panicked at y:\n%s: recorded keys beside the markers in rounds that counted: wait 1500 ms: 9\ntest result: FAILED. 0 passed; 1 failed; 0 ignored\n" "$test"
    exit 101
    ;;
esac
printf 'test result: ok. 1 passed; 0 failed; 0 ignored\n'
exit 0
EOF
  chmod +x "$work/bin/cargo" "$work/bin/clang" "$work/bin/stall-stub" || exit 2
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name-self-test.XXXXXX") || {
    printf 'FAIL self-test:%s (mktemp failed)\n' "$name"
    exit 1
  }
  mkdir -p "$work/bin" "$work/repo/target/ci-artifacts/12-test" || exit 2
  write_stubs
  printf 'the_probe not-run\nphase_one phase\nphase_two phase\nknown_one known-fault\nthree_one three-way\nlaunch_one twelve\nmeasured_one measured\nloop_one loop\n' >"$work/expected-small"
  printf 'phase_two shows what it shows; it does not show the rest\n' >"$work/notes-small"
  git -C "$work/repo" init -q || exit 2
  printf 'tracked\n' >"$work/repo/tracked.txt" || exit 2
  mkdir -p "$work/repo/crates/halvo-capture" "$work/repo/crates/other" || exit 2
  printf '[features]\nstall-seam = []\n' >"$work/repo/crates/halvo-capture/Cargo.toml" || exit 2
  printf '[dependencies]\n' >"$work/repo/crates/other/Cargo.toml" || exit 2
  git -C "$work/repo" add tracked.txt crates || exit 2
  twelve=$work/repo/target/ci-artifacts/12-test/tests.txt
  ignored='     Running tests/macos_stall_fixture.rs (target/debug/deps/macos_stall_fixture-0)
test a_key_typed_ahead_into_a_secure_field_is_not_recorded ... ignored, posts keys and shows a window: the hosted step alone runs it
test the_probe_reports_what_the_stall_phases_rest_on ... ignored
test launch_one ... ok
     Running tests/source_rules.rs (target/debug/deps/source_rules-0)
test a_rule ... ok'
  printf '%s\n' "$ignored" >"$twelve"
  wrong=
  rejected=0
  limit=
  check all-pass PASS 'MS total=4 pass=3 known_fault=1 notdriven=0 notkept=0 fail=0' ''
  check note-line PASS 'MS note phase_two: shows what it shows; it does not show the rest' ''
  check measured-named-not-run PASS 'MS measured_one kind=measured run=in_a_step_of_its_own' ''
  check loop-named-not-run PASS 'MS loop_one kind=loop run=in_a_step_of_its_own' ''
  check history-line PASS 'MS note pushes 5 to 7: in the Tab, click, focus-move, leaving-key and whole-password phases' ''
  check three-way-not-kept PASS 'MS three_one result=pass verdict=notkept' 'notkept-three_one'
  check three-way-not-driven PASS 'MS three_one result=pass verdict=notdriven' 'notdriven-three_one'
  check three-way-without-a-verdict FAIL 'printed no verdict line of held, notdriven or notkept' 'silent-three_one'
  check phase-fails FAIL 'FAIL 51-macos-stall (1 tests failed)' 'fail-phase_one'
  check phase-runs-no-test FAIL 'phase_two: exit 0, and not one test ran and passed' 'empty-phase_two'
  check known-fault-passes FAIL 'FIXED-BUT-LISTED' 'pass-known_one'
  check known-fault-fails-otherwise FAIL 'failed without its assertion of a recorded key' 'fail-known_one'
  check phase-leaks FAIL 'phase_one result=FAIL' 'leak-phase_one'
  check extra-test FAIL 'lists tests that this script' 'extra-test'
  check twins-fail FAIL 'the twins of the proofs did not build' 'twins-fail'
  check proofs-build FAIL 'the proofs built' 'proofs-build'
  check proof-fails-with-another-code FAIL 'did not fail with E0599' 'proof-other-code'
  check one-error-for-two-proofs FAIL '(1 errors E0599 for 2 proofs)' 'proof-one-e0599'
  check fixture-fails FAIL 'the fixture did not compile' 'fixture-fails'
  check test-build-fails FAIL 'the test build failed' 'test-build-fails'
  check tracked-file-changed FAIL 'a tracked file differs from HEAD after the run' 'touch-phase_two'
  git -C "$work/repo" checkout -q -- tracked.txt 2>/dev/null || printf 'tracked\n' >"$work/repo/tracked.txt"
  limit=1
  check time-limit FAIL 'phase_one: exit 142' 'hang-phase_one'
  printf '     Running tests/macos_stall_fixture.rs (x)\ntest a_key_typed_ahead_into_a_secure_field_is_not_recorded ... ok\ntest launch_one ... ok\ntest the_probe_reports_what_the_stall_phases_rest_on ... ignored\n' >"$twelve"
  check twelve-test-ran-a-phase FAIL 'that are not the launch-path test: a_key_typed_ahead_into_a_secure_field_is_not_recorded' ''
  printf '     Running tests/macos_stall_fixture.rs (x)\ntest the_probe_reports_what_the_stall_phases_rest_on ... ignored\n' >"$twelve"
  check twelve-test-ran-no-launch FAIL 'did not run the launch-path test of tests/macos_stall_fixture.rs: launch_one' ''
  printf '     Running tests/macos_stall_fixture.rs (x)\ntest launch_one ... FAILED\ntest the_probe_reports_what_the_stall_phases_rest_on ... ignored\n' >"$twelve"
  check twelve-test-launch-failed FAIL 'ran the launch-path test and it did not pass: launch_one' ''
  printf '     Running tests/macos_stall_fixture.rs (x)\ntest launch_one ... ok\n' >"$twelve"
  check twelve-test-listed-none FAIL '12-test listed no test of tests/macos_stall_fixture.rs as ignored' ''
  mv "$twelve" "$work/twelve-moved" || exit 2
  check twelve-test-absent PASS '12-test: no list of tests in this job' ''
  printf '[dependencies]\nhalvo-capture = { path = "../halvo-capture", features = ["stall-seam"] }\n' >"$work/repo/crates/other/Cargo.toml" || exit 2
  git -C "$work/repo" add crates/other/Cargo.toml || exit 2
  check seam-named-by-another-manifest FAIL 'manifests that name it: crates/halvo-capture/Cargo.toml crates/other/Cargo.toml' ''
  printf '[dependencies]\n' >"$work/repo/crates/other/Cargo.toml" || exit 2
  printf '[features]\n' >"$work/repo/crates/halvo-capture/Cargo.toml" || exit 2
  git -C "$work/repo" add crates || exit 2
  check seam-in-no-manifest FAIL 'manifests that name it: none' ''
  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (cases that gave another verdict:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  '') measure ;;
  *)
    printf 'usage: sh %s [--self-test]\n' "$0" >&2
    exit 64
    ;;
esac
