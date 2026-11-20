#!/bin/sh
# jobs: windows
# rule: every Windows functional test runs in its own process, its log is read by the prints rule of scripts/r1/windows-prints-rule.sh (win-3b), whose vocabulary reads this check's lists macos_only_paths and expected_tests in their here-document form, and the check fails on any line that the rule prints and never prints such a log (the rule's standard error is read with its output, and the rule, the verdict and the printed lines read one copy of the log taken when the test ended; the logs of the long form's loops reach the job log unread by the rule, a named limit), and a listed defect must fail with its marker; a test's WF line carries the last rounds_decided_by_the_time_rule=N that a TF line of a log that the rule passed holds (ruling W3-32); the short page controls are summed by state, and a run that took more than one second control fails; the stall tests run the rounds that HALVO_WF_FORM names, a closed word that the check hands on alone (short or unset in the every-push job, long on a scratch push, which also loops the tests of its loop list by exact name); and in the short form the passing long-form run that docs/baseline/ledger.d/win-3.txt records ran on the trees, blobs and windows lock lines that the head has of the files whose change owes that run
# lane: alone
set -u
group=${WF_GROUP:-native}
case $group in
  native) name=50-windows-functional ;;
  browser) name=52-windows-browser ;;
  *)
    printf 'FAIL 50-windows-functional (WF_GROUP is native or browser, not %s)\n' "$group"
    exit 1
    ;;
esac
test_limit_s=${WF_TEST_LIMIT_S:-360}
timeout_tool=${WF_TIMEOUT_TOOL:-timeout}
failing_fixtures=68
form_test=stall::the_round_count_of_the_stall_tests
ledger=${WF_LEDGER_FILE:-docs/baseline/ledger.d/win-3.txt}
prints_rule=scripts/r1/windows-prints-rule.sh
prints_rule_line='^prints-rule [A-Za-z0-9_:]+ line=[0-9]+ length=[0-9]+ token=[0-9]+$'
shutdown_line='TF shutdown source='
fixtures_at_once=4
LC_ALL=C
export LC_ALL
cr=$(printf '\r')
nl='
'

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

expected_tests() {
  cat <<'EOF'
a_dropped_keyboard_source_stops_capturing group=native
a_timed_out_stop_reads_no_input_posted_after_its_answer group=native
browser::a_burst_whose_first_letter_swaps_records_nothing_in_chrome group=browser
browser::a_burst_whose_first_letter_swaps_records_nothing_in_edge group=browser
browser::a_letter_and_a_tab_out_of_a_swapped_field_record_nothing_in_chrome group=browser
browser::a_letter_and_a_tab_out_of_a_swapped_field_record_nothing_in_edge group=browser
browser::a_renamed_copy_of_chrome_reads_unknown_and_records_nothing group=browser
browser::a_renamed_copy_of_firefox_reads_unknown_and_records_nothing group=browser
browser::after_a_start_behind_another_window_in_chrome group=browser
browser::after_a_start_behind_another_window_in_edge group=browser
browser::burst_rounds::a_burst_with_no_round_that_the_time_rule_decided_prints_a_count_of_zero group=browser
browser::burst_rounds::a_counted_round_reaches_the_tally_with_its_four_counts_and_its_freed_at_ms group=browser
browser::burst_rounds::a_counted_round_that_a_rule_of_the_swap_decided_with_nothing_recorded_passes group=browser
browser::burst_rounds::a_counted_round_that_failed_fails_the_burst_in_both_forms group=browser
browser::burst_rounds::a_counted_round_that_the_time_rule_decided_with_nothing_recorded_passes group=browser
browser::burst_rounds::a_counted_round_whose_first_letter_no_rule_decided_fails group=browser
browser::burst_rounds::a_counted_round_with_a_keystroke_event_fails_whatever_the_rule group=browser
browser::burst_rounds::a_counted_round_with_a_recorded_letter_fails_whatever_the_rule group=browser
browser::burst_rounds::a_counted_round_with_a_recorded_tab_fails_whatever_the_rule group=browser
browser::burst_rounds::a_counted_round_with_another_recorded_key_fails_whatever_the_rule group=browser
browser::burst_rounds::both_forms_fail_five_rounds_of_five_that_the_time_rule_decided group=browser
browser::burst_rounds::both_forms_fail_two_rounds_of_five_that_the_time_rule_decided_with_their_freed_at_ms group=browser
browser::burst_rounds::both_forms_take_one_round_of_five_that_the_time_rule_decided_and_print_its_count group=browser
browser::burst_rounds::the_burst_of_a_browser_runs_five_rounds_in_both_forms group=browser
browser::burst_rounds::the_burst_prints_the_count_of_its_counted_rounds_that_the_time_rule_decided_and_their_freed_at_ms group=browser
browser::cold_and_warm_orders_in_chrome group=browser
browser::cold_and_warm_orders_in_edge group=browser
browser::stall_phases_of_the_page_in_chrome group=browser
browser::stall_phases_of_the_page_in_edge group=browser
browser::steps::a_page_that_names_no_step_does_not_count group=browser
browser::steps::a_round_on_another_step_of_the_page_does_not_count group=browser
browser::steps::a_round_on_its_own_step_of_the_page_counts group=browser
browser::steps::a_round_on_the_page_in_another_phase_or_hold_at_its_step_does_not_count group=browser
browser::steps::the_steps_of_a_page_go_into_its_address_in_order group=browser
browser::swap_phases_of_the_page_in_chrome group=browser
browser::swap_phases_of_the_page_in_edge group=browser
c1_raw_input_delivers_keystrokes_and_pointer_events group=native
c2_screen_source_delivers_a_frame group=native
c3_foreground_source_reports_an_activated_window group=native
c4_window_title_source_reports_a_title_change group=native
c5_installed_software_returns_registry_entries group=native
c5_watch_precondition_removes_an_empty_per_user_key_and_goes_on group=native
c8_display_affinity_exclude_from_capture_is_set_and_read_back group=native
c8b_window_destroyed_after_the_last_capture_session group=native
c9_peak_working_set_of_sources_1_to_4_for_60_seconds group=native
capture_times_follow_the_input_order group=native
helper_child_window group=native
helper_limited_listener group=native
keys_queued_before_a_stop_are_delivered_or_counted group=native
known_defect::browser_self_move_after_a_held_main_thread_in_chrome W3-5-A2 group=browser
known_defect::browser_self_move_after_a_held_main_thread_in_edge W3-5-A2 group=browser
known_defect::browser_swaps_on_a_held_main_thread_in_chrome W3-7-Q2i group=browser scope=long-only
known_defect::browser_swaps_on_a_held_main_thread_in_edge W3-7-Q2i group=browser scope=long-only
known_defect::c5_watch_armed_while_the_per_user_key_is_absent RR-windows-12 group=native
known_defect::stall_no_focus_key_and_the_program_moves_the_focus_itself W3-5-A2 group=browser scope=long-only
known_defect::style_masked_input_in_chrome_and_edge W3-20-style-mask group=browser
known_defect::x16_packaged_apps_and_the_inventory RR-windows-12 group=native
password_field_fixture_chrome group=browser
password_field_fixture_edge group=browser
password_field_fixture_firefox group=browser
password_field_fixture_native_dialog_phases_a_to_f group=browser
pointer_moves_coalesce group=native
stall::a_masked_edit_and_a_focus_notice_that_the_system_does_not_deliver_record_no_letter group=native
stall::a_second_field_and_a_window_of_another_thread_while_the_program_waits_300_ms group=native
stall::click_then_letters_while_the_program_serves_sent_messages_at_each_wait group=native
stall::click_then_letters_while_the_program_sleeps_at_each_wait group=native
stall::enter_a_tab_out_and_a_focus_move_in_two_steps_record_no_letter group=native
stall::five_letters_of_the_plain_field_after_the_programs_own_focus_move_are_recorded group=native
stall::helper_stall_fixture group=native
stall::kinds::a_native_phase_fails_each_of_its_own_faults group=native
stall::kinds::a_phase_before_the_before_control_or_after_the_after_control_fails group=native
stall::kinds::a_phase_between_the_two_controls_of_its_own_session_passes group=native
stall::kinds::a_phase_that_the_controls_of_another_session_bracket_fails group=native
stall::kinds::a_phase_whose_control_recorded_fewer_than_five_letters_fails group=native
stall::kinds::a_phase_whose_two_controls_come_from_two_sessions_fails group=native
stall::kinds::each_check_of_a_phase_fails_its_own_fault_in_each_family group=native
stall::kinds::the_kinds_hold_one_phase_for_each_test_and_sub_case_that_they_replace group=native
stall::tab_then_letters_while_the_program_serves_sent_messages_at_each_wait group=native
stall::tab_then_letters_while_the_program_sleeps_at_each_wait group=native
stall::the_round_count_of_the_stall_tests group=native
stop_with_a_full_sink group=native
the_fault_line_allocates_nothing group=native
the_first_own_read_after_a_start_reads_the_focused_password_field group=native
title_event_on_a_switch_to_a_window_with_a_fixed_title group=native
x12_screen_source_fifteen_start_stop_cycles group=native
x13_frames_on_a_static_screen group=native
x14_foreground_app_of_a_hosted_app group=native
x15_a_held_key group=native
x1_keystrokes_typed_into_a_password_field group=native
x2_window_title_events_for_other_windows_and_controls group=native
x3_attribution_before_the_first_foreground_change group=native
x5_screen_source_stop_and_start_again group=native
x7_capture_border_on_the_screen group=native
x8_raw_input_across_the_integrity_boundary group=native
x9_screen_source_across_a_display_mode_change group=native
EOF
}

loop_tests() {
  cat <<'EOF'
lib 20 secure_input::windows::detector::tests::an_event_of_another_element_does_not_let_the_kept_password_element_go
lib 20 secure_input::windows::detector::tests::an_own_read_between_a_focus_events_start_and_its_mark_does_not_let_its_reading_go
lib 20 secure_input::windows::detector::tests::a_focus_events_clear_from_the_callback_clears_no_key
lib 20 windows::input::tests::a_held_tab_sets_one_latch_for_each_key_down_at_the_source
lib 20 windows::input::tests::keys_wait_for_a_held_pointer_pump_and_an_idle_pointer_source_lets_them_through
lib 20 windows::input::tests::a_keyboard_stop_lets_the_click_route_go_only_when_its_window_and_gate_are_its_own
lib 20 windows::input::tests::a_click_of_the_same_tick_from_the_pointer_pump_leaves_a_password_key_withheld
lib 20 windows::input::tests::keys_at_a_gate_whose_start_read_reads_unknown_are_withheld_with_no_focus_event
lib 20 windows::input::tests::the_pointer_owns_the_clicks_from_a_tick_step_before_its_registration
functional 20 stall::five_letters_of_the_plain_field_after_the_programs_own_focus_move_are_recorded
lib 200 windows::input::tests::every_key_down_of_a_focus_key_latches_and_only_a_recorded_one_holds_first
lib 200 secure_input::windows::detector::tests::the_process_file_is_the_own_executables_name_and_nothing_for_an_ended_process
lib 200 secure_input::windows::detector::tests::a_desktop_hook_that_cannot_be_set_refuses_the_attach_and_removes_the_focus_handler
lib 200 secure_input::windows::detector::tests::an_attach_whose_first_read_does_not_answer_times_out_and_stops_that_detector
EOF
}

ledger_paths() {
  cat <<'EOF'
scripts/r1/ci.d/50-windows-functional.sh
scripts/r1/windows-prints-rule.sh
crates/halvo-control
crates/halvo-sqlcipher
EOF
}

macos_only_paths() {
  cat <<'EOF'
crates/halvo-capture/src/macos	src/lib.rs	cfg-mod macos
crates/halvo-capture/src/secure_input_macos.rs	src/secure_input.rs	cfg-mod macos
crates/halvo-capture/src/custody/macos.rs	src/custody/mod.rs	cfg-mod macos
crates/halvo-capture/src/secure_input_macos_hold.rs	src/secure_input_macos.rs	path-mod hold
crates/halvo-capture/tests/macos_stall_fixture.rs	tests/macos_stall_fixture.rs	inner-cfg
crates/halvo-capture/tests/macos_stall_fixture	-	no-build
EOF
}

crate_view_id() {
  macos_only_paths | cut -f 1 >"$work/macos-only" || return 1
  git ls-tree -r "$1" -- crates/halvo-capture >"$work/crate-tree" || return 1
  awk -F '\t' 'NR == FNR { skip[$1] = 1; next }
    {
      path = $2
      for (prefix in skip) if (path == prefix || index(path, prefix "/") == 1) next
      print
    }' "$work/macos-only" "$work/crate-tree" | git hash-object --no-filters --stdin
}

windows_lock_id() {
  git show "${1:-HEAD}:Cargo.lock" | tr -d '\r' | awk '
    /^\[\[package\]\]$/ { if (keep) printf "%s", block; block = $0 "\n"; keep = 0; next }
    /^name = "windows/ { keep = 1 }
    { block = block $0 "\n" }
    END { if (keep) printf "%s", block }
  ' | git hash-object --no-filters --stdin
}

