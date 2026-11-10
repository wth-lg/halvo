#!/bin/sh
set -eu

synopsis() {
  cat <<'EOF'
  sh scripts/check-own-source-reads.sh
      Check the tracked scripts of the clone that holds the current directory:
      every tracked file whose name ends in .sh or whose first line is an
      interpreter line. The check reads them in the work tree.
  sh scripts/check-own-source-reads.sh --self-test
      Prove the rule on fixtures, and the choice of files in a throwaway
      repository under the temporary directory. The self-test removes nothing
      and prints where the repository is.
EOF
}

help_text() {
  cat <<'EOF'
Refuses a script that reads its own source by line number. A help text that a
script prints with sed over lines 2 to 20 of its own file changes when a line
is added above it or a comment is removed, and nothing fails.
  - A line of a script fails when a command on it gets the path of the script,
    written as the quoted parameter 0, and reads it by line number. Such a
    command is sed whose script starts with a line number, head, tail, or awk
    with NR or FNR, named alone or by a path such as /usr/bin/sed. The path of
    the script can come as an operand, on standard input, or from cat through
    a pipe.
The fix is to print the text from a here-document, as this script does.
The check reads one line at a time. It does not see a path that a variable
holds, a command that continues on another line, an address that a variable
holds, a line number after the first command of a sed script, a reader that
another command such as xargs starts, or a read by a pattern. It cannot tell
code from the text of a here-document, so a line of such a text that starts
with such a read is refused too.

EOF
  synopsis
  cat <<'EOF'

Exit codes: 0 no script breaks the rule, 1 a script does, 2 the check could not
run, 64 bad usage. The check only reads, so it is safe to run twice.
EOF
}

INTERPRETER_LINE='^#![[:blank:]]*/'

NEWLINE='
'

die() {
  printf 'check-own-source-reads: error: %s\n' "$1" >&2
  exit 2
}

usage() {
  printf 'check-own-source-reads: %s\n' "$1" >&2
  synopsis >&2
  exit 64
}

reads_of_own_source_by_line_number() {
  SCRIPT_PATH=$1 LC_ALL=C awk '
    function without_single_quoted_text(line,    text, i, last, character, in_double_quotes, closing) {
      text = ""
      last = length(line)
      in_double_quotes = 0
      for (i = 1; i <= last; i++) {
        character = substr(line, i, 1)
        if (character == "\\" && i < last) {
          text = text character substr(line, i + 1, 1)
          i++
        } else if (character == "\"") {
          in_double_quotes = !in_double_quotes
          text = text character
        } else if (character == "\047" && !in_double_quotes) {
          closing = index(substr(line, i + 1), "\047")
          if (closing == 0) return text
          text = text "\047\047"
          i += closing
        } else text = text character
      }
      return text
    }
    function command_gets_own_path(reader,    count, part, i) {
      count = split(shell_text, part, /[|;&()`]/)
      for (i = 1; i <= count; i++)
        if (part[i] ~ own_path && part[i] ~ ("^[ \t]*(" before_a_command ")*" a_directory reader "([ \t]|$)")) return 1
      return shell_text ~ ("(^|[|;&()`])[ \t]*(" before_a_command ")*" a_directory "cat[ \t][^|;&()`]*" own_path "[^|;&()`]*[|][ \t]*(" before_a_command ")*" a_directory reader "([ \t]|$)")
    }
    BEGIN {
      script_path = ENVIRON["SCRIPT_PATH"]
      own_path = "\"[$](0|[{]0[}])\""
      a_directory = "([^ \t|;&()`]*/)?"
      before_a_command = "([A-Za-z_][A-Za-z0-9_]*=[^ \t]*|[0-9]*[<>]+[ \t]*[^ \t]+|if|then|else|elif|do|while|until|exec|command|[!{])[ \t]+"
      sed_and_its_options = "(^|[^A-Za-z0-9_.-])sed[ \t][^|;&]*"
      after_an_address = "[ \t]*[,~!{=A-Za-z]"
    }
    {
      shell_text = without_single_quoted_text($0)
      text_outside_quotes = shell_text
      gsub(/"[^"]*"/, "\"\"", text_outside_quotes)
      sed_starts_at_a_line_number = $0 ~ (sed_and_its_options "\047[ \t]*([$]|[0-9]+)" after_an_address) ||
        $0 ~ (sed_and_its_options "\"[ \t]*[0-9]+" after_an_address) ||
        $0 ~ (sed_and_its_options "[/],[ \t]*[0-9]+") ||
        text_outside_quotes ~ ("(^|[^A-Za-z0-9_.-])sed[ \t]([^|;&]*[ \t])?(-[A-Za-z]*e)?[0-9]+" after_an_address)
      reader = ""
      if (sed_starts_at_a_line_number && command_gets_own_path("sed")) reader = "sed with an address that is a line number"
      else if (command_gets_own_path("head")) reader = "head"
      else if (command_gets_own_path("tail")) reader = "tail"
      else if ($0 ~ /(^|[^A-Za-z0-9_])F?NR([^A-Za-z0-9_]|$)/ && command_gets_own_path("awk")) reader = "awk with NR or FNR"
      if (reader != "") printf "%s:%d: this line hands the path of the script to %s, so the text that it prints changes when a line is added or removed above that text. Fix: print the text from a here-document (cat <<\047EOF\047, the text, EOF), which holds the text and no line number.\n", script_path, NR, reader
    }
  ' "$2"
}

