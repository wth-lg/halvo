#!/bin/sh
set -u
ROOT=${HALVO_ROOT:-$(cd "$(dirname "$0")/../.." && pwd -P)}
. "$ROOT/scripts/env.sh"
FIXTURES="$ROOT/gate/fixtures/schema"
STATE=${GATE_STATE:-$ROOT/.tmp}
mkdir -p "$STATE"

check_in() {
  dir=$1
  vitest_log=$(mktemp "$STATE/schema-vitest.XXXXXX") || return 1
  (cd "$dir" && XDG_CONFIG_HOME="$HALVO_WRANGLER_CONFIG_HOME" "$dir/node_modules/.bin/vitest" run --root "$dir" --silent=false \
    test/contract/schema-check.test.ts) > "$vitest_log" 2>&1
  status=$?
  echo "  vitest exit $status; its log: $vitest_log"
  line=$(sed -n 's/^schema-check-result: //p' "$vitest_log" | tail -1)
  case "$status:$line" in
    "0:PASS schema: "*)
      printf '%s\n' "$line"
      return 0
      ;;
    *":FAIL schema: "*)
      printf '%s\n' "$line"
      return 1
      ;;
  esac
  tail -20 "$vitest_log" | sed 's/^/  /'
  echo "FAIL schema: seed=unmeasured unmodified=unmeasured validator=unmeasured forbidden=unmeasured arbitrary=unmeasured query=unmeasured raw=unmeasured 5xx=unmeasured handler-calls=unmeasured sentinel=unmeasured; first failure: test/contract/schema-check.test.ts printed no result line (vitest exit $status)"
  return 1
}

if [ "${1:-}" != --self-test ]; then
  check_in "$ROOT"
  exit $?
fi

scratch_copy() {
  copy=$(mktemp -d "$STATE/schema-copy-$1.XXXXXX") || return 1
  (cd "$ROOT" && git ls-files -z --cached --others --exclude-standard | tar --null -T - -cf -) | tar -xf - -C "$copy" || return 1
  ln -s "$ROOT/node_modules" "$copy/node_modules"
  printf '%s\n' "$copy"
}

expect() {
  kind=$1
  fixture=$2
  dir=$3
  log="$STATE/schema-self-test-$fixture.log"
  check_in "$dir" > "$log" 2>&1
  status=$?
  line=$(grep -E '^(PASS|FAIL) schema: ' "$log" | tail -1)
  sed 's/^/  /' "$log"
  case "$kind:$status:$line" in
    "good:0:PASS schema: "*)
      echo "self-test schema: good $fixture: PASS as expected"
      return 0
      ;;
    "bad:1:FAIL schema: "*)
      missing=$(printf '%s\n' "$want" | while IFS= read -r part; do
        [ -z "$part" ] || printf '%s\n' "$line" | grep -qF -- "$part" || printf '%s;' "$part"
      done)
      if [ -z "$missing" ]; then
        echo "self-test schema: bad $fixture: FAIL as expected"
        return 0
      fi
      ;;
  esac
  echo "self-test schema: $kind $fixture: exit $status, line \"$line\"${want:+, expected the text $want}"
  return 1
}

failed=0
if [ "$(git -C "$ROOT" rev-parse --show-toplevel 2> /dev/null)" != "$ROOT" ]; then
  echo "self-test schema: the bad fixtures need a git checkout, as the gate does: they copy the files that git lists; $ROOT is not the top of one"
  exit 1
fi
want=
expect good checkout "$ROOT" || failed=1
for mutant in "$FIXTURES"/bad-*.json; do
  [ -f "$mutant" ] || continue
  fixture=$(basename "$mutant" .json)
  want=$(node -p '[JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")).line].flat().join("\n")' "$mutant")
  copy=$(scratch_copy "$fixture") || {
    echo "self-test schema: bad $fixture: the scratch copy could not be made"
    failed=1
    continue
  }
  if ! node "$ROOT/gate/fixtures/contract/apply-mutant.mjs" "$mutant" "$copy"; then
    echo "self-test schema: bad $fixture: the mutant does not apply to the checkout"
    failed=1
    continue
  fi
  expect bad "$fixture" "$copy" || failed=1
done
exit "$failed"
