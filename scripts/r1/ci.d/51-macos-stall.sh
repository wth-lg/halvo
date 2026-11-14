#!/bin/sh
# jobs: macos
# rule: Every stall phase of tests/macos_stall_fixture.rs runs in its own process against the fixture and passes, each listed known fault fails by its own assertion of a recorded key, the phases run at the gap of the test's own, which input.rs decides (from HALVO_STALL_GAP_MS, or from measure_the_readings_of_the_hold when it is unset, measured once more above its cap) and which fails the check by name when it cannot be taken, in the form that HALVO_STALL_FORM names, a closed word that the check hands on alone: short (or unset) in the every-push job, which runs every wait of each mended phase with the rounds that tests/macos_stall_fixture.rs gives it, the return phase, the control, the two-sided gap rule, both listed known faults and the proofs, and prints a line for each thing that it leaves out; and long, which the ledger owes for a change of the hold's nine files and which also runs the line at the product's constants, short_window form (a), the loop and the whole-password lines; a measured line asserts nothing and holds no promise, D2 is held by a stand-in at the product's gap listed as a known fault, the measured run of the long form that passed and that docs/baseline/ledger.d/macos-2b.txt records ran on the blobs that the head has of the nine files of the hold, 12-test ran the tests of the file that drive stand-ins or tables (the launch path, one fixture for the line at the product's constants, the stamps command, the verdict and case tables and the on-time with the keys' fates) and passed them and ran no other test of the file, and a check build of the library without debug assertions has no InputSource::with_hold_bound, no Debug for a held key and no InputSource::with_seam, the last also with the feature stall-seam.
set -u
name=51-macos-stall
limit_override=${MS_TEST_LIMIT_S:-}
budget_s=${MS_BUDGET_S:-240}
form_test=the_form_of_the_hosted_phases
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
the_form_of_the_hosted_phases not-run
a_key_typed_ahead_into_a_secure_field_is_not_recorded phase
a_key_typed_ahead_of_a_click_into_a_secure_field_is_not_recorded phase
a_key_typed_ahead_of_a_focus_move_by_the_program_is_not_recorded phase
a_key_typed_on_a_return_to_a_secure_field_is_not_recorded three-way
the_up_of_the_key_that_leaves_a_secure_field_is_not_recorded phase
the_control_records_the_keys_typed_in_the_plain_field_and_none_of_the_secure_field phase
at_the_bound_of_the_product_a_stall_records_nothing_and_the_control_records_its_keys three-way long
the_gap_rule_through_the_real_timer_drops_every_key_when_every_tick_is_a_gap_and_none_when_none_is phase
known_fault_a_key_typed_ahead_of_a_stall_longer_than_the_bound_is_not_recorded known-fault
known_fault_secure_input_on_for_less_than_the_spacing_of_two_readings_releases_the_keys known-fault
measured_a_password_typed_whole_ahead_at_machine_speed measured
measured_a_password_typed_whole_ahead_at_a_persons_pace measured
short_window_a_the_click_round_as_it_was_at_wait_150_ms measured
short_window_a_the_click_round_as_it_was_at_wait_2000_ms measured
short_window_b_the_leave_asked_after_n_ms not-run
the_mended_click_phase_counts_62_rounds_at_wait_2000_ms loop
the_launch_path_fails_with_a_line_that_names_the_wait_that_ran_out twelve
the_line_at_the_products_constants_starts_one_fixture_for_its_phase_and_its_control twelve
the_fixture_gives_out_its_stamps_on_a_command_through_the_launch_path twelve
the_verdict_of_a_mended_round_holds_each_rule_on_a_table_of_entries twelve
the_verdict_at_the_products_constants_holds_each_rule_on_a_table twelve
each_judge_of_a_mended_phase_fails_it_at_one_round_for_each_wait twelve
the_waits_rounds_and_kept_time_of_both_forms_are_the_ruled_ones twelve
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
short_window_a_the_click_round_as_it_was_at_wait_150_ms measures D2, asserts nothing and holds no promise; the listed stand-in holds D2 (ruling MA-22 L10)
known_fault_secure_input_on_for_less_than_the_spacing_of_two_readings_releases_the_keys holds D2 (MA-12 D3) with stand-ins at the product's gap: an on-time shorter than the spacing of two readings that are both in time releases the keys; D2 stays an open fault until a ruling names the unit that closed it (ruling MA-22 L10)
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

