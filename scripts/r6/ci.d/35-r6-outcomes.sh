#!/bin/sh
# jobs: macos windows
# rule: The importer passes its golden cases, and once docs/r6/outcomes/ holds a table, each trial file still has the blob that sources.tsv records and the import of the two files gives the four committed files again.
set -u
name=35-r6-outcomes
case $0 in
  /*) self=$0 ;;
  *) self=$(pwd)/$0 ;;
esac
outcomes=docs/r6/outcomes
golden=crates/halvo-route-gate/tests/data/r3
driver_path=docs/r3/trials/driver.md
cells_path=docs/r3/trials/model-cells.md
tab=$(printf '\t')
models_header="model${tab}machine${tab}bound_bytes${tab}fits${tab}group${tab}source"
sources_header="id${tab}path${tab}blob${tab}date${tab}measures${tab}limits"
date_form='[0-9]{4}-[0-9]{2}-[0-9]{2}'
count_form='(0|[1-9][0-9]*)'
token_form='[a-z0-9_]{1,32}'
golden_tests=2

enter_repo() {
  if [ -n "${CI_D_REPO:-}" ]; then
    repo=$CI_D_REPO
  else
    repo=$(git rev-parse --show-toplevel) || return 1
  fi
  cd "$repo" || return 1
}

broken() {
  printf 'FAIL self-test:%s (%s)\n' "$name" "$1"
  exit 1
}

setup_failed() {
  printf 'FAIL %s (setup)\n' "$name"
  exit 1
}

source_row() {
  sed -n "$1p" "$outcomes/sources.tsv" | grep -Eq "^$2${tab}[^${tab}]+${tab}[0-9a-f]{40}${tab}${date_form}${tab}$3${tab}$4\$"
}

calendar_date() {
  printf '%s\n' "$1" | awk -F- '
    !/^[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]$/ { bad = 1; exit }
    {
      y = $1 + 0; m = $2 + 0; d = $3 + 0
      split("31 28 31 30 31 30 31 31 30 31 30 31", last, " ")
      if ((y % 4 == 0 && y % 100 != 0) || y % 400 == 0) last[2] = 29
      if (m < 1 || m > 12 || d < 1 || d > last[m]) bad = 1
    }
    END { exit bad }'
}

committed_form() {
  [ -f "$outcomes/models.tsv" ] && [ -f "$outcomes/sources.tsv" ] && [ -f "$outcomes/freeze.txt" ] || return 1
  [ "$(sed -n 1p "$outcomes/models.tsv")" = "$models_header" ] || return 1
  [ "$(grep -c '' "$outcomes/sources.tsv")" = 3 ] || return 1
  [ "$(sed -n 1p "$outcomes/sources.tsv")" = "$sources_header" ] || return 1
  source_row 2 s1 driver_trial_attempts 3_seeds_2_machines_team_fixtures || return 1
  source_row 3 s2 footprint_bounds 5_runs_a_cell_no_16gb_laptop || return 1
  [ "$(cell 2 2)" = "$driver_path" ] && [ "$(cell 3 2)" = "$cells_path" ] || return 1
  calendar_date "$(cell 2 4)" && calendar_date "$(cell 3 4)" || return 1
  [ "$(grep -c '' "$outcomes/freeze.txt")" = 2 ] || return 1
  sed -n 1p "$outcomes/freeze.txt" |
    grep -Eq "^freeze rows=${count_form} models=${count_form} sources=2 date=${date_form}\$" || return 1
  calendar_date "$(sed -n 1p "$outcomes/freeze.txt" | sed 's/^.* date=//')" || return 1
  sed -n 2p "$outcomes/freeze.txt" | grep -Eq "^on_device driver=${token_form} writer=${token_form}\$"
}

cell() {
  sed -n "$1p" "$outcomes/sources.tsv" | cut -f "$2"
}

measure() {
  enter_repo || setup_failed
  artifacts=${CI_D_ARTIFACTS:-target/ci-artifacts/$name}
  mkdir -p "$artifacts" || setup_failed
  scratch=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || setup_failed

  if ! cargo test --locked -p halvo-route-gate --test import_golden >"$artifacts/cargo-test.txt" 2>&1 ||
    ! grep -q "^test result: ok\\. $golden_tests passed; " "$artifacts/cargo-test.txt"; then
    printf 'FAIL %s (tests)\n' "$name"
    exit 1
  fi

  if [ ! -f "$outcomes/recorded.tsv" ]; then
    printf 'outcomes rows=0 models=0 sources=0 same=- stale=-\n'
    printf 'PASS %s\n' "$name"
    exit 0
  fi

  if ! committed_form; then
    printf 'outcomes rows=0 models=0 sources=0 same=no stale=0\n'
    printf 'FAIL %s (differs)\n' "$name"
    exit 1
  fi

  stale=0
  for row in 2 3; do
    path=$(cell "$row" 2)
    blob=$(cell "$row" 3)
    now=$(git hash-object --no-filters -- "$path" 2>>"$artifacts/hash-object.err") || now=
    [ "$now" = "$blob" ] || stale=$((stale + 1))
  done

  fresh=$scratch/import
  mkdir "$fresh" || setup_failed
  cargo run -q --locked -p halvo-route-gate --bin route-gate -- import \
    --driver "$(cell 2 2)" --cells "$(cell 3 2)" \
    --date "s1=$(cell 2 4)" --date "s2=$(cell 3 4)" --out "$fresh" \
    >"$artifacts/import.out" 2>"$artifacts/import.err"
  import_code=$?
  line=$(cat "$artifacts/import.out")
  imported=no
  if [ "$import_code" -eq 0 ] &&
    printf '%s\n' "$line" | grep -Eq "^import rows=${count_form} models=${count_form} sources=2\$"; then
    imported=yes
    counts=$(printf '%s\n' "$line" | sed 's/^import rows=\([0-9]*\) models=\([0-9]*\) sources=2$/\1 \2/')
  else
    counts=$(sed -n 1p "$outcomes/freeze.txt" | sed 's/^freeze rows=\([0-9]*\) models=\([0-9]*\) .*$/\1 \2/')
  fi
  rows=${counts% *}
  models=${counts#* }

  same=yes
  if [ "$imported" = yes ]; then
    for file in recorded.tsv models.tsv sources.tsv freeze.txt; do
      cmp -s "$fresh/$file" "$outcomes/$file" || same=no
    done
  else
    same=no
  fi
  [ "$stale" -eq 0 ] || same=no
  printf 'outcomes rows=%s models=%s sources=2 same=%s stale=%s\n' "$rows" "$models" "$same" "$stale"

  if [ "$stale" -gt 0 ]; then
    cause=stale
  elif [ "$imported" = no ]; then
    cause=import
  elif [ "$same" = no ]; then
    cause=differs
  else
    printf 'PASS %s\n' "$name"
    exit 0
  fi
  printf 'FAIL %s (%s)\n' "$name" "$cause"
  exit 1
}

fixture() {
  case_name=$1
  case_repo=$work/$1/repo
  mkdir -p "$case_repo/$outcomes" || broken "cannot make the fixture $1"
  if [ "$2" = - ]; then
    return 0
  fi
  mkdir -p "$case_repo/docs/r3/trials" || broken "cannot make the fixture $1"
  cp "$golden/$2/driver.md" "$case_repo/$driver_path" || broken "cannot copy the golden case $2 into the fixture $1"
  cp "$golden/$2/model-cells.md" "$case_repo/$cells_path" || broken "cannot copy the golden case $2 into the fixture $1"
  cp "$golden/$2/expected/recorded.tsv" "$golden/$2/expected/models.tsv" \
    "$golden/$2/expected/sources.tsv" "$golden/$2/expected/freeze.txt" "$case_repo/$outcomes/" ||
    broken "cannot copy the golden case $2 into the fixture $1"
  edit "$outcomes/sources.tsv" "s|^s1${tab}driver\\.md${tab}|s1${tab}${driver_path}${tab}|;s|^s2${tab}model-cells\\.md${tab}|s2${tab}${cells_path}${tab}|"
}

edit() {
  sed "$2" "$case_repo/$1" >"$work/edited" || broken "cannot edit $1 in the fixture $case_name"
  cmp -s "$work/edited" "$case_repo/$1" && broken "the edit of $1 in the fixture $case_name changed nothing"
  cp "$work/edited" "$case_repo/$1" || broken "cannot write $1 in the fixture $case_name"
}

rehash() {
  blob=$(git hash-object --no-filters -- "$case_repo/$1") || broken "cannot hash $1 in the fixture $case_name"
  edit "$outcomes/sources.tsv" "$2s/^\\(s[12]${tab}[^${tab}]*${tab}\\)[0-9a-f]*${tab}/\\1${blob}${tab}/"
}

check() {
  out=$(
    cd "$case_repo" || exit 2
    PATH=$work/bin:$PATH
    CI_D_REPO=$case_repo
    export PATH CI_D_REPO STUB_TEST_EXIT STUB_TEST_PASSED R6_OUTCOMES_TOOL
    if [ -n "$crlf_filter" ]; then
      GIT_CONFIG_COUNT=1
      GIT_CONFIG_KEY_0=core.autocrlf
      GIT_CONFIG_VALUE_0=true
      export GIT_CONFIG_COUNT GIT_CONFIG_KEY_0 GIT_CONFIG_VALUE_0
    fi
    CI_D_ARTIFACTS=$work/artifacts/$1 sh "$self" 2>&1
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
  case ${4:-}:$out in
    :*) ;;
    "$4":*"$4"*) got=$got-with-the-text-it-must-not-hold ;;
  esac
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$got"
  if [ "$got" != "$2" ]; then
    wrong="$wrong $1"
  elif [ "$2" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
  STUB_TEST_EXIT=0
  STUB_TEST_PASSED=$golden_tests
  crlf_filter=
}

self_test() {
  enter_repo || broken "cannot enter the repository"
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || broken "cannot make a temporary directory"
  mkdir -p "$work/bin" "$work/artifacts" || broken "cannot make a directory"
  artifacts=${CI_D_ARTIFACTS:-target/ci-artifacts/$name}
  mkdir -p "$artifacts" || broken "cannot make the artifacts directory"
  if ! cargo build -q --locked -p halvo-route-gate --bin route-gate >"$artifacts/self-test-build.txt" 2>&1; then
    printf 'FAIL self-test:%s (the tool did not build)\n' "$name"
    exit 1
  fi
  case ${CARGO_TARGET_DIR:-} in
    /*) R6_OUTCOMES_TOOL=$CARGO_TARGET_DIR/debug/route-gate ;;
    *) R6_OUTCOMES_TOOL=$(pwd)/${CARGO_TARGET_DIR:-target}/debug/route-gate ;;
  esac
  if [ -f "$R6_OUTCOMES_TOOL.exe" ]; then
    R6_OUTCOMES_TOOL=$R6_OUTCOMES_TOOL.exe
  fi
  cat >"$work/bin/cargo" <<'EOF'
#!/bin/sh
case $* in
  'test --locked -p halvo-route-gate --test import_golden')
    printf 'test result: ok. %s passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n' "$STUB_TEST_PASSED"
    exit "$STUB_TEST_EXIT"
    ;;
  'run -q --locked -p halvo-route-gate --bin route-gate -- import '*)
    shift 8
    exec "$R6_OUTCOMES_TOOL" "$@"
    ;;
  *)
    printf 'stub cargo: got the arguments: %s\n' "$*"
    exit 97
    ;;
esac
EOF
  chmod +x "$work/bin/cargo" || broken "cannot make the stub cargo"
  rejected=0
  wrong=
  STUB_TEST_EXIT=0
  STUB_TEST_PASSED=$golden_tests
  crlf_filter=

  fixture waiting -
  check waiting PASS "outcomes rows=0 models=0 sources=0 same=- stale=-
PASS $name"
  fixture full-holds full
  check full-holds PASS "outcomes rows=132 models=6 sources=2 same=yes stale=0
PASS $name"
  fixture no-writer-holds no_writer
  check no-writer-holds PASS "outcomes rows=132 models=6 sources=2 same=yes stale=0"
  fixture no-driver-holds no_driver
  check no-driver-holds PASS "outcomes rows=1 models=1 sources=2 same=yes stale=0"
  fixture full-imported-with-two-days-holds full
  edit "$outcomes/sources.tsv" "2s/${tab}2026-11-10${tab}/${tab}2026-11-09${tab}/"
  check full-imported-with-two-days-holds PASS "outcomes rows=132 models=6 sources=2 same=yes stale=0"

  fixture recorded-pass-flipped full
  edit "$outcomes/recorded.tsv" "2s/${tab}yes${tab}/${tab}no${tab}/"
  check recorded-pass-flipped FAIL "outcomes rows=132 models=6 sources=2 same=no stale=0
FAIL $name (differs)"
  fixture driver-pass-flipped-blob-stale full
  edit "$driver_path" 's/^\(attempt model=m_small machine=mac task=bill seed=1 kind=compile i=1 \)pass=yes /\1pass=no /'
  check driver-pass-flipped-blob-stale FAIL "same=no stale=1
FAIL $name (stale)"
  fixture cells-bound-changed-blob-stale full
  edit "$cells_path" 's/^bound model=m_tiny machine=mac bound_bytes=2684354560 fits=yes$/bound model=m_tiny machine=mac bound_bytes=2684354560 fits=no/'
  check cells-bound-changed-blob-stale FAIL "same=no stale=1
FAIL $name (stale)"
  fixture both-files-changed full
  edit "$driver_path" 's/^verdict driver=m_small path=A writer=m_small$/verdict driver=m_tiny path=A writer=m_tiny/'
  edit "$cells_path" 's/^verdict candidates=m_tiny,m_small$/verdict candidates=m_tiny/'
  check both-files-changed FAIL "same=no stale=2
FAIL $name (stale)"
  fixture source-file-missing full
  mv "$case_repo/$cells_path" "$case_repo/moved.md" || broken "cannot change the fixture $case_name"
  check source-file-missing FAIL "stale=1
FAIL $name (stale)"
  fixture driver-pass-flipped-table-not-imported full
  edit "$driver_path" 's/^\(attempt model=m_tiny machine=mac task=bill seed=1 kind=compile i=1 \)pass=yes /\1pass=no /'
  rehash "$driver_path" 2
  check driver-pass-flipped-table-not-imported FAIL "outcomes rows=132 models=6 sources=2 same=no stale=0
FAIL $name (differs)"
  fixture import-refused full
  edit "$driver_path" 's/^\(attempt model=m_small machine=mac task=bill seed=1 kind=compile i=1 \)pass=yes /\1pass=maybe /'
  rehash "$driver_path" 2
  check import-refused FAIL "outcomes rows=132 models=6 sources=2 same=no stale=0
FAIL $name (import)"
  fixture stale-and-the-import-ran full
  edit "$driver_path" '/^attempt model=m_tiny machine=mac task=bill seed=1 kind=replay i=3 /d'
  check stale-and-the-import-ran FAIL "outcomes rows=131 models=6 sources=2 same=no stale=1
FAIL $name (stale)"
  fixture stale-and-the-import-refused full
  edit "$driver_path" 's/^\(attempt model=m_small machine=mac task=bill seed=1 kind=compile i=1 \)pass=yes /\1pass=maybe /'
  check stale-and-the-import-refused FAIL "outcomes rows=132 models=6 sources=2 same=no stale=1
FAIL $name (stale)"
  fixture crlf-that-a-filter-hides full
  awk '{ printf "%s\r\n", $0 }' "$case_repo/$driver_path" >"$work/crlf" || broken "cannot change the fixture $case_name"
  cp "$work/crlf" "$case_repo/$driver_path" || broken "cannot change the fixture $case_name"
  crlf_filter=1
  check crlf-that-a-filter-hides FAIL "outcomes rows=132 models=6 sources=2 same=no stale=1
FAIL $name (stale)"
  fixture models-fits-flipped full
  edit "$outcomes/models.tsv" "2s/${tab}not-measured${tab}/${tab}no${tab}/"
  check models-fits-flipped FAIL "same=no stale=0
FAIL $name (differs)"
  fixture freeze-writer-changed full
  edit "$outcomes/freeze.txt" 's/ writer=m_small$/ writer=none/'
  check freeze-writer-changed FAIL "same=no stale=0
FAIL $name (differs)"
  fixture source-date-changed full
  edit "$outcomes/sources.tsv" "2s/${tab}2026-11-10${tab}/${tab}2026-11-11${tab}/"
  check source-date-changed FAIL "same=no stale=0
FAIL $name (differs)"
  fixture freeze-missing full
  mv "$case_repo/$outcomes/freeze.txt" "$work/freeze-missing/freeze.txt" || broken "cannot change the fixture $case_name"
  check freeze-missing FAIL "outcomes rows=0 models=0 sources=0 same=no stale=0
FAIL $name (differs)"
  fixture sources-one-row full
  edit "$outcomes/sources.tsv" '3d'
  check sources-one-row FAIL "outcomes rows=0 models=0 sources=0 same=no stale=0
FAIL $name (differs)"
  fixture sources-rows-swapped full
  edit "$outcomes/sources.tsv" '2{h;d;};3G'
  check sources-rows-swapped FAIL "outcomes rows=0 models=0 sources=0 same=no stale=0
FAIL $name (differs)"
  fixture sources-names-a-copy full
  cp "$case_repo/$driver_path" "$case_repo/driver-copy.md" || broken "cannot change the fixture $case_name"
  edit "$outcomes/sources.tsv" "2s|${tab}${driver_path}${tab}|${tab}driver-copy.md${tab}|"
  check sources-names-a-copy FAIL "outcomes rows=0 models=0 sources=0 same=no stale=0
FAIL $name (differs)"
  fixture models-header-wrong full
  edit "$outcomes/models.tsv" "1s/${tab}group${tab}/${tab}groups${tab}/"
  check models-header-wrong FAIL "outcomes rows=0 models=0 sources=0 same=no stale=0
FAIL $name (differs)"
  fixture freeze-third-line full
  printf 'extra\n' >>"$case_repo/$outcomes/freeze.txt" || broken "cannot change the fixture $case_name"
  check freeze-third-line FAIL "outcomes rows=0 models=0 sources=0 same=no stale=0
FAIL $name (differs)"
  fixture sources-fourth-line full
  printf 's3%sx\n' "$tab" >>"$case_repo/$outcomes/sources.tsv" || broken "cannot change the fixture $case_name"
  check sources-fourth-line FAIL "outcomes rows=0 models=0 sources=0 same=no stale=0
FAIL $name (differs)"
  fixture freeze-rows-with-a-leading-zero full
  edit "$outcomes/freeze.txt" '1s/ rows=132 / rows=0132 /'
  check freeze-rows-with-a-leading-zero FAIL "outcomes rows=0 models=0 sources=0 same=no stale=0
FAIL $name (differs)"
  fixture freeze-writer-upper-case full
  edit "$outcomes/freeze.txt" '2s/ writer=m_small$/ writer=M_small/'
  check freeze-writer-upper-case FAIL "outcomes rows=0 models=0 sources=0 same=no stale=0
FAIL $name (differs)"
  fixture sources-measures-changed full
  edit "$outcomes/sources.tsv" "2s/${tab}driver_trial_attempts${tab}/${tab}driver_trial_attempt${tab}/"
  check sources-measures-changed FAIL "outcomes rows=0 models=0 sources=0 same=no stale=0
FAIL $name (differs)"
  fixture sources-date-not-a-calendar-day full
  edit "$outcomes/sources.tsv" "2s/${tab}2026-11-10${tab}/${tab}2026-02-29${tab}/"
  check sources-date-not-a-calendar-day FAIL "outcomes rows=0 models=0 sources=0 same=no stale=0
FAIL $name (differs)"
  fixture freeze-date-not-a-calendar-day full
  edit "$outcomes/freeze.txt" '1s/ date=2026-11-10$/ date=2026-11-31/'
  check freeze-date-not-a-calendar-day FAIL "outcomes rows=0 models=0 sources=0 same=no stale=0
FAIL $name (differs)"
  fixture tests-fail full
  STUB_TEST_EXIT=101
  check tests-fail FAIL "FAIL $name (tests)" "outcomes rows="
  fixture tests-fail-and-stale full
  edit "$driver_path" 's/^verdict driver=m_small path=A writer=m_small$/verdict driver=m_tiny path=A writer=m_tiny/'
  STUB_TEST_EXIT=101
  check tests-fail-and-stale FAIL "FAIL $name (tests)" "outcomes rows="
  fixture golden-test-emptied full
  STUB_TEST_PASSED=0
  check golden-test-emptied FAIL "FAIL $name (tests)" "outcomes rows="
  fixture golden-test-count-changed full
  STUB_TEST_PASSED=3
  check golden-test-count-changed FAIL "FAIL $name (tests)" "outcomes rows="

  usage=$(sh "$self" --all 2>&1)
  code=$?
  if [ "$code:$usage" = "1:FAIL $name (usage)" ]; then
    printf 'case %s/unknown-argument want=FAIL got=FAIL\n' "$name"
    rejected=$((rejected + 1))
  else
    printf 'case %s/unknown-argument want=FAIL got=exit-%s\n' "$name" "$code"
    wrong="$wrong unknown-argument"
  fi

  setup=$(CI_D_REPO=$work/no-such-directory sh "$self" 2>&1)
  code=$?
  last=$(printf '%s\n' "$setup" | tail -n 1)
  if [ "$code:$last" = "1:FAIL $name (setup)" ]; then
    printf 'case %s/repository-missing want=FAIL got=FAIL\n' "$name"
    rejected=$((rejected + 1))
  else
    printf 'case %s/repository-missing want=FAIL got=exit-%s\n' "$name" "$code"
    wrong="$wrong repository-missing"
  fi

  if [ -z "${R6_OUTCOMES_NESTED:-}" ]; then
    nested=$(cd crates && R6_OUTCOMES_NESTED=1 sh "../scripts/r6/ci.d/$name.sh" --self-test 2>&1 | tail -n 1)
    case $nested in
      "PASS self-test:$name "*) printf 'case %s/self-test-from-a-subdirectory want=PASS got=PASS\n' "$name" ;;
      *)
        printf 'case %s/self-test-from-a-subdirectory want=PASS got=%s\n' "$name" "$nested"
        wrong="$wrong self-test-from-a-subdirectory"
        ;;
    esac
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
