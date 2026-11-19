#!/bin/sh
# jobs: macos
# rule: Every stall phase of tests/macos_stall_fixture.rs runs in its own process against the fixture and passes, each listed known fault fails by its own assertion of a recorded key, the phases run at the gap of the test's own, which input.rs decides (from HALVO_STALL_GAP_MS, or from measure_the_readings_of_the_hold when it is unset, measured once more above its cap) and which fails the check by name when it cannot be taken, in the form that HALVO_STALL_FORM names, a closed word that the check hands on alone: short (or unset) in the every-push job, which runs every wait of each mended phase with the rounds that tests/macos_stall_fixture.rs gives it (in the short form a phase passes with one held round and no failed round, and its rounds not driven are printed by wait; in the long form every wait of each mended phase needs a held round (the return phase keeps its three-way verdict of ruling MA-23), or that wait reads not driven and the ledger takes no record from the run; on the hosted runners a round is often not driven through no fault of the product, and a short form that failed on that turned two landing jobs red, so the long form holds the promise for every wait; a test that reads not driven is never a pass), the return phase, the control, the two-sided gap rule, both listed known faults and the proofs, and prints a line for each thing that it leaves out; and long, which the ledger owes for a change of the hold's nine files and which also runs the line at the product's constants, short_window form (a), the loop and the whole-password lines; a measured line asserts nothing and holds no promise, D2 is held by a stand-in at the product's gap listed as a known fault, the count of the cause unclocked is printed by name, and the measured run of the long form that passed and that docs/baseline/ledger.d/macos-2b.txt records ran on the blobs that the head has of the eight files of the hold and on the stall-seam lines of crates/halvo-capture/Cargo.toml, each with its table and with a features list read as its stall-seam alone, and not on that file's blob; for a test that exits 101 the check prints its panic line and the lines after it, it keeps each test's whole output under tests/ of its artifacts directory, which the job uploads, and it prints for each test its counted rounds and its rounds not driven (a round in which a reply of the fixture did not come or the fixture's output ended, never a pass) and their sums on the total line, fails a test in which a round not driven recorded a key and a test whose every round was not driven, and passes one round not driven beside a counted round (ruling PR-32); 12-test ran the tests of the file that drive stand-ins or tables (the launch path, one fixture for the line at the product's constants, the stamps command, the verdict and case tables and the on-time with the keys' fates) and passed them and ran no other test of the file, which the short form reads from 12-test's list and fails without it (a list that names no test is none), while the long form, run by name, runs without 12-test, and a check build of the library without debug assertions has no InputSource::with_hold_bound, no Debug for a held key or for a tap event and no InputSource::with_seam, the last also with the feature stall-seam, where a build without debug assertions has no DropCounts either (ruling MA-21); it runs the phase of a password typed whole ahead into a field that is secure for less than the spacing of two readings, which must read 0 recorded keys by the notice of the focus change, and the same at the product's constants, whose verdict is held, notkept or failed, and the lag phase, whose session reader the seam hands in with a delay (rulings MA-12 E, MA-20 Addition 1, MA-22 (d)); every line that a hosted test prints under its own name holds names and numbers alone (ruling MA-21); it runs the phase in which a focus notice into a plain field withholds no key, which prints the keys withheld by a notice, by a doubt and by the read at the start of a watch, and the phase of a program that comes to the front with its secure field focused, which must read 0 recorded keys (rulings MA-12 H5, MA-30 M3, M5); and three rules read the tree alone: EnableSecureEventInput, DisableSecureEventInput and every link_name that is no plain literal stand in crates/halvo-capture/src/secure_input_macos.rs alone, read over the files .rs .m .mm .c .cc .cpp .h .swift; a lookup of a symbol by name (dlsym, dlopen, CFBundleGetFunctionPointerForName) stands there alone too, but that crates/halvo-capture/src/store/hardening_tests/full_flush.rs may look a symbol up in its one fn symbol, defined outside every brace of the code, whose two lines the check holds and whose calls in that file give it plain C string literals beginning r1_, while that file holds no word pub in its code, no mod item, no include!, no macro_rules!, no attribute but #[test], an attribute being read from its #[ to its matching ] and a #[ inside it beginning another in its place and no C string literal that does not begin r1_, and no other tracked text file whose name ends in .rs holds the letters full_flush.rs, in any letter case, with these named limits: a macro of another file or crate that the file calls and that gives fn symbol a name written as no C string literal in the file, a visibility, an attribute or a place inside an item of its own, an attribute named test that a use of the file brings from another crate or file, an attribute of the file that holds the tokens of another attribute inside its own brackets, an asm! or global_asm! of the file that gives fn symbol a symbol name, a path to the file joined at build time or a link to it under another name, the name of the file written with an escape in a string of another Rust file, a file whose name does not end in .rs in small letters, which a Rust file includes or takes as a module and which names the file, and a form of Rust text that the small reader of this check, of comments, strings, raw strings and character literals, reads otherwise than the compiler; and of these three rules alike, a tracked file that git takes as no text file, by a NUL byte in it or by an attribute of .gitattributes, which no search of this check reads (rulings JB-56, JB-58, JB-59, JB-60, JB-61); no field, payload, alias, renaming use, static or annotated let of crates/halvo-capture/src/macos/input.rs or crates/halvo-capture/src/secure_input_macos_hold.rs holds a reading of the secure-input state between two events, with one named limit, a cache whose type is inferred (rulings MA-14, MA-30 M6); and the fields of the clocks Monotonic and Uptime stay private (ruling MA-30 M10).
# lane: alone
set -u
name=51-macos-stall
limit_override=${MS_TEST_LIMIT_S:-}
budget_s=${MS_BUDGET_S:-240}
form_test=the_form_of_the_hosted_phases
fixture_source=crates/halvo-capture/tests/macos_stall_fixture/fixture.m
held=crates/halvo-capture/src/store/hardening_tests/full_flush.rs
leak='recorded keys beside the markers in rounds that counted'
LC_ALL=C
export LC_ALL

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

expected_tests() {
  cat <<'EOF'
the_probe_reports_what_the_stall_phases_rest_on not-run
the_form_of_the_hosted_phases not-run
a_key_typed_ahead_into_a_secure_field_is_not_recorded phase
a_key_typed_ahead_of_a_click_into_a_secure_field_is_not_recorded phase
a_key_typed_ahead_of_a_focus_move_by_the_program_is_not_recorded phase
a_key_typed_on_a_return_to_a_secure_field_is_not_recorded three-way
the_up_of_the_key_that_leaves_a_secure_field_is_not_recorded phase
a_password_typed_whole_ahead_into_a_field_secure_for_less_than_the_spacing_of_two_readings_is_not_recorded phase
at_the_products_constants_a_password_typed_whole_ahead_is_not_recorded three-way
a_key_whose_own_reading_ends_after_the_lag_bound_is_not_recorded phase
the_control_records_the_keys_typed_in_the_plain_field_and_none_of_the_secure_field phase
a_focus_notice_into_a_plain_field_withholds_no_key phase
a_program_that_comes_to_the_front_with_its_secure_field_focused_records_no_key_typed_ahead phase
at_the_bound_of_the_product_a_stall_records_nothing_and_the_control_records_its_keys three-way long
the_gap_rule_through_the_real_timer_drops_every_key_when_every_tick_is_a_gap_and_none_when_none_is phase
known_fault_a_key_typed_ahead_of_a_stall_longer_than_the_bound_is_not_recorded known-fault
known_fault_secure_input_on_for_less_than_the_spacing_of_two_readings_releases_the_keys known-fault
measured_a_password_typed_whole_ahead_at_machine_speed measured
measured_a_password_typed_whole_ahead_at_a_persons_pace measured
short_window_a_the_click_round_as_it_was_at_wait_150_ms measured
short_window_a_the_click_round_as_it_was_at_wait_2000_ms measured
short_window_b_the_leave_asked_after_n_ms not-run
the_trial_of_the_focus_notice_reports_its_reach_and_lateness not-run
the_probe_of_a_text_input_masked_by_style_reads_the_secure_input_state not-run
the_mended_click_phase_counts_62_rounds_at_wait_2000_ms loop
the_launch_path_fails_with_a_line_that_names_the_wait_that_ran_out twelve
the_line_at_the_products_constants_starts_one_fixture_for_its_phase_and_its_control twelve
the_fixture_gives_out_its_stamps_on_a_command_through_the_launch_path twelve
the_verdict_of_a_mended_round_holds_each_rule_on_a_table_of_entries twelve
the_verdict_at_the_products_constants_holds_each_rule_on_a_table twelve
the_three_way_verdict_of_the_return_phase_holds_each_rule_on_a_table twelve
each_judge_of_a_mended_phase_fails_it_at_one_round_for_each_wait twelve
the_waits_rounds_and_kept_time_of_both_forms_are_the_ruled_ones twelve
the_case_of_a_short_window_round_holds_each_rule_on_a_table_of_entries twelve
the_on_time_and_the_fates_of_the_keys_are_read_from_the_raw_entries twelve
a_reading_of_on_outside_every_span_is_counted_from_the_rounds_first_stamp twelve
a_round_whose_fixture_output_ends_before_a_reply_is_not_driven twelve
a_round_whose_reply_comes_after_its_wait_is_not_driven twelve
a_round_whose_stamps_reply_does_not_come_is_not_driven twelve
a_key_ahead_in_a_round_not_driven_whose_first_stamp_reads_on_fails_its_phase twelve
the_late_reply_of_one_round_is_not_taken_as_the_reply_of_the_next twelve
a_round_whose_last_stamps_reply_comes_late_after_its_markers_records_nothing twelve
a_round_whose_output_ended_before_its_last_stamps_reply_records_nothing twelve
a_round_not_driven_after_its_markers_reads_recorded_by_its_own_keys_alone twelve
in_the_short_form_one_held_round_passes_a_phase_and_rounds_not_driven_are_printed_by_wait twelve
in_the_long_form_a_wait_with_no_held_round_reads_not_driven_and_takes_no_record twelve
a_phase_whose_every_round_was_not_driven_reads_not_driven_in_both_forms twelve
a_recorded_key_in_a_round_not_driven_fails_its_phase twelve
each_judge_reads_a_round_not_driven_by_its_rule_on_a_table twelve
the_fixture_and_the_test_read_one_table_of_verbs_and_each_reply_time_names_its_clock twelve
the_verdict_of_a_short_on_time_at_the_products_constants_holds_each_rule_on_a_table twelve
a_round_lost_to_a_late_marker_reads_by_its_rule_on_a_table twelve
EOF
}

notes() {
  if [ -n "${MS_NOTES_FILE:-}" ]; then
    tr -d '\r' <"$MS_NOTES_FILE"
    return
  fi
  cat <<'EOF'
measured_a_password_typed_whole_ahead_at_machine_speed in pushes 6 and 7 the keys were held under a reading of off and dropped by a reading that fell into the on-time while the program handled them; the on-time was not printed. D2 stays an open fault (MA-12 D, ruling MA-19)
short_window_a_the_click_round_as_it_was_at_wait_150_ms measures D2, asserts nothing and holds no promise; the listed stand-in holds D2 (ruling MA-22 L10)
known_fault_secure_input_on_for_less_than_the_spacing_of_two_readings_releases_the_keys holds D2 (MA-12 D3) with stand-ins at the product's gap: an on-time shorter than the spacing of two readings that are both in time releases the keys; D2 stays an open fault until a ruling names the unit that closed it (ruling MA-22 L10); the queued notice of a focus change into a secure field drops such keys of a native field on the hosted Mac at a spacing of 200 ms (19 of 20 rounds held, 18 by the notice, in run 44094620566), while at the product's constants the gap rule drops the keys and the markers first and that phase reads notkept, and a page gives no notice (rulings MA-12 E, PR-30 PR15), so D2 stays listed
short_window_a_the_click_round_as_it_was_at_wait_2000_ms D2 stays an open fault. Its mechanism has been seen on the hosted machine at the test's gap in three of the four recorded long-form runs of the measured line: 3 of 124 rounds in run 43604085133, 2 in run 43627916230, 2 in run 43655810029, and none in run 43741572763. An on-time shorter than the spacing of two readings that are both in time releases the keys. At the product's gap of 30 ms no hosted round shows it. The cause of push 7's recorded click round is not shown (rulings MA-20 (1), (7) and MA-25 item 2)
short_window_b_the_leave_asked_after_n_ms names the leave that the fixture was asked to make after n ms; the set on-times were not reached, because the leave ran behind the queued events, so each on-time is as measured, and the sample under 40 ms is thin (8 of 599 rounds in push 8) (ruling MA-20 (6))
EOF
}

history_note() {
  cat <<'EOF'
MS note pushes 5 to 7: in the Tab, click, focus-move, leaving-key and whole-password phases the test neither kept nor measured the on-time; each round that read 0 shows that a reading fell inside it, and no more; 1 of 105 counted click rounds recorded its 8 letters (ruling MA-19 (7))
EOF
}

proof_lines() {
  cat <<'EOF'
hold_bound_in_a_release_build E0599
held_key_printed E0277
tap_event_printed E0277
seam_in_a_release_build E0599
EOF
}

seam_manifests() {
  git grep -l -F -e stall-seam -- 'Cargo.toml' '*/Cargo.toml' >"$work/seam" 2>/dev/null
  case $? in 0 | 1) ;; *) fail "git grep could not read the manifests" ;; esac
  named=$(tr '\n' ' ' <"$work/seam")
  [ "$named" = "crates/halvo-capture/Cargo.toml " ] ||
    fail "the feature stall-seam must stand in crates/halvo-capture/Cargo.toml alone; manifests that name it: ${named:-none}"
  printf 'seam: the feature stall-seam stands in crates/halvo-capture/Cargo.toml alone\n'
}

carbon_calls() {
  lock=crates/halvo-capture/src/secure_input_macos.rs
  git grep --cached -I -n -E -e '(^|[^A-Za-z0-9_])_*(Enable|Disable)SecureEventInput([^A-Za-z0-9_]|$)' \
    -- . ":(exclude)$lock" ':(exclude)docs/research/researcher-4.md' ":(exclude)scripts/r1/ci.d/$name.sh" \
    >"$work/carbon" 2>/dev/null
  case $? in 0 | 1) ;; *) fail "git grep could not read the tree for the Carbon calls" ;; esac
  git grep --cached -I -n -E -e 'link_name[[:space:]]*=' -- . ":(exclude)$lock" ":(exclude)scripts/r1/ci.d/$name.sh" \
    >"$work/link-names" 2>/dev/null
  case $? in 0 | 1) ;; *) fail "git grep could not read the tree for link_name" ;; esac
  awk '{
    n = split($0, part, /link_name[[:space:]]*=/)
    for (i = 2; i <= n; i++) if (part[i] !~ /^[[:space:]]*"[A-Za-z0-9_$]+"/) { print; next }
  }' "$work/link-names" >>"$work/carbon" || fail "the link_name lines cannot be read"
  git grep --cached -I -n -E -e '(^|[^A-Za-z0-9_])(dlsym|dlopen|CFBundleGetFunctionPointerForName)([^A-Za-z0-9_]|$)' \
    -- . ":(exclude)$lock" ":(exclude)scripts/r1/ci.d/$name.sh" ':(exclude)scripts/r4/ci.d/20-r4-core.sh' \
    ':(exclude)tools/r4-trials/app/system/macos.swift' ":(exclude)$held" >>"$work/carbon" 2>/dev/null
  case $? in 0 | 1) ;; *) fail "git grep could not read the tree for a lookup by name" ;; esac
  other=$(cut -d : -f 1 "$work/carbon" | sort -u | tr '\n' ' ')
  [ -z "$other" ] ||
    fail "the name EnableSecureEventInput or DisableSecureEventInput (with or without leading underscores), a link_name whose name is no plain literal, or a lookup of a symbol by name (dlsym, dlopen, CFBundleGetFunctionPointerForName) stands outside $lock, whose one lock holds the three Carbon calls: ${other% } (rulings MA-14, MA-30 M6, MA-31 N6); call the hold of that file"
  held_lookups
  printf 'carbon: in every tracked text file but %s, docs/research/researcher-4.md and this check, no name of the two Carbon calls (with or without leading underscores), no link_name whose name is no plain literal, and no lookup of a symbol by name (scripts/r4/ci.d/20-r4-core.sh and tools/r4-trials/app/system/macos.swift may look up others, and %s %s); limits: a name joined at build or run time, a lookup through a function or a crate that this check does not list, linker arguments from outside the tree, a tracked file that git takes as no text file, by a NUL byte in it or by an attribute of .gitattributes, which no search of this check reads, and in %s a macro of another file or crate that the file calls and that gives fn symbol a name written as no C string literal in the file, a visibility, an attribute or a place inside an item of its own, an attribute named test that a use of the file brings from another crate or file, an attribute of the file that holds the tokens of another attribute inside its own brackets, an asm! or global_asm! of the file that gives fn symbol a symbol name, a path to the file joined at build time or a link to it under another name, the name of the file written with an escape in a string of another Rust file, a file whose name does not end in .rs in small letters, which a Rust file includes or takes as a module and which names the file, and a form of Rust text that the small reader of this check, of comments, strings, raw strings and character literals, reads otherwise than the compiler\n' "$lock" "$held" "$held_said" "$held"
}

