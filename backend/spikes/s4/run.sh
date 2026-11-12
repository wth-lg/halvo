#!/bin/sh
set -u
ROOT=$(cd "$(dirname "$0")/../.." && pwd -P)
cd "$ROOT" || exit 2
if [ ! -d spikes/s4/node_modules/better-auth ] || [ ! -d node_modules/playwright ]; then
  echo "s4: packages are missing: run sh scripts/prerequisites.sh first" >&2
  exit 2
fi
mkdir -p .tmp/s4

BETTER_AUTH_TELEMETRY=0
S4_RUN_ID="$$-$(date +%s)"
export BETTER_AUTH_TELEMETRY S4_RUN_ID
unset BETTER_AUTH_TELEMETRY_ENDPOINT BETTER_AUTH_TELEMETRY_DEBUG

sh scripts/outbound-hosts.sh browser node spikes/s4/measure.mjs > .tmp/s4/sandbox.txt 2>&1
if [ $? -ge 128 ]; then
  echo "s4: the measuring step was interrupted; nothing was written" >&2
  exit 2
fi
denied=$(sed -n 's/^outbound: denied=\([0-9]*\)$/\1/p' .tmp/s4/sandbox.txt | tail -1)
grep '^outbound: command: s4: ' .tmp/s4/sandbox.txt | sed 's/^outbound: command: //'
echo "s4: outbound sandbox: denied=${denied:-uncounted} (its full output is in .tmp/s4/sandbox.txt)"

node spikes/s4/report.mjs "${denied:-uncounted}" > .tmp/s4/report.txt
report_status=$?
cat .tmp/s4/report.txt
case "$report_status:$(sed -n 1p .tmp/s4/report.txt)" in
  "0:rule: MET"*) exit 0 ;;
  "1:rule: MISSED"*) exit 1 ;;
esac
echo "s4: the deciding step did not finish (exit $report_status)" >&2
exit 2
