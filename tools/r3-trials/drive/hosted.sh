#!/bin/sh
set -u

usage() {
  cat <<'EOF'
usage: sh tools/r3-trials/drive/hosted.sh --tier low_cost_hosted|frontier --ceiling MINOR
           --browser PATH --port N --model TOKEN --list FILE --key-file FILE [--cell TASK:SEED]
       sh tools/r3-trials/drive/hosted.sh --standin --tier low_cost_hosted|frontier --ceiling MINOR
           --browser PATH --port N
       sh tools/r3-trials/drive/hosted.sh --help

Runs every cell of the driver trial (contracts.md A13.6) with the door halvo-model of
crates/halvo-model in the place of the model child (unit U0.8): bill and order, seeds 1 to 3
(a compile of at most 32 asks, three replays with no model, one recovery on changed_page_2),
then reply, seeds 1 to 3 (two bodies), in one process, tools/r3-trials/drive's r3-drive
--round. Its rule is docs/r3/trials/driver-hosted.md.

The paid form is ONE round on one Mac. It refuses to start, before it reads the key or starts
a browser, a door or a count, while the rate of the tier in crates/halvo-route/data/prices.tsv
has a source that begins fixture: (REFUSED prices). It builds the door in release without the
feature loopback, and starts it for each run that asks the model under sh
scripts/egress/check.sh --one-named-host with the list FILE, which must hold ONE entry, the
host:443 of the origin of the row of the tier in crates/halvo-route/data/hosts.tsv; the check
takes a name behind that argument alone, and then lets name lookups and tcp to port 443 through
and denies and logs all else (r3 U0.8c). The key is read ONCE from
the key file, by r3-drive, and goes to the door as its first line behind its length line;
no line and no file of a cell holds it.

--cell TASK:SEED (bill:1 to reply:3) runs that one cell alone, under the same count of the
ceiling and the same bound of requests, and prints its lines, its ends line and the three lines
below; no line of such a call enters a verdict, which stands on the nine cells of ONE round. It
is refused beside --standin, because the control reads all nine cells.

--standin runs the same cells against the scripted host of r3-drive on 127.0.0.1:47201, the
one entry of scripts/egress/lists/loopback.txt, with the door built with its feature loopback
and a made-up key that names itself as no key of a host; it reaches no host. The scripted host
walks each task as a right driver would, and ends three answers without one: the round's first
action ask at the output limit, its second as declined, and the first ask for a reply body at
the output limit. It is the control of the paid form: it reads the lines against
control/hosted-expected.txt, checks that the scripted host received as many requests as the
spend line counts, and prints last "control mode=hosted machine=<m> result=<as_expected|differs>".

Before each request the round reserves a hold on a count of crates/halvo-route (count.rs)
whose cap is MINOR, the ceiling in minor units, at the prices of the build; after the answer
it settles at the token counts that the door's reply line reports. A cell whose next hold
does not fit prints "notrun ... cause=ceiling" and sends nothing, and the round stops; so does
a cell whose next request would pass the round's bound of 216 requests, with the cause
requests. The round's lines are those of run.sh, each cell's followed by

  ends model=<token> machine=<m> task=<t> seed=<n> cut=<n> declined=<n>

the requests of the cell that the door refused as cut (the host's answer ended at the output
limit) or declined (the host declined it): no ask that the model failed, each settled at its
whole hold. Then

  spend tier=<tier> requests=<n> spent_micro=<n> ceiling_minor=<n>
  egress list=<file> origin=<origin> starts=<n> runs=<n> denied=<n>
  host tier=<tier> origin=<origin> model=<word> adapter=<word>

requests= counts the requests handed to the door, each behind a hold; starts= the door's starts
and runs= the egress check's lines that the round read: where the two differ, the round exits 1
and gives no verdict.

The four prompt files must have the SHA-256 of the prompt lines of docs/r3/trials/driver.md
(REFUSED prompts). The cells keep their directories under one directory under TMPDIR or
/tmp, with host-requests.txt (the bodies that the scripted host received) and count.txt (the
count's row); the script removes nothing.

Exit 0: every cell ran (and, --standin, the reading is as expected). 1: a cell printed
notrun or the reading differs. 2: a refusal, one line "REFUSED <cause>: <detail>" on
standard error; the causes are usage, prices, prompts, browser, port, list, row and machine.
EOF
}

refuse() {
  printf 'REFUSED %s: %s\n' "$1" "$2" >&2
  exit 2
}

here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P) || refuse usage "the directory of hosted.sh cannot be read"
root=$(CDPATH='' cd -- "$here/../../.." && pwd -P) || refuse usage "the root of the repository cannot be read"
cd "$root" || refuse usage "the root of the repository cannot be entered"

