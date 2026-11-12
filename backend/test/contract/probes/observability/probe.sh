#!/bin/sh
set -u
ROOT=$(cd "$(dirname "$0")/../../../.." && pwd -P)
. "$ROOT/test/contract/probes/lib/common.sh"
DIR=$1
version="wrangler $(package_version wrangler)"
mkdir -p "$DIR/control" "$DIR/contract"
cp "$LIB/variables-worker.js" "$DIR/control/index.js"
cp "$LIB/variables-worker.js" "$DIR/contract/index.js"
wrangler_config "$DIR/control/wrangler.jsonc" index.js \
  '{"observability":{"enabled":false,"logs":{"enabled":false,"invocation_log":false}}}'
wrangler_config "$DIR/contract/wrangler.jsonc" index.js \
  '{"observability":{"enabled":false,"logs":{"enabled":false,"invocation_logs":false}}}'

under_outbound_profile "$DIR/control/outbound.log" node "$LIB/serve.mjs" "$DIR/control/wrangler.jsonc" "$DIR/control/state" \
  '[{"vars":[],"paths":["/"]}]' 45
control_exit=$outbound_exit
control_last=$outbound_last
control_denied=$(printf '%s\n' "$control_last" | sed -n 's/^outbound: denied=\([0-9][0-9]*\)$/\1/p')

under_outbound_profile "$DIR/contract/outbound.log" node "$LIB/serve.mjs" "$DIR/contract/wrangler.jsonc" "$DIR/contract/state" \
  '[{"vars":[],"paths":["/"]}]'
contract_started=$(json_field "$(served_result)" 'r.runs[0].started && r.runs[0].answers.length === 1 && r.runs[0].answers[0].status === 200')

control_held=
[ "$control_exit" = 1 ] && [ -n "$control_denied" ] && [ "$control_denied" -gt 0 ] && control_held=1
if [ -n "$control_held" ] && outbound_was_clean && [ "$contract_started" = true ]; then
  echo "PASS probe observability: the block with invocation_log misspelled ended as \"$control_last\" with exit 1; the block of contract 6 started and answered 200 with denied=0, $version"
  exit 0
fi
echo "FAIL probe observability: control leg exit $control_exit, last line \"$control_last\" (expected denied above 0 and exit 1); contract leg $(outbound_state), started and answered: $contract_started, $version"
exit 1
