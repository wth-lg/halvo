#!/bin/sh
# Deletes every Rust cache of one job of .github/workflows/ci.yml before that
# job's Rust cache step saves a new one. Help: sh scripts/ci/keep-one-rust-cache.sh --help
set -u

help_text() {
  cat <<'EOF'
Keeps one Rust cache for each job of .github/workflows/ci.yml. GitHub keeps
at most 10 GB of caches for a repository, and past that it drops the cache
that was read least lately. Two Rust caches of the windows job and one of the
macos job do not fit, so a new Rust cache would push out another one. The
step "Keep one Rust cache" of each job runs this script in a push run of main,
as the last step of the job, when the job's Rust cache step found no cache
with its full key. The script deletes every Rust cache of that job, and then
the post step of the Rust cache step saves the new one.

  sh scripts/ci/keep-one-rust-cache.sh <job>
      <job> is macos or windows. Lists the caches whose key begins with
      v0-rust-<job>- (gh cache list --key) and deletes each of them by its id
      (gh cache delete). It deletes no cache of the other job and no cache
      whose key begins otherwise. gh reads the token from GH_TOKEN and the
      repository from GH_REPO; the token needs the permission actions: write.
      Prints one line for each cache that it deletes, and one line at the end.
      When the list or a delete fails, the line at the end names what failed
      and what follows from it, and the script still exits 0: a failed delete
      fails no job.
  sh scripts/ci/keep-one-rust-cache.sh --self-test
      Runs the script against a stub gh, which deletes nothing on GitHub, in
      three cases: two Rust caches of the job and one of the other job are
      listed; the list fails; a delete fails. Leaves its directory under
      TMPDIR in place.
  sh scripts/ci/keep-one-rust-cache.sh --help
      Prints this text.

Exit codes: 0 done, also after a failed list or a failed delete; 1 a case of
the self-test behaved otherwise; 2 the self-test could not run; 64 bad usage.
EOF
}

say() {
  printf 'keep-one-rust-cache: %s\n' "$1"
}

usage() {
  {
    printf 'keep-one-rust-cache: %s\n' "$1"
    printf 'Usage: sh scripts/ci/keep-one-rust-cache.sh <job>\n       sh scripts/ci/keep-one-rust-cache.sh --self-test\n       sh scripts/ci/keep-one-rust-cache.sh --help\n'
  } >&2
  exit 64
}

first_line() {
  printf '%s\n' "$1" | tr -d '\r' | sed -n '/[^[:space:]]/{s/^[[:space:]]*//;p;q;}' | cut -c1-300
}

keep_one() {
  job=$1
  prefix="v0-rust-$job-"
  after='the new cache that this job saves next stands beside the old ones, and the repository may pass the cap of 10 GB of caches, past which GitHub drops the cache read least lately, which may be the new Rust cache of the other job'
  status=0
  listed=$(gh cache list --key "$prefix" --limit 1000 --json id,key --jq '.[] | "cache \(.id) \(.key)"' 2>&1) || status=$?
  if [ "$status" -ne 0 ]; then
    cause=$(first_line "$listed")
    say "the list of the caches failed (exit $status: $cause), so no Rust cache of the job $job was deleted; $after"
    return 0
  fi
  listed=$(printf '%s\n' "$listed" | tr -d '\r')
  found=0
  deleted=0
  failed=0
  cause=
  while read -r word id key rest; do
    [ "$word" = cache ] && [ -z "$rest" ] || continue
    case $id in
      '' | *[!0-9]*) continue ;;
    esac
    case $key in
      "$prefix"?*) ;;
      *) continue ;;
    esac
    found=$((found + 1))
    status=0
    answer=$(gh cache delete "$id" 2>&1) || status=$?
    if [ "$status" -eq 0 ]; then
      deleted=$((deleted + 1))
      say "deleted the cache $id, $key"
    else
      failed=$((failed + 1))
      if [ -z "$cause" ]; then
        cause=$(first_line "$answer")
        cause="the cache $id: exit $status: $cause"
      fi
    fi
  done <<EOF
$listed
EOF
  if [ "$failed" -ne 0 ]; then
    say "the delete of $failed of $found Rust caches of the job $job failed (first $cause); they stay, and $after"
  elif [ "$found" -eq 0 ]; then
    say "found no Rust cache of the job $job (no key that begins with $prefix); nothing to delete"
  else
    say "deleted $deleted of $found Rust caches of the job $job; the post step of the Rust cache step saves the new one after this step"
  fi
  return 0
}