standin=
tier=
ceiling=
browser=
port=
model=
list=
key_file=
cell=
# Whether --cell was given is a fact of its own, set when the argument is seen: an empty value is a
# --cell all the same, which the check below refuses, and never a call of every cell.
cell_given=
while [ $# -gt 0 ]; do
  case $1 in
    --help)
      usage
      exit 0
      ;;
    --standin)
      [ -z "$standin" ] || refuse usage "--standin is given twice"
      standin=yes
      shift
      ;;
    --tier | --ceiling | --browser | --port | --model | --list | --key-file | --cell)
      [ $# -ge 2 ] || refuse usage "$1 has no value"
      case $1 in
        --cell) [ -z "$cell_given" ] || refuse usage "--cell is given twice"; cell_given=yes; cell=$2 ;;
        --tier) [ -z "$tier" ] || refuse usage "--tier is given twice"; tier=$2 ;;
        --ceiling) [ -z "$ceiling" ] || refuse usage "--ceiling is given twice"; ceiling=$2 ;;
        --browser) [ -z "$browser" ] || refuse usage "--browser is given twice"; browser=$2 ;;
        --port) [ -z "$port" ] || refuse usage "--port is given twice"; port=$2 ;;
        --model) [ -z "$model" ] || refuse usage "--model is given twice"; model=$2 ;;
        --list) [ -z "$list" ] || refuse usage "--list is given twice"; list=$2 ;;
        --key-file) [ -z "$key_file" ] || refuse usage "--key-file is given twice"; key_file=$2 ;;
      esac
      shift 2
      ;;
    *) refuse usage "unknown argument $1" ;;
  esac
done
if [ -n "$cell_given" ]; then
  case $cell in
    bill:[123] | order:[123] | reply:[123]) ;;
    *) refuse usage "--cell takes bill:1 to reply:3" ;;
  esac
  [ -z "$standin" ] || refuse usage "--cell is not taken with --standin: the control reads all nine cells"
fi
case $tier in
  low_cost_hosted | frontier) ;;
  *) refuse usage "--tier takes low_cost_hosted or frontier" ;;
esac
case $ceiling in
  '' | 0* | *[!0123456789]*) refuse usage "--ceiling takes 1 to 100000000 minor units" ;;
esac
[ "${#ceiling}" -le 9 ] && [ "$ceiling" -le 100000000 ] || refuse usage "--ceiling takes 1 to 100000000 minor units"

# The coordinator's log line 17658: a ceiling held at made-up rates is no ceiling, so the paid
# form reads the source of the tier's rate in the tracked price file of this checkout, which the
# door is built from, before anything else, and refuses while it is a fixture's.
if [ -z "$standin" ]; then
  source=$(awk -F '\t' -v t="$tier" '$1 == t { print $4 }' crates/halvo-route/data/prices.tsv) ||
    refuse prices "crates/halvo-route/data/prices.tsv cannot be read"
  case $source in
    '') refuse prices "crates/halvo-route/data/prices.tsv has no rate for the tier $tier" ;;
    fixture:*) refuse prices "the rate of the tier $tier in crates/halvo-route/data/prices.tsv has the source $source, a fixture's: the paid round starts only on the host's own rates with their date" ;;
  esac
fi

