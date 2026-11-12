#!/bin/sh
set -eu
cd "$(dirname "$0")/../.."
if [ ! -d node_modules/fast-check ]; then
  echo "run.sh: node_modules/fast-check is missing: run sh scripts/prerequisites.sh first" >&2
  exit 2
fi
exec node spikes/s9/run.mjs "$@"
