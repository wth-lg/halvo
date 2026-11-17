"""The loop step of unit U0.6b (scratch branches alone): builds the drive crate's test binaries,
then runs the tests that the unit added ROUNDS times in each binary with --exact, and counts a
round as passed only when its result line says that exactly the named tests ran and passed.
usage: python3 -I tools/r3-trials/drive/loop-u06b.py ROUNDS
Prints counts and seconds alone; exit 0 only when every round of every binary passed."""
import json
import os
import re
import subprocess
import sys
import time

TESTS = {
    "r3-drive": [
        "tests::the_fence_is_on_in_this_test_target",
        "tests::the_fence_lint_takes_the_assertion_alone",
        "tests::every_test_target_asserts_that_its_fence_is_on",
        "tests::the_fence_lint_refuses_a_test_that_is_not_compiled_or_not_run_here",
        "tests::the_manifest_check_reads_the_section_and_the_feature_of_the_line",
        "tests::the_lint_finds_each_kind_of_test_target",
        "tests::the_lint_refuses_a_manifest_that_it_does_not_read",
        "tests::the_lint_refuses_an_item_named_after_the_crate",
        "tests::the_lint_refuses_each_spelling_that_opens_the_fence",
        "body::tests::a_format_character_inside_a_host_is_left_out",
        "body::tests::a_nonspacing_mark_stays_inside_its_host",
        "body::tests::an_enclosing_mark_stays_inside_its_host",
        "body::tests::a_variation_selector_stays_inside_its_host",
        "body::tests::the_lower_case_of_a_dotted_capital_i_keeps_the_host_whole",
        "body::tests::a_dotted_name_with_a_port_is_an_address_whatever_its_last_label",
        "body::tests::a_name_with_a_port_after_a_hyphen_look_alike_is_an_address",
        "body::tests::a_vertical_ideographic_full_stop_is_a_dot",
        "body::tests::the_small_at_sign_alone_names_an_address",
        "body::tests::a_hyphen_at_the_ends_of_a_host_is_trimmed",
        "body::tests::the_boundaries_of_an_xn_label_and_of_an_octet",
        "body::tests::the_review_s_address_forms_name_an_address",
        "body::tests::a_host_followed_by_a_path_names_an_address",
        "body::tests::a_name_with_a_port_followed_by_a_path_names_an_address",
        "body::tests::a_full_width_or_small_at_sign_names_an_address",
        "body::tests::an_ipv6_address_names_an_address",
        "body::tests::full_width_forms_and_ideographic_full_stops_are_read_as_ascii",
        "body::tests::each_seed_selects_its_two_messages_through_the_reader",
        "body::tests::the_senders_are_read_by_the_reader_of_halvo_schema",
        "body::tests::a_list_that_the_reader_refuses_selects_nothing",
    ],
    "r3-standin": ["tests::the_fence_is_on_in_this_test_target"],
}


def main(argv):
    rounds = int(argv[0])
    crate = os.path.join("tools", "r3-trials", "drive")
    build = subprocess.run(
        ["cargo", "test", "--locked", "--manifest-path", os.path.join(crate, "Cargo.toml"), "--no-run", "--message-format=json"],
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
    )
    if build.returncode != 0:
        print("loop u06b: the test binaries were not built (exit %d)" % build.returncode)
        return 1
    binaries = {}
    for line in build.stdout.decode("utf-8", "replace").splitlines():
        try:
            message = json.loads(line)
        except ValueError:
            continue
        if message.get("reason") == "compiler-artifact" and message.get("executable") and message["target"]["name"] in TESTS:
            if message.get("profile", {}).get("test"):
                binaries[message["target"]["name"]] = message["executable"]
    ok = True
    for target, names in TESTS.items():
        exe = binaries.get(target)
        if exe is None:
            print("loop u06b %s: no test binary" % target)
            ok = False
            continue
        want = re.compile(r"^test result: ok\. %d passed; 0 failed; 0 ignored; 0 measured; \d+ filtered out;" % len(names), re.M)
        passed = 0
        start = time.time()
        for _ in range(rounds):
            run = subprocess.run(
                [exe, "--exact", "--test-threads=4"] + names,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                cwd=crate,
            )
            if run.returncode == 0 and want.search(run.stdout.decode("utf-8", "replace")):
                passed += 1
        print(
            "loop u06b %s: passed %d of %d (tests per round %d, %d s)"
            % (target, passed, rounds, len(names), int(time.time() - start))
        )
        ok = ok and passed == rounds
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
