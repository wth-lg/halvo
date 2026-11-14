#!/bin/sh
# jobs: macos
# rule: a second click at the point of "Delete everything" lands on no control that deletes
set -u
name=31-app-page-layout
probe=ui/tests/layout/delete-confirm.swift
fixtures=ui/tests/fixtures/capture
redraw_cases=12

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

run_probe() {
  runner=$1
  page=$2
  out=$3
  if [ -n "$runner" ]; then
    "$runner" "$probe" "$page" "$fixtures" >"$out" 2>&1
  else
    command -v xcrun >/dev/null 2>&1 || fail "xcrun is not on the PATH"
    xcrun swift "$probe" "$page" "$fixtures" >"$out" 2>&1
  fi
  probe_code=$?
}

verdict() {
  out=$1
  code=$2
  for key in at_point_after_first_click delete_all_calls_after_second_click_at_same_point confirm_below_delete redraw_cases; do
    line=$(grep -E "^$key=[A-Za-z0-9_-]*$" "$out" | tail -n 1)
    printf '%s\n' "${line:-$key=missing}"
  done
  printf 'probe exit: %s\n' "$code"
  [ "$code" -ne 2 ] || fail "the probe did not load the page"
  grep -qx 'at_point_after_first_click=delete' "$out" || fail "after the first click another element than Delete everything sits at its point"
  grep -qx 'delete_all_calls_after_second_click_at_same_point=0' "$out" || fail "a second click at the point of Delete everything sent delete_all"
  grep -qx 'confirm_below_delete=true' "$out" || fail "the confirmation is not on a row below Delete everything"
  grep -qx "redraw_cases=$redraw_cases" "$out" || fail "the probe ran another count of redraw cases than $redraw_cases"
  [ "$code" -eq 0 ] || fail "the probe exited $code"
}

measure() {
  repo=${CI_D_REPO:-$(git rev-parse --show-toplevel)} || exit 2
  cd "$repo" || exit 2
  artifacts=${CI_D_ARTIFACTS:-}
  if [ -z "$artifacts" ]; then
    artifacts=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail "mktemp failed"
  fi
  run_probe '' ui/app/index.html "$artifacts/probe.txt"
  verdict "$artifacts/probe.txt" "$probe_code"
  git diff --quiet HEAD -- || fail "a tracked file differs from HEAD after the probe"
  printf 'PASS %s\n' "$name"
}

fixture() {
  page=$1
  runner=$2
  root=$3
  run_probe "$runner" "$page" "$root/probe.txt"
  verdict "$root/probe.txt" "$probe_code"
  printf 'PASS %s\n' "$name"
}

canned() {
  mkdir -p "$work/$1"
  printf '#!/bin/sh\nprintf %s\nexit %s\n' "'$2'" "$3" >"$work/$1/runner"
  chmod +x "$work/$1/runner"
}

check() {
  mkdir -p "$work/$1"
  out=$(sh "$self" --fixture "$2" "$3" "$work/$1" 2>&1)
  code=$?
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  case $out in
    *"$5"*) ;;
    *) got=$got-without-the-text ;;
  esac
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$4" "$got"
  if [ "$got" != "$4" ]; then
    wrong="$wrong $1"
  elif [ "$4" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

self_test() {
  repo=${CI_D_REPO:-$(git rev-parse --show-toplevel)} || exit 2
  cd "$repo" || exit 2
  dir=$(cd "$(dirname "$0")" && pwd) || exit 2
  self=$dir/$(basename "$0")
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  rejected=0
  wrong=
  good='at_point_after_first_click=delete\ndelete_all_calls_after_second_click_at_same_point=0\nconfirm_below_delete=true\nredraw_cases=12\n'
  canned canned-safe "$good" 0
  canned canned-delete-sent 'at_point_after_first_click=delete\ndelete_all_calls_after_second_click_at_same_point=1\nconfirm_below_delete=true\nredraw_cases=12\n' 1
  canned canned-confirm-at-point 'at_point_after_first_click=confirm-delete\ndelete_all_calls_after_second_click_at_same_point=0\nconfirm_below_delete=false\n' 1
  canned canned-same-row 'at_point_after_first_click=delete\ndelete_all_calls_after_second_click_at_same_point=0\nconfirm_below_delete=false\n' 1
  canned canned-no-load 'load failed: no page\n' 2
  canned canned-no-redraw-cases 'at_point_after_first_click=delete\ndelete_all_calls_after_second_click_at_same_point=0\nconfirm_below_delete=true\nredraw_cases=0\n' 0
  canned canned-exit-without-lines "$good" 1

  check delete-view-of-685d846 ui/tests/layout/fixtures/index-685d846.html '' FAIL 'another element than Delete everything sits at its point)'
  check delete-view-of-40418d21 ui/tests/layout/fixtures/app-40418d21/index.html '' FAIL 'a second click at the point of Delete everything sent delete_all)'
  check canned-safe ui/app/index.html "$work/canned-safe/runner" PASS 'delete_all_calls_after_second_click_at_same_point=0'
  check canned-delete-sent ui/app/index.html "$work/canned-delete-sent/runner" FAIL 'sent delete_all)'
  check canned-confirm-at-point ui/app/index.html "$work/canned-confirm-at-point/runner" FAIL 'another element than Delete everything sits at its point)'
  check canned-same-row ui/app/index.html "$work/canned-same-row/runner" FAIL 'not on a row below Delete everything)'
  check canned-no-load ui/app/index.html "$work/canned-no-load/runner" FAIL 'the probe did not load the page)'
  check canned-no-redraw-cases ui/app/index.html "$work/canned-no-redraw-cases/runner" FAIL 'another count of redraw cases than 12)'
  check canned-exit-without-lines ui/app/index.html "$work/canned-exit-without-lines/runner" FAIL 'the probe exited 1)'
  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  --fixture) fixture "$2" "$3" "$4" ;;
  '') measure ;;
  *) exit 64 ;;
esac