case $port in
  '' | 0* | *[!0123456789]*) refuse port "--port takes a number from 1024 to 65535" ;;
esac
[ "${#port}" -le 5 ] && [ "$port" -ge 1024 ] && [ "$port" -le 65535 ] || refuse port "--port takes a number from 1024 to 65535"
[ "$port" != 47201 ] || refuse port "47201 is the scripted host's port"
[ -n "$browser" ] || refuse usage "--browser is missing"
browser_name=${browser##*/}
browser_name=${browser_name##*\\}
case $browser_name in
  'Google Chrome' | 'Google Chrome for Testing' | chrome.exe | chrome | 'Microsoft Edge' | msedge.exe | msedge) ;;
  *) refuse browser "the file name is no Chrome or Edge binary" ;;
esac
[ -f "$browser" ] || refuse browser "no file at the path"

sha_of() {
  if command -v shasum >/dev/null 2>&1; then
    sum=$(shasum -a 256 "$1") || return 1
  else
    sum=$(sha256sum "$1") || return 1
  fi
  printf '%s\n' "${sum%% *}"
}
for name in boundary.txt goal-bill.txt goal-order.txt goal-reply.txt; do
  hex=$(sha_of "tools/r3-trials/drive/prompts/$name") || refuse prompts "prompts/$name cannot be read"
  grep -qx "prompt file=$name sha256=$hex" docs/r3/trials/driver.md ||
    refuse prompts "prompts/$name has no prompt line of docs/r3/trials/driver.md with its SHA-256 $hex"
done

system=$(uname -s) || refuse usage "uname -s failed"
case $system in
  Darwin)
    exe=
    if [ "${GITHUB_ACTIONS:-}" = true ]; then machine=hosted-mac; else machine=mac; fi
    ;;
  MINGW* | MSYS* | CYGWIN*)
    exe=.exe
    if [ "${GITHUB_ACTIONS:-}" = true ]; then machine=hosted-win; else machine=windows; fi
    ;;
  *)
    exe=
    machine=other
    ;;
esac

adapter=anthropic-messages-2026-10-21
if [ -n "$standin" ]; then
  [ -z "$model$list$key_file" ] || refuse usage "--standin takes no --model, --list or --key-file"
  case $machine in
    mac | hosted-mac | hosted-win) ;;
    *) refuse machine "the stand-in form runs on mac, hosted-mac or hosted-win, not $machine" ;;
  esac
  model=standin_host
  list=scripts/egress/lists/loopback.txt
  origin=http://127.0.0.1:47201
  row="host tier=$tier origin=$origin model=stand-in-model adapter=$adapter"
else
  [ "$machine" = mac ] || refuse machine "the paid round is ONE round on one Mac, not on $machine"
  case $model in
    '' | none | synthetic | *[!abcdefghijklmnopqrstuvwxyz0123456789_]*) refuse usage "--model takes a model token" ;;
  esac
  [ -n "$key_file" ] && [ -f "$key_file" ] || refuse usage "--key-file names no file"
  [ -n "$list" ] && [ -f "$list" ] || refuse list "--list names no file"
  fields=$(awk -F '\t' -v t="$tier" '$1 == t { print $2, $3, $4; n++ } END { exit n != 1 }' crates/halvo-route/data/hosts.tsv) ||
    refuse row "crates/halvo-route/data/hosts.tsv holds no one row for the tier $tier"
  origin=${fields%% *}
  rest=${fields#* }
  case $origin in
    https://*) ;;
    *) refuse row "the origin of the row is no https:// origin" ;;
  esac
  row="host tier=$tier origin=$origin model=${rest%% *} adapter=${rest#* }"
  entries=$(awk '/^\[/ || /^#/ || /^[[:space:]]*$/ { next } { print; n++ } END { exit n != 1 }' "$list") ||
    refuse list "$list holds no one entry"
  [ "$entries" = "${origin#https://}:443" ] || refuse list "the one entry of $list is not ${origin#https://}:443"
fi

