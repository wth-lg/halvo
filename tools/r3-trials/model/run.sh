#!/bin/sh
set -u

usage() {
  cat <<'EOF'
usage: sh tools/r3-trials/model/run.sh --model TOKEN [--prompt action|reply]
       sh tools/r3-trials/model/run.sh --help

Runs the cells of one model of weights.txt on the machine it is on: mac (macOS
outside CI) or hosted-win (Windows on a hosted runner). A cell is one prompt of
prompts/ (action, then reply, or the one that --prompt names), answered by the
child r3-model under measure-peak of crates/halvo-cap:

  measure-peak --limit 8589934592 --runs 5 -- r3-model --weights FILE
      --settings tools/r3-trials/model/settings.toml --requests FILE --replies FILE

Each run answers the one request of the prompt. The script joins the line of
measure-peak for run i with the ready line and the reply line that the child
appended in run i, and prints one line per run:

  cell model=<token> file_sha256=<hex> machine=<mac|hosted-win> prompt=<action|reply>
       run=<i> peak_bytes=<n> capped=<yes|no> load_ms=<n> first_token_ms=<n>
       tokens_per_s=<n> tokens_in=<n> tokens_out=<n>

and once the line "runtime crate=<version> backend=cpu load=none sampling=greedy
ctx=<n> batch=<n> threads=<n> cache=f16". tokens_per_s is tokens_out times 1000
over the time from the first reply token to the end of the reply, rounded down.
A value that no line of the child gives, because a run did not end by itself, is
"-". The lines of measure-peak and the child's standard error go to standard
error. No reply text is printed.

It needs, at the root of the repository: target/r3-weights/<file> of the model
(sh tools/r3-trials/model/fetch.sh TOKEN), target/release/measure-peak (cargo
build --release --locked -p halvo-cap --bin measure-peak) and
tools/r3-trials/model/target/release/r3-model (cargo build --release --locked
in tools/r3-trials/model).

Exit 0: every cell ran. 1: a cell or a run could not run, and a line
"notrun model=<token> machine=<m> prompt=<p> [run=<i> ]cause=<token>" says
which. 2: a refusal, one line "REFUSED <cause>: <detail>" on standard error;
the causes are usage and weights.
EOF
}

here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P) || exit 2
root=$(CDPATH='' cd -- "$here/../../.." && pwd -P) || exit 2
cd "$root" || exit 2
limit=8589934592
runs=5
level=medium

refuse() {
  printf 'REFUSED %s: %s\n' "$1" "$2" >&2
  exit 2
}

model=
prompts="action reply"
prompt_given=no
while [ $# -gt 0 ]; do
  case $1 in
    --help)
      usage
      exit 0
      ;;
    --model)
      [ $# -ge 2 ] || refuse usage "--model has no value"
      [ -z "$model" ] || refuse usage "--model is given twice"
      model=$2
      shift 2
      ;;
    --prompt)
      [ $# -ge 2 ] || refuse usage "--prompt has no value"
      [ "$prompt_given" = no ] || refuse usage "--prompt is given twice"
      case $2 in
        action | reply) prompts=$2 ;;
        *) refuse usage "--prompt takes action or reply" ;;
      esac
      prompt_given=yes
      shift 2
      ;;
    *) refuse usage "unknown argument $1" ;;
  esac
done
[ -n "$model" ] || refuse usage "--model is missing"
case $model in
  *[!abcdefghijklmnopqrstuvwxyz0123456789_]*) refuse usage "$model is no model token" ;;
esac

line=$(grep "^weights model=$model " tools/r3-trials/model/weights.txt) ||
  refuse weights "weights.txt has no line for the model $model"
