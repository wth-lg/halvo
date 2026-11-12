port=$((8791 + STACK_PORT_OFFSET))
run() {
  exec node "$ROOT/stress/upstream.mjs" "$STACK_STATE/upstream-$$.jsonl" "$port"
}
