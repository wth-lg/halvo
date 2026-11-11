#!/bin/sh
# sh verifier-probes/hosted.sh JOB: the verifier's probes of the egress check on a hosted runner.
# Every case is one run of the check in the default mode of the platform. Always exits 0: the
# lines are the result.
set -u
job=$1
cd "$GITHUB_WORKSPACE" || exit 0
empty=scripts/egress/lists/empty.txt
work=target/verifier-probes
mkdir -p "$work"
sink=$work/sink.txt
: >"$sink"
case $job in
  macos) py=python3 ;;
  *) py=python ;;
esac
port=47411

show_sink() {
  printf 'sink after %s:\n' "$1"
  sed 's/^/  /' "$sink"
  : >"$sink"
  printf 'sink port=%s cleared\n' "$port" >>"$sink"
}

# run_check LABEL COMMAND...: one run of the check with the empty list; prints the command's
# output, the check's last line, its exit status, the denial log and the counts file.
run_check() {
  label=$1
  shift
  printf '\n== case %s\n' "$label"
  sh scripts/egress/check.sh "$empty" -- "$@" >"$work/$label.out" 2>"$work/$label.err"
  code=$?
  sed 's/^/  out: /' "$work/$label.out" | cut -c1-300 | head -n 24
  grep -v '^egress mode=' "$work/$label.err" | sed 's/^/  err: /' | cut -c1-300 | tail -n 12
  last=$(grep '^egress mode=' "$work/$label.err" | tail -n 1)
  printf 'case %s exit=%s last=%s\n' "$label" "$code" "${last:-none}"
  log=$(printf '%s\n' "$last" | sed -n 's/.* log=\(.*\)$/\1/p')
  if [ -n "$log" ] && [ -f "$log" ]; then
    sed 's/ at=[0-9-]*//' "$log" | sort | uniq -c | sed 's/^/  log: /' | head -n 16
    [ -f "${log%/*}/counts" ] && sed 's/^/  counts: /' "${log%/*}/counts"
    [ -f "${log%/*}/rules" ] && sed 's/^/  rules-added: /' "${log%/*}/rules"
  fi
}

rustc --version
"$py" --version
"$py" verifier-probes/sink.py "$port" "$sink" &
sink_pid=$!
sleep 2
cat "$sink"

if [ "$job" = windows ]; then
  state() {
    powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File verifier-probes/state.ps1 -Label "$1"
  }
  state before-the-first-run
  run_check w0-quiet "$py" verifier-probes/client.py sleep 0
  state after-the-first-run
  run_check w1-loopback-by-the-wrapped-binary "$py" verifier-probes/client.py tcp 127.0.0.1 "$port" w1
  show_sink w1
  run_check w2-loopback-by-a-later-binary sh -c "$py verifier-probes/client.py tcp 127.0.0.1 $port w2"
  show_sink w2
  "$py" verifier-probes/sink.py 47202 "$sink" &
  sink2_pid=$!
  sleep 2
  run_check w3-loopback-by-the-probe sh scripts/egress/probe.sh loopback2
  show_sink w3
  kill "$sink2_pid" 2>/dev/null
  run_check w4-lookup-by-the-wrapped-binary "$py" verifier-probes/client.py lookup example.com
  run_check w5-lookup-again "$py" verifier-probes/client.py lookup example.com
  run_check w6-lookup-by-nslookup sh -c 'nslookup example.com 2>&1 | tail -n 4'
  run_check w7-connect-by-a-later-binary sh -c 'curl.exe -m 12 -s -o /dev/null -w "http=%{http_code} remote=%{remote_ip}:%{remote_port}\n" http://example.com/; echo "curl exit=$?"'
  run_check w8-connect-by-the-wrapped-binary curl.exe -m 12 -s -o /dev/null -w 'http=%{http_code} remote=%{remote_ip}:%{remote_port}\n' http://example.com/
  winpy=$(cygpath -w "$(command -v "$py")")
  winclient=$(cygpath -w "$PWD/verifier-probes/client.py")
  run_check w9-handed-to-wmi powershell.exe -NoLogo -NoProfile -NonInteractive -Command \
    "\$r = Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments @{ CommandLine = '$winpy $winclient tcp 127.0.0.1 $port w9' }; 'wmi create return=' + \$r.ReturnValue + ' pid=' + \$r.ProcessId; Start-Sleep -Seconds 5"
  show_sink w9
  run_check w10-command-fails sh -c 'exit 3'
  state after-a-failing-command
  printf '\n== case w11-check-ended-by-a-signal\n'
  sh scripts/egress/check.sh "$empty" -- "$py" verifier-probes/client.py sleep 60 >"$work/w11.out" 2>"$work/w11.err" &
  check_pid=$!
  sleep 25
  state during-the-run-that-is-ended
  kill -TERM "$check_pid"
  sleep 3
  if kill -0 "$check_pid" 2>/dev/null; then
    printf 'case w11 the check is still alive after TERM\n'
  else
    printf 'case w11 the check is gone after TERM\n'
  fi
  sed 's/^/  err: /' "$work/w11.err" | tail -n 4
  state after-the-check-was-ended
  run_check w12-the-next-run "$py" verifier-probes/client.py sleep 0
  state after-the-next-run
