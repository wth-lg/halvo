#!/bin/bash
# Verifier 103, scratch only: my one-rule changes, one after another in this
# checkout: apply, build, run each named test in a process of its own under a
# time limit, and when no named test failed, the in-crate tests of windows::
# once; then restore the file with git checkout. Exits 0 only when every change
# applied and built.
set +e
set -u
out=${RUNNER_TEMP:-.}/v103
mkdir -p "$out"
T=${V103_TIMEOUT:-timeout}
PY=${V103_PYTHON:-python}
command -v "$PY" >/dev/null 2>&1 || PY=python3
command -v "$PY" >/dev/null 2>&1 || PY=py
echo "V103 python=$PY $("$PY" --version 2>&1 | tr -d '\r')"
list=scratch/verifier-103/changes.py
FUNC=keys_queued_before_a_stop_are_delivered_or_counted
n=0
by_named=0
by_other=0
not_caught=0
nobuild=0
timed=0
unapplied=0

outcome_of() {
  # $1 exit code, $2 log: passed, failed, time-limit or other
  if [ "$1" -eq 0 ]; then
    echo passed
  elif [ "$1" -eq 124 ]; then
    echo time-limit
  elif tr -d '\r' <"$2" | grep -a -q '^test result: FAILED'; then
    echo failed
  else
    echo other
  fi
}

show_failure() {
  tr -d '\r' <"$1" | grep -a -E -A5 'panicked at' | grep -a -v '^--$' | head -n 14 | sed 's/^/V103     | /'
  tr -d '\r' <"$1" | grep -a -E '^TF |^GP summary |^GP done |^test .* \.\.\. |^test result' | head -n 24 | sed 's/^/V103     | /'
}

run_functional() {
  # $1 id, $2 test name, $3 log
  "$T" 420 cargo test --locked -p halvo-capture --test windows_functional -- --ignored --exact "$2" --nocapture >"$3" 2>&1
  fcode=$?
  fout=$(outcome_of "$fcode" "$3")
  echo "V103   functional id=$1 test=$2 exit=$fcode outcome=$fout"
  show_failure "$3"
}

run_probe() {
  # $1 id, $2 probe name, $3 log
  HALVO_GATE_TRIALS=5 HALVO_GATE_PROBE=$2 "$T" 600 cargo test --locked -p halvo-capture --lib -- --exact "windows::input::gate_probes::$2" --nocapture --test-threads=1 >"$3" 2>&1
  pcode=$?
  pn=$(tr -d '\r' <"$3" | grep -a '^GP summary ' | sed 's/.* in_neither_count=\([0-9]*\) .*/\1/' | awk '{ s += $1 } END { print s + 0 }')
  pb=$(tr -d '\r' <"$3" | grep -a '^GP summary ' | sed 's/.* in_two_counts=\([0-9]*\) .*/\1/' | awk '{ s += $1 } END { print s + 0 }')
  pd=$(tr -d '\r' <"$3" | grep -a -c '^GP done ')
  echo "V103   probe-under-change id=$1 probe=$2 trials_per_variant=5 exit=$pcode done_lines=$pd events_in_neither_count=$pn events_in_two_counts=$pb"
  tr -d '\r' <"$3" | grep -a -E '^GP summary |^GP done ' | sed 's/^/V103     | /'
}

