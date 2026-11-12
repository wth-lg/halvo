#!/bin/sh
set -u
ROOT=$(cd "$(dirname "$0")/../../.." && pwd -P)
. "$ROOT/scripts/env.sh"
PROBES="$ROOT/test/contract/probes"
ALL="var-colon var-wins observability dry-run-modules typify-defs zod-enum-one node-ts vitest-two-projects platform-tables constructor-rearm"

usage() {
  cat >&2 <<'EOF'
Runs the probes of unit 2.0: each shows one fact about a pinned tool on throwaway files that it
writes into a new directory under .tmp/ of the checkout.

  sh test/contract/probes/run.sh         run the ten, in order
  sh test/contract/probes/run.sh NAME    run one

It prints one line for each probe, "PASS probe NAME: ..." or "FAIL probe NAME: ...", and names
the directory of each run on stderr. Exit 0 only when every probe that ran passed, 1 when one
failed, 64 for a name that is no probe. The probes that start wrangler run it through
sh scripts/outbound-hosts.sh run.
EOF
  exit 64
}

is_a_probe() {
  for one in $ALL; do
    [ "$one" != "$1" ] || return 0
  done
  return 1
}

case $# in
  0) names=$ALL ;;
  1) is_a_probe "$1" || usage; names=$1 ;;
  *) usage ;;
esac

mkdir -p "$ROOT/.tmp"
failed=0
for name in $names; do
  dir=$(mktemp -d "$ROOT/.tmp/probe-$name.XXXXXX") || exit 2
  echo "probe $name: files in ${dir#"$ROOT"/}" >&2
  sh "$PROBES/$name/probe.sh" "$dir" > "$dir/result.txt" 2> "$dir/probe.log"
  status=$?
  own_line="^(PASS|FAIL) probe $name: "
  count=$(grep -cE "$own_line" "$dir/result.txt")
  line=$(grep -E "$own_line" "$dir/result.txt" | tail -1)
  case "$status:$count:$line" in
    "0:1:PASS probe $name: "*) printf '%s\n' "$line" ;;
    "1:1:FAIL probe $name: "*) printf '%s\n' "$line"; failed=1 ;;
    *)
      echo "FAIL probe $name: the probe broke its contract (exit $status, $count result lines); its log: ${dir#"$ROOT"/}/probe.log"
      failed=1
      ;;
  esac
done
exit "$failed"
