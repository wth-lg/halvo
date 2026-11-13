#!/bin/sh
# scratch-loop-k.sh: K3's test of halvo-learn-gate in 200 rounds and the self-tests of 20-r5-rule and 50-r5-drift in
# 10 rounds each; prints the counts of passes and fails when one is short. Used on a scratch branch alone.
set -u
k3=tests::delete::delete_line_reads_other_served_no_when_the_other_workflow_s_reverted_row_is_taken
cargo test --locked -p halvo-learn-gate --bin learn-gate --no-run >/dev/null 2>&1 || exit 1
ok=0
i=0
while [ "$i" -lt 200 ]; do
  i=$((i + 1))
  out=$(cargo test --locked -p halvo-learn-gate --bin learn-gate -- --exact "$k3" 2>/dev/null) || continue
  case $out in
    *"test $k3 ... ok"*) ok=$((ok + 1)) ;;
  esac
done
printf 'loop test=%s ok=%d of=200\n' "$k3" "$ok"
bad=0
[ "$ok" -eq 200 ] || bad=1
for s in 20-r5-rule 50-r5-drift; do
  ok=0
  i=0
  while [ "$i" -lt 10 ]; do
    i=$((i + 1))
    out=$(sh "scripts/r5/ci.d/$s.sh" --self-test 2>/dev/null) || continue
    case $out in
      *"PASS self-test:$s "*) ok=$((ok + 1)) ;;
    esac
  done
  printf 'loop self-test=%s ok=%d of=10\n' "$s" "$ok"
  [ "$ok" -eq 10 ] || bad=1
done
exit "$bad"
