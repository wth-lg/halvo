#!/bin/sh
# jobs: windows
# rule: every Windows functional test runs in its own process, and a listed defect must fail with its marker
set -u
name=50-windows-functional
test_limit_s=${WF_TEST_LIMIT_S:-360}
timeout_tool=${WF_TIMEOUT_TOOL:-timeout}
failing_fixtures=27
shutdown_line='TF shutdown source='
fixtures_at_once=4
LC_ALL=C
export LC_ALL
cr=$(printf '\r')
nl='
'

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

expected_tests() {
  cat <<'EOF'
a_dropped_keyboard_source_stops_capturing
a_timed_out_stop_reads_no_input_posted_after_its_answer
browser::a_renamed_copy_of_chrome_reads_unknown_and_records_nothing
browser::a_renamed_copy_of_firefox_reads_unknown_and_records_nothing
browser::cold_and_warm_orders_in_chrome
browser::cold_and_warm_orders_in_edge
browser::stall_phases_of_the_page_in_chrome
browser::stall_phases_of_the_page_in_edge
browser::swap_phases_of_the_page_in_chrome
browser::swap_phases_of_the_page_in_edge
c1_raw_input_delivers_keystrokes_and_pointer_events
c2_screen_source_delivers_a_frame
c3_foreground_source_reports_an_activated_window
c4_window_title_source_reports_a_title_change
c5_installed_software_returns_registry_entries
c8_display_affinity_exclude_from_capture_is_set_and_read_back
c8b_window_destroyed_after_the_last_capture_session
c9_peak_working_set_of_sources_1_to_4_for_60_seconds
capture_times_follow_the_input_order
helper_child_window
helper_limited_listener
keys_queued_before_a_stop_are_delivered_or_counted
known_defect::browser_burst_whose_first_letter_swaps_in_chrome W3-7-Q2i
known_defect::browser_burst_whose_first_letter_swaps_in_edge W3-7-Q2i
known_defect::browser_letter_and_tab_out_of_a_swapped_field_in_chrome W3-7-C5
known_defect::browser_letter_and_tab_out_of_a_swapped_field_in_edge W3-7-C5
known_defect::browser_self_move_after_a_held_main_thread_in_chrome W3-5-A2
known_defect::browser_self_move_after_a_held_main_thread_in_edge W3-5-A2
known_defect::browser_swaps_on_a_held_main_thread_in_chrome W3-7-Q2i
known_defect::browser_swaps_on_a_held_main_thread_in_edge W3-7-Q2i
known_defect::c5_watch_armed_while_the_per_user_key_is_absent RR-windows-12
known_defect::stall_no_focus_key_and_the_program_moves_the_focus_itself W3-5-A2
known_defect::x16_packaged_apps_and_the_inventory RR-windows-12
password_field_fixture_chrome
password_field_fixture_edge
password_field_fixture_firefox
password_field_fixture_native_dialog_phases_a_to_f
pointer_moves_coalesce
stall::a_focus_notice_for_a_field_that_does_not_take_the_focus_records_no_letter
stall::a_second_tab_out_of_the_password_field_then_a_letter_records_nothing
stall::an_edit_masked_while_it_has_the_focus_records_no_letter_after_the_mask
stall::click_then_letters_while_the_program_serves_sent_messages_150_ms
stall::click_then_letters_while_the_program_serves_sent_messages_2000_ms
stall::click_then_letters_while_the_program_serves_sent_messages_200_ms
stall::click_then_letters_while_the_program_serves_sent_messages_300_ms
stall::click_then_letters_while_the_program_serves_sent_messages_600_ms
stall::click_then_letters_while_the_program_sleeps_150_ms
stall::click_then_letters_while_the_program_sleeps_2000_ms
stall::click_then_letters_while_the_program_sleeps_200_ms
stall::click_then_letters_while_the_program_sleeps_300_ms
stall::click_then_letters_while_the_program_sleeps_600_ms
stall::enter_with_no_focus_move_then_five_letters_records_no_letter
stall::five_letters_of_the_plain_field_after_the_programs_own_focus_move_are_recorded
stall::helper_stall_fixture
stall::tab_and_letters_while_a_window_of_another_thread_comes_to_the_front
stall::tab_tab_and_a_letter_across_a_second_field_while_the_program_waits_after_the_first_tab
stall::tab_then_letters_while_the_program_serves_sent_messages_150_ms
stall::tab_then_letters_while_the_program_serves_sent_messages_2000_ms
stall::tab_then_letters_while_the_program_serves_sent_messages_200_ms
stall::tab_then_letters_while_the_program_serves_sent_messages_300_ms
stall::tab_then_letters_while_the_program_serves_sent_messages_600_ms
stall::tab_then_letters_while_the_program_sleeps_150_ms
stall::tab_then_letters_while_the_program_sleeps_2000_ms
stall::tab_then_letters_while_the_program_sleeps_200_ms
stall::tab_then_letters_while_the_program_sleeps_300_ms
stall::tab_then_letters_while_the_program_sleeps_600_ms
stall::the_program_moves_the_focus_in_two_steps_and_no_letter_is_recorded
stop_with_a_full_sink
the_fault_line_allocates_nothing
the_first_own_read_after_a_start_reads_the_focused_password_field
title_event_on_a_switch_to_a_window_with_a_fixed_title
x12_screen_source_fifteen_start_stop_cycles
x13_frames_on_a_static_screen
x14_foreground_app_of_a_hosted_app
x15_a_held_key
x1_keystrokes_typed_into_a_password_field
x2_window_title_events_for_other_windows_and_controls
x3_attribution_before_the_first_foreground_change
x5_screen_source_stop_and_start_again
x7_capture_border_on_the_screen
x8_raw_input_across_the_integrity_boundary
x9_screen_source_across_a_display_mode_change
EOF
}