for id in $("$PY" "$list" ids | tr -d '\r'); do
  n=$((n + 1))
  file=$("$PY" "$list" file "$id" | tr -d '\r')
  began=$(date +%s)
  git checkout --quiet -- "$file"
  if ! "$PY" "$list" apply "$id" . >"$out/apply-$id.log" 2>&1; then
    unapplied=$((unapplied + 1))
    echo "V103 change id=$id result=not-applied"
    git checkout --quiet -- "$file"
    continue
  fi
  blog=$out/build-$id.log
  "$T" 900 cargo test --locked -p halvo-capture --lib --no-run >"$blog" 2>&1
  bcode=$?
  ran=0
  failed=0
  limit=0
  result=
  if [ "$bcode" -eq 124 ]; then
    result=time-limit
    timed=$((timed + 1))
  elif [ "$bcode" -ne 0 ]; then
    result=no-build
    nobuild=$((nobuild + 1))
    tr -d '\r' <"$blog" | grep -a -E -A6 '^error' | head -n 20 | sed 's/^/V103     | /'
  else
    for t in $("$PY" "$list" tests "$id" | tr -d '\r'); do
      ran=$((ran + 1))
      tlog=$out/test-$id-$ran.log
      case $t in
        functional:*)
          run_functional "$id" "${t#functional:}" "$tlog"
          tout=$fout
          ;;
        *)
          "$T" 300 cargo test --locked -p halvo-capture --lib -- --exact "$t" --test-threads=1 >"$tlog" 2>&1
          tcode=$?
          tout=$(outcome_of "$tcode" "$tlog")
          ones=$(tr -d '\r' <"$tlog" | grep -a -c '^test .* \.\.\. ')
          echo "V103   named id=$id test=$t exit=$tcode tests_ran=$ones outcome=$tout"
          [ "$tout" = passed ] || show_failure "$tlog"
          ;;
      esac
      [ "$tout" = failed ] && failed=$((failed + 1))
      [ "$tout" = time-limit ] && limit=$((limit + 1))
    done
    if [ "$failed" -gt 0 ]; then
      result=caught-by-named
      by_named=$((by_named + 1))
    elif [ "$limit" -gt 0 ]; then
      result=time-limit
      timed=$((timed + 1))
    else
      wlog=$out/whole-$id.log
      "$T" 900 cargo test --locked -p halvo-capture --lib -- windows:: >"$wlog" 2>&1
      wcode=$?
      wout=$(outcome_of "$wcode" "$wlog")
      wfailed=$(tr -d '\r' <"$wlog" | grep -a '^test .* \.\.\. FAILED$' | sed 's/^test \(.*\) \.\.\. FAILED$/\1/' | tr '\n' ' ')
      echo "V103   in-crate id=$id filter=windows:: exit=$wcode outcome=$wout failed=[$wfailed]"
      tr -d '\r' <"$wlog" | grep -a '^test result' | sed 's/^/V103     | /'
      if [ "$wout" = failed ]; then
        result=caught-by-another
        by_other=$((by_other + 1))
        show_failure "$wlog"
      elif [ "$wout" = time-limit ]; then
        result=time-limit
        timed=$((timed + 1))
      else
        result=not-caught-in-the-crate
      fi
    fi
    extra=
    case $id in
      s01-* | s03-* | s04-* | s05-*)
        case " $("$PY" "$list" tests "$id" | tr -d '\r' | tr '\n' ' ')" in
          *"functional:$FUNC"*) ;;
          *)
            run_functional "$id" "$FUNC" "$out/extra-functional-$id.log"
            extra=$fout
            ;;
        esac
        run_probe "$id" gate_probe_end_stop "$out/extra-probe-$id.log"
        ;;
      h06-* | h07-* | h08-* | k06-*)
        run_probe "$id" gate_probe_end_shutdown_sink_full "$out/extra-probe-$id.log"
        ;;
      k04-*)
        run_functional "$id" x5_screen_source_stop_and_start_again "$out/extra-functional-$id.log"
        extra=$fout
        ;;
    esac
    if [ "$result" = not-caught-in-the-crate ]; then
      if [ "$extra" = failed ]; then
        result=caught-by-another-functional
        by_other=$((by_other + 1))
      else
        result=not-caught
        not_caught=$((not_caught + 1))
      fi
    fi
  fi
  git checkout --quiet -- "$file"
  left=$(git status --porcelain | wc -l | tr -d ' ')
  echo "V103 change id=$id file=$file build_exit=$bcode named_tests=$ran named_failed=$failed result=$result files_changed_after_restore=$left seconds=$(($(date +%s) - began))"
done
echo "V103 changes summary changes=$n caught_by_named=$by_named caught_by_another=$by_other not_caught=$not_caught no_build=$nobuild time_limit=$timed not_applied=$unapplied"
[ "$n" -gt 0 ] && [ "$unapplied" -eq 0 ] && [ "$nobuild" -eq 0 ]
