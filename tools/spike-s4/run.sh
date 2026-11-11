#!/bin/sh
# Spike S4 loop: start the spike_s4 test binary once per launch and count the
# faults per test and variant. Launches are interleaved (one of each variant per
# round) so that a change of the runner over the job hits every variant alike.
# Prints counts, exit codes and the binary's own `S4 ` lines only.
set -u
launches=${S4_LAUNCHES:-60}
dirty_launches=${S4_DIRTY_LAUNCHES:-0}
exe=$(ls target/debug/deps/spike_s4-*.exe | head -n 1)
[ -n "$exe" ] || { echo "S4 error no test binary"; exit 2; }
echo "S4 env launches_per_variant=$launches dirty_launches=$dirty_launches"
powershell -NoProfile -Command '"S4 env os_version=" + [Environment]::OSVersion.Version.ToString()'
powershell -NoProfile -Command '$p = Get-CimInstance Win32_Processor | Select-Object -First 1; "S4 env cpu=" + ($p.Name -replace " ", "_") + " logical=" + $p.NumberOfLogicalProcessors'

sets=${S4_SETS:-"c8b:baseline c8b:product-source c8b:product-source+mta-ref c8b:product-source+message-window"}
tally=$(mktemp)

run_one() {
  test=$1 variant=$2 round=$3
  out=$(S4_VARIANT="$variant" timeout 300 "$exe" --ignored --nocapture --exact "s4_$test" 2>&1)
  rc=$?
  case "$rc" in
    0) kind=pass ;;
    139 | 3221225477 | -1073741819) kind=fault ;;
    124) kind=timeout ;;
    *) kind=other ;;
  esac
  printf 'S4 launch test=%s variant=%s round=%s exit=%s kind=%s\n' "$test" "$variant" "$round" "$rc" "$kind"
  printf '%s\n' "$out" | grep -a '^S4 ' | sed 's/^/  /'
  printf '%s %s %s\n' "$test" "$variant" "$kind" >>"$tally"
}

round=0
while [ "$round" -lt "$launches" ]; do
  round=$((round + 1))
  for s in $sets; do
    run_one "${s%%:*}" "${s#*:}" "$round"
  done
  # A probe bug shows in round 1 as a timeout or another exit code in every
  # set; stop then instead of holding the runner for the whole loop.
  if [ "$round" -eq 1 ] && [ "$(grep -c ' pass$\| fault$' "$tally")" -eq 0 ]; then
    echo "S4 error round 1 had no pass and no fault; the probe is broken"
    exit 3
  fi
done

for s in $sets; do
  test=${s%%:*} variant=${s#*:}
  for k in pass fault timeout other; do
    eval "n_$k=\$(grep -c \"^$test $variant $k\$\" \"\$tally\")"
  done
  printf 'S4 summary test=%s variant=%s launches=%s pass=%s faults=%s timeouts=%s other=%s\n' \
    "$test" "$variant" "$launches" "$n_pass" "$n_fault" "$n_timeout" "$n_other"
done

i=0
while [ "$i" -lt "$dirty_launches" ]; do
  i=$((i + 1))
  out=$(timeout 300 "$exe" --ignored --nocapture --exact s4_dirty 2>&1)
  rc=$?
  printf 'S4 launch test=dirty n=%s exit=%s\n' "$i" "$rc"
  printf '%s\n' "$out" | grep -a '^S4 ' | sed 's/^/  /'
done
echo "S4 done"