scan_log() {
  summary=
  panics=0
  panic_lines=
  markers=0
  marker=
  before_marker=
  timed_out=no
  prev=
  while IFS= read -r line || [ -n "$line" ]; do
    while :; do
      case $line in
        *"$cr"*) line=${line%%"$cr"*}${line#*"$cr"} ;;
        *) break ;;
      esac
    done
    printf '    | %s\n' "$line"
    case $line in
      'test result: '*) summary=${line#'test result: '} ;;
    esac
    case $line in
      "thread '"*" panicked at "*)
        panics=$((panics + 1))
        panic_lines=$panic_lines$line$nl
        ;;
    esac
    case $line in
      'KNOWN-DEFECT '*)
        markers=$((markers + 1))
        if [ "$markers" -eq 1 ]; then
          marker=$line
          before_marker=$prev
        fi
        ;;
    esac
    case $line in
      'TF '*' result=TIMEOUT'*) timed_out=yes ;;
    esac
    prev=$line
  done <"$1"
}

judge() {
  verdict=FAIL
  id=
  cause=
  scan_log "$3"
  case $1 in
    known_defect::*) ;;
    *)
      if [ "$2" -ne 0 ]; then
        cause="exit $2"
        return 0
      fi
      case $summary in
        'ok. 1 passed; 0 failed;'*) ;;
        *)
          cause='exit 0, and not one test ran and passed'
          return 0
          ;;
      esac
      if [ "$panics" -gt 0 ]; then
        cause='exit 0 with a panic on a thread of the test'
        return 0
      fi
      verdict=pass
      return 0
      ;;
  esac
  if [ "$2" -eq 0 ]; then
    case $summary in
      'ok. 1 passed; 0 failed;'*)
        verdict=FIXED-BUT-LISTED
        cause='the listed defect passed'
        ;;
      *) cause='exit 0, and not one test ran and passed' ;;
    esac
    return 0
  fi
  if [ "$markers" -eq 0 ]; then
    cause="exit $2 without a KNOWN-DEFECT line"
    return 0
  fi
  if [ "$markers" -gt 1 ]; then
    cause="exit $2 with more than one KNOWN-DEFECT line"
    return 0
  fi
  if [ "$2" -ne 101 ]; then
    cause="exit $2 with a KNOWN-DEFECT line, and not the exit 101 of one failed test"
    return 0
  fi
  case $summary in
    'FAILED. 0 passed; 1 failed;'*) ;;
    *)
      cause='exit 101 with a KNOWN-DEFECT line and no summary line of one failed test'
      return 0
      ;;
  esac
  got=${marker#KNOWN-DEFECT }
  case $got in
    '' | *[!A-Za-z0-9-]*)
      cause='a KNOWN-DEFECT line that holds more than the marker'
      return 0
      ;;
  esac
  if [ "$got" != "$4" ]; then
    cause="the marker names $got, and the list of this script gives ${4:-no id} for the test"
    return 0
  fi
  owner=
  case $before_marker in
    "thread '"*"'"*" panicked at "*)
      owner=${before_marker#"thread '"}
      owner=${owner%%"'"*}
      ;;
  esac
  if [ -z "$owner" ]; then
    cause='a KNOWN-DEFECT line that no panic line comes right before'
    return 0
  fi
  others=0
  rest=$panic_lines
  while [ -n "$rest" ]; do
    panic=${rest%%"$nl"*}
    rest=${rest#*"$nl"}
    case $panic in
      *"thread '$owner'"*) ;;
      *) others=$((others + 1)) ;;
    esac
  done
  if [ "$others" -gt 0 ]; then
    cause="a panic on another thread than the thread $owner of the marker"
    return 0
  fi
  if [ "$timed_out" = yes ]; then
    cause='the test reached its time limit'
    return 0
  fi
  verdict='known-defect'
  id=$got
}

