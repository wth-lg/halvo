#!/bin/sh
set -u

usage() {
  cat <<'EOF'
Usage: sh tools/r3-trials/web/run.sh --candidate cdp --browser PATH --port N --runs N [--step NAME]
       sh tools/r3-trials/web/run.sh --candidate cdp --browser PATH --port N --runs N --rows isolation [--step NAME]
       sh tools/r3-trials/web/run.sh --candidate cdp --browser PATH --port N --extras
       sh tools/r3-trials/web/run.sh --candidate cdp --browser PATH --port N --resolver-probe
       sh tools/r3-trials/web/run.sh --candidate cdp --browser PATH --port N --snapshot-pay FILE
       sh tools/r3-trials/web/run.sh --help

The web executor trial of unit U0.2 (contract A13.2). It builds the trial program with
cargo --locked into its own target directory for the host that rustc -vV names, whatever
CARGO_TARGET_DIR or a build target of the caller says, and runs that binary. It starts the
fixture server of tools/fixture-server/variants on 127.0.0.1:N,
starts the browser headless with a new profile for every run, and stops both at the end.

  --candidate C     cdp, the adapter on chromiumoxide 0.9.1. webdriver and playwright have
                    no adapter: the trial builds one only after the candidate before it missed.
  --browser PATH    the Chrome or Edge binary (the file name says which).
  --port N          the fixture port. --extras also uses N+1 for the debugging port;
                    --fault also uses N+2 for the fixture behind the fault proxy.
  --runs N          runs per step, from 1 to 4294967295; a cell of A13.2 is 10.
  --step NAME       one of the nine steps; without it, all nine in the order of A13.2.
  --rows isolation  runs the six rows of A13.7 of unit U0.7 in place of the nine steps, in the
                    order of A13.7, or with --step one of them (payment_frame_isolated,
                    changed_page_1_isolated, block_popup, block_frame_post, block_websocket,
                    cert_error). Each browser of these rows starts with one start flag more,
                    --isolate-origins=http://pay.agent.test:N, which puts the payment frame in a
                    process of its own. cert_error makes a certificate with the system's openssl in
                    the scratch directory and serves it with tools/r3-trials/web/tls-listener.mjs on
                    a loopback port that the system picks; without openssl its cell is a notrun
                    line with cause=openssl. --fault does not take these rows.
  --rows driver     runs the row driver_killed_port of ruling CP-22: a driver process (this
                    program) starts the browser over the loopback port, holds one POST paused,
                    and is killed with the system's hard kill (SIGKILL, or TerminateProcess on
                    Windows); tools/r3-trials/web/record-listener.mjs records for 10 s every
                    connection and byte that reaches the POST's address, and the program watches
                    the browser's process. One control run lets the POST through first, and the
                    listener must see it. --runs gives the number of kills; the row's cells take 20.
  --driver PATH     taken by the webdriver candidate only; refused here.
  --extras          prints the extras line: added_bytes (release binary with the driver crate
                    against one without it, both stripped), cold_ms (median of 10 starts to the
                    load of /signin), record, attach and pipe. With --round 3 the binary with the
                    driver is built with the protocol types and the pipe alone (no port control,
                    --no-default-features --features driver), every start is the product's on the
                    pipe with the relay and its probe, and the line has no attach and no pipe key.
  --resolver-probe  starts one browser, opens a name outside *.test and one inside, and prints
                    which error the first got and how many requests each sent to the fixture.
  --snapshot-pay F  writes the snapshot of /pay of seed 1 with its frame to F.
  --fault NAME      runs the steps behind tools/r3-trials/web/faults.mjs, which breaks one route
                    or one page (signin_500, history_changed, prefs_500, frame_hangs,
                    upload_input_gone, pay_now_renamed, prefs_get_form, password_plain).
  --scratch DIR     the directory for profiles and upload files; default a new mktemp directory.
  --round 2         runs the second round of A13.7 (U0.7b): needs --relay; every browser starts with
                    the agent's settings in its new profile and the start flags of U0.7b, and the
                    relay holds a list: the fixture's utility and pay hosts on its port, and what a
                    row adds.
  --round 3         runs the third round of A13.7 (U0.7c) as --round 2 does, with each browser
                    started by the trial program on its debugging pipe and driven over it alone.
  --channel C       with --round 3: pipe (the default, the product's form) or port, the control
                    in which the browser is driven over a debugging port on the loopback address.
                    With --round 3 every run on the pipe also reads the checks of A11.1 at the
                    start, after the row and after the stop, and each cell prints a check line.
  --rows round3     with --round 3 and --relay, runs the rows of U0.7c: pipe_inherit,
                    relay_foreign, start_probe, same_site_pref and same_site_rule.
  --rows org        with --round 3 and --relay and --step org_debugging or org_proxy, runs a cell
                    of org_policy: the caller has set the organisation's setting (debugging
                    forbidden, or a proxy) for the browser before the call and takes it away
                    after it; on this Mac the cell is a notrun line with cause=no_policy_place.
  --rows driver     with --round 3 also runs relay_port_free after the four forms.
  --rows relay      with --round 2, runs the rows of the relay's list: list_refuse, list_hops,
                    list_close and own_traffic (tools/r3-trials/web/chain-listener.mjs serves the
                    hops).
  --rows ways       with --round 2, runs the recorded rows of U0.7b that the program holds:
                    ws_allowed with its control (tools/r3-trials/web/ws-listener.mjs is the
                    listener) and frame_doc on the port with the relay.
  --rows egress     with --round 2, runs the rows of the ways out (no_worker_paths, udp_quic,
                    webtransport, peer_connection, secure_dns, lan_discovery, link_lookup,
                    after_unload, reports, handler_launch, updater_wake) with the browser as the
                    command of sh scripts/egress/check.sh and a list that holds the relay's port
                    alone (tools/r3-trials/web/ways-listener.mjs serves the pages); handler_launch
                    does not run on this Mac.
  --rows reach      with --round 2, runs the rows of ruling CP-78: through the relay, a read to a
                    host outside the request rule's list from a page, a page opened after the
                    rule was armed, a frame, a dedicated, a shared and a service worker.
  --net-log DIR     with --rows egress, each browser writes its network log to DIR/<row>-<i>.json.
  --relay           runs the cells with the relay of ruling CP-24 in the trial program's own
                    process: every browser gets it as its only route, with the flags that the
                    README names.
  --routes          after each run's step, reads the system's table of established TCP
                    connections to the fixture's port (lsof on macOS, netstat on Windows) and
                    prints one route line per cell: how many came from this program, which
                    holds the relay, and how many from any other process.

