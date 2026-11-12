#!/bin/sh
set -u

usage() {
  cat >&2 <<'EOF'
Stamps the records of the backend's gate, docs/r7/gate/NN-name.txt, one for each check
backend/gate/checks/NN-name.sh, as scripts/r7/ci.d/22-r7-backend-records.sh reads them.

  sh scripts/r7/stamp-gate-records.sh

Run it at the top of the repository on macOS, after sh scripts/prerequisites.sh and
sh backend/scripts/prerequisites.sh, with nothing uncommitted under backend/ or
formal/r7-ledger/. It brings the backend's stack up (backend/scripts/stack.sh up), runs
backend/scripts/gate.sh once, brings the stack down, and writes for each check the result
line that the gate printed for it, the UTC time and the 1-minute load at which that line
came, and the trees of backend/ and of formal/r7-ledger/ at HEAD. The gate takes about half
an hour. It writes nothing else in the repository; the gate's own logs stay under
backend/.tmp/. Commit the 13 records with the change that made the old ones stale: any
commit that changes backend/ or formal/r7-ledger/ does.

Exit 0 when it wrote the 13 records, whatever lines the gate printed; 1 when it wrote none,
because the trees were not clean, the stack did not come up, the trees changed during the
run, or a check printed no line or more than one; 64 on bad usage.
EOF
  exit 64
}

refuse() {
  printf 'stamp-gate-records: %s; no record written\n' "$1"
  exit 1
}

load1() {
  if [ -r /proc/loadavg ]; then
    cut -d ' ' -f 1 /proc/loadavg
  else
    sysctl -n vm.loadavg | awk '{ print $2 }'
  fi
}

[ $# -eq 0 ] || usage
top=$(git rev-parse --show-toplevel) || refuse 'this is no git checkout'
cd "$top" || refuse "cannot enter $top"
dirty=$(git status --porcelain -- backend formal/r7-ledger | awk 'END { print NR }')
[ "$dirty" = 0 ] || refuse "$dirty paths under backend/ or formal/r7-ledger/ differ from HEAD; commit them first"
tree=$(git rev-parse HEAD:backend) || refuse 'HEAD has no backend/'
model=$(git rev-parse HEAD:formal/r7-ledger) || refuse 'HEAD has no formal/r7-ledger/'
work=$(mktemp -d "${TMPDIR:-/tmp}/stamp-gate-records.XXXXXX") || refuse 'mktemp failed'
start=$(date +%s)
printf 'stamp-gate-records: backend/ %s, formal/r7-ledger/ %s; the gate runs, its output is under %s\n' "$tree" "$model" "$work"
if ! sh backend/scripts/stack.sh up >"$work/stack-up.txt" 2>&1; then
  sh backend/scripts/stack.sh down >"$work/stack-down.txt" 2>&1
  refuse "the stack did not come up; its output is $work/stack-up.txt"
fi
{
  sh backend/scripts/gate.sh 2>"$work/gate-err.txt"
  printf '%s\n' "$?" >"$work/gate-exit.txt"
} | while IFS= read -r line; do
  printf '%s\t%s\t%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$(load1)" "$line"
done >"$work/stamped.txt"
sh backend/scripts/stack.sh down >"$work/stack-down.txt" 2>&1
seconds=$(($(date +%s) - start))
[ "$(git rev-parse HEAD:backend)" = "$tree" ] && [ "$(git rev-parse HEAD:formal/r7-ledger)" = "$model" ] ||
  refuse 'HEAD moved during the run'
dirty=$(git status --porcelain -- backend formal/r7-ledger | awk 'END { print NR }')
[ "$dirty" = 0 ] || refuse "$dirty paths under backend/ or formal/r7-ledger/ changed during the run"
git -c core.quotepath=off ls-files -- 'backend/gate/checks/' |
  sed -n 's|^backend/gate/checks/\([0-9][0-9]-[a-z0-9-]*\)\.sh$|\1|p' >"$work/checks.txt"
[ -s "$work/checks.txt" ] || refuse 'git lists no check under backend/gate/checks/'
while IFS= read -r check; do
  awk -F '\t' -v name="${check#??-}" '$3 ~ ("^(PASS|FAIL) " name ": ")' "$work/stamped.txt" >"$work/$check.tsv"
  lines=$(awk 'END { print NR }' "$work/$check.tsv")
  [ "$lines" = 1 ] || refuse "the gate printed $lines result lines for $check"
done <"$work/checks.txt"
mkdir -p docs/r7/gate || refuse 'cannot make docs/r7/gate'
written=0
while IFS= read -r check; do
  awk -F '\t' -v tree="$tree" -v model="$model" \
    '{ printf "line: %s\nstamp: %s\nload: %s\ntree: %s\nmodel: %s\n", $3, $1, $2, tree, model }' \
    "$work/$check.tsv" >"docs/r7/gate/$check.txt" || refuse "cannot write docs/r7/gate/$check.txt"
  written=$((written + 1))
done <"$work/checks.txt"
printf 'stamp-gate-records: wrote %s records in %s s; the gate exited %s with %s PASS and %s FAIL lines\n' \
  "$written" "$seconds" "$(cat "$work/gate-exit.txt")" \
  "$(grep -c -E '^line: PASS ' docs/r7/gate/*.txt | awk -F: '{ n += $2 } END { print n + 0 }')" \
  "$(grep -c -E '^line: FAIL ' docs/r7/gate/*.txt | awk -F: '{ n += $2 } END { print n + 0 }')"
