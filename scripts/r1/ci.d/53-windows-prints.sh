#!/bin/sh
# jobs: windows
# rule: the literal text of every print call and the format string of every format!, write! and writeln! of the Windows functional tests (tests/windows_functional.rs and the modules stall and browser of crates/halvo-capture) and of the print tool (tests/windows_prints.rs) holds numbers, flags and fixed names alone by the closed forms of scripts/r1/windows-prints-rule.sh (sh 53-windows-prints.sh --help prints them), save the format strings that build a URL, a query, a registry path or a command and that the list unprinted_format_strings names, so that no literal key, window title, text of a field, address or path is printed, and 50-windows-functional reads the values that the calls fill in at run time through the same rule
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

unprinted_format_strings() {
  cat <<'EOF'
crates/halvo-capture/tests/browser/mod.rs	file:///{}	the URL of the fixture page that a page test opens in the browser
crates/halvo-capture/tests/browser/mod.rs	{}?phase={}&hold={}&run={}	the query of that URL
crates/halvo-capture/tests/windows_functional.rs	file:///{}	the URL of the fixture page that a page test opens in the browser
crates/halvo-capture/tests/windows_functional.rs	{}?phase={}&run={}	the query of that URL
crates/halvo-capture/tests/windows_functional.rs	{}\{}	the path of a registry key under the Uninstall key that a test opens
crates/halvo-capture/tests/windows_prints.rs	file:///{}	the URL of the fixture page that a print opens in the browser
crates/halvo-capture/tests/windows_prints.rs	{}?{}&run={}	the query of that URL
crates/halvo-capture/tests/windows_prints.rs	field=plain&swap=1&hold={}	the query of the swap page that a print opens
crates/halvo-capture/tests/windows_prints.rs	(Get-Process {} -ErrorAction SilentlyContinue | Measure-Object -Property CPU -Sum).Sum	the PowerShell command whose number a print reads
crates/halvo-capture/tests/windows_prints.rs	(Get-Process {} -ErrorAction SilentlyContinue | Measure-Object -Property WorkingSet64 -Sum).Sum	the PowerShell command whose number a print reads
EOF
}

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
  wpr_check_lines "$work/vocab" "$name" source <"$work/texts" >"$work/refused-all"
  status=$?
  case $status in
    0 | 1) ;;
    *) return 2 ;;
  esac
  "$3" >"$work/unprinted" || return 2
  : >"$work/refused" || return 2
  : >"$work/exempt-used" || return 2
  while IFS= read -r line; do
    index=${line#* line=}
    index=${index%% *}
    place=$(sed -n "${index}p" "$work/places") || return 2
    text=$(sed -n "${index}p" "$work/texts" | awk '{ gsub(/\003/, "{}"); print }') || return 2
    exempt=$(WPR_FILE=${place%:*} WPR_TEXT=$text awk -F '\t' '$1 == ENVIRON["WPR_FILE"] && $2 == ENVIRON["WPR_TEXT"] { print NR; exit }' "$work/unprinted") || return 2
    if [ -n "$exempt" ]; then
      printf '%s\n' "$exempt" >>"$work/exempt-used" || return 2
      continue
    fi
    printf '%s\n' "$line" >>"$work/refused" || return 2
    printf 'refused %s %s\n' "$place" "${line#* line=* }"
  done <"$work/refused-all"
  refused=$(grep -c '' "$work/refused") || refused=0
  exempted=$(sort -u "$work/exempt-used" | grep -c '') || exempted=0
  listed=$(grep -c '' "$work/unprinted") || listed=0
  [ "$status" -eq 1 ] || [ "$refused" -eq 0 ] || return 2
  [ "$refused" -eq 0 ]
}

