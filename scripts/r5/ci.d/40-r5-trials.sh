#!/bin/sh
# jobs: macos windows
# rule: The release profile of tools/r5-trials/Cargo.toml holds overflow-checks = true, each trial refuses a wrong call, the trials' workspace tests pass with --locked, and each trial prints the block of its result file under docs/r5/trials/ and the bytes of the routine sample.
set -u
LC_ALL=C
export LC_ALL
name=40-r5-trials
trials='routine stat drift'
fence=$(printf '\140\140\140')

finish() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

note() {
  printf '%s\n' "$*" >>"$art/checks.txt"
}

profile_ok() {
  [ -f "$1" ] || return 1
  awk '
    {
      line = $0
      sub(/^[ \t]+/, "", line)
      sub(/[ \t]+$/, "", line)
    }
    line == "[profile.release]" { inside = 1; next }
    substr(line, 1, 1) == "[" { inside = 0 }
    inside && line == "overflow-checks = true" { found = 1 }
    END { if (!found) exit 1 }
  ' "$1"
}

refusal_ok() {
  code=$1
  out_file=$2
  err_file=$3
  root=$4
  cause=$5
  [ "$code" -eq 2 ] || return 1
  [ -f "$out_file" ] && [ ! -s "$out_file" ] || return 1
  lines=$(wc -l <"$err_file") || return 1
  [ "$((lines))" -eq 1 ] || return 1
  first=$(head -n 1 "$err_file") || return 1
  bytes=$(wc -c <"$err_file") || return 1
  [ "$((bytes))" -eq "$((${#first} + 1))" ] || return 1
  case $first in
    "REFUSED $cause: "*) ;;
    *) return 1 ;;
  esac
  [ ! -e "$root/target" ]
}

refuse_call() {
  trial=$1
  call=$2
  cause=$3
  shift 3
  base=$art/refusal-$trial-$call
  : >"$base.out"
  if [ "$call" = closed ]; then
    sh "$copy" >&- 2>"$base.err"
  else
    sh "$copy" "$@" >"$base.out" 2>"$base.err"
  fi
  code=$?
  if refusal_ok "$code" "$base.out" "$base.err" "$d" "$cause"; then
    note "refusal trial=$trial call=$call exit=$code ok=yes"
    return 0
  fi
  note "refusal trial=$trial call=$call exit=$code ok=no"
  return 1
}

block_of() {
  awk '
    !inside && $0 == "```" { inside = 1; next }
    inside && $0 == "```" { closed = 1; exit 0 }
    inside { print }
    END { if (!closed) exit 1 }
  ' "$1"
}

result_of() {
  case $1 in
    routine) printf 'docs/r5/trials/routine.md\n' ;;
    stat) printf 'docs/r5/trials/stat.md\n' ;;
    drift) printf 'docs/r5/trials/admission.md\n' ;;
  esac
}

compare_trial() {
  md=$(result_of "$1")
  if [ ! -d "tools/r5-trials/$1" ] || [ ! -e "$md" ]; then
    printf 'absent\n'
  elif block_of "$md" >"$art/$1.expected" && cmp -s "$art/$1.expected" "$art/$1.out"; then
    printf 'same\n'
  else
    printf 'differ\n'
  fi
}

