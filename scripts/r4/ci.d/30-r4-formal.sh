#!/bin/sh
# jobs: macos windows
# rule: TLC finds no invariant of the coordinator's two models broken in the small configurations that formal/r4-coordinator/README.md names for CI, the committed traces are what the committed models export, and each of two control copies breaks its invariant.
# lane: free
set -u
LC_ALL=C
export LC_ALL
unset CDPATH
unset JAVA_TOOL_OPTIONS JDK_JAVA_OPTIONS _JAVA_OPTIONS
name=30-r4-formal
label=$name
workers=3
ci_schedule=days,queue,store,holds,left,reap
ci_confirm=full,apart
case $0 in
  /*) self=$0 ;;
  *) self=$(pwd)/$0 ;;
esac

c1_old='ModelFree(s, a) == a \notin NeedsModel \/ \A r \in s.runs : r.agent \notin NeedsModel'
c1_new='ModelFree(s, a) == TRUE'
l5_old='ELSE IF s.items[i].st # "recorded" THEN No(s, "state")'
l5_new='ELSE IF s.items[i].st \notin {"answered", "recorded"} THEN No(s, "state")'

fail() {
  printf 'FAIL %s (%s)\n' "$label" "$1"
  exit 1
}

made=
cleanup() {
  case $made in
    */r4-formal.??????) rm -rf "$made" ;;
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
  dir=$repo/formal/r4-coordinator
  art=${CI_D_ARTIFACTS:-$repo/target/ci-artifacts/$name}
}

scratch() {
  tmp=$(mktemp -d "${TMPDIR:-/tmp}/r4-formal.XXXXXX") || tmp=
  made=$tmp
  [ -z "$tmp" ] || tmp=$(cd "$tmp" && pwd) || tmp=
  [ -z "$tmp" ] || made=$tmp
}

sum_of() {
  tr -d '\r' <"$1" >"$tmp/sum.txt" || return 1
  sum=$(sha256 <"$tmp/sum.txt") || return 1
  sum=${sum%% *}
  [ "${#sum}" -eq 64 ]
}