file=${line#* file=}
file=${file%% *}
sha256=${line#* sha256=}
sha256=${sha256%% *}
weights=target/r3-weights/$file
[ -f "$weights" ] || refuse weights "$weights is missing: run sh tools/r3-trials/model/fetch.sh $model"
if command -v shasum >/dev/null 2>&1; then
  sum=$(shasum -a 256 "$weights") || refuse weights "$weights cannot be read"
else
  sum=$(sha256sum "$weights") || refuse weights "$weights cannot be read"
fi
[ "${sum%% *}" = "$sha256" ] || refuse weights "$weights does not have the SHA-256 of its line"

case $(uname -s) in
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
measure=target/release/measure-peak$exe
child=tools/r3-trials/model/target/release/r3-model$exe
[ -x "$measure" ] || refuse usage "$measure is missing: build it first"
[ -x "$child" ] || refuse usage "$child is missing: build it first"

crate=$(sed -n '/^name = "llama-cpp-2"$/{n;s/^version = "\(.*\)"$/\1/p;}' tools/r3-trials/model/Cargo.lock)

failed=0
current=
measure_pid=

# shellcheck disable=SC2329
stop() {
  if [ -n "$measure_pid" ]; then
    if command -v pgrep >/dev/null 2>&1; then
      kids=$(pgrep -P "$measure_pid") || kids=
      for kid in $kids; do kill "$kid" 2>/dev/null; done
    fi
    kill "$measure_pid" 2>/dev/null
  fi
  echo "notrun model=$model machine=$machine prompt=${current:-$prompts} cause=stopped"
  exit 1
}
trap stop INT TERM

request_line() {
  awk -v grammar="$2" -v level="$level" '
    function esc(s,   out, i, c, n) {
      out = ""
      n = length(s)
      for (i = 1; i <= n; i++) {
        c = substr(s, i, 1)
        if (c == "\\") out = out "\\\\"
        else if (c == "\"") out = out "\\\""
        else if (c == "\t") out = out "\\t"
        else if (c == "\r") out = out "\\r"
        else out = out c
      }
      return out
    }
    $0 == "[system]" || $0 == "[prompt]" || $0 == "[snapshot]" { part = $0; next }
    {
      if (part == "") { bad = 1; exit }
      text[part] = (part in text) ? text[part] "\\n" esc($0) : esc($0)
    }
    END {
      if (bad || !("[system]" in text) || !("[prompt]" in text)) exit 1
      snapshot = ("[snapshot]" in text) ? "\"" text["[snapshot]"] "\"" : "null"
      printf "{\"id\":1,\"system\":\"%s\",\"prompt\":\"%s\",\"snapshot\":%s,\"grammar\":\"%s\",\"level\":\"%s\"}\n", text["[system]"], text["[prompt]"], snapshot, grammar, level
    }
  ' "$1"
}

join_lines() {
  awk -v model="$model" -v sha="$sha256" -v machine="$machine" -v prompt="$1" -v runs="$runs" -v crate="$crate" '
    FNR == NR {
      if ($1 == "run") {
        n++
        for (k = 2; k <= NF; k++) { split($k, kv, "="); r[n, kv[1]] = kv[2] }
      }
      next
    }
    /^ready / {
      g++
      for (k = 2; k <= NF; k++) { split($k, kv, "="); ready[g, kv[1]] = kv[2] }
      next
    }
    /^[{]/ {
      if (match($0, /"tokens_in":[0-9]+,"tokens_out":[0-9]+,"first_token_ms":[0-9]+,"ms":[0-9]+[}]$/)) {
        s = substr($0, RSTART, RLENGTH)
        gsub(/[^0-9,]/, "", s)
        split(s, v, ",")
        tin[g] = v[1]; tout[g] = v[2]; first[g] = v[3]; ms[g] = v[4]; reply[g] = 1
      }
    }
    END {
      joined = (n == runs && g == runs)
      for (i = 1; i <= n; i++) if (r[i, "exit"] != "0" || !(i in reply)) joined = 0
      if (g >= 1) printf "runtime crate=%s backend=cpu load=none sampling=greedy ctx=%s batch=%s threads=%s cache=f16\n", crate, ready[1, "ctx"], ready[1, "batch"], ready[1, "threads"]
      for (i = 1; i <= n; i++) {
        if (r[i, "exit"] != "0" && r[i, "capped"] != "yes") {
          printf "notrun model=%s machine=%s prompt=%s run=%d cause=child_failed\n", model, machine, prompt, i
          continue
        }
        if (joined) {
          rate = (ms[i] > first[i]) ? int(tout[i] * 1000 / (ms[i] - first[i])) : 0
          values = sprintf("load_ms=%s first_token_ms=%s tokens_per_s=%d tokens_in=%s tokens_out=%s", ready[i, "load_ms"], first[i], rate, tin[i], tout[i])
        } else {
          values = "load_ms=- first_token_ms=- tokens_per_s=- tokens_in=- tokens_out=-"
        }
        printf "cell model=%s file_sha256=%s machine=%s prompt=%s run=%d peak_bytes=%s capped=%s %s\n", model, sha, machine, prompt, i, r[i, "peak_bytes"], r[i, "capped"], values
      }
      for (i = n + 1; i <= runs; i++) printf "notrun model=%s machine=%s prompt=%s run=%d cause=no_run_line\n", model, machine, prompt, i
    }
  ' "$2" "$3"
}

for prompt in $prompts; do
  current=$prompt
  case $machine in
    mac | hosted-win) ;;
    *)
      echo "notrun model=$model machine=$machine prompt=$prompt cause=machine"
      failed=1
      continue
      ;;
  esac
  case $prompt in
    action) grammar=action ;;
    reply) grammar=text ;;
  esac
  dir=$(mktemp -d "${TMPDIR:-/tmp}/r3-model-cell.XXXXXX") || exit 1
  if ! request_line "tools/r3-trials/model/prompts/$prompt.txt" "$grammar" >"$dir/requests.txt"; then
    echo "notrun model=$model machine=$machine prompt=$prompt cause=prompt_file"
    failed=1
    continue
  fi
  "$measure" --limit "$limit" --runs "$runs" -- "$child" --weights "$weights" \
    --settings tools/r3-trials/model/settings.toml \
    --requests "$dir/requests.txt" --replies "$dir/replies.txt" \
    >"$dir/measure.txt" 2>"$dir/stderr.txt" &
  measure_pid=$!
  wait "$measure_pid"
  status=$?
  measure_pid=
  cat "$dir/stderr.txt" "$dir/measure.txt" >&2
  if [ "$status" -eq 2 ]; then
    echo "notrun model=$model machine=$machine prompt=$prompt cause=cap_refused"
    failed=1
    continue
  fi
  touch "$dir/replies.txt"
  join_lines "$prompt" "$dir/measure.txt" "$dir/replies.txt" >"$dir/lines.txt"
  cat "$dir/lines.txt"
  if grep -q '^notrun ' "$dir/lines.txt"; then
    failed=1
  fi
done
exit "$failed"
