#!/bin/sh
set -u
ROOT=${HALVO_ROOT:-$(cd "$(dirname "$0")/../.." && pwd -P)}
HOSTS="$ROOT/scripts/outbound-hosts.sh"
HERE="$ROOT/gate/fixtures/outbound"
ATTEMPT="$HERE/attempt.sh"
CHECKS=${GATE_CHECKS_DIR:-$ROOT/gate/checks}

denied_of() { printf '%s\n' "$1" | sed -n 's/^outbound: denied=\([0-9]*\)$/\1/p' | tail -1; }
why_of() { printf '%s\n' "$1" | sed -n 's/^outbound: cannot count: //p' | tail -1; }
probes_of() { printf '%s\n' "$1" | sed -n 's/^outbound: browser: net log: .*, \([0-9]*\) left out as probes of IPv6$/\1/p' | tail -1; }
hints_of() { printf '%s\n' "$1" | sed -n 's/^outbound: hint: //p' | paste -s -d '|' - | sed 's/|/; /g'; }
inside_of() { printf '%s\n' "$1" | sed -n 's/^outbound: inside: //p'; }

outside_line_of() {
  for log_of_the_gate_run_that_started_this_check in "${GATE_STATE:-$ROOT/.tmp/no-gate-run}"/[0-9][0-9]-"$1".log; do
    if [ -f "$log_of_the_gate_run_that_started_this_check" ]; then
      grep -aE "^(PASS|FAIL) $1: " "$log_of_the_gate_run_that_started_this_check" | tail -1
      return
    fi
  done
  for check_that_the_gate_runs_after_this_one in "$CHECKS"/[0-9][0-9]-"$1".sh; do
    [ ! -f "$check_that_the_gate_runs_after_this_one" ] ||
      sh "$check_that_the_gate_runs_after_this_one" 2>&1 | grep -aE "^(PASS|FAIL) $1: " | tail -1
  done
}

why_the_inside_differs() {
  printf '%s\n' "$1" | grep '^FAIL ' | while IFS= read -r inside_line; do
    name=${inside_line#FAIL }
    name=${name%%:*}
    case "$(outside_line_of "$name")" in
      "FAIL $name: "*) ;;
      "PASS $name: "*)
        echo "$name reads FAIL inside the sandbox and PASS outside it"
        break
        ;;
      *)
        echo "$name reads FAIL inside the sandbox and gave no line outside it"
        break
        ;;
    esac
  done
}

line_of() {
  run_exit=$1
  run_output=$2
  phrase_after_count=$3
  case "$run_exit" in
    0)
      probes=$(probes_of "$run_output")
      echo "PASS outbound: denied=0 $phrase_after_count${probes:+; probes left out=$probes}"
      ;;
    2) echo "FAIL outbound: cannot count: $(why_of "$run_output")" ;;
    *)
      hints=$(hints_of "$run_output")
      echo "FAIL outbound: denied=$(denied_of "$run_output") $phrase_after_count; the number is a sum over the sources, not a count of attempts${hints:+; $hints}"
      ;;
  esac
}

RECORD_A_GATE_RUN_RELIES_ON='^outbound: system log: [1-9][0-9]* denied$'
BROWSER_RECORD_A_GATE_RUN_RELIES_ON='^outbound: browser: system log: [1-9][0-9]* denied$'
NO_BROWSER_RECORD_SO_ONLY_THE_NET_LOG_SHOWS_IT='^outbound: browser: system log: 0 denied$'
BUILTIN_WITH_NO_FILE='eval'
NO_JOB_CONTROL_SO_THE_BACKGROUND_RUN_IGNORES_INT_AND_GETS=TERM
SECONDS_IN_LSOF_SO_THE_SAMPLER_OF_THE_RUN_OUTLASTS_THE_INTERRUPT=3

listing=
only=
runs_fixture() {
  if [ -n "$listing" ]; then
    printf '%s\n' "$1"
    return 1
  fi
  [ -n "$only" ] || return 0
  case " $only " in
    *" $1 "*) return 0 ;;
  esac
  return 1
}

