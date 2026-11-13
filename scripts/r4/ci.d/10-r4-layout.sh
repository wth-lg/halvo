#!/bin/sh
# jobs: macos windows
# rule: Every tracked file under a root of I14 of the interface run matches a pattern of scripts/r4/layout.txt, which holds the thirteen roots of I14 and lines of its form.
# lane: free
set -u
LC_ALL=C
export LC_ALL
unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE CDPATH
name=10-r4-layout
case $0 in
  /*) self=$0 ;;
  *) self=$(pwd)/$0 ;;
esac
nl='
'
tab='	'

roots() {
  printf '%s\n' docs/r4/ scripts/r4/ scripts/r4-gate/ formal/r4-coordinator/ tools/r4-page/ \
    tools/r4-trials/ tools/r4-app-test/ ui/tests/r4/ ui/tests/fixtures/coord/ crates/halvo-coord/ \
    crates/halvo-coord-app/ crates/halvo-coord-store/ crates/halvo-coord-gate/
}

usage() {
  printf 'FAIL %s (usage)\n' "$name"
  exit 1
}

setup() {
  printf 'layout setup=%s\n' "$1"
  printf 'FAIL %s (setup)\n' "$name"
  exit 1
}

measure() {
  if [ -n "${CI_D_REPO:-}" ]; then
    repo=$CI_D_REPO
  else
    case $0 in
      */*) here=${0%/*} ;;
      *) here=. ;;
    esac
    repo=$(git -C "$here" rev-parse --show-toplevel) || repo=
  fi
  [ -n "$repo" ] || setup repository
  inside=$(git -C "$repo" rev-parse --is-inside-work-tree --show-cdup) || setup repository
  [ "$inside" = true ] || setup repository
  art=${CI_D_ARTIFACTS:-$repo/target/ci-artifacts/$name}
  mkdir -p -- "$art" || setup artifacts
  git -C "$repo" -c core.quotepath=true ls-files >"$art/tracked.txt" || setup git
  roots >"$art/roots.txt" || setup roots

  faults=0
  : >"$art/faults.txt" || setup artifacts
  layout=$repo/scripts/r4/layout.txt
  if [ -f "$layout" ]; then
    tr -cd '\r' <"$layout" >"$art/cr.txt" || setup tr
    if [ -s "$art/cr.txt" ]; then
      printf 'layout fault=carriage-return line=-\n' >>"$art/faults.txt" || setup artifacts
      faults=$((faults + 1))
    fi
    tr -cd '\000' <"$layout" >"$art/nul.txt" || setup tr
    if [ -s "$art/nul.txt" ]; then
      printf 'layout fault=nul line=-\n' >>"$art/faults.txt" || setup artifacts
      faults=$((faults + 1))
    fi
    tr -d '\r\000' <"$layout" >"$art/layout-clean.txt" || setup tr
  else
    printf 'layout fault=missing line=-\n' >>"$art/faults.txt" || setup artifacts
    faults=$((faults + 1))
    : >"$art/layout-clean.txt" || setup artifacts
  fi

  cd -- "$art" || setup artifacts
  awk -v all="$all" -v faults="$faults" '
    function fault(why, n) {
      print "layout fault=" why " line=" n
      faults++
    }
    function decode(s,    out, i, n, c) {
      if (substr(s, 1, 1) != "\"") return s
      out = ""
      n = length(s) - 1
      for (i = 2; i <= n; i++) {
        c = substr(s, i, 1)
        if (c == "\\") {
          out = out quoted
          if (substr(s, i + 1, 1) ~ /[0-7]/) i += 3
          else i++
        } else out = out c
      }
      return out
    }
    function under_root(s, fold,    r, head) {
      for (r = 1; r <= nroots; r++) {
        head = substr(s, 1, length(root[r]))
        if (fold) head = tolower(head)
        if (head == root[r]) return 1
      }
      return 0
    }
    function seg(p, s,    k, rest, i) {
      k = index(p, "*")
      if (k == 0) return (p "") == (s "")
      if (substr(s, 1, k - 1) != substr(p, 1, k - 1)) return 0
      rest = substr(p, k + 1)
      s = substr(s, k)
      for (i = 0; i <= length(s); i++) if (seg(rest, substr(s, i + 1))) return 1
      return 0
    }
    function matches(p, s,    deep, np, ns, a, b, i) {
      deep = substr(p, length(p) - 2) == "/**"
      if (deep) p = substr(p, 1, length(p) - 3)
      np = split(p, a, "/")
      ns = split(s, b, "/")
      if (deep && ns <= np) return 0
      if (!deep && ns != np) return 0
      for (i = 1; i <= np; i++) if (!seg(a[i], b[i])) return 0
      return 1
    }
    BEGIN { quoted = sprintf("%c", 1) }
    FILENAME == "faults.txt" { print; next }
    FILENAME == "roots.txt" { root[++nroots] = $0; want[$0] = 1; next }
    FILENAME == "layout-clean.txt" {
      if (substr($0, 1, 1) == "#") next
      if (split($0, f, "\t") != 2) { fault("form", FNR); next }
      if (f[1] == "root") {
        if (!(f[2] in want)) { fault("root", FNR); next }
        if (f[2] in got) { fault("root-twice", FNR); next }
        got[f[2]] = 1
        next
      }
      if (f[1] !~ /^U[0-9]+\.[0-9]+$/) { fault("unit", FNR); next }
      p = f[2]
      if (p ~ /[^-A-Za-z0-9._\/*]/) { fault("byte", FNR); next }
      if (!under_root(p, 0)) { fault("under-no-root", FNR); next }
      q = p
      if (substr(q, length(q) - 2) == "/**") q = substr(q, 1, length(q) - 3)
      if (index(q, "**")) { fault("stars", FNR); next }
      if (p in seen) { fault("pattern-twice", FNR); next }
      seen[p] = 1
      pat[++npat] = p
      next
    }
    FILENAME == "tracked.txt" {
      if ($0 in listed) next
      listed[$0] = 1
      if ($0 == "scripts/r4/layout.txt") tracked_layout = 1
      s = decode($0)
      if (!under_root(s, 1)) next
      files++
      hit = 0
      for (k = 1; k <= npat; k++) if (matches(pat[k], s)) { hit = 1; filled[k] = 1 }
      if (!hit) {
        outside++
        print "layout outside=" $0
      }
    }
    END {
      for (r = 1; r <= nroots; r++) if (!(root[r] in got)) fault("root-missing root=" root[r], "-")
      if (!tracked_layout) fault("untracked", "-")
      empty = 0
      if (all) for (k = 1; k <= npat; k++) if (!(k in filled)) {
        empty++
        print "layout empty=" pat[k]
      }
      printf "layout patterns=%d files=%d outside=%d empty=%s\n", npat, files, outside, (all ? empty : "-")
      if (faults) exit 10
      if (outside) exit 11
      if (empty) exit 12
      exit 0
    }
  ' faults.txt roots.txt layout-clean.txt tracked.txt >report.txt
  verdict=$?
  case $verdict in
    0 | 10 | 11 | 12) ;;
    *) setup awk ;;
  esac
  last=
  while IFS= read -r line; do
    printf '%s\n' "$line"
    last=$line
  done <report.txt
  case $last in
    'layout patterns='*) ;;
    *) setup report ;;
  esac
  case $verdict in
    0) printf 'PASS %s\n' "$name" ;;
    10) printf 'FAIL %s (layout)\n' "$name" ;;
    11) printf 'FAIL %s (outside)\n' "$name" ;;
    12) printf 'FAIL %s (empty)\n' "$name" ;;
  esac
  [ "$verdict" -eq 0 ] || exit 1
}

broken() {
  printf 'FAIL self-test:%s (fixture)\n' "$name"
  printf 'self-test fixture step failed: %s\n' "$1" >&2
  exit 1
}

judge() {
  case_name=$1
  want=$2
  shift 2
  last=${out##*"$nl"}
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")")
      got=${last#"FAIL $name ("}
      got="FAIL ${got%)}"
      ;;
    *) got=exit-$code ;;
  esac
  for text in "$@"; do
    case $out in
      *"$text"*) ;;
      *) got="$got-without-text" ;;
    esac
  done
  printf 'case %s want=%s got=%s\n' "$case_name" "$want" "$got"
  if [ "$got" != "$want" ]; then
    wrong="$wrong $case_name"
    printf '%s\n' "$out" >"$selfart/case-$case_name.txt" || broken "saving the output of $case_name"
  else
    case $want in
      FAIL*) rejected=$((rejected + 1)) ;;
    esac
  fi
}

run() {
  case_name=$1
  shift
  out=$(CI_D_REPO=$fx CI_D_ARTIFACTS=$work/art/$case_name sh "$self" "$@" 2>&1)
  code=$?
}

make_repo() {
  git init -q "$1" >"$work/init.txt" 2>&1 || broken "git init $1"
  git -C "$1" config core.protectNTFS false || broken "config $1"
  git -C "$1" config core.ignorecase false || broken "config $1"
  git -C "$1" config core.quotepath false || broken "config $1"
  mkdir -p "$1/scripts/r4/ci.d" || broken "mkdir $1"
}

index_set() {
  git -C "$1" read-tree --empty || broken "read-tree $1"
  git -C "$1" update-index --index-info <"$2" >"$work/update.txt" 2>&1 || broken "update-index $2"
  git -C "$1" -c core.quotepath=true ls-files >"$work/indexed.txt" || broken "ls-files $1"
  awk 'BEGIN { FS = "\t" }
    NR == FNR { want[$2] = 1; next }
    { got[$0] = 1 }
    END {
      for (k in want) if (!(k in got)) exit 1
      for (k in got) if (!(k in want)) exit 1
    }' "$2" "$work/indexed.txt" || broken "the index of $1 differs from $2"
}

entries() {
  for path in "$@"; do
    printf '100644 %s\t%s\n' "$blob" "$path" || broken "writing an entry"
  done
}

with_stray() {
  { cat "$work/good.txt" && entries "$@"; } >"$work/stray.txt" || broken "writing the stray list"
  index_set "$fx" "$work/stray.txt"
}

layout_set() {
  printf '%s' "$1" >"$fx/scripts/r4/layout.txt" || broken "writing the layout"
}

stub() {
  mkdir -p "$work/stubs/$1" || broken "mkdir stubs"
  printf '#!/bin/sh\nexit %s\n' "$2" >"$work/stubs/$1/$1" || broken "writing the stub $1"
  chmod 755 "$work/stubs/$1/$1" || broken "chmod the stub $1"
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || broken mktemp
  selfart=${CI_D_ARTIFACTS:-$work/artifacts}
  mkdir -p "$selfart" "$work/art" || broken "mkdir artifacts"
  rejected=0
  wrong=
  fx=$work/fx
  make_repo "$fx"
  make_repo "$work/other"
  blob=$(git -C "$fx" hash-object -w --stdin </dev/null) || broken "hash-object"
  git -C "$work/other" hash-object -w --stdin </dev/null >"$work/blob.txt" || broken "hash-object other"

  {
    entries tools/r4-page/index.mjs tools/r4-page/frame/index.html \
      '"tools/r4-page/q\"uote.txt"' '"tools/r4-page/back\\slash.txt"' '"tools/r4-page/tab\there.txt"' \
      '"tools/r4-page/new\nline.txt"' '"tools/r4-page/\303\251.txt"' \
      crates/halvo-coord/src/types/a/b/c.rs crates/halvo-coord/tests/schedule_.rs \
      crates/halvo-coord/tests/schedule_x.y.rs '"crates/halvo-coord/tests/schedule_\303\251.rs"' \
      crates/halvo-coord-app/src/present/x.rs docs/r4/log-a docs/r4/log- \
      README.md docs/r40/x.md crates/halvo-coordx/a.rs scripts/r4/layout.txt
    printf '100644 %s 1\tdocs/r4/README.md\n100644 %s 2\tdocs/r4/README.md\n100644 %s 3\tdocs/r4/README.md\n' \
      "$blob" "$blob" "$blob" || broken "writing the unmerged entries"
  } >"$work/good.txt" || broken "writing the good list"
  index_set "$fx" "$work/good.txt"
  index_set "$work/other" "$work/good.txt"

  roots >"$work/roots.txt" || broken "writing the roots"
  root_lines=
  root_but_formal=
  while IFS= read -r r; do
    root_lines="${root_lines}root$tab$r$nl"
    [ "$r" = formal/r4-coordinator/ ] || root_but_formal="${root_but_formal}root$tab$r$nl"
  done <"$work/roots.txt"
  e_acute=$(printf '\303\251') || broken "writing a byte outside ASCII"
  pats="U0.1${tab}tools/r4-page/**
U1.1${tab}crates/halvo-coord/src/types/**
U1.4${tab}docs/r4/README.md
U1.4${tab}docs/r4/log-*
U2.1${tab}crates/halvo-coord/tests/schedule_*.rs
U4.1${tab}crates/halvo-coord-app/**
U4.2${tab}crates/halvo-coord-app/src/present/**
U1.4${tab}scripts/r4/layout.txt
"
  good="# a comment line
#${tab}a comment with${tab}tabs
$root_lines$pats"
  layout_set "$good"
  sum='layout patterns=8 files=16 outside=0 empty=-'
  run good
  judge good PASS "$sum"
  layout_set "$good"
  run good-all --all
  judge good-all PASS 'layout patterns=8 files=16 outside=0 empty=0'

  mv "$fx/scripts/r4/layout.txt" "$work/moved-layout.txt" || broken "moving the layout away"
  out=$(CI_D_REPO=$fx CI_D_ARTIFACTS=$work/art/rerun sh "$self" 2>&1)
  layout_set "$good"
  out=$(CI_D_REPO=$fx CI_D_ARTIFACTS=$work/art/rerun sh "$self" 2>&1)
  code=$?
  case $out in
    *"layout fault"*) code=stale ;;
  esac
  judge rerun-into-a-used-directory PASS "$sum"

  grep -v "${tab}scripts/r4/layout.txt\$" "$work/good.txt" >"$work/untracked.txt" || broken "dropping the layout entry"
  index_set "$fx" "$work/untracked.txt"
  run layout-untracked
  judge layout-untracked 'FAIL layout' 'layout fault=untracked line=-'

  : >"$work/every-root.txt" || broken "writing the every-root list"
  set --
  while IFS= read -r r; do
    entries "${r}stray-x" >>"$work/every-root.txt" || broken "writing the every-root list"
    set -- "$@" "layout outside=${r}stray-x$nl"
  done <"$work/roots.txt"
  entries scripts/r4/layout.txt >>"$work/every-root.txt" || broken "writing the every-root list"
  index_set "$fx" "$work/every-root.txt"
  layout_set "${root_lines}U1.4${tab}scripts/r4/layout.txt$nl"
  run stray-under-every-root
  judge stray-under-every-root 'FAIL outside' "$@" 'files=14 outside=13'

  with_stray docs/r4/010
  layout_set "${good}U1.4${tab}docs/r4/10$nl"
  run stray-a-number-in-another-form
  judge stray-a-number-in-another-form 'FAIL outside' "layout outside=docs/r4/010$nl" 'files=17 outside=1'

  with_stray '"docs/r4/a\"b.md"' '"docs/r4/x\t"'
  layout_set "${good}U1.4${tab}docs/r4/a-b.md${nl}U1.4${tab}docs/r4/a.b.md${nl}U1.4${tab}docs/r4/a_b.md${nl}U1.4${tab}docs/r4/a/b.md${nl}U1.4${tab}docs/r4/aab.md${nl}U1.4${tab}docs/r4/a0b.md${nl}U1.4${tab}docs/r4/x*t$nl"
  run stray-letter-escapes-are-unknown-bytes
  judge stray-letter-escapes-are-unknown-bytes 'FAIL outside' "layout outside=\"docs/r4/a\\\"b.md\"$nl" \
    "layout outside=\"docs/r4/x\\t\"$nl" 'files=18 outside=2'
  layout_set "$good"

  set -- literal-pattern-as-a-prefix docs/r4/README.md.bak path-shorter-than-a-literal docs/r4/README.m \
    star-crosses-a-slash crates/halvo-coord/tests/schedule_a/b.rs star-stops-before-the-end crates/halvo-coord/tests/schedule_a.rsx \
    pattern-as-a-suffix crates/halvo-coord/tests/xschedule_a.rs directory-pattern-as-a-prefix crates/halvo-coord/src/typesx/a.rs \
    root-in-another-case Docs/r4/README.md no-pattern-under-a-root ui/tests/r4/a.js \
    a-file-below-a-file-pattern crates/halvo-coord/tests/schedule_a.rs/x.rs
  while [ "$#" -ge 2 ]; do
    with_stray "$2"
    run "stray-$1"
    judge "stray-$1" 'FAIL outside' "layout outside=$2$nl" 'files=17 outside=1'
    shift 2
  done

  with_stray scripts/r4/words
  layout_set "${good}U1.3${tab}scripts/r4/words/**$nl"
  run stray-a-file-named-as-a-directory-pattern
  judge stray-a-file-named-as-a-directory-pattern 'FAIL outside' "layout outside=scripts/r4/words$nl" 'files=17 outside=1'

  with_stray docs/r4/trials/x.md
  layout_set "${good}U0.2${tab}docs/r4/trials/$nl"
  run stray-a-pattern-that-ends-in-a-slash
  judge stray-a-pattern-that-ends-in-a-slash 'FAIL outside' "layout outside=docs/r4/trials/x.md$nl" 'files=17 outside=1'

  with_stray docs/r4/trials
  layout_set "${good}U0.2${tab}docs/r4/trials/*$nl"
  run stray-a-file-one-segment-above-a-star-pattern
  judge stray-a-file-one-segment-above-a-star-pattern 'FAIL outside' "layout outside=docs/r4/trials$nl" 'files=17 outside=1'
  layout_set "${good}U0.2${tab}docs/r4/trials/$nl"
  run stray-a-file-one-segment-above-a-slash-pattern
  judge stray-a-file-one-segment-above-a-slash-pattern 'FAIL outside' "layout outside=docs/r4/trials$nl" 'files=17 outside=1'

  with_stray '"docs/r4/\303\251.md"'
  layout_set "$good"
  run stray-quoted-name-printed-quoted
  judge stray-quoted-name-printed-quoted 'FAIL outside' "layout outside=\"docs/r4/\\303\\251.md\"$nl" 'files=17 outside=1'

  with_stray '"docs/r4/x\003"'
  layout_set "${good}U1.4${tab}docs/r4/x*3${nl}U1.4${tab}docs/r4/x$nl"
  run stray-quoted-byte-is-one-unknown-byte
  judge stray-quoted-byte-is-one-unknown-byte 'FAIL outside' "layout outside=\"docs/r4/x\\003\"$nl" 'files=17 outside=1'

  index_set "$fx" "$work/good.txt"
  mv "$fx/scripts/r4/layout.txt" "$work/moved-layout.txt" || broken "moving the layout away"
  run layout-missing
  judge layout-missing 'FAIL layout' 'layout fault=missing line=-'

  layout_set "$good"
  printf 'U1.1\tcrates/halvo-coord/src/lib.rs\r\n' >>"$fx/scripts/r4/layout.txt" || broken "writing a CR line"
  run layout-carriage-return
  judge layout-carriage-return 'FAIL layout' 'layout fault=carriage-return line=-'

  layout_set "$good"
  printf 'U1.1\tcrates/halvo-coord/src/lib.rs\000x\n' >>"$fx/scripts/r4/layout.txt" || broken "writing a NUL line"
  run layout-nul
  judge layout-nul 'FAIL layout' 'layout fault=nul line=-'

  layout_set "${good}U1.1 crates/halvo-coord/src/lib.rs${nl}U1.1${tab}crates/halvo-coord/src/lib.rs${tab}x$nl${nl}"
  run layout-form
  judge layout-form 'FAIL layout' 'layout fault=form line=24' 'layout fault=form line=25' 'layout fault=form line=26'

  layout_set "${good}root${tab}docs/r5/${nl}root${tab}docs/r4$nl"
  run layout-unknown-root
  judge layout-unknown-root 'FAIL layout' 'layout fault=root line=24' 'layout fault=root line=25'

  layout_set "${good}root${tab}docs/r4/$nl"
  run layout-root-twice
  judge layout-root-twice 'FAIL layout' 'layout fault=root-twice line=24'

  layout_set "${good%"$root_lines$pats"}$root_but_formal$pats"
  run layout-root-missing
  judge layout-root-missing 'FAIL layout' 'layout fault=root-missing root=formal/r4-coordinator/ line=-'

  layout_set "${good}xU1.1${tab}docs/r4/a.md${nl}U1.1a${tab}docs/r4/b.md${nl}U1x1${tab}docs/r4/c.md${nl}U1.${tab}docs/r4/d.md${nl}U.1${tab}docs/r4/e.md$nl"
  run layout-unit
  judge layout-unit 'FAIL layout' 'layout fault=unit line=24' 'layout fault=unit line=25' 'layout fault=unit line=26' \
    'layout fault=unit line=27' 'layout fault=unit line=28'

  rest='?[] "\+:!,~@=%()$&;^{}|<>`#'"'"
  lines=
  n=24
  set --
  while [ -n "$rest" ]; do
    c=${rest%"${rest#?}"}
    rest=${rest#?}
    lines="${lines}U1.1${tab}docs/r4/a${c}b.md$nl"
    set -- "$@" "layout fault=byte line=$n$nl"
    n=$((n + 1))
  done
  layout_set "$good${lines}U1.1${tab}docs/r4/caf$e_acute.md$nl"
  run layout-byte
  judge layout-byte 'FAIL layout' "$@" "layout fault=byte line=$n$nl"

  layout_set "${good}U1.1${tab}docs/r5/a.md${nl}U1.1${tab}Docs/r4/a.md${nl}U1.1${tab}docs/r4$nl"
  run layout-under-no-root
  judge layout-under-no-root 'FAIL layout' 'layout fault=under-no-root line=24' 'layout fault=under-no-root line=25' \
    'layout fault=under-no-root line=26'

  layout_set "${good}U1.1${tab}docs/r4/**/a.md${nl}U1.1${tab}docs/r4/a**${nl}U1.1${tab}docs/r4/***${nl}U1.1${tab}docs/r4/x/**/**$nl"
  run layout-stars
  judge layout-stars 'FAIL layout' 'layout fault=stars line=24' 'layout fault=stars line=25' 'layout fault=stars line=26' \
    'layout fault=stars line=27'

  layout_set "${good}U3.1${tab}docs/r4/log-*$nl"
  run layout-pattern-twice
  judge layout-pattern-twice 'FAIL layout' 'layout fault=pattern-twice line=24'

  with_stray docs/r4/stray.md
  layout_set "${good}U1.1${tab}docs/r5/a.md$nl"
  run cause-layout-before-outside
  judge cause-layout-before-outside 'FAIL layout' 'layout outside=docs/r4/stray.md'

  layout_set "${good}U8.1${tab}docs/r4/gate.md$nl"
  run cause-outside-before-empty --all
  judge cause-outside-before-empty 'FAIL outside' 'layout empty=docs/r4/gate.md' 'outside=1 empty=1'

  index_set "$fx" "$work/good.txt"
  run empty --all
  judge empty 'FAIL empty' "layout empty=docs/r4/gate.md$nl" 'layout patterns=9 files=16 outside=0 empty=1'
  run empty-without-all
  judge empty-without-all PASS 'layout patterns=9 files=16 outside=0 empty=-'

  layout_set "$good"
  run usage-unknown-argument --bogus
  judge usage-unknown-argument 'FAIL usage'
  run usage-two-arguments --all --all
  judge usage-two-arguments 'FAIL usage'

  mkdir -p "$fx/docs" "$work/plain" || broken "mkdir a directory"
  out=$(CI_D_REPO=$fx/docs CI_D_ARTIFACTS=$work/art/subdirectory sh "$self" 2>&1)
  code=$?
  judge setup-repository-is-a-subdirectory 'FAIL setup' 'layout setup=repository'
  out=$(CI_D_REPO=$work/plain CI_D_ARTIFACTS=$work/art/plain sh "$self" 2>&1)
  code=$?
  judge setup-not-a-repository 'FAIL setup' 'layout setup=repository'

  with_stray docs/r4/stray.md
  set -- git repository awk awk tr tr mkdir artifacts
  while [ "$#" -ge 2 ]; do
    stub "$1" 127
    out=$(PATH=$work/stubs/$1:$PATH CI_D_REPO=$fx CI_D_ARTIFACTS=$work/art/stub-$1 sh "$self" 2>&1)
    code=$?
    judge "setup-missing-$1" 'FAIL setup' "layout setup=$2"
    shift 2
  done
  real_tr=$(command -v tr) || broken "finding tr"
  set -- cr '\r' nul '\000' clean '\r\000'
  while [ "$#" -ge 2 ]; do
    mkdir -p "$work/stubs/tr-$1" || broken "mkdir stubs"
    printf "#!/bin/sh\n[ \"\$2\" = '%s' ] && exit 1\nexec \"%s\" \"\$@\"\n" "$2" "$real_tr" >"$work/stubs/tr-$1/tr" ||
      broken "writing the tr stub"
    chmod 755 "$work/stubs/tr-$1/tr" || broken "chmod the tr stub"
    out=$(PATH=$work/stubs/tr-$1:$PATH CI_D_REPO=$fx CI_D_ARTIFACTS=$work/art/tr-$1-fails sh "$self" 2>&1)
    code=$?
    judge "setup-tr-$1-fails" 'FAIL setup' 'layout setup=tr'
    shift 2
  done
  real_git=$(command -v git) || broken "finding git"
  mkdir -p "$work/stubs/ls-files" || broken "mkdir stubs"
  printf '#!/bin/sh\ncase " $* " in *" ls-files "*) exit 128 ;; esac\nexec "%s" "$@"\n' "$real_git" >"$work/stubs/ls-files/git" ||
    broken "writing the ls-files stub"
  chmod 755 "$work/stubs/ls-files/git" || broken "chmod the ls-files stub"
  out=$(PATH=$work/stubs/ls-files:$PATH CI_D_REPO=$fx CI_D_ARTIFACTS=$work/art/ls-files-fails sh "$self" 2>&1)
  code=$?
  judge setup-ls-files-fails 'FAIL setup' 'layout setup=git'
  mkdir -p "$work/stubs/silent-git" "$work/stubs/silent-ls-files" || broken "mkdir stubs"
  printf '#!/bin/sh\nexit 0\n' >"$work/stubs/silent-git/git" || broken "writing the silent git"
  printf '#!/bin/sh\ncase " $* " in *" ls-files "*) exit 0 ;; esac\nexec "%s" "$@"\n' "$real_git" >"$work/stubs/silent-ls-files/git" ||
    broken "writing the silent ls-files stub"
  chmod 755 "$work/stubs/silent-git/git" "$work/stubs/silent-ls-files/git" || broken "chmod the silent git stubs"
  out=$(PATH=$work/stubs/silent-git:$PATH CI_D_REPO=$fx CI_D_ARTIFACTS=$work/art/silent-git sh "$self" 2>&1)
  code=$?
  judge setup-silent-git 'FAIL setup' 'layout setup=repository'
  out=$(PATH=$work/stubs/silent-ls-files:$PATH CI_D_REPO=$fx CI_D_ARTIFACTS=$work/art/silent-ls-files sh "$self" 2>&1)
  code=$?
  judge layout-silent-ls-files 'FAIL layout' 'layout fault=untracked line=-'
  mkdir -p "$work/stubs/silent" || broken "mkdir stubs"
  printf '#!/bin/sh\nexit 0\n' >"$work/stubs/silent/awk" || broken "writing the silent awk"
  chmod 755 "$work/stubs/silent/awk" || broken "chmod the silent awk"
  out=$(PATH=$work/stubs/silent:$PATH CI_D_REPO=$fx CI_D_ARTIFACTS=$work/art/silent-awk sh "$self" 2>&1)
  code=$?
  judge setup-silent-awk 'FAIL setup' 'layout setup=report'

  out=$(GIT_INDEX_FILE=$work/no-index CI_D_REPO=$fx CI_D_ARTIFACTS=$work/art/git-index-file sh "$self" 2>&1)
  code=$?
  judge env-git-index-file-is-ignored 'FAIL outside' 'layout outside=docs/r4/stray.md'
  out=$(GIT_DIR=$work/other/.git CI_D_REPO=$fx CI_D_ARTIFACTS=$work/art/git-dir sh "$self" 2>&1)
  code=$?
  judge env-git-dir-is-ignored 'FAIL outside' 'layout outside=docs/r4/stray.md'
  out=$(GIT_WORK_TREE=$work/other CI_D_REPO=$fx CI_D_ARTIFACTS=$work/art/git-work-tree sh "$self" 2>&1)
  code=$?
  judge env-git-work-tree-is-ignored 'FAIL outside' 'layout outside=docs/r4/stray.md'

  cp "$self" "$fx/scripts/r4/ci.d/$name.sh" || broken "copying the script"
  out=$(cd "$work" && unset CI_D_REPO CI_D_ARTIFACTS && sh "$fx/scripts/r4/ci.d/$name.sh" 2>&1)
  code=$?
  if [ -f "$fx/target/ci-artifacts/$name/report.txt" ]; then
    out="artifacts=made$nl$out"
  fi
  judge without-the-driver 'FAIL outside' 'layout outside=docs/r4/stray.md' 'artifacts=made'

  out=$(cd "$work" && CI_D_REPO='' CI_D_ARTIFACTS='' sh "$fx/scripts/r4/ci.d/$name.sh" 2>&1)
  code=$?
  judge empty-driver-variables 'FAIL outside' 'layout outside=docs/r4/stray.md'

  mkdir -p "$work/norepo/scripts/r4/ci.d" || broken "mkdir norepo"
  cp "$self" "$work/norepo/scripts/r4/ci.d/$name.sh" || broken "copying the script"
  out=$(cd "$fx" && unset CI_D_REPO CI_D_ARTIFACTS && sh "$work/norepo/scripts/r4/ci.d/$name.sh" 2>&1)
  code=$?
  judge setup-script-outside-a-repository 'FAIL setup' 'layout setup=repository'

  mkdir -p "$work/evil/rel-cdpath" || broken "mkdir evil"
  for f in faults.txt roots.txt layout-clean.txt tracked.txt; do
    : >"$work/evil/rel-cdpath/$f" || broken "planting $f"
  done
  out=$(cd "$work" && CDPATH=$work/evil CI_D_REPO=$fx CI_D_ARTIFACTS=rel-cdpath sh "$self" 2>&1)
  code=$?
  judge env-cdpath-is-ignored 'FAIL outside' 'layout outside=docs/r4/stray.md'

  out=$(cd "$work" && CI_D_REPO=$fx CI_D_ARTIFACTS=-art sh "$self" 2>&1)
  code=$?
  judge artifacts-path-with-a-leading-dash 'FAIL outside' 'layout outside=docs/r4/stray.md'

  if [ -n "$wrong" ]; then
    printf 'self-test wrong verdict in:%s\n' "$wrong"
    printf 'FAIL self-test:%s (wrong)\n' "$name"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

all=0
case $# in
  0) measure ;;
  1)
    case $1 in
      --all)
        all=1
        measure
        ;;
      --self-test) self_test ;;
      *) usage ;;
    esac
    ;;
  *) usage ;;
esac
