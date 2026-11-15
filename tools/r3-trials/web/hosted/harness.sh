#!/bin/sh
set -u

usage() {
  cat <<'EOF'
Usage: sh tools/r3-trials/web/hosted/harness.sh [--guard]
       sh tools/r3-trials/web/hosted/harness.sh --self-test
       sh tools/r3-trials/web/hosted/harness.sh --help

The hosted harness of the org_ cells of A13.7 (U0.7c; ruling CP-133 (i)). On a GitHub-hosted
runner it sets each of the two organisation settings (RemoteDebuggingAllowed off, ProxyMode
direct) for Chrome and Edge as the system's policy, reads the browser's own policy page back, runs
the cell with sh tools/r3-trials/web/run.sh --rows org --round 3 --relay, and takes the setting
away: on macOS a property list in /Library/Managed Preferences installed with sudo (mac.sh, with
the page reader reader.swift, which swiftc builds on the runner), on Windows a value under
HKLM\SOFTWARE\Policies (win.sh). It runs from the repository's root, after a release build of the
web trial, and prints counts, booleans and closed-list words alone. collect.py builds the block of
docs/r3/trials/web-isolation.md from the lines.

The harness changes the machine's policy, so it refuses to run, with one line
REFUSED host: github_actions=<yes|no> runner_environment=<yes|no> image_os=<yes|no>
on standard error and exit 2, before any privileged call, unless it finds itself on a hosted
runner by three facts that a person's Mac lacks: GITHUB_ACTIONS is true, RUNNER_ENVIRONMENT is
github-hosted and ImageOS is set. A person can set the three by hand; the guard keeps the harness
from running by accident, and no check and no test of the repository calls the harness.

What the harness puts back: a policy file (macOS) or value (Windows) that stood before a cell is
moved aside or read before the cell's own and put back after it; a key or the managed directory
that the harness made is taken away; a trap on EXIT, HUP, INT and TERM does the same and ends the
call in progress when the harness itself is ended. Each call of run.sh on Windows gets a port that
tools/r3-trials/web/port-free.mjs reads as free. The preferences daemon of macOS is ended by its
recorded number and start time, read again before the kill; the processes of a policy page's
browser on Windows by their recorded number and creation time (page.ps1), as a tree under the
browser that page.ps1 started. On Windows pypdf comes from the line of fetch.txt alone: its bytes
and SHA-256 are checked and its licence is read from the wheel before pip installs it under
RUNNER_TEMP.

  --guard      runs the guard alone: prints "guard passed" and exits 0, or refuses.
  --self-test  runs the refusal with no privilege: the harness under each environment that lacks a
               fact, with stand-ins for sudo, reg, powershell, killall, install, defaults and
               swiftc on the PATH that record any call, must exit 2 with the one refused line and
               no call recorded; the guard with the three facts passes. Then the functions that
               put a setting back, on stand-ins (a registry of directories, sudo without privilege,
               node, curl and python): each named case of the list above. Prints one line per
               case and "self-test cases=<n> failed=<n>"; exit 0 when none failed.
Optional variables of the run:
  LIMIT       seconds that one call of run.sh may take before the harness ends it (default 420)
  RUN_SH      on macOS, the trial's run.sh (default tools/r3-trials/web/run.sh)
  PAGE_WAIT   on macOS, seconds to wait for a policy page's PDF (default 45)
  RUNS        on Windows, runs per cell (default 10; macOS runs 10)
  KEEP_PAGES  on Windows, when set: keep each page's PDF and text under target/ci-artifacts/pages
  PREFS_DAEMON on macOS, the program of the preferences daemon (default /usr/sbin/cfprefsd)
Exit 0 when every cell ran, 1 when a cell or a step failed, 2 for a refusal.
EOF
}

yes_no() {
  if "$@"; then echo yes; else echo no; fi
}

facts() {
  a=$(yes_no [ "${GITHUB_ACTIONS:-}" = true ])
  r=$(yes_no [ "${RUNNER_ENVIRONMENT:-}" = github-hosted ])
  i=$(yes_no [ -n "${ImageOS:-}" ])
}

