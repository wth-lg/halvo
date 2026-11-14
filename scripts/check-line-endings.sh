#!/bin/sh
set -eu

synopsis() {
  cat <<'EOF'
  sh scripts/check-line-endings.sh
      Check the tracked files of the clone that holds the current directory.
  sh scripts/check-line-endings.sh --self-test
      Prove the rules on fixtures in a throwaway repository under the temporary
      directory. The self-test removes nothing and prints where the repository is.
EOF
}

help_text() {
  cat <<'EOF'
Keeps scripts runnable in a clone with core.autocrlf=true, the installer default
of Git for Windows. In such a clone git checks out a text file with CRLF unless
the file has the eol=lf attribute. The sh of Git for Windows runs a script with
CRLF. Other shells do not: sh, dash and ksh on macOS stop with an error.
  - A tracked file whose first line is an interpreter line, "#!" and then a path
    that starts with "/", has the eol=lf attribute.
  - A tracked file with the eol=lf attribute is not stored in the index with CRLF.
  - A tracked file that the attributes make text with eol=lf holds no carriage
    return inside a line and no NUL byte: git reads such content as binary.
  - A tracked file with eol=lf that the attributes do not make text, under
    text=auto or -text, holds no carriage return, unless it holds a NUL byte.
    Where git reports such content as binary, the check reads the bytes in
    the index: content with a NUL byte is binary and may hold any byte, and
    content without one is text that every checkout gets with its carriage
    returns.
  - Three tracked texts quote by hand the lines of .gitattributes at the root
    that give eol=lf: the fix that the windows step "Scripts and hook in a
    core.autocrlf=true clone" prints (.github/workflows/ci.yml), row 4 of
    AGENTS.md, and a fixture of this
    script. Each of these files that is tracked holds its text with those
    lines as .gitattributes has them.
The check reads file content and the .gitattributes files from the index, not
from the work tree. It does not read the attributes file of the user or of the
system. A line in info/attributes of the git directory wins over every
.gitattributes file, and no other clone receives that file, so the check stops
with exit 2 when that file holds an attribute line.

EOF
  synopsis
  cat <<'EOF'

Exit codes: 0 every rule holds, 1 a file breaks a rule, 2 the check could not run,
64 bad usage. The check only reads, so it is safe to run twice.
EOF
}

INTERPRETER_LINE='^#![[:blank:]]*/'

NEWLINE='
'
TICK=$(printf '\140')
QUOTING_PATHS='AGENTS.md .github/workflows/ci.yml scripts/check-line-endings.sh'

die() {
  printf 'check-line-endings: error: %s\n' "$1" >&2
  exit 2
}

usage() {
  printf 'check-line-endings: %s\n' "$1" >&2
  synopsis >&2
  exit 64
}

files_that_start_with_an_interpreter_line() {
  interpreter_lines=$(git -c core.quotePath=false grep --cached --no-recurse-submodules --color=never --no-column -a -n -G -e "$INTERPRETER_LINE") ||
    [ $? = 1 ] || die 'git grep could not search the index'
  files_with_an_interpreter_line=$(git -c core.quotePath=false grep --cached --no-recurse-submodules --color=never -a -l -G -e "$INTERPRETER_LINE") ||
    [ $? = 1 ] || die 'git grep could not search the index'
  [ -n "$files_with_an_interpreter_line" ] || return 0
  while IFS= read -r path; do
    case $NEWLINE$interpreter_lines in
      *"$NEWLINE$path:1:#!"*) printf '%s\n' "$path" ;;
    esac
  done <<EOF
$files_with_an_interpreter_line
EOF
}

files_whose_index_content_is() {
  printf '%s\n' "$eol_listing" |
    awk -F '\t' -v states="$1" '$1 ~ ("^i/(" states ") ") { print substr($0, index($0, "\t") + 1) }' ||
    die 'awk exited with an error while reading the output of git ls-files --eol'
}

attributes_of() {
  paths=$1
  shift
  printf '%s\n' "$paths" |
    GIT_ATTR_NOSYSTEM=1 git -c core.quotePath=false -c core.attributesFile=/dev/null check-attr --cached --stdin "$@" ||
    die 'git check-attr could not read the attributes'
}