ledger_check() {
  fix="run the long form (HALVO_WF_FORM=long) on a scratch push of this head, then record each attempt at this head in $ledger, numbered from 1, with its run, job, functional line, short controls, second controls and slipped controls, the newest one passed, and the ids that the head has of the listed paths and of the windows lines of Cargo.lock (rulings W3-17 S4, W3-19 (1))"
  [ -f "$ledger" ] || fail "$ledger is missing: $fix"
  tr -d '\r' <"$ledger" >"$work/ledger" || fail "$ledger cannot be read"
  ! grep -q '^measured run=' "$work/ledger" || fail "$ledger holds a line of the form measured run=, which records no attempts: $fix"
  attempt_form='^measured attempt=[0-9]+ run=[0-9]+ job=[0-9]+ form=long result=(pass|fail) commit=[0-9a-f]{40} wf=[^ ]+ wf_native=[^ ]+ wf_browser=[^ ]+ short_controls=[^ ]+ second_controls_taken=[0-9]+ slipped_controls=[0-9]+( |$)'
  grep '^measured attempt' "$work/ledger" >"$work/attempts"
  [ -s "$work/attempts" ] || fail "$ledger records no attempt: $fix"
  ! grep -v -q -E "$attempt_form" "$work/attempts" || fail "$ledger holds an attempt line that is not in the form of the long form: $fix"
  numbers=$(sed -E 's/^measured attempt=([0-9]+) .*/\1/' "$work/attempts" | tr '\n' ' ')
  total=$(grep -c '' "$work/attempts")
  wanted=
  index=0
  while [ "$index" -lt "$total" ]; do
    index=$((index + 1))
    wanted="$wanted$index "
  done
  [ "$numbers" = "$wanted" ] || fail "$ledger numbers its attempts ${numbers% } and not 1 to $total in order: $fix"
  commits=$(sed -E 's/.* commit=([0-9a-f]+) .*/\1/' "$work/attempts" | sort -u | grep -c '')
  [ "$commits" = 1 ] || fail "$ledger records attempts at more than one commit: $fix"
  newest=$(tail -n 1 "$work/attempts")
  case $newest in
    *' result=pass '*) ;;
    *) fail "the newest attempt that $ledger records did not pass: $fix" ;;
  esac
  ! sed '$d' "$work/attempts" | grep -q ' result=pass ' || fail "$ledger records a passed attempt before its newest one: $fix"
  run=$(printf '%s\n' "$newest" | sed -E 's/^measured attempt=[0-9]+ run=([0-9]+) .*/\1/')
  sums=$(printf '%s\n' "$newest" | awk '
    function field(line, key,   rest) {
      if (!match(line, " " key "=[^ ]+")) return ""
      return substr(line, RSTART + length(key) + 2, RLENGTH - length(key) - 2)
    }
    function count(list, key,   n, parts, i, kv) {
      n = split(list, parts, ",")
      for (i = 1; i <= n; i++) { split(parts[i], kv, "="); if (kv[1] == key) return kv[2] + 0 }
      return -1
    }
    {
      whole = field($0, "wf"); native = field($0, "wf_native"); browser = field($0, "wf_browser")
      bad = ""
      split("total pass known_defect fail", keys, " ")
      for (i = 1; i <= 4; i++) {
        k = keys[i]
        if (count(native, k) < 0 || count(browser, k) < 0 || count(whole, k) < 0 || count(native, k) + count(browser, k) != count(whole, k)) bad = bad " " k
      }
      print bad
    }')
  [ -z "$sums" ] ||
    fail "the newest attempt that $ledger records gives functional lines of the two scripts that do not sum to its whole line in:$sums: $fix"
  sed -n -E 's/^measured (tree|blob) path=([^ ]*) id=[0-9a-f]+$/\2/p' "$work/ledger" | sort >"$work/ledger-paths" ||
    fail "$ledger cannot be read"
  ledger_paths | sort >"$work/listed-paths" || fail "the list of the ledger's paths cannot be read"
  cmp -s "$work/ledger-paths" "$work/listed-paths" ||
    fail "$ledger records other paths than the listed ones, or a path more than once: $fix"
  while read -r path; do
    recorded=$(awk -v path="$path" '$1 == "measured" && $3 == "path=" path { sub(/^id=/, "", $4); print $4 }' "$work/ledger")
    head_id=$(git rev-parse --verify -q "HEAD:$path") || fail "$path is not in the head: $fix"
    [ "$recorded" = "$head_id" ] ||
      fail "the measured run $run ran on another $path than the head's ($recorded against $head_id): $fix"
  done <"$work/listed-paths"
  macos_only_paths >"$work/macos-only-lines" || fail "the list of the crate's macOS-only paths cannot be read"
  macos_only_paths | cut -f 1 >"$work/macos-only" || fail "the list of the crate's macOS-only paths cannot be read"
  while IFS="$(printf '\t')" read -r path file kind; do
    case $kind in
      "cfg-mod "*)
        module=${kind#cfg-mod }
        git show "HEAD:crates/halvo-capture/$file" 2>/dev/null | tr -d '\r' | awk -v module="$module" '
          $0 ~ "^[[:space:]]*(pub )?mod " module ";" && (above1 ~ /#\[cfg\(target_os = "macos"\)\]/ || above2 ~ /#\[cfg\(target_os = "macos"\)\]/) { held = 1 }
          { above2 = above1; above1 = $0 }
          END { exit held ? 0 : 1 }' ||
          fail "crates/halvo-capture/$file no longer declares mod $module under the macOS cfg that leaves $path out of the ledger's tree: read the crate's cfg lines and mend the list"
        ;;
      "path-mod "*)
        module=${kind#path-mod }
        grep -q -x -F "crates/halvo-capture/$file" "$work/macos-only" ||
          fail "crates/halvo-capture/$file, which declares $path, is not itself a macOS-only path of the list: read the crate's cfg lines and mend the list"
        git show "HEAD:crates/halvo-capture/$file" 2>/dev/null | tr -d '\r' | awk -v module="$module" -v target="${path##*/}" '
          $0 ~ "^[[:space:]]*(pub )?mod " module ";" && (index(above1, "#[path = \"" target "\"]") || index(above2, "#[path = \"" target "\"]")) { held = 1 }
          { above2 = above1; above1 = $0 }
          END { exit held ? 0 : 1 }' ||
          fail "crates/halvo-capture/$file no longer declares mod $module by #[path = \"${path##*/}\"], which leaves $path out of the ledger's tree as a module of a macOS-only file: read the crate's cfg lines and mend the list"
        ;;
      inner-cfg)
        first=$(git show "HEAD:crates/halvo-capture/$file" 2>/dev/null | tr -d '\r' | sed -n 1p)
        [ "$first" = '#![cfg(target_os = "macos")]' ] ||
          fail "crates/halvo-capture/$file no longer starts with the line #![cfg(target_os = \"macos\")] that leaves $path out of the ledger's tree: read the crate's cfg lines and mend the list"
        ;;
      no-build)
        if git cat-file -e HEAD:crates/halvo-capture/build.rs 2>/dev/null ||
          git show HEAD:crates/halvo-capture/Cargo.toml | tr -d '\r' | grep -q '^[[:space:]]*build[[:space:]]*='; then
          fail "crates/halvo-capture has a build script, which can compile what $path holds: read the crate's build and mend the list"
        fi
        rust=$(git ls-tree -r --name-only HEAD -- "$path" | grep '\.rs$' | tr '\n' ' ')
        [ -z "$rust" ] ||
          fail "$path holds Rust files, which a test target can compile: ${rust% }: read the crate's tests and mend the list"
        ;;
      *) fail "the list of the crate's macOS-only paths names no kind that the check knows for $path: $kind" ;;
    esac
    case $kind in
      "cfg-mod "*) continue ;;
    esac
    base=${path##*/}
    stem=${base%.rs}
    quoted=$(printf '%s\n' "$base" | sed 's/\./\\./g') || fail "the name $base cannot be read"
    pattern="(#\\[path|include(_str|_bytes)?!)[^\"]*\"[^\"]*${quoted}|mod[[:space:]]+${stem}[[:space:]]*;"
    set -- 'crates/halvo-capture/*.rs'
    while read -r listed; do
      set -- "$@" ":(exclude)$listed"
    done <"$work/macos-only"
    git grep -l -E "$pattern" HEAD -- "$@" >"$work/namers"
    case $? in
      0)
        namers=$(awk '{ sub(/^HEAD:/, ""); printf "%s ", $0 }' "$work/namers") || fail "the files that name $path cannot be read"
        fail "a file that the Windows target can build names $path, which the list leaves out of the ledger's tree: ${namers% }"
        ;;
      1) ;;
      *) fail "the crate's files cannot be searched for what names $path" ;;
    esac
  done <"$work/macos-only-lines"
  recorded=$(sed -n 's/^measured view path=crates\/halvo-capture without=macos-only id=\([0-9a-f]*\)$/\1/p' "$work/ledger")
  head_id=$(crate_view_id HEAD) || fail "the tree of crates/halvo-capture cannot be read"
  [ -n "$recorded" ] || fail "$ledger records no id of the tree of crates/halvo-capture without its macOS-only paths: $fix"
  [ "$recorded" = "$head_id" ] ||
    fail "the measured run $run ran on another tree of crates/halvo-capture, without its macOS-only paths, than the head's ($recorded against $head_id): $fix"
  recorded=$(sed -n 's/^measured lock path=Cargo.lock crates=windows id=\([0-9a-f]*\)$/\1/p' "$work/ledger")
  head_id=$(windows_lock_id) || fail "the windows lines of Cargo.lock cannot be read"
  [ -n "$recorded" ] || fail "$ledger records no id of the windows lines of Cargo.lock: $fix"
  [ "$recorded" = "$head_id" ] ||
    fail "the measured run $run ran on other windows lines of Cargo.lock than the head's ($recorded against $head_id): $fix"
  printf 'ledger: the measured run %s of the long form, attempt %s, ran on what the head has of crates/halvo-capture without its macOS-only paths, of the check and of its prints rule, of crates/halvo-control and crates/halvo-sqlcipher, which the Windows build of the capture crate compiles, and of the windows lines of Cargo.lock\n' "$run" "$total"
}

loops() {
  cargo test --locked -p halvo-capture --lib --no-run --message-format json >"$work/lib.json" ||
    fail "the library's test build failed: cargo test --lib --no-run exited $?"
  lib_exe=$(grep '"kind":\["lib"\]' "$work/lib.json" | grep '"test":true' |
    sed -n 's/.*"name":"halvo_capture".*"executable":"\([^"]*\)".*/\1/p')
  [ -n "$lib_exe" ] || fail "cargo named no test executable of the library"
  case $lib_exe in
    *"$nl"*) fail "cargo named more than one test executable of the library" ;;
  esac
  lib_exe=$(printf '%s\n' "$lib_exe" | sed 's/\\\\/\//g')
  [ -f "$lib_exe" ] || fail "the library's test executable that cargo named does not exist"
  if [ -n "${WF_LOOP_FILE:-}" ]; then
    tr -d '\r' <"$WF_LOOP_FILE" >"$work/loops" || fail "the loop list cannot be read"
  else
    loop_tests >"$work/loops"
  fi
  loop_failed=0
  while read -r kind runs test; do
    case $kind in
      lib) set -- "$lib_exe" --exact --nocapture --test-threads=1 "$test" ;;
      functional) set -- "$exe" --ignored --exact --nocapture "$test" ;;
      *) fail "the loop list names the kind $kind" ;;
    esac
    passes=0
    run_index=0
    while [ "$run_index" -lt "$runs" ]; do
      run_index=$((run_index + 1))
      "$timeout_tool" "$test_limit_s" "$@" </dev/null >"$work/loop.log" 2>&1
      tr -d '\r' <"$work/loop.log" | sed -n "s/^TF pointer-queue /WF loop-print ${test##*::} run=$run_index pointer-queue /p"
      if tr -d '\r' <"$work/loop.log" | grep -q '^test result: ok\. 1 passed; 0 failed;'; then
        passes=$((passes + 1))
      else
        cp "$work/loop.log" "$artifacts/loop-${test##*::}-$run_index.log" || fail "a loop's log cannot be kept"
        tr -d '\r' <"$work/loop.log" | sed 's/^/    | /'
      fi
    done
    line="WF loop $test kind=$kind runs=$runs passed=$passes"
    printf '%s\n' "$line"
    printf '%s\n' "$line" >>"$artifacts/wf.txt" || fail "wf.txt cannot be written"
    [ "$passes" -eq "$runs" ] || loop_failed=$((loop_failed + 1))
  done <"$work/loops"
}

scan_log() {
  summary=
  panics=0
  panic_lines=
  markers=0
  marker=
  before_marker=
  timed_out=no
  prev=
  while IFS= read -r line || [ -n "$line" ]; do
    while :; do
      case $line in
        *"$cr"*) line=${line%%"$cr"*}${line#*"$cr"} ;;
        *) break ;;
      esac
    done
    printf '    | %s\n' "$line"
    case $line in
      'test result: '*) summary=${line#'test result: '} ;;
    esac
    case $line in
      "thread '"*" panicked at "*)
        panics=$((panics + 1))
        panic_lines=$panic_lines$line$nl
        ;;
    esac
    case $line in
      'KNOWN-DEFECT '*)
        markers=$((markers + 1))
        if [ "$markers" -eq 1 ]; then
          marker=$line
          before_marker=$prev
        fi
        ;;
    esac
    case $line in
      'TF '*' result=TIMEOUT'*) timed_out=yes ;;
    esac
    prev=$line
  done <"$1"
}