measured_files() {
  cat <<'EOF'
crates/halvo-capture/src/macos/input.rs
crates/halvo-capture/src/secure_input_macos.rs
crates/halvo-capture/src/secure_input_macos_hold.rs
crates/halvo-capture/tests/macos_stall_fixture.rs
crates/halvo-capture/tests/macos_stall_fixture/fixture.m
crates/halvo-capture/src/macos/secure_input_byte.rs
crates/halvo-capture/src/macos/run_loop.rs
crates/halvo-capture/Cargo.toml
scripts/r1/ci.d/51-macos-stall.sh
EOF
}

ledger_check() {
  ledger=docs/baseline/ledger.d/macos-2b.txt
  fix="run the long form (HALVO_STALL_FORM=long) on a scratch push of this head, then record that run with form=long, result=pass, its figures and the blob ids of the nine files of the hold in $ledger (rulings MA-22 L10, MA-23)"
  [ -f "$ledger" ] || fail "$ledger is missing: $fix"
  tr -d '\r' <"$ledger" >"$work/ledger" || fail "$ledger cannot be read"
  runs=$(sed -n 's/^measured run=\([0-9][0-9]*\) .*/\1/p' "$work/ledger" | tr '\n' ' ')
  long=$(grep -c -E '^measured run=[0-9]+ (.* )?form=long( .*)? result=pass( |$)' "$work/ledger")
  run=${runs% }
  case $run in
    '') fail "$ledger names no measured run: $fix" ;;
    *' '*) fail "$ledger names more than one measured run ($run): $fix" ;;
  esac
  [ "$long" = 1 ] || fail "$ledger names the measured run $run without form=long and result=pass: $fix"
  sed -n 's/^measured blob path=\([^ ]*\) id=[0-9a-f]*$/\1/p' "$work/ledger" | sort >"$work/ledger-paths" ||
    fail "$ledger cannot be read"
  measured_files | sort >"$work/measured-paths" || fail "the list of the nine files cannot be read"
  cmp -s "$work/ledger-paths" "$work/measured-paths" ||
    fail "$ledger records the blobs of files other than the nine of the hold, once each: $fix"
  while read -r path; do
    recorded=$(awk -v path="$path" '$1 == "measured" && $2 == "blob" && $3 == "path=" path { sub(/^id=/, "", $4); print $4 }' "$work/ledger")
    staged=$(git ls-files --stage -- "$path" | cut -d ' ' -f 2)
    [ -n "$staged" ] || fail "$path is not tracked: $fix"
    [ "$recorded" = "$staged" ] ||
      fail "the measured run $run ran on a blob of $path other than the head's ($recorded against $staged): $fix"
  done <"$work/measured-paths"
  printf 'ledger: the measured run %s of the long form ran on the blobs that the head has of the nine files of the hold\n' "$run"
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
    phase | three-way | measured | loop)
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

lib_gap() {
  perl -e 'alarm shift; exec @ARGV or exit 127' 150 "$lib" --ignored --exact --nocapture "macos::input::tests::$1" \
    </dev/null >"$work/gap-$2.txt" 2>&1
  code=$?
  tr -d '\r' <"$work/gap-$2.txt" >"$work/gap-$2-lines.txt" || fail "the output of $1 cannot be read"
  [ "$code" -eq 0 ] || fail "the gap of the test's own could not be measured: $1 exited $code"
  grep '^measure' "$work/gap-$2-lines.txt"
  decided=$(sed -n 's/^gap_of_the_test //p' "$work/gap-$2-lines.txt" | tail -n 1)
  [ -n "$decided" ] || fail "the gap of the test's own could not be measured: $1 printed no gap line"
}

measure_gap() {
  if [ -n "${HALVO_STALL_GAP_MS+set}" ]; then
    lib_gap the_gap_of_the_test_from_its_variable variable
  else
    lib_gap measure_the_readings_of_the_hold first
    grep -q '^measure ' "$work/gap-first-lines.txt" ||
      fail "the gap of the test's own could not be measured: measure_the_readings_of_the_hold printed no measure line"
    case $decided in
      above_the_cap*)
        printf 'the gap of the measurement is above its cap: measured again (ruling MA-23 Addition 1 (4))\n'
        first_line=$(grep '^measure ' "$work/gap-first-lines.txt")
        lib_gap measure_the_readings_of_the_hold second
        case $decided in
          above_the_cap*)
            second_line=$(grep '^measure ' "$work/gap-second-lines.txt")
            fail "the runner's readings came further apart than the hosted proof is stated for: $first_line / $second_line; run the job again"
            ;;
        esac
        ;;
    esac
  fi
  case $decided in
    refused*) fail "the gap of the test's own could not be measured: ${decided#*reason=}" ;;
    gap_ms=*) ;;
    *) fail "the gap of the test's own could not be measured: the gap line reads $decided" ;;
  esac
  HALVO_STALL_GAP_MS=$(printf '%s\n' "$decided" | sed -n 's/^gap_ms=\([0-9][0-9]*\) .*/\1/p')
  [ -n "$HALVO_STALL_GAP_MS" ] || fail "the gap of the test's own could not be measured: the gap line reads $decided"
  export HALVO_STALL_GAP_MS
  printf 'MS %s\n' "$decided" | tee -a "$artifacts/ms.txt" || fail "ms.txt cannot be written"
}

