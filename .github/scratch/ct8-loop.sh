#!/bin/sh
# Scratch only (control-2, the round of ruling CT-8): runs each test that the round added or changed
# many times, one process for each run, and prints its count of passes. A test that this system does
# not build is named as such. Exit 1 when any run failed.
set -u
out=$(cargo test -p halvo-control --locked --lib --no-run 2>&1 | tr -d '\r') || {
  printf '%s\n' "$out" | tail -n 20
  exit 1
}
bin=$(printf '%s\n' "$out" | sed -n 's/.*Executable unittests src.lib\.rs (\(.*\))$/\1/p' | tail -n 1 | tr '\\' '/')
[ -n "$bin" ] || { echo "loop: no test binary in the output of cargo"; exit 1; }
listed=$("$bin" --list 2>/dev/null | tr -d '\r')
failed=0
while read -r count name; do
  [ -n "$name" ] || continue
  if ! printf '%s\n' "$listed" | grep -q -x -F "$name: test"; then
    printf 'loop %s: not built on this system\n' "$name"
    continue
  fi
  pass=0
  i=0
  start=$(date +%s)
  while [ "$i" -lt "$count" ]; do
    if "$bin" --exact "$name" --test-threads 1 -q >/dev/null 2>&1; then
      pass=$((pass + 1))
    else
      printf 'loop %s: run %s failed\n' "$name" "$((i + 1))"
    fi
    i=$((i + 1))
  done
  printf 'loop %s: %s of %s passed in %s s\n' "$name" "$pass" "$count" "$(($(date +%s) - start))"
  [ "$pass" = "$count" ] || failed=$((failed + 1))
done <<'EOF'
200 client::tests::a_cap_set_whose_status_breaks_a_rule_of_the_type_is_its_own_error
200 client::tests::a_set_timeout_that_fails_after_a_reply_frame_ends_the_wait_and_reads_nothing_more
20 client::tests::a_whole_reply_frame_whose_last_byte_comes_as_the_deadline_passes_is_the_reply
200 client::tests::an_interrupted_read_in_the_wait_after_a_reply_frame_goes_on_with_the_time_that_is_left
200 client::tests::the_wait_after_a_reply_frame_never_passes_the_request_s_deadline
200 client_unix::tests::a_fifo_at_the_data_directory_path_is_an_io_error_with_exit_1_at_once_through_the_connector
200 client_unix::tests::an_open_error_of_the_lock_read_is_an_io_error_with_exit_1_through_the_connector
20 client_unix::tests::the_in_use_answer_comes_no_later_than_the_deadline_and_one_step
200 client_unix::tests::a_lock_that_is_freed_during_the_wait_gives_unreachable
200 client_unix::tests::a_peer_that_sends_its_whole_reply_and_keeps_the_connection_gives_the_reply_inside_the_deadline
200 ctl_text::tests::the_deleted_and_the_already_deleted_replies_render_their_fixtures_with_not_followed_0_put_into_c2_deleted_until_gate_3_adds_it
200 ctl_text::tests::pause_and_resume_exit_0_only_when_the_reply_reaches_the_verb_s_end_and_the_state_file_was_written
200 instance_lock::tests::a_read_of_a_directory_that_cannot_be_opened_is_an_error_and_never_no_directory
200 instance_lock::tests::an_entry_that_is_no_directory_at_the_data_directory_path_is_an_error_at_once
200 instance_lock::tests::two_reads_side_by_side_both_answer_free
200 instance_lock::tests::every_one_of_200_takes_succeeds_against_a_loop_of_reads
200 proto::tests::a_frame_of_another_version_is_unsupported_version_only_when_the_decoder_reads_all_of_it
200 proto::tests::the_delete_rule_reads_the_two_ids_in_the_order_of_ruling_pl_8
200 service::login_item_tests::a_fifo_or_a_socket_at_the_login_item_path_reads_stale_at_once
200 service::login_item_tests::an_entry_put_in_place_of_the_login_item_after_the_look_is_not_taken_and_never_waited_on
200 service::login_item_tests::the_two_opens_of_an_install_are_an_error_at_once_on_a_planted_entry
200 texts::tests::an_impaired_source_line_counts_as_running_in_recorded
200 texts::tests::e9_recorded_beside_a_stopped_or_failed_keyboard_or_screen_line_is_pinned_as_the_head_gives_it
200 texts::tests::e9_the_password_sentence_of_windows_with_no_guard_beside_a_running_keyboard_is_pinned_as_the_head_gives_it
20 wire::tests::one_decode_call_holds_at_most_its_named_peak_of_live_bytes_and_frees_them_on_both_sides
EOF
printf 'loop: %s tests with a failed run\n' "$failed"
[ "$failed" = 0 ]
