#!/bin/sh
set -u

usage() {
  cat <<'EOF'
usage: sh tools/r3-trials/drive/run.sh --model TOKEN --task bill|order|reply --seed 1|2|3
           --browser PATH --port N [--driver PATH]
       sh tools/r3-trials/drive/run.sh --prompt-lines
       sh tools/r3-trials/drive/run.sh --help

Runs one cell of the driver trial (contracts.md A13.6) on the machine it is on: mac
(macOS outside CI) or hosted-win (Windows on a hosted runner). The model is the line
"weights model=TOKEN" of tools/r3-trials/model/weights.txt, whose file must be
target/r3-weights/<file> with the SHA-256 of that line (sh
tools/r3-trials/model/fetch.sh TOKEN). The child is r3-model of tools/r3-trials/model
with the settings docs/r3/model-settings.toml at the level medium, under the cap of
8589934592 bytes of crates/halvo-cap; the browser is started by the cdp library of
tools/r3-trials/web, headless; the fixture is tools/fixture-server/variants/server.mjs
on 127.0.0.1:N with the seed, reset before each run.

For bill and order the cell is a compile run, three replays of its playbook and one
recovery run on the variant changed_page_2, and it prints one attempt line per run and
then one drive line:

  attempt model=<token> machine=<m> task=<t> seed=<n> kind=<compile|replay|recover>
          i=<n> pass=<yes|no> steps=<n> wall_s=<n> tokens_in=<n> tokens_out=<n>
  drive model=<token> machine=<m> task=<t> seed=<n> compile=<ok|miss> replays_ok=<n>
        of=3 recover=<ok|miss> steps=<n> wall_s=<n> model_tokens=<n>
        typed_credential=<n> effects_without_confirm=<n>

For reply it is one write run, and it prints its attempt line (kind=write) and

  reply model=<token> machine=<m> seed=<n> bodies=<n> of=2 wall_s=<n> model_tokens=<n>

On mac each run is preceded by the line
"load model=<token> machine=mac task=<t> seed=<n> kind=<k> i=<n> load1=<x>", the
1-minute load when the run started. --prompt-lines prints one line
"prompt file=<name> sha256=<hex>" for each file of tools/r3-trials/drive/prompts/, then
the line "prompt file=src/words.rs sha256=<hex>" for the module that holds the harness's
own words in a request (ruling CP-179 (2) (a)), and runs nothing. No verdict line is printed.

--driver names the driver binary of a candidate of A13.2 that needs one; the verdict
of docs/r3/trials/web-executor.md is cdp, which needs none, so --driver is refused.

Before the cell the script builds tools/r3-trials/model/target/release/r3-model with
GGML_METAL=OFF (cmake from PATH or from sh tools/r3-trials/model/fetch.sh --cmake) and
tools/r3-trials/drive/target/release/r3-drive, each with cargo build --release
--locked; cargo's output goes to standard error. Each cell keeps a directory of its
own under TMPDIR or /tmp with the trace of its runs, the prompt and the snapshot of each
request (requests.txt), the model's standard error, the playbook that the compile left
(playbook.jsonl) and the reply bodies, and the script removes nothing: the trace holds
the model's replies, the requests the fixture's invented values and the bodies their text.

Exit 0: the cell ran. 1: it could not run, and a line
"notrun model=<token> machine=<m> task=<t> seed=<n> cause=<token>" says why
(build, fixture, browser_start, model_start, lock, signal and others). 2: a refusal,
one line "REFUSED <cause>: <detail>" on standard error; the causes are usage,
browser, port and weights.
EOF
}

refuse() {
  printf 'REFUSED %s: %s\n' "$1" "$2" >&2
  exit 2
}

here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P) || refuse usage "the directory of run.sh cannot be read"
root=$(CDPATH='' cd -- "$here/../../.." && pwd -P) || refuse usage "the root of the repository cannot be read"
cd "$root" || refuse usage "the root of the repository cannot be entered"

sha_of() {
  if command -v shasum >/dev/null 2>&1; then
    sum=$(shasum -a 256 "$1") || return 1
  else
    sum=$(sha256sum "$1") || return 1
  fi
  printf '%s\n' "${sum%% *}"
}

