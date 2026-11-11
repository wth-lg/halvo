#!/bin/sh
set -u

usage() {
  cat <<'EOF'
usage: R7_BACKEND_DIR=<backend checkout> sh account/schema/vendor.sh [--check] COMMIT
       R7_BACKEND_DIR=<backend checkout> sh account/schema/vendor.sh --self-test COMMIT

Copies four files of the backend repository, as they stand at COMMIT, into
account/schema/ and writes account/schema/vendored.txt: the commit, then one
line for each file with its sha256, its name here and its path in the backend.
COMMIT is the 40 lower-case hexadecimal digits of a commit object. The script
reads each file with git cat-file, never from the backend's work tree, and
running it twice with one COMMIT changes nothing.

With --check it changes nothing under account/schema/: it compares each
vendored file and the record with the backend at COMMIT, prints one line of
counts and exits 0 only when all of them are equal.

With --self-test it runs its own cases on a copy of account/schema/ in a new
temp directory, against the backend at COMMIT, and changes nothing here. It
needs the backend, so it runs where the backend is and in no CI job.

Each run stages the backend's files in a new directory under TMPDIR (or /tmp)
and leaves it in place.

After a COMMIT that changes schema.v1.json, write the types again:
  cargo run --manifest-path account/Cargo.toml --locked -p halvo-wire --example write-types

Exit status: 0 done or equal, 1 a file differs or the backend lacks the commit
or a file, 2 a step could not run, 64 bad usage.
EOF
}

files='contract/schema/v1.json schema.v1.json
contract/routes/v1.json routes.v1.json
contract/samples/v1.json samples.v1.json
gate/prompt-budget.json prompt-budget.json'

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sum=$(sha256sum <"$1") || return 1
  else
    sum=$(shasum -a 256 <"$1") || return 1
  fi
  printf '%s\n' "${sum%% *}"
}

self_test() {
  dir=$(mktemp -d "${TMPDIR:-/tmp}/r7-vendor-self-test.XXXXXX") || exit 2
  for name in vendor.sh schema.v1.json routes.v1.json samples.v1.json prompt-budget.json vendored.txt; do
    cp "$here/$name" "$dir/$name" || exit 2
  done
  wrong=
  cases=0
  older=$(git -C "$backend" log --format=%H --diff-filter=A -1 "$commit" -- contract/schema/v1.json) || exit 2
  older=$(git -C "$backend" rev-parse "$older^") || exit 2
  tree=$(git -C "$backend" rev-parse "$commit^{tree}") || exit 2
  upper=$(printf '%s' "$commit" | tr 'a-f' 'A-F')
  while IFS='|' read -r case want edit args; do
    case $edit in
      byte) sed '1s/{/[/' "$here/routes.v1.json" >"$dir/routes.v1.json" ;;
      sum) sed '/ samples\.v1\.json /s/^0/g/; / samples\.v1\.json /s/^[1-9a-f]/0/; / samples\.v1\.json /s/^g/1/' "$here/vendored.txt" >"$dir/vendored.txt" ;;
      none) ;;
    esac
    case $args in
      check) set -- --check "$commit" ;;
      vendor) set -- "$commit" ;;
      tree) set -- --check "$tree" ;;
      older) set -- --check "$older" ;;
      short) set -- --check "${commit%?}" ;;
      upper) set -- --check "$upper" ;;
      nobackend) set -- --check "$commit" ;;
    esac
    if [ "$args" = nobackend ]; then
      out=$(R7_BACKEND_DIR="$dir/no-backend" sh "$dir/vendor.sh" "$@" 2>&1)
    else
      out=$(R7_BACKEND_DIR="$backend" sh "$dir/vendor.sh" "$@" 2>&1)
    fi
    code=$?
    last=${out##*"
"}
    cases=$((cases + 1))
    case "$code $last" in
      "$want") verdict=ok ;;
      *) verdict=wrong wrong="$wrong $case" ;;
    esac
    printf 'case vendor/%s want=%s got=%s %s: %s\n' "$case" "$want" "$code" "$verdict" "$last"
    cp "$here/routes.v1.json" "$dir/routes.v1.json" || exit 2
    cp "$here/vendored.txt" "$dir/vendored.txt" || exit 2
  done <<EOF
