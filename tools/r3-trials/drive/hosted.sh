#!/bin/sh
set -u

usage() {
  cat <<'EOF'
usage: sh tools/r3-trials/drive/hosted.sh --tier low_cost_hosted|frontier --ceiling MINOR
           --browser PATH --port N --model TOKEN --list FILE --key-file FILE [--cell TASK:SEED]
       sh tools/r3-trials/drive/hosted.sh --standin --tier low_cost_hosted|frontier --ceiling MINOR
           --browser PATH --port N
       sh tools/r3-trials/drive/hosted.sh --marks --key-file FILE --ceiling MINOR
           [--passes N] [--browser PATH] [--port N]
       sh tools/r3-trials/drive/hosted.sh --marks --standin --ceiling MINOR [--browser PATH] [--port N]
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
takes a name behind that argument alone, and then lets the resolver daemon's socket and service
whole (name lookups, and also local-link service registrations, browses and gateway port
mappings) and tcp to port 443 through, unlogged, and denies and logs all else that it denies
without the argument (r3 U0.8c). The key is read ONCE from
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

--marks is the form with the runner's mask (unit U0.10); its rule is
docs/r3/trials/driver-hosted-marks.md. It runs the tier frontier and then the tier
low_cost_hosted in ONE call, under the ONE ceiling MINOR and one count, each tier N passes over
the nine cells (--passes, 1 to 9; without it 1). The snapshot of every request is made by the
runner's own maker of a snapshot, which the tool compiles from
crates/halvo-runner/src/model/snapshot.rs: a page's lines with the mark [slot NAME] in the place
of each text of a slot of the task, and a message with the From line From: [slot reply_senders].
A cell runs no recovery, and a body that holds the head of a mark is no body. One tier makes at
most 198 requests a pass: 192 in compiles and 6 for replies. The form takes no --tier, --model,
--list or --cell: the model token of a tier is the model word of its row of
crates/halvo-route/data/hosts.tsv with _ for each -, and the script writes the list of each
tier, the one entry host:443 of that row's origin, into the run's directory. Without
--browser it takes the Chrome of this system's usual place, and without --port 18471. It refuses
to start while a tracked file of the checkout is changed (REFUSED tree): its first line is

  commit head=<40 hex> mask=<sha256 of the mask's source file>

Behind each cell's ends line it prints

  marks model=<token> machine=<m> task=<t> seed=<n> i=<pass> requests=<n> with_mark=<n> slot_text=<n>

behind each tier's host line

  count tier=<tier> model=<token> drive=<k> of=<n> reply=<k> of=<n> typed_credential=<n>
        effects_without_confirm=<n> marked=<k> of=<n> slot_text=<n>

(one line), and at its end

  records asks=<n> requests_sha256=<hex> answers_sha256=<hex>
  verdict driver=<token|none> path=<A|B> writer=<token|none>

The compile and the write of the pass p are written with i=<p>, the replays with i from 3(p-1)+1
to 3(p-1)+3, and the drive and the reply line of the pass hold i=<p> behind seed. It keeps every
request line whole as it hands it to the door in door-requests.jsonl and every line that the door
gave back in door-answers.jsonl, one line an ask in each and in the order of the asks (an empty
line where the door gave none back), with door-asks.txt, which names the cell of each ask; the
records line gives the count of the asks and the SHA-256 of each of the two files. No byte of the
key stands in them. Where the round gave a verdict line it writes block.txt beside lines.txt: the
commit, attempt, drive, reply, marks, count, records and verdict lines alone, and no line of a
cell that ended notrun.

--marks --standin is the control of that form: two passes of both tiers against the scripted
host, which also answers the third ask for a reply body with a body that holds the head of a
mark; then the cell bill:1 once more under a ceiling of 1 minor unit, which must end notrun with
the cause ceiling and send nothing. It reads the lines against control/hosted-marks-expected.txt,
the requests that the scripted host received against the spend lines and against the kept records,
the record of the scripted host for a mark in each cell's requests and for no text of a slot of
the three seeds, every file of the run's directory for the made-up key, and the block for its
eight kinds of line, and prints "control readings ..." with each count and last
"control mode=hosted-marks machine=<m> result=<as_expected|differs>".

The four prompt files must have the SHA-256 of the prompt lines of docs/r3/trials/driver.md
(REFUSED prompts). The cells keep their directories under one directory under TMPDIR or
/tmp, with host-requests.txt (the bodies that the scripted host received) and count.txt (the
count's row); the script removes nothing.

Exit 0: every cell ran (and, --standin, the reading is as expected). 1: a cell printed
notrun or the reading differs. 2: a refusal, one line "REFUSED <cause>: <detail>" on
standard error; the causes are usage, prices, prompts, browser, port, list, row, machine and
tree.
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
marks=
passes=
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
    --marks)
      [ -z "$marks" ] || refuse usage "--marks is given twice"
      marks=yes
      shift
      ;;
    --tier | --ceiling | --browser | --port | --model | --list | --key-file | --cell | --passes)
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
        --passes) [ -z "$passes" ] || refuse usage "--passes is given twice"; passes=$2 ;;
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
if [ -n "$marks" ]; then
  [ -z "$tier$model$list$cell_given" ] ||
    refuse usage "--marks takes no --tier, --model, --list or --cell: it runs both tiers, and takes the token and the list of each from the table of hosts"
  if [ -n "$standin" ]; then
    [ -z "$passes" ] || refuse usage "--marks --standin takes no --passes: the control runs two passes"
    passes=2
  fi
  case ${passes:=1} in
    [123456789]) ;;
    *) refuse usage "--passes takes 1 to 9" ;;
  esac
  tiers='frontier low_cost_hosted'
