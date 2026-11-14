#!/bin/sh
# jobs: macos windows
# rule: Admit.cfg and Small.cfg of formal/r5-admit have the one fixed form of the model's bound and formulas, TLC finds none of Q1 to Q14 violated at the small bound, the model exports the committed table and traces byte for byte, each of two control copies breaks its property, and record.txt holds the full bound, the small run's count of states and every run of runs.txt at the stamp of the model's files.
# lane: free
set -u
LC_ALL=C
export LC_ALL
unset CDPATH JAVA_TOOL_OPTIONS JDK_JAVA_OPTIONS _JAVA_OPTIONS
name=30-r5-formal
label=$name
workers=3
art=
detail=-
case $0 in
  /*) self=$0 ;;
  *) self=$(pwd)/$0 ;;
esac

check() {
  shift
  "$@"
}

advice() {
  printf 'cause: %s\n' "$1"
  case $1 in
    model) printf 'read: formal/r5-admit/README.md\ndo: formal/r5-admit/ is missing; restore it from main.\n' ;;
    scratch) printf 'read: the variable TMPDIR\ndo: mktemp -d made no directory under TMPDIR, or /tmp when it is not set; point TMPDIR at a writable directory.\n' ;;
    attributes) printf 'read: formal/r5-admit/.gitattributes, formal/r3-confirm/.gitattributes, and attributes.txt in this directory, which lists each file that git does not take as text with eol=lf\ndo: keep the one line "* text eol=lf" in each of the two files.\n' ;;
    pin) printf 'read: formal/r5-admit/README.md, section The checker, and the tla2tools.jar line of formal/r3-confirm/README.md\ndo: give each README one jar line, both with the same address, size and SHA-256.\n' ;;
    java) printf 'read: formal/r5-admit/README.md, section The checker\ndo: install java 17 or later, or set JAVA_HOME to one.\n' ;;
    fetch) printf 'read: formal/r5-admit/README.md, section The checker\ndo: check the address of the jar line and the network, or name the jar in TLA2TOOLS_JAR.\n' ;;
    hash) printf 'read: formal/r5-admit/README.md, section The checker\ndo: the SHA-256 of the jar is not the pinned one; take the jar that the line pins.\n' ;;
    invariant)
      printf 'read: formal/r5-admit/README.md, sections The bounds of Admit.cfg, The properties and Running TLC; small.txt (the TLC output), admit-want.cfg and small-want.cfg in this directory\n'
      printf 'do: result=%s. admit-form or small-form: make Admit.cfg or Small.cfg equal to admit-want.cfg or small-want.cfg (a change of the bound is a Revision of item L12). formulas-form: each line of Admit.tla whose first word, after leading blanks or LOCAL, is Q and a digit must be the definition, in the first column, of one of the 18 formulas that 30-r5-formal.sh lists (Q1 to Q14, Q7Step, Q11Step, Q12Step, Q12Move), as "Q7Step == ...". Word a comment so that no line of it starts with such a name, and put another token before a formula that an indented line uses, such as the conjunction sign. A new formula goes into the list of 30-r5-formal.sh and into Admit.cfg and Small.cfg in the same change. violated, status or incomplete: read small.txt and replay its trace against the code and the contract.\n' "$detail"
      ;;
    table | traces) printf 'read: formal/r5-admit/README.md, sections edit-table.tsv, traces.jsonl and export.sh; export.txt and the directory export/ here hold what the model exports now\ndo: run sh formal/r5-admit/export.sh and commit both files; never edit them by hand.\n' ;;
    control) printf 'read: formal/r5-admit/README.md, sections The wrong designs and Running TLC; control-*.txt in this directory\ndo: the copy with Fault serve_reverted must report Q1 and the copy with Fault no_bar must report Q5.\n' ;;
    record) printf 'read: formal/r5-admit/README.md, sections The record and Writing the record\ndo: the record must name the model'"'"'s files, the small run and runs.txt as they are now; write it again with sh formal/r5-admit/record.sh: list, then run for each line it prints, then write.\n' ;;
    selftest) printf 'read: selftest-*.txt in this directory\ndo: a fixture of the self-test did not end as it should.\n' ;;
    usage) printf 'do: the arguments are none or --self-test.\n' ;;
  esac
  printf 'run again: sh scripts/ci/run-ci-d.sh %s %s\n' "${CI_D_JOB:-macos}" "$name"
}

fail() {
  printf 'FAIL %s (%s)\n' "$label" "$1"
  [ -z "$art" ] || advice "$1" >"$art/fix.txt" 2>/dev/null
  exit 1
}

made=
cleanup() {
  case $made in
    */r5-formal.??????) rm -rf "$made" ;;
  esac
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum
  else
    shasum -a 256
  fi
}

number() {
  case $1 in
    '' | *[!0-9]*) return 1 ;;
  esac
}

locate() {
  if [ -n "${CI_D_REPO:-}" ]; then
    repo=$CI_D_REPO
  else
    repo=$(cd "${self%/*}/../../.." && pwd) || repo=
  fi
  dir=$repo/formal/r5-admit
  art_dir=${CI_D_ARTIFACTS:-$repo/target/ci-artifacts/$name}
}

scratch() {
  tmp=$(mktemp -d "${TMPDIR:-/tmp}/r5-formal.XXXXXX" 2>/dev/null) || tmp=
  made=$tmp
  [ -z "$tmp" ] || tmp=$(cd "$tmp" && pwd) || tmp=
  [ -z "$tmp" ] || made=$tmp
}

since() {
  now=$(date +%s)
  seconds=$((now - start))
  start=$now
}

other_form() {
  [ "$other_lines" -eq 0 ] || [ "$other_lines:$other_read" = 1:1 ]
}

other_same() {
  [ "$other_lines" -eq 0 ] || [ "$other" = "$own" ]
}

