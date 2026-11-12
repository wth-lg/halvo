#!/bin/sh
# shellcheck source=gate/lib/stub.sh
. "$(dirname "$0")/../lib/stub.sh"
stub stub-call-and-more "never" "${1:-}" 2>&1
