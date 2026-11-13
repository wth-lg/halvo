#!/bin/sh
# jobs: macos windows
# rule: The backend's typecheck (tsc --noEmit in backend/, over the files that backend/tsconfig.json includes) reports no error, and biome lint, with backend/biome.json, exits 0, prints no count of errors, warnings or infos, and reads at least one file, in backend/ and in the model formal/r7-ledger/.
# lane: free
set -u
name=20-r7-backend-static

static_in() {
  repo=$1
  out=$2
  bin=$repo/backend/node_modules/.bin
  for tool in tsc biome; do
    if [ ! -f "$bin/$tool" ]; then
      cause="backend/node_modules/.bin/$tool is missing; sh scripts/prerequisites.sh installs it"
      return 1
    fi
  done
  (cd "$repo/backend" && node_modules/.bin/tsc --noEmit) >"$out/typecheck.txt" 2>&1
  typecheck=$?
  (cd "$repo/backend" && node_modules/.bin/biome lint .) >"$out/lint-backend.txt" 2>&1
  lint_backend=$?
  (cd "$repo" && backend/node_modules/.bin/biome lint --config-path=backend formal/r7-ledger) >"$out/lint-model.txt" 2>&1
  lint_model=$?
  errors=$(grep -c 'error TS[0-9]' "$out/typecheck.txt")
  backend_files=$(sed -n 's/^Checked \([0-9][0-9]*\) files\{0,1\} in .*/\1/p' "$out/lint-backend.txt" | tail -n 1)
  backend_found=$(grep -c -E '^Found [0-9]+ (error|warning|info)' "$out/lint-backend.txt")
  model_found=$(grep -c -E '^Found [0-9]+ (error|warning|info)' "$out/lint-model.txt")
  model_files=$(sed -n 's/^Checked \([0-9][0-9]*\) files\{0,1\} in .*/\1/p' "$out/lint-model.txt" | tail -n 1)
  counts="typecheck-exit=$typecheck typecheck-errors=$errors lint-backend-exit=$lint_backend lint-backend-files=${backend_files:-none} lint-model-exit=$lint_model lint-model-files=${model_files:-none}"
  cause=
  [ "$typecheck" = 0 ] && [ "$errors" = 0 ] || cause="the typecheck of backend/ reports an error"
  [ -n "$cause" ] || [ "$lint_backend" = 0 ] || cause="biome lint reports a finding in backend/"
  [ -n "$cause" ] || [ "$lint_model" = 0 ] || cause="biome lint reports a finding in formal/r7-ledger/"
  [ -n "$cause" ] || [ "$backend_found" = 0 ] || cause="biome lint prints a count of findings in backend/"
  [ -n "$cause" ] || [ "$model_found" = 0 ] || cause="biome lint prints a count of findings in formal/r7-ledger/"
  [ -n "$cause" ] || [ "${backend_files:-0}" -gt 0 ] || cause="biome lint read no file in backend/"
  [ -n "$cause" ] || [ "${model_files:-0}" -gt 0 ] || cause="biome lint read no file in formal/r7-ledger/"
  [ -z "$cause" ]
}

measure() {
  repo=${CI_D_REPO:-$(git rev-parse --show-toplevel)} || exit 2
  artifacts=${CI_D_ARTIFACTS:-$repo/target/ci-artifacts/$name}
  mkdir -p "$artifacts" || exit 2
  counts=unmeasured
  if static_in "$repo" "$artifacts"; then
    printf '%s\n' "$counts"
    printf 'PASS %s\n' "$name"
  else
    printf '%s\n' "$counts"
    printf 'FAIL %s (%s)\n' "$name" "$cause"
    exit 1
  fi
}

stub() {
  cat >"$1" <<'EOF'
#!/bin/sh
tool=$(basename "$0")
where=$(pwd -P)
printf '%s|%s|%s\n' "$tool" "$where" "$*" >>"$STUB_STATE/calls"
case "$tool:$*" in
  tsc:*) kind=tsc ;;
  biome:*--config-path*) kind=model ;;
  *) kind=backend ;;
esac
[ ! -f "$STUB_STATE/$kind.out" ] || cat "$STUB_STATE/$kind.out"
exit "$(cat "$STUB_STATE/$kind.exit" 2>/dev/null || echo 0)"
EOF
  chmod +x "$1"
}

