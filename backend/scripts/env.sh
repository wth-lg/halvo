: "${ROOT:?set ROOT to the repository root before sourcing scripts/env.sh}"
export HALVO_ROOT="$ROOT"
export PATH="$ROOT/node_modules/.bin:$PATH"
turn_off_wrangler_network_use() {
  export WRANGLER_SEND_METRICS=false
  export WRANGLER_SEND_ERROR_REPORTS=false
  export WRANGLER_HIDE_BANNER=true
}
turn_off_wrangler_network_use
export HALVO_WRANGLER_CONFIG_HOME="$ROOT/.tmp/wrangler-config"
export WRANGLER_LOG_PATH="$HALVO_WRANGLER_CONFIG_HOME/.wrangler/logs"
export WRANGLER_REGISTRY_PATH="$HALVO_WRANGLER_CONFIG_HOME/.wrangler/registry"
if [ -z "${STACK_PORT_OFFSET:-}" ] && [ -s "$ROOT/.stack/port-offset" ]; then
  STACK_PORT_OFFSET=$(sed -n 1p "$ROOT/.stack/port-offset")
fi
PORT_OFFSET_SLOTS=300
PORT_OFFSET_STEP_FOR_THREE_STACKS_OF_TEN_PORTS=30
if [ -z "${STACK_PORT_OFFSET:-}" ]; then
  STACK_PORT_OFFSET=$(($(printf '%s' "$ROOT" | cksum | cut -d ' ' -f 1) % PORT_OFFSET_SLOTS * PORT_OFFSET_STEP_FOR_THREE_STACKS_OF_TEN_PORTS))
fi
export STACK_PORT_OFFSET
if [ "${HALVO_OUTBOUND_CONTROL:-0}" = 1 ]; then
  unset CLOUDFLARE_CF_FETCH_ENABLED
else
  export CLOUDFLARE_CF_FETCH_ENABLED=false
fi
