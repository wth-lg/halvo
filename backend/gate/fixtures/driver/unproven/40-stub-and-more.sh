#!/bin/sh
echo "this file does more than call stub" > /dev/null
stub() {
  [ "$3" != --self-test ] || exit 0
  echo "FAIL $1: not built yet ($2)"
  exit 1
}
stub stub-and-more "never" "${1:-}"
