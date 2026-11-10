#!/bin/sh
# jobs: macos windows
# rule: Every substitution of a mutants.rows file still applies and builds, and a push to main runs every mutant.
set -u
name=30-mutants
nl='
'

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

run_rows() {
  label=$1
  shift
  out=$(sh scripts/r1-gate/mutation-test.sh "$@" 2>&1)
  code=$?
  printf '%s\n' "$out"
  first=$(printf '%s\n' "$out" | sed -n '/^FAIL /{p;q;}')
  [ "$code" -eq 0 ] || fail "$label exited $code: ${first:-it printed no FAIL line}"
  [ -z "$first" ] || fail "$label exited 0 and printed: $first"
  case $nl$out in
    *"${nl}PASS mutation:"*) ;;
    *) fail "$label printed no PASS mutation line" ;;
  esac
}

measure() {
  cd "$CI_D_REPO" || exit 2
  [ -f scripts/r1-gate/mutation-test.sh ] || fail "scripts/r1-gate/mutation-test.sh is missing"
  grep -q -e '--apply-only' scripts/r1-gate/mutation-test.sh ||
    fail "scripts/r1-gate/mutation-test.sh takes no --apply-only"
  run_rows 'mutation-test.sh --apply-only' --apply-only
  if [ "${GITHUB_EVENT_NAME:-}" = push ] && [ "${GITHUB_REF:-}" = refs/heads/main ]; then
    run_rows 'mutation-test.sh'
  fi
  printf 'PASS %s\n' "$name"
}

check() {
  out=$(
    CI_D_REPO=$work/$1
    STUB_CALLS=$work/$1/calls
    export CI_D_REPO STUB_CALLS
    unset GITHUB_EVENT_NAME GITHUB_REF
    if [ -n "$event" ]; then
      GITHUB_EVENT_NAME=$event
      GITHUB_REF=$ref
      export GITHUB_EVENT_NAME GITHUB_REF
    fi
    sh "$0" 2>&1
  )
  code=$?
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  calls=
  [ ! -f "$work/$1/calls" ] || calls=$(cat "$work/$1/calls")
  [ "$calls" = "$4" ] || got=$got-with-other-calls
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
  event=push
  ref=refs/heads/a-branch
}

# shellcheck disable=SC2016
runner() {
  mkdir -p "$work/$1/scripts/r1-gate" || exit 2
  printf 'printf "call %%s\\n" "$*" >>"$STUB_CALLS"\n%s\n' "$2" >"$work/$1/scripts/r1-gate/mutation-test.sh"
}

# shellcheck disable=SC2016
self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  only='[ "${1:-}" = --apply-only ] || exit 9'
  applies='case ${1:-} in --apply-only) printf "PASS mutation:shredded-key-survives (applies and builds)\n"; exit 0 ;; esac'
  apply_only='call --apply-only'
  both="call --apply-only${nl}call "
  rejected=0
  wrong=
  event=push
  ref=refs/heads/a-branch

  runner rows-apply "$applies; exit 9"
  check rows-apply PASS '' "$apply_only"
  runner anchor-moved "$only; printf \"PASS mutation:checkpoint-not-verified (applies and builds)\\nFAIL mutation:shredded-key-survives (the substitution did not apply)\\n\"; exit 1"
  check anchor-moved FAIL 'mutation-test.sh --apply-only exited 1: FAIL mutation:shredded-key-survives (the substitution did not apply))' "$apply_only"
  runner runner-exits-2 "$only; printf \"PASS mutation:shredded-key-survives (applies and builds)\\n\"; exit 2"
  check runner-exits-2 FAIL 'mutation-test.sh --apply-only exited 2: it printed no FAIL line)' "$apply_only"
  runner fail-line-with-exit-0 "$only; printf \"FAIL mutation:shredded-key-survives (the substitution did not apply)\\n\""
  check fail-line-with-exit-0 FAIL 'mutation-test.sh --apply-only exited 0 and printed: FAIL mutation:shredded-key-survives' "$apply_only"
  runner other-fail-line-with-exit-0 "$only; printf \"PASS mutation:shredded-key-survives (applies and builds)\\nFAIL unmutated:store (gate said: FAIL store)\\n\""
  check other-fail-line-with-exit-0 FAIL 'mutation-test.sh --apply-only exited 0 and printed: FAIL unmutated:store' "$apply_only"
  runner fail-line-on-standard-error "$only; printf \"PASS mutation:checkpoint-not-verified (applies and builds)\\n\"; printf \"FAIL mutation:shredded-key-survives (the substitution did not apply)\\n\" >&2"
  check fail-line-on-standard-error FAIL 'mutation-test.sh --apply-only exited 0 and printed: FAIL mutation:shredded-key-survives' "$apply_only"
  runner no-row "$only; exit 0"
  check no-row FAIL 'mutation-test.sh --apply-only printed no PASS mutation line)' "$apply_only"
  runner only-other-lines "$only; printf \"PASS unmutated:store\\n\""
  check only-other-lines FAIL 'mutation-test.sh --apply-only printed no PASS mutation line)' "$apply_only"
  runner rows-all-pending "$only; printf \"OPEN mutation:shredded-key-survives (no work)\\n\"; printf \"scratch mutation: /tmp/r1-gate-mutation.x\\n\" >&2"
  check rows-all-pending FAIL 'mutation-test.sh --apply-only printed no PASS mutation line)' "$apply_only"
  runner scratch-line-only "$only; printf \"scratch mutation: /tmp/r1-gate-mutation.x\\n\" >&2"
  check scratch-line-only FAIL 'mutation-test.sh --apply-only printed no PASS mutation line)' "$apply_only"
  runner runner-without-the-flag 'printf "PASS mutation:shredded-key-survives (gate said: FAIL deletion)\n"'
  check runner-without-the-flag FAIL 'scripts/r1-gate/mutation-test.sh takes no --apply-only)' ''
  mkdir -p "$work/runner-missing"
  check runner-missing FAIL 'scripts/r1-gate/mutation-test.sh is missing)' ''
  runner run-outside-github-actions "$applies; exit 9"
  event=
  check run-outside-github-actions PASS '' "$apply_only"

  runner push-to-main-full-run-fails "$applies; printf \"FAIL mutation:event-rows-never-written (gate said: PASS store)\\n\"; exit 1"
  ref=refs/heads/main
  check push-to-main-full-run-fails FAIL 'mutation-test.sh exited 1: FAIL mutation:event-rows-never-written (gate said: PASS store))' "$both"
  runner push-to-main-full-run-prints-no-row "$applies; exit 0"
  ref=refs/heads/main
  check push-to-main-full-run-prints-no-row FAIL 'mutation-test.sh printed no PASS mutation line)' "$both"
  runner push-to-main-full-run-passes "$applies; printf \"PASS mutation:shredded-key-survives (decryptable)\\n\""
  ref=refs/heads/main
  check push-to-main-full-run-passes PASS '' "$both"
  runner push-to-a-branch-that-ends-in-main "$applies; exit 9"
  ref=refs/heads/team/main
  check push-to-a-branch-that-ends-in-main PASS '' "$apply_only"
  runner pull-request-into-main "$applies; exit 9"
  event=pull_request
  ref=refs/heads/main
  check pull-request-into-main PASS '' "$apply_only"

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
