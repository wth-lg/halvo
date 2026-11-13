#!/bin/sh
# jobs: windows
# rule: The halvo-derived test binaries of the workspace's test build, which 12-test has built before it in the hosted job, pass, each test that this file lists passes under its test target, cargo tree -p halvo-derived -e features lists no bundled feature and no openssl-sys, and no tracked file differs from HEAD after the tests.
set -u
name=50-derived
own=scripts/r2/ci.d/50-derived.sh
package=/crates/halvo-derived#
case $0 in
  /*) self=$0 ;;
  *) self=$(pwd)/$0 ;;
esac

# The tests that must pass, read from the commit before any test runs: one line '#= <test target> <test name>'.
#= src/lib.rs tests::no_catalogued_statement_plans_a_temporary_b_tree
#= src/lib.rs tests::the_progress_handler_interrupts_a_statement_past_its_budget
#= src/lib.rs tests::a_setting_that_is_not_in_effect_is_refused
#= src/lib.rs hash::tests::the_site_hash_is_hmac_sha256
#= src/lib.rs tests::expire_removes_and_counts_a_bounded_row_with_no_day_which_the_report_reads_as_due_now
#= src/lib.rs tests::a_row_more_than_two_days_after_the_clock_is_due_now_and_one_two_days_after_it_is_not
#= src/lib.rs tests::the_store_s_own_connection_has_secure_delete_on_after_a_first_and_a_second_open
#= src/lib.rs tests::rows_read_hands_out_a_kept_and_a_window_row_with_no_day_and_no_bounded_row_without_one
#= tests/open.rs open_refuses_a_key_that_does_not_open_the_database
#= tests/open.rs open_refuses_while_another_store_holds_the_writer_lock
#= tests/open.rs the_database_refuses_another_connection_while_the_store_is_open
#= tests/open.rs open_refuses_entries_that_check_entries_reports
#= tests/open.rs open_creates_no_index_in_a_database_that_holds_tables
#= tests/open.rs the_database_holds_no_full_text_index_and_no_shared_memory_file
#= tests/open.rs open_removes_rows_that_name_a_summary_or_a_workflow_that_is_not_stored
#= tests/commit.rs commit_batch_stores_each_summary_and_its_evidence_with_the_position
#= tests/commit.rs commit_batch_hashes_the_title_token_with_the_key_of_its_own_database
#= tests/commit.rs commit_batch_refuses_a_batch_read_from_another_store
#= tests/commit.rs a_commit_at_or_before_the_committed_position_changes_nothing
#= tests/commit.rs commit_batch_stores_no_row_past_the_bound_of_its_class
#= tests/commit.rs an_episode_that_names_one_segment_twice_is_refused_by_its_reader
#= tests/purge.rs purge_removes_every_window_row_that_names_a_shredded_ordinal
#= tests/purge.rs purge_below_removes_every_window_row_below_the_first_live_ordinal
#= tests/purge.rs a_commit_stores_no_window_row_that_names_a_purged_ordinal
#= tests/purge.rs a_purge_at_the_highest_ordinal_refuses_nothing_and_keeps_the_ordinal_purged
#= tests/propose.rs a_proposal_of_the_recurrence_step_is_stored_as_a_proposed_workflow_with_its_support_rows
#= tests/propose.rs a_proposal_with_the_key_of_a_stored_workflow_updates_that_row_and_adds_none
#= tests/propose.rs workflow_ids_are_minted_in_the_order_of_category_and_then_lowest_summary_id
#= tests/propose.rs a_proposal_that_names_a_summary_of_another_key_is_refused_and_stores_nothing
#= tests/propose.rs the_support_rows_of_an_updated_workflow_are_those_of_its_latest_proposal_alone
#= tests/propose.rs a_proposal_that_names_a_stored_summary_outside_its_bound_is_refused_as_built
#= tests/propose.rs the_recurrence_step_reads_the_summaries_inside_their_bound_by_day_and_then_id
#= tests/propose.rs the_summary_read_leaves_out_a_summary_whose_stored_day_of_going_has_come
#= tests/propose.rs of_proposals_of_one_key_in_one_call_the_first_that_passes_stands_and_each_later_one_is_refused
#= tests/decide.rs while_a_dismissed_workflow_is_stored_no_proposal_with_its_key_is_stored
#= tests/decide.rs delete_removes_every_row_that_names_the_workflow_and_every_summary_of_its_key
#= tests/decide.rs delete_of_a_dismissed_workflow_removes_the_summaries_of_its_key
#= tests/decide.rs the_summaries_of_a_proposal_under_another_key_never_support_a_workflow_to_dismiss_and_delete
#= tests/decide.rs after_a_refused_proposal_under_another_key_a_batch_with_no_episode_stores_no_workflow
#= tests/decide.rs the_delete_removes_all_that_supported_a_workflow_in_every_state_and_every_history_of_its_support_set
#= tests/typed.rs a_step_after_the_recurrence_step_has_its_records_stored_by_the_typed_commit
#= tests/typed.rs the_typed_commit_refuses_the_whole_list_for_one_record_that_breaks_a_rule
#= tests/typed.rs the_typed_commit_refuses_a_record_of_origin_own_from_a_step_and_stores_it_for_a_runner_and_a_coordinator_write
#= tests/typed.rs deleting_a_workflow_leaves_no_row_of_a_registered_test_kind_that_names_it
#= tests/typed.rs without_the_workflow_key_in_its_entry_the_rows_of_a_test_kind_outlive_the_delete
#= tests/typed.rs a_row_stored_before_its_entry_gave_a_workflow_key_goes_with_its_workflow
#= tests/typed.rs an_open_refuses_an_entry_that_gives_a_stored_kind_another_class
#= tests/typed.rs an_open_refuses_an_entry_whose_workflow_key_names_another_id_than_a_stored_row
#= tests/typed.rs a_refused_open_changes_no_row_no_revision_and_no_value_of_meta
#= tests/typed.rs a_window_record_that_names_a_purged_ordinal_is_dropped_and_the_rest_of_the_list_applied
#= tests/typed.rs a_window_record_that_names_one_segment_twice_is_stored_once_and_goes_with_its_purge
#= tests/typed.rs a_handler_in_front_passes_the_request_on_and_commits_after_the_owning_handler
#= tests/typed.rs a_handler_s_typed_commit_refuses_the_list_for_a_bounded_day_after_the_clock_or_outside_its_bound
#= tests/typed.rs a_request_of_a_further_role_that_a_handler_passes_on_stores_no_record_of_origin_own
#= tests/typed.rs a_window_record_whose_expiry_has_passed_is_dropped_and_counted_by_a_handler_and_by_a_step
#= tests/typed.rs a_handler_s_row_read_leaves_out_a_bounded_row_from_the_first_instant_of_its_day_of_going
#= tests/typed.rs row_read_visits_the_rows_of_its_own_family_alone_in_byte_order
#= tests/state.rs the_state_call_moves_an_approved_workflow_among_approved_active_and_paused
#= tests/state.rs the_state_call_refuses_a_proposed_a_dismissed_and_a_missing_workflow_and_an_active_one_with_no_agent
#= tests/state.rs the_state_call_answers_no_workflow_for_a_workflow_that_is_not_stored_before_the_rules_of_the_move
#= tests/state.rs a_state_call_in_the_same_state_sets_the_agent_given_or_clears_it
#= tests/state.rs the_state_call_is_refused_while_a_handler_answers_a_request_of_another_role
#= tests/expiry.rs expiry_at_open_removes_each_bounded_row_past_its_bound_or_stamped_after_the_clock
#= tests/expiry.rs expiry_at_open_removes_each_window_row_whose_expiry_has_passed
#= tests/expiry.rs expiry_at_open_reads_the_bound_of_the_entry_that_names_the_kind
#= tests/expiry.rs expiry_at_open_removes_a_row_past_its_bound_of_a_kind_that_the_open_does_not_name
#= tests/expiry.rs the_store_gives_its_host_the_report_after_each_committed_transaction
#= tests/expiry.rs the_report_gives_the_deadline_of_the_bound_that_the_store_expires_by
#= tests/expiry.rs expire_at_the_reported_deadline_removes_a_row_and_one_nanosecond_before_it_none
#= tests/expiry.rs a_row_one_or_two_days_after_the_clock_stays_and_a_row_three_days_after_it_goes_at_the_next_expiry
#= tests/expiry.rs a_bounded_row_with_no_day_goes_at_the_next_expiry_and_a_kept_or_window_row_with_none_stays
#= tests/expiry.rs a_kind_of_a_new_family_beside_the_registry_gets_its_revision_its_place_in_the_report_and_its_count
#= tests/read.rs a_page_of_261120_bytes_passes_and_a_page_one_byte_longer_is_refused
#= tests/read.rs a_page_holds_whole_items_while_the_reply_stays_inside_its_bound
#= tests/read.rs a_page_counts_the_id_under_next_against_the_bound
#= tests/read.rs the_evidence_read_leaves_out_a_window_row_from_its_expiry_instant
#= tests/read.rs the_hint_falls_to_the_next_evidence_row_inside_its_window_and_is_null_when_none_is_left
#= tests/read.rs a_page_of_the_bound_stays_whole_when_the_other_evidence_row_has_passed_its_expiry
#= tests/export.rs export_proposals_writes_the_header_and_one_line_per_proposed_workflow_in_order
#= tests/export.rs export_proposals_writes_no_id_of_a_support_row_past_its_bound
#= tests/past_bound.rs every_read_hands_out_what_an_expiry_at_its_clock_would_leave
#= tests/past_bound.rs at_the_report_s_deadline_the_due_row_leaves_every_read_before_any_expiry
#= tests/past_bound.rs no_read_writes_past_a_deadline
#= tests/past_bound.rs a_row_one_or_two_days_ahead_is_read_three_days_ahead_is_not_and_the_classes_side_by_side
#= tests/past_bound.rs an_update_removes_the_support_row_of_a_summary_past_its_bound_that_no_read_hands_out
#= tests/past_bound.rs a_delete_a_purge_and_the_counts_still_find_rows_that_the_reads_leave_out
#= tests/past_bound.rs a_step_builds_no_record_from_a_row_past_its_bound
#= tests/past_bound.rs a_clock_set_back_over_a_deadline_hands_a_row_out_again_until_the_host_s_expiry_has_run
#= tests/past_bound.rs every_read_of_one_call_reads_by_one_reading_of_the_host_s_clock
#= tests/past_bound.rs the_interface_read_leaves_rows_past_their_bound_out_of_summaries_and_evidence_pages

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

# judge <test status> <test log> <list>: the reason of a failure in $cause, or status 0.
judge() {
  cause=
  if [ "$1" -ne 0 ]; then
    cause="a test binary of halvo-derived exited $1"
    return 1
  fi
  gap=$(awk -v list="$3" '
    { sub(/\r$/, "") }
    FILENAME == list {
      if (NF != 2) { if (bad == "") bad = "a line of the list is not <test target> <test name>"; next }
      n++; target[n] = $1; test[n] = $2
      next
    }
    $1 == "Running" {
      at = ($2 == "unittests") ? $3 : $2
      gsub(/\\/, "/", at)
      ran[at] = 1
      next
    }
    at != "" && /^test .* \.\.\. ok$/ { ok[at, substr($0, 6, length($0) - 12)] = 1 }
    END {
      if (bad != "") { print bad; exit }
      if (n == 0) { print "the list names no test"; exit }
      for (i = 1; i <= n; i++) {
        if (!(target[i] in ran)) { print "the test target " target[i] " did not run"; exit }
        if (!((target[i], test[i]) in ok)) { print "the test " test[i] " of " target[i] " did not pass"; exit }
      }
    }
  ' "$3" "$2") || {
    cause="the test log cannot be read"
    return 1
  }
  if [ -n "$gap" ]; then
    cause=$gap
    return 1
  fi
  return 0
}

# judge_tree <status> <cargo tree output>: the reason of a failure in $cause, or status 0.
judge_tree() {
  cause=
  if [ "$1" -ne 0 ]; then
    cause="cargo tree -p halvo-derived -e features exited $1"
    return 1
  fi
  bundled=$(grep -c -e 'feature "bundled' "$2")
  [ "$?" -le 1 ] || {
    cause="the output of cargo tree cannot be read"
    return 1
  }
  openssl=$(grep -c -e 'openssl-sys' "$2")
  [ "$?" -le 1 ] || {
    cause="the output of cargo tree cannot be read"
    return 1
  }
  printf 'cargo tree -p halvo-derived -e features: bundled features %s, openssl-sys lines %s\n' "$bundled" "$openssl"
  if [ "$bundled" -ne 0 ] || [ "$openssl" -ne 0 ]; then
    cause="cargo tree -p halvo-derived -e features lists a bundled feature or openssl-sys"
    return 1
  fi
  return 0
}

# binaries <cargo messages> <output>: one line '<test target> <executable>' per test binary of halvo-derived, with
# the doubled backslashes of a JSON string made single.
binaries() {
  awk -v package="$package" '
    index($0, "\"reason\":\"compiler-artifact\"") && index($0, package) && index($0, "\"executable\":\"") {
      exe = $0; sub(/.*"executable":"/, "", exe); sub(/".*/, "", exe)
      target = $0; sub(/.*"target":\{"kind":\["/, "", target); kind = target; sub(/".*/, "", kind)
      sub(/.*"name":"/, "", target); sub(/".*/, "", target)
      label = (kind == "lib") ? "src/lib.rs" : "tests/" target ".rs"
      print label " " exe
    }
  ' "$1" >"$2.raw" || return 1
  sed 's/\\\\/\\/g' "$2.raw" >"$2"
}

