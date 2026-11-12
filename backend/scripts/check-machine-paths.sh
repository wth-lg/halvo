#!/bin/sh
set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd -P)
A_SINGLE_QUOTE="'"
STARTS_A_PLACEHOLDER='<$%*{'
ENDS_THE_BARE_DIRECTORY='"`,()[}'"$A_SINGLE_QUOTE"
SEPARATOR_BLANK_OR_END_OF_LINE='/\\[:space:][:cntrl:]'
FIRST_CHARACTER_OF_A_NAME="[^]$STARTS_A_PLACEHOLDER$ENDS_THE_BARE_DIRECTORY.$SEPARATOR_BLANK_OR_END_OF_LINE]"
PERIOD_THAT_STARTS_A_NAME='\.[^[:space:][:cntrl:]]'
NAME="($FIRST_CHARACTER_OF_A_NAME|$PERIOD_THAT_STARTS_A_NAME)"'[^[:punct:][:space:][:cntrl:]]*'
HOME_OF_ANY_PLATFORM='[/\\]+(Users|home|Documents and Settings)[/\\]+'
WINDOWS_HOME_IN_ANY_LETTER_CASE='([A-Za-z]:[/\\]+|\\+)[Uu][Ss][Ee][Rr][Ss][/\\]+'
START_OF_A_WORD='(^|[[:space:]"`'"$A_SINGLE_QUOTE"'/\\])'
HOME_WITH_DASHES_FOR_SEPARATORS="$START_OF_A_WORD"'(-(Users|home)-|[A-Za-z]--[Uu][Ss][Ee][Rr][Ss]-)'
HOME_DIRECTORY_AND_A_NAME="($HOME_OF_ANY_PLATFORM|$WINDOWS_HOME_IN_ANY_LETTER_CASE|$HOME_WITH_DASHES_FOR_SEPARATORS)$NAME"
HOME_WITH_DASHES_AND_A_NAME="$HOME_WITH_DASHES_FOR_SEPARATORS$NAME"

