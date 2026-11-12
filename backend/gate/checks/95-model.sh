#!/bin/sh
set -u
ROOT=${HALVO_ROOT:-$(cd "$(dirname "$0")/../.." && pwd -P)}
. "$ROOT/scripts/env.sh"
FIXTURES="$ROOT/gate/fixtures/model"

if [ "${1:-}" != --self-test ]; then
  HALVO_ROOT=$ROOT node "$ROOT/formal/check.mjs"
  exit $?
fi

STATE=${GATE_STATE:-$ROOT/.tmp}
mkdir -p "$STATE"

scratch_copy() {
  copy=$(mktemp -d "$STATE/model-copy-$1.XXXXXX") || return 1
  (cd "$ROOT" && git ls-files -z --cached --others --exclude-standard | tar --null -T - -cf -) | tar -xf - -C "$copy" || return 1
  ln -s "$ROOT/node_modules" "$copy/node_modules"
  ln -s "$ROOT/.tools" "$copy/.tools"
  printf '%s\n' "$copy"
}

field() {
  node -p '[JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"))[process.argv[2]] ?? ""].flat().join("\n")' "$1" "$2"
}

expect() {
  kind=$1
  fixture=$2
  dir=$3
  traces=$4
  only=$5
  log="$STATE/model-self-test-$fixture.log"
  if [ "$only" = replay ]; then
    env -u HALVO_ROOT "HALVO_ROOT=$dir" node "$dir/formal/check.mjs" --replay-only ${traces:+"$traces"} > "$log" 2>&1
  else
    env -u HALVO_ROOT "HALVO_ROOT=$dir" node "$dir/formal/check.mjs" > "$log" 2>&1
  fi
  status=$?
  line=$(grep -E '^(PASS|FAIL) model: ' "$log" | tail -1)
  sed 's/^/  /' "$log"
  case "$kind:$status:$line" in
    "good:0:PASS model: "*)
      echo "self-test model: good $fixture: PASS as expected"
      return 0
      ;;
    "bad:1:FAIL model: "*)
      missing=$(printf '%s\n' "$want" | while IFS= read -r part; do
        [ -z "$part" ] || printf '%s\n' "$line" | grep -qF -- "$part" || printf '%s;' "$part"
      done)
      if [ -z "$missing" ]; then
        echo "self-test model: bad $fixture: FAIL as expected"
        return 0
      fi
      ;;
  esac
  echo "self-test model: $kind $fixture: exit $status, line \"$line\"${want:+, expected the text $want}"
  return 1
}

if [ "$(git -C "$ROOT" rev-parse --show-toplevel 2> /dev/null)" != "$ROOT" ]; then
  echo "self-test model: the bad fixtures need a git checkout: they copy the files that git lists; $ROOT is not the top of one"
  exit 1
fi
for mutant in "$FIXTURES"/bad-*.json; do
  [ -f "$mutant" ] || continue
  fixture=$(basename "$mutant" .json)
  want=$(field "$mutant" line)
  traces=$(field "$mutant" traces)
  only=$(field "$mutant" only)
  if [ -n "$traces" ] && [ "$only" != replay ]; then
    echo "self-test model: bad $fixture: a fixture with traces runs the replay alone and must say so"
    exit 1
  fi
  if [ -n "$traces" ]; then
    expect bad "$fixture" "$ROOT" "$FIXTURES/$traces" "$only" || exit 1
    continue
  fi
  copy=$(scratch_copy "$fixture") || {
    echo "self-test model: bad $fixture: the scratch copy could not be made"
    exit 1
  }
  if ! node "$ROOT/gate/fixtures/contract/apply-mutant.mjs" "$mutant" "$copy"; then
    echo "self-test model: bad $fixture: the mutant does not apply to the checkout"
    exit 1
  fi
  expect bad "$fixture" "$copy" "" "$only" || exit 1
done
want=
expect good checkout "$ROOT" "" "" || exit 1
log="$STATE/model-self-test-view.log"
HALVO_ROOT=$ROOT node "$ROOT/formal/controls.mjs" view > "$log" 2>&1
status=$?
sed 's/^/  /' "$log"
if [ "$status" -ne 0 ]; then
  echo "self-test model: good view: exit $status, the control of the view did not end as expected"
  exit 1
fi
echo "self-test model: good view: PASS as expected"
exit 0