pin() {
  : >"$tmp/own.txt"
  : >"$tmp/other.txt"
  [ ! -f "$dir/README.md" ] || grep '^jar ' "$dir/README.md" >"$tmp/own.txt"
  [ ! -f "$repo/formal/r3-confirm/README.md" ] || grep '^tla2tools\.jar ' "$repo/formal/r3-confirm/README.md" >"$tmp/other.txt"
  sed -n 's/^jar url=\([^ ]*\) bytes=\([0-9][0-9]*\) sha256=\([0-9a-f]\{64\}\)$/\1 \2 \3/p' "$tmp/own.txt" >"$tmp/own-read.txt"
  sed -n 's/^tla2tools\.jar release=[^ ]* url=\([^ ]*\) bytes=\([0-9][0-9]*\) sha256=\([0-9a-f]\{64\}\)$/\1 \2 \3/p' \
    "$tmp/other.txt" >"$tmp/other-read.txt"
  own_lines=$(grep -c '' "$tmp/own.txt")
  own_read=$(grep -c '' "$tmp/own-read.txt")
  other_lines=$(grep -c '' "$tmp/other.txt")
  other_read=$(grep -c '' "$tmp/other-read.txt")
  own=
  read -r own <"$tmp/own-read.txt" || own=
  other=
  read -r other <"$tmp/other-read.txt" || other=
  state=same
  check pin-alone [ "$other_lines" -ne 0 ] || state=alone
  check pin-other-form other_form || state=differ
  check pin-other-same other_same || state=differ
  check pin-own [ "$own_lines:$own_read" = 1:1 ] || state=differ
  printf 'formal pin=%s\n' "$state"
  check pin-differ [ "$state" != differ ] || fail pin
  url=${own%% *}
  want=${own##* }
}

attributes() {
  (cd "$repo" && git -c core.quotepath=off ls-files -- 'formal/r5-admit/*.tla' 'formal/r5-admit/*.cfg') \
    >"$tmp/attr-files.txt" 2>/dev/null
  printf '%s\n' formal/r5-admit/edit-table.tsv formal/r5-admit/traces.jsonl formal/r5-admit/record.txt \
    formal/r5-admit/runs.txt formal/r5-admit/README.md >>"$tmp/attr-files.txt"
  [ ! -f "$repo/formal/r3-confirm/README.md" ] || printf 'formal/r3-confirm/README.md\n' >>"$tmp/attr-files.txt"
  (cd "$repo" && git check-attr --stdin text eol <"$tmp/attr-files.txt") >"$tmp/attr.txt" 2>/dev/null
  awk -F': ' -v bad="$art/attributes.txt" '
    NR == FNR { want[$0] = 1; n++; next }
    $2 == "text" && $3 == "set" { t[$1] = 1 }
    $2 == "eol" && $3 == "lf" { e[$1] = 1 }
    END {
      printf "" >bad
      for (f in want) if ((f in t) && (f in e)) good++; else print f >>bad
      printf "%d %d\n", n, good
    }' "$tmp/attr-files.txt" "$tmp/attr.txt" >"$tmp/attr-count.txt" || fail attributes
  files=0
  lf=-1
  read -r files lf <"$tmp/attr-count.txt" || fail attributes
  printf 'formal attributes files=%s lf=%s\n' "$files" "$lf"
  check attributes [ "$files" -eq "$lf" ] || fail attributes
}

java_of() {
  if [ -n "${JAVA_HOME:-}" ]; then
    if command -v cygpath >/dev/null 2>&1; then
      JAVA_HOME=$(cygpath -u "$JAVA_HOME") || fail java
      export JAVA_HOME
    fi
    java=$JAVA_HOME/bin/java
  else
    java=java
  fi
  "$java" -version >"$tmp/java.raw" 2>&1 </dev/null
  tr -d '\r' <"$tmp/java.raw" >"$tmp/java.txt" || fail java
  sed -n 's/.* version "\([0-9][0-9]*\).*/\1/p' "$tmp/java.txt" >"$tmp/major.txt" || fail java
  major=none
  read -r major <"$tmp/major.txt" || major=none
  printf 'formal java major=%s\n' "$major"
  check java-17 java_new || fail java
}

java_new() {
  number "$major" && [ "$major" -ge 17 ]
}

fetch_jar() {
  command -v curl >/dev/null 2>&1 &&
    (cd "$tmp" && curl -q -fsSL --retry 2 --max-time 300 -o tla2tools.jar "$url") &&
    [ -f "$tmp/tla2tools.jar" ]
}

jar_missing() {
  printf 'formal jar source=%s bytes=0 same=no\n' "$source"
  fail fetch
}

jar() {
  if [ -n "${TLA2TOOLS_JAR:-}" ]; then
    source=given
    check jar-given cp "$TLA2TOOLS_JAR" "$tmp/tla2tools.jar" 2>/dev/null || jar_missing
  else
    source=fetch
    check jar-fetch fetch_jar || jar_missing
  fi
  size=$(wc -c <"$tmp/tla2tools.jar") || fail hash
  size=$((size + 0))
  sum=$(sha256 <"$tmp/tla2tools.jar") || fail hash
  sum=${sum%% *}
  same=yes
  check jar-sum [ "$sum" = "$want" ] || same=no
  printf 'formal jar source=%s bytes=%s same=%s\n' "$source" "$size" "$same"
  check jar-same [ "$same" = yes ] || fail hash
}

tlc() {
  (
    cd "$1" || exit 99
    "$java" -Xmx2g -XX:+UseParallelGC -Djava.io.tmpdir=jtmp -cp ../tla2tools.jar tlc2.TLC \
      -workers "$workers" -metadir meta -config "$2" Admit >out.raw 2>&1 </dev/null
  )
  status=$?
  tr -d '\r' <"$1/out.raw" >"$1/out.txt" || status=99
  cp "$1/out.txt" "$art/${1##*/}.txt" || status=99
  awk '
    /^Error: (Invariant|Action property) [A-Za-z0-9_]+ is violated/ {
      n = ($2 == "Invariant") ? $3 : $4
      print "violated " n
    }
    /^[0-9]+ states generated, [0-9]+ distinct states found, 0 states left on queue\.$/ { print "distinct " $4 }
    $0 == "Model checking completed. No error has been found." { print "complete" }
  ' "$1/out.txt" >"$1/facts.txt" || status=99
}

formulas() {
  printf '%s\n' Q1 Q2 Q3 Q4 Q5 Q6 Q7 Q7Step Q8 Q9 Q10 Q11 Q11Step Q12 Q12Step Q12Move Q13 Q14 >"$tmp/names.txt" ||
    fail invariant
  sed -E 's/^Q([0-9]+).*$/\1/' "$tmp/names.txt" | sort -u >"$tmp/numbers.txt" || fail invariant
  numbers=$(grep -c '' "$tmp/numbers.txt")
  : >"$tmp/q-lines.txt"
  [ ! -f "$dir/Admit.tla" ] || grep -E '^[[:blank:]]*(LOCAL[[:blank:]]+)?Q[0-9]' "$dir/Admit.tla" >"$tmp/q-lines.txt"
  odd_q=$(awk -v list="$tmp/names.txt" '
    BEGIN { while ((getline n < list) > 0) ok[n] = 1 }
    !($0 ~ /^Q[0-9]+[A-Za-z]* ==( |$)/ && ($1 in ok)) { odd++ }
    END { print odd + 0 }' "$tmp/q-lines.txt")
}

cfg_text() {
  printf 'CONSTANTS\n'
  printf '    Slots = {r1, r2}\n    FurtherA = {a1, a2, a3}\n    FurtherB = {b1, b2, b3}\n    Compiled = {c1, c2}\n'
  printf '    ListMax = 2\n    EditMax = 3\n    RunsMax = %s\n    RunsOnBothSteps = %s\n    Fault = "%s"\n' "$1" "$2" "$3"
  printf 'SYMMETRY Symmetry\nCHECK_DEADLOCK FALSE\nCONSTRAINT Bound\nINIT Init\nNEXT Next\nPROPERTIES\n'
  sed 's/^/    /' "$4"
}

miss() {
  [ "$result" != ok ] || result=$1
}

tracked() {
  (cd "$repo" && git -c core.quotepath=off ls-files -- 'formal/r5-admit/*.tla') >"$tmp/tracked-tla.txt" 2>/dev/null ||
    : >"$tmp/tracked-tla.txt"
}

copy_model() {
  while IFS= read -r f; do
    cp "$repo/$f" "$1/" || return 1
  done <"$tmp/tracked-tla.txt"
}

small() {
  result=ok
  formulas
  cfg_text 5 3 none "$tmp/names.txt" >"$tmp/admit-want.cfg" || fail invariant
  cfg_text 3 2 none "$tmp/names.txt" >"$tmp/small-want.cfg" || fail invariant
  cp "$tmp/admit-want.cfg" "$tmp/small-want.cfg" "$art/" || fail invariant
  check admit-form cmp -s "$tmp/admit-want.cfg" "$dir/Admit.cfg" || miss admit-form
  check small-form cmp -s "$tmp/small-want.cfg" "$dir/Small.cfg" || miss small-form
  check formulas-form [ "$odd_q" -eq 0 ] || miss formulas-form
  distinct=0
  violated=0
  if [ "$result" = ok ]; then
    run=$tmp/small
    mkdir "$run" "$run/jtmp" || fail scratch
    copy_model "$run" || fail invariant
    cp "$dir/Small.cfg" "$run/" || fail invariant
    tlc "$run" Small.cfg
    sed -n 's/^violated //p' "$run/facts.txt" | sort -u >"$tmp/violated.txt" || fail invariant
    violated=$(grep -c '' "$tmp/violated.txt")
    sed -n 's/^distinct //p' "$run/facts.txt" >"$tmp/distinct.txt" || fail invariant
    read -r distinct <"$tmp/distinct.txt" || distinct=0
    complete=$(grep -c '^complete$' "$run/facts.txt")
    check tlc-violated [ "$violated" -eq 0 ] || miss violated
    check tlc-status [ "$status" -eq 0 ] || miss status
    check tlc-complete [ "$complete" -eq 1 ] || miss incomplete
  fi
  printf 'formal states=%s properties=%s violated=%s\n' "$distinct" "$numbers" "$violated"
  [ "$result" = ok ] || printf 'formal small result=%s\n' "$result"
  detail=$result
  check small-ok [ "$result" = ok ] || fail invariant
}

exported() {
  mkdir -p "$art/export" || fail invariant
  : >"$art/export/edit-table.tsv"
  : >"$art/export/traces.jsonl"
  mkdir "$tmp/export-tmp" || fail scratch
  TLA2TOOLS_JAR=$tmp/tla2tools.jar TMPDIR=$tmp/export-tmp sh "$dir/export.sh" --out "$art/export" \
    >"$art/export.txt" 2>&1 </dev/null
  code=$?
  rows=0
  [ ! -f "$dir/edit-table.tsv" ] || rows=$(grep -c '' "$dir/edit-table.tsv")
  lines=0
  [ ! -f "$dir/traces.jsonl" ] || lines=$(grep -c '' "$dir/traces.jsonl")
  count=$((lines - 1))
  [ "$count" -ge 0 ] || count=0
  table=yes
  check export-status [ "$code" -eq 0 ] || table=no
  check table-rows [ "$rows" -eq 57 ] || table=no
  check table-same cmp -s "$dir/edit-table.tsv" "$art/export/edit-table.tsv" || table=no
  traces=yes
  check traces-same cmp -s "$dir/traces.jsonl" "$art/export/traces.jsonl" || traces=no
  [ "$code" -eq 0 ] || printf 'formal export status=%s\n' "$code"
  printf 'formal table rows=%s same=%s\n' "$rows" "$table"
  printf 'formal traces count=%s same=%s\n' "$count" "$traces"
  check table-ok [ "$table" = yes ] || fail table
  check traces-ok [ "$traces" = yes ] || fail traces
}

control() {
  run=$tmp/control-$1
  mkdir "$run" "$run/jtmp" || return 2
  copy_model "$run" || return 2
  printf '%s\n' "$2" >"$run/property.txt" || return 2
  cfg_text 5 3 "$1" "$run/property.txt" >"$run/Control.cfg" || return 2
  tlc "$run" Control.cfg
  sed -n 's/^violated //p' "$run/facts.txt" >"$run/violated.txt" || return 2
  first=
  read -r first <"$run/violated.txt" || first=
  check control-status [ "$status" -ne 0 ] || return 1
  check control-property [ "$first" = "$2" ] || return 1
}

controls() {
  broken=0
  for copy in serve_reverted:Q1 no_bar:Q5; do
    if control "${copy%:*}" "${copy#*:}"; then
      broken=$((broken + 1))
    fi
  done
  printf 'formal control violated=%s\n' "$broken"
  check control-count [ "$broken" -eq 2 ] || fail control
}

stamp_of() {
  stamp=
  (cd "$1" && git -c core.quotepath=off ls-files -- 'formal/r5-admit/*.tla' 'formal/r5-admit/*.cfg') \
    >"$2.files" 2>/dev/null || return 1
  sort "$2.files" >"$2.sorted" || return 1
  : >"$2.list" || return 1
  while IFS= read -r f; do
    one=$(sha256 <"$1/$f") || return 1
    printf '%s  %s\n' "${one%% *}" "$f" >>"$2.list" || return 1
  done <"$2.sorted"
  stamp=$(sha256 <"$2.list") || return 1
  stamp=${stamp%% *}
}

runs_miss() {
  [ "$runs_state" != ok ] || runs_state=$1
}

stamp_miss() {
  [ "$stamped" != same ] || stamped=$1
}

lines_miss() {
  [ "$lines" != ok ] || lines=$1
}

runs_read() {
  runs_state=ok
  : >"$tmp/runs.txt"
  [ ! -f "$dir/runs.txt" ] || cp "$dir/runs.txt" "$tmp/runs.txt" || fail record
  runs_odd=$(grep -c -v -E '^(design [a-z0-9_]+ [1-9] [0-9] [1-9] (Q([1-9]|1[0-4])(Step|Move)?|nothing)|reach (Not|Never)[A-Z][A-Za-z]* [1-9] [0-9])$' "$tmp/runs.txt")
  runs_dup=$(awk '{ k = ($1 == "design") ? $1 " " $2 : $1 " " $2 " " $3 " " $4; if (k in s) d++; s[k] = 1 } END { print d + 0 }' "$tmp/runs.txt")
  grep '^design ' "$tmp/runs.txt" | sort >"$tmp/want-designs.txt"
  grep '^reach ' "$tmp/runs.txt" | sort >"$tmp/want-reach.txt"
  nd=$(grep -c '' "$tmp/want-designs.txt")
  nr=$(grep -c '' "$tmp/want-reach.txt")
  check runs-present [ -f "$dir/runs.txt" ] || runs_miss absent
  check runs-form [ "$runs_odd" -eq 0 ] || runs_miss unreadable
  check runs-once [ "$runs_dup" -eq 0 ] || runs_miss twice
  check runs-designs [ "$nd" -gt 0 ] || runs_miss no-designs
  check runs-reach [ "$nr" -gt 0 ] || runs_miss no-reach
  : >"$tmp/model-designs.txt"
  [ ! -f "$dir/Admit.tla" ] || grep -o -E 'Fault (=|#) "[a-z0-9_]+"|Fault \\(notin|in) \{[^}]*\}' "$dir/Admit.tla" |
    grep -o -E '"[a-z0-9_]+"' | tr -d '"' | grep -v -x none | sort -u >"$tmp/model-designs.txt"
  awk '$1 == "design" { print $2 }' "$tmp/runs.txt" | sort -u >"$tmp/runs-designs.txt"
  : >"$tmp/model-reach.txt"
  [ ! -f "$dir/Reach.tla" ] || sed -n -E 's/^((Not|Never)[A-Za-z]+) ==.*$/\1/p' "$dir/Reach.tla" | sort -u >"$tmp/model-reach.txt"
  awk '$1 == "reach" { print $2 }' "$tmp/runs.txt" | sort -u >"$tmp/runs-reach.txt"
  check runs-designs-model cmp -s "$tmp/model-designs.txt" "$tmp/runs-designs.txt" || runs_miss designs-model
  check runs-reach-model cmp -s "$tmp/model-reach.txt" "$tmp/runs-reach.txt" || runs_miss reach-model
}

record_ok() {
  [ "$stamped:$full:$small_state:$runs_state:$designs_ok:$reach_ok:$lines" = same:ok:same:ok:yes:yes:ok ]
}

record() {
  file=$dir/record.txt
  r=$tmp/record.txt
  runs_read
  stamped=same
  full=ok
  small_state=same
  designs_ok=yes
  reach_ok=yes
  lines=ok
  : >"$r"
  [ ! -f "$file" ] || cp "$file" "$r" || fail record
  n_stamp=$(grep -c '^stamp=' "$r")
  sed -n 's/^stamp=\([0-9a-f]\{64\}\)$/\1/p' "$r" >"$tmp/have-stamp.txt" || fail record
  n_have=$(grep -c '' "$tmp/have-stamp.txt")
  have=
  read -r have <"$tmp/have-stamp.txt" || have=
  stamp_of "$repo" "$tmp/stamp" || stamp=
  check record-present [ -f "$file" ] || stamp_miss absent
  check record-stamp-form [ "$n_stamp:$n_have" = 1:1 ] || stamp_miss none
  check record-stamp [ "$have" = "$stamp" ] || stamp_miss differ
  n_full=$(grep -c '^full ' "$r")
  m_full=$(grep -c -E '^full config=Admit\.cfg states=[1-9][0-9]* properties=14 violated=0 workers=[1-9][0-9]* run=(helper|nice19) seconds=[0-9]+$' "$r")
  check record-full [ "$n_full:$m_full" = 1:1 ] || full=miss
  n_small=$(grep -c '^small ' "$r")
  m_small=$(grep -c -x -F "small config=Small.cfg states=$distinct" "$r")
  check record-small [ "$n_small:$m_small" = 1:1 ] || small_state=differ
  sed -n 's/^design fault=\([a-z0-9_]*\) bound=\([1-9]\)-\([0-9]\)-\([1-9]\) reports=\([A-Za-z0-9]*\) states=[0-9][0-9]*$/design \1 \2 \3 \4 \5/p' \
    "$r" | sort >"$tmp/have-designs.txt"
  sed -n 's/^reach property=\([A-Za-z]*\) bound=\([1-9]\)-\([0-9]\) reached=yes states=[0-9][0-9]*$/reach \1 \2 \3/p' \
    "$r" | sort >"$tmp/have-reach.txt"
  check record-designs cmp -s "$tmp/want-designs.txt" "$tmp/have-designs.txt" || designs_ok=no
  check record-reach cmp -s "$tmp/want-reach.txt" "$tmp/have-reach.txt" || reach_ok=no
  comm -12 "$tmp/want-designs.txt" "$tmp/have-designs.txt" >"$tmp/both-designs.txt" || fail record
  comm -12 "$tmp/want-reach.txt" "$tmp/have-reach.txt" >"$tmp/both-reach.txt" || fail record
  reported=$(grep -c '' "$tmp/both-designs.txt")
  reached=$(grep -c '' "$tmp/both-reach.txt")
  total=$(grep -c '' "$r")
  od -An -tx1 "$r" | tr -s ' ' '\n' >"$tmp/record-bytes.txt" || fail record
  crs=$(grep -c '^0d$' "$tmp/record-bytes.txt")
  check record-cr [ "$crs" -eq 0 ] || lines_miss cr
  check record-lines [ "$total" -eq $((3 + nd + nr)) ] || lines_miss differ
  printf 'formal record stamp=%s full=%s small=%s runs=%s designs=%s reported=%s reach=%s reached=%s lines=%s\n' \
    "$stamped" "$full" "$small_state" "$runs_state" "$nd" "$reported" "$nr" "$reached" "$lines"
  check record-ok record_ok || fail record
}

measure() {
  locate
  check artifacts mkdir -p "$art_dir" 2>/dev/null || fail artifacts
  art=$art_dir
  check model [ -d "$dir" ] || fail model
  scratch
  check scratch [ -n "$tmp" ] || fail scratch
  tracked
  pin
  attributes
  java_of
  jar
  start=$(date +%s)
  small
  since
  t_small=$seconds
  exported
  since
  t_export=$seconds
  controls
  since
  printf 'formal seconds small=%s export=%s control=%s\n' "$t_small" "$t_export" "$seconds"
  record
  printf 'cause: none\n' >"$art/fix.txt" 2>/dev/null
  printf 'PASS %s\n' "$name"
}

stubs() {
  mkdir -p "$work/stub" "$work/jdk/bin" "$work/empty" "$work/stub-curl" || fail selftest
  cat >"$work/stub/java" <<'EOF' || fail selftest
#!/bin/sh
mode=${R5_STUB_MODE:-good}
if [ -n "${JAVA_TOOL_OPTIONS:-}${JDK_JAVA_OPTIONS:-}${_JAVA_OPTIONS:-}" ]; then
  printf 'a java option variable reached java\r\n' >&2
  exit 96
fi
if [ "${1:-}" = -version ]; then
  case $mode in
    java-old) printf 'openjdk version "11.0.2" 2019-01-15\r\n' >&2 ;;
    *) printf 'openjdk version "17.0.1" 2021-10-19\r\n' >&2 ;;
  esac
  exit 0
fi
cfg=
module=
while [ $# -gt 0 ]; do
  [ "$1" = -config ] && cfg=$2
  module=$1
  shift
done
printf '%s %s\n' "$module" "$cfg" >>"$R5_STUB_LOG"
if [ -f FiniteSets.tla ]; then
  printf 'Fatal errors while parsing TLA+ spec in file FiniteSets\r\n'
  exit 150
fi
good() {
  printf 'Model checking completed. No error has been found.\r\n'
  printf '%s states generated, %s distinct states found, 0 states left on queue.\r\n' "$2" "$1"
}
broke() {
  printf 'Error: Action property %s is violated.\r\n' "$1"
  printf '90 states generated, 40 distinct states found, 7 states left on queue.\r\n'
  exit 13
}
if [ "$module" = Export ]; then
  awk '{ gsub(/\t/, " "); printf "\"row %s\"\r\n", $0 }' "$R5_STUB_FROM/edit-table.tsv"
  sed -e 1d -e 's/^{"moves":\[//' -e 's/\]}$//' \
    -e 's/{"move":"\([^"]*\)","guard":"\([^"]*\)","answer":"\([^"]*\)","clean":\([^}]*\)}/\1:\2:\3:\4/g' \
    -e 's/,/ /g' "$R5_STUB_FROM/traces.jsonl" | awk '{ printf "\"trace %d %s\"\r\n", NR, $0 }'
  good 1 1
  exit 0
fi
fault=$(sed -n 's/^    Fault = "\(.*\)"$/\1/p' "$cfg")
want=
[ "$fault" = serve_reverted ] && want=Q1
[ "$fault" = no_bar ] && want=Q5
if [ -n "$want" ]; then
  case $mode in
    control-holds)
      good 9 27
      exit 0
      ;;
    control-other) broke Q3 ;;
    control-holds-"$want")
      good 9 27
      exit 0
      ;;
    control-status)
      printf 'Error: Action property %s is violated.\r\n' "$want"
      exit 0
      ;;
  esac
  broke "$want"