stop_at_an_attribute_line_of_this_clone_alone() {
  info_attributes=$(git rev-parse --git-path info/attributes) ||
    die 'git rev-parse could not name the info/attributes file of this clone'
  [ -f "$info_attributes" ] || return 0
  attribute_line_count=$(LC_ALL=C grep -c -v -e '^[[:space:]]*$' -e '^[[:space:]]*#' "$info_attributes") ||
    [ $? = 1 ] || die "grep could not read $info_attributes"
  [ "$attribute_line_count" -gt 0 ] || return 0
  die "$info_attributes holds an attribute line. A line of that file wins over every .gitattributes file, and no other clone receives the file, so this clone cannot show which attributes the tracked .gitattributes files give. Fix: move the line to a tracked .gitattributes file, or to the attributes file of the user, which this check does not read (git config core.attributesFile names it), then run the check again."
}

name_needs_no_quoting() {
  case $1 in
    *[!A-Za-z0-9._/-]*) return 1 ;;
  esac
}

nearest_tracked_attributes_file() {
  nearest=.gitattributes
  nearest_directory=
  while IFS= read -r attributes_file; do
    [ -n "$attributes_file" ] || continue
    directory=${attributes_file%.gitattributes}
    case $1 in
      "$directory"*) [ ${#directory} -lt ${#nearest_directory} ] || nearest=$attributes_file nearest_directory=$directory ;;
    esac
  done <<EOF
$tracked_attributes_files
EOF
  printf '%s\n' "$nearest"
}

print_the_attribute_line_to_add() {
  attributes_file=$(nearest_tracked_attributes_file "$1")
  case $attributes_file in
    .gitattributes) place='.gitattributes at the root of the clone' ;;
    *) place="$attributes_file, the tracked .gitattributes file nearest to this file, whose lines win over the files above it" ;;
  esac
  path_below_that_file=${1#"${attributes_file%.gitattributes}"}
  if name_needs_no_quoting "$path_below_that_file"; then
    printf 'Fix: add this line at the end of %s:\n/%s text eol=lf\n' "$place" "$path_below_that_file"
  else
    printf 'Fix: add a line at the end of %s that gives this file: text eol=lf. Its name has a character that a pattern must quote; gitattributes(5) says how.\n' "$place"
  fi
}

report_script_without_eol_lf() {
  path=$1 eol_attribute=$2
  failures=$((failures + 1))
  {
    printf 'check-line-endings: %s starts with an interpreter line and its eol attribute is %s, not lf.\n' "$path" "$eol_attribute"
    print_the_attribute_line_to_add "$path"
  } >&2
}

report_eol_lf_file_stored_with_crlf() {
  path=$1 text_attribute=$2
  failures=$((failures + 1))
  {
    if [ "$text_attribute" = unset ]; then
      printf 'check-line-endings: %s has eol=lf and -text, and is stored in the index with CRLF. Under -text git converts no line ending, so every checkout gets the carriage returns and git add --renormalize changes nothing.\n' "$path"
      print_the_attribute_line_to_add "$path"
      first_words='Then run' last_words='commit both changes'
    else
      printf 'check-line-endings: %s has eol=lf and is stored in the index with CRLF, so every checkout gets the carriage returns.\n' "$path"
      first_words='Fix: run' last_words='commit the result'
    fi
    if name_needs_no_quoting "$path"; then
      printf '%s this command at the root of the clone and %s:\ngit add --renormalize -- %s\n' "$first_words" "$last_words" "$path"
    else
      printf '%s git add --renormalize on this file and %s.\n' "$first_words" "$last_words"
    fi
  } >&2
}

report_text_file_with_binary_content() {
  failures=$((failures + 1))
  printf 'check-line-endings: %s has eol=lf, and git reads its content in the index as binary: it holds a carriage return outside a CRLF line ending, or a NUL byte.\nFix: take that byte out of the file and commit. If the file is not text, give it -text in .gitattributes.\n' "$1" >&2
}