usage() {
  cat >&2 <<'EOF'
Refuses a home-directory path of any platform in a tracked file: a path under /Users/ or /home/
or under a Windows users directory, when a name follows the directory. Such a path names a
machine and an account. Write the path relative to the repository, take the location from an
environment variable, or write the name as a placeholder. A name is taken for a placeholder only
when its first character is <, $, %, * or {: <name>, $USER, %USERNAME%, * and {name} pass. Every
other name is refused, also one that starts with _, - or a dot. The directory alone passes when
a blank, the end of the line, a double or single quote, a backtick, a comma, a round, square or
curly bracket, or a period before a blank or the end of the line follows it. What stands behind
one of these characters is not read as a name.

  sh scripts/check-machine-paths.sh                  every file in the index
  sh scripts/check-machine-paths.sh --staged [TREE]  the files that the index adds or changes
                                                     against TREE (default: the last commit);
                                                     .githooks/pre-commit runs this, and
                                                     .githooks/pre-merge-commit when a merge
                                                     makes a commit
  sh scripts/check-machine-paths.sh --self-test      prove both ways on indexes made for it

It reads the index of the checkout it is started in, from any directory of it, and not the
working tree: when that checkout holds the directory above scripts/, only the files under that
directory, named from there, and else the whole index. It prints
"FILE:LINE:PATH" for each path it finds, in text and binary files and in the target of a
symbolic link. A Windows path counts in any letter case. A home path that a tool wrote with
dashes for separators into the name of a directory, -Users-<name> or -home-<name>, counts at the
start of a word: at the start of a line, or after a blank, a quote, a backtick, a slash or a
backslash. So does its Windows form, a drive letter and two dashes before Users-<name>, which
counts in any letter case. In the
name of a tracked file, which it prints as line 0, it looks only for the form with dashes, and
not for the forms with slashes, because a directory named home or Users is a common name in a
source tree. It reads bytes: a URL or a
relative path that holds such a part is refused too, and a path in UTF-16 text or in
percent-encoding is not found. Exit 0 when it finds none, 1 when it finds one, 2 when it cannot
read the index, 64 on bad usage. It only reads, so it is safe to run twice; the self-test
leaves its indexes under .tmp/. One of them is a repository in which the self-test commits and
merges as the configured git user, the merges and one refused commit under a pre-commit and a
pre-merge-commit hook that run it with --staged, which the self-test writes beside its indexes.
EOF
  exit 64
}

cannot_read() {
  echo "machine paths: cannot read the index: $1" >&2
  exit 2
}

# shellcheck disable=SC2016
PATHS_IN_THE_GIVEN_FILES_OR_THE_WHOLE_INDEX='
  scope=$1
  pattern=$2
  pattern_for_names=$3
  shift 3
  [ "$scope" = index ] || [ $# -gt 0 ] || exit 0
  take_names_as_the_index_spells_them=core.precomposeunicode=false
  LC_ALL=C git -c "$take_names_as_the_index_spells_them" --literal-pathspecs grep --cached -n -a -o -E -e "$pattern" -- "$@"
  [ $? -le 1 ] || exit 255
  git -c "$take_names_as_the_index_spells_them" --literal-pathspecs ls-files -s -- "$@" | while read -r mode object _ file; do
    [ "$mode" = 120000 ] || continue
    target=$(git cat-file blob "$object" | LC_ALL=C grep -a -o -E -e "$pattern" | sed -n 1p)
    [ -z "$target" ] || printf "%s:1:%s\n" "$file" "$target"
  done
  git -c "$take_names_as_the_index_spells_them" --literal-pathspecs ls-files -z -- "$@" | LC_ALL=C tr "\000" "\n" | LC_ALL=C grep -a -E -e "$pattern_for_names" | while IFS= read -r name; do
    printf "%s:0:%s\n" "$name" "$(printf "%s\n" "$name" | LC_ALL=C grep -a -o -E -e "$pattern_for_names" | sed -n 1p)"
  done
'

paths_in_the_index() {
  sh -c "$PATHS_IN_THE_GIVEN_FILES_OR_THE_WHOLE_INDEX" sh index "$HOME_DIRECTORY_AND_A_NAME" "$HOME_WITH_DASHES_AND_A_NAME"
}

paths_in_the_staged_files() {
  git diff --cached --name-only --relative --diff-filter=ACMRT -z "$@" |
    xargs -0 sh -c "$PATHS_IN_THE_GIVEN_FILES_OR_THE_WHOLE_INDEX" sh staged "$HOME_DIRECTORY_AND_A_NAME" "$HOME_WITH_DASHES_AND_A_NAME"
}

report() {
  found=$1
  [ -n "$found" ] || exit 0
  printf '%s\n' "$found"
  echo "machine paths: $(printf '%s\n' "$found" | wc -l | tr -d ' ') found; write each relative to the repository, take the location from an environment variable, or write the name as a placeholder, which starts with <, \$, %, * or {: <name>, \$USER, %USERNAME%, * or {name}. The directory alone passes when a blank, the end of the line, a double or single quote, a backtick, a comma, a round, square or curly bracket, or a period before a blank or the end of the line follows it." >&2
  exit 1
}

check() {
  [ "$(git rev-parse --is-inside-work-tree 2> /dev/null)" = true ] || cannot_read "$(pwd) is in no git checkout"
  top_of_the_work_tree=$(git rev-parse --show-toplevel) || cannot_read "git names no top of the work tree for $(pwd)"
  where_to_read=$top_of_the_work_tree
  [ "$(git -C "$ROOT" rev-parse --show-toplevel 2> /dev/null)" != "$top_of_the_work_tree" ] || where_to_read=$ROOT
  cd "$where_to_read" || cannot_read "$where_to_read cannot be entered"
  if [ "$1" = index ]; then
    found=$(paths_in_the_index) || cannot_read "git grep failed"
  else
    shift
    git diff --cached --name-only "$@" > /dev/null || cannot_read "git diff failed"
    found=$(paths_in_the_staged_files "$@") || cannot_read "git grep failed"
  fi
  report "$found"
}

fixture_text() {
  e_acute=$(printf '\303\251')
  invalid_byte=$(printf '\377')
  printf '%s\n' "$1" | LC_ALL=C sed -e 's/%/\//g' -e 's/\^/\\/g' -e 's/{percent}/%/g' -e "s/{e-acute}/$e_acute/g" -e "s/{invalid-byte}/$invalid_byte/g" -e 's/{dash}/-/g' | LC_ALL=C tr '@' '\000'
}

expect() {
  kind=$1
  name=$2
  want_exit=$3
  want_lines=$4
  shift 4
  output=$(sh "$ROOT/scripts/check-machine-paths.sh" "$@" 2>&1)
  got=$?
  printf '%s\n' "$output" | sed "s/^/  $name: /"
  lines=$(printf '%s\n' "$output" | grep -ac '^[^:]*:[0-9][0-9]*:')
  if [ "$got" = "$want_exit" ] && [ "$lines" = "$want_lines" ]; then
    case "$kind" in
      good) echo "self-test machine-paths: good $name: PASS as expected" ;;
      bad) echo "self-test machine-paths: bad $name: FAIL as expected" ;;
      stopped) echo "self-test machine-paths: stopped $name: exit $want_exit as expected" ;;
    esac
    return 0
  fi
  echo "self-test machine-paths: $name: exit $got with $lines paths (expected exit $want_exit with $want_lines)"
  return 1
}

