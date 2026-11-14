#!/bin/sh
# jobs: macos windows
# rule: On a hosted runner the job's own run of the dialog targets of halvo-coord-app prints each target's own end line with every case of that system run and none failed (dialog_hosted 4 of 4 on macOS and 5 of 5 on Windows, guard_hosted 5 of 5 on macOS and 7 of 7 on Windows) and no line that a target was not run, cargo exits 0 and no tracked file differs from HEAD; off a hosted runner it skips.
set -u
name=46-r4-dialog-ran
off='hosted alone: not run on this machine'
targets='dialog_hosted 4 5
guard_hosted 5 7'

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

broke() {
  printf '%s\n' "$1"
  broken="$broken${broken:+; }$1"
}

wanted() {
  printf '%s\n' "$targets" | while read -r target macos windows; do
    case $1 in
      macos) printf '%s %s\n' "$target" "$macos" ;;
      *) printf '%s %s\n' "$target" "$windows" ;;
    esac
  done
}

decide() {
  log=$1
  code=$2
  want=$3
  [ "$code" = 0 ] || broke "cargo test -p halvo-coord-app --locked --test dialog_hosted --test guard_hosted exited $code"
  offs=$(grep -c -x -F "$off" "$log")
  [ "$offs" = 0 ] || broke "a target printed the line that it was not run, $offs times"
  while read -r target count; do
    line="$target end: cases ran $count of $count, failed 0"
    ends=$(grep -c "^$target end: " "$log")
    exact=$(grep -c -x -F "$line" "$log")
    printf '%s: end lines %s, of them the line of %s of %s with none failed %s\n' "$target" "$ends" "$count" "$count" "$exact"
    if [ "$ends" != 1 ]; then
      broke "$target printed $ends end lines, not one"
    elif [ "$exact" != 1 ]; then
      found=$(grep "^$target end: " "$log")
      broke "$target printed \"$found\", not \"$line\""
    fi
  done <<EOF
$want
EOF
}

tracked() {
  status=$(git status --porcelain --untracked-files=no) || fail "git status failed $1 cargo ran"
  [ -z "$status" ] && return 0
  case $1 in
    before) fail "a tracked file differs from HEAD before cargo ran, so commit or stash the change, then run the check again" ;;
    *) fail "a tracked file differs from HEAD after cargo ran: a test or a build of the run wrote a tracked file, so read git status and mend what wrote it" ;;
  esac
}

measure() {
  case ${CI_D_JOB:-} in
    macos | windows) ;;
    *) fail "the job is ${CI_D_JOB:-not set}, not macos or windows" ;;
  esac
  want=$(wanted "$CI_D_JOB")
  if [ "${GITHUB_ACTIONS:-}" != true ] || [ "${RUNNER_ENVIRONMENT:-}" != github-hosted ]; then
    printf 'SKIP %s (the dialog targets show their dialogs on a hosted runner alone)\n' "$name"
    exit 77
  fi
  [ -n "${CI_D_REPO:-}" ] || fail "CI_D_REPO is not set"
  [ -n "${CI_D_ARTIFACTS:-}" ] || fail "CI_D_ARTIFACTS is not set"
  cd "$CI_D_REPO" || fail "the repository directory cannot be entered"
  [ -d "$CI_D_ARTIFACTS" ] || mkdir -p "$CI_D_ARTIFACTS" || fail "the artifacts directory cannot be made"
  tracked before
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail "mktemp failed"
  cargo test -p halvo-coord-app --locked --test dialog_hosted --test guard_hosted >"$work/out" 2>&1
  code=$?
  tracked after
  tr -d '\r' <"$work/out" >"$CI_D_ARTIFACTS/dialog-targets.txt" || fail "the output of the run cannot be written to the artifacts directory"
  broken=
  decide "$CI_D_ARTIFACTS/dialog-targets.txt" "$code" "$want"
  [ -z "$broken" ] || fail "$broken"
  printf 'PASS %s\n' "$name"
  exit 0
}

check() {
  label=$1
  want=$2
  text=$3
  job=$4
  shift 4
  if [ "$job" = --bogus ]; then
    out=$(sh "$self" --bogus 2>&1)
  else
    out=$(env PATH="$work/bin:$PATH" STUB_CALLS="$work/calls.$label" STUB_OUT="$STUB_OUT" \
      STUB_EXIT="$STUB_EXIT" STUB_WRITES="$STUB_WRITES" CI_D_REPO="$work/repo" \
      CI_D_ARTIFACTS="$work/artifacts" CI_D_JOB="$job" GITHUB_ACTIONS=true \
      RUNNER_ENVIRONMENT=github-hosted "$@" sh "$self" 2>&1)
  fi
  code=$?
  last=${out##*"
"}
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    "77:SKIP $name ("*")") got=SKIP ;;
    *) got=exit-$code ;;
  esac
  case $out in
    *"$text"*) ;;
    *) got=$got-without-the-text ;;
  esac
  if [ "$want" = SKIP ] && [ -s "$work/calls.$label" ]; then
    got=$got-after-cargo-ran
  fi
  printf 'case %s/%s want=%s got=%s\n' "$name" "$label" "$want" "$got"
  if [ "$got" != "$want" ]; then
    wrong="$wrong $label"
  elif [ "$want" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
  STUB_EXIT=0
  STUB_WRITES=no
}

