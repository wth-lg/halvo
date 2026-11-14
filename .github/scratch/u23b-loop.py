"""Runs each test that U2.3b's round of ruling RD-98 adds 200 times from its test binary and counts a pass only when the
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
        "a_start_whose_read_brings_an_open_ready_item_answers_it_first_in_its_body",
        "after_a_start_that_answered_the_ready_item_its_read_brought_the_next_slot_raises_one_ready_item",
        "a_start_that_leaves_the_queue_answers_the_ready_item_that_its_read_brings",
        "a_start_whose_read_brings_the_ready_item_closes_it_though_step_7_or_8_fails",
        "a_tick_may_raise_ready_beside_the_check_item_of_an_earlier_life_whose_lapse_is_owed",
        "a_switch_off_that_first_reads_the_record_raises_no_check_and_the_next_tick_lapses_and_raises",
        "a_skip_that_first_reads_the_record_gives_the_slot_away_and_the_next_tick_raises_the_check_alone",
        "review28_a_skip_beside_an_earlier_skip_of_another_item_whose_read_shows_the_ready_item_lapsed_is_stale",
        "a_skip_after_a_queued_start_whose_answer_the_owner_stored_with_a_lost_acknowledgement_is_stale",
        "no_check_is_raised_while_the_board_holds_the_run_live_though_the_store_recorded_it",
        "a_runs_check_item_that_a_read_brings_is_not_taken_up_as_the_agents_item",
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
