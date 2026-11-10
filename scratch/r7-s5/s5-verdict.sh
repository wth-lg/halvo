#!/bin/sh
# Prints the S5 rule line of one leg. ci.sh sources this file; tests/s5-verdict-test.sh checks it.
# s5_verdict <runner> <exit code of build 2's read> <code hash of build 1> <code hash of build 2>
# Returns 0 after a MET or MISSED line and 1 when the line does not count.
s5_verdict() {
  v_runner=$1
  v_rc=$2
  v_first=$3
  v_second=$4
  if [ -z "$v_first" ] || [ -z "$v_second" ] || [ "$v_first" = "$v_second" ]; then
    echo "S5 RULE $v_runner: DOES NOT COUNT (the two builds do not have two different code hashes, '$v_first' and '$v_second')"
    return 1
  fi
  case $v_rc in
    0) echo "S5 RULE $v_runner: MET (rebuilt build 2, $v_second, read the item that build 1, $v_first, stored, with user interaction disabled)" ;;
    10 | 12) echo "S5 RULE $v_runner: MISSED (rebuilt build 2, $v_second, could not read the item that build 1, $v_first, stored, with user interaction disabled; read exit $v_rc)" ;;
    *)
      echo "S5 RULE $v_runner: DOES NOT COUNT (build 2's read ended with exit $v_rc)"
      return 1
      ;;
  esac
}
