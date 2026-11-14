#!/bin/sh
# jobs: macos windows
# rule: backend/scripts/check-machine-paths.sh, the backend's own check, finds no home-directory path of any platform in the tracked files under backend/, under formal/r7-ledger/ (the model), under docs/r7/gate/ (the gate's records) and under scripts/r7/ (the run's scripts), and git lists at least one file under each.
# lane: free
set -u
name=23-r7-machine-paths

verdict_of() {
  case $1 in
    0) ;;
    1) cause="the check found home-directory paths under $2; the list is among the artifacts" ;;
    *) cause="the check could not read the index of $2 (exit $1)" ;;
  esac
}

paths_in() {
  repo=$1
  out=$2
  cd "$repo" || return 2
  files=$(git -c core.quotepath=off ls-files -- backend/ | wc -l | tr -d ' ')
  model_files=$(git -c core.quotepath=off ls-files -- formal/r7-ledger/ | wc -l | tr -d ' ')
  records_files=$(git -c core.quotepath=off ls-files -- docs/r7/gate/ | wc -l | tr -d ' ')
  scripts_files=$(git -c core.quotepath=off ls-files -- scripts/r7/ | wc -l | tr -d ' ')
  if [ ! -f backend/scripts/check-machine-paths.sh ]; then
    counts="files=$files model-files=$model_files found=unmeasured exit=none"
    cause="backend/scripts/check-machine-paths.sh is missing"
    return 1
  fi
  sh backend/scripts/check-machine-paths.sh >"$out/paths.txt" 2>"$out/stderr.txt"
  code=$?
  model=$(mktemp -d "${TMPDIR:-/tmp}/$name-model.XXXXXX") || return 2
  git init -q "$model" || return 2
  : >"$out/model-paths.txt"
  model_code=2
  if git -c core.quotepath=off ls-files -z -- formal/r7-ledger/ docs/r7/gate/ scripts/r7/ >"$out/model-files.z" &&
    git checkout-index -z --force --stdin --prefix="$model/" <"$out/model-files.z" &&
    git -C "$model" add -A .; then
    (cd "$model" && sh "$repo/backend/scripts/check-machine-paths.sh") >"$out/model-paths.txt" 2>"$out/model-stderr.txt"
    model_code=$?
  fi
  found=$(grep -c '^[^:]*:[0-9][0-9]*:' "$out/paths.txt")
  model_found=$(grep -c '^[^:]*:[0-9][0-9]*:' "$out/model-paths.txt")
  counts="files=$files found=$found exit=$code model-files=$model_files records-files=$records_files scripts-files=$scripts_files others-found=$model_found others-exit=$model_code"
  cause=
  verdict_of "$code" backend/
  [ -n "$cause" ] || verdict_of "$model_code" "formal/r7-ledger/, docs/r7/gate/ or scripts/r7/"
  [ -n "$cause" ] || [ "$files" -gt 0 ] || cause="git lists no tracked file under backend/"
  [ -n "$cause" ] || [ "$model_files" -gt 0 ] || cause="git lists no tracked file under formal/r7-ledger/"
  [ -n "$cause" ] || [ "$records_files" -gt 0 ] || cause="git lists no tracked file under docs/r7/gate/"
  [ -n "$cause" ] || [ "$scripts_files" -gt 0 ] || cause="git lists no tracked file under scripts/r7/"
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
  mkdir -p "$dir/backend/scripts" "$dir/formal/r7-ledger" "$dir/docs/r7/gate" "$dir/scripts/r7" "$dir/other" "$dir/out" || exit 2
  git -C "$dir" init -q || exit 2
  cp "$check" "$dir/backend/scripts/check-machine-paths.sh" || exit 2
  printf 'a file without a path\n' >"$dir/backend/notes.txt"
  printf 'a file without a path\n' >"$dir/formal/r7-ledger/m.tla"
  printf 'line: PASS model: states=1\n' >"$dir/docs/r7/gate/95-model.txt"
  printf 'echo a file without a path\n' >"$dir/scripts/r7/run.sh"
  printf 'a file without a path\n' >"$dir/other/notes.txt"
}

stage() {
  git -C "$scratch/$1" add -A . || exit 2
}

