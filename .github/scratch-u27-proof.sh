#!/bin/sh
# The scratch proof of unit U2.7 of the learning run, on a scratch branch alone and never merged. Run at the root of
# the repository after the ci.d step, as: sh .github/scratch-u27-proof.sh <job> play|loop|selftests
# play: one play of learn-gate through cargo run and the same file run directly, by hand, for three argument lists;
# prints for each whether standard output, standard error and the status are the same, with byte counts.
# loop: each test that the unit adds, 200 rounds of its test binary with --exact; a round counts when its output holds
# the line "test result: ok. 1 passed; 0 failed"; prints the count of each.
# selftests: 10 rounds of the driver's self-test of 50-r5-drift and 55-r5-routine; prints the PASS count of each.
# It prints counts and booleans alone, starts with no -e, and exits 1 at its end when a count is not the wanted one.
set +e
job=$1
part=$2
work=$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/u27.XXXXXX") || exit 1
bad=0

play() {
  cargo build --locked -q -p halvo-learn-gate --bin learn-gate
  printf 'u27 play build status=%s\n' "$?"
  file=${CARGO_TARGET_DIR:-target}/debug/learn-gate
  if [ -f "$file.exe" ]; then
    file=$file.exe
  fi
  n=0
  for args in 'drift --gate dev --cases fixtures/r5/cases/dev --store memory' 'drift --gate dev --cases fixtures/r5/cases/dev --store derived' 'routine --store derived'; do
    n=$((n + 1))
    # shellcheck disable=SC2086
    cargo run --locked -q -p halvo-learn-gate --bin learn-gate -- $args >"$work/c.out" 2>"$work/c.err" </dev/null
    c=$?
    # shellcheck disable=SC2086
    "$file" $args >"$work/d.out" 2>"$work/d.err" </dev/null
    d=$?
    same=no
    if cmp -s "$work/c.out" "$work/d.out" && cmp -s "$work/c.err" "$work/d.err" && [ "$c" = "$d" ]; then
      same=yes
    fi
    [ "$same" = yes ] || bad=1
    printf 'u27 play %s job=%s same=%s status_cargo=%s status_file=%s out_bytes=%s,%s err_bytes=%s,%s\n' "$n" "$job" "$same" "$c" "$d" \
      "$(wc -c <"$work/c.out" | tr -d ' ')" "$(wc -c <"$work/d.out" | tr -d ' ')" "$(wc -c <"$work/c.err" | tr -d ' ')" "$(wc -c <"$work/d.err" | tr -d ' ')"
  done
}

loop() {
  cargo test --locked -p halvo-learn-gate --bin learn-gate --no-run >"$work/no-run.txt" 2>&1
  printf 'u27 loop build status=%s\n' "$?"
  bin=$(tr -d '\r' <"$work/no-run.txt" | sed -n 's/.*Executable unittests src[\\/]main.rs (\(.*\))$/\1/p' | tail -n 1 | tr '\\' '/')
  [ -n "$bin" ] || { printf 'u27 loop binary=none\n'; exit 1; }
  for t in routine_reader_refuses_a_kind_that_is_not_the_kind_of_the_trains_number \
    routine_opens_the_store_and_the_batches_on_every_day_also_after_a_stop_trains_last_occurrence \
    routine_a_play_that_panics_gives_status_2_and_one_line_on_standard_error_with_the_train_and_the_message \
    routine_the_refusal_names_the_first_panic_in_place_order_before_an_earlier_error_on_one_line \
    routine_the_open_of_day_730_removes_the_rows_of_an_occurrence_on_day_637 \
    routine_a_panic_outside_the_plays_while_they_run_goes_to_the_hook_that_was_there_before; do
    ok=0
    start=$(date -u +%s)
    i=0
    while [ "$i" -lt 200 ]; do
      i=$((i + 1))
      "$bin" --exact "tests::routine::$t" >"$work/round.txt" 2>&1 </dev/null
      if grep -q '^test result: ok\. 1 passed; 0 failed' "$work/round.txt"; then
        ok=$((ok + 1))
      fi
    done
    [ "$ok" -eq 200 ] || bad=1
    printf 'u27 loop job=%s test=%s rounds=200 passed=%s seconds=%s\n' "$job" "$t" "$ok" "$(($(date -u +%s) - start))"
  done
}

selftests() {
  for name in 50-r5-drift 55-r5-routine; do
    ok=0
    start=$(date -u +%s)
    i=0
    while [ "$i" -lt 10 ]; do
      i=$((i + 1))
      sh scripts/ci/run-ci-d.sh "$job" --self-test "$name" >"$work/self.txt" 2>&1 </dev/null
      if grep -q "^PASS self-test:$name (" "$work/self.txt"; then
        ok=$((ok + 1))
      fi
    done
    [ "$ok" -eq 10 ] || bad=1
    printf 'u27 self-test job=%s name=%s rounds=10 passed=%s seconds=%s\n' "$job" "$name" "$ok" "$(($(date -u +%s) - start))"
  done
}

case $part in
  play) play ;;
  loop) loop ;;
  selftests) selftests ;;
  *) printf 'u27 usage\n'; exit 64 ;;
esac
printf 'u27 %s job=%s all=%s\n' "$part" "$job" "$([ "$bad" -eq 0 ] && echo yes || echo no)"
exit "$bad"
