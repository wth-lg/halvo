#!/bin/sh
set -u

usage() {
  cat <<'EOF'
usage: sh tools/r3-trials/drive/control.sh --mode MODE --task bill|order --browser PATH --port N
       sh tools/r3-trials/drive/control.sh --help

The control of the driver trial (ruling CP-173 E1): one cell of seed 1 in which the
stand-in child r3-standin of this crate takes the place of the model. It loads nothing
and takes no model lock; its mode is the one line of control/<mode>.txt: right,
effect_early, option_then_right, or, on bill alone, sign_in_click, option_line,
extract_then_click, option_twice_then_right, extract_to_cap and effect_twice. For
effect_twice the compile run has the fixture's variant slow_charge, whose answer to the
effect button's post comes after the harness's settle (ruling CP-179, its Addition on
H9 (c)).

It refuses a browser and a port as run.sh does, builds the crate with cargo build
--release --locked, and calls r3-drive with the argument list of run.sh, in which three
values alone differ: --model is standin_<mode>, which is no line of weights.txt,
--weights is control/<mode>.txt, and --child is the stand-in. It prints r3-drive's lines
as they come, then one line "trace <words>" for each line of the cell's trace that holds
the harness's own words (those that begin compile, run, replay, recover, stuck, handback
or "effect allowed", the last with its origin cut off), and last one line

  control mode=<m> machine=<x> task=<t> result=<as_expected|differs|reading>

from a field-by-field reading of the attempt and drive lines against control/expected.txt,
where * is an open field, and of the texts that the trace or the request log of the cell
must hold or must not hold. A mode with no expected lines gives result=reading. The cell's
directory under TMPDIR or /tmp keeps the trace, the playbook and the request log, which
no line prints.

Exit 0: the cell ran and the reading is as expected or a reading; 1: it differs, or a
cell could not run; 2: a refusal, one line "REFUSED <cause>: <detail>" on standard error,
the causes usage, browser and port.
EOF
}

refuse() {
  printf 'REFUSED %s: %s\n' "$1" "$2" >&2
  exit 2
}

here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P) || refuse usage "the directory of control.sh cannot be read"
root=$(CDPATH='' cd -- "$here/../../.." && pwd -P) || refuse usage "the root of the repository cannot be read"
cd "$root" || refuse usage "the root of the repository cannot be entered"

mode=
task=
browser=
port=
while [ $# -gt 0 ]; do
  case $1 in
    --help)
      usage
      exit 0
      ;;
    --mode | --task | --browser | --port)
      [ $# -ge 2 ] || refuse usage "$1 has no value"
      case $1 in
        --mode) [ -z "$mode" ] || refuse usage "--mode is given twice"; mode=$2 ;;
        --task) [ -z "$task" ] || refuse usage "--task is given twice"; task=$2 ;;
        --browser) [ -z "$browser" ] || refuse usage "--browser is given twice"; browser=$2 ;;
        --port) [ -z "$port" ] || refuse usage "--port is given twice"; port=$2 ;;
      esac
      shift 2
      ;;
    *) refuse usage "unknown argument $1" ;;
  esac
done
case $mode in
  right | effect_early | option_then_right) ;;
  sign_in_click | option_line | extract_then_click | option_twice_then_right | extract_to_cap | effect_twice)
    [ "$task" = bill ] || refuse usage "--mode $mode runs on bill alone"
    ;;
  *) refuse usage "--mode takes right, effect_early, option_then_right, sign_in_click, option_line, extract_then_click, option_twice_then_right, extract_to_cap or effect_twice" ;;
esac
set --
[ "$mode" = effect_twice ] && set -- --compile-variant slow_charge
case $task in
  bill | order) ;;
  *) refuse usage "--task takes bill or order" ;;
esac
case $port in
  '' | 0* | *[!0123456789]*) refuse port "--port takes a number from 1024 to 65535" ;;
