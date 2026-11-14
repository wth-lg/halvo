#!/bin/sh
# sh scripts/egress/check.sh [--mode MODE] LISTFILE -- COMMAND [ARG...]
# sh scripts/egress/check.sh [--mode MODE] --self-test [--control-no-deny]
set -u
LC_ALL=C
export LC_ALL

refuse() {
  printf 'REFUSED %s: %s\n' "$1" "$2" >&2
  exit 2
}

help() {
  cat <<'EOF'
sh scripts/egress/check.sh [--mode MODE] LISTFILE -- COMMAND [ARG...]
sh scripts/egress/check.sh [--mode MODE] --self-test [--control-no-deny]
sh scripts/egress/check.sh --help
sh scripts/egress/probe.sh NAME
Runs COMMAND with its outbound connections, datagrams and name lookups outside LISTFILE denied,
or logged where the mode cannot stop them, and writes each to target/egress/<n>/denied.log
(contracts.md C26).
EOF
}

# The constants of C26.3 and C26.7, set from docs/r2/trials/egress.md.
# default_mode PLATFORM: prints the default mode of a platform, a mode or none.
default_mode() {
  case $1 in
    macos-15) printf '%s\n' sandbox ;;
    this-mac) printf '%s\n' sandbox ;;
    windows-2025) printf '%s\n' wfp ;;
    *) printf '%s\n' none ;;
  esac
}

# not_observable PLATFORM MODE PROBE: exits 0 when the mode cannot observe the path of the probe.
not_observable() {
  case $1:$2:$3 in
    windows-2025:wfp:ipv6) return 0 ;;
    *) return 1 ;;
  esac
}

built="sandbox wfp"

sandbox_entry() {
  case $2:$3 in
    v4:127.0.0.1) return 0 ;;
  esac
  return 1
}

sandbox_rights() {
  if [ ! -x /usr/bin/sandbox-exec ] || [ ! -x /usr/bin/log ]; then
    rights="sandbox-exec or log is missing"
    return 1
  fi
  if ! /usr/bin/sandbox-exec -p '(version 1)(allow default)' /usr/bin/true 2>/dev/null; then
    rights="sandbox-exec cannot apply a profile here"
    return 1
  fi
}

sandbox_profile() {
  printf '(version 1)\n(allow default)\n'
  printf '(deny file-write* (subpath "%s/target/egress") (subpath "%s/scripts/egress"))\n' "$real_root" "$real_root"
  above=$real_root
  printf '(deny file-write* (literal "%s/target") (literal "%s/scripts")' "$real_root" "$real_root"
  while [ -n "$above" ]; do
    printf ' (literal "%s")' "$above"
    above=${above%/*}
  done
  printf ')\n'
  printf '(deny signal)\n(allow signal (target same-sandbox))\n'
  [ "$2" -eq 1 ] || return 0
  printf '(deny network-outbound (with message "%s other"))\n' "$1"
  printf '(allow network-outbound (remote unix-socket))\n'
  printf '(deny network-outbound (remote unix-socket (path-literal "/private/var/run/mDNSResponder")) (with message "%s dns"))\n' "$1"
  for family in tcp4 tcp6 udp4 udp6; do
    printf '(deny network-outbound (remote %s "*:*") (with message "%s %s any"))\n' "$family" "$1" "$family"
    printf '(deny network-outbound (remote %s "localhost:*") (with message "%s %s lo"))\n' "$family" "$1" "$family"
  done
  printf '(deny network-outbound (socket-protocol 1) (with message "%s icmp4"))\n' "$1"
  printf '(deny network-outbound (socket-protocol 58) (with message "%s icmp6"))\n' "$1"
  printf '(deny mach-lookup (global-name "com.apple.dnssd.service") (with message "%s dns"))\n' "$1"
  printf '(deny mach-lookup (global-name "com.apple.nsurlsessiond") (global-name "com.apple.nsurlsessiond-launchondemand") (with message "%s session"))\n' "$1"
  printf '(deny lsopen (with message "%s open"))\n' "$1"
  while read -r number part kind host port; do
    [ -n "$number" ] || continue
    case $kind:$part:$host in
      v4:*) printf '(allow network-outbound (remote ip4 "localhost:%s"))\n' "$port" ;;
    esac
  done <"$dir/entries"
}

sandbox_mark() {
  tries=0
  while [ "$tries" -lt 20 ]; do
    tries=$((tries + 1))
    /usr/bin/sandbox-exec -p "(version 1)(allow default)(deny network-outbound (with message \"$2 mark-$3\"))" \
      "$runner" connect loopback2 >/dev/null 2>&1 &
    marker=$!
    wait "$marker"
    case $3 in
      start) starts="$starts $marker" ;;
      end) ends="$ends $marker" ;;
    esac
    waited=0
    while [ "$waited" -lt 10 ]; do
      if awk -v token="$2" -v mark="$3" -v runner="${runner##*/}" -v starts="$starts" -v ends="$ends" \
        -f "$here/sandbox.awk" "$1/stream.ndjson" 2>/dev/null; then
        return 0
      fi
      sleep 0.05
      waited=$((waited + 1))
    done
  done
  return 1
}