judge_tree() {
  scan "$1" "$2" "$3"
  scanned=$?
  [ "$scanned" -le 1 ] || fail "the print calls of the tests cannot be read"
  while IFS= read -r file; do
    found=$(awk -v file="$file:" 'index($0, file) == 1 { n++ } END { print n + 0 }' "$work/places") || found=0
    [ "$found" -gt 0 ] || fail "no print call was read in $file"
  done <"$4"
  files=$(grep -c '' "$1") || files=0
  words=$(grep -c '' "$work/vocab") || words=0
  printf 'prints: files=%s literal_lines=%s vocabulary_words=%s refused=%s unprinted_format_strings=%s of %s\n' "$files" "$lines" "$words" "$refused" "$exempted" "$listed"
  [ "$exempted" -eq "$listed" ] ||
    fail "$((listed - exempted)) entries of the list of unprinted format strings match no refused literal: take each such entry out of the list"
  [ "$scanned" -eq 0 ] ||
    fail "$refused literal lines of print calls break the rule; each is named above by its file and line, without its text: print a count, a flag or a fixed name in its place"
  printf 'PASS %s\n' "$name"
}

measure() {
  load_rule fail
  git ls-tree -r --name-only HEAD -- crates/halvo-capture/tests >"$work/tree" || fail "git cannot list the tests of the crate at HEAD"
  grep -E '^crates/halvo-capture/tests/(windows_functional\.rs|windows_prints\.rs|(stall|browser)/.*\.rs)$' "$work/tree" >"$work/files" ||
    fail "HEAD holds none of the files that this check reads"
  printf '%s\n' "$required" >"$work/required" || fail "the list of the required files cannot be written"
  while IFS= read -r file; do
    grep -q -x -F "$file" "$work/files" || fail "HEAD holds no $file"
  done <"$work/required"
  judge_tree "$work/files" head_reader unprinted_format_strings "$work/required"
}

judge_case() {
  (judge_tree "$work/plain/list" plain_reader plain_unprinted "$work/plain/required") >"$work/$1.out"
  got=$?
  [ "$got" -eq "$2" ] || fail_self "the fixture $1 judged the tree with exit $got, not $2"
  last=$(tail -n 1 "$work/$1.out") || fail_self "the answer of $1 cannot be read"
  [ "$last" = "$3" ] || fail_self "the fixture $1 ended with another verdict than $3"
  if [ -n "${4:-}" ]; then
    grep -q -x -F "$4" "$work/$1.out" || fail_self "the fixture $1 did not name the planted place"
  fi
  [ "$got" -eq 0 ] || rejected=$((rejected + 1))
  printf 'case %s/%s want=exit-%s got=exit-%s\n' "$name" "$1" "$2" "$got"
}

plain_unprinted() {
  cat "$work/plain/unprinted"
}

tree_case() {
  scan "$work/plain/list" plain_reader plain_unprinted >"$work/$1.out"
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
KNOWN-DEFECT KNOWN_ID
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
TF c1 key= first_subkeys=[] class_name=- handler_counts=2/3 marks=[read@12->14:Some(Clear):Some(Same):kind=Chrome/is_password=0,latch-input@9:None:None:none]
TF x14 children_of_other_processes=SystemSettings.exe:Windows.UI.Core.CoreWindow test=stall::tab_and_letters
EOF
}