fixture() {
  kind=$1
  name=$2
  want=$3
  patterns=$4
  shift 4
  runs_fixture "$name" || return 0
  output=$(sh "$HOSTS" "$@")
  got=$?
  line=$(line_of "$got" "$output" "in this run")
  printf '%s\n' "$output"
  echo "self-test outbound: the line of $name: $line"
  if [ "$got" != "$want" ]; then
    echo "self-test outbound: $kind $name: exit $got, expected $want"
    return 1
  fi
  missing=$(printf '%s\n' "$patterns" | while IFS= read -r pattern; do
    [ -z "$pattern" ] || printf '%s\n%s\n' "$output" "$line" | grep -qE -- "$pattern" || printf ' %s' "$pattern"
  done)
  if [ -n "$missing" ]; then
    echo "self-test outbound: $kind $name: the run shows no line that matches:$missing"
    return 1
  fi
  case "$kind" in
    good) echo "self-test outbound: good $name: PASS as expected" ;;
    bad) echo "self-test outbound: bad $name: FAIL as expected" ;;
  esac
}

fixture_with() {
  variable=$1
  value=$2
  shift 2
  (
    export "$variable=$value"
    fixture "$@"
  )
}

netlog_fixture() {
  kind=$1
  name=$2
  want=$3
  runs_fixture "$name" || return 0
  counted=$(node "$ROOT/gate/lib/netlog.mjs" "$HERE/$name.json" | tr '\n' ' ')
  case "$counted" in
    *"$want"*)
      case "$kind" in
        good) echo "self-test outbound: good $name: PASS as expected" ;;
        bad) echo "self-test outbound: bad $name: FAIL as expected" ;;
      esac
      ;;
    *)
      echo "self-test outbound: $kind $name: the reader printed: $counted(expected: $want)"
      return 1
      ;;
  esac
}

interrupt_fixture() {
  mode=$1
  runs_fixture interrupt || return 0
  mkdir -p "$ROOT/.tmp"
  marks=$(mktemp -d "$ROOT/.tmp/outbound-interrupt.XXXXXX") || return 1
  mkdir "$marks/slow-lsof"
  printf '#!/bin/sh\nsleep %s\n' "$SECONDS_IN_LSOF_SO_THE_SAMPLER_OF_THE_RUN_OUTLASTS_THE_INTERRUPT" > "$marks/slow-lsof/lsof"
  chmod +x "$marks/slow-lsof/lsof"
  PATH="$marks/slow-lsof:$PATH" sh "$HOSTS" "$mode" sh "$HERE/tree.sh" "$marks" > "$marks/run.log" 2>&1 &
  run=$!
  tries=0
  while [ ! -s "$marks/child.pid" ] && [ "$tries" -lt 300 ] && kill -0 "$run" 2> /dev/null; do
    sleep 0.1
    tries=$((tries + 1))
  done
  if [ ! -s "$marks/child.pid" ]; then
    wait "$run"
    sed 's/^/  /' "$marks/run.log"
    echo "self-test outbound: interrupt: the command of the fixture did not start"
    return 1
  fi
  started="root $(cat "$marks/root.pid"), child $(cat "$marks/child.pid")"
  if [ "$mode" = browser ]; then
    browser=$(sed -n 's/^outbound: browser: pid \([0-9]*\) .*$/\1/p' "$marks/run.log")
    started="$started, browser ${browser:-of which the run printed no pid}"
  fi
  kill -s "$NO_JOB_CONTROL_SO_THE_BACKGROUND_RUN_IGNORES_INT_AND_GETS" "$run"
  wait "$run"
  status=$?
  own_processes_at_return=$(ps -axo pid=,command= | awk -v script="$HOSTS" -v marks="$marks" 'index($0, script) && index($0, marks) && $2 != "awk" { printf " %s", $1 }')
  if [ -n "$own_processes_at_return" ]; then
    echo "self-test outbound: interrupt: the run returned while processes of its own script were still alive:$own_processes_at_return"
    return 1
  fi
  tries=0
  while :; do
    left=
    for pid in $(printf '%s\n' "$started" | tr -c '0-9\n' ' '); do
      ! kill -0 "$pid" 2> /dev/null || left="$left $pid"
    done
    [ -n "$left" ] && [ "$tries" -lt 50 ] || break
    sleep 0.1
    tries=$((tries + 1))
  done
  if [ -n "$left" ]; then
    for pid in $left; do kill -s KILL "$pid" 2> /dev/null; done
    echo "self-test outbound: interrupt: the run started $started, and after the interrupt these were still alive:$left"
    return 1
  fi
  if [ "$status" != 130 ] || { [ "$mode" = browser ] && [ -z "$browser" ]; }; then
    sed 's/^/  /' "$marks/run.log"
    echo "self-test outbound: interrupt: exit $status (expected 130) after the run started $started"
    return 1
  fi
  echo "self-test outbound: interrupt: the run stopped what it had started ($started)"
}