esac
[ "$port" -ge 1024 ] && [ "$port" -le 65535 ] || refuse port "--port takes a number from 1024 to 65535"
[ -n "$browser" ] || refuse usage "--browser is missing"
browser_name=$(basename -- "$browser")
case $browser_name in
  'Google Chrome' | 'Google Chrome for Testing' | chrome.exe | chrome | 'Microsoft Edge' | msedge.exe | msedge) ;;
  *) refuse browser "the file name is no Chrome or Edge binary" ;;
esac
[ -f "$browser" ] || refuse browser "no file at the path"
command -v node >/dev/null 2>&1 || refuse usage "node is not on the PATH"
command -v cargo >/dev/null 2>&1 || refuse usage "cargo is not on the PATH"
taken=$(node tools/r3-trials/web/port-free.mjs "$port") || refuse port "127.0.0.1:$taken is taken"

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
model=standin_$mode
fail() {
  echo "notrun model=$model machine=$machine task=$task seed=1 cause=$1"
  echo "control mode=$mode machine=$machine task=$task result=differs"
  exit 1
}

(cd tools/r3-trials/drive && cargo build --release --locked) >&2 || fail build
drive=tools/r3-trials/drive/target/release/r3-drive$exe
child=tools/r3-trials/drive/target/release/r3-standin$exe
[ -x "$drive" ] && [ -x "$child" ] || fail build
dir=$(mktemp -d "${TMPDIR:-/tmp}/r3-drive-control.XXXXXX") || fail tmp

pid=
# shellcheck disable=SC2329
stop() {
  if [ -n "$pid" ]; then
    kill -TERM "$pid" 2>/dev/null
    wait "$pid"
  fi
  fail signal
}
trap stop INT TERM

"$drive" --repo "$root" --model "$model" --weights "tools/r3-trials/drive/control/$mode.txt" --child "$child" \
  --settings docs/r3/model-settings.toml --task "$task" --seed 1 \
  --browser "$browser" --port "$port" --machine "$machine" --run-dir "$dir" "$@" >"$dir/lines.txt" &
pid=$!
wait "$pid"
status=$?
pid=
cat "$dir/lines.txt"
[ "$status" -eq 0 ] || fail "drive_exit_$status"

awk '{ sub(/^[0-9.]+ /, "") }
  /^effect allowed/ { print "trace effect allowed"; next }
  /^(compile|run|replay|recover|stuck|handback)/ { print "trace " $0 }' "$dir/trace.txt"

result=$(awk -v mode="$mode" -v task="$task" -v dir="$dir" '
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
  function holds(file, text,   line, found) {
    found = 0
    while ((getline line < file) > 0) if (index(line, text)) found = 1
    close(file)
    return found
  }
  FILENAME == ARGV[1] {
    if ($1 != mode || $2 != task) next
    kind = $3
    rest = $0
    sub(/^[^ ]+ [^ ]+ [^ ]+ /, "", rest)
    wanted++
    if (kind == "line") want[++nw] = rest
    else if (kind == "tracehas" && !holds(dir "/trace.txt", rest)) bad++
    else if (kind == "tracelacks" && holds(dir "/trace.txt", rest)) bad++
    else if (kind == "requesthas" && !holds(dir "/requests.txt", rest)) bad++
    else if (kind == "requestlacks" && holds(dir "/requests.txt", rest)) bad++
    next
  }
  /^(attempt|drive) / { got[++ng] = $0 }
  END {
    if (!wanted) { print "reading"; exit }
    if (nw != ng) bad++
    for (i = 1; i <= nw && i <= ng; i++) if (!same(want[i], got[i])) bad++
    print (bad ? "differs" : "as_expected")
  }' tools/r3-trials/drive/control/expected.txt "$dir/lines.txt")
echo "control mode=$mode machine=$machine task=$task result=$result"
[ "$result" = differs ] && exit 1
exit 0