run_test() {
  "$timeout_tool" "$test_limit_s" "$exe" --ignored --exact --nocapture "$1" </dev/null >"$2" 2>&1
}

measure() {
  cd "$CI_D_REPO" || exit 2
  artifacts=${CI_D_ARTIFACTS:-$CI_D_REPO/target/ci-artifacts/$name}
  mkdir -p "$artifacts" || fail "the artifacts directory cannot be made"
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail "mktemp failed"
  command -v "$timeout_tool" >/dev/null 2>&1 || fail "$timeout_tool is not installed, and each test runs under its time limit"
  if [ -n "${WF_EXPECTED_FILE:-}" ]; then
    tr -d '\r' <"$WF_EXPECTED_FILE" >"$work/expected" || fail "the expected list cannot be read"
  else
    expected_tests >"$work/expected"
  fi
  cut -d ' ' -f 1 "$work/expected" | sort >"$work/expected-names" || fail "the expected list cannot be read"

  cargo test --locked -p halvo-capture --test windows_functional --no-run --message-format json >"$work/build.json" ||
    fail "the test build failed: cargo test --no-run exited $?"
  exe=$(sed -n 's/.*"name":"windows_functional".*"executable":"\([^"]*\)".*/\1/p' "$work/build.json")
  [ -n "$exe" ] || fail "cargo named no executable for the test target windows_functional"
  case $exe in
    *"
"*) fail "cargo named more than one executable for the test target windows_functional" ;;
  esac
  exe=$(printf '%s\n' "$exe" | sed 's/\\\\/\//g')
  [ -f "$exe" ] || fail "the test executable that cargo named does not exist"

  "$exe" --list >"$work/list.txt" 2>&1 || fail "the test executable did not list its tests: exited $?"
  tr -d '\r' <"$work/list.txt" | sed -n 's/: test$//p' >"$work/names" || fail "the test list cannot be read"
  total=$(grep -c '' "$work/names")
  [ "$total" -gt 0 ] || fail "the test list is empty"
  if grep -v -q '^[A-Za-z0-9_:]*$' "$work/names"; then
    fail "the test list holds a name with a character outside A-Z, a-z, 0-9, _ and :"
  fi
  sort "$work/names" >"$work/listed-names" || fail "the test list cannot be sorted"
  distinct=$(sort -u "$work/names" | grep -c '')
  [ "$distinct" -eq "$total" ] || fail "the test executable lists a test more than once"
  missing=$(comm -23 "$work/expected-names" "$work/listed-names" | tr '\n' ' ')
  [ -z "$missing" ] || fail "the test executable does not list these tests of this script's list: ${missing% }"
  extra=$(comm -13 "$work/expected-names" "$work/listed-names" | tr '\n' ' ')
  [ -z "$extra" ] || fail "the test executable lists tests that this script's list does not hold: ${extra% }"

  : >"$artifacts/wf.txt" || fail "wf.txt cannot be written"
  passed=0
  known=0
  fixed=0
  failed=0
  index=0
  while IFS= read -r test; do
    index=$((index + 1))
    log=$work/test-$index.log
    want_id=$(awk -v test="$test" '$1 == test { print $2 }' "$work/expected")
    run_test "$test" "$log"
    code=$?
    judge "$test" "$code" "$log" "$want_id"
    case $verdict in
      pass) passed=$((passed + 1)) ;;
      known-defect) known=$((known + 1)) ;;
      FIXED-BUT-LISTED) fixed=$((fixed + 1)) ;;
      *) failed=$((failed + 1)) ;;
    esac
    [ -z "$cause" ] || printf '%s: %s: %s\n' "$name" "$test" "$cause"
    line="WF $test result=$verdict${id:+ id=$id}"
    printf '%s\n' "$line"
    printf '%s\n' "$line" >>"$artifacts/wf.txt" || fail "wf.txt cannot be written"
  done <"$work/names"

  for source in keyboard pointer window_title foreground screen; do
    not_ok=0
    for log in "$work"/test-*.log; do
      [ -f "$log" ] || continue
      found=$(tr -d '\r' <"$log" | grep -c "$shutdown_line$source ok=false")
      not_ok=$((not_ok + found))
    done
    line="shutdown_not_ok source=$source count=$not_ok"
    printf '%s\n' "$line"
    printf '%s\n' "$line" >>"$artifacts/wf.txt" || fail "wf.txt cannot be written"
  done

  line="WF total=$total pass=$passed known_defect=$known fail=$((failed + fixed))"
  printf '%s\n' "$line"
  printf '%s\n' "$line" >>"$artifacts/wf.txt" || fail "wf.txt cannot be written"
  [ "$fixed" -eq 0 ] || fail "$fixed listed defects passed (FIXED-BUT-LISTED): move each test out of mod known_defect"
  [ "$failed" -eq 0 ] || fail "$failed tests failed"
  printf 'PASS %s\n' "$name"
}

