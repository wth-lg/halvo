#!/bin/sh
# jobs: windows
# rule: the literal text of every print call of the Windows functional tests (tests/windows_functional.rs and the modules stall and browser of crates/halvo-capture) and of the print tool (tests/windows_prints.rs) holds numbers, flags and fixed names alone by the closed pattern of scripts/r1/windows-prints-rule.sh, so that no literal key, window title or text of a field is printed, and 50-windows-functional reads the values that the calls fill in at run time through the same rule
# lane: free
set -u
name=53-windows-prints
rule_file=scripts/r1/windows-prints-rule.sh
required='crates/halvo-capture/tests/windows_functional.rs
crates/halvo-capture/tests/windows_prints.rs
crates/halvo-capture/tests/stall/mod.rs
crates/halvo-capture/tests/browser/mod.rs'
LC_ALL=C
export LC_ALL

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

fail_self() {
  printf 'FAIL self-test:%s (%s)\n' "$name" "$1"
  exit 1
}

load_rule() {
  cd "$CI_D_REPO" || exit 2
  [ -f "$rule_file" ] && [ -r "$rule_file" ] || "$1" "$rule_file cannot be read"
  . ./scripts/r1/windows-prints-rule.sh
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || "$1" "mktemp failed"
  wpr_vocabulary "$work/vocab" || "$1" "the vocabulary cannot be made from the crate at HEAD"
}

head_reader() {
  git show "HEAD:$1"
}

plain_reader() {
  cat "$work/plain/$1"
}

scan() {
  : >"$work/literals" || return 2
  while IFS= read -r file; do
    "$2" "$file" >"$work/source" || return 2
    wpr_print_literals "$file" <"$work/source" >>"$work/literals" || return 2
  done <"$1"
  cut -f 1 "$work/literals" >"$work/places" || return 2
  cut -f 2- "$work/literals" >"$work/texts" || return 2
  lines=$(grep -c '' "$work/texts") || lines=0
  wpr_check_lines "$work/vocab" "$name" <"$work/texts" >"$work/refused"
  status=$?
  refused=$(grep -c '' "$work/refused") || refused=0
  case $status in
    0) [ "$refused" -eq 0 ] || return 2 ;;
    1) [ "$refused" -gt 0 ] || return 2 ;;
    *) return 2 ;;
  esac
  while IFS= read -r line; do
    index=${line#* line=}
    index=${index%% *}
    place=$(sed -n "${index}p" "$work/places") || return 2
    printf 'refused %s %s\n' "$place" "${line#* line=* }"
  done <"$work/refused"
  [ "$refused" -eq 0 ]
}

measure() {
  load_rule fail
  git ls-tree -r --name-only HEAD -- crates/halvo-capture/tests >"$work/tree" || fail "git cannot list the tests of the crate at HEAD"
  grep -E '^crates/halvo-capture/tests/(windows_functional\.rs|windows_prints\.rs|(stall|browser)/.*\.rs)$' "$work/tree" >"$work/files" ||
    fail "HEAD holds none of the files that this check reads"
  while IFS= read -r file; do
    grep -q -x -F "$file" "$work/files" || fail "HEAD holds no $file"
  done <<EOF
$required
EOF
  scan "$work/files" head_reader
  scanned=$?
  [ "$scanned" -le 1 ] || fail "the print calls of the tests cannot be read at HEAD"
  while IFS= read -r file; do
    found=$(grep -c -F "$file:" "$work/places") || found=0
    [ "$found" -gt 0 ] || fail "no print call was read in $file"
  done <<EOF
$required
EOF
  files=$(grep -c '' "$work/files") || files=0
  words=$(grep -c '' "$work/vocab") || words=0
  printf 'prints: files=%s literal_lines=%s vocabulary_words=%s refused=%s\n' "$files" "$lines" "$words" "$refused"
  [ "$scanned" -eq 0 ] ||
    fail "$refused literal lines of print calls break the rule; each is named above by its file and line, without its text: print a count, a flag or a fixed name in its place"
  printf 'PASS %s\n' "$name"
}

tree_case() {
  scan "$work/plain/list" plain_reader >"$work/$1.out"
  got=$?
  [ "$got" -eq "$2" ] || fail_self "the fixture $1 scanned with exit $got, not $2"
  named=$(cat "$work/$1.out") || fail_self "the answer of $1 cannot be read"
  [ "$named" = "$3" ] || fail_self "the fixture $1 named another place than the one that it plants"
  [ "$got" -eq 0 ] || rejected=$((rejected + 1))
  printf 'case %s/%s want=exit-%s got=exit-%s (lines read %s, lines named %s)\n' "$name" "$1" "$2" "$got" "$lines" "$refused"
}

