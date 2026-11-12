#!/bin/sh
out=$(node "$HALVO_ROOT/gate/fixtures/outbound/attempt-browser.mjs" loopback 2>&1)
case "$out" in
  *"browser page: loaded"*) echo "PASS browser-page: a page loaded in the browser that the outbound check started" ;;
  *)
    echo "FAIL browser-page: $(printf '%s\n' "$out" | sed -n 1p)"
    exit 1
    ;;
esac
