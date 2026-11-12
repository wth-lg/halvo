#!/bin/sh
if [ "${1:-}" = --self-test ]; then
  echo "FAIL bad-only: the run on the bad fixture"
  echo "self-test bad-only: bad broken: FAIL as expected"
  exit 0
fi
echo "PASS bad-only: a check with a bad fixture and no good one"
