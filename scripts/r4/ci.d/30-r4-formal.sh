#!/bin/sh
# jobs: macos windows
# rule: TLC finds no invariant of the coordinator's two models broken in the small configurations that formal/r4-coordinator/README.md names for CI, the committed traces are what the committed models export, and each of two control copies breaks its invariant.
set -u
LC_ALL=C
export LC_ALL
unset CDPATH
name=30-r4-formal
label=$name
workers=3
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
    (cd "$tmp" && curl -fsSL --retry 2 --max-time 300 -o tla2tools.jar "$url") || fail fetch
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
  java -version >"$tmp/java.raw" 2>&1 || fail java
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

configurations() {
  awk -F'|' '
    /^\| `(Schedule|Confirm4)` \| `[a-z]+` \| `[A-Za-z0-9.-]+\.cfg` \| (yes|no) \|/ {
      for (i = 2; i <= 7; i++) gsub(/[ `,]/, "", $i)
      print $2, $3, $4, $7, $5
    }' "$1"
}

run_tlc() {
  deadlock=
  [ "$2" = Confirm4 ] && deadlock=-deadlock
  (
    cd "$1" || exit 99
    mkdir -p jtmp || exit 99
    java -Xmx2g -XX:+UseParallelGC -Djava.io.tmpdir=jtmp -cp ../tla2tools.jar tlc2.TLC \
      -workers "$workers" -metadir meta -config "$3" ${deadlock:+"$deadlock"} "$2" >out.raw 2>&1
  )
  status=$?
  tr -d '\r' <"$1/out.raw" >"$1/out.txt" || status=99
  cp "$1/out.txt" "$art/${1##*/}.txt" || status=99
  sed -n 's/^Error: Invariant \([A-Za-z0-9_]*\) is violated.*$/\1/p' "$1/out.txt" >"$1/violated.txt" || status=99
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
  result=ok
  [ -f "$dir/$2" ] || result=missing
  i=1
  while [ "$result" = ok ] && [ "$i" -le "$count" ]; do
    grep -q -x "INVARIANT $letter$i" "$dir/$2" || result=invariants
    i=$((i + 1))
  done
  if [ "$result" = ok ]; then
    lines=$(grep -c '^INVARIANT ' "$dir/$2")
    [ "$lines" -eq "$count" ] || result=invariants
    if grep -q -F -e '(*' -e '\*' "$dir/$2"; then
      result=comment
    fi
  fi
}

small() {
  model=$1
  config=$2
  file=$3
  want_distinct=$4
  run=$tmp/$key-$config
  mkdir "$run" || fail invariant
  cp "$dir"/*.tla "$run/" || fail invariant
  status=-
  distinct=0
  generated=0
  t0=$(date +%s)
  if [ "$result" = ok ]; then
    cp "$dir/$file" "$run/$file" || fail invariant
    run_tlc "$run" "$model" "$file"
    cat "$run/violated.txt" >>"$tmp/violated-$key.txt" || status=99
    counts=$(sed -n 's/^\([0-9][0-9]*\) states generated, \([0-9][0-9]*\) distinct states found, 0 states left on queue\.$/\1 \2/p' "$run/out.txt")
    case $counts in
      *"
"*) counts= ;;
    esac
    if [ -n "$counts" ]; then
      generated=${counts% *}
      distinct=${counts#* }
    fi
    if [ -s "$run/violated.txt" ]; then
      result=violated
    elif [ "$status" != 0 ]; then
      result=status
    elif ! grep -q -x 'Model checking completed. No error has been found.' "$run/out.txt"; then
      result=incomplete
    elif [ "$distinct" != "$want_distinct" ]; then
      result=counts
    fi
  fi
  t1=$(date +%s)
  printf 'formal run model=%s configuration=%s status=%s states=%s generated=%s seconds=%s result=%s\n' \
    "$key" "$config" "$status" "$distinct" "$generated" "$((t1 - t0))" "$result"
  [ "$result" = ok ] || bad=$((bad + 1))
  if [ "$key" = schedule ]; then
    schedule_states=$((schedule_states + distinct))
  else
    confirm_states=$((confirm_states + distinct))
  fi
}

models() {
  configurations "$tmp/readme.txt" >"$tmp/configurations.txt" || fail invariant
  schedule_names=
  confirm_names=
  while read -r model config file want_distinct ci; do
    [ "$ci" = yes ] || continue
    case $model in
      Schedule) schedule_names=${schedule_names:+$schedule_names,}$config ;;
      *) confirm_names=${confirm_names:+$confirm_names,}$config ;;
    esac
  done <"$tmp/configurations.txt"
  printf 'formal configurations schedule=%s confirm=%s\n' "${schedule_names:--}" "${confirm_names:--}"
  [ -n "$schedule_names" ] || fail invariant
  [ -n "$confirm_names" ] || fail invariant
  schedule_states=0
  confirm_states=0
  bad=0
  : >"$tmp/violated-schedule.txt" || fail invariant
  : >"$tmp/violated-confirm.txt" || fail invariant
  while read -r model config file want_distinct ci; do
    declared "$model" "$file"
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
}

stamp_of() {
  (cd "$1" && cat Schedule.tla Schedule.cfg Confirm4.tla Confirm4.cfg ScheduleTrace.tla Confirm4Trace.tla \
    TraceText.tla limits.json) >"$2.bin" || return 1
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
  named=$(grep -c '^export compared in jobs:' "$tmp/readme.txt")
  [ "$named" -eq 1 ] || fault=${fault:-jobs}
  other=$(sed -n '/^export compared in jobs:/{s///;s/ macos//;s/ windows//;p;}' "$tmp/readme.txt")
  [ -z "$other" ] || fault=${fault:-jobs}
  if [ -n "$fault" ]; then
    printf 'formal traces fault=%s\n' "$fault"
    printf 'formal traces schedule=%s confirm=%s same=-\n' "$schedule_traces" "$confirm_traces"
    fail traces
  fi
}

compare() {
  same=-
  jobs=$(sed -n 's/^export compared in jobs:\(.*\)$/\1/p' "$tmp/readme.txt")
  case " $jobs " in
    *" ${CI_D_JOB:-} "*) compare=yes ;;
    *) compare=no ;;
  esac
  [ -n "${CI_D_JOB:-}" ] || compare=yes
  if [ "$compare" = yes ]; then
    mkdir "$tmp/export" || fail traces
    t0=$(date +%s)
    sh "$dir/export.sh" --jar "$tmp/tla2tools.jar" --out "$tmp/export" >"$tmp/export.txt" 2>&1
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
  mkdir "$run" || return 2
  cp "$dir"/*.tla "$run/" || return 2
  cp "$dir/$2" "$run/$2" || return 2
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
  first=$(sed -n 1p "$run/violated.txt")
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
  stamps
  models
  compare
  controls
  printf 'PASS %s\n' "$name"
}

fixture() {
  mkdir -p "$work/$1/formal" || fail selftest
  cp -R "$repo/formal/r4-coordinator" "$work/$1/formal/" || fail selftest
  fdir=$work/$1/formal/r4-coordinator
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
}

expect() {
  out=$(
    cd "$work" || exit 98
    CI_D_REPO=$work/$1
    CI_D_ARTIFACTS=$work/$1/art
    CI_D_JOB=macos
    R4_STUB_MODE=$3
    R4_STUB_COUNTS=$work/counts.txt
    R4_STUB_TRACES=${traces_from:-$work/$1/formal/r4-coordinator}
    TMPDIR="$work/t m p"
    PATH=$work/stub:$PATH
    export CI_D_REPO CI_D_ARTIFACTS CI_D_JOB R4_STUB_MODE R4_STUB_COUNTS R4_STUB_TRACES TMPDIR PATH
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
  if [ "$code:$last" = "$2" ]; then
    printf 'selftest fixture=%s mode=%s as-expected=yes\n' "$1" "$3"
    rejected=$((rejected + 1))
  else
    printf 'selftest fixture=%s mode=%s as-expected=no\n' "$1" "$3"
    printf '%s\n' "$out" >"$art/selftest-$1-$3.txt"
    missed=$((missed + 1))
  fi
  traces_from=
  jar_from=$tmp/tla2tools.jar
}

selftest() {
  label=self-test:$name
  locate
  [ -d "$dir" ] || fail fetch
  mkdir -p "$art" || fail fetch
  scratch
  [ -n "$tmp" ] || fail fetch
  get_jar
  check_java
  work=$tmp/fixtures
  mkdir -p "$work/stub" "$work/stub-curl" "$work/t m p" || fail selftest
  configurations "$tmp/readme.txt" >"$work/counts.txt" || fail selftest
  cat >"$work/stub/java" <<'EOF' || fail selftest
#!/bin/sh
mode=${R4_STUB_MODE:-good}
if [ "${1:-}" = -version ]; then
  case $mode in
    java-old) printf 'openjdk version "16.0.2" 2021-07-20\r\n' >&2 ;;
    java-unreadable) printf 'a java with no version line\r\n' >&2 ;;
    *) printf 'Picked up a note\r\nopenjdk version "17.0.1" 2021-10-19\r\n' >&2 ;;
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
case $module in
  ScheduleTrace) file=traces-schedule.jsonl ;;
  Confirm4Trace) file=traces-confirm.jsonl ;;
  *) file= ;;
esac
if [ -n "$file" ]; then
  sed -e 1d -e 's/"/\\"/g' -e 's/^/"/' -e 's/$/"/' "$R4_STUB_TRACES/$file"
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
distinct=$(awk -v f="$cfg" '$3 == f { print $4 }' "$R4_STUB_COUNTS")
generated=$((distinct * 3))
case $mode:$cfg in
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
  printf '#!/bin/sh\nexit 22\n' >"$work/stub-curl/curl" || fail selftest
  chmod +x "$work/stub/java" "$work/stub-curl/curl" || fail selftest
  rejected=0
  missed=0
  traces_from=
  jar_from=$tmp/tla2tools.jar

  edit() {
    sed "$3" "$fdir/$2" >"$work/edited" || fail selftest
    cmp -s "$work/edited" "$fdir/$2" && fail selftest
    mv "$work/edited" "$fdir/$2" || fail selftest
  }
  elsewhere='s/^export compared in jobs:.*$/export compared in jobs: windows/'

  fixture good
  expect good "0:PASS $name" good '' ' same=yes'
  fixture not-compared
  edit not-compared README.md "$elsewhere"
  expect not-compared "0:PASS $name" good '' ' same=-'
  fixture days-not-for-ci
  edit days-not-for-ci README.md "$elsewhere"
  edit days-not-for-ci README.md 's/\(| .Schedule-days\.cfg. |\) yes |/\1 no |/'
  expect days-not-for-ci "0:PASS $name" violated
  if [ "$missed" -ne 0 ]; then
    printf 'selftest a fixture that must pass did not pass\n'
    fail selftest
  fi
  rejected=0
  expect good "1:FAIL $name (usage)" good --other
  jar_from=fetch
  expect good "1:FAIL $name (fetch)" fetch
  jar_from=$work/no-such.jar
  expect good "1:FAIL $name (fetch)" no-file
  {
    printf 'X'
    tail -c +2 "$tmp/tla2tools.jar"
  } >"$work/other.jar" || fail selftest
  jar_from=$work/other.jar
  expect good "1:FAIL $name (hash)" other-bytes
  cat "$tmp/tla2tools.jar" "$work/stub-curl/curl" >"$work/longer.jar" || fail selftest
  jar_from=$work/longer.jar
  expect good "1:FAIL $name (hash)" other-size
  for mode in java-old java-unreadable; do
    expect good "1:FAIL $name (java)" "$mode"
  done
  for mode in silent killed violated violated-status status distinct unfinished; do
    expect good "1:FAIL $name (invariant)" "$mode"
  done
  for mode in control-holds control-other control-status control-one; do
    expect not-compared "1:FAIL $name (control)" "$mode"
  done

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

  for file in traces-schedule.jsonl traces-confirm.jsonl; do
    fixture "line-added-$file"
    edit "line-added-$file" README.md "$elsewhere"
    edit "line-added-$file" "$file" 9p
    expect "line-added-$file" "1:FAIL $name (traces)" good
    fixture "line-removed-$file"
    edit "line-removed-$file" README.md "$elsewhere"
    edit "line-removed-$file" "$file" 9d
    expect "line-removed-$file" "1:FAIL $name (traces)" good
    fixture "two-stamps-$file"
    edit "two-stamps-$file" README.md "$elsewhere"
    edit "two-stamps-$file" "$file" '1s/"stamp":"[0-9a-f]\{4\}/"stamp":"0000/'
    expect "two-stamps-$file" "1:FAIL $name (traces)" good
    fixture "changed-line-$file"
    edit "changed-line-$file" "$file" '9s/"r":"ok"/"r":"state"/'
    traces_from=$dir
    expect "changed-line-$file" "1:FAIL $name (traces)" good
  done

  fixture other-models
  edit other-models README.md "$elsewhere"
  printf '\n' >>"$fdir/Confirm4.tla" || fail selftest
  expect other-models "1:FAIL $name (traces)" good

  fixture no-jobs-line
  edit no-jobs-line README.md '/^export compared in jobs:/d'
  expect no-jobs-line "1:FAIL $name (traces)" good

  fixture control-moved
  edit control-moved README.md "$elsewhere"
  edit control-moved Schedule.tla 's/^ModelFree(s, a) == a /ModelFree(s, a) ==  a /'
  restamp control-moved
  expect control-moved "1:FAIL $name (control)" good

  keep=$tmp
  tmp="$keep/real copies"
  mkdir "$tmp" || fail selftest
  cp "$keep/tla2tools.jar" "$tmp/tla2tools.jar" || fail selftest
  for copy in C1 L5; do
    if real_control "$copy"; then
      printf 'selftest control=%s rejected=yes\n' "$copy"
      rejected=$((rejected + 1))
    else
      printf 'selftest control=%s rejected=no\n' "$copy"
      missed=$((missed + 1))
    fi
  done
  tmp=$keep

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