else
  run_check m0-quiet "$py" verifier-probes/client.py sleep 0
  run_check m1-in-tree-connect sh -c "printf 'm1\n' | /usr/bin/nc -w 2 127.0.0.1 $port; echo nc exit=\$?"
  show_sink m1
  run_check m2-nscurl-lookup-alone /usr/bin/nscurl -m 6 https://r2-lookup-check.invalid/
  run_check m3-nscurl-resolvable /usr/bin/nscurl -m 8 http://example.com/
  printf '\n== resolver daemon, lines for nscurl\n'
  /usr/bin/log show --last 3m --info --style compact --predicate 'process == "mDNSResponder"' >"$work/resolver.txt" 2>&1
  grep -c . "$work/resolver.txt"
  grep 'client pid: [0-9]* (nscurl)' "$work/resolver.txt" | cut -c1-280 | head -n 8
  for request in $(grep 'getaddrinfo start.*(nscurl)' "$work/resolver.txt" | grep -o '\[R[0-9]*\]' | tr -d '[]' | sort -u); do
    for question in $(grep -F "[$request->" "$work/resolver.txt" | grep -o 'Q[0-9]*' | sort -u); do
      grep -F "[$question]" "$work/resolver.txt" | grep 'Sent\|Received' | sed -E 's/([0-9]{1,3}\.){3}[0-9]{1,3}/SERVER/g' | cut -c1-260 | head -n 4
    done
    grep -F "[$request->" "$work/resolver.txt" | grep 'getaddrinfo result' | cut -c1-260 | head -n 6
  done
  run_check m4-handed-to-launchd sh -c "/bin/launchctl submit -l r2-verifier-handoff -- /bin/sh -c \"printf 'm4-launchd\n' | /usr/bin/nc -w 2 127.0.0.1 $port; sleep 30\"; echo submit exit=\$?; sleep 5; /bin/launchctl remove r2-verifier-handoff; echo remove exit=\$?"
  show_sink m4
  run_check m5-handed-to-open sh -c "/usr/bin/open -g 'http://127.0.0.1:$port/m5-open'; echo open exit=\$?; sleep 25"
  show_sink m5
  run_check m6-handed-by-apple-event "$py" verifier-probes/client.py timed 25 /usr/bin/osascript -e "tell application \"Terminal\" to do script \"printf 'm6-apple-event\\\\n' | /usr/bin/nc -w 2 127.0.0.1 $port\""
  sleep 6
  show_sink m6
  /bin/launchctl list 2>/dev/null | grep -c r2-verifier-handoff
fi
kill "$sink_pid" 2>/dev/null
printf '\nverifier probes done\n'
exit 0
