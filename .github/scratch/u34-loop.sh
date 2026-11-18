#!/bin/sh
# One round of the loop of unit U3.4 on a scratch branch: each test that the unit adds or changes runs by its name
# with --exact from the test binaries that cargo built for halvo-agent-store, and a round passes only when the
# result line of each binary shows the expected count passed and none failed. Prints counts alone.
set -u
dir="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/u34-loop"
list="$dir/binaries.txt"
mkdir -p "$dir" || exit 1
if [ ! -s "$list" ]; then
  cargo test -p halvo-agent-store --locked --no-run --message-format=json >"$dir/build.json" 2>"$dir/build.err" || {
    echo "loop-u34: build failed"
    exit 1
  }
  sed -n 's/.*"target":{[^}]*"name":"\([a-z_]*\)"[^}]*"test":true}.*"executable":"\([^"]*\)".*/\1 \2/p' "$dir/build.json" |
    sed 's#\\\\#/#g' >"$list.new" || exit 1
  mv "$list.new" "$list" || exit 1
fi
round=0
check() {
  target=$1
  shift
  want=$#
  bin=$(awk -v t="$target" '$1 == t { print $2; exit }' "$list") || bin=
  if [ -z "$bin" ]; then
    echo "loop-u34: no test binary for $target"
    exit 1
  fi
  "$bin" --exact "$@" >"$dir/out.txt" 2>&1
  if ! grep -q "^test result: ok\. $want passed; 0 failed; 0 ignored; 0 measured; [0-9]* filtered out" "$dir/out.txt"; then
    echo "loop-u34: $target did not pass $want tests"
    tail -n 30 "$dir/out.txt"
    exit 1
  fi
  round=$((round + want))
}
check after_end \
  a_load_meets_a_session_part_s_absence_at_its_bound \
  after_a_clock_set_back_by_a_day_an_ended_run_still_refuses_a_new_effect_and_answers_a_repeat
check checkpoint_bodies \
  a_made_row_dated_before_its_planned_row_is_refused_and_stores_nothing
check coordinator \
  a_configuration_stored_later_does_not_move_the_start_row_and_load_answers_it \
  a_repeated_create_agent_stores_the_start_row_only_when_none_stands \
  a_start_run_whose_start_row_lies_past_the_log_s_bound_does_not_open_the_run_again \
  create_agent_after_a_stop_behind_its_commit_and_sent_again_with_a_new_agent_id \
  create_agent_stores_the_start_row_in_the_commit_of_the_preset_from_the_request_s_clock \
  load_answers_start_null_where_no_start_row_of_the_agent_stands
check memory \
  the_kind_table_is_the_family_agent
check refusal \
  a_releasing_local_edit_is_decided_with_the_checkpoint_of_its_cleared_first_row \
  a_releasing_row_under_a_newest_configuration_that_gives_refuse_is_refused_and_stores_nothing
check replies \
  a_begun_row_after_the_drafted_row_that_follows_begun_meets_the_same_keeper \
  a_begun_row_is_refused_while_another_agent_holds_begun_committed_or_unknown \
  a_begun_row_is_stored_while_another_agent_holds_drafted_lost_or_withheld \
  a_begun_row_keeps_the_draft_of_its_drafted_row_and_committed_or_unknown_keep_the_begun_row \
  a_drafted_or_a_lost_row_after_begun_is_refused_and_stores_nothing \
  a_drafted_row_after_begun_is_refused_unless_its_effect_s_newest_row_is_not_made \
  a_drafted_row_after_lost_needs_a_new_draft_and_no_effect \
  a_first_row_that_is_not_drafted_is_refused_and_stores_nothing \
  a_repeat_of_the_asking_agent_s_own_begun_row_is_answered_as_the_first_whatever_others_hold \
  a_repeat_of_the_newest_row_is_answered_as_the_first_and_stores_nothing \
  a_reply_row_is_bound_to_its_agent_s_workflow_and_started_open_run \
  a_reply_row_is_held_to_the_day_of_its_at_and_to_the_clock \
  a_row_that_skips_or_repeats_a_seq_is_refused \
  a_run_s_end_leaves_every_other_row_as_it_stands \
  after_a_run_s_end_its_reply_rows_are_refused \
  an_id_whose_only_row_is_another_agent_s_begun_committed_or_unknown_is_blocked_and_no_key_of_it_leaves \
  another_agent_s_drafted_or_lost_row_is_answered_nowhere \
  another_agent_s_withheld_row_is_in_blocked_withheld_unless_one_holds_a_sent_state \
  asked_for_25_ids_of_776_bytes_whose_rows_cannot_fit_the_handler_returns_no_shorter_list \
  asked_for_25_ids_that_are_all_blocked_the_handler_answers_every_one \
  end_run_leaves_a_begun_row_of_its_run_unknown \
  every_row_after_committed_or_unknown_is_refused_and_stores_nothing \
  every_row_after_withheld_is_refused_and_stores_nothing \
  finish_leaves_a_begun_row_of_its_run_unknown \
  load_replies_answers_the_newest_own_row_of_each_id_in_the_order_asked \
  load_replies_is_bound_to_a_started_open_run_of_its_agent \
  of_two_agents_with_drafted_rows_the_second_begun_row_is_refused_in_either_order \
  reply_row_stores_a_first_drafted_row \
  the_closed_order_of_one_message_is_stored_row_by_row \
  the_ids_are_blocked_also_with_the_other_agent_paused_and_with_its_workflow_not_stored \
  the_keeper_and_the_list_blocked_agree \
  the_keeper_does_not_order_two_agents_rows_by_their_instants \
  the_rows_of_one_agent_are_ordered_by_seq_and_two_agents_rows_are_not_ordered_by_their_instants \
  the_unknown_rows_of_a_run_s_end_are_in_the_one_commit_of_its_record \
  withheld_as_a_first_row_after_begun_and_after_lost_is_refused \
  withheld_is_allowed_after_a_first_drafted_row \
  withheld_is_allowed_after_the_drafted_row_that_follows_lost \
  withheld_is_refused_after_the_drafted_row_that_follows_begun \
  withheld_keeps_the_draft_and_the_run_of_its_drafted_row
check runner \
  a_repeat_of_a_stored_stopped_row_stores_nothing_and_is_answered_with_its_count \
  put_playbook_answers_a_write_to_a_file_that_is_not_the_slot_workbook_with_playbook \
  the_bodies_reply_row_and_load_replies_are_answered_by_their_handlers
if [ "$round" -ne 54 ]; then
  echo "loop-u34: $round tests passed, not 54"
  exit 1
fi
echo "loop-u34: 54 tests in 7 binaries passed"
