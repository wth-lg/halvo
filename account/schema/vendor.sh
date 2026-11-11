#!/bin/sh
set -u

usage() {
  cat <<'EOF'
usage: R7_BACKEND_DIR=<backend checkout> sh account/schema/vendor.sh [--check] COMMIT

Copies four files of the backend repository, as they stand at COMMIT, into
account/schema/ and writes account/schema/vendored.txt: the commit, then one
line for each file with its sha256, its name here and its path in the backend.
It reads each file with git cat-file, never from the backend's work tree, and
running it twice with one COMMIT changes nothing.

With --check it writes nothing: it compares each vendored file and the record
with the backend at COMMIT, prints one line of counts and exits 0 only when
all of them are equal.

After a COMMIT that changes schema.v1.json, write the types again:
  cargo run --manifest-path account/Cargo.toml --locked -p halvo-wire --example write-types

Exit status: 0 done or equal, 1 a file differs or the backend lacks it,
2 a step could not run, 64 bad usage.
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

check=no
if [ "${1:-}" = --check ]; then
  check=yes
  shift
fi
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
if ! git -C "$backend" cat-file -e "$commit^{commit}" 2>/dev/null; then
  printf 'vendor: the backend has no commit %s\n' "$commit" >&2
  exit 1
fi
here=$(cd "$(dirname "$0")" && pwd) || exit 2
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
  elif [ "$check" = no ]; then
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
  if [ "$check" = no ]; then
    cp "$stage/vendored.txt" "$here/vendored.txt" || exit 2
  fi
fi

if [ "$check" = no ]; then
  printf 'vendored commit=%s files=%s changed=%s record=%s\n' "$commit" "$total" "$((total - equal))" "$written"
  exit 0
fi
printf 'vendor-check commit=%s files=%s equal=%s record=%s\n' "$commit" "$total" "$equal" "$record"
if [ "$equal" -ne "$total" ] || [ "$record" != equal ]; then
  exit 1
fi
