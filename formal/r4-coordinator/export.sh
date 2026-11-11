#!/bin/sh
set -u

usage() {
    cat <<'EOF'
usage: sh formal/r4-coordinator/export.sh --jar PATH [--out DIR] [--seed N --count N]

Writes traces-schedule.jsonl and traces-confirm.jsonl from the models Schedule and
Confirm4 of this directory: N random walks of each model and, after them, the directed
traces, 13 of Schedule and 15 of Confirm4. PATH is tla2tools.jar of the release that
README.md names. DIR is an existing directory and defaults to this one. Without --seed
and --count the export takes both from the first line of the committed
traces-schedule.jsonl. The first line of both files holds one stamp, the SHA-256 of
the model files that the traces were exported from.

Exit status: 0 when both files are written. 1 when the export ran and a check or a
write missed; it prints which, the two trace files of DIR are as they were, and DIR
holds no file that the export made. 2 on a refusal: one line
"REFUSED <cause>: <detail>" on standard error, with the cause usage, java or jar.
EOF
}

refuse() {
    printf 'REFUSED %s: %s\n' "$1" "$2" >&2
    exit 2
}

miss() {
    printf 'export %s\n' "$1"
    exit 1
}

number() {
    case $1 in
        '' | *[!0-9]*) return 1 ;;
    esac
    test "${#1}" -le 9
}

sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum | cut -d' ' -f1
    else
        shasum -a 256 | cut -d' ' -f1
    fi
}

directed() {
    found=$(grep -c -F "\"case\":\"$2\"" "$1")
    test "$found" -eq 1 || miss "case=$2 lines=$found"
    last=$(grep -F "\"case\":\"$2\"" "$1" | sed 's/.*{"m":/{"m":/')
    case $last in
        "{\"m\":\"$3\","*"},\"r\":\"$4\",\"e\":["*) ;;
        *) miss "case=$2 last=$3 answer=$4 missing" ;;
    esac
    test -n "${5:-}" || return 0
    case $last in
        *"$5"*) ;;
        *) miss "case=$2 last=$3 holds no $5" ;;
    esac
}

here=$(cd "$(dirname "$0")" && pwd)
jar=
out=$here
seed=
count=
while test $# -gt 0; do
    case $1 in
        --help)
            usage
            exit 0
            ;;
        --jar | --out | --seed | --count)
            test $# -ge 2 || refuse usage "$1 needs a value"
            case $1 in
                --jar) jar=$2 ;;
                --out) out=$2 ;;
                --seed) seed=$2 ;;
                --count) count=$2 ;;
            esac
            shift 2
            ;;
        *) refuse usage "unknown argument" ;;
    esac
done

test -n "$jar" || refuse usage "--jar PATH is required"
test -d "$out" || refuse usage "--out names no directory"
if test -n "$seed" || test -n "$count"; then
    number "$seed" || refuse usage "--seed and --count come together, each a number of at most 9 digits"
    number "$count" || refuse usage "--seed and --count come together, each a number of at most 9 digits"
