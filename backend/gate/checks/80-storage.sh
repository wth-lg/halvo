#!/bin/sh
# shellcheck source=gate/lib/stub.sh
. "$(dirname "$0")/../lib/stub.sh"
stub storage "P6" "${1:-}"
