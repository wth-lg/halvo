#!/bin/sh
set -u

usage() {
  cat <<'EOF'
usage: sh account/schema/vendor.sh [--check | --self-test]

Copies four files of the backend in the same tree into account/schema/ and
writes account/schema/vendored.txt: one line for each file with its sha256, its
name here and its path from the top of the tree. The script reads each file at
that path in the work tree that holds it, and running it twice changes nothing.
The four files:
  backend/contract/schema/v1.json as schema.v1.json
  backend/contract/routes/v1.json as routes.v1.json
  backend/contract/samples/v1.json as samples.v1.json
  backend/gate/prompt-budget.json as prompt-budget.json

With --check it changes nothing under account/schema/: it compares each
vendored file and the record with the four files of the backend, prints one
line for each file that differs and for a record that differs, then one line
of counts, and exits 0 only when all of them are equal.

With --self-test it runs its own cases on a copy of account/schema/ and of the
four files of the backend in a new temp directory, and changes nothing here.

Each run stages its files in a new directory under TMPDIR (or /tmp) and leaves
it in place.

After a run that changes schema.v1.json, write the types again:
  cargo run --manifest-path account/Cargo.toml --locked -p halvo-wire --example write-types

Exit status: 0 done or equal, 1 a file differs or the backend lacks a file, 2
a step could not run, 64 bad usage.
EOF
}

files='backend/contract/schema/v1.json schema.v1.json
backend/contract/routes/v1.json routes.v1.json
backend/contract/samples/v1.json samples.v1.json
backend/gate/prompt-budget.json prompt-budget.json'

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sum=$(sha256sum <"$1") || return 1
  else
    sum=$(shasum -a 256 <"$1") || return 1
  fi
  printf '%s\n' "${sum%% *}"
}

fresh_copy() {
  for name in vendor.sh schema.v1.json routes.v1.json samples.v1.json prompt-budget.json vendored.txt; do
    cp "$here/$name" "$dir/account/schema/$name" || exit 2
  done
  while read -r source name; do
    cp "$root/$source" "$dir/$source" || exit 2
  done <<EOF
$files
EOF
}

self_test() {
  dir=$(mktemp -d "${TMPDIR:-/tmp}/r7-vendor-self-test.XXXXXX") || exit 2
  mkdir -p "$dir/account/schema" || exit 2
  while read -r source name; do
    mkdir -p "$dir/${source%/*}" || exit 2
  done <<EOF
$files
EOF
  mkdir -p "$dir/elsewhere/a" "$dir/elsewhere/account/schema" || exit 2
  while read -r source name; do
    mkdir -p "$dir/elsewhere/${source%/*}" && cp "$here/$name" "$dir/elsewhere/$source" || exit 2
  done <<EOF
$files
EOF
  wrong=
  cases=0
  copy=$dir/account/schema
  linked=yes
  ln -s "$copy" "$dir/elsewhere/a/s" 2>/dev/null || linked=no
  [ -L "$dir/elsewhere/a/s" ] || linked=no
  while IFS='|' read -r case want edit args after; do
    fresh_copy
    case $edit in
      byte) sed '1s/{/[/' "$here/routes.v1.json" >"$copy/routes.v1.json" ;;
      sum) sed '/ samples\.v1\.json /s/^0/g/; / samples\.v1\.json /s/^[1-9a-f]/0/; / samples\.v1\.json /s/^g/1/' "$here/vendored.txt" >"$copy/vendored.txt" ;;
      backend-byte) sed '1s/{/[/' "$root/backend/contract/schema/v1.json" >"$dir/backend/contract/schema/v1.json" ;;
      backend-routes) sed '1s/{/[/' "$root/backend/contract/routes/v1.json" >"$dir/backend/contract/routes/v1.json" ;;
      backend-samples) sed '1s/{/[/' "$root/backend/contract/samples/v1.json" >"$dir/backend/contract/samples/v1.json" ;;
      backend-budget) sed '1s/{/[/' "$root/backend/gate/prompt-budget.json" >"$dir/backend/gate/prompt-budget.json" ;;
      backend-gone) mv "$dir/backend/gate/prompt-budget.json" "$dir/prompt-budget.json.gone" || exit 2 ;;
      old-form) awk 'BEGIN { print "commit 651e68fb43c14381c8ae104a7fa3f40d4d3311f7" } { sub(/^[^\/]*\//, "", $3); print }' "$here/vendored.txt" >"$copy/vendored.txt" ;;
      none) ;;
    esac
    case $args in
      check) set -- --check ;;
      vendor) set -- ;;
      commit) set -- --check 651e68fb43c14381c8ae104a7fa3f40d4d3311f7 ;;
      two) set -- --check --self-test ;;
      other) set -- --write ;;
    esac
    if [ "$args" = link ] && [ "$linked" = no ]; then
      printf 'case vendor/%s not-run: this system made no symbolic link\n' "$case"
      continue
    fi
    case $args in
      link) out=$(sh "$dir/elsewhere/a/s/vendor.sh" --check 2>&1) ;;
      cdpath) out=$(cd "$dir" && CDPATH="$dir/elsewhere" sh account/schema/vendor.sh --check 2>&1) ;;
      *) out=$(sh "$copy/vendor.sh" "$@" 2>&1) ;;
    esac
    code=$?
    last=${out##*"
"}
    cases=$((cases + 1))
    case "$code $last" in
      "$want") verdict=ok ;;
      *) verdict=wrong ;;
    esac
    case $after in
      routes-kept) ! cmp -s "$copy/routes.v1.json" "$here/routes.v1.json" || verdict=wrong ;;
      record-kept) ! cmp -s "$copy/vendored.txt" "$here/vendored.txt" || verdict=wrong ;;
      all-equal)
        for name in schema.v1.json routes.v1.json samples.v1.json prompt-budget.json vendored.txt; do
          cmp -s "$copy/$name" "$here/$name" || verdict=wrong
        done
        ;;
      schema-follows)
        cmp -s "$copy/schema.v1.json" "$dir/backend/contract/schema/v1.json" || verdict=wrong
        ! cmp -s "$copy/vendored.txt" "$here/vendored.txt" || verdict=wrong
        ;;
      from-routes)
        case $out in
          *'vendor-check: routes.v1.json differs from backend/contract/routes/v1.json'*) ;;
          *) verdict=wrong ;;
        esac
        cmp -s "$copy/routes.v1.json" "$here/routes.v1.json" || verdict=wrong
        ;;
      from-samples)
        case $out in
          *'vendor-check: samples.v1.json differs from backend/contract/samples/v1.json'*) ;;
          *) verdict=wrong ;;
        esac
        cmp -s "$copy/samples.v1.json" "$here/samples.v1.json" || verdict=wrong
        ;;
      from-budget)
        case $out in
          *'vendor-check: prompt-budget.json differs from backend/gate/prompt-budget.json'*) ;;
          *) verdict=wrong ;;
        esac
        cmp -s "$copy/prompt-budget.json" "$here/prompt-budget.json" || verdict=wrong
        ;;
      names-schema)
        case $out in
          *'vendor-check: schema.v1.json differs from backend/contract/schema/v1.json'*) ;;
          *) verdict=wrong ;;
        esac
        cmp -s "$copy/schema.v1.json" "$here/schema.v1.json" || verdict=wrong
        ;;
      names-routes)
        case $out in
          *'vendor-check: routes.v1.json differs from backend/contract/routes/v1.json'*) ;;
          *) verdict=wrong ;;
        esac
        ! cmp -s "$copy/routes.v1.json" "$here/routes.v1.json" || verdict=wrong
        ;;
      names-record)
        case $out in
          *'vendor-check: vendored.txt differs from the files that it names'*) ;;
          *) verdict=wrong ;;
        esac
        ! cmp -s "$copy/vendored.txt" "$here/vendored.txt" || verdict=wrong
        ;;
      changes-nothing)
        for name in schema.v1.json routes.v1.json samples.v1.json prompt-budget.json vendored.txt; do
          cmp -s "$copy/$name" "$here/$name" || verdict=wrong
        done
        ;;
    esac
    [ "$verdict" = ok ] || wrong="$wrong $case"
    printf 'case vendor/%s want=%s got=%s %s: %s\n' "$case" "$want" "$code" "$verdict" "$last"
  done <<EOF
