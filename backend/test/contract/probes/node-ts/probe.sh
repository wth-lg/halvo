#!/bin/sh
set -u
ROOT=$(cd "$(dirname "$0")/../../../.." && pwd -P)
. "$ROOT/test/contract/probes/lib/common.sh"
DIR=$1
version="node $(node --version), tsc $("$ROOT/node_modules/.bin/tsc" --version | sed 's/^Version //')"
cp "$ROOT/test/contract/probes/node-ts/shape.ts" "$ROOT/test/contract/probes/node-ts/main.ts" "$DIR/"
(cd "$DIR" && node main.ts) > "$DIR/node.out" 2>&1
node_status=$?
cat "$DIR/node.out" >&2
if [ "$node_status" != 0 ] || [ "$(cat "$DIR/node.out")" != "node-ts 42" ]; then
  echo "FAIL probe node-ts: node main.ts exited $node_status and printed \"$(tr '\n' ' ' < "$DIR/node.out")\", $version"
  exit 1
fi
relative_root=$(node -p 'require("path").relative(process.argv[1], process.argv[2])' "$DIR" "$ROOT")
printf '{ "extends": "%s/tsconfig.json", "compilerOptions": { "allowImportingTsExtensions": false }, "include": ["shape.ts", "main.ts"] }\n' "$relative_root" > "$DIR/tsconfig.json"
printf '{ "extends": "%s/tsconfig.json", "compilerOptions": { "allowImportingTsExtensions": true }, "include": ["shape.ts", "main.ts"] }\n' "$relative_root" > "$DIR/tsconfig.key.json"
"$ROOT/node_modules/.bin/tsc" --noEmit -p "$DIR/tsconfig.json" > "$DIR/tsc.out" 2>&1
without=$?
"$ROOT/node_modules/.bin/tsc" --noEmit -p "$DIR/tsconfig.key.json" > "$DIR/tsc.key.out" 2>&1
with=$?
cat "$DIR/tsc.out" "$DIR/tsc.key.out" >&2
if [ "$without" = 0 ]; then
  echo "PASS probe node-ts: node runs the two sources without a flag, and tsc accepts their .ts import paths without allowImportingTsExtensions, $version"
  exit 0
fi
if [ "$with" = 0 ]; then
  echo "PASS probe node-ts: node runs the two sources without a flag; tsc refuses the .ts import paths without allowImportingTsExtensions ($(grep -o 'error TS[0-9]*' "$DIR/tsc.out" | sort -u | tr '\n' ' ' | sed 's/ $//')) and accepts them with it, $version"
  exit 0
fi
echo "FAIL probe node-ts: tsc refuses the sources also with allowImportingTsExtensions: $(head -3 "$DIR/tsc.key.out" | tr '\n' ' '), $version"
exit 1