guard() {
  facts
  if [ "$a$r$i" != yesyesyes ]; then
    echo "REFUSED host: github_actions=$a runner_environment=$r image_os=$i" >&2
    exit 2
  fi
}

here=$(cd "$(dirname "$0")" && pwd)

self_case() {
  name=$1
  shift
  log="$work/calls.$name"
  : >"$log"
  env -i PATH="$work/bin:/usr/bin:/bin" HOME="$work" R3_STUB_LOG="$log" "$@" \
    sh "$here/harness.sh" >"$work/out.$name" 2>"$work/err.$name"
  e=$?
  lines=$(wc -l <"$work/err.$name" | tr -d ' ')
  refused=$(grep -c '^REFUSED host: github_actions=[a-z]* runner_environment=[a-z]* image_os=[a-z]*$' "$work/err.$name")
  out=$(wc -c <"$work/out.$name" | tr -d ' ')
  calls=$(wc -l <"$log" | tr -d ' ')
  ok=no
  [ "$e" -eq 2 ] && [ "$lines" -eq 1 ] && [ "$refused" -eq 1 ] && [ "$out" -eq 0 ] && [ "$calls" -eq 0 ] && ok=yes
  echo "case $name exit=$e refused_lines=$refused stderr_lines=$lines stdout_bytes=$out privileged_calls=$calls ok=$ok"
  cases=$((cases + 1))
  [ "$ok" = yes ] || failed=$((failed + 1))
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/r3-hosted-harness.XXXXXX") || exit 2
  mkdir -p "$work/bin"
  cat >"$work/stub" <<'STUB'
#!/bin/sh
echo "$(basename "$0") $*" >>"$R3_STUB_LOG"
exit 0
STUB
  for t in sudo reg powershell killall install defaults swiftc plutil; do
    cp "$work/stub" "$work/bin/$t"
    chmod 755 "$work/bin/$t"
  done
  cases=0
  failed=0
  self_case none
  self_case actions_only GITHUB_ACTIONS=true
  self_case no_image GITHUB_ACTIONS=true RUNNER_ENVIRONMENT=github-hosted
  self_case empty_image GITHUB_ACTIONS=true RUNNER_ENVIRONMENT=github-hosted ImageOS=
  self_case actions_one GITHUB_ACTIONS=1 RUNNER_ENVIRONMENT=github-hosted ImageOS=macos15
  self_case self_hosted GITHUB_ACTIONS=true RUNNER_ENVIRONMENT=self-hosted ImageOS=macos15
  self_case no_actions RUNNER_ENVIRONMENT=github-hosted ImageOS=macos15
  : >"$work/calls.control"
  R3_STUB_LOG="$work/calls.control" "$work/bin/sudo" true
  seen=$(wc -l <"$work/calls.control" | tr -d ' ')
  ok=no
  [ "$seen" -eq 1 ] && ok=yes
  echo "case stub_records_a_call calls=$seen ok=$ok"
  cases=$((cases + 1))
  [ "$ok" = yes ] || failed=$((failed + 1))
  passed=$(env -i PATH="$work/bin:/usr/bin:/bin" R3_STUB_LOG="$work/calls.guard" GITHUB_ACTIONS=true \
    RUNNER_ENVIRONMENT=github-hosted ImageOS=macos15 sh "$here/harness.sh" --guard 2>/dev/null)
  e=$?
  ok=no
  [ "$e" -eq 0 ] && [ "$passed" = "guard passed" ] && ok=yes
  echo "case three_facts_pass_the_guard exit=$e ok=$ok"
  cases=$((cases + 1))
  [ "$ok" = yes ] || failed=$((failed + 1))
  sh "$here/harness.sh" --help >"$work/help" 2>&1
  missing=
  for v in "LIMIT .*420" "RUN_SH .*tools/r3-trials/web/run.sh" "PAGE_WAIT .*45" "RUNS .*10" "KEEP_PAGES " \
    "PREFS_DAEMON .*/usr/sbin/cfprefsd"; do
    grep -q "^  $v" "$work/help" || missing="$missing ${v%% *}"
  done
  ok=no
  [ -z "$missing" ] && ok=yes
  echo "case help_names_each_optional_variable missing=${missing:- -} ok=$ok"
  cases=$((cases + 1))
  [ "$ok" = yes ] || failed=$((failed + 1))
  put_back_cases
  echo "self-test cases=$cases failed=$failed"
  [ "$failed" -eq 0 ]
}

