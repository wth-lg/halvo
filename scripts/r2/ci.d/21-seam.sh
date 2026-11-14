#!/bin/sh
# jobs: windows
# rule: cargo test -p halvo-seam passes with the committed lockfile and shows the four conformance tests, the five tests of the checks of C5.13 item 12, the reduction test, the memory test, the two tests of set D's step back and the test of the matcher of the suite's controls as passed, each in the block of its own test file.
set -u
name=21-seam
tests='conformance:conformance_resume conformance:conformance_shredded conformance:conformance_filters conformance:conformance_store_changed conformance:conformance_check_store_changed_names_the_current_id conformance:conformance_check_open_while_open conformance:conformance_check_open_at_the_watermark_after_a_shred conformance:conformance_check_filtered_stops conformance:conformance_check_one_expiry_instant reduction:reduction_app_title memory:transport_memory conformance:conformance_filters_step_back conformance:conformance_set_d_holds_its_step_back_when_served suite_controls:an_expectation_holds_only_when_its_last_part_is_the_last_part_of_the_failure'

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

self_fail() {
  printf 'FAIL self-test:%s (%s)\n' "$name" "$1"
  exit 1
}

block() {
  awk -v f="$1" '
    { line = $0; sub(/^[ \t]+/, "", line) }
    line ~ /^(Running|Doc-tests) / {
      inside = (index(line, "Running tests/" f ".rs ") == 1 || index(line, "Running tests\\" f ".rs ") == 1)
      next
    }
    inside { print }
  ' "$2"
}

