#!/bin/sh
# scratch-loop.sh: the round's new tests of halvo-learn-gate, each run in 200 rounds; prints one line per test with
# its count of passes and fails when a count is below 200. Used on a scratch branch alone.
set -u
rounds=200
unit='tests::compile::the_compile_run_of_the_stub_stands_at_rev_1_and_at_rev_2_verified_with_no_base
tests::targets::constants_match_targets
tests::delete::delete_seeds_both_workflows_with_the_rows_of_the_same_number_of_runs
tests::delete::delete_line_reads_ok_with_the_store_s_cascade
tests::delete::delete_line_misses_a_cascade_that_keeps_a_learn_row_of_the_deleted_workflow
tests::delete::delete_line_misses_a_cascade_that_takes_a_row_of_the_other_workflow
tests::delete::delete_line_misses_a_cascade_that_rewrites_a_row_of_the_other_workflow
tests::delete::delete_line_misses_a_cascade_that_takes_nothing_or_names_no_stored_workflow
tests::delete::delete_line_misses_when_the_other_agent_is_no_longer_served_its_learned_step
tests::verdict::verdict_holds_on_tallies_that_meet_every_clause
tests::verdict::verdict_misses_a_loss_of_harm_alone
tests::verdict::verdict_misses_a_failed_occurrence_of_a_benign_admit_line
tests::verdict::verdict_misses_a_failed_occurrence_of_any_other_line
tests::verdict::verdict_misses_assisted_below_drifts
tests::verdict::verdict_misses_wins_below_critical
tests::verdict::verdict_misses_a_cost_over_its_budget
tests::host::the_gate_host_gives_the_agent_store_s_handler_the_clock_s_day
tests::reader::reader_answers_the_unit_s_own_files_as_the_trial_does'
cli='drift_prints_a_miss_and_exits_1'
out=target/scratch-loop.txt
mkdir -p target || exit 1
: >"$out" || exit 1
# shellcheck disable=SC2086
cargo test --locked -p halvo-learn-gate --no-run >/dev/null 2>&1 || exit 1
i=0
while [ "$i" -lt "$rounds" ]; do
  i=$((i + 1))
  # shellcheck disable=SC2086
  cargo test --locked -p halvo-learn-gate --bin learn-gate -- --exact $unit >"$out.round" 2>/dev/null
  cargo test --locked -p halvo-learn-gate --test drift_cli -- --exact $cli >>"$out.round" 2>/dev/null
  tr -d '\r' <"$out.round" | grep '^test .* \.\.\. ' >>"$out"
done
printf '%s\n%s\n' "$unit" "$cli" | awk -v rounds="$rounds" -v file="$out" '
  BEGIN {
    while ((getline line < file) > 0) {
      split(line, f, " ")
      if (f[4] == "ok") ok[f[2]]++
    }
  }
  {
    n = ok[$0] + 0
    printf "loop test=%s ok=%d of=%d\n", $0, n, rounds
    if (n < rounds) bad = 1
  }
  END { exit bad }
'
