#!/bin/sh
# jobs: macos windows
# rule: The router's tests pass, the gate tool's self-test fails the three bad routers and passes the derived one on 20 generated tables, and the tool regenerates the committed specimen.
set -u
name=30-r6-table
specimen=crates/halvo-route-gate/tests/data/specimen.tsv
want_line='selftest tables=20 derived_pass=20 product_pass=- one_tier_fail=20 random_fail=20 peek_fail=20'
want_product='selftest tables=20 derived_pass=20 product_pass=20 one_tier_fail=20 random_fail=20 peek_fail=20'

enter_repo() {
  if [ -n "${CI_D_REPO:-}" ]; then
    repo=$CI_D_REPO
  else
    repo=$(git rev-parse --show-toplevel) || exit 2
  fi
  cd "$repo" || exit 2
}

measure() {
  enter_repo
  artifacts=${CI_D_ARTIFACTS:-target/ci-artifacts/$name}
  mkdir -p "$artifacts" || exit 2
  scratch=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  cause=

  if ! cargo test --locked -p halvo-route -p halvo-route-gate >"$artifacts/cargo-test.txt" 2>&1; then
    cause=tests
  fi

  cargo run -q --locked -p halvo-route-gate --bin route-gate -- self-test --tables 20 --seed 1 \
    >"$artifacts/selftest.out" 2>"$artifacts/selftest.err"
  selftest_code=$?
  grep '^selftest ' "$artifacts/selftest.out"
  last=$(tail -n 1 "$artifacts/selftest.out")
  if [ "$selftest_code" -ne 0 ] || { [ "$last" != "$want_line" ] && [ "$last" != "$want_product" ]; }; then
    cause=${cause:-selftest}
  fi

  fresh=$scratch/specimen.tsv
  cargo run -q --locked -p halvo-route-gate --bin route-gate -- generate --seed 1 --out "$fresh" \
    >"$artifacts/generate.out" 2>"$artifacts/generate.err"
  rows=0
  same=no
  if [ -f "$fresh" ]; then
    lines=$(grep -c '' "$fresh")
    [ "$lines" -gt 0 ] && rows=$((lines - 1))
    cmp -s "$fresh" "$specimen" && same=yes
  fi
  printf 'specimen rows=%s same=%s\n' "$rows" "$same"
  [ "$same" = yes ] || cause=${cause:-specimen}

  if [ -n "$cause" ]; then
    printf 'FAIL %s (%s)\n' "$name" "$cause"
    exit 1
  fi
  printf 'PASS %s\n' "$name"
}

check() {
  out=$(
    PATH=$work/bin:$PATH
    CI_D_REPO=$work/repo
    CI_D_ARTIFACTS=$work/artifacts/$1
    export PATH CI_D_REPO CI_D_ARTIFACTS STUB_TEST_EXIT STUB_SELFTEST STUB_SELFTEST_EXIT STUB_FRESH
    sh "$0" 2>&1
  )
  code=$?
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  case $out in
    *"$3"*) ;;
    *) got=$got-without-the-text ;;
  esac
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$got"
  if [ "$got" != "$2" ]; then
    wrong="$wrong $1"
  elif [ "$2" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
  STUB_TEST_EXIT=0
  STUB_SELFTEST=$want_line
  STUB_SELFTEST_EXIT=0
  STUB_FRESH=$work/fresh.tsv
}

self_test() {
  enter_repo
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  mkdir -p "$work/bin" "$work/repo/crates/halvo-route-gate/tests/data" "$work/artifacts" || exit 2
  cp "$specimen" "$work/fresh.tsv" || exit 2
  cp "$specimen" "$work/repo/$specimen" || exit 2
  sed '2d' "$specimen" >"$work/short.tsv" || exit 2
  tab=$(printf '\t')
  sed "2s/${tab}yes${tab}/${tab}no${tab}/" "$specimen" >"$work/flipped.tsv" || exit 2
  cmp -s "$work/flipped.tsv" "$work/fresh.tsv" && exit 2
  cat >"$work/bin/cargo" <<'EOF'
#!/bin/sh
case $* in
  'test --locked -p halvo-route -p halvo-route-gate')
    exit "$STUB_TEST_EXIT"
    ;;
  'run -q --locked -p halvo-route-gate --bin route-gate -- self-test --tables 20 --seed 1')
    [ -z "$STUB_SELFTEST" ] || printf '%s\n' "$STUB_SELFTEST"
    exit "$STUB_SELFTEST_EXIT"
    ;;
  'run -q --locked -p halvo-route-gate --bin route-gate -- generate --seed 1 --out '*)
    [ -f "$STUB_FRESH" ] || exit 2
    shift 12
    cp "$STUB_FRESH" "$1"
    ;;
  *)
    printf 'stub cargo: got the arguments: %s\n' "$*"
    exit 97
    ;;
