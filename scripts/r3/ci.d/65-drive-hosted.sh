#!/bin/sh
# jobs: windows
# rule: The hosted form of the driver trial (r3 U0.8) holds against its scripted host on 127.0.0.1: the tests of tools/r3-trials/drive pass, its three whole rounds pass (a cell past the ceiling sends nothing, no request holds a word of a stored step, the key stands in no line and no file of a cell), the paid form of hosted.sh refuses with REFUSED prices on the fixture rates of the build before it starts anything, and hosted.sh --standin reads as expected with the door under the egress check; no run reaches a host.
# lane: alone
# limit: 2400
set -u
name=65-drive-hosted
rounds='tests::a_cell_past_the_ceiling_ends_notrun_and_sends_nothing tests::no_request_of_a_cell_holds_a_word_of_a_stored_step tests::the_key_stands_in_no_line_and_no_file_of_a_cell'

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

# The passed and failed counts of the test result lines of the file $1.
counts() {
  awk '/^test result: / { for (i = 1; i < NF; i++) { if ($(i + 1) == "passed;") p += $i; if ($(i + 1) == "failed;") f += $i } } END { print p + 0, f + 0 }' "$1"
}

# The defect of one case: $1 the case, $2 its exit status, $3 its output (for paid: its standard
# error), $4 for paid its standard output and $5 the number of entries of its TMPDIR; none when
# the case holds.
defect_of() {
  case $1 in
    tests)
      c=$(counts "$3")
      [ "$2" -eq 0 ] && [ "${c#* }" = 0 ] && [ "${c% *}" -gt 0 ] && return 0
      printf 'the tests of tools/r3-trials/drive exited %s with %s passed and %s failed: read %s/tests.txt of the artifacts\n' "$2" "${c% *}" "${c#* }" "$name"
      return 1
      ;;
    rounds)
      c=$(counts "$3")
      [ "$2" -eq 0 ] && [ "$c" = '3 0' ] && return 0
      printf 'the three whole rounds exited %s with %s passed and %s failed, not 3 and 0: read %s/rounds.txt of the artifacts\n' "$2" "${c% *}" "${c#* }" "$name"
      return 1
      ;;
    paid)
      if [ "$2" -eq 2 ] && [ ! -s "$4" ] && [ "$5" -eq 0 ] &&
        grep -q '^REFUSED prices: the rate of the tier frontier in crates/halvo-route/data/prices.tsv has the source fixture:' "$3"; then
        return 0
      fi
      printf 'the paid form on the fixture rates exited %s, wrote %s bytes on standard output and %s entries in its TMPDIR, and is no REFUSED prices: read %s/paid-err.txt of the artifacts\n' "$2" "$(wc -c <"$4" | tr -d ' ')" "$5" "$name"
      return 1
      ;;
    standin)
      last=$(awk 'END { print }' "$3")
      case $2:$last in
        '0:control mode=hosted machine='*' result=as_expected') return 0 ;;
      esac
      printf 'hosted.sh --standin exited %s and its last line is no control line with result=as_expected: read %s/standin.txt of the artifacts\n' "$2" "$name"
      return 1
      ;;
  esac
  return 1
}

seconds() {
  date +%s
}