heads_log_lines() {
  cat <<'EOF'
running 1 test
test a_dropped_keyboard_source_stops_capturing ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 86 filtered out; finished in 6.01s
test c9_peak_working_set_of_sources_1_to_4_for_60_seconds has been running for over 60 seconds
thread 'tf-c5-watch' (4132) panicked at crates\halvo-capture\tests\windows_functional.rs:5467:13:
KNOWN-DEFECT W3-5-A2
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
TF browser-swap-edge browser=edge phase=show-control round=2 part=shown printed_only=1 counted=1 letters_sent=3 landed_text=1 landed_password=2 tabs_into_password=0 tabs_into_text=0 button_downs=0 focus_after=password letters_decided=3 letters_recorded=0 tabs_recorded=0 other_recorded=0 first_letter_rule=Some(MarkAfterTheKey) first_tab_rule=None keystroke_events=0 sink_differs_from_the_seam=0 landings=tpp landed_at_ms=1262,1305,1344 freed_at_ms=-1 password_letters_recorded_by_key=0 uncounted_because=none
TF fixture-chrome browser=chrome phase=E page_window_found=true page_window_foreground=true page_plain=40 page_password=20 page_masked=0 keystroke_events=1 gate=Some(Clear)
TF c9 memory phase=t+5s working_set_bytes=29646848 peak_working_set_bytes=29646848 private_bytes=6701056 peak_private_bytes=6709248
TF fixture-native-f fixture_process_started=true fixture_window_foreground=1 method=1
TF c8 affinity_before=Ok(0) set_display_affinity=ok affinity_after=Ok(17) wda_excludefromcapture=17 set_display_affinity_null_window=error(Invalid window handle. (0x80070578))
TF dropped env session_id_ok=true session_id=2 window_station_is_winsta0=true desktop_is_default=true input_desktop_is_default=true screen=1024x768 monitors=1 remote_session=0 foreground_window_present=true foreground_app_known=true
TF browser-after-start-chrome browser=chrome first_typing after_the_letters handler_counts_before_the_read=5/5 uia_of_the_focused_element is_password=false legacy_state=0x100004 protected=false control_type=50004 localized_control_type=edit class_name=- framework=Chrome aria_role=textbox aria_properties=expanded=false;multiline=false;multiselectable=false;required=false;hasactions=false automation_id=plain item_type=- handler_counts_1_s_after_the_read=5/5
TF x14 before_anchor sample foreground_class=CASCADIA_HOSTING_WINDOW_CLASS foreground_image=WindowsTerminal.exe child_windows=3 children_of_other_processes=none anchor_foreground=true anchor_method=attach-thread-input allow_set_foreground_any=false
child: TF child first_window_foreground=true method=SetForegroundWindow
TF stall-click-serves-150 phase=click_with_no_wait round=0 counted=1 letters_sent=1 letters_landed=1 letters_recorded=0 tabs_recorded=0 other_recorded=0 recorded_by_the_seam=0 keystroke_events=0 letters_withheld=1 letters_late=0 tabs_withheld=0 lost_at_stop=0 rule_of_the_first_letter=4 rule_of_the_first_tab=-1 fg_at_send=1 focus_at_send=1 plain_len_at_send=0 password_len_at_send=0 plain_len_at_wake=0 password_len_at_wake=0 plain_len_after=0 password_len_after=1 letters_plain=0 letters_password=1 letters_second=0 letters_other=0 tabs_plain=0 tabs_password=0 tabs_second=0 fg_after=1 reactivated=0 aimed=1 late_events_moved_by=0 keys_withheld_before=0 keys_withheld_after=1 sink_differs_from_the_seam=0 uncounted_because=none
TF dropped uninstall_key_before per_user_uninstall_key=native exists=false subkeys=0 values=0 first_subkeys=[]
TF browser-renamed-firefox browser=renamed-firefox field=plain foreground=true focus_before=plain gate_while_focused=Some(Unknown) counted=1 letters_sent=5 landed_text=0 landed_password=0 tabs_into_password=0 tabs_into_text=0 button_downs=0 focus_after=plain letters_decided=5 letters_recorded=0 tabs_recorded=0 other_recorded=0 first_letter_rule=Some(StateAtTheKey) first_tab_rule=None keystroke_events=0 sink_differs_from_the_seam=0 landings=- landed_at_ms= freed_at_ms=-1 password_letters_recorded_by_key=0 uncounted_because=none framework=Gecko window_class=MozillaWindowClass file=renamed-browser.exe kind=Other is_password=0 protected=0 pid_read=1 file_read=ok
fault code=0xc0000005 module=windows_functional-38082c0e82ee5731.exe offset=0x46f40
left_out the rounds of every other stall phase but one, of phase E but one, of the fixtures' phases E and F but one
EOF
}