Output: one run line per run, one cell line per step, and no verdict line. A run line holds the
browser product and version that the browser reported over the protocol. median_ms is the median
of the runs that printed ok. A run whose fixture control failed, a browser that did not start
(cause=browser_start), and a cell with no browser version make a notrun line in place of the cell
line. The standard error of a browser that did not start is kept beside its profile directory in
the scratch directory, in a file whose name ends in .stderr, and is not printed.
Exit 0 when every cell ran (a miss is a result), 1 when a cell could not run (a notrun line
names it), 2 for a refusal: one line REFUSED <usage|browser|port>: <detail> on standard error.
run.sh ends by exec of the trial program, so a TERM or INT sent to run.sh reaches the program. On
TERM or INT the program stops its browsers, the fixture server and the fault proxy, prints a
notrun line with cause=signal and exits 1.
EOF
}

refuse() {
  echo "REFUSED $1: $2" >&2
  exit 2
}

candidate='' browser='' port='' runs='' step='' mode=cells out='' fault='' scratch='' rows=nine
relay='' routes='' round='' netlog='' channel=''
while [ $# -gt 0 ]; do
  case "$1" in
    --help) usage; exit 0 ;;
    --extras) mode=extras; shift; continue ;;
    --resolver-probe) mode=resolver; shift; continue ;;
    --relay) relay=yes; shift; continue ;;
    --net-log)
      [ $# -ge 2 ] || refuse usage "--net-log needs a directory"
      netlog=$2; shift 2; continue ;;
    --routes) routes=yes; shift; continue ;;
  esac
  [ $# -ge 2 ] || refuse usage "$1 needs a value or is unknown; see --help"
  case "$1" in
    --candidate) candidate=$2 ;;
    --browser) browser=$2 ;;
    --port) port=$2 ;;
    --runs) runs=$2 ;;
    --step) step=$2 ;;
    --round) round=$2 ;;
    --channel) channel=$2 ;;
    --rows) rows=$2 ;;
    --driver) refuse usage "--driver belongs to the webdriver candidate, which has no adapter" ;;
    --snapshot-pay) mode=snapshot; out=$2 ;;
    --fault) fault=$2 ;;
    --scratch) scratch=$2 ;;
    *) refuse usage "unknown argument $1; see --help" ;;
  esac
  shift 2