run_test() {
  HALVO_STALL_FIXTURE=$fixture perl -e 'alarm shift; exec @ARGV or exit 127' "$3" \
    "$exe" --ignored --exact --nocapture --test-threads=1 "$1" </dev/null >"$2" 2>&1
}

proof_build() {
  build_list=$1
  build_log=$2
  build_features=${3:-}
  set --
  for proof in $build_list; do
    set -- "$@" --cfg "halvo_proof=\"$proof\""
  done
  if [ -n "$build_features" ]; then
    cargo rustc --locked -p halvo-capture --lib --profile check --message-format short --features "$build_features" \
      -- -C debug-assertions=off "$@" >"$build_log" 2>&1
  else
    cargo rustc --locked -p halvo-capture --lib --profile check --message-format short -- -C debug-assertions=off "$@" >"$build_log" 2>&1
  fi
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
  check_started=$(date +%s)
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
  [ "$status" -ne 0 ] || fail "the proofs built: a build without debug assertions has with_hold_bound, with_seam or Debug for a held key"
  cut -d ' ' -f 2 "$work/proofs" >"$work/proof-codes" || fail "the list of proofs cannot be read"
  while read -r proof code; do
    wanted=$(grep -c -x -e "$code" "$work/proof-codes")
    found=$(grep -c "error\[$code\]" "$work/proofs.txt")
    [ "$found" -ge "$wanted" ] || fail "the proof $proof did not fail with $code ($found errors $code for $wanted proofs)"
    printf 'proof %s: refused with %s\n' "$proof" "$code"
  done <"$work/proofs"
  proof_build seam_in_a_release_build_twin "$work/seam-twin.txt" stall-seam
  status=$?
  [ "$status" -eq 0 ] || fail "the twin of the seam proof did not build with the feature stall-seam: cargo rustc exited $status"
  proof_build seam_in_a_release_build "$work/seam-proof.txt" stall-seam
  status=$?
  [ "$status" -ne 0 ] || fail "the seam proof built with the feature stall-seam: a build without debug assertions has with_seam"
  grep -q 'error\[E0599\]' "$work/seam-proof.txt" || fail "the seam proof with the feature stall-seam did not fail with E0599"
  printf 'proof seam_in_a_release_build with the feature stall-seam: refused with E0599\n'

  fixture=$work/stall-fixture
  clang -fobjc-arc -Wall -Wextra -Werror -O1 -framework AppKit -framework Carbon -framework CoreGraphics \
    -o "$fixture" "$fixture_source" >"$work/fixture.txt" 2>&1 ||
    fail "the fixture did not compile: clang exited $?"

  built_from=$(date +%s)
  cargo test --locked -p halvo-capture --lib --test macos_stall_fixture --no-run --message-format json >"$work/build.json" ||
    fail "the test build failed: cargo test --no-run exited $?"
  printf 'test build of the library and the stall file: seconds=%s\n' "$(($(date +%s) - built_from))"
  lib=$(sed -n 's/.*"kind":\["lib"\].*"name":"halvo_capture".*"executable":"\([^"]*\)".*/\1/p' "$work/build.json" | tail -n 1)
  [ -n "$lib" ] && [ -f "$lib" ] ||
    fail "the gap of the test's own could not be measured: cargo named no test executable of the library"
  exe=$(sed -n 's/.*"name":"macos_stall_fixture".*"executable":"\([^"]*\)".*/\1/p' "$work/build.json")
  [ -n "$exe" ] || fail "cargo named no executable for the test target macos_stall_fixture"
  case $exe in
    *"
"*) fail "cargo named more than one executable for the test target macos_stall_fixture" ;;
  esac
  [ -f "$exe" ] || fail "the test executable that cargo named does not exist"

  form=${HALVO_STALL_FORM:-short}
  case $form in
    short | long) ;;
    *) fail "HALVO_STALL_FORM is short or long, not $form" ;;
  esac
  HALVO_STALL_FORM=$form
  export HALVO_STALL_FORM

  "$exe" --list >"$work/list.txt" 2>&1 || fail "the test executable did not list its tests: exited $?"
  sed -n 's/: test$//p' "$work/list.txt" | sort >"$work/listed" || fail "the test list cannot be read"
  cut -d ' ' -f 1 "$work/expected" | sort >"$work/expected-names" || fail "the expected list cannot be read"
  missing=$(comm -23 "$work/expected-names" "$work/listed" | tr '\n' ' ')
  [ -z "$missing" ] || fail "the test executable does not list these tests of this script's list: ${missing% }"
  extra=$(comm -13 "$work/expected-names" "$work/listed" | tr '\n' ' ')
  [ -z "$extra" ] || fail "the test executable lists tests that this script's list does not hold: ${extra% }"

  : >"$artifacts/ms.txt" || fail "ms.txt cannot be written"
  "$exe" --ignored --exact --nocapture --test-threads=1 "$form_test" </dev/null >"$work/form.txt" 2>&1 ||
    fail "$form_test did not print the form: exited $?"
  form_line=$(tr -d '\r' <"$work/form.txt" | sed -n 's/^form=/form=/p' | tail -n 1)
  case $form_line in
    "form=$form "*) ;;
    *) fail "$form_test printed no line of the form $form" ;;
  esac
  printf 'MS %s\n' "$form_line" | tee -a "$artifacts/ms.txt" || fail "ms.txt cannot be written"
  tr -d '\r' <"$work/form.txt" | sed -n 's/^left_out /MS left out: /p' | tee -a "$artifacts/ms.txt" ||
    fail "ms.txt cannot be written"
  measure_gap
  notices=$(grep -c '^measure notice' "$work"/gap-*-lines.txt | awk -F: '{ n += $NF } END { print n + 0 }')
  passed=0
  known=0
  fixed=0
  failed=0
  not_driven=0
  not_kept=0
  index=0
  while read -r test kind only; do
    case $kind in
      not-run | twelve) continue ;;
    esac
    case $form:$kind:$only in
      short:measured:* | short:loop:* | short:*:long)
        line="MS left out: $test kind=$kind form=long run=not_in_this_form"
        [ "$kind" != measured ] || line="$line asserts=nothing"
        printf '%s\n' "$line" | tee -a "$artifacts/ms.txt" || fail "ms.txt cannot be written"
        note=$(notes | sed -n "s/^$test //p")
        [ -z "$note" ] || printf 'MS note %s: %s\n' "$test" "$note" | tee -a "$artifacts/ms.txt" || fail "ms.txt cannot be written"
        continue
        ;;
    esac
    index=$((index + 1))
    log=$work/test-$index.log
    started=$(date +%s)
    case $form:$kind in
      short:*) limit=150 ;;
      long:measured | long:loop) limit=1500 ;;
      *) limit=900 ;;
    esac
    limit=${limit_override:-$limit}
    run_test "$test" "$log" "$limit"
    code=$?
    seconds=$(($(date +%s) - started))
    tr -d '\r' <"$log" | grep -E "^$test (session_reader=|gap_ms=|fixture=|round=|control=|verdict=|notdriven |notcounted |late_click |entries |drops |side=)|^$test: " | sed 's/^/    | /'
    found=$(tr -d '\r' <"$log" | grep -c -E "^$test (late_click |drops .* sleep=[1-9])")
    notices=$((notices + found))
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
    line="MS $test result=$verdict${three:+ verdict=$three} seconds=$seconds limit=$limit"
    [ "$kind" != measured ] || line="$line asserts=nothing"
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
  seconds=$(($(date +%s) - check_started))
  if [ "$form" = short ]; then
    [ "$seconds" -le "$budget_s" ] ||
      printf 'MS the short form took %s s, above its budget of %s s; this fails nothing (ruling MA-23 Addition 1 (5))\n' \
        "$seconds" "$budget_s" | tee -a "$artifacts/ms.txt" || fail "ms.txt cannot be written"
    line="MS form=short seconds=$seconds budget=$budget_s notices=$notices"
  else
    line="MS form=long seconds=$seconds notices=$notices"
  fi
  printf '%s\n' "$line" | tee -a "$artifacts/ms.txt" || fail "ms.txt cannot be written"
  [ "$fixed" -eq 0 ] || fail "$fixed listed known faults passed (FIXED-BUT-LISTED): take each out of this script's list"
  [ "$failed" -eq 0 ] || fail "$failed tests failed"
  ledger_check
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
    STUB_LIB=$work/bin/halvo_capture-stub
    STUB_CALLS=$work/calls-$case_name
    MS_EXPECTED_FILE=$work/expected-small
    MS_TEST_LIMIT_S=${limit:-}
    MS_NOTES_FILE=$work/notes-small
    export PATH CI_D_REPO CI_D_ARTIFACTS STUB_SPEC STUB_EXE STUB_LIB STUB_CALLS MS_EXPECTED_FILE MS_TEST_LIMIT_S MS_NOTES_FILE
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
  'rustc --locked -p halvo-capture --lib --profile check --message-format short --features stall-seam -- -C debug-assertions=off --cfg halvo_proof="seam_in_a_release_build_twin"')
    spec seam-twin-fails && exit 101
    exit 0
    ;;
  'rustc --locked -p halvo-capture --lib --profile check --message-format short --features stall-seam -- -C debug-assertions=off --cfg halvo_proof="seam_in_a_release_build"')
    spec seam-proof-builds && exit 0
    spec seam-proof-other-code && { printf 'error[E0308]: mismatched types\n'; exit 101; }
    printf 'error[E0599]: no function named `with_seam`\n'
    exit 101
    ;;
  'test --locked -p halvo-capture --lib --test macos_stall_fixture --no-run --message-format json')
    spec test-build-fails && exit 101
    spec no-library || printf '{"reason":"compiler-artifact","target":{"kind":["lib"],"crate_types":["lib"],"name":"halvo_capture","test":true},"executable":"%s","fresh":true}\n' "$STUB_LIB"
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
  cat >"$work/bin/halvo_capture-stub" <<'EOF'
