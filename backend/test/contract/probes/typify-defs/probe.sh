#!/bin/sh
set -u
ROOT=$(cd "$(dirname "$0")/../../../.." && pwd -P)
. "$ROOT/test/contract/probes/lib/common.sh"
CARGO_HOME=${CARGO_HOME:-$HOME/.cargo}
PATH="$CARGO_HOME/bin:$PATH"
DIR=$1
crate="$DIR/crate"
mkdir -p "$crate/src"
cp "$ROOT/spikes/s3/crate/Cargo.toml" "$ROOT/spikes/s3/crate/Cargo.lock" "$crate/"
cp "$ROOT/test/contract/probes/typify-defs/build.rs" "$crate/build.rs"
cp "$ROOT/test/contract/probes/typify-defs/main.rs" "$crate/src/main.rs"
cp "$ROOT/test/contract/probes/typify-defs/schema.json" "$crate/schema.json"
typify=$(sed -n '/^name = "typify"$/{n;s/^version = "\(.*\)"$/\1/p;}' "$ROOT/spikes/s3/crate/Cargo.lock")
version="typify $typify, $(cargo --version 2>&1)"
(cd "$crate" && CARGO_TARGET_DIR="$DIR/target" cargo run --locked --offline --quiet) > "$DIR/generated.rs" 2> "$DIR/cargo.log"
status=$?
cat "$DIR/cargo.log" >&2
if [ "$status" != 0 ]; then
  echo "FAIL probe typify-defs: cargo run --locked --offline exited $status; its log: ${DIR#"$ROOT"/}/cargo.log, $version"
  exit 1
fi
missing=
for type in "struct Inner " "struct Outer " "enum Mode "; do
  grep -q "pub $type" "$DIR/generated.rs" || missing="$missing ${type% }"
done
if [ -z "$missing" ]; then
  echo "PASS probe typify-defs: the root of \$schema and \$defs alone gave one type per key, named by it (Inner, Outer, Mode), $version"
  exit 0
fi
echo "FAIL probe typify-defs: the generated source lacks$missing; it is ${DIR#"$ROOT"/}/generated.rs, $version"
exit 1
