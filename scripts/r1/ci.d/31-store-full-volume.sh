#!/bin/sh
# jobs: macos
# rule: On a disk image that the check fills, delete-all of a store in each of the scenarios A, B and C ends with the store's root file and store.key gone.
set -u
name=31-store-full-volume
test_name=store::hardening_tests::a_store_on_a_full_volume_ends_delete_all_with_no_root_and_no_key_file

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

verdict_defect() {
  [ "$1" -eq 0 ] || { printf 'cargo test exited %s in scenario %s\n' "$1" "$3"; return 1; }
  counts=$(awk '/^test result: / { for (i = 1; i < NF; i++) { if ($(i + 1) == "passed;") p += $i; if ($(i + 1) == "failed;") f += $i } } END { print p + 0, f + 0 }' "$2")
  [ "$counts" = '1 0' ] || { printf 'the filter ran %s passed and %s failed tests in scenario %s\n' "${counts% *}" "${counts#* }" "$3"; return 1; }
  line=$(grep -o "scenario=$3 delete_all=[^ ]* root_gone=[a-z]* key_file_gone=[a-z]*" "$2" | head -n 1)
  [ -n "$line" ] || { printf 'scenario %s printed no line of its end\n' "$3"; return 1; }
  case $line in
    *' root_gone=yes key_file_gone=yes') return 0 ;;
  esac
  printf 'scenario %s ended with %s\n' "$3" "${line#* delete_all=}"
  return 1
}

detach() {
  hdiutil detach "$1" > /dev/null 2>&1 || hdiutil detach -force "$1" > /dev/null 2>&1
}

measure() {
  cd "$CI_D_REPO" || exit 2
  git diff --quiet HEAD -- || fail 'the checkout differs from HEAD before the run'
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  hdiutil create -quiet -size 48m -fs APFS -volname halvo-full -layout NONE "$work/volume.dmg" || fail 'hdiutil could not make the disk image'
  mkdir "$work/mnt" || exit 2
  hdiutil attach -quiet -nobrowse -mountpoint "$work/mnt" "$work/volume.dmg" || fail 'hdiutil could not attach the disk image'
  found=
  for scenario in A B C; do
    HALVO_FULL_VOLUME=$work/mnt HALVO_FULL_SCENARIO=$scenario cargo test --locked -p halvo-capture --lib -- --exact "$test_name" --nocapture > "$CI_D_ARTIFACTS/scenario-$scenario.txt" 2>&1
    code=$?
    grep -o "scenario=$scenario [a-z_]*=[^ ]*.*" "$CI_D_ARTIFACTS/scenario-$scenario.txt" | head -n 3
    defect=$(verdict_defect "$code" "$CI_D_ARTIFACTS/scenario-$scenario.txt" "$scenario") || found="$found $defect;"
  done
  detach "$work/mnt"
  git diff --quiet HEAD -- || fail 'a tracked file differs from HEAD after the run'
  [ -z "$found" ] || fail "${found# }"
  printf 'PASS %s\n' "$name"
}

case_of() {
  printf '%s\n' "$4" > "$work/$1"
  got=PASS
  verdict_defect "$2" "$work/$1" "$3" > /dev/null || got=FAIL
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$5" "$got"
  if [ "$got" != "$5" ]; then wrong="$wrong $1"; elif [ "$5" = FAIL ]; then rejected=$((rejected + 1)); fi
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  rejected=0
  wrong=
  result='test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 420 filtered out; finished in 2.10s'
  case_of all-gone 0 B "scenario=B delete_all=Ok(DeleteReport root_gone=yes key_file_gone=yes
$result" PASS
  case_of root-left 0 A "scenario=A delete_all=Err(DeleteIncomplete(DeleteReport root_gone=no key_file_gone=yes
$result" FAIL
  case_of key-file-left 0 C "scenario=C delete_all=Err(DeleteIncomplete(DeleteReport root_gone=yes key_file_gone=no
$result" FAIL
  case_of line-of-another-scenario 0 C "scenario=B delete_all=Ok(DeleteReport root_gone=yes key_file_gone=yes
$result" FAIL
  case_of no-line 0 A "$result" FAIL
  case_of test-failed 101 A "scenario=A delete_all=Ok(DeleteReport root_gone=yes key_file_gone=yes
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 420 filtered out; finished in 2.10s" FAIL
  case_of no-test-ran 0 A 'scenario=A delete_all=Ok root_gone=yes key_file_gone=yes
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 421 filtered out; finished in 0.00s' FAIL
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
