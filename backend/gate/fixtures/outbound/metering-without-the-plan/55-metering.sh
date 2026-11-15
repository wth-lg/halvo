#!/bin/sh
set -u
ROOT=$HALVO_ROOT
. "$ROOT/scripts/env.sh"
unset HALVO_METERING_PLAN
HALVO_ROOT=$ROOT node "$ROOT/test-kit/fake-meter/rerun/check.mjs" --seeds "" --settings 25