measure() {
  if [ -n "${CI_D_REPO:-}" ]; then
    repo=$CI_D_REPO
  else
    here=$(dirname "$0")
    repo=$(cd -P "$here/../../.." && pwd -P) || exit 2
  fi
  art=${CI_D_ARTIFACTS:-$repo/target/ci-artifacts/$name}
  mkdir -p "$art" || exit 2
  art=$(cd -P "$art" && pwd -P) || exit 2
  cd "$repo" || exit 2
  : >"$art/checks.txt"

  if ! profile_ok tools/r5-trials/Cargo.toml; then
    note "profile overflow_checks=no"
    finish profile
  fi
  note "profile overflow_checks=yes"

  for trial in $trials; do
    [ -d "tools/r5-trials/$trial" ] || continue
    d=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
    copy=$d/tools/r5-trials/$trial/run.sh
    if ! { mkdir -p "$d/tools/r5-trials/$trial" && cp "tools/r5-trials/$trial/run.sh" "$copy"; }; then
      note "refusal trial=$trial copy=no"
      finish refusal
    fi
    refuse_call "$trial" argument usage --no-such-argument || finish refusal
    refuse_call "$trial" help-extra usage --help extra || finish refusal
    refuse_call "$trial" closed output || finish refusal
  done

  if ! command -v cargo >/dev/null 2>&1; then
    note "test cargo=missing"
    finish test
  fi
  cargo test --locked --manifest-path tools/r5-trials/Cargo.toml --workspace --target-dir target/r5-trials >"$art/cargo-test.txt" 2>&1
  code=$?
  note "test exit=$code"
  [ "$code" -eq 0 ] || finish test

  for trial in $trials; do
    [ -d "tools/r5-trials/$trial" ] || continue
    sh "tools/r5-trials/$trial/run.sh" >"$art/$trial.out" 2>"$art/$trial.err"
    code=$?
    note "run trial=$trial exit=$code"
    case $trial:$code in
      *:0 | stat:1) ;;
      *) finish run ;;
    esac
  done
  if [ -e fixtures/r5/trains/sample.jsonl ]; then
    : >"$art/sample.jsonl"
    sh tools/r5-trials/routine/run.sh --sample "$art/sample.jsonl" >"$art/sample.out" 2>"$art/sample.err"
    code=$?
    note "run sample exit=$code"
    [ "$code" -eq 0 ] || finish run
  fi

  routine=$(compare_trial routine)
  stat=$(compare_trial stat)
  admission=$(compare_trial drift)
  if [ ! -e fixtures/r5/trains/sample.jsonl ]; then
    sample=absent
  elif cmp -s fixtures/r5/trains/sample.jsonl "$art/sample.jsonl"; then
    sample=same
  else
    sample=differ
  fi
  printf 'trials routine=%s stat=%s admission=%s sample=%s\n' "$routine" "$stat" "$admission" "$sample"
  case "$routine $stat $admission $sample" in
    *differ*) finish differ ;;
  esac
  printf 'PASS %s\n' "$name"
}

stub_trial() {
  cat <<'EOF'
#!/bin/sh
trial=$(basename "$(dirname "$0")")
refuse() {
  [ "${STUB_REFUSAL_STDOUT:-}" != "$trial" ] || printf 'a line on standard output\n'
  [ "${STUB_REFUSAL_TARGET:-}" != "$trial" ] || mkdir -p "$(dirname "$0")/../../../target"
  printf 'REFUSED %s: %s\n' "$1" "stub" >&2
  [ "${STUB_REFUSAL_TWO_LINES:-}" != "$trial" ] || printf 'a second line\n' >&2
  exit "${STUB_REFUSAL_EXIT:-2}"
}
if ! true 2>/dev/null 3>&1; then
  [ "${STUB_IGNORE_CLOSED:-}" = "$trial" ] && exit 0
  refuse output
fi
case $# in
  0)
    if [ "${STUB_RUN_REFUSES:-}" = "$trial" ]; then
      printf 'REFUSED build: stub\n' >&2
      exit 2
    fi
    if [ "${STUB_STAT_MISS:-}" = yes ] && [ "$trial" = stat ]; then
      printf 'block stat\nverdict stat=miss\n'
      exit 1
    fi
    printf 'block %s\nverdict %s=ok\n' "$trial" "$trial"
    exit "${STUB_RUN_EXIT:-0}"
    ;;
  2)
    if [ "$trial" = routine ] && [ "$1" = --sample ]; then
      [ -z "${STUB_SAMPLE_EXIT:-}" ] || exit "$STUB_SAMPLE_EXIT"
      printf '{"cell":"7-0-0"}\n' >"$2"
      exit 0
    fi
    ;;
esac
refuse usage
EOF
}