source_case() {
  case_name=$1
  want=$2
  printf '%s\n' "$3" >"$work/$case_name.rs" || fail_self "the fixture $case_name cannot be written"
  wpr_print_literals "fixture.rs" <"$work/$case_name.rs" >"$work/$case_name.tsv" || fail_self "the literals of $case_name cannot be read"
  read_count=$(grep -c '' "$work/$case_name.tsv") || read_count=0
  cut -f 2- "$work/$case_name.tsv" >"$work/$case_name.txt" || fail_self "the texts of $case_name cannot be written"
  wpr_check_lines "$work/vocab" "$case_name" source <"$work/$case_name.txt" >"$work/$case_name.out"
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
  source_case src-helper-format-string refused 'fn line(&self) -> String { format!("round={} title=Untitled - Notepad", self.round) }'
  source_case src-defect-evidence refused 'fn x() { defect!("W3-5-A2", format!("TF x14 field=hunter2 count={n}")); }'
  source_case src-write-to-stdout refused 'fn x() { writeln!(std::io::stdout(), "title=Untitled - Notepad").unwrap(); }'
  source_case src-dbg refused 'fn x() { dbg!("hunter2"); }'
  source_case src-blank-before-the-bang refused 'fn x() { println !("field=hunter2"); }'
  source_case src-paren-on-the-next-line refused 'fn x() {
    eprintln!
        ("field=hunter2");
}'
  source_case src-path-expect refused 'fn x() { Option::expect(window, "the window Untitled - Notepad"); }'
  source_case src-panic-any refused 'fn x() { std::panic::panic_any("field=hunter2"); }'
  source_case src-format-argument-left passed 'fn t() { let title = format!("halvo-verify-{}", "x".repeat(3)); }'
  [ "$read_count" -eq 1 ] || fail_self "the fixture src-format-argument-left read $read_count lines, not the one format string"
  source_case src-say-literal refused 'fn c4() { say("c4", "title=Untitled - Notepad"); }'
  source_case src-say-paren-on-the-next-line refused 'fn x() {
    say
        ("c4", "field=hunter2");
}'
  source_case src-unreachable refused 'fn x() { unreachable!("title=Untitled - Notepad"); }'
  source_case src-todo refused 'fn x() { todo!("field=hunter2"); }'
  source_case src-unimplemented refused 'fn x() { unimplemented!("the window Untitled - Notepad"); }'
  source_case src-debug-assert refused 'fn x() { debug_assert!(ok, "field=hunter2"); }'
  source_case src-heads-lines passed 'fn c1() {
    println!("TF c1 keystroke_events={n} ok={}", flag(ok));
    say(test, format!("phase=E rounds={r} counted_rounds={c} landings={l}"));
    say("c9", memory_line(&format!("t+{s}s"), &now));
    say(test, format!("first_focus={:?} reads=[{}] code=0x{:x}", f, r, c));
    panic!("INCONCLUSIVE: the fixture process did not answer {kind}");
    println!("left_out the rounds of the fixtures'"'"' phases E and F but one");
    say(test, format!("marks=[{}@{}->{}:{:?}:{}] counts={}/{} first_frame_within_{}s=1 value={}", a, b, c, d, e, f, g, h, i));
    say(test, format!("list={}", v.join(",")));
}'
  verdict_min=5
  verdict_min=8
  [ "$read_count" -ge "$verdict_min" ] || fail_self "the fixture src-heads-lines read $read_count lines, fewer than $verdict_min"
  source_case src-not-a-print passed 'fn t() {
    let title = String::from("Untitled - Notepad");
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
  plant log-typed-keys "TF browser-stall-chrome phases E and F landings=ppt$cr
TF stall-stale-focus control_reads=phase-E$cr
and q typed$cr
TF c1 key=s$cr
TF c1 key=t$cr
TF c1 keys a and q$cr
TF c1 key=q1$cr
TF c1 key=e5$cr
TF browser-stall-chrome landings=pq$cr"
  log_case log-typed-keys refused 'prints-rule log-typed-keys line=3 length=11 token=2
prints-rule log-typed-keys line=4 length=11 token=3
prints-rule log-typed-keys line=5 length=11 token=3
prints-rule log-typed-keys line=6 length=18 token=6
prints-rule log-typed-keys line=7 length=12 token=3
prints-rule log-typed-keys line=8 length=12 token=3
prints-rule log-typed-keys line=9 length=35 token=3'
  printf 'TF c4\tbytes=5\r\n' >"$work/log-tab-in-a-line.log" || fail_self "the fixture log-tab-in-a-line cannot be written"
  log_case log-tab-in-a-line refused 'prints-rule log-tab-in-a-line line=1 length=13 token=2'
  plant log-field-text-of-a-literal "TF stall plain_len=5 plain_text=aaaaa$cr