else
  [ -z "$passes" ] || refuse usage "--passes is taken with --marks alone"
  case $tier in
    low_cost_hosted | frontier) ;;
    *) refuse usage "--tier takes low_cost_hosted or frontier" ;;
  esac
  tiers=$tier
fi
case $ceiling in
  '' | 0* | *[!0123456789]*) refuse usage "--ceiling takes 1 to 100000000 minor units" ;;
esac
[ "${#ceiling}" -le 9 ] && [ "$ceiling" -le 100000000 ] || refuse usage "--ceiling takes 1 to 100000000 minor units"

# The coordinator's log line 17658: a ceiling held at made-up rates is no ceiling, so the paid
# form reads the source of the tier's rate in the tracked price file of this checkout, which the
# door is built from, before anything else, and refuses while it is a fixture's.
if [ -z "$standin" ]; then
  for t in $tiers; do
    source=$(awk -F '\t' -v t="$t" '$1 == t { print $4 }' crates/halvo-route/data/prices.tsv) ||
      refuse prices "crates/halvo-route/data/prices.tsv cannot be read"
    case $source in
      '') refuse prices "crates/halvo-route/data/prices.tsv has no rate for the tier $t" ;;
      fixture:*) refuse prices "the rate of the tier $t in crates/halvo-route/data/prices.tsv has the source $source, a fixture's: the paid round starts only on the host's own rates with their date" ;;
    esac
  done
fi

system=$(uname -s) || refuse usage "uname -s failed"
if [ -n "$marks" ]; then
  [ -n "$port" ] || port=18471
  if [ -z "$browser" ]; then
    case $system in
      Darwin) browser='/Applications/Google Chrome.app/Contents/MacOS/Google Chrome' ;;
      MINGW* | MSYS* | CYGWIN*) browser='C:\Program Files\Google\Chrome\Application\chrome.exe' ;;
    esac
  fi
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

