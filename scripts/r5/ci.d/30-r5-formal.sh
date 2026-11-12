#!/bin/sh
# jobs: macos windows
# rule: TLC finds none of the properties Q1 to Q14 of formal/r5-admit violated at the small bound of Small.cfg, the model exports the committed table and traces byte for byte, each of two control copies breaks its property, and formal/r5-admit/record.txt holds the full bound, every design and every reach run of the README at the stamp of the model's files.
set -u
LC_ALL=C
export LC_ALL
unset CDPATH JAVA_TOOL_OPTIONS JDK_JAVA_OPTIONS _JAVA_OPTIONS
name=30-r5-formal
label=$name
workers=3
case $0 in
  /*) self=$0 ;;
  *) self=$(pwd)/$0 ;;
esac

fail() {
  printf 'FAIL %s (%s)\n' "$label" "$1"
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

locate() {
  if [ -n "${CI_D_REPO:-}" ]; then
    repo=$CI_D_REPO
  else
    repo=$(cd "${self%/*}/../../.." && pwd) || repo=
  fi
  dir=$repo/formal/r5-admit
  art=${CI_D_ARTIFACTS:-$repo/target/ci-artifacts/$name}
}

scratch() {
  tmp=$(mktemp -d "${TMPDIR:-/tmp}/r5-formal.XXXXXX") || tmp=
  made=$tmp
  [ -z "$tmp" ] || tmp=$(cd "$tmp" && pwd) || tmp=
  [ -z "$tmp" ] || made=$tmp
}

since() {
  now=$(date +%s)
  seconds=$((now - start))
  start=$now
}

pin() {
  own=
  [ -f "$dir/README.md" ] && own=$(tr -d '\r' <"$dir/README.md" |
    sed -n 's/^jar url=\([^ ]*\) bytes=\([0-9][0-9]*\) sha256=\([0-9a-f]\{64\}\)$/\1 \2 \3/p')
  other=
  named=0
  if [ -f "$repo/formal/r3-confirm/README.md" ]; then
    other=$(tr -d '\r' <"$repo/formal/r3-confirm/README.md" |
      sed -n 's/^tla2tools\.jar release=[^ ]* url=\([^ ]*\) bytes=\([0-9][0-9]*\) sha256=\([0-9a-f]\{64\}\)$/\1 \2 \3/p')
    named=$(grep -c '^tla2tools\.jar ' "$repo/formal/r3-confirm/README.md")
  fi
  case $own in
    '' | *"
"*) state=differ ;;
    *)
      case $named:$other in
        0:) state=alone ;;
        "1:$own") state=same ;;
        *) state=differ ;;
      esac
      ;;
  esac
  printf 'formal pin=%s\n' "$state"
  [ "$state" != differ ] || fail pin
  url=${own%% *}
  rest=${own#* }
  bytes=${rest% *}
  want=${rest#* }
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
  major=none
  if command -v "$java" >/dev/null 2>&1; then
    "$java" -version >"$tmp/java.raw" 2>&1 </dev/null || major=none
    tr -d '\r' <"$tmp/java.raw" >"$tmp/java.txt" || fail java
    sed -n 's/.* version "\([0-9][0-9]*\).*/\1/p' "$tmp/java.txt" >"$tmp/major.txt" || fail java
    read -r major <"$tmp/major.txt" || major=none
  fi
  printf 'formal java major=%s\n' "$major"
  case $major in
    '' | *[!0-9]*) fail java ;;
  esac
  [ "$major" -ge 17 ] || fail java
}