launch() {
  printf '%s\n' "$3" >"$work/spec-$1"
  if [ "$expected" = real ]; then
    expected_file=
  else
    expected_file=$work/expected-$1
    printf '%s\n' "$expected" >"$expected_file"
  fi
  (
    PATH=$work/bin:$PATH
    CI_D_REPO=$work/repo
    CI_D_ARTIFACTS=$work/artifacts-$1
    STUB_SPEC=$work/spec-$1
    STUB_TF=$work/bin/tf-stub
    WF_EXPECTED_FILE=$expected_file
    STUB_SHUTDOWN_LINE=$test_file_shutdown_line
    export PATH CI_D_REPO CI_D_ARTIFACTS STUB_SPEC STUB_TF WF_EXPECTED_FILE STUB_EXE STUB_LIST_EXIT
    export STUB_SHUTDOWN_LINE
    if [ -n "$limit" ]; then
      WF_TEST_LIMIT_S=$limit
      export WF_TEST_LIMIT_S
    fi
    if [ -n "$tool" ]; then
      WF_TIMEOUT_TOOL=$tool
      export WF_TIMEOUT_TOOL
    fi
    sh "$0" >"$work/out-$1" 2>&1
    printf '%s\n' "$?" >"$work/code-$1"
  ) &
  running=$((running + 1))
  if [ "$running" -ge "$fixtures_at_once" ]; then
    wait
    running=0
  fi
}

check() {
  case_name=$1
  want=$2
  if [ "$pass" = launch ]; then
    launch "$@"
    reset_knobs
    return 0
  fi
  out=$(cat "$work/out-$case_name") || exit 2
  code=$(cat "$work/code-$case_name") || exit 2
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  shift 3
  for text in "$@"; do
    case $text in
      '!'*)
        case $out in
          *"${text#!}"*) got=$got-with-a-text-it-must-not-hold ;;
        esac
        ;;
      *)
        case $out in
          *"$text"*) ;;
          *) got=$got-without-the-text ;;
        esac
        ;;
    esac
  done
  printf 'case %s/%s want=%s got=%s\n' "$name" "$case_name" "$want" "$got"
  if [ "$got" != "$want" ]; then
    wrong="$wrong $case_name"
  elif [ "$want" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
  reset_knobs
}

