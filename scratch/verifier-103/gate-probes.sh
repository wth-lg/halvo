#!/bin/bash
# Verifier 103, scratch only: each probe of windows/gate_probes.rs in a process
# of its own, by its exact name. Prints every line of every probe. Exits 0 only
# when each probe printed its done line.
set +e
set -u
out=${RUNNER_TEMP:-.}/v103
mkdir -p "$out"
T=${V103_TIMEOUT:-timeout}
probes="gate_probe_end_stop gate_probe_end_shutdown_sink_full gate_probe_end_drop gate_probe_end_drop_after_timeout"
n=0
ended=0
for p in $probes; do
  n=$((n + 1))
  log=$out/probe-$p.log
  began=$(date +%s)
  HALVO_GATE_PROBE=$p "$T" 1200 cargo test --locked -p halvo-capture --lib -- --exact "windows::input::gate_probes::$p" --nocapture --test-threads=1 >"$log" 2>&1
  code=$?
  tr -d '\r' <"$log" | grep -a -E '^GP |^test |panicked at|^error' | sed 's/^/V103 /'
  d=$(tr -d '\r' <"$log" | grep -a -c '^GP done ')
  s=$(tr -d '\r' <"$log" | grep -a -c '^GP summary ')
  t=$(tr -d '\r' <"$log" | grep -a -c '^GP trial ')
  echo "V103 probe name=$p exit=$code trial_lines=$t summary_lines=$s done_lines=$d seconds=$(($(date +%s) - began))"
  if [ "$code" -eq 0 ] && [ "$d" -eq 1 ]; then
    ended=$((ended + 1))
  else
    tr -d '\r' <"$log" | tail -n 20 | sed 's/^/V103   tail | /'
  fi
done
echo "V103 probes summary probes=$n ended=$ended"
[ "$ended" -eq "$n" ]