get_jar() {
  tr -d '\r' <"$dir/README.md" >"$tmp/readme.txt" || fail fetch
  line=$(sed -n 's/^jar url=\([^ ]*\) bytes=\([0-9][0-9]*\) sha256=\([0-9a-f]\{64\}\)$/\1 \2 \3/p' "$tmp/readme.txt")
  case $line in
    *' '*' '*' '* | '' | *"
"*) fail fetch ;;
  esac
  url=${line%% *}
  rest=${line#* }
  bytes=${rest% *}
  want=${rest#* }
  if [ -n "${R4_FORMAL_JAR:-}" ]; then
    source=given
    [ -f "$R4_FORMAL_JAR" ] || fail fetch
    cp "$R4_FORMAL_JAR" "$tmp/tla2tools.jar" || fail fetch
  else
    source=fetch
    command -v curl >/dev/null 2>&1 || fail fetch
    (cd "$tmp" && curl -q -fsSL --retry 2 --max-time 300 -o tla2tools.jar "$url") || fail fetch
    [ -f "$tmp/tla2tools.jar" ] || fail fetch
  fi
  size=$(wc -c <"$tmp/tla2tools.jar") || fail hash
  size=$((size + 0))
  sum=$(sha256 <"$tmp/tla2tools.jar") || fail hash
  sum=${sum%% *}
  printf 'formal jar source=%s bytes=%s sha256=%s\n' "$source" "$size" "$sum"
  [ "$size" = "$bytes" ] || fail hash
  [ "$sum" = "$want" ] || fail hash
}

check_java() {
  command -v java >/dev/null 2>&1 || fail java
  java -version >"$tmp/java.raw" 2>&1 </dev/null || fail java
  tr -d '\r' <"$tmp/java.raw" >"$tmp/java.txt" || fail java
  version=$(sed -n 's/.* version "\([0-9][0-9._]*\).*/\1/p' "$tmp/java.txt")
  version=${version%%"
"*}
  major=${version%%.*}
  case $major in
    '' | *[!0-9]*) fail java ;;
  esac
  printf 'formal java version=%s\n' "$version"
  [ "$major" -ge 17 ] || fail java
}

table() {
  : >"$tmp/configurations.txt" || fail invariant
  : >"$tmp/table-files.txt" || fail invariant
  awk -F'|' -v rowfile="$tmp/configurations.txt" -v namefile="$tmp/table-files.txt" '
    function trim(x) { gsub(/^[ \t]+/, "", x); gsub(/[ \t]+$/, "", x); return x }
    function bad(why) { faults++ }
    /^\| Model \| Configuration \| File \| CI \| / { heads++; state = 1; next }
    state == 1 { state = 2; next }
    state == 2 && /^\|/ {
      rows++
      model = trim($2); config = trim($3); file = trim($4); ci = trim($5); distinct = trim($7)
      if (NF != 10) bad("cells")
      else if (model !~ /^`(Schedule|Confirm4)`$/) bad("model")
      else if (config !~ /^`[a-z0-9-]+`$/) bad("name")
      else if (file !~ /^`[A-Za-z0-9._-]+\.cfg`$/) bad("file")
      else if (ci != "yes" && ci != "no") bad("ci")
      else if (distinct !~ /^[0-9][0-9,]*$/) bad("count")
      else if (trim($6) == "" || trim($8) == "" || trim($9) == "") bad("empty")
      else {
        gsub(/`/, "", model); gsub(/`/, "", config); gsub(/`/, "", file); gsub(/,/, "", distinct)
        if (ci == "yes") marked++
        printf "%s %s %s %s %s\n", model, config, file, distinct, ci >rowfile
        print file >namefile
      }
      next
    }
    state == 2 { state = 0 }
    /^\|[ \t]*`(Schedule|Confirm4)`[ \t]*\|/ && /\.cfg/ { bad("outside") }
    END { printf "%d %d %d %d\n", heads, rows, faults, marked }' "$tmp/readme.txt" >"$tmp/table-summary.txt" || fail invariant
  heads=0
  rows=0
  faults=1
  marked=0
  read -r heads rows faults marked <"$tmp/table-summary.txt" || fail invariant
  named='
'
  while read -r f; do
    named=$named$f'
'
  done <"$tmp/table-files.txt"
  unnamed=0
  for f in "$dir"/*.cfg; do
    [ -e "$f" ] || continue
    case $named in
      *"
${f##*/}
"*) ;;
      *) unnamed=$((unnamed + 1)) ;;
    esac
  done
  schedule_names=
  confirm_names=
  while read -r model config file want_distinct ci; do
    [ "$ci" = yes ] || continue
    case $model in
      Schedule) schedule_names=${schedule_names:+$schedule_names,}$config ;;
      *) confirm_names=${confirm_names:+$confirm_names,}$config ;;
    esac
  done <"$tmp/configurations.txt"
  printf 'formal table lines=%s marked=%s faults=%s unnamed=%s\n' "$rows" "$marked" "$faults" "$unnamed"
  printf 'formal configurations schedule=%s confirm=%s\n' "${schedule_names:--}" "${confirm_names:--}"
  [ "$heads" -eq 1 ] || fail invariant
  [ "$faults" -eq 0 ] || fail invariant
  [ "$unnamed" -eq 0 ] || fail invariant
  [ "$schedule_names" = "$ci_schedule" ] || fail invariant
  [ "$confirm_names" = "$ci_confirm" ] || fail invariant
}

run_tlc() {
  deadlock=
  [ "$2" = Confirm4 ] && deadlock=-deadlock
  (
    cd "$1" || exit 99
    java -Xmx2g -XX:+UseParallelGC -Djava.io.tmpdir=jtmp -cp ../tla2tools.jar tlc2.TLC \
      -workers "$workers" -metadir meta -config "$3" ${deadlock:+"$deadlock"} "$2" >out.raw 2>&1 </dev/null
  )
  status=$?
  tr -d '\r' <"$1/out.raw" >"$1/out.txt" || status=99
  cp "$1/out.txt" "$art/${1##*/}.txt" || status=99
  : >"$1/violated.txt" || status=99
  awk -v names="$1/violated.txt" '
    /^Error: Invariant [A-Za-z0-9_]* is violated/ { print $3 >names }
    /^[0-9][0-9]* states generated, [0-9][0-9]* distinct states found, 0 states left on queue\.$/ { n++; all = $1; found = $4 }
    $0 == "Model checking completed. No error has been found." { whole = 1 }
    END {
      if (n != 1) { all = 0; found = 0 }
      printf "%s %s %d\n", all, found, whole
    }' "$1/out.txt" >"$1/summary.txt" || status=99
  generated=0
  distinct=0
  complete=0
  read -r generated distinct complete <"$1/summary.txt" || status=99
}

declared() {
  case $1 in
    Schedule)
      key=schedule
      letter=C
      count=12
      ;;
    *)
      key=confirm
      letter=L
      count=10
      ;;
  esac
  result=missing
  [ -f "$dir/$2" ] || return 0
  result=$(awk -v letter="$letter" -v count="$count" -v ci="$3" '
    {
      sub(/\r$/, "")
      if (substr($0, 1, 10) == "INVARIANT ") { lines++; have[substr($0, 11)] = 1 }
      if (index($0, "(*") > 0 || index($0, "\\*") > 0) comment = 1
      line = $0
      gsub(/\t/, " ", line)
      flat = flat " " line
    }
    END {
      for (i = 1; i <= count; i++) if (!((letter i) in have)) { print "invariants"; exit }
      if (comment) { print "comment"; exit }
      if (lines != count) { print "invariants"; exit }
      if (ci == "yes") {
        for (i = 1; i <= count; i++) if (flat ~ ("(^| )" letter i " *<-")) { print "replaced"; exit }
      }
      print "ok"
    }' "$dir/$2") || result=missing
  [ -n "$result" ] || result=missing
}

small() {
  model=$1
  config=$2
  file=$3
  want_distinct=$4
  run=$tmp/$key-$config
  mkdir "$run" "$run/jtmp" || fail invariant
  status=-
  distinct=0
  generated=0
  t0=$t1
  if [ "$result" = ok ]; then
    cp "$dir"/*.tla "$dir/$file" "$run/" || fail invariant
    run_tlc "$run" "$model" "$file"
    while read -r one; do
      printf '%s\n' "$one"
    done <"$run/violated.txt" >>"$tmp/violated-$key.txt" || status=99
    if [ -s "$run/violated.txt" ]; then
      result=violated
    elif [ "$status" != 0 ]; then
      result=status
    elif [ "$complete" != 1 ]; then
      result=incomplete
    elif [ "$distinct" != "$want_distinct" ]; then
      result=counts
    fi
  fi
  t1=$(date +%s)
  printf 'formal run model=%s configuration=%s status=%s states=%s generated=%s seconds=%s result=%s\n' \
    "$key" "$config" "$status" "$distinct" "$generated" "$((t1 - t0))" "$result"
  ran=$((ran + 1))
  [ "$result" = ok ] || bad=$((bad + 1))
  if [ "$key" = schedule ]; then
    schedule_states=$((schedule_states + distinct))
  else
    confirm_states=$((confirm_states + distinct))
  fi
}

models() {
  schedule_states=0
  confirm_states=0
  bad=0
  ran=0
  : >"$tmp/violated-schedule.txt" || fail invariant
  : >"$tmp/violated-confirm.txt" || fail invariant
  t1=$(date +%s)
  while read -r model config file want_distinct ci; do
    declared "$model" "$file" "$ci"
    if [ "$ci" = yes ]; then
      small "$model" "$config" "$file" "$want_distinct"
    elif [ "$result" != ok ]; then
      printf 'formal run model=%s configuration=%s status=- states=0 generated=0 seconds=0 result=%s\n' \
        "$key" "$config" "$result"
      bad=$((bad + 1))
    fi
  done <"$tmp/configurations.txt"
  sort -u "$tmp/violated-schedule.txt" >"$tmp/violated-schedule.names" || fail invariant
  sort -u "$tmp/violated-confirm.txt" >"$tmp/violated-confirm.names" || fail invariant
  broken_schedule=$(grep -c . "$tmp/violated-schedule.names")
  broken_confirm=$(grep -c . "$tmp/violated-confirm.names")
  printf 'formal schedule states=%s invariants=12 violated=%s\n' "$schedule_states" "$broken_schedule"
  printf 'formal confirm states=%s invariants=10 violated=%s\n' "$confirm_states" "$broken_confirm"
  [ "$bad" -eq 0 ] || fail invariant
  if [ "$ran" -ne "$marked" ]; then
    printf 'formal runs=%s marked=%s\n' "$ran" "$marked"
    fail invariant
  fi
}

stamp_of() {
  (cd "$1" && cat Schedule.tla Schedule.cfg Schedule-holds.cfg Schedule-left.cfg Schedule-reap.cfg \
    Confirm4.tla Confirm4.cfg ScheduleTrace.tla Confirm4Trace.tla TraceText.tla limits.json) >"$2.bin" || return 1
  tr -d '\r' <"$2.bin" >"$2.txt" || return 1
  stamp=$(sha256 <"$2.txt") || return 1
  stamp=${stamp%% *}
  [ "${#stamp}" -eq 64 ]
}

first_line() {
  sed -n '1s/^{"seed":\([0-9][0-9]*\),"count":\([0-9][0-9]*\),"stamp":"\([0-9a-f]\{64\}\)"}$/\1 \2 \3/p' "$1"
}

stamps() {
  s=$dir/traces-schedule.jsonl
  c=$dir/traces-confirm.jsonl
  schedule_traces=0
  confirm_traces=0
  fault=
  if [ -f "$s" ] && [ -f "$c" ]; then
    first_s=$(first_line "$s")
    first_c=$(first_line "$c")
    lines_s=$(wc -l <"$s") || lines_s=0
    lines_c=$(wc -l <"$c") || lines_c=0
    schedule_traces=$((lines_s - 1))
    confirm_traces=$((lines_c - 1))
    stamp_of "$dir" "$tmp/stamp" || stamp=
    if [ -z "$first_s" ] || [ -z "$first_c" ]; then
      fault=first
    elif [ "$first_s" != "$first_c" ]; then
      fault=pair
    elif [ "${first_s##* }" != "$stamp" ]; then
      fault=stamp
    else
      count=${first_s#* }
      count=${count% *}
      [ "$schedule_traces" -eq $((count + 13)) ] || fault=lines
      [ "$confirm_traces" -eq $((count + 15)) ] || fault=lines
    fi
  else
    fault=missing
  fi
  outside=0
  for f in "$dir"/*.tla "$dir"/*.cfg "$dir"/*.json; do
    [ -e "$f" ] || continue
    case ${f##*/} in
      Schedule.tla | Schedule.cfg | Confirm4.tla | Confirm4.cfg | ScheduleTrace.tla | Confirm4Trace.tla) ;;
      TraceText.tla | limits.json | Schedule-holds.cfg | Schedule-left.cfg | Schedule-reap.cfg) ;;
      *)
        case $named in
          *"
${f##*/}
"*) ;;
          *) outside=$((outside + 1)) ;;
        esac
        ;;
    esac
  done
  [ "$outside" -eq 0 ] || fault=${fault:-outside}
  named=$(grep -c '^export compared in jobs:' "$tmp/readme.txt")
  [ "$named" -eq 1 ] || fault=${fault:-jobs}
  jobs=$(sed -n '/^export compared in jobs:/{s///;p;}' "$tmp/readme.txt")
  case " $jobs " in
    *' macos '* | *' windows '*) ;;
    *) fault=${fault:-jobs} ;;
  esac
  other=$(sed -n '/^export compared in jobs:/{s///;s/ macos//;s/ windows//;p;}' "$tmp/readme.txt")
  [ -z "$other" ] || fault=${fault:-jobs}
  if [ -n "$fault" ]; then
    printf 'formal traces fault=%s\n' "$fault"
    printf 'formal traces schedule=%s confirm=%s same=-\n' "$schedule_traces" "$confirm_traces"
    fail traces
  fi
}

record() {
  fault=
  have_stamp=-
  have_script=-
  whole=no
  if [ -f "$dir/gate-record.txt" ]; then
    tr -d '\r' <"$dir/gate-record.txt" >"$tmp/record.txt" || fail record
    awk '
      /^stamp=/ { s++; stamp = substr($0, 7) }
      /^script=/ { c++; script = substr($0, 8) }
      /^selftest / { n++; if ($0 !~ /=yes$/) no++ }
      /^results=/ { t++; total = substr($0, 9) }
      END {
        if (s != 1 || length(stamp) != 64 || stamp !~ /^[0-9a-f]*$/) stamp = "-"
        if (c != 1 || length(script) != 64 || script !~ /^[0-9a-f]*$/) script = "-"
        printf "%s %s %s\n", stamp, script, (t == 1 && n > 0 && total == n && no == 0) ? "yes" : "no"
      }' "$tmp/record.txt" >"$tmp/record-summary.txt" || fail record
    read -r have_stamp have_script whole <"$tmp/record-summary.txt" || fail record
    sum_of "$self" || fail record
    if [ "$have_stamp" = - ]; then
      fault=no-stamp
    elif [ "$have_stamp" != "$stamp" ]; then
      fault=stamp
    elif [ "$have_script" != "$sum" ]; then
      fault=script
    elif [ "$whole" != yes ]; then
      fault=lines
    fi
  else
    fault=missing
  fi
  printf 'formal record fault=%s\n' "${fault:-none}"
  [ -z "$fault" ] || fail record
}

compare() {
  same=-
  jobs=$(sed -n 's/^export compared in jobs:\(.*\)$/\1/p' "$tmp/readme.txt")
  compare=no
  if [ -z "${CI_D_JOB:-}" ]; then
    compare=yes
  fi
  case " $jobs " in
    *" ${CI_D_JOB:-none} "*) compare=yes ;;
  esac
  if [ "$compare" = yes ]; then
    mkdir "$tmp/export" || fail traces
    t0=$(date +%s)
    TMPDIR=$tmp sh "$dir/export.sh" --jar "$tmp/tla2tools.jar" --out "$tmp/export" >"$tmp/export.txt" 2>&1 </dev/null
    code=$?
    t1=$(date +%s)
    cp "$tmp/export.txt" "$art/export.txt" || fail traces
    same=no
    if [ "$code" -eq 0 ] && cmp -s "$s" "$tmp/export/traces-schedule.jsonl" &&
      cmp -s "$c" "$tmp/export/traces-confirm.jsonl"; then
      same=yes
    fi
    printf 'formal export status=%s seconds=%s\n' "$code" "$((t1 - t0))"
  fi
  printf 'formal traces schedule=%s confirm=%s same=%s\n' "$schedule_traces" "$confirm_traces" "$same"
  [ "$same" != no ] || fail traces
}

control() {
  run=$tmp/control-$4
  mkdir "$run" "$run/jtmp" || return 2
  cp "$dir"/*.tla "$dir/$2" "$run/" || return 2
  R4_OLD=$5 R4_NEW=$6 awk '
    BEGIN { old = ENVIRON["R4_OLD"]; to = ENVIRON["R4_NEW"]; n = 0 }
    {
      i = index($0, old)
      if (i > 0) {
        n++
        $0 = substr($0, 1, i - 1) to substr($0, i + length(old))
      }
      print
    }
    END { if (n != 1) exit 3 }' "$dir/$1.tla" >"$run/$1.tla" || return 2
  run_tlc "$run" "$1" "$2"
  first=
  read -r first <"$run/violated.txt" || first=
  printf 'formal control copy=%s status=%s broken=%s\n' "$4" "$status" "${first:--}"
  [ "$status" = 12 ] || return 1
  [ "$first" = "$4" ] || return 1
}

real_control() {
  case $1 in
    C1) control Schedule Schedule-queue.cfg - C1 "$c1_old" "$c1_new" ;;
    *) control Confirm4 Confirm4.cfg - L5 "$l5_old" "$l5_new" ;;
  esac
}

controls() {
  broken=0
  applied=yes
  for copy in C1 L5; do
    real_control "$copy"
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

measure() {
  locate
  [ -d "$dir" ] || fail fetch
  mkdir -p "$art" || fail fetch
  scratch
  [ -n "$tmp" ] || fail fetch
  get_jar
  check_java
  table
  stamps
  record
  models
  compare
  controls
  printf 'PASS %s\n' "$name"
}

write_record() {
  {
    printf 'gate record of formal/r4-coordinator: sh scripts/r4/ci.d/30-r4-formal.sh --record\n'
    printf 'stamp=%s\nscript=%s\n' "$2" "$3"
    cat "$4"
    printf 'results=%s\n' "$5"
  } >"$1" || fail selftest
}

fixture() {
  mkdir -p "$work/$1/formal" || fail selftest
  cp -R "$repo/formal/r4-coordinator" "$work/$1/formal/" || fail selftest
  fdir=$work/$1/formal/r4-coordinator
  write_record "$fdir/gate-record.txt" "$real_stamp" "$self_sum" "$work/one-line.txt" 1
}

restamp() {
  stamp_of "$fdir" "$work/$1/stamp" || fail selftest
  for f in traces-schedule.jsonl traces-confirm.jsonl; do
    old=$(first_line "$fdir/$f")
    [ -n "$old" ] || fail selftest
    seed=${old%% *}
    rest=${old#* }
    {
      printf '{"seed":%s,"count":%s,"stamp":"%s"}\n' "$seed" "${rest% *}" "$stamp"
      sed 1d "$fdir/$f"
    } >"$work/$1/$f" || fail selftest
    mv "$work/$1/$f" "$fdir/$f" || fail selftest
  done
  write_record "$fdir/gate-record.txt" "$stamp" "$self_sum" "$work/one-line.txt" 1
}

say() {
  printf '%s\n' "$1"
  printf '%s\n' "$1" >>"$work/lines.txt" || missed=$((missed + 1))
}

expect() {
  out=$(
    cd "$work" || exit 98
    CI_D_REPO=$work/$1
    CI_D_ARTIFACTS=$work/$1/art
    CI_D_JOB=$job_as
    R4_STUB_MODE=$3
    R4_STUB_COUNTS=$work/counts.txt
    R4_STUB_TRACES=${traces_from:-$work/$1/formal/r4-coordinator}
    R4_STUB_LOG=$work/$1/java-$3.log
    R4_STUB_CURL=$work/$1/curl-$3.log
    JAVA_TOOL_OPTIONS=-Dr4.stub=1
    JDK_JAVA_OPTIONS=-Dr4.stub=2
    _JAVA_OPTIONS=-Dr4.stub=3
    TMPDIR="${tmp_as:-$work/t m p}"
    PATH=$work/stub:$PATH
    export CI_D_REPO CI_D_ARTIFACTS CI_D_JOB R4_STUB_MODE R4_STUB_COUNTS R4_STUB_TRACES R4_STUB_LOG R4_STUB_CURL
    export JAVA_TOOL_OPTIONS JDK_JAVA_OPTIONS _JAVA_OPTIONS TMPDIR PATH
    if [ "$job_as" = none ]; then
      unset CI_D_JOB
    fi
    : >"$R4_STUB_LOG"
    : >"$R4_STUB_CURL"
    if [ "$jar_from" = fetch ]; then
      unset R4_FORMAL_JAR
      PATH=$work/stub-curl:$PATH
    else
      R4_FORMAL_JAR=$jar_from
      export R4_FORMAL_JAR
    fi
    sh "$self" ${4:+"$4"} 2>&1
  )
  code=$?
  last=${out##*"
"}
  case $out in
    *"${5:-}"*) ;;
    *) code=line ;;
  esac
  case $out in
    *"${6:-}"*) ;;
    *) code=line ;;
  esac
  if [ "$code:$last" = "$2" ]; then
    say "selftest fixture=$1 mode=$3 as-expected=yes"
    case $2 in
      0:*) ;;
      *) rejected=$((rejected + 1)) ;;
    esac
  else
    say "selftest fixture=$1 mode=$3 as-expected=no"
    printf '%s\n' "$out" >"$art/selftest-$1-$3.txt"
    missed=$((missed + 1))
  fi
  traces_from=
  jar_from=$tmp/tla2tools.jar
  job_as=macos
  tmp_as=
}

held() {
  if "$@"; then
    say "selftest rule=$rule held=yes"
  else
    say "selftest rule=$rule held=no"
    missed=$((missed + 1))
  fi
}

absolute() {
  (cd "$work" && TMPDIR=. && export TMPDIR && scratch && [ "${tmp#/}" != "$tmp" ])
}

emptied() {
  set -- "$work/t m p"/*
  [ ! -e "$1" ]
}

recorded_jar() {
  awk -v given="cp=$tmp/tla2tools.jar " '
    $1 ~ /^module=(Schedule|Confirm4)$/ { n++; if (index($0, " cp=../tla2tools.jar cwd=") == 0) bad = 1 }
    index($0, given) > 0 { bad = 1 }
    END { exit (bad || n < 3) }' "$work/good/java-good.log"
}

recorded_deadlock() {
  awk '
    $1 == "module=Confirm4" { c++; if ($3 != "deadlock=yes") bad = 1 }
    $1 == "module=Schedule" { s++; if ($3 != "deadlock=no") bad = 1 }
    END { exit (bad || c == 0 || s == 0) }' "$work/good/java-good.log"
}

recorded_configurations() {
  awk '
    NR == FNR { if ($5 == "yes") want[$1 " " $3] = 1; next }
    { seen[substr($1, 8) " " substr($2, 8)] = 1 }
    END { for (k in want) if (!(k in seen)) bad = 1; exit (bad + 0) }' "$work/counts.txt" "$work/good/java-good.log"
}

edit() {
  sed "$3" "$fdir/$2" >"$work/edited" || fail selftest
  cmp -s "$work/edited" "$fdir/$2" && fail selftest
  mv "$work/edited" "$fdir/$2" || fail selftest
}

other_digit() {
  case $1 in
    0*) digit=1 ;;
    *) digit=0 ;;
  esac
}

stubs() {
  cat >"$work/stub/java" <<'EOF' || fail selftest
#!/bin/sh
mode=${R4_STUB_MODE:-good}
if [ -n "${JAVA_TOOL_OPTIONS:-}${JDK_JAVA_OPTIONS:-}${_JAVA_OPTIONS:-}" ]; then
  printf 'a java option variable reached java\r\n' >&2
  exit 96
fi
if [ "${1:-}" = -version ]; then
  case $mode in
    java-old) printf 'openjdk version "16.0.2" 2021-07-20\r\n' >&2 ;;
    java-unreadable) printf 'a java with no version line\r\n' >&2 ;;
    signal)
      kill -s TERM "$PPID"
      printf 'openjdk version "17.0.1" 2021-10-19\r\n' >&2
      ;;
    *) printf 'Picked up a note\r\nopenjdk version "17.0.1" 2021-10-19\r\n' >&2 ;;
  esac
  exit 0
fi
cfg=
module=
jar=
deadlock=no
while [ $# -gt 0 ]; do
  [ "$1" = -config ] && cfg=$2
  [ "$1" = -cp ] && jar=$2
  [ "$1" = -deadlock ] && deadlock=yes
  module=$1
  shift
done
if [ -n "${R4_STUB_LOG:-}" ]; then
  printf 'module=%s config=%s deadlock=%s cp=%s cwd=%s\n' "$module" "$cfg" "$deadlock" "$jar" "$PWD" >>"$R4_STUB_LOG"
fi
case $module in
  ScheduleTrace) file=traces-schedule.jsonl ;;
  Confirm4Trace) file=traces-confirm.jsonl ;;
  *) file= ;;
esac
if [ -n "$file" ]; then
  if grep -q 'CONSTANT Config = "full"' "$cfg"; then
    sed -e 1d -e 's/"/\\"/g' -e 's/^/"/' -e 's/$/"/' "$R4_STUB_TRACES/$file"
  fi
  exit 0
fi
copy=
if [ "$module" = Schedule ] && grep -q -F 'ModelFree(s, a) == TRUE' Schedule.tla; then
  copy=C1
fi
if [ "$module" = Confirm4 ] && grep -q -F '\notin {"answered", "recorded"} THEN No(s, "state")' Confirm4.tla; then
  copy=L5
fi
good() {
  printf 'Model checking completed. No error has been found.\r\n'
  printf '%s states generated, %s distinct states found, 0 states left on queue.\r\n' "$2" "$1"
}
if [ -n "$copy" ]; then
  case $mode in
    control-holds)
      good 5 9
      exit 0
      ;;
    control-other) printf 'Error: Invariant C2 is violated.\r\n' ;;
    control-one)
      if [ "$copy" = L5 ]; then
        good 5 9
        exit 0
      fi
      printf 'Error: Invariant %s is violated.\r\n' "$copy"
      ;;
    control-status) printf 'Error: Invariant %s is violated.\r\n' "$copy"; exit 0 ;;
    *) printf 'Error: Invariant %s is violated.\r\n' "$copy" ;;
  esac
  exit 12
fi
if [ "$mode" = reads-input ]; then
  cat >/dev/null
fi
distinct=0
while read -r one two three four five; do
  if [ "$three" = "$cfg" ]; then
    distinct=$four
  fi
done <"$R4_STUB_COUNTS"
generated=$((distinct * 3))
case $mode:$cfg in
  violated-apart:Confirm4-apart.cfg)
    printf 'Error: Invariant L3 is violated.\r\n'
    exit 12
    ;;
  silent:Schedule-queue.cfg) exit 0 ;;
  killed:Schedule-days.cfg)
    printf 'Progress(9): 1,000 states generated\r\n'
    exit 137
    ;;
  violated:Schedule-days.cfg)
    printf 'Error: Invariant C7 is violated.\r\n'
    printf '90 states generated, 40 distinct states found, 7 states left on queue.\r\n'
    exit 12
    ;;
  violated-status:Confirm4.cfg)
    printf 'Error: Invariant L7 is violated.\r\n'
    good "$distinct" "$generated"
    exit 0
    ;;
  status:Confirm4.cfg)
    good "$distinct" "$generated"
    exit 1
    ;;
  distinct:Schedule-queue.cfg) good "$((distinct - 1))" "$generated" ;;
  unfinished:Confirm4.cfg) printf '%s states generated, %s distinct states found, 0 states left on queue.\r\n' "$generated" "$distinct" ;;
  *) good "$distinct" "$generated" ;;
esac
exit 0
EOF
  cat >"$work/stub-curl/curl" <<'EOF' || fail selftest
#!/bin/sh
printf '%s\n' "$*" >>"$R4_STUB_CURL"
exit 22
EOF
  chmod +x "$work/stub/java" "$work/stub-curl/curl" || fail selftest
}

every_push() {
  expect good "1:FAIL $name (usage)" good --other
  jar_from=fetch
  expect good "1:FAIL $name (fetch)" fetch
  rule=download
  held grep -q '^-q ' "$work/good/curl-fetch.log"
  {
    printf 'X'
    tail -c +2 "$tmp/tla2tools.jar"
  } >"$work/other.jar" || fail selftest
  jar_from=$work/other.jar
  expect good "1:FAIL $name (hash)" other-bytes
  expect good "1:FAIL $name (java)" java-old

  fixture line-beside-its-copy
  edit line-beside-its-copy README.md 's/^\(| .Schedule. | .queue. | .Schedule-queue\.cfg. |\)\(.*\)$/\1\2\
\1/'
  expect line-beside-its-copy "1:FAIL $name (invariant)" good
  fixture unnamed-configuration
  cp "$fdir/Schedule-queue.cfg" "$fdir/Schedule-more.cfg" || fail selftest
  expect unnamed-configuration "1:FAIL $name (invariant)" good
  fixture days-not-for-ci
  edit days-not-for-ci README.md "$elsewhere"
  edit days-not-for-ci README.md 's/\(| .Schedule-days\.cfg. |\) yes |/\1 no |/'
  expect days-not-for-ci "1:FAIL $name (invariant)" good

  fixture invariant-twice
  edit invariant-twice Schedule-queue.cfg 's/^INVARIANT C1$/INVARIANT C10/'
  expect invariant-twice "1:FAIL $name (invariant)" good '' ' result=invariants'
  fixture replaced-invariant
  printf 'CONSTANT C1 <- C2\n' >>"$fdir/Schedule-queue.cfg" || fail selftest
  expect replaced-invariant "1:FAIL $name (invariant)" good '' ' result=replaced'
  fixture comment-mark
  {
    printf '\\* a note\n'
    cat "$fdir/Schedule-store.cfg"
  } >"$work/marked.cfg" || fail selftest
  mv "$work/marked.cfg" "$fdir/Schedule-store.cfg" || fail selftest
  expect comment-mark "1:FAIL $name (invariant)" good '' ' result=comment'

  expect good "1:FAIL $name (invariant)" violated '' ' invariants=12 violated=1' ' result=violated'
  expect good "1:FAIL $name (invariant)" status '' ' result=status'
  expect good "1:FAIL $name (invariant)" distinct '' ' result=counts'
  expect good "1:FAIL $name (invariant)" unfinished '' ' result=incomplete'

  fixture two-stamps-traces-schedule.jsonl
  edit two-stamps-traces-schedule.jsonl README.md "$elsewhere"
  edit two-stamps-traces-schedule.jsonl traces-schedule.jsonl '1s/"stamp":"[0-9a-f]\{4\}/"stamp":"0000/'
  expect two-stamps-traces-schedule.jsonl "1:FAIL $name (traces)" good '' 'formal traces fault=pair'
  fixture other-models
  edit other-models README.md "$elsewhere"
  printf '\n' >>"$fdir/Confirm4.tla" || fail selftest
  expect other-models "1:FAIL $name (traces)" good '' 'formal traces fault=stamp'
  fixture line-added-traces-schedule.jsonl
  edit line-added-traces-schedule.jsonl README.md "$elsewhere"
  edit line-added-traces-schedule.jsonl traces-schedule.jsonl 9p
  expect line-added-traces-schedule.jsonl "1:FAIL $name (traces)" good '' 'formal traces fault=lines'
  fixture module-outside
  cp "$fdir/TraceText.tla" "$fdir/Extra.tla" || fail selftest
  expect module-outside "1:FAIL $name (traces)" good '' 'formal traces fault=outside'
  fixture no-job-named
  edit no-job-named README.md 's/^export compared in jobs:.*$/export compared in jobs:/'
  expect no-job-named "1:FAIL $name (traces)" good '' 'formal traces fault=jobs'

  fixture record-missing
  mv "$fdir/gate-record.txt" "$work/record-away.txt" || fail selftest
  expect record-missing "1:FAIL $name (record)" good '' 'formal record fault=missing'
  fixture record-no-stamp
  edit record-no-stamp gate-record.txt '/^stamp=/d'
  expect record-no-stamp "1:FAIL $name (record)" good '' 'formal record fault=no-stamp'
  fixture record-one-digit
  other_digit "$real_stamp"
  edit record-one-digit gate-record.txt "s/^stamp=./stamp=$digit/"
  expect record-one-digit "1:FAIL $name (record)" good '' 'formal record fault=stamp'
  fixture record-other-script
  other_digit "$self_sum"
  edit record-other-script gate-record.txt "s/^script=./script=$digit/"
  expect record-other-script "1:FAIL $name (record)" good '' 'formal record fault=script'
  fixture record-cut
  edit record-cut gate-record.txt 's/^results=.*$/results=2/'
  expect record-cut "1:FAIL $name (record)" good '' 'formal record fault=lines'

  fixture changed-line-no-job
  edit changed-line-no-job README.md "$elsewhere"
  edit changed-line-no-job traces-schedule.jsonl '9s/"r":"ok"/"r":"state"/'
  traces_from=$dir
  job_as=none
  expect changed-line-no-job "1:FAIL $name (traces)" good '' ' same=no'

  expect not-compared "1:FAIL $name (control)" control-holds
  fixture control-moved
  edit control-moved README.md "$elsewhere"
  edit control-moved Schedule.tla 's/^ModelFree(s, a) == a /ModelFree(s, a) ==  a /'
  restamp control-moved
  expect control-moved "1:FAIL $name (control)" good '' 'formal control applied=no'
  rule=removed-after-a-failure
  held emptied
}

for_the_record() {
  expect not-compared "0:PASS $name" good '' ' same=-'
  expect not-compared "0:PASS $name" reads-input '' ' configuration=apart '
  tmp_as='t m p'
  expect not-compared "0:PASS $name" relative '' ' same=-'
  jar_from=$work/no-such.jar
  expect good "1:FAIL $name (fetch)" no-file
  cat "$tmp/tla2tools.jar" "$work/stub-curl/curl" >"$work/longer.jar" || fail selftest
  jar_from=$work/longer.jar
  expect good "1:FAIL $name (hash)" other-size
  expect good "1:FAIL $name (java)" java-unreadable
  for mode in silent killed violated violated-status; do
    expect good "1:FAIL $name (invariant)" "$mode"
  done
  expect good "1:FAIL $name (invariant)" violated-status '' ' invariants=10 violated=1'
  for mode in control-other control-status control-one; do
    expect not-compared "1:FAIL $name (control)" "$mode"
  done

  fixture line-deleted
  edit line-deleted README.md "$elsewhere"
  edit line-deleted README.md '/^| .Schedule. | .days. | .Schedule-days\.cfg. |/d'
  mv "$fdir/Schedule-days.cfg" "$work/days-away.cfg" || fail selftest
  expect line-deleted "1:FAIL $name (invariant)" good
  fixture apart-not-for-ci
  edit apart-not-for-ci README.md "$elsewhere"
  edit apart-not-for-ci README.md 's/\(| .Confirm4-apart\.cfg. |\) yes |/\1 no |/'
  expect apart-not-for-ci "1:FAIL $name (invariant)" good
  fixture two-spaces
  edit two-spaces README.md "$elsewhere"
  edit two-spaces README.md 's/\(| .Confirm4-apart\.cfg. |\) yes |/\1  yes  |/'
  expect two-spaces "1:FAIL $name (invariant)" violated-apart '' ' configuration=apart status=12 '
  fixture name-with-a-dash
  edit name-with-a-dash README.md "$elsewhere"
  edit name-with-a-dash README.md 's/| \(.\)apart. | .Confirm4-apart/| \1a-part\1 | \1Confirm4-apart/'
  expect name-with-a-dash "1:FAIL $name (invariant)" good
  fixture empty-count
  edit empty-count README.md 's/\(| .Schedule-queue\.cfg. | yes | [^|]* |\) [0-9,]* |/\1  |/'
  expect empty-count "1:FAIL $name (invariant)" good
  fixture line-cut-short
  edit line-cut-short README.md 's/\(| .Schedule-store\.cfg. | yes | [^|]* |\).*$/\1/'
  expect line-cut-short "1:FAIL $name (invariant)" good

  fixture renamed-invariant
  edit renamed-invariant Schedule-days.cfg 's/^INVARIANT C7$/INVARIANT C1/'
  expect renamed-invariant "1:FAIL $name (invariant)" good
  fixture extra-invariant
  edit extra-invariant Schedule-store.cfg 's/^INVARIANT C7$/INVARIANT C7\
INVARIANT C7/'
  expect extra-invariant "1:FAIL $name (invariant)" good
  fixture comment-in-configuration
  {
    printf '(*\n'
    cat "$fdir/Schedule-store.cfg"
    printf '*)\n'
  } >"$work/commented.cfg" || fail selftest
  mv "$work/commented.cfg" "$fdir/Schedule-store.cfg" || fail selftest
  expect comment-in-configuration "1:FAIL $name (invariant)" good
  fixture full-without-an-invariant
  edit full-without-an-invariant README.md "$elsewhere"
  edit full-without-an-invariant Schedule.cfg '/^INVARIANT C12$/d'
  restamp full-without-an-invariant
  expect full-without-an-invariant "1:FAIL $name (invariant)" good
  fixture no-schedule-for-ci
  edit no-schedule-for-ci README.md 's/\(| .Schedule-[a-z]*\.cfg. |\) yes |/\1 no |/'
  expect no-schedule-for-ci "1:FAIL $name (invariant)" good
  fixture no-confirm-for-ci
  edit no-confirm-for-ci README.md 's/\(| .Confirm4[a-z-]*\.cfg. |\) yes |/\1 no |/'
  expect no-confirm-for-ci "1:FAIL $name (invariant)" good

  fixture unknown-job
  edit unknown-job README.md 's/^export compared in jobs:.*$/export compared in jobs: macos linux/'
  expect unknown-job "1:FAIL $name (traces)" good
  fixture no-jobs-line
  edit no-jobs-line README.md '/^export compared in jobs:/d'
  expect no-jobs-line "1:FAIL $name (traces)" good
  fixture line-added-traces-confirm.jsonl
  edit line-added-traces-confirm.jsonl README.md "$elsewhere"
  edit line-added-traces-confirm.jsonl traces-confirm.jsonl 9p
  expect line-added-traces-confirm.jsonl "1:FAIL $name (traces)" good
  fixture two-stamps-traces-confirm.jsonl
  edit two-stamps-traces-confirm.jsonl README.md "$elsewhere"
  edit two-stamps-traces-confirm.jsonl traces-confirm.jsonl '1s/"stamp":"[0-9a-f]\{4\}/"stamp":"0000/'
  expect two-stamps-traces-confirm.jsonl "1:FAIL $name (traces)" good
  for file in traces-schedule.jsonl traces-confirm.jsonl; do
    fixture "line-removed-$file"
    edit "line-removed-$file" README.md "$elsewhere"
    edit "line-removed-$file" "$file" 9d
    expect "line-removed-$file" "1:FAIL $name (traces)" good
    fixture "changed-line-$file"
    edit "changed-line-$file" "$file" '9s/"r":"ok"/"r":"state"/'
    traces_from=$dir
    expect "changed-line-$file" "1:FAIL $name (traces)" good
  done
  for file in Schedule-holds.cfg Schedule-left.cfg Schedule-reap.cfg; do
    fixture "other-$file"
    edit "other-$file" README.md "$elsewhere"
    printf '\n' >>"$fdir/$file" || fail selftest
    expect "other-$file" "1:FAIL $name (traces)" good
  done

  fixture control-twice
  edit control-twice README.md "$elsewhere"
  sed -n '/^ModelFree(s, a) == a /p' "$fdir/Schedule.tla" >"$work/twice.txt" || fail selftest
  cat "$work/twice.txt" >>"$fdir/Schedule.tla" || fail selftest
  restamp control-twice
  expect control-twice "1:FAIL $name (control)" good '' 'formal control applied=no'

  expect good "143:formal jar source=given bytes=$bytes sha256=$want" signal
  rule=removed-after-a-signal
  held emptied
}

selftest() {
  kind=$1
  label=$kind:$name
  locate
  [ -d "$dir" ] || fail fetch
  mkdir -p "$art" || fail fetch
  scratch
  [ -n "$tmp" ] || fail fetch
  get_jar
  check_java
  work=$tmp/fixtures
  mkdir -p "$work/stub" "$work/stub-curl" "$work/t m p" || fail selftest
  : >"$work/lines.txt" || fail selftest
  printf 'selftest fixture=one mode=one as-expected=yes\n' >"$work/one-line.txt" || fail selftest
  table >/dev/null
  cp "$tmp/configurations.txt" "$work/counts.txt" || fail selftest
  stamp_of "$dir" "$tmp/real-stamp" || fail selftest
  real_stamp=$stamp
  sum_of "$self" || fail selftest
  self_sum=$sum
  stubs
  rejected=0
  missed=0
  traces_from=
  jar_from=$tmp/tla2tools.jar
  job_as=macos
  tmp_as=
  elsewhere='s/^export compared in jobs:.*$/export compared in jobs: windows/'

  fixture good
  expect good "0:PASS $name" good '' ' same=yes' 'formal record fault=none'
  fixture not-compared
  edit not-compared README.md "$elsewhere"
  tmp_as='t m p'
  expect not-compared "0:PASS $name" reads-input '' ' same=-' ' configuration=apart '
  rule=scratch
  held absolute
  rule=jar
  held recorded_jar
  rule=deadlock
  held recorded_deadlock
  rule=configurations
  held recorded_configurations
  rule=removed-after-a-pass
  held emptied
  if [ "$missed" -ne 0 ]; then
    printf 'selftest a fixture that must pass did not pass, or a rule of the recorded runs did not hold\n'
    fail selftest
  fi
  every_push
  if [ "$kind" = record ]; then
    for_the_record
  fi

  keep=$tmp
  tmp="$keep/real copies"
  mkdir "$tmp" || fail selftest
  cp "$keep/tla2tools.jar" "$tmp/tla2tools.jar" || fail selftest
  for copy in C1 L5; do
    if real_control "$copy"; then
      say "selftest control=$copy rejected=yes"
      rejected=$((rejected + 1))
    else
      say "selftest control=$copy rejected=no"
      missed=$((missed + 1))
    fi
  done
  tmp=$keep

  [ "$missed" -eq 0 ] || fail selftest
  if [ "$kind" = record ]; then
    lines=$(wc -l <"$work/lines.txt") || fail selftest
    lines=$((lines + 0))
    write_record "$art/gate-record.txt" "$real_stamp" "$self_sum" "$work/lines.txt" "$lines"
    printf 'formal record lines=%s stamp=%s\n' "$lines" "$real_stamp"
  fi
  printf 'PASS %s:%s (%s failing fixtures rejected)\n' "$kind" "$name" "$rejected"
}

case $# in
  0) measure ;;
  1)
    case $1 in
      --self-test) selftest self-test ;;
      --record) selftest record ;;
      *) fail usage ;;
    esac
    ;;
  *) fail usage ;;
esac