main_path_fixture() {
  kind=$1
  name=$2
  want=$3
  pattern=$4
  runs_fixture "$name" || return 0
  output=$(HALVO_ROOT="$HERE/main" GATE_CHECKS_DIR="$HERE/main/checks" GATE_STATE="$HERE/main/$9" STUB_GATE_EXIT="$5" STUB_GATE_OUTPUT="$6" STUB_CONTROL_EXIT="$7" STUB_CONTROL_OUTPUT="$8" sh "$0")
  got=$?
  line=$(printf '%s\n' "$output" | tail -1)
  if [ "$got" != "$want" ] || ! printf '%s\n' "$line" | grep -qE -- "$pattern"; then
    echo "self-test outbound: $kind $name: exit $got (expected $want) with the last line: $line"
    return 1
  fi
  if [ "$(printf '%s\n' "$output" | sed '$d')" != "$(cat "$HERE/main/$6" "$HERE/main/$8")" ]; then
    echo "self-test outbound: $kind $name: the check did not print the lines of the gate run, then those of the control run, then one result line"
    return 1
  fi
  case "$kind" in
    good) echo "self-test outbound: good $name: PASS as expected" ;;
    bad) echo "self-test outbound: bad $name: FAIL as expected" ;;
  esac
}

fixtures() {
  fixture good loopback-only 0 '' run /usr/bin/curl -s -o /dev/null --max-time 2 http://127.0.0.1:9/ || failed=1
  fixture good loopback-fetch-by-workerd 0 '^outbound: command: worker fetch:' run sh "$ATTEMPT" workerd loopback || failed=1
  fixture bad apple-binary-to-test-net 1 "$RECORD_A_GATE_RUN_RELIES_ON" run /usr/bin/curl -s -o /dev/null --max-time 2 http://192.0.2.1:9/ || failed=1
  for kind_and_its_node_records in node:1 workerd:0 stripe-mock:0 native:0; do
    kind=${kind_and_its_node_records%:*}
    node_records=${kind_and_its_node_records#*:}
    fixture bad "one-attempt-by-$kind" 1 "^outbound: system log: 1 denied\$
^outbound: node record: $node_records denied\$
^outbound: killed with no record: 0\$
^outbound: the next number is the sum of the counts above, so an attempt that two of them hold counts twice\$
^outbound: denied=$((1 + node_records))\$
^FAIL outbound: denied=$((1 + node_records)) in this run; the number is a sum over the sources, not a count of attempts\$" run sh "$ATTEMPT" "$kind" || failed=1
  done
  fixture bad one-attempt-by-browser 1 "$BROWSER_RECORD_A_GATE_RUN_RELIES_ON
^outbound: command: browser page: tcp:[ ]
^outbound:   chrome-headless-shell\\([0-9]+\\) deny\\(1\\) network-outbound .*:80\$" browser sh "$ATTEMPT" browser || failed=1
  fixture bad browser-tcp-443-after-the-probe 1 "$BROWSER_RECORD_A_GATE_RUN_RELIES_ON
^outbound: command: browser page: tcp443:[ ]
^outbound:   chrome-headless-shell\\([0-9]+\\) deny\\(1\\) network-outbound .*:443\$" browser node "$HERE/attempt-browser.mjs" tcp443 || failed=1
  fixture bad node-http-to-test-net 1 "$RECORD_A_GATE_RUN_RELIES_ON
^outbound:   pid [0-9]+ connect 192\\.0\\.2\\.1\$" run node -e 'require("node:http").get("http://192.0.2.1:9/").on("error", () => {})' || failed=1
  fixture bad node-name-lookup 1 "$RECORD_A_GATE_RUN_RELIES_ON" run node -e 'fetch("https://halvo-outbound.invalid/").catch(() => {})' || failed=1
  fixture bad node-resolve-of-an-address 1 '^outbound:   pid [0-9]+ resolve 192\.0\.2\.1$' run node -e 'const resolver = new (require("node:dns").Resolver)({ timeout: 300, tries: 1 }); resolver.setServers(["127.0.0.1:9"]); resolver.resolve4("192.0.2.1", () => {})' || failed=1
  fixture bad control-geolocation-fetch 1 "$RECORD_A_GATE_RUN_RELIES_ON" control || failed=1
  fixture bad node-lookup-of-localhost 1 "$RECORD_A_GATE_RUN_RELIES_ON
^outbound:   pid [0-9]+ lookup localhost\$
^FAIL outbound: denied=[0-9]+ in this run; .*use 127\\.0\\.0\\.1" run node -e 'require("node:dns").lookup("localhost", () => {})' || failed=1
  # shellcheck disable=SC2016
  fixture good hard-link-and-rename-of-a-linked-file 0 '^outbound: command: linked and renamed$' run sh -c 'echo x > "$OUTBOUND_RUN/first-name" && ln "$OUTBOUND_RUN/first-name" "$OUTBOUND_RUN/second-name" && mv "$OUTBOUND_RUN/second-name" "$OUTBOUND_RUN/third-name" && echo "linked and renamed"' || failed=1
  fixture good a-process-left-behind-is-stopped 0 '^outbound: left behind by the command and stopped: .*sleep\([0-9]+\).*sleep\([0-9]+\).*sleep\([0-9]+\)' run sh -c '(set -m; sleep 41 & wait) > /dev/null 2>&1 & (trap "" TERM; sleep 43) > /dev/null 2>&1 & (sleep 4; exec /usr/bin/curl -s -o /dev/null --max-time 2 http://192.0.2.1:9/) > /dev/null 2>&1 &' || failed=1
  # shellcheck disable=SC2016
  fixture bad command-ended-by-kill-with-no-record 1 '^outbound: command exit 137$
^outbound: killed with no record: 1$
^outbound: denied=1$' run sh -c 'kill -s KILL $$' || failed=1
  fixture good listener-on-loopback 0 '^outbound: command: listening on 127\.0\.0\.1$' run node -e 'require("node:net").createServer().listen(0, "127.0.0.1", function () { console.log("listening on", this.address().address); this.close(); })' || failed=1
  fixture bad lookup-through-the-dns-mach-service 1 "$RECORD_A_GATE_RUN_RELIES_ON
^outbound: command: mach lookup of the DNS service \\(outbound-native\\.[A-Za-z0-9]+\\): refused\$
^outbound:   attempt\\([0-9]+\\) deny\\(1\\) mach-lookup com\\.apple\\.dnssd\\.service\$" run sh "$ATTEMPT" mach-lookup || failed=1
  fixture good command-exit-64 0 '^outbound: command exit 64$' run sh -c 'exit 64' || failed=1
  fixture bad command-not-found 2 '^FAIL outbound: cannot count: .* could not start ' run "$ROOT/.tmp/no-such-command" || failed=1
  fixture bad command-cannot-start 2 '^FAIL outbound: cannot count: .* could not start ' run "$BUILTIN_WITH_NO_FILE" : || failed=1
  fixture good browser-page-on-loopback 0 '^outbound: command: browser page: loaded$
^PASS outbound: denied=0 in this run; probes left out=[1-9]' browser sh "$ATTEMPT" browser loopback || failed=1
  fixture_with GATE_CHECKS_DIR "$HERE/checks" good a-check-drives-the-browser 0 '^outbound: inside: PASS browser-page:[ ]
^PASS outbound: denied=0 in this run; probes left out=[1-9]' gate || failed=1
  fixture_with GATE_CHECKS_DIR "$HERE/metering" good check-55-runs-its-bounded-plan-inside 0 '^outbound: inside: (PASS|FAIL) metering: plan=bounded 
^PASS outbound: denied=0 in this run' gate || failed=1
  fixture_with GATE_CHECKS_DIR "$HERE/metering-without-the-plan" bad check-55-without-its-bounded-plan-inside 2 '^outbound: inside: (PASS|FAIL) metering: plan=whole 
^FAIL outbound: cannot count: check 55 ran inside the sandbox with a plan other than bounded: (PASS|FAIL) metering: plan=whole ' gate || failed=1
  fixture_with GATE_CHECKS_DIR "$HERE/checks-with-a-broken-contract" bad a-check-breaks-its-contract-inside 2 '^FAIL outbound: cannot count: inside the sandbox: FAIL no-result-line: the check broke its contract ' gate || failed=1
  fixture_with GATE_CHECKS_DIR "$HERE/main" bad no-check-runs-inside 2 '^FAIL outbound: cannot count: the gate inside the sandbox ended with exit 2, ' gate || failed=1
  fixture bad browser-udp-443-elsewhere 1 "$NO_BROWSER_RECORD_SO_ONLY_THE_NET_LOG_SHOWS_IT
^outbound:   [0-9]+ x UDP_CONNECT 192\\.0\\.2\\.1:443\$" browser node "$HERE/attempt-browser.mjs" udp443 || failed=1
  fixture good browser-udp-443-on-loopback 0 '^outbound: command: browser page: udp443-loopback:[ ]
^outbound: browser: net log: 0 attempts outside loopback ' browser node "$HERE/attempt-browser.mjs" udp443-loopback || failed=1
  # shellcheck disable=SC2016
  fixture bad browser-net-log-missing 2 '^FAIL outbound: cannot count: the browser left no net log' browser sh -c 'mv "$OUTBOUND_RUN/browser/netlog.json" "$OUTBOUND_RUN/browser/netlog.moved"' || failed=1
  fixture bad browser-started-inside-the-sandbox 1 "$RECORD_A_GATE_RUN_RELIES_ON
^FAIL outbound: denied=[0-9]+ in this run; .*\\(chrome-headless-shell\\).*browser mode" run node "$HERE/attempt-browser.mjs" own || failed=1
  fixture good helper-refuses-its-own-browser 0 '^outbound: command: browser: no browser for this run: HALVO_BROWSER_ENDPOINT is not set' run node "$HERE/attempt-browser.mjs" loopback || failed=1
  fixture_with HALVO_BROWSER_ENDPOINT ws://127.0.0.1:9/devtools/browser/of-the-caller good helper-gets-no-endpoint-from-the-caller 0 '^outbound: command: browser: no browser for this run: HALVO_BROWSER_ENDPOINT is not set' run node "$HERE/attempt-browser.mjs" loopback || failed=1
  fixture good helper-refuses-a-context-with-its-own-proxy 0 '^outbound: command: browser: test-kit/browser refuses a browser context that sets a proxy of its own' browser node "$HERE/attempt-browser.mjs" proxy || failed=1
  # shellcheck disable=SC2016
  fixture bad browser-network-service-crashed 2 "^FAIL outbound: cannot count: the browser's own log reports a crash of its network service" browser sh -c 'printf "%s\n" "[1:2:ERROR:network_service_instance_impl.cc(600)] Network service crashed, restarting service." >> "$OUTBOUND_RUN/browser/browser.log"' || failed=1
  netlog_fixture good netlog-probe 'probes 1 attempts 0 ' || failed=1
  netlog_fixture bad netlog-session-to-the-probe-address 'probes 0 attempts 1 ' || failed=1
  netlog_fixture bad netlog-proxy-settings-of-the-system 'cannot count: the browser took its proxy settings from macOS' || failed=1
  netlog_fixture bad netlog-two-sockets-for-one-check 'probes 1 attempts 1 ' || failed=1
  netlog_fixture bad netlog-datagram-on-a-probe-shaped-socket 'probes 1 attempts 1 ' || failed=1
  netlog_fixture bad netlog-not-finished "cannot count: the browser's net log is not complete" || failed=1
  netlog_fixture bad netlog-refused-datagram-on-a-loopback-socket 'probes 0 attempts 1 attempt 1 x UDP_SEND_ERROR that macOS refused on a socket that named a loopback address' || failed=1
  netlog_fixture bad netlog-no-events "cannot count: the browser's net log holds no event" || failed=1
  netlog_fixture bad netlog-datagram-on-a-socket-without-an-address 'probes 0 attempts 1 attempt 1 x a datagram on a socket whose address the log does not give' || failed=1
  netlog_fixture bad netlog-refused-datagram-on-a-probe-shaped-socket 'probes 1 attempts 1 ' || failed=1
  netlog_fixture bad netlog-outside-addresses-under-other-keys 'probes 0 attempts 3 ' || failed=1
  netlog_fixture bad netlog-lookup-of-a-name 'probes 0 attempts 1 attempt 1 x lookup example.invalid ' || failed=1
  netlog_fixture bad netlog-socket-of-a-check-to-another-address 'probes 0 attempts 1 ' || failed=1
  netlog_fixture good netlog-loopback-addresses 'probes 0 attempts 0 ' || failed=1
  netlog_fixture good netlog-connection-refused-on-a-loopback-socket 'probes 0 attempts 0 ' || failed=1
  netlog_fixture bad netlog-loopback-and-outside-address-on-one-source 'probes 0 attempts 1 attempt 1 x SOCKET_CONNECT 203.0.113.5:443 ' || failed=1
  interrupt_fixture browser || failed=1
  main_path_fixture good main-path-clean-and-caught 0 '^PASS outbound: denied=0 over the stack and the other checks, which read 1 PASS and 2 FAIL inside the sandbox; probes left out=0; control=caught$' \
    0 gate-clean.txt 1 control-caught.txt outside-as-inside || failed=1
  main_path_fixture bad main-path-attempt-in-the-gate-run 1 '^FAIL outbound: denied=1 over the stack and the other checks, which read 1 PASS and 2 FAIL inside the sandbox; the number is a sum over the sources, not a count of attempts$' \
    1 gate-denied.txt 1 control-caught.txt outside-as-inside || failed=1
  main_path_fixture bad main-path-gate-run-not-counted 1 '^FAIL outbound: cannot count: the system log dropped messages during the run$' \
    2 gate-cannot-count.txt 1 control-caught.txt outside-as-inside || failed=1
  main_path_fixture bad main-path-control-not-counted 1 '^FAIL outbound: the control run could not count: the system log dropped messages during the run$' \
    0 gate-clean.txt 2 gate-cannot-count.txt outside-as-inside || failed=1
  main_path_fixture bad main-path-control-not-caught 1 '^FAIL outbound: the control run counted no denied attempt, so denied=0 proves nothing$' \
    0 gate-clean.txt 0 gate-clean.txt outside-as-inside || failed=1
  main_path_fixture bad main-path-control-without-a-record 1 '^FAIL outbound: the system log holds no record of the control run.s attempt, so denied=0 proves nothing$' \
    0 gate-clean.txt 1 control-without-a-record.txt outside-as-inside || failed=1
  main_path_fixture bad main-path-a-check-fails-only-inside 1 '^FAIL outbound: cannot count: setuid-step reads FAIL inside the sandbox and PASS outside it$' \
    0 gate-a-check-fails-inside.txt 1 control-caught.txt outside-the-check-passes || failed=1
  main_path_fixture bad main-path-a-check-after-this-one-fails-only-inside 1 '^FAIL outbound: cannot count: late-step reads FAIL inside the sandbox and PASS outside it$' \
    0 gate-a-late-check-fails-inside.txt 1 control-caught.txt outside-as-inside || failed=1
  main_path_fixture bad main-path-no-line-from-outside 1 '^FAIL outbound: cannot count: setuid-step reads FAIL inside the sandbox and gave no line outside it$' \
    0 gate-a-check-fails-inside.txt 1 control-caught.txt no-gate-run || failed=1
}

if [ "${1:-}" = --self-test ]; then
  shift
  only=$*
  known=$(listing=1 && fixtures)
  for name in $only; do
    if ! printf '%s\n' "$known" | grep -qxF -- "$name"; then
      echo "self-test outbound: no fixture is named $name" >&2
      exit 64
    fi
  done
  failed=0
  fixtures
  exit "$failed"
fi

clean=$(sh "$HOSTS" gate)
clean_status=$?
printf '%s\n' "$clean"
control=$(sh "$HOSTS" control)
control_status=$?
printf '%s\n' "$control"

inside=$(inside_of "$clean")
passes_inside=$(printf '%s\n' "$inside" | grep -c '^PASS ')
fails_inside=$(printf '%s\n' "$inside" | grep -c '^FAIL ')
line=$(line_of "$clean_status" "$clean" "over the stack and the other checks, which read $passes_inside PASS and $fails_inside FAIL inside the sandbox")
if [ "$clean_status" != 0 ]; then
  echo "$line"
  exit 1
fi
differs=$(why_the_inside_differs "$inside")
if [ -n "$differs" ]; then
  echo "FAIL outbound: cannot count: $differs"
  exit 1
fi
if [ "$control_status" = 2 ]; then
  echo "FAIL outbound: the control run could not count: $(why_of "$control")"
  exit 1
fi
if [ "$control_status" != 1 ]; then
  echo "FAIL outbound: the control run counted no denied attempt, so denied=0 proves nothing"
  exit 1
fi
if ! printf '%s\n' "$control" | grep -qE -- "$RECORD_A_GATE_RUN_RELIES_ON"; then
  echo "FAIL outbound: the system log holds no record of the control run's attempt, so denied=0 proves nothing"
  exit 1
fi
echo "$line; control=caught"