#!/bin/sh
spec() { grep -q "^$1\$" "$STUB_SPEC"; }
ok() { printf 'test result: ok. 1 passed; 0 failed; 0 ignored\n'; exit 0; }
case $4 in
  macos::input::tests::the_gap_of_the_test_from_its_variable)
    case $HALVO_STALL_GAP_MS in
      600) printf 'gap_of_the_test refused source=HALVO_STALL_GAP_MS reason=HALVO_STALL_GAP_MS of 600 ms is above the cap of 500 ms\n' ;;
      *) printf 'gap_of_the_test gap_ms=%s source=HALVO_STALL_GAP_MS spacing_max_us=- load1=-\n' "$HALVO_STALL_GAP_MS" ;;
    esac
    ok
    ;;
  macos::input::tests::measure_the_readings_of_the_hold) ;;
  *) exit 96 ;;
esac
spec measure-exits && { printf 'test result: FAILED. 0 passed; 1 failed; 0 ignored\n'; exit 101; }
spec measure-none && ok
spec measure-gap-alone && { printf 'gap_of_the_test gap_ms=190 source=measured spacing_max_us=90474 load1=1.00\n'; ok; }
calls=$(cat "$STUB_CALLS" 2>/dev/null || printf 0)
calls=$((calls + 1))
printf '%s\n' "$calls" >"$STUB_CALLS"
if spec measure-refused; then
  printf 'measure dictionary=1 on_console=1 seconds=20.0 readings=12 spacing_max_us=90474\n'
  printf 'gap_of_the_test refused source=measured reason=12 readings, fewer than 300\n'
  ok