verdict() {
  ok=no
  [ "$2" = yes ] && ok=yes
  echo "case $1 $3 ok=$ok"
  cases=$((cases + 1))
  [ "$ok" = yes ] || failed=$((failed + 1))
}

holds() {
  if "$@"; then echo yes; else echo no; fi
}

live() {
  s=$(ps -o stat= -p "$1" 2>/dev/null | tr -d ' ')
  [ -n "$s" ] && [ "${s#Z}" = "$s" ]
}

has() {
  grep -q -- "$2" "$1" 2>/dev/null
}

stand_ins() {
  rb="$work/run-bin"
  mkdir -p "$rb"
  cat >"$rb/sudo" <<'STUB'
#!/bin/sh
echo "sudo $*" >>"$R3_STUB_LOG"
if [ "$1" = install ]; then
  shift
  args=
  while [ $# -gt 2 ]; do
    case $1 in
      -o | -g) shift 2 ;;
      *) args="$args $1" && shift ;;
    esac
  done
  # shellcheck disable=SC2086
  exec install $args "$1" "$2"
fi
exec "$@"
STUB
  cat >"$rb/reg" <<'STUB'
#!/bin/sh
echo "reg $*" >>"$R3_STUB_LOG"
op=$1
key=$2
shift 2
rel=$(printf '%s' "$key" | tr '\\' '/' | sed 's|^HKLM/SOFTWARE/Policies||')
d="$R3_REG$rel"
name=
type=
data=
while [ $# -gt 0 ]; do
  case $1 in
    /v) name=$2 && shift 2 ;;
    /t) type=$2 && shift 2 ;;
    /d) data=$2 && shift 2 ;;
    *) shift ;;
  esac
done
case $op in
  query)
    [ -d "$d" ] || exit 1
    [ -z "$name" ] && exit 0
    [ -f "$d/$name" ] || exit 1
    v=$(cat "$d/$name")
    printf '
%s
    %s    %s    %s

' "$key" "$name" "${v%% *}" "${v#* }"
    ;;
  add)
    mkdir -p "$d" || exit 1
    [ -z "$name" ] || printf '%s %s' "$type" "$data" >"$d/$name"
    ;;
  delete)
    [ -n "${R3_REG_NO_DELETE:-}" ] && exit 1
    mkdir -p "$R3_REG.gone"
    to="$R3_REG.gone/$(find "$R3_REG.gone" -mindepth 1 -maxdepth 1 | wc -l | tr -d ' ')"
    if [ -n "$name" ]; then
      [ -f "$d/$name" ] && mv "$d/$name" "$to"
    else
      [ -d "$d" ] && mv "$d" "$to"
    fi
    ;;
esac
STUB
  cat >"$rb/node" <<'STUB'
#!/bin/sh
echo "node $*" >>"$R3_STUB_LOG"
[ "$2" = "${R3_TAKEN_PORT:-0}" ] && exit 1
exit 0
STUB
  cat >"$rb/curl" <<'STUB'
#!/bin/sh
echo "curl $*" >>"$R3_STUB_LOG"
while [ $# -gt 0 ]; do
  [ "$1" = -o ] && printf 'not the wheel\n' >"$2"
  shift
done
exit 0
STUB
  sed 's/^echo "curl/echo "python/' "$rb/curl" >"$rb/python"
  for s in sudo reg node curl python; do chmod 755 "$rb/$s"; done
}

in_stand_ins() {
  env -i PATH="${R3_FIRST_BIN:-$work/run-bin}:$work/run-bin:/usr/bin:/bin" HOME="$work" R3_STUB_LOG="$1/calls" \
    R3_REG="$1/reg" R3_REG_NO_DELETE="${R3_REG_NO_DELETE:-}" R3_TAKEN_PORT=18190 RUNNER_TEMP="$1" here="$here" \
    "${R3_CASE_SH:-sh}" -s >"$1/out" 2>&1
}