sandbox_setup() {
  real_root=$(cd "$root" && pwd -P) || refuse output "target/egress/$n"
  case $real_root in
    *'"'* | *\\*) refuse output "target/egress/$n" ;;
  esac
  sandbox_profile "$2" "$3" >"$1/profile.sb" || refuse output "target/egress/$n/profile.sb"
  starts=
  ends=
  [ "$3" -eq 1 ] || return 0
  /usr/bin/log stream --style ndjson --predicate "eventMessage CONTAINS \"$2\"" >"$1/stream.ndjson" 2>"$1/stream.err" &
  stream=$!
  started=$(ps -o lstart= -p "$stream") || started=
  printf 'stream=%s\nstarted=%s\n' "$stream" "$started" >>"$1/state" || refuse output "target/egress/$n/state"
  if ! sandbox_mark "$1" "$2" start; then
    sandbox_teardown "$1" "$2"
    refuse mode "the log stream gave no start marker"
  fi
}

sandbox_launch() {
  run_dir=$1
  shift 2
  {
    run_reported=$(/usr/bin/sandbox-exec -f "$run_dir/profile.sb" "$runner" run fd3 "$@" 3>&1 1>&4 4>&-)
  } 4>&1
}

sandbox_collect() {
  [ "$3" -eq 1 ] || return 0
  if ! sandbox_mark "$1" "$2" end; then
    sandbox_teardown "$1" "$2"
    refuse mode "the log stream gave no end marker"
  fi
  now=$(ps -o lstart= -p "$stream" 2>/dev/null) || now=
  command=$(ps -o command= -p "$stream" 2>/dev/null) || command=
  case $command in
    "/usr/bin/log stream"*) ;;
    *) now= ;;
  esac
  if [ -z "$now" ] || [ "$now" != "$started" ]; then
    sandbox_teardown "$1" "$2"
    refuse mode "the log stream of the check was not alive at its end marker"
  fi
  sandbox_teardown "$1" "$2"
  awk -v token="$2" -v runner="${runner##*/}" -v starts="$starts" -v ends="$ends" -f "$here/sandbox.awk" "$1/stream.ndjson" >"$1/denied.log" ||
    refuse output "target/egress/$n/denied.log"
}

sandbox_teardown() {
  if [ -n "${stream:-}" ]; then
    kill "$stream" 2>/dev/null
    wait "$stream" 2>/dev/null
    stream=
  fi
}

