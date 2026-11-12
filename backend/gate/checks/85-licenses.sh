#!/bin/sh
set -eu
ROOT=${HALVO_ROOT:-$(cd "$(dirname "$0")/../.." && pwd -P)}
. "$ROOT/scripts/env.sh"
exec node "$ROOT/gate/lib/licenses.mjs" "$@"