held_lookups() {
  if ! git cat-file -e ":$held" 2>/dev/null; then
    held_said='is not in the index'
    return 0
  fi
  git show ":$held" >"$work/held" 2>/dev/null || fail "$held cannot be read from the index"
  git grep --cached -I -l -i -F -e "${held##*/}" -- '*.rs' ":(exclude)$held" >"$work/held-named" 2>/dev/null
  case $? in 0 | 1) ;; *) fail "git grep could not read the tree for the name ${held##*/}" ;; esac
  named=$(tr '\n' ' ' <"$work/held-named")
  [ -z "$named" ] ||
    fail "a tracked text file other than $held whose name ends in .rs holds the letters ${held##*/} in some letter case, so that it could compile the text of $held again beside a call of fn symbol of its own (ruling JB-60): ${named% }"
  awk -v held="$held" -v q="'" '
    function bad(what) { print held ": " what " at line " NR > found_bad }
    function bad_at(what, at) { print held ": " what " at line " at > found_bad }
    function done_c() { if (lit !~ /^r1_/) bad_at("a C string literal that does not begin r1_", litline) }
    function read_word(w) {
      if (w == "mod") bad("a module item, the word mod in the code,")
      else if (w == "pub") bad("the word pub in the code,")
      else if (w == "include") bad("an include!, the word include in the code,")
      else if (w == "macro_rules") bad("a macro_rules!, the word macro_rules in the code,")
      else if (w == "path" && attr) bad("a path attribute, the word path in an attribute,")
    }
    BEGIN { found_bad = "/dev/stderr" }
    {
      sub(/\r$/, "")
      gsub(/\r/, " ")
      depth_at_start = depth
      line = $0
      len = length(line)
      i = 1
      while (i <= len) {
        ch = substr(line, i, 1)
        two = substr(line, i, 2)
        if (st == 2) {
          if (two == "/*") { cdepth++; i += 2; continue }
          if (two == "*/") { cdepth--; i += 2; if (cdepth == 0) st = 0; continue }
          i++
          continue
        }
        if (st == 3 || st == 5) {
          if (ch == "\\") { if (st == 5) lit = lit two; i += 2; continue }
          if (ch == "\"") { if (st == 5) done_c(); st = 0; i++; continue }
          if (st == 5) lit = lit ch
          i++
          continue
        }
        if (st == 4 || st == 6) {
          if (ch == "\"" && substr(line, i + 1, hashes) == hashstr) { if (st == 6) done_c(); st = 0; i += 1 + hashes; continue }
          if (st == 6) lit = lit ch
          i++
          continue
        }
        if (two == "//") { st = 1; break }
        if (two == "/*") { st = 2; cdepth = 1; i += 2; continue }
        if (ch == "\"") { if (attr) atext = atext "\""; st = 3; pend = 0; i++; continue }
        if (ch == q) {
          pend = 0
          if (substr(line, i + 1, 1) == "\\") {
            j = i + 3
            while (j <= len && substr(line, j, 1) != q) j++
            i = j + 1
          } else if (substr(line, i + 2, 1) == q) i += 3
          else i++
          continue
        }
        if (ch ~ /[A-Za-z_]/) {
          pend = 0
          j = i
          while (j <= len && substr(line, j, 1) ~ /[A-Za-z0-9_]/) j++
          word = substr(line, i, j - i)
          next_ch = substr(line, j, 1)
          if ((word == "r" || word == "br" || word == "cr") && (next_ch == "#" || next_ch == "\"")) {
            k = j
            h = ""
            while (substr(line, k, 1) == "#") { h = h "#"; k++ }
            if (substr(line, k, 1) == "\"") {
              st = word == "cr" ? 6 : 4
              hashes = length(h)
              hashstr = h
              lit = ""
              litline = NR
              i = k + 1
              continue
            }
            if (word == "r" && h == "#") {
              m = k
              while (m <= len && substr(line, m, 1) ~ /[A-Za-z0-9_]/) m++
              word = substr(line, k, m - k)
              j = m
            }
          } else if ((word == "c" || word == "b") && next_ch == "\"") {
            st = word == "c" ? 5 : 3
            lit = ""
            litline = NR
            i = j + 1
            continue
          }
          read_word(word)
          if (attr) atext = atext word
          i = j
          continue
        }
        if (ch == "[") {
          if (pend) { attr = 1; adepth = 1; atext = ""; ainner = bang; aline = NR } else if (attr) { adepth++; atext = atext ch }
          pend = 0
        } else if (ch == "]") {
          if (attr) {
            adepth--
            if (adepth == 0) {
              attr = 0
              if (ainner || atext != "test") bad_at("an attribute other than #[test]", aline)
            } else atext = atext ch
          }
          pend = 0
        } else if (ch == "#") { pend = 1; bang = 0 }
        else if (ch == "!" && pend) bang = 1
        else if (ch != " " && ch != "\t") {
          pend = 0
          if (ch == "{") depth++
          else if (ch == "}") depth--
          if (attr) atext = atext ch
        }
        i++
      }
    }
    /^fn symbol\(name: &CStr\) -> Option<\*mut libc::c_void> \{$/ { if (depth_at_start != 0) bad("fn symbol defined inside a brace of the code,"); defined++; inside = 1; body = 0; next }
    inside {
      body++
      if (body == 1 && $0 != "    let found = unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) };") bad("fn symbol is not the two lines that this check holds,")
      else if (body == 2 && $0 != "    (!found.is_null()).then_some(found)") bad("fn symbol is not the two lines that this check holds,")
      else if (body == 3 && $0 != "}") bad("fn symbol is not the two lines that this check holds,")
      if (body == 3 || $0 == "}") inside = 0
      next
    }
    /(^|[^A-Za-z0-9_])(dlsym|dlopen|CFBundleGetFunctionPointerForName)([^A-Za-z0-9_]|$)/ { bad("a lookup by name outside fn symbol") }
    {
      rest = $0
      while (match(rest, /symbol\(c"r1_[A-Za-z0-9_]+"\)/)) {
        before = RSTART > 1 ? substr(rest, RSTART - 1, 1) : ""
        one = substr(rest, RSTART + 9, RLENGTH - 11)
        if (before !~ /[A-Za-z0-9_]/ && !(one in seen)) { seen[one] = 1; order[++n] = one }
        rest = substr(rest, 1, RSTART - 1) "#" substr(rest, RSTART + RLENGTH)
      }
      if (rest ~ /(^|[^A-Za-z0-9_])symbol([^A-Za-z0-9_]|$)/) bad("fn symbol gets a name that is no plain literal beginning r1_, or is named other than in such a call,")
    }
    END {
      if (defined > 1 || (defined == 0 && n > 0)) print held ": fn symbol is not defined once, as the two lines that this check holds (defined " defined + 0 " times)" > found_bad
      printf "%d", n
      for (i = 1; i <= n; i++) printf " %s", order[i]
      printf "\n"
    }' "$work/held" >"$work/held-names" 2>"$work/held-bad" || fail "$held cannot be read"
  if [ -s "$work/held-bad" ]; then
    detail=$(tr '\n' ';' <"$work/held-bad" | sed 's/;$//; s/;/; /g')
    fail "the lookups of a symbol by name in $held are not held (rulings JB-56, JB-58, JB-59 and JB-60: that file may look a symbol up only in its fn symbol, defined outside every brace of the code, whose two lines this check holds and whose calls in that file give it plain C string literals beginning r1_; it may hold no word pub in its code, no mod item, no include!, no macro_rules!, no attribute but #[test], an attribute being read from its #[ to its matching ] and a #[ inside it beginning another in its place and no C string literal that does not begin r1_): $detail"
  fi
  read -r count names <"$work/held-names" || fail "$held cannot be read"
  if [ "$count" = 0 ]; then
    held_said='looks no symbol up by name'
  else
    held_said="looks a symbol up by name only in its fn symbol, by the plain literal names $names, and fn symbol is defined outside every brace; the file holds no word pub in its code, no mod item, no include!, no macro_rules!, no attribute but #[test], an attribute being read from its #[ to its matching ] and a #[ inside it beginning another in its place and no C string literal that does not begin r1_, and no other tracked text file whose name ends in .rs holds the letters full_flush.rs, in any letter case"
  fi
}

reading_places_of() {
  git show ":$1" >"$work/reading-source" 2>/dev/null || fail "$1 cannot be read from the index"
  tr -d '\r' <"$work/reading-source" | awk -v seam='#[cfg(all(debug_assertions, feature = "stall-seam"))]' '
    function count(text, pattern) { return gsub(pattern, "", text) }
    function braces(text) { return count(text, "[{]") - count(text, "[}]") }
    function nests(text) { return count(text, "[{(]") - count(text, "[})]") }
    function names(text) { return text ~ /(^|[^A-Za-z_])Reading([^A-Za-z_:]|$)/ }
    function plain(text) { gsub(/fn\([^)]*\) -> [A-Za-z_:<>]+/, "", text); return text }
    function in_angles(text,    i, c, depth, rest) {
      depth = 0
      for (i = 1; i <= length(text); i++) {
        c = substr(text, i, 1)
        if ((c == "-" || c == "=") && substr(text, i + 1, 1) == ">") { i++; continue }
        if (c == "<") depth++
        else if (c == ">" && depth > 0) depth--
        else if (depth > 0 && substr(text, i, 7) == "Reading") {
          rest = substr(text, i + 7, 1)
          if (substr(text, i - 1, 1) !~ /[A-Za-z_]/ && rest !~ /[A-Za-z_:]/) return 1
        }
      }
      return 0
    }
    {
      line = $0
      sub(/\/\/.*$/, "", line)
    }
    skip {
      if (!opened && index(line, "{") > 0) opened = 1
      depth += braces(line)
      if (opened && depth <= 0) skip = 0
      next
    }
    macro {
      if (names(plain(line))) print NR
      depth += braces(line)
      if (depth <= 0) macro = 0
      next
    }
    using {
      text = text " " line
      if (index(line, ";") > 0) {
        using = 0
        if (text ~ /(^|[^A-Za-z_])Reading[[:space:]]+as[[:space:]]/) print start
      }
      next
    }
    item {
      if (pending) {
        if (line ~ /^[[:space:]]*$/) next
        pending = 0
      }
      depth += nests(line)
      if (!(kind == "enum" && line ~ /^[[:space:]]*Reading,?[[:space:]]*$/) && names(plain(line))) print NR
      if (depth <= 0) item = 0
      next
    }
    {
      rest = line
      while (match(rest, /^[[:space:]]*#\[[^]]*\][[:space:]]*/)) {
        attr = substr(rest, RSTART, RLENGTH)
        if (attr ~ /#\[cfg\((test\)|all\(test[,)])/) testattr = 1
        if (index(attr, seam) > 0) seamattr = 1
        rest = substr(rest, RSTART + RLENGTH)
      }
      if (rest ~ /^[[:space:]]*$/) next
      sub(/^[[:space:]]*/, "", rest)
      sub(/^pub(\([^)]*\))?[[:space:]]+/, "", rest)
      held_test = testattr
      held_seam = seamattr
      testattr = 0
      seamattr = 0
    }
    rest ~ /^mod[[:space:]]+[A-Za-z_]/ && held_test {
      skip = 1
      opened = index(rest, "{") > 0
      depth = braces(rest)
      if (opened && depth <= 0) skip = 0
      next
    }
    rest ~ /^(struct|enum|union)[[:space:]]+[A-Za-z_]/ {
      kind = rest
      sub(/[[:space:]].*/, "", kind)
      header = rest
      sub(/^(struct|enum|union)[[:space:]]+[A-Za-z0-9_]+/, "", header)
      depth = nests(rest)
      if (held_seam) {
        if (depth > 0) { skip = 1; opened = 1; depth = braces(rest) }
        next
      }
      if (names(plain(header))) print NR
      if (depth > 0) item = 1
      else if (rest !~ /[;{}()]/) { item = 1; pending = 1; depth = 0 }
      next
    }
    rest ~ /^macro_rules![[:space:]]*[A-Za-z_]/ {
      if (names(plain(rest))) print NR
      depth = braces(rest)
      if (depth > 0) macro = 1
      next
    }
    rest ~ /^use[[:space:]]/ {
      if (index(rest, ";") > 0) {
        if (rest ~ /(^|[^A-Za-z_])Reading[[:space:]]+as[[:space:]]/) print NR
      } else {
        using = 1
        text = rest
        start = NR
      }
      next
    }
    rest ~ /^type[[:space:]]+[A-Za-z_]/ && names(plain(rest)) { print NR; next }
    rest ~ /^(static|const)[[:space:]]/ && rest !~ /^const[[:space:]]+(unsafe[[:space:]]+)?fn[[:space:]]/ && names(plain(rest)) { print NR; next }
    rest ~ /thread_local!/ && names(plain(rest)) { print NR; next }
    /let (mut )?[A-Za-z_]+[[:space:]]*:/ {
      text = line
      sub(/=.*/, "", text)
      if (names(plain(text))) { print NR; next }
    }
    in_angles(plain(rest)) { print NR }
  ' >"$work/reading-places" || fail "$1 cannot be read"
  places=$(sort -n -u "$work/reading-places" | tr '\n' ' ')
  [ -z "$places" ] ||
    fail "$1 can keep a reading of the secure-input state between two events: a field, a payload, an alias, a renaming use, a static, a thread_local, a macro_rules body, a generic argument or an annotated let names the type Reading (lines ${places% }); the reading of an event is taken by the one function that handles the event and handed on by value (rulings MA-14, MA-30 M6, MA-31 N7)"
}

reading_places() {
  for source in crates/halvo-capture/src/macos/input.rs crates/halvo-capture/src/secure_input_macos_hold.rs; do
    reading_places_of "$source"
  done
  printf 'reading: no field, payload, alias, renaming use, static, thread_local, macro_rules body, generic argument or annotated let of crates/halvo-capture/src/macos/input.rs or crates/halvo-capture/src/secure_input_macos_hold.rs outside a test module names the type Reading; limits: a cache whose type is inferred, an alias, a wrapper or a re-export made in another file, a type made by a macro of another file or crate, and a reading kept as another type (a bool, a number, an atomic)\n'
}

clock_fields() {
  git show ":crates/halvo-capture/src/secure_input_macos_hold.rs" >"$work/hold-source" 2>/dev/null ||
    fail "crates/halvo-capture/src/secure_input_macos_hold.rs cannot be read from the index"
  for clock in Monotonic Uptime; do
    defined=$(tr -d '\r' <"$work/hold-source" | grep -c -E "(^|[^A-Za-z0-9_])struct[[:space:]]+$clock([^A-Za-z0-9_]|$)")
    tr -d '\r' <"$work/hold-source" | grep -q -x -F -e "pub struct $clock(u64);" && [ "$defined" = 1 ] ||
      fail "the clock $clock of crates/halvo-capture/src/secure_input_macos_hold.rs is not defined once, as exactly 'pub struct $clock(u64);' (defined $defined times): its field is public in some form, it has a second definition, or the type moved; it stays private, made by from_nanos and read by nanos (rulings MA-21, MA-29 (2), MA-30 M10, MA-31 N7)"
  done
  printf 'clocks: Monotonic and Uptime are each defined once, with a private field\n'
}

names_and_numbers() {
  awk -v test="$1" '
    index($0, test " ") == 1 {
      n = split(substr($0, length(test) + 2), field, " ")
      for (i = 1; i <= n; i++) {
        if (field[i] ~ /[^A-Za-z0-9_=.,:\/+-]/ || (i > 1 && field[i] !~ /[=:]/ && field[i] !~ /^[0-9.,+-]+$/)) {
          print NR
          exit
        }
      }
    }' "$2"
}

measured_files() {
  cat <<'EOF'
crates/halvo-capture/src/macos/input.rs
crates/halvo-capture/src/secure_input_macos.rs
crates/halvo-capture/src/secure_input_macos_hold.rs
crates/halvo-capture/tests/macos_stall_fixture.rs
crates/halvo-capture/tests/macos_stall_fixture/fixture.m
crates/halvo-capture/src/macos/secure_input_byte.rs
crates/halvo-capture/src/macos/run_loop.rs
scripts/r1/ci.d/51-macos-stall.sh
EOF
}

seam_lines() {
  git show ":crates/halvo-capture/Cargo.toml" >"$work/manifest" 2>/dev/null ||
    fail "crates/halvo-capture/Cargo.toml cannot be read from the index"
  tr -d '\r' <"$work/manifest" >"$work/manifest-lines" || fail "crates/halvo-capture/Cargo.toml cannot be read"
  awk '
    /^[[:space:]]*\[/ { table = $0 }
    /stall-seam/ {
      line = $0
      if (match(line, /features[[:space:]]*=[[:space:]]*\[[^]]*\]/))
        line = substr(line, 1, RSTART - 1) "features = [\"stall-seam\"]" substr(line, RSTART + RLENGTH)
      printf "measured seam path=crates/halvo-capture/Cargo.toml table=%s text=%s\n", table, line
    }
  ' "$work/manifest-lines" >"$work/seam-head" || fail "crates/halvo-capture/Cargo.toml cannot be read"
}