sandbox_reap() {
  for state in "$root"/target/egress/*/state; do
    [ -f "$state" ] || continue
    if grep -q '^done$' "$state"; then
      continue
    fi
    owner=$(sed -n 's/^pid=//p' "$state") || continue
    if [ -z "$owner" ] || kill -0 "$owner" 2>/dev/null; then
      continue
    fi
    left=$(sed -n 's/^stream=//p' "$state") || left=
    if [ -n "$left" ]; then
      recorded=$(sed -n 's/^started=//p' "$state") || recorded=
      now=$(ps -o lstart= -p "$left" 2>/dev/null) || now=
      command=$(ps -o command= -p "$left" 2>/dev/null) || command=
      case $command in
        "/usr/bin/log stream"*)
          if [ -n "$now" ] && [ "$now" = "$recorded" ]; then
            kill "$left" 2>/dev/null
          fi
          ;;
      esac
    fi
    printf 'done\n' >>"$state"
  done
}

wfp_ps() {
  MSYS2_ARG_CONV_EXCL='*' powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass \
    -File "$wfp_script" "$@"
}

wfp_entry() {
  case $2 in
    v4 | v6) return 0 ;;
  esac
  return 1
}

wfp_rights() {
  wfp_script=$(cygpath -w "$here/wfp.ps1") || {
    rights="cygpath is missing"
    return 1
  }
  checker=
  read -r checker <"/proc/$$/winpid" || {
    rights="the Windows process id of the check cannot be read"
    return 1
  }
  rights=$(wfp_ps -Action rights 2>&1) || return 1
  rights=
}

wfp_program() {
  found=$(command -v "$1" 2>/dev/null) || return 0
  case $found in
    /*) ;;
    *) return 0 ;;
  esac
  [ -f "$found" ] || found=$found.exe
  [ -f "$found" ] || return 0
  cygpath -w "$found"
}

wfp_setup() {
  wfp_dir=$1
  wfp_token=$2
  wfp_deny=$3
  shift 3
  {
    cygpath -w "$runner"
    wfp_program "$1"
  } >"$wfp_dir/programs" || refuse output "target/egress/$n/programs"
  win_dir=$(cygpath -w "$wfp_dir") || refuse output "target/egress/$n"
  out=$(wfp_ps -Action setup -Dir "$win_dir" -Token "$wfp_token" -Deny "$wfp_deny" -CheckPid "$checker" 2>&1) ||
    refuse mode "${out:-the wfp set-up failed}"
  wfp_start=${out##*start=}
  wfp_rules=${out##*rules=}
  wfp_rules=${wfp_rules%% *}
}

wfp_launch() {
  run_dir=$1
  shift 2
  "$runner" run "$run_dir/status" "$@" <&0 &
  launched_pid=$!
  root_pid=0
  read -r root_pid <"/proc/$launched_pid/winpid" || root_pid=0
  wait "$launched_pid"
  launched=$?
  run_reported=
  if [ -f "$run_dir/status" ]; then
    read -r run_reported <"$run_dir/status" || run_reported=
  fi
  return "$launched"
}

wfp_collect() {
  win_dir=$(cygpath -w "$1") || refuse output "target/egress/$n"
  allowed_pairs=$(printf '%s\n' "$entries" | awk 'NF == 5 { printf "%s%s %s", sep, $4, $5; sep = ";" }') || allowed_pairs=
  out=$(wfp_ps -Action collect -Dir "$win_dir" -Token "$2" -CheckPid "$checker" -Deny "$3" -Root "${root_pid:-0}" -Start "$wfp_start" -Rules "$wfp_rules" -Listed "${allowed_pairs:-none}" 2>&1) || {
    wfp_teardown "$1" "$2"
    refuse mode "${out:-the wfp audit could not be read}"
  }
}

wfp_teardown() {
  win_dir=$(cygpath -w "$1") || win_dir=$1
  lost=$(wfp_ps -Action teardown -Dir "$win_dir" -Token "$2" -CheckPid "$checker" 2>&1) || {
    lost="the wfp teardown failed: ${lost:-no message}"
    return 1
  }
}

wfp_reap() {
  return 0
}

mode_entry() {
  case $mode in
    sandbox) sandbox_entry "$@" ;;
    wfp) wfp_entry "$@" ;;
  esac
}

mode_rights() {
  case $mode in
    sandbox) sandbox_rights ;;
    wfp) wfp_rights ;;
  esac
}

mode_setup() {
  case $mode in
    sandbox) sandbox_setup "$@" ;;
    wfp) wfp_setup "$@" ;;
  esac
}

mode_launch() {
  case $mode in
    sandbox) sandbox_launch "$@" ;;
    wfp) wfp_launch "$@" ;;
  esac
}

mode_collect() {
  case $mode in
    sandbox) sandbox_collect "$@" ;;
    wfp) wfp_collect "$@" ;;
  esac
}

mode_teardown() {
  case $mode in
    sandbox) sandbox_teardown "$@" ;;
    wfp) wfp_teardown "$@" ;;
  esac
}

mode_reap() {
  case $mode in
    sandbox) sandbox_reap ;;
    wfp) wfp_reap ;;
  esac
}

rights=

mode=
given=0
self_test=0
control=0
list=
while [ $# -gt 0 ]; do
  case $1 in
    --help)
      help
      exit 0
      ;;
    --mode)
      [ $# -ge 2 ] || refuse usage "--mode needs a MODE"
      [ "$given" -eq 0 ] || refuse usage "--mode given twice"
      mode=$2
      given=1
      shift 2
      ;;
    --self-test)
      [ "$self_test" -eq 0 ] || refuse usage "--self-test given twice"
      self_test=1
      shift
      ;;
    --control-no-deny)
      [ "$control" -eq 0 ] || refuse usage "--control-no-deny given twice"
      control=1
      shift
      ;;
    --) refuse usage "-- without a LISTFILE before it" ;;
    -*) refuse usage "$1" ;;
    *)
      [ "$self_test" -eq 0 ] && [ "$control" -eq 0 ] || refuse usage "$1"
      case $1 in
        *[$(printf '\001-\037')]*) refuse usage "the LISTFILE path holds a byte below 0x20" ;;
      esac
      list=$1
      shift
      [ "${1:-}" = -- ] || refuse usage "LISTFILE without -- after it"
      shift
      [ $# -gt 0 ] || refuse usage "no COMMAND after --"
      break
      ;;
  esac
done
if [ "$self_test" -eq 0 ] && [ -z "$list" ]; then
  refuse usage "no LISTFILE and no --self-test"
fi
if [ "$control" -eq 1 ] && [ "$self_test" -eq 0 ]; then
  refuse usage "--control-no-deny needs --self-test"
fi
if [ "$control" -eq 1 ] && [ "${GITHUB_ACTIONS:-}" != true ]; then
  refuse usage "--control-no-deny runs on a hosted runner only"
fi
if [ "$given" -eq 1 ]; then
  case $mode in
    sandbox | pf | wfp) ;;
    *) refuse usage "--mode $mode" ;;
  esac
fi

root=$(git rev-parse --show-toplevel 2>/dev/null) || refuse no-repository "$PWD"
[ -n "$root" ] || refuse no-repository "$PWD"
here=$root/scripts/egress

exe=
system=$(uname -s)
case $system in
  Darwin)
    if [ "${GITHUB_ACTIONS:-}" = true ]; then
      platform=macos-15
    else
      platform=this-mac
    fi
    ;;
  MINGW* | MSYS* | CYGWIN*)
    platform=windows-2025
    exe=.exe
    ;;
  *) platform=none ;;
esac

# read_list LISTFILE: sets entries to the entry lines of the list file, or refuses with the cause list.
read_list() {
  [ -f "$1" ] && [ -r "$1" ] || refuse list "$1"
  size=$(wc -c <"$1") || refuse list "$1"
  entries=$(awk -v BINMODE=3 -v size="$((size))" -f "$here/list.awk" "$1") || {
    case $entries in
      "line "*) refuse list "$1 $entries" ;;
      *) refuse list "$1" ;;
    esac
  }
}

if [ -n "$list" ]; then
  read_list "$list"
fi

if [ "$given" -eq 0 ]; then
  mode=$(default_mode "$platform")
  [ "$mode" != none ] || refuse mode "no default mode on $platform"
fi
case $platform:$mode in
  this-mac:sandbox | macos-15:sandbox | windows-2025:wfp) ;;
  macos-15:pf) ;;
  this-mac:pf) refuse mode "pf runs where GITHUB_ACTIONS is true only" ;;
  *) refuse mode "$mode is no mode of $platform" ;;
esac
case " $built " in
  *" $mode "*) ;;
  *) refuse mode "$mode was not built" ;;
esac

if [ -n "$list" ]; then
  while read -r number part kind host port; do
    [ -n "$number" ] || continue
    [ "$kind" != name ] || refuse entry "$number"
    mode_entry "$part" "$kind" "$host" "$port" || refuse entry "$number"
  done <<EOF
$entries
EOF
fi

mode_rights || refuse mode "$rights"

bin=$root/target/egress/bin
runner=$bin/egress-spawn-$$$exe
digest() {
  if [ -x /usr/bin/shasum ]; then
    /usr/bin/shasum -a 256 "$@"
  else
    sha256sum "$@"
  fi
}

script_sums() {
  digest "$here/check.sh" "$here/list.awk" "$here/sandbox.awk" "$here/probe.sh" "$here/probe.rs" "$here/wfp.ps1" 2>/dev/null |
    awk '{ print $1 }'
}

build_probe() {
  scripts_sum=$(script_sums)
  [ -n "$scripts_sum" ] || refuse mode "the scripts of the check have no digest"
  mkdir -p "$bin" || refuse output target/egress/bin
  rustc --edition 2021 -O -o "$runner" "$here/probe.rs" >"$bin/build-$$.log" 2>&1 ||
    refuse output "target/egress/bin/egress-spawn$exe"
  helper_sum=$(digest "$runner" | awk '{ print $1 }')
  [ -n "$helper_sum" ] || refuse mode "the helper binary has no digest"
  for name in egress-spawn egress-probe; do
    cp "$runner" "$bin/$name-$$-copy$exe" || refuse output "target/egress/bin/$name$exe"
    mv -f "$bin/$name-$$-copy$exe" "$bin/$name$exe" || refuse output "target/egress/bin/$name$exe"
  done
}

parts_changed() {
  kept=$(digest "$runner" "$bin/egress-spawn$exe" "$bin/egress-probe$exe" 2>/dev/null |
    awk -v want="$helper_sum" '$1 == want { n++ } END { print n + 0 }')
  if [ "$kept" != 3 ]; then
    lost="a helper binary of the check changed"
  elif [ "$(script_sums)" != "$scripts_sum" ]; then
    lost="a script of the check changed"
  else
    return 1
  fi
}

identity() {
  stat -c %i "$1" 2>/dev/null || stat -f %i "$1" 2>/dev/null
}

moved() {
  [ "$(identity "$dir")" != "$dir_id" ] || [ "$(identity "$root")" != "$root_id" ] || [ "$(identity "$here")" != "$here_id" ]
}

new_run() {
  mkdir -p "$root/target/egress" || refuse output target/egress
  mode_reap
  n=1
  for taken in "$root"/target/egress/[1-9]*; do
    taken=${taken##*/}
    case $taken in
      *[!0-9]* | ??????????*) continue ;;
    esac
    [ "$taken" -lt "$n" ] || n=$((taken + 1))
  done
  while ! mkdir "$root/target/egress/$n" 2>/dev/null; do
    [ -d "$root/target/egress/$n" ] || refuse output "target/egress/$n"
    n=$((n + 1))
  done
  dir=$root/target/egress/$n
  dir_id=$(identity "$dir")
  root_id=$(identity "$root")
  here_id=$(identity "$here")
  token=halvo-egress-$n-$$-$(date +%s) || refuse output "target/egress/$n"
  printf 'pid=%s\ntoken=%s\n' "$$" "$token" >"$dir/state" || refuse output "target/egress/$n/state"
  : >"$dir/denied.log" || refuse output "target/egress/$n/denied.log"
}

