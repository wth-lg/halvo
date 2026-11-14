#!/bin/sh
set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd -P)
. "$ROOT/scripts/env.sh"
PROFILE="$ROOT/scripts/outbound.sb"
BROWSER_PROFILE="$ROOT/scripts/outbound-browser.sb"
CANARY_URL=http://192.0.2.1:9/
drop_proxies_that_pass_the_profile_on_loopback() {
  unset http_proxy https_proxy all_proxy HTTP_PROXY HTTPS_PROXY ALL_PROXY npm_config_proxy npm_config_https_proxy
}
drop_proxies_that_pass_the_profile_on_loopback

usage() {
  cat >&2 <<'EOF'
Runs a command under a sandbox profile (scripts/outbound.sb) that denies every connection outside
loopback, name lookups included, ends the process that tried, and counts the attempts. A denied
attempt does not leave the machine. The README lists what the profile does not deny.

  sh scripts/outbound-hosts.sh gate            the stack and every check except 90-outbound,
                                               with a browser for the checks; each line of
                                               those checks is printed as "outbound: inside: ..."; check 55
                                               runs its bounded plan (HALVO_METERING_PLAN=bounded), and a line
                                               of it that reads another plan cannot be counted
  sh scripts/outbound-hosts.sh control         the stack alone, with the Worker runtime's
                                               geolocation fetch left on: it must count an attempt
  sh scripts/outbound-hosts.sh run CMD...      any command
  sh scripts/outbound-hosts.sh browser CMD...  any command, with a browser for it

The last line is "outbound: denied=N". N is the sum of the counts that the lines above it give,
so an attempt that two sources hold counts twice. Exit 0 when N is 0, 1 when it is not, 2 when the count
could not be taken. The command runs in a process group of its own. What is left of that group
and of its children when the command ends is listed and stopped before the count is taken; the
count cannot be taken when it does not stop. In the gate mode the count cannot be taken when the gate inside the sandbox
ends with a code other than 0 or 1, or when a check breaks its contract there.
macOS only: it needs sandbox-exec and the log command.
EOF
  exit 64
}

cannot_count() {
  echo "outbound: cannot count: $1"
  exit 2
}

if [ "${1:-}" = inside ]; then
  sh "$ROOT/scripts/stack.sh" up || {
    : > "$OUTBOUND_RUN/stack-failed"
    exit 1
  }
  if [ "$2" = gate ]; then
    HALVO_METERING_PLAN=bounded sh "$ROOT/scripts/gate.sh" --except 90-outbound > "$OUTBOUND_RUN/nested-gate.txt"
    echo $? > "$OUTBOUND_RUN/nested-gate.exit"
  fi
  sh "$ROOT/scripts/stack.sh" status > /dev/null || : > "$OUTBOUND_RUN/stack-failed"
  sh "$ROOT/scripts/stack.sh" down || {
    : > "$OUTBOUND_RUN/stack-failed"
    exit 1
  }
  exit 0
fi

mode=${1:-}
with_browser=
case "$mode" in
  gate)
    [ $# -eq 1 ] || usage
    with_browser=1
    ;;
  control) [ $# -eq 1 ] || usage ;;
  run | browser)
    [ "$mode" = run ] || with_browser=1
    shift
    [ $# -ge 1 ] || usage
    ;;
  *) usage ;;
esac

drop_inherited_browser_endpoint() {
  unset HALVO_BROWSER_ENDPOINT
}
drop_inherited_browser_endpoint

[ "$(uname -s)" = Darwin ] || cannot_count "this needs macOS: sandbox-exec and log"
command -v sandbox-exec > /dev/null 2>&1 || cannot_count "sandbox-exec is missing"
set -m 2> /dev/null
case $- in
  *m*) ;;
  *) cannot_count "this sh has no job control, so the run cannot give the command a process group of its own and find what the command leaves behind" ;;
esac
set +m