ledger_check() {
  ledger=docs/baseline/ledger.d/macos-2b.txt
  fix="run the long form (HALVO_STALL_FORM=long) on a scratch push of this head, then record that run with form=long, result=pass, its figures, the blob ids of the eight files of the hold and the stall-seam lines of crates/halvo-capture/Cargo.toml in $ledger (rulings MA-22 L10, MA-23; the inbox entry of 2026-11-16T10:12:55Z)"
  [ -f "$ledger" ] || fail "$ledger is missing: $fix"
  tr -d '\r' <"$ledger" >"$work/ledger" || fail "$ledger cannot be read"
  runs=$(sed -n 's/^measured run=\([0-9][0-9]*\) .*/\1/p' "$work/ledger" | tr '\n' ' ')
  long=$(grep -c -E '^measured run=[0-9]+ (.* )?form=long( .*)? result=pass( |$)' "$work/ledger")
  run=${runs% }
  case $run in
    '') fail "$ledger names no measured run: $fix" ;;
    *' '*) fail "$ledger names more than one measured run ($run): $fix" ;;
  esac
  [ "$long" = 1 ] || fail "$ledger names the measured run $run without form=long and result=pass: $fix"
  grep -q -E "^measured run=$run (.* )?phases_not_driven=0( |\$)" "$work/ledger" ||
    fail "$ledger names the measured run $run without phases_not_driven=0: a long form with a wait that held no round takes no record: $fix"
  sed -n 's/^measured blob path=\([^ ]*\) id=[0-9a-f]*$/\1/p' "$work/ledger" | sort >"$work/ledger-paths" ||
    fail "$ledger cannot be read"
  measured_files | sort >"$work/measured-paths" || fail "the list of the eight files cannot be read"
  cmp -s "$work/ledger-paths" "$work/measured-paths" ||
    fail "$ledger records the blobs of files other than the eight of the hold, once each: $fix"
  while read -r path; do
    recorded=$(awk -v path="$path" '$1 == "measured" && $2 == "blob" && $3 == "path=" path { sub(/^id=/, "", $4); print $4 }' "$work/ledger")
    staged=$(git ls-files --stage -- "$path" | cut -d ' ' -f 2)
    [ -n "$staged" ] || fail "$path is not tracked: $fix"
    [ "$recorded" = "$staged" ] ||
      fail "the measured run $run ran on a blob of $path other than the head's ($recorded against $staged): $fix"
  done <"$work/measured-paths"
  sed -n '/^measured seam /p' "$work/ledger" >"$work/seam-ledger" || fail "$ledger cannot be read"
  [ -s "$work/seam-ledger" ] || fail "$ledger names no stall-seam line of crates/halvo-capture/Cargo.toml: $fix"
  seam_lines
  if ! cmp -s "$work/seam-ledger" "$work/seam-head"; then
    head_lines=$(sed 's/^measured seam path=[^ ]* //' "$work/seam-head" | tr '\n' '|')
    fail "the stall-seam lines of crates/halvo-capture/Cargo.toml differ from those that the measured run $run ran on (the head has: ${head_lines%|}): $fix"
  fi
  printf 'ledger: the measured run %s of the long form ran on the blobs that the head has of the eight files of the hold and on its stall-seam lines of crates/halvo-capture/Cargo.toml\n' "$run"
}

tracked_changes() {
  git status --porcelain --untracked-files=no
}

judge() {
  verdict=FAIL
  cause=
  out=$(tr -d '\r' <"$3")
  summary=$(printf '%s\n' "$out" | sed -n 's/^test result: //p' | tail -n 1)
  case $2 in
    phase | three-way | measured | loop)
      if [ "$1" -ne 0 ]; then
        cause="exit $1"
        return 0
      fi
      case $summary in
        'ok. 1 passed; 0 failed;'*) verdict=pass ;;
        *) cause='exit 0, and not one test ran and passed' ;;
      esac
      return 0
      ;;
  esac
  if [ "$1" -eq 0 ]; then
    case $summary in
      'ok. 1 passed; 0 failed;'*)
        verdict=FIXED-BUT-LISTED
        cause='the listed known fault passed'
        ;;
      *) cause='exit 0, and not one test ran and passed' ;;
    esac
    return 0
  fi
  case $1:$summary in
    '101:FAILED. 0 passed; 1 failed;'*) ;;
    *)
      cause="exit $1, and not the exit 101 of one failed test"
      return 0
      ;;
  esac
  case $out in
    *"$leak"*) verdict='known-fault' ;;
    *) cause='the listed known fault failed without its assertion of a recorded key' ;;
  esac
}

lib_gap() {
  perl -e 'alarm shift; exec @ARGV or exit 127' 150 "$lib" --ignored --exact --nocapture "macos::input::tests::$1" \
    </dev/null >"$work/gap-$2.txt" 2>&1
  code=$?
  cp "$work/gap-$2.txt" "$artifacts/tests/$1-$2.txt" || fail "the output of $1 cannot be kept in $artifacts/tests"
  tr -d '\r' <"$work/gap-$2.txt" >"$work/gap-$2-lines.txt" || fail "the output of $1 cannot be read"
  [ "$code" -eq 0 ] || fail "the gap of the test's own could not be measured: $1 exited $code"
  grep '^measure' "$work/gap-$2-lines.txt"
  decided=$(sed -n 's/^gap_of_the_test //p' "$work/gap-$2-lines.txt" | tail -n 1)
  [ -n "$decided" ] || fail "the gap of the test's own could not be measured: $1 printed no gap line"
}

measure_gap() {
  if [ -n "${HALVO_STALL_GAP_MS+set}" ]; then
    lib_gap the_gap_of_the_test_from_its_variable variable
  else
    lib_gap measure_the_readings_of_the_hold first
    grep -q '^measure ' "$work/gap-first-lines.txt" ||
      fail "the gap of the test's own could not be measured: measure_the_readings_of_the_hold printed no measure line"
    case $decided in
      above_the_cap*)
        printf 'the gap of the measurement is above its cap: measured again (ruling MA-23 Addition 1 (4))\n'
        first_line=$(grep '^measure ' "$work/gap-first-lines.txt")
        lib_gap measure_the_readings_of_the_hold second
        case $decided in
          above_the_cap*)
            second_line=$(grep '^measure ' "$work/gap-second-lines.txt")
            fail "the runner's readings came further apart than the hosted proof is stated for: $first_line / $second_line; run the job again"
            ;;
        esac
        ;;
    esac
  fi
  case $decided in
    refused*) fail "the gap of the test's own could not be measured: ${decided#*reason=}" ;;
    gap_ms=*) ;;
    *) fail "the gap of the test's own could not be measured: the gap line reads $decided" ;;
  esac
  HALVO_STALL_GAP_MS=$(printf '%s\n' "$decided" | sed -n 's/^gap_ms=\([0-9][0-9]*\) .*/\1/p')
  [ -n "$HALVO_STALL_GAP_MS" ] || fail "the gap of the test's own could not be measured: the gap line reads $decided"
  export HALVO_STALL_GAP_MS
  printf 'MS %s\n' "$decided" | tee -a "$artifacts/ms.txt" || fail "ms.txt cannot be written"
}

run_test() {
  HALVO_STALL_FIXTURE=$fixture perl -e 'alarm shift; exec @ARGV or exit 127' "$3" \
    "$exe" --ignored --exact --nocapture --test-threads=1 "$1" </dev/null >"$2" 2>&1
}

proof_build() {
  build_list=$1
  build_log=$2
  build_features=${3:-}
  set --
  for proof in $build_list; do
    set -- "$@" --cfg "halvo_proof=\"$proof\""
  done
  if [ -n "$build_features" ]; then
    cargo rustc --locked -p halvo-capture --lib --profile check --message-format short --features "$build_features" \
      -- -C debug-assertions=off "$@" >"$build_log" 2>&1
  else
    cargo rustc --locked -p halvo-capture --lib --profile check --message-format short -- -C debug-assertions=off "$@" >"$build_log" 2>&1
  fi
}

twelve_test() {
  list=$CI_D_REPO/target/ci-artifacts/12-test/tests.txt
  if [ ! -f "$list" ] || ! grep -q '^test ' "$list"; then
    [ "${HALVO_STALL_FORM:-short}" = long ] ||
      fail "12-test left no list of its tests (target/ci-artifacts/12-test/tests.txt is missing or names no test): the short form runs in the every-push job after 12-test and reads the tests of tests/macos_stall_fixture.rs that 12-test ran; run the whole job, or the long form by name"
    printf '12-test: no list of tests in this job (the long form, run by name)\n'
    return 0
  fi
  : >"$work/twelve-ran" || fail "the list of 12-test cannot be read"
  awk -v ran_file="$work/twelve-ran" '
    /^ *Running / { inside = index($0, "tests/macos_stall_fixture.rs") > 0; next }
    /^ *Doc-tests / { inside = 0; next }
    inside && /^test / {
      if ($0 ~ / \.\.\. ignored(,|$)/) ignored++
      else { ran++; print $2, $NF > ran_file }
    }
    END { printf "%d %d\n", ignored, ran }' "$list" >"$work/twelve" || fail "the list of 12-test cannot be read"
  read -r ignored ran <"$work/twelve" || fail "the list of 12-test cannot be read"
  sed -n 's/ twelve$//p' "$work/expected" | sort >"$work/twelve-want" || fail "the expected list cannot be read"
  cut -d ' ' -f 1 "$work/twelve-ran" | sort >"$work/twelve-got" || fail "the list of 12-test cannot be read"
  printf '12-test: tests of macos_stall_fixture ignored=%s ran=%s\n' "$ignored" "$ran"
  other=$(comm -13 "$work/twelve-want" "$work/twelve-got" | tr '\n' ' ')
  [ -z "$other" ] || fail "12-test ran tests of tests/macos_stall_fixture.rs that are not the launch-path test: ${other% }"
  absent=$(comm -23 "$work/twelve-want" "$work/twelve-got" | tr '\n' ' ')
  [ -z "$absent" ] || fail "12-test did not run the launch-path test of tests/macos_stall_fixture.rs: ${absent% }"
  failed_here=$(grep -v ' ok$' "$work/twelve-ran" | cut -d ' ' -f 1 | tr '\n' ' ')
  [ -z "$failed_here" ] || fail "12-test ran the launch-path test and it did not pass: ${failed_here% }"
  [ "$ignored" -gt 0 ] || fail "12-test listed no test of tests/macos_stall_fixture.rs as ignored"
}

rules() {
  repo=${CI_D_REPO:-}
  [ -n "$repo" ] || repo=$(git rev-parse --show-toplevel 2>/dev/null) || fail "no repository: run it inside a checkout or give CI_D_REPO"
  cd "$repo" || exit 2
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name-rules.XXXXXX") || fail "mktemp failed"
  seam_manifests
  carbon_calls
  reading_places
  clock_fields
  printf 'rules: only the parts of the check that read the index and the tree ran (seam, carbon, reading, clocks); no build, no test and no process of the product was started, and neither the ledger nor the list of 12-test was read\n'
  printf 'PASS %s\n' "$name"
}