measure() {
  cd "$CI_D_REPO" || fail "the repository root cannot be entered"
  artifacts=$CI_D_ARTIFACTS
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail "mktemp failed"
  git show "HEAD:$own" >"$work/own.sh" || fail "git show HEAD:$own failed"
  sed -n 's/^#= //p' "$work/own.sh" >"$work/required" || fail "the list of $own cannot be read"
  listed=$(grep -c '' "$work/required") || fail "the list of $own names no test"

  cargo test --workspace --locked --no-run --message-format=json >"$work/messages.json" 2>"$artifacts/no-run.txt" ||
    fail "cargo test --workspace --locked --no-run exited $?"
  binaries "$work/messages.json" "$work/binaries" || fail "the messages of cargo cannot be read"
  count=$(grep -c '' "$work/binaries") || fail "cargo lists no test binary of halvo-derived"
  log=$artifacts/tests.txt
  : >"$log" || fail "the test log cannot be written"
  status=0
  while read -r label exe; do
    printf '     Running %s (%s)\n' "$label" "$exe" >>"$log"
    (cd crates/halvo-derived && "$exe") >>"$log" 2>&1
    code=$?
    [ "$code" -eq 0 ] || status=$code
  done <"$work/binaries"
  passed=$(grep -c ' \.\.\. ok' "$log")
  printf 'halvo-derived test binaries run %s, tests passed %s, listed tests %s\n' "$count" "$passed" "$listed"
  judge "$status" "$log" "$work/required" || fail "$cause"

  cargo tree --locked -p halvo-derived -e features >"$work/tree.txt" 2>"$artifacts/tree-err.txt"
  judge_tree "$?" "$work/tree.txt" || fail "$cause"

  git diff --quiet HEAD -- || fail "a tracked file differs from HEAD after the tests"
  printf 'PASS %s\n' "$name"
}