else
    first=
    test -f "$here/traces-schedule.jsonl" && first=$(sed -n '1s/^{"seed":\([0-9]*\),"count":\([0-9]*\),"stamp":"[0-9a-f]*"}$/\1 \2/p' "$here/traces-schedule.jsonl")
    test -n "$first" || refuse usage "no committed first line, give --seed and --count"
    seed=${first% *}
    count=${first#* }
fi
test "$count" -ge 1 || refuse usage "--count is at least 1"

command -v java >/dev/null 2>&1 || refuse java "no java on the path"
major=$(java -version 2>&1 | sed -n 's/.* version "\([0-9][0-9]*\).*/\1/p' | sed -n 1p)
test -n "$major" || refuse java "the version of java is not readable"
test "$major" -ge 17 || refuse java "java $major is older than 17"

test -f "$jar" || refuse jar "no file at the path"
want=$(sed -n 's/^jar url=[^ ]* bytes=[0-9]* sha256=\([0-9a-f]*\)$/\1/p' "$here/README.md")
got=$(sha256 <"$jar")
test -n "$want" && test "$got" = "$want" || refuse jar "the SHA-256 is not the one of README.md"
jar=$(cd "$(dirname "$jar")" && pwd)/$(basename "$jar")

for pair in max_live_runs:MaxLive queue_max:QueueMax starts_per_day:StartsPerDay fails_to_pause:FailsToPause catch_up_days:CatchUpDays; do
    field=${pair%:*}
    name=${pair#*:}
    a=$(sed -n "s/.*\"$field\":\([0-9][0-9]*\).*/\1/p" "$here/limits.json")
    b=$(sed -n "s/^CONSTANT $name = \([0-9][0-9]*\)\$/\1/p" "$here/Schedule.cfg")
    test -n "$a" && test "$a" = "$b" || miss "limits field=$field json=${a:--} cfg=${b:--}"
done

work=$(mktemp -d "${TMPDIR:-/tmp}/r4-export.XXXXXX") || miss "scratch=miss"
cp "$here"/*.tla "$here"/*.cfg "$here/limits.json" "$work/" || miss "scratch=miss"
stamp=$(cd "$work" && cat Schedule.tla Schedule.cfg Confirm4.tla Confirm4.cfg ScheduleTrace.tla Confirm4Trace.tla TraceText.tla limits.json | tr -d '\r' | sha256)
test "${#stamp}" -eq 64 || miss "stamp=miss"

for model in Schedule Confirm4; do
    case $model in
        Schedule)
            file=traces-schedule.jsonl
            cases=13
            ;;
        Confirm4)
            file=traces-confirm.jsonl
            cases=15
            ;;
    esac
    {
        grep -v -e '^INIT ' -e '^NEXT ' -e '^CONSTRAINT ' -e '^VIEW ' "$work/$model.cfg"
        printf 'CONSTANT Seed = %s\nCONSTANT Count = %s\nCONSTANT Depth = 16\n' "$seed" "$count"
        printf 'INIT TraceInit\nNEXT TraceNext\n'
    } >"$work/${model}Trace.cfg"
    (cd "$work" && java -Xmx1g -XX:+UseParallelGC -cp "$jar" tlc2.TLC -workers 1 -deadlock \
        -metadir "meta-$model" -config "${model}Trace.cfg" "${model}Trace" >"$model.out" 2>&1)
    code=$?
    test "$code" -eq 0 || miss "model=$model tlc=$code"
    {
        printf '{"seed":%s,"count":%s,"stamp":"%s"}\n' "$seed" "$count" "$stamp"
        tr -d '\r' <"$work/$model.out" | sed -n 's/^"\({.*}\)"$/\1/p' | sed 's/\\"/"/g'
    } >"$work/$file"
    lines=$(wc -l <"$work/$file" | tr -d ' ')
    test "$lines" -eq $((count + 1 + cases)) || miss "model=$model lines=$lines want=$((count + 1 + cases))"
    named=$(grep -c '^{"n":[0-9]*,"case":"' "$work/$file")
    test "$named" -eq "$cases" || miss "model=$model directed=$named want=$cases"
done

for answer in budget queue_full queued; do
    sed -n "2,$((count + 1))p" "$work/traces-schedule.jsonl" | grep -q "\"r\":\"$answer\"" || miss "answers missing=$answer"
done

s=$work/traces-schedule.jsonl
directed "$s" two_models start queued
directed "$s" two_runs_agent start state
directed "$s" hold_start start hold
directed "$s" unrecorded_start start no_service
directed "$s" second_slot_run tick ok '"started":[]'
directed "$s" auto_needs_person tick ok '"raised":["a1"'
directed "$s" over_budget start budget
directed "$s" queue_full start queue_full
directed "$s" outlives_app restart ok '"cause":"interrupted"'
directed "$s" stop_tree tick ok '"cause":"cancelled"'
directed "$s" deleted_workflow tick ok '"cause":"deleted"'
directed "$s" store_not_ended start state
directed "$s" reap_alive restart ok '"call":"end_run","run":"r1"'
c=$work/traces-confirm.jsonl
directed "$c" answer_twice answer stale
directed "$c" answer_early answer early
directed "$c" answer_page_native answer presenter
directed "$c" timer_allow lapse ok '"answer":"reject_once"'
directed "$c" no_record lapse ok '"cause":"no_record"'
directed "$c" digest_shown issue ok '"digest_ok":"no"'
for reason in unknown_id used_row digest effect reject_row releasing_no_row releasing_unused_row stopped_run; do
    directed "$c" "reason_$reason" guard approval
done
directed "$c" guard_releasing_other guard approval '"state":"releasing"'

old_s=$out/traces-schedule.jsonl
old_c=$out/traces-confirm.jsonl
new_s=$out/.traces-schedule.jsonl.new
new_c=$out/.traces-confirm.jsonl.new
kept=$out/.traces-schedule.jsonl.kept
for path in "$new_s" "$new_c" "$kept"; do
    test ! -e "$path" || miss "write=miss in-the-way=$(basename "$path")"
done
if ! cp "$s" "$new_s"; then
    rm -f "$new_s"
    miss "write=miss copy=schedule"
fi
if ! cp "$c" "$new_c"; then
    rm -f "$new_s" "$new_c"
    miss "write=miss copy=confirm"
fi
before=no
if test -e "$old_s"; then
    before=yes
    if ! cp -p "$old_s" "$kept"; then
        rm -f "$new_s" "$new_c" "$kept"
        miss "write=miss copy=kept"
    fi
fi
if ! mv "$new_s" "$old_s"; then
    rm -f "$new_s" "$new_c" "$kept"
    miss "write=miss rename=schedule"
fi
if ! mv "$new_c" "$old_c"; then
    rm -f "$new_c"
    if test "$before" = yes; then
        mv "$kept" "$old_s" || miss "write=miss rename=confirm restore=miss kept=$(basename "$kept")"
    else
        rm -f "$old_s"
    fi
    miss "write=miss rename=confirm"
fi
rm -f "$kept"
printf 'export seed=%s count=%s stamp=%s schedule=%s confirm=%s\n' "$seed" "$count" "$stamp" \
    "$(($(wc -l <"$old_s") - 1))" "$(($(wc -l <"$old_c") - 1))"
