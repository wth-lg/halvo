port=$((12113 + STACK_PORT_OFFSET))
run() {
  mock=$(sh "$ROOT/scripts/stack.sh" url stripe-mock)
  worker=$(sh "$ROOT/scripts/stack.sh" url worker)
  worker_test=$(sh "$ROOT/scripts/stack.sh" url worker-test)
  exec node "$ROOT/test-kit/provider-fake/main.mjs" --port "$port" --mock "$mock" \
    --entry "$worker" --entry "$worker_test" --vars "$ROOT/stack.d/vars.env"
}