decide() {
  [ "$1" -eq 0 ] || fail "cargo test -p halvo-seam --locked exited $1"
  tr -d '\r' <"$2" >"$2.lf" || fail "the output of cargo test cannot be read"
  for pair in $tests; do
    file=${pair%%:*}
    test=${pair#*:}
    block "$file" "$2.lf" | grep -x "test $test \.\.\. ok" || fail "cargo test -p halvo-seam --locked does not show $test as passed in tests/$file.rs"
  done
  passed=$(grep -c -x 'test .* \.\.\. ok' "$2.lf")
  printf 'halvo-seam tests passed: %s, of them read by name: %s\n' "$passed" "$(printf '%s\n' "$tests" | wc -w | tr -d ' ')"
}

measure() {
  [ -n "${CI_D_REPO:-}" ] || fail "CI_D_REPO is not set"
  [ -n "${CI_D_ARTIFACTS:-}" ] || fail "CI_D_ARTIFACTS is not set"
  cd "$CI_D_REPO" || fail "the repository directory cannot be entered"
  if ! command -v cargo >/dev/null 2>&1; then
    [ "${CI:-}" = true ] && fail "cargo is not installed"
    printf 'SKIP %s (cargo is not installed)\n' "$name"
    exit 77
  fi
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail "mktemp failed"
  cargo test -p halvo-seam --locked >"$work/test.txt" 2>&1
  code=$?
  cp "$work/test.txt" "$CI_D_ARTIFACTS/cargo-test.txt" || fail "the output of cargo test cannot be saved"
  grep '^test result:' "$work/test.txt"
  decide "$code" "$work/test.txt"
  printf 'PASS %s\n' "$name"
}

passing_output() {
  cat <<'EOF'
   Compiling halvo-seam v0.0.0 (/work/halvo/crates/halvo-seam)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 5.08s
     Running unittests src/lib.rs (target/debug/deps/halvo_seam-4527526ae09af152)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/client.rs (target/debug/deps/client-e2a02076d245eaf8)

running 29 tests
test a_batch_outside_the_form_of_c5_3_stops_the_client_on_any_link ... ok
test a_batch_whose_watermark_falls_below_that_of_an_earlier_batch_stops_the_client ... ok
test a_frame_or_a_message_outside_the_protocol_stops_the_client_and_it_sends_nothing_more ... ok
test a_batch_that_breaks_a_rule_of_c5_7_stops_the_client ... ok
test a_read_whose_next_names_segment_zero_stops_the_client ... ok
test a_record_below_the_next_of_an_earlier_batch_stops_the_client ... ok
test a_refusal_that_c5_9_allows_for_the_message_sent_stays_a_refusal ... ok
test a_reply_to_open_cursor_with_an_expiry_entry_stops_the_client ... ok
test a_reply_to_open_cursor_outside_c5_6_stops_the_client ... ok
test after_bad_position_the_position_of_the_open_stays_readable ... ok
test after_store_changed_to_an_open_the_position_of_the_open_stays_readable ... ok
test after_store_error_the_position_of_the_last_batch_taken_stays_readable ... ok
test after_store_error_to_an_open_the_position_of_the_open_stays_readable ... ok
test bad_position_that_answers_a_close_is_a_failed_read ... ok
test an_expiry_out_of_rising_seg_order_or_with_a_seg_twice_stops_the_client ... ok
test bad_position_that_answers_a_read_is_a_failed_read ... ok
test bad_position_that_answers_an_open_at_the_zero_position_is_a_failed_read ... ok
test bad_request_that_answers_a_close_of_an_open_cursor_is_a_failed_read ... ok
test after_store_changed_to_a_read_the_position_of_the_last_batch_taken_stays_readable ... ok
test bad_request_that_answers_a_read_of_an_open_cursor_is_a_failed_read ... ok
test store_changed_that_answers_a_close_is_a_failed_read ... ok
test store_changed_or_bad_position_that_answers_an_analysis_write_is_a_failed_read ... ok
test store_changed_that_answers_an_open_with_a_null_store_is_a_failed_read ... ok
test store_changed_that_names_the_store_its_open_named_is_a_failed_read ... ok
test store_changed_that_names_the_store_of_the_open_cursor_to_a_read_is_a_failed_read ... ok
test the_errors_of_c5_10_leave_the_state_that_their_table_gives ... ok
test the_analysis_write_goes_out_as_to_bytes_wrote_it_and_its_reply_is_decoded ... ok
test the_client_reads_a_store_through_both_entries_alike ... ok
test a_reader_that_resumes_at_its_position_skips_and_repeats_no_record ... ok

test result: ok. 29 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.13s

     Running tests/conformance.rs (target/debug/deps/conformance-80bcebace094fcfe)

running 11 tests
test conformance_check_one_expiry_instant ... ok
test conformance_check_store_changed_names_the_current_id ... ok
test conformance_filters ... ok
test conformance_filters_step_back ... ok
test conformance_resume ... ok
test conformance_check_filtered_stops ... ok
test conformance_shredded ... ok
test conformance_store_changed ... ok
test conformance_check_open_at_the_watermark_after_a_shred ... ok
test conformance_set_d_holds_its_step_back_when_served ... ok
test conformance_check_open_while_open ... ok

test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 20.05s

     Running tests/frame.rs (target/debug/deps/frame-aec2a3e72eac0962)

running 8 tests
test a_failed_read_is_an_io_error ... ok
test a_frame_one_byte_long_leaves_its_last_byte_as_the_start_of_the_next_length ... ok
test a_frame_one_byte_short_ends_inside_the_frame ... ok
test a_frame_of_the_bound_is_taken_and_one_byte_over_it_is_refused_unread ... ok
test a_frame_split_at_every_byte_and_two_frames_in_one_read_are_read_whole ... ok
test a_length_of_zero_is_refused_and_no_payload_is_read ... ok
test the_end_of_the_stream_before_a_frame_is_no_error_and_inside_the_length_is_one ... ok
test the_example_of_c5_2_travels_after_its_four_length_bytes ... ok

test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s

     Running tests/memory.rs (target/debug/deps/memory-2ce0681e34376726)

running 1 test
test transport_memory ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.26s

     Running tests/message.rs (target/debug/deps/message-ed2d3f5802e48ee7)

running 16 tests
test a_key_twice_is_a_bad_frame_in_the_message_the_position_and_the_range ... ok
test a_missing_or_unknown_key_or_a_value_of_another_type_is_a_bad_frame ... ok
test a_key_of_another_message_is_a_bad_frame_also_when_its_value_is_null ... ok
test a_message_of_c5_4_with_an_illegal_value_is_a_bad_request ... ok
test a_server_error_keeps_its_rules ... ok
test a_number_outside_its_form_or_range_is_a_bad_frame ... ok
test a_server_message_outside_c5_5_is_refused ... ok
test a_watermark_or_a_segment_ordinal_outside_the_form_of_c5_3_is_refused ... ok
test an_array_in_place_of_an_object_is_a_bad_frame ... ok
test an_expiry_seg_above_2_63_minus_1_gets_the_fixed_error_of_an_expiry_entry ... ok
test keys_stand_in_any_order_with_json_white_space_and_escaped_keys ... ok
test the_client_messages_decode_and_write_back_byte_for_byte ... ok
test the_analysis_write_carries_the_bytes_of_to_bytes_unchanged ... ok
test the_typed_open_cursor_keeps_its_rules ... ok
test the_server_messages_of_c5_14_decode_and_write_back_byte_for_byte ... ok
test the_serde_forms_of_a_record_an_entry_and_an_expiry_entry_hold_no_value_in_their_errors ... ok

test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s

     Running tests/record.rs (target/debug/deps/record-80f25ae29b0277f7)

running 20 tests
test a_kind_outside_c4_4_is_refused ... ok
test a_key_written_with_an_escape_is_that_key ... ok
test a_number_outside_the_decimal_integer_form_is_refused ... ok
test a_key_twice_is_refused_in_the_record_in_p_and_in_site ... ok
test an_app_with_a_control_character_of_the_category_cc_is_taken_as_null ... ok
test an_array_in_place_of_an_object_is_refused ... ok
test an_unknown_key_is_refused_and_named_by_the_object_that_holds_it ... ok
test an_error_names_the_key_and_holds_no_value_of_the_record ... ok
test each_missing_key_is_named ... ok
test example_lines_of_c4_8_read_and_write_back_byte_for_byte ... ok
test every_payload_form_of_c4_3_is_read_and_written_back ... ok
test keys_stand_in_any_order_with_spaces_and_tabs_between_tokens ... ok
test lines_hold_one_line_and_count_a_long_one_without_holding_it ... ok
test lines_keep_a_line_of_16384_bytes_and_pass_over_one_of_16385 ... ok
test new_keeps_the_rules_of_the_reader ... ok
test reduced_records_are_read_and_written_back ... ok
test payload_keys_follow_the_kind ... ok
test the_id_has_the_form_of_a_uuid_v7 ... ok
test the_app_is_cut_to_its_file_name_and_kept_as_an_app_id ... ok
test the_line_reader_keeps_the_rules_of_c4_8 ... ok

test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s

     Running tests/reduction.rs (target/debug/deps/reduction-14fa013e95c66e19)

running 4 tests
test the_set_files_of_c6_parse ... ok
test the_reduction_serves_the_listed_keys_and_the_marks_and_nothing_else ... ok
test reduction_app_title ... ok
test set_reader_refusals_report_the_path_and_the_line ... ok

test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.18s

     Running tests/set_memory.rs (target/debug/deps/set_memory-bb1b4f3b074e4513)

running 1 test
test set_reader_memory ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.11s

     Running tests/set_reader.rs (target/debug/deps/set_reader-ed06e12a81efd24e)

running 4 tests
test a_set_path_that_is_no_regular_file_is_unreadable ... ok
test a_set_path_that_is_a_symbolic_link_is_unreadable_also_when_it_names_a_regular_file ... ok
test the_set_reader_takes_at_most_1024_bytes_and_one_more ... ok
test the_set_reader_refuses_a_fifo_as_unreadable_without_waiting ... ok

test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s

     Running tests/suite_controls.rs (target/debug/deps/suite_controls-d85fa6ae5030b25b)

running 50 tests
test an_expectation_holds_only_when_its_last_part_is_the_last_part_of_the_failure ... ok
test control_a_batch_of_another_store_than_its_open_reply ... ok
test control_a_cursor_opened_with_the_new_id_that_serves_other_entries ... ok
test control_a_kinds_list_out_of_the_order_of_c4_4_that_gets_bad_request ... ok
test control_a_next_above_the_watermark ... ok
test control_a_filtered_cursor_opened_past_the_zero_position_that_serves_from_the_start ... ok
test control_a_position_that_repeats_inside_a_batch ... ok
test control_a_read_before_the_replace_that_gets_closed ... ok
test control_a_read_that_never_reaches_the_watermark ... ok
test control_a_reply_frame_over_the_server_limit ... ok
test control_a_screen_frame_record_served ... ok
test a_server_whose_restart_adds_a_segment_passes ... ok
test control_a_server_that_goes_on_serving_after_the_replace ... ok
test a_server_that_ends_each_batch_at_a_segment_end_passes ... ok
test control_a_server_that_drops_the_last_record_of_set_c ... ok
test control_a_next_that_falls ... ok
test control_a_server_that_ignores_the_byte_bound ... ok
test control_a_server_that_repeats_the_record_at_a_stop ... ok
test control_a_server_that_serves_the_old_records_after_the_replace ... ok
test control_a_server_that_skips_the_record_at_a_stop ... ok
test control_a_server_without_look_ahead ... ok
test control_a_set_c_that_stops_one_batch_before_its_end ... ok
test control_a_set_c_too_short_to_stop_a_batch ... ok
test control_a_server_that_opens_inside_the_watermark_segment ... ok
test control_a_store_changed_after_the_replace_that_names_the_old_store ... ok
test control_a_server_that_refuses_a_position_past_the_end_of_an_inner_segment ... ok
test control_a_server_that_refuses_a_position_two_past_the_end_of_an_inner_segment ... ok
test control_a_watermark_that_falls ... ok
test control_an_empty_batch_whose_next_does_not_rise ... ok
test control_an_entry_at_next ... ok
test control_a_store_changed_that_names_the_old_store ... ok
test control_a_store_changed_whose_id_the_new_store_does_not_carry ... ok
test control_an_error_of_another_cause_than_store_changed ... ok
test control_an_error_of_another_cause_than_store_changed_after_the_replace ... ok
test control_an_expiry_instant_not_after_its_records ... ok
test control_an_expiry_out_of_rising_order ... ok
test control_an_expiry_that_misses_a_segment_of_its_records ... ok
test control_an_expiry_that_names_a_segment_with_no_record ... ok
test control_an_open_reply_frame_over_the_server_limit ... ok
test control_an_open_reply_of_another_store_than_the_one_asked_for ... ok
test control_an_open_reply_whose_next_is_not_the_position_asked_for ... ok
test control_an_entry_below_the_cursor_position ... ok
test control_an_open_reply_with_a_record ... ok
test control_an_open_reply_with_an_expiry_entry ... ok
test control_an_error_of_another_cause_than_bad_position ... ok
test control_replies_of_the_wrong_message ... ok
test control_another_store_id_at_the_zero_position_that_opens_a_cursor ... ok
test control_an_open_reply_whose_watermark_falls_below_the_reference_after_a_shred ... ok
test control_an_error_of_another_cause_than_bad_request_for_each_refused_message ... ok
test run_passes_a_conforming_server_and_fails_each_broken_one_in_the_case_that_shows_it ... ok

test result: ok. 50 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 6.13s

     Running tests/transport.rs (target/debug/deps/transport-5e3b8824c212752f)

running 20 tests
test a_close_after_a_replace_leaves_no_store_changed_for_the_next_cursor ... ok
test a_filter_that_leaves_every_line_out_still_reads_at_most_the_line_bound ... ok
test a_malformed_line_after_the_bound_is_met_by_the_batch_that_would_hold_it ... ok
test a_store_id_of_another_store_gets_store_changed_and_opens_no_cursor ... ok
test a_shredded_segment_serves_no_record_and_is_no_error ... ok
test a_malformed_line_gets_store_error_in_place_of_its_batch_and_closes_the_cursor ... ok
test messages_out_of_order_and_the_analysis_write_get_bad_request ... ok
test an_empty_segment_is_passed_in_the_same_read_and_named_in_no_expiry ... ok
test open_fails_on_a_missing_file_a_first_live_of_zero_and_a_refused_set ... ok
test positions_after_the_watermark_and_beyond_a_file_are_handled_by_c5_9 ... ok
test an_input_set_reduces_records_and_changes_no_position_expiry_or_next ... ok
test the_end_of_a_connection_after_a_replace_leaves_no_store_changed_for_the_next_one ... ok
test the_example_of_c5_14_travels_byte_for_byte ... ok
test the_filters_change_no_position_and_serve_marks_and_never_screen_frames ... ok
test a_read_that_the_byte_bound_stops_serves_every_line_once_and_both_entries_agree ... ok
test the_first_line_is_checked_when_a_read_starts_inside_its_file ... ok
test the_store_id_is_the_hash_of_every_file_in_list_order ... ok
test the_expiry_instant_is_the_first_line_floored_to_a_second_plus_fourteen_days ... ok
test the_read_bound_takes_262144_bytes_of_lines_and_not_one_more ... ok
test the_stream_entry_ends_at_end_of_file_and_after_a_bad_frame ... ok

test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.39s

   Doc-tests halvo_seam

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
EOF
}

verdict() {
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$3"
  if [ "$3" != "$2" ]; then
    wrong="$wrong $1"
  elif [ "$2" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

check() {
  passing_output >"$work/$1.base" || self_fail "the fixture cannot be written"
  sed "$4" "$work/$1.base" >"$work/$1.txt" || self_fail "the fixture of $1 cannot be made"
  out=$( (decide "$3" "$work/$1.txt") 2>&1)
  code=$?
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:halvo-seam tests passed: $due, of them read by name: $named") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  verdict "$1" "$2" "$got"
}

run_measure() {
  mkdir -p "$work/$1/bin" "$work/$1/artifacts" || self_fail "the directories of $1 cannot be made"
  search="$work/$1/bin"
  case $3 in
    no-cargo*) ;;
    *)
      printf '#!/bin/sh\nprintf "%%s\\n" %s >"%s"\ncat "%s"\nexit %s\n' "\"\$*\"" "$work/$1/arguments" "$5" "$3" >"$work/$1/bin/cargo" ||
        self_fail "the stub of $1 cannot be written"
      chmod +x "$work/$1/bin/cargo" || self_fail "the stub of $1 cannot be made executable"
      search="$search:$PATH"
      ;;
  esac
  out=$( (
    export PATH="$search" CI_D_ARTIFACTS="$work/$1/artifacts"
    if [ -n "$4" ]; then export CI_D_REPO="$4"; else unset CI_D_REPO; fi
    if [ "$3" = no-cargo-in-ci ]; then export CI=true; fi
    if [ "$3" = no-cargo ]; then unset CI; fi
    measure
  ) 2>&1)
  code=$?
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    "77:SKIP $name ("*")") got=SKIP ;;
    *) got=exit-$code ;;
  esac
  if [ -f "$work/$1/arguments" ]; then
    IFS= read -r arguments <"$work/$1/arguments" || arguments=
    [ "$arguments" = 'test -p halvo-seam --locked' ] || got="$got,other-arguments"
    cmp -s "$5" "$work/$1/artifacts/cargo-test.txt" || got="$got,no-saved-output"
  fi
  verdict "$1" "$2" "$got"
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || self_fail "mktemp failed"
  rejected=0
  wrong=
  cr=$(printf '\r')
  named=$(printf '%s\n' "$tests" | wc -w | tr -d ' ')
  due=$(passing_output | grep -c -x 'test .* \.\.\. ok')
  check all-passed PASS 0 's/^//'
  check crlf-lines PASS 0 "s/\$/$cr/"
  check windows-paths PASS 0 's|Running tests/|Running tests\\|; s|target/debug/deps/|target\\debug\\deps\\|'
  check cargo-test-fails FAIL 101 's/^//'
  check filters-missing FAIL 0 '/conformance_filters/d'
  check reduction-failed FAIL 0 's/test reduction_app_title ... ok/test reduction_app_title ... FAILED/'
  check memory-ignored FAIL 0 's/test transport_memory ... ok/test transport_memory ... ignored/'
  check resume-renamed FAIL 0 's/conformance_resume/conformance_resume_old/'
  check store-changed-with-a-suffix FAIL 0 's/conformance_store_changed ... ok/conformance_store_changed ... ok, 61 s/'
  check no-output FAIL 0 'd'
  check reduction-ok-only-in-another-file FAIL 0 's/^test reduction_app_title \.\.\. ok$/test reduction_app_title ... ignored/