# The row of the table of hosts for the tier $1, as "origin model adapter", and its one check.
row_of() {
  fields=$(awk -F '\t' -v t="$1" '$1 == t { print $2, $3, $4; n++ } END { exit n != 1 }' crates/halvo-route/data/hosts.tsv) ||
    refuse row "crates/halvo-route/data/hosts.tsv holds no one row for the tier $1"
  origin=${fields%% *}
  rest=${fields#* }
  case $origin in
    https://*) ;;
    *) refuse row "the origin of the row is no https:// origin" ;;
  esac
}

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
  if [ -z "$marks" ]; then
    case $model in
      '' | none | synthetic | *[!abcdefghijklmnopqrstuvwxyz0123456789_]*) refuse usage "--model takes a model token" ;;
    esac
  fi
  [ -n "$key_file" ] && [ -f "$key_file" ] || refuse usage "--key-file names no file"
  if [ -z "$marks" ]; then
    [ -n "$list" ] && [ -f "$list" ] || refuse list "--list names no file"
    row_of "$tier"
    row="host tier=$tier origin=$origin model=${rest%% *} adapter=${rest#* }"
    entries=$(awk '/^\[/ || /^#/ || /^[[:space:]]*$/ { next } { print; n++ } END { exit n != 1 }' "$list") ||
      refuse list "$list holds no one entry"
    [ "$entries" = "${origin#https://}:443" ] || refuse list "the one entry of $list is not ${origin#https://}:443"
  fi
fi

# The form with the runner's mask (r3 U0.10): for each tier its model token, its origin and the model
# word of its row, joined by commas as r3-drive takes them; the lists are written behind the builds.
if [ -n "$marks" ]; then
  models=
  origins=
  words=
  for t in $tiers; do
    if [ -n "$standin" ]; then
      token=standin_$t
      origin=http://127.0.0.1:47201
      word=stand-in-model
    else
      row_of "$t"
      word=${rest%% *}
      [ "${rest#* }" = "$adapter" ] || refuse row "the adapter of the row of the tier $t is not $adapter"
      token=$(printf '%s' "$word" | tr '-' '_')
      case $token in
        '' | none | synthetic | *[!abcdefghijklmnopqrstuvwxyz0123456789_]*) refuse row "the model word of the row of the tier $t gives no model token" ;;
      esac
    fi
    models=${models:+$models,}$token
    origins=${origins:+$origins,}$origin
    words=${words:+$words,}$word
  done
  changed=$(git status --porcelain --untracked-files=no) || refuse tree "git status cannot read the checkout"
  [ -z "$changed" ] || refuse tree "a tracked file of the checkout is changed: a round runs at a commit, which its first line names"
  head=$(git rev-parse HEAD) || refuse tree "git rev-parse cannot read the head"
  mask=$(sha_of crates/halvo-runner/src/model/snapshot.rs) || refuse tree "the mask's source file cannot be read"
fi

command -v node >/dev/null 2>&1 || refuse usage "node is not on the PATH"
command -v cargo >/dev/null 2>&1 || refuse usage "cargo is not on the PATH"
taken=$(node tools/r3-trials/web/port-free.mjs "$port") || refuse port "127.0.0.1:$taken is taken"
if [ -n "$standin" ]; then
  taken=$(node tools/r3-trials/web/port-free.mjs 47201) || refuse port "127.0.0.1:$taken, the scripted host's port, is taken"
fi

