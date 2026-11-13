#!/bin/sh
# shellcheck source=gate/lib/stub.sh
. "$(dirname "$0")/../lib/stub.sh"
stub not-built "a later unit" "${1:-}"