tracked_scripts() {
  named_sh=$(git -c core.quotePath=false ls-files -- '*.sh') || die 'git ls-files could not list the tracked files'
  interpreter_lines=$(git -c core.quotePath=false grep --no-recurse-submodules --color=never --no-column -a -n -G -e "$INTERPRETER_LINE") ||
    [ $? = 1 ] || die 'git grep could not search the tracked files'
  with_an_interpreter_line=$(printf '%s\n' "$interpreter_lines" | LC_ALL=C sed -n 's/^\(.*\):1:#!.*$/\1/p') ||
    die 'sed could not read the output of git grep'
  printf '%s\n%s\n' "$named_sh" "$with_an_interpreter_line" | LC_ALL=C sort -u || die 'sort could not order the list of scripts'
}

check() {
  root=$(git rev-parse --show-toplevel 2>/dev/null) ||
    die 'the current directory is not inside the work tree of a git clone'
  cd "$root" || die "cannot enter $root"
  scripts=$(tracked_scripts) || exit
  script_count=0
  findings=
  while IFS= read -r path; do
    [ -n "$path" ] || continue
    case $path in
      '"'*) die "git quotes the name of this tracked file, so the check cannot open it: $path" ;;
    esac
    [ -f "$path" ] || continue
    script_count=$((script_count + 1))
    found=$(reads_of_own_source_by_line_number "$path" "./$path") || die "awk exited with an error while reading $path"
    [ -z "$found" ] || findings=$findings$found$NEWLINE
  done <<EOF
$scripts
EOF
  if [ -n "$findings" ]; then
    {
      printf '%s' "$findings"
      printf 'check-own-source-reads: FAIL: scripts read: %d, lines that read their own script by line number: %d.\n' "$script_count" "$(($(printf '%s' "$findings" | wc -l)))"
    } >&2
    exit 1
  fi
  printf 'check-own-source-reads: PASS: scripts read: %d, lines that read their own script by line number: 0.\n' "$script_count"
}

leave_the_repository_that_git_variables_name() {
  repository_variables=$(git rev-parse --local-env-vars) || die 'git rev-parse could not list the variables that name a repository'
  for repository_variable in $repository_variables; do
    unset "$repository_variable"
  done
}

