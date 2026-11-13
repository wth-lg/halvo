#!/bin/sh
# jobs: macos windows
# rule: Every file under crates/halvo-capture/src/daemon/ but the data file mutants.rows, whatever its name and through a link, and crates/halvo-capture/src/bin/halvo-capture.rs hold no allow or expect of dead_code, of an unused lint or of warnings; no mod line under a cfg that names test, but the one platform condition of daemon/mod.rs, names a file that is not test support (*_tests.rs, *_props.rs, model_*.rs, scripted.rs; an inline mod tests is no file), also written r#name; no cfg under src/daemon/ joins test with a platform but that one condition; an attribute is read whole over its lines; and src/daemon/ holds a file to read.
set -u
name=22-daemon-no-dead-allow

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

scan() {
  awk -v mod_rs="$1/mod.rs" '
    function rel(path) {
      sub(/^.*\/crates\/halvo-capture\//, "crates/halvo-capture/", path)
      return path
    }
    function cause(file, at, text) {
      print rel(file) ":" at ": " text
      found = 1
      exit 1
    }
    function trimmed(text) {
      gsub(/[[:space:]]+/, " ", text)
      gsub(/^ | $/, "", text)
      return text
    }
    function count(text, char,    copy) {
      copy = text
      return gsub(char, "", copy)
    }
    function check_allowances() {
      if (file != "" && joined ~ /(allow|expect)[[:space:]]*\([^)]*(dead_code|unused|warnings)/)
        cause(file, "-", "an allow or an expect of dead_code, of an unused lint or of warnings")
    }
    function attribute(text, at) {
      if (text !~ /cfg[[:space:]]*\(/) return
      tested = (text ~ /[^A-Za-z0-9_]test[^A-Za-z0-9_]/)
      platform = (text ~ /(target_os|target_family|target_arch|target_env|target_vendor|[^A-Za-z0-9_]windows[^A-Za-z0-9_]|[^A-Za-z0-9_]unix[^A-Za-z0-9_])/)
      if (tested && platform) {
        if (FILENAME != mod_rs) cause(FILENAME, at, "a cfg joins test with a platform outside daemon/mod.rs")
        if (condition == "") condition = trimmed(text)
        else if (condition != trimmed(text)) cause(FILENAME, at, "daemon/mod.rs joins test with a platform in a second condition")
        pending = 0
      } else {
        pending = tested
      }
    }
    FNR == 1 {
      check_allowances()
      file = FILENAME
      joined = ""
      pending = 0
      open_attr = ""
    }
    {
      joined = joined " " $0
      line = $0
    }
    FILENAME ~ /\/src\/bin\// { next }
    open_attr != "" {
      open_attr = open_attr " " line
      if (count(open_attr, "\\[") > count(open_attr, "\\]")) next
      attribute(open_attr, attr_at)
      open_attr = ""
      next
    }
    line ~ /^[[:space:]]*#!?\[/ {
      if (count(line, "\\[") > count(line, "\\]")) {
        open_attr = line
        attr_at = FNR
        next
      }
      attribute(line, FNR)
      rest = line
      sub(/^.*\][[:space:]]*/, "", rest)
      if (rest == "") next
      line = rest
    }
    pending {
      if (line ~ /^[[:space:]]*(pub(\([^)]*\))?[[:space:]]+)?mod[[:space:]]+(r#)?[A-Za-z0-9_]+[[:space:]]*;/) {
        module = line
        sub(/^[[:space:]]*(pub(\([^)]*\))?[[:space:]]+)?mod[[:space:]]+(r#)?/, "", module)
        sub(/[[:space:]]*;.*$/, "", module)
        if (module !~ /(_tests|_props)$/ && module !~ /^model_/ && module != "scripted")
          cause(FILENAME, FNR, "mod " module " under a cfg that names test names a file that is not test support")
        pending = 0
      } else if (line !~ /^[[:space:]]*\/\// && trimmed(line) != "") {
        pending = 0
      }
    }
    END {
      if (!found) check_allowances()
    }
  ' "$@"
}

classify() {
  root=$1
  daemon=$root/crates/halvo-capture/src/daemon
  bin=$root/crates/halvo-capture/src/bin/halvo-capture.rs
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
  if [ -f "$bin" ]; then
    set -- "$@" "$bin"
  fi
  found=$(scan "$daemon" "$@")
  status=$?
  case $status in
    0) return 0 ;;
    1)
      printf '%s' "$found"
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
  tree=$work/$1
  mkdir -p "$tree/crates/halvo-capture/src/daemon" "$tree/crates/halvo-capture/src/bin" || exit 2
  printf '%s\n' \
    '#[cfg(any(test, all(target_os = "macos", debug_assertions)))]' \
    'pub mod owner;' \
    '#[cfg(any(test, all(target_os = "macos", debug_assertions)))]' \
    'pub mod port;' \
    '#[cfg(test)]' \
    'mod model_store;' \
    '#[cfg(test)]' \
    'mod owner_props;' \
    '#[cfg(test)] mod scripted;' \
    '#[cfg(feature = "r2")]' \
    '#[rustfmt::skip]' \
    'mod r2;' \
    'mod report;' >"$tree/crates/halvo-capture/src/daemon/mod.rs" || exit 2
  printf '%s\n' \
    'pub struct Owner;' \
    '#[cfg(test)]' \
    'mod tests {' \
    '    #[test]' \
    '    fn t() {}' \
    '}' >"$tree/crates/halvo-capture/src/daemon/owner.rs" || exit 2
  printf '%s\n' 'expiry-never-runs	maintenance.rs	s/a/b/	test x	required	B14-05' \
    >"$tree/crates/halvo-capture/src/daemon/mutants.rows" || exit 2
  printf 'fn main() {}\n' >"$tree/crates/halvo-capture/src/bin/halvo-capture.rs" || exit 2
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