done

case "$candidate" in
  cdp) ;;
  webdriver | playwright) refuse usage "no adapter for $candidate: the trial builds it only after the candidate before it missed" ;;
  *) refuse usage "--candidate takes cdp" ;;
esac
case "$port" in
  '' | *[!0-9]*) refuse usage "--port takes an integer from 1 to 65532" ;;
esac
[ "$port" -ge 1 ] && [ "$port" -le 65532 ] || refuse usage "--port takes an integer from 1 to 65532"
if [ "$mode" = cells ]; then
  case "$runs" in
    '' | *[!0-9]* | 0*) refuse usage "--runs takes a count from 1 to 4294967295" ;;
  esac
  [ "${#runs}" -le 10 ] && [ "$runs" -le 4294967295 ] ||
    refuse usage "--runs takes a count from 1 to 4294967295"
fi
case "$rows" in
  nine)
    case "$step" in
      '' | signin_form | table_read | form_fill | payment_frame | file_upload | session_expiry | \
        changed_page_1 | block_request | credential_mark) ;;
      *) refuse usage "--step takes one of the nine steps of A13.2, or with --rows isolation one of the six rows of A13.7" ;;
    esac
    ;;
  isolation)
    [ "$mode" = cells ] || refuse usage "--rows isolation runs cells only"
    [ -z "$fault" ] || refuse usage "--fault runs the nine steps only"
    case "$step" in
      '') ;;
      payment_frame_isolated | changed_page_1_isolated | block_popup | block_frame_post | block_websocket | cert_error) ;;
      *) refuse usage "--step with --rows isolation takes one of the six rows of A13.7" ;;
    esac
    ;;
  driver)
    [ "$mode" = cells ] || refuse usage "--rows driver runs cells only"
    [ -z "$fault" ] || refuse usage "--fault runs the nine steps only"
    case "$step" in
      '' | driver_killed_pipe | driver_killed_port | driver_killed_pipe_relay | driver_killed_port_relay) ;;
      relay_port_free) [ "$round" = 3 ] || refuse usage "relay_port_free belongs to --round 3" ;;
      *) refuse usage "--step with --rows driver takes one of the four forms of driver_killed, or with --round 3 relay_port_free" ;;
    esac
    ;;
  round3)
    [ "$round" = 3 ] || refuse usage "--rows round3 belongs to --round 3"
    case "$step" in
      '' | pipe_inherit | relay_foreign | start_probe | same_site_pref | same_site_rule) ;;
      *) refuse usage "--step with --rows round3 takes one of pipe_inherit, relay_foreign, start_probe, same_site_pref and same_site_rule" ;;
    esac
    ;;
  org)
    [ "$round" = 3 ] || refuse usage "--rows org belongs to --round 3"
    case "$step" in
      org_debugging | org_proxy) ;;
      *) refuse usage "--step with --rows org takes org_debugging or org_proxy" ;;
    esac
    ;;
  relay)
    case "$round" in 2 | 3) ;; *) refuse usage "--rows relay belongs to --round 2 or 3" ;; esac
    case "$step" in
      '' | list_refuse | list_hops | list_close | own_traffic) ;;
      *) refuse usage "--step with --rows relay takes one of list_refuse, list_hops, list_close and own_traffic" ;;
    esac
    ;;
  ways)
    case "$round" in 2 | 3) ;; *) refuse usage "--rows ways belongs to --round 2 or 3" ;; esac
    case "$step" in
      '' | ws_allowed | ws_allowed_control | frame_doc) ;;
      *) refuse usage "--step with --rows ways takes one of ws_allowed, ws_allowed_control and frame_doc" ;;
    esac
    ;;
  egress)
    case "$round" in 2 | 3) ;; *) refuse usage "--rows egress belongs to --round 2 or 3" ;; esac
    case "$step" in
      '' | no_worker_paths | udp_quic | webtransport | peer_connection | secure_dns | lan_discovery | link_lookup | after_unload | reports | handler_launch | updater_wake) ;;
      *) refuse usage "--step with --rows egress takes one of the rows of the ways out" ;;
    esac
    ;;
  reach)
    case "$round" in 2 | 3) ;; *) refuse usage "--rows reach belongs to --round 2 or 3" ;; esac
    case "$step" in
      '' | reach_page | reach_opened | reach_frame | reach_worker | reach_shared | reach_service) ;;
      *) refuse usage "--step with --rows reach takes one of the six contexts of CP-78" ;;
    esac
    ;;
  *) refuse usage "--rows takes isolation, driver, relay, ways, egress, reach, round3 or org" ;;