reset_knobs() {
  expected=$small
  STUB_EXE=one
  STUB_LIST_EXIT=0
  limit=
  tool=
}

write_stubs() {
  cat >"$work/bin/cargo" <<'EOF'
#!/bin/sh
[ "$*" = 'test --locked -p halvo-capture --test windows_functional --no-run --message-format json' ] || exit 97
path=$(printf '%s\n' "$STUB_TF" | sed 's/\//\\\\/g')
artifact() {
  printf '{"reason":"compiler-artifact","target":{"kind":["test"],"crate_types":["bin"],"name":"windows_functional","test":true},"executable":"%s","fresh":true}\n' "$1"
}
printf '{"reason":"compiler-artifact","target":{"kind":["lib"],"name":"halvo_capture"},"executable":null,"fresh":true}\n'
case $STUB_EXE in
  one) artifact "$path" ;;
  two)
    artifact "$path"
    artifact "$path"
    ;;
  missing) artifact "$path-gone" ;;
  none) ;;
  *) exit 96 ;;
esac
printf '{"reason":"build-finished","success":true}\n'
EOF
  cat >"$work/bin/tf-stub" <<'EOF'
#!/bin/sh
if [ "$*" = --list ]; then
  count=0
  while IFS='|' read -r test kind id; do
    [ -n "$test" ] || continue
    printf '%s: test\r\n' "$test"
    count=$((count + 1))
  done <"$STUB_SPEC"
  printf '\n%s tests, 0 benchmarks\n' "$count"
  exit "$STUB_LIST_EXIT"
fi
[ "$#" -eq 4 ] && [ "$1 $2 $3" = '--ignored --exact --nocapture' ] || exit 97
test=$4
line=
while IFS= read -r spec; do
  case $spec in
    *"$test|"*)
      line=$spec
      break
      ;;
  esac
done <"$STUB_SPEC"
rest=${line#*|}
kind=${rest%%|*}
id=
case $rest in
  *'|'*)
    id=${rest#*|}
    id=${id%%|*}
    ;;
esac
thread=tf-${test##*::}
ok="test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out; finished in 0.01s"
failed="test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 3 filtered out; finished in 0.01s"
panic() {
  printf "\nthread '%s' (4242) panicked at crates\\\\halvo-capture\\\\tests\\\\windows_functional.rs:1:1:\r\n%s\r\n" "$1" "$2" >&2
}
printf '\nrunning 1 test\n'
case $kind in
  pass)
    printf 'TF %s source_built_by_test=keyboard\ntest %s ... ok\n\n%s\n\n' "$test" "$test" "$ok"
    exit 0
    ;;
  shutdown-not-ok)
    printf '%skeyboard ok=false\ntest %s ... ok\n\n%s\n\n' "$STUB_SHUTDOWN_LINE" "$test" "$ok"
    exit 0
    ;;
  fakes)
    printf 'WF ghost_test_not_in_the_file result=pass\nWF %s result=known-defect id=RR-windows-99\n' "$test"
    printf 'WF total=99 pass=99 known_defect=0 fail=0\nPASS 50-windows-functional\n'
    printf 'test %s ... ok\n\n%s\n\n' "$test" "$ok"
    exit 0
    ;;
  defect | wrong-id | two-markers | marker-139 | marker-124 | marker-no-summary | other-thread | timeout-line | marker-more | marker-mid | marker-no-panic)
    printf 'TF %s keystroke_events_while_typing_into_password_field=5\r\n' "$test"
    case $kind in
      other-thread) panic "$thread-helper" 'called `Result::unwrap()` on an `Err` value' ;;
      timeout-line) printf 'TF %s result=TIMEOUT limit_secs=60\n' "$test" ;;
    esac
    case $kind in
      wrong-id) panic "$thread" 'KNOWN-DEFECT RR-windows-99' ;;
      two-markers)
        panic "$thread" "KNOWN-DEFECT $id"
        printf '%s\n' "KNOWN-DEFECT $id" >&2
        ;;
      marker-more) panic "$thread" "KNOWN-DEFECT $id and more" ;;
      marker-mid) panic "$thread" "assertion failed: KNOWN-DEFECT $id was expected" ;;
      marker-no-panic) printf '%s\n' "KNOWN-DEFECT $id" ;;
      *) panic "$thread" "KNOWN-DEFECT $id" ;;
    esac
    case $kind in
      marker-139) exit 139 ;;
      marker-124) exit 124 ;;
      marker-no-summary) exit 101 ;;
    esac
    printf 'test %s ... FAILED\n\nfailures:\n    %s\n\n%s\n\n' "$test" "$test" "$failed"
    exit 101
    ;;
  inconclusive)
    panic "$thread" 'INCONCLUSIVE: SendInput inserted no events on this machine'
    printf 'test %s ... FAILED\n\n%s\n\n' "$test" "$failed"
    exit 101
    ;;
  zero)
    printf '\nrunning 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 4 filtered out; finished in 0.00s\n\n'
    exit 0
    ;;
  pass-then-139)
    printf 'test %s ... ok\n\n%s\n\n' "$test" "$ok"
    exit 139
    ;;
  pass-with-panic)
    panic "$thread-helper" 'called `Option::unwrap()` on a `None` value'
    printf 'test %s ... ok\n\n%s\n\n' "$test" "$ok"
    exit 0
    ;;
  hang) exec sleep 30 ;;
  *) exit 98 ;;
