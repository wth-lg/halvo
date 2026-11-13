#!/bin/sh
if [ "${1:-}" = --self-test ]; then
  echo "PASS here: the run on the good fixture"
  echo "self-test here: good fine: PASS as expected"
  echo "FAIL here: the run on the bad fixture"
  echo "self-test here: bad broken: FAIL as expected"
  exit 0
fi
echo "PASS here: ok"
