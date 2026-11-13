#!/bin/sh
# usage: sh tools/r4-trials/driver/read.sh --candidate webdriver|wdio | --dialog plugin|system | --attrs
# Prints the reading lines of the driver and native dialog trial (contracts.md I11.5) on this Mac; starts no app.
set -u
here=$(cd "$(dirname "$0")" && pwd)
exec node "$here/read.mjs" "$@"