judge() {
  verdict=FAIL
  id=
  cause=
  scan_log "$3"
  case $1 in
    known_defect::*) ;;
    *)
      if [ "$2" -ne 0 ]; then
        cause="exit $2"
        return 0
      fi
      case $summary in
        'ok. 1 passed; 0 failed;'*) ;;
        *)
          cause='exit 0, and not one test ran and passed'
          return 0
          ;;
      esac
      if [ "$panics" -gt 0 ]; then
        cause='exit 0 with a panic on a thread of the test'
        return 0
      fi
      verdict=pass
      return 0
      ;;
  esac
  if [ "$2" -eq 0 ]; then
    case $summary in
      'ok. 1 passed; 0 failed;'*)
        verdict=FIXED-BUT-LISTED
        cause='the listed defect passed'
        ;;
      *) cause='exit 0, and not one test ran and passed' ;;
    esac
    return 0
  fi
  if [ "$markers" -eq 0 ]; then
    cause="exit $2 without a KNOWN-DEFECT line"
    return 0
  fi
  if [ "$markers" -gt 1 ]; then
    cause="exit $2 with more than one KNOWN-DEFECT line"
    return 0
  fi
  if [ "$2" -ne 101 ]; then
    cause="exit $2 with a KNOWN-DEFECT line, and not the exit 101 of one failed test"
    return 0
  fi
  case $summary in
    'FAILED. 0 passed; 1 failed;'*) ;;
    *)
      cause='exit 101 with a KNOWN-DEFECT line and no summary line of one failed test'
      return 0
      ;;
  esac
  got=${marker#KNOWN-DEFECT }
  case $got in
    '' | *[!A-Za-z0-9-]*)
      cause='a KNOWN-DEFECT line that holds more than the marker'
      return 0
      ;;
  esac
  if [ "$got" != "$4" ]; then
    cause="the marker names $got, and the list of this script gives ${4:-no id} for the test"
    return 0
  fi
  owner=
  case $before_marker in
    "thread '"*"'"*" panicked at "*)
      owner=${before_marker#"thread '"}
      owner=${owner%%"'"*}
      ;;
  esac
  if [ -z "$owner" ]; then
    cause='a KNOWN-DEFECT line that no panic line comes right before'
    return 0
  fi
  others=0
  rest=$panic_lines
  while [ -n "$rest" ]; do
    panic=${rest%%"$nl"*}
    rest=${rest#*"$nl"}
    case $panic in
      *"thread '$owner'"*) ;;
      *) others=$((others + 1)) ;;
    esac
  done
  if [ "$others" -gt 0 ]; then
    cause="a panic on another thread than the thread $owner of the marker"
    return 0
  fi
  if [ "$timed_out" = yes ]; then
    cause='the test reached its time limit'
    return 0
  fi
  verdict='known-defect'
  id=$got
}

run_test() {
  "$timeout_tool" "$test_limit_s" "$exe" --ignored --exact --nocapture "$1" </dev/null >"$2" 2>&1
}

measure() {
  check_started=$(date +%s)
  cd "$CI_D_REPO" || exit 2
  form=${HALVO_WF_FORM:-short}
  case $form in
    short | long) ;;
    *) fail "HALVO_WF_FORM is short or long, not $form" ;;
  esac
  HALVO_WF_FORM=$form
  export HALVO_WF_FORM
  artifacts=${CI_D_ARTIFACTS:-$CI_D_REPO/target/ci-artifacts/$name}
  mkdir -p "$artifacts" || fail "the artifacts directory cannot be made"
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail "mktemp failed"
  command -v "$timeout_tool" >/dev/null 2>&1 || fail "$timeout_tool is not installed, and each test runs under its time limit"
  if [ -n "${WF_EXPECTED_FILE:-}" ]; then
    tr -d '\r' <"$WF_EXPECTED_FILE" >"$work/expected" || fail "the expected list cannot be read"
  else
    expected_tests >"$work/expected"
  fi
  awk '{
      id = "-"; grp = ""; scope = "-"
      for (i = 2; i <= NF; i++) {
        if ($i ~ /^group=/) grp = substr($i, 7)
        else if ($i == "scope=long-only") scope = "long-only"
        else if (id == "-") id = $i
      }
      if (grp == "") grp = "native"
      print $1 "\t" grp "\t" scope "\t" id
    }' "$work/expected" >"$work/plan" || fail "the expected list cannot be read"
  stray=$(awk -F '\t' '$2 != "native" && $2 != "browser" { print $1 }' "$work/plan" | tr '\n' ' ')
  [ -z "$stray" ] || fail "the list puts these tests in neither script (the group is native or browser): ${stray% }"
  twice=$(cut -f 1 "$work/plan" | sort | uniq -d | tr '\n' ' ')
  [ -z "$twice" ] || fail "the list names these tests more than once, so a test stands in both scripts or twice in one: ${twice% }"
  cut -f 1 "$work/plan" | sort >"$work/expected-names" || fail "the expected list cannot be read"
  [ -f "$prints_rule" ] || fail "$prints_rule is missing, and its rule reads every test's log (ruling W3-29)"
  . ./scripts/r1/windows-prints-rule.sh || fail "$prints_rule cannot be read"
  (wpr_vocabulary "$work/vocabulary") || fail "wpr_vocabulary of $prints_rule exited $?, so no log can be read by its rule"

  cargo test --locked -p halvo-capture --test windows_functional --no-run --message-format json >"$work/build.json" ||
    fail "the test build failed: cargo test --no-run exited $?"
  exe=$(sed -n 's/.*"name":"windows_functional".*"executable":"\([^"]*\)".*/\1/p' "$work/build.json")
  [ -n "$exe" ] || fail "cargo named no executable for the test target windows_functional"
  case $exe in
    *"
"*) fail "cargo named more than one executable for the test target windows_functional" ;;
  esac
  exe=$(printf '%s\n' "$exe" | sed 's/\\\\/\//g')
  [ -f "$exe" ] || fail "the test executable that cargo named does not exist"

  "$exe" --list >"$work/list.txt" 2>&1 || fail "the test executable did not list its tests: exited $?"
  tr -d '\r' <"$work/list.txt" | sed -n 's/: test$//p' >"$work/names" || fail "the test list cannot be read"
  total=$(grep -c '' "$work/names")
  [ "$total" -gt 0 ] || fail "the test list is empty"
  if grep -v -q '^[A-Za-z0-9_:]*$' "$work/names"; then
    fail "the test list holds a name with a character outside A-Z, a-z, 0-9, _ and :"
  fi
  sort "$work/names" >"$work/listed-names" || fail "the test list cannot be sorted"
  distinct=$(sort -u "$work/names" | grep -c '')
  [ "$distinct" -eq "$total" ] || fail "the test executable lists a test more than once"
  missing=$(comm -23 "$work/expected-names" "$work/listed-names" | tr '\n' ' ')
  [ -z "$missing" ] || fail "the test executable does not list these tests of this script's list: ${missing% }"
  extra=$(comm -13 "$work/expected-names" "$work/listed-names" | tr '\n' ' ')
  [ -z "$extra" ] || fail "the test executable lists tests that this script's list does not hold: ${extra% }"

  : >"$artifacts/wf.txt" || fail "wf.txt cannot be written"
  passed=0
  known=0
  fixed=0
  failed=0
  rule_broken=0
  rule_failed=0
  long_only=0
  ran=0
  index=0
  ledger_view=
  [ ! -f "$ledger" ] ||
    ledger_view=$(tr -d '\r' <"$ledger" | sed -n 's/^measured view path=crates\/halvo-capture without=macos-only id=\([0-9a-f]*\)$/\1/p')
  while IFS= read -r test; do
    index=$((index + 1))
    log=$work/test-$index.log
    test_group=$(awk -F '\t' -v test="$test" '$1 == test { print $2 }' "$work/plan")
    [ "$test_group" = "$group" ] || continue
    scope=$(awk -F '\t' -v test="$test" '$1 == test { print $3 }' "$work/plan")
    if [ "$scope" = long-only ] && [ "$form" = short ]; then
      [ -n "$ledger_view" ] ||
        fail "$test runs in the long form alone, and $ledger holds no ledger id of the tree that a long run read for it"
      long_only=$((long_only + 1))
      line="WF $test result=long-form-only ledger=$ledger_view"
      printf '%s\n' "$line"
      printf '%s\n' "$line" >>"$artifacts/wf.txt" || fail "wf.txt cannot be written"
      continue
    fi
    ran=$((ran + 1))
    want_id=$(awk -F '\t' -v test="$test" '$1 == test && $4 != "-" { print $4 }' "$work/plan")
    run_test "$test" "$log.run"
    code=$?
    cp "$log.run" "$log" || fail "the log of $test cannot be copied for its one read"
    rule_out=$(wpr_check_log "$test" "$log" "$work/vocabulary" 2>&1)
    rule_code=$?
    time_rule=
    if [ "$rule_code" -eq 0 ]; then
      judge "$test" "$code" "$log" "$want_id"
      time_rule=$(tr -d '\r' <"$log" | sed -E -n 's/^TF .* (rounds_decided_by_the_time_rule=[0-9]+)( .*)?$/\1/p' | tail -n 1)
    else
      rule_lines=$(printf '%s\n' "$rule_out" | grep -E "$prints_rule_line")
      [ -z "$rule_lines" ] || printf '%s\n' "$rule_lines"
      broken=$(printf '%s\n' "$rule_lines" | grep -c -E "$prints_rule_line")
      rule_broken=$((rule_broken + broken))
      rule_failed=$((rule_failed + 1))
      verdict=FAIL
      id=
      cause="wpr_check_log exited $rule_code with $broken lines that break the prints rule, so its log is not printed"
    fi
    case $verdict in
      pass) passed=$((passed + 1)) ;;
      known-defect) known=$((known + 1)) ;;
      FIXED-BUT-LISTED) fixed=$((fixed + 1)) ;;
      *) failed=$((failed + 1)) ;;
    esac
    [ -z "$cause" ] || printf '%s: %s: %s\n' "$name" "$test" "$cause"
    line="WF $test result=$verdict${id:+ id=$id}${time_rule:+ $time_rule}"
    printf '%s\n' "$line"
    printf '%s\n' "$line" >>"$artifacts/wf.txt" || fail "wf.txt cannot be written"
  done <"$work/names"
  line="prints_rule broken_lines=$rule_broken withheld_logs=$rule_failed"
  printf '%s\n' "$line"
  printf '%s\n' "$line" >>"$artifacts/wf.txt" || fail "wf.txt cannot be written"
  [ "$rule_failed" -eq 0 ] ||
    fail "the logs of $rule_failed tests hold $rule_broken lines that break the prints rule of $prints_rule, and those logs are not printed: read the prints-rule lines above"

  form_line=
  : >"$work/left-out"
  index=0
  while IFS= read -r test; do
    index=$((index + 1))
    log=$work/test-$index.log
    [ -f "$log" ] || continue
    if tr -d '\r' <"$log" | grep -q -E '(^| )counted_rounds=0( |$)'; then
      fail "$test ran zero rounds of a phase that it names"
    fi
    if [ "$test" = "$form_test" ]; then
      form_line=$(tr -d '\r' <"$log" | sed -E -n 's/^(.* )?(form=(short|long) )/\2/p' | tail -n 1)
      tr -d '\r' <"$log" | sed -E -n 's/^(.* )?left_out /WF left out: /p' >"$work/left-out"
    fi
    any=$(tr -d '\r' <"$log" | grep -c -E '(^| )form=[a-z]+ ')
    same=$(tr -d '\r' <"$log" | grep -c -E "(^| )form=$form ")
    [ "$any" = "$same" ] || fail "$test printed a line of another form than $form"
  done <"$work/names"
  if [ "$group" = native ] && grep -q -x -F "$form_test" "$work/names"; then
    case $form_line in
      "form=$form "*) ;;
      *) fail "$form_test printed no line of the form $form" ;;
    esac
    printf 'WF %s\n' "$form_line" | tee -a "$artifacts/wf.txt" || fail "wf.txt cannot be written"
    tee -a "$artifacts/wf.txt" <"$work/left-out" || fail "wf.txt cannot be written"
  fi

  short=
  for state in gate_not_clear late_read_no_latch other; do
    found=0
    for log in "$work"/test-*.log; do
      [ -f "$log" ] || continue
      in_log=$(tr -d '\r' <"$log" | grep -c -E "(^| )control_state=$state( |$)")
      found=$((found + in_log))
    done
    short="$short $state=$found"
  done
  seconds_taken=0
  for log in "$work"/test-*.log; do
    [ -f "$log" ] || continue
    in_log=$(tr -d '\r' <"$log" | grep -c -E "(^| )second_control=taken( |$)")
    seconds_taken=$((seconds_taken + in_log))
  done
  slipped=0
  for log in "$work"/test-*.log; do
    [ -f "$log" ] || continue
    in_log=$(tr -d '\r' <"$log" | grep -c -E "(^| )control_state=schedule_slipped( |$)")
    slipped=$((slipped + in_log))
  done
  met=0
  runs_after_start=0
  for log in "$work"/test-*.log; do
    [ -f "$log" ] || continue
    in_log=$(tr -d '\r' <"$log" | grep -c -E "(^| )after_start_state=met( |$)")
    met=$((met + in_log))
    in_log=$(tr -d '\r' <"$log" | grep -c -E "(^| )after_start_state=(met|not_met)( |$)")
    runs_after_start=$((runs_after_start + in_log))
  done
  line="WF short controls$short second_controls_taken=$seconds_taken slipped_controls=$slipped"
  [ "$group" != browser ] || line="$line after_start_unknown=$met/$runs_after_start"
  printf '%s\n' "$line"
  printf '%s\n' "$line" >>"$artifacts/wf.txt" || fail "wf.txt cannot be written"

  for source in keyboard pointer window_title foreground screen; do
    not_ok=0
    for log in "$work"/test-*.log; do
      [ -f "$log" ] || continue
      found=$(tr -d '\r' <"$log" | grep -c "$shutdown_line$source ok=false")
      not_ok=$((not_ok + found))
    done
    line="shutdown_not_ok source=$source count=$not_ok"
    printf '%s\n' "$line"
    printf '%s\n' "$line" >>"$artifacts/wf.txt" || fail "wf.txt cannot be written"
  done

  line="WF total=$((ran + long_only)) pass=$passed known_defect=$known long_form_only=$long_only fail=$((failed + fixed)) group=$group"
  printf '%s\n' "$line"
  printf '%s\n' "$line" >>"$artifacts/wf.txt" || fail "wf.txt cannot be written"
  loop_failed=0
  [ "$form" = short ] || [ "$group" != native ] || loops
  [ "$fixed" -eq 0 ] || fail "$fixed listed defects passed (FIXED-BUT-LISTED): move each test out of mod known_defect"
  [ "$failed" -eq 0 ] || fail "$failed tests failed"
  [ "$seconds_taken" -le 1 ] ||
    fail "$seconds_taken second controls were taken in this run, and more than one fails the run (ruling W3-18 (13))"
  [ "$loop_failed" -eq 0 ] || fail "$loop_failed loops had a run that was not one passed test"
  if [ "$form" = long ]; then
    printf 'ledger: not read in the long form, whose passing run the ledger records\n'
  else
    ledger_check
  fi
  line="WF form=$form seconds=$(($(date +%s) - check_started))"
  printf '%s\n' "$line"
  printf '%s\n' "$line" >>"$artifacts/wf.txt" || fail "wf.txt cannot be written"
  printf 'PASS %s\n' "$name"
}

