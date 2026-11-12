#!/bin/sh
set -u
ROOT=${HALVO_ROOT:-$(cd "$(dirname "$0")/../.." && pwd -P)}
. "$ROOT/scripts/env.sh"
FIXTURES="$ROOT/gate/fixtures/concurrency"
SELF_TEST_CELLS='^(wave@200|usage-last@20)$'

check_in() {
  dir=$1
  shift
  HALVO_ROOT=$dir node "$dir/stress/check.mjs" "$@"
}

if [ "${1:-}" != --self-test ]; then
  check_in "$ROOT"
  exit $?
fi

STATE=${GATE_STATE:-$ROOT/.tmp}
mkdir -p "$STATE"

scratch_copy() {
  copy=$(mktemp -d "$STATE/concurrency-copy-$1.XXXXXX") || return 1
  (cd "$ROOT" && git ls-files -z --cached --others --exclude-standard | tar --null -T - -cf -) | tar -xf - -C "$copy" || return 1
  ln -s "$ROOT/node_modules" "$copy/node_modules"
  printf '%s\n' "$copy"
}

expect() {
  kind=$1
  fixture=$2
  dir=$3
  cells=$4
  log="$STATE/concurrency-self-test-$fixture.log"
  check_in "$dir" --cells "$cells" --runs 1 > "$log" 2>&1
  status=$?
  line=$(grep -E '^(PASS|FAIL) concurrency: ' "$log" | tail -1)
  sed 's/^/  /' "$log"
  case "$kind:$status:$line" in
    "good:0:PASS concurrency: "*)
      echo "self-test concurrency: good $fixture: PASS as expected"
      return 0
      ;;
    "bad:1:FAIL concurrency: "*)
      missing=$(printf '%s\n' "$want" | while IFS= read -r part; do
        [ -z "$part" ] || printf '%s\n' "$line" | grep -qF -- "$part" || printf '%s;' "$part"
      done)
      if [ -z "$missing" ]; then
        echo "self-test concurrency: bad $fixture: FAIL as expected"
        return 0
      fi
      ;;
  esac
  echo "self-test concurrency: $kind $fixture: exit $status, line \"$line\"${want:+, expected the text $want}"
  return 1
}

failed=0
if [ -z "$(git -C "$ROOT" ls-files --cached --others --exclude-standard -- gate/checks/50-concurrency.sh 2> /dev/null)" ]; then
  echo "self-test concurrency: the bad fixtures need a git checkout: they copy the files that git lists under $ROOT, and git lists no gate/checks/50-concurrency.sh there"
  exit 1
fi
node "$FIXTURES/records.mjs" || failed=1
want=
expect good checkout "$ROOT" "$SELF_TEST_CELLS" || failed=1
for mutant in "$FIXTURES"/bad-*.json; do
  [ -f "$mutant" ] || continue
  fixture=$(basename "$mutant" .json)
  want=$(node -p '[JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")).line].flat().join("\n")' "$mutant")
  cells=$(node -p 'JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")).cells ?? process.argv[2]' "$mutant" "$SELF_TEST_CELLS")
  copy=$(scratch_copy "$fixture") || {
    echo "self-test concurrency: bad $fixture: the scratch copy could not be made"
    failed=1
    continue
  }
  if ! node "$ROOT/gate/fixtures/contract/apply-mutant.mjs" "$mutant" "$copy"; then
    echo "self-test concurrency: bad $fixture: the mutant does not apply to the checkout"
    failed=1
    continue
  fi
  expect bad "$fixture" "$copy" "$cells" || failed=1
done
exit "$failed"