esac
case "$fault" in
  '') ;;
  signin_500 | history_changed | prefs_500 | frame_hangs | upload_input_gone | pay_now_renamed | \
    prefs_get_form | password_plain)
    [ "$mode" = cells ] || refuse usage "--fault runs cells only"
    ;;
  *) refuse usage "--fault takes one of the faults that --help names" ;;
esac
[ "$mode" = snapshot ] && [ -z "$out" ] && refuse usage "--snapshot-pay needs a file"
case "$round" in
  '' | 2 | 3) ;;
  *) refuse usage "--round takes 2 or 3" ;;
esac
case "$channel" in
  '') ;;
  pipe | port) [ "$round" = 3 ] || refuse usage "--channel belongs to --round 3" ;;
  *) refuse usage "--channel takes pipe or port" ;;
esac
system=$(uname -s)
if [ -n "${GITHUB_ACTIONS:-}" ] && [ "${RUNNER_OS:-}" = macOS ]; then
  machine=hosted-mac
elif [ -n "${GITHUB_ACTIONS:-}" ] && [ "${RUNNER_OS:-}" = Windows ]; then
  machine=hosted-win
elif [ "$system" = Darwin ]; then
  machine=mac
else
  refuse usage "this machine is none of mac, hosted-mac and hosted-win"
fi
[ -n "$browser" ] || refuse usage "--browser is required"
case "$(basename "$browser")" in
  'Google Chrome' | 'Google Chrome for Testing' | chrome.exe | chrome | 'Microsoft Edge' | msedge.exe | msedge) ;;
  *) refuse browser "the file name is no Chrome or Edge binary" ;;
esac
[ -f "$browser" ] || refuse browser "no file at the path"
command -v node >/dev/null 2>&1 || refuse usage "node is not on the PATH"
command -v cargo >/dev/null 2>&1 || refuse usage "cargo is not on the PATH"

dir=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$dir/../../.." && pwd)

