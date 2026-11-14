"""Runs each test that U2.3b adds 200 times from its test binary and counts a pass only when the
run's own output shows the test ran and passed: the line "test <name> ... ok" and the result line
"test result: ok. 1 passed; 0 failed". A filter that matches no test exits 0, so the exit code is
never counted alone. Prints one line per test and a total; exits 1 when any test missed a pass.
"""
import json
import os
import subprocess
import sys

RUNS = int(os.environ.get("U23B_LOOP_RUNS", "200"))
TESTS = {
    "run_check": [
        "a_check_lapses_an_open_ready_item_and_waits_for_a_failed_store",
        "the_answer_to_a_check_lapses_an_open_ready_item_and_answers_its_check_in_one_body",
        "the_answer_went_through_over_an_open_ready_item_stores_its_slot_skipped_last",
        "the_answer_after_a_refusal_seq_answers_the_check_that_the_read_shows",
        "an_answer_over_a_ready_item_stored_with_a_lost_acknowledgement_is_taken_once",
        "an_open_ready_item_that_the_owner_stored_with_a_lost_acknowledgement_lapses_for_the_check",
        "one_body_lapses_the_ready_item_before_it_raises_the_check",
        "after_a_refusal_seq_the_check_reads_the_ready_item_anew_and_lapses_none_that_a_skip_closed",
        "a_lapse_body_that_the_owner_stored_with_a_lost_acknowledgement_is_held_and_not_written_again",
        "the_check_waits_behind_an_open_setup_item",
        "the_lapse_stores_no_slot_skipped_and_after_try_again_the_next_tick_raises_ready_again",
        "a_lost_start_that_the_checks_read_brings_is_closed",
        "a_lost_start_that_the_answers_read_brings_is_closed",
        "a_skip_whose_read_shows_the_ready_item_lapsed_is_stale_and_the_board_holds_the_check",
        "a_skip_that_the_owner_stored_with_a_lost_acknowledgement_is_taken_once",
        "a_switch_off_whose_read_shows_the_check_body_holds_that_check",
        "an_answer_whose_read_shows_the_check_answered_otherwise_is_stale",
        "the_answer_to_the_check_of_a_gone_workflow_is_state_and_sends_nothing",
        "a_start_raises_the_check_of_its_own_agent_alone",
        "the_end_of_a_run_reads_the_check_of_every_agent",
        "the_board_holds_the_body_that_the_raise_stored",
    ],
    "names_same": [
        "item_lapse_cause_names_are_the_seven_causes_of_an_item_lapsed_row_of_i7_1",
    ],
}


def executables():
    args = ["cargo", "test", "--locked", "-p", "halvo-coord", "--no-run", "--message-format=json"]
    for target in TESTS:
        args += ["--test", target]
    out = subprocess.run(args, capture_output=True, text=True, check=True).stdout
    found = {}
    for line in out.splitlines():
        if not line.startswith("{"):
            continue
        msg = json.loads(line)
        target = msg.get("target", {})
        if msg.get("reason") == "compiler-artifact" and msg.get("executable") and target.get("name") in TESTS:
            found[target["name"]] = msg["executable"]
    return found


def main():
    exe = executables()
    missing = [t for t in TESTS if t not in exe]
    if missing:
        print("loop no executable for %s" % ", ".join(missing))
        return 1
    short = 0
    for target, names in TESTS.items():
        for name in names:
            passed = 0
            for _ in range(RUNS):
                run = subprocess.run([exe[target], "--exact", name, "--test-threads=1"],
                                     capture_output=True, text=True)
                text = run.stdout
                if (run.returncode == 0 and ("test %s ... ok" % name) in text
                        and "test result: ok. 1 passed; 0 failed" in text):
                    passed += 1
            print("loop %s::%s runs=%d passed=%d" % (target, name, RUNS, passed))
            if passed != RUNS:
                short += 1
    total = sum(len(v) for v in TESTS.values())
    print("loop tests=%d runs_each=%d tests_short_of_all_passes=%d" % (total, RUNS, short))
    return 1 if short else 0


if __name__ == "__main__":
    sys.exit(main())
