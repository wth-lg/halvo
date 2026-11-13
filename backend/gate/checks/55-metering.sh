#!/bin/sh
set -u
ROOT=${HALVO_ROOT:-$(cd "$(dirname "$0")/../.." && pwd -P)}
. "$ROOT/scripts/env.sh"
FIXTURES="$ROOT/gate/fixtures/metering"
SELF_TEST_ARGS=${METERING_SELF_TEST_ARGS:---seeds 1 --orderings 600 --settings 24,25,26,27,28,29,30,31,32,33}

check_in() {
  dir=$1
  shift
  HALVO_ROOT=$dir node "$dir/test-kit/fake-meter/rerun/check.mjs" "$@"
}

if [ "${1:-}" != --self-test ]; then
  check_in "$ROOT"
  exit $?
fi

STATE=${GATE_STATE:-$ROOT/.tmp}
mkdir -p "$STATE"

scratch_copy() {
  copy=$(mktemp -d "$STATE/metering-copy-$1.XXXXXX") || return 1
  (cd "$ROOT" && git ls-files -z --cached --others --exclude-standard | tar --null -T - -cf -) | tar -xf - -C "$copy" || return 1
  ln -s "$ROOT/node_modules" "$copy/node_modules"
  ln -s "$ROOT/.tools" "$copy/.tools"
  printf '%s\n' "$copy"
}

checkout_copy() {
  top=$(git -C "$ROOT" rev-parse --show-toplevel) || return 1
  head=$(git -C "$ROOT" rev-parse HEAD) || return 1
  below=${ROOT#"$top"}
  base=$(mktemp -d "$STATE/metering-checkout-$1.XXXXXX") || return 1
  git clone -q --shared --no-checkout "$top" "$base/repo" || return 1
  git -C "$base/repo" checkout -q --detach "$head" || return 1
  (cd "$ROOT" && git ls-files -z --cached --others --exclude-standard | tar --null -T - -cf -) | tar -xf - -C "$base/repo$below" || return 1
  ln -s "$ROOT/node_modules" "$base/repo$below/node_modules"
  ln -s "$ROOT/.tools" "$base/repo$below/.tools"
  printf '%s\n' "$base/repo$below"
}

field() {
  node -p "JSON.parse(require('fs').readFileSync(process.argv[1], 'utf8')).$2 ?? ''" "$1"
}

in_the_copy() {
  env -u STACK_PORT_OFFSET -u STACK_STATE -u HALVO_ROOT "$@"
}

expect() {
  kind=$1
  fixture=$2
  dir=$3
  shift 3
  log="$STATE/metering-self-test-$fixture.log"
  if [ "$dir" = "$ROOT" ]; then
    check_in "$dir" "$@" > "$log" 2>&1
  else
    in_the_copy "HALVO_ROOT=$dir" "GATE_STATE=$STATE" node "$dir/test-kit/fake-meter/rerun/check.mjs" "$@" > "$log" 2>&1
  fi
  status=$?
  line=$(grep -E '^(PASS|FAIL) metering: ' "$log" | tail -1)
  sed 's/^/  /' "$log"
  case "$kind:$status:$line" in
    "good:0:PASS metering: "*)
      echo "self-test metering: good $fixture: PASS as expected"
      return 0
      ;;
    "bad:1:FAIL metering: "*)
      missing=$(printf '%s\n' "$want" | while IFS= read -r part; do
        [ -z "$part" ] || printf '%s\n' "$line" | grep -qF -- "$part" || printf '%s;' "$part"
      done)
      if [ -z "$missing" ]; then
        echo "self-test metering: bad $fixture: FAIL as expected"
        return 0
      fi
      ;;
  esac
  echo "self-test metering: $kind $fixture: exit $status, line \"$line\"${want:+, expected the text $want}"
  return 1
}

failed=0
if [ -z "$(git -C "$ROOT" ls-files --cached --others --exclude-standard -- gate/checks/55-metering.sh 2> /dev/null)" ]; then
  echo "self-test metering: the bad fixtures need a git checkout: they copy the files that git lists under $ROOT, and git lists no gate/checks/55-metering.sh there"
  exit 1
fi
want=
# shellcheck disable=SC2086
expect good checkout "$ROOT" $SELF_TEST_ARGS || failed=1
for good in "$FIXTURES"/good-*.json; do
  [ -f "$good" ] || continue
  fixture=$(basename "$good" .json)
  [ -z "${METERING_SELF_TEST_ONLY:-}" ] || [ "$fixture" = "$METERING_SELF_TEST_ONLY" ] || continue
  set --
  for arg in $(node -p '(JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")).args ?? []).map((a) => a === "" ? "-" : a).join("\n")' "$good"); do
    [ "$arg" = - ] && arg=
    set -- "$@" "$arg"
  done
  expect good "$fixture" "$ROOT" "$@" || failed=1
done
for mutant in "$FIXTURES"/bad-*.json; do
  [ -f "$mutant" ] || continue
  fixture=$(basename "$mutant" .json)
  [ -z "${METERING_SELF_TEST_ONLY:-}" ] || [ "$fixture" = "$METERING_SELF_TEST_ONLY" ] || continue
  want=$(node -p '[JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")).line].flat().join("\n")' "$mutant")
  if [ "$(field "$mutant" checkout)" = true ]; then
    copy=$(checkout_copy "$fixture")
  else
    copy=$(scratch_copy "$fixture")
  fi || {
    echo "self-test metering: bad $fixture: the scratch copy could not be made"
    failed=1
    continue
  }
  if ! node "$ROOT/gate/fixtures/contract/apply-mutant.mjs" "$mutant" "$copy"; then
    echo "self-test metering: bad $fixture: the mutant does not apply to the checkout"
    failed=1
    continue
  fi
  set --
  for arg in $(node -p '(JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")).args ?? []).map((a) => a === "" ? "-" : a).join("\n")' "$mutant"); do
    [ "$arg" = - ] && arg=
    set -- "$@" "$arg"
  done
  if [ "$(field "$mutant" control)" = wrong-text ]; then
    if expect bad "$fixture" "$copy" "$@"; then
      echo "self-test metering: control $fixture: a wrong expected text read as expected"
      failed=1
    else
      echo "self-test metering: control $fixture: the wrong expected text is refused, as it must be"
    fi
  else
    expect bad "$fixture" "$copy" "$@" || failed=1
  fi
done
exit "$failed"