names_unstopped_member() {
  for file in "$1"/*.pid; do
    [ ! -s "$file" ] || return 0
  done
  return 1
}
stop_stacks_left_by_killed_runs() {
  for left in "$ROOT"/.tmp/outbound.*/stack; do
    [ -d "$left" ] && [ ! -e "$left/known-down" ] || continue
    names_unstopped_member "$left" || continue
    if STACK_STATE="$left" sh "$ROOT/scripts/stack.sh" down > /dev/null 2>&1; then
      : > "$left/known-down"
    fi
  done
}
owner_still_runs() {
  ps -o command= -p "$(sed -n 1p "$1/owner")" 2> /dev/null | grep -q 'outbound-hosts\.sh'
}
stop_browsers_left_by_killed_runs() {
  for left in "$ROOT"/.tmp/outbound.*/browser; do
    [ -s "$left/pid" ] && [ ! -e "$left/stopped" ] || continue
    if ! owner_still_runs "$left"; then
      if ps -o command= -p "$(sed -n 1p "$left/pid")" 2> /dev/null | grep -qF -- "--user-data-dir=$left/data"; then
        kill -s TERM "$(sed -n 1p "$left/pid")" 2> /dev/null
      fi
      : > "$left/stopped"
    fi
  done
}

mkdir -p "$ROOT/.tmp"
stop_stacks_left_by_killed_runs
stop_browsers_left_by_killed_runs

RUN=$(mktemp -d "$ROOT/.tmp/outbound.XXXXXX") || cannot_count "mktemp failed"
TAG="halvo-outbound-$$-$(date +%s)"
echo "outbound: mode $mode, files in $RUN"