command -v node >/dev/null 2>&1 || refuse usage "node is not on the PATH"
command -v cargo >/dev/null 2>&1 || refuse usage "cargo is not on the PATH"
taken=$(node tools/r3-trials/web/port-free.mjs "$port") || refuse port "127.0.0.1:$taken is taken"
if [ -n "$standin" ]; then
  taken=$(node tools/r3-trials/web/port-free.mjs 47201) || refuse port "127.0.0.1:$taken, the scripted host's port, is taken"
fi

notrun() {
  echo "notrun model=$model machine=$machine task=bill seed=1 cause=$1"
  [ -z "$standin" ] || echo "control mode=hosted machine=$machine result=differs"
  exit 1
}
drive=tools/r3-trials/drive/target/release/r3-drive$exe
(cd tools/r3-trials/drive && cargo build --release --locked) >&2 || notrun build
if [ -n "$standin" ]; then
  cargo build --locked -p halvo-model --features loopback >&2 || notrun build
  door=$root/target/debug/halvo-model$exe
else
  cargo build --release --locked -p halvo-model >&2 || notrun build
  door=$root/target/release/halvo-model$exe
fi
[ -x "$drive" ] && [ -x "$door" ] || notrun build

dir=$(mktemp -d "${TMPDIR:-/tmp}/r3-hosted-round.XXXXXX") || notrun tmp
set -- --round --repo "$root" --model "$model" --settings docs/r3/model-settings.toml \
  --browser "$browser" --port "$port" --machine "$machine" --run-dir "$dir" --door "$door" \
  --tier "$tier" --ceiling "$ceiling" --origin "$origin" --egress-list "$list"
if [ -n "$standin" ]; then
  keys=$(mktemp -d "${TMPDIR:-/tmp}/r3-hosted-key.XXXXXX") || notrun tmp
  printf '%s\n' u08-stand-in-key-made-up-not-a-host-key >"$keys/key.txt" || notrun tmp
  set -- "$@" --key-file "$keys/key.txt" --standin-host 47201
else
  set -- "$@" --key-file "$key_file"
fi
[ -z "$cell_given" ] || set -- "$@" --cell "$cell"

pid=
# shellcheck disable=SC2329
stop() {
  if [ -n "$pid" ]; then
    kill -TERM "$pid" 2>/dev/null
    wait "$pid"
  fi
  notrun signal
}
trap stop INT TERM

"$drive" "$@" >"$dir/lines.txt" &
pid=$!
wait "$pid"
status=$?
pid=
cat "$dir/lines.txt"
echo "$row"
[ -n "$standin" ] || exit "$status"

echo "$row" >>"$dir/lines.txt"
sent=$(grep -c '^--- request ' "$dir/host-requests.txt" 2>/dev/null)
result=$(awk -v sent="${sent:-0}" '
  function same(want, got,   w, g, n, m, i, a, b) {
    n = split(want, w, " ")
    m = split(got, g, " ")
    if (n != m) return 0
    for (i = 1; i <= n; i++) {
      if (w[i] == g[i]) continue
      split(w[i], a, "=")
      split(g[i], b, "=")
      if (a[2] == "*" && a[1] == b[1]) continue
      return 0
    }
    return 1
  }
  FILENAME == ARGV[1] {
    if ($1 == "line") { sub(/^line /, ""); want[++nw] = $0 }
    next
  }
  /^(attempt|drive|reply|ends|notrun|spend|egress|host) / {
    got[++ng] = $0
    if ($1 == "spend") for (i = 2; i <= NF; i++) if ($i ~ /^requests=/) counted = substr($i, 10)
  }
  END {
    if (nw != ng || counted != sent) bad++
    for (i = 1; i <= nw && i <= ng; i++) if (!same(want[i], got[i])) bad++
    print (bad ? "differs" : "as_expected")
  }' tools/r3-trials/drive/control/hosted-expected.txt "$dir/lines.txt")
echo "control mode=hosted machine=$machine result=$result"
[ "$status" -eq 0 ] && [ "$result" = as_expected ] && exit 0
exit 1