model=
task=
seed=
browser=
port=
while [ $# -gt 0 ]; do
  case $1 in
    --help)
      usage
      exit 0
      ;;
    --prompt-lines)
      [ $# -eq 1 ] || refuse usage "--prompt-lines takes no other argument"
      for name in boundary.txt goal-bill.txt goal-order.txt goal-reply.txt; do
        hex=$(sha_of "tools/r3-trials/drive/prompts/$name") || refuse usage "prompts/$name cannot be read"
        echo "prompt file=$name sha256=$hex"
      done
      hex=$(sha_of tools/r3-trials/drive/src/words.rs) || refuse usage "src/words.rs cannot be read"
      echo "prompt file=src/words.rs sha256=$hex"
      exit 0
      ;;
    --model | --task | --seed | --browser | --port | --driver)
      [ $# -ge 2 ] || refuse usage "$1 has no value"
      case $1 in
        --model) [ -z "$model" ] || refuse usage "--model is given twice"; model=$2 ;;
        --task) [ -z "$task" ] || refuse usage "--task is given twice"; task=$2 ;;
        --seed) [ -z "$seed" ] || refuse usage "--seed is given twice"; seed=$2 ;;
        --browser) [ -z "$browser" ] || refuse usage "--browser is given twice"; browser=$2 ;;
        --port) [ -z "$port" ] || refuse usage "--port is given twice"; port=$2 ;;
        --driver) refuse usage "the executor of the verdict, cdp, takes no driver binary" ;;
      esac
      shift 2
      ;;
    *) refuse usage "unknown argument $1" ;;
  esac
done
[ -n "$model" ] || refuse usage "--model is missing"
case $model in
  none | synthetic | *[!abcdefghijklmnopqrstuvwxyz0123456789_]*) refuse usage "$model is no model token" ;;
esac
case $task in
  bill | order | reply) ;;
  *) refuse usage "--task takes bill, order or reply" ;;
esac
case $seed in
  1 | 2 | 3) ;;
  *) refuse usage "--seed takes 1, 2 or 3" ;;
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
notrun() {
  echo "notrun model=$model machine=$machine task=$task seed=$seed cause=$1"
  exit 1
}
case $machine in
  mac | hosted-win) ;;
  *) notrun machine ;;
esac

line=$(grep "^weights model=$model " tools/r3-trials/model/weights.txt) ||
  refuse weights "weights.txt has no line for the model $model"
file=${line#* file=}
file=${file%% *}
sha256=${line#* sha256=}
sha256=${sha256%% *}
weights=target/r3-weights/$file
[ -f "$weights" ] || refuse weights "$weights is missing: run sh tools/r3-trials/model/fetch.sh $model"
sum=$(sha_of "$weights") || refuse weights "$weights cannot be read"
[ "$sum" = "$sha256" ] || refuse weights "$weights does not have the SHA-256 of its line"

child=tools/r3-trials/model/target/release/r3-model$exe
drive=tools/r3-trials/drive/target/release/r3-drive$exe
build() {
  if ! command -v cmake >/dev/null 2>&1; then
    cmake_dir=$(sh tools/r3-trials/model/fetch.sh --cmake-dir) || return 1
    PATH="$cmake_dir:$PATH"
    export PATH
  fi
  (cd tools/r3-trials/model && GGML_METAL=OFF cargo build --release --locked) || return 1
  (cd tools/r3-trials/drive && cargo build --release --locked) || return 1
  [ -x "$child" ] && [ -x "$drive" ]
}
build >&2 || notrun build

dir=$(mktemp -d "${TMPDIR:-/tmp}/r3-drive-cell.XXXXXX") || notrun tmp

pid=
# shellcheck disable=SC2329
stop() {
  if [ -n "$pid" ]; then
    kill -TERM "$pid" 2>/dev/null
    wait "$pid"
  else
    notrun signal
  fi
  exit 1
}
trap stop INT TERM

"$drive" --repo "$root" --model "$model" --weights "$weights" --child "$child" \
  --settings docs/r3/model-settings.toml --task "$task" --seed "$seed" \
  --browser "$browser" --port "$port" --machine "$machine" --run-dir "$dir" &
pid=$!
wait "$pid"
status=$?
pid=
exit "$status"