# first_form LISTFILE DENY COMMAND...: one run of the first form; sets run_status, run_denied and run_code.
first_form() {
  run_list=$1
  run_deny=$2
  shift 2
  if parts_changed; then
    refuse mode "$lost"
  fi
  new_run
  printf '%s\n' "$entries" >"$dir/entries" || refuse output "target/egress/$n/entries"
  mode_setup "$dir" "$token" "$run_deny" "$@"
  run_reported=
  mode_launch "$dir" "$runner" -- "$@"
  launched=$?
  if moved; then
    mode_teardown "$dir" "$token"
    refuse mode "a directory of the check was moved during the run"
  fi
  run_status=
  if [ "$launched" -eq 0 ]; then
    case $run_reported in
      not-started | signal-[0-9]* | [0-9]* | -[0-9]*) run_status=$run_reported ;;
    esac
  fi
  if [ -z "$run_status" ]; then
    if [ "$launched" -gt 128 ]; then
      run_status=signal-$((launched - 128))
    else
      run_status=not-started
    fi
  fi
  mode_collect "$dir" "$token" "$run_deny"
  mode_teardown "$dir" "$token" || refuse mode "$lost"
  printf 'done\n' >>"$dir/state"
  if parts_changed; then
    refuse mode "$lost"
  fi
  run_denied=$(awk 'END { print NR }' "$dir/denied.log") || refuse output "target/egress/$n/denied.log"
  printf 'egress mode=%s list=%s command=%s denied=%s log=target/egress/%s/denied.log\n' \
    "$mode" "$run_list" "$run_status" "$run_denied" "$n" >&2
  if [ "$run_status" = 0 ] && [ "$run_denied" -eq 0 ]; then
    run_code=0
  else
    run_code=1
  fi
}