report_a_carriage_return_in_content_without_a_nul_byte() {
  path=$1 text_attribute=$2
  case $path in
    '"'*) die "git prints the name of this file in quotes, because it holds a double quote, a backslash or a control character: $path. Git takes its content for binary, and the check cannot read the bytes of a file under such a name. Fix: rename the file." ;;
  esac
  git cat-file -e ":0:$path" 2>/dev/null || die "git cat-file could not read the content of $path in the index"
  nul_bytes=$(($(git cat-file blob ":0:$path" | LC_ALL=C tr -d -c '\000' | wc -c)))
  [ "$nul_bytes" = 0 ] || return 0
  carriage_returns=$(($(git cat-file blob ":0:$path" | LC_ALL=C tr -d -c '\r' | wc -c)))
  [ "$carriage_returns" -gt 0 ] || return 0
  failures=$((failures + 1))
  printf 'check-line-endings: %s has eol=lf, and its content in the index holds a carriage return and no NUL byte, while the attributes do not make it text (its text attribute is %s). Git converts no line ending of such content, so every checkout gets the carriage returns.\nFix: take every carriage return out of the file and commit. If the file is not text, take eol=lf from it in .gitattributes.\n' "$path" "$text_attribute" >&2
}

wording_of_the_text_in() {
  case $1 in
    AGENTS.md) opening="$TICK.gitattributes$TICK holds the lines $TICK" separator="$TICK and $TICK" closing=$TICK ;;
    .github/workflows/ci.yml) opening=".gitattributes has to hold the lines '" separator="' and '" closing="'" ;;
    scripts/check-line-endings.sh) opening="start_fixture '" separator="' '" closing="'" ;;
    *) die "this check expects no text in $1" ;;
  esac
}

expected_text_in() {
  wording_of_the_text_in "$1"
  printf '%s\n' "$eol_lf_lines" |
    awk -v opening="$opening" -v separator="$separator" -v closing="$closing" '{ text = text (NR > 1 ? separator : opening) $0 } END { print text closing }' ||
    die 'awk could not build the text that quotes the lines of .gitattributes'
}

file_holds_the_text_and_no_further_quoted_line() {
  wording_of_the_text_in "$1"
  git cat-file blob ":0:$1" |
    EXPECTED_TEXT=$2 FURTHER_QUOTED_LINE=${separator#"$closing"} LC_ALL=C awk '
      BEGIN { text = ENVIRON["EXPECTED_TEXT"]; further = ENVIRON["FURTHER_QUOTED_LINE"] }
      {
        at = index($0, text)
        if (at && index(substr($0, at + length(text)), further) != 1) { found = 1; exit }
      }
      END { exit !found }
    '
}

check_the_texts_that_quote_the_eol_lf_lines() {
  quoting_texts=0
  git cat-file -e :0:.gitattributes 2>/dev/null || return 0
  eol_lf_lines=$(git cat-file blob :0:.gitattributes | LC_ALL=C grep -E -e '^[[:blank:]]*[^#[:blank:]].*[[:space:]]eol=lf([[:space:]]|$)') || return 0
  for quoting_path in $QUOTING_PATHS; do
    git cat-file -e ":0:$quoting_path" 2>/dev/null || continue
    quoting_texts=$((quoting_texts + 1))
    expected_text=$(expected_text_in "$quoting_path") || exit
    file_holds_the_text_and_no_further_quoted_line "$quoting_path" "$expected_text" && continue
    failures=$((failures + 1))
    printf 'check-line-endings: %s does not hold this text, which quotes the lines of .gitattributes that give eol=lf: %s\nFix: write that text into %s. If the wording of that place has to change, change the text that expected_text_in of scripts/check-line-endings.sh builds for it.\n' "$quoting_path" "$expected_text" "$quoting_path" >&2
  done
}

check() {
  root=$(git rev-parse --show-toplevel 2>/dev/null) ||
    die 'the current directory is not inside the work tree of a git clone'
  cd "$root" || die "cannot enter $root"
  stop_at_an_attribute_line_of_this_clone_alone
  tracked_attributes_files=$(git -c core.quotePath=false ls-files -- .gitattributes '*/.gitattributes') ||
    die 'git ls-files could not list the tracked .gitattributes files'
  failures=0

  script_count=0
  scripts=$(files_that_start_with_an_interpreter_line) || exit
  if [ -n "$scripts" ]; then
    attribute_lines=$(attributes_of "$scripts" eol) || exit
    while IFS= read -r attribute_line; do
      script_count=$((script_count + 1))
      case $attribute_line in
        *': eol: lf') ;;
        *) report_script_without_eol_lf "${attribute_line%: eol: *}" "${attribute_line##*: eol: }" ;;
      esac
    done <<EOF