fi
if spec measure-under-30 ; then
  printf 'measure dictionary=1 on_console=1 seconds=20.0 readings=701 spacing_max_us=5000\n'
  printf 'gap_of_the_test gap_ms=30 source=measured spacing_max_us=5000 load1=1.00\n'
  ok
fi
if spec measure-above-twice || { spec measure-above-once && [ "$calls" = 1 ]; }; then
  printf 'measure dictionary=1 on_console=1 seconds=20.0 readings=701 spacing_max_us=260000 run=%s\n' "$calls"
  printf 'gap_of_the_test above_the_cap gap_ms=520 cap_ms=500 source=measured spacing_max_us=260000 load1=1.00\n'
  ok
fi
printf 'measure dictionary=1 on_console=1 seconds=20.0 readings=701 spacing_max_us=90474\n'
spec measure-notice && printf 'measure notice for the coordinator: spacing_p50_us=60000 readings=200\n'
printf 'gap_of_the_test gap_ms=190 source=measured spacing_max_us=90474 load1=1.00\n'
ok
EOF
  cat >"$work/bin/stall-stub" <<'EOF'
#!/bin/sh
spec() { grep -q "^$1\$" "$STUB_SPEC"; }
if [ "$*" = --list ]; then
  spec missing-test || printf 'the_probe: test\n'
  printf 'the_form_of_the_hosted_phases: test\nphase_one: test\nphase_two: test\nknown_one: test\nthree_one: test\nthree_long: test\nlaunch_one: test\nmeasured_one: test\nloop_one: test\n'
  spec extra-test && printf 'unlisted: test\n'
  exit 0
