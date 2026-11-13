#!/bin/sh
# jobs: macos windows
# rule: Every tracked file under the routing run's seven roots matches a line of scripts/r6/layout.txt, the file of path patterns that name the unit owning each file.
# lane: free
set -u
LC_ALL=C
export LC_ALL
name=10-r6-layout
layout=scripts/r6/layout.txt
case $0 in
  /*) self=$0 ;;
  *) self=$(pwd)/$0 ;;
esac

roots() {
  printf '%s\n' docs/r6/ scripts/r6/ scripts/r6-gate/ formal/r6-spend/ \
    crates/halvo-route/ crates/halvo-route-wire/ crates/halvo-route-gate/
}

enter_repo() {
  if [ -n "${CI_D_REPO:-}" ]; then
    repo=$CI_D_REPO
  else
    repo=$(git rev-parse --show-toplevel) || return 1
  fi
  cd "$repo" || return 1
}

setup_failed() {
  printf 'FAIL %s (setup)\n' "$name"
  exit 1
}

usage() {
  printf 'FAIL %s (usage)\n' "$name"
  exit 1
}

broken() {
  printf 'FAIL self-test:%s (%s)\n' "$name" "$1"
  exit 1
}

measure() {
  enter_repo || setup_failed
  artifacts=${CI_D_ARTIFACTS:-target/ci-artifacts/$name}
  mkdir -p "$artifacts" || setup_failed
  scratch=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || setup_failed
  problems=$artifacts/list.txt
  : >"$problems" || setup_failed
  roots >"$scratch/roots.txt" || setup_failed
  git -c core.quotepath=off ls-files >"$scratch/tracked.txt" 2>"$artifacts/ls-files.err" || setup_failed

  patterns=$layout
  if [ ! -f "$layout" ]; then
    printf 'file no regular file\n' >>"$problems"
    : >"$scratch/none.txt" || setup_failed
    patterns=$scratch/none.txt
  else
    printf '\n' >"$scratch/lf.txt" || setup_failed
    tail -c 1 "$layout" >"$scratch/last.txt" || setup_failed
    cmp -s "$scratch/last.txt" "$scratch/lf.txt" || printf 'file no LF at its end\n' >>"$problems"
    iconv -f UTF-8 -t UTF-8 "$layout" >"$scratch/utf8.txt" 2>"$scratch/iconv.err" ||
      printf 'file not UTF-8\n' >>"$problems"
    tr -d '\r' <"$layout" >"$scratch/no-cr.txt" || setup_failed
    cmp -s "$scratch/no-cr.txt" "$layout" || printf 'file carriage return\n' >>"$problems"
    patterns=$scratch/no-cr.txt
  fi

  awk -v all="$all" -v waits="$waits" -v dir="$scratch" -v problems="$problems" '
    function glob(p, s,    c, rest, i, n) {
      while (p != "") {
        c = substr(p, 1, 1)
        if (c == "*") {
          n = length(s)
          if (substr(p, 2, 1) == "*") {
            rest = substr(p, 3)
            for (i = 0; i <= n; i++) if (glob(rest, substr(s, i + 1))) return 1
            return 0
          }
          rest = substr(p, 2)
          for (i = 0; i <= n; i++) {
            if (glob(rest, substr(s, i + 1))) return 1
            if (substr(s, i + 1, 1) == "/") return 0
          }
          return 0
        }
        if (substr(s, 1, 1) != c) return 0
        p = substr(p, 2)
        s = substr(s, 2)
      }
      return s == ""
    }
    function under_root(s,    r) {
      for (r = 1; r <= nroots; r++) if (substr(s, 1, length(root[r])) == root[r]) return 1
      return 0
    }
    function problem(why) {
      printf "line %d %s\n", FNR, why >>problems
    }
    FILENAME == ARGV[1] { root[++nroots] = $0; next }
    FILENAME == ARGV[2] {
      lines++
      if (FNR == 1 && substr($0, 1, 3) == "\357\273\277") { problem("byte-order mark"); next }
      t = $0
      if (gsub(/ /, "", t) != 1) { problem("not one space"); next }
      sp = index($0, " ")
      p = substr($0, 1, sp - 1)
      u = substr($0, sp + 1)
      if (u !~ /^U[0-9]+\.[0-9]+[a-z]?$/) { problem("unit"); next }
      if (p in seen) { problem("pattern twice"); next }
      seen[p] = 1
      if (!under_root(p)) { problem("under no root"); next }
      pat[++npat] = p
      unit[npat] = u
      lit[npat] = p
      sub(/\*.*/, "", lit[npat])
      known[u] = 1
      next
    }
    {
      s = $0
      quoted = substr(s, 1, 1) == "\""
      if (quoted) s = substr(s, 2)
      if (!under_root(s)) next
      files++
      best = 0
      if (!quoted) {
        for (k = 1; k <= npat; k++) {
          if (best && length(pat[k]) <= length(pat[best])) continue
          if (substr(s, 1, length(lit[k])) != lit[k]) continue
          if (glob(pat[k], s)) best = k
        }
      }
      if (best) owned[best]++
      else {
        outside++
        print "layout stray=" $0 >(dir "/stray.txt")
      }
    }
    END {
      if (waits != "") {
        n = split(waits, w, ",")
        for (i = 1; i <= n; i++) {
          if (!(w[i] in known)) exit 3
          waiting[w[i]] = 1
        }
      }
      empty = 0
      if (all) {
        for (k = 1; k <= npat; k++) {
          if (owned[k]) continue
          if (unit[k] in waiting) print "layout waits=" pat[k] >(dir "/waits.txt")
          else {
            empty++
            print "layout empty=" pat[k] >(dir "/empty.txt")
          }
        }
      }
      printf "layout patterns=%d files=%d outside=%d empty=%s\n", lines, files, outside, (all ? empty : "-") >(dir "/summary.txt")
    }
  ' "$scratch/roots.txt" "$patterns" "$scratch/tracked.txt"
  code=$?
  [ "$code" -ne 3 ] || usage
  [ "$code" -eq 0 ] || setup_failed

  cat "$scratch/summary.txt" || setup_failed
  for kind in stray empty waits; do
    if [ -f "$scratch/$kind.txt" ]; then
      sort "$scratch/$kind.txt" || setup_failed
    fi
  done

  cause=
  if [ -s "$problems" ]; then
    cause=list
  elif [ -s "$scratch/stray.txt" ]; then
    cause=outside
  elif [ -s "$scratch/empty.txt" ]; then
    cause=empty
  fi
  if [ -n "$cause" ]; then
    printf 'FAIL %s (%s)\n' "$name" "$cause"
    exit 1
  fi
  printf 'PASS %s\n' "$name"
}