expect_the_row() {
  name=$1
  want_row=$2
  row=$(sh "$ROOT/scripts/check-machine-paths.sh" 2> /dev/null)
  if [ "$row" = "$want_row" ]; then
    echo "self-test machine-paths: bad $name: FAIL as expected"
    return 0
  fi
  echo "self-test machine-paths: $name: printed $row (expected $want_row)"
  return 1
}

expect_under_the_hooks() {
  kind=$1
  name=$2
  want_exit=$3
  want_lines=$4
  want_commits=$5
  shift 5
  output=$(git -c core.hooksPath="$scratch/hooks" -c commit.gpgsign=false "$@" 2>&1)
  got=$?
  printf '%s\n' "$output" | sed "s/^/  $name: /"
  lines=$(printf '%s\n' "$output" | grep -ac '^[^:]*:[0-9][0-9]*:')
  commits=$(git rev-list --count HEAD)
  if [ "$got" = "$want_exit" ] && [ "$lines" = "$want_lines" ] && [ "$commits" = "$want_commits" ]; then
    case "$kind" in
      good) echo "self-test machine-paths: good $name: PASS as expected" ;;
      bad) echo "self-test machine-paths: bad $name: FAIL as expected" ;;
    esac
    return 0
  fi
  echo "self-test machine-paths: $name: git $1 exit $got with $lines paths and $commits commits (expected exit $want_exit with $want_lines and $want_commits)"
  return 1
}

new_index() {
  mkdir -p "$1" && cd "$1" && git init -q . 2> /dev/null
}

commit_without_hooks() {
  git -c commit.gpgsign=false -c core.hooksPath=/dev/null commit -q -m "$1"
}

entry_in_the_index() {
  blob=$(fixture_text "$3" | git hash-object -w --stdin) && git update-index --add --cacheinfo "$1,$blob,$2"
}

self_test() {
  unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE
  state=${GATE_STATE:-$ROOT/.tmp}
  mkdir -p "$state"
  scratch=$(mktemp -d "$state/machine-paths.XXXXXX") || exit 2
  mkdir -p "$scratch/hooks" || exit 2
  MACHINE_PATHS_SCRIPT=$ROOT/scripts/check-machine-paths.sh
  export MACHINE_PATHS_SCRIPT
  for hook in pre-commit pre-merge-commit; do
    printf '#!/bin/sh\nexec sh "$%s" --staged\n' MACHINE_PATHS_SCRIPT > "$scratch/hooks/$hook" || exit 2
    chmod +x "$scratch/hooks/$hook" || exit 2
  done
  failed=0
  while read -r kind name want_lines text; do
    (
      new_index "$scratch/$name" || exit 1
      fixture_text "$text" > notes.txt
      git add notes.txt || exit 1
      case "$kind" in
        good) want_exit=0 ;;
        bad) want_exit=1 ;;
      esac
      expect "$kind" "$name" "$want_exit" "$want_lines" || exit 1
      expect "$kind" "$name-staged" "$want_exit" "$want_lines" --staged
    ) || failed=1
  done <<'EOF'
