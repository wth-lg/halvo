#!/bin/sh
# jobs: macos
# rule: ci.yml is the only workflow file, actionlint accepts it, every action is pinned to a 40-hex commit, permissions are set at the top level, and the timeout of each job is the bound ci_job_timeout_min.
set -u
name=17-workflow-lint
nl='
'

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

have_actionlint() {
  if ! command -v actionlint >/dev/null 2>&1 && [ "${CI:-}" = true ]; then
    HOMEBREW_NO_AUTO_UPDATE=1 HOMEBREW_NO_INSTALL_CLEANUP=1 brew install actionlint >"$CI_D_ARTIFACTS/brew-install.txt" 2>&1
  fi
  command -v actionlint >/dev/null 2>&1
}

finding() {
  findings=$((findings + 1))
  [ -n "$first" ] || first=$1
  printf 'finding: %s\n' "$1"
}

measure() {
  cd "$CI_D_REPO" || exit 2
  if ! have_actionlint; then
    printf 'SKIP %s (actionlint is not installed)\n' "$name"
    exit 77
  fi
  version=$(actionlint -version) || fail "actionlint -version failed"
  version=$(printf '%s\n' "$version" | sed 1q)
  printf 'actionlint version: %s\n' "$version"
  findings=0
  first=

  file=.github/workflows/ci.yml
  entries=$(ls -A .github/workflows) || fail ".github/workflows cannot be listed"
  [ "$entries" = ci.yml ] ||
    finding ".github/workflows must hold ci.yml and no other file; to add a check, add a script under scripts/<run>/ci.d/, not a workflow file"
  actionlint "$file" || finding "actionlint reports a finding in ci.yml"

  uses=$(grep -c 'uses:' "$file")
  pinned=$(grep -c -E 'uses: [^ ]+@[0-9a-f]{40}( |$)' "$file")
  [ "$uses" -gt 0 ] || finding "ci.yml holds no uses line"
  [ "$uses" -eq "$pinned" ] ||
    finding "$((uses - pinned)) of $uses uses lines of ci.yml take no 40-hex commit; pin the action to a commit and keep the version in a comment"

  permissions=$(grep -c '^permissions:' "$file")
  [ "$permissions" -eq 1 ] ||
    finding "ci.yml must set permissions once at the top level, and it does so $permissions times"

  [ -f scripts/r1-gate/lib.sh ] || fail "scripts/r1-gate/lib.sh is missing"
  GATE_ROOT=$CI_D_REPO
  export GATE_ROOT
  # shellcheck source=scripts/r1-gate/lib.sh disable=SC1091
  . scripts/r1-gate/lib.sh
  jobs=$(grep -c '^    runs-on: ' "$file")
  [ "$jobs" -gt 0 ] || finding "ci.yml holds no job with runs-on at four spaces"
  if bound=$(gate_bound ci_job_timeout_min); then
    case $bound in
      '' | *[!0-9]*) finding "the bound ci_job_timeout_min is no number" ;;
      *)
        timed=$(grep -c -x "    timeout-minutes: $bound" "$file")
        [ "$jobs" -eq "$timed" ] ||
          finding "$timed of $jobs jobs of ci.yml set timeout-minutes: $bound, the bound ci_job_timeout_min"
        ;;
    esac
  else
    finding "the bound ci_job_timeout_min cannot be read with gate_bound of scripts/r1-gate/lib.sh"
  fi

  printf 'jobs: %s, uses lines: %s, findings: %s\n' "$jobs" "$uses" "$findings"
  [ "$findings" -le 1 ] || first="$first; and $((findings - 1)) more above"
  [ "$findings" -eq 0 ] || fail "$first"
  printf 'PASS %s\n' "$name"
}

check() {
  out=$(PATH=$path CI=$ci CI_D_REPO=$work/$1 CI_D_ARTIFACTS=$work/artifacts "$shell" "$0" 2>&1)
  code=$?
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    "77:SKIP $name ("*")") got=SKIP ;;
    *) got=exit-$code ;;
  esac
  case $out in
    *"${3:-}"*) ;;
    *) got=$got-without-the-text ;;
  esac
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$got"
  if [ "$got" != "$2" ]; then
    wrong="$wrong $1"
  elif [ "$2" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
  STUB_BOUND=30
  path=$PATH
  ci=
}

tree() {
  repo=$work/$1
  mkdir -p "$repo/.github/workflows" "$repo/scripts/r1-gate" || exit 2
  sed "$2" "$work/ci.yml" >"$repo/.github/workflows/ci.yml"
  cp "$work/lib.sh" "$repo/scripts/r1-gate/lib.sh"
}