jar() {
  if [ -n "${TLA2TOOLS_JAR:-}" ]; then
    source=given
    if [ ! -f "$TLA2TOOLS_JAR" ] || ! cp "$TLA2TOOLS_JAR" "$tmp/tla2tools.jar"; then
      printf 'formal jar source=given bytes=0 same=no\n'
      fail fetch
    fi
  else
    source=fetch
    if ! command -v curl >/dev/null 2>&1 ||
      ! (cd "$tmp" && curl -q -fsSL --retry 2 --max-time 300 -o tla2tools.jar "$url") ||
      [ ! -f "$tmp/tla2tools.jar" ]; then
      printf 'formal jar source=fetch bytes=0 same=no\n'
      fail fetch
    fi
  fi
  size=$(wc -c <"$tmp/tla2tools.jar") || fail hash
  size=$((size + 0))
  sum=$(sha256 <"$tmp/tla2tools.jar") || fail hash
  sum=${sum%% *}
  same=no
  [ "$size" = "$bytes" ] && [ "$sum" = "$want" ] && same=yes
  printf 'formal jar source=%s bytes=%s same=%s\n' "$source" "$size" "$same"
  [ "$same" = yes ] || fail hash
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

numbers() {
  tr -d '\r' <"$2" | sed -n 's/^\(Q[0-9][0-9]*[A-Za-z]*\) ==.*$/\1/p' | sort -u >"$tmp/formulas.txt" || return 1
  awk -v formulas="$tmp/formulas.txt" '
    BEGIN { while ((getline f < formulas) > 0) want[f] = 1 }
    { sub(/\r$/, ""); sub(/\\\*.*$/, "") }
    index($0, "(*") > 0 || index($0, "<-") > 0 { odd = 1 }
    part == "constants" && match($0, /^[ \t]*[A-Za-z_][A-Za-z0-9_]*[ \t]*=/) {
      c = substr($0, RSTART, RLENGTH)
      gsub(/[ \t=]/, "", c)
      if (c in assigned) odd = 1
      assigned[c] = 1
    }
    {
      for (i = 1; i <= NF; i++) {
        w = $i
        if (w ~ /^[A-Z][A-Z_]*$/ && w != "TRUE" && w != "FALSE") {
          if (w in keyword) odd = 1
          keyword[w] = 1
          if (w ~ /^(PROPERT(Y|IES)|INVARIANTS?)$/) part = "checked"
          else if (w ~ /^CONSTANTS?$/) part = "constants"
          else part = "other"
          continue
        }
        if (part != "checked") {
          if (w ~ /^Q[0-9]/) odd = 1
          continue
        }
        if (w !~ /^Q[0-9]+(Step|Move)?$/ || (w in listed)) { odd = 1; continue }
        listed[w] = 1
        q = w
        sub(/^Q/, "", q)
        sub(/[A-Za-z]+$/, "", q)
        if (q + 0 >= 1 && q + 0 <= 14) seen[q + 0] = 1
      }
    }
    END {
      for (q in seen) k++
      for (f in want) if (!(f in listed)) missing++
      printf "%d %s %d\n", k, odd ? "odd" : "plain", missing
    }' "$1"
}

small() {
  result=ok
  config=$dir/Small.cfg
  sed -e 's/^    RunsMax = 5$/    RunsMax = 3/' -e 's/^    RunsOnBothSteps = 3$/    RunsOnBothSteps = 2/' \
    "$dir/Admit.cfg" >"$tmp/small-want.cfg" || fail invariant
  a=$(grep -c '^    RunsMax = 5$' "$dir/Admit.cfg")
  b=$(grep -c '^    RunsOnBothSteps = 3$' "$dir/Admit.cfg")
  if [ "$a:$b" != 1:1 ] || [ ! -f "$config" ] || ! cmp -s "$tmp/small-want.cfg" "$config"; then
    result=config
  fi
  checked=0
  shape=plain
  if [ "$result" = ok ]; then
    numbers "$config" "$dir/Admit.tla" >"$tmp/numbers.txt" || fail invariant
    read -r checked shape missing <"$tmp/numbers.txt" || fail invariant
    if [ "$shape" != plain ]; then
      result=form
    elif [ "$missing" -ne 0 ]; then
      result=formulas
    fi
  fi
  distinct=0
  violated=0
  if [ "$result" = ok ]; then
    run=$tmp/small
    mkdir "$run" "$run/jtmp" || fail invariant
    cp "$dir"/*.tla "$config" "$run/" || fail invariant
    tlc "$run" Small.cfg
    sed -n 's/^violated //p' "$run/facts.txt" | sort -u >"$tmp/violated.txt" || fail invariant
    violated=$(grep -c . "$tmp/violated.txt")
    sed -n 's/^distinct //p' "$run/facts.txt" >"$tmp/distinct.txt" || fail invariant
    read -r distinct <"$tmp/distinct.txt" || distinct=0
    complete=$(grep -c '^complete$' "$run/facts.txt")
    if [ "$violated" -ne 0 ]; then
      result=violated
    elif [ "$status" -ne 0 ]; then
      result=status
    elif [ "$complete" -ne 1 ] || [ "$distinct" -eq 0 ]; then
      result=incomplete
    elif [ "$checked" -ne 14 ]; then
      result=properties
    fi
  fi
  printf 'formal states=%s properties=%s violated=%s\n' "$distinct" "$checked" "$violated"
  if [ "$result" != ok ]; then
    printf 'formal small result=%s\n' "$result"
    fail invariant
  fi
}

exported() {
  mkdir "$tmp/export" "$tmp/export-tmp" || fail table
  TLA2TOOLS_JAR=$tmp/tla2tools.jar TMPDIR=$tmp/export-tmp sh "$dir/export.sh" --out "$tmp/export" \
    >"$tmp/export.txt" 2>&1 </dev/null
  code=$?
  cp "$tmp/export.txt" "$art/export.txt" || fail table
  rows=0
  [ -f "$dir/edit-table.tsv" ] && rows=$(wc -l <"$dir/edit-table.tsv")
  rows=$((rows + 0))
  table=no
  if [ "$code" -eq 0 ] && [ "$rows" -eq 57 ] && cmp -s "$dir/edit-table.tsv" "$tmp/export/edit-table.tsv"; then
    table=yes
  fi
  lines=0
  [ -f "$dir/traces.jsonl" ] && lines=$(wc -l <"$dir/traces.jsonl")
  count=$((lines - 1))
  [ "$count" -ge 0 ] || count=0
  traces=no
  if [ "$code" -eq 0 ] && cmp -s "$dir/traces.jsonl" "$tmp/export/traces.jsonl"; then
    traces=yes
  fi
  [ "$code" -eq 0 ] || printf 'formal export status=%s\n' "$code"
  printf 'formal table rows=%s same=%s\n' "$rows" "$table"
  printf 'formal traces count=%s same=%s\n' "$count" "$traces"
  [ "$table" = yes ] || fail table
  [ "$traces" = yes ] || fail traces
}

control() {
  run=$tmp/control-$1
  mkdir "$run" "$run/jtmp" || return 2
  cp "$dir"/*.tla "$run/" || return 2
  R5_FAULT=$1 R5_PROPERTY=$2 awk '
    BEGIN { fault = ENVIRON["R5_FAULT"]; property = ENVIRON["R5_PROPERTY"] }
    $0 == "    Fault = \"none\"" { print "    Fault = \"" fault "\""; f++; next }
    $0 == "PROPERTIES" { print "PROPERTY " property; p++; block = 1; next }
    block && /^    / { next }
    { block = 0; print }
    END { if (f != 1 || p != 1) exit 3 }' "$dir/Admit.cfg" >"$run/Control.cfg" || return 2
  tlc "$run" Control.cfg
  sed -n 's/^violated //p' "$run/facts.txt" >"$run/violated.txt" || return 2
  first=
  read -r first <"$run/violated.txt" || first=
  [ "$status" -ne 0 ] && [ "$first" = "$2" ]
}

controls() {
  broken=0
  applied=yes
  for copy in serve_reverted:Q1 no_bar:Q5; do
    control "${copy%:*}" "${copy#*:}"
    case $? in
      0) broken=$((broken + 1)) ;;
      2) applied=no ;;
    esac
  done
  [ "$applied" = yes ] || printf 'formal control applied=no\n'
  printf 'formal control violated=%s\n' "$broken"
  [ "$applied" = yes ] || fail control
  [ "$broken" -eq 2 ] || fail control
}

stamp_of() {
  stamp=
  (cd "$1" && git -c core.quotepath=off ls-files -- 'formal/r5-admit/*.tla' 'formal/r5-admit/*.cfg') \
    >"$2.files" 2>/dev/null || return 1
  sort "$2.files" >"$2.sorted" || return 1
  : >"$2.list" || return 1
  while IFS= read -r f; do
    [ -f "$1/$f" ] || return 1
    one=$(sha256 <"$1/$f") || return 1
    printf '%s  %s\n' "${one%% *}" "$f" >>"$2.list" || return 1
  done <"$2.sorted"
  [ -s "$2.list" ] || return 1
  stamp=$(sha256 <"$2.list") || return 1
  stamp=${stamp%% *}
  [ "${#stamp}" -eq 64 ]
}

designs_of() {
  tr -d '\r' <"$1" | awk '
    $0 == "| `Fault` | from | what it changes | TLC reports | distinct states | bound | output |" { state = 1; heads++; next }
    state == 1 { state = 2; next }
    state == 2 && /^\|/ {
      n = split($0, cell, "|")
      f = cell[2]; r = cell[5]
      gsub(/^[ ]+|[ ]+$/, "", f); gsub(/^[ ]+|[ ]+$/, "", r)
      if (n != 9 || f !~ /^`[a-z0-9_]+`$/ || r !~ /^(Q[0-9]+[A-Za-z]*|nothing)$/) bad++
      gsub(/`/, "", f)
      print f, r
      next
    }
    state == 2 { state = 0 }
    /^[ \t]*\|[ \t]*`[a-z0-9_]+`[ \t]*\|/ && split($0, cell, "|") == 9 { bad++ }
    END { if (heads != 1 || bad) print "bad bad" }'
}

reaches_of() {
  tr -d '\r' <"$1" | awk '
    $0 == "## The reach runs" { part = 1; next }
    part && /^## / { part = 0 }
    part && !block && /^[ \t]*```/ { block = 1; blocks++; next }
    block && $0 == "```" { block = 0; next }
    !block { next }
    /^for name in / { collect = 1; sub(/^for name in /, "") }
    collect {
      line = $0
      last = (line ~ /; do$/)
      sub(/; do$/, "", line); sub(/\\$/, "", line)
      k = split(line, word, " ")
      for (i = 1; i <= k; i++) wait[++waiting] = word[i]
      if (last) collect = 0
      next
    }
    /^    reach (INVARIANT|PROPERTY) "(Not|Never)\$name" [0-9]+ [0-9]+$/ {
      p = $3; gsub(/"/, "", p); sub(/\$name$/, "", p)
      if (($2 == "INVARIANT") != (p == "Not")) bad++
      if (!waiting) bad++
      for (i = 1; i <= waiting; i++) print p wait[i], $4 "-" $5
      waiting = 0
      next
    }
    /^reach (INVARIANT|PROPERTY) (Not|Never)[A-Za-z]+ [0-9]+ [0-9]+$/ {
      if (($2 == "INVARIANT") != ($3 ~ /^Not/)) bad++
      print $3, $4 "-" $5
      next
    }
    /(^|[^A-Za-z0-9_])(reach|for)([^A-Za-z0-9_(]|$)/ { bad++ }
    END { if (blocks != 1 || waiting || collect || bad) print "bad bad" }'
}

record() {
  stamped=absent
  full=miss
  file=$dir/record.txt
  designs_of "$dir/README.md" | sort >"$tmp/want-designs.txt" || fail record
  reaches_of "$dir/README.md" | sort >"$tmp/want-reach.txt" || fail record
  if [ -f "$file" ]; then
    tr -d '\r' <"$file" >"$tmp/record.txt" || fail record
    stamped=none
    n=$(grep -c '^stamp=' "$tmp/record.txt")
    have=$(sed -n 's/^stamp=\([0-9a-f]\{64\}\)$/\1/p' "$tmp/record.txt")
    if [ "$n" -eq 1 ] && [ -n "$have" ]; then
      stamped=differ
      stamp_of "$repo" "$tmp/stamp" || stamp=
      [ "$have" != "$stamp" ] || stamped=same
    fi
    n=$(grep -c '^full ' "$tmp/record.txt")
    m=$(grep -c -E '^full config=Admit\.cfg states=[1-9][0-9]* properties=14 violated=0 workers=[1-9][0-9]* run=(helper|nice19) seconds=[0-9]+$' "$tmp/record.txt")
    [ "$n:$m" != 1:1 ] || full=ok
    sed -n 's/^design fault=\([^ ]*\) reports=\([^ ]*\) states=[0-9][0-9]*$/\1 \2/p' "$tmp/record.txt" |
      sort >"$tmp/have-designs.txt" || fail record
    sed -n 's/^reach property=\([^ ]*\) bound=\([^ ]*\) reached=yes states=[0-9][0-9]*$/\1 \2/p' "$tmp/record.txt" |
      sort >"$tmp/have-reach.txt" || fail record
  else
    : >"$tmp/have-designs.txt" || fail record
    : >"$tmp/have-reach.txt" || fail record
  fi
  lines=$(grep -c . "$tmp/record.txt" 2>/dev/null) || lines=0
  designs=$(grep -c . "$tmp/want-designs.txt")
  reported=$(comm -12 "$tmp/want-designs.txt" "$tmp/have-designs.txt" | grep -c .)
  reach=$(grep -c . "$tmp/want-reach.txt")
  reached=$(comm -12 "$tmp/want-reach.txt" "$tmp/have-reach.txt" | grep -c .)
  readme=ok
  if grep -q '^bad bad$' "$tmp/want-designs.txt" "$tmp/want-reach.txt" ||
    [ "$designs" -eq 0 ] || [ "$reach" -eq 0 ]; then
    readme=unreadable
  fi
  printf 'formal record stamp=%s full=%s designs=%s reported=%s reach=%s reached=%s readme=%s\n' \
    "$stamped" "$full" "$designs" "$reported" "$reach" "$reached" "$readme"
  [ "$stamped" = same ] || fail record
  [ "$full" = ok ] || fail record
  if [ "$readme" != ok ] ||
    ! cmp -s "$tmp/want-designs.txt" "$tmp/have-designs.txt" ||
    ! cmp -s "$tmp/want-reach.txt" "$tmp/have-reach.txt" ||
    [ "$lines" -ne $((2 + designs + reach)) ]; then
    fail record
  fi
}

measure() {
  locate
  [ -d "$dir" ] || fail pin
  mkdir -p "$art" || fail fetch
  pin
  scratch
  [ -n "$tmp" ] || fail fetch
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
  printf 'PASS %s\n' "$name"
}

stubs() {
  mkdir -p "$work/stub" "$work/jdk/bin" "$work/empty" || fail selftest
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
  incomplete) exit 0 ;;
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
  cp "$work/stub/java" "$work/jdk/bin/java" || fail selftest
  chmod +x "$work/stub/java" "$work/jdk/bin/java" "$work/stub-curl/curl" || fail selftest
}

write_record() {
  {
    printf 'stamp=%s\n' "$1"
    printf 'full config=Admit.cfg states=1660030 properties=14 violated=0 workers=4 run=nice19 seconds=5009\n'
    while read -r f r; do
      printf 'design fault=%s reports=%s states=100\n' "$f" "$r"
    done <"$work/designs.txt"
    while read -r p b; do
      printf 'reach property=%s bound=%s reached=yes states=10\n' "$p" "$b"
    done <"$work/reach.txt"
  } >"$2" || fail selftest
}

base() {
  b=$work/base
  mkdir -p "$b/formal/r3-confirm" || fail selftest
  cp -R "$dir" "$b/formal/" || fail selftest
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
  designs_of "$b/formal/r5-admit/README.md" >"$work/designs.txt" || fail selftest
  reaches_of "$b/formal/r5-admit/README.md" >"$work/reach.txt" || fail selftest
  if grep -q '^bad bad$' "$work/designs.txt" "$work/reach.txt"; then
    printf 'selftest the README lists of the model are not readable\n'
    fail selftest
  fi
  git init -q "$b" >/dev/null 2>&1 || fail selftest
  (cd "$b" && git add -A) >/dev/null 2>&1 || fail selftest
  stamp_of "$b" "$work/base-stamp" || fail selftest
  base_stamp=$stamp
  write_record "$base_stamp" "$b/formal/r5-admit/record.txt"
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

expect() {
  mkdir -p "$work/$1/tmp" "$work/$1/art" || fail selftest
  : >"$work/$1/java.log" || fail selftest
  (
    CI_D_REPO=$work/$1
    CI_D_ARTIFACTS=$work/$1/art
    CI_D_JOB=macos
    R5_STUB_MODE=$3
    R5_STUB_FROM=$work/base/formal/r5-admit
    R5_STUB_LOG=$work/$1/java.log
    JAVA_TOOL_OPTIONS=-Dr5.stub=1
    JDK_JAVA_OPTIONS=-Dr5.stub=2
    _JAVA_OPTIONS=-Dr5.stub=3
    TMPDIR=$work/$1/tmp
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
    [ -z "$needle" ] || grep -q -F -e "$needle" "$work/$one/out.txt" || ok=no
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
}

other_digit() {
  case $1 in
    0*) digit=1 ;;
    *) digit=0 ;;
  esac
}

fixtures() {
  java_as=$work/jdk
  jar_as=$work/stub.jar
  passed="0:PASS $name"

  designs_n=$(grep -c . "$work/designs.txt")
  reach_n=$(grep -c . "$work/reach.txt")
  traces_n=$(($(wc -l <"$dir/traces.jsonl") - 1))
  fixture good
  expect good "$passed" good 'formal pin=same' \
    "formal record stamp=same full=ok designs=$designs_n reported=$designs_n reach=$reach_n reached=$reach_n"
  logged
  fixture on-the-path
  java_as=
  expect on-the-path "$passed" good 'formal java major=17'
  fixture alone
  mv "$work/alone/formal/r3-confirm/README.md" "$work/alone-r3.md" || fail selftest
  expect alone "$passed" good 'formal pin=alone'

  fixture pin-differs
  other_digit "$fake_sum"
  edit pin-differs formal/r3-confirm/README.md "s/sha256=./sha256=$digit/"
  expect pin-differs "1:FAIL $name (pin)" good 'formal pin=differ'
  fixture java-old
  expect java-old "1:FAIL $name (java)" java-old 'formal java major=11'
  fixture java-missing
  java_as=$work/empty
  expect java-missing "1:FAIL $name (java)" good 'formal java major=none'
  fixture fetch
  jar_as=
  expect fetch "1:FAIL $name (fetch)" good 'formal jar source=fetch bytes=0 same=no'
  fixture hash
  printf 'r5 stub jaR\n' >"$work/other.jar" || fail selftest
  jar_as=$work/other.jar
  expect hash "1:FAIL $name (hash)" good 'formal jar source=given bytes=12 same=no'
  fixture pin-unreadable
  edit pin-unreadable formal/r3-confirm/README.md 's/$/ /'
  expect pin-unreadable "1:FAIL $name (pin)" good 'formal pin=differ'

  fixture small-property-out
  edit small-property-out formal/r5-admit/Small.cfg '/^    Q5$/d'
  expect small-property-out "1:FAIL $name (invariant)" good 'formal small result=config'
  fixture property-out-of-both
  edit property-out-of-both formal/r5-admit/Small.cfg '/^    Q5$/d'
  edit property-out-of-both formal/r5-admit/Admit.cfg '/^    Q5$/d'
  edit property-out-of-both formal/r5-admit/Admit.tla 's/^Q5 ==/Kept5 ==/'
  expect property-out-of-both "1:FAIL $name (invariant)" good 'formal states=15690 properties=13 violated=0' \
    'formal small result=properties'
  fixture small-other-bound
  edit small-other-bound formal/r5-admit/Small.cfg 's/^    ListMax = 2$/    ListMax = 1/'
  expect small-other-bound "1:FAIL $name (invariant)" good 'formal small result=config'
  fixture property-replaced
  edit property-replaced formal/r5-admit/Small.cfg 's/^    Fault = "none"$/&\
    Q5 <- Q4/'
  edit property-replaced formal/r5-admit/Admit.cfg 's/^    Fault = "none"$/&\
    Q5 <- Q4/'
  expect property-replaced "1:FAIL $name (invariant)" good 'formal small result=form'
  fixture formula-dropped
  edit formula-dropped formal/r5-admit/Small.cfg '/^    Q12Move$/d'
  edit formula-dropped formal/r5-admit/Admit.cfg '/^    Q12Move$/d'
  expect formula-dropped "1:FAIL $name (invariant)" good 'formal states=0 properties=14 violated=0' 'result=formulas'
  fixture constant-for-a-property
  edit constant-for-a-property formal/r5-admit/Small.cfg 's/^    Fault = "none"$/&\
    Q14 = TRUE/'
  edit constant-for-a-property formal/r5-admit/Admit.cfg 's/^    Fault = "none"$/&\
    Q14 = TRUE/'
  expect constant-for-a-property "1:FAIL $name (invariant)" good 'formal small result=form'
  fixture unknown-section
  edit unknown-section formal/r5-admit/Small.cfg 's/^    Q14$/TYPE_CONSTRAINT\
&/'
  edit unknown-section formal/r5-admit/Admit.cfg 's/^    Q14$/TYPE_CONSTRAINT\
&/'
  expect unknown-section "1:FAIL $name (invariant)" good 'formal small result=form'
  fixture constant-twice
  edit constant-twice formal/r5-admit/Small.cfg 's/^    Q14$/&\
CONSTANTS\
    ListMax = 1/'
  edit constant-twice formal/r5-admit/Admit.cfg 's/^    Q14$/&\
CONSTANTS\
    ListMax = 1/'
  expect constant-twice "1:FAIL $name (invariant)" good 'formal small result=form'
  fixture model-invariant
  expect model-invariant "1:FAIL $name (invariant)" invariant-q7 'formal states=0 properties=14 violated=1'
  fixture model-as-control-q1
  expect model-as-control-q1 "1:FAIL $name (invariant)" as-control-q1 'formal states=0 properties=14 violated=1'
  fixture model-as-control-q5
  expect model-as-control-q5 "1:FAIL $name (invariant)" as-control-q5 'formal small result=violated'
  fixture tlc-status
  expect tlc-status "1:FAIL $name (invariant)" status 'formal small result=status'
  fixture tlc-incomplete
  expect tlc-incomplete "1:FAIL $name (invariant)" incomplete 'formal small result=incomplete'

  fixture table-row-removed
  edit table-row-removed formal/r5-admit/edit-table.tsv 9d
  expect table-row-removed "1:FAIL $name (table)" good 'formal table rows=56 same=no'
  fixture table-token
  edit table-token formal/r5-admit/edit-table.tsv '1s/edit/refused/'
  expect table-token "1:FAIL $name (table)" good 'formal table rows=57 same=no'
  fixture trace-move
  edit trace-move formal/r5-admit/traces.jsonl '9s/"move":"recompile"/"move":"use_ok"/'
  expect trace-move "1:FAIL $name (traces)" good 'formal table rows=57 same=yes' "formal traces count=$traces_n same=no"

  fixture control-holds
  expect control-holds "1:FAIL $name (control)" control-holds 'formal control violated=0'
  fixture control-other
  expect control-other "1:FAIL $name (control)" control-other 'formal control violated=0'
  fixture control-unapplied
  edit control-unapplied formal/r5-admit/Admit.cfg 's/^    Fault = "none"$/    Fault = "none" /'
  edit control-unapplied formal/r5-admit/Small.cfg 's/^    Fault = "none"$/    Fault = "none" /'
  expect control-unapplied "1:FAIL $name (control)" good 'formal control applied=no'

  fixture record-missing
  mv "$work/record-missing/formal/r5-admit/record.txt" "$work/record-away.txt" || fail selftest
  expect record-missing "1:FAIL $name (record)" good 'formal record stamp=absent'
  fixture record-no-stamp
  edit record-no-stamp formal/r5-admit/record.txt '/^stamp=/d'
  expect record-no-stamp "1:FAIL $name (record)" good 'formal record stamp=none'
  fixture record-one-digit
  other_digit "$base_stamp"
  edit record-one-digit formal/r5-admit/record.txt "s/^stamp=./stamp=$digit/"
  expect record-one-digit "1:FAIL $name (record)" good 'formal record stamp=differ'
  fixture record-older-model
  printf '\n' >>"$work/record-older-model/formal/r5-admit/Admit.tla" || fail selftest
  expect record-older-model "1:FAIL $name (record)" good 'formal record stamp=differ'
  fixture record-full-violated
  edit record-full-violated formal/r5-admit/record.txt 's/^\(full .*\) violated=0 /\1 violated=1 /'
  expect record-full-violated "1:FAIL $name (record)" good 'formal record stamp=same full=miss'
  fixture record-full-how-unsaid
  edit record-full-how-unsaid formal/r5-admit/record.txt 's/^\(full .*\) run=nice19 /\1 /'
  expect record-full-how-unsaid "1:FAIL $name (record)" good 'formal record stamp=same full=miss'
  whole="designs=$designs_n reported=$designs_n reach=$reach_n reached=$reach_n"
  fixture record-design-missing
  edit record-design-missing formal/r5-admit/record.txt '/^design fault=no_scope /d'
  expect record-design-missing "1:FAIL $name (record)" good \
    "full=ok designs=$designs_n reported=$((designs_n - 1)) reach=$reach_n reached=$reach_n"
  fixture record-design-other
  edit record-design-other formal/r5-admit/record.txt 's/^\(design fault=no_scope\) reports=Q3 /\1 reports=Q4 /'
  expect record-design-other "1:FAIL $name (record)" good "reported=$((designs_n - 1)) "
  fixture record-design-twice
  edit record-design-twice formal/r5-admit/record.txt '/^design fault=no_scope /p'
  expect record-design-twice "1:FAIL $name (record)" good "stamp=same full=ok $whole"
  fixture record-reach-not-reached
  edit record-reach-not-reached formal/r5-admit/record.txt 's/^\(reach property=NotListCut .*\) reached=yes /\1 reached=no /'
  expect record-reach-not-reached "1:FAIL $name (record)" good \
    "designs=$designs_n reported=$designs_n reach=$reach_n reached=$((reach_n - 1))"
  fixture record-design-added
  edit record-design-added formal/r5-admit/README.md '/^| .no_scope. | first round | /{p;s/no_scope/no_scope_twice/;}'
  expect record-design-added "1:FAIL $name (record)" good \
    "designs=$((designs_n + 1)) reported=$designs_n reach=$reach_n reached=$reach_n"
  fixture record-reach-added
  edit record-reach-added formal/r5-admit/README.md 's/^reach PROPERTY NeverCountedOnActive 5 0$/&\
reach INVARIANT NotListCut 3 2/'
  expect record-reach-added "1:FAIL $name (record)" good \
    "designs=$designs_n reported=$designs_n reach=$((reach_n + 1)) reached=$reach_n readme=ok"
  fixture record-reach-unreadable
  edit record-reach-unreadable formal/r5-admit/README.md 's/^reach PROPERTY NeverCountedOnActive 5 0$/& /'
  expect record-reach-unreadable "1:FAIL $name (record)" good 'stamp=same full=ok' 'readme=unreadable'
  fixture record-design-unreadable
  edit record-design-unreadable formal/r5-admit/README.md 's/^\(| .no_scope. | first round | .*\) | 3 runs | \(.*\)$/\1 | 3 runs | | \2/'
  expect record-design-unreadable "1:FAIL $name (record)" good 'stamp=same full=ok' 'readme=unreadable'
  fixture record-design-header-twice
  edit record-design-header-twice formal/r5-admit/README.md '/^| .Fault. | from | /p'
  expect record-design-header-twice "1:FAIL $name (record)" good 'stamp=same full=ok' 'readme=unreadable'
  fixture record-design-indented
  edit record-design-indented formal/r5-admit/README.md '/^| .no_scope. | first round | /{p;s/no_scope/no_scope_twice/;s/^/ /;}'
  expect record-design-indented "1:FAIL $name (record)" good 'stamp=same full=ok' 'readme=unreadable'
  fixture record-reach-pairing
  edit record-reach-pairing formal/r5-admit/README.md 's/^reach PROPERTY NeverCountedOnActive 5 0$/reach INVARIANT NeverCountedOnActive 5 0/'
  expect record-reach-pairing "1:FAIL $name (record)" good 'stamp=same full=ok' 'readme=unreadable'
  tab=$(printf '\t')
  fence=$(printf '\140\140\140')
  fixture record-reach-tab
  edit record-reach-tab formal/r5-admit/README.md "s/^reach PROPERTY NeverCountedOnActive 5 0\$/&\\
${tab}reach INVARIANT NotListCut 3 2/"
  expect record-reach-tab "1:FAIL $name (record)" good 'stamp=same full=ok' 'readme=unreadable'
  fixture record-reach-second-block
  edit record-reach-second-block formal/r5-admit/README.md "s/^| name | line of the configuration | bound |.*\$/${fence}sh\\
reach INVARIANT NotListCut 3 2\\
${fence}\\
&/"
  expect record-reach-second-block "1:FAIL $name (record)" good 'stamp=same full=ok' 'readme=unreadable'
  fixture record-extra-line
  printf 'note=x\n' >>"$work/record-extra-line/formal/r5-admit/record.txt" || fail selftest
  expect record-extra-line "1:FAIL $name (record)" good "stamp=same full=ok $whole readme=ok"
}

logged() {
  n_small=$(grep -c '^Admit Small\.cfg$' "$work/good/java.log")
  n_control=$(grep -c '^Admit Control\.cfg$' "$work/good/java.log")
  n_export=$(grep -c '^Export Export\.cfg$' "$work/good/java.log")
  n_all=$(grep -c . "$work/good/java.log")
  ran=no
  [ "$n_small:$n_control:$n_export:$n_all" = 1:2:1:4 ] && ran=yes
  printf 'selftest rule=runs held=%s\n' "$ran"
  [ "$ran" = yes ] || missed=$((missed + 1))
}

selftest() {
  label=self-test:$name
  locate
  [ -d "$dir" ] || fail selftest
  mkdir -p "$art" || fail selftest
  scratch
  [ -n "$tmp" ] || fail selftest
  work=$tmp/fixtures
  mkdir -p "$work/stub-curl" || fail selftest
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