add_line() {
  printf '%s\n' "$3" >>"$work/$1/crates/halvo-capture/src/$2" || exit 2
}

self_test() {
  rejected=0
  wrong=
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  fixture clean
  case_of clean PASS
  fixture allowance
  add_line allowance daemon/owner.rs '#[allow(dead_code)]'
  case_of allowance FAIL
  fixture expect-dead
  add_line expect-dead daemon/owner.rs '#[expect(dead_code)] fn f() {}'
  case_of expect-dead FAIL
  fixture unused-crate-wide
  add_line unused-crate-wide daemon/owner.rs '#![allow(unused_variables)]'
  case_of unused-crate-wide FAIL
  fixture split-allowance
  add_line split-allowance daemon/owner.rs '#[allow('
  add_line split-allowance daemon/owner.rs '    dead_code'
  add_line split-allowance daemon/owner.rs ')]'
  case_of split-allowance FAIL
  fixture bin-allowance
  add_line bin-allowance bin/halvo-capture.rs '#[allow(unused_imports)]'
  case_of bin-allowance FAIL
  fixture warnings-group
  add_line warnings-group daemon/owner.rs '#[allow(warnings)]'
  add_line warnings-group daemon/owner.rs 'fn unused() {}'
  case_of warnings-group FAIL
  fixture warnings-inner
  add_line warnings-inner daemon/owner.rs '#![allow(warnings)]'
  case_of warnings-inner FAIL
  fixture test-only-state
  add_line test-only-state daemon/mod.rs '#[cfg(test)] mod state;'
  case_of test-only-state FAIL
  fixture test-only-next-line
  add_line test-only-next-line daemon/mod.rs '#[cfg(test)]'
  add_line test-only-next-line daemon/mod.rs 'pub(crate) mod supervisor;'
  case_of test-only-next-line FAIL
  fixture test-and-feature
  add_line test-and-feature daemon/mod.rs '#[cfg(all(test, feature = "r2"))]'
  add_line test-and-feature daemon/mod.rs 'mod seam;'
  case_of test-and-feature FAIL
  fixture any-test
  add_line any-test daemon/mod.rs '#[cfg(any(test))]'
  add_line any-test daemon/mod.rs 'mod state;'
  case_of any-test FAIL
  fixture raw-name
  add_line raw-name daemon/mod.rs '#[cfg(test)]'
  add_line raw-name daemon/mod.rs 'mod r#state;'
  case_of raw-name FAIL
  fixture split-cfg
  add_line split-cfg daemon/mod.rs '#[cfg(all('
  add_line split-cfg daemon/mod.rs '    test,'
  add_line split-cfg daemon/mod.rs '    feature = "r2"'
  add_line split-cfg daemon/mod.rs '))]'
  add_line split-cfg daemon/mod.rs 'mod seam;'
  case_of split-cfg FAIL
  fixture split-platform
  add_line split-platform daemon/owner.rs '#[cfg(all('
  add_line split-platform daemon/owner.rs '    test,'
  add_line split-platform daemon/owner.rs '    target_os = "windows"'
  add_line split-platform daemon/owner.rs '))]'
  add_line split-platform daemon/owner.rs 'fn only_in_windows_tests() {}'
  case_of split-platform FAIL
  fixture test-support-names
  add_line test-support-names daemon/mod.rs '#[cfg(test)]'
  add_line test-support-names daemon/mod.rs 'mod model_clock;'
  add_line test-support-names daemon/mod.rs '#[cfg(all(test, feature = "r2"))]'
  add_line test-support-names daemon/mod.rs 'mod seam_tests;'
  add_line test-support-names daemon/mod.rs '#[cfg(test)]'
  add_line test-support-names daemon/mod.rs 'mod r#raw_tests;'
  case_of test-support-names PASS
  fixture platform-in-owner
  add_line platform-in-owner daemon/owner.rs '#[cfg(any(test, windows))]'
  add_line platform-in-owner daemon/owner.rs 'fn g() {}'
  case_of platform-in-owner FAIL
  fixture second-condition
  add_line second-condition daemon/mod.rs '#[cfg(any(test, target_os = "windows"))]'
  add_line second-condition daemon/mod.rs 'pub mod maintenance;'
  case_of second-condition FAIL
  fixture platform-alone
  add_line platform-alone daemon/owner.rs '#[cfg(target_os = "macos")]'
  add_line platform-alone daemon/owner.rs 'fn free() {}'
  case_of platform-alone PASS
  fixture upper-suffix
  printf '%s\n' '#![allow(dead_code)]' 'fn reach() {}' >"$work/upper-suffix/crates/halvo-capture/src/daemon/evil.RS" || exit 2
  case_of upper-suffix FAIL
  fixture no-suffix
  printf '%s\n' '#![allow(dead_code)]' >"$work/no-suffix/crates/halvo-capture/src/daemon/evil" || exit 2
  case_of no-suffix FAIL
  fixture linked
  mkdir -p "$work/linked/outside" || exit 2
  printf '%s\n' '#![allow(dead_code)]' >"$work/linked/outside/linked.txt" || exit 2
  ln -s ../../../../outside/linked.txt "$work/linked/crates/halvo-capture/src/daemon/linked_tests.rs" || exit 2
  if [ -L "$work/linked/crates/halvo-capture/src/daemon/linked_tests.rs" ]; then made='link'; else made='copy'; fi
  printf 'case %s/linked: ln -s made a %s\n' "$name" "$made"
  case_of linked FAIL
  fixture data-file-holds-any-text
  printf '%s\n' '#![allow(dead_code)]' >"$work/data-file-holds-any-text/crates/halvo-capture/src/daemon/mutants.rows" || exit 2
  case_of data-file-holds-any-text PASS
  fixture data-file-renamed
  mv "$work/data-file-renamed/crates/halvo-capture/src/daemon/mutants.rows" "$work/data-file-renamed/crates/halvo-capture/src/daemon/Mutants.rows" || exit 2
  printf '%s\n' '#![allow(dead_code)]' >>"$work/data-file-renamed/crates/halvo-capture/src/daemon/Mutants.rows" || exit 2
  case_of data-file-renamed FAIL
  fixture unlisted-dir
  locked=$work/unlisted-dir/crates/halvo-capture/src/daemon/locked
  mkdir -p "$locked" || exit 2
  printf '%s\n' '#![allow(dead_code)]' >"$locked/mod.rs" || exit 2
  trap 'chmod 755 "$locked" 2>/dev/null' EXIT INT TERM HUP
  chmod 311 "$locked" || exit 2
  if [ "$(id -u)" = 0 ]; then
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
  mkdir -p "$work/data-alone/crates/halvo-capture/src/daemon" || exit 2
  printf 'x\n' >"$work/data-alone/crates/halvo-capture/src/daemon/mutants.rows" || exit 2
  case_of data-alone FAIL
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
