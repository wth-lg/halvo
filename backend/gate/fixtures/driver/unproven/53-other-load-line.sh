#!/bin/sh
# shellcheck source=gate/lib/stub.sh
. "$(dirname "$0")/../lib/./stub.sh"
stub other-load-line "never" "${1:-}"