esac
EOF
  chmod +x "$work/bin/cargo"
  rejected=0
  wrong=
  STUB_TEST_EXIT=0
  STUB_SELFTEST=$want_line
  STUB_SELFTEST_EXIT=0
  STUB_FRESH=$work/fresh.tsv

  check all-hold PASS "$want_line
specimen rows=540 same=yes
PASS $name"
  STUB_SELFTEST=$want_product
  check product-counted PASS 'product_pass=20 one_tier_fail=20'
  STUB_TEST_EXIT=101
  check tests-fail FAIL "FAIL $name (tests)"
  STUB_SELFTEST='selftest miss router=random seed=7
selftest tables=20 derived_pass=20 product_pass=- one_tier_fail=20 random_fail=19 peek_fail=20'
  STUB_SELFTEST_EXIT=1
  check random-passes-once FAIL "random_fail=19 peek_fail=20
specimen rows=540 same=yes
FAIL $name (selftest)"
  STUB_SELFTEST='selftest tables=20 derived_pass=20 product_pass=- one_tier_fail=20 random_fail=20 peek_fail=19'
  check wrong-line-with-exit-0 FAIL "FAIL $name (selftest)"
  STUB_SELFTEST_EXIT=1
  check right-line-with-exit-1 FAIL "FAIL $name (selftest)"
  STUB_SELFTEST='selftest tables=19 derived_pass=19 product_pass=- one_tier_fail=19 random_fail=19 peek_fail=19'
  check nineteen-tables FAIL "FAIL $name (selftest)"
  STUB_SELFTEST='selftest tables=20 derived_pass=20 product_pass=19 one_tier_fail=20 random_fail=20 peek_fail=20'
  check product-misses-once FAIL "FAIL $name (selftest)"
  STUB_SELFTEST="$want_line extra=1"
  check line-with-another-key FAIL "FAIL $name (selftest)"
  STUB_SELFTEST=
  check no-line FAIL "FAIL $name (selftest)"
  STUB_FRESH=$work/short.tsv
  check fresh-output-without-a-row FAIL "specimen rows=539 same=no
FAIL $name (specimen)"
  STUB_FRESH=$work/flipped.tsv
  check fresh-output-with-a-pass-flipped FAIL "specimen rows=540 same=no
FAIL $name (specimen)"
  STUB_FRESH=$work/missing.tsv
  check no-fresh-output FAIL "specimen rows=0 same=no
FAIL $name (specimen)"
  cp "$work/short.tsv" "$work/repo/$specimen" || exit 2
  check committed-specimen-without-a-row FAIL "specimen rows=540 same=no
FAIL $name (specimen)"
  STUB_TEST_EXIT=101
  STUB_SELFTEST_EXIT=1
  check three-causes-name-the-first FAIL "FAIL $name (tests)"
  cp "$work/fresh.tsv" "$work/repo/$specimen" || exit 2

  usage=$(sh "$0" --all 2>&1)
  code=$?
  if [ "$code:$usage" = "1:FAIL $name (usage)" ]; then
    printf 'case %s/unknown-argument want=FAIL got=FAIL\n' "$name"
    rejected=$((rejected + 1))
  else
    printf 'case %s/unknown-argument want=FAIL got=exit-%s\n' "$name" "$code"
    wrong="$wrong unknown-argument"
  fi

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  '') measure ;;
  *)
    printf 'FAIL %s (usage)\n' "$name"
    exit 1
    ;;
esac
