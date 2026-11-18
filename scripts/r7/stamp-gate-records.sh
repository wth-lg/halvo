#!/bin/sh
set -u

usage() {
  cat >&2 <<'EOF'
Stamps the records of the backend's gate, docs/r7/gate/NN-name.txt, one for each check
backend/gate/checks/NN-name.sh, as scripts/r7/ci.d/22-r7-backend-records.sh reads them.

  sh scripts/r7/stamp-gate-records.sh
  sh scripts/r7/stamp-gate-records.sh --self-test

Run it at the top of the repository on macOS, after sh scripts/prerequisites.sh and
sh backend/scripts/prerequisites.sh, with nothing uncommitted under backend/ or
formal/r7-ledger/. It first runs check 55's memory self-check,
node backend/test-kit/fake-meter/rerun/check.mjs --memory-self-check, and goes on only when
that exits 0 with its PASS line last. It then brings the backend's stack up
(backend/scripts/stack.sh up), runs backend/scripts/gate.sh once, brings the stack down, and
writes for each check the result line that the gate printed for it, the UTC time and the
1-minute load at which that line came, and the trees of backend/ and of formal/r7-ledger/ at
HEAD. Before it writes any record it reads check 55's line with
sh scripts/r7/ci.d/22-r7-backend-records.sh --metering-line, which holds it to the numbers of
contract 9.8. The gate takes about half an hour. It writes nothing else in the repository;
the gate's own logs stay under backend/.tmp/. Commit the 13 records with the change that
made the old ones stale: any commit that changes backend/ or formal/r7-ledger/ does.

With --self-test it runs its cases in throwaway repositories with stand-ins for the stack,
the gate and the memory self-check, and prints PASS self-test:stamp-gate-records last.

Exit 0 when it wrote the 13 records, whatever lines the gate printed but check 55's; 1 when it
wrote none, because the trees were not clean, the memory self-check failed, the stack did
not come up, the trees changed during the run, a check printed no line or more than one, or
check 55's line is not a PASS of the whole plan with the numbers of contract 9.8; 64 on bad
usage.
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

fixture_commit() {
  git -C "$1" -c commit.gpgsign=false -c core.hooksPath=/dev/null commit -q -m "$2" >/dev/null || exit 2
}

stub_repo() {
  dir=$scratch/$1
  mkdir -p "$dir/backend/gate/checks" "$dir/backend/scripts" "$dir/backend/test-kit/fake-meter/rerun" "$dir/formal/r7-ledger" "$dir/scripts/r7/ci.d" || exit 2
  git -C "$dir" init -q || exit 2
  printf 'echo\n' >"$dir/backend/gate/checks/10-subscription.sh"
  printf 'echo\n' >"$dir/backend/gate/checks/55-metering.sh"
  printf 'exit 0\n' >"$dir/backend/scripts/stack.sh"
  cat >"$dir/backend/scripts/gate.sh" <<'EOF'
cat "$STAMP_STUB_GATE"
EOF
  printf 'const how = process.env.STAMP_STUB_MEMORY;\nif (how === "pass") console.log("PASS metering-memory: setting=25 rounds=4");\nif (how === "fail") console.log("FAIL metering-memory: setting=25 rounds=4; first failure: 484 of 484 databases live on");\nprocess.exit(how === "pass" || how === "silent" ? 0 : 1);\n' >"$dir/backend/test-kit/fake-meter/rerun/check.mjs"
  printf 'm\n' >"$dir/formal/r7-ledger/m.tla"
  cp "$records_check" "$dir/scripts/r7/ci.d/22-r7-backend-records.sh" || exit 2
  git -C "$dir" add -A . || exit 2
  fixture_commit "$dir" 'A backend with stand-ins'
  printf 'PASS subscription: active=1\n%s\n' "$2" >"$scratch/$1.gate"
}

stamp_case() {
  want=$1
  case_name=$2
  stub_repo "$case_name" "$3" "$4"
  (cd "$scratch/$case_name" && STAMP_STUB_GATE="$scratch/$case_name.gate" STAMP_STUB_MEMORY=$4 sh "$self" >"$scratch/$case_name.out" 2>&1)
  code=$?
  written=$(find "$scratch/$case_name/docs/r7/gate" -name '*.txt' 2>/dev/null | awk 'END { print NR }')
  case $code:$written in
    0:2) got=stamped ;;
    1:0) got=refused ;;
    *) got="exit-$code-with-$written-records" ;;
  esac
  printf 'case stamp-gate-records/%s want=%s got=%s\n' "$case_name" "$want" "$got"
  if [ "$got" != "$want" ]; then
    wrong="$wrong $case_name"
  elif [ "$want" = refused ]; then
    rejected=$((rejected + 1))
  fi
}