fi
case $mode in
  as-control-q1) broke Q1 ;;
  as-control-q5) broke Q5 ;;
  invariant-q7)
    printf 'Error: Invariant Q7 is violated.\r\n'
    exit 12
    ;;
  incomplete)
    printf '300000 states generated, 15690 distinct states found, 0 states left on queue.\r\n'
    exit 0
    ;;
  status)
    good 15690 300000
    exit 1
    ;;
esac
good 15690 300000
exit 0
EOF
  cat >"$work/stub-curl/curl" <<'EOF' || fail selftest
#!/bin/sh
exit 22
EOF
  cat >"$work/copy-export.sh" <<'EOF' || fail selftest
#!/bin/sh
here=$(cd "$(dirname "$0")" && pwd)
cp "$here/edit-table.tsv" "$here/traces.jsonl" "$2/" || exit 1
exit STATUS
EOF
  cp "$work/stub/java" "$work/jdk/bin/java" || fail selftest
  chmod +x "$work/stub/java" "$work/jdk/bin/java" "$work/stub-curl/curl" || fail selftest
}

write_record() {
  {
    printf 'stamp=%s\n' "$1"
    printf 'full config=Admit.cfg states=1660030 properties=14 violated=0 workers=4 run=nice19 seconds=2193\n'
    printf 'small config=Small.cfg states=15690\n'
    awk '
      $1 == "design" { printf "design fault=%s bound=%s-%s-%s reports=%s states=100\n", $2, $3, $4, $5, $6 }
      $1 == "reach" { printf "reach property=%s bound=%s-%s reached=yes states=10\n", $2, $3, $4 }' "$3"
  } >"$2" || fail selftest
}