set -- "$port"
[ "$mode" = extras ] && set -- "$@" "$((port + 1))"
[ -n "$fault" ] && set -- "$@" "$((port + 2))"
taken=$(node "$dir/port-free.mjs" "$@") || refuse port "127.0.0.1:$taken is taken"

if [ -z "$scratch" ]; then
  scratch=$(mktemp -d "${TMPDIR:-/tmp}/r3-web.XXXXXX") || {
    echo "notrun candidate=cdp machine=$machine cause=scratch"
    exit 1
  }
fi

exe=
case "$system" in
  MINGW* | MSYS* | CYGWIN*) exe=.exe ;;
esac
if command -v cygpath >/dev/null 2>&1; then
  repo_arg=$(cygpath -w "$repo")
  scratch_arg=$(cygpath -w "$scratch")
else
  repo_arg=$repo
  scratch_arg=$scratch
fi

host=$("${RUSTC:-rustc}" -vV | sed -n 's/^host: //p')
[ -n "$host" ] || {
  echo "notrun candidate=cdp machine=$machine cause=build"
  exit 1
}
cargo build --release --locked --quiet --target "$host" --target-dir "$dir/target" \
  --manifest-path "$dir/Cargo.toml" || {
  echo "notrun candidate=cdp machine=$machine cause=build"
  exit 1
}
bin="$dir/target/$host/release/web-trial$exe"

set -- --candidate cdp --browser "$browser" --port "$port" --machine "$machine" \
  --repo "$repo_arg" --scratch "$scratch_arg"
[ -n "$fault" ] && set -- "$@" --fault "$fault"
case "$mode" in
  cells)
    set -- "$@" --runs "$runs"
    [ -n "$step" ] && set -- "$@" --step "$step"
    [ "$rows" = isolation ] &&
      set -- "$@" --rows isolation --add-flag "--isolate-origins=http://pay.agent.test:$port"
    [ "$rows" = driver ] && set -- "$@" --rows driver
    [ -n "$relay" ] && set -- "$@" --relay
    [ -n "$routes" ] && set -- "$@" --routes
    [ "$rows" = relay ] && set -- "$@" --rows relay
    [ "$rows" = ways ] && set -- "$@" --rows ways
    [ "$rows" = egress ] && set -- "$@" --rows egress
    [ "$rows" = reach ] && set -- "$@" --rows reach
    [ "$rows" = round3 ] && set -- "$@" --rows round3
    [ "$rows" = org ] && set -- "$@" --rows org
    [ -n "$netlog" ] && set -- "$@" --net-log "$netlog"
    [ -n "$round" ] && set -- "$@" --round "$round"
    [ -n "$channel" ] && set -- "$@" --channel "$channel"
    ;;
  extras)
    if [ "$round" = 3 ]; then
      cargo build --release --locked --quiet --target "$host" --target-dir "$dir/target/pipe-only" \
        --no-default-features --features driver --manifest-path "$dir/Cargo.toml" || {
        echo "notrun candidate=cdp machine=$machine cause=build"
        exit 1
      }
      bin="$dir/target/pipe-only/$host/release/web-trial$exe"
      set -- "$@" --round 3 --relay
    fi
    with=$(wc -c <"$bin" | tr -d ' ')
    cargo build --release --locked --quiet --target "$host" --target-dir "$dir/target/no-driver" \
      --no-default-features --manifest-path "$dir/Cargo.toml" || {
      echo "notrun candidate=cdp machine=$machine cause=build"
      exit 1
    }
    without=$(wc -c <"$dir/target/no-driver/$host/release/web-trial$exe" | tr -d ' ')
    echo "binary with_driver_bytes=$with without_driver_bytes=$without"
    set -- "$@" --extras --added-bytes "$((with - without))"
    ;;
  resolver) set -- "$@" --resolver-probe ;;
  snapshot) set -- "$@" --snapshot-pay --out "$out" ;;
esac
exec "$bin" "$@"
