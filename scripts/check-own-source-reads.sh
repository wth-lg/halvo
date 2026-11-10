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
  - A line of a script fails when a command on it gets the path of the script
    and reads it by line number. The path is the parameter 0 alone, with or
    without quotes: "$0", "${0}", $0 or ${0}. The command gets it as an
    operand, on standard input, or from cat through one pipe. Such a command
    is sed with a script that starts with a line number, head, tail, or awk
    with NR or FNR.
  - The check splits each line into commands and words as sh does: it follows
    quotes, backslashes, command substitutions and redirections. So it sees
    such a command named alone, by a path such as /usr/bin/sed, also one
    whose directory a variable holds, in quotes, after a backslash, with
    .exe at its end, after assignments, and after exec or command -p.
The fix is to print the text from a here-document, as this script does.
The check reads one line at a time and expands nothing. It does not see:
  - a path that a variable holds or that is built from the parameter 0, as in
    "$(dirname "$0")/name", a file descriptor that another line opened, and a
    command that continues on another line;
  - a reader that a parameter names, as in ${SED:-sed}, a reader of another
    name, such as gsed or perl, and a reader that another command starts,
    such as env, nice or xargs;
  - standard input that a group, a loop or a function hands on, as in
    { head -n 3; } <"$0", and a script that comes through more than one pipe;
  - in sed, an address that a variable holds, a line number after the first
    command of a script, as in '/x/p;3p', and a read by a pattern.
