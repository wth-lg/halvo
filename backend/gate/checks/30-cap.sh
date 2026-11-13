#!/bin/sh
set -u
ROOT=${HALVO_ROOT:-$(cd "$(dirname "$0")/../.." && pwd -P)}
. "$ROOT/scripts/env.sh"
FIXTURES="$ROOT/gate/fixtures/cap"

check_in() {
  HALVO_ROOT=$1 node "$1/test/ledger/cap-check.mjs"
}

if [ "${1:-}" != --self-test ]; then
  check_in "$ROOT"
  exit $?
fi

STATE=${GATE_STATE:-$ROOT/.tmp}
mkdir -p "$STATE"

scratch_copy() {
  copy=$(mktemp -d "$STATE/cap-copy-$1.XXXXXX") || return 1
  (cd "$ROOT" && git ls-files -z --cached --others --exclude-standard | tar --null -T - -cf -) | tar -xf - -C "$copy" || return 1
  ln -s "$ROOT/node_modules" "$copy/node_modules"
  ln -s "$ROOT/.tools" "$copy/.tools"
  printf '%s\n' "$copy"
}

in_the_copy() {
  env -u STACK_PORT_OFFSET -u STACK_STATE -u HALVO_ROOT "$@"
}

expect() {
  kind=$1
  fixture=$2
  dir=$3
  log="$STATE/cap-self-test-$fixture.log"
  if [ "$dir" = "$ROOT" ]; then
    check_in "$dir" > "$log" 2>&1
  else
    in_the_copy "HALVO_ROOT=$dir" node "$dir/test/ledger/cap-check.mjs" > "$log" 2>&1
  fi
  status=$?
  line=$(grep -E '^(PASS|FAIL) cap: ' "$log" | tail -1)
  sed 's/^/  /' "$log"
  case "$kind:$status:$line" in
    "good:0:PASS cap: "*)
      echo "self-test cap: good $fixture: PASS as expected"
      return 0
      ;;
    "bad:1:FAIL cap: "*)
      missing=$(printf '%s\n' "$want" | while IFS= read -r part; do
        [ -z "$part" ] || printf '%s\n' "$line" | grep -qF -- "$part" || printf '%s;' "$part"
      done)
      if [ -z "$missing" ]; then
        echo "self-test cap: bad $fixture: FAIL as expected"
        return 0
      fi
      ;;
  esac
  echo "self-test cap: $kind $fixture: exit $status, line \"$line\"${want:+, expected the text $want}"
  return 1
}

failed=0
if [ -z "$(git -C "$ROOT" ls-files --cached --others --exclude-standard -- gate/checks/30-cap.sh 2> /dev/null)" ]; then
  echo "self-test cap: the bad fixtures need a git checkout: they copy the files that git lists under $ROOT, and git lists no gate/checks/30-cap.sh there"
  exit 1
fi
sh "$ROOT/scripts/stack.sh" up > "$STATE/cap-self-test-stack.log" 2>&1 ||
  echo "self-test cap: the stack did not come up; its log: $STATE/cap-self-test-stack.log"
want=
expect good checkout "$ROOT" || failed=1
for mutant in "$FIXTURES"/bad-*.json; do
  [ -f "$mutant" ] || continue
  fixture=$(basename "$mutant" .json)
  want=$(node -p '[JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")).line].flat().join("\n")' "$mutant")
  copy=$(scratch_copy "$fixture") || {
    echo "self-test cap: bad $fixture: the scratch copy could not be made"
    failed=1
    continue
  }
  if ! node "$ROOT/gate/fixtures/contract/apply-mutant.mjs" "$mutant" "$copy"; then
    echo "self-test cap: bad $fixture: the mutant does not apply to the checkout"
    failed=1
    continue
  fi
  if ! in_the_copy sh "$copy/scripts/stack.sh" up > "$STATE/cap-self-test-$fixture-stack.log" 2>&1; then
    echo "self-test cap: bad $fixture: the copy's stack did not come up; its log: $STATE/cap-self-test-$fixture-stack.log"
    in_the_copy sh "$copy/scripts/stack.sh" down >> "$STATE/cap-self-test-$fixture-stack.log" 2>&1
    failed=1
    continue
  fi
  expect bad "$fixture" "$copy" || failed=1
  in_the_copy sh "$copy/scripts/stack.sh" down >> "$STATE/cap-self-test-$fixture-stack.log" 2>&1 ||
    echo "self-test cap: bad $fixture: the copy's stack did not stop; its log: $STATE/cap-self-test-$fixture-stack.log"
done
exit "$failed"