notrun() {
  if [ -n "$marks" ]; then
    echo "notrun model=${models%%,*} machine=$machine task=bill seed=1 cause=$1"
    [ -z "$standin" ] || echo "control mode=hosted-marks machine=$machine result=differs"
    exit 1
  fi
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
if [ -n "$marks" ]; then
  # The list of each tier: the stand-in's is the tracked list of the loopback; the paid form's is the one
  # entry host:443 of the tier's origin, written here in the form of that tracked list.
  lists=
  for t in $tiers; do
    if [ -n "$standin" ]; then
      one=scripts/egress/lists/loopback.txt
    else
      row_of "$t"
      one=$dir/list-$t.txt
      printf '[fixed]\n%s:443\n[agent]\n' "${origin#https://}" >"$one" || notrun tmp
    fi
    lists=${lists:+$lists,}$one
  done
  set -- --round --form marks --passes "$passes" --repo "$root" --model "$models" \
    --settings docs/r3/model-settings.toml --browser "$browser" --port "$port" --machine "$machine" \
    --run-dir "$dir" --door "$door" --tier "$(printf '%s' "$tiers" | tr ' ' ',')" --ceiling "$ceiling" \
    --origin "$origins" --egress-list "$lists" --host-model "$words" --adapter "$adapter"
else
  set -- --round --repo "$root" --model "$model" --settings docs/r3/model-settings.toml \
    --browser "$browser" --port "$port" --machine "$machine" --run-dir "$dir" --door "$door" \
    --tier "$tier" --ceiling "$ceiling" --origin "$origin" --egress-list "$list"
fi
standin_key=u08-stand-in-key-made-up-not-a-host-key
if [ -n "$standin" ]; then
  keys=$(mktemp -d "${TMPDIR:-/tmp}/r3-hosted-key.XXXXXX") || notrun tmp
  printf '%s\n' "$standin_key" >"$keys/key.txt" || notrun tmp
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

if [ -z "$marks" ]; then
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
fi

# The form with the runner's mask (r3 U0.10). lines.txt holds every line: the commit line, the lines of
# r3-drive without its verdict line, the records line, and the verdict line last.
"$drive" "$@" >"$dir/drive-lines.txt" &
pid=$!
wait "$pid"
status=$?
pid=
: >>"$dir/door-requests.jsonl"
: >>"$dir/door-answers.jsonl"
requests=$(grep -c '' "$dir/door-requests.jsonl")
answers=$(grep -c '' "$dir/door-answers.jsonl")
requests_sha=$(sha_of "$dir/door-requests.jsonl") || notrun tmp
answers_sha=$(sha_of "$dir/door-answers.jsonl") || notrun tmp
{
  echo "commit head=$head mask=$mask"
  grep -v '^verdict ' "$dir/drive-lines.txt"
  echo "records asks=$requests requests_sha256=$requests_sha answers_sha256=$answers_sha"
  grep '^verdict ' "$dir/drive-lines.txt"
} >"$dir/lines.txt"
cat "$dir/lines.txt"
# THE BLOCK, written in this one place, where the round gave a verdict line: the eight kinds of line of
# lines.txt in its order, and no other line. A cell's attempt, drive and reply lines are held until its marks
# line, which ends the cell, and are left out with it where the cell wrote a notrun line: every cell of a
# block is whole.
block_kinds='commit attempt drive reply marks count records verdict'
if grep -q '^verdict ' "$dir/lines.txt"; then
  awk -v kinds="$block_kinds" '
    BEGIN { n = split(kinds, k, " "); for (i = 1; i <= n; i++) kind[k[i]] = 1 }
    $1 == "notrun" { stopped = 1; next }
    !($1 in kind) { next }
    $1 == "attempt" || $1 == "drive" || $1 == "reply" { held[++cell] = $0; next }
    $1 == "marks" {
      if (!stopped) { for (i = 1; i <= cell; i++) print held[i]; print }
      cell = 0; stopped = 0; next
    }
    { print }' "$dir/lines.txt" >"$dir/block.txt" || notrun tmp
fi
[ -n "$standin" ] || exit "$status"

# The control. The ceiling's stop, once: the cell bill:1 of the tier frontier again, under a ceiling of 1
# minor unit, with a record of the scripted host of its own.
mkdir "$dir/ceiling" || notrun tmp
"$drive" --round --form marks --repo "$root" --model "${models%%,*}" \
  --settings docs/r3/model-settings.toml --browser "$browser" --port "$port" --machine "$machine" \
  --run-dir "$dir/ceiling" --door "$door" --tier frontier --ceiling 1 --origin "${origins%%,*}" \
  --egress-list scripts/egress/lists/loopback.txt --host-model "${words%%,*}" --adapter "$adapter" \
  --key-file "$keys/key.txt" --standin-host 47201 --cell bill:1 >"$dir/ceiling-lines.txt" &
pid=$!
wait "$pid"
ceiling_status=$?
pid=
cat "$dir/ceiling-lines.txt"

# The five readings. (1) The lines against the expected ones, and (2) the requests that the scripted host
# received against the spend lines; the ceiling's cell must have sent none.
sent=$(grep -c '^--- request ' "$dir/host-requests.txt" 2>/dev/null)
past=$(grep -c '^--- request ' "$dir/ceiling/host-requests.txt" 2>/dev/null)
result=$(cat "$dir/lines.txt" "$dir/ceiling-lines.txt" | awk -v sent="${sent:-0}" -v past="${past:-0}" '
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
  /^(commit|attempt|drive|reply|ends|marks|notrun|spend|egress|host|count|records|verdict) / {
    got[++ng] = $0
    if ($1 == "spend") for (i = 2; i <= NF; i++) if ($i ~ /^requests=/) counted += substr($i, 10)
  }
  END {
    if (nw != ng || counted != sent || past != 0) bad++
    for (i = 1; i <= nw && i <= ng; i++) if (!same(want[i], got[i])) bad++
    print (bad ? "differs" : "as_expected")
  }' tools/r3-trials/drive/control/hosted-marks-expected.txt -)
# (3) The record of the scripted host: a mark in the requests of every cell, by the tool's own marks
# lines and by the three marks in the record itself, and no text of a slot of the three seeds; the word
# "Payment method" must be there, so that a record that was not read cannot pass.
unmarked=$(grep -c -E '^marks .* (with_mark=0|slot_text=[1-9])' "$dir/lines.txt" 2>/dev/null)
node -e '
  const fs = require("fs");
  for (const seed of [1, 2, 3]) {
    const data = JSON.parse(fs.readFileSync(`tools/fixture-server/variants/data/seed-${seed}.json`, "utf8"));
    for (const task of ["bill", "order", "reply"]) {
      for (const slot of data.bindings[task].slots) {
        for (const value of slot.value) console.log(slot.name === "start" ? `${value}:${process.argv[1]}` : value);
      }
    }
  }' "$port" >"$dir/slot-texts.txt" || notrun tmp
texts=$(grep -c '' "$dir/slot-texts.txt" 2>/dev/null)
held=$(grep -c -F -f "$dir/slot-texts.txt" "$dir/host-requests.txt" 2>/dev/null)
read_words=$(grep -c -F 'Payment method' "$dir/host-requests.txt" 2>/dev/null)
marked=0
for mark in '[slot instrument]' '[slot seller]' '[slot reply_senders]'; do
  grep -q -F "$mark" "$dir/host-requests.txt" 2>/dev/null && marked=$((marked + 1))
done
# (4) The made-up key in no file of the run's directory, with the count of the files that were read.
files=$(find "$dir/." -type f | grep -c '')
keyed=$(grep -r -l -F "$standin_key" "$dir/." | grep -c '')
# A count that could not be read reads as a fault: no number, no pass.
unmarked=${unmarked:-1}
texts=${texts:-0}
held=${held:-1}
read_words=${read_words:-0}
files=${files:-0}
keyed=${keyed:-1}
# (5) The block: its eight kinds of line and no other, the commit line first, the verdict line last, and no
# path in it.
block=differs
if [ -f "$dir/block.txt" ]; then
  block=$(awk -v kinds="$block_kinds" '
    BEGIN { n = split(kinds, k, " "); for (i = 1; i <= n; i++) kind[k[i]] = 1 }
    !($1 in kind) || /[\/\\]/ { bad++ }
    NR == 1 && $1 != "commit" { bad++ }
    { last = $1 }
    END { print (bad || last != "verdict" || NR == 0 ? "differs" : "as_expected") }' "$dir/block.txt")
fi
echo "control readings lines=$result sent=${sent:-0} past_the_ceiling=${past:-0} asks=$requests answers=$answers unmarked=$unmarked slot_texts=$texts held=$held read_words=$read_words marks=$marked files=$files keyed=$keyed block=$block"
if [ "$result" != as_expected ] || [ "$unmarked" -ne 0 ] || [ "$texts" -eq 0 ] || [ "$held" -ne 0 ] ||
  [ "$read_words" -eq 0 ] || [ "$marked" -ne 3 ] || [ "$files" -eq 0 ] || [ "$keyed" -ne 0 ] ||
  [ "$block" != as_expected ] || [ "$status" -ne 0 ] || [ "$ceiling_status" -ne 1 ] ||
  [ "$requests" != "${sent:-0}" ] || [ "$answers" != "$requests" ]; then
  result=differs
fi
echo "control mode=hosted-marks machine=$machine result=$result"
[ "$result" = as_expected ] && exit 0
exit 1
