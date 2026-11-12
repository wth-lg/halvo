port=$((8788 + STACK_PORT_OFFSET))
run() {
  set -- --var "PUBLIC_ORIGIN:http://127.0.0.1:$port"
  provider=$(sh "$ROOT/scripts/stack.sh" url provider-fake 2> /dev/null) ||
    provider=$(sh "$ROOT/scripts/stack.sh" url stripe-mock 2> /dev/null) || provider=
  [ -z "$provider" ] || set -- "$@" --var "PROVIDER_BASE_URL:$provider"
  upstream=$(sh "$ROOT/scripts/stack.sh" url upstream 2> /dev/null) || upstream=
  [ -z "$upstream" ] || set -- "$@" --var "UPSTREAM_BASE_URL:$upstream"
  [ -f "$ROOT/stack.d/vars.env" ] || {
    echo "worker-test: stack.d/vars.env is missing" >&2
    exit 1
  }
  line_number=0
  while IFS= read -r line || [ -n "$line" ]; do
    line_number=$((line_number + 1))
    case "$line" in
      '' | '#'*) continue ;;
    esac
    printf '%s\n' "$line" | grep -qE '^[A-Z][A-Z0-9_]*=[A-Za-z0-9_]+$' || {
      echo "worker-test: stack.d/vars.env line $line_number is not NAME=VALUE" >&2
      exit 1
    }
    set -- "$@" --var "${line%%=*}:${line#*=}"
  done <<EOF
$(LC_ALL=C tr '\000' '?' < "$ROOT/stack.d/vars.env")
EOF
  XDG_CONFIG_HOME="$HALVO_WRANGLER_CONFIG_HOME" exec wrangler dev --config "$ROOT/wrangler.test.jsonc" \
    --ip 127.0.0.1 --port "$port" --inspector-port "$((port + 10000))" --persist-to "$STACK_STATE/worker-test" "$@"
}
