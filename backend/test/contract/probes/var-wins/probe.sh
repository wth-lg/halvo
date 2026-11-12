#!/bin/sh
set -u
ROOT=$(cd "$(dirname "$0")/../../../.." && pwd -P)
. "$ROOT/test/contract/probes/lib/common.sh"
DIR=$1
cp "$LIB/variables-worker.js" "$DIR/index.js"
wrangler_config "$DIR/wrangler.jsonc" index.js '{"vars":{"A":"file"}}'
under_outbound_profile "$DIR/outbound.log" node "$LIB/serve.mjs" "$DIR/wrangler.jsonc" "$DIR/state" \
  '[{"vars":[],"paths":["/"]},{"vars":["A:flag"],"paths":["/"]}]'
version="wrangler $(package_version wrangler)"
if ! outbound_was_clean; then
  echo "FAIL probe var-wins: the run under the outbound profile was not clean ($(outbound_state)), $version"
  exit 1
fi
without=$(json_field "$(served_result)" 'JSON.parse(r.runs[0].answers[0].body).A')
with=$(json_field "$(served_result)" 'JSON.parse(r.runs[1].answers[0].body).A')
if [ "$without" = file ] && [ "$with" = flag ]; then
  echo "PASS probe var-wins: the config declares A as file and the Worker answers $without; with --var A:flag it answers $with, $version"
  exit 0
fi
echo "FAIL probe var-wins: without --var the Worker answered \"$without\" (expected file), with --var A:flag \"$with\" (expected flag), $version"
exit 1