source_case() {
  case_name=$1
  want=$2
  printf '%s\n' "$3" >"$work/$case_name.rs" || fail_self "the fixture $case_name cannot be written"
  wpr_print_literals "fixture.rs" <"$work/$case_name.rs" >"$work/$case_name.tsv" || fail_self "the literals of $case_name cannot be read"
  read_count=$(grep -c '' "$work/$case_name.tsv") || read_count=0
  cut -f 2- "$work/$case_name.tsv" >"$work/$case_name.txt" || fail_self "the texts of $case_name cannot be written"
  wpr_check_lines "$work/vocab" "$case_name" <"$work/$case_name.txt" >"$work/$case_name.out"
  got=$?
  verdict "$case_name" "$want" "$got" "$read_count"
}

plant() {
  printf '%s' "$2" >"$work/$1.log" || fail_self "the fixture $1 cannot be written"
}

log_case() {
  case_name=$1
  wpr_check_log "$case_name" "$work/$case_name.log" "$work/vocab" >"$work/$case_name.out"
  got=$?
  read_count=$(grep -c '' "$work/$case_name.log") || read_count=0
  verdict "$case_name" "$2" "$got" "$read_count"
  named=$(cat "$work/$case_name.out") || fail_self "the answer of $case_name cannot be read"
  [ "$named" = "${3:-}" ] || fail_self "the fixture $case_name named another line than the one that it plants"
}

verdict() {
  lines_named=$(grep -c '' "$work/$1.out") || lines_named=0
  case $2 in
    refused)
      [ "$3" -eq 1 ] && [ "$lines_named" -gt 0 ] ||
        fail_self "the fixture $1 was not refused: exit $3, $lines_named lines named"
      rejected=$((rejected + 1))
      printf 'case %s/%s want=refused got=refused (lines read %s, lines named %s)\n' "$name" "$1" "$4" "$lines_named"
      ;;
    unreadable)
      [ "$3" -eq 2 ] && [ "$lines_named" -eq 0 ] ||
        fail_self "the fixture $1 was read as a log: exit $3, $lines_named lines named"
      rejected=$((rejected + 1))
      printf 'case %s/%s want=unreadable got=unreadable\n' "$name" "$1"
      ;;
    passed)
      [ "$3" -eq 0 ] && [ "$lines_named" -eq 0 ] ||
        fail_self "the fixture $1 did not pass as it should: exit $3, $lines_named lines named"
      printf 'case %s/%s want=passed got=passed (lines read %s)\n' "$name" "$1" "$4"
      ;;
  esac
}

self_test() {
  load_rule fail_self
  rejected=0
  source_case src-typed-key refused 'fn c1() { println!("TF c1 key=q vk={vk}"); }'
  source_case src-window-title refused 'fn c4() { say("c4", format!("title=Untitled - Notepad bytes={n}")); }'
  source_case src-field-text refused 'fn s() { panic!("the plain field holds hunter2 after the letters"); }'
  source_case src-title-outside-ascii refused 'fn c4() { println!("TF c4 bytes={n} title=\u{65e5}\u{672c}"); }'
  source_case src-line-built-before-its-print refused 'fn x() { let evidence = format!("TF x14 title=Untitled"); println!("{evidence}"); }'
  source_case src-expect-message refused 'fn x() { open().expect("the window Untitled - Notepad"); }'
  source_case src-assert-message refused 'fn x() { assert_eq!(text, "hunter2"); }'
  source_case src-print-tool refused 'fn q() { say(format!("q3 target={name} typed=hunter2")); }'
  source_case src-heads-lines passed 'fn c1() {
    println!("TF c1 keystroke_events={n} ok={}", flag(ok));
    say(test, format!("phase=E rounds={r} counted_rounds={c} landings={l}"));
    say("c9", memory_line(&format!("t+{s}s"), &now));
    say(test, format!("first_focus={:?} reads=[{}] code=0x{:x}", f, r, c));
    panic!("INCONCLUSIVE: the fixture process did not answer {kind}");
    println!("left_out the rounds of the fixtures'"'"' phases E and F but one");
}'
  verdict_min=5
  [ "$read_count" -ge "$verdict_min" ] || fail_self "the fixture src-heads-lines read $read_count lines, fewer than $verdict_min"
  source_case src-not-a-print passed 'fn t() {
    let title = format!("Untitled - Notepad");
    // println!("TF c4 title=Untitled");
    let text = "hunter2";
}'
  [ "$read_count" -eq 0 ] || fail_self "the fixture src-not-a-print read $read_count lines of a text that no print call holds"
  cr=$(printf '\r')
  plant log-typed-key "TF c1 keystroke_events=1$cr