equal|0 vendor-check commit=$commit files=4 equal=4 record=equal|none|check
second-run-changes-nothing|0 vendored commit=$commit files=4 changed=0 record=kept|none|vendor
changed-byte|1 vendor-check commit=$commit files=4 equal=3 record=equal|byte|check
changed-byte-is-written-back|0 vendored commit=$commit files=4 changed=1 record=kept|byte|vendor
changed-sum|1 vendor-check commit=$commit files=4 equal=4 record=differs|sum|check
changed-sum-is-written-back|0 vendored commit=$commit files=4 changed=0 record=written|sum|vendor
a-tree-is-no-commit|1 vendor: the backend has no commit $tree|none|tree
a-commit-without-a-file|1 vendor: the backend has no contract/schema/v1.json at $older|none|older
a-short-commit|64 or a file, 2 a step could not run, 64 bad usage.|none|short
an-upper-case-commit|64 or a file, 2 a step could not run, 64 bad usage.|none|upper
no-backend-repository|2 vendor: R7_BACKEND_DIR names no git repository|none|nobackend
EOF
  if [ -n "$wrong" ]; then
    printf 'FAIL vendor self-test (wrong verdict in:%s)\n' "$wrong"
    exit 1
  fi
  printf 'PASS vendor self-test (%s cases)\n' "$cases"
}

mode=vendor
case ${1:-} in
  --check)
    mode=check
    shift
    ;;
  --self-test)
    mode=self-test
    shift
    ;;
esac
if [ $# -ne 1 ]; then
  usage >&2
  exit 64
fi
commit=$1
case $commit in
  *[!0-9a-f]*)
    usage >&2
    exit 64
    ;;
esac
if [ "${#commit}" -ne 40 ]; then
  usage >&2
  exit 64
fi
backend=${R7_BACKEND_DIR:-}
if [ -z "$backend" ]; then
  printf 'vendor: R7_BACKEND_DIR is not set\n' >&2
  exit 64
fi
if ! git -C "$backend" rev-parse --git-dir >/dev/null 2>&1; then
  printf 'vendor: R7_BACKEND_DIR names no git repository\n' >&2
  exit 2
fi
kind=$(git -C "$backend" cat-file -t "$commit" 2>/dev/null)
if [ "$kind" != commit ]; then
  printf 'vendor: the backend has no commit %s\n' "$commit" >&2
  exit 1
fi
here=$(cd "$(dirname "$0")" && pwd) || exit 2
if [ "$mode" = self-test ]; then
  self_test
  exit 0
fi
stage=$(mktemp -d "${TMPDIR:-/tmp}/r7-vendor.XXXXXX") || exit 2

printf 'commit %s\n' "$commit" >"$stage/vendored.txt" || exit 2
while read -r source name; do
  if ! git -C "$backend" cat-file blob "$commit:$source" >"$stage/$name" 2>/dev/null; then
    printf 'vendor: the backend has no %s at %s\n' "$source" "$commit" >&2
    exit 1
  fi
  sum=$(sha256_of "$stage/$name") || exit 2
  printf '%s %s %s\n' "$sum" "$name" "$source" >>"$stage/vendored.txt" || exit 2
done <<EOF
$files
EOF

total=0
equal=0
while read -r source name; do
  total=$((total + 1))
  if cmp -s "$stage/$name" "$here/$name"; then
    equal=$((equal + 1))
  elif [ "$mode" = vendor ]; then
    cp "$stage/$name" "$here/$name" || exit 2
  fi
done <<EOF
$files
EOF
if cmp -s "$stage/vendored.txt" "$here/vendored.txt"; then
  record=equal
  written=kept
else
  record=differs
  written=written
  if [ "$mode" = vendor ]; then
    cp "$stage/vendored.txt" "$here/vendored.txt" || exit 2
  fi
fi

if [ "$mode" = vendor ]; then
  printf 'vendored commit=%s files=%s changed=%s record=%s\n' "$commit" "$total" "$((total - equal))" "$written"
  exit 0
fi
printf 'vendor-check commit=%s files=%s equal=%s record=%s\n' "$commit" "$total" "$equal" "$record"
if [ "$equal" -ne "$total" ] || [ "$record" != equal ]; then
  exit 1
fi