base() {
  b=$work/base
  mkdir -p "$b/formal/r3-confirm" || fail selftest
  cp -R "$dir" "$b/formal/" || fail selftest
  printf '* text eol=lf\n' >"$b/formal/r3-confirm/.gitattributes" || fail selftest
  printf 'r5 stub jar\n' >"$work/stub.jar" || fail selftest
  fake_bytes=$(wc -c <"$work/stub.jar") || fail selftest
  fake_bytes=$((fake_bytes + 0))
  fake_sum=$(sha256 <"$work/stub.jar") || fail selftest
  fake_sum=${fake_sum%% *}
  fake_url=https://example.invalid/tla2tools.jar
  sed "s#^jar url=.*\$#jar url=$fake_url bytes=$fake_bytes sha256=$fake_sum#" "$dir/README.md" \
    >"$b/formal/r5-admit/README.md" || fail selftest
  printf 'tla2tools.jar release=v1.7.4 url=%s bytes=%s sha256=%s\n' "$fake_url" "$fake_bytes" "$fake_sum" \
    >"$b/formal/r3-confirm/README.md" || fail selftest
  git init -q "$b" >/dev/null 2>&1 || fail selftest
  (cd "$b" && git add -A) >/dev/null 2>&1 || fail selftest
  stamp_of "$b" "$work/base-stamp" || fail selftest
  base_stamp=$stamp
  write_record "$base_stamp" "$b/formal/r5-admit/record.txt" "$b/formal/r5-admit/runs.txt"
  copy_export base 0
  nd=$(grep -c '^design ' "$b/formal/r5-admit/runs.txt")
  nr=$(grep -c '^reach ' "$b/formal/r5-admit/runs.txt")
}

