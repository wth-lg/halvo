#!/bin/sh
set -u
cd "$GITHUB_WORKSPACE" || exit 2
out=target/ver
mkdir -p "$out"
py=$(command -v python3)
printf 'python3=%s\n' "$py"
rustc --version
sh scripts/egress/probe.sh none
printf 'build exit=%s\n' "$?"
sudo -n tcpdump -i any -n -U -w "$out/cap.pcap" 'host 192.0.2.1' >"$out/tcpdump.err" 2>&1 &
sleep 3
run_case() {
  label=$1
  shift
  printf '::group::case %s\n' "$label"
  printf 'start %s\n' "$(date -u +%H:%M:%S)"
  sh scripts/egress/check.sh scripts/egress/lists/empty.txt -- "$@" >"$out/$label.out" 2>"$out/$label.err"
  code=$?
  cat "$out/$label.out"
  head -n 20 "$out/$label.err"
  last=$(tail -n 1 "$out/$label.err")
  log=${last##*log=}
  [ -f "$log" ] && sed 's/^/  log: /' "$log"
  printf 'end %s\n' "$(date -u +%H:%M:%S)"
  printf '::endgroup::\n'
  printf 'case %s exit=%s | %s\n' "$label" "$code" "$last"
}
run_case m7-sudo-launchd sh scratch-ver/m-sudo-launchd.sh "$py"
printf 'm7 job result:\n'
cat "$out/m7-job.txt" 2>/dev/null || printf 'none\n'
sudo -n launchctl remove r2-ver-m7 2>/dev/null
launchctl remove r2-ver-m6 2>/dev/null
printf 'm7 jobs removed at %s\n' "$(date -u +%H:%M:%S)"
run_case m5-cron sh scratch-ver/m-cron.sh "$py"
printf 'waiting 75 s outside the check for cron, from %s\n' "$(date -u +%H:%M:%S)"
sleep 75
printf 'm5 cron result at %s:\n' "$(date -u +%H:%M:%S)"
cat "$out/m5-cron.txt" 2>/dev/null || printf 'none\n'
crontab -r 2>/dev/null
printf 'crontab cleared exit=%s\n' "$?"
sleep 2
sudo -n pkill -INT -x tcpdump
sleep 2
printf '::group::packets to or from 192.0.2.1\n'
sudo -n tcpdump -n -r "$out/cap.pcap" 2>&1 | head -n 40
printf '::endgroup::\n'
cat "$out/tcpdump.err"
