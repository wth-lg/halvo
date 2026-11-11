#!/bin/sh
here=$(dirname "$0")
exec node "$here/fetch.mjs" "$@"
