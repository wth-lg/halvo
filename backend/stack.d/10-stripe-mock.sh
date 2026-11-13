port=$((12111 + STACK_PORT_OFFSET))
run() {
  exec "$ROOT/.tools/stripe-mock/stripe-mock" -http-addr "127.0.0.1:$port" -https-addr "127.0.0.1:$((port + 1))"
}