if [ "$self_test" -eq 0 ]; then
  build_probe
  first_form "$list" 1 "$@"
  exit "$run_code"
fi

# want PROBE: prints the proto and the dest that a log line of the probe holds.
want() {
  case $1 in
    ipv4) printf '%s\n' 'tcp 192.0.2.1:443 egress-probe' ;;
    ipv6) printf '%s\n' 'tcp [2001:db8::1]:443 egress-probe' ;;
    dns) printf '%s\n' 'dns * egress-probe' ;;
    loopback) printf '%s\n' 'tcp 127.0.0.1:47201 egress-probe' ;;
    udp) printf '%s\n' 'udp 192.0.2.1:9 egress-probe' ;;
    icmp) printf '%s\n' 'icmp * egress-probe' ;;
    resolver) printf '%s\n' 'dns * nscurl' ;;
    open) printf '%s\n' '- -:- open' ;;
    session) printf '%s\n' '- -:- nscurl' ;;
    loopback2) printf '%s\n' 'tcp 127.0.0.1:47202 egress-probe' ;;
  esac
}

# classify PROBE TABLE-PROBE: reads the run that first_form just made and prints denied, not-observable or missed.
classify() {
  wanted=$(want "$1")
  if [ "$run_code" -eq 1 ] && awk -v want="$wanted" '
    function field(key,    i, n, parts) {
      n = split($0, parts, " ")
      for (i = 1; i <= n; i++) if (index(parts[i], key "=") == 1) return substr(parts[i], length(key) + 2)
      return ""
    }
    function port(dest) {
      sub(/^.*:/, "", dest)
      return dest
    }
    BEGIN { split(want, w, " ") }
    {
      proto = field("proto"); dest = field("dest"); image = field("image")
      if (proto != w[1]) next
      if (image != "-" && image != w[3] && image != w[3] ".exe") next
      if (w[2] == "*" || dest == w[2]) found = 1
      else if (index(dest, "-:") == 1 && port(dest) == port(w[2])) found = 1
    }
    END { exit found ? 0 : 1 }' "$dir/denied.log"; then
    printf '%s\n' denied
  elif [ ! -s "$dir/denied.log" ] && [ "$run_status" != 0 ] && not_observable "$platform" "$mode" "$2"; then
    printf '%s\n' not-observable
  else
    printf '%s\n' missed
  fi
}

