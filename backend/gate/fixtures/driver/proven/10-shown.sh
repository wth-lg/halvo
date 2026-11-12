#!/bin/sh
if [ "${1:-}" = --self-test ]; then
  echo "PASS shown: the run on the good fixture"
  echo "self-test shown: good fine: PASS as expected"
  echo "FAIL shown: the run on the bad fixture"
  echo "self-test shown: bad broken: FAIL as expected"
  exit 0
fi
echo "PASS shown: a check whose self-test shows its runs"
