#!/bin/sh
# shellcheck source=gate/lib/stub.sh
. "$(dirname "$0")/../lib/stub.sh"
true # a comment after code on its line
stub code-before-a-comment "never" "${1:-}"