# empty_ok: prints ok when the run that first_form just made exited 0 with an empty log.
empty_ok() {
  if [ "$run_code" -eq 0 ] && [ ! -s "$dir/denied.log" ]; then
    printf '%s\n' ok
  else
    printf '%s\n' miss
  fi
}

listeners=
stop_listeners() {
  : >"$work/stop"
  for pid in $listeners; do
    wait "$pid"
  done
  listeners=
}

start_listener() {
  if parts_changed; then
    stop_listeners
    refuse mode "$lost"
  fi
  "$runner" serve "$1" "$work/stop" "$work/ready-$1" &
  listeners="$listeners $!"
  waited=0
  while [ ! -f "$work/ready-$1" ] && [ "$waited" -lt 100 ]; do
    kill -0 "$!" 2>/dev/null || break
    sleep 0.05
    waited=$((waited + 1))
  done
  if [ ! -f "$work/ready-$1" ]; then
    stop_listeners
    refuse port "127.0.0.1:$1"
  fi
}

deny=1
[ "$control" -eq 0 ] || deny=0
build_probe
work=$(mktemp -d "${TMPDIR:-/tmp}/egress-self-test.XXXXXX") || refuse output "${TMPDIR:-/tmp}"
trap stop_listeners EXIT
start_listener 47201
start_listener 47202
empty=scripts/egress/lists/empty.txt
loopback=scripts/egress/lists/loopback.txt
cd "$root" || refuse output "$root"
read_list "$empty"
if [ "$deny" -eq 1 ]; then
  lines="egress self-test mode=$mode deny=on"
