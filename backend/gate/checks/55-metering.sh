#!/bin/sh
set -u
ROOT=${HALVO_ROOT:-$(cd "$(dirname "$0")/../.." && pwd -P)}
. "$ROOT/scripts/env.sh"
FIXTURES="$ROOT/gate/fixtures/metering"
SELF_TEST_ARGS=${METERING_SELF_TEST_ARGS:---seeds 1 --orderings 0 --settings 24,25,26,27,28,29,30}
COPY_LINE="self-test copy, no git: "

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

copy_files() {
  (cd "$ROOT" && git ls-files -z --cached --others --exclude-standard > "$2/files") || return 1
  (cd "$ROOT" && tar --null -T "$2/files" -cf "$2/files.tar") || return 1
  tar -xf "$2/files.tar" -C "$1" || return 1
  ln -sfn "$ROOT/node_modules" "$1/node_modules" || return 1
  ln -sfn "$ROOT/.tools" "$1/.tools" || return 1
}

scratch_copy() {
  base=$(mktemp -d "$STATE/metering-copy-$1.XXXXXX") || return 1
  mkdir "$base/backend" || return 1
  copy_files "$base/backend" "$base" || return 1
  printf '%s\n' "$base/backend"
}

checkout_copy() {
  top=$(git -C "$ROOT" rev-parse --show-toplevel) || return 1
  head=$(git -C "$ROOT" rev-parse HEAD) || return 1
  below=${ROOT#"$top"}
  base=$(mktemp -d "$STATE/metering-checkout-$1.XXXXXX") || return 1
  git clone -q --shared --no-checkout "$top" "$base/repo" || return 1
  git -C "$base/repo" checkout -q --detach "$head" || return 1
  copy_files "$base/repo$below" "$base" || return 1
  git init -q "$base/other" || return 1
  printf '%s\n' "$base/repo$below"
}

field() {
  node -p "JSON.parse(require('fs').readFileSync(process.argv[1], 'utf8')).$2 ?? ''" "$1"
}

env_of() {
  node -p 'Object.entries(JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")).env ?? {}).map(([k, v]) => k + "=" + v).join("\n")' "$1"
}

with_fixture_env() {
  unset OUTBOUND_RUN HALVO_METERING_PLAN
  if [ -n "$fixture_env" ]; then
    while IFS= read -r pair; do
      export "${pair?}"
    done <<EOF
$fixture_env
EOF
  fi
}

in_the_copy() {
  (
    with_fixture_env
    exec env -u STACK_PORT_OFFSET -u STACK_STATE -u HALVO_ROOT "$@"
  )
}

in_the_root() {
  (
    with_fixture_env
    check_in "$@"
  )
}

expect() {
  kind=$1
  fixture=$2
  dir=$3
  shift 3
  log="$STATE/metering-self-test-$fixture.log"
  ran=$((ran + 1))
  prefix=
  if [ "$dir" = "$ROOT" ]; then
    in_the_root "$dir" "$@" > "$log" 2>&1
  elif [ "$kind" = checkout ]; then
    kind=bad
    in_the_copy "HALVO_ROOT=$dir" "GATE_STATE=$STATE" node "$dir/test-kit/fake-meter/rerun/check.mjs" "$@" > "$log" 2>&1
  else
    prefix=$COPY_LINE
    in_the_copy "HALVO_ROOT=$dir" "GATE_STATE=$STATE" node "$dir/test-kit/fake-meter/rerun/check.mjs" --self-test-copy "$@" > "$log" 2>&1
  fi
  status=$?
  case "$kind" in
    own) line=$(grep -E '^(PASS|FAIL) metering-[a-z-]+: ' "$log" | tail -1) ;;
    *) line=$(grep -E "^$prefix(PASS|FAIL) metering: " "$log" | tail -1) ;;
  esac
  line=${line#"$prefix"}
  sed 's/^/  /' "$log"
  missing=$(printf '%s\n' "$want" | while IFS= read -r part; do
    [ -z "$part" ] || printf '%s\n' "$line" | grep -qF -- "$part" || printf '%s;' "$part"
  done)
  case "$kind:$status:$missing:$line" in
    "good:0::PASS metering: "*)
      echo "self-test metering: good $fixture: PASS as expected"
      return 0
      ;;
    "own:0::PASS metering-"*)
      echo "self-test metering: own $fixture: PASS as expected"
      return 0
      ;;
    "bad:1::FAIL metering: "*)
      echo "self-test metering: bad $fixture: FAIL as expected"
      return 0
      ;;
  esac
  echo "self-test metering: $kind $fixture: exit $status, line \"$line\"${want:+, expected the text $want}"
  return 1
}

