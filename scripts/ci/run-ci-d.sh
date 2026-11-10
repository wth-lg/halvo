#!/bin/sh
set -u
LC_ALL=C
export LC_ALL

usage() {
  cat <<'EOF'
usage: sh scripts/ci/run-ci-d.sh <job> [--self-test] [NAME...]
       sh scripts/ci/run-ci-d.sh --list [<job>]
       sh scripts/ci/run-ci-d.sh --rules
<job> is macos or windows. Start it at the repo root.
scripts/ci/README.md states the interface.
EOF
}

nl='
'
root=${CI_D_SCRIPTS:-scripts}
mode=run
job=
wanted=

case ${1:-} in
  -h | --help)
    usage
    exit 0
    ;;
  --rules)
    [ $# -eq 1 ] || { usage >&2; exit 64; }
    mode=rules
    ;;
  --list)
    [ $# -le 2 ] || { usage >&2; exit 64; }
    mode=list
    job=${2:-}
    ;;
  macos | windows)
    job=$1
    shift
    for arg in "$@"; do
      case $arg in
        --self-test) mode='self-test' ;;
        '' | -* | *[!a-z0-9-]*) usage >&2; exit 64 ;;
        *) wanted="$wanted $arg" ;;
      esac
    done
    ;;
  *)
    usage >&2
    exit 64
    ;;
esac
case $mode:$job in
  rules: | list: | *:macos | *:windows) ;;
  *) usage >&2; exit 64 ;;
esac

cdup=$(git rev-parse --show-cdup 2>/dev/null)
code=$?
if [ "$code" -ne 0 ] || [ -n "$cdup" ]; then
  printf 'FAIL run-ci-d (start it at the root of the git work tree)\n'
  exit 1
fi
repo=$(pwd)

inspect() {
  script_file=$1
  script_name=${script_file##*/}
  script_name=${script_name%.sh}
  script_jobs=
  script_rule=
  case ${script_name#[0-9][0-9]-} in
    "$script_name" | '' | *[!a-z0-9-]*)
      refusal="the file name is not NN-name.sh, two digits and a name of a-z, 0-9 and -"
      return 1
      ;;
  esac
  script_jobs=$(sed -n '1,10s/^# jobs: //p' "$script_file")
  case $script_jobs in
    '' | *"$nl"*)
      refusal="its first 10 lines hold no single '# jobs:' line"
      return 1
      ;;
    *[!a-z\ ]*)
      refusal="its '# jobs:' line names something other than macos and windows"
      return 1
      ;;
  esac
  for word in $script_jobs; do
    case $word in
      macos | windows) ;;
      *)
        refusal="its '# jobs:' line names $word; a job is macos or windows"
        return 1
        ;;
    esac
  done
  script_rule=$(sed -n '1,10s/^# rule: //p' "$script_file")
  case $script_rule in
    '' | *"$nl"*)
      refusal="its first 10 lines hold no single '# rule:' line"
      return 1
      ;;
  esac
  refusal=
  return 0
}

names_job() {
  case " $script_jobs " in
    *" $1 "*) return 0 ;;
  esac
  return 1
}