launch() {
  printf '%s\n' "$3" >"$work/spec-$1"
  if [ "$expected" = real ]; then
    expected_file=
  else
    expected_file=$work/expected-$1
    printf '%s\n' "$expected" >"$expected_file"
  fi
  (
    PATH=$work/bin:$PATH
    CI_D_REPO=${repo_knob:-$work/repo}
    CI_D_ARTIFACTS=$work/artifacts-$1
    STUB_SPEC=$work/spec-$1
    STUB_TF=$work/bin/tf-stub
    WF_EXPECTED_FILE=$expected_file
    STUB_SHUTDOWN_LINE=$test_file_shutdown_line
    WF_GROUP=$group_knob
    export WF_GROUP
    STUB_LIB=$work/bin/lib-stub
    export PATH CI_D_REPO CI_D_ARTIFACTS STUB_SPEC STUB_TF WF_EXPECTED_FILE STUB_EXE STUB_LIST_EXIT
    export STUB_SHUTDOWN_LINE STUB_LIB STUB_LIB_EXE STUB_LIB_FAIL
    unset HALVO_WF_FORM WF_LEDGER_FILE WF_LOOP_FILE
    if [ -n "$form_word" ]; then
      HALVO_WF_FORM=$form_word
      export HALVO_WF_FORM
    fi
    if [ -n "$ledger_knob" ]; then
      WF_LEDGER_FILE=$ledger_knob
      export WF_LEDGER_FILE
    fi
    if [ -n "$loop_list" ]; then
      WF_LOOP_FILE=$work/loops-$1
      printf '%s\n' "$loop_list" >"$WF_LOOP_FILE"
      export WF_LOOP_FILE
    fi
    if [ -n "$limit" ]; then
      WF_TEST_LIMIT_S=$limit
      export WF_TEST_LIMIT_S
    fi
    if [ -n "$tool" ]; then
      WF_TIMEOUT_TOOL=$tool
      export WF_TIMEOUT_TOOL
    fi
    sh "$0" >"$work/out-$1" 2>&1
    printf '%s\n' "$?" >"$work/code-$1"
  ) &
  running=$((running + 1))
  if [ "$running" -ge "$fixtures_at_once" ]; then
    wait
    running=0
  fi
}

check() {
  case_name=$1
  want=$2
  if [ "$pass" = launch ]; then
    launch "$@"
    reset_knobs
    return 0
  fi
  out=$(cat "$work/out-$case_name") || exit 2
  code=$(cat "$work/code-$case_name") || exit 2
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  shift 3
  for text in "$@"; do
    case $text in
      '!'*)
        case $out in
          *"${text#!}"*) got=$got-with-a-text-it-must-not-hold ;;
        esac
        ;;
      '@'*)
        case $(cat "$work/artifacts-$case_name/wf.txt" 2>/dev/null) in
          *"${text#@}"*) ;;
          *) got=$got-without-the-text-in-wf.txt ;;
        esac
        ;;
      *)
        case $out in
          *"$text"*) ;;
          *) got=$got-without-the-text ;;
        esac
        ;;
    esac
  done
  printf 'case %s/%s want=%s got=%s\n' "$name" "$case_name" "$want" "$got"
  if [ "$got" != "$want" ]; then
    wrong="$wrong $case_name"
  elif [ "$want" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
  reset_knobs
}

reset_knobs() {
  expected=$small
  STUB_EXE=one
  STUB_LIST_EXIT=0
  limit=
  tool=
  form_word=
  ledger_knob=
  repo_knob=
  group_knob=$group
  loop_list=
  STUB_LIB_EXE=one
  STUB_LIB_FAIL=
}

write_stubs() {
  cat >"$work/bin/cargo" <<'EOF'
#!/bin/sh
if [ "$*" = 'test --locked -p halvo-capture --lib --no-run --message-format json' ]; then
  lib=$(printf '%s\n' "$STUB_LIB" | sed 's/\//\\\\/g')
  printf '{"reason":"compiler-artifact","target":{"kind":["lib"],"crate_types":["lib"],"name":"halvo_capture","test":true},"profile":{"test":true},"executable":null,"fresh":true}\n'
  [ "$STUB_LIB_EXE" = none ] ||
    printf '{"reason":"compiler-artifact","target":{"kind":["lib"],"crate_types":["lib"],"name":"halvo_capture","test":true},"profile":{"test":true},"executable":"%s","fresh":true}\n' "$lib"
  printf '{"reason":"build-finished","success":true}\n'
  exit 0
fi
[ "$*" = 'test --locked -p halvo-capture --test windows_functional --no-run --message-format json' ] || exit 97
path=$(printf '%s\n' "$STUB_TF" | sed 's/\//\\\\/g')
artifact() {
  printf '{"reason":"compiler-artifact","target":{"kind":["test"],"crate_types":["bin"],"name":"windows_functional","test":true},"executable":"%s","fresh":true}\n' "$1"
}
printf '{"reason":"compiler-artifact","target":{"kind":["lib"],"name":"halvo_capture"},"executable":null,"fresh":true}\n'
case $STUB_EXE in
  one) artifact "$path" ;;
  two)
    artifact "$path"
    artifact "$path"
    ;;
  missing) artifact "$path-gone" ;;
  none) ;;
  *) exit 96 ;;
esac
printf '{"reason":"build-finished","success":true}\n'
EOF
  cat >"$work/bin/tf-stub" <<'EOF'
#!/bin/sh
if [ "$*" = --list ]; then
  count=0
  while IFS='|' read -r test kind id; do
    [ -n "$test" ] || continue
    printf '%s: test\r\n' "$test"
    count=$((count + 1))
  done <"$STUB_SPEC"
  printf '\n%s tests, 0 benchmarks\n' "$count"
  exit "$STUB_LIST_EXIT"
fi
[ "$#" -eq 4 ] && [ "$1 $2 $3" = '--ignored --exact --nocapture' ] || exit 97
test=$4
line=
while IFS= read -r spec; do
  case $spec in
    *"$test|"*)
      line=$spec
      break
      ;;
  esac