make_repo() {
  dir=$work/$1
  git init -q "$dir" >"$work/$1.init.txt" 2>&1 || broken "cannot make the fixture repository $1"
  git -C "$dir" config core.protectNTFS false || broken "cannot set core.protectNTFS in $1"
  blob=$(git -C "$dir" hash-object -w --stdin </dev/null) || broken "cannot write a blob in $1"
  sed "s|^|100644 $blob$tab|" "$2" >"$work/$1.index.txt" || broken "cannot write the index list of $1"
  git -C "$dir" update-index --index-info <"$work/$1.index.txt" >"$work/$1.update.txt" 2>&1 ||
    broken "cannot fill the index of $1"
  git -C "$dir" ls-files >"$work/$1.indexed.txt" || broken "cannot list the index of $1"
  indexed=$(grep -c '' "$work/$1.indexed.txt") || broken "the index of $1 is empty"
  listed=$(grep -c '' "$2") || broken "the list of $1 is empty"
  [ "$indexed" = "$listed" ] || broken "the index of $1 holds $indexed of its $listed paths"
  mkdir -p "$dir/scripts/r6" "$dir/docs/r6" || broken "cannot make the directories of $1"
}

check() {
  label=$1
  want=$2
  fixture=$3
  if [ "$4" != - ]; then
    cp "$work/layouts/$4" "$work/$fixture/$layout" || broken "cannot place the layout $4"
  fi
  text=$5
  shift 5
  out=$(CI_D_REPO=$work/$fixture CI_D_ARTIFACTS=$work/artifacts/$label sh "$self" "$@" 2>&1)
  code=$?
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  [ "$out" = "$text" ] || got=$got-with-other-lines
  printf 'case %s/%s want=%s got=%s\n' "$name" "$label" "$want" "$got"
  if [ "$got" != "$want" ]; then
    wrong="$wrong $label"
    printf '%s\n' "$out" >"$work/$label.out"
    [ -z "${CI_D_ARTIFACTS:-}" ] || cp "$work/$label.out" "$CI_D_ARTIFACTS/$label.out"
  elif [ "$want" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

self_test() {
  enter_repo || broken "cannot enter the repository"
  [ -f "$layout" ] || broken "no $layout"
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || broken "cannot make a temporary directory"
  mkdir "$work/layouts" "$work/artifacts" || broken "cannot make the fixture directories"
  cp "$layout" "$work/layouts/real.txt" || broken "cannot copy $layout"
  git -c core.quotepath=off ls-files >"$work/real.txt" || broken "cannot list the tracked files"
  printf '%s\n' docs/r6/stray.md >>"$work/real.txt" || broken "cannot add the stray path"
  roots >"$work/roots.txt" || broken "cannot list the roots"
  set --
  while IFS= read -r root; do
    set -- "$@" "$root"
  done <"$work/roots.txt"
  git ls-files -- "$@" >"$work/real-under-roots.txt" || broken "cannot list the tracked files under the roots"
  real_files=$(grep -c '' "$work/real-under-roots.txt")
  real_files=$((real_files + 1))
  real_patterns=$(grep -c '' "$layout")
  tab=$(printf '\t')
  rejected=0
  wrong=

  printf '%s\n' docs/r6/README.md docs/r6/gateway/sequences.jsonl scripts/r6/layout.txt \
    scripts/r6/ci.d/10-r6-layout.sh src/main.rs crates/halvo-routes/src/lib.rs >"$work/a.txt"
  { cat "$work/a.txt" && printf '%s\n' docs/r6/notes.txt; } >"$work/b.txt"
  printf '%s\n' docs/r6/a.md docs/r6/sub/b.md scripts/r6-gate/run.sh scripts/r6/x.sh >"$work/c.txt"
  printf '%s\n' docs/r6/aa.md >"$work/d.txt"
  printf '%s\n' 'docs/r6/q"q.md' >"$work/e.txt"
  { cat "$work/a.txt" && printf '%s\n' docs/r6/README.md.bak; } >"$work/f.txt"
  printf '%s\n' docs/r6/notes docs/r6/.md docs/r6/log-x docs/r6/a >"$work/g.txt"
  for fixture in a b c d e f g real; do
    make_repo "$fixture" "$work/$fixture.txt"
  done
  make_repo missing "$work/a.txt"
  : >"$work/a/docs/r6/untracked.md" || broken "cannot write the untracked file"

  good='docs/r6/README.md U0.5
docs/r6/gateway/** U0.3
docs/r6/gateway/sequences.jsonl U2.3
scripts/r6/layout.txt U0.5
scripts/r6/ci.d/10-r6-layout.sh U0.5
docs/r6/bar.md U1.2'
  rest='docs/r6/gateway/** U0.3
docs/r6/gateway/sequences.jsonl U2.3
scripts/r6/layout.txt U0.5
scripts/r6/ci.d/10-r6-layout.sh U0.5
docs/r6/bar.md U1.2'
  l=$work/layouts
  printf '%s\n' "$good" >"$l/good.txt"
  printf '%s\n%s\n' 'docs/r6/*.md U0.5' 'scripts/r6/** U0.5' >"$l/star.txt"
  printf '%s\n%s\n' 'docs/r6/README.mdU0.5' "$rest" >"$l/no-space.txt"
  printf '%s\n%s\n' 'docs/r6/README.md  U0.5' "$rest" >"$l/two-spaces.txt"
  printf '%s\n%s\n' "$good" 'docs/r6/README.md U1.1' >"$l/twice.txt"
  printf '%s\n%s\n' "$good" 'docs/r7/README.md U0.5' >"$l/no-root.txt"
  printf '%s\r\n%s\n' 'docs/r6/README.md U0.5' "$rest" >"$l/carriage-return-at-the-end.txt"
  printf '%s\ndocs/r6/b\rr.md U1.2\n' "$good" >"$l/carriage-return-inside.txt"
  printf '%s\n%s\n' 'docs/r6/a*.md U0.5' 'docs/r6/*a.md U1.2' >"$l/tie.txt"
  printf '%s\n' 'docs/r6/** U0.5' >"$l/below.txt"
  edges='docs/r6/notes** U0.5
docs/r6/*.md U0.5
docs/r6/log-* U0.5'
  printf '%s\n%s\n' "$edges" 'docs/r6/a U0.5' >"$l/edges-all.txt"
  printf '%s\n' "$edges" >"$l/edges.txt"
  printf '\357\273\277%s\n' "$good" >"$l/byte-order-mark.txt"
  printf '%s' "$good" >"$l/no-final-lf.txt"
  printf '%s\ndocs/r6/b\377r.md U1.2\n' "$good" >"$l/not-utf8.txt"
  printf '%s\n%s\n' "$good" 'docs/r6/hypothesis.md u1.2' >"$l/unit.txt"
  printf '%s\n%s\n' "$good" 'docs/r6/hypothesis.md U1.2d' >"$l/letter-unit.txt"
  printf '%s\n%s\n' "$good" 'docs/r6/hypothesis.md U1.2dd' >"$l/two-letter-unit.txt"
  : >"$l/empty-file.txt"

  head='layout patterns=6 files=4 outside=0'
  check holds PASS a good.txt "$head empty=-
PASS $name"
  check all-finds-two-empty FAIL a good.txt "$head empty=2
layout empty=docs/r6/bar.md
layout empty=docs/r6/gateway/**
FAIL $name (empty)" --all
  check all-waits-for-both PASS a good.txt "$head empty=0
layout waits=docs/r6/bar.md
layout waits=docs/r6/gateway/**
PASS $name" --all --waits U1.2,U0.3
  check all-waits-for-one FAIL a good.txt "$head empty=1
layout empty=docs/r6/gateway/**
layout waits=docs/r6/bar.md
FAIL $name (empty)" --all --waits U1.2
  check one-stray-path FAIL b good.txt "layout patterns=6 files=5 outside=1 empty=-
layout stray=docs/r6/notes.txt
FAIL $name (outside)"
  check literal-pattern-with-a-suffix FAIL f good.txt "layout patterns=6 files=5 outside=1 empty=-
layout stray=docs/r6/README.md.bak
FAIL $name (outside)"
  check stars-match-nothing-and-everything PASS g edges-all.txt "layout patterns=4 files=4 outside=0 empty=-
PASS $name"
  check path-shorter-than-a-pattern FAIL g edges.txt "layout patterns=3 files=4 outside=1 empty=-
layout stray=docs/r6/a
FAIL $name (outside)"
  check star-and-roots FAIL c star.txt "layout patterns=2 files=4 outside=2 empty=-
layout stray=docs/r6/sub/b.md
layout stray=scripts/r6-gate/run.sh
FAIL $name (outside)"
  check tracked-layout-with-a-stray-path FAIL real real.txt "layout patterns=$real_patterns files=$real_files outside=1 empty=-
layout stray=docs/r6/stray.md
FAIL $name (outside)"
  check tie-goes-to-the-earlier-line FAIL d tie.txt "layout patterns=2 files=1 outside=0 empty=1
layout empty=docs/r6/*a.md
FAIL $name (empty)" --all
  check quoted-path-is-stray FAIL e below.txt "layout patterns=1 files=1 outside=1 empty=-
layout stray=\"docs/r6/q\\\"q.md\"
FAIL $name (outside)"
  for fixture in no-space two-spaces twice no-root carriage-return-at-the-end carriage-return-inside byte-order-mark no-final-lf not-utf8 unit empty-file; do
    case $fixture in
      twice | no-root | carriage-return-inside | not-utf8 | unit) count=7 files=4 outside=0 ;;
      empty-file) count=0 files=4 outside=4 ;;
      *) count=6 files=4 outside=0 ;;
    esac
    case $fixture in
      no-space | two-spaces | byte-order-mark) outside=1 ;;
    esac
    text="layout patterns=$count files=$files outside=$outside empty=-"
    case $outside in
      1) text="$text
layout stray=docs/r6/README.md" ;;
      4) text="$text
layout stray=docs/r6/README.md
layout stray=docs/r6/gateway/sequences.jsonl
layout stray=scripts/r6/ci.d/10-r6-layout.sh
layout stray=scripts/r6/layout.txt" ;;
    esac
    check "list-$fixture" FAIL a "$fixture.txt" "$text
FAIL $name (list)"
  done
  check unit-with-a-letter PASS a letter-unit.txt "layout patterns=7 files=4 outside=0 empty=-
PASS $name"
  check list-unit-with-two-letters FAIL a two-letter-unit.txt "layout patterns=7 files=4 outside=0 empty=-
FAIL $name (list)"
  check list-missing-file FAIL missing - "layout patterns=0 files=4 outside=4 empty=-
layout stray=docs/r6/README.md
layout stray=docs/r6/gateway/sequences.jsonl
layout stray=scripts/r6/ci.d/10-r6-layout.sh
layout stray=scripts/r6/layout.txt
FAIL $name (list)"
  check list-before-outside FAIL b no-space.txt "layout patterns=6 files=5 outside=2 empty=-
layout stray=docs/r6/README.md
layout stray=docs/r6/notes.txt
FAIL $name (list)"
  check usage-waits-without-all FAIL a good.txt "FAIL $name (usage)" --waits U0.3
  check usage-waits-for-no-unit FAIL a good.txt "FAIL $name (usage)" --all --waits U9.9
  check usage-waits-with-an-empty-token FAIL a good.txt "FAIL $name (usage)" --all --waits U0.3,
  check usage-waits-without-a-value FAIL a good.txt "FAIL $name (usage)" --all --waits
  check usage-unknown-argument FAIL a good.txt "FAIL $name (usage)" --list
  check usage-all-twice FAIL a good.txt "FAIL $name (usage)" --all --all

  out=$(CI_D_REPO=$work/no-such-directory CI_D_ARTIFACTS=$work/artifacts/setup sh "$self" 2>&1)
  code=$?
  last=$(printf '%s\n' "$out" | tail -n 1)
  if [ "$code:$last" = "1:FAIL $name (setup)" ]; then
    printf 'case %s/setup want=FAIL got=FAIL\n' "$name"
    rejected=$((rejected + 1))
  else
    printf 'case %s/setup want=FAIL got=exit-%s\n' "$name" "$code"
    wrong="$wrong setup"
  fi

  out=$(cd "$work/a/docs/r6" && CI_D_REPO='' CI_D_ARTIFACTS='' sh "$self" 2>&1)
  if [ "$out" = "$head empty=-
PASS $name" ]; then
    printf 'case %s/from-a-subdirectory want=PASS got=PASS\n' "$name"
  else
    printf 'case %s/from-a-subdirectory want=PASS got=other\n' "$name"
    wrong="$wrong from-a-subdirectory"
  fi

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

all=0
waits=
waits_set=0
if [ "${1:-}" = --self-test ]; then
  [ $# -eq 1 ] || usage
  self_test
  exit 0
fi
while [ $# -gt 0 ]; do
  case $1 in
    --all)
      [ "$all" = 0 ] || usage
      all=1
      ;;
    --waits)
      if [ "$waits_set" = 1 ] || [ $# -lt 2 ] || [ -z "$2" ]; then
        usage
      fi
      waits=$2
      waits_set=1
      shift
      ;;
    *) usage ;;
  esac
  shift
done
if [ "$waits_set" = 1 ] && [ "$all" = 0 ]; then
  usage
fi
measure
