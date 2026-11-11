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
sudo -n tcpdump -i any -n -U -w "$out/cap.pcap" 'host 192.0.2.1 or port 53 or icmp' >"$out/tcpdump.err" 2>&1 &
sleep 3
run_case() {
  label=$1
  shift
  printf '::group::case %s\n' "$label"
  printf 'start %s\n' "$(date -u +%H:%M:%S)"
  sh scripts/egress/check.sh scripts/egress/lists/empty.txt -- "$@" >"$out/$label.out" 2>"$out/$label.err"
  code=$?
  cat "$out/$label.out"
  tail -n 3 "$out/$label.err"
  last=$(tail -n 1 "$out/$label.err")
  log=${last##*log=}
  [ -f "$log" ] && sed 's/^/  log: /' "$log"
  printf 'end %s\n' "$(date -u +%H:%M:%S)"
  printf '::endgroup::\n'
  printf 'case %s exit=%s | %s\n' "$label" "$code" "$last"
}
run_case m0-direct "$py" scratch-ver/connect.py 192.0.2.1 443
run_case m1-launchd sh scratch-ver/m-launchd.sh "$py"
printf 'm1 job result:\n'
cat "$out/m1-job.txt" 2>/dev/null || printf 'none\n'
cat "$out/m1-job.err" 2>/dev/null
launchctl remove r2-ver-m1 2>/dev/null
printf 'm1 job removed at %s\n' "$(date -u +%H:%M:%S)"
run_case m2-dscacheutil-invalid dscacheutil -q host -a name egress-ver-m2.invalid
run_case m2b-dscacheutil-name dscacheutil -q host -a name example.com
run_case m3-icmp ping -c 2 -t 3 192.0.2.1
run_case m4-getaddrinfo "$py" -c "import socket; socket.getaddrinfo('egress-ver-m4.invalid', 443)"
sleep 2
sudo -n pkill -INT -x tcpdump
sleep 2
printf '::group::packets\n'
sudo -n tcpdump -n -r "$out/cap.pcap" 2>&1 | head -n 80
printf '::endgroup::\n'
cat "$out/tcpdump.err"