done <"$STUB_SPEC"
rest=${line#*|}
kind=${rest%%|*}
id=
case $rest in
  *'|'*)
    id=${rest#*|}
    id=${id%%|*}
    ;;
esac
thread=tf-${test##*::}
ok="test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out; finished in 0.01s"
failed="test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 3 filtered out; finished in 0.01s"
panic() {
  printf "\nthread '%s' (4242) panicked at crates\\\\halvo-capture\\\\tests\\\\windows_functional.rs:1:1:\r\n%s\r\n" "$1" "$2" >&2
}
printf '\nrunning 1 test\n'
case $kind in
  pass)
    printf 'TF %s source_built_by_test=keyboard\ntest %s ... ok\n\n%s\n\n' "$test" "$test" "$ok"
    exit 0
    ;;
  shutdown-not-ok)
    printf '%skeyboard ok=false\ntest %s ... ok\n\n%s\n\n' "$STUB_SHUTDOWN_LINE" "$test" "$ok"
    exit 0
    ;;
  rule-break | rule-unreadable | rule-leaky)
    case $kind in
      rule-break) printf 'TF %s typed=BREAKS-THE-RULE hunter2-typed-text\n' "$test" ;;
      rule-unreadable) printf 'TF %s RULE-CANNOT-READ hunter2-typed-text\n' "$test" ;;
      rule-leaky) printf 'TF %s LEAKS-TEXT hunter2-typed-text\n' "$test" ;;
    esac
    printf 'test %s ... ok\n\n%s\n\n' "$test" "$ok"
    exit 0
    ;;
  rule-stderr)
    printf 'TF %s STDERR-TEXT hunter2-typed-text\ntest %s ... ok\n\n%s\n\n' "$test" "$test" "$ok"
    exit 0
    ;;
  late-writer)
    printf 'TF %s SLOW-RULE source_built_by_test=keyboard\ntest %s ... ok\n\n%s\n\n' "$test" "$test" "$ok"
    (
      sleep 1
      printf 'TF late typed=BREAKS-THE-RULE hunter2-typed-text\n'
    ) &
    exit 0
    ;;
  rule-mixed)
    printf 'TF %s MIXED-ANSWER hunter2-typed-text\ntest %s ... ok\n\n%s\n\n' "$test" "$test" "$ok"
    exit 0
    ;;
  rule-break-late-line)
    for filler in 1 2 3 4 5 6 7 8 9 10; do
      printf 'TF %s filler=%s\n' "$test" "$filler"
    done
    printf 'TF %s typed=BREAKS-THE-RULE hunter2-typed-text\ntest %s ... ok\n\n%s\n\n' "$test" "$test" "$ok"
    exit 0
    ;;
  rule-break-fail)
    printf 'TF %s typed=BREAKS-THE-RULE hunter2-typed-text\n' "$test"
    panic "$thread" 'assertion failed: the field held hunter2-typed-text'
    printf 'test %s ... FAILED\n\n%s\n\n' "$test" "$failed"
    exit 101
    ;;
  rule-break-defect)
    printf 'TF %s typed=BREAKS-THE-RULE hunter2-typed-text\n' "$test"
    panic "$thread" "KNOWN-DEFECT $id"
    printf 'test %s ... FAILED\n\n%s\n\n' "$test" "$failed"
    exit 101
    ;;
  form-break)
    printf 'form=%s counted_rounds_at_each_wait_ms=150:20,200:5\n' "${HALVO_WF_FORM:-unset}"
    printf 'left_out BREAKS-THE-RULE hunter2-typed-text\n'
    printf 'test %s ... ok\n\n%s\n\n' "$test" "$ok"
    exit 0
    ;;
  form)
    printf 'form=%s counted_rounds_at_each_wait_ms=150:20,200:5\n' "${HALVO_WF_FORM:-unset}"
    [ "${HALVO_WF_FORM:-}" != short ] || printf 'left_out 15 of the 20 counted rounds at the wait of 200 ms\n'
    printf 'test %s ... ok\n\n%s\n\n' "$test" "$ok"
    exit 0
    ;;
  other-form)
    printf 'TF %s form=long rounds_of_the_phase=20\ntest %s ... ok\n\n%s\n\n' "$test" "$test" "$ok"
    exit 0
    ;;
  fakes)
    printf 'WF ghost_test_not_in_the_file result=pass\nWF %s result=known-defect id=RR-windows-99\n' "$test"
    printf 'WF total=99 pass=99 known_defect=0 fail=0\nPASS 50-windows-functional\n'
    printf 'test %s ... ok\n\n%s\n\n' "$test" "$ok"
    exit 0
    ;;
  defect | wrong-id | two-markers | marker-139 | marker-124 | marker-no-summary | other-thread | timeout-line | marker-more | marker-mid | marker-no-panic)
    printf 'TF %s keystroke_events_while_typing_into_password_field=5\r\n' "$test"
    case $kind in
      other-thread) panic "$thread-helper" 'called `Result::unwrap()` on an `Err` value' ;;
      timeout-line) printf 'TF %s result=TIMEOUT limit_secs=60\n' "$test" ;;
    esac
    case $kind in
      wrong-id) panic "$thread" 'KNOWN-DEFECT RR-windows-99' ;;
      two-markers)
        panic "$thread" "KNOWN-DEFECT $id"
        printf '%s\n' "KNOWN-DEFECT $id" >&2
        ;;
      marker-more) panic "$thread" "KNOWN-DEFECT $id and more" ;;
      marker-mid) panic "$thread" "assertion failed: KNOWN-DEFECT $id was expected" ;;
      marker-no-panic) printf '%s\n' "KNOWN-DEFECT $id" ;;
      *) panic "$thread" "KNOWN-DEFECT $id" ;;
    esac
    case $kind in
      marker-139) exit 139 ;;
      marker-124) exit 124 ;;
      marker-no-summary) exit 101 ;;
    esac
    printf 'test %s ... FAILED\n\nfailures:\n    %s\n\n%s\n\n' "$test" "$test" "$failed"
    exit 101
    ;;
  after-start-met)
    printf 'TF %s browser=edge after_start_state=met\ntest %s ... ok\n\n%s\n\n' "$test" "$test" "$ok"
    exit 0
    ;;
  time-rule)
    printf 'TF %s phase=burst counted=5 rounds_decided_by_the_time_rule=0 freed_at_ms_of_those_rounds=-\r\n' "$test"
    printf 'TF %s phase=burst counted=5 rounds_decided_by_the_time_rule=1\r\n' "$test"
    printf 'TF %s phase=burst counted=5 rounds_decided_by_the_time_rule= freed_at_ms_of_those_rounds=-\r\n' "$test"
    printf 'phase=burst rounds_decided_by_the_time_rule=7\r\n'
    printf 'test %s ... ok\n\n%s\n\n' "$test" "$ok"
    exit 0
    ;;
  time-rule-break)
    printf 'TF %s phase=burst counted=5 rounds_decided_by_the_time_rule=1 freed_at_ms_of_those_rounds=1564\n' "$test"
    printf 'TF %s typed=BREAKS-THE-RULE hunter2-typed-text\n' "$test"
    printf 'test %s ... ok\n\n%s\n\n' "$test" "$ok"
    exit 0
    ;;
  time-rule-fail)
    printf 'TF %s phase=burst counted=5 rounds_decided_by_the_time_rule=2 freed_at_ms_of_those_rounds=1564,1601\r\n' "$test"
    panic "$thread" 'the long form takes at most 1 round of the burst five that the time rule decided'
    printf 'test %s ... FAILED\n\n%s\n\n' "$test" "$failed"
    exit 101
    ;;
  after-start-not-met)
    printf 'TF %s browser=chrome after_start_state=not_met reason=a_letter_was_not_withheld_by_the_state_at_the_key\ntest %s ... ok\n\n%s\n\n' "$test" "$test" "$ok"
    exit 0
    ;;
  second-control)
    printf 'TF %s browser=edge control=before control_state=late_read_no_latch pump_largest_lag_ms=3\n' "$test"
    printf 'TF %s browser=edge control=before second_control=taken first_control_keystroke_events=0\n' "$test"
    printf 'TF %s browser=edge control=before-second control_state=full pump_largest_lag_ms=3\n' "$test"
    printf 'test %s ... ok\n\n%s\n\n' "$test" "$ok"
    exit 0
    ;;
  short-control)
    printf 'TF %s browser=edge control=before control_state=late_read_no_latch pump_largest_lag_ms=3\n' "$test"
    printf 'TF %s browser=edge control=after control_state=full pump_largest_lag_ms=3\n' "$test"
    panic "$thread" 'a control recorded fewer than 5 letters of the text input: control_before=0 control_after=5'
    printf 'test %s ... FAILED\n\n%s\n\n' "$test" "$failed"
    exit 101
    ;;
  inconclusive)
    panic "$thread" 'INCONCLUSIVE: SendInput inserted no events on this machine'
    printf 'test %s ... FAILED\n\n%s\n\n' "$test" "$failed"
    exit 101
    ;;
  zero-rounds)
    printf 'TF %s browser=edge phase=swap-timer rounds=1 counted_rounds=0\ntest %s ... ok\n\n%s\n\n' "$test" "$test" "$ok"
    exit 0
    ;;
  zero)
    printf '\nrunning 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 4 filtered out; finished in 0.00s\n\n'
    exit 0
    ;;
  pass-then-139)
    printf 'test %s ... ok\n\n%s\n\n' "$test" "$ok"
    exit 139
    ;;
  pass-with-panic)
    panic "$thread-helper" 'called `Option::unwrap()` on a `None` value'
    printf 'test %s ... ok\n\n%s\n\n' "$test" "$ok"
    exit 0
    ;;
  hang) exec sleep 30 ;;
  *) exit 98 ;;
esac
EOF
  cat >"$work/bin/lib-stub" <<'EOF'
#!/bin/sh
[ "$#" -eq 4 ] && [ "$1 $2 $3" = '--exact --nocapture --test-threads=1' ] || exit 97
printf '\nrunning 1 test\ntest %s ... ' "$4"
if [ "$4" = "$STUB_LIB_FAIL" ]; then
  printf 'FAILED\n\ntest result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 9 filtered out; finished in 0.01s\n\n'
  exit 101
fi
printf 'ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 9 filtered out; finished in 0.01s\n\n'
EOF
  chmod +x "$work/bin/cargo" "$work/bin/tf-stub" "$work/bin/lib-stub" || exit 2
}

make_ledger_repo() {
  repo=$work/repo
  mkdir -p "$repo/crates/halvo-capture/src/windows" "$repo/crates/halvo-capture/tests" "$repo/scripts/r1/ci.d" \
    "$repo/docs/baseline/ledger.d" "$repo/crates/halvo-capture/src/macos" || exit 2
  printf 'pub fn s() {}\n' >"$repo/crates/halvo-capture/src/source.rs" || exit 2
  printf 'fn m() {}\n' >"$repo/crates/halvo-capture/src/macos/input.rs" || exit 2
  printf 'fn a() {}\n' >"$repo/crates/halvo-capture/src/windows/input.rs" || exit 2
  printf 'pub mod windows;\n' >"$repo/crates/halvo-capture/src/secure_input.rs" || exit 2
  printf 'fn t() {}\n' >"$repo/crates/halvo-capture/tests/windows_functional.rs" || exit 2
  printf '[package]\nname = "halvo-capture"\n' >"$repo/crates/halvo-capture/Cargo.toml" || exit 2
  for crate in halvo-control halvo-sqlcipher halvo-route; do
    mkdir -p "$repo/crates/$crate/src" || exit 2
    printf '[package]\nname = "%s"\n' "$crate" >"$repo/crates/$crate/Cargo.toml" || exit 2
    printf 'pub fn c() {}\n' >"$repo/crates/$crate/src/lib.rs" || exit 2
  done
  printf '#!/bin/sh\n' >"$repo/scripts/r1/ci.d/50-windows-functional.sh" || exit 2
  cat >"$repo/scripts/r1/windows-prints-rule.sh" <<'EOF' || exit 2
wpr_vocabulary() {
  : >"$1"
}
wpr_check_log() {
  [ -r "$2" ] && [ -r "$3" ] || return 2
  if grep -q SLOW-RULE "$2"; then
    sleep 3
  fi
  if grep -q STDERR-TEXT "$2"; then
    printf 'prints-rule %s line=3 length=40 token=2\n' "$1"
    printf 'the line read: %s\n' "$(grep STDERR-TEXT "$2")" >&2
    return 1
  fi
  ! grep -q RULE-CANNOT-READ "$2" || return 2
  if grep -q MIXED-ANSWER "$2"; then
    printf 'prints-rule %s line=3 length=40 token=2\n' "$1"
    printf 'prints-rule %s line=3 length=40 token=2 hunter2-after-the-token\n' "$1"
    printf 'hunter2-before-the-name prints-rule %s line=3 length=40 token=2\n' "$1"
    printf 'prints-rule hunter2 in-the-name line=3 length=40 token=2\n'
    return 1
  fi
  if grep -q LEAKS-TEXT "$2"; then
    printf 'prints-rule %s line=1 text=%s\n' "$1" "$(grep LEAKS-TEXT "$2")"
    return 1
  fi
  awk -v test="$1" '/BREAKS-THE-RULE/ { n++; printf "prints-rule %s line=%d length=%d token=2\n", test, NR, length($0) } END { exit n ? 1 : 0 }' "$2"
}
EOF
  printf '[[package]]\nname = "serde"\nversion = "1.0.0"\n\n[[package]]\nname = "windows"\nversion = "0.62.2"\n' \
    >"$repo/Cargo.lock" || exit 2
  mkdir -p "$repo/crates/halvo-capture/src/custody" || exit 2
  mkdir -p "$repo/crates/halvo-capture/tests/macos_stall_fixture" || exit 2
  macos_only_paths | while IFS="$(printf '\t')" read -r path file kind; do
    case $kind in
      "cfg-mod "*) printf '// a line above\n#[cfg(target_os = "macos")]\npub mod %s;\n' "${kind#cfg-mod }" >"$repo/crates/halvo-capture/$file" ;;
      "path-mod "*)
        printf '#[path = "%s"]\npub mod %s;\n' "${path##*/}" "${kind#path-mod }" >"$repo/crates/halvo-capture/$file"
        printf 'fn h() {}\n' >"$repo/$path"
        ;;
      inner-cfg) printf '#![cfg(target_os = "macos")]\nfn f() {}\n' >"$repo/crates/halvo-capture/$file" ;;
      no-build) printf 'int main(void) { return 0; }\n' >"$repo/$path/fixture.m" ;;
    esac || exit 2
  done || exit 2
  (
    cd "$repo" || exit 2
    commit_all() {
      git add -A && git -c user.name=self-test -c user.email=self-test@halvo.invalid -c core.hooksPath=/dev/null \
        -c commit.gpgsign=false commit -q -m "$1"
    }
    ledger_at() {
      commit=$(git rev-parse "$1") || exit 2
      printf 'measured attempt=1 run=6 job=60 form=long result=fail commit=%s wf=total=3,pass=2,known_defect=0,fail=1 wf_native=total=2,pass=1,known_defect=0,fail=1 wf_browser=total=1,pass=1,known_defect=0,fail=0 short_controls=gate_not_clear=1,late_read_no_latch=0,other=0 second_controls_taken=0 slipped_controls=0\n' "$commit"
      printf 'measured attempt=2 run=6 job=61 form=long result=pass commit=%s wf=total=3,pass=2,known_defect=1,fail=0 wf_native=total=2,pass=2,known_defect=0,fail=0 wf_browser=total=1,pass=0,known_defect=1,fail=0 short_controls=gate_not_clear=0,late_read_no_latch=0,other=0 second_controls_taken=0 slipped_controls=1\n' "$commit"
      view=$(crate_view_id "$1") || exit 2
      printf 'measured view path=crates/halvo-capture without=macos-only id=%s\n' "$view"
      ledger_paths | while read -r path; do
        kind=$(git cat-file -t "$1:$path") || exit 2
        id=$(git rev-parse "$1:$path") || exit 2
        printf 'measured %s path=%s id=%s\n' "$kind" "$path" "$id"
      done
      lock=$(windows_lock_id "$1") || exit 2
      printf 'measured lock path=Cargo.lock crates=windows id=%s\n' "$lock"
    }
    git init -q . || exit 2
    commit_all fixture || exit 2
    first=$(git rev-parse HEAD) || exit 2
    printf 'pub fn s() { let _ = 1; }\n' >crates/halvo-capture/src/source.rs || exit 2
    commit_all "a changed source.rs" || exit 2
    second=$(git rev-parse HEAD) || exit 2
    printf 'fn m() { let _ = 2; }\n' >crates/halvo-capture/src/macos/input.rs || exit 2
    commit_all "a changed file under src/macos" || exit 2
    ledger_at HEAD >docs/baseline/ledger.d/win-3.txt || exit 2
    ledger_at "$first" >"$work/ledger-source-changed" || exit 2
    ledger_at "$second" >"$work/ledger-macos-changed" || exit 2
  ) || exit 2
  right_ledger=$repo/docs/baseline/ledger.d/win-3.txt
  git clone -q "$repo" "$work/repo-moved" || exit 2
  mkdir -p "$work/repo-moved/docs/baseline/ledger.d" || exit 2
  cp "$right_ledger" "$work/repo-moved/docs/baseline/ledger.d/win-3.txt" || exit 2
  (
    cd "$work/repo-moved" || exit 2
    printf '// a line above\npub mod macos;\n' >"$work/lib-moved.rs" || exit 2
    cp "$work/lib-moved.rs" crates/halvo-capture/src/lib.rs || exit 2
    git add -A && git -c user.name=self-test -c user.email=self-test@halvo.invalid -c core.hooksPath=/dev/null \
      -c commit.gpgsign=false commit -q -m "the macOS cfg line moved" || exit 2
  ) || exit 2
  for case_repo in mac-unit-changed hold-without-path hold-named-elsewhere inner-cfg-gone build-script rust-in-fixture \
    control-changed sqlcipher-changed other-crate-changed no-prints-rule vocabulary-fails prints-rule-changed; do
    git clone -q "$repo" "$work/repo-$case_repo" || exit 2
    mkdir -p "$work/repo-$case_repo/docs/baseline/ledger.d" || exit 2
    cp "$right_ledger" "$work/repo-$case_repo/docs/baseline/ledger.d/win-3.txt" || exit 2
    (
      cd "$work/repo-$case_repo/crates/halvo-capture" || exit 2
      case $case_repo in
        mac-unit-changed)
          printf 'fn h() { let _ = 3; }\n' >src/secure_input_macos_hold.rs &&
            printf '#![cfg(target_os = "macos")]\nfn f() { let _ = 3; }\n' >tests/macos_stall_fixture.rs &&
            printf 'int main(void) { return 3; }\n' >tests/macos_stall_fixture/fixture.m
          ;;
        hold-without-path) printf 'pub mod hold;\n' >src/secure_input_macos.rs ;;
        hold-named-elsewhere) printf '#[path = "secure_input_macos_hold.rs"]\nmod hold_too;\n' >>src/source.rs ;;
        inner-cfg-gone) printf 'fn f() {}\n' >tests/macos_stall_fixture.rs ;;
        build-script) printf 'fn main() {}\n' >build.rs ;;
        rust-in-fixture) printf 'fn g() {}\n' >tests/macos_stall_fixture/more.rs ;;
        control-changed) printf 'pub fn c() { let _ = 4; }\n' >../halvo-control/src/lib.rs ;;
        sqlcipher-changed) printf '[package]\nname = "halvo-sqlcipher"\nbuild = "build.rs"\n' >../halvo-sqlcipher/Cargo.toml ;;
        other-crate-changed) printf 'pub fn c() { let _ = 5; }\n' >../halvo-route/src/lib.rs ;;
        no-prints-rule) mv ../../scripts/r1/windows-prints-rule.sh ../../scripts/r1/windows-prints-rule.sh.gone ;;
        vocabulary-fails) printf 'wpr_vocabulary() {\n  return 2\n}\nwpr_check_log() {\n  return 0\n}\n' >../../scripts/r1/windows-prints-rule.sh ;;
        prints-rule-changed) printf 'wpr_unused() {\n  :\n}\n' >>../../scripts/r1/windows-prints-rule.sh ;;
      esac || exit 2
      git add -A && git -c user.name=self-test -c user.email=self-test@halvo.invalid -c core.hooksPath=/dev/null \
        -c commit.gpgsign=false commit -q -m "$case_repo" || exit 2
    ) || exit 2
  done
  sed 's/^measured attempt=2 /measured attempt=3 /' "$right_ledger" >"$work/ledger-out-of-order" || exit 2
  sed 's/^\(measured attempt=2 .*\) form=long /\1 form=short /' "$right_ledger" >"$work/ledger-short" || exit 2
  sed 's/^\(measured attempt=2 .*\) result=pass /\1 result=fail /' "$right_ledger" >"$work/ledger-newest-failed" || exit 2
  sed 's/^\(measured attempt=1 .*\) result=fail /\1 result=pass /' "$right_ledger" >"$work/ledger-pass-before" || exit 2
  { printf 'measured run=7 form=long result=pass figures=none\n' && cat "$right_ledger"; } >"$work/ledger-old-form" || exit 2
  sed -E 's#^(measured view path=crates/halvo-capture without=macos-only id=).*#\10000000000000000000000000000000000000000#' \
    "$right_ledger" >"$work/ledger-stale" || exit 2
  sed -E 's#^(measured lock path=Cargo.lock crates=windows id=).*#\10000000000000000000000000000000000000000#' \
    "$right_ledger" >"$work/ledger-lock" || exit 2
  grep -v 'path=scripts/r1/ci.d/50-windows-functional.sh ' "$right_ledger" >"$work/ledger-paths" || exit 2
  sed 's/ wf_browser=total=1,pass=0,known_defect=1,fail=0 / wf_browser=total=1,pass=0,known_defect=0,fail=0 /' "$right_ledger" >"$work/ledger-sums" || exit 2
  grep -v '^measured view ' "$right_ledger" >"$work/ledger-no-view" || exit 2
}