stub_cargo() {
  cat <<'EOF'
#!/bin/sh
[ -f fixture-root ] || { printf 'stub cargo: started outside the fixture root\n'; exit 98; }
[ "$*" = 'test --locked --manifest-path tools/r5-trials/Cargo.toml --workspace --target-dir target/r5-trials' ] ||
  { printf 'stub cargo: got the arguments: %s\n' "$*"; exit 97; }
exit "${STUB_CARGO_EXIT:-0}"
EOF
}

result_text() {
  printf 'date 2026-11-11\nmachine mac\n\n%s\nblock %s\nverdict %s=%s\n%s\n' "$fence" "$1" "$1" "${2:-ok}" "$fence"
}

fixture() {
  r=$work/$1
  mkdir -p "$r/docs/r5/trials" "$r/fixtures/r5/trains" || exit 2
  : >"$r/fixture-root"
  printf '[workspace]\nmembers = [\n    "stat",\n]\n\n[profile.release]\noverflow-checks = true\n' >"$r/Cargo.toml.fixture"
  for trial in $trials; do
    mkdir -p "$r/tools/r5-trials/$trial" || exit 2
    stub_trial >"$r/tools/r5-trials/$trial/run.sh"
  done
  cp "$r/Cargo.toml.fixture" "$r/tools/r5-trials/Cargo.toml" || exit 2
  result_text routine >"$r/docs/r5/trials/routine.md"
  result_text stat >"$r/docs/r5/trials/stat.md"
  result_text drift >"$r/docs/r5/trials/admission.md"
  printf '{"cell":"7-0-0"}\n' >"$r/fixtures/r5/trains/sample.jsonl"
}

check() {
  case_name=$1
  want=$2
  text=$3
  shift 3
  out=$(env "$@" PATH="${case_path:-$work/bin:$PATH}" CI_D_REPO="$work/$case_name" CI_D_ARTIFACTS="$work/artifacts-$case_name" sh "$self" 2>&1)
  code=$?
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")")
      got=${last#"FAIL $name ("}
      got=FAIL-${got%)}
      ;;
    *) got=exit-$code ;;
  esac
  case $out in
    *"$text"*) ;;
    *) got=$got-without-the-text ;;
  esac
  printf 'case %s/%s want=%s got=%s\n' "$name" "$case_name" "$want" "$got"
  if [ "$got" != "$want" ]; then
    wrong="$wrong $case_name"
  elif [ "$want" != PASS ]; then
    rejected=$((rejected + 1))
  fi
}