$attribute_lines
EOF
  fi

  eol_listing=$(git -c core.quotePath=false ls-files --eol) || die 'git ls-files could not list the index'

  crlf_files=$(files_whose_index_content_is 'crlf|mixed') || exit
  if [ -n "$crlf_files" ]; then
    attribute_lines=$(attributes_of "$crlf_files" text eol) || exit
    while IFS= read -r text_attribute_line && IFS= read -r eol_attribute_line; do
      case $eol_attribute_line in
        *': eol: lf') report_eol_lf_file_stored_with_crlf "${eol_attribute_line%: eol: *}" "${text_attribute_line##*: text: }" ;;
      esac
    done <<EOF
$attribute_lines
EOF
  fi

  binary_files=$(files_whose_index_content_is '-text') || exit
  if [ -n "$binary_files" ]; then
    attribute_lines=$(attributes_of "$binary_files" text eol) || exit
    while IFS= read -r text_attribute_line && IFS= read -r eol_attribute_line; do
      case $eol_attribute_line in
        *': eol: lf') ;;
        *) continue ;;
      esac
      case $text_attribute_line in
        *': text: set' | *': text: unspecified') report_text_file_with_binary_content "${eol_attribute_line%: eol: *}" ;;
        *) report_a_carriage_return_in_content_without_a_nul_byte "${eol_attribute_line%: eol: *}" "${text_attribute_line##*: text: }" ;;
      esac
    done <<EOF
$attribute_lines
EOF
  fi

  check_the_texts_that_quote_the_eol_lf_lines

  if [ "$failures" -gt 0 ]; then
    printf 'check-line-endings: FAIL: files that break a rule: %d. In a clone with core.autocrlf=true, the installer default of Git for Windows, git checks out a file without eol=lf with CRLF, and a shell other than the sh of Git for Windows does not run a script with CRLF.\n' "$failures" >&2
    exit 1
  fi
  [ "$quoting_texts" = 0 ] ||
    printf 'check-line-endings: texts that quote the lines of .gitattributes with eol=lf, each as the file has them: %d.\n' "$quoting_texts"
  printf 'check-line-endings: PASS: files that start with an interpreter line: %d, each with eol=lf; files with eol=lf that are stored with CRLF or as binary content: 0.\n' "$script_count"
}

leave_the_repository_that_git_variables_name() {
  repository_variables=$(git rev-parse --local-env-vars) || die 'git rev-parse could not list the variables that name a repository'
  for repository_variable in $repository_variables; do
    unset "$repository_variable"
  done
}

