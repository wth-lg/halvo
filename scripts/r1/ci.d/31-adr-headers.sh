#!/bin/sh
# jobs: macos
# rule: Every file under docs/adr is NNNN-name.md with a number that no other file has, its line 1 is a heading, its line 3 is a decision line with a UTC timestamp, and every entry under its Amendments heading starts with a date.
set -u
name=31-adr-headers

problem() {
  printf 'problem %s: %s\n' "$1" "$2"
  problems=$((problems + 1))
}

amendment_lines_without_a_date() {
  awk '
    /^## / { section = ($0 == "## Amendments"); next }
    section && $0 != "" && $0 !~ /^  / && $0 !~ /^- [0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9][: ]/ { print NR }
  ' "$1"
}

check_file() {
  base=${1##*/}
  if [ ! -f "$1" ] || [ -L "$1" ]; then
    problem "$base" "not a regular file"
    return
  fi
  case $base in
    [0-9][0-9][0-9][0-9]-*.md) ;;
    *)
      problem "$base" "the name is not NNNN-name.md"
      return
      ;;
  esac
  stem=${base#????-}
  stem=${stem%.md}
  case $stem in
    '' | -* | *- | *--* | *[!abcdefghijklmnopqrstuvwxyz0123456789-]*) problem "$base" "the name after the number is not lower-case words joined by -" ;;
  esac
  number=${base%%-*}
  case " $seen " in
    *" $number "*) problem "$base" "the number $number is also the number of another file" ;;
  esac
  seen="$seen $number"
  first=$(sed -n '1p' "$1")
  case $first in
    '# '[!\ ]*) ;;
    *) problem "$base" "line 1 is not a heading '# <decision label>'" ;;
  esac
  third=$(sed -n '3p' "$1")
  case $third in
    'decision: '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T[0-9][0-9]:[0-9][0-9]:[0-9][0-9]Z) ;;
    *) problem "$base" "line 3 is not 'decision: YYYY-MM-DDTHH:MM:SSZ'" ;;
  esac
  for n in $(amendment_lines_without_a_date "$1"); do
    problem "$base" "line $n under '## Amendments' does not start with '- YYYY-MM-DD'"
  done
}

measure() {
  dir=$1/docs/adr
  [ -d "$dir" ] || {
    printf 'FAIL %s (docs/adr is missing)\n' "$name"
    exit 1
  }
  problems=0
  files=0
  seen=
  for f in "$dir"/* "$dir"/.[!.]* "$dir"/..?*; do
    [ -e "$f" ] || [ -L "$f" ] || continue
    files=$((files + 1))
    check_file "$f"
  done
  printf 'files %s problems %s\n' "$files" "$problems"
  if [ "$files" -eq 0 ]; then
    printf 'FAIL %s (docs/adr holds no file)\n' "$name"
    exit 1
  fi
  if [ "$problems" -ne 0 ]; then
    printf 'FAIL %s (%s problems in docs/adr; fix each line that starts with problem)\n' "$name" "$problems"
    exit 1
  fi
  printf 'PASS %s\n' "$name"
}

check() {
  out=$(CI_D_REPO=$work/$1 sh "$0" 2>&1)
  code=$?
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  case $out in
    *"${3:-}"*) ;;
    *) got=$got-without-the-text ;;
  esac
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$got"
  if [ "$got" != "$2" ]; then
    wrong="$wrong $1"
  elif [ "$2" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

adr() {
  mkdir -p "$work/$1/docs/adr" || exit 2
  printf '%s\n' "$3" >"$work/$1/docs/adr/$2" || exit 2
}

good='# a decision

decision: 2026-11-09T07:51:36Z
status: accepted
decided by: planner-1, run 20261108-halvo-r1-capture

## Decision

One sentence.'

amended="$good

## Amendments

- 2026-11-11: a dated line that wraps
  onto a second line.
- 2026-11-12 another dated line.

## Notes

A line under another heading needs no date."

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  rejected=0
  wrong=

  adr headers-hold 0001-first.md "$good"
  adr headers-hold 0002-second-one.md "$amended"
  check headers-hold PASS 'files 2 problems 0'

  adr shared-number 0004-capture.md "$good"
  adr shared-number 0004-storage.md "$good"
  check shared-number FAIL 'problem 0004-storage.md: the number 0004 is also the number of another file'

  adr missing-decision-line 0001-first.md "$(printf '%s\n' "$good" | sed '3d')"
  check missing-decision-line FAIL "problem 0001-first.md: line 3 is not 'decision: YYYY-MM-DDTHH:MM:SSZ'"

  adr decision-not-utc 0001-first.md "$(printf '%s\n' "$good" | sed 's/^decision: .*/decision: 2026-11-09 07:51/')"
  check decision-not-utc FAIL "problem 0001-first.md: line 3 is not 'decision: YYYY-MM-DDTHH:MM:SSZ'"

  adr amendment-without-date 0001-first.md "$good

## Amendments

- 2026-11-11: dated.
- the Decision describes an older commit."
  check amendment-without-date FAIL "problem 0001-first.md: line 14 under '## Amendments' does not start with '- YYYY-MM-DD'"

  adr no-heading-on-line-1 0001-first.md "$(printf '%s\n' "$good" | sed '1s/^# //')"
  check no-heading-on-line-1 FAIL "problem 0001-first.md: line 1 is not a heading '# <decision label>'"

  adr name-without-number 0001-first.md "$good"
  adr name-without-number README.md "$good"
  check name-without-number FAIL 'problem README.md: the name is not NNNN-name.md'

  adr name-with-capitals 0001-First.md "$good"
  check name-with-capitals FAIL 'problem 0001-First.md: the name after the number is not lower-case words joined by -'

  adr directory-entry 0001-first.md "$good"
  mkdir -p "$work/directory-entry/docs/adr/0002-notes.md" || exit 2
  check directory-entry FAIL 'problem 0002-notes.md: not a regular file'

  adr hidden-file 0001-first.md "$good"
  adr hidden-file .0002-hidden.md "$good"
  check hidden-file FAIL 'problem .0002-hidden.md: the name is not NNNN-name.md'

  mkdir -p "$work/empty-directory/docs/adr" || exit 2
  check empty-directory FAIL 'docs/adr holds no file'

  mkdir -p "$work/no-directory" || exit 2
  check no-directory FAIL 'docs/adr is missing'

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  '') measure "$CI_D_REPO" ;;
  *) exit 64 ;;
esac