self_test() {
  test_file=$(cd "$(dirname "$0")/../../.." && pwd)/crates/halvo-capture/tests/windows_functional.rs
  prints_rule_file=$(cd "$(dirname "$0")/.." && pwd)/windows-prints-rule.sh
  test_file_shutdown_line=$(sed -n 's/^const SHUTDOWN_LINE: &str = "\(.*\)";$/\1/p' "$test_file")
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  mkdir -p "$work/bin" "$work/repo" || exit 2
  write_stubs
  make_ledger_repo
  rejected=0
  wrong=
  small="c1_raw_input${nl}helper_child_window${nl}known_defect::x1_password RR-windows-01"
  right="c1_raw_input|pass${nl}helper_child_window|pass${nl}known_defect::x1_password|defect|RR-windows-01"
  if [ "$group" = browser ]; then
    set_of_fixtures=browser_fixtures
    failing_fixtures=9
  else
    set_of_fixtures=fixtures
  fi
  reset_knobs
  running=0
  pass=launch
  $set_of_fixtures
  wait
  pass=judge
  $set_of_fixtures
  if [ "$group" = native ]; then
    rule_reads every-list-of-this-check-in-the-rule-s-read PASS none
    rule_reads the-macos-only-list-out-of-its-here-document-form FAIL macos_only_paths
    rule_reads the-test-list-out-of-its-here-document-form FAIL expected_tests
  fi

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  if [ "$rejected" -ne "$failing_fixtures" ]; then
    printf 'FAIL self-test:%s (rejected %s failing fixtures, and the self-test holds %s)\n' "$name" "$rejected" "$failing_fixtures"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

rule_reads() {
  dir=$work/rule-reads-$1
  mkdir -p "$dir/scripts/r1/ci.d" "$dir/crates/halvo-capture/src" || exit 2
  if [ "$3" = none ]; then
    cp "$0" "$dir/scripts/r1/ci.d/50-windows-functional.sh" || exit 2
  else
    "$3" >"$work/rule-reads-lines-$1" || exit 2
    awk -v fn="$3" -v lines="$work/rule-reads-lines-$1" '
      $0 == fn "() {" {
        print
        while ((getline line < lines) > 0) printf "  printf \047%%s\\n\047 \047%s\047\n", line
        skip = 1
        next
      }
      skip && $0 == "}" { skip = 0 }
      !skip { print }' "$0" >"$dir/scripts/r1/ci.d/50-windows-functional.sh" || exit 2
  fi
  printf 'fn g() { let zzwindowsbuilt = 1; }\n' >"$dir/crates/halvo-capture/src/lib.rs" || exit 2
  cp "$prints_rule_file" "$dir/scripts/r1/windows-prints-rule.sh" || exit 2
  count=0
  macos_only_paths | cut -f 1 >"$work/rule-reads-macos-$1" || exit 2
  while read -r path; do
    count=$((count + 1))
    case $path in
      *.rs) file=$dir/$path ;;
      *) file=$dir/$path/w.rs ;;
    esac
    mkdir -p "$(dirname "$file")" || exit 2
    printf 'fn f() { let zzmacosonly%s = 1; }\n' "$count" >"$file" || exit 2
  done <"$work/rule-reads-macos-$1"
  (
    cd "$dir" || exit 2
    git init -q . && git add -A && git -c user.name=self-test -c user.email=self-test@halvo.invalid -c core.hooksPath=/dev/null \
      -c commit.gpgsign=false commit -q -m "a stand-in of the paths that the prints rule reads" || exit 2
    . ./scripts/r1/windows-prints-rule.sh || exit 2
    wpr_vocabulary "$work/rule-reads-vocabulary-$1"
  ) >"$work/rule-reads-out-$1" 2>&1
  got=PASS
  grep -q -x -F zzwindowsbuilt "$work/rule-reads-vocabulary-$1" 2>/dev/null || got=FAIL
  ! grep -q '^zzmacosonly' "$work/rule-reads-vocabulary-$1" 2>/dev/null || got=FAIL
  expected_tests | awk '{ for (i = 2; i <= NF; i++) if ($i !~ /^(group|scope)=/) print "id:" $i }' | sort -u >"$work/rule-reads-ids-$1"
  while read -r wanted_id; do
    grep -q -x -F "$wanted_id" "$work/rule-reads-vocabulary-$1" 2>/dev/null || got=FAIL
  done <"$work/rule-reads-ids-$1"
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$got"
  if [ "$got" != "$2" ]; then
    wrong="$wrong $1"
  elif [ "$2" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

browser_fixtures() {
  mixed="c1_raw_input${nl}browser::a_page_test group=browser${nl}known_defect::a_held_swap W3-7-Q2i group=browser scope=long-only"
  mixed_spec="c1_raw_input|pass${nl}browser::a_page_test|pass${nl}known_defect::a_held_swap|defect|W3-7-Q2i"
  expected=$mixed
  check the-browser-script-runs-its-own-tests PASS "$mixed_spec" \
    "${nl}WF browser::a_page_test result=pass${nl}" \
    "${nl}WF known_defect::a_held_swap result=long-form-only ledger=" \
    "${nl}WF short controls gate_not_clear=0 late_read_no_latch=0 other=0 second_controls_taken=0 slipped_controls=0 after_start_unknown=0/0${nl}" \
    "${nl}WF total=2 pass=1 known_defect=0 long_form_only=1 fail=0 group=browser${nl}" \
    "${nl}PASS 52-windows-browser" "!${nl}WF c1_raw_input result="
  expected="browser::after_a group=browser${nl}browser::after_b group=browser"
  check the-runs-after-a-start-are-counted PASS "browser::after_a|after-start-met${nl}browser::after_b|after-start-not-met" \
    "${nl}WF short controls gate_not_clear=0 late_read_no_latch=0 other=0 second_controls_taken=0 slipped_controls=0 after_start_unknown=1/2${nl}" "${nl}PASS 52-windows-browser"
  expected=$mixed
  ledger_knob=$work/ledger-no-view
  check a-long-form-only-test-with-no-ledger-id FAIL "$mixed_spec" \
    "FAIL 52-windows-browser (known_defect::a_held_swap runs in the long form alone, and $work/ledger-no-view holds no ledger id"
  expected="c1_raw_input${nl}browser::a_page_test group=nowhere"
  check a-test-in-neither-script FAIL "c1_raw_input|pass${nl}browser::a_page_test|pass" \
    "FAIL 52-windows-browser (the list puts these tests in neither script (the group is native or browser): browser::a_page_test)"
  expected="c1_raw_input${nl}browser::a_page_test group=browser${nl}browser::a_page_test group=native"
  check a-test-in-both-scripts FAIL "c1_raw_input|pass${nl}browser::a_page_test|pass" \
    "FAIL 52-windows-browser (the list names these tests more than once, so a test stands in both scripts or twice in one: browser::a_page_test)"
  expected="c1_raw_input${nl}browser::a_page_test group=browser"
  check a-test-that-ran-zero-rounds FAIL "c1_raw_input|pass${nl}browser::a_page_test|zero-rounds" \
    "FAIL 52-windows-browser (browser::a_page_test ran zero rounds of a phase that it names)"
  expected="c1_raw_input${nl}browser::a_page_test group=browser"
  ledger_knob=$work/ledger-sums
  check the-two-scripts-lines-do-not-sum-to-the-ledgers FAIL "c1_raw_input|pass${nl}browser::a_page_test|pass" \
    "do not sum to its whole line in: known_defect"
  expected=$mixed
  ledger_knob=$right_ledger
  repo_knob=$work/repo-control-changed
  check a-changed-halvo-control-is-refused-by-the-browser-script FAIL "$mixed_spec" \
    "FAIL 52-windows-browser (the measured run 6 ran on another crates/halvo-control than the head's"
  expected=$mixed
  check a-page-test-log-that-breaks-the-prints-rule FAIL "c1_raw_input|pass${nl}browser::a_page_test|rule-break${nl}known_defect::a_held_swap|defect|W3-7-Q2i" \
    "prints-rule browser::a_page_test line=3 length=" "${nl}prints_rule broken_lines=1 withheld_logs=1${nl}" \
    "FAIL 52-windows-browser (the logs of 1 tests hold 1 lines that break the prints rule" "!hunter2-typed-text"
  expected="c1_raw_input${nl}browser::a_page_test group=browser${nl}browser::b_page_test group=browser"
  check the-wf-line-carries-the-last-count-of-rounds-that-the-time-rule-decided PASS "c1_raw_input|pass${nl}browser::a_page_test|time-rule${nl}browser::b_page_test|pass" \
    "${nl}WF browser::a_page_test result=pass rounds_decided_by_the_time_rule=1${nl}" \
    "@WF browser::a_page_test result=pass rounds_decided_by_the_time_rule=1${nl}" \
    "${nl}WF browser::b_page_test result=pass${nl}" "${nl}PASS 52-windows-browser"
  expected="c1_raw_input${nl}browser::a_page_test group=browser${nl}browser::b_page_test group=browser"
  check a-log-that-the-prints-rule-refused-is-not-read-for-the-count FAIL "c1_raw_input|pass${nl}browser::a_page_test|time-rule${nl}browser::b_page_test|time-rule-break" \
    "${nl}WF browser::a_page_test result=pass rounds_decided_by_the_time_rule=1${nl}" \
    "${nl}WF browser::b_page_test result=FAIL${nl}" "!hunter2-typed-text"
  expected="c1_raw_input${nl}browser::a_page_test group=browser"
  check a-failed-test-s-wf-line-carries-its-count-of-rounds-that-the-time-rule-decided FAIL "c1_raw_input|pass${nl}browser::a_page_test|time-rule-fail" \
    "${nl}WF browser::a_page_test result=FAIL rounds_decided_by_the_time_rule=2${nl}" \
    "@WF browser::a_page_test result=FAIL rounds_decided_by_the_time_rule=2${nl}" "FAIL 52-windows-browser (1 tests failed)"
}

fixtures() {
  check every-test-right PASS "$right" "${nl}prints_rule broken_lines=0 withheld_logs=0${nl}" "@prints_rule broken_lines=0 withheld_logs=0" \
    "${nl}    | TF c1_raw_input source_built_by_test=keyboard${nl}" \
    "${nl}WF short controls gate_not_clear=0 late_read_no_latch=0 other=0 second_controls_taken=0 slipped_controls=0${nl}" \
    "${nl}WF c1_raw_input result=pass${nl}" "${nl}WF helper_child_window result=pass${nl}" \
    "${nl}WF known_defect::x1_password result=known-defect id=RR-windows-01${nl}" \
    "${nl}WF total=3 pass=2 known_defect=1 long_form_only=0 fail=0 group=native${nl}ledger: the measured run 6 of the long form, attempt 2, " "${nl}WF form=short seconds=" "${nl}PASS $name"
  real=$(expected_tests | awk -v form="$form_test" '{
    id = ""
    for (i = 2; i <= NF; i++) if ($i !~ /^group=/ && $i !~ /^scope=/ && id == "") id = $i
    if (id != "") print $1 "|defect|" id; else if ($1 == form) print $1 "|form"; else print $1 "|pass"
  }')
  real_total=$(expected_tests | grep -c ' group=native$')
  real_known=$(expected_tests | awk '/ group=native$/ && $2 !~ /^group=/' | grep -c '')
  expected=real
  check every-test-of-the-real-list-right PASS "$real" \
    "${nl}WF c1_raw_input_delivers_keystrokes_and_pointer_events result=pass${nl}" "${nl}WF known_defect::c5_watch_armed_while_the_per_user_key_is_absent result=known-defect id=RR-windows-12${nl}" "!${nl}WF browser::" \
    "${nl}shutdown_not_ok source=keyboard count=0${nl}" \
    "${nl}WF form=short counted_rounds_at_each_wait_ms=150:20,200:5${nl}" \
    "${nl}WF left out: 15 of the 20 counted rounds at the wait of 200 ms${nl}" \
    "${nl}ledger: the measured run 6 of the long form, attempt 2, ran on what the head has of crates/halvo-capture" \
    "${nl}WF total=$real_total pass=$((real_total - real_known)) known_defect=$real_known long_form_only=0 fail=0 group=native${nl}ledger: " "${nl}WF form=short seconds=" \
    "${nl}PASS $name"
  check a-not-ok-shutdown-in-the-form-of-the-test-file-is-counted PASS "c1_raw_input|shutdown-not-ok${nl}helper_child_window|pass${nl}known_defect::x1_password|defect|RR-windows-01" \
    "${nl}shutdown_not_ok source=keyboard count=1${nl}" \
    "${nl}WF total=3 pass=2 known_defect=1 long_form_only=0 fail=0 group=native${nl}ledger: the measured run 6 of the long form, attempt 2, " "${nl}WF form=short seconds=" "${nl}PASS $name"
  check a-test-prints-the-lines-of-the-script PASS "c1_raw_input|fakes${nl}helper_child_window|pass${nl}known_defect::x1_password|defect|RR-windows-01" \
    "${nl}    | WF ghost_test_not_in_the_file result=pass${nl}" \
    "${nl}WF total=3 pass=2 known_defect=1 long_form_only=0 fail=0 group=native${nl}ledger: the measured run 6 of the long form, attempt 2, " "${nl}WF form=short seconds=" "${nl}PASS $name" \
    "!${nl}WF ghost_test_not_in_the_file" "!${nl}WF total=99" "!${nl}PASS $name${nl}" \
    "!${nl}WF c1_raw_input result=known-defect"

  check kd-test-passes FAIL "c1_raw_input|pass${nl}helper_child_window|pass${nl}known_defect::x1_password|pass" \
    "${nl}WF known_defect::x1_password result=FIXED-BUT-LISTED${nl}" \
    "${nl}WF total=3 pass=2 known_defect=0 long_form_only=0 fail=1 group=native${nl}FAIL $name (1 listed defects passed (FIXED-BUT-LISTED): move each test out of mod known_defect)"
  check kd-test-fails-without-the-marker FAIL "c1_raw_input|pass${nl}helper_child_window|pass${nl}known_defect::x1_password|inconclusive" \
    "${nl}$name: known_defect::x1_password: exit 101 without a KNOWN-DEFECT line${nl}WF known_defect::x1_password result=FAIL${nl}" \
    "${nl}WF total=3 pass=2 known_defect=0 long_form_only=0 fail=1 group=native${nl}FAIL $name (1 tests failed)"
  check zero-tests-ran FAIL "c1_raw_input|zero${nl}helper_child_window|pass${nl}known_defect::x1_password|defect|RR-windows-01" \
    "${nl}$name: c1_raw_input: exit 0, and not one test ran and passed${nl}WF c1_raw_input result=FAIL${nl}" \
    "${nl}WF total=3 pass=1 known_defect=1 long_form_only=0 fail=1 group=native${nl}FAIL $name (1 tests failed)"
  check empty-list FAIL '' "FAIL $name (the test list is empty)"
  check control-failed FAIL "c1_raw_input|inconclusive${nl}helper_child_window|pass${nl}known_defect::x1_password|defect|RR-windows-01" \
    "${nl}$name: c1_raw_input: exit 101${nl}WF c1_raw_input result=FAIL${nl}" \
    "${nl}WF total=3 pass=1 known_defect=1 long_form_only=0 fail=1 group=native${nl}FAIL $name (1 tests failed)"
  check two-markers FAIL "c1_raw_input|pass${nl}helper_child_window|pass${nl}known_defect::x1_password|two-markers|RR-windows-01" \
    "${nl}$name: known_defect::x1_password: exit 101 with more than one KNOWN-DEFECT line${nl}WF known_defect::x1_password result=FAIL${nl}"
  check a-marker-inside-a-longer-line FAIL "c1_raw_input|pass${nl}helper_child_window|pass${nl}known_defect::x1_password|marker-mid|RR-windows-01" \
    "${nl}$name: known_defect::x1_password: exit 101 without a KNOWN-DEFECT line${nl}"
  check a-marker-with-more-on-its-line FAIL "c1_raw_input|pass${nl}helper_child_window|pass${nl}known_defect::x1_password|marker-more|RR-windows-01" \
    "${nl}$name: known_defect::x1_password: a KNOWN-DEFECT line that holds more than the marker${nl}"
  check a-marker-with-exit-139 FAIL "c1_raw_input|pass${nl}helper_child_window|pass${nl}known_defect::x1_password|marker-139|RR-windows-01" \
    "${nl}$name: known_defect::x1_password: exit 139 with a KNOWN-DEFECT line, and not the exit 101 of one failed test${nl}"
  check a-marker-with-exit-124 FAIL "c1_raw_input|pass${nl}helper_child_window|pass${nl}known_defect::x1_password|marker-124|RR-windows-01" \
    "${nl}$name: known_defect::x1_password: exit 124 with a KNOWN-DEFECT line, and not the exit 101 of one failed test${nl}"
  check a-marker-with-exit-101-and-no-summary-line FAIL "c1_raw_input|pass${nl}helper_child_window|pass${nl}known_defect::x1_password|marker-no-summary|RR-windows-01" \
    "${nl}$name: known_defect::x1_password: exit 101 with a KNOWN-DEFECT line and no summary line of one failed test${nl}"
  check a-regular-test-passes-then-exits-139 FAIL "c1_raw_input|pass-then-139${nl}helper_child_window|pass${nl}known_defect::x1_password|defect|RR-windows-01" \
    "${nl}$name: c1_raw_input: exit 139${nl}WF c1_raw_input result=FAIL${nl}"
  STUB_LIST_EXIT=3
  check the-list-call-fails FAIL "$right" "FAIL $name (the test executable did not list its tests: exited 3)"
  limit=1
  check a-test-hangs-past-its-limit FAIL "c1_raw_input|hang${nl}helper_child_window|pass${nl}known_defect::x1_password|defect|RR-windows-01" \
    "${nl}$name: c1_raw_input: exit 124${nl}WF c1_raw_input result=FAIL${nl}"
  STUB_EXE=none
  check cargo-names-no-executable FAIL "$right" "FAIL $name (cargo named no executable for the test target windows_functional)"
  STUB_EXE=two
  check cargo-names-two-executables FAIL "$right" "FAIL $name (cargo named more than one executable for the test target windows_functional)"
  STUB_EXE=missing
  check the-named-executable-is-missing FAIL "$right" "FAIL $name (the test executable that cargo named does not exist)"
  expected="c1-raw-input${nl}helper_child_window"
  check a-name-outside-the-alphabet FAIL "c1-raw-input|pass${nl}helper_child_window|pass" \
    "FAIL $name (the test list holds a name with a character outside A-Z, a-z, 0-9, _ and :)"
  check the-binary-lacks-a-test-of-the-list FAIL "c1_raw_input|pass${nl}helper_child_window|pass" \
    "FAIL $name (the test executable does not list these tests of this script's list: known_defect::x1_password)"
  check the-binary-lists-a-test-that-the-list-lacks FAIL "$right${nl}c99_new_test|pass" \
    "FAIL $name (the test executable lists tests that this script's list does not hold: c99_new_test)"
  check the-binary-lists-a-test-twice FAIL "$right${nl}c1_raw_input|pass" \
    "FAIL $name (the test executable lists a test more than once)"
  check a-marker-of-another-id FAIL "c1_raw_input|pass${nl}helper_child_window|pass${nl}known_defect::x1_password|wrong-id|RR-windows-01" \
    "${nl}$name: known_defect::x1_password: the marker names RR-windows-99, and the list of this script gives RR-windows-01 for the test${nl}"
  check a-panic-on-another-thread-than-the-marker FAIL "c1_raw_input|pass${nl}helper_child_window|pass${nl}known_defect::x1_password|other-thread|RR-windows-01" \
    "${nl}$name: known_defect::x1_password: a panic on another thread than the thread tf-x1_password of the marker${nl}"
  check a-time-limit-line-then-the-marker FAIL "c1_raw_input|pass${nl}helper_child_window|pass${nl}known_defect::x1_password|timeout-line|RR-windows-01" \
    "${nl}$name: known_defect::x1_password: the test reached its time limit${nl}"
  check a-marker-that-no-panic-line-comes-before FAIL "c1_raw_input|pass${nl}helper_child_window|pass${nl}known_defect::x1_password|marker-no-panic|RR-windows-01" \
    "${nl}$name: known_defect::x1_password: a KNOWN-DEFECT line that no panic line comes right before${nl}"
  check a-regular-test-passes-with-a-panic-on-another-thread FAIL "c1_raw_input|pass-with-panic${nl}helper_child_window|pass${nl}known_defect::x1_password|defect|RR-windows-01" \
    "${nl}$name: c1_raw_input: exit 0 with a panic on a thread of the test${nl}"
  check a-short-control-is-summed-by-its-state FAIL "c1_raw_input|short-control${nl}helper_child_window|pass${nl}known_defect::x1_password|defect|RR-windows-01" \
    "${nl}WF short controls gate_not_clear=0 late_read_no_latch=1 other=0 second_controls_taken=0 slipped_controls=0${nl}" "FAIL $name (1 tests failed)"
  check one-second-control-is-taken PASS "c1_raw_input|second-control${nl}helper_child_window|pass${nl}known_defect::x1_password|defect|RR-windows-01" \
    "${nl}WF short controls gate_not_clear=0 late_read_no_latch=1 other=0 second_controls_taken=1 slipped_controls=0${nl}" "${nl}PASS $name"
  check two-second-controls-in-one-run FAIL "c1_raw_input|second-control${nl}helper_child_window|second-control${nl}known_defect::x1_password|defect|RR-windows-01" \
    "${nl}WF short controls gate_not_clear=0 late_read_no_latch=2 other=0 second_controls_taken=2 slipped_controls=0${nl}" \
    "FAIL $name (2 second controls were taken in this run, and more than one fails the run (ruling W3-18 (13)))"
  tool=no-such-timeout-tool
  check the-time-limit-tool-is-missing FAIL "$right" "FAIL $name (no-such-timeout-tool is not installed, and each test runs under its time limit)"

  form_word=medium
  check the-form-word-is-not-short-or-long FAIL "$right" "FAIL $name (HALVO_WF_FORM is short or long, not medium)"
  expected="$small${nl}$form_test"
  check the-round-count-test-prints-no-form FAIL "$right${nl}$form_test|pass" \
    "FAIL $name ($form_test printed no line of the form short)"
  check a-test-prints-a-line-of-another-form FAIL "c1_raw_input|other-form${nl}helper_child_window|pass${nl}known_defect::x1_password|defect|RR-windows-01" \
    "FAIL $name (c1_raw_input printed a line of another form than short)"
  ledger_knob=$work/no-such-ledger
  check the-ledger-is-missing FAIL "$right" "FAIL $name ($work/no-such-ledger is missing: run the long form"
  ledger_knob=$work/ledger-out-of-order
  check the-ledger-numbers-its-attempts-out-of-order FAIL "$right" "numbers its attempts 1 3 and not 1 to 2 in order"
  ledger_knob=$work/ledger-short
  check the-ledger-records-an-attempt-of-the-short-form FAIL "$right" "holds an attempt line that is not in the form of the long form"
  ledger_knob=$work/ledger-newest-failed
  check the-ledger-s-newest-attempt-failed FAIL "$right" "the newest attempt that $work/ledger-newest-failed records did not pass"
  ledger_knob=$work/ledger-pass-before
  check the-ledger-records-a-pass-before-its-newest-attempt FAIL "$right" "records a passed attempt before its newest one"
  ledger_knob=$work/ledger-old-form
  check the-ledger-holds-a-run-line-of-the-old-form FAIL "$right" "holds a line of the form measured run=, which records no attempts"
  ledger_knob=$work/ledger-stale
  check the-ledger-holds-another-tree-of-the-crate FAIL "$right" "the measured run 6 ran on another tree of crates/halvo-capture, without its macOS-only paths, than the head's (0000000000000000000000000000000000000000 against "
  ledger_knob=$work/ledger-source-changed
  check a-changed-source-rs-is-refused FAIL "$right" "the measured run 6 ran on another tree of crates/halvo-capture, without its macOS-only paths, than the head's"
  ledger_knob=$work/ledger-macos-changed
  repo_knob=$work/repo-moved
  check a-macos-module-without-its-cfg-is-refused FAIL "$right" "crates/halvo-capture/src/lib.rs no longer declares mod macos under the macOS cfg that leaves crates/halvo-capture/src/macos out"
  ledger_knob=$work/ledger-macos-changed
  check a-changed-file-under-src-macos-is-not-refused PASS "$right" "${nl}ledger: the measured run 6 of the long form, attempt 2, ran on what the head has of crates/halvo-capture without its macOS-only paths" "${nl}PASS $name"
  ledger_knob=$right_ledger
  repo_knob=$work/repo-mac-unit-changed
  check the-mac-unit-s-hold-file-and-stall-fixture-changed-are-not-refused PASS "$right" "${nl}ledger: the measured run 6 of the long form, attempt 2, ran on what the head has of crates/halvo-capture without its macOS-only paths" "${nl}PASS $name"
  ledger_knob=$right_ledger
  repo_knob=$work/repo-hold-without-path
  check a-hold-module-without-its-path-attribute-is-refused FAIL "$right" "crates/halvo-capture/src/secure_input_macos.rs no longer declares mod hold by #[path = \"secure_input_macos_hold.rs\"]"
  ledger_knob=$right_ledger
  repo_knob=$work/repo-hold-named-elsewhere
  check a-windows-built-file-that-names-the-hold-file-is-refused FAIL "$right" "a file that the Windows target can build names crates/halvo-capture/src/secure_input_macos_hold.rs, which the list leaves out of the ledger's tree: crates/halvo-capture/src/source.rs"
  ledger_knob=$right_ledger
  repo_knob=$work/repo-inner-cfg-gone
  check a-stall-fixture-without-its-inner-cfg-is-refused FAIL "$right" "crates/halvo-capture/tests/macos_stall_fixture.rs no longer starts with the line #![cfg(target_os = \"macos\")]"
  ledger_knob=$right_ledger
  repo_knob=$work/repo-build-script
  check a-build-script-of-the-crate-is-refused FAIL "$right" "crates/halvo-capture has a build script, which can compile what crates/halvo-capture/tests/macos_stall_fixture holds"
  ledger_knob=$right_ledger
  repo_knob=$work/repo-rust-in-fixture
  check a-rust-file-under-the-stall-fixture-is-refused FAIL "$right" "crates/halvo-capture/tests/macos_stall_fixture holds Rust files, which a test target can compile: crates/halvo-capture/tests/macos_stall_fixture/more.rs"
  ledger_knob=$right_ledger
  repo_knob=$work/repo-control-changed
  check a-changed-halvo-control-is-refused FAIL "$right" "the measured run 6 ran on another crates/halvo-control than the head's"
  ledger_knob=$right_ledger
  repo_knob=$work/repo-sqlcipher-changed
  check a-changed-halvo-sqlcipher-is-refused FAIL "$right" "the measured run 6 ran on another crates/halvo-sqlcipher than the head's"
  check two-logs-that-break-the-prints-rule-are-not-printed FAIL "c1_raw_input|rule-break${nl}helper_child_window|rule-break${nl}known_defect::x1_password|defect|RR-windows-01" \
    "prints-rule c1_raw_input line=3 length=" "$name: c1_raw_input: wpr_check_log exited 1 with 1 lines that break the prints rule, so its log is not printed${nl}WF c1_raw_input result=FAIL${nl}" \
    "${nl}prints-rule helper_child_window line=3 length=" "${nl}prints_rule broken_lines=2 withheld_logs=2${nl}" \
    "FAIL $name (the logs of 2 tests hold 2 lines that break the prints rule of scripts/r1/windows-prints-rule.sh, and those logs are not printed" \
    "!hunter2-typed-text"
  check a-log-that-the-prints-rule-cannot-read-fails FAIL "c1_raw_input|rule-unreadable${nl}helper_child_window|pass${nl}known_defect::x1_password|defect|RR-windows-01" \
    "$name: c1_raw_input: wpr_check_log exited 2 with 0 lines that break the prints rule, so its log is not printed${nl}" \
    "${nl}prints_rule broken_lines=0 withheld_logs=1${nl}" "!hunter2-typed-text" \
    "FAIL $name (the logs of 1 tests hold 0 lines that break the prints rule of scripts/r1/windows-prints-rule.sh, and those logs are not printed"
  check a-failing-test-and-a-known-defect-whose-logs-break-the-rule FAIL "c1_raw_input|rule-break-fail${nl}helper_child_window|pass${nl}known_defect::x1_password|rule-break-defect|RR-windows-01" \
    "prints-rule c1_raw_input line=3 length=" "${nl}prints-rule known_defect::x1_password line=3 length=" "${nl}prints_rule broken_lines=2 withheld_logs=2${nl}" \
    "FAIL $name (the logs of 2 tests hold 2 lines that break the prints rule" "!hunter2-typed-text" "@prints_rule broken_lines=2 withheld_logs=2"
  expected="$small${nl}$form_test"
  check a-form-test-log-that-breaks-the-rule-stops-before-the-form-reads FAIL "$right${nl}$form_test|form-break" \
    "${nl}prints-rule $form_test line=4 length=" "FAIL $name (the logs of 1 tests hold 1 lines that break the prints rule" \
    "!${nl}WF form=" "!WF left out:" "!hunter2-typed-text"
  check a-rule-answer-that-mixes-a-strict-line-and-leaky-ones-prints-the-strict-line-alone FAIL "c1_raw_input|rule-mixed${nl}helper_child_window|pass${nl}known_defect::x1_password|defect|RR-windows-01" \
    "prints-rule c1_raw_input line=3 length=40 token=2${nl}" "${nl}prints_rule broken_lines=1 withheld_logs=1${nl}" "!hunter2"
  check the-rule-s-standard-error-is-read-and-never-printed FAIL "c1_raw_input|rule-stderr${nl}helper_child_window|pass${nl}known_defect::x1_password|defect|RR-windows-01" \
    "prints-rule c1_raw_input line=3 length=40 token=2${nl}" "${nl}prints_rule broken_lines=1 withheld_logs=1${nl}" "!hunter2-typed-text"
  check a-line-written-after-the-test-ended-is-not-read PASS "c1_raw_input|late-writer${nl}helper_child_window|pass${nl}known_defect::x1_password|defect|RR-windows-01" \
    "${nl}prints_rule broken_lines=0 withheld_logs=0${nl}" "${nl}PASS $name" "!hunter2-typed-text"
  check a-breaking-line-past-line-9 FAIL "c1_raw_input|rule-break-late-line${nl}helper_child_window|pass${nl}known_defect::x1_password|defect|RR-windows-01" \
    "prints-rule c1_raw_input line=13 length=" "${nl}prints_rule broken_lines=1 withheld_logs=1${nl}" "!hunter2-typed-text"
  check a-breaking-log-after-a-known-defect-names-no-id FAIL "known_defect::x1_password|defect|RR-windows-01${nl}c1_raw_input|rule-break${nl}helper_child_window|pass" \
    "${nl}WF known_defect::x1_password result=known-defect id=RR-windows-01${nl}" "${nl}WF c1_raw_input result=FAIL${nl}" "!hunter2-typed-text"
  check a-rule-line-that-holds-text-is-not-printed FAIL "c1_raw_input|rule-leaky${nl}helper_child_window|pass${nl}known_defect::x1_password|defect|RR-windows-01" \
    "$name: c1_raw_input: wpr_check_log exited 1 with 0 lines that break the prints rule, so its log is not printed${nl}" \
    "!hunter2-typed-text" "!LEAKS-TEXT"
  repo_knob=$work/repo-no-prints-rule
  check the-prints-rule-is-missing FAIL "$right" "FAIL $name (scripts/r1/windows-prints-rule.sh is missing, and its rule reads every test's log (ruling W3-29))"
  repo_knob=$work/repo-vocabulary-fails
  check the-prints-rule-cannot-write-its-vocabulary FAIL "$right" "FAIL $name (wpr_vocabulary of scripts/r1/windows-prints-rule.sh exited 2, so no log can be read by its rule)"
  ledger_knob=$right_ledger
  repo_knob=$work/repo-prints-rule-changed
  check a-changed-prints-rule-is-refused FAIL "$right" "the measured run 6 ran on another scripts/r1/windows-prints-rule.sh than the head's"
  ledger_knob=$right_ledger
  repo_knob=$work/repo-other-crate-changed
  check a-changed-crate-outside-the-bound-trees-is-not-refused PASS "$right" \
    "${nl}ledger: the measured run 6 of the long form, attempt 2, ran on what the head has of crates/halvo-capture without its macOS-only paths, of the check and of its prints rule, of crates/halvo-control and crates/halvo-sqlcipher, which the Windows build of the capture crate compiles, and of the windows lines of Cargo.lock${nl}" \
    "${nl}PASS $name"
  ledger_knob=$work/ledger-lock
  check the-ledger-holds-other-windows-lock-lines FAIL "$right" "the measured run 6 ran on other windows lines of Cargo.lock than the head's"
  ledger_knob=$work/ledger-paths
  check the-ledger-lacks-a-listed-path FAIL "$right" "records other paths than the listed ones, or a path more than once"
  form_word=long
  loop_list="lib 3 a::plain_one${nl}functional 2 c1_raw_input"
  check the-long-form-loops-its-list PASS "$right" \
    "${nl}WF loop a::plain_one kind=lib runs=3 passed=3${nl}WF loop c1_raw_input kind=functional runs=2 passed=2${nl}" \
    "${nl}ledger: not read in the long form, whose passing run the ledger records${nl}WF form=long seconds=" \
    "!${nl}ledger: the measured run"
  form_word=long
  loop_list="lib 3 a::plain_one${nl}lib 2 a::flaky_one"
  STUB_LIB_FAIL=a::flaky_one
  check a-loop-run-that-fails FAIL "$right" \
    "${nl}WF loop a::flaky_one kind=lib runs=2 passed=0${nl}" \
    "FAIL $name (1 loops had a run that was not one passed test)"
  form_word=long
  loop_list="lib 3 a::plain_one"
  STUB_LIB_EXE=none
  check cargo-names-no-library-test-executable FAIL "$right" "FAIL $name (cargo named no test executable of the library)"
  form_word=long
  loop_list="lib 2 a::plain_one"
  check the-long-form-loops-when-a-test-failed FAIL "c1_raw_input|inconclusive${nl}helper_child_window|pass${nl}known_defect::x1_password|defect|RR-windows-01" \
    "${nl}WF loop a::plain_one kind=lib runs=2 passed=2${nl}" "FAIL $name (1 tests failed)"
  form_word=long
  loop_list="bench 3 a::plain_one"
  check a-loop-of-another-kind FAIL "$right" "FAIL $name (the loop list names the kind bench)"
}

case ${1:-} in
  --self-test) self_test ;;
  '') measure ;;
  *) exit 64 ;;
esac
