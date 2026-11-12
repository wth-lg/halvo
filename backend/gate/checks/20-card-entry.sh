#!/bin/sh
set -u
ROOT=${HALVO_ROOT:-$(cd "$(dirname "$0")/../.." && pwd -P)}
. "$ROOT/scripts/env.sh"
FIXTURES="$ROOT/gate/fixtures/card-entry"
STATE=${GATE_STATE:-$ROOT/.tmp}
mkdir -p "$STATE"

check_in() {
  HALVO_ROOT=$1 node "$1/test/billing/card-entry-check.mjs"
}

if [ "${1:-}" != --self-test ]; then
  check_in "$ROOT"
  exit $?
fi

scratch_copy() {
  copy=$(mktemp -d "$STATE/card-entry-copy-$1.XXXXXX") || return 1
  (cd "$ROOT" && git ls-files -z --cached --others --exclude-standard | tar --null -T - -cf -) | tar -xf - -C "$copy" || return 1
  ln -s "$ROOT/node_modules" "$copy/node_modules"
  ln -s "$ROOT/.tools" "$copy/.tools"
  printf '%s\n' "$copy"
}

offset_is_free() {
  for base in 8787 8788 12111 12112 12113 18787 18788; do
    ! lsof -nP -iTCP:"$((base + $1))" -sTCP:LISTEN > /dev/null 2>&1 || return 1
  done
}

free_offset() {
  offset=$(($(printf '%s' "$1" | cksum | cut -d ' ' -f 1) % 300 * 30))
  tries=0
  while ! offset_is_free "$offset"; do
    tries=$((tries + 1))
    [ "$tries" -lt 300 ] || return 1
    offset=$(((offset + 30) % 9000))
  done
  printf '%s\n' "$offset"
}

expect() {
  kind=$1
  fixture=$2
  dir=$3
  log="$STATE/card-entry-self-test-$fixture.log"
  check_in "$dir" > "$log" 2>&1
  status=$?
  line=$(grep -E '^(PASS|FAIL) card-entry: ' "$log" | tail -1)
  sed 's/^/  /' "$log"
  case "$kind:$status:$line" in
    "good:0:PASS card-entry: "*)
      echo "self-test card-entry: good $fixture: PASS as expected"
      return 0
      ;;
    "bad:1:FAIL card-entry: "*)
      missing=$(printf '%s\n' "$want" | while IFS= read -r part; do
        [ -z "$part" ] || printf '%s\n' "$line" | grep -qF -- "$part" || printf '%s;' "$part"
      done)
      if [ -z "$missing" ]; then
        echo "self-test card-entry: bad $fixture: FAIL as expected"
        return 0
      fi
      ;;
  esac
  echo "self-test card-entry: $kind $fixture: exit $status, line \"$line\"${want:+, expected the text $want}"
  return 1
}

failed=0
if [ "$(git -C "$ROOT" rev-parse --show-toplevel 2> /dev/null)" != "$ROOT" ]; then
  echo "self-test card-entry: the bad fixtures need a git checkout, as the gate does: they copy the files that git lists; $ROOT is not the top of one"
  exit 1
fi
sh "$ROOT/scripts/stack.sh" up > "$STATE/card-entry-self-test-stack.log" 2>&1 ||
  echo "self-test card-entry: the stack did not come up; its log: $STATE/card-entry-self-test-stack.log"
want=
expect good checkout "$ROOT" || failed=1
for mutant in "$FIXTURES"/bad-*.json; do
  [ -f "$mutant" ] || continue
  fixture=$(basename "$mutant" .json)
  want=$(node -p '[JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")).line].flat().join("\n")' "$mutant")
  copy=$(scratch_copy "$fixture") || {
    echo "self-test card-entry: bad $fixture: the scratch copy could not be made"
    failed=1
    continue
  }
  if ! node "$ROOT/gate/fixtures/contract/apply-mutant.mjs" "$mutant" "$copy"; then
    echo "self-test card-entry: bad $fixture: the mutant does not apply to the checkout"
    failed=1
    continue
  fi
  stack=$(node -p 'JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")).stack ?? "shared"' "$mutant")
  if [ "$stack" = shared ]; then
    expect bad "$fixture" "$copy" || failed=1
    continue
  fi
  offset=$(free_offset "$copy") || {
    echo "self-test card-entry: bad $fixture: no free port offset for the copy's stack"
    failed=1
    continue
  }
  mkdir -p "$copy/.stack"
  printf '%s\n' "$offset" > "$copy/.stack/port-offset"
  env -u STACK_PORT_OFFSET -u STACK_STATE -u HALVO_ROOT -u GATE_STATE sh "$copy/scripts/stack.sh" up > "$STATE/card-entry-self-test-$fixture-stack.log" 2>&1 ||
    echo "self-test card-entry: bad $fixture: the copy's stack did not come up; its log: $STATE/card-entry-self-test-$fixture-stack.log"
  (
    unset STACK_PORT_OFFSET STACK_STATE
    STACK_PORT_OFFSET=$offset
    export STACK_PORT_OFFSET
    expect bad "$fixture" "$copy"
  ) || failed=1
  env -u STACK_PORT_OFFSET -u STACK_STATE -u HALVO_ROOT -u GATE_STATE sh "$copy/scripts/stack.sh" down >> "$STATE/card-entry-self-test-$fixture-stack.log" 2>&1
done
exit "$failed"