measure() {
  check_started=$(date +%s)
  cd "$CI_D_REPO" || exit 2
  artifacts=${CI_D_ARTIFACTS:-$CI_D_REPO/target/ci-artifacts/$name}
  mkdir -p "$artifacts" || fail "the artifacts directory cannot be made"
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail "mktemp failed"
  if [ -n "${MS_EXPECTED_FILE:-}" ]; then
    tr -d '\r' <"$MS_EXPECTED_FILE" >"$work/expected" || fail "the expected list cannot be read"
  else
    expected_tests >"$work/expected"
  fi
  proof_lines >"$work/proofs"
  before=$(tracked_changes) || fail "git status failed before the run"

  twelve_test
  seam_manifests
  carbon_calls
  reading_places
  clock_fields
  printf 'session reader of the phases: %s\n' "${HALVO_STALL_SESSION:-real}"

  proofs=$(cut -d ' ' -f 1 "$work/proofs" | tr '\n' ' ')
  twins=$(cut -d ' ' -f 1 "$work/proofs" | sed 's/$/_twin/' | tr '\n' ' ')
  proof_build "$twins" "$work/twins.txt"
  status=$?
  [ "$status" -eq 0 ] || fail "the twins of the proofs did not build: cargo rustc exited $status"
  proof_build "$proofs" "$work/proofs.txt"
  status=$?
  [ "$status" -ne 0 ] || fail "the proofs built: a build without debug assertions has with_hold_bound, with_seam, or Debug for a held key or a tap event"
  cut -d ' ' -f 2 "$work/proofs" >"$work/proof-codes" || fail "the list of proofs cannot be read"
  while read -r proof code; do
    wanted=$(grep -c -x -e "$code" "$work/proof-codes")
    found=$(grep -c "error\[$code\]" "$work/proofs.txt")
    [ "$found" -ge "$wanted" ] || fail "the proof $proof did not fail with $code ($found errors $code for $wanted proofs)"
    printf 'proof %s: refused with %s\n' "$proof" "$code"
  done <"$work/proofs"
  proof_build "seam_in_a_release_build_twin drop_counts_in_a_release_build_twin" "$work/seam-twin.txt" stall-seam
  status=$?
  [ "$status" -eq 0 ] || fail "the twin of the seam proof did not build with the feature stall-seam: cargo rustc exited $status"
  proof_build "seam_in_a_release_build drop_counts_in_a_release_build" "$work/seam-proof.txt" stall-seam
  status=$?
  [ "$status" -ne 0 ] || fail "the seam proof built with the feature stall-seam: a build without debug assertions has with_seam"
  grep -q 'error\[E0599\]' "$work/seam-proof.txt" || fail "the seam proof with the feature stall-seam did not fail with E0599"
  printf 'proof seam_in_a_release_build with the feature stall-seam: refused with E0599\n'
  grep -q 'error\[E0433\]' "$work/seam-proof.txt" ||
    fail "the proof drop_counts_in_a_release_build with the feature stall-seam did not fail with E0433: a build without debug assertions has DropCounts (ruling MA-21)"
  printf 'proof drop_counts_in_a_release_build with the feature stall-seam: refused with E0433\n'

  fixture=$work/stall-fixture
  clang -fobjc-arc -Wall -Wextra -Werror -O1 -framework AppKit -framework Carbon -framework CoreGraphics \
    -o "$fixture" "$fixture_source" >"$work/fixture.txt" 2>&1 ||
    fail "the fixture did not compile: clang exited $?"

  built_from=$(date +%s)
  cargo test --locked -p halvo-capture --lib --test macos_stall_fixture --no-run --message-format json >"$work/build.json" ||
    fail "the test build failed: cargo test --no-run exited $?"
  built_at=$(date +%s)
  printf 'test build of the library and the stall file: seconds=%s\n' "$((built_at - built_from))"
  lib=$(sed -n 's/.*"kind":\["lib"\].*"name":"halvo_capture".*"executable":"\([^"]*\)".*/\1/p' "$work/build.json" | tail -n 1)
  [ -n "$lib" ] && [ -f "$lib" ] ||
    fail "the gap of the test's own could not be measured: cargo named no test executable of the library"
  exe=$(sed -n 's/.*"name":"macos_stall_fixture".*"executable":"\([^"]*\)".*/\1/p' "$work/build.json")
  [ -n "$exe" ] || fail "cargo named no executable for the test target macos_stall_fixture"
  case $exe in
    *"
"*) fail "cargo named more than one executable for the test target macos_stall_fixture" ;;
  esac
  [ -f "$exe" ] || fail "the test executable that cargo named does not exist"

  form=${HALVO_STALL_FORM:-short}
  case $form in
    short | long) ;;
    *) fail "HALVO_STALL_FORM is short or long, not $form" ;;
  esac
  HALVO_STALL_FORM=$form
  export HALVO_STALL_FORM

  "$exe" --list >"$work/list.txt" 2>&1 || fail "the test executable did not list its tests: exited $?"
  sed -n 's/: test$//p' "$work/list.txt" | sort >"$work/listed" || fail "the test list cannot be read"
  cut -d ' ' -f 1 "$work/expected" | sort >"$work/expected-names" || fail "the expected list cannot be read"
  missing=$(comm -23 "$work/expected-names" "$work/listed" | tr '\n' ' ')
  [ -z "$missing" ] || fail "the test executable does not list these tests of this script's list: ${missing% }"
  extra=$(comm -13 "$work/expected-names" "$work/listed" | tr '\n' ' ')
  [ -z "$extra" ] || fail "the test executable lists tests that this script's list does not hold: ${extra% }"

  : >"$artifacts/ms.txt" || fail "ms.txt cannot be written"
  mkdir -p "$artifacts/tests" || fail "the directory of the tests' outputs cannot be made"
  "$exe" --ignored --exact --nocapture "$form_test" </dev/null >"$work/form.txt" 2>&1
  code=$?
  cp "$work/form.txt" "$artifacts/tests/$form_test.txt" || fail "the output of $form_test cannot be kept in $artifacts/tests"
  [ "$code" -eq 0 ] || fail "$form_test did not print the form: exited $code"
  form_line=$(tr -d '\r' <"$work/form.txt" | sed -E -n 's/^(.* )?(form=(short|long) )/\2/p' | tail -n 1)
  case $form_line in
    "form=$form "*) ;;
    *) fail "$form_test printed no line of the form $form" ;;
  esac
  printf 'MS %s\n' "$form_line" | tee -a "$artifacts/ms.txt" || fail "ms.txt cannot be written"
  tr -d '\r' <"$work/form.txt" | sed -E -n 's/^(.* )?left_out /MS left out: /p' | tee -a "$artifacts/ms.txt" ||
    fail "ms.txt cannot be written"
  measure_gap
  notices=$(grep -c '^measure notice' "$work"/gap-*-lines.txt | awk -F: '{ n += $NF } END { print n + 0 }')
  unclocked=0
  passed=0
  known=0
  fixed=0
  failed=0
  not_driven=0
  not_kept=0
  rounds_counted=0
  rounds_not_driven=0
  phases_not_driven=0
  index=0
  while read -r test kind only; do
    case $kind in
      not-run | twelve) continue ;;
    esac
    case $form:$kind:$only in
      short:measured:* | short:loop:* | short:*:long)
        line="MS left out: $test kind=$kind form=long run=not_in_this_form"
        [ "$kind" != measured ] || line="$line asserts=nothing"
        printf '%s\n' "$line" | tee -a "$artifacts/ms.txt" || fail "ms.txt cannot be written"
        note=$(notes | sed -n "s/^$test //p")
        [ -z "$note" ] || printf 'MS note %s: %s\n' "$test" "$note" | tee -a "$artifacts/ms.txt" || fail "ms.txt cannot be written"
        continue
        ;;
    esac
    index=$((index + 1))
    log=$work/test-$index.log
    started=$(date +%s)
    case $form:$kind in
      short:*) limit=150 ;;
      long:measured | long:loop) limit=1500 ;;
      *) limit=900 ;;
    esac
    limit=${limit_override:-$limit}
    run_test "$test" "$log" "$limit"
    code=$?
    seconds=$(($(date +%s) - started))
    cp "$log" "$artifacts/tests/$test.txt" || fail "the output of $test cannot be kept in $artifacts/tests"
    tr -d '\r' <"$log" >"$work/lines-$index" || fail "the output of $test cannot be read"
    awk -v test="$test" -v panics="$((code == 101))" '
      panics && !after && /thread .* panicked at / { after = 1 }
      after { print "    | " $0; next }
      index($0, test ": ") == 1 { print "    | " $0; next }
      index($0, test " ") == 1 && substr($0, length(test) + 2) ~ /^(session_reader=|gap_ms=|fixture=|round=|control=|verdict=|notdriven |notcounted |late_click |entries |drops |side=|by_wait |not_driven_waits=|short_on_time )/ { print "    | " $0 }
    ' "$work/lines-$index" || fail "the output of $test cannot be read"
    awk -v test="$test" '
      index($0, test " round=") == 1 || index($0, test " control=") == 1 {
        kind = index($0, test " round=") == 1 ? "round" : "control"
        lines[kind]++
        if ($0 ~ / counted=1( |$)/) counted++
        if ($0 ~ / not_driven=[^- ]/) { not_driven[kind]++; if ($0 ~ / rv=recorded( |$)/) recorded++ }
        if (kind == "round" && match($0, / wait_ms=[0-9]+/)) {
          wait = substr($0, RSTART + 9, RLENGTH - 9)
          if (!(wait in held)) { order[++waits] = wait; held[wait] = 0; missed[wait] = 0 }
          if ($0 ~ / counted=1( |$)/) held[wait]++
          if ($0 ~ / not_driven=[^- ]/) missed[wait]++
        }
      }
      END {
        by = ""
        for (i = 1; i <= waits; i++) by = by (i > 1 ? "," : "") order[i] ":" held[order[i]] "/" missed[order[i]]
        printf "%d %d %d %d %d %d %s\n", lines["round"], lines["control"], counted,
          not_driven["round"], not_driven["control"], recorded, (by == "" ? "-" : by)
      }' "$work/lines-$index" >"$work/rounds-$index" ||
      fail "the round lines of $test cannot be read"
    read -r round_lines control_lines counted_here not_driven_rounds not_driven_controls recorded_here by_wait \
      <"$work/rounds-$index" || fail "the round lines of $test cannot be read"
    waits_not_driven=$(sed -n "s/^$test not_driven_waits=//p" "$work/lines-$index" | tail -n 1)
    not_driven_here=$((not_driven_rounds + not_driven_controls))
    rounds_counted=$((rounds_counted + counted_here))
    rounds_not_driven=$((rounds_not_driven + not_driven_here))
    found=$(tr -d '\r' <"$log" | grep -c -E "^$test (late_click |drops .* sleep=[1-9])")
    notices=$((notices + found))
    found=$(tr -d '\r' <"$log" | sed -n -E "s/^$test drops .* unclocked=([0-9]+).*/\1/p" | sort -n | tail -n 1)
    unclocked=$((unclocked + ${found:-0}))
    judge "$code" "$kind" "$log"
    prose=$(names_and_numbers "$test" "$work/lines-$index") || fail "the output of $test cannot be read"
    if [ -n "$prose" ]; then
      verdict=FAIL
      cause="line $prose of its output holds more than names and numbers under the name of the test (ruling MA-21)"
    fi
    case $kind in
      phase | three-way | loop | known-fault)
        if [ "$kind" != known-fault ] && [ "$recorded_here" -gt 0 ]; then
          verdict=FAIL
          cause="a key was recorded in a round not driven ($recorded_here rounds)"
        elif [ "$round_lines" -gt 0 ] && [ "$not_driven_rounds" -eq "$round_lines" ]; then
          verdict=not-driven
          cause="every round of the phase was not driven ($round_lines rounds)"
        elif [ "$control_lines" -gt 0 ] && [ "$not_driven_controls" -eq "$control_lines" ]; then
          verdict=not-driven
          cause="every round of the control was not driven ($control_lines rounds)"
        elif [ -n "$waits_not_driven" ]; then
          verdict=not-driven
          cause="not driven at waits $waits_not_driven (the long form takes no record from this run)"
        fi
        ;;
    esac
    three=
    if [ "$kind" = three-way ] && [ "$verdict" = pass ]; then
      three=$(tr -d '\r' <"$log" | sed -n -E "s/^$test verdict=(held|notdriven|notkept) .*/\1/p" | tail -n 1)
      case $three in
        held) ;;
        notdriven) not_driven=$((not_driven + 1)) ;;
        notkept) not_kept=$((not_kept + 1)) ;;
        *)
          verdict=FAIL
          cause='the three-way phase printed no verdict line of held, notdriven or notkept'
          ;;
      esac
    fi
    case $verdict in
      pass) passed=$((passed + 1)) ;;
      not-driven) phases_not_driven=$((phases_not_driven + 1)) ;;
      known-fault) known=$((known + 1)) ;;
      FIXED-BUT-LISTED) fixed=$((fixed + 1)) ;;
      *) failed=$((failed + 1)) ;;
    esac
    [ -z "$cause" ] || printf '%s: %s: %s\n' "$name" "$test" "$cause"
    line="MS $test result=$verdict${three:+ verdict=$three} counted=$counted_here not_driven=$not_driven_here seconds=$seconds limit=$limit"
    [ "$kind" != measured ] || line="$line asserts=nothing"
    [ "$by_wait" = - ] || line="$line by_wait=$by_wait"
    printf '%s\n' "$line"
    printf '%s\n' "$line" >>"$artifacts/ms.txt" || fail "ms.txt cannot be written"
    note=$(notes | sed -n "s/^$test //p")
    [ -z "$note" ] || printf 'MS note %s: %s\n' "$test" "$note" | tee -a "$artifacts/ms.txt" || fail "ms.txt cannot be written"
  done <"$work/expected"

  after=$(tracked_changes) || fail "git status failed after the run"
  [ "$after" = "$before" ] || fail "a tracked file differs from HEAD after the run"
  history_note | tee -a "$artifacts/ms.txt" || fail "ms.txt cannot be written"
  line="MS total=$index pass=$passed known_fault=$known notdriven=$not_driven notkept=$not_kept fail=$((failed + fixed)) rounds_counted=$rounds_counted rounds_not_driven=$rounds_not_driven phases_not_driven=$phases_not_driven"
  printf '%s\n' "$line"
  printf '%s\n' "$line" >>"$artifacts/ms.txt" || fail "ms.txt cannot be written"
  seconds=$(($(date +%s) - check_started))
  if [ "$form" = short ]; then
    [ "$seconds" -le "$budget_s" ] ||
      printf 'MS the short form took %s s, above its budget of %s s; this fails nothing (ruling MA-23 Addition 1 (5))\n' \
        "$seconds" "$budget_s" | tee -a "$artifacts/ms.txt" || fail "ms.txt cannot be written"
    line="MS form=short seconds=$seconds budget=$budget_s notices=$notices unclocked=$unclocked"
  else
    line="MS form=long seconds=$seconds notices=$notices unclocked=$unclocked"
  fi
  printf '%s\n' "$line" | tee -a "$artifacts/ms.txt" || fail "ms.txt cannot be written"
  [ "$fixed" -eq 0 ] || fail "$fixed listed known faults passed (FIXED-BUT-LISTED): take each out of this script's list"
  [ "$failed" -eq 0 ] || fail "$failed tests failed"
  [ "$phases_not_driven" -eq 0 ] ||
    fail "$phases_not_driven tests read not driven, which is never a pass (ruling MA-27 item 2)"
  ledger_check
  printf 'PASS %s\n' "$name"
}

check() {
  case_name=$1
  want=$2
  want_text=$3
  spec_file=$work/spec-$case_name
  printf '%s\n' "$4" >"$spec_file"
  : >"$work/rules-calls"
  out=$(
    PATH=$work/bin:$PATH
    [ "${5:-}" != rules ] || PATH=$work/rules-bin:$PATH
    RULES_CALLS=$work/rules-calls
    export RULES_CALLS
    CI_D_REPO=$work/repo
    CI_D_ARTIFACTS=$work/artifacts-$case_name
    STUB_SPEC=$spec_file
    STUB_EXE=$work/bin/stall-stub
    STUB_LIB=$work/bin/halvo_capture-stub
    STUB_CALLS=$work/calls-$case_name
    MS_EXPECTED_FILE=$work/expected-small
    MS_TEST_LIMIT_S=${limit:-}
    MS_NOTES_FILE=$work/notes-small
    export PATH CI_D_REPO CI_D_ARTIFACTS STUB_SPEC STUB_EXE STUB_LIB STUB_CALLS MS_EXPECTED_FILE MS_TEST_LIMIT_S MS_NOTES_FILE
    if [ "${5:-}" = rules ]; then sh "$0" --rules 2>&1; else sh "$0" 2>&1; fi
  )
  code=$?
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  case $out in
    *"$want_text"*) ;;
    *) got=$got-without-the-text ;;
  esac
  [ "${5:-}" != rules ] || [ ! -s "$work/rules-calls" ] || got=$got-with-a-start-of-$(head -n 1 "$work/rules-calls" | cut -d ' ' -f 1)
  printf 'case %s/%s want=%s got=%s\n' "$name" "$case_name" "$want" "$got"
  if [ "$got" != "$want" ]; then
    printf '%s\n' "$out" | tail -n 3 | sed 's/^/    | /'
    wrong="$wrong $case_name"
  elif [ "$want" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
  limit=
}

write_stubs() {
  cat >"$work/bin/cargo" <<'EOF'
#!/bin/sh
spec() { grep -q "^$1\$" "$STUB_SPEC"; }
case $* in
  'rustc --locked -p halvo-capture --lib --profile check --message-format short -- -C debug-assertions=off --cfg halvo_proof="hold_bound_in_a_release_build_twin" --cfg halvo_proof="held_key_printed_twin" --cfg halvo_proof="tap_event_printed_twin" --cfg halvo_proof="seam_in_a_release_build_twin"')
    spec twins-fail && exit 101
    exit 0
    ;;
  'rustc --locked -p halvo-capture --lib --profile check --message-format short -- -C debug-assertions=off --cfg halvo_proof="hold_bound_in_a_release_build" --cfg halvo_proof="held_key_printed" --cfg halvo_proof="tap_event_printed" --cfg halvo_proof="seam_in_a_release_build"')
    spec proofs-build && exit 0
    spec proof-other-code && { printf 'error[E0308]: mismatched types\nerror[E0277]: no Debug\n'; exit 101; }
    spec proof-one-e0277 && { printf 'error[E0599]: no function named `with_hold_bound`\nerror[E0277]: `HeldKey` does not implement `Debug`\nerror[E0599]: no function named `with_seam`\n'; exit 101; }
    spec proof-one-e0599 && { printf 'error[E0599]: no function named `with_hold_bound`\nerror[E0277]: `HeldKey` does not implement `Debug`\n'; exit 101; }
    printf 'error[E0599]: no function named `with_hold_bound`\nerror[E0277]: `HeldKey` does not implement `Debug`\nerror[E0277]: `TapEvent` does not implement `Debug`\nerror[E0599]: no function named `with_seam`\n'
    exit 101
    ;;
  'rustc --locked -p halvo-capture --lib --profile check --message-format short --features stall-seam -- -C debug-assertions=off --cfg halvo_proof="seam_in_a_release_build_twin" --cfg halvo_proof="drop_counts_in_a_release_build_twin"')
    spec seam-twin-fails && exit 101
    exit 0
    ;;
  'rustc --locked -p halvo-capture --lib --profile check --message-format short --features stall-seam -- -C debug-assertions=off --cfg halvo_proof="seam_in_a_release_build" --cfg halvo_proof="drop_counts_in_a_release_build"')
    spec seam-proof-builds && exit 0
    spec seam-proof-other-code && { printf 'error[E0308]: mismatched types\n'; exit 101; }
    printf 'error[E0599]: no function named `with_seam`\n'
    spec drop-counts-in-a-release-build || printf 'error[E0433]: cannot find `DropCounts` in `super`\n'
    exit 101
    ;;
  'test --locked -p halvo-capture --lib --test macos_stall_fixture --no-run --message-format json')
    spec test-build-fails && exit 101
    spec no-library || printf '{"reason":"compiler-artifact","target":{"kind":["lib"],"crate_types":["lib"],"name":"halvo_capture","test":true},"executable":"%s","fresh":true}\n' "$STUB_LIB"
    printf '{"reason":"compiler-artifact","target":{"kind":["test"],"crate_types":["bin"],"name":"macos_stall_fixture","test":true},"executable":"%s","fresh":true}\n' "$STUB_EXE"
    exit 0
    ;;
esac
exit 97
EOF
  cat >"$work/bin/clang" <<'EOF'
#!/bin/sh
grep -q '^fixture-fails$' "$STUB_SPEC" && exit 1
for arg; do
  case $prev in -o) printf '#!/bin/sh\nexit 0\n' >"$arg" && chmod +x "$arg" ;; esac
  prev=$arg
done
exit 0
EOF
  cat >"$work/bin/halvo_capture-stub" <<'EOF'
#!/bin/sh
spec() { grep -q "^$1\$" "$STUB_SPEC"; }
ok() { printf 'test result: ok. 1 passed; 0 failed; 0 ignored\n'; exit 0; }
case $4 in
  macos::input::tests::the_gap_of_the_test_from_its_variable)
    case $HALVO_STALL_GAP_MS in
      600) printf 'gap_of_the_test refused source=HALVO_STALL_GAP_MS reason=HALVO_STALL_GAP_MS of 600 ms is above the cap of 500 ms\n' ;;
      *) printf 'gap_of_the_test gap_ms=%s source=HALVO_STALL_GAP_MS spacing_max_us=- load1=-\n' "$HALVO_STALL_GAP_MS" ;;
    esac
    ok
    ;;
  macos::input::tests::measure_the_readings_of_the_hold) ;;
  *) exit 96 ;;
esac
spec measure-exits && { printf 'test result: FAILED. 0 passed; 1 failed; 0 ignored\n'; exit 101; }
spec measure-none && ok
spec measure-no-digits && {
  printf 'measure dictionary=1 on_console=1 seconds=20.0 readings=701 spacing_max_us=90474\n'
  printf 'gap_of_the_test gap_ms=x90 source=measured spacing_max_us=90474 load1=1.00\n'
  ok
}
spec measure-gap-alone && { printf 'gap_of_the_test gap_ms=190 source=measured spacing_max_us=90474 load1=1.00\n'; ok; }
calls=$(cat "$STUB_CALLS" 2>/dev/null || printf 0)
calls=$((calls + 1))
printf '%s\n' "$calls" >"$STUB_CALLS"
if spec measure-refused; then
  printf 'measure dictionary=1 on_console=1 seconds=20.0 readings=12 spacing_max_us=90474\n'
  printf 'gap_of_the_test refused source=measured reason=12 readings, fewer than 300\n'
  ok
fi
if spec measure-under-30 ; then
  printf 'measure dictionary=1 on_console=1 seconds=20.0 readings=701 spacing_max_us=5000\n'
  printf 'gap_of_the_test gap_ms=30 source=measured spacing_max_us=5000 load1=1.00\n'
  ok
fi
if spec measure-above-twice || { spec measure-above-once && [ "$calls" = 1 ]; }; then
  printf 'measure dictionary=1 on_console=1 seconds=20.0 readings=701 spacing_max_us=260000 run=%s\n' "$calls"
  printf 'gap_of_the_test above_the_cap gap_ms=520 cap_ms=500 source=measured spacing_max_us=260000 load1=1.00\n'
  ok
fi
printf 'measure dictionary=1 on_console=1 seconds=20.0 readings=701 spacing_max_us=90474\n'
spec measure-notice && printf 'measure notice for the coordinator: spacing_p50_us=60000 readings=200\n'
printf 'gap_of_the_test gap_ms=190 source=measured spacing_max_us=90474 load1=1.00\n'
ok
EOF
  cat >"$work/bin/stall-stub" <<'EOF'
