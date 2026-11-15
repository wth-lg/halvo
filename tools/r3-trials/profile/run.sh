#!/bin/sh
set -u

usage() {
  cat <<'EOF'
Usage: sh tools/r3-trials/profile/run.sh --item 1|2|3|4 --browser PATH --port N [--driver PATH]
         [--leave-out NAME]... [--plant ENCODING:PLACE] [--runs N] [--shape S] [--flags SET]
         [--sandbox-off] [--scratch DIR]
       sh tools/r3-trials/profile/run.sh --help

The browser profile trial of unit U0.3 (contract A13.3). It builds the trial programs of
tools/r3-trials/profile with cargo --locked into their own target directory, starts the fixture
server of tools/fixture-server/variants on 127.0.0.1:N, and starts the browser headless as the
product starts it (A11.1, the flag set of U0.7c): on its debugging pipe, with the agent's profile
settings and the same-site setting written into the new profile before the start, the first flag
set of A13.2, the flags of U0.7b for the ways out, the resolver rule, and the relay of the trial
program as its only route, whose list holds the fixture's utility and payment hosts on port N. On
macOS the library adds the mock keychain, a flag of the trial alone.
sh tools/r3-trials/profile/target/<host>/release/profile-trial --print-set prints the flag and
pref lines of that set for the result file.

  --item 1        signs in on the utility fixture with the variant second_factor in profile A,
                  exports the cookies and the localStorage of the utility and payment origins,
                  stops the browser, which removes profile A, starts a new browser on an empty
                  profile B of another name, imports, opens /bill and reads the page it lands on.
                  Prints state, stop, profile, session, watch and keychain lines.
  --item 2        item 1, then searches every file under the parent directory of the profiles and
                  of their temporary directories, every entry that appeared in the system's
                  temporary directory during the run, and on macOS the files changed during the run
                  in the browser's per-user places outside the profile (Crashpad, the helper
                  caches), for the page marker of seed 1 in UTF-8, UTF-16LE and UTF-16BE, and for
                  the name of the cookie that carries the marker, because the browser stores that
                  cookie's value encrypted. marker_hits on the disk line sums both and adds one for
                  each profile or temporary directory still on disk. Prints searched, trace (the
                  utility host name over the parent directory and the new temporary entries) and
                  disk lines; when the search could not finish, a notrun line in place of disk.
  --item 3        with Chrome: runs, under sh scripts/egress/check.sh with the list
                  scripts/r3/egress/fixtures.txt (A14.3), the trial program, which starts the
                  fixture on 127.0.0.1:N and one browser that signs in with the second factor, opens
                  /bill and /pay and holds that page for 20 s. N must be 18090, the list's entry.
                  The browser starts as above without the relay, which is the executor's own and
                  outside the check's tree; the relay's flags that close a way out stay. Prints the
                  program's egress_run line, egress_set, and one line
                  egress machine=<m> mode=<mode> browser_started=<yes|no> denied=<n>, where denied
                  is the count of the check's denial log; on standard error the count of the log's
                  lines by their first word. No --no-sandbox is given: a browser that starts under
                  the check's mode only with its own sandbox off is browser_started=no.
  --item 4        two shapes under tree-peak: five runs of the nine steps of tools/r3-trials/web
                  (shape=steps), then five runs of one browser that signs in on the utility fixture
                  with the variant second_factor, opens /bill and /pay and holds that page for 20 s
                  (shape=task). Prints the lines of the programs, one browser line per run and
                  shape, and one bound line for this call's browser: 8 MiB times ceil(5 times the
                  larger of the two median peaks / 33554432).
  --browser PATH  the Chrome or Edge binary (the file name says which).
  --port N        the fixture port, 1 to 65532.
  --driver PATH   taken by the webdriver candidate only; refused here.
  --leave-out N   items 1 and 2: leave the cookie N out of the import; all leaves out every
                  cookie and every storage item. Repeatable. A check of the item, not a cell.
  --plant E:P     item 2: before the search, write the marker in encoding E (utf8, utf16le,
                  utf16be) to a file in place P: parent (the parent directory of the profiles)
                  or temp (a new directory in the temporary directory). A check, not a cell.
  --runs N        item 4: runs of each shape under tree-peak, default 5.
  --shape S       item 4: only the shape S, steps or task; default both. The bound line then
                  comes from that shape's median alone. The steps shape runs the nine steps of
                  tools/r3-trials/web with --round 3 --relay, the product's start of U0.7c.
  --flags SET     item 3: final (the default), the set above, or first, the first flag set of
                  A13.2 with the resolver rule and no setting written, where the item begins.
  --sandbox-off   item 3: a control, not a cell: adds --no-sandbox and prints an egress_control
                  line in place of the egress line.
  --scratch DIR   the parent directory of the profiles; default a new mktemp directory.