self_test() {
  if ! have_actionlint; then
    printf 'FAIL self-test:%s (actionlint is not installed)\n' "$name"
    exit 1
  fi
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  mkdir -p "$work/artifacts" "$work/no-tools" "$work/tools"
  shell=$(command -v sh) || exit 2
  STUB_REAL=$(command -v actionlint) || exit 2
  STUB_TOOLS=$work/tools
  STUB_BOUND=30
  export STUB_BOUND STUB_REAL STUB_TOOLS
  cat >"$work/tools/brew" <<'EOF'
#!/bin/sh
[ "$*" = 'install actionlint' ] || exit 1
printf '#!/bin/sh\nexec "%s" "$@"\n' "$STUB_REAL" >"$STUB_TOOLS/actionlint"
chmod +x "$STUB_TOOLS/actionlint"
EOF
  chmod +x "$work/tools/brew"
  cat >"$work/lib.sh" <<'EOF'
gate_bound() {
  [ "${GATE_ROOT:-}" = "$CI_D_REPO" ] || return 1
  [ "$1" = ci_job_timeout_min ] && printf '%s\n' "$STUB_BOUND"
}
EOF
  cat >"$work/ci.yml" <<'EOF'
name: ci
on: push
permissions:
  contents: read
jobs:
  macos:
    runs-on: macos-15
    timeout-minutes: 30
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
  windows:
    runs-on: windows-2025
    timeout-minutes: 30
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
EOF
  rejected=0
  wrong=
  STUB_BOUND=30
  path=$PATH
  ci=

  tree pinned-and-timed ''
  check pinned-and-timed PASS 'jobs: 2, uses lines: 2, findings: 0'
  tree action-on-a-tag '10s/@[0-9a-f]* # v7.0.1/@v7/'
  check action-on-a-tag FAIL '1 of 2 uses lines of ci.yml take no 40-hex commit'
  tree action-on-a-short-commit '15s/@[0-9a-f]\{33\}/@/'
  check action-on-a-short-commit FAIL '1 of 2 uses lines of ci.yml take no 40-hex commit'
  tree action-on-41-hex '15s/ # v7.0.1/a # v7.0.1/'
  check action-on-41-hex FAIL '1 of 2 uses lines of ci.yml take no 40-hex commit'
  tree named-step-on-a-tag "10s/.*/      - name: Checkout\\${nl}        uses: actions\\/checkout@v7/"
  check named-step-on-a-tag FAIL '1 of 2 uses lines of ci.yml take no 40-hex commit'
  tree no-uses-line '10s/.*/      - run: "true"/;15s/.*/      - run: "true"/'
  check no-uses-line FAIL 'FAIL 17-workflow-lint (ci.yml holds no uses line)'
  tree second-workflow-file ''
  cp "$work/ci.yml" "$work/second-workflow-file/.github/workflows/package.yml"
  check second-workflow-file FAIL 'add a script under scripts/<run>/ci.d/, not a workflow file)'
  tree hidden-second-workflow-file ''
  cp "$work/ci.yml" "$work/hidden-second-workflow-file/.github/workflows/.package.yml"
  check hidden-second-workflow-file FAIL 'add a script under scripts/<run>/ci.d/, not a workflow file)'
  tree no-permissions '3,4d'
  check no-permissions FAIL 'ci.yml must set permissions once at the top level, and it does so 0 times)'
  tree permissions-at-job-level-only "3,4d;7s/\$/\\${nl}    permissions: {}/"
  check permissions-at-job-level-only FAIL 'ci.yml must set permissions once at the top level, and it does so 0 times)'
  tree job-without-timeout '13d'
  check job-without-timeout FAIL '1 of 2 jobs of ci.yml set timeout-minutes: 30, the bound ci_job_timeout_min)'
  tree step-timeout-in-place-of-the-job-timeout "13d;15s/\$/\\${nl}        timeout-minutes: 30/"
  check step-timeout-in-place-of-the-job-timeout FAIL '1 of 2 jobs of ci.yml set timeout-minutes: 30, the bound ci_job_timeout_min)'
  tree job-keys-at-another-indent '6,15s/^/  /;8d;13d'
  check job-keys-at-another-indent FAIL 'FAIL 17-workflow-lint (ci.yml holds no job with runs-on at four spaces)'
  tree timeout-above-the-bound '8s/30/31/'
  check timeout-above-the-bound FAIL '1 of 2 jobs of ci.yml set timeout-minutes: 30, the bound ci_job_timeout_min)'
  tree timeout-ten-times-the-bound '8s/30/300/'
  check timeout-ten-times-the-bound FAIL '1 of 2 jobs of ci.yml set timeout-minutes: 30, the bound ci_job_timeout_min)'
  tree bound-moved ''
  STUB_BOUND=45
  check bound-moved FAIL '0 of 2 jobs of ci.yml set timeout-minutes: 45, the bound ci_job_timeout_min)'
  tree bound-is-no-number ''
  STUB_BOUND=30m
  check bound-is-no-number FAIL 'the bound ci_job_timeout_min is no number)'
  tree bound-unreadable ''
  printf 'gate_bound() {\n  return 1\n}\n' >"$work/bound-unreadable/scripts/r1-gate/lib.sh"
  check bound-unreadable FAIL 'the bound ci_job_timeout_min cannot be read with gate_bound of scripts/r1-gate/lib.sh)'
  tree actionlint-finding '2s/push/pushed/'
  check actionlint-finding FAIL 'actionlint reports a finding in ci.yml)'
  tree two-findings '10s/@[0-9a-f]* # v7.0.1/@v7/;3,4d'
  check two-findings FAIL '1 of 2 uses lines of ci.yml take no 40-hex commit; pin the action to a commit and keep the version in a comment; and 1 more above)'
  tree actionlint-missing ''
  path=$work/no-tools
  check actionlint-missing SKIP 'SKIP 17-workflow-lint (actionlint is not installed)'
  tree actionlint-installed-in-ci ''
  path=$work/tools:/usr/bin:/bin
  ci=true
  check actionlint-installed-in-ci PASS 'jobs: 2, uses lines: 2, findings: 0'

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