#!/bin/sh
spec() { grep -q "^$1\$" "$STUB_SPEC"; }
if [ "$*" = --list ]; then
  spec missing-test || printf 'the_probe: test\n'
  printf 'the_form_of_the_hosted_phases: test\nphase_one: test\nphase_two: test\nknown_one: test\nthree_one: test\nthree_long: test\nlaunch_one: test\nmeasured_one: test\nloop_one: test\n'
  spec extra-test && printf 'unlisted: test\n'
  exit 0
fi
for test; do :; done
if [ "$test" = the_form_of_the_hosted_phases ]; then
  spec no-form-line && exit 0
  spec form-exits && exit 101
  case $HALVO_STALL_FORM in
    long) printf 'test the_form_of_the_hosted_phases ... form=long waits_ms_and_bounds_ms=150:1000 rounds_for_each_wait=3 cap_for_each_wait=4\n' ;;
    *)
      printf 'test the_form_of_the_hosted_phases ... form=short waits_ms_and_bounds_ms=150:1000 rounds_for_each_wait=1 cap_for_each_wait=2\n'
      printf 'left_out counted rounds 2 to 3 of each wait of each mended phase\n'
      ;;
  esac
  exit 0
fi
[ -x "$HALVO_STALL_FIXTURE" ] || exit 98
[ -n "${HALVO_STALL_GAP_MS:-}" ] && [ -n "${HALVO_STALL_FORM:-}" ] || exit 99
handed=$(env | sed -n 's/^\(HALVO_STALL_[A-Z_]*\)=.*/\1/p' | sort | tr '\n' ',')
spec "prefixed-panic-$test" && { printf "test %s ... thread '%s' (1234) panicked at crates/halvo-capture/tests/macos_stall_fixture.rs:101:5:\nthe session dictionary could not be read\nnote: run with RUST_BACKTRACE=1 environment variable to display a backtrace\ntest result: FAILED. 0 passed; 1 failed; 0 ignored\n" "$test" "$test"; exit 101; }
spec "prefixed-panic-$test" && { printf "test %s ... thread '%s' (1234) panicked at crates/halvo-capture/tests/macos_stall_fixture.rs:101:5:\nthe session dictionary could not be read\nnote: run with RUST_BACKTRACE=1 environment variable to display a backtrace\ntest result: FAILED. 0 passed; 1 failed; 0 ignored\n" "$test" "$test"; exit 101; }
printf '%s gap_ms=%s form=%s handed=%s\n' "$test" "$HALVO_STALL_GAP_MS" "$HALVO_STALL_FORM" "$handed"
spec "late-click-$test" && printf '%s late_click wait_ms=150 ahead=8\n' "$test"
spec "sleep-$test" && printf '%s drops round=0 on=1 gap=0 sleep=1 stop=0\n' "$test"
spec "unclocked-$test" && printf '%s drops round=0 on=1 gap=0 sleep=0 unclocked=1 stop=0\n%s drops round=1 on=1 gap=0 sleep=0 unclocked=2 stop=0\n' "$test" "$test"
case $test in
  measured_* | loop_* | three_long)
    if [ "$HALVO_STALL_FORM" != long ]; then
      printf "thread 'x' panicked at y:\n%s: ran in the short form\ntest result: FAILED. 0 passed; 1 failed; 0 ignored\n" "$test"
      exit 101
    fi
    ;;
esac
if spec "all-not-driven-$test"; then
  printf '%s round=0 wait_ms=150 bound_ms=1000 counted=0 ahead=0 markers=0 plain=0 secure=0 rv=notdriven not_driven=output_ended_before_cleared phase_counted=0 phase_not_driven=1\n' "$test"
else
  printf '%s round=0 wait_ms=150 bound_ms=1000 counted=1 ahead=0 markers=2 plain=2 secure=8 rv=held not_driven=- phase_counted=1 phase_not_driven=0\n' "$test"
fi
spec "one-not-driven-$test" && printf '%s round=1 wait_ms=300 bound_ms=1000 counted=0 ahead=0 markers=0 plain=0 secure=8 rv=notdriven not_driven=late_reply_left_secure_within_3000_ms phase_counted=1 phase_not_driven=1\n' "$test"
spec "control-line-$test" && printf '%s control=0 bound_ms=1000 counted=1 landed_plain=5 landed_secure=0 plain_records=5 secure_records=0 after_records=3 rv=- not_driven=- phase_counted=1 phase_not_driven=0\n' "$test"
if spec "one-held-rest-not-driven-$test"; then
  for wait in 300 600 850 2000; do
    for number in 1 2; do
      printf '%s round=%s wait_ms=%s bound_ms=1000 counted=0 ahead=0 markers=0 plain=0 secure=8 rv=notdriven not_driven=late_reply_left_secure_within_3000_ms phase_counted=1 phase_not_driven=1\n' "$test" "$number" "$wait"
    done
  done
fi
spec "waits-not-driven-$test" && {
  printf "%s not_driven_waits=300,600,850,2000\nthread '%s' (1234) panicked at crates/halvo-capture/tests/macos_stall_fixture.rs:1600:9:\n%s: not driven at wait 300 ms, wait 600 ms, wait 850 ms, wait 2000 ms\ntest result: FAILED. 0 passed; 1 failed; 0 ignored\n" "$test" "$test" "$test"
  exit 101
}
spec "control-not-driven-$test" && printf '%s control=0 bound_ms=1000 counted=0 landed_plain=0 landed_secure=0 plain_records=0 secure_records=0 after_records=0 rv=notdriven not_driven=output_ended_before_cleared phase_counted=0 phase_not_driven=1\n' "$test"
spec "recorded-not-driven-$test" && printf '%s round=1 wait_ms=300 bound_ms=1000 counted=0 ahead=2 markers=0 plain=0 secure=8 rv=recorded not_driven=output_ended_before_left_secure phase_counted=1 phase_not_driven=1\n' "$test"
spec "long-output-$test" && printf 'a line that the check does not print\n  and a second one\n'
spec "late-rule-$test" && printf '%s rounds_lost_to_the_late_rule=1 by_wait=150:0,300:1\n' "$test"
spec "prose-$test" && printf '%s the session was lost\n' "$test"
spec "debug-word-$test" && printf '%s round=2 wait_ms=150 rv=notcounted("the markers were not recorded")\n' "$test"
spec "panic-$test" && {
  printf "thread '%s' (1234) panicked at crates/halvo-capture/tests/macos_stall_fixture.rs:500:27:\nthe fixture gave no line that starts with left_secure within 3000 ms\nnote: run with RUST_BACKTRACE=1 environment variable to display a backtrace\ntest %s ... FAILED\ntest result: FAILED. 0 passed; 1 failed; 0 ignored\n" "$test" "$test"
  exit 101
}
spec "touch-$test" && printf 'changed\n' >>"$CI_D_REPO/tracked.txt"
spec "hang-$test" && sleep 5
spec "slow-$test" && sleep 1
spec "fail-$test" && { printf "thread 'x' panicked at y:\n%s: rounds that did not count: wait 150 ms\ntest result: FAILED. 0 passed; 1 failed; 0 ignored\n" "$test"; exit 101; }
spec "leak-$test" && { printf "thread 'x' panicked at y:\n%s: recorded keys beside the markers in rounds that counted: wait 150 ms: 3\ntest result: FAILED. 0 passed; 1 failed; 0 ignored\n" "$test"; exit 101; }
spec "empty-$test" && { printf 'test result: ok. 0 passed; 0 failed; 1 ignored\n'; exit 0; }
spec "crash-$test" && { printf 'test result: FAILED. 0 passed; 1 failed; 0 ignored\n'; exit 134; }
case $test in
  three_*)
    if spec "silent-$test"; then :
    elif spec "notdriven-$test"; then printf '%s verdict=notdriven held=3 failed=0 notdriven=2\n' "$test"
    elif spec "notkept-$test"; then printf '%s verdict=notkept widest_gap_us=61000 gap_drops=2\n' "$test"
    else printf '%s verdict=held held=5 failed=0 notdriven=0\n' "$test"; fi
    printf 'test result: ok. 1 passed; 0 failed; 0 ignored\n'
    exit 0
    ;;
  known_*)
    spec "pass-$test" && { printf 'test result: ok. 1 passed; 0 failed; 0 ignored\n'; exit 0; }
    printf "thread 'x' panicked at y:\n%s: recorded keys beside the markers in rounds that counted: wait 1500 ms: 9\ntest result: FAILED. 0 passed; 1 failed; 0 ignored\n" "$test"
    exit 101
    ;;
esac
printf 'test result: ok. 1 passed; 0 failed; 0 ignored\n'
exit 0
EOF
  chmod +x "$work/bin/cargo" "$work/bin/clang" "$work/bin/stall-stub" "$work/bin/halvo_capture-stub" || exit 2
}

manifest_case() {
  printf '%b' "$4" >"$work/repo/crates/halvo-capture/Cargo.toml" || exit 2
  git -C "$work/repo" add crates || exit 2
  check "$1" "$2" "$3" ''
}

