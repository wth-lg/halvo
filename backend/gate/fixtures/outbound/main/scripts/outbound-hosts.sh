#!/bin/sh
set -u
here=$(cd "$(dirname "$0")/.." && pwd -P)
case "${1:-}" in
  gate)
    cat "$here/$STUB_GATE_OUTPUT"
    exit "$STUB_GATE_EXIT"
    ;;
  control)
    cat "$here/$STUB_CONTROL_OUTPUT"
    exit "$STUB_CONTROL_EXIT"
    ;;
esac
exit 64