if [ "$mode" = list ] || [ "$mode" = rules ]; then
  status=0
  count=0
  for file in "$root"/*/ci.d/*.sh; do
    [ -f "$file" ] || continue
    if ! inspect "$file"; then
      printf 'FAIL %s (%s)\n' "$script_name" "$refusal"
      status=1
      continue
    fi
    if [ "$mode" = rules ]; then
      printf '%s (%s): %s\n' "$script_name" "$script_jobs" "$script_rule"
    else
      [ -z "$job" ] || names_job "$job" || continue
      printf '%s\n' "$file"
    fi
    count=$((count + 1))
  done
  if [ "$count" -eq 0 ] && [ "$status" -eq 0 ]; then
    printf 'FAIL run-ci-d (no scripts%s)\n' "${job:+ for $job}"
    status=1
  fi
  exit "$status"
fi

tmp=$(mktemp -d "${TMPDIR:-/tmp}/run-ci-d.XXXXXX") || {
  printf 'FAIL run-ci-d (mktemp failed)\n'
  exit 1
}
recap=$tmp/recap
: >"$recap"
passed=0
failed=0
skipped=0
status=0

cargo_home=${CARGO_HOME:-$HOME/.cargo}
if command -v cygpath >/dev/null 2>&1; then
  cargo_home=$(cygpath -u "$cargo_home")
fi
PATH=$cargo_home/bin:$PATH
export PATH

case $0 in
  /*) self=$0 ;;
  *) self=$repo/$0 ;;
esac

record() {
  printf '%s\n' "$2" >>"$recap"
  case $1 in
    PASS) passed=$((passed + 1)) ;;
    FAIL) failed=$((failed + 1)) ;;
    SKIP) skipped=$((skipped + 1)) ;;
  esac
}

judge() {
  if [ "$mode" = self-test ]; then
    tag=self-test:$script_name
    case $code:$last in
      "0:PASS $tag ("*" failing fixtures rejected)")
        k=${last#"PASS $tag ("}
        k=${k%" failing fixtures rejected)"}
        case $k in
          '' | *[!0-9]*) record FAIL "FAIL $tag (exit 0)" ;;
          *)
            if [ "$k" -gt 0 ]; then
              record PASS "$last"
            else
              record FAIL "FAIL $tag (0 failing fixtures rejected)"
            fi
            ;;
        esac
        ;;
      "1:FAIL $tag ("*")") record FAIL "$last" ;;
      *) record FAIL "FAIL $tag (exit $code)" ;;
    esac
    return
  fi
  case $code:$last in
    "0:PASS $script_name") record PASS "$last" ;;
    "1:FAIL $script_name ("*")") record FAIL "$last" ;;
    "77:SKIP $script_name ("*")")
      if [ "${CI:-}" = true ]; then
        reason=${last#"SKIP $script_name ("}
        record FAIL "FAIL $script_name (skipped in CI: $reason"
      else
        record SKIP "$last"
      fi
      ;;
    *) record FAIL "FAIL $script_name (exit $code)" ;;
  esac
}

run_script() {
  out=$tmp/$script_name.out
  code_file=$tmp/$script_name.code
  artifacts=$repo/target/ci-artifacts/$script_name
  mkdir -p "$artifacts" || {
    record FAIL "FAIL $script_name (its artifacts directory cannot be made)"
    return
  }
  if [ "$mode" = self-test ]; then
    set -- --self-test
  else
    set --
  fi
  started=$(date +%s)
  (
    CI_D_REPO=$repo
    CI_D_JOB=$job
    CI_D_ARTIFACTS=$artifacts
    export CI_D_REPO CI_D_JOB CI_D_ARTIFACTS
    sh "$script_file" "$@" </dev/null
    printf '%s\n' "$?" >"$code_file"
  ) | tee "$out"
  ended=$(date +%s)
  printf 'seconds %s %s\n' "$script_name" "$((ended - started))"
  code=$(cat "$code_file" 2>/dev/null)
  last=$(tail -n 1 "$out")
  judge
}

new_repo() {
  fixture=$(mktemp -d "$tmp/repo.XXXXXX") || return 1
  git -C "$fixture" init -q >/dev/null 2>&1 || return 1
  printf 'version = 4\n' >"$fixture/Cargo.lock"
  git -C "$fixture" add Cargo.lock >/dev/null 2>&1
}

add_script() {
  mkdir -p "$fixture/scripts/$1/ci.d" || return 1
  printf '#!/bin/sh\n%s\n%s\n' "$3" "$4" >"$fixture/scripts/$1/ci.d/$2"
}

own_case() {
  case_name=$1
  want_exit=$2
  want_line=$3
  shift 3
  got_out=$(
    cd "$fixture/$case_cd" || exit 99
    unset CI CI_D_SCRIPTS
    [ -z "$case_ci" ] || { CI=$case_ci; export CI; }
    [ -z "$case_scripts" ] || { CI_D_SCRIPTS=$case_scripts; export CI_D_SCRIPTS; }
    [ -z "$case_locale" ] || { LC_ALL=$case_locale; export LC_ALL; }
    CARGO_HOME=$tmp/cargo-home
    TMPDIR=$tmp
    export CARGO_HOME TMPDIR
    sh "$self" "$@" 2>&1
  )
  got_exit=$?
  if [ "$want_exit" -eq 0 ]; then want=PASS; else want=FAIL; fi
  if [ "$got_exit" -ne "$want_exit" ]; then
    got=exit-$got_exit
  elif printf '%s\n' "$got_out" | grep -qxF -e "$want_line"; then
    got=$want
  else
    got=line-missing
  fi
  printf 'case run-ci-d/%s want=%s got=%s\n' "$case_name" "$want" "$got"
  if [ "$got" != "$want" ]; then
    wrong="$wrong $case_name"
  elif [ "$want" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
  case_ci=
  case_scripts=
  case_cd=
  case_locale=
}

# shellcheck disable=SC2016
own_self_test() {
  good="# jobs: macos windows$nl# rule: A fixture rule."
  seven="#$nl#$nl#$nl#$nl#$nl#$nl#"
  pass_a='[ $# -eq 0 ] || exit 3; printf "PASS 10-a\n"'
  pass_b='printf "PASS 20-b\n"'
  fail_a='printf "FAIL 10-a (planned)\n"; exit 1'
  skip_a='printf "SKIP 10-a (no tool)\n"; exit 77'
  rejected=0
  wrong=
  case_ci=
  case_scripts=
  case_cd=
  case_locale=

  new_repo && add_script r1 10-a.sh "$good" "$pass_a" && add_script r1 20-b.sh "$good" "$pass_b"
  own_case all-pass 0 'ci.d macos: 2 passed, 0 failed, 0 skipped' macos
  own_case rules 0 '10-a (macos windows): A fixture rule.' --rules
  own_case list 0 'scripts/r1/ci.d/20-b.sh' --list macos
  own_case name-matches-no-script 1 'FAIL run-ci-d (no script 99-none for macos)' macos 99-none
  own_case one-name-matches-no-script 1 'FAIL run-ci-d (no script 99-none for macos)' macos 20-b 99-none
  own_case unknown-job 64 'usage: sh scripts/ci/run-ci-d.sh <job> [--self-test] [NAME...]' linux
  case_cd=scripts
  own_case below-the-root 1 'FAIL run-ci-d (start it at the root of the git work tree)' macos
  case_scripts=$(mktemp -d "$tmp/empty.XXXXXX")
  own_case no-scripts 1 'FAIL run-ci-d (no scripts for macos)' macos

  new_repo && add_script r1 10-a.sh '# rule: A fixture rule.' "$pass_a" && add_script r1 20-b.sh "$good" "$pass_b"
  own_case no-jobs-line 1 "FAIL 10-a (its first 10 lines hold no single '# jobs:' line)" macos
  own_case rules-refuses 1 "FAIL 10-a (its first 10 lines hold no single '# jobs:' line)" --rules
  new_repo && add_script r1 10-a.sh '# jobs: macos' "$pass_a"
  own_case no-rule-line 1 "FAIL 10-a (its first 10 lines hold no single '# rule:' line)" macos
  new_repo && add_script r1 10-a.sh "$seven$nl$good" "$pass_a"
  own_case header-ends-at-line-10 0 'ci.d macos: 1 passed, 0 failed, 0 skipped' macos
  new_repo && add_script r1 10-a.sh "$seven$nl#$nl$good" "$pass_a"
  own_case rule-at-line-11 1 "FAIL 10-a (its first 10 lines hold no single '# rule:' line)" macos
  new_repo && add_script r1 10-a.sh "$seven$nl#$nl# rule: A fixture rule.$nl# jobs: macos" "$pass_a"
  own_case jobs-at-line-11 1 "FAIL 10-a (its first 10 lines hold no single '# jobs:' line)" macos
  new_repo && add_script r1 10-a.sh "# jobs: macos linux$nl# rule: A fixture rule." "$pass_a"
  own_case unknown-job-word 1 "FAIL 10-a (its '# jobs:' line names linux; a job is macos or windows)" macos
  new_repo && add_script r1 format.sh "$good" 'printf "PASS format\n"'
  own_case file-name 1 'FAIL format (the file name is not NN-name.sh, two digits and a name of a-z, 0-9 and -)' macos
  new_repo && add_script r1 10-a.sh "$good" "$pass_a" && add_script r2 10-a.sh "$good" "$pass_a"
  own_case name-used-twice 1 'FAIL 10-a (a second script has this name: scripts/r2/ci.d/10-a.sh)' macos

  new_repo && add_script r1 10-a.sh "$good" 'printf "PASS 10-a\n"; exit 1'
  own_case pass-line-with-exit-1 1 'FAIL 10-a (exit 1)' macos
  new_repo && add_script r1 10-a.sh "$good" 'printf "done\n"'
  own_case exit-0-without-pass-line 1 'FAIL 10-a (exit 0)' macos
  new_repo && add_script r1 10-a.sh "$good" "$fail_a" && add_script r1 20-b.sh "$good" "$pass_b"
  own_case fail-does-not-stop-the-run 1 'ci.d macos: 1 passed, 1 failed, 0 skipped' macos
  own_case name-selects-one 0 'ci.d macos: 1 passed, 0 failed, 0 skipped' macos 20-b
  new_repo && add_script r1 10-a.sh "$good" "$skip_a" && add_script r1 20-b.sh "$good" "$pass_b"
  own_case skip-outside-ci 0 'ci.d macos: 1 passed, 0 failed, 1 skipped' macos
  case_ci=true
  own_case skip-in-ci 1 'FAIL 10-a (skipped in CI: no tool)' macos
  new_repo && add_script r1 10-a.sh "$good" "$skip_a"
  own_case every-script-skipped 1 'ci.d macos: 0 passed, 0 failed, 1 skipped' macos
  new_repo && add_script r1 10-a.sh "# jobs: windows$nl# rule: A fixture rule." "$pass_a"
  own_case other-job-only 1 'FAIL run-ci-d (no scripts for macos)' macos
  own_case job-named 0 'ci.d windows: 1 passed, 0 failed, 0 skipped' windows
  own_case list-other-job-only 1 'FAIL run-ci-d (no scripts for macos)' --list macos
  new_repo && add_script r1 10-a.sh "$good" 'printf "x\n" >>"$CI_D_REPO/Cargo.lock"; printf "PASS 10-a\n"'
  own_case lockfile-changed 1 'FAIL run-ci-d (Cargo.lock or package-lock.json differs from the index; a script builds with --locked and changes no tracked file)' macos

  new_repo && add_script r1 10-a.sh "$good" '[ "${1:-}" = --self-test ] || exit 3; printf "PASS self-test:10-a (2 failing fixtures rejected)\n"'
  own_case self-test-rejects-two 0 'ci.d macos self-test: 1 passed, 0 failed' macos --self-test 10-a
  new_repo && add_script r1 10-a.sh "$good" 'printf "PASS self-test:10-a (0 failing fixtures rejected)\n"'
  own_case self-test-rejects-nothing 1 'FAIL self-test:10-a (0 failing fixtures rejected)' macos --self-test 10-a
  new_repo && add_script r1 10-a.sh "$good" 'printf "PASS self-test:10-a (no failing fixtures rejected)\n"'
  own_case self-test-count-is-no-number 1 'FAIL self-test:10-a (exit 0)' macos --self-test 10-a

  new_repo && add_script B 10-x.sh "$good" ': >"$CI_D_REPO/first-ran"; printf "PASS 10-x\n"' &&
    add_script a 20-y.sh "$good" '[ -e "$CI_D_REPO/first-ran" ] || { printf "FAIL 20-y (it ran first)\n"; exit 1; }; printf "PASS 20-y\n"'
  case_locale=en_US.UTF-8
  own_case byte-order 0 'ci.d macos: 2 passed, 0 failed, 0 skipped' macos
  new_repo && add_script r1 10-a.sh "$good" 'here=$(pwd); [ "$CI_D_REPO" = "$here" ] && [ "$CI_D_JOB" = macos ] && [ "$CI_D_ARTIFACTS" = "$here/target/ci-artifacts/10-a" ] && [ -d "$CI_D_ARTIFACTS" ] && [ "$LC_ALL" = C ] && [ "${PATH%%:*}" = "$CARGO_HOME/bin" ] || { printf "FAIL 10-a (environment)\n"; exit 1; }; printf "PASS 10-a\n"'
  case_locale=en_US.UTF-8
  own_case environment 0 'ci.d macos: 1 passed, 0 failed, 0 skipped' macos

  if [ -n "$wrong" ]; then
    record FAIL "FAIL self-test:run-ci-d (wrong verdict in:$wrong)"
  elif [ "$rejected" -eq 0 ]; then
    record FAIL "FAIL self-test:run-ci-d (0 failing fixtures rejected)"
  else
    record PASS "PASS self-test:run-ci-d ($rejected failing fixtures rejected)"
  fi
}

if [ "$mode" = self-test ]; then
  case " $wanted " in
    '  ' | *' run-ci-d '*) own_self_test ;;
  esac
fi

seen=' '
ran=' run-ci-d '
for file in "$root"/*/ci.d/*.sh; do
  [ -f "$file" ] || continue
  if ! inspect "$file"; then
    record FAIL "FAIL $script_name ($refusal)"
    continue
  fi
  case $seen in
    *" $script_name "*)
      record FAIL "FAIL $script_name (a second script has this name: $file)"
      continue
      ;;
  esac
  seen="$seen$script_name "
  names_job "$job" || continue
  if [ -n "$wanted" ]; then
    case " $wanted " in
      *" $script_name "*) ;;
      *) continue ;;
    esac
  fi
  ran="$ran$script_name "
  run_script
done

for name in $wanted; do
  case $ran in
    *" $name "*) ;;
    *)
      printf 'FAIL run-ci-d (no script %s for %s)\n' "$name" "$job"
      status=1
      ;;
  esac
done

if [ "$((passed + failed + skipped))" -eq 0 ]; then
  [ "$status" -ne 0 ] || printf 'FAIL run-ci-d (no scripts for %s)\n' "$job"
  exit 1
fi

cat "$recap"
if [ "$mode" = self-test ]; then
  printf 'ci.d %s self-test: %s passed, %s failed\n' "$job" "$passed" "$failed"
else
  printf 'ci.d %s: %s passed, %s failed, %s skipped\n' "$job" "$passed" "$failed" "$skipped"
fi
if [ "$failed" -gt 0 ] || [ "$passed" -eq 0 ]; then
  status=1
fi
if ! git diff --exit-code -- Cargo.lock package-lock.json; then
  printf 'FAIL run-ci-d (Cargo.lock or package-lock.json differs from the index; a script builds with --locked and changes no tracked file)\n'
  status=1
fi
exit "$status"