fixture() {
  dir=$scratch/$1
  mkdir -p "$dir/backend/node_modules/.bin" "$dir/formal/r7-ledger" "$dir/state" || exit 2
  stub "$dir/backend/node_modules/.bin/tsc"
  stub "$dir/backend/node_modules/.bin/biome"
  : >"$dir/state/tsc.out"
  printf 'Checked 4 files in 3ms. No fixes applied.\n' >"$dir/state/backend.out"
  printf 'Checked 2 files in 1ms. No fixes applied.\n' >"$dir/state/model.out"
}

expect() {
  want=$1
  case_name=$2
  dir=$scratch/$case_name
  mkdir -p "$dir/out" || exit 2
  (STUB_STATE=$dir/state static_in "$dir" "$dir/out") >/dev/null 2>&1
  code=$?
  case $code in
    0) got=PASS ;;
    1) got=FAIL ;;
    *) got=exit-$code ;;
  esac
  if [ "$got" = PASS ]; then
    top=$(cd "$dir" && pwd -P)
    printf 'tsc|%s/backend|--noEmit\nbiome|%s/backend|lint .\nbiome|%s|lint --config-path=backend formal/r7-ledger\n' "$top" "$top" "$top" >"$dir/calls.wanted"
    cmp -s "$dir/calls.wanted" "$dir/state/calls" || got=PASS-with-other-calls
  fi
  printf 'case %s/%s want=%s got=%s\n' "$name" "$case_name" "$want" "$got"
  if [ "$got" != "$want" ]; then
    wrong="$wrong $case_name"
  elif [ "$want" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

self_test() {
  scratch=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  rejected=0
  wrong=

  fixture good
  expect PASS good

  fixture typecheck-fails
  printf 'src/a.ts(1,7): error TS2322: Type string is not assignable to type number.\n' >"$scratch/typecheck-fails/state/tsc.out"
  printf '2\n' >"$scratch/typecheck-fails/state/tsc.exit"
  expect FAIL typecheck-fails

  fixture typecheck-prints-an-error-and-exits-0
  printf 'src/a.ts(1,7): error TS2322: Type string is not assignable to type number.\n' >"$scratch/typecheck-prints-an-error-and-exits-0/state/tsc.out"
  expect FAIL typecheck-prints-an-error-and-exits-0

  fixture lint-of-backend-fails
  printf '1\n' >"$scratch/lint-of-backend-fails/state/backend.exit"
  expect FAIL lint-of-backend-fails

  fixture lint-of-the-model-fails
  printf '1\n' >"$scratch/lint-of-the-model-fails/state/model.exit"
  expect FAIL lint-of-the-model-fails

  fixture lint-of-backend-warns-and-exits-0
  printf 'Checked 4 files in 3ms. No fixes applied.\nFound 1 warning.\n' >"$scratch/lint-of-backend-warns-and-exits-0/state/backend.out"
  expect FAIL lint-of-backend-warns-and-exits-0

  fixture lint-of-the-model-warns-and-exits-0
  printf 'Checked 2 files in 1ms. No fixes applied.\nFound 2 warnings.\n' >"$scratch/lint-of-the-model-warns-and-exits-0/state/model.out"
  expect FAIL lint-of-the-model-warns-and-exits-0

  fixture lint-of-backend-reads-no-file
  printf 'Checked 0 files in 1ms. No fixes applied.\n' >"$scratch/lint-of-backend-reads-no-file/state/backend.out"
  expect FAIL lint-of-backend-reads-no-file

  fixture lint-of-the-model-reads-no-file
  printf 'Checked 0 files in 1ms. No fixes applied.\n' >"$scratch/lint-of-the-model-reads-no-file/state/model.out"
  expect FAIL lint-of-the-model-reads-no-file

  fixture lint-of-backend-prints-no-count
  : >"$scratch/lint-of-backend-prints-no-count/state/backend.out"
  expect FAIL lint-of-backend-prints-no-count

  fixture lint-prints-no-count
  : >"$scratch/lint-prints-no-count/state/model.out"
  expect FAIL lint-prints-no-count

  fixture one-file-counts
  printf 'Checked 1 file in 1ms. No fixes applied.\n' >"$scratch/one-file-counts/state/model.out"
  expect PASS one-file-counts

  fixture tools-missing
  mv "$scratch/tools-missing/backend/node_modules" "$scratch/tools-missing/node_modules.moved" || exit 2
  expect FAIL tools-missing

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  '') measure ;;
  *) printf 'FAIL %s (usage)\n' "$name"; exit 1 ;;
esac