s/^test conformance_filters \.\.\. ok$/&\
test reduction_app_title ... ok/'
  check memory-ok-only-in-another-file FAIL 0 '/^test transport_memory \.\.\. ok$/d
s/^test a_set_path_that_is_no_regular_file_is_unreadable \.\.\. ok$/&\
test transport_memory ... ok/'
  check conformance-ok-only-in-the-unit-tests FAIL 0 '/^test conformance_shredded \.\.\. ok$/d
s/^running 0 tests$/&\
test conformance_shredded ... ok/'
  check memory-ok-only-after-the-doc-tests-line FAIL 0 '/^test transport_memory \.\.\. ok$/d
s|Running tests/transport\.rs|Running tests/memory.rs|
s/^   Doc-tests halvo_seam$/&\
test transport_memory ... ok/'
  check cargo-test-exits-1 FAIL 1 's/^//'
  check cargo-test-killed FAIL 137 's/^//'
  check memory-ok-only-in-a-file-whose-name-extends-memory FAIL 0 's/^test transport_memory \.\.\. ok$/test transport_memory ... ignored/
s|Running tests/message\.rs|Running tests/memory_limits.rs|
s/^test an_expiry_seg_above_2_63_minus_1_gets_the_fixed_error_of_an_expiry_entry \.\.\. ok$/&\
test transport_memory ... ok/'
  check memory-ok-only-in-a-file-whose-name-extends-memory-with-windows-paths FAIL 0 's/^test transport_memory \.\.\. ok$/test transport_memory ... ignored/