measure() {
  cd "$CI_D_REPO" || exit 2
  [ -n "${HALVO_BROWSER:-}" ] && [ -f "$HALVO_BROWSER" ] || fail "HALVO_BROWSER names no browser; the job's step gives it"
  a=$(seconds)
  (cd tools/r3-trials/drive && cargo test --locked) >"$CI_D_ARTIFACTS/tests.txt" 2>&1
  code=$?
  b=$(seconds)
  defect=$(defect_of tests "$code" "$CI_D_ARTIFACTS/tests.txt") || fail "$defect"
  printf 'case %s/tests passed=%s seconds=%s\n' "$name" "$(counts "$CI_D_ARTIFACTS/tests.txt" | cut -d' ' -f1)" "$((b - a))"
  # shellcheck disable=SC2086
  (cd tools/r3-trials/drive && cargo test --locked -- --ignored --exact --test-threads=1 $rounds) >"$CI_D_ARTIFACTS/rounds.txt" 2>&1
  code=$?
  c=$(seconds)
  defect=$(defect_of rounds "$code" "$CI_D_ARTIFACTS/rounds.txt") || fail "$defect"
  printf 'case %s/rounds passed=3 seconds=%s\n' "$name" "$((c - b))"
  empty=$(mktemp -d "${TMPDIR:-/tmp}/$name-paid.XXXXXX") || exit 2
  env PATH=/usr/bin:/bin TMPDIR="$empty" sh tools/r3-trials/drive/hosted.sh --tier frontier --ceiling 2000 \
    >"$CI_D_ARTIFACTS/paid-out.txt" 2>"$CI_D_ARTIFACTS/paid-err.txt"
  code=$?
  entries=$(find "$empty" -mindepth 1 | wc -l | tr -d ' ')
  defect=$(defect_of paid "$code" "$CI_D_ARTIFACTS/paid-err.txt" "$CI_D_ARTIFACTS/paid-out.txt" "$entries") || fail "$defect"
  printf 'case %s/paid-refuses-on-fixture-rates exit=%s tmp_entries=%s\n' "$name" "$code" "$entries"
  d=$(seconds)
  sh tools/r3-trials/drive/hosted.sh --standin --tier frontier --ceiling 2000 --browser "$HALVO_BROWSER" --port 18471 \
    >"$CI_D_ARTIFACTS/standin.txt" 2>"$CI_D_ARTIFACTS/standin-err.txt"
  code=$?
  e=$(seconds)
  defect=$(defect_of standin "$code" "$CI_D_ARTIFACTS/standin.txt") || fail "$defect"
  cells=$(awk '/^round cell task=/ { n++; for (i = 1; i <= NF; i++) if ($i ~ /^ms=/) { ms = substr($i, 4); s += ms; if (n == 1) one = ms } } END { printf "%d %.1f %.1f", n, s / 1000, one / 1000 }' "$CI_D_ARTIFACTS/standin-err.txt")
  # shellcheck disable=SC2086
  set -- $cells
  printf 'case %s/standin cells=%s nine_cells_s=%s one_cell_s=%s whole_s=%s\n' "$name" "$1" "$2" "$3" "$((e - d))"
  printf 'PASS %s\n' "$name"
}

case_of() {
  printf '%s\n' "$4" >"$work/$1.txt"
  : >"$work/$1-out.txt"
  got=PASS
  defect_of "$2" "$3" "$work/$1.txt" "$work/$1-out.txt" "${6:-0}" >/dev/null || got=FAIL
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$5" "$got"
  if [ "$got" != "$5" ]; then wrong="$wrong $1"; elif [ "$5" = FAIL ]; then rejected=$((rejected + 1)); fi
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  rejected=0
  wrong=
  case_of tests-pass tests 0 "test result: ok. 101 passed; 0 failed; 3 ignored; 0 measured; 0 filtered out; finished in 1.20s
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s" PASS
  case_of tests-one-failed tests 101 "test result: FAILED. 100 passed; 1 failed; 3 ignored; 0 measured; 0 filtered out; finished in 1.20s" FAIL
  case_of rounds-pass rounds 0 "test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 101 filtered out; finished in 25.09s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 6 filtered out; finished in 0.00s" PASS
  case_of rounds-a-filter-that-ran-none rounds 0 "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 104 filtered out; finished in 0.00s" FAIL
  case_of paid-refused paid 2 "REFUSED prices: the rate of the tier frontier in crates/halvo-route/data/prices.tsv has the source fixture:B-3.2, a fixture's: the paid round starts only on the host's own rates with their date" PASS
  case_of paid-refused-for-another-cause paid 2 "REFUSED usage: --key-file names no file" FAIL
  case_of paid-made-a-directory paid 2 "REFUSED prices: the rate of the tier frontier in crates/halvo-route/data/prices.tsv has the source fixture:B-3.2, a fixture's: the paid round starts only on the host's own rates with their date" FAIL 1
  case_of standin-as-expected standin 0 "spend tier=frontier requests=45 spent_micro=387713 ceiling_minor=2000
control mode=hosted machine=hosted-win result=as_expected" PASS
  case_of standin-differs standin 1 "control mode=hosted machine=hosted-win result=differs" FAIL
  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  '') measure ;;
  *) exit 64 ;;
esac