commit() {
  git -C "$scratch/$1" -c commit.gpgsign=false -c core.hooksPath=/dev/null commit -q -m "$2" >/dev/null || exit 2
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
  { git var GIT_AUTHOR_IDENT && git var GIT_COMMITTER_IDENT; } >/dev/null 2>&1 || {
    printf 'FAIL self-test:%s (the self-test commits in throwaway repositories, and git has no name and email for a commit here. Configure a git user, or set the variables GIT_AUTHOR_NAME, GIT_AUTHOR_EMAIL, GIT_COMMITTER_NAME and GIT_COMMITTER_EMAIL.)\n' "$name"
    exit 1
  }
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

  fixture home-path-committed-to-backend
  home_path >"$scratch/home-path-committed-to-backend/backend/notes.txt"
  stage home-path-committed-to-backend
  commit home-path-committed-to-backend 'A home path that the index and the last commit both hold'
  expect FAIL home-path-committed-to-backend

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

  fixture home-path-in-the-model
  home_path >"$scratch/home-path-in-the-model/formal/r7-ledger/m.tla"
  stage home-path-in-the-model
  expect FAIL home-path-in-the-model

  fixture home-path-in-the-model-staged-but-not-in-the-work-tree
  home_path >"$scratch/home-path-in-the-model-staged-but-not-in-the-work-tree/formal/r7-ledger/m.tla"
  stage home-path-in-the-model-staged-but-not-in-the-work-tree
  printf 'a file without a path\n' >"$scratch/home-path-in-the-model-staged-but-not-in-the-work-tree/formal/r7-ledger/m.tla"
  expect FAIL home-path-in-the-model-staged-but-not-in-the-work-tree

  fixture no-file-under-the-model
  mv "$scratch/no-file-under-the-model/formal" "$scratch/no-file-under-the-model/formal.moved" || exit 2
  stage no-file-under-the-model
  expect FAIL no-file-under-the-model

  fixture check-exits-2-with-files-listed
  printf '#!/bin/sh\nexit 2\n' >"$scratch/check-exits-2-with-files-listed/backend/scripts/check-machine-paths.sh"
  stage check-exits-2-with-files-listed
  expect FAIL check-exits-2-with-files-listed

  fixture home-path-in-a-model-file-that-is-no-tla
  home_path >"$scratch/home-path-in-a-model-file-that-is-no-tla/formal/r7-ledger/check.mjs"
  stage home-path-in-a-model-file-that-is-no-tla
  expect FAIL home-path-in-a-model-file-that-is-no-tla

  fixture model-copy-fails-while-the-list-works
  stage model-copy-fails-while-the-list-works
  git -C "$scratch/model-copy-fails-while-the-list-works" update-index --add --info-only --cacheinfo 100644,1111111111111111111111111111111111111111,formal/r7-ledger/gone.tla || exit 2
  expect FAIL model-copy-fails-while-the-list-works

  fixture model-tracked-and-nothing-under-backend
  git -C "$scratch/model-tracked-and-nothing-under-backend" add formal docs scripts || exit 2
  expect FAIL model-tracked-and-nothing-under-backend

  fixture home-path-in-a-record
  printf 'line: FAIL model: states=1; first failure: ' >"$scratch/home-path-in-a-record/docs/r7/gate/95-model.txt"
  home_path >>"$scratch/home-path-in-a-record/docs/r7/gate/95-model.txt"
  stage home-path-in-a-record
  expect FAIL home-path-in-a-record

  fixture home-path-in-a-run-script
  home_path >>"$scratch/home-path-in-a-run-script/scripts/r7/run.sh"
  stage home-path-in-a-run-script
  expect FAIL home-path-in-a-run-script

  fixture no-file-under-the-records
  mv "$scratch/no-file-under-the-records/docs" "$scratch/no-file-under-the-records/docs.moved" || exit 2
  stage no-file-under-the-records
  expect FAIL no-file-under-the-records

  fixture no-file-under-the-run-scripts
  mv "$scratch/no-file-under-the-run-scripts/scripts/r7" "$scratch/no-file-under-the-run-scripts/r7.moved" || exit 2
  stage no-file-under-the-run-scripts
  expect FAIL no-file-under-the-run-scripts

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
