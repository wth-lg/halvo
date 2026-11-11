#!/bin/sh
set -u

usage() {
  cat <<'EOF'
Usage: sh tools/r3-trials/web/run.sh --candidate cdp --browser PATH --port N --runs N [--step NAME]
       sh tools/r3-trials/web/run.sh --candidate cdp --browser PATH --port N --extras
       sh tools/r3-trials/web/run.sh --candidate cdp --browser PATH --port N --resolver-probe
       sh tools/r3-trials/web/run.sh --candidate cdp --browser PATH --port N --snapshot-pay FILE
       sh tools/r3-trials/web/run.sh --help

The web executor trial of unit U0.2 (contract A13.2). It builds the trial program with
cargo --locked, starts the fixture server of tools/fixture-server/variants on 127.0.0.1:N,
starts the browser headless with a new profile for every run, and stops both at the end.

  --candidate C     cdp, the adapter on chromiumoxide 0.9.1. webdriver and playwright have
                    no adapter: the trial builds one only after the candidate before it missed.
  --browser PATH    the Chrome or Edge binary (the file name says which).
  --port N          the fixture port. --extras also uses N+1 for the debugging port;
                    --fault also uses N+2 for the fixture behind the fault proxy.
  --runs N          runs per step; a cell of A13.2 is 10.
  --step NAME       one of the nine steps; without it, all nine in the order of A13.2.
  --driver PATH     taken by the webdriver candidate only; refused here.
  --extras          prints the extras line: added_bytes (release binary with the driver crate
                    against one without it, both stripped), cold_ms (median of 10 starts to the
                    load of /signin), record, attach and pipe.
  --resolver-probe  starts one browser, opens a name outside *.test and one inside, and prints
                    which error the first got and how many requests each sent to the fixture.
  --snapshot-pay F  writes the snapshot of /pay of seed 1 with its frame to F.
  --fault NAME      runs the steps behind tools/r3-trials/web/faults.mjs, which breaks one route
                    or one page (signin_500, history_changed, prefs_500, frame_hangs,
                    upload_input_gone, pay_now_renamed, prefs_get_form, password_plain).
  --scratch DIR     the directory for profiles and upload files; default a new mktemp directory.

Output: one run line per run, one cell line per step, and no verdict line. A run line holds the
browser product and version that the browser reported over the protocol. median_ms is the median
of the runs that printed ok. A run whose fixture control failed, and a cell with no browser
version, make a notrun line in place of the cell line.
Exit 0 when every cell ran (a miss is a result), 1 when a cell could not run (a notrun line
names it), 2 for a refusal: one line REFUSED <usage|browser|port>: <detail> on standard error.
EOF
}

refuse() {
  echo "REFUSED $1: $2" >&2
  exit 2
}

candidate= browser= port= runs= step= mode=cells out= fault= scratch=
while [ $# -gt 0 ]; do
  case "$1" in
    --help) usage; exit 0 ;;
    --extras) mode=extras; shift; continue ;;
    --resolver-probe) mode=resolver; shift; continue ;;
  esac
  [ $# -ge 2 ] || refuse usage "$1 needs a value or is unknown; see --help"
  case "$1" in
    --candidate) candidate=$2 ;;
    --browser) browser=$2 ;;
    --port) port=$2 ;;
    --runs) runs=$2 ;;
    --step) step=$2 ;;
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
    '' | *[!0-9]* | 0*) refuse usage "--runs takes a count from 1" ;;
  esac
fi
case "$step" in
  '' | signin_form | table_read | form_fill | payment_frame | file_upload | session_expiry | \
    changed_page_1 | block_request | credential_mark) ;;
  *) refuse usage "--step takes one of the nine steps of A13.2" ;;
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
if [ -n "${GITHUB_ACTIONS:-}" ] && [ "${RUNNER_OS:-}" = macOS ]; then
  machine=hosted-mac
elif [ -n "${GITHUB_ACTIONS:-}" ] && [ "${RUNNER_OS:-}" = Windows ]; then
  machine=hosted-win
elif [ "$(uname -s)" = Darwin ]; then
  machine=mac
else
  refuse usage "this machine is none of mac, hosted-mac and hosted-win"
fi
[ -n "$browser" ] || refuse usage "--browser is required"
case "$(basename "$browser")" in
  'Google Chrome' | chrome.exe | chrome | 'Microsoft Edge' | msedge.exe | msedge) ;;
  *) refuse browser "the file name is no Chrome or Edge binary" ;;
esac
[ -f "$browser" ] || refuse browser "no file at the path"
command -v node >/dev/null 2>&1 || refuse usage "node is not on the PATH"
command -v cargo >/dev/null 2>&1 || refuse usage "cargo is not on the PATH"

dir=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$dir/../../.." && pwd)

ports=$port
[ "$mode" = extras ] && ports="$port $((port + 1))"
[ -n "$fault" ] && ports="$port $((port + 2))"
# shellcheck disable=SC2086
taken=$(node "$dir/port-free.mjs" $ports) || refuse port "127.0.0.1:$taken is taken"

if [ -z "$scratch" ]; then
  scratch=$(mktemp -d "${TMPDIR:-/tmp}/r3-web.XXXXXX") || {
    echo "notrun candidate=cdp machine=$machine cause=scratch"
    exit 1
  }
fi

exe=
case "$(uname -s)" in
  MINGW* | MSYS* | CYGWIN*) exe=.exe ;;
esac
if command -v cygpath >/dev/null 2>&1; then
  repo_arg=$(cygpath -w "$repo")
  scratch_arg=$(cygpath -w "$scratch")
else
  repo_arg=$repo
  scratch_arg=$scratch
fi

cargo build --release --locked --quiet --manifest-path "$dir/Cargo.toml" || {
  echo "notrun candidate=cdp machine=$machine cause=build"
  exit 1
}
bin="$dir/target/release/web-trial$exe"

set -- --candidate cdp --browser "$browser" --port "$port" --machine "$machine" \
  --repo "$repo_arg" --scratch "$scratch_arg"
[ -n "$fault" ] && set -- "$@" --fault "$fault"
case "$mode" in
  cells)
    set -- "$@" --runs "$runs"
    [ -n "$step" ] && set -- "$@" --step "$step"
    ;;
  extras)
    with=$(wc -c <"$bin" | tr -d ' ')
    CARGO_TARGET_DIR="$dir/target/no-driver" cargo build --release --locked --quiet \
      --no-default-features --manifest-path "$dir/Cargo.toml" || {
      echo "notrun candidate=cdp machine=$machine cause=build"
      exit 1
    }
    without=$(wc -c <"$dir/target/no-driver/release/web-trial$exe" | tr -d ' ')
    echo "binary with_driver_bytes=$with without_driver_bytes=$without"
    set -- "$@" --extras --added-bytes "$((with - without))"
    ;;
  resolver) set -- "$@" --resolver-probe ;;
  snapshot) set -- "$@" --snapshot-pay --out "$out" ;;
esac
"$bin" "$@"