fi
test=$5
if [ "$test" = the_form_of_the_hosted_phases ]; then
  spec no-form-line && exit 0
  case $HALVO_STALL_FORM in
    long) printf 'form=long waits_ms_and_bounds_ms=150:1000 rounds_for_each_wait=3 cap_for_each_wait=4\n' ;;
    *)
      printf 'form=short waits_ms_and_bounds_ms=150:1000 rounds_for_each_wait=1 cap_for_each_wait=2\n'
      printf 'left_out counted rounds 2 to 3 of each wait of each mended phase\n'
      ;;
  esac
  exit 0
fi
[ -x "$HALVO_STALL_FIXTURE" ] || exit 98
[ -n "${HALVO_STALL_GAP_MS:-}" ] && [ -n "${HALVO_STALL_FORM:-}" ] || exit 99
handed=$(env | sed -n 's/^\(HALVO_STALL_[A-Z_]*\)=.*/\1/p' | sort | tr '\n' ' ')
printf '%s gap_ms=%s form=%s handed=%s\n' "$test" "$HALVO_STALL_GAP_MS" "$HALVO_STALL_FORM" "$handed"
spec "late-click-$test" && printf '%s late_click wait_ms=150 ahead=8\n' "$test"
spec "sleep-$test" && printf '%s drops round=0 on=1 gap=0 sleep=1 stop=0\n' "$test"
case $test in
  measured_* | loop_* | three_long)
    if [ "$HALVO_STALL_FORM" != long ]; then
      printf "thread 'x' panicked at y:\n%s: ran in the short form\ntest result: FAILED. 0 passed; 1 failed; 0 ignored\n" "$test"
      exit 101
    fi
    ;;
esac
printf '%s round=0 wait_ms=150 bound_ms=1000 counted=1 ahead=0 markers=2 plain=2 secure=8\n' "$test"
spec "touch-$test" && printf 'changed\n' >>"$CI_D_REPO/tracked.txt"
spec "hang-$test" && sleep 5
spec "fail-$test" && { printf "thread 'x' panicked at y:\n%s: rounds that did not count: wait 150 ms\ntest result: FAILED. 0 passed; 1 failed; 0 ignored\n" "$test"; exit 101; }
spec "leak-$test" && { printf "thread 'x' panicked at y:\n%s: recorded keys beside the markers in rounds that counted: wait 150 ms: 3\ntest result: FAILED. 0 passed; 1 failed; 0 ignored\n" "$test"; exit 101; }
spec "empty-$test" && { printf 'test result: ok. 0 passed; 0 failed; 1 ignored\n'; exit 0; }
spec "crash-$test" && { printf 'test result: FAILED. 0 passed; 1 failed; 0 ignored\n'; exit 134; }
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
  chmod +x "$work/bin/cargo" "$work/bin/clang" "$work/bin/stall-stub" "$work/bin/halvo_capture-stub" || exit 2
}