TF c1 key=q vk=81$cr"
  log_case log-typed-key refused 'prints-rule log-typed-key line=2 length=17 token=3'
  plant log-window-title "TF c4 source=window_title start=ok$cr
TF c4 title=Untitled - Notepad bytes=18$cr"
  log_case log-window-title refused 'prints-rule log-window-title line=2 length=39 token=3'
  plant log-field-text "TF stall plain_len=7 plain_text=hunter2$cr"
  log_case log-field-text refused 'prints-rule log-field-text line=1 length=39 token=4'
  printf 'TF c4 bytes=6 title=\346\227\245\346\234\254\r' >"$work/log-title-outside-ascii.log" ||
    fail_self "the fixture log-title-outside-ascii cannot be written"
  log_case log-title-outside-ascii refused 'prints-rule log-title-outside-ascii line=1 length=26 token=4'
  plant log-typed-keys "TF c1 key=s$cr
TF c1 key=t$cr
TF c1 keys a and q$cr
TF c1 key=q1$cr
TF c1 key=e5$cr
TF browser-stall-chrome landings=pq$cr
TF browser-stall-chrome phases E and F landings=ppt$cr"
  log_case log-typed-keys refused 'prints-rule log-typed-keys line=1 length=11 token=3
prints-rule log-typed-keys line=2 length=11 token=3
prints-rule log-typed-keys line=3 length=18 token=6
prints-rule log-typed-keys line=4 length=12 token=3
prints-rule log-typed-keys line=5 length=12 token=3
prints-rule log-typed-keys line=6 length=35 token=3'
  plant log-field-text-of-a-literal "TF stall plain_len=5 plain_text=aaaaa$cr
TF browser-stall-chrome field=secret$cr"
  log_case log-field-text-of-a-literal refused 'prints-rule log-field-text-of-a-literal line=1 length=37 token=4
prints-rule log-field-text-of-a-literal line=2 length=36 token=3'
  heads_log_lines >"$work/heads.txt" || fail_self "the fixture log-heads-lines cannot be written"
  awk '{ printf "%s\r\n", $0 }' "$work/heads.txt" >"$work/log-heads-lines.log" ||
    fail_self "the fixture log-heads-lines cannot be written"
  log_case log-heads-lines passed
  [ "$read_count" -ge 21 ] || fail_self "the fixture log-heads-lines read $read_count lines, fewer than 21"
  wpr_check_log log-missing "$work/no-such.log" "$work/vocab" >"$work/log-missing.out"
  verdict log-missing unreadable "$?" 0
  : >"$work/empty-vocab"
  printf 'TF c1 ok=true\n' >"$work/one.log" || fail_self "the fixture log-without-a-vocabulary cannot be written"
  wpr_check_log log-without-a-vocabulary "$work/one.log" "$work/empty-vocab" >"$work/log-without-a-vocabulary.out"
  verdict log-without-a-vocabulary unreadable "$?" 0
  mkdir "$work/plain" || fail_self "the fixture tree cannot be made"
  printf 'a.rs\nb.rs\n' >"$work/plain/list" || fail_self "the fixture tree cannot be written"
  printf 'fn a() {\n    println!("TF c1 ok={}", flag(ok));\n}\n' >"$work/plain/a.rs" || fail_self "the fixture tree cannot be written"
  printf 'fn b() {\n    say("c4", "start=ok");\n    say("c4", format!("bytes={n} title=Untitled - Notepad"));\n}\n' >"$work/plain/b.rs" ||
    fail_self "the fixture tree cannot be written"
  tree_case tree-with-a-planted-title 1 'refused b.rs:3 length=32 token=2'
  printf 'fn b() {\n    say("c4", "start=ok");\n}\n' >"$work/plain/b.rs" || fail_self "the fixture tree cannot be written"
  tree_case tree-without-a-planted-title 0 ''
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  '') measure ;;
  --self-test) self_test ;;
  *)
    printf 'usage: sh %s [--self-test]\n' "$0"
    exit 64
    ;;
esac
