#!/bin/sh
set -eu
EXIT_THIS_SCRIPT_COULD_NOT_RUN_ITS_STEPS=70
a_failure_of_this_script_is_no_verdict() {
  [ "$1" -eq 0 ] || exit "$EXIT_THIS_SCRIPT_COULD_NOT_RUN_ITS_STEPS"
}
trap 'a_failure_of_this_script_is_no_verdict "$?"' EXIT
ROOT=$(cd "$(dirname "$0")/../.." && pwd -P)
cd "$ROOT"
OUT="$ROOT/.tmp/s3"
mkdir -p "$OUT"
NAMES="extend intersection allof control_open control_map probe_union"

empty_earlier_outputs() {
  : > "$OUT/vitest.log"
  : > "$OUT/vitest.exit"
  : > "$OUT/cargo.version"
  for name in $NAMES; do
    : > "$OUT/cargo-$name.log"
    : > "$OUT/cargo-$name.exit"
  done
}
empty_earlier_outputs

status=0
sh scripts/with-env.sh node_modules/.bin/vitest run --root spikes/s3 --silent=false > "$OUT/vitest.log" 2>&1 || status=$?
printf '%s\n' "$status" > "$OUT/vitest.exit"

CARGO_HOME=${CARGO_HOME:-$HOME/.cargo}
PATH="$CARGO_HOME/bin:$PATH"
for name in $NAMES; do
  status=0
  (cd spikes/s3/crate && S3_SCHEMA="$name" CARGO_TARGET_DIR="$OUT/target" cargo run --locked --offline --quiet) \
    > "$OUT/cargo-$name.log" 2>&1 || status=$?
  printf '%s\n' "$status" > "$OUT/cargo-$name.exit"
done
cargo --version > "$OUT/cargo.version" 2>&1 || true

exec node spikes/s3/judge.mjs "$OUT"
