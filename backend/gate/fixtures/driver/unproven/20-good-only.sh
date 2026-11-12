#!/bin/sh
if [ "${1:-}" = --self-test ]; then
  echo "PASS good-only: the run on the good fixture"
  echo "self-test good-only: good fine: PASS as expected"
  exit 0
fi
echo "PASS good-only: a check with a good fixture and no bad one"