output() {
  STUB_OUT=$work/out.$1
  shift
  : >"$STUB_OUT" || exit 2
  for line in "$@"; do
    printf '%s\r\n' "$line" >>"$STUB_OUT" || exit 2
  done
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  mkdir -p "$work/bin" "$work/repo" || exit 2
  cat >"$work/bin/cargo" <<'EOF'
#!/bin/sh
printf '%s\n' "$*" >>"$STUB_CALLS"
[ "$*" = 'test -p halvo-coord-app --locked --test dialog_hosted --test guard_hosted' ] || exit 97
[ "$STUB_WRITES" = no ] || printf 'written by a test\n' >>tracked.txt
cat "$STUB_OUT"
exit "$STUB_EXIT"
EOF
  chmod +x "$work/bin/cargo" || exit 2
  found=$(env PATH="$work/bin:$PATH" sh -c 'command -v cargo') || exit 2
  if [ "$found" != "$work/bin/cargo" ]; then
    printf 'FAIL self-test:%s (cargo is not the stub of the self-test)\n' "$name"
    exit 1
  fi
  printf 'tracked\n' >"$work/repo/tracked.txt" || exit 2
  git -C "$work/repo" init -q || exit 2
  git -C "$work/repo" add tracked.txt || exit 2
  git -C "$work/repo" commit -q -m fixture || exit 2
  STUB_EXIT=0
  STUB_WRITES=no
  rejected=0
  wrong=
  running='     Running tests/dialog_hosted.rs (target/debug/deps/dialog_hosted-0123456789abcdef)'
  mac_dialog='dialog_hosted end: cases ran 4 of 4, failed 0'
  mac_guard='guard_hosted end: cases ran 5 of 5, failed 0'
  win_dialog='dialog_hosted end: cases ran 5 of 5, failed 0'
  win_guard='guard_hosted end: cases ran 7 of 7, failed 0'

  output macos "$running" 'dialog_hosted case macos press_allow PASS' "$mac_dialog" "$mac_guard"
  check macos-own-lines PASS 'guard_hosted: end lines 1, of them the line of 5 of 5 with none failed 1' macos
  output windows "$running" "$win_dialog" "$win_guard"
  check windows-own-lines PASS 'dialog_hosted: end lines 1, of them the line of 5 of 5 with none failed 1' windows
  output off "$off" "$mac_guard"
  check macos-hosted-alone FAIL 'a target printed the line that it was not run, 1 times' macos
  output off-both "$off" "$off"
  check macos-both-hosted-alone FAIL 'dialog_hosted printed 0 end lines, not one' macos
  output off-beside "$off" "$mac_dialog" "$mac_guard"
  check macos-hosted-alone-beside-the-lines FAIL 'a target printed the line that it was not run, 1 times' macos
  output short 'dialog_hosted end: cases ran 3 of 4, failed 0' "$mac_guard"
  check macos-short-count FAIL 'dialog_hosted printed "dialog_hosted end: cases ran 3 of 4, failed 0", not "dialog_hosted end: cases ran 4 of 4, failed 0"' macos
  output short-flag 'dialog_hosted end: cases ran 3 of 4, failed 0, FAIL: not every case ran' "$mac_guard"
  check macos-short-count-with-its-mark FAIL 'not "dialog_hosted end: cases ran 4 of 4, failed 0"' macos
  output win-short "$win_dialog" 'guard_hosted end: cases ran 6 of 7, failed 0'
  check windows-short-count FAIL 'not "guard_hosted end: cases ran 7 of 7, failed 0"' windows
  output win-count "$win_dialog" "$win_guard"
  check macos-job-with-the-windows-count FAIL 'not "dialog_hosted end: cases ran 4 of 4, failed 0"' macos
  output mac-count "$mac_dialog" "$mac_guard"
  check windows-job-with-the-macos-count FAIL 'not "dialog_hosted end: cases ran 5 of 5, failed 0"' windows
  output zero 'dialog_hosted end: cases ran 0 of 0, failed 0' "$mac_guard"
  check macos-zero-of-zero FAIL 'not "dialog_hosted end: cases ran 4 of 4, failed 0"' macos
  output failed 'dialog_hosted end: cases ran 4 of 4, failed 1' "$mac_guard"
  check macos-a-failed-case FAIL 'not "dialog_hosted end: cases ran 4 of 4, failed 0"' macos
  output no-guard "$mac_dialog"
  check macos-no-guard-line FAIL 'guard_hosted printed 0 end lines, not one' macos
  output two "$mac_dialog" "$mac_dialog" "$mac_guard"
  check macos-two-end-lines FAIL 'dialog_hosted printed 2 end lines, not one' macos
  output prefix "x $mac_dialog" "$mac_guard"
  check macos-line-inside-another FAIL 'dialog_hosted printed 0 end lines, not one' macos
  output cargo "$mac_dialog" "$mac_guard"
  STUB_EXIT=101
  check macos-cargo-failed FAIL 'exited 101' macos
  STUB_WRITES=yes
  check macos-a-test-writes-the-checkout FAIL 'a tracked file differs from HEAD after cargo ran' macos
  git -C "$work/repo" checkout -q -- tracked.txt || exit 2
  printf 'changed before the check\n' >>"$work/repo/tracked.txt" || exit 2
  check macos-on-a-changed-checkout FAIL 'a tracked file differs from HEAD before cargo ran' macos
  git -C "$work/repo" checkout -q -- tracked.txt || exit 2
  check macos-after-the-checkout-is-clean-again PASS 'PASS' macos
  check not-in-actions SKIP 'on a hosted runner alone' macos GITHUB_ACTIONS=
  check self-hosted-runner SKIP 'on a hosted runner alone' windows RUNNER_ENVIRONMENT=self-hosted
  check another-job FAIL 'the job is linux, not macos or windows' linux
  check an-argument FAIL "FAIL $name (usage)" --bogus

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
  exit 0
}

main() {
  self=$0
  case $# in
    0) measure ;;
    1)
      case $1 in
        --self-test) self_test ;;
        *) fail usage ;;
      esac
      ;;
    *) fail usage ;;
  esac
}

main "$@"