self_test() {
  failures=0
  dollar='$'
  own_path="\"${dollar}0\""
  own_path_in_braces="\"${dollar}{0}\""
  lines_2_to_20="'2,20p'"

  expect() {
    name=$1 want_lines=$2 text=$3
    found=$(printf '%s\n' "$text" | reads_of_own_source_by_line_number fixture -) || die 'awk exited with an error while reading a fixture'
    lines=$(printf '%s\n' "$found" | sed -n 's/^fixture:\([0-9][0-9]*\): .* Fix: .*$/\1/p' | tr '\n' ' ')
    lines=${lines% }
    if [ "$lines" = "$want_lines" ]; then
      printf 'PASS %s\n' "$name"
    else
      printf 'FAIL %s (expected findings with a fix on lines [%s]; found lines [%s] and this output)\n%s\n' "$name" "$want_lines" "$lines" "$found"
      failures=$((failures + 1))
    fi
  }

  expect 'sed over lines 2 to 20 of the script, the help of scripts/r1-gate/run.sh as it was, is refused' 1 "    -h|--help) sed -n $lines_2_to_20 $own_path | sed 's/^# \\{0,1\\}//'; exit 0 ;;"
  expect 'the finding names the line of the read' 3 "#!/bin/sh${NEWLINE}set -eu${NEWLINE}sed -n $lines_2_to_20 $own_path"
  expect 'two reads in one script are two findings' '1 3' "sed -n $lines_2_to_20 $own_path${NEWLINE}echo${NEWLINE}head -n 3 $own_path"
  expect 'sed with one line number and no quotes is refused' 1 "sed -n 3p $own_path"
  expect 'sed that deletes by line number is refused' 1 "sed '1,2d' $own_path"
  expect 'sed that prints the last line is refused' 1 "sed -n '\$p' $own_path"
  expect 'sed from a pattern to a line number is refused' 1 "sed -n '/^Usage/,20p' $own_path"
  expect 'sed with the script on standard input is refused' 1 "sed -n $lines_2_to_20 <$own_path"
  expect 'sed with the path written in braces is refused' 1 "sed -n $lines_2_to_20 $own_path_in_braces"
  expect 'sed that reads the script through cat and a pipe is refused' 1 "cat $own_path | sed -n $lines_2_to_20"
  expect 'head is refused' 1 "head -n 20 $own_path"
  expect 'tail is refused' 1 "tail -n +2 $own_path | head -n 19"
  expect 'awk with NR is refused, whatever its program holds' 1 "awk 'NR >= 2 && NR <= 20 { print }' $own_path"
  expect 'sed with a pattern as its address is accepted' '' "sed -n 's/^#: //p' $own_path"
  expect 'sed by line number over another file is accepted' '' "sed -n $lines_2_to_20 \"\$file\""
  expect 'awk without NR over the script is accepted' '' "awk '/^#: / { print }' $own_path"
  expect 'awk that prints its record is accepted' '' "awk 'NR > 1 { print \$0 }' \"\$file\""
  expect 'the directory of the script is accepted' '' "DIR=\$(cd \"\$(dirname $own_path)\" && pwd)"
  expect 'head over a file beside the script is accepted' '' "head -n 1 \"\$(dirname $own_path)/../rust-toolchain.toml\""
  expect 'head after a command that only names the script is accepted' '' "basename $own_path | head -n 1"
  expect 'a message that names the script is accepted' '' "printf '%s\\n' \"usage: \$0 [--help]\""
  expect 'a help text in a here-document is accepted' '' "help_text() {${NEWLINE}  cat <<'EOF'${NEWLINE}Runs the six sub-checks.${NEWLINE}EOF${NEWLINE}}"
  expect 'sed with a line number before the command s is refused' 1 "sed -n '3s/^# //p' $own_path"
  expect 'sed with a line number glued to -e is refused' 1 "sed -n -e2p $own_path"
  expect 'sed with a line number in double quotes is refused' 1 "sed -n \"2,20p\" $own_path"
  expect 'sed with the script on standard input before the command is refused' 1 "<$own_path sed -n $lines_2_to_20"
  expect 'head with the script on standard input before the command is refused' 1 "<$own_path head -n 3"
  expect 'a read after an apostrophe inside double quotes is refused' 1 "echo \"the script's help\"; sed -n $lines_2_to_20 $own_path"
  expect 'sed after an assignment is refused' 1 "LC_ALL=C sed -n $lines_2_to_20 $own_path"
  expect 'sed that quits at a line number is refused' 1 "sed 5q $own_path"
  expect 'sed that deletes one line is refused' 1 "sed '1d' $own_path"
  expect 'sed that prints all but one line is refused' 1 "sed -n '1!p' $own_path"
  expect 'sed with a block at a line number is refused' 1 "sed -n '2{p;q;}' $own_path"
  expect 'awk with FNR is refused' 1 "awk 'FNR <= 3' $own_path"
  expect 'a message that holds the word head before the name of the script is accepted' '' "printf '%s\\n' \"pipe the output to head first\" $own_path"
  expect 'sed with a variable as its script is accepted' '' "sed -n \"\$pattern\" $own_path"
  expect 'sed with a pattern that holds a number is accepted' '' "sed -n '/^# 2nd/p' $own_path"
  expect 'sed named by its path is refused' 1 "/usr/bin/sed -n $lines_2_to_20 $own_path"
  expect 'head named by its path is refused' 1 "/usr/bin/head -n 3 $own_path"
  expect 'tail named by a relative path is refused' 1 "./tools/tail -n +2 $own_path"
  expect 'awk named by its path, with FNR, is refused' 1 "/usr/bin/awk 'FNR <= 3' $own_path"
  expect 'sed named by its path after cat named by its path and a pipe is refused' 1 "/bin/cat $own_path | /usr/bin/sed -n $lines_2_to_20"
  expect 'sed named by its path after an assignment is refused' 1 "LC_ALL=C /usr/bin/sed -n $lines_2_to_20 $own_path"
  expect 'the path of a reader as an operand of another command is accepted' '' "ls -l /usr/bin/head $own_path"

  leave_the_repository_that_git_variables_name
  fixture_directory=$(mktemp -d "${TMPDIR:-/tmp}/check-own-source-reads-self-test.XXXXXX") || die 'mktemp could not make a directory'
  repository=$fixture_directory/repository
  printf 'check-own-source-reads: the fixture repository is %s, which is left in place\n' "$repository"
  git init -q "$repository" || die "git init failed in $fixture_directory"
  mkdir "$repository/hooks" || die "mkdir failed in $repository"
  read_by_line_number="sed -n $lines_2_to_20 $own_path"
  printf '%s\n' "$read_by_line_number" >"$repository/named.sh"
  printf '#!/bin/sh\n%s\n' "$read_by_line_number" >"$repository/hooks/with-an-interpreter-line"
  printf '#! /usr/bin/env sh\n%s\n' "$read_by_line_number" >"$repository/hooks/with-a-space-in-the-interpreter-line"
  printf 'echo clean\n' >"$repository/clean.sh"
  printf 'A script starts like this:\n#!/bin/sh\n%s\n' "$read_by_line_number" >"$repository/notes.md"
  printf '#![cfg_attr(test, allow(unused))]\n// %s\n' "$read_by_line_number" >"$repository/main.rs"
  printf '#!/bin/sh # caf\351\n%s\n' "$read_by_line_number" >"$repository/hooks/a-byte-that-is-not-utf-8"
  printf '%s\n' "$read_by_line_number" >"$repository/untracked.sh"
  git -C "$repository" add named.sh hooks clean.sh notes.md main.rs || die "git add failed in $repository"
  want='hooks/a-byte-that-is-not-utf-8:2 hooks/with-a-space-in-the-interpreter-line:2 hooks/with-an-interpreter-line:2 named.sh:1 FAIL:5'
  status=0
  output=$(cd "$repository" && LC_ALL=en_US.UTF-8 && export LC_ALL && check 2>&1) || status=$?
  got=$(printf '%s\n' "$output" | sed -n -e 's/^\([^ ]*:[0-9][0-9]*\): this line .* Fix: .*$/\1/p' -e 's/^check-own-source-reads: FAIL: scripts read: \([0-9]*\),.*$/FAIL:\1/p' | tr '\n' ' ')
  got=${got% }
  if [ "$status" = 1 ] && [ "$got" = "$want" ]; then
    printf 'PASS in a repository and under a UTF-8 locale, the check reads the tracked files that are named .sh or start with an interpreter line, and no other file\n'
  else
    printf 'FAIL in a repository and under a UTF-8 locale, the check reads the tracked files that are named .sh or start with an interpreter line, and no other file (expected exit 1 and [%s]; found exit %s, [%s] and this output)\n%s\n' "$want" "$status" "$got" "$output"
    failures=$((failures + 1))
  fi

  if [ "$failures" -gt 0 ]; then
    printf 'check-own-source-reads: self-test FAIL: fixtures misjudged: %d\n' "$failures"
    return 1
  fi
  printf 'check-own-source-reads: self-test PASS\n'
}

mode=check
while [ $# -gt 0 ]; do
  case "$1" in
    --self-test) mode=self-test ;;
    -h|--help) help_text; exit 0 ;;
    *) usage "unknown argument: $1" ;;
  esac
  shift
done

case $mode in
  self-test) self_test ;;
  check) check ;;
esac