write_measured_ledger() {
  ledger_file=$work/repo/docs/baseline/ledger.d/macos-2b.txt
  mkdir -p "$work/repo/docs/baseline/ledger.d" "$work/repo/crates/halvo-capture/src/macos" \
    "$work/repo/crates/halvo-capture/tests/macos_stall_fixture" "$work/repo/scripts/r1/ci.d" || exit 2
  measured_files | while read -r path; do
    [ -f "$work/repo/$path" ] || printf '%s\n' "$path" >"$work/repo/$path" || exit 2
    git -C "$work/repo" add -- "$path" || exit 2
  done
  printf '%s\n' "${ledger_run_line:-measured run=1 form=long result=pass gap_ms=190 mended_rounds_held=60 phases_not_driven=0 commit=0 figures=none}" >"$ledger_file" || exit 2
  measured_files | while read -r path; do
    entry=$(git -C "$work/repo" ls-files --stage -- "$path") || exit 2
    blob=${entry#* }
    printf 'measured blob path=%s id=%s\n' "$path" "${blob%% *}"
  done >>"$ledger_file" || exit 2
  printf '%s\n' 'measured seam path=crates/halvo-capture/Cargo.toml table=[features] text=stall-seam = []' \
    'measured seam path=crates/halvo-capture/Cargo.toml table=[dev-dependencies] text=halvo-capture = { path = ".", features = ["stall-seam"] }' \
    >>"$ledger_file" || exit 2
  git -C "$work/repo" add docs || exit 2
}

self_test() {
  unset HALVO_STALL_GAP_MS HALVO_STALL_FORM MS_BUDGET_S
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name-self-test.XXXXXX") || {
    printf 'FAIL self-test:%s (mktemp failed)\n' "$name"
    exit 1
  }
  mkdir -p "$work/bin" "$work/repo/target/ci-artifacts/12-test" || exit 2
  write_stubs
  printf 'the_probe not-run\nthe_form_of_the_hosted_phases not-run\nphase_one phase\nphase_two phase\nknown_one known-fault\nthree_one three-way\nthree_long three-way long\nlaunch_one twelve\nmeasured_one measured\nloop_one loop\n' >"$work/expected-small"
  printf 'phase_two shows what it shows; it does not show the rest\n' >"$work/notes-small"
  git -C "$work/repo" init -q || exit 2
  printf 'tracked\n' >"$work/repo/tracked.txt" || exit 2
  mkdir -p "$work/repo/crates/halvo-capture" "$work/repo/crates/other" || exit 2
  clock_lines='pub struct Monotonic(u64);
pub struct Uptime(u64);'
  mkdir -p "$work/repo/crates/halvo-capture/src" || exit 2
  printf '%s\n' "$clock_lines" >"$work/repo/crates/halvo-capture/src/secure_input_macos_hold.rs" || exit 2
  seam_manifest='[package]\nname = "halvo-capture"\n\n[features]\nstall-seam = []\n\n[dependencies]\n\n[dev-dependencies]\nhalvo-capture = { path = ".", features = ["stall-seam"] }\n'
  printf '%b' "$seam_manifest" >"$work/repo/crates/halvo-capture/Cargo.toml" || exit 2
  printf '[dependencies]\n' >"$work/repo/crates/other/Cargo.toml" || exit 2
  git -C "$work/repo" add tracked.txt crates || exit 2
  write_measured_ledger
  twelve=$work/repo/target/ci-artifacts/12-test/tests.txt
  ignored='     Running tests/macos_stall_fixture.rs (target/debug/deps/macos_stall_fixture-0)
test a_key_typed_ahead_into_a_secure_field_is_not_recorded ... ignored, posts keys and shows a window: the hosted step alone runs it
test the_probe_reports_what_the_stall_phases_rest_on ... ignored
test launch_one ... ok
     Running tests/source_rules.rs (target/debug/deps/source_rules-0)
test a_rule ... ok'
  printf '%s\n' "$ignored" >"$twelve"
  wrong=
  rejected=0
  limit=
  check all-pass PASS 'MS total=4 pass=3 known_fault=1 notdriven=0 notkept=0 fail=0' ''
  check note-line PASS 'MS note phase_two: shows what it shows; it does not show the rest' ''
  check measured-named-not-run PASS 'MS left out: measured_one kind=measured form=long run=not_in_this_form asserts=nothing' ''
  check loop-asserts PASS 'MS left out: loop_one kind=loop form=long run=not_in_this_form
MS ' ''
  check loop-named-not-run PASS 'MS left out: loop_one kind=loop form=long run=not_in_this_form' ''
  check long-three-way-not-run PASS 'MS left out: three_long kind=three-way form=long run=not_in_this_form' ''
  check form-line-first PASS 'MS form=short waits_ms_and_bounds_ms=150:1000 rounds_for_each_wait=1 cap_for_each_wait=2
MS left out: counted rounds 2 to 3 of each wait of each mended phase
measure dictionary=1 on_console=1 seconds=20.0 readings=701 spacing_max_us=90474
MS gap_ms=190 source=measured spacing_max_us=90474 load1=1.00' ''
  check form-line-missing FAIL 'the_form_of_the_hosted_phases printed no line of the form short' 'no-form-line'
  check form-test-exits FAIL 'the_form_of_the_hosted_phases did not print the form: exited 101' 'form-exits'
  check the-word-alone PASS '| phase_one gap_ms=190 form=short handed=HALVO_STALL_FIXTURE,HALVO_STALL_FORM,HALVO_STALL_GAP_MS,' ''
  check budget-line PASS 'budget=240 notices=0' ''
  check notice-of-the-measurement PASS 'budget=240 notices=1' 'measure-notice'
  check notice-of-a-late-click PASS 'budget=240 notices=1' 'late-click-phase_one'
  check notice-of-sleep PASS 'budget=240 notices=1' 'sleep-phase_two'
  check unclocked-by-name PASS 'budget=240 notices=0 unclocked=2' 'unclocked-phase_one'
  check unclocked-of-none PASS 'budget=240 notices=0 unclocked=0' ''
  MS_BUDGET_S=0
  export MS_BUDGET_S
  check over-the-budget PASS 'above its budget of 0 s; this fails nothing' 'slow-phase_one'
  unset MS_BUDGET_S
  check gap-measured-again PASS 'the gap of the measurement is above its cap: measured again (ruling MA-23 Addition 1 (4))
measure dictionary=1 on_console=1 seconds=20.0 readings=701 spacing_max_us=90474
MS gap_ms=190 source=measured' 'measure-above-once'
  check gap-above-the-cap-twice FAIL "the runner's readings came further apart than the hosted proof is stated for: measure dictionary=1 on_console=1 seconds=20.0 readings=701 spacing_max_us=260000 run=1 / measure dictionary=1 on_console=1 seconds=20.0 readings=701 spacing_max_us=260000 run=2; run the job again" 'measure-above-twice'
  check gap-refused FAIL "the gap of the test's own could not be measured: 12 readings, fewer than 300" 'measure-refused'
  check gap-under-30 PASS 'MS gap_ms=30 source=measured spacing_max_us=5000' 'measure-under-30'
  check gap-no-measure-line FAIL "measure_the_readings_of_the_hold printed no gap line" 'measure-none'
  check gap-line-without-digits FAIL "the gap of the test's own could not be measured: the gap line reads gap_ms=x90 source=measured" 'measure-no-digits'
  check gap-line-without-a-measure-line FAIL "measure_the_readings_of_the_hold printed no measure line" 'measure-gap-alone'
  check limit-of-the-short-form PASS ' limit=150' ''
  check gap-measurement-exits FAIL "the gap of the test's own could not be measured: measure_the_readings_of_the_hold exited 101" 'measure-exits'
  check gap-no-library FAIL "cargo named no test executable of the library" 'no-library'
  check no-phase-without-a-gap FAIL 'MS form=short' 'measure-refused'
  HALVO_STALL_GAP_MS=200
  export HALVO_STALL_GAP_MS
  check gap-from-the-variable PASS 'MS gap_ms=200 source=HALVO_STALL_GAP_MS spacing_max_us=- load1=-' ''
  check gap-of-the-variable-reaches-the-phases PASS '| phase_one gap_ms=200 form=short' ''
  HALVO_STALL_GAP_MS=600
  check gap-variable-above-the-cap FAIL 'HALVO_STALL_GAP_MS of 600 ms is above the cap of 500 ms' ''
  unset HALVO_STALL_GAP_MS
  HALVO_STALL_FORM=long
  export HALVO_STALL_FORM
  check long-form PASS 'MS total=7 pass=6 known_fault=1 notdriven=0 notkept=0 fail=0' ''
  check long-form-measured-line PASS ' limit=1500 asserts=nothing' ''
  check limit-of-a-long-phase PASS ' limit=900' ''
  check long-form-line PASS 'MS form=long seconds=' ''
  check long-form-reaches-the-phases PASS '| phase_one gap_ms=190 form=long' ''
  HALVO_STALL_FORM=medium
  check form-refused FAIL 'HALVO_STALL_FORM is short or long, not medium' ''
  unset HALVO_STALL_FORM
  check history-line PASS 'MS note pushes 5 to 7: in the Tab, click, focus-move, leaving-key and whole-password phases' ''
  check three-way-not-kept PASS 'MS three_one result=pass verdict=notkept' 'notkept-three_one'
  check three-way-not-driven PASS 'MS three_one result=pass verdict=notdriven' 'notdriven-three_one'
  check three-way-without-a-verdict FAIL 'printed no verdict line of held, notdriven or notkept' 'silent-three_one'
  check panic-line FAIL "    | thread 'phase_one' (1234) panicked at crates/halvo-capture/tests/macos_stall_fixture.rs:500:27:
    | the fixture gave no line that starts with left_secure within 3000 ms" 'panic-phase_one'
  check panic-line-after-the-test-name FAIL '    | the session dictionary could not be read' 'prefixed-panic-phase_one'
  check every-phase-round-not-driven-beside-a-control FAIL 'three_one: every round of the phase was not driven' 'all-not-driven-three_one
control-line-three_one'
  check every-control-round-not-driven FAIL 'phase_two: every round of the control was not driven' 'control-not-driven-phase_two'
  check every-round-reads-not-driven FAIL 'MS phase_one result=not-driven counted=0 not_driven=1 seconds=' 'all-not-driven-phase_one'
  check short-form-one-held-round-passes PASS 'MS phase_one result=pass counted=1 not_driven=8 seconds=' 'one-held-rest-not-driven-phase_one'
  check short-form-prints-by-wait PASS ' by_wait=150:1/0,300:0/2,600:0/2,850:0/2,2000:0/2' 'one-held-rest-not-driven-phase_one'
  check short-form-total-names-no-phase-not-driven PASS 'rounds_not_driven=8 phases_not_driven=0' 'one-held-rest-not-driven-phase_one'
  HALVO_STALL_FORM=long
  export HALVO_STALL_FORM
  check long-form-waits-not-driven FAIL 'MS phase_one result=not-driven counted=1 not_driven=8 seconds=' 'one-held-rest-not-driven-phase_one
waits-not-driven-phase_one'
  check long-form-waits-not-driven-named FAIL 'phase_one: not driven at waits 300,600,850,2000 (the long form takes no record from this run)' 'one-held-rest-not-driven-phase_one
waits-not-driven-phase_one'
  check long-form-total-names-the-phase FAIL 'phases_not_driven=1' 'one-held-rest-not-driven-phase_one
waits-not-driven-phase_one'
  check long-form-every-round-reads-not-driven FAIL 'MS phase_one result=not-driven counted=0 not_driven=1 seconds=' 'all-not-driven-phase_one'
  unset HALVO_STALL_FORM
  check panic-line-after-the-test-name FAIL '    | the session dictionary could not be read' 'prefixed-panic-phase_one'
  check one-round-not-driven PASS 'MS phase_one result=pass counted=1 not_driven=1 seconds=' 'one-not-driven-phase_one'
  check known-fault-recorded-in-a-round-not-driven PASS 'MS known_one result=known-fault counted=1 not_driven=1 seconds=' 'recorded-not-driven-known_one'
  check rounds-not-driven-in-the-total PASS 'MS total=4 pass=3 known_fault=1 notdriven=0 notkept=0 fail=0 rounds_counted=4 rounds_not_driven=1' 'one-not-driven-phase_one'
  check every-round-not-driven FAIL 'phase_one: every round of the phase was not driven' 'all-not-driven-phase_one'
  check recorded-in-a-round-not-driven FAIL 'phase_one: a key was recorded in a round not driven' 'recorded-not-driven-phase_one'
  check artifacts-hold-the-output PASS 'MS phase_one result=pass' 'long-output-phase_one'
  check prose-under-the-test-name FAIL 'phase_one: line 3 of its output holds more than names and numbers under the name of the test (ruling MA-21)' 'prose-phase_one'
  check debug-text-under-the-test-name FAIL 'phase_two: line 3 of its output holds more than names and numbers under the name of the test (ruling MA-21)' 'debug-word-phase_two'
  check names-and-numbers-of-a-known-fault PASS 'MS known_one result=known-fault' ''
  check rounds-lost-to-the-late-rule-on-the-ms-line PASS ' by_wait=150:1/0 rounds_lost_to_the_late_rule=1 lost_by_wait=150:0,300:1' 'late-rule-phase_one'
  check rounds-lost-to-the-late-rule-in-the-total PASS 'phases_not_driven=0 rounds_lost_to_the_late_rule=1' 'late-rule-phase_one'
  printf '%s\n' 'phase_one gap_ms=190 form=short handed=HALVO_STALL_FIXTURE,HALVO_STALL_FORM,HALVO_STALL_GAP_MS,' \
    'phase_one round=0 wait_ms=150 bound_ms=1000 counted=1 ahead=0 markers=2 plain=2 secure=8 rv=held not_driven=- phase_counted=1 phase_not_driven=0' \
    'a line that the check does not print' '  and a second one' 'test result: ok. 1 passed; 0 failed; 0 ignored' \
    >"$work/output-wanted" || exit 2
  kept=$work/artifacts-artifacts-hold-the-output/tests
  if cmp -s "$work/output-wanted" "$kept/phase_one.txt" && [ -s "$kept/the_form_of_the_hosted_phases.txt" ] &&
    [ -s "$kept/measure_the_readings_of_the_hold-first.txt" ]; then
    printf 'case %s/artifacts-hold-the-whole-output want=PASS got=PASS\n' "$name"
  else
    printf 'case %s/artifacts-hold-the-whole-output want=PASS got=another-output\n' "$name"
    wrong="$wrong artifacts-hold-the-whole-output"
  fi
  check phase-fails FAIL 'FAIL 51-macos-stall (1 tests failed)' 'fail-phase_one'
  check phase-runs-no-test FAIL 'phase_two: exit 0, and not one test ran and passed' 'empty-phase_two'
  check known-fault-passes FAIL 'FIXED-BUT-LISTED' 'pass-known_one'
  check known-fault-fails-otherwise FAIL 'failed without its assertion of a recorded key' 'fail-known_one'
  check phase-leaks FAIL 'phase_one result=FAIL' 'leak-phase_one'
  check extra-test FAIL 'lists tests that this script' 'extra-test'
  check listed-test-missing FAIL 'does not list these tests of this script'"'"'s list: the_probe' 'missing-test'
  check known-fault-other-exit FAIL 'known_one: exit 134, and not the exit 101 of one failed test' 'crash-known_one'
  check known-fault-runs-no-test FAIL 'known_one: exit 0, and not one test ran and passed' 'empty-known_one'
  check twins-fail FAIL 'the twins of the proofs did not build' 'twins-fail'
  check proofs-build FAIL 'the proofs built' 'proofs-build'
  check proof-fails-with-another-code FAIL 'did not fail with E0599' 'proof-other-code'
  check one-error-for-two-proofs FAIL '(1 errors E0599 for 2 proofs)' 'proof-one-e0599'
  check one-debug-error-for-two-proofs FAIL '(1 errors E0277 for 2 proofs)' 'proof-one-e0277'
  check seam-twin-fails FAIL 'the twin of the seam proof did not build with the feature stall-seam' 'seam-twin-fails'
  check seam-proof-builds FAIL 'the seam proof built with the feature stall-seam' 'seam-proof-builds'
  check seam-proof-other-code FAIL 'the seam proof with the feature stall-seam did not fail with E0599' 'seam-proof-other-code'
  check drop-counts-in-a-release-build FAIL 'the proof drop_counts_in_a_release_build with the feature stall-seam did not fail with E0433' 'drop-counts-in-a-release-build'
  check drop-counts-proof-refused PASS 'proof drop_counts_in_a_release_build with the feature stall-seam: refused with E0433' ''
  check fixture-fails FAIL 'the fixture did not compile' 'fixture-fails'
  check test-build-fails FAIL 'the test build failed' 'test-build-fails'
  check tracked-file-changed FAIL 'a tracked file differs from HEAD after the run' 'touch-phase_two'
  git -C "$work/repo" checkout -q -- tracked.txt 2>/dev/null || printf 'tracked\n' >"$work/repo/tracked.txt"
  limit=1
  check time-limit FAIL 'phase_one: exit 142' 'hang-phase_one'
  printf '     Running tests/macos_stall_fixture.rs (x)\ntest a_key_typed_ahead_into_a_secure_field_is_not_recorded ... ok\ntest launch_one ... ok\ntest the_probe_reports_what_the_stall_phases_rest_on ... ignored\n' >"$twelve"
  check twelve-test-ran-a-phase FAIL 'that are not the launch-path test: a_key_typed_ahead_into_a_secure_field_is_not_recorded' ''
  printf '     Running tests/macos_stall_fixture.rs (x)\ntest the_probe_reports_what_the_stall_phases_rest_on ... ignored\n' >"$twelve"
  check twelve-test-ran-no-launch FAIL 'did not run the launch-path test of tests/macos_stall_fixture.rs: launch_one' ''
  printf '     Running tests/macos_stall_fixture.rs (x)\ntest launch_one ... FAILED\ntest the_probe_reports_what_the_stall_phases_rest_on ... ignored\n' >"$twelve"
  check twelve-test-launch-failed FAIL 'ran the launch-path test and it did not pass: launch_one' ''
  printf '     Running tests/macos_stall_fixture.rs (x)\ntest launch_one ... ok\n' >"$twelve"
  check twelve-test-listed-none FAIL '12-test listed no test of tests/macos_stall_fixture.rs as ignored' ''
  mv "$twelve" "$work/twelve-moved" || exit 2
  absent_text='12-test left no list of its tests (target/ci-artifacts/12-test/tests.txt is missing or names no test): the short form runs in the every-push job after 12-test'
  check short-form-with-an-empty-directory FAIL "$absent_text" ''
  HALVO_STALL_FORM=long
  export HALVO_STALL_FORM
  check long-form-with-an-empty-directory PASS '12-test: no list of tests in this job (the long form, run by name)' ''
  unset HALVO_STALL_FORM
  : >"$twelve" || exit 2
  check short-form-with-an-empty-list FAIL "$absent_text" ''
  HALVO_STALL_FORM=long
  export HALVO_STALL_FORM
  check long-form-with-an-empty-list PASS '12-test: no list of tests in this job (the long form, run by name)' ''
  unset HALVO_STALL_FORM
  printf '%s\n' "$ignored" >"$twelve"
  printf '[dependencies]\nhalvo-capture = { path = "../halvo-capture", features = ["stall-seam"] }\n' >"$work/repo/crates/other/Cargo.toml" || exit 2
  git -C "$work/repo" add crates/other/Cargo.toml || exit 2
  check seam-named-by-another-manifest FAIL 'manifests that name it: crates/halvo-capture/Cargo.toml crates/other/Cargo.toml' ''
  printf '[dependencies]\n' >"$work/repo/crates/other/Cargo.toml" || exit 2
  printf '[features]\n' >"$work/repo/crates/halvo-capture/Cargo.toml" || exit 2
  git -C "$work/repo" add crates || exit 2
  check seam-in-no-manifest FAIL 'manifests that name it: none' ''
  printf '%b' "$seam_manifest" >"$work/repo/crates/halvo-capture/Cargo.toml" || exit 2
  git -C "$work/repo" add crates || exit 2
  write_measured_ledger
  check ledger-holds PASS 'ledger: the measured run 1 of the long form ran on the blobs that the head has of the eight files of the hold and on its stall-seam lines of crates/halvo-capture/Cargo.toml' ''
  printf 'mod carbon {\n    fn EnableSecureEventInput() -> i32;\n    fn DisableSecureEventInput() -> i32;\n    #[link_name = concat!("Is", "SecureEventInputEnabled")]\n    fn is_on() -> u8;\n}\n' >"$work/repo/crates/halvo-capture/src/secure_input_macos.rs" || exit 2
  mkdir -p "$work/repo/crates/other/src" "$work/repo/docs/research" "$work/repo/scripts/r4/ci.d" "$work/repo/tools/r4-trials/app/system" "$work/repo/.cargo" || exit 2
  printf 'extern "C" {\n    #[link_name = "fdopendir%sINODE64"]\n    fn fdopendir() -> i32;\n}\n' "\$" >"$work/repo/crates/other/src/lib.rs" || exit 2
  printf 'The hold takes EnableSecureEventInput and gives it back by DisableSecureEventInput.\n' >"$work/repo/docs/research/researcher-4.md" || exit 2
  printf '# rule: a name inside a string (link_name, dlsym)\n' >"$work/repo/scripts/r4/ci.d/20-r4-core.sh" || exit 2
  printf 'let responsibleCall = dlsym(dlopen(nil, RTLD_NOW), "responsibility_get_pid_responsible_for_pid")\n' >"$work/repo/tools/r4-trials/app/system/macos.swift" || exit 2
  printf 'BEGIN { link_name["a"] = 1 }\n' >"$work/repo/crates/other/names.awk" || exit 2
  git -C "$work/repo" add -A || exit 2
  write_measured_ledger
  check carbon-calls-and-a-plain-link-name PASS 'carbon: in every tracked text file but crates/halvo-capture/src/secure_input_macos.rs' ''
  printf 'paste::paste! {\n    extern "C" {\n        fn [<Enable SecureEventInput>]() -> i32;\n    }\n}\n' >"$work/repo/crates/other/src/lib.rs" || exit 2
  git -C "$work/repo" add -A || exit 2
  check carbon-limit-a-name-joined-at-build-time PASS 'limits: a name joined at build or run time' ''
  carbon_text='stands outside crates/halvo-capture/src/secure_input_macos.rs, whose one lock holds the three Carbon calls: '
  n=0
  for form in 'crates/other/src/lib.rs:extern "C" {\n    fn DisableSecureEventInput() -> i32;\n}\n' \
    'crates/other/src/hold.mm:void hold(void) { EnableSecureEventInput(); }\n' \
    'crates/other/src/hold.cpp:void hold(void) { DisableSecureEventInput(); }\n' \
    'crates/other/src/lib.rs:extern "C" {\n    #[link_name = concat!("Enable", "SecureEventInput")]\n    fn hold() -> i32;\n}\n' \
    'crates/other/src/lib.rs:extern "C" {\n    #[link_name = "\\x45nableSecureEventInput"]\n    fn hold() -> i32;\n}\n' \
    'crates/other/src/hold.c:int hold(void) __asm__("_EnableSecureEventInput");\nint take(void) { return hold(); }\n' \
    'crates/other/src/hold.s:\t.globl _take\n_take:\n\tb _DisableSecureEventInput\n' \
    'crates/other/build.rs:fn main() {\n    println!("cargo:rustc-link-arg=-Wl,-alias,_EnableSecureEventInput,_halvo_hold");\n}\n' \
    'crates/other/src/hold.cxx:void hold() { EnableSecureEventInput(); }\n' \
    'crates/other/src/hold.hpp:inline void hold(void) { EnableSecureEventInput(); }\n' \
    'crates/other/src/hold.inc:extern "C" {\n    fn DisableSecureEventInput() -> i32;\n}\n' \
    'crates/other/src/lib.rs:extern "C" { #[link_name = "fdopendir"] fn a() -> i32; #[link_name = concat!("Enable", "SecureEventInput")] fn hold() -> i32; }\n' \
    'crates/other/src/lib.rs:fn hold(handle: *mut u8) -> *mut u8 {\n    unsafe { dlsym(handle, concat!("Enable", "SecureEventInput").as_ptr()) }\n}\n' \
    'crates/other/src/lib.rs:fn open() -> *mut u8 {\n    unsafe { dlopen(c"/System/Library/Frameworks/Carbon.framework/Carbon".as_ptr(), 1) }\n}\n' \
    'crates/other/src/hold.m:void *hold(CFBundleRef bundle) { return CFBundleGetFunctionPointerForName(bundle, CFSTR("Enable" "SecureEventInput")); }\n' \
    'docs/notes.md:A later unit may call EnableSecureEventInput from its own file.\n'; do
    path=${form%%:*}
    printf '%b' "${form#*:}" >"$work/repo/$path" || exit 2
    git -C "$work/repo" add -A || exit 2
    n=$((n + 1))
    check "carbon-outside-the-lock-$n-in-${path##*/}" FAIL "$carbon_text$path (" ''
    printf 'none\n' >"$work/repo/$path" || exit 2
    git -C "$work/repo" add -A || exit 2
  done
  printf '[build]\n%s%s = ["-C", "link-arg=-Wl,-alias,_DisableSecureEventInput,_halvo_give"]\n' rust flags >"$work/repo/.cargo/config.toml" || exit 2
  git -C "$work/repo" add -A || exit 2
  check carbon-outside-the-lock-in-a-cargo-config FAIL "$carbon_text.cargo/config.toml (" ''
  printf 'none\n' >"$work/repo/.cargo/config.toml" || exit 2
  git -C "$work/repo" add -A || exit 2
  held=crates/halvo-capture/src/store/hardening_tests/full_flush.rs
  held_head='use std::ffi::CStr;\n\nconst INTERPOSER: &str = r#"\nlong r1_full_flushes(void) { return full_flushes; }\nINTERPOSE(r1_fcntl, fcntl);\n"#;\n\n'
  held_symbol='fn symbol(name: &CStr) -> Option<*mut libc::c_void> {\n    let found = unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) };\n    (!found.is_null()).then_some(found)\n}\n\n'
  held_calls='fn loaded() -> Option<()> {\n    let total = symbol(c"r1_full_flushes")?;\n    let plain = symbol(c"r1_plain_flushes")?;\n    let path_at = symbol(c"r1_full_flush_path")?;\n    let fail_next = symbol(c"r1_fail_full_flushes")?;\n    Some(())\n}\n'
  held_store3c=$held_head$held_symbol$held_calls
  mkdir -p "$work/repo/${held%/*}" || exit 2
  held_case() {
    printf '%b' "$4" >"$work/repo/$5" || exit 2
    git -C "$work/repo" add -A || exit 2
    check "$1" "$2" "$3" ''
  }
  held_case carbon-held-file-as-store-3c-has-it PASS "$held looks a symbol up by name only in its fn symbol, by the plain literal names r1_full_flushes r1_plain_flushes r1_full_flush_path r1_fail_full_flushes" "$held_store3c" "$held"
  held_case carbon-held-file-lookup-by-another-name FAIL "$held: fn symbol gets a name that is no plain literal beginning r1_, or is named other than in such a call, at line 15" \
    "$held_head$held_symbol"'fn loaded() -> Option<()> {\n    let total = symbol(c"r1_full_flushes")?;\n    let pid = symbol(c"getpid")?;\n    Some(())\n}\n' "$held"
  held_case carbon-held-file-lookup-by-a-variable FAIL "$held: fn symbol gets a name that is no plain literal beginning r1_, or is named other than in such a call, at line 15" \
    "$held_head$held_symbol"'fn loaded() -> Option<()> {\n    let name = c"r1_full_flushes";\n    let total = symbol(name)?;\n    Some(())\n}\n' "$held"
  held_case carbon-held-file-lookup-by-a-name-built-at-run-time FAIL "$held: fn symbol gets a name that is no plain literal beginning r1_, or is named other than in such a call, at line 14" \
    "$held_head$held_symbol"'fn loaded() -> Option<()> {\n    let total = symbol(&std::ffi::CString::new(format!("r1_{}", "full_flushes")).ok()?)?;\n    Some(())\n}\n' "$held"
  held_case carbon-held-file-lookup-outside-fn-symbol FAIL "$held: a lookup by name outside fn symbol at line 22" \
    "$held_store3c"'\nfn other() -> *mut libc::c_void {\n    unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"getpid".as_ptr()) }\n}\n' "$held"
  held_case carbon-held-file-dlopen-in-fn-symbol FAIL "$held: fn symbol is not the two lines that this check holds, at line 9" \
    "$held_head"'fn symbol(name: &CStr) -> Option<*mut libc::c_void> {\n    let found = unsafe { libc::dlopen(name.as_ptr(), 1) };\n    (!found.is_null()).then_some(found)\n}\n\n'"$held_calls" "$held"
  held_case carbon-held-file-name-shadowed-in-fn-symbol FAIL "$held: fn symbol is not the two lines that this check holds, at line 9" \
    "$held_head"'fn symbol(name: &CStr) -> Option<*mut libc::c_void> {\n    let name = c"getpid";\n    let found = unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) };\n    (!found.is_null()).then_some(found)\n}\n\n'"$held_calls" "$held"
  held_case carbon-held-file-fn-symbol-as-a-value FAIL "$held: fn symbol gets a name that is no plain literal beginning r1_, or is named other than in such a call, at line 22" \
    "$held_store3c"'\nfn other() -> Option<*mut libc::c_void> {\n    let look = symbol;\n    look(c"getpid")\n}\n' "$held"
  held_case carbon-held-file-a-second-fn-symbol-in-a-module FAIL "$held: a lookup by name outside fn symbol at line 24" \
    "$held_store3c"'\nmod inner {\n    use std::ffi::CStr;\n    fn symbol(name: &CStr) -> *mut libc::c_void {\n        unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) }\n    }\n}\n' "$held"
  held_case carbon-held-file-a-module-in-another-file FAIL "$held: a module item, the word mod in the code, at line 21" \
    "$held_store3c"'\nmod more;\n' "$held"
  held_case carbon-held-file-a-carbon-name FAIL "$carbon_text$held (" "$held_store3c"'\nfn hold() {\n    unsafe { EnableSecureEventInput() };\n}\n' "$held"
  held_case carbon-held-file-a-carbon-name-with-underscores FAIL "$carbon_text$held (" "$held_store3c"'\nconst NAME: &str = "__DisableSecureEventInput";\n' "$held"
  held_case carbon-held-file-a-link-name-that-is-no-plain-literal FAIL "$carbon_text$held (" \
    "$held_store3c"'\nextern "C" {\n    #[link_name = concat!("r1_", "full_flushes")]\n    fn total() -> i64;\n}\n' "$held"
  printf 'none\n' >"$work/repo/$held" || exit 2
  held_case carbon-a-lookup-in-another-file-of-the-store FAIL "${carbon_text}crates/halvo-capture/src/store/lookup.rs (" \
    'fn total() -> *mut libc::c_void {\n    unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"r1_full_flushes".as_ptr()) }\n}\n' crates/halvo-capture/src/store/lookup.rs
  printf 'none\n' >"$work/repo/crates/halvo-capture/src/store/lookup.rs" || exit 2
  held_more=crates/halvo-capture/src/store/hardening_tests/full_flush_more.rs
  held_parent=crates/halvo-capture/src/store/hardening_tests.rs
  printf 'fn other() -> Option<*mut libc::c_void> {\n    super::symbol(c"getpid")\n}\n' >"$work/repo/$held_more" || exit 2
  held_case carbon-held-file-b12-a-path-attribute-and-a-module-on-one-line FAIL "$held: a path attribute, the word path in an attribute, at line 21" \
    "$held_store3c"'\n#[path = "full_flush_more.rs"] mod more;\n' "$held"
  held_case carbon-held-file-b14-a-module-with-its-semicolon-on-the-next-line FAIL "$held: a module item, the word mod in the code, at line 21" \
    "$held_store3c"'\nmod full_flush_more\n;\n' "$held"
  printf 'fn other() -> Option<*mut libc::c_void> {\n    symbol(c"getpid")\n}\n' >"$work/repo/$held_more" || exit 2
  held_case carbon-held-file-b13-an-include-of-another-file FAIL "$held: an include!, the word include in the code, at line 21" \
    "$held_store3c"'\ninclude!("full_flush_more.rs");\n' "$held"
  printf 'none\n' >"$work/repo/$held_more" || exit 2
  printf 'macro_rules! lookup {\n    (%sn:expr) => {\n        symbol(%sn)\n    };\n}\n' "\$" "\$" >"$work/repo/$held_parent" || exit 2
  held_case carbon-held-file-b16-a-macro-of-the-parent-file-given-a-c-string FAIL "$held: a C string literal that does not begin r1_ at line 22" \
    "$held_store3c"'\nfn other() -> Option<*mut libc::c_void> {\n    lookup!(c"getpid")\n}\n' "$held"
  printf 'none\n' >"$work/repo/$held_parent" || exit 2
  held_case carbon-held-file-a-module-with-a-body-after-an-attribute FAIL "$held: a module item, the word mod in the code, at line 21" \
    "$held_store3c"'\n#[cfg(test)] pub(crate) mod inner {\n}\n' "$held"
  held_case carbon-held-file-a-macro-rules FAIL "$held: a macro_rules!, the word macro_rules in the code, at line 22" \
    "$held_store3c"'\n// a comment before the item\nmacro_rules! nothing {\n    () => {};\n}\n' "$held"
  held_case carbon-held-file-a-path-attribute-on-a-function FAIL "$held: a path attribute, the word path in an attribute, at line 21" \
    "$held_store3c"'\n#[cfg_attr(test, path = "other.rs")]\nfn f() {}\n' "$held"
  held_case carbon-held-file-an-include-by-its-path-beside-include-str FAIL "$held: an include!, the word include in the code, at line 22" \
    "$held_store3c"'\nconst TEXT: &str = include_str!("x.txt");\nstd::include!("full_flush_more.rs");\n' "$held"
  held_case carbon-held-file-a-module-after-a-char-literal-of-a-quote FAIL "$held: a module item, the word mod in the code, at line 22" \
    "$held_store3c"'\nconst QUOTE: char = \0047"\0047;\nmod more;\n' "$held"
  held_case carbon-held-file-a-module-after-a-string-with-an-escaped-quote FAIL "$held: a module item, the word mod in the code, at line 22" \
    "$held_store3c"'\nconst TEXT: &str = "a \\" b";\nmod more;\n' "$held"
  held_case carbon-held-file-p1-pub-crate-on-the-line-above-fn-symbol FAIL "$held: the word pub in the code, at line 8" \
    "$held_head"'pub(crate)\n'"$held_symbol$held_calls" "$held"
  held_case carbon-held-file-p2-pub-and-a-comment-on-the-line-above-fn-symbol FAIL "$held: the word pub in the code, at line 8" \
    "$held_head"'pub /* seen by all */\n'"$held_symbol$held_calls" "$held"
  held_case carbon-held-file-pub-above-an-attribute-line FAIL "$held: the word pub in the code, at line 8" \
    "$held_head"'pub\n#[inline]\n'"$held_symbol$held_calls" "$held"
  held_case carbon-held-file-pub-in-a-path-over-two-lines FAIL "$held: the word pub in the code, at line 8" \
    "$held_head"'pub(in crate::\nstore)\n'"$held_symbol$held_calls" "$held"
  held_case carbon-held-file-c01-a-module-after-a-bare-carriage-return FAIL "$held: a module item, the word mod in the code, at line 21" \
    "$held_store3c"'\nmod\rfull_flush_more;\n' "$held"
  held_case carbon-held-file-the-letters-of-pub-elsewhere PASS "$held looks a symbol up by name only in its fn symbol, by the plain literal names r1_full_flushes r1_plain_flushes r1_full_flush_path r1_fail_full_flushes" \
    "$held_store3c"'\nfn publish() {}\n// pub in a comment\nconst P: &str = "pub";\nconst Q: &str = r#"pub fn"#;\n' "$held"
  printf '%b' "$held_store3c" | awk '{ printf "%s\r\n", $0 }' >"$work/repo/$held" || exit 2
  git -C "$work/repo" add -A || exit 2
  check carbon-held-file-with-cr-lf-line-ends PASS "$held looks a symbol up by name only in its fn symbol, by the plain literal names r1_full_flushes r1_plain_flushes r1_full_flush_path r1_fail_full_flushes" ''
  held_store3c_text="$held looks a symbol up by name only in its fn symbol, by the plain literal names r1_full_flushes r1_plain_flushes r1_full_flush_path r1_fail_full_flushes"
  held_case carbon-held-file-x04-fn-symbol-in-an-impl-of-a-trait FAIL "$held: fn symbol defined inside a brace of the code, at line 9" \
    "$held_head"'impl R1Look for R1Reach {\n'"$held_symbol"'}\n\nfn loaded() -> Option<()> {\n    let total = R1Reach::symbol(c"r1_full_flushes")?;\n    Some(())\n}\n' "$held"
  held_case carbon-held-file-fn-symbol-in-the-body-of-another-fn FAIL "$held: fn symbol defined inside a brace of the code, at line 9" \
    "$held_head"'fn outer() {\n'"$held_symbol"'}\n'"$held_calls" "$held"
  held_case carbon-held-file-braces-in-strings-chars-and-comments-above-fn-symbol PASS "$held_store3c_text" \
    "$held_head"'const OPEN: &str = "{{{";\nconst ONE: char = \0047{\0047;\n// {\n/* { */\n'"$held_symbol$held_calls" "$held"
  held_case carbon-held-file-x01-no-mangle-on-fn-symbol FAIL "$held: an attribute other than #[test] at line 8" \
    "$held_head"'#[no_mangle]\n'"$held_symbol$held_calls" "$held"
  held_case carbon-held-file-x02-export-name-on-fn-symbol FAIL "$held: an attribute other than #[test] at line 8" \
    "$held_head"'#[export_name = "r1_lookup"]\n'"$held_symbol$held_calls" "$held"
  held_case carbon-held-file-an-inner-attribute FAIL "$held: an attribute other than #[test] at line 21" \
    "$held_store3c"'\n#![allow(dead_code)]\n' "$held"
  held_case carbon-held-file-an-inner-attribute-named-test FAIL "$held: an attribute other than #[test] at line 21" \
    "$held_store3c"'\n#![test]\n' "$held"
  held_case carbon-held-file-cfg-test-on-an-item FAIL "$held: an attribute other than #[test] at line 21" \
    "$held_store3c"'\n#[cfg(test)]\nfn nothing() {}\n' "$held"
  held_case carbon-held-file-an-attribute-over-two-lines FAIL "$held: an attribute other than #[test] at line 21" \
    "$held_store3c"'\n#[cfg(\n    test\n)]\nfn nothing() {}\n' "$held"
  held_case carbon-held-file-r03-a-path-attribute-split-by-a-bare-carriage-return FAIL "$held: an attribute other than #[test] at line 21" \
    "$held_store3c"'\n#\r[path = "full_flush_more.rs"]\nfn nothing() {}\n' "$held"
  held_case carbon-held-file-the-spellings-of-test-that-pass PASS "$held_store3c_text" \
    "$held_store3c"'\n#[test]\nfn a() {}\n# [ test ]\nfn b() {}\n#[\ntest\n]\nfn c() {}\n#[test /* a comment */]\nfn d() {}\n' "$held"
  held_named_text="a tracked text file other than $held whose name ends in .rs holds the letters full_flush.rs"
  printf 'mod full_flush_again {\n    include!("hardening_tests/full_flush.rs");\n    fn other() -> Option<*mut libc::c_void> {\n        symbol(c"getpid")\n    }\n}\n' >"$work/repo/$held_parent" || exit 2
  held_case carbon-held-file-x03-the-parent-file-includes-the-held-file FAIL "$held_named_text" "$held_store3c" "$held"
  printf 'none\n' >"$work/repo/$held_parent" || exit 2
  printf '#[path = "../../halvo-capture/src/store/hardening_tests/full_flush.rs"]\nmod again;\n' >"$work/repo/crates/other/src/lib.rs" || exit 2
  held_case carbon-held-file-a-path-attribute-in-another-rust-file FAIL "$held_named_text" "$held_store3c" "$held"
  printf 'include!("Hardening_Tests/Full_Flush.RS");\n' >"$work/repo/crates/other/src/lib.rs" || exit 2
  held_case carbon-held-file-named-in-another-letter-case FAIL "$held_named_text" "$held_store3c" "$held"
  printf 'fn other() {}\n' >"$work/repo/crates/other/src/lib.rs" || exit 2
  printf 'The test of crates/halvo-capture/src/store/hardening_tests/full_flush.rs counts the flushes.\n' >"$work/repo/docs/notes.md" || exit 2
  held_case carbon-held-file-named-in-a-file-that-is-no-rust-file PASS "$held_store3c_text" "$held_store3c" "$held"
  printf 'none\n' >"$work/repo/docs/notes.md" || exit 2
  held_case carbon-held-file-a-c-string-in-a-const FAIL "$held: a C string literal that does not begin r1_ at line 21" \
    "$held_store3c"'\nconst NAME: &CStr = c"getpid";\n' "$held"
  held_case carbon-held-file-a-raw-c-string FAIL "$held: a C string literal that does not begin r1_ at line 21" \
    "$held_store3c"'\nconst NAME: &CStr = cr"getpid";\n' "$held"
  held_case carbon-held-file-a-raw-c-string-with-hashes FAIL "$held: a C string literal that does not begin r1_ at line 21" \
    "$held_store3c"'\nconst NAME: &CStr = cr#"getpid"#;\n' "$held"
  held_case carbon-held-file-the-same-letters-elsewhere PASS "$held looks a symbol up by name only in its fn symbol, by the plain literal names r1_full_flushes r1_plain_flushes r1_full_flush_path r1_fail_full_flushes" \
    "$held_store3c"'\n// a mod, an include, a macro_rules and a path in a comment\nconst WORDS: &str = "mod include macro_rules #[path]";\nfn module_path_of(path: &str) -> &str {\n    let model = path;\n    model\n}\nconst SOURCE: &str = r#"\n#include <fcntl.h>\n"#;\nfn file() -> &\0047static str {\n    "counter.c"\n}\nconst A: &CStr = cr#"r1_a"#;\nconst B: &CStr = cr"r1_b";\nconst QUOTE: char = \0047"\0047;\nconst RAW: &str = r#"printf("mod %%s", "include");"#;\n' "$held"
  for pair in 'pub=pub' 'pub-crate=pub(crate)' 'pub-super=pub(super)' 'pub-in-crate-store=pub(in crate::store)'; do
    form=${pair#*=}
    held_case "carbon-held-file-fn-symbol-made-${pair%%=*}" FAIL "$held: fn symbol is not defined once, as the two lines that this check holds (defined 0 times)" \
      "$held_head$form"' fn symbol(name: &CStr) -> Option<*mut libc::c_void> {\n    let found = unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) };\n    (!found.is_null()).then_some(found)\n}\n\n'"$held_calls" "$held"
  done
  printf 'none\n' >"$work/repo/$held" || exit 2
  git -C "$work/repo" add -A || exit 2
  mkdir -p "$work/rules-bin" || exit 2
  cat >"$work/rules-bin/cargo" <<'EOF'
#!/bin/sh
printf '%s %s\n' "${0##*/}" "$*" >>"$RULES_CALLS"
exit 99
EOF
  chmod +x "$work/rules-bin/cargo" || exit 2
  for tool in rustc clang swiftc xcrun; do
    cp "$work/rules-bin/cargo" "$work/rules-bin/$tool" || exit 2
  done
  check rules-form-starts-no-build-and-no-test PASS 'rules: only the parts of the check that read the index and the tree ran' '' rules
  twelve_aside=no
  if [ -f "$twelve" ]; then
    mv "$twelve" "$twelve.aside" || exit 2
    twelve_aside=yes
  fi
  cp "$work/repo/docs/baseline/ledger.d/macos-2b.txt" "$work/ledger-aside" || exit 2
  printf 'measured run=1 form=short result=fail\n' >"$work/repo/docs/baseline/ledger.d/macos-2b.txt" || exit 2
  git -C "$work/repo" add -A || exit 2
  check rules-form-reads-neither-the-ledger-nor-the-list-of-12-test PASS 'seam: the feature stall-seam stands in crates/halvo-capture/Cargo.toml alone' '' rules
  [ "$twelve_aside" = no ] || mv "$twelve.aside" "$twelve" || exit 2
  check the-measure-reads-the-stale-ledger FAIL 'names the measured run 1 without form=long and result=pass' ''
  cp "$work/ledger-aside" "$work/repo/docs/baseline/ledger.d/macos-2b.txt" || exit 2
  printf '%b' "$held_store3c" >"$work/repo/$held" || exit 2
  git -C "$work/repo" add -A || exit 2
  check rules-form-holds-the-file-as-store-3c-has-it PASS "$held looks a symbol up by name only in its fn symbol, by the plain literal names r1_full_flushes r1_plain_flushes r1_full_flush_path r1_fail_full_flushes" '' rules
  printf '%b' "$held_head$held_symbol"'fn loaded() -> Option<()> {\n    let pid = symbol(c"getpid")?;\n    Some(())\n}\n' >"$work/repo/$held" || exit 2
  git -C "$work/repo" add -A || exit 2
  check rules-form-refuses-a-lookup-by-another-name FAIL "$held: fn symbol gets a name that is no plain literal beginning r1_, or is named other than in such a call, at line 14" '' rules
  printf '%b' "$held_store3c" >"$work/repo/$held" || exit 2
  printf 'fn total() -> *mut libc::c_void {\n    unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"r1_full_flushes".as_ptr()) }\n}\n' >"$work/repo/crates/halvo-capture/src/store/lookup.rs" || exit 2
  git -C "$work/repo" add -A || exit 2
  check rules-form-refuses-a-lookup-in-another-file-of-the-store FAIL "${carbon_text}crates/halvo-capture/src/store/lookup.rs (" '' rules
  printf 'none\n' >"$work/repo/crates/halvo-capture/src/store/lookup.rs" || exit 2
  printf 'none\n' >"$work/repo/$held" || exit 2
  git -C "$work/repo" add -A || exit 2
  printf 'fn other() {}\n' >"$work/repo/crates/other/src/lib.rs" || exit 2
  printf 'struct Pair(\n    u8,\n    u8,\n);\n\nenum Kind {\n    Key,\n    Reading,\n}\n\nstruct TapSettings {\n    secure: fn(Session) -> Reading,\n}\n\n#[cfg(all(debug_assertions, feature = "stall-seam"))]\n#[derive(Debug)]\npub enum RawEntry {\n    Reading {\n        value: Reading,\n    },\n}\n\nfn read() {\n    let reading = (self.secure)(state.session);\n    let step = Reading::Off;\n}\n\nfn handed(reading: Reading, at: Stamps) -> (Reading, Stamps) {\n    (reading, at)\n}\n\n#[cfg(all(test, debug_assertions))]\nmod tests {\n    thread_local! {\n        static ANSWER: Cell<Reading> = const { Cell::new(Reading::Off) };\n    }\n    struct Kept {\n        kept: Reading,\n    }\n}\n\n#[cfg(test)]\nmod more_tests {\n    static LAST: Mutex<Option<Reading>> = Mutex::new(None);\n}\n' >"$work/repo/crates/halvo-capture/src/macos/input.rs" || exit 2
  git -C "$work/repo" add crates || exit 2
  write_measured_ledger
  check reading-in-a-variant-a-reader-type-the-seam-and-a-test PASS 'reading: no field, payload, alias, renaming use, static, thread_local, macro_rules body, generic argument or annotated let' ''
  printf 'use super::Kept;\n\nstruct HoldState {\n    kept: Option<Kept>,\n}\n' >"$work/repo/crates/halvo-capture/src/macos/input.rs" || exit 2
  git -C "$work/repo" add crates || exit 2
  write_measured_ledger
  check reading-limit-an-alias-made-in-another-file PASS 'limits: a cache whose type is inferred, an alias, a wrapper or a re-export made in another file' ''
  reading_text='can keep a reading of the secure-input state between two events'
  for form in 'field:struct HoldState {\n    keys: HeldKeys<HeldKey>,\n    kept: Option<(Reading, Stamps)>,\n}\n' \
    'closure:fn tap_callback() {\n    let kept: std::cell::Cell<Option<Reading>> = std::cell::Cell::new(None);\n}\n' \
    'newtype:struct LastReading(Option<Reading>);\n' \
    'alias:type Kept = Option<Reading>;\n' \
    'use-as:use crate::secure_input_macos::Reading as State;\n\nstruct HoldState {\n    kept: Option<State>,\n}\n' \
    'enum-wrapper:enum Kept {\n    One(Reading),\n}\n' \
    'static:static LAST: Mutex<Option<Reading>> = Mutex::new(None);\n' \
    'static-after-the-tests:#[cfg(test)]\nmod tests {\n    fn x() {}\n}\n\nstatic LAST_READING: Mutex<Option<Reading>> = Mutex::new(None);\n' \
    'module-under-cfg-any-test:#[cfg(any(test, target_os = "macos"))]\nmod live {\n    struct HoldState {\n        kept: Option<Reading>,\n    }\n}\n' \
    'module-under-cfg-not-test:#[cfg(not(test))]\nmod kept {\n    static LAST: Mutex<Option<Reading>> = Mutex::new(None);\n}\n' \
    'pub-in-path:pub(in crate::macos) struct HoldState {\n    kept: Reading,\n}\n' \
    'attribute-on-the-item-line:#[derive(Default)] struct HoldState {\n    kept: Reading,\n}\n' \
    'brace-on-the-next-line:struct HoldState\n{\n    kept: Reading,\n}\n' \
    'tuple-struct-over-lines:struct LastReading(\n    Reading,\n);\n' \
    'split-field:struct HoldState {\n    kept: Cell<\n        Option<\n            Reading,\n        >,\n    >,\n}\n' \
    'generic-argument-of-a-parameter:struct Kept<R> {\n    last: Option<R>,\n}\n\nfn on_event(kept: &mut Kept<Reading>, reading: Reading) {\n    kept.last = Some(reading);\n}\n' \
    'generic-argument-of-a-return:struct TapContext<R> {\n    kept: std::cell::Cell<Option<R>>,\n}\n\nfn tap_context() -> TapContext<Reading> {\n    TapContext { kept: std::cell::Cell::new(None) }\n}\n' \
    'use-as-over-lines:use crate::secure_input_macos::{\n    Reading as State,\n};\n\nstruct HoldState {\n    kept: Option<State>,\n}\n' \
    'thread-local-on-one-line:thread_local!(static LAST: Reading = const { Reading::Off });\n' \
    'macro-rules-body:macro_rules! kept {\n    () => {\n        Reading\n    };\n}\n\nstruct HoldState {\n    kept: kept!(),\n}\n'; do
    printf '%b' "${form#*:}" >"$work/repo/crates/halvo-capture/src/macos/input.rs" || exit 2
    git -C "$work/repo" add crates || exit 2
    check "reading-kept-by-a-${form%%:*}" FAIL "crates/halvo-capture/src/macos/input.rs $reading_text" ''
  done
  printf 'fn x() {}\n' >"$work/repo/crates/halvo-capture/src/macos/input.rs" || exit 2
  printf '%s\npub struct HeldKeys<T> {\n    keys: VecDeque<T>,\n    kept: Option<Reading>,\n}\n' "$clock_lines" >"$work/repo/crates/halvo-capture/src/secure_input_macos_hold.rs" || exit 2
  git -C "$work/repo" add crates || exit 2
  check reading-kept-in-the-hold-file FAIL "crates/halvo-capture/src/secure_input_macos_hold.rs $reading_text" ''
  for form in 'pub-crate:pub struct Monotonic(pub(crate) u64);\npub struct Uptime(u64);\n' \
    'pub:pub struct Monotonic(u64);\npub struct Uptime(pub u64);\n' \
    'monotonic-defined-twice:#[cfg(test)]\npub struct Monotonic(u64);\n#[cfg(not(test))]\npub struct Monotonic(pub u64);\npub struct Uptime(u64);\n' \
    'uptime-defined-twice:pub struct Monotonic(u64);\n#[cfg(test)]\npub struct Uptime(u64);\n#[cfg(not(test))]\npub struct Uptime(pub u64);\n'; do
    printf '%b' "${form#*:}" >"$work/repo/crates/halvo-capture/src/secure_input_macos_hold.rs" || exit 2
    git -C "$work/repo" add crates || exit 2
    check "clock-${form%%:*}" FAIL 'is not defined once, as exactly' ''
  done
  for path in crates/halvo-capture/src/macos/input.rs crates/halvo-capture/src/secure_input_macos.rs; do
    printf '%s\n' "$path" >"$work/repo/$path" || exit 2
  done
  printf '%s\n' "$clock_lines" >"$work/repo/crates/halvo-capture/src/secure_input_macos_hold.rs" || exit 2
  git -C "$work/repo" add crates || exit 2
  write_measured_ledger
  printf 'changed\n' >>"$work/repo/crates/halvo-capture/src/secure_input_macos_hold.rs" || exit 2
  git -C "$work/repo" add crates || exit 2
  check ledger-blob-changed FAIL 'ran on a blob of crates/halvo-capture/src/secure_input_macos_hold.rs other than the head'"'"'s' ''
  check ledger-fix-text FAIL 'run the long form (HALVO_STALL_FORM=long) on a scratch push of this head, then record that run with form=long, result=pass' ''
  write_measured_ledger
  grep -v '^measured run=' "$work/repo/docs/baseline/ledger.d/macos-2b.txt" >"$work/ledger-without-run" || exit 2
  cp "$work/ledger-without-run" "$work/repo/docs/baseline/ledger.d/macos-2b.txt" || exit 2
  check ledger-names-no-run FAIL 'names no measured run' ''
  write_measured_ledger
  grep -v 'path=crates/halvo-capture/tests/macos_stall_fixture/fixture.m ' "$work/repo/docs/baseline/ledger.d/macos-2b.txt" >"$work/ledger-four" || exit 2
  cp "$work/ledger-four" "$work/repo/docs/baseline/ledger.d/macos-2b.txt" || exit 2
  check ledger-four-files FAIL 'records the blobs of files other than the eight of the hold' ''
  write_measured_ledger
  printf 'measured run=2 form=long result=pass commit=0 figures=none\n' >>"$work/repo/docs/baseline/ledger.d/macos-2b.txt" || exit 2
  check ledger-two-runs FAIL 'names more than one measured run (1 2)' ''
  ledger_run_line='measured run=1 form=short result=pass gap_ms=190 commit=0 figures=none'
  write_measured_ledger
  check ledger-run-of-the-short-form FAIL 'names the measured run 1 without form=long and result=pass' ''
  ledger_run_line='measured run=1 form=long result=fail gap_ms=190 commit=0 figures=none'
  write_measured_ledger
  check ledger-run-that-failed FAIL 'names the measured run 1 without form=long and result=pass' ''
  ledger_run_line=
  write_measured_ledger
  printf 'changed\n' >>"$work/repo/scripts/r1/ci.d/51-macos-stall.sh" || exit 2
  git -C "$work/repo" add scripts || exit 2
  check ledger-ninth-file-changed FAIL 'ran on a blob of scripts/r1/ci.d/51-macos-stall.sh other than the head' ''
  write_measured_ledger
  seam_text='the stall-seam lines of crates/halvo-capture/Cargo.toml differ from those that the measured run 1 ran on'
  manifest_case ledger-seam-line-changed FAIL "$seam_text" \
    '[package]\nname = "halvo-capture"\n\n[features]\nstall-seam = ["other"]\n\n[dependencies]\n\n[dev-dependencies]\nhalvo-capture = { path = ".", features = ["stall-seam"] }\n'
  manifest_case ledger-seam-line-moved FAIL "$seam_text" \
    '[package]\nname = "halvo-capture"\n\n[features]\nstall-seam = []\n\n[dependencies]\nhalvo-capture = { path = ".", features = ["stall-seam"] }\n\n[dev-dependencies]\n'
  manifest_case ledger-seam-line-dropped FAIL "$seam_text" \
    '[package]\nname = "halvo-capture"\n\n[features]\nstall-seam = []\n\n[dependencies]\n\n[dev-dependencies]\nhalvo-capture = { path = "." }\n'
  manifest_case ledger-seam-line-gains-win-3s-feature PASS 'ledger: the measured run 1 of the long form ran on' \
    '[package]\nname = "halvo-capture"\n\n[features]\nstall-seam = []\ntest-seam = []\n\n[dependencies]\n\n[dev-dependencies]\nhalvo-capture = { path = ".", features = ["stall-seam", "test-seam"] }\n'
  manifest_case ledger-seam-dependency-on-another-path FAIL "$seam_text" \
    '[package]\nname = "halvo-capture"\n\n[features]\nstall-seam = []\n\n[dependencies]\n\n[dev-dependencies]\nhalvo-capture = { path = "../other", features = ["stall-seam", "test-seam"] }\n'
  manifest_case ledger-unrelated-manifest-line PASS 'ledger: the measured run 1 of the long form ran on' \
    '[package]\nname = "halvo-capture"\nversion = "0.2.0"\n\n[features]\nstall-seam = []\n\n[dependencies]\nlibc = "0.2"\n\n[dev-dependencies]\nhalvo-capture = { path = ".", features = ["stall-seam"] }\n'
  printf '%b' "$seam_manifest" >"$work/repo/crates/halvo-capture/Cargo.toml" || exit 2
  git -C "$work/repo" add crates || exit 2
  write_measured_ledger
  grep -v '^measured seam ' "$work/repo/docs/baseline/ledger.d/macos-2b.txt" >"$work/ledger-without-seam" || exit 2
  cp "$work/ledger-without-seam" "$work/repo/docs/baseline/ledger.d/macos-2b.txt" || exit 2
  check ledger-names-no-seam-line FAIL 'names no stall-seam line of crates/halvo-capture/Cargo.toml' ''
  ledger_run_line='measured run=1 form=long result=pass gap_ms=190 mended_rounds_held=60 phases_not_driven=1 commit=0 figures=none'
  write_measured_ledger
  check ledger-run-with-a-phase-not-driven FAIL 'names the measured run 1 without phases_not_driven=0' ''
  ledger_run_line='measured run=1 form=long result=pass gap_ms=190 mended_rounds_held=60 commit=0 figures=none'
  write_measured_ledger
  check ledger-run-without-the-phases-not-driven FAIL 'names the measured run 1 without phases_not_driven=0' ''
  ledger_run_line=
  write_measured_ledger
  d2_note=$(
    unset MS_NOTES_FILE
    notes | sed -n 's/^short_window_a_the_click_round_as_it_was_at_wait_2000_ms //p'
  )
  case $d2_note in
    *"Its mechanism has been seen on the hosted machine at the test's gap in three of the four recorded long-form runs of the measured line: 3 of 124 rounds in run 43604085133, 2 in run 43627916230, 2 in run 43655810029, and none in run 43741572763."*)
      printf 'case %s/d2-note-sentence want=PASS got=PASS\n' "$name"
      ;;
    *)
      printf 'case %s/d2-note-sentence want=PASS got=another-sentence\n    | %s\n' "$name" "$d2_note"
      wrong="$wrong d2-note-sentence"
      ;;
  esac
  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (cases that gave another verdict:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  --rules) rules ;;
  '') measure ;;
  *)
    printf 'usage: sh %s [--self-test | --rules]\n  --rules runs only the parts that read the index and the tree (seam, carbon, reading, clocks): it builds nothing, runs no test, starts no process of the product and reads neither the ledger nor the list of 12-test\n' "$0" >&2
    exit 64
    ;;
esac
