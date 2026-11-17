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

read_literals() {
  : >"$2" || return 2
  while IFS= read -r file; do
    git show "HEAD:$file" >"$work/source" || return 2
    wpr_print_literals "$file" <"$work/source" >>"$2" || return 2
  done <"$1"
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
  read_literals "$work/files" "$work/literals" || fail "a file of the tests cannot be read at HEAD"
  while IFS= read -r file; do
    found=$(grep -c -F "$file:" "$work/literals") || found=0
    [ "$found" -gt 0 ] || fail "no print call was read in $file"
  done <<EOF
$required
EOF
  cut -f 1 "$work/literals" >"$work/places" || fail "the places of the literals cannot be written"
  cut -f 2- "$work/literals" >"$work/texts" || fail "the texts of the literals cannot be written"
  files=$(grep -c '' "$work/files") || files=0
  lines=$(grep -c '' "$work/texts") || lines=0
  words=$(grep -c '' "$work/vocab") || words=0
  printf 'prints: files=%s literal_lines=%s vocabulary_words=%s\n' "$files" "$lines" "$words"
  wpr_check_lines "$work/vocab" "$name" <"$work/texts" >"$work/refused"
  status=$?
  refused=$(grep -c '' "$work/refused") || refused=0
  case $status in
    0) ;;
    1) [ "$refused" -gt 0 ] || fail "the rule exited 1 and named no line" ;;
    *) fail "the rule could not read the literals: exit $status" ;;
  esac
  while IFS= read -r line; do
    index=${line#* line=}
    index=${index%% *}
    place=$(sed -n "${index}p" "$work/places")
    printf 'refused %s %s\n' "$place" "${line#* line=* }"
  done <"$work/refused"
  [ "$refused" -eq 0 ] ||
    fail "$refused literal lines of print calls break the rule; each is named above by its file and line, without its text: print a count, a flag or a fixed name in its place"
  printf 'PASS %s\n' "$name"
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

verdict() {
  lines_named=$(grep -c '' "$work/$1.out") || lines_named=0
  case $2 in
    refused)
      [ "$3" -eq 1 ] && [ "$lines_named" -gt 0 ] ||
        fail_self "the fixture $1 was not refused: exit $3, $lines_named lines named"
      rejected=$((rejected + 1))
      printf 'case %s/%s want=refused got=refused (lines read %s, lines named %s)\n' "$name" "$1" "$4" "$lines_named"
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
  source_case src-title-outside-ascii refused 'fn c4() { println!("TF c4 title=Caf\u{e9} bytes={n}"); }'
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
