#!/bin/sh
# jobs: macos windows
# rule: No file under crates/halvo-capture/src/daemon/ names EncryptedStore, SegmentStore, rusqlite or store::db (also spelled store::{ or with blanks around ::), and src/daemon/ holds a .rs file.
set -u
name=23-daemon-port-only
banned='EncryptedStore|SegmentStore|rusqlite|(^|[^A-Za-z0-9_])store[[:space:]]*::[[:space:]]*(db|[{])'

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

classify() {
  daemon=$1/crates/halvo-capture/src/daemon
  files=$(find "$daemon" -type f -name '*.rs' 2>/dev/null | LC_ALL=C sort)
  if [ -z "$files" ]; then
    printf 'crates/halvo-capture/src/daemon holds no .rs file'
    return 1
  fi
  count=$(printf '%s\n' "$files" | grep -c '')
  printf 'daemon files: %s\n' "$count" >&2
  set --
  while IFS= read -r file; do
    set -- "$@" "$file"
  done <<EOF
$files
EOF
  matches=$(grep -nE "$banned" "$@")
  status=$?
  case $status in
    1) return 0 ;;
    0)
      first=$(printf '%s\n' "$matches" | sed -e 's|^\([^:]*:[0-9]*\):.*$|\1|' -e 's|^.*/crates/halvo-capture/|crates/halvo-capture/|' -e 1q)
      printf 'the daemon names the store behind its port at %s' "$first"
      return 1
      ;;
    *)
      printf 'grep exited %s' "$status"
      return 1
      ;;
  esac
}

measure() {
  cause=$(classify "${CI_D_REPO:-.}") || fail "$cause"
  printf 'PASS %s\n' "$name"
}

fixture() {
  tree=$work/$1/crates/halvo-capture/src/daemon
  mkdir -p "$tree" || exit 2
  printf '%s\n' 'pub mod owner;' 'pub mod port;' >"$tree/mod.rs" || exit 2
  printf '%s\n' 'use super::port::CaptureStore;' 'pub struct Owner;' >"$tree/owner.rs" || exit 2
  printf '%s\n' 'pub trait CaptureStore {}' >"$tree/port.rs" || exit 2
}

case_of() {
  cause=$(classify "$work/$1" 2>/dev/null)
  status=$?
  if [ "$status" -eq 0 ]; then got=PASS; else got=FAIL; fi
  printf 'case %s/%s want=%s got=%s %s\n' "$name" "$1" "$2" "$got" "$cause"
  if [ "$got" != "$2" ]; then
    wrong="$wrong $1"
  elif [ "$got" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

with_line() {
  fixture "$1"
  printf '%s\n' "$2" >>"$work/$1/crates/halvo-capture/src/daemon/owner.rs" || exit 2
  case_of "$1" FAIL
}

self_test() {
  rejected=0
  wrong=
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  fixture clean
  case_of clean PASS
  with_line encrypted-store-use 'use crate::store::db::EncryptedStore;'
  with_line segment-store 'fn f(_: &dyn crate::store::SegmentStore) {}'
  with_line rusqlite 'use rusqlite::Connection;'
  with_line store-db-path 'fn g() { let _ = crate::store::db::open; }'
  with_line store-brace 'use crate::store::{db, segment};'
  with_line store-spaced 'use crate::store :: db;'
  fixture nested
  mkdir -p "$work/nested/crates/halvo-capture/src/daemon/server" || exit 2
  printf '%s\n' 'use rusqlite::Connection;' >"$work/nested/crates/halvo-capture/src/daemon/server/mod.rs" || exit 2
  case_of nested FAIL
  fixture like-names
  printf '%s\n' 'pub struct StoredText;' 'fn restore_db() {}' 'pub mod store_db_like;' 'use crate::daemon::model_store::{Harness, ModelStore};' 'fn h() { let _ = restore::db; }' >>"$work/like-names/crates/halvo-capture/src/daemon/owner.rs" || exit 2
  case_of like-names PASS
  mkdir -p "$work/empty/crates/halvo-capture/src/daemon" || exit 2
  case_of empty FAIL
  case_of missing FAIL
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