artifact() {
  printf '{"reason":"compiler-artifact","package_id":"path+file:///w/crates/halvo-derived#0.0.0","target":{"kind":["%s"],"crate_types":["%s"],"name":"%s","src_path":"x"},"executable":"%s","fresh":true}\n' "$@"
}

check() {
  out=$(
    PATH=$work/bin:$PATH
    CI_D_REPO=$work/repo
    CI_D_ARTIFACTS=$work/artifacts
    export PATH CI_D_REPO CI_D_ARTIFACTS STUB_WORK STUB_LIST STUB_MESSAGES STUB_RUN_EXIT STUB_TREE STUB_TREE_EXIT STUB_DIFF_EXIT STUB_SHOW_EXIT
    sh "$self" 2>&1
  )
  code=$?
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  case $out in
    *"$3"*) ;;
    *) got=$got-without-the-text ;;
  esac
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$got"
  if [ "$got" != "$2" ]; then
    wrong="$wrong $1"
  elif [ "$2" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
  STUB_LIST=$work/list.good
  STUB_MESSAGES=$work/messages.good
  STUB_RUN_EXIT=0
  STUB_TREE=$work/tree.good
  STUB_TREE_EXIT=0
  STUB_DIFF_EXIT=0
  STUB_SHOW_EXIT=0
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  STUB_WORK=$work
  mkdir -p "$work/bin" "$work/repo/crates/halvo-derived" "$work/artifacts" || exit 2
  cat >"$work/bin/cargo" <<'EOF'
#!/bin/sh
case $* in
  'test --workspace --locked --no-run --message-format=json') cat "$STUB_MESSAGES" ;;
  'tree --locked -p halvo-derived -e features')
    cat "$STUB_TREE"
    exit "$STUB_TREE_EXIT"
    ;;
  *)
    printf 'stub cargo: got the arguments: %s\n' "$*"
    exit 97
    ;;
