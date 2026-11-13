#!/bin/sh
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd -P)
. "$ROOT/scripts/env.sh"
exec "$@"