bad mac-home 1 the notes are under %Users%ada%notes.txt
bad linux-home 1 config: %home%ada%.config%halvo
bad windows-backslash 1 C:^Users^ada^notes.txt
bad windows-doubled-backslash 1 "C:^^Users^^ada^^notes.txt"
bad windows-forward-slash 1 C:%Users%ada%notes.txt
bad windows-lowercase 1 c:^users^ada^notes.txt
bad windows-lowercase-forward-slash 1 c:%users%ada%notes.txt
bad windows-before-vista 1 C:^Documents and Settings^ada^notes.txt
bad file-url 1 file:%%%Users%ada%notes.txt
bad include-flag 1 cc -I%Users%ada%include
bad mount-of-another-system 2 %mnt%c%Users%ada and %System%Volumes%Data%home%ada: one line, two paths
bad directory-named-after-a-path 1 ~%.claude%projects%-Users-ada-Documents-halvo%memory
bad dashed-form-at-the-start-of-a-line 1 {dash}Users-ada-Documents-halvo is the directory of a tool
bad dashed-form-after-a-blank 1 the tool keeps its notes in {dash}home-ada-work
bad dashed-form-after-a-double-quote 1 "{dash}Users-ada-Documents-halvo"
bad dashed-form-after-a-single-quote 1 '{dash}Users-ada-Documents-halvo'
bad dashed-form-after-a-backtick 1 `{dash}Users-ada-Documents-halvo`
bad dashed-form-after-a-backslash 1 projects^{dash}Users-ada-Documents-halvo^memory
bad dashed-form-with-a-windows-drive 1 C{dash}-Users-ada-Documents-halvo
bad dashed-form-with-a-small-drive-letter 1 c{dash}-Users-ada-Documents-halvo
bad dashed-form-with-users-in-small-letters-after-a-small-drive-letter 1 c{dash}-users-ada-Documents-halvo
bad dashed-form-with-users-in-capital-letters 1 C{dash}-USERS-ada-Documents-halvo
bad name-outside-ascii 1 %Users%{e-acute}lodie%notes.txt
bad line-with-an-invalid-byte 1 {invalid-byte} %Users%ada%notes.txt
bad binary-file 1 a@b %Users%ada%notes.txt
bad two-lines 2 %Users%ada%one.txt and then %home%ada%two.txt
bad name-that-starts-with-an-underscore 1 a service account: %home%_svc%notes.txt
bad name-that-starts-with-a-dash 1 %Users%-ada%notes.txt
bad name-that-starts-with-a-dot 1 %Users%.ada%notes.txt
bad name-that-starts-with-a-dot-and-an-underscore 1 %Users%._ada%notes.txt
bad name-of-one-character 1 %Users%a%notes.txt
bad name-with-a-period-inside 1 %home%j.doe%notes.txt
bad a-name-before-a-double-quote 1 p.startsWith("%Users%ada")
bad a-name-before-a-single-quote 1 p.startsWith('%home%ada')
bad a-name-before-a-backtick 1 the notes are under `%Users%ada`
bad a-name-before-a-comma 1 %Users%ada, then the rest
bad a-name-before-an-opening-round-bracket 1 %Users%ada(name)
bad a-name-before-a-closing-round-bracket 1 the notes (under %Users%ada) of a Mac
bad a-name-before-an-opening-square-bracket 1 %home%ada[0]
bad a-name-before-a-closing-square-bracket 1 the notes [under %home%ada] of Linux
bad a-name-before-an-opening-curly-bracket 1 %Users%ada{ and nothing else
bad a-name-before-a-closing-curly-bracket 1 the notes {under %Users%ada} of a Mac
bad a-name-before-a-period 1 the notes are under %Users%ada. The rest is elsewhere
good tilde 0 the notes are under ~%notes.txt
good home-variable 0 $HOME%notes.txt and ${HOME}%.config%halvo
good the-directory-alone 0 %Users% holds the accounts of a Mac, %home% those of Linux, %Users%
good the-directory-before-a-double-quote 0 p.startsWith("%Users%")
good the-directory-before-a-single-quote 0 p.startsWith('%home%')
good the-directory-before-a-backtick 0 the accounts of a Mac are under `%Users%`
good the-directory-before-a-comma 0 %Users%, then %home%, then the rest
good the-directory-before-an-opening-round-bracket 0 %Users%(name)
good the-directory-before-a-closing-round-bracket 0 the accounts (under %Users%) of a Mac
good the-directory-before-an-opening-square-bracket 0 the expression %^%Users^%[a-z]+% finds a home path
good the-directory-before-a-closing-square-bracket 0 the accounts [under %home%] of Linux
good the-directory-before-an-opening-curly-bracket 0 %Users%{ and nothing else
good the-directory-before-a-closing-curly-bracket 0 the accounts {under %Users%} of a Mac
good the-directory-before-a-period-and-a-blank 0 the accounts are under %Users%. The rest is elsewhere
good the-directory-before-a-period-at-the-end-of-the-line 0 the accounts are under %home%.
good what-stands-behind-an-opening-square-bracket 0 %Users%[name]%notes.txt
good what-stands-behind-a-double-quote 0 %Users%"ada"
good what-stands-behind-an-opening-round-bracket 0 %Users%(ada)
good placeholder 0 %Users%<name>%notes.txt, %home%$USER%notes.txt, C:^Users^{percent}USERNAME{percent}^notes.txt, %home%*%notes.txt and %Users%{name}%notes.txt
good pattern-of-this-check 0 [%^^]+(Users|home)[%^^]+[^<${percent}*{%^^[:space:][:cntrl:]]
good lowercase-route 0 GET %users%42 and %homepage%index.html
good dashes-inside-a-word 0 the class nav-home-link, the flag --home-dir and the list all-Users-of-a-plan
good windows-drive-form-inside-a-word 0 the modifier card{dash}-Users-of-a-plan
EOF
  (
    new_index "$scratch/staged" || exit 1
    fixture_text 'an older file: %Users%ada%old.txt' > old.txt
    echo 'a file without a path' > kept.txt
    echo 'a plain file' > plain.txt
    git add old.txt kept.txt plain.txt || exit 1
    tree=$(git write-tree) || exit 1
    expect good nothing-staged-beside-an-older-path 0 0 --staged "$tree" || exit 1
    echo 'a clean file' > 'clean notes.txt'
    git add 'clean notes.txt' || exit 1
    expect good staged-clean-file-beside-an-older-path 0 0 --staged "$tree" || exit 1
    entry_in_the_index 100644 '*.txt' 'a clean file whose name is a pattern' || exit 1
    expect good staged-clean-file-named-like-a-pattern 0 0 --staged "$tree" || exit 1
    fixture_text 'now with a path: %home%ada%kept.txt' > kept.txt
    git add kept.txt || exit 1
    expect bad staged-change-of-an-older-file 1 1 --staged "$tree" || exit 1
    fixture_text 'a new file: %home%ada%new.txt' > '[new] notes.txt'
    git add '[new] notes.txt' || exit 1
    expect bad staged-new-file-with-brackets-in-its-name 1 2 --staged "$tree" || exit 1
    expect bad whole-index-with-all-three 1 3 || exit 1
    echo 'the working tree no longer holds the path that the index holds' > '[new] notes.txt'
    expect bad index-and-not-the-working-tree 1 3 || exit 1
    expect bad staged-index-and-not-the-working-tree 1 2 --staged "$tree" || exit 1
    git mv old.txt moved.txt || exit 1
    expect bad staged-rename-of-a-file-with-a-path 1 3 --staged "$tree" || exit 1
    entry_in_the_index 120000 plain.txt '%Users%ada%target' || exit 1
    expect bad staged-file-that-became-a-link 1 4 --staged "$tree" || exit 1
    expect stopped tree-that-does-not-exist 2 0 --staged no-such-tree || exit 1
    expect stopped unknown-argument 64 0 --bogus
  ) || failed=1
  (
    new_index "$scratch/links" || exit 1
    entry_in_the_index 120000 'link to a home directory' '%Users%ada%Documents%target' || exit 1
    entry_in_the_index 120000 'link inside the checkout' '..%notes.txt' || exit 1
    expect bad link-to-a-home-directory 1 1 || exit 1
    expect bad link-to-a-home-directory-staged 1 1 --staged
  ) || failed=1
  (
    new_index "$scratch/names" || exit 1
    directory_named_after_a_home_path=$(fixture_text '{dash}Users-ada-Documents-halvo')
    page_under_a_directory_named_home=$(fixture_text 'src%pages%home%index.ts')
    mkdir -p "./$directory_named_after_a_home_path" "$(dirname "$page_under_a_directory_named_home")" || exit 1
    echo 'a file without a path' > "./$directory_named_after_a_home_path/memory.md"
    echo 'a page without a path' > "$page_under_a_directory_named_home"
    git add src || exit 1
    expect good file-under-a-directory-named-home 0 0 || exit 1
    expect good file-under-a-directory-named-home-staged 0 0 --staged || exit 1
    git add . || exit 1
    expect bad file-under-a-directory-named-after-a-home-path 1 1 || exit 1
    expect bad file-under-a-directory-named-after-a-home-path-staged 1 1 --staged || exit 1
    expect_the_row row-of-a-file-under-a-directory-named-after-a-home-path "$directory_named_after_a_home_path/memory.md:0:$(fixture_text '{dash}Users-ada')" || exit 1
    name_with_an_invalid_byte_that_sorts_first=$(fixture_text '!{invalid-byte}.txt')
    entry_in_the_index 100644 "$name_with_an_invalid_byte_that_sorts_first" 'a file without a path' || exit 1
    LC_ALL=en_US.UTF-8 expect bad name-with-an-invalid-byte-before-that-directory 1 1 || exit 1
    LC_ALL=en_US.UTF-8 expect bad name-with-an-invalid-byte-before-that-directory-staged 1 1 --staged
  ) || failed=1
  (
    new_index "$scratch/unreadable-index" || exit 1
    echo 'this is no index' > .git/index
    expect stopped index-that-git-cannot-read 2 0 || exit 1
    expect stopped index-that-git-cannot-read-staged 2 0 --staged
  ) || failed=1
  (
    mkdir -p "$scratch/no-checkout/plain" && cd "$scratch/no-checkout/plain" || exit 1
    GIT_CEILING_DIRECTORIES="$scratch/no-checkout" expect stopped no-checkout 2 0
  ) || failed=1
  (
    new_index "$scratch/started-in-a-subdirectory" || exit 1
    fixture_text 'the notes are under %Users%ada%notes.txt' > notes.txt
    mkdir -p deeper/still || exit 1
    echo 'a file without a path' > deeper/still/clean.txt
    git add notes.txt deeper/still/clean.txt || exit 1
    cd deeper/still || exit 1
    expect bad whole-index-from-a-subdirectory 1 1 || exit 1
    expect bad staged-from-a-subdirectory 1 1 --staged
  ) || failed=1
  (
    new_index "$scratch/decomposed-name" || exit 1
    git config core.precomposeunicode true || exit 1
    tree=$(git write-tree) || exit 1
    e_and_a_combining_acute=$(printf 'e\314\201')
    blob=$(fixture_text 'a new file: %home%ada%new.txt' | git hash-object -w --stdin) || exit 1
    git -c core.precomposeunicode=false update-index --add --cacheinfo "100644,$blob,$e_and_a_combining_acute.txt" || exit 1
    expect bad whole-index-with-a-decomposed-name 1 1 || exit 1
    expect bad staged-file-with-a-decomposed-name 1 1 --staged "$tree" || exit 1
    target=$(fixture_text '%Users%ada%target' | git hash-object -w --stdin) || exit 1
    git -c core.precomposeunicode=false update-index --add --cacheinfo "120000,$target,$e_and_a_combining_acute-link" || exit 1
    expect bad staged-link-with-a-decomposed-name 1 2 --staged "$tree" || exit 1
    tree_with_the_file_and_the_link=$(git write-tree) || exit 1
    clean_blob=$(echo 'a file without a path' | git hash-object -w --stdin) || exit 1
    git -c core.precomposeunicode=false update-index --add --cacheinfo "100644,$clean_blob,$e_and_a_combining_acute/$(fixture_text '{dash}Users-ada-work')/x.md" || exit 1
    expect bad staged-file-under-a-decomposed-name-and-a-dashed-form 1 1 --staged "$tree_with_the_file_and_the_link"
  ) || failed=1
  (
    new_index "$scratch/merge" || exit 1
    trunk=$(git symbolic-ref --short HEAD) || exit 1
    echo 'a file without a path' > kept.txt
    fixture_text 'an older file: %Users%ada%old.txt' > old.txt
    git add kept.txt old.txt || exit 1
    commit_without_hooks 'A file without a path and an older file with a home path' || exit 1
    git checkout -q -b with-a-path || exit 1
    fixture_text 'the notes are under %Users%ada%notes.txt' > notes.txt
    git add notes.txt || exit 1
    commit_without_hooks 'A file with a home path' || exit 1
    git checkout -q -b clean "$trunk" || exit 1
    echo 'a clean file' > clean.txt
    git add clean.txt || exit 1
    commit_without_hooks 'A clean file' || exit 1
    git checkout -q "$trunk" || exit 1
    expect_under_the_hooks good merge-of-a-clean-branch 0 0 3 merge --no-ff --no-edit clean || exit 1
    expect_under_the_hooks bad merge-of-a-branch-with-a-home-path 1 1 3 merge --no-ff --no-edit with-a-path || exit 1
    git merge --abort || exit 1
    fixture_text 'more notes are under %home%ada%more.txt' > more.txt
    git add more.txt || exit 1
    expect_under_the_hooks bad commit-of-a-file-with-a-home-path 1 1 3 commit -q -m 'A second file with a home path'
  ) || failed=1
  exit "$failed"
}

case "${1:-}" in
  "")
    [ $# -eq 0 ] || usage
    check index
    ;;
  --staged)
    [ $# -le 2 ] || usage
    check "$@"
    ;;
  --self-test)
    [ $# -eq 1 ] || usage
    self_test
    ;;
  *) usage ;;
esac