TF browser-stall-chrome field=secret$cr"
  log_case log-field-text-of-a-literal refused 'prints-rule log-field-text-of-a-literal line=1 length=37 token=4
prints-rule log-field-text-of-a-literal line=2 length=36 token=3'
  known_id=$(sed -n 's/^id://p' "$work/vocab" | head -n 1) || fail_self "the vocabulary cannot be read"
  plant log-address-and-path "TF c1 host=10.0.0.7 port=443$cr
TF c1 addr=2001:4860:0:2001::68$cr
TF c1 file=/windows/keyboard/source$cr
TF c1 dir=crates\\windows\\keyboard$cr
TF c1 page=file:///windows/temp$cr
TF c1 user=window@keyboard$cr
TF c1 addr=2001:4860:0:0:0:0:0:68$cr
TF c1 host=window:8080$cr
TF c1 addr=window::8080$cr
TF c4 class=Keyboard.Window$cr
TF c1 site=keyboard.window$cr
TF c1 /windows/keyboard$cr
TF c1 dir=windows/$cr
TF c1 /windows$cr"
  log_case log-address-and-path refused 'prints-rule log-address-and-path line=1 length=28 token=3
prints-rule log-address-and-path line=2 length=31 token=3
prints-rule log-address-and-path line=3 length=35 token=3
prints-rule log-address-and-path line=4 length=33 token=3
prints-rule log-address-and-path line=5 length=31 token=3
prints-rule log-address-and-path line=6 length=26 token=3
prints-rule log-address-and-path line=7 length=33 token=3
prints-rule log-address-and-path line=8 length=22 token=3
prints-rule log-address-and-path line=9 length=23 token=3
prints-rule log-address-and-path line=10 length=27 token=3
prints-rule log-address-and-path line=11 length=26 token=3
prints-rule log-address-and-path line=12 length=23 token=3
prints-rule log-address-and-path line=13 length=18 token=3
prints-rule log-address-and-path line=14 length=14 token=3'
  plant log-punctuation-alone "TF ;$cr
TF !@#\$%^&*()$cr
TF c1 key=--$cr
TF c1 load=50%$cr"
  log_case log-punctuation-alone refused 'prints-rule log-punctuation-alone line=1 length=4 token=2
prints-rule log-punctuation-alone line=2 length=13 token=2
prints-rule log-punctuation-alone line=3 length=12 token=3
prints-rule log-punctuation-alone line=4 length=14 token=3'
  plant log-known-defect-text "KNOWN-DEFECT Hunter2-Qzxvsecret-Mybanktitle$cr
KNOWN-DEFECT Hunter$cr
KNOWN-DEFECT $known_id Hunter-Two$cr
TF KNOWN-DEFECT $known_id$cr
KNOWN-DEFECT $known_id $known_id$cr"
  log_case log-known-defect-text refused "prints-rule log-known-defect-text line=1 length=43 token=2