esac
EOF
  chmod +x "$work/bin/cargo" "$work/bin/tf-stub" || exit 2
}

self_test() {
  test_file=$(cd "$(dirname "$0")/../../.." && pwd)/crates/halvo-capture/tests/windows_functional.rs
  test_file_shutdown_line=$(sed -n 's/^const SHUTDOWN_LINE: &str = "\(.*\)";$/\1/p' "$test_file")
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  mkdir -p "$work/bin" "$work/repo" || exit 2
  write_stubs
  rejected=0
  wrong=
  small="c1_raw_input${nl}helper_child_window${nl}known_defect::x1_password RR-windows-01"
  right="c1_raw_input|pass${nl}helper_child_window|pass${nl}known_defect::x1_password|defect|RR-windows-01"
  reset_knobs
  running=0
  pass=launch
  fixtures
  wait
  pass=judge
  fixtures

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  if [ "$rejected" -ne "$failing_fixtures" ]; then
    printf 'FAIL self-test:%s (rejected %s failing fixtures, and the self-test holds %s)\n' "$name" "$rejected" "$failing_fixtures"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

fixtures() {
  check every-test-right PASS "$right" \
    "${nl}    | TF c1_raw_input source_built_by_test=keyboard${nl}" \
    "${nl}WF c1_raw_input result=pass${nl}" "${nl}WF helper_child_window result=pass${nl}" \
    "${nl}WF known_defect::x1_password result=known-defect id=RR-windows-01${nl}" \
    "${nl}WF total=3 pass=2 known_defect=1 fail=0${nl}PASS $name"
  real=$(expected_tests | awk '{ if (NF == 2) print $1 "|defect|" $2; else print $1 "|pass" }')
  real_total=$(expected_tests | grep -c '')
  real_known=$(expected_tests | awk 'NF == 2' | grep -c '')
  expected=real
  check every-test-of-the-real-list-right PASS "$real" \
    "${nl}WF known_defect::c5_watch_armed_while_the_per_user_key_is_absent result=known-defect id=RR-windows-12${nl}" \
    "${nl}shutdown_not_ok source=keyboard count=0${nl}" \
    "${nl}WF total=$real_total pass=$((real_total - real_known)) known_defect=$real_known fail=0${nl}PASS $name"
  check a-not-ok-shutdown-in-the-form-of-the-test-file-is-counted PASS "c1_raw_input|shutdown-not-ok${nl}helper_child_window|pass${nl}known_defect::x1_password|defect|RR-windows-01" \
    "${nl}shutdown_not_ok source=keyboard count=1${nl}" \
    "${nl}WF total=3 pass=2 known_defect=1 fail=0${nl}PASS $name"
  check a-test-prints-the-lines-of-the-script PASS "c1_raw_input|fakes${nl}helper_child_window|pass${nl}known_defect::x1_password|defect|RR-windows-01" \
    "${nl}    | WF ghost_test_not_in_the_file result=pass${nl}" \
    "${nl}WF total=3 pass=2 known_defect=1 fail=0${nl}PASS $name" \
    "!${nl}WF ghost_test_not_in_the_file" "!${nl}WF total=99" "!${nl}PASS $name${nl}" \
    "!${nl}WF c1_raw_input result=known-defect"

  check kd-test-passes FAIL "c1_raw_input|pass${nl}helper_child_window|pass${nl}known_defect::x1_password|pass" \
    "${nl}WF known_defect::x1_password result=FIXED-BUT-LISTED${nl}" \
    "${nl}WF total=3 pass=2 known_defect=0 fail=1${nl}FAIL $name (1 listed defects passed (FIXED-BUT-LISTED): move each test out of mod known_defect)"
  check kd-test-fails-without-the-marker FAIL "c1_raw_input|pass${nl}helper_child_window|pass${nl}known_defect::x1_password|inconclusive" \
    "${nl}$name: known_defect::x1_password: exit 101 without a KNOWN-DEFECT line${nl}WF known_defect::x1_password result=FAIL${nl}" \
    "${nl}WF total=3 pass=2 known_defect=0 fail=1${nl}FAIL $name (1 tests failed)"
  check zero-tests-ran FAIL "c1_raw_input|zero${nl}helper_child_window|pass${nl}known_defect::x1_password|defect|RR-windows-01" \
    "${nl}$name: c1_raw_input: exit 0, and not one test ran and passed${nl}WF c1_raw_input result=FAIL${nl}" \
    "${nl}WF total=3 pass=1 known_defect=1 fail=1${nl}FAIL $name (1 tests failed)"
  check empty-list FAIL '' "FAIL $name (the test list is empty)"
  check control-failed FAIL "c1_raw_input|inconclusive${nl}helper_child_window|pass${nl}known_defect::x1_password|defect|RR-windows-01" \
    "${nl}$name: c1_raw_input: exit 101${nl}WF c1_raw_input result=FAIL${nl}" \
    "${nl}WF total=3 pass=1 known_defect=1 fail=1${nl}FAIL $name (1 tests failed)"
  check two-markers FAIL "c1_raw_input|pass${nl}helper_child_window|pass${nl}known_defect::x1_password|two-markers|RR-windows-01" \
    "${nl}$name: known_defect::x1_password: exit 101 with more than one KNOWN-DEFECT line${nl}WF known_defect::x1_password result=FAIL${nl}"
  check a-marker-inside-a-longer-line FAIL "c1_raw_input|pass${nl}helper_child_window|pass${nl}known_defect::x1_password|marker-mid|RR-windows-01" \
    "${nl}$name: known_defect::x1_password: exit 101 without a KNOWN-DEFECT line${nl}"
  check a-marker-with-more-on-its-line FAIL "c1_raw_input|pass${nl}helper_child_window|pass${nl}known_defect::x1_password|marker-more|RR-windows-01" \
    "${nl}$name: known_defect::x1_password: a KNOWN-DEFECT line that holds more than the marker${nl}"
  check a-marker-with-exit-139 FAIL "c1_raw_input|pass${nl}helper_child_window|pass${nl}known_defect::x1_password|marker-139|RR-windows-01" \
    "${nl}$name: known_defect::x1_password: exit 139 with a KNOWN-DEFECT line, and not the exit 101 of one failed test${nl}"
  check a-marker-with-exit-124 FAIL "c1_raw_input|pass${nl}helper_child_window|pass${nl}known_defect::x1_password|marker-124|RR-windows-01" \
    "${nl}$name: known_defect::x1_password: exit 124 with a KNOWN-DEFECT line, and not the exit 101 of one failed test${nl}"
  check a-marker-with-exit-101-and-no-summary-line FAIL "c1_raw_input|pass${nl}helper_child_window|pass${nl}known_defect::x1_password|marker-no-summary|RR-windows-01" \
    "${nl}$name: known_defect::x1_password: exit 101 with a KNOWN-DEFECT line and no summary line of one failed test${nl}"
  check a-regular-test-passes-then-exits-139 FAIL "c1_raw_input|pass-then-139${nl}helper_child_window|pass${nl}known_defect::x1_password|defect|RR-windows-01" \
    "${nl}$name: c1_raw_input: exit 139${nl}WF c1_raw_input result=FAIL${nl}"
  STUB_LIST_EXIT=3
  check the-list-call-fails FAIL "$right" "FAIL $name (the test executable did not list its tests: exited 3)"
  limit=1
  check a-test-hangs-past-its-limit FAIL "c1_raw_input|hang${nl}helper_child_window|pass${nl}known_defect::x1_password|defect|RR-windows-01" \
    "${nl}$name: c1_raw_input: exit 124${nl}WF c1_raw_input result=FAIL${nl}"
  STUB_EXE=none
  check cargo-names-no-executable FAIL "$right" "FAIL $name (cargo named no executable for the test target windows_functional)"
  STUB_EXE=two
  check cargo-names-two-executables FAIL "$right" "FAIL $name (cargo named more than one executable for the test target windows_functional)"
  STUB_EXE=missing
  check the-named-executable-is-missing FAIL "$right" "FAIL $name (the test executable that cargo named does not exist)"
  expected="c1-raw-input${nl}helper_child_window"
  check a-name-outside-the-alphabet FAIL "c1-raw-input|pass${nl}helper_child_window|pass" \
    "FAIL $name (the test list holds a name with a character outside A-Z, a-z, 0-9, _ and :)"
  check the-binary-lacks-a-test-of-the-list FAIL "c1_raw_input|pass${nl}helper_child_window|pass" \
    "FAIL $name (the test executable does not list these tests of this script's list: known_defect::x1_password)"
  check the-binary-lists-a-test-that-the-list-lacks FAIL "$right${nl}c99_new_test|pass" \
    "FAIL $name (the test executable lists tests that this script's list does not hold: c99_new_test)"
  check the-binary-lists-a-test-twice FAIL "$right${nl}c1_raw_input|pass" \
    "FAIL $name (the test executable lists a test more than once)"
  check a-marker-of-another-id FAIL "c1_raw_input|pass${nl}helper_child_window|pass${nl}known_defect::x1_password|wrong-id|RR-windows-01" \
    "${nl}$name: known_defect::x1_password: the marker names RR-windows-99, and the list of this script gives RR-windows-01 for the test${nl}"
  check a-panic-on-another-thread-than-the-marker FAIL "c1_raw_input|pass${nl}helper_child_window|pass${nl}known_defect::x1_password|other-thread|RR-windows-01" \
    "${nl}$name: known_defect::x1_password: a panic on another thread than the thread tf-x1_password of the marker${nl}"
  check a-time-limit-line-then-the-marker FAIL "c1_raw_input|pass${nl}helper_child_window|pass${nl}known_defect::x1_password|timeout-line|RR-windows-01" \
    "${nl}$name: known_defect::x1_password: the test reached its time limit${nl}"
  check a-marker-that-no-panic-line-comes-before FAIL "c1_raw_input|pass${nl}helper_child_window|pass${nl}known_defect::x1_password|marker-no-panic|RR-windows-01" \
    "${nl}$name: known_defect::x1_password: a KNOWN-DEFECT line that no panic line comes right before${nl}"
  check a-regular-test-passes-with-a-panic-on-another-thread FAIL "c1_raw_input|pass-with-panic${nl}helper_child_window|pass${nl}known_defect::x1_password|defect|RR-windows-01" \
    "${nl}$name: c1_raw_input: exit 0 with a panic on a thread of the test${nl}"
  tool=no-such-timeout-tool
  check the-time-limit-tool-is-missing FAIL "$right" "FAIL $name (no-such-timeout-tool is not installed, and each test runs under its time limit)"
}

case ${1:-} in
  --self-test) self_test ;;
  '') measure ;;
  *) exit 64 ;;
esac
