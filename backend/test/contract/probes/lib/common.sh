. "$ROOT/scripts/env.sh"
LIB="$ROOT/test/contract/probes/lib"

package_version() {
  node -p 'require(process.argv[1]).version' "$ROOT/node_modules/$1/package.json"
}

wrangler_config() {
  node "$LIB/config.mjs" "$1" "$ROOT" "$2" "$3"
}

under_outbound_profile() {
  outbound_log=$1
  shift
  sh "$ROOT/scripts/outbound-hosts.sh" run "$@" > "$outbound_log" 2>&1
  outbound_exit=$?
  outbound_last=$(tail -1 "$outbound_log")
  command_exit=$(sed -n 's/^outbound: command exit \([0-9][0-9]*\)$/\1/p' "$outbound_log" | tail -1)
  cat "$outbound_log" >&2
}

outbound_was_clean() {
  [ "$outbound_exit" = 0 ] && [ "$outbound_last" = "outbound: denied=0" ] && [ "$command_exit" = 0 ]
}

outbound_state() {
  printf 'outbound exit %s, last line "%s", command exit %s' "$outbound_exit" "$outbound_last" "${command_exit:-none}"
}

served_result() {
  sed -n 's/^outbound: command: PROBE-RESULT //p' "$outbound_log" | tail -1
}

json_field() {
  node -e 'const v = new Function("r", "return " + process.argv[2])(JSON.parse(process.argv[1])); process.stdout.write(typeof v === "string" ? v : JSON.stringify(v));' "$1" "$2"
}
