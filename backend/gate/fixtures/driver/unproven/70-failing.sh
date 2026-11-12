#!/bin/sh
if [ "${1:-}" = --self-test ]; then
  echo "PASS failing: the run on the good fixture"
  echo "self-test failing: good fine: PASS as expected"
  echo "self-test failing: bad broken: FAIL as expected"
  echo "self-test failing: another: the fixture did not hold"
  exit 1
fi
echo "PASS failing: a check whose self-test fails on one of its fixtures"