write_measured_ledger() {
  ledger_file=$work/repo/docs/baseline/ledger.d/macos-2b.txt
  mkdir -p "$work/repo/docs/baseline/ledger.d" "$work/repo/crates/halvo-capture/src/macos" \
    "$work/repo/crates/halvo-capture/tests/macos_stall_fixture" "$work/repo/scripts/r1/ci.d" || exit 2
  measured_files | while read -r path; do
    [ -f "$work/repo/$path" ] || printf '%s\n' "$path" >"$work/repo/$path" || exit 2
    git -C "$work/repo" add -- "$path" || exit 2
  done
  printf '%s\n' "${ledger_run_line:-measured run=1 form=long result=pass gap_ms=190 mended_rounds_held=60 commit=0 figures=none}" >"$ledger_file" || exit 2
  measured_files | while read -r path; do
    entry=$(git -C "$work/repo" ls-files --stage -- "$path") || exit 2
    blob=${entry#* }
    printf 'measured blob path=%s id=%s\n' "$path" "${blob%% *}"
  done >>"$ledger_file" || exit 2
  git -C "$work/repo" add docs || exit 2
}

self_test() {
  unset HALVO_STALL_GAP_MS HALVO_STALL_FORM MS_BUDGET_S
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name-self-test.XXXXXX") || {
    printf 'FAIL self-test:%s (mktemp failed)\n' "$name"
    exit 1
  }
  mkdir -p "$work/bin" "$work/repo/target/ci-artifacts/12-test" || exit 2
  write_stubs
  printf 'the_probe not-run\nthe_form_of_the_hosted_phases not-run\nphase_one phase\nphase_two phase\nknown_one known-fault\nthree_one three-way\nthree_long three-way long\nlaunch_one twelve\nmeasured_one measured\nloop_one loop\n' >"$work/expected-small"
  printf 'phase_two shows what it shows; it does not show the rest\n' >"$work/notes-small"
  git -C "$work/repo" init -q || exit 2
  printf 'tracked\n' >"$work/repo/tracked.txt" || exit 2
  mkdir -p "$work/repo/crates/halvo-capture" "$work/repo/crates/other" || exit 2
  printf '[features]\nstall-seam = []\n' >"$work/repo/crates/halvo-capture/Cargo.toml" || exit 2
  printf '[dependencies]\n' >"$work/repo/crates/other/Cargo.toml" || exit 2
  git -C "$work/repo" add tracked.txt crates || exit 2
  write_measured_ledger
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
  check measured-named-not-run PASS 'MS left out: measured_one kind=measured form=long run=not_in_this_form asserts=nothing' ''
  check loop-asserts PASS 'MS left out: loop_one kind=loop form=long run=not_in_this_form
MS ' ''
  check loop-named-not-run PASS 'MS left out: loop_one kind=loop form=long run=not_in_this_form' ''
  check long-three-way-not-run PASS 'MS left out: three_long kind=three-way form=long run=not_in_this_form' ''
  check form-line-first PASS 'MS form=short waits_ms_and_bounds_ms=150:1000 rounds_for_each_wait=1 cap_for_each_wait=2
MS left out: counted rounds 2 to 3 of each wait of each mended phase
measure dictionary=1 on_console=1 seconds=20.0 readings=701 spacing_max_us=90474
MS gap_ms=190 source=measured spacing_max_us=90474 load1=1.00' ''
  check form-line-missing FAIL 'the_form_of_the_hosted_phases printed no line of the form short' 'no-form-line'
  check the-word-alone PASS '| phase_one gap_ms=190 form=short handed=HALVO_STALL_FIXTURE HALVO_STALL_FORM HALVO_STALL_GAP_MS ' ''
  check budget-line PASS 'budget=240 notices=0' ''
  check notice-of-the-measurement PASS 'budget=240 notices=1' 'measure-notice'
  check notice-of-a-late-click PASS 'budget=240 notices=1' 'late-click-phase_one'
  check notice-of-sleep PASS 'budget=240 notices=1' 'sleep-phase_two'
  MS_BUDGET_S=0
  export MS_BUDGET_S
  check over-the-budget PASS 'above its budget of 0 s; this fails nothing' ''
  unset MS_BUDGET_S
  check gap-measured-again PASS 'the gap of the measurement is above its cap: measured again (ruling MA-23 Addition 1 (4))
measure dictionary=1 on_console=1 seconds=20.0 readings=701 spacing_max_us=90474
MS gap_ms=190 source=measured' 'measure-above-once'
  check gap-above-the-cap-twice FAIL "the runner's readings came further apart than the hosted proof is stated for: measure dictionary=1 on_console=1 seconds=20.0 readings=701 spacing_max_us=260000 run=1 / measure dictionary=1 on_console=1 seconds=20.0 readings=701 spacing_max_us=260000 run=2; run the job again" 'measure-above-twice'
  check gap-refused FAIL "the gap of the test's own could not be measured: 12 readings, fewer than 300" 'measure-refused'
  check gap-under-30 PASS 'MS gap_ms=30 source=measured spacing_max_us=5000' 'measure-under-30'
  check gap-no-measure-line FAIL "measure_the_readings_of_the_hold printed no gap line" 'measure-none'
  check gap-line-without-a-measure-line FAIL "measure_the_readings_of_the_hold printed no measure line" 'measure-gap-alone'
  check limit-of-the-short-form PASS ' limit=150' ''
  check gap-measurement-exits FAIL "the gap of the test's own could not be measured: measure_the_readings_of_the_hold exited 101" 'measure-exits'
  check gap-no-library FAIL "cargo named no test executable of the library" 'no-library'
  check no-phase-without-a-gap FAIL 'MS form=short' 'measure-refused'
  HALVO_STALL_GAP_MS=200
  export HALVO_STALL_GAP_MS
  check gap-from-the-variable PASS 'MS gap_ms=200 source=HALVO_STALL_GAP_MS spacing_max_us=- load1=-' ''
  check gap-of-the-variable-reaches-the-phases PASS '| phase_one gap_ms=200 form=short' ''
  HALVO_STALL_GAP_MS=600
  check gap-variable-above-the-cap FAIL 'HALVO_STALL_GAP_MS of 600 ms is above the cap of 500 ms' ''
  unset HALVO_STALL_GAP_MS
  HALVO_STALL_FORM=long
  export HALVO_STALL_FORM
  check long-form PASS 'MS total=7 pass=6 known_fault=1 notdriven=0 notkept=0 fail=0' ''
  check long-form-measured-line PASS ' limit=1500 asserts=nothing' ''
  check limit-of-a-long-phase PASS ' limit=900' ''
  check long-form-line PASS 'MS form=long seconds=' ''
  check long-form-reaches-the-phases PASS '| phase_one gap_ms=190 form=long' ''
  HALVO_STALL_FORM=medium
  check form-refused FAIL 'HALVO_STALL_FORM is short or long, not medium' ''
  unset HALVO_STALL_FORM
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
  check listed-test-missing FAIL 'does not list these tests of this script'"'"'s list: the_probe' 'missing-test'
  check known-fault-other-exit FAIL 'known_one: exit 134, and not the exit 101 of one failed test' 'crash-known_one'
  check known-fault-runs-no-test FAIL 'known_one: exit 0, and not one test ran and passed' 'empty-known_one'
  check twins-fail FAIL 'the twins of the proofs did not build' 'twins-fail'
  check proofs-build FAIL 'the proofs built' 'proofs-build'
  check proof-fails-with-another-code FAIL 'did not fail with E0599' 'proof-other-code'
  check one-error-for-two-proofs FAIL '(1 errors E0599 for 2 proofs)' 'proof-one-e0599'
  check seam-twin-fails FAIL 'the twin of the seam proof did not build with the feature stall-seam' 'seam-twin-fails'
  check seam-proof-builds FAIL 'the seam proof built with the feature stall-seam' 'seam-proof-builds'
  check seam-proof-other-code FAIL 'the seam proof with the feature stall-seam did not fail with E0599' 'seam-proof-other-code'
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
  printf '[features]\nstall-seam = []\n' >"$work/repo/crates/halvo-capture/Cargo.toml" || exit 2
  git -C "$work/repo" add crates || exit 2
  write_measured_ledger
  check ledger-holds PASS 'ledger: the measured run 1 of the long form ran on the blobs that the head has of the nine files of the hold' ''
  printf 'changed\n' >>"$work/repo/crates/halvo-capture/src/secure_input_macos_hold.rs" || exit 2
  git -C "$work/repo" add crates || exit 2
  check ledger-blob-changed FAIL 'ran on a blob of crates/halvo-capture/src/secure_input_macos_hold.rs other than the head'"'"'s' ''
  check ledger-fix-text FAIL 'run the long form (HALVO_STALL_FORM=long) on a scratch push of this head, then record that run with form=long, result=pass' ''
  write_measured_ledger
  grep -v '^measured run=' "$work/repo/docs/baseline/ledger.d/macos-2b.txt" >"$work/ledger-without-run" || exit 2
  cp "$work/ledger-without-run" "$work/repo/docs/baseline/ledger.d/macos-2b.txt" || exit 2
  check ledger-names-no-run FAIL 'names no measured run' ''
  write_measured_ledger
  grep -v 'path=crates/halvo-capture/tests/macos_stall_fixture/fixture.m ' "$work/repo/docs/baseline/ledger.d/macos-2b.txt" >"$work/ledger-four" || exit 2
  cp "$work/ledger-four" "$work/repo/docs/baseline/ledger.d/macos-2b.txt" || exit 2
  check ledger-four-files FAIL 'records the blobs of files other than the nine of the hold' ''
  write_measured_ledger
  printf 'measured run=2 form=long result=pass commit=0 figures=none\n' >>"$work/repo/docs/baseline/ledger.d/macos-2b.txt" || exit 2
  check ledger-two-runs FAIL 'names more than one measured run (1 2)' ''
  ledger_run_line='measured run=1 form=short result=pass gap_ms=190 commit=0 figures=none'
  write_measured_ledger
  check ledger-run-of-the-short-form FAIL 'names the measured run 1 without form=long and result=pass' ''
  ledger_run_line='measured run=1 form=long result=fail gap_ms=190 commit=0 figures=none'
  write_measured_ledger
  check ledger-run-that-failed FAIL 'names the measured run 1 without form=long and result=pass' ''
  ledger_run_line=
  write_measured_ledger
  printf 'changed\n' >>"$work/repo/scripts/r1/ci.d/51-macos-stall.sh" || exit 2
  git -C "$work/repo" add scripts || exit 2
  check ledger-ninth-file-changed FAIL 'ran on a blob of scripts/r1/ci.d/51-macos-stall.sh other than the head' ''
  write_measured_ledger
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
