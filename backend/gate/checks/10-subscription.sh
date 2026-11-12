#!/bin/sh
set -u
ROOT=${HALVO_ROOT:-$(cd "$(dirname "$0")/../.." && pwd -P)}
. "$ROOT/scripts/env.sh"
FIXTURES="$ROOT/gate/fixtures/subscription"
STATE=${GATE_STATE:-$ROOT/.tmp}
PARALLEL_COPIES=${SUBSCRIPTION_SELF_TEST_COPIES:-3}
mkdir -p "$STATE"

check_in() {
  HALVO_ROOT=$1 node "$1/test/billing/subscription-check.mjs"
}

if [ "${1:-}" != --self-test ]; then
  check_in "$ROOT"
  exit $?
fi

copy_scripts() {
  env -u STACK_PORT_OFFSET -u STACK_STATE -u HALVO_ROOT -u GATE_STATE "$@"
}

scratch_copy() {
  copy=$(mktemp -d "$STATE/subscription-copy-$1.XXXXXX") || return 1
  (cd "$ROOT" && git ls-files -z --cached --others --exclude-standard | tar --null -T - -cf -) | tar -xf - -C "$copy" || return 1
  ln -s "$ROOT/node_modules" "$copy/node_modules"
  ln -s "$ROOT/.tools" "$copy/.tools"
  printf '%s\n' "$copy"
}

port_is_free() {
  ! lsof -nP -iTCP:"$1" -sTCP:LISTEN > /dev/null 2>&1
}

offset_is_free() {
  case " $taken " in *" $1 "*) return 1 ;; esac
  for base in 8787 8788 12111 12112 12113 18787 18788; do
    port_is_free "$((base + $1))" || return 1
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

expect_line() {
  kind=$1
  fixture=$2
  status=$3
  log=$4
  want=$5
  line=$(grep -E '^(PASS|FAIL) subscription: ' "$log" | tail -1)
  case "$kind:$status:$line" in
    "good:0:PASS subscription: "*)
      echo "self-test subscription: good $fixture: PASS as expected"
      return 0
      ;;
    "bad:1:FAIL subscription: "*)
      missing=$(printf '%s\n' "$want" | while IFS= read -r part; do
        [ -z "$part" ] || printf '%s\n' "$line" | grep -qF -- "$part" || printf '%s;' "$part"
      done)
      if [ -z "$missing" ]; then
        echo "self-test subscription: bad $fixture: FAIL as expected"
        return 0
      fi
      ;;
  esac
  echo "self-test subscription: $kind $fixture: exit $status, line \"$line\"${want:+, expected the text $want}"
  return 1
}

run_bad() {
  mutant=$1
  copy=$2
  fixture=$3
  log="$STATE/subscription-self-test-$fixture.log"
  want=$(node -p '[JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")).line].flat().join("\n")' "$mutant")
  {
    echo "fixture $fixture in $copy, port offset $(cat "$copy/.stack/port-offset")"
    copy_scripts sh "$copy/scripts/stack.sh" up 2>&1
    up=$?
    if [ "$up" -eq 0 ]; then
      copy_scripts sh "$copy/gate/checks/10-subscription.sh"
      echo "check exit $?"
    else
      echo "the copy's stack did not come up (exit $up)"
    fi
    copy_scripts sh "$copy/scripts/stack.sh" down 2>&1
  } > "$log" 2>&1
  status=$(sed -n 's/^check exit //p' "$log" | tail -1)
  expect_line bad "$fixture" "${status:-none}" "$log" "$want" > "$log.result"
}

failed=0
if [ "$(git -C "$ROOT" rev-parse --show-toplevel 2> /dev/null)" != "$ROOT" ]; then
  echo "self-test subscription: the bad fixtures need a git checkout, as the gate does: they copy the files that git lists; $ROOT is not the top of one"
  exit 1
fi
sh "$ROOT/scripts/stack.sh" up > "$STATE/subscription-self-test-stack.log" 2>&1 ||
  echo "self-test subscription: the stack did not come up; its log: $STATE/subscription-self-test-stack.log"
good_log="$STATE/subscription-self-test-checkout.log"
check_in "$ROOT" > "$good_log" 2>&1
good_status=$?
sed 's/^/  /' "$good_log"
expect_line good checkout "$good_status" "$good_log" "" || failed=1

batch=
taken=
running=0
finish_batch() {
  wait
  for fixture in $batch; do
    log="$STATE/subscription-self-test-$fixture.log"
    sed 's/^/  /' "$log"
    if [ -s "$log.result" ]; then
      cat "$log.result"
      grep -q ' as expected$' "$log.result" || failed=1
    else
      echo "self-test subscription: bad $fixture: no result"
      failed=1
    fi
  done
  batch=
  taken=
  running=0
}

for mutant in "$FIXTURES"/bad-*.json; do
  [ -f "$mutant" ] || continue
  fixture=$(basename "$mutant" .json)
  copy=$(scratch_copy "$fixture") || {
    echo "self-test subscription: bad $fixture: the scratch copy could not be made"
    failed=1
    continue
  }
  if ! node "$ROOT/gate/fixtures/contract/apply-mutant.mjs" "$mutant" "$copy"; then
    echo "self-test subscription: bad $fixture: the mutant does not apply to the checkout"
    failed=1
    continue
  fi
  offset=$(free_offset "$copy") || {
    echo "self-test subscription: bad $fixture: no free port offset for the copy's stack"
    failed=1
    continue
  }
  mkdir -p "$copy/.stack"
  printf '%s\n' "$offset" > "$copy/.stack/port-offset"
  taken="$taken $offset"
  batch="$batch $fixture"
  run_bad "$mutant" "$copy" "$fixture" &
  running=$((running + 1))
  [ "$running" -lt "$PARALLEL_COPIES" ] || finish_batch
done
finish_batch
exit "$failed"
