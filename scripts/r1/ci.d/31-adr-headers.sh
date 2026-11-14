#!/bin/sh
# jobs: macos
# rule: Every file under docs/adr is NNNN-name.md with a number that no other file has, its line 1 is a heading, its line 3 is a decision line with a UTC timestamp, and every entry under its Amendments heading starts with a date.
# lane: free
set -u
name=31-adr-headers

problem() {
  printf 'problem %s: %s\n' "$1" "$2"
  problems=$((problems + 1))
}

header_problems() {
  awk '
    function real_date(s, y, m, d, days) {
      y = substr(s, 1, 4) + 0
      m = substr(s, 6, 2) + 0
      d = substr(s, 9, 2) + 0
      if (m < 1 || m > 12 || d < 1) return 0
      days = substr("312831303130313130313031", 2 * m - 1, 2) + 0
      if (m == 2 && ((y % 4 == 0 && y % 100 != 0) || y % 400 == 0)) days = 29
      return d <= days
    }
    function real_time(s) {
      return substr(s, 1, 2) + 0 <= 23 && substr(s, 4, 2) + 0 <= 59 && substr(s, 7, 2) + 0 <= 59
    }
    NR == 3 && /^decision: [0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T[0-9][0-9]:[0-9][0-9]:[0-9][0-9]Z$/ {
      if (!real_date(substr($0, 11, 10)) || !real_time(substr($0, 22, 8)))
        print "line 3 holds " substr($0, 11) ", a date or a time that no calendar has"
    }
    /^#/ {
      title = $0
      sub(/^#+[ \t]*/, "", title)
      sub(/[ \t]+$/, "", title)
      if (tolower(title) == "amendments") {
        section = 1
        entry = 0
        if ($0 != "## Amendments") print "line " NR " '\''" $0 "'\'' is not the heading '\''## Amendments'\''"
        next
      }
      if ($0 ~ /^## /) { section = 0; next }
    }
    !section { next }
    $0 == "" { entry = 0; next }
    /^  / {
      if (!entry) print "line " NR " under '\''## Amendments'\'' is indented and continues no dated entry"
      next
    }
    /^- [0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9][: ]/ {
      entry = 1
      if (!real_date(substr($0, 3, 10))) print "line " NR " under '\''## Amendments'\'' has the date " substr($0, 3, 10) ", which no calendar has"
      next
    }
    {
      entry = 0
      print "line " NR " under '\''## Amendments'\'' does not start with '\''- YYYY-MM-DD'\''"
    }
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
  found=$(header_problems "$1") || exit 2
  [ -n "$found" ] || return
  while IFS= read -r line; do
    problem "$base" "$line"
  done <<EOF
$found
EOF
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

  text=$(printf '%s\n' "$good" | sed '3d') || exit 2
  adr missing-decision-line 0001-first.md "$text"
  check missing-decision-line FAIL "problem 0001-first.md: line 3 is not 'decision: YYYY-MM-DDTHH:MM:SSZ'"

  text=$(printf '%s\n' "$good" | sed 's/^decision: .*/decision: 2026-11-09 07:51/') || exit 2
  adr decision-not-utc 0001-first.md "$text"
  check decision-not-utc FAIL "problem 0001-first.md: line 3 is not 'decision: YYYY-MM-DDTHH:MM:SSZ'"

  adr amendment-without-date 0001-first.md "$good

## Amendments

- 2026-11-11: dated.
- the Decision describes an older commit."
  check amendment-without-date FAIL "problem 0001-first.md: line 14 under '## Amendments' does not start with '- YYYY-MM-DD'"

  text=$(printf '%s\n' "$good" | sed '1s/^# //') || exit 2
  adr no-heading-on-line-1 0001-first.md "$text"
  check no-heading-on-line-1 FAIL "problem 0001-first.md: line 1 is not a heading '# <decision label>'"

  adr indented-after-blank 0001-first.md "$good

## Amendments

- 2026-11-11: dated.

  an undated line."
  check indented-after-blank FAIL "problem 0001-first.md: line 15 under '## Amendments' is indented and continues no dated entry"

  text=$(printf '%s\n\n## Amendments\n\n- 2026-11-11: dated.\n' "$good" | sed 's/^## Amendments$/& /') || exit 2
  adr heading-trailing-space 0001-first.md "$text"
  check heading-trailing-space FAIL "problem 0001-first.md: line 11 '## Amendments ' is not the heading '## Amendments'"

  adr heading-lower-case 0001-first.md "$good

## amendments

- 2026-11-11: dated."
  check heading-lower-case FAIL "problem 0001-first.md: line 11 '## amendments' is not the heading '## Amendments'"

  adr heading-other-level 0001-first.md "$good

### Amendments

- 2026-11-11: dated."
  check heading-other-level FAIL "problem 0001-first.md: line 11 '### Amendments' is not the heading '## Amendments'"

  adr amendment-date-no-calendar-has 0001-first.md "$good

## Amendments

- 2026-02-29: dated on a day that 2026 does not have."
  check amendment-date-no-calendar-has FAIL "problem 0001-first.md: line 13 under '## Amendments' has the date 2026-02-29, which no calendar has"

  text=$(printf '%s\n' "$good" | sed 's/^decision: .*/decision: 2026-11-31T24:51:36Z/') || exit 2
  adr decision-time-no-calendar-has 0001-first.md "$text"
  check decision-time-no-calendar-has FAIL "problem 0001-first.md: line 3 holds 2026-11-31T24:51:36Z, a date or a time that no calendar has"

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
