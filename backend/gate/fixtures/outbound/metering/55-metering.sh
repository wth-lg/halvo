#!/bin/sh
set -u
ROOT=$HALVO_ROOT
. "$ROOT/scripts/env.sh"
HALVO_ROOT=$ROOT node "$ROOT/test-kit/fake-meter/rerun/check.mjs" --seeds "" --settings 25
