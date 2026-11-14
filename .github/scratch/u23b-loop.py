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
        "review_seq_3_a_start_lapses_the_owed_ready_item_with_the_check",
        "recoverys_own_raise_after_its_refused_body_lapses_the_owed_ready_item",
        "a_recovery_body_stored_with_a_lost_acknowledgement_lets_the_check_go_alone",
        "a_raise_that_meets_seq_for_another_writer_still_lapses_the_owed_ready_item",
        "a_tick_whose_list_does_not_answer_still_lapses_the_owed_ready_item",
        "the_answer_over_an_owed_ready_item_lapses_it_in_its_body",
        "of_two_agents_only_the_one_that_owes_lapses_its_ready_item",
        "the_raise_writes_no_owed_lapse_of_a_check_item",
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
