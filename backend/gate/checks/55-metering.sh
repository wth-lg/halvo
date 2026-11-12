#!/bin/sh
# shellcheck source=gate/lib/stub.sh
. "$(dirname "$0")/../lib/stub.sh"
stub metering "P5, unit 5.3" "${1:-}"
