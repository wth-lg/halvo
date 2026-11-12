#!/bin/sh
set -u
ROOT=$(cd "$(dirname "$0")/../../../.." && pwd -P)
. "$ROOT/test/contract/probes/lib/common.sh"
DIR=$1
version="wrangler $(package_version wrangler)"
cp "$ROOT/test/contract/probes/constructor-rearm/worker.js.in" "$DIR/index.js"
wrangler_config "$DIR/wrangler.jsonc" index.js \
  '{"durable_objects":{"bindings":[{"name":"PROBE","class_name":"Probe"}]},"migrations":[{"tag":"v1","new_sqlite_classes":["Probe"]}]}'
under_outbound_profile "$DIR/outbound.log" node "$LIB/serve.mjs" "$DIR/wrangler.jsonc" "$DIR/state" \
  '[{"vars":[],"paths":["/store"]},{"vars":[],"paths":["/alarm"]}]'
if ! outbound_was_clean; then
  echo "FAIL probe constructor-rearm: the runs under the outbound profile were not clean ($(outbound_state)), $version"
  exit 1
fi
answer=$(served_result)
stored=$(json_field "$answer" 'JSON.parse(r.runs[0].answers[0].body).stored')
deleted=$(json_field "$answer" 'JSON.parse(r.runs[0].answers[0].body).alarm')
after=$(json_field "$answer" 'JSON.parse(r.runs[1].answers[0].body).alarm')
if [ "$deleted" = null ] && [ -n "$stored" ] && [ "$after" = "$stored" ]; then
  echo "PASS probe constructor-rearm: after a restart on the same state the first request read the alarm at $after, the stored time, which only the constructor set (it was deleted before the restart), $version"
  exit 0
fi
echo "FAIL probe constructor-rearm: stored $stored, alarm before the restart $deleted (expected null), first read after it $after, $version"
exit 1
