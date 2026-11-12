#!/bin/sh
set -u
ROOT=${HALVO_ROOT:-$(cd "$(dirname "$0")/../.." && pwd -P)}
. "$ROOT/scripts/env.sh"
CARGO_HOME=${CARGO_HOME:-$HOME/.cargo}
PATH="$CARGO_HOME/bin:$PATH"
FIXTURES="$ROOT/gate/fixtures/contract"

check_in() {
  HALVO_ROOT=$1 node "$1/test/contract/contract-check.mjs"
}

if [ "${1:-}" != --self-test ]; then
  check_in "$ROOT"
  exit $?
fi

STATE=${GATE_STATE:-$ROOT/.tmp}
mkdir -p "$STATE"

scratch_copy() {
  copy=$(mktemp -d "$STATE/contract-copy-$1.XXXXXX") || return 1
  (cd "$ROOT" && git ls-files -z --cached --others --exclude-standard | tar --null -T - -cf -) | tar -xf - -C "$copy" || return 1
  ln -s "$ROOT/node_modules" "$copy/node_modules"
  mkdir -p "$copy/.tmp/contract-rust"
  if [ -d "$ROOT/.tmp/contract-rust/target" ]; then
    cp -cR "$ROOT/.tmp/contract-rust/target" "$copy/.tmp/contract-rust/target" 2> /dev/null ||
      cp -R "$ROOT/.tmp/contract-rust/target" "$copy/.tmp/contract-rust/target"
  fi
  printf '%s\n' "$copy"
}

expect() {
  kind=$1
  fixture=$2
  dir=$3
  log="$STATE/contract-self-test-$fixture.log"
  check_in "$dir" > "$log" 2>&1
  status=$?
  line=$(grep -E '^(PASS|FAIL) contract: ' "$log" | tail -1)
  sed 's/^/  /' "$log"
  case "$kind:$status:$line" in
    "good:0:PASS contract: "*)
      echo "self-test contract: good $fixture: PASS as expected"
      return 0
      ;;
    "bad:1:FAIL contract: "*)
      missing=$(printf '%s\n' "$want" | while IFS= read -r part; do
        [ -z "$part" ] || printf '%s\n' "$line" | grep -qF -- "$part" || printf '%s;' "$part"
      done)
      if [ -z "$missing" ]; then
        echo "self-test contract: bad $fixture: FAIL as expected"
        return 0
      fi
      ;;
  esac
  echo "self-test contract: $kind $fixture: exit $status, line \"$line\"${want:+, expected the text $want}"
  return 1
}

failed=0
if [ -z "$(git -C "$ROOT" ls-files --cached --others --exclude-standard -- gate/checks/70-contract.sh 2> /dev/null)" ]; then
  echo "self-test contract: the bad fixtures need a git checkout, as the gate does: they copy the files that git lists under $ROOT, and git lists no gate/checks/70-contract.sh there"
  exit 1
fi
sh "$ROOT/scripts/stack.sh" up > "$STATE/contract-self-test-stack.log" 2>&1 ||
  echo "self-test contract: the stack did not come up; its log: $STATE/contract-self-test-stack.log"
want=
expect good checkout "$ROOT" || failed=1
for mutant in "$FIXTURES"/bad-*.json; do
  [ -f "$mutant" ] || continue
  fixture=$(basename "$mutant" .json)
  want=$(node -p '[JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")).line].flat().join("\n")' "$mutant")
  copy=$(scratch_copy "$fixture") || {
    echo "self-test contract: bad $fixture: the scratch copy could not be made"
    failed=1
    continue
  }
  if ! node "$FIXTURES/apply-mutant.mjs" "$mutant" "$copy"; then
    echo "self-test contract: bad $fixture: the mutant does not apply to the checkout"
    failed=1
    continue
  fi
  expect bad "$fixture" "$copy" || failed=1
done
exit "$failed"