self_test() {
  script=$0
  work=$(mktemp -d "${TMPDIR:-/tmp}/keep-one-rust-cache-self-test.XXXXXX") || {
    printf 'keep-one-rust-cache: error: mktemp could not make a directory\n' >&2
    exit 2
  }
  say "self-test in $work, which is left in place"
  mkdir "$work/bin" "$work/gh-config" || exit 2
  cat >"$work/bin/gh" <<'EOF'
#!/bin/sh
state=$KEEP_ONE_STUB_STATE
printf '%s\n' "$*" >>"$state/gh-calls"
case "$1 $2" in
  'cache list')
    if [ -e "$state/list-fails" ]; then
      printf 'HTTP 503: the stub list fails\n' >&2
      exit 1
    fi
    cat "$state/caches"
    ;;
  'cache delete')
    if [ -e "$state/delete-fails-$3" ]; then
      printf 'HTTP 403: the stub delete of %s fails\n' "$3" >&2
      exit 1
    fi
    ;;
  *)
    printf 'stub gh: no such call: %s\n' "$*" >&2
    exit 1
    ;;
esac
EOF
  chmod +x "$work/bin/gh" || exit 2
  found_gh=$(PATH="$work/bin:$PATH" command -v gh) || found_gh=
  if [ "$found_gh" != "$work/bin/gh" ]; then
    printf 'keep-one-rust-cache: error: the stub gh is not the first gh on PATH (found: %s), so the self-test stops before any case\n' "$found_gh" >&2
    exit 2
  fi
  list_call='cache list --key v0-rust-macos- --limit 1000 --json id,key --jq .[] | "cache \(.id) \(.key)"'
  cases=0
  failures=0

  run_case() {
    name=$1
    mkdir "$work/$name" || exit 2
    printf '%s\n' 'cache 11 v0-rust-macos-Darwin-arm64-aaaa-1111' 'cache 12 v0-rust-macos-Darwin-arm64-aaaa-2222' \
      'cache 13 v0-rust-windows-Windows_NT-x64-bbbb-3333' >"$work/$name/caches" || exit 2
    : >"$work/$name/gh-calls"
    case $name in
      list-fails) : >"$work/$name/list-fails" ;;
      delete-fails) : >"$work/$name/delete-fails-12" ;;
    esac
    status=0
    PATH="$work/bin:$PATH" KEEP_ONE_STUB_STATE="$work/$name" GH_CONFIG_DIR="$work/gh-config" GH_REPO=stub.invalid/owner/repo GH_TOKEN='' GITHUB_TOKEN='' \
      sh "$script" macos >"$work/$name/out.txt" 2>&1 || status=$?
  }

  judge() {
    name=$1 want_deletes=$2 want_text=$3 want_lines=$4
    cases=$((cases + 1))
    deletes=$(sed -n 's/^cache delete //p' "$work/$name/gh-calls" | tr '\n' ' ')
    listed=$(sed -n '/^cache list /p' "$work/$name/gh-calls")
    last=$(tail -n 1 "$work/$name/out.txt")
    lines=$(wc -l <"$work/$name/out.txt" | tr -d ' ')
    if [ "$status" = 0 ] && [ "$deletes" = "$want_deletes" ] && [ "$listed" = "$list_call" ] && [ "$lines" = "$want_lines" ]; then
      case $last in
        *"$want_text"*)
          printf 'PASS %s (exit 0, deletes: %s, %s lines; last: %s)\n' "$name" "${deletes:-none}" "$lines" "$last"
          return
          ;;
      esac
    fi
    printf 'FAIL %s (exit %s, deletes: %s, %s lines; expected exit 0, deletes: %s, %s lines, the list call and the text "%s" on the last line; output in %s)\n' \
      "$name" "$status" "${deletes:-none}" "$lines" "${want_deletes:-none}" "$want_lines" "$want_text" "$work/$name/out.txt"
    failures=$((failures + 1))
  }

  run_case two-of-the-job-one-of-the-other
  judge two-of-the-job-one-of-the-other '11 12 ' 'deleted 2 of 2 Rust caches of the job macos;' 3
  run_case list-fails
  judge list-fails '' 'the list of the caches failed (exit 1: HTTP 503: the stub list fails), so no Rust cache of the job macos was deleted; the new cache that this job saves next stands beside the old ones, and the repository may pass the cap of 10 GB' 1
  run_case delete-fails
  judge delete-fails '11 12 ' 'the delete of 1 of 2 Rust caches of the job macos failed (first the cache 12: exit 1: HTTP 403: the stub delete of 12 fails); they stay, and the new cache that this job saves next stands beside the old ones, and the repository may pass the cap of 10 GB' 2

  if [ "$failures" -ne 0 ]; then
    say "self-test FAIL: $failures of $cases cases behaved otherwise. Outputs are in $work."
    exit 1
  fi
  say "self-test PASS: $cases cases (two Rust caches of the job deleted and the other job's kept; a failed list and a failed delete each exit 0 with one line)."
}

[ $# -eq 1 ] || usage 'give one argument: a job (macos or windows), --self-test or --help'
case $1 in
  -h | --help) help_text ;;
  --self-test) self_test ;;
  macos | windows) keep_one "$1" ;;
  *) usage "unknown argument: $1" ;;
esac
