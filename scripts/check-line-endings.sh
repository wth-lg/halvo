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

report_script_without_eol_lf() {
  path=$1 eol_attribute=$2
  failures=$((failures + 1))
  {
    printf 'check-line-endings: %s starts with an interpreter line and its eol attribute is %s, not lf.\n' "$path" "$eol_attribute"
    if name_needs_no_quoting "$path"; then
      printf 'Fix: add this line at the end of .gitattributes at the root of the clone:\n/%s text eol=lf\n' "$path"
    else
      printf 'Fix: add a line at the end of .gitattributes at the root of the clone that gives this file: text eol=lf. Its name has a character that a pattern must quote; gitattributes(5) says how.\n'
    fi
  } >&2
}

report_eol_lf_file_stored_with_crlf() {
  path=$1
  failures=$((failures + 1))
  {
    printf 'check-line-endings: %s has eol=lf and is stored in the index with CRLF, so every checkout gets the carriage returns.\n' "$path"
    if name_needs_no_quoting "$path"; then
      printf 'Fix: run this command at the root of the clone and commit the result:\ngit add --renormalize -- %s\n' "$path"
    else
      printf 'Fix: run git add --renormalize on this file and commit the result.\n'
    fi
  } >&2
}

report_text_file_with_binary_content() {
  failures=$((failures + 1))
  printf 'check-line-endings: %s has eol=lf, and git reads its content in the index as binary: it holds a carriage return outside a CRLF line ending, or a NUL byte.\nFix: take that byte out of the file and commit. If the file is not text, give it -text in .gitattributes.\n' "$1" >&2
}

check() {
  root=$(git rev-parse --show-toplevel 2>/dev/null) ||
    die 'the current directory is not inside the work tree of a git clone'
  cd "$root" || die "cannot enter $root"
  stop_at_an_attribute_line_of_this_clone_alone
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
    attribute_lines=$(attributes_of "$crlf_files" eol) || exit
    while IFS= read -r attribute_line; do
      case $attribute_line in
        *': eol: lf') report_eol_lf_file_stored_with_crlf "${attribute_line%: eol: *}" ;;
      esac
    done <<EOF
$attribute_lines
EOF
  fi

  binary_files=$(files_whose_index_content_is '-text') || exit
  if [ -n "$binary_files" ]; then
    attribute_lines=$(attributes_of "$binary_files" text eol) || exit
    while IFS= read -r text_attribute_line && IFS= read -r eol_attribute_line; do
      case $text_attribute_line in
        *': text: set' | *': text: unspecified') ;;
        *) continue ;;
      esac
      case $eol_attribute_line in
        *': eol: lf') report_text_file_with_binary_content "${eol_attribute_line%: eol: *}" ;;
      esac
    done <<EOF
$attribute_lines
EOF
  fi

  if [ "$failures" -gt 0 ]; then
    printf 'check-line-endings: FAIL: files that break a rule: %d. In a clone with core.autocrlf=true, the installer default of Git for Windows, git checks out a file without eol=lf with CRLF, and a shell other than the sh of Git for Windows does not run a script with CRLF.\n' "$failures" >&2
    exit 1
  fi
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

  start_fixture '*.sh text eol=lf' '.githooks/* text eol=lf'
  track .githooks/commit-msg '#!/bin/sh\nexec true\n'
  track scripts/check.sh '#!/bin/sh\necho\n'
  expect 'the two patterns of this repository cover a hook and a .sh file' 0 "$(pass_line 2)"

  start_fixture '*.sh eol=crlf'
  track scripts/check.sh '#!/bin/sh\necho\n'
  expect 'a file that starts with an interpreter line and has eol=crlf is refused' 1 'check-line-endings: scripts/check.sh starts with an interpreter line and its eol attribute is crlf, not lf.'

  start_fixture 'configure -diff'
  track configure '#!/bin/sh\necho\n'
  expect 'a script that git does not diff, with no eol attribute, is refused' 1 '/configure text eol=lf'

  start_fixture
  track 'my tool' '#! /usr/bin/env python3\nprint()\n'
  expect 'a file whose name has a space and whose interpreter follows a space is read and refused, with no line that would not work' 1 'Fix: add a line at the end of .gitattributes at the root of the clone that gives this file: text eol=lf. Its name has a character that a pattern must quote; gitattributes(5) says how.'

  start_fixture '*.sh text eol=lf'
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

  start_fixture '*.sh text eol=lf'
  track scripts/check.sh '#!/bin/sh\r\necho\n'
  expect 'a file with eol=lf that is stored with CRLF on one of its lines is refused' 1 'git add --renormalize -- scripts/check.sh'

  start_fixture '*.sh text eol=lf'
  track scripts/check.sh '#!/bin/sh\r\nprintf x\ry\r\n'
  expect 'a file with eol=lf that holds a carriage return inside a line is refused' 1 'check-line-endings: scripts/check.sh has eol=lf, and git reads its content in the index as binary: it holds a carriage return outside a CRLF line ending, or a NUL byte.'

  start_fixture '* text=auto eol=lf'
  track icon.png '\0211PNG\r\n\0032\n\0000\0000'
  expect 'an image under text=auto eol=lf is accepted' 0 "$(pass_line 0)"

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
