#!/bin/sh
# jobs: macos windows
# rule: Every file under crates/halvo-capture/src/daemon/ but the data file mutants.rows, whatever its name and through a link, read as text with its lines joined, names neither EncryptedStore, SegmentStore nor rusqlite, nor a path from crate, $crate, super or halvo_capture to the module store (also after a brace or as r#store), nor store::db or store::{, nor a path segment that a macro's metavariable gives, nor an alias of crate, self, super or halvo_capture, nor a glob of the crate root (crate::*, $crate::*, halvo_capture::*, a brace list with * right under one of them, or two or more super before it), nor the module's name store, plain or raw, at the head of a path, in use store or in a brace list of a use line (a single use super::*; stays allowed); a listing that fails fails the check with its path; and src/daemon/ holds a file to read.
set -u
name=23-daemon-port-only

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

scan() {
  awk '
    function rel(path) {
      sub(/^.*\/crates\/halvo-capture\//, "crates/halvo-capture/", path)
      return path
    }
    function judge(    text) {
      if (file == "") return
      text = joined
      gsub(/\$crate/, "crate", text)
      if (text ~ /(EncryptedStore|SegmentStore|rusqlite)/)
        why = "names the encrypted store, the segment store or rusqlite"
      else if (text ~ /(^|[^A-Za-z0-9_])(crate|super|halvo_capture)[[:space:]]*::[[:space:]]*([{]([^}]*[^A-Za-z0-9_])?)?(r#)?store([^A-Za-z0-9_]|$)/)
        why = "names a path to the module store"
      else if (text ~ /(^|[^A-Za-z0-9_])(r#)?store[[:space:]]*::[[:space:]]*(r#)?(db|[{])/)
        why = "names store::db or store::{"
      else if (text ~ /\$[A-Za-z_][A-Za-z0-9_]*[[:space:]]*::|::[[:space:]]*\$[A-Za-z_]/)
        why = "builds a path from a macro metavariable"
      else if (text ~ /(^|[^A-Za-z0-9_])(crate|self|super|halvo_capture)[[:space:]]+as[[:space:]]/)
        why = "makes an alias of crate, self, super or halvo_capture"
      else if (text ~ /(^|[^A-Za-z0-9_])(crate|halvo_capture)[[:space:]]*::[[:space:]]*([*]|[{]([^}]*,)?[[:space:]]*[*])/)
        why = "imports the names of the crate root with a glob"
      else if (text ~ /(^|[^A-Za-z0-9_])super[[:space:]]*::[[:space:]]*(super[[:space:]]*::[[:space:]]*)+([*]|[{]([^}]*,)?[[:space:]]*[*])/)
        why = "imports with a glob the names of a module two or more levels up"
      else if (text ~ /(^|[^A-Za-z0-9_:.])(r#)?store[[:space:]]*::/ || text ~ /(^|[^A-Za-z0-9_])use[[:space:]]+(r#)?store([^A-Za-z0-9_]|$)/ || text ~ /(^|[^A-Za-z0-9_])use[[:space:]][^;]*[{][^;]*(^|[^A-Za-z0-9_:])(r#)?store([^A-Za-z0-9_]|$)/)
        why = "names the module store at the head of a path"
      else
        return
      print rel(file) ": " why
      found = 1
      exit 1
    }
    FNR == 1 {
      judge()
      file = FILENAME
      joined = ""
    }
    { joined = joined " " $0 }
    END { if (!found) judge() }
  ' "$@"
}

classify() {
  root=$1
  daemon=$root/crates/halvo-capture/src/daemon
  if [ ! -d "$daemon" ]; then
    printf 'crates/halvo-capture/src/daemon holds no file to read'
    return 1
  fi
  found=$(find "$daemon" ! -type d 2>&1)
  find_status=$?
  broken=$(printf '%s\n' "$found" | awk -v d="$daemon/" 'index($0, d) != 1 && $0 != ""')
  if [ -n "$broken" ]; then
    first=$(printf '%s\n' "$broken" | sed -n 1p)
    printf 'the listing of the daemon directory failed: %s' "${first#*"$root"/}"
    return 1
  fi
  if [ "$find_status" != 0 ]; then
    printf 'the listing of the daemon directory failed: find exited %s' "$find_status"
    return 1
  fi
  listed=$(printf '%s\n' "$found" | LC_ALL=C sort)
  files=$(printf '%s\n' "$listed" | grep -v -x -F -e "$daemon/mutants.rows" -e '')
  if [ -z "$files" ]; then
    printf 'crates/halvo-capture/src/daemon holds no file to read'
    return 1
  fi
  count=$(printf '%s\n' "$files" | grep -c '')
  printf 'daemon files: %s\n' "$count" >&2
  set --
  while IFS= read -r file; do
    if [ -d "$file" ] || [ ! -r "$file" ]; then
      printf '%s is a link to a directory or to no readable file' "${file#"$root"/}"
      return 1
    fi
    set -- "$@" "$file"
  done <<EOF
$files
EOF
  left=$(cat "$@" | grep -c -E 'use[[:space:]]+super[[:space:]]*::[[:space:]]*[*][[:space:]]*;')
  printf 'lines that the glob rules leave alone: %s (use super::*;, a glob of the parent module), exceptions listed: 0\n' "$left" >&2
  found=$(scan "$@")
  status=$?
  case $status in
    0) return 0 ;;
    1)
      printf 'the daemon reaches the store behind its port: %s' "$found"
      return 1
      ;;
    *)
      printf 'awk exited %s' "$status"
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
  printf '%s\n' 'pub mod owner;' 'pub mod port;' 'mod model_store;' >"$tree/mod.rs" || exit 2
  printf '%s\n' 'use super::port::CaptureStore;' 'use crate::daemon::model_store::{Harness, ModelStore};' 'pub struct Owner;' >"$tree/owner.rs" || exit 2
  printf '%s\n' 'pub trait CaptureStore {}' 'pub struct SegmentWrite;' >"$tree/port.rs" || exit 2
  printf '%s\n' 'row	owner.rs	s/use crate::store::db::EncryptedStore;/x/	test y	required	P2' >"$tree/mutants.rows" || exit 2
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
  with_line alias 'use crate::store as st;'
  with_line glob 'use crate::store::*;'
  with_line raw-ident 'fn h() { let _ = std::any::type_name::<crate::store::r#db::Db>(); }'
  with_line raw-store 'fn h() { let _ = crate::r#store::db::Db::open; }'
  with_line macro-arguments "macro_rules! from { (\$m:ident :: \$t:ident) => { crate::\$m::\$t }; }"
  with_line macro-crate "macro_rules! from { () => { \$crate::store::db::Db }; }"
  with_line super-path 'use super::super::store::db;'
  with_line brace-import 'use crate::{daemon, store};'
  with_line crate-alias 'use crate as root;'
  with_line glob-then-glob 'use crate::*; use store::*; fn f() { let _ = std::any::type_name::<db::Db>(); }'
  with_line glob-then-alias 'use crate::*; use store as backing; fn f() { let _ = std::any::type_name::<backing::db::Db>(); }'
  with_line name-glob-in-expression 'use halvo_capture::*; fn f() { let _ = store::db::Db::open; }'
  with_line super-super-glob 'use super::super::*; fn f() { let _ = store::db::Db::open; }'
  with_line brace-glob 'use crate::{*};'
  with_line glob-in-inline-module 'mod inner { use crate::*; fn f() { let _ = std::any::type_name::<store::db::Db>(); } }'
  with_line raw-name-at-head 'fn f() { let _ = r#store::db::Db::open; }'
  with_line name-glob-to-segment 'use halvo_capture::*; fn f() { let _ = store::segment::SegmentKey::random; }'
  with_line super-super-glob-to-segment 'use super::super::*; fn f() { let _ = store::segment::SegmentKey::random; }'
  with_line glob-in-inline-module-to-segment 'mod inner { use crate::*; fn f() { let _ = store::segment::SegmentKey::random; } }'
  with_line raw-name-at-head-to-segment 'fn f() { let _ = r#store::segment::SegmentKey::random; }'
  with_line name-alias 'use halvo_capture as hc;'
  with_line extern-self-alias 'extern crate self as me;'
  with_line use-name-alone 'use store;'
  with_line name-in-brace-list 'use self::{port, store};'
  fixture parent-glob
  printf '%s\n' '#[cfg(test)]' 'mod tests {' '    use super::*;' '}' >>"$work/parent-glob/crates/halvo-capture/src/daemon/owner.rs" || exit 2
  case_of parent-glob PASS
  fixture split-brace
  printf '%s\n' 'use crate::{' '    daemon::port,' '    store::db,' '};' >>"$work/split-brace/crates/halvo-capture/src/daemon/owner.rs" || exit 2
  case_of split-brace FAIL
  fixture upper-suffix
  printf '%s\n' 'use crate::store::db::Db;' >"$work/upper-suffix/crates/halvo-capture/src/daemon/evil.RS" || exit 2
  case_of upper-suffix FAIL
  fixture no-suffix
  printf '%s\n' 'use crate::store::db::Db;' >"$work/no-suffix/crates/halvo-capture/src/daemon/evil" || exit 2
  case_of no-suffix FAIL
  fixture linked
  mkdir -p "$work/linked/outside" || exit 2
  printf '%s\n' 'use crate::store::db::Db;' >"$work/linked/outside/linked.txt" || exit 2
  ln -s ../../../../outside/linked.txt "$work/linked/crates/halvo-capture/src/daemon/linked_tests.rs" || exit 2
  if [ -L "$work/linked/crates/halvo-capture/src/daemon/linked_tests.rs" ]; then made='link'; else made='copy'; fi
  printf 'case %s/linked: ln -s made a %s\n' "$name" "$made"
  case_of linked FAIL
  fixture data-file-renamed
  mv "$work/data-file-renamed/crates/halvo-capture/src/daemon/mutants.rows" "$work/data-file-renamed/crates/halvo-capture/src/daemon/Mutants.rows" || exit 2
  case_of data-file-renamed FAIL
  fixture nested
  mkdir -p "$work/nested/crates/halvo-capture/src/daemon/server" || exit 2
  printf '%s\n' 'use rusqlite::Connection;' >"$work/nested/crates/halvo-capture/src/daemon/server/mod.rs" || exit 2
  case_of nested FAIL
  fixture like-names
  printf '%s\n' 'pub struct StoredText;' 'fn restore_db() {}' 'pub mod store_db_like;' 'fn h() { let _ = restore::db; }' 'use crate::daemon::{model_store, port};' 'fn k(store: &mut ModelStore) { store.records(); }' 'use self::port::CaptureStore as Port;' >>"$work/like-names/crates/halvo-capture/src/daemon/owner.rs" || exit 2
  case_of like-names PASS
  fixture unlisted-dir
  locked=$work/unlisted-dir/crates/halvo-capture/src/daemon/locked
  mkdir -p "$locked" || exit 2
  printf '%s\n' 'use crate::store::db::Db;' >"$locked/mod.rs" || exit 2
  trap 'chmod 755 "$locked" 2>/dev/null' EXIT INT TERM HUP
  chmod 311 "$locked" || exit 2
  uid=$(id -u) || exit 2
  if [ "$uid" = 0 ]; then
    printf 'case %s/unlisted-dir: the system'"'"'s first user lists any directory, so the fixture cannot be made; the self-test fails closed\n' "$name"
    wrong="$wrong unlisted-dir"
  elif ls "$locked" >/dev/null 2>&1; then
    printf 'case %s/unlisted-dir: not run, this system lists a directory of mode 311\n' "$name"
  else
    case_of unlisted-dir FAIL
  fi
  chmod 755 "$locked" || exit 2
  trap - EXIT INT TERM HUP
  mkdir -p "$work/empty/crates/halvo-capture/src/daemon" || exit 2
  case_of empty FAIL
  case_of missing FAIL
  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

usage() {
  cat <<'EOF'
usage: sh scripts/r1/ci.d/23-daemon-port-only.sh [--self-test | --help]
Start it at the root of the repository, or run it through
sh scripts/ci/run-ci-d.sh <job> 23-daemon-port-only.
With no argument it reads every file under crates/halvo-capture/src/daemon/
as text and prints PASS, or FAIL with the file and the cause. --self-test runs
it over fixture trees that must pass or fail. The rule that it holds is the
line "# rule:" of this file, which sh scripts/ci/run-ci-d.sh --rules prints.

Limit: checks 22 and 23 read text. The two checks refuse the listed spellings
alone, and a daemon file that reaches the store's database type by another
spelling is refused by review and not by a check. Ruling DM-7 moves the
boundary to the compiler in a later unit. Until it lands, these forms pass
this check and reach the store's database type in the real crate:
- a use line with the name store right behind the brace:
  use {store as backing, sink};
- self::store after the one allowed use super::*; in daemon/mod.rs, where the
  parent is the crate root: use super::*; then use self::store as backing;
- self::store after a glob of the crate root written with doubled braces,
  use crate::{{*}};, or with a comment before the star, use crate::/* x */*;
- a macro in a daemon file that joins its arguments by a repetition,
  ($($seg:ident),+) => { $($seg)::+ }, called as p!(crate, store, db, Db).
EOF
}

case ${1:-} in
  --self-test) self_test ;;
  -h | --help) usage ;;
  '') measure ;;
  *) exit 64 ;;
esac
