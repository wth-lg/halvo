#!/bin/sh
set -u
ROOT=$(cd "$(dirname "$0")/../../../.." && pwd -P)
. "$ROOT/test/contract/probes/lib/common.sh"
DIR=$1
VALUE=http://127.0.0.1:8787
cp "$LIB/variables-worker.js" "$DIR/index.js"
wrangler_config "$DIR/wrangler.jsonc" index.js '{}'
under_outbound_profile "$DIR/outbound.log" node "$LIB/serve.mjs" "$DIR/wrangler.jsonc" "$DIR/state" \
  "[{\"vars\":[\"ORIGIN:$VALUE\"],\"paths\":[\"/\"]}]"
version="wrangler $(package_version wrangler)"
if ! outbound_was_clean; then
  echo "FAIL probe var-colon: the run under the outbound profile was not clean ($(outbound_state)), $version"
  exit 1
fi
got=$(json_field "$(served_result)" 'JSON.parse(r.runs[0].answers[0].body).ORIGIN')
if [ "$got" = "$VALUE" ]; then
  echo "PASS probe var-colon: --var ORIGIN:$VALUE arrived whole as $got, $version"
  exit 0
fi
echo "FAIL probe var-colon: --var ORIGIN:$VALUE arrived as \"$got\", $version"
exit 1