esac
EOF
  cat >"$work/bin/git" <<'EOF'
#!/bin/sh
case $* in
  'show HEAD:scripts/r2/ci.d/50-derived.sh')
    cat "$STUB_LIST"
    exit "$STUB_SHOW_EXIT"
    ;;
  'diff --quiet HEAD --') exit "$STUB_DIFF_EXIT" ;;
  *) exit 97 ;;
esac
EOF
  printf '#!/bin/sh\nprintf "test tests::alpha ... ok\\n"\n' >"$work/lib-tests"
  cat >"$work/open-tests" <<'EOF'
#!/bin/sh
printf 'test beta ... ok\r\ntest gamma ... ok\r\n'
exit "$STUB_RUN_EXIT"
EOF
  printf '#!/bin/sh\nprintf "test beta ... ignored\\ntest delta ... ok\\n"\n' >"$work/other-tests"
  chmod +x "$work/bin/cargo" "$work/bin/git" "$work/lib-tests" "$work/open-tests" "$work/other-tests" || exit 2
  {
    artifact lib lib halvo_derived "$work/lib-tests"
    artifact test bin open "$work/open-tests"
    printf '{"reason":"compiler-artifact","package_id":"path+file:///w/crates/halvo-schema#0.0.0","target":{"kind":["test"],"crate_types":["bin"],"name":"open","src_path":"x"},"executable":"%s","fresh":true}\n' "$work/other-tests"
    printf '{"reason":"compiler-artifact","package_id":"path+file:///w/crates/halvo-derived#0.0.0","target":{"kind":["lib"],"crate_types":["lib"],"name":"halvo_derived","src_path":"x"},"executable":null,"fresh":true}\n'
    printf '{"reason":"build-finished","success":true}\n'
  } >"$work/messages.good" || exit 2
  sed 's|/w/crates/halvo-derived#|/w/crates/halvo-other#|' "$work/messages.good" >"$work/messages.none" || exit 2
  printf '#!/bin/sh\n#= src/lib.rs tests::alpha\n#= tests/open.rs beta\n#= tests/open.rs gamma\n' >"$work/list.good"
  printf '#!/bin/sh\n#= src/lib.rs tests::alpha\n#= tests/open.rs delta\n' >"$work/list.other-binary"
  printf '#!/bin/sh\n#= tests/open.rs epsilon\n' >"$work/list.missing"
  printf '#!/bin/sh\n#= tests/purge.rs beta\n' >"$work/list.unrun"
  printf '#!/bin/sh\n' >"$work/list.empty"
  printf 'halvo-derived v0.0.0\n|-- rusqlite feature "sqlcipher"\n|   `-- libsqlite3-sys feature "sqlcipher"\n' >"$work/tree.good"
  printf 'halvo-derived v0.0.0\n|-- rusqlite feature "bundled-sqlcipher"\n' >"$work/tree.bundled"
  printf 'halvo-derived v0.0.0\n|-- libsqlite3-sys v0.39.1\n|   `-- openssl-sys v0.9.111\n' >"$work/tree.openssl"
  rejected=0
  wrong=
  STUB_LIST=$work/list.good
  STUB_MESSAGES=$work/messages.good
  STUB_RUN_EXIT=0
  STUB_TREE=$work/tree.good
  STUB_TREE_EXIT=0
  STUB_DIFF_EXIT=0
  STUB_SHOW_EXIT=0

  check passes PASS 'halvo-derived test binaries run 2, tests passed 3, listed tests 3'
  STUB_RUN_EXIT=101
  check a-test-binary-fails FAIL 'a test binary of halvo-derived exited 101)'
  STUB_LIST=$work/list.other-binary
  check a-test-of-another-crate FAIL 'the test delta of tests/open.rs did not pass)'
  STUB_LIST=$work/list.missing
  check a-listed-test-missing FAIL 'the test epsilon of tests/open.rs did not pass)'
  STUB_LIST=$work/list.unrun
  check a-listed-target-not-run FAIL 'the test target tests/purge.rs did not run)'
  STUB_LIST=$work/list.empty
  check an-empty-list FAIL "the list of $own names no test)"
  STUB_SHOW_EXIT=128
  check the-commit-unreadable FAIL "git show HEAD:$own failed)"
  STUB_MESSAGES=$work/messages.none
  check no-test-binary FAIL 'cargo lists no test binary of halvo-derived)'
  STUB_TREE=$work/tree.bundled
  check a-bundled-feature FAIL 'lists a bundled feature or openssl-sys)'
  STUB_TREE=$work/tree.openssl
  check openssl-sys FAIL 'lists a bundled feature or openssl-sys)'
  STUB_TREE_EXIT=101
  check cargo-tree-fails FAIL 'cargo tree -p halvo-derived -e features exited 101)'
  STUB_DIFF_EXIT=1
  check a-tracked-file-changed FAIL 'a tracked file differs from HEAD after the tests)'

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  '') measure ;;
  *) exit 64 ;;
esac