The keychain line says prompt=yes when the front application changed to a process of the
browser's tree or to a prompt process of the system, or when a new on-screen window of such a
process appeared; any other change of the front application is counted on standard error and not
in the line. The watch reads both every 250 ms from before the first start until 10 s after the
last stop. A read that fails, also of the front application's process, makes a notrun line in place
of the session line. On macOS the library adds the mock keychain, so the browser does not read the
system keychain, and the line can show only the other prompts of the system.
Exit 0 when every cell ran, 1 when a cell could not run (a notrun line ends with its cause),
2 for a refusal: one line REFUSED <usage|browser|port>: <detail> on standard error.
run.sh ends by exec of the trial program, so a TERM or INT sent to run.sh reaches the program,
which stops its browsers, removes their profiles and temporary directories (also of a browser that
was starting), stops the fixture server and exits 1 with a notrun line, also after the session.
In item 4 the program of tools/r3-trials/web removes the directories of its browsers the same way.
EOF
}

refuse() {
  echo "REFUSED $1: $2" >&2
  exit 2
}

item='' browser='' port='' runs=5 scratch='' plant='' shape='' flags='' sandbox_off=''
leave=''
while [ $# -gt 0 ]; do
  case "$1" in
    --help) usage; exit 0 ;;
    --sandbox-off) sandbox_off=yes; shift; continue ;;
  esac
  [ $# -ge 2 ] || refuse usage "$1 needs a value or is unknown; see --help"
  case "$1" in
    --item) item=$2 ;;
    --browser) browser=$2 ;;
    --port) port=$2 ;;
    --driver) refuse usage "--driver belongs to the webdriver candidate, which has no adapter" ;;
    --leave-out)
      case "$2" in
        '' | *[!A-Za-z0-9_]*) refuse usage "--leave-out takes a cookie name or all" ;;
      esac
      leave="$leave $2"
      ;;
    --plant)
      case "$2" in
        utf8:parent | utf8:temp | utf16le:parent | utf16le:temp | utf16be:parent | utf16be:temp) plant=$2 ;;
        *) refuse usage "--plant takes utf8|utf16le|utf16be:parent|temp" ;;
      esac
      ;;
    --runs) runs=$2 ;;
    --shape)
      case "$2" in
        steps | task) shape=$2 ;;
        *) refuse usage "--shape takes steps or task" ;;
      esac
      ;;
    --flags)
      case "$2" in
        first | final) flags=$2 ;;
        *) refuse usage "--flags takes first or final" ;;
      esac
      ;;
    --scratch) scratch=$2 ;;
    *) refuse usage "unknown argument $1; see --help" ;;
  esac
  shift 2
done

case "$item" in
  1 | 2 | 3 | 4) ;;
  *) refuse usage "--item takes 1, 2, 3 or 4" ;;
esac
case "$port" in
  '' | *[!0-9]*) refuse usage "--port takes an integer from 1 to 65532" ;;