fixture() {
  cp -R "$work/base" "$work/$1" || fail selftest
}

edit() {
  sed "$3" "$work/$1/$2" >"$work/edited" || fail selftest
  if cmp -s "$work/edited" "$work/$1/$2"; then
    printf 'selftest fixture=%s edit=none\n' "$1"
    fail selftest
  fi
  cp "$work/edited" "$work/$1/$2" || fail selftest
}

away() {
  mv "$work/$1/$2" "$work/$1-away" || fail selftest
}

real_export() {
  cp "$dir/export.sh" "$work/$1/formal/r5-admit/export.sh" || fail selftest
}

copy_export() {
  sed "s/STATUS/$2/" "$work/copy-export.sh" >"$work/$1/formal/r5-admit/export.sh" || fail selftest
}

expect() {
  mkdir -p "$work/$1/tmp" "$work/$1/art" || fail selftest
  : >"$work/$1/java.log" || fail selftest
  (
    CI_D_REPO=$work/$1
    CI_D_ARTIFACTS=${art_as:-$work/$1/art}
    CI_D_JOB=macos
    R5_STUB_MODE=$3
    R5_STUB_FROM=$work/base/formal/r5-admit
    R5_STUB_LOG=$work/$1/java.log
    JAVA_TOOL_OPTIONS=-Dr5.stub=1
    JDK_JAVA_OPTIONS=-Dr5.stub=2
    _JAVA_OPTIONS=-Dr5.stub=3
    TMPDIR=${tmp_as:-$work/$1/tmp}
    PATH=$work/stub:$PATH
    JAVA_HOME=$java_as
    TLA2TOOLS_JAR=$jar_as
    export CI_D_REPO CI_D_ARTIFACTS CI_D_JOB R5_STUB_MODE R5_STUB_FROM R5_STUB_LOG
    export JAVA_TOOL_OPTIONS JDK_JAVA_OPTIONS _JAVA_OPTIONS TMPDIR PATH JAVA_HOME TLA2TOOLS_JAR
    [ -n "$java_as" ] || unset JAVA_HOME
    if [ -z "$jar_as" ]; then
      unset TLA2TOOLS_JAR
      PATH=$work/stub-curl:$PATH
    fi
    cd "$work/$1" || exit 98
    sh "$self" >"$work/$1/out.txt" 2>&1 </dev/null
  )
  code=$?
  one=$1
  wanted=$2
  last=
  while IFS= read -r line; do
    last=$line
  done <"$work/$one/out.txt"
  ok=yes
  [ "$code:$last" = "$wanted" ] || ok=no
  for needle in "${4:-}" "${5:-}"; do
    case $needle in
      '') ;;
      ^*) grep -q -x -F -e "${needle#^}" "$work/$one/out.txt" || ok=no ;;
      *) grep -q -F -e "$needle" "$work/$one/out.txt" || ok=no ;;
    esac
  done
  set -- "$work/$one/tmp"/*
  [ ! -e "$1" ] || ok=no
  printf 'selftest fixture=%s as-expected=%s\n' "$one" "$ok"
  if [ "$ok" = yes ]; then
    case $wanted in
      0:*) ;;
      *) rejected=$((rejected + 1)) ;;
    esac
  else
    cp "$work/$one/out.txt" "$art/selftest-$one.txt" || missed=$((missed + 1))
    missed=$((missed + 1))
  fi
  java_as=$work/jdk
  jar_as=$work/stub.jar
  art_as=
  tmp_as=
}

other_digit() {
  case $1 in
    0*) digit=1 ;;
    *) digit=0 ;;
  esac
}

logged() {
  n_small=$(grep -c '^Admit Small\.cfg$' "$work/good/java.log")
  n_control=$(grep -c '^Admit Control\.cfg$' "$work/good/java.log")
  n_export=$(grep -c '^Export Export\.cfg$' "$work/good/java.log")
  n_all=$(grep -c '' "$work/good/java.log")
  ran=no
  [ "$n_small:$n_control:$n_export:$n_all" = 1:2:1:4 ] && ran=yes
  printf 'selftest rule=runs held=%s\n' "$ran"
  [ "$ran" = yes ] || missed=$((missed + 1))
}

fixtures() {
  java_as=$work/jdk
  jar_as=$work/stub.jar
  art_as=
  tmp_as=
  passed="0:PASS $name"
  invariant="1:FAIL $name (invariant)"
  rec="1:FAIL $name (record)"
  whole="designs=$nd reported=$nd reach=$nr reached=$nr"
  cr=$(printf '\r')
  tab=$(printf '\t')
  a=formal/r5-admit/Admit.cfg
  s=formal/r5-admit/Small.cfg
  t=formal/r5-admit/Admit.tla
  rt=formal/r5-admit/record.txt
  ru=formal/r5-admit/runs.txt

  fixture good
  real_export good
  expect good "$passed" good '^formal pin=same' "formal record stamp=same full=ok small=same runs=ok $whole lines=ok"
  logged
  fixture on-the-path
  java_as=
  expect on-the-path "$passed" good '^formal java major=17'
  fixture alone
  away alone formal/r3-confirm/README.md
  expect alone "$passed" good '^formal pin=alone'

  fixture artifacts-a-file
  : >"$work/artifacts-a-file/art-file" || fail selftest
  art_as=$work/artifacts-a-file/art-file
  expect artifacts-a-file "1:FAIL $name (artifacts)" good
  fixture model-missing
  away model-missing formal/r5-admit
  expect model-missing "1:FAIL $name (model)" good
  fixture scratch-missing
  tmp_as=$work/scratch-missing/no-such-directory
  expect scratch-missing "1:FAIL $name (scratch)" good
  fixture attributes-text-auto
  edit attributes-text-auto formal/r5-admit/.gitattributes 's/^\* text eol=lf$/* text=auto eol=lf/'
  expect attributes-text-auto "1:FAIL $name (attributes)" good '^formal attributes files=12 lf=1'
  fixture attributes-readme-not-text
  printf 'README.md -text\n' >>"$work/attributes-readme-not-text/formal/r5-admit/.gitattributes" || fail selftest
  expect attributes-readme-not-text "1:FAIL $name (attributes)" good '^formal attributes files=12 lf=11'
  fixture attributes-no-eol
  edit attributes-no-eol formal/r5-admit/.gitattributes 's/ eol=lf$//'
  expect attributes-no-eol "1:FAIL $name (attributes)" good 'formal attributes files='

  fixture pin-own-twice
  edit pin-own-twice formal/r5-admit/README.md '/^jar url=/p'
  expect pin-own-twice "1:FAIL $name (pin)" good '^formal pin=differ'
  fixture pin-other-two-lines
  edit pin-other-two-lines formal/r3-confirm/README.md '/^tla2tools/{p;s/ sha256=/ sha256=x/;}'
  expect pin-other-two-lines "1:FAIL $name (pin)" good '^formal pin=differ'
  fixture pin-differs
  other_digit "$fake_sum"
  edit pin-differs formal/r3-confirm/README.md "s/sha256=./sha256=$digit/"
  expect pin-differs "1:FAIL $name (pin)" good '^formal pin=differ'

  fixture java-missing
  java_as=$work/empty
  expect java-missing "1:FAIL $name (java)" good '^formal java major=none'
  fixture java-old
  expect java-old "1:FAIL $name (java)" java-old '^formal java major=11'
  fixture jar-given-missing
  jar_as=$work/no-such.jar
  expect jar-given-missing "1:FAIL $name (fetch)" good '^formal jar source=given bytes=0 same=no'
  fixture fetch
  jar_as=
  expect fetch "1:FAIL $name (fetch)" good '^formal jar source=fetch bytes=0 same=no'
  fixture hash
  printf 'r5 stub jaR\n' >"$work/other.jar" || fail selftest
  jar_as=$work/other.jar
  expect hash "1:FAIL $name (hash)" good '^formal jar source=given bytes=12 same=no'

  fixture admit-constant
  edit admit-constant "$a" 's/^    FurtherA = {a1, a2, a3}$/    FurtherA = {a1}/'
  expect admit-constant "$invariant" good '^formal small result=admit-form'
  fixture small-other
  edit small-other "$s" 's/^    ListMax = 2$/    ListMax = 1/'
  expect small-other "$invariant" good '^formal small result=small-form'
  fixture formula-two-spaces
  edit formula-two-spaces "$t" 's/^Q7Step == /Q7Step  == /'
  expect formula-two-spaces "$invariant" good '^formal small result=formulas-form'
  fixture comment-line-q
  edit comment-line-q "$t" 's/^Q7Step == /(* The step form of Q7:\
Q7 holds on states, Q7Step on steps. *)\
&/'
  expect comment-line-q "$invariant" good '^formal small result=formulas-form'
  fixture use-line-q
  edit use-line-q "$t" 's/^Q7Step == /AllOfQ1AndQ2 ==\
    Q1 \/\\ Q2\
\
&/'
  expect use-line-q "$invariant" good '^formal small result=formulas-form'
  for c in comment-before comment-end-line; do
    fixture "$c"
    case $c in
      comment-before) edit "$c" "$t" 's/^Q7Step == /(* q *) &/' ;;
      comment-end-line) edit "$c" "$t" 's/^Q7Step == /(* the step form of Q7\
*) &/' ;;
    esac
    edit "$c" "$a" '/^    Q7Step$/d'
    edit "$c" "$s" '/^    Q7Step$/d'
    expect "$c" "$invariant" good '^formal small result=admit-form'
  done
  for c in formula-indented formula-tab formula-local formula-other-suffix; do
    fixture "$c"
    case $c in
      formula-indented) edit "$c" "$t" 's/^Q7Step == /  Q7Step == /' ;;
      formula-tab) edit "$c" "$t" "s/^Q7Step == /${tab}Q7Step == /" ;;
      formula-local) edit "$c" "$t" 's/^Q7Step == /LOCAL Q7Step == /' ;;
      formula-other-suffix) edit "$c" "$t" 's/^Q7Step == /Q7Foo == /' ;;
    esac
    expect "$c" "$invariant" good '^formal small result=formulas-form'
  done
  fixture formula-number-15
  printf 'Q15 == TRUE\n' >>"$work/formula-number-15/$t" || fail selftest
  expect formula-number-15 "$invariant" good '^formal small result=formulas-form'
  fixture property-out-of-both
  edit property-out-of-both "$t" 's/^Q5 ==/Kept5 ==/'
  edit property-out-of-both "$a" '/^    Q5$/d'
  edit property-out-of-both "$s" '/^    Q5$/d'
  expect property-out-of-both "$invariant" good '^formal states=0 properties=14 violated=0' '^formal small result=admit-form'
  fixture model-as-control-q1
  expect model-as-control-q1 "$invariant" as-control-q1 '^formal states=0 properties=14 violated=1' '^formal small result=violated'
  fixture tlc-status
  expect tlc-status "$invariant" status '^formal small result=status'
  fixture tlc-incomplete
  expect tlc-incomplete "$invariant" incomplete '^formal small result=incomplete'

  for c in constraints-init constant-runsmax glued-brace view cr-inside constraints-after-bound; do
    fixture "$c"
    for f in "$a" "$s"; do
      case $c in
        constraints-init) edit "$c" "$f" 's/^    Q14$/&\
    CONSTRAINTS Init/' ;;
        constant-runsmax) edit "$c" "$f" 's/^    Q14$/&\
    CONSTANT RunsMax = 1/' ;;
        glued-brace) edit "$c" "$f" 's/^    Compiled = {c1, c2}$/&Q14 = TRUE/' ;;
        view) edit "$c" "$f" 's/^    Q14$/&\
VIEW Init/' ;;
        cr-inside) edit "$c" "$f" "s/^CONSTRAINT Bound\$/CONSTRAINT Bound${cr}CONSTRAINT Init/" ;;
        constraints-after-bound) edit "$c" "$f" 's/^CONSTRAINT Bound$/&\
CONSTRAINTS Init/' ;;
      esac
    done
    expect "$c" "$invariant" good '^formal small result=admit-form'
  done
  fixture comment-opener
  edit comment-opener "$a" 's/^CHECK_DEADLOCK FALSE$/& (* x *)/'
  edit comment-opener "$s" 's/^CHECK_DEADLOCK FALSE$/& (* x *)/'
  expect comment-opener "$invariant" good '^formal small result=admit-form'
  fixture operator-replaced
  edit operator-replaced "$a" 's/^    Fault = "none"$/&\
    Bound <- Init/'
  edit operator-replaced "$s" 's/^    Fault = "none"$/&\
    Bound <- Init/'
  expect operator-replaced "$invariant" good '^formal small result=admit-form'
  fixture keyword-twice
  edit keyword-twice "$a" 's/^CONSTRAINT Bound$/&\
CONSTRAINT Bound/'
  edit keyword-twice "$s" 's/^CONSTRAINT Bound$/&\
CONSTRAINT Bound/'
  expect keyword-twice "$invariant" good '^formal small result=admit-form'
  fixture constant-twice
  edit constant-twice "$a" 's/^    ListMax = 2$/&\
    ListMax = 2/'
  edit constant-twice "$s" 's/^    ListMax = 2$/&\
    ListMax = 2/'
  expect constant-twice "$invariant" good '^formal small result=admit-form'
  fixture untracked-module
  : >"$work/untracked-module/formal/r5-admit/FiniteSets.tla" || fail selftest
  expect untracked-module "$passed" good '^formal states=15690 properties=14 violated=0'
  fixture model-as-control-q5
  expect model-as-control-q5 "$invariant" as-control-q5 '^formal small result=violated'
  fixture model-invariant
  expect model-invariant "$invariant" invariant-q7 '^formal states=0 properties=14 violated=1'

  fixture export-exit-1
  copy_export export-exit-1 1
  expect export-exit-1 "1:FAIL $name (table)" good '^formal export status=1' '^formal table rows=57 same=no'
  fixture table-56-rows
  copy_export table-56-rows 0
  edit table-56-rows formal/r5-admit/edit-table.tsv 9d
  expect table-56-rows "1:FAIL $name (table)" good '^formal table rows=56 same=no'
  fixture table-token
  real_export table-token
  edit table-token formal/r5-admit/edit-table.tsv '1s/edit/refused/'
  expect table-token "1:FAIL $name (table)" good '^formal table rows=57 same=no'
  fixture trace-move
  real_export trace-move
  edit trace-move formal/r5-admit/traces.jsonl '9s/"move":"recompile"/"move":"use_ok"/'
  expect trace-move "1:FAIL $name (traces)" good '^formal table rows=57 same=yes' '^formal traces count=200 same=no'

  fixture control-status
  expect control-status "1:FAIL $name (control)" control-status '^formal control violated=0'
  fixture control-other
  expect control-other "1:FAIL $name (control)" control-other '^formal control violated=0'
  fixture control-holds
  expect control-holds "1:FAIL $name (control)" control-holds '^formal control violated=0'
  fixture control-holds-q1
  expect control-holds-q1 "1:FAIL $name (control)" control-holds-Q1 '^formal control violated=1'
  fixture control-holds-q5
  expect control-holds-q5 "1:FAIL $name (control)" control-holds-Q5 '^formal control violated=1'

  fixture runs-missing
  away runs-missing "$ru"
  expect runs-missing "$rec" good 'runs=absent'
  fixture runs-odd-line
  edit runs-odd-line "$ru" '1s/$/ /'
  expect runs-odd-line "$rec" good 'runs=unreadable'
  fixture runs-twice
  edit runs-twice "$ru" '1p'
  expect runs-twice "$rec" good 'runs=twice'
  fixture runs-design-twice
  printf 'design no_scope 5 0 3 Q3\n' >>"$work/runs-design-twice/$ru" || fail selftest
  expect runs-design-twice "$rec" good 'runs=twice'
  fixture runs-bound-zero
  edit runs-bound-zero "$ru" 's/^design no_scope 3 2 3 Q3$/design no_scope 0 2 3 Q3/'
  edit runs-bound-zero "$rt" 's/^design fault=no_scope bound=3-2-3 /design fault=no_scope bound=0-2-3 /'
  expect runs-bound-zero "$rec" good 'runs=unreadable'
  fixture runs-design-dropped
  edit runs-design-dropped "$ru" '/^design no_scope /d'
  edit runs-design-dropped "$rt" '/^design fault=no_scope /d'
  expect runs-design-dropped "$rec" good 'runs=designs-model'
  fixture runs-reach-dropped
  edit runs-reach-dropped "$ru" '/^reach NotCauseBase /d'
  edit runs-reach-dropped "$rt" '/^reach property=NotCauseBase /d'
  expect runs-reach-dropped "$rec" good 'runs=reach-model'
  fixture runs-no-designs
  edit runs-no-designs "$ru" '/^design /d'
  edit runs-no-designs "$rt" '/^design /d'
  expect runs-no-designs "$rec" good 'runs=no-designs'
  fixture runs-no-reach
  edit runs-no-reach "$ru" '/^reach /d'
  edit runs-no-reach "$rt" '/^reach /d'
  expect runs-no-reach "$rec" good 'runs=no-reach'

  fixture record-missing
  away record-missing "$rt"
  expect record-missing "$rec" good 'formal record stamp=absent'
  fixture record-no-stamp
  edit record-no-stamp "$rt" '/^stamp=/d'
  expect record-no-stamp "$rec" good 'formal record stamp=none'
  fixture record-one-digit
  other_digit "$base_stamp"
  edit record-one-digit "$rt" "s/^stamp=./stamp=$digit/"
  expect record-one-digit "$rec" good 'formal record stamp=differ'
  fixture record-older-model
  printf '\n' >>"$work/record-older-model/$t" || fail selftest
  expect record-older-model "$rec" good 'formal record stamp=differ'
  fixture record-cfg-changed
  printf '\n' >>"$work/record-cfg-changed/formal/r5-admit/Export.cfg" || fail selftest
  expect record-cfg-changed "$rec" good 'formal record stamp=differ full=ok'
  fixture record-full-properties
  edit record-full-properties "$rt" 's/^\(full .*\) properties=14 /\1 properties=13 /'
  expect record-full-properties "$rec" good 'formal record stamp=same full=miss small=same'
  fixture record-full-violated
  edit record-full-violated "$rt" 's/^\(full .*\) violated=0 /\1 violated=1 /'
  expect record-full-violated "$rec" good 'formal record stamp=same full=miss small=same'
  fixture record-small-states
  edit record-small-states "$rt" 's/^small config=Small.cfg states=15690$/small config=Small.cfg states=15689/'
  expect record-small-states "$rec" good 'formal record stamp=same full=ok small=differ runs=ok'
  fixture record-small-longer
  edit record-small-longer "$rt" 's/^small config=Small.cfg states=15690$/small config=Small.cfg states=156901/'
  expect record-small-longer "$rec" good 'formal record stamp=same full=ok small=differ runs=ok'
  fixture record-design-missing
  edit record-design-missing "$rt" '/^design fault=no_scope /d'
  expect record-design-missing "$rec" good "designs=$nd reported=$((nd - 1)) reach=$nr reached=$nr"
  fixture record-design-other
  edit record-design-other "$rt" 's/^\(design fault=no_scope .*\) reports=Q3 /\1 reports=Q4 /'
  expect record-design-other "$rec" good "designs=$nd reported=$((nd - 1)) reach=$nr reached=$nr lines=ok"
  fixture record-design-bound-changed
  edit record-design-bound-changed "$ru" 's/^design no_scope 3 2 3 Q3$/design no_scope 5 0 3 Q3/'
  expect record-design-bound-changed "$rec" good "designs=$nd reported=$((nd - 1)) reach=$nr reached=$nr lines=ok"
  fixture record-reach-not-reached
  edit record-reach-not-reached "$rt" 's/^\(reach property=NotListCut .*\) reached=yes /\1 reached=no /'
  expect record-reach-not-reached "$rec" good "designs=$nd reported=$nd reach=$nr reached=$((nr - 1))"
  fixture record-cr
  edit record-cr "$rt" "s/\$/${cr}/"
  expect record-cr "$rec" good 'lines=cr'
  fixture record-one-cr
  edit record-one-cr "$rt" "\$s/\$/${cr}/"
  expect record-one-cr "$rec" good 'lines=cr'
  fixture record-empty-line
  printf '\n' >>"$work/record-empty-line/$rt" || fail selftest
  expect record-empty-line "$rec" good "stamp=same full=ok small=same runs=ok $whole lines=differ"
}

selftest() {
  label=self-test:$name
  locate
  mkdir -p "$art_dir" 2>/dev/null || fail artifacts
  art=$art_dir
  [ -d "$dir" ] || fail model
  scratch
  [ -n "$tmp" ] || fail scratch
  work=$tmp/fixtures
  mkdir -p "$work" || fail selftest
  stubs
  base
  rejected=0
  missed=0
  fixtures
  [ "$missed" -eq 0 ] || fail selftest
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case $# in
  0) measure ;;
  1)
    case $1 in
      --self-test) selftest ;;
      *) fail usage ;;
    esac
    ;;
  *) fail usage ;;
esac