else
  lines="egress self-test mode=$mode deny=off"
fi
probes=ok
names="ipv4 ipv6 dns loopback udp icmp"
[ "$mode" != sandbox ] || names="$names resolver session"
[ "$mode" != sandbox ] || [ "$platform" = this-mac ] || names="$names open"
lost_report() {
  [ "$1" = missed ] && [ "$deny" -eq 1 ] && [ "$run_status" != 0 ] && [ ! -s "$dir/denied.log" ]
}

lost_total=0
for name in $names; do
  lost_runs=0
  while :; do
    first_form "$empty" "$deny" sh scripts/egress/probe.sh "$name" </dev/null >/dev/null
    result=$(classify "$name" "$name")
    if lost_report "$result"; then
      lost_runs=$((lost_runs + 1))
      [ "$lost_runs" -gt 3 ] || continue
    fi
    break
  done
  [ "$result" != missed ] || probes=miss
  lost_total=$((lost_total + lost_runs))
  lines="$lines
probe name=$name result=$result"
done
first_form "$empty" "$deny" sh scripts/egress/probe.sh none </dev/null >/dev/null
quiet=$(empty_ok)
lines="$lines
quiet result=$quiet"
read_list "$loopback"
first_form "$loopback" "$deny" sh scripts/egress/probe.sh loopback </dev/null >/dev/null
listed=$(empty_ok)
unlisted_lost=0
while :; do
  first_form "$loopback" "$deny" sh scripts/egress/probe.sh loopback2 </dev/null >/dev/null
  unlisted=$(classify loopback2 loopback)
  if lost_report "$unlisted"; then
    unlisted_lost=$((unlisted_lost + 1))
    [ "$unlisted_lost" -gt 3 ] || continue
  fi
  break
done
stop_listeners
first_form "$loopback" "$deny" sh scripts/egress/probe.sh listen </dev/null >/dev/null
listen=$(empty_ok)
lines="$lines
listed result=$listed
unlisted result=$unlisted
listen result=$listen"
if [ "${GITHUB_ACTIONS:-}" != true ]; then
  lines="$lines
control result=left-out reason=not-a-hosted-runner"
fi
loop=miss
if [ "$listed" = ok ] && [ "$unlisted" != missed ] && [ "$listen" = ok ]; then
  loop=ok
fi
default=miss
trial=$root/docs/r2/trials/egress.md
constant=$(default_mode "$platform")
if [ -f "$trial" ]; then
  first=$(awk -v key="verdict egress-modes $platform: " '
    index($0, key) == 1 { rest = substr($0, length(key) + 1); sub(/[, ].*$/, "", rest); print rest; exit }' "$trial") || first=
  [ -z "$first" ] || [ "$first" != "$constant" ] || default=ok
fi
printf '%s\nchecks probes=%s quiet=%s loopback=%s default=%s lost=%s\n' "$lines" "$probes" "$quiet" "$loop" "$default" "$((lost_total + unlisted_lost))"
if [ "$probes" = ok ] && [ "$quiet" = ok ] && [ "$loop" = ok ] && [ "$default" = ok ]; then
  exit 0
fi
exit 1