self_test() {
  leave_the_repository_that_git_variables_name
  script=$(cd "$(dirname "$0")" && pwd)/$(basename "$0") || die "cannot resolve the path of $0"
  fixture_directory=$(mktemp -d "${TMPDIR:-/tmp}/check-line-endings-self-test.XXXXXX") || die 'mktemp could not make a directory'
  repository=$fixture_directory/repository
  git init -q "$repository" || die "git init failed in $fixture_directory"
  mkdir "$repository/scripts" || die "mkdir failed in $repository"
  failures=0

  start_fixture() {
    git -C "$repository" read-tree --empty || die 'git read-tree could not empty the index of the fixture repository'
    [ $# -eq 0 ] || track .gitattributes "$(printf '%s\\n' "$@")"
  }

  track() {
    path=$1 content=$2
    blob=$(printf '%b' "$content" | git -C "$repository" hash-object -w --stdin) ||
      die "git hash-object could not store the fixture file $path"
    git -C "$repository" update-index --add --cacheinfo 100755 "$blob" "$path" ||
      die "git update-index could not track the fixture file $path"
  }

  track_under_the_name_that_git_prints_as() {
    quoted_name=$1 content=$2
    blob=$(printf '%b' "$content" | git -C "$repository" hash-object -w --stdin) ||
      die "git hash-object could not store the fixture file $quoted_name"
    printf '100755 %s\t%s\n' "$blob" "$quoted_name" | git -C "$repository" update-index --index-info ||
      die "git update-index could not track the fixture file $quoted_name"
  }

  expect() {
    name=$1 want_status=$2 want_line=$3 directory=${4:-.}
    status=0
    output=$(cd "$repository/$directory" && sh "$script" 2>&1) || status=$?
    if [ "$status" = "$want_status" ] && printf '%s\n' "$output" | grep -Fqx -e "$want_line"; then
      printf 'PASS %s\n' "$name"
    else
      printf 'FAIL %s (expected exit %s and the line [%s]; found exit %s and this output)\n%s\n' "$name" "$want_status" "$want_line" "$status" "$output"
      failures=$((failures + 1))
    fi
  }

  pass_line() {
    printf 'check-line-endings: PASS: files that start with an interpreter line: %d, each with eol=lf; files with eol=lf that are stored with CRLF or as binary content: 0.' "$1"
  }

  start_fixture
  track tool '#!/bin/sh\necho\n'
  expect 'a file that starts with an interpreter line and has no eol attribute is refused, with the line to add' 1 '/tool text eol=lf'

  start_fixture '/tool text eol=lf'
  track tool '#!/bin/sh\necho\n'
  expect 'the same file is accepted once .gitattributes holds that line' 0 "$(pass_line 1)"

  start_fixture_with_the_lines_of_this_repository() {
    start_fixture '*.sh text eol=lf' '.githooks/** text eol=lf' 'docs/baseline/r1-gate-bounds.txt text eol=lf' 'mutants.rows text eol=lf' 'mutants.expected text eol=lf' '/scripts/r3/egress/fixtures.txt text eol=lf'
  }

  start_fixture_with_the_lines_of_this_repository
  track .githooks/commit-msg '#!/bin/sh\nexec true\n'
  track .githooks/lib/helper '#!/bin/sh\necho\n'
  track scripts/check.sh '#!/bin/sh\necho\n'
  expect 'the two patterns of this repository cover a hook, a file in a subdirectory of .githooks and a .sh file' 0 "$(pass_line 3)"

  start_fixture '.githooks/* text eol=lf' '*.sh text eol=lf'
  track .githooks/commit-msg '#!/bin/sh\nexec true\n'
  track .githooks/lib/helper '#!/bin/sh\necho\n'
  expect 'the older pattern .githooks/* leaves a file in a subdirectory of .githooks without eol=lf' 1 '/.githooks/lib/helper text eol=lf'

  start_fixture '*.sh eol=crlf'
  track scripts/check.sh '#!/bin/sh\necho\n'
  expect 'a file that starts with an interpreter line and has eol=crlf is refused' 1 'check-line-endings: scripts/check.sh starts with an interpreter line and its eol attribute is crlf, not lf.'

  start_fixture 'configure -diff'
  track configure '#!/bin/sh\necho\n'
  expect 'a script that git does not diff, with no eol attribute, is refused' 1 '/configure text eol=lf'

  start_fixture
  track 'my tool' '#! /usr/bin/env python3\nprint()\n'
  expect 'a file whose name has a space and whose interpreter follows a space is read and refused, with no line that would not work' 1 'Fix: add a line at the end of .gitattributes at the root of the clone that gives this file: text eol=lf. Its name has a character that a pattern must quote; gitattributes(5) says how.'

  start_fixture
  track .gitattributes '*.sh text eol=lf\n'
  track scripts/check.sh '#!/bin/sh\r\necho\r\n'
  expect 'a file with eol=lf that is stored with CRLF is refused, with the command to run' 1 'git add --renormalize -- scripts/check.sh'

  printf '#!/bin/sh\r\necho\r\n' >"$repository/scripts/check.sh"
  git -C "$repository" add --renormalize -- scripts/check.sh || die 'git add --renormalize failed in the fixture repository'
  expect 'the same file is accepted after that command' 0 "$(pass_line 1)"

  start_fixture
  track docs/guide.md 'Run this:\n#!/bin/sh\n'
  track src/main.rs '#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]\nfn main() {}\n'
  track notes.txt 'stored with CRLF\r\n'
  expect 'an interpreter line below the first line, a Rust inner attribute and CRLF without eol=lf are accepted' 0 "$(pass_line 0)"

  start_fixture
  track tool '#!\t/bin/sh\necho\n'
  expect 'an interpreter line with a tab after #! is refused' 1 '/tool text eol=lf'

  start_fixture
  track tool '#!/bin/sh\necho\n'
  expect 'the lint reads the whole clone when it runs in a directory below the root' 1 '/tool text eol=lf' scripts

  start_fixture '/my-tool text eol=lf'
  track my-tool '#!/bin/sh\necho\n'
  track tool 'notes\n#!/bin/sh\n'
  expect 'a file is not a script because the name of a script ends with its name' 0 "$(pass_line 1)"

  start_fixture
  track .gitattributes '*.sh text eol=lf\n'
  track scripts/check.sh '#!/bin/sh\r\necho\n'
  expect 'a file with eol=lf that is stored with CRLF on one of its lines is refused' 1 'git add --renormalize -- scripts/check.sh'

  start_fixture
  track .gitattributes '*.sh text eol=lf\n'
  track scripts/check.sh '#!/bin/sh\r\nprintf x\ry\r\n'
  expect 'a file with eol=lf that holds a carriage return inside a line is refused' 1 'check-line-endings: scripts/check.sh has eol=lf, and git reads its content in the index as binary: it holds a carriage return outside a CRLF line ending, or a NUL byte.'

  start_fixture '* text=auto eol=lf'
  track icon.png '\0211PNG\r\n\0032\n\0000\0000'
  expect 'an image under text=auto eol=lf is accepted' 0 "$(pass_line 0)"

  start_fixture '* text=auto eol=lf'
  track tool '#!/bin/sh\r\nprintf x\ry\r\n'
  expect 'a script under text=auto eol=lf that is stored with CRLF and with a carriage return inside a line is refused' 1 'check-line-endings: tool has eol=lf, and its content in the index holds a carriage return and no NUL byte, while the attributes do not make it text (its text attribute is auto). Git converts no line ending of such content, so every checkout gets the carriage returns.'

  start_fixture '*.sh text eol=lf' 'vendor/** -text'
  track vendor/x.sh '#!/bin/sh\r\nprintf x\ry\r\n'
  expect 'a file with eol=lf and -text that holds a carriage return inside a line is refused' 1 'check-line-endings: vendor/x.sh has eol=lf, and its content in the index holds a carriage return and no NUL byte, while the attributes do not make it text (its text attribute is unset). Git converts no line ending of such content, so every checkout gets the carriage returns.'

  start_fixture '*.sh text eol=lf' 'vendor/** -text'
  track vendor/blob.sh 'BLOB\0000\r\n'
  expect 'a file with eol=lf and -text that holds a NUL byte is accepted as binary content' 0 "$(pass_line 0)"

  start_fixture '* text=auto eol=lf'
  track tool '#!/bin/sh\nprintf x\ry\n'
  expect 'one carriage return inside a line is enough under text=auto eol=lf' 1 'check-line-endings: tool has eol=lf, and its content in the index holds a carriage return and no NUL byte, while the attributes do not make it text (its text attribute is auto). Git converts no line ending of such content, so every checkout gets the carriage returns.'

  start_fixture '* text=auto eol=lf'
  track data.bin '\0001\0001\0001\0001\n'
  expect 'content without a carriage return that git reads as binary for its control characters is accepted' 0 "$(pass_line 0)"

  start_fixture '* text=auto eol=lf'
  track_under_the_name_that_git_prints_as '"data\177.bin"' '\0001\0001\0001\0001\n'
  expect 'a file whose name git prints in quotes stops the check, with the cause and the fix' 2 'check-line-endings: error: git prints the name of this file in quotes, because it holds a double quote, a backslash or a control character: "data\177.bin". Git takes its content for binary, and the check cannot read the bytes of a file under such a name. Fix: rename the file.'

  start_fixture '*.sh eol=lf'
  track scripts/check.sh '#!/bin/sh\r\nprintf x\ry\r\n'
  expect 'a file with eol=lf and no text attribute that holds a carriage return inside a line is refused as text with binary content' 1 'check-line-endings: scripts/check.sh has eol=lf, and git reads its content in the index as binary: it holds a carriage return outside a CRLF line ending, or a NUL byte.'

  start_fixture
  track notes.txt 'a\rb\n'
  expect 'a file without eol=lf that holds a carriage return inside a line is accepted' 0 "$(pass_line 0)"

  start_fixture
  track installer/windows/.gitattributes '* text eol=crlf\n'
  track installer/windows/build.sh '#!/bin/sh\necho build\n'
  expect 'under a nearer .gitattributes file that sets eol=crlf, the line to add belongs at the end of that file' 1 '/build.sh text eol=lf'

  start_fixture
  track installer/windows/.gitattributes '* text eol=crlf\n/build.sh text eol=lf\n'
  track installer/windows/build.sh '#!/bin/sh\necho build\n'
  expect 'the same file is accepted once the nearer file ends with that line' 0 "$(pass_line 1)"

  start_fixture '*.sh text eol=lf' 'vendor/** -text'
  track vendor/x.sh '#!/bin/sh\r\necho x\r\n'
  expect 'a file with eol=lf and -text that is stored with CRLF gets the line that makes it text again' 1 '/vendor/x.sh text eol=lf'

  start_fixture '*.sh text eol=lf' 'vendor/** -text' '/vendor/x.sh text eol=lf'
  track vendor/x.sh '#!/bin/sh\r\necho x\r\n'
  mkdir "$repository/vendor" || die "mkdir failed in $repository"
  printf '#!/bin/sh\r\necho x\r\n' >"$repository/vendor/x.sh"
  git -C "$repository" add --renormalize -- vendor/x.sh || die 'git add --renormalize failed in the fixture repository'
  expect 'the same file is accepted after that line and git add --renormalize' 0 "$(pass_line 1)"

  start_fixture
  track tool '#!/bin/sh\necho\n'
  printf '/tool text eol=lf\n' >"$repository/.gitattributes"
  expect 'an attribute line that only the work tree has does not count' 1 '/tool text eol=lf'

  start_fixture
  track tool '#!/bin/sh\necho\n'
  printf '/tool text eol=lf\n' >"$repository/.git/info/attributes"
  expect 'an attribute line that only info/attributes of the clone has stops the check, with the name of that file' 2 'check-line-endings: error: .git/info/attributes holds an attribute line. A line of that file wins over every .gitattributes file, and no other clone receives the file, so this clone cannot show which attributes the tracked .gitattributes files give. Fix: move the line to a tracked .gitattributes file, or to the attributes file of the user, which this check does not read (git config core.attributesFile names it), then run the check again.'

  start_fixture '/tool text eol=lf'
  track tool '#!/bin/sh\necho\n'
  printf '# a comment and an empty line\n\n' >"$repository/.git/info/attributes"
  expect 'comment lines and empty lines in info/attributes do not stop the check' 0 "$(pass_line 1)"

  start_fixture
  track tool '#!/bin/sh\necho\n'
  printf '/tool text eol=lf\n' >"$fixture_directory/attributes-of-the-user"
  GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=core.attributesFile GIT_CONFIG_VALUE_0=$fixture_directory/attributes-of-the-user
  export GIT_CONFIG_COUNT GIT_CONFIG_KEY_0 GIT_CONFIG_VALUE_0
  expect 'an attribute line that only the attributes file of the user has does not count' 1 '/tool text eol=lf'
  unset GIT_CONFIG_COUNT GIT_CONFIG_KEY_0 GIT_CONFIG_VALUE_0

  track_the_texts_that_quote_the_lines() {
    start_fixture_with_the_lines_of_this_repository
    eol_lf_lines=$(git -C "$repository" cat-file blob :0:.gitattributes) ||
      die 'git cat-file could not read the .gitattributes file of the fixture repository'
    for quoting_path in $QUOTING_PATHS; do
      track "$quoting_path" "$(expected_text_in "$quoting_path")\\n"
    done
  }

  track_the_texts_as_this_repository_words_them() {
    track AGENTS.md "${TICK}.gitattributes${TICK} holds the lines ${TICK}*.sh text eol=lf${TICK} and ${TICK}.githooks/** text eol=lf${TICK}.\\n"
    track .github/workflows/ci.yml "echo \"Fix: .gitattributes has to hold the lines '*.sh text eol=lf' and '.githooks/** text eol=lf', and no later line may take eol=lf from these files.\"\\n"
    track scripts/check-line-endings.sh "$(printf 'start_fixture %s %s' "'*.sh text eol=lf'" "'.githooks/** text eol=lf'")\\n"
  }

  start_fixture
  track .gitattributes '*.sh text eol=lf\n.githooks/** text eol=lf\n'
  track_the_texts_as_this_repository_words_them
  expect 'the three texts that quote the eol=lf lines of .gitattributes, worded as this repository words them, are accepted' 0 'check-line-endings: texts that quote the lines of .gitattributes with eol=lf, each as the file has them: 3.'

  start_fixture
  track .gitattributes '# *.txt text eol=lf\n*.sh text eol=lf\n.githooks/** text eol=lf\n'
  track_the_texts_as_this_repository_words_them
  expect 'a comment line of .gitattributes that names eol=lf is not a line to quote' 0 'check-line-endings: texts that quote the lines of .gitattributes with eol=lf, each as the file has them: 3.'

  start_fixture
  track .gitattributes ' # *.txt text eol=lf\n\t# *.md text eol=lf\n*.sh text eol=lf\n.githooks/** text eol=lf\n'
  track_the_texts_as_this_repository_words_them
  expect 'a comment line of .gitattributes that a space or a tab indents and that names eol=lf is not a line to quote' 0 'check-line-endings: texts that quote the lines of .gitattributes with eol=lf, each as the file has them: 3.'

  start_fixture
  track .gitattributes ' /tool text eol=lf\n'
  track AGENTS.md 'no text that quotes a line\n'
  expect 'a line of .gitattributes with eol=lf that a space indents is a line to quote, with its space' 1 "check-line-endings: AGENTS.md does not hold this text, which quotes the lines of .gitattributes that give eol=lf: ${TICK}.gitattributes${TICK} holds the lines ${TICK} /tool text eol=lf${TICK}"

  start_fixture
  track .gitattributes '\f#notes text eol=lf\n'
  track AGENTS.md 'no text that quotes a line\n'
  expect 'a line of .gitattributes that starts with a form feed and a number sign is a line to quote, as git reads it' 1 "check-line-endings: AGENTS.md does not hold this text, which quotes the lines of .gitattributes that give eol=lf: ${TICK}.gitattributes${TICK} holds the lines ${TICK}$(printf '\f')#notes text eol=lf${TICK}"

  start_fixture
  track .gitattributes '/tool text eol=lf\n'
  track AGENTS.md "${TICK}.gitattributes${TICK} holds the lines ${TICK}/tool text eol=lf${TICK}. A later sentence of the line names ${TICK}a${TICK} and ${TICK}b${TICK}.\\n"
  expect 'a text is accepted when a further quoted word stands later on its line and not right after it' 0 'check-line-endings: texts that quote the lines of .gitattributes with eol=lf, each as the file has them: 1.'

  for stale_path in $QUOTING_PATHS; do
    track_the_texts_that_quote_the_lines
    wanted_text=$(expected_text_in "$stale_path")
    eol_lf_lines=$(printf '%s\n' "$eol_lf_lines" | sed 's/[*][*]/*/')
    track "$stale_path" "$(expected_text_in "$stale_path")\\n"
    expect "a text in $stale_path that quotes an older line of .gitattributes is refused, with the text that the file has to hold" 1 "check-line-endings: $stale_path does not hold this text, which quotes the lines of .gitattributes that give eol=lf: $wanted_text"

    track_the_texts_that_quote_the_lines
    eol_lf_lines=$(printf '%s\n' "$eol_lf_lines" | sed '2s/eol=lf/eol=lf -diff/')
    track "$stale_path" "$(expected_text_in "$stale_path")\\n"
    expect "a text in $stale_path that quotes a longer line than .gitattributes has is refused, with the text that the file has to hold" 1 "check-line-endings: $stale_path does not hold this text, which quotes the lines of .gitattributes that give eol=lf: $wanted_text"

    track_the_texts_that_quote_the_lines
    eol_lf_lines=$eol_lf_lines$NEWLINE'/tool text eol=lf'
    track "$stale_path" "$(expected_text_in "$stale_path")\\n"
    expect "a text in $stale_path that quotes one line more than .gitattributes has is refused, with the text that the file has to hold" 1 "check-line-endings: $stale_path does not hold this text, which quotes the lines of .gitattributes that give eol=lf: $wanted_text"
  done

  if [ "$failures" -gt 0 ]; then
    printf 'check-line-endings: self-test FAIL: fixtures misjudged: %d. The fixture repository is %s\n' "$failures" "$repository"
    return 1
  fi
  printf 'check-line-endings: self-test PASS. The fixture repository is %s\n' "$repository"
}

[ $# -le 1 ] || usage 'more than one argument'
case ${1:-} in
  '') check ;;
  --self-test) self_test ;;
  -h|--help) help_text ;;
  *) usage "unknown argument: $1" ;;
esac