s|Running tests/message\.rs|Running tests/memory_limits.rs|
s|Running tests/|Running tests\\|
s/^test an_expiry_seg_above_2_63_minus_1_gets_the_fixed_error_of_an_expiry_entry \.\.\. ok$/&\
test transport_memory ... ok/'
  check new-test-ignored FAIL 0 's/^test conformance_check_open_while_open \.\.\. ok$/test conformance_check_open_while_open ... ignored/'
  check new-test-absent FAIL 0 '/^test conformance_check_one_expiry_instant \.\.\. ok$/d'
  check new-test-ok-only-in-another-file FAIL 0 '/^test conformance_check_filtered_stops \.\.\. ok$/d
s/^test run_passes_a_conforming_server_and_fails_each_broken_one_in_the_case_that_shows_it \.\.\. ok$/&\
test conformance_check_filtered_stops ... ok/'
  passing_output >"$work/real.txt" || self_fail "the fixture cannot be written"
  : >"$work/empty.txt" || self_fail "the empty output cannot be written"
  run_measure measure-passes PASS 0 "$work" "$work/real.txt"
  run_measure measure-takes-the-failing-status FAIL 101 "$work" "$work/real.txt"
  run_measure measure-with-no-output FAIL 0 "$work" "$work/empty.txt"
  run_measure measure-in-a-missing-directory FAIL 0 "$work/missing" "$work/real.txt"
  run_measure measure-with-ci-d-repo-not-set FAIL 0 "" "$work/real.txt"
  run_measure measure-in-ci-with-no-cargo FAIL no-cargo-in-ci "$work" "$work/real.txt"
  run_measure measure-outside-ci-with-no-cargo SKIP no-cargo "$work" "$work/real.txt"
  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  '') measure ;;
  *)
    printf 'FAIL %s (unknown argument)\n' "$name"
    exit 64
    ;;
esac