failed=0
ran=0
fixture_env=
narrowed=
[ -z "${METERING_SELF_TEST_ONLY:-}" ] || narrowed="${narrowed}METERING_SELF_TEST_ONLY=$METERING_SELF_TEST_ONLY "
[ -z "${METERING_SELF_TEST_ARGS:-}" ] || narrowed="${narrowed}METERING_SELF_TEST_ARGS=$METERING_SELF_TEST_ARGS "
[ -z "$narrowed" ] || echo "self-test metering: NARROWED by ${narrowed}: this run is not the whole self-test"
if [ -z "$(git -C "$ROOT" ls-files --cached --others --exclude-standard -- gate/checks/55-metering.sh 2> /dev/null)" ]; then
  echo "self-test metering: the bad fixtures need a git checkout: they copy the files that git lists under $ROOT, and git lists no gate/checks/55-metering.sh there"
  exit 1
fi
want=
if [ -z "${METERING_SELF_TEST_ONLY:-}" ] || [ "$METERING_SELF_TEST_ONLY" = checkout ]; then
  # shellcheck disable=SC2086
  expect good checkout "$ROOT" $SELF_TEST_ARGS || failed=1
fi
for own in "$FIXTURES"/own-*.json; do
  [ -f "$own" ] || continue
  fixture=$(basename "$own" .json)
  [ -z "${METERING_SELF_TEST_ONLY:-}" ] || [ "$fixture" = "$METERING_SELF_TEST_ONLY" ] || continue
  want=$(node -p '[JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")).line].flat().join("\n")' "$own")
  set --
  for arg in $(node -p '(JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")).args ?? []).join("\n")' "$own"); do
    set -- "$@" "$arg"
  done
  expect own "$fixture" "$ROOT" "$@" || failed=1
done
want=
for good in "$FIXTURES"/good-*.json; do
  [ -f "$good" ] || continue
  fixture=$(basename "$good" .json)
  [ -z "${METERING_SELF_TEST_ONLY:-}" ] || [ "$fixture" = "$METERING_SELF_TEST_ONLY" ] || continue
  want=$(node -p '[JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")).line ?? []].flat().join("\n")' "$good")
  fixture_env=$(env_of "$good")
  set --
  for arg in $(node -p '(JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")).args ?? []).map((a) => a === "" ? "-" : a).join("\n")' "$good"); do
    [ "$arg" = - ] && arg=
    set -- "$@" "$arg"
  done
  expect good "$fixture" "$ROOT" "$@" || failed=1
  fixture_env=
done
want=
for mutant in "$FIXTURES"/bad-*.json; do
  [ -f "$mutant" ] || continue
  fixture=$(basename "$mutant" .json)
  [ -z "${METERING_SELF_TEST_ONLY:-}" ] || [ "$fixture" = "$METERING_SELF_TEST_ONLY" ] || continue
  want=$(node -p '[JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")).line].flat().join("\n")' "$mutant")
  fixture_env=$(env_of "$mutant")
  kind=bad
  if [ "$(field "$mutant" checkout)" = true ]; then
    kind=checkout
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
    if expect "$kind" "$fixture" "$copy" "$@"; then
      echo "self-test metering: control $fixture: a wrong expected text read as expected"
      failed=1
    else
      echo "self-test metering: control $fixture: the wrong expected text is refused, as it must be"
    fi
  else
    expect "$kind" "$fixture" "$copy" "$@" || failed=1
  fi
  fixture_env=
done
echo "self-test metering: ran $ran fixtures${narrowed:+, narrowed by $narrowed}"
exit "$failed"
