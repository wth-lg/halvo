#!/bin/sh
# usage: sh tools/r4-trials/driver/run.sh --candidate webdriver|wdio|all | --dialog plugin|system|all
# Makes the cells of the driver and native dialog trial (contracts.md I11.5) on a hosted runner; refuses with not_hosted where neither CI nor R4_GUEST is set.
set -u
here=$(cd "$(dirname "$0")" && pwd)
exec node "$here/run.mjs" "$@"