stream=
command_pid=
command_group=
sampler=
browser_pid=
left_and_stopped=
left_fault=
stack_leaders() {
  cat "$RUN"/stack/*.pid 2> /dev/null | awk 'NF == 1 && /^[0-9]+$/' | tr '\n' ' '
}
tree_pids() {
  ps -axo pid=,ppid=,pgid= | awk -v root="$1" -v leaders="${2:-}" '
    BEGIN { n = split(leaders, l, " "); for (i = 1; i <= n; i++) group[l[i]] = 1; tree[root] = 1 }
    { pid[NR] = $1; parent[NR] = $2; if ($3 in group) tree[$1] = 1 }
    END {
      do {
        grew = 0
        for (i = 1; i <= NR; i++) if (!(pid[i] in tree) && (parent[i] in tree)) { tree[pid[i]] = 1; grew = 1 }
      } while (grew)
      separator = ""
      for (p in tree) { printf "%s%s", separator, p; separator = "," }
    }'
}
wait_without_job_notice() {
  wait "$1" 2> /dev/null
}
still_alive() {
  ps -o stat= -p "$1" 2> /dev/null | grep -qv '^Z'
}
signal_each() {
  printf '%s\n' "$2" | tr ',' '\n' | while IFS= read -r pid; do
    kill -s "$1" "$pid" 2> /dev/null
  done
}
TENTHS_OF_A_SECOND_FROM_TERM_TO_KILL=20
stop_pids() {
  signal_each TERM "$1"
  waits=0
  while [ "$waits" -lt "$TENTHS_OF_A_SECOND_FROM_TERM_TO_KILL" ] && still_alive "$1"; do
    sleep 0.1
    waits=$((waits + 1))
  done
  signal_each KILL "$1"
}
stop_helpers() {
  : > "$RUN/stop"
  if [ -n "$browser_pid" ]; then
    stop_pids "$(tree_pids "$browser_pid")"
    : > "$RUN/browser/stopped"
    browser_pid=
  fi
  [ -z "$stream" ] || kill -s INT "$stream" 2> /dev/null
  [ -z "$stream" ] || wait_without_job_notice "$stream"
  stream=
}
alive_of() {
  ps -o pid=,stat= -p "$1" 2> /dev/null | awk '$2 !~ /^Z/ { printf "%s%s", separator, $1; separator = "," }'
}
left_of_the_command() {
  [ -z "$command_group" ] || alive_of "$(tree_pids "$command_group" "$command_group $1")"
}
stop_what_is_left_of_the_command() {
  groups_of_a_stack_that_is_up=${1:-}
  left=$(left_of_the_command "$groups_of_a_stack_that_is_up")
  [ -n "$left" ] || return 0
  [ -n "$command_pid" ] || left_and_stopped=$(ps -o pid=,ucomm= -p "$left" 2> /dev/null |
    awk '{ pid = $1; $1 = ""; sub(/^ /, ""); printf "%s%s(%s)", separator, $0, pid; separator = ", " }')
  rounds=0
  while [ -n "$left" ] && [ "$rounds" -lt 5 ]; do
    stop_pids "$left"
    sleep 0.2
    left=$(left_of_the_command "$groups_of_a_stack_that_is_up")
    rounds=$((rounds + 1))
  done
  [ -z "$left" ] || left_fault="the command left processes behind that did not stop: $left"
}
interrupted() {
  stop_what_is_left_of_the_command "$(stack_leaders)"
  stop_helpers
  [ -z "$sampler" ] || wait_without_job_notice "$sampler"
  STACK_STATE="$RUN/stack" sh "$ROOT/scripts/stack.sh" down > "$RUN/stack-down.log" 2>&1
  exit 130
}
trap interrupted INT TERM HUP

KERNEL_RECORDS='sender == "Sandbox"'
SANDBOXD_RECORDS='subsystem == "com.apple.sandbox.reporting"'
/usr/bin/log stream --style compact \
  --predicate "($KERNEL_RECORDS OR $SANDBOXD_RECORDS) AND eventMessage CONTAINS \"$TAG\"" \
  > "$RUN/system-log.txt" 2>&1 &
stream=$!
trap stop_helpers EXIT

APPLE_SIGNED_CLIENT=/usr/bin/curl
canary() {
  deny_without_kill_so_no_crash_report="(version 1)(allow default)(deny network-outbound (with message \"$TAG-canary-$1\"))"
  tries=0
  while [ "$tries" -lt 20 ]; do
    sandbox-exec -p "$deny_without_kill_so_no_crash_report" \
      "$APPLE_SIGNED_CLIENT" -s -o /dev/null --max-time 2 "$CANARY_URL"
    polls=0
    while [ "$polls" -lt 5 ]; do
      grep -q -- "$TAG-canary-$1" "$RUN/system-log.txt" && return 0
      sleep 0.1
      polls=$((polls + 1))
    done
    tries=$((tries + 1))
  done
  return 1
}

sample_lsof() {
  while [ ! -e "$RUN/stop" ]; do
    pids=$(tree_pids "$1" "$(stack_leaders)")
    [ -z "$browser_pid" ] || pids="$pids,$(tree_pids "$browser_pid")"
    lsof -nP -i -a -p "$pids" 2> /dev/null | sed 1d >> "$RUN/lsof.txt"
    echo sample >> "$RUN/lsof-samples.txt"
    sleep 0.5
  done
}

network_process() {
  ps -axo pid=,ppid=,command= |
    awk -v browser="$browser_pid" '$2 == browser && /--utility-sub-type=network\.mojom\.NetworkService/ { print $1 }' |
    tr '\n' ' '
}

start_browser() {
  mkdir -p "$RUN/browser"
  if ! NODE_OPTIONS='' node "$ROOT/test-kit/browser/command.mjs" "$RUN/browser/data" "$RUN/browser/netlog.json" \
    > "$RUN/browser/command.txt" 2> "$RUN/browser/command.err"; then
    browser_fault="no browser to start: $(sed -n 1p "$RUN/browser/command.err")"
    return 1
  fi
  set --
  while IFS= read -r argument; do
    set -- "$@" "$argument"
  done < "$RUN/browser/command.txt"
  echo $$ > "$RUN/browser/owner"
  : > "$RUN/browser/browser.log"
  sandbox-exec -D "STRICT=$PROFILE" -D "TAG=$TAG-browser" -f "$BROWSER_PROFILE" "$@" \
    >> "$RUN/browser/browser.log" 2>&1 &
  browser_pid=$!
  echo "$browser_pid" > "$RUN/browser/pid"
  tries=0
  port=
  path=
  network_before=
  while [ "$tries" -lt 300 ] && kill -0 "$browser_pid" 2> /dev/null; do
    port=$(sed -n 1p "$RUN/browser/data/DevToolsActivePort" 2> /dev/null)
    path=$(sed -n 2p "$RUN/browser/data/DevToolsActivePort" 2> /dev/null)
    network_before=$(network_process)
    [ -z "$port" ] || [ -z "$path" ] || [ -z "$network_before" ] || break
    sleep 0.1
    tries=$((tries + 1))
  done
  if [ -z "$port" ] || [ -z "$path" ] || [ -z "$network_before" ]; then
    browser_fault="the browser did not start: see $RUN/browser/browser.log"
    return 1
  fi
  HALVO_BROWSER_ENDPOINT="ws://127.0.0.1:$port$path"
  export HALVO_BROWSER_ENDPOINT
  echo "outbound: browser: pid $browser_pid under scripts/outbound-browser.sb, endpoint $HALVO_BROWSER_ENDPOINT"
}

stop_browser() {
  if [ "$(network_process)" != "$network_before" ]; then
    browser_fault="the browser's network process ended during the run, so its net log does not cover the run"
  fi
  kill -s TERM "$browser_pid" 2> /dev/null
  waits=0
  while [ "$waits" -lt 300 ] && still_alive "$browser_pid"; do
    sleep 0.1
    waits=$((waits + 1))
  done
  if still_alive "$browser_pid"; then
    stop_pids "$(tree_pids "$browser_pid")"
    browser_fault=${browser_fault:-"the browser did not end within 30 s of TERM, so its net log is not complete"}
  fi
  wait_without_job_notice "$browser_pid"
  browser_pid=
  if LC_ALL=C grep -q 'Network service crashed' "$RUN/browser/browser.log" 2> /dev/null; then
    browser_fault=${browser_fault:-"the browser's own log reports a crash of its network service"}
  fi
  : > "$RUN/browser/stopped"
  netlog=$(NODE_OPTIONS='' node "$ROOT/gate/lib/netlog.mjs" "$RUN/browser/netlog.json" 2> "$RUN/browser/netlog.err")
  netlog_status=$?
  net_attempts=$(printf '%s\n' "$netlog" | sed -n 's/^attempts \([0-9]*\)$/\1/p')
  net_probes=$(printf '%s\n' "$netlog" | sed -n 's/^probes \([0-9]*\)$/\1/p')
  net_events=$(printf '%s\n' "$netlog" | sed -n 's/^events \([0-9]*\)$/\1/p')
  if [ "$netlog_status" != 0 ] || [ -z "$net_attempts" ] || [ -z "$net_probes" ]; then
    why=$(printf '%s\n' "$netlog" | sed -n 's/^cannot count: //p' | sed -n 1p)
    browser_fault=${browser_fault:-${why:-"the net log could not be read: see $RUN/browser/netlog.err"}}
    net_attempts=${net_attempts:-0}
  fi
}

if ! canary start; then
  cannot_count "the system log showed no denial for the canary, so a count of 0 would prove nothing"
fi

browser_fault=
net_attempts=0
if [ -n "$with_browser" ] && ! start_browser; then
  cannot_count "$browser_fault"
fi

export OUTBOUND_RUN="$RUN"
export HALVO_OUTBOUND_AUDIT="$RUN/node-audit.txt"
: > "$HALVO_OUTBOUND_AUDIT"
: > "$RUN/lsof.txt"
: > "$RUN/lsof-samples.txt"
export NODE_OPTIONS="--require \"$ROOT/scripts/outbound-audit.cjs\"${NODE_OPTIONS:+ $NODE_OPTIONS}"
export STACK_STATE="$RUN/stack"
case "$mode" in
  gate)
    export STACK_PORT_OFFSET=$((STACK_PORT_OFFSET + 10))
    set -- sh "$ROOT/scripts/outbound-hosts.sh" inside gate
    ;;
  control)
    export STACK_PORT_OFFSET=$((STACK_PORT_OFFSET + 20))
    export HALVO_OUTBOUND_CONTROL=1
    export CLOUDFLARE_CF_FETCH_PATH="$RUN/cf.json"
    unset CLOUDFLARE_CF_FETCH_ENABLED
    set -- sh "$ROOT/scripts/outbound-hosts.sh" inside control
    ;;
esac

set -m
# shellcheck disable=SC2016
sandbox-exec -D "TAG=$TAG" -f "$PROFILE" /bin/sh -c \
  'command -v -- "$1" > /dev/null 2>&1 || exit 127
  : > "$OUTBOUND_RUN/sandboxed"
  shopt -s execfail 2> /dev/null
  exec "$@"
  code=$?
  : > "$OUTBOUND_RUN/not-started"
  exit "$code"' sh "$@" < /dev/null > "$RUN/command.log" 2>&1 &
command_pid=$!
command_group=$command_pid
set +m
sample_lsof "$command_pid" &
sampler=$!
wait_without_job_notice "$command_pid"
command_status=$?
command_pid=
: > "$RUN/stop"
wait_without_job_notice "$sampler"
sampler=
if STACK_STATE="$RUN/stack" sh "$ROOT/scripts/stack.sh" down > "$RUN/stack-down.log" 2>&1 && [ -d "$RUN/stack" ]; then
  : > "$RUN/stack/known-down"
fi
stop_what_is_left_of_the_command
[ -z "$with_browser" ] || stop_browser

if ! canary end; then
  cannot_count "the system log stopped showing denials during the run"
fi
SANDBOXD_LAG_POLLS_OF_A_FIFTH_OF_A_SECOND=25
wait_for_late_sandboxd_record() {
  waits=0
  while [ "$waits" -lt "$SANDBOXD_LAG_POLLS_OF_A_FIFTH_OF_A_SECOND" ] &&
    ! grep -v '^Filtering the log data' "$RUN/system-log.txt" | grep -- "$TAG" | grep -qv -- "$TAG-canary-"; do
    sleep 0.2
    waits=$((waits + 1))
  done
}
wait_for_late_sandboxd_record
stop_helpers
trap - INT TERM HUP

sed 's/^/outbound: command: /' "$RUN/command.log"
echo "outbound: command exit $command_status"
[ ! -e "$RUN/nested-gate.txt" ] || sed 's/^/outbound: inside: /' "$RUN/nested-gate.txt"
[ -z "$left_and_stopped" ] || echo "outbound: left behind by the command and stopped: $left_and_stopped"
if [ ! -e "$RUN/sandboxed" ] || [ -e "$RUN/not-started" ]; then
  cannot_count "the sandbox could not start the command (exit $command_status)"
fi
if grep -q 'Messages dropped' "$RUN/system-log.txt"; then
  cannot_count "the system log dropped messages during the run"
fi

: > "$RUN/log-denied.txt"
: > "$RUN/browser-denied.txt"
counts=$(awk -v tag="$TAG" -v run="$RUN" '
  function is_filter_banner(line) { return line ~ /^Filtering the log data/ }
  function starts_record(line) { return line ~ /^[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9] / }
  function close_record() {
    if (whose != "" && whose != "canary") {
      sub(/^.*Sandbox: /, "", first_line_of_record)
      if (!((whose, first_line_of_record) in counted_once)) {
        counted_once[whose, first_line_of_record] = 1
        count[whose]++
        print first_line_of_record > (run "/" whose ".txt")
      }
    }
    whose = ""
  }
  is_filter_banner($0) { next }
  starts_record($0) { close_record(); first_line_of_record = $0 }
  index($0, tag) {
    if (index($0, tag "-canary-")) whose = "canary"
    else if (index($0, tag "-browser")) whose = "browser-denied"
    else whose = "log-denied"
  }
  END { close_record(); print count["log-denied"] + 0, count["browser-denied"] + 0 }' "$RUN/system-log.txt")
denied_in_system_log=${counts%% *}
denied_in_browser_system_log=${counts#* }
denied_in_node_record=$(awk '{ print $1, $3 }' "$RUN/node-audit.txt" | sort -u | wc -l | tr -d ' ')
SIGKILL_STATUS=137
killed=0
[ "$command_status" != "$SIGKILL_STATUS" ] || [ "$((denied_in_system_log + denied_in_node_record))" -ne 0 ] || killed=1
echo "outbound: system log: $denied_in_system_log denied"
sed 's/^/outbound:   /' "$RUN/log-denied.txt"
echo "outbound: node record: $denied_in_node_record denied"
sort -u "$RUN/node-audit.txt" | sed 's/^/outbound:   pid /'
echo "outbound: killed with no record: $killed"
if [ -n "$with_browser" ]; then
  echo "outbound: browser: system log: $denied_in_browser_system_log denied"
  sed 's/^/outbound:   /' "$RUN/browser-denied.txt"
  if [ -n "$browser_fault" ]; then
    echo "outbound: browser: net log: no count: $browser_fault"
  else
    echo "outbound: browser: net log: $net_attempts attempts outside loopback in $net_events events, $net_probes left out as probes of IPv6"
  fi
  printf '%s\n' "$netlog" | sed -n 's/^attempt /outbound:   /p'
fi
print_lsof_samples_as_a_second_look() {
  echo "outbound: lsof: $(wc -l < "$RUN/lsof-samples.txt" | tr -d ' ') samples of the process tree; sockets outside loopback:"
  awk '$8 == "TCP" || $8 == "UDP" {
      n = split($9, ends, "->")
      for (i = 1; i <= n; i++) {
        host = ends[i]
        sub(/:[^:]*$/, "", host)
        if (host !~ /^127\./ && host != "[::1]" && host != "localhost") { print $1, $8, $9, $10; break }
      }
    }' "$RUN/lsof.txt" | sort -u | sed 's/^/outbound:   /' > "$RUN/lsof-outside.txt"
  if [ -s "$RUN/lsof-outside.txt" ]; then cat "$RUN/lsof-outside.txt"; else echo "outbound:   none"; fi
}
print_lsof_samples_as_a_second_look

if awk '$2 == "lookup" && $3 ~ /^(.*\.)?localhost$/ { found = 1 } END { exit !found }' "$RUN/node-audit.txt"; then
  echo "outbound: hint: a Node process looked up the name localhost, and the profile ends a process at any lookup: use 127.0.0.1"
elif grep -q ' network-outbound /private/var/run/mDNSResponder$' "$RUN/log-denied.txt" &&
  ! awk '$2 == "lookup" || $2 == "resolve" { found = 1 } END { exit !found }' "$RUN/node-audit.txt"; then
  echo "outbound: hint: a process looked up a name, and the profile ends a process at any lookup, of localhost too: use 127.0.0.1"
fi
own_browser=$(sed -n -E 's/^((chrome-headless-shell|Google Chrome for Testing|Chromium)[^(]*)\([0-9]+\) deny.*$/\1/p' "$RUN/log-denied.txt" | sed -n 1p)
if [ -n "$own_browser" ]; then
  echo "outbound: hint: the command started a browser of its own ($own_browser), which ended inside the sandbox at a call outside loopback, such as Chromium's probe of IPv6: get the browser from test-kit/browser and run the command in the browser mode"
fi

denied=$((denied_in_system_log + denied_in_node_record + killed + denied_in_browser_system_log + net_attempts))
if [ "$denied" -eq 0 ] && [ -e "$RUN/stack-failed" ]; then
  cannot_count "the stack did not start, lost a member or did not stop inside the sandbox, and no denied attempt explains it"
fi
if [ "$denied" -eq 0 ] && [ -n "$browser_fault" ]; then
  cannot_count "$browser_fault"
fi
if [ "$denied" -eq 0 ] && [ -n "$left_fault" ]; then
  cannot_count "$left_fault"
fi
if [ "$denied" -eq 0 ] && [ "$mode" = gate ]; then
  nested_exit=$(sed -n 1p "$RUN/nested-gate.exit" 2> /dev/null)
  case "$nested_exit" in
    0 | 1) ;;
    *) cannot_count "the gate inside the sandbox ended with exit ${nested_exit:-unknown}, so it did not run every other check" ;;
  esac
  broke_its_contract=$(grep '^FAIL [^:]*: the check broke its contract (exit ' "$RUN/nested-gate.txt" | sed -n 1p)
  [ -z "$broke_its_contract" ] || cannot_count "inside the sandbox: $broke_its_contract"
  unbounded=$(grep -E '^(PASS|FAIL) metering: ' "$RUN/nested-gate.txt" | grep -vE '^(PASS|FAIL) metering: plan=bounded ' | sed -n 1p)
  [ -z "$unbounded" ] || cannot_count "check 55 ran inside the sandbox with a plan other than bounded: $(printf '%s\n' "$unbounded" | cut -c1-80)"
fi
echo "outbound: the next number is the sum of the counts above, so an attempt that two of them hold counts twice"
echo "outbound: denied=$denied"
[ "$denied" -eq 0 ]