prints-rule log-known-defect-text line=2 length=19 token=2
prints-rule log-known-defect-text line=3 length=$((24 + ${#known_id})) token=3
prints-rule log-known-defect-text line=4 length=$((16 + ${#known_id})) token=3
prints-rule log-known-defect-text line=5 length=$((14 + 2 * ${#known_id})) token=3"
  plant log-phase-letters "TF phase h and u and n and t and e and r$cr
TF stall phases E and F and G$cr
TF stall phase=e$cr
TF c9 memory phase=t+5s$cr
TF stall phase E and F$cr
TF stall phases E keys and Q$cr"
  log_case log-phase-letters refused 'prints-rule log-phase-letters line=1 length=40 token=3
prints-rule log-phase-letters line=2 length=29 token=8
prints-rule log-phase-letters line=3 length=16 token=3
prints-rule log-phase-letters line=5 length=22 token=6
prints-rule log-phase-letters line=6 length=28 token=7'
  plant log-closed-values "TF c5 scanned_key=HKLM/keyboard$cr
TF c5 scanned_key=WINDOW/native$cr
TF browser-stall-chrome file_reads=ok:hunter,failed:0,not_read:0,no_element:0$cr
thread 'tf-c5' (1) panicked at crates\\halvo-capture\\src\\macos\\tap.rs:1:1:$cr
thread 'tf-c5' (1) panicked at crates\\secret\\notes.rs:1:1:$cr"
  log_case log-closed-values refused 'prints-rule log-closed-values line=1 length=31 token=3
prints-rule log-closed-values line=2 length=31 token=3
prints-rule log-closed-values line=3 length=77 token=3
prints-rule log-closed-values line=4 length=73 token=6
prints-rule log-closed-values line=5 length=58 token=6'
  plant log-review-forms "TF c1 key=a$cr
TF c1 key=f$cr
TF c4 title=defaced$cr
TF c1 count=12hunter$cr
TF c1 value=0xhunter$cr"
  log_case log-review-forms refused 'prints-rule log-review-forms line=1 length=11 token=3
prints-rule log-review-forms line=2 length=11 token=3
prints-rule log-review-forms line=3 length=19 token=3
prints-rule log-review-forms line=4 length=20 token=3
prints-rule log-review-forms line=5 length=20 token=3'
  heads_log_lines | sed "s/KNOWN_ID/$known_id/" >"$work/heads.txt" || fail_self "the fixture log-heads-lines cannot be written"
  awk '{ printf "%s\r\n", $0 }' "$work/heads.txt" >"$work/log-heads-lines.log" ||
    fail_self "the fixture log-heads-lines cannot be written"
  log_case log-heads-lines passed
  [ "$read_count" -ge 23 ] || fail_self "the fixture log-heads-lines read $read_count lines, fewer than 23"
  wpr_check_log log-missing "$work/no-such.log" "$work/vocab" >"$work/log-missing.out"
  verdict log-missing unreadable "$?" 0
  : >"$work/empty-vocab"
  printf 'TF c1 ok=true\n' >"$work/one.log" || fail_self "the fixture log-without-a-vocabulary cannot be written"
  wpr_check_log log-without-a-vocabulary "$work/one.log" "$work/empty-vocab" >"$work/log-without-a-vocabulary.out"
  verdict log-without-a-vocabulary unreadable "$?" 0
  mkdir "$work/plain" || fail_self "the fixture tree cannot be made"
  printf 'a.rs\nb.rs\n' >"$work/plain/list" || fail_self "the fixture tree cannot be written"
  : >"$work/plain/unprinted" || fail_self "the fixture tree cannot be written"
  printf 'fn a() {\n    println!("TF c1 ok={}", flag(ok));\n}\n' >"$work/plain/a.rs" || fail_self "the fixture tree cannot be written"
  printf 'fn b() {\n    say("c4", "start=ok");\n    say("c4", format!("bytes={n} title=Untitled - Notepad"));\n}\n' >"$work/plain/b.rs" ||
    fail_self "the fixture tree cannot be written"
  tree_case tree-with-a-planted-title 1 'refused b.rs:3 length=32 token=2'
  printf 'fn b() {\n    say("c4", "start=ok");\n}\n' >"$work/plain/b.rs" || fail_self "the fixture tree cannot be written"
  tree_case tree-without-a-planted-title 0 ''
  printf 'a.rs\n' >"$work/plain/required" || fail_self "the fixture tree cannot be written"
  judge_case judge-a-clean-tree 0 "PASS $name"
  printf 'fn b() {\n    say("c4", "start=ok");\n    say("c4", format!("bytes={n} title=Untitled - Notepad"));\n}\n' >"$work/plain/b.rs" ||
    fail_self "the fixture tree cannot be written"
  judge_case judge-a-planted-title 1 "FAIL $name (1 literal lines of print calls break the rule; each is named above by its file and line, without its text: print a count, a flag or a fixed name in its place)" 'refused b.rs:3 length=32 token=2'
  printf 'fn b() {\n    say("c4", "start=ok");\n}\n' >"$work/plain/b.rs" || fail_self "the fixture tree cannot be written"
  printf 'a.rs\nab.rs\n' >"$work/plain/required" || fail_self "the fixture tree cannot be written"
  printf 'fn c() {}\n' >"$work/plain/ab.rs" || fail_self "the fixture tree cannot be written"
  printf 'a.rs\nb.rs\nab.rs\n' >"$work/plain/list" || fail_self "the fixture tree cannot be written"
  judge_case judge-a-required-file-without-a-print 1 "FAIL $name (no print call was read in ab.rs)"
  printf 'a.rs\n' >"$work/plain/required" || fail_self "the fixture tree cannot be written"
  printf 'b.rs\tfile:///{}\tan entry that no literal of the tree holds\n' >"$work/plain/unprinted" || fail_self "the fixture tree cannot be written"
  judge_case judge-a-stale-unprinted-entry 1 "FAIL $name (1 entries of the list of unprinted format strings match no refused literal: take each such entry out of the list)"
  printf 'b.rs\tfile:///{}\tthe URL of a planted page\n' >"$work/plain/unprinted" || fail_self "the fixture tree cannot be written"
  printf 'fn b() {\n    let url = format!("file:///{}", path);\n}\n' >"$work/plain/b.rs" || fail_self "the fixture tree cannot be written"
  judge_case judge-an-unprinted-url 0 "PASS $name"
  printf '100644 blob 1111111111111111111111111111111111111111\tcrates/halvo-capture/src/lib.rs\n100644 blob 2222222222222222222222222222222222222222\tcrates/halvo-capture/src/macos/tap.rs\n100644 blob 3333333333333333333333333333333333333333\tcrates/halvo-capture/src/macos.rs\n100644 blob 4444444444444444444444444444444444444444\tcrates/halvo-capture/tests/macos_stall_fixture.rs\n' >"$work/view-tree" ||
    fail_self "the fixture view-without-macos-only-paths cannot be written"
  printf 'crates/halvo-capture/src/macos\ncrates/halvo-capture/tests/macos_stall_fixture.rs\n' >"$work/view-skip" ||
    fail_self "the fixture view-without-macos-only-paths cannot be written"
  wpr_view_tree "$work/view-tree" "$work/view-skip" >"$work/view-out" || fail_self "the fixture view-without-macos-only-paths cannot be read"
  kept=$(cut -f 2 "$work/view-out" | tr '\n' ' ') || fail_self "the fixture view-without-macos-only-paths cannot be read"
  [ "$kept" = 'crates/halvo-capture/src/lib.rs crates/halvo-capture/src/macos.rs ' ] ||
    fail_self "the fixture view-without-macos-only-paths kept another set of paths than the view's"
  rejected=$((rejected + 1))
  printf 'case %s/view-without-macos-only-paths want=two-paths-kept got=two-paths-kept\n' "$name"
  macos_paths=$(grep -c '^path:crates/halvo-capture/src/macos/' "$work/vocab") || macos_paths=0
  view_paths=$(grep -c '^path:crates/halvo-capture/' "$work/vocab") || view_paths=0
  [ "$macos_paths" -eq 0 ] && [ "$view_paths" -gt 0 ] ||
    fail_self "the vocabulary at HEAD names $macos_paths macOS-only paths and $view_paths paths of the view"
  printf 'case %s/vocabulary-of-the-windows-view macos_only_paths=0 view_paths=%s\n' "$name" "$view_paths"
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  '') measure ;;
  --self-test) self_test ;;
  --help)
    cd "${CI_D_REPO:-.}" || exit 2
    . ./scripts/r1/windows-prints-rule.sh
    wpr_forms
    ;;
  *)
    printf 'usage: sh %s [--self-test | --help]\n' "$0"
    exit 64
    ;;
esac
