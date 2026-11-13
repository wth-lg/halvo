#!/bin/sh
set -u
ROOT=$(cd "$(dirname "$0")/../../../.." && pwd -P)
. "$ROOT/test/contract/probes/lib/common.sh"
HERE="$ROOT/test/contract/probes/vitest-two-projects"
DIR=$1
version="vitest $(package_version vitest), @cloudflare/vitest-plugin $(package_version @cloudflare/vitest-plugin), node $(node --version)"
mkdir -p "$DIR/tests"
cp "$HERE/vitest.config.mjs.in" "$DIR/vitest.config.mjs"
cp "$HERE/worker.js.in" "$DIR/worker.js"
cp "$HERE/workerd.test.ts.in" "$DIR/tests/workerd.test.ts"
cp "$HERE/sqlite.node.test.ts.in" "$DIR/tests/sqlite.node.test.ts"
wrangler_config "$DIR/wrangler.jsonc" worker.js '{}'
(cd "$DIR" && XDG_CONFIG_HOME="$HALVO_WRANGLER_CONFIG_HOME" "$ROOT/node_modules/.bin/vitest" run --root "$DIR" \
  --reporter=verbose --reporter=json --outputFile.json="$DIR/report.json") > "$DIR/vitest.out" 2>&1
status=$?
cat "$DIR/vitest.out" >&2
if [ "$status" != 0 ] || [ ! -s "$DIR/report.json" ]; then
  echo "FAIL probe vitest-two-projects: vitest run exited $status; its output: ${DIR#"$ROOT"/}/vitest.out, $version"
  exit 1
fi
verdict=$(json_field "$(cat "$DIR/report.json")" 'r.numTotalTests === 2 && r.numPassedTests === 2 && r.testResults.map((f) => f.name.split("/").pop() + ":" + f.status).sort().join(" ")')
if [ "$verdict" = "sqlite.node.test.ts:passed workerd.test.ts:passed" ]; then
  projects=$(grep -oE '\|(workerd|node)\|' "$DIR/vitest.out" | sort -u | tr -d '|' | tr '\n' ' ')
  echo "PASS probe vitest-two-projects: one vitest run ran and passed the workerd test and the node:sqlite test (projects seen: ${projects% }), $version"
  exit 0
fi
echo "FAIL probe vitest-two-projects: the report says $verdict, $version"
exit 1