self_test() {
  { git var GIT_AUTHOR_IDENT && git var GIT_COMMITTER_IDENT; } >/dev/null 2>&1 || {
    printf 'FAIL self-test:stamp-gate-records (the self-test commits in throwaway repositories, and git has no name and email for a commit here. Configure a git user, or set the variables GIT_AUTHOR_NAME, GIT_AUTHOR_EMAIL, GIT_COMMITTER_NAME and GIT_COMMITTER_EMAIL.)\n'
    exit 1
  }
  self=$(cd "$(dirname "$0")" && pwd -P)/$(basename "$0")
  records_check=$(dirname "$self")/ci.d/22-r7-backend-records.sh
  scratch=$(mktemp -d "${TMPDIR:-/tmp}/stamp-gate-records-self-test.XXXXXX") || exit 2
  rejected=0
  wrong=
  stamped='PASS metering: plan=whole fake-checks=22 of 22 orderings=100000 of 100000 voided=101957 sequences=1188388 settings=41 of 41 expiry-on-time=passed stripe-mock-accepts=passed refusal-limit=passed send-limit=passed late-read=passed unexplained-total=passed retry-delay=passed'
  stamp_case stamped whole-plan-and-memory-passes "$stamped" pass
  stamp_case refused memory-self-check-fails "$stamped" fail
  stamp_case refused memory-self-check-prints-no-pass-line "$stamped" silent
  stamp_case refused metering-plan-bounded "$(printf '%s\n' "$stamped" | sed 's/plan=whole/plan=bounded/')" pass
  stamp_case refused metering-orderings-one-short "$(printf '%s\n' "$stamped" | sed 's/orderings=100000 of 100000/orderings=99999 of 100000/')" pass
  stamp_case refused metering-fail-line "$(printf '%s\n' "$stamped" | sed 's/^PASS/FAIL/')" pass
  stamp_case refused metering-plan-bounded-later "$stamped plan=bounded" pass
  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:stamp-gate-records (wrong verdict in:%s)\n' "$wrong"
    exit 1
  fi
  printf 'PASS self-test:stamp-gate-records (%s failing fixtures rejected)\n' "$rejected"
  exit 0
}

case $# in
  0) ;;
  1) [ "$1" = --self-test ] || usage; self_test ;;
  *) usage ;;
esac
top=$(git rev-parse --show-toplevel) || refuse 'this is no git checkout'
cd "$top" || refuse "cannot enter $top"
dirty=$(git status --porcelain -- backend formal/r7-ledger | awk 'END { print NR }')
[ "$dirty" = 0 ] || refuse "$dirty paths under backend/ or formal/r7-ledger/ differ from HEAD; commit them first"
tree=$(git rev-parse HEAD:backend) || refuse 'HEAD has no backend/'
model=$(git rev-parse HEAD:formal/r7-ledger) || refuse 'HEAD has no formal/r7-ledger/'
work=$(mktemp -d "${TMPDIR:-/tmp}/stamp-gate-records.XXXXXX") || refuse 'mktemp failed'
(cd backend && HALVO_ROOT="$top/backend" node test-kit/fake-meter/rerun/check.mjs --memory-self-check) >"$work/memory.txt" 2>&1
memory=$?
memory_line=$(awk 'END { print }' "$work/memory.txt")
case $memory:$memory_line in
  "0:PASS metering-memory: "*) printf 'stamp-gate-records: the memory self-check of check 55 passed: %s\n' "$memory_line" ;;
  *) refuse "the memory self-check of check 55 exited $memory with the last line \"$memory_line\"; its output is $work/memory.txt" ;;
esac
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
if [ -f "$work/55-metering.tsv" ]; then
  awk -F '\t' '{ printf "line: %s\n", $3 }' "$work/55-metering.tsv" >"$work/55-metering.line" || refuse 'cannot read the line of check 55'
  sh scripts/r7/ci.d/22-r7-backend-records.sh --metering-line "$work/55-metering.line" >"$work/55-metering.verdict" 2>&1 ||
    refuse "$(awk 'END { print }' "$work/55-metering.verdict")"
fi
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
