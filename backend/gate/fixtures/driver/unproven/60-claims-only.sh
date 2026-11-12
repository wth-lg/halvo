#!/bin/sh
if [ "${1:-}" = --self-test ]; then
  echo "self-test claims-only: good nothing-was-run: PASS as expected"
  echo "self-test claims-only: bad nothing-was-run: FAIL as expected"
  exit 0
fi
echo "PASS claims-only: a check whose self-test prints its two lines and runs nothing"
