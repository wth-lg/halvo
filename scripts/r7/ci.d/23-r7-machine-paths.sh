#!/bin/sh
# jobs: macos windows
# rule: backend/scripts/check-machine-paths.sh, the backend's own check, finds no home-directory path of any platform in the tracked files under backend/, and git lists at least one such file.
set -u
name=23-r7-machine-paths

paths_in() {
  repo=$1
  out=$2
  cd "$repo" || return 2
  files=$(git -c core.quotepath=off ls-files -- backend/ | wc -l | tr -d ' ')
  if [ ! -f backend/scripts/check-machine-paths.sh ]; then
    counts="files=$files found=unmeasured exit=none"
    cause="backend/scripts/check-machine-paths.sh is missing"
    return 1
  fi
  sh backend/scripts/check-machine-paths.sh >"$out/paths.txt" 2>"$out/stderr.txt"
  code=$?
  found=$(grep -c '^[^:]*:[0-9][0-9]*:' "$out/paths.txt")
  counts="files=$files found=$found exit=$code"
  cause=
  case $code in
    0) ;;
    1) cause="the check found $found home-directory paths; the list is paths.txt among the artifacts" ;;
    *) cause="the check could not read the index (exit $code)" ;;
  esac
  [ -n "$cause" ] || [ "$files" -gt 0 ] || cause="git lists no tracked file under backend/"
  [ -z "$cause" ]
}

measure() {
  repo=${CI_D_REPO:-$(git rev-parse --show-toplevel)} || exit 2
  artifacts=${CI_D_ARTIFACTS:-$repo/target/ci-artifacts/$name}
  mkdir -p "$artifacts" || exit 2
  if paths_in "$repo" "$artifacts"; then
    printf '%s\n' "$counts"
    printf 'PASS %s\n' "$name"
  else
    printf '%s\n' "$counts"
    printf 'FAIL %s (%s)\n' "$name" "$cause"
    exit 1
  fi
}

home_path() {
  printf 'the notes are under %sUsers%sada%snotes.txt\n' / / /
}

fixture() {
  dir=$scratch/$1
  mkdir -p "$dir/backend/scripts" "$dir/other" "$dir/out" || exit 2
  git -C "$dir" init -q || exit 2
  cp "$check" "$dir/backend/scripts/check-machine-paths.sh" || exit 2
  printf 'a file without a path\n' >"$dir/backend/notes.txt"
  printf 'a file without a path\n' >"$dir/other/notes.txt"
}

stage() {
  git -C "$scratch/$1" add -A . || exit 2
}

expect() {
  want=$1
  case_name=$2
  (paths_in "$scratch/$case_name" "$scratch/$case_name/out") >/dev/null 2>&1
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
  check=$repo/backend/scripts/check-machine-paths.sh
  if [ ! -f "$check" ]; then
    printf 'FAIL self-test:%s (backend/scripts/check-machine-paths.sh is missing)\n' "$name"
    exit 1
  fi
  scratch=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  rejected=0
  wrong=

  fixture good
  stage good
  expect PASS good

  fixture home-path-in-backend
  home_path >"$scratch/home-path-in-backend/backend/notes.txt"
  stage home-path-in-backend
  expect FAIL home-path-in-backend

  fixture windows-home-path-in-backend
  printf 'C:%sUsers%sada%snotes.txt\n' "\\" "\\" "\\" >"$scratch/windows-home-path-in-backend/backend/notes.txt"
  stage windows-home-path-in-backend
  expect FAIL windows-home-path-in-backend

  fixture home-path-in-a-link-target
  stage home-path-in-a-link-target
  blob=$(printf '%sUsers%sada%starget' / / / | git -C "$scratch/home-path-in-a-link-target" hash-object -w --stdin) || exit 2
  git -C "$scratch/home-path-in-a-link-target" update-index --add --cacheinfo "120000,$blob,backend/link" || exit 2
  expect FAIL home-path-in-a-link-target

  fixture home-path-staged-but-not-in-the-work-tree
  home_path >"$scratch/home-path-staged-but-not-in-the-work-tree/backend/notes.txt"
  stage home-path-staged-but-not-in-the-work-tree
  printf 'a file without a path\n' >"$scratch/home-path-staged-but-not-in-the-work-tree/backend/notes.txt"
  expect FAIL home-path-staged-but-not-in-the-work-tree

  fixture home-path-outside-backend-is-not-read
  home_path >"$scratch/home-path-outside-backend-is-not-read/other/notes.txt"
  stage home-path-outside-backend-is-not-read
  expect PASS home-path-outside-backend-is-not-read

  fixture untracked-home-path-is-not-read
  stage untracked-home-path-is-not-read
  home_path >"$scratch/untracked-home-path-is-not-read/backend/untracked.txt"
  expect PASS untracked-home-path-is-not-read

  fixture no-file-under-backend
  git -C "$scratch/no-file-under-backend" add other || exit 2
  expect FAIL no-file-under-backend

  fixture check-missing
  mv "$scratch/check-missing/backend/scripts" "$scratch/check-missing/scripts.moved" || exit 2
  stage check-missing
  expect FAIL check-missing

  fixture index-unreadable
  stage index-unreadable
  printf 'this is no index\n' >"$scratch/index-unreadable/.git/index"
  expect FAIL index-unreadable

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