self_test() {
  self=$0
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  mkdir -p "$work/bin" || exit 2
  stub_cargo >"$work/bin/cargo"
  chmod +x "$work/bin/cargo" || exit 2
  rejected=0
  wrong=
  case_path=
  all_same='trials routine=same stat=same admission=same sample=same'

  fixture good
  check good PASS "$all_same"

  fixture profile-under-another-header
  printf '[workspace]\nmembers = [\n    "stat",\n]\n\n[profile.release]\nopt-level = 3\n\n[profile.dev]\noverflow-checks = true\n' >"$work/profile-under-another-header/tools/r5-trials/Cargo.toml"
  check profile-under-another-header FAIL-profile 'FAIL 40-r5-trials (profile)'

  fixture profile-indented-header-before-the-line
  printf '[workspace]\nmembers = [\n    "stat",\n]\n\n[profile.release]\n  [profile.dev]\noverflow-checks = true\n' >"$work/profile-indented-header-before-the-line/tools/r5-trials/Cargo.toml"
  check profile-indented-header-before-the-line FAIL-profile 'FAIL 40-r5-trials (profile)'

  fixture profile-indented-release-header
  printf '[workspace]\nmembers = [\n    "stat",\n]\n\n  [profile.release]\n  overflow-checks = true\n' >"$work/profile-indented-release-header/tools/r5-trials/Cargo.toml"
  check profile-indented-release-header PASS "$all_same"

  fixture profile-false
  printf '[workspace]\nmembers = [\n    "stat",\n]\n\n[profile.release]\noverflow-checks = false\n' >"$work/profile-false/tools/r5-trials/Cargo.toml"
  check profile-false FAIL-profile 'FAIL 40-r5-trials (profile)'

  fixture profile-first-and-block-differs
  printf '[workspace]\n' >"$work/profile-first-and-block-differs/tools/r5-trials/Cargo.toml"
  result_text drift 0k >"$work/profile-first-and-block-differs/docs/r5/trials/admission.md"
  check profile-first-and-block-differs FAIL-profile 'FAIL 40-r5-trials (profile)'

  fixture refusal-with-a-line-on-standard-output
  check refusal-with-a-line-on-standard-output FAIL-refusal 'FAIL 40-r5-trials (refusal)' STUB_REFUSAL_STDOUT=drift

  fixture refusal-exits-1
  check refusal-exits-1 FAIL-refusal 'FAIL 40-r5-trials (refusal)' STUB_REFUSAL_EXIT=1

  fixture refusal-with-two-lines-on-standard-error
  check refusal-with-two-lines-on-standard-error FAIL-refusal 'FAIL 40-r5-trials (refusal)' STUB_REFUSAL_TWO_LINES=stat

  fixture refusal-leaves-a-build-directory
  check refusal-leaves-a-build-directory FAIL-refusal 'FAIL 40-r5-trials (refusal)' STUB_REFUSAL_TARGET=routine

  fixture closed-output-not-refused
  check closed-output-not-refused FAIL-refusal 'FAIL 40-r5-trials (refusal)' STUB_IGNORE_CLOSED=drift

  fixture cargo-test-fails
  check cargo-test-fails FAIL-test 'FAIL 40-r5-trials (test)' STUB_CARGO_EXIT=101

  fixture cargo-missing
  ifs=$IFS
  IFS=:
  for dir in $PATH; do
    [ -x "$dir/cargo" ] || [ -x "$dir/cargo.exe" ] || case_path=${case_path:+$case_path:}$dir
  done
  IFS=$ifs
  check cargo-missing FAIL-test 'FAIL 40-r5-trials (test)'
  case_path=

  fixture trial-refuses-its-run
  check trial-refuses-its-run FAIL-run 'FAIL 40-r5-trials (run)' STUB_RUN_REFUSES=drift

  fixture routine-exits-1
  check routine-exits-1 FAIL-run 'FAIL 40-r5-trials (run)' STUB_RUN_EXIT=1

  fixture sample-call-fails
  check sample-call-fails FAIL-run 'FAIL 40-r5-trials (run)' STUB_SAMPLE_EXIT=2

  fixture stat-miss-exits-1
  check stat-miss-exits-1 FAIL-differ 'trials routine=same stat=differ admission=same sample=same' STUB_STAT_MISS=yes

  fixture block-differs-in-one-line
  result_text drift 0k >"$work/block-differs-in-one-line/docs/r5/trials/admission.md"
  check block-differs-in-one-line FAIL-differ 'trials routine=same stat=same admission=differ sample=same'

  fixture sample-differs
  printf '{"cell":"7-0-1"}\n' >"$work/sample-differs/fixtures/r5/trains/sample.jsonl"
  check sample-differs FAIL-differ 'trials routine=same stat=same admission=same sample=differ'

  fixture result-file-absent
  mv "$work/result-file-absent/docs/r5/trials/stat.md" "$work/result-file-absent/stat.md.moved" || exit 2
  check result-file-absent PASS 'trials routine=same stat=absent admission=same sample=same'

  fixture sample-absent-and-not-run
  mv "$work/sample-absent-and-not-run/fixtures/r5/trains/sample.jsonl" "$work/sample-absent-and-not-run/sample.jsonl.moved" || exit 2
  check sample-absent-and-not-run PASS 'trials routine=same stat=same admission=same sample=absent' STUB_SAMPLE_EXIT=2

  fixture trial-directory-absent
  mv "$work/trial-directory-absent/tools/r5-trials/drift" "$work/trial-directory-absent/drift.moved" || exit 2
  check trial-directory-absent PASS 'trials routine=same stat=same admission=absent sample=same'

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
