#!/bin/sh
# control-2d's loop step on a scratch commit alone: each test that the unit adds runs 200 times, counted by the
# result line with the count of tests (a run that shows another count, or none, is no pass).
#   sh loop-step.sh macos|windows
set +e
job=$1
rounds=${LOOP_ROUNDS:-200}
case $job in
  macOS) job=macos ;;
  Windows) job=windows ;;
esac
case $job in
  macos)
    lib_tests="client_unix::tests::ct15_ctl_prints_the_cause_at_the_data_folder_and_no_raw_os_error_at_the_name_and_below_it client_unix::tests::ct15_ctl_reads_a_data_folder_whose_name_is_longer_than_255_bytes_as_cannot_be_read ctl_text::tests::ct13_a_linked_data_directory_prints_its_resolved_folder_after_parent_and_a_plain_one_none ctl_text::tests::ct13_the_resolved_line_of_a_folder_with_a_line_break_is_one_line ctl_text::tests::ct14_a_plain_busy_an_already_deleted_not_taken_and_the_deadline_of_a_delete_say_what_did_not_happen ctl_text::tests::ct15_a_resolved_folder_whose_name_is_not_utf_8_prints_a_replacement_character ctl_text::tests::pr10_keys_under_secure_input_late_events_and_withheld_keys_print_yes_or_no_and_never_a_digit paths::tests::k5_one_line_turns_each_control_character_and_line_separator_into_a_question_mark_and_cuts_nothing paths::tests::p7_104_delete_all_leaves_a_saved_sign_in_and_the_account_helper_s_delete_takes_it_away paths::tests::st36_check::ct15_a_name_longer_than_255_bytes_reads_cannot_be_read_from_the_four_and_255_bytes_do_not paths::tests::st36_check::ct15_the_resolved_folder_and_its_line_read_a_name_that_is_not_utf_8_with_a_replacement_character paths::tests::st36_check::k1_a_lock_taken_on_the_link_s_name_with_a_slash_a_dot_or_two_slashes_is_through_a_link paths::tests::st36_check::k1_a_path_with_a_part_dot_dot_or_a_relative_path_is_refused_by_the_constructor_before_any_lock paths::tests::st36_check::k5_the_resolved_line_is_one_line_with_each_control_character_and_line_separator_as_a_question_mark proto::tests::ct13_a_mac_status_carries_the_resolved_folder_only_when_linked_and_a_windows_status_never proto::tests::pr10_a_count_of_late_events_or_of_keys_under_secure_input_is_refused_on_the_wire proto::tests::pr10_a_mac_reports_secure_input_keystrokes_as_a_yes_or_no_and_windows_reports_none proto::tests::pr10_the_status_reports_the_password_guard_with_its_bound_and_a_guard_without_one_is_refused testing::tests::ct13_the_scripted_daemon_s_own_timed_out_pause_and_its_own_stop_leave_its_life_as_it_was texts::tests::pr10_withheld_keys_on_a_mac_are_an_item_of_the_texts_that_is_no_warning_and_windows_shows_none"
    lib_count=20
    doc_filters="one_line InstanceLock"
    doc_count=3
    ;;
  windows)
    lib_tests="ctl_text::tests::ct13_a_linked_data_directory_prints_its_resolved_folder_after_parent_and_a_plain_one_none ctl_text::tests::ct13_the_resolved_line_of_a_folder_with_a_line_break_is_one_line ctl_text::tests::ct14_a_plain_busy_an_already_deleted_not_taken_and_the_deadline_of_a_delete_say_what_did_not_happen ctl_text::tests::pr10_keys_under_secure_input_late_events_and_withheld_keys_print_yes_or_no_and_never_a_digit paths::tests::k5_one_line_turns_each_control_character_and_line_separator_into_a_question_mark_and_cuts_nothing paths::tests::p7_104_delete_all_leaves_a_saved_sign_in_and_the_account_helper_s_delete_takes_it_away proto::tests::ct13_a_mac_status_carries_the_resolved_folder_only_when_linked_and_a_windows_status_never proto::tests::pr10_a_count_of_late_events_or_of_keys_under_secure_input_is_refused_on_the_wire proto::tests::pr10_a_mac_reports_secure_input_keystrokes_as_a_yes_or_no_and_windows_reports_none proto::tests::pr10_the_status_reports_the_password_guard_with_its_bound_and_a_guard_without_one_is_refused testing::tests::ct13_the_scripted_daemon_s_own_timed_out_pause_and_its_own_stop_leave_its_life_as_it_was texts::tests::pr10_withheld_keys_on_a_mac_are_an_item_of_the_texts_that_is_no_warning_and_windows_shows_none"
    lib_count=12
    doc_filters="one_line"
    doc_count=1
    ;;
  *)
    echo "usage: sh loop-step.sh macos|windows"
    exit 64
    ;;
esac
mkdir -p target
log=target/control-2d-loop-step.log
cargo test -p halvo-control --locked >"$log" 2>&1
gate=$?
echo "gate: cargo test -p halvo-control --locked exited $gate"
tr -d '\r' <"$log" | grep -E '^test result|FAILED'
for name in $lib_tests; do
  tr -d '\r' <"$log" | grep -F "test $name ... " || echo "MISSING $name"
done
tr -d '\r' <"$log" | grep -E '^test .*(one_line|InstanceLock).* \.\.\. '
start=$(date +%s)
lib_pass=0
doc_pass=0
round=1
while [ "$round" -le "$rounds" ]; do
  # shellcheck disable=SC2086
  cargo test -p halvo-control --locked --lib -- --exact $lib_tests >"$log" 2>&1
  if tr -d '\r' <"$log" | grep -q "^test result: ok. $lib_count passed; 0 failed; 0 ignored;"; then
    lib_pass=$((lib_pass + 1))
  else
    echo "round $round: lib tests"
    tr -d '\r' <"$log" | grep -E '^test |^test result|panicked' | head -40
  fi
  # shellcheck disable=SC2086
  cargo test -p halvo-control --locked --doc -- $doc_filters >"$log" 2>&1
  if tr -d '\r' <"$log" | grep -q "^test result: ok. $doc_count passed; 0 failed; 0 ignored;"; then
    doc_pass=$((doc_pass + 1))
  else
    echo "round $round: doc tests"
    tr -d '\r' <"$log" | grep -E '^test |^test result' | head -20
  fi
  round=$((round + 1))
done
end=$(date +%s)
echo "loop $job: lib tests $lib_pass of $rounds rounds with $lib_count passed each; doc tests $doc_pass of $rounds rounds with $doc_count passed each; seconds $((end - start))"
[ "$lib_pass" = "$rounds" ] && [ "$doc_pass" = "$rounds" ]