The check cannot tell code from text: it reads every line as code. So it
also refuses a line of a here-document, or of a quoted string that began on
a line above, when such a read is a command of that line.

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
    function clear_word() {
      word_value[depth] = ""
      word_raw[depth] = ""
      word_started[depth] = 0
    }
    function add_to_word(value, raw) {
      word_value[depth] = word_value[depth] value
      word_raw[depth] = word_raw[depth] raw
      word_started[depth] = 1
    }
    function start_frame(kind) {
      depth++
      frame_kind[depth] = kind
      in_double_quotes[depth] = 0
      open_parentheses[depth] = 0
      command_of_frame[depth] = 0
      command_before_a_pipe[depth] = 0
      redirection[depth] = ""
      clear_word()
    }
    function current_command() {
      if (!command_of_frame[depth]) {
        command_count++
        command_of_frame[depth] = command_count
        word_count[command_count] = 0
        own_path_on_standard_input[command_count] = 0
        piped_from[command_count] = command_before_a_pipe[depth]
        command_before_a_pipe[depth] = 0
      }
      return command_of_frame[depth]
    }
    function end_word(    command, is_own_path) {
      if (!word_started[depth]) return
      is_own_path = (word_raw[depth] ~ own_path)
      command = current_command()
      if (redirection[depth] == "") {
        word_count[command]++
        word_values[command, word_count[command]] = word_value[depth]
        word_raws[command, word_count[command]] = word_raw[depth]
        word_is_own_path[command, word_count[command]] = is_own_path
      } else if (redirection[depth] == "input" && is_own_path) own_path_on_standard_input[command] = 1
      redirection[depth] = ""
      clear_word()
    }
    function end_command(with_a_pipe) {
      end_word()
      redirection[depth] = ""
      command_before_a_pipe[depth] = (with_a_pipe ? command_of_frame[depth] : 0)
      command_of_frame[depth] = 0
    }
    function end_frame() {
      end_command(0)
      depth--
    }
    function split_into_commands(line,    i, last, character, next_character, rest, closing, operator, descriptor) {
      split("", word_values)
      split("", word_raws)
      split("", word_is_own_path)
      command_count = 0
      depth = -1
      start_frame("")
      last = length(line)
      for (i = 1; i <= last; i++) {
        character = substr(line, i, 1)
        next_character = substr(line, i + 1, 1)
        if (character == "\\" && i < last) {
          if (in_double_quotes[depth] && index("$`\"\\", next_character) == 0) add_to_word(character next_character, character next_character)
          else add_to_word(next_character, character next_character)
          i++
        } else if (character == "\"") {
          in_double_quotes[depth] = !in_double_quotes[depth]
          add_to_word("", character)
        } else if (character == "$" && next_character == "(") {
          add_to_word(expansion, "$()")
          i++
          start_frame("substitution")
        } else if (character == "`" && frame_kind[depth] == "backquotes" && !in_double_quotes[depth]) {
          end_frame()
        } else if (character == "`") {
          add_to_word(expansion, "``")
          start_frame("backquotes")
        } else if (character == "$" && next_character ~ /[A-Za-z0-9_{@*#?$!-]/) {
          add_to_word(expansion, character)
        } else if (in_double_quotes[depth]) {
          add_to_word(character, character)
        } else if (character == "\047") {
          rest = substr(line, i + 1)
          closing = index(rest, "\047")
          if (closing == 0) {
            add_to_word(rest, character rest)
            break
          }
          add_to_word(substr(rest, 1, closing - 1), substr(line, i, closing + 1))
          i += closing
        } else if (character == ")") {
          if (open_parentheses[depth] > 0) {
            open_parentheses[depth]--
            end_command(0)
          } else if (frame_kind[depth] == "substitution") end_frame()
          else end_command(0)
        } else if (character == "(") {
          end_command(0)
          open_parentheses[depth]++
        } else if (character == "|") {
          end_command(1)
        } else if (character == "&" || character == ";") {
          end_command(0)
        } else if (character == " " || character == "\t") {
          end_word()
        } else if (character == "<" || character == ">") {
          descriptor = ""
          if (word_started[depth] && word_raw[depth] ~ /^[0-9]+$/) {
            descriptor = word_raw[depth]
            clear_word()
          } else end_word()
          operator = character
          if (next_character == character || next_character == "&" || (character == "<" && next_character == ">") || (character == ">" && next_character == "|")) {
            operator = operator next_character
            i++
          }
          if (operator == "<<" && substr(line, i + 1, 1) == "-") i++
          if ((operator == "<&" || operator == ">&") && match(substr(line, i + 1), /^([0-9]+-?|-)/)) i += RLENGTH
          else redirection[depth] = ((operator == "<" || operator == "<>") && descriptor + 0 == 0) ? "input" : "other"
        } else if (character == "#" && !word_started[depth]) {
          break
        } else add_to_word(character, character)
      }
      while (depth >= 0) end_frame()
    }
    function command_word(command,    word, raw) {
      for (word = 1; word <= word_count[command]; word++) {
        raw = word_raws[command, word]
        if (raw ~ /^[A-Za-z_][A-Za-z0-9_]*[+]?=/ || raw ~ /^(if|then|else|elif|do|while|until|!|[{]|exec)$/) continue
        if (raw != "command") return word
        while (word < word_count[command] && word_raws[command, word + 1] ~ /^(-p|--)$/) word++
      }
      return 0
    }
    function name_of(command, word,    name) {
      if (!word) return ""
      name = word_values[command, word]
      sub("^.*/", "", name)
      sub("[.][Ee][Xx][Ee]$", "", name)
      return name
    }
    function operand_or_standard_input_is_own_path(command, first,    word) {
      for (word = first + 1; word <= word_count[command]; word++) if (word_is_own_path[command, word]) return 1
      return own_path_on_standard_input[command]
    }
    function gets_own_path(command, first,    before, first_of_before) {
      if (operand_or_standard_input_is_own_path(command, first)) return 1
      before = piped_from[command]
      if (!before) return 0
      first_of_before = command_word(before)
      return name_of(before, first_of_before) == "cat" && operand_or_standard_input_is_own_path(before, first_of_before)
    }
    function starts_at_a_line_number(script) {
      return script ~ line_number_first || script ~ pattern_to_line_number
    }
    function a_sed_script_starts_at_a_line_number(command, first,    word, value, position, letter, script_follows, script_seen) {
      script_follows = 0
      script_seen = 0
      for (word = first + 1; word <= word_count[command]; word++) {
        value = word_values[command, word]
        if (script_follows) {
          script_follows = 0
          if (starts_at_a_line_number(value)) return 1
        } else if (value ~ /^--expression=/) {
          script_seen = 1
          if (starts_at_a_line_number(substr(value, 14))) return 1
        } else if (value == "--expression") {
          script_seen = 1
          script_follows = 1
        } else if (value ~ /^--file/) {
          script_seen = 1
        } else if (value ~ /^-[A-Za-z]/) {
          for (position = 2; position <= length(value); position++) {
            letter = substr(value, position, 1)
            if (letter == "e") {
              script_seen = 1
              if (position == length(value)) script_follows = 1
              else if (starts_at_a_line_number(substr(value, position + 1))) return 1
              break
            }
            if (letter == "f") script_seen = 1
            if (letter == "f" || letter == "i") break
          }
        } else if (value !~ /^--/ && !script_seen) {
          script_seen = 1
          if (starts_at_a_line_number(value)) return 1
        }
      }
      return 0
    }
    function a_word_holds_nr_or_fnr(command, first,    word) {
      for (word = first + 1; word <= word_count[command]; word++)
        if (word_values[command, word] ~ /(^|[^A-Za-z0-9_])F?NR([^A-Za-z0-9_]|$)/) return 1
      return 0
    }
    BEGIN {
      script_path = ENVIRON["SCRIPT_PATH"]
      expansion = "\001"
      own_path = "^(\"[$](0|[{]0[}])\"|[$](0|[{]0[}]))$"
      line_number_first = "^[ \t]*([$]|[0-9]+)[ \t]*[,~!{=A-Za-z]"
      pattern_to_line_number = "^[ \t]*[/][^;]*[/][ \t]*,[ \t]*([$]|[0-9]+)"
    }
    {
      split_into_commands($0)
      reader = ""
      for (command = 1; command <= command_count && reader == ""; command++) {
        first = command_word(command)
        name = name_of(command, first)
        if (name != "sed" && name != "head" && name != "tail" && name != "awk") continue
        if (!gets_own_path(command, first)) continue
        if (name == "sed") {
          if (a_sed_script_starts_at_a_line_number(command, first)) reader = "sed with an address that is a line number"
        } else if (name == "awk") {
          if (a_word_holds_nr_or_fnr(command, first)) reader = "awk with NR or FNR"
        } else reader = name
      }
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
  tab=$(printf '\t')

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
  expect 'sed after exec is refused' 1 "exec sed -n $lines_2_to_20 $own_path"
  expect 'awk inside a command substitution after an assignment is refused' 1 "help=\$(/usr/bin/awk 'NR > 1 && NR < 9' $own_path)"
  expect 'awk inside a command substitution in double quotes is refused' 1 "printf '%s\\n' \"\$(awk 'NR>=2 && NR<=3' $own_path)\""
  expect 'sed after a redirection that copies a descriptor is refused' 1 ">&2 sed -n '2,3p' $own_path"
  expect 'sed with a script in double quotes that holds a second command is refused' 1 "sed -n \"2,3p;3q\" $own_path"
  expect 'sed named by a path in double quotes is refused' 1 "\"/usr/bin/sed\" -n $lines_2_to_20 $own_path"
  expect 'head named by a path in single quotes is refused' 1 "'/usr/bin/head' -n 3 $own_path"
  expect 'sed named by a path whose directory a variable holds is refused' 1 "\"\$BIN/sed\" -n $lines_2_to_20 $own_path"
  expect 'sed named with .exe at its end is refused' 1 "/usr/bin/sed.exe -n $lines_2_to_20 $own_path"
  expect 'head after command -p is refused' 1 "command -p /usr/bin/head -n 3 $own_path"
  expect 'sed named in double quotes after cat and a pipe is refused' 1 "/bin/cat $own_path | \"/usr/bin/sed\" -n $lines_2_to_20"
  expect 'sed after a backslash is refused' 1 "\\sed -n '2,3p' $own_path"
  expect 'head after cat that reads the script on standard input is refused' 1 "<$own_path cat | head -n 3"
  expect 'head after cat with a redirection before the pipe is refused' 1 "cat $own_path 2>&1 | head -n 3"
  expect 'sed over the parameter 0 without quotes is refused' 1 "sed -n '2,3p' ${dollar}0"
  expect 'head inside backquotes in double quotes is refused' 1 "first=\"\`head -n 3 $own_path\`\""
  expect 'sed with a second script that starts with a line number is refused' 1 "sed -n -e 's/a/b/' -e '2p' $own_path"
  expect 'a reader that an assignment only names before exec of the script is accepted' '' "TAIL=/usr/bin/tail exec $own_path --inner"
  expect 'a reader that an assignment only names before another command is accepted' '' "HEAD_BIN=/usr/bin/head sh $own_path --child"
  expect 'a path that ends in the name of a reader in an assignment is accepted' '' "log=/var/log/tail echo $own_path"
  expect 'a redirection to a file that is named like a reader is accepted' '' ">/tmp/head echo $own_path"
  expect 'two assignments, one of the script and one of a reader, are accepted' '' "self=$own_path tool=bin/tail"
  expect 'awk with NR over another file, with the script in a variable of awk, is accepted' '' "awk -v prog=$own_path 'NR==1{print prog}' data.txt"
  expect 'tail over a file whose name starts with the name of the script is accepted' '' "tail -n 1 $own_path.log"
  expect 'head over a file below a directory that is named after the script is accepted' '' "head -1 $own_path.d/VERSION"
  expect 'head over another file with its output beside the script is accepted' '' "head -n 1 \"\$1\" > $own_path.first"
  expect 'a reader that a parameter names is not seen' '' "\${SED:-/usr/bin/sed} -n $lines_2_to_20 $own_path"
  expect 'a reader that env starts is not seen' '' "/usr/bin/env sed -n $lines_2_to_20 $own_path"
  expect 'a reader of another name is not seen' '' "/opt/homebrew/bin/gsed -n $lines_2_to_20 $own_path"
  expect 'standard input that a group hands on is not seen' '' "{ /usr/bin/head -n 3; } <$own_path"
  expect 'a script that comes through two pipes is not seen' '' "cat $own_path | grep -v '^#!' | head -n 2"
  expect 'a path that is built from the parameter 0 is not seen' '' "sed -n '2,3p' \"\$(dirname $own_path)/tool.sh\""
  expect 'a line number after the first command of a sed script is not seen' '' "sed -n '/x/p;3p' $own_path"
  expect 'a reader that nice starts is not seen' '' "/usr/bin/nice /usr/bin/head -n 3 $own_path"
  expect 'a reader that xargs starts is not seen' '' "echo $own_path | xargs head -n 3"
  expect 'perl, a reader of another name, is not seen' '' "perl -ne 'print if \$. <= 3' $own_path"
  expect 'standard input that a loop hands on is not seen' '' "while :; do head -n 3; break; done <$own_path"
  expect 'a command that continues on another line is not seen' '' "sed -n $lines_2_to_20 \\${NEWLINE}  $own_path"
  expect 'a line of a here-document that holds such a read as its command is refused too' 2 "cat <<'EOF'${NEWLINE}  sed -n $lines_2_to_20 $own_path${NEWLINE}EOF"
  expect 'head after cat with a command substitution among its operands is refused' 1 "cat $own_path \$(true) | head -n 3"
  expect 'command -v, which starts no reader, is accepted' '' "command -v head $own_path"
  for word in 'if' 'then' 'else' 'elif' 'do' 'while' 'until' '!' '{'; do
    expect "head after the word $word is refused" 1 "$word head -n 1 $own_path"
  done
  expect 'sed with --expression= and a line number is refused' 1 "sed -n --expression=2p $own_path"
  expect 'sed with --expression, a space and a line number is refused' 1 "sed -n --expression 2p $own_path"
  expect 'a file operand after --file is no script' '' "sed --file=commands.sed 5k.txt $own_path"
  expect 'a file operand that starts with a digit is no script' '' "sed -n 's/x/y/p' 2nd-file.txt $own_path"
  expect 'the letters after -i are a suffix and no script' '' "sed -ie2p 's/a/b/' $own_path"
  expect 'sed with the script on descriptor 0, written with its number, is refused' 1 "sed -n $lines_2_to_20 0<$own_path"
  expect 'a redirection of another descriptor from the script is accepted' '' "head -n 1 3<$own_path"
  expect 'a read behind a comment sign is accepted' '' "echo usage # ; sed -n $lines_2_to_20 $own_path"
  expect 'a line of a here-document that names such a read after other words is accepted' '' "cat <<'EOF'${NEWLINE}Run sed -n $lines_2_to_20 $own_path to see the usage.${NEWLINE}EOF"
  expect 'sed after a test and two ampersands on one line is refused' 1 "[ \"\$1\" = --help ] && sed -n '2,3p' $own_path"
  expect 'sed that a tab indents and that tabs separate from its words is refused' 1 "${tab}sed${tab}-n${tab}$lines_2_to_20${tab}$own_path"
  expect 'head after cat that reads another file is accepted' '' "cat \"\$list\" | head -n 3"
  expect 'sed after a command that one ampersand sends to the background is refused' 1 "sleep 1 & sed -n $lines_2_to_20 $own_path"

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