esac
[ "${#port}" -le 5 ] && [ "$port" -ge 1 ] && [ "$port" -le 65532 ] ||
  refuse usage "--port takes an integer from 1 to 65532"
case "$runs" in
  '' | *[!0-9]* | 0*) refuse usage "--runs takes a count from 1 to 99" ;;
esac
[ "${#runs}" -le 2 ] || refuse usage "--runs takes a count from 1 to 99"
[ -n "$plant" ] && [ "$item" != 2 ] && refuse usage "--plant belongs to --item 2"
[ -n "$shape" ] && [ "$item" != 4 ] && refuse usage "--shape belongs to --item 4"
[ -n "$flags" ] && [ "$item" != 3 ] && refuse usage "--flags belongs to --item 3"
[ -n "$sandbox_off" ] && [ "$item" != 3 ] && refuse usage "--sandbox-off belongs to --item 3"
[ "$item" = 3 ] && [ "$port" != 18090 ] &&
  refuse usage "--item 3 takes --port 18090, the entry of scripts/r3/egress/fixtures.txt"
if [ -n "$leave" ]; then
  case "$item" in
    1 | 2) ;;
    *) refuse usage "--leave-out belongs to --item 1 and 2" ;;
  esac
fi
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
  'Google Chrome' | 'Google Chrome for Testing' | chrome.exe | chrome | 'Microsoft Edge' | msedge.exe | msedge) ;;
  *) refuse browser "the file name is no Chrome or Edge binary" ;;
esac
[ -f "$browser" ] || refuse browser "no file at the path"
case "$(basename "$browser")" in
  'Microsoft Edge' | msedge.exe | msedge) kind=edge ;;
  *) kind=chrome ;;
esac
command -v node >/dev/null 2>&1 || refuse usage "node is not on the PATH"
command -v cargo >/dev/null 2>&1 || refuse usage "cargo is not on the PATH"

dir=$(cd "$(dirname "$0")" && pwd)
web=$(cd "$dir/../web" && pwd)
repo=$(cd "$dir/../../.." && pwd)
taken=$(node "$web/port-free.mjs" "$port") || refuse port "127.0.0.1:$taken is taken"

if [ -z "$scratch" ]; then
  scratch=$(mktemp -d "${TMPDIR:-/tmp}/r3-profile.XXXXXX") || {
    echo "notrun item=$item machine=$machine browser=$kind cause=scratch"
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

host=$("${RUSTC:-rustc}" -vV | sed -n 's/^host: //p')
[ -n "$host" ] || {
  echo "notrun item=$item machine=$machine browser=$kind cause=build"
  exit 1
}
cargo build --release --locked --quiet --target "$host" --target-dir "$dir/target" \
  --manifest-path "$dir/Cargo.toml" || {
  echo "notrun item=$item machine=$machine browser=$kind cause=build"
  exit 1
}
bin="$dir/target/$host/release"

set -- --item "$item" --browser "$browser" --port "$port" --machine "$machine" \
  --repo "$repo_arg" --scratch "$scratch_arg"
for name in $leave; do
  set -- "$@" --leave-out "$name"
done
[ -n "$plant" ] && set -- "$@" --plant "$plant"
[ -n "$flags" ] && set -- "$@" --flags "$flags"
[ -n "$sandbox_off" ] && set -- "$@" --sandbox-off
if [ "$item" = 4 ]; then
  cargo build --release --locked --quiet --target "$host" --target-dir "$web/target" \
    --manifest-path "$web/Cargo.toml" || {
    echo "notrun item=4 machine=$machine browser=$kind cause=build"
    exit 1
  }
  set -- "$@" --runs "$runs" --tree-peak "$bin/tree-peak$exe" \
    --web-trial "$web/target/$host/release/web-trial$exe"
  [ -n "$shape" ] && set -- "$@" --shape "$shape"
fi
exec "$bin/profile-trial$exe" "$@"