posix_sh() {
  if [ -x /bin/dash ]; then echo /bin/dash; else echo sh; fi
}

put_back_cases() {
  stand_ins
  c="$work/w-earlier"
  mkdir -p "$c/reg/Google/Chrome"
  printf 'REG_DWORD 0x1' >"$c/reg/Google/Chrome/RemoteDebuggingAllowed"
  in_stand_ins "$c" <<'CASE'
. "$here/win.sh"; k=chrome
policy_set "Google\\Chrome" RemoteDebuggingAllowed REG_DWORD 0
[ "$(cat "$R3_REG/Google/Chrome/RemoteDebuggingAllowed")" = "REG_DWORD 0" ] || echo not_set
policy_unset
CASE
  v=$(cat "$c/reg/Google/Chrome/RemoteDebuggingAllowed" 2>/dev/null)
  verdict win_puts_an_earlier_value_back \
    "$(holds test "$v" = "REG_DWORD 0x1" -a -d "$c/reg/Google/Chrome")$(has "$c/out" not_set && echo _not_set)" \
    "value_after=$(printf '%s' "$v" | tr ' ' '_')"
  c="$work/w-made"
  mkdir -p "$c/reg/Microsoft"
  in_stand_ins "$c" <<'CASE'
. "$here/win.sh"; k=chrome
policy_set "Google\\Chrome" ProxyMode REG_SZ direct
[ "$(cat "$R3_REG/Google/Chrome/ProxyMode")" = "REG_SZ direct" ] || echo not_set
policy_unset
CASE
  verdict win_takes_away_the_keys_that_it_made \
    "$(holds test ! -e "$c/reg/Google" -a -d "$c/reg/Microsoft")$(has "$c/out" "policy removed=yes name=ProxyMode earlier=no keys_left=0" || echo _line)$(has "$c/out" not_set && echo _not_set)" \
    "google_key_left=$(holds test -e "$c/reg/Google")"
  c="$work/w-term"
  mkdir -p "$c/reg/Google/Chrome"
  printf 'REG_SZ fixed_servers' >"$c/reg/Google/Chrome/ProxyMode"
  R3_CASE_SH=$(posix_sh) in_stand_ins "$c" <<'CASE'
. "$here/win.sh"; k=chrome; arm_undo
policy_set "Google\\Chrome" ProxyMode REG_SZ direct >/dev/null
[ "$(cat "$R3_REG/Google/Chrome/ProxyMode")" = "REG_SZ direct" ] || echo not_set
kill -TERM $$; sleep 5; echo not_ended
CASE
  v=$(cat "$c/reg/Google/Chrome/ProxyMode" 2>/dev/null)
  verdict win_a_term_puts_the_value_back \
    "$(holds test "$v" = "REG_SZ fixed_servers")$(has "$c/out" not_ended && echo _not_ended)$(has "$c/out" not_set && echo _not_set)" \
    "value_after=$(printf '%s' "$v" | tr ' ' '_')"
  c="$work/w-stuck"
  mkdir -p "$c/reg/Google/Chrome"
  R3_REG_NO_DELETE=1 in_stand_ins "$c" <<'CASE'
. "$here/win.sh"; k=chrome
policy_set "Google\\Chrome" ProxyMode REG_SZ direct >/dev/null
policy_unset
CASE
  verdict win_says_removed_no_when_the_value_stays \
    "$(holds has "$c/out" "policy removed=no name=ProxyMode earlier=no keys_left=0")" "-"
  c="$work/w-call"
  mkdir -p "$c"
  in_stand_ins "$c" <<'CASE'
. "$here/win.sh"
(sleep 3 && echo late >"$RUNNER_TEMP/late") &
current_call=$!
undo
sleep 5
if [ -e "$RUNNER_TEMP/late" ]; then echo call_alive; else echo call_ended; fi
CASE
  verdict win_ends_the_call_in_progress_when_it_is_ended \
    "$(holds has "$c/out" call_ended)" "out=$(tr ' \n' '_;' <"$c/out" | cut -c1-120)"
  c="$work/w-port"
  mkdir -p "$c"
  in_stand_ins "$c" <<'CASE'
. "$here/win.sh"; free_port 18190; echo "port=$port"
CASE
  verdict win_moves_a_call_off_a_taken_port \
    "$(holds has "$c/out" "port_check port=18190 free=no used=19190")$(has "$c/out" "port=19190" || echo _port)" "-"
  c="$work/w-wheel"
  mkdir -p "$c"
  in_stand_ins "$c" <<'CASE'
. "$here/win.sh"; pypdf_ready "$here" && echo installed
CASE
  verdict win_refuses_a_wheel_whose_bytes_differ_from_its_line \
    "$(holds has "$c/out" "bytes_ok=no sha256_ok=no")$(has "$c/out" installed && echo _installed)$(has "$c/calls" "^python" && echo _python)" \
    "python_calls=$(grep -c '^python' "$c/calls")"
  for same in yes no; do
    c="$work/m-reuse-$same"
    mkdir -p "$c/bin"
    cp "$work/stub" "$c/bin/sudo"
    later="Sat Nov 15 09:00:00 2026"
    [ "$same" = yes ] && later="Sat Nov 15 08:00:00 2026"
    printf '#!/bin/sh\ncase "$*" in\n  *-axo*) echo "4242 Sat Nov 15 08:00:00 2026 /usr/sbin/cfprefsd" ;;\n  *lstart=*) echo "%s" ;;\nesac\n' "$later" >"$c/bin/ps"
    chmod 755 "$c/bin/sudo" "$c/bin/ps"
    R3_FIRST_BIN="$c/bin" in_stand_ins "$c" <<'CASE'
. "$here/mac.sh"
prefs_flush
CASE
    if [ "$same" = yes ]; then
      verdict mac_ends_a_daemon_whose_start_time_reads_the_same \
        "$(holds has "$c/out" "prefs_daemon found=1 ended=1")$(has "$c/calls" "^sudo kill 4242$" || echo _no_kill)" "-"
    else
      verdict mac_leaves_a_process_whose_start_time_changed \
        "$(holds has "$c/out" "prefs_daemon found=1 ended=0")$(has "$c/calls" "kill" && echo _killed)" "-"
    fi
  done
  [ "$(uname -s)" = Darwin ] || return 0
  c="$work/m-earlier"
  mkdir -p "$c/managed" "$c/work"
  printf 'earlier\n' >"$c/managed/com.google.Chrome.plist"
  in_stand_ins "$c" <<'CASE'
. "$here/mac.sh"; dir="$RUNNER_TEMP/managed"; work="$RUNNER_TEMP/work"
PREFS_DAEMON="$RUNNER_TEMP/none"; set_policy com.google.Chrome chrome RemoteDebuggingAllowed "<false/>"
grep -q RemoteDebuggingAllowed "$dir/com.google.Chrome.plist" || echo not_set; take_away
CASE
  verdict mac_puts_an_earlier_policy_file_back \
    "$(holds test "$(cat "$c/managed/com.google.Chrome.plist")" = earlier)$(has "$c/out" not_set && echo _not_set)$(has "$c/out" "policy removed=yes name=RemoteDebuggingAllowed earlier=yes put_back=yes" || echo _line)$([ "$(grep -c '^prefs_daemon ' "$c/out")" = 2 ] || echo _flushes)" \
    "flushes=$(grep -c '^prefs_daemon ' "$c/out")"
  c="$work/m-call"
  mkdir -p "$c/work"
  in_stand_ins "$c" <<'CASE'
. "$here/mac.sh"; dir="$RUNNER_TEMP/managed"; work="$RUNNER_TEMP/work"; PREFS_DAEMON="$RUNNER_TEMP/none"
perl -e 'setpgrp(0, 0); exec @ARGV or exit 127' sleep 30 &
current_call=$!
sleep 1
undo
if alive "$current_call"; then echo call_alive; else echo call_ended; fi
kill -TERM -- "-$current_call" 2>/dev/null
CASE
  verdict mac_ends_the_call_in_progress_when_it_is_ended \
    "$(holds has "$c/out" call_ended)" "-"
  c="$work/m-made"
  mkdir -p "$c/work"
  in_stand_ins "$c" <<'CASE'
. "$here/mac.sh"; dir="$RUNNER_TEMP/managed"; work="$RUNNER_TEMP/work"
PREFS_DAEMON="$RUNNER_TEMP/none"; set_policy com.google.Chrome chrome ProxyMode "<string>direct</string>"; undo
CASE
  verdict mac_takes_away_its_file_and_the_directory_that_it_made \
    "$(holds test ! -e "$c/managed")$(has "$c/out" "harness_end dir_removed=yes" || echo _line)" "-"
  c="$work/m-kept"
  mkdir -p "$c/work" "$c/managed"
  in_stand_ins "$c" <<'CASE'
. "$here/mac.sh"; dir="$RUNNER_TEMP/managed"; work="$RUNNER_TEMP/work"
PREFS_DAEMON="$RUNNER_TEMP/none"; set_policy com.google.Chrome chrome ProxyMode "<string>direct</string>"; undo
CASE
  verdict mac_keeps_a_directory_that_stood_before \
    "$(holds test -d "$c/managed" -a ! -e "$c/managed/com.google.Chrome.plist")$(has "$c/out" "harness_end dir_removed=-" || echo _line)" "-"
  c="$work/m-term"
  mkdir -p "$c/managed" "$c/work"
  printf 'earlier\n' >"$c/managed/com.google.Chrome.plist"
  R3_CASE_SH=$(posix_sh) in_stand_ins "$c" <<'CASE'
. "$here/mac.sh"; dir="$RUNNER_TEMP/managed"; work="$RUNNER_TEMP/work"
PREFS_DAEMON="$RUNNER_TEMP/none"; arm_undo
set_policy com.google.Chrome chrome ProxyMode "<string>direct</string>"
grep -q ProxyMode "$dir/com.google.Chrome.plist" || echo not_set
kill -TERM $$; sleep 5; echo not_ended
CASE
  verdict mac_a_term_puts_the_policy_file_back \
    "$(holds test "$(cat "$c/managed/com.google.Chrome.plist")" = earlier)$(has "$c/out" not_ended && echo _not_ended)$(has "$c/out" not_set && echo _not_set)" "-"
  c="$work/m-daemon"
  mkdir -p "$c/bin"
  ln -s /bin/sleep "$c/bin/r3prefsd"
  "$c/bin/r3prefsd" 30 &
  dp=$!
  /bin/sleep 30 &
  op=$!
  sleep 1
  in_stand_ins "$c" <<'CASE'
. "$here/mac.sh"; PREFS_DAEMON="$RUNNER_TEMP/bin/r3prefsd"; prefs_flush
CASE
  sleep 1
  d_alive=no
  o_alive=no
  live "$dp" && d_alive=yes
  live "$op" && o_alive=yes
  kill "$dp" "$op" 2>/dev/null
  wait "$dp" "$op" 2>/dev/null
  verdict mac_ends_the_preferences_daemon_by_its_recorded_number \
    "$(holds has "$c/out" "prefs_daemon found=1 ended=1")$([ "$d_alive$o_alive" = noyes ] || echo _alive)" \
    "daemon_alive=$d_alive other_alive=$o_alive"
}

case ${1:-} in
  --help)
    usage
    exit 0
    ;;
  --self-test)
    [ $# -eq 1 ] || {
      echo "REFUSED usage: --self-test takes no argument" >&2
      exit 2
    }
    self_test
    exit $?
    ;;
  --guard)
    guard
    echo "guard passed"
    exit 0
    ;;
  '') ;;
  *)
    echo "REFUSED usage: unknown argument; see --help" >&2
    exit 2
    ;;
esac

guard
case $(uname -s) in
  Darwin)
    # shellcheck source=SCRIPTDIR/mac.sh
    . "$here/mac.sh"
    mac_main "$here"
    ;;
  MINGW* | MSYS* | CYGWIN*)
    # shellcheck source=SCRIPTDIR/win.sh
    . "$here/win.sh"
    win_main "$here"
    ;;
  *)
    echo "REFUSED host: no harness for $(uname -s)" >&2
    exit 2
    ;;
esac