equal|0 vendor-check files=4 equal=4 record=equal|none|check|all-equal
second-run-changes-nothing|0 vendored files=4 changed=0 record=kept|none|vendor|all-equal
changed-byte|1 vendor-check files=4 equal=3 record=equal|byte|check|names-routes
changed-byte-is-written-back|0 vendored files=4 changed=1 record=kept|byte|vendor|all-equal
changed-sum|1 vendor-check files=4 equal=4 record=differs|sum|check|names-record
changed-sum-is-written-back|0 vendored files=4 changed=0 record=written|sum|vendor|all-equal
changed-backend-byte|1 vendor-check files=4 equal=3 record=differs|backend-byte|check|names-schema
changed-backend-routes|1 vendor-check files=4 equal=3 record=differs|backend-routes|check|from-routes
changed-backend-samples|1 vendor-check files=4 equal=3 record=differs|backend-samples|check|from-samples
changed-backend-prompt-budget|1 vendor-check files=4 equal=3 record=differs|backend-budget|check|from-budget
run-through-a-link|1 vendor-check files=4 equal=3 record=differs|backend-byte|link|names-schema
cdpath-exported|0 vendor-check files=4 equal=4 record=equal|none|cdpath|all-equal
changed-backend-byte-is-vendored|0 vendored files=4 changed=1 record=written|backend-byte|vendor|schema-follows
record-of-the-old-form|1 vendor-check files=4 equal=4 record=differs|old-form|check|names-record
backend-without-a-file|1 vendor: backend/gate/prompt-budget.json is missing|backend-gone|vendor|changes-nothing
a-commit-argument|64 a step could not run, 64 bad usage.|none|commit|changes-nothing
two-modes|64 a step could not run, 64 bad usage.|none|two|changes-nothing
another-flag|64 a step could not run, 64 bad usage.|none|other|changes-nothing
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
if [ $# -ne 0 ]; then
  usage >&2
  exit 64
fi
unset CDPATH
here=$(cd -P "$(dirname "$0")" && pwd -P) || exit 2
root=$(cd -P "$here/../.." && pwd -P) || exit 2
if [ "$mode" = self-test ]; then
  self_test
  exit 0
fi
stage=$(mktemp -d "${TMPDIR:-/tmp}/r7-vendor.XXXXXX") || exit 2

: >"$stage/vendored.txt" || exit 2
while read -r source name; do
  if [ ! -f "$root/$source" ]; then
    printf 'vendor: %s is missing\n' "$source" >&2
    exit 1
  fi
  cp "$root/$source" "$stage/$name" || exit 2
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
  else
    printf 'vendor-check: %s differs from %s\n' "$name" "$source"
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
  else
    printf 'vendor-check: vendored.txt differs from the files that it names\n'
  fi
fi

if [ "$mode" = vendor ]; then
  printf 'vendored files=%s changed=%s record=%s\n' "$total" "$((total - equal))" "$written"
  exit 0
fi
printf 'vendor-check files=%s equal=%s record=%s\n' "$total" "$equal" "$record"
if [ "$equal" -ne "$total" ] || [ "$record" != equal ]; then
  exit 1
fi
