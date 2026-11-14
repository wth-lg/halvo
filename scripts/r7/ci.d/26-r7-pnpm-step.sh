#!/bin/sh
# jobs: macos windows
# rule: The self-test of scripts/prerequisites.sh, which holds the cases of the pnpm step and of its reader of the lock, exits 0 with at least one PASS case, no FAIL line and its PASS line last, and leaves every tracked file as the index holds it.
set -u
name=26-r7-pnpm-step

self_test_of() {
  tree=$1
  out=$2
  (cd "$tree" && sh scripts/prerequisites.sh --self-test) >"$out/self-test.txt" 2>&1
  code=$?
  passed=$(grep -c '^PASS ' "$out/self-test.txt")
  failed=$(grep -c '^FAIL ' "$out/self-test.txt")
  last=$(sed -n '$p' "$out/self-test.txt")
  counts="cases=$((passed + failed)) passed=$passed failed=$failed exit=$code"
  cause=
  if [ "$code" -ne 0 ]; then
    cause="the self-test exited $code"
  elif [ "$failed" -gt 0 ]; then
    cause="the self-test printed $failed FAIL lines"
  elif [ "$passed" -eq 0 ] || [ "$last" != "prerequisites: self-test PASS" ]; then
    cause="the self-test did not end with its PASS line after its cases"
  elif [ "$(git -C "$tree" rev-parse --is-inside-work-tree 2>/dev/null)" = true ] && ! git -C "$tree" diff --quiet; then
    cause="a tracked file differs from the index after the self-test"
  fi
  [ -z "$cause" ]
}

measure() {
  repo=${CI_D_REPO:-$(git rev-parse --show-toplevel)} || exit 2
  artifacts=${CI_D_ARTIFACTS:-$repo/target/ci-artifacts/$name}
  mkdir -p "$artifacts" || exit 2
  if self_test_of "$repo" "$artifacts"; then
    printf '%s\n' "$counts"
    printf 'PASS %s\n' "$name"
  else
    printf '%s\n' "$counts"
    printf 'FAIL %s (%s; its output is self-test.txt among the artifacts)\n' "$name" "$cause"
    exit 1
  fi
}

fixture() {
  dir=$scratch/$1
  mkdir -p "$dir/scripts" "$dir/out" || exit 2
}

stub() {
  fixture "$1"
  printf '%s\n' "$2" >"$dir/scripts/prerequisites.sh"
}

expect() {
  want=$1
  case_name=$2
  (self_test_of "$scratch/$case_name" "$scratch/$case_name/out") >/dev/null 2>&1
  code=$?
  case $code in
    0) got=PASS ;;
    1) got=FAIL ;;
    *) got=exit-$code ;;
  esac
  printf 'case %s/%s want=%s got=%s\n' "$name" "$case_name" "$want" "$got"
  if [ "$got" != "$want" ]; then
    wrong="$wrong $case_name"
  elif [ "$want" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

self_test() {
  repo=${CI_D_REPO:-$(git rev-parse --show-toplevel)} || exit 2
  scratch=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  rejected=0
  wrong=

  stub passes 'echo "PASS a case"; echo "prerequisites: self-test PASS"'
  expect PASS passes

  stub exits-1 'echo "PASS a case"; echo "prerequisites: self-test PASS"; exit 1'
  expect FAIL exits-1

  stub prints-a-fail-line 'echo "FAIL a case"; echo "prerequisites: self-test PASS"'
  expect FAIL prints-a-fail-line

  stub no-case 'echo "prerequisites: self-test PASS"'
  expect FAIL no-case

  stub no-pass-line-last 'echo "prerequisites: self-test PASS"; echo "PASS a case"'
  expect FAIL no-pass-line-last

  stub changes-a-tracked-file 'echo changed >>notes.txt; echo "PASS a case"; echo "prerequisites: self-test PASS"'
  printf 'notes\n' >"$dir/notes.txt"
  git -C "$dir" init -q || exit 2
  git -C "$dir" add notes.txt scripts/prerequisites.sh || exit 2
  expect FAIL changes-a-tracked-file

  fixture platform-package-unchecked
  sed '/^    if .*hex_of_base64/s/.*/    if false; then/' "$repo/scripts/prerequisites.sh" >"$dir/scripts/prerequisites.sh" || exit 2
  if cmp -s "$repo/scripts/prerequisites.sh" "$dir/scripts/prerequisites.sh"; then
    printf 'FAIL self-test:%s (the fixture platform-package-unchecked cannot be made: scripts/prerequisites.sh holds no line that compares the platform package with the lock)\n' "$name"
    exit 1
  fi
  expect FAIL platform-package-unchecked

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%d failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  *) measure ;;
esac
