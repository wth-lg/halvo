#!/bin/sh
# jobs: macos windows
# rule: No file under crates/halvo-capture/src/daemon/, and not crates/halvo-capture/src/bin/halvo-capture.rs, holds an allow or an expect of dead_code or of an unused lint, no mod line under a test-only cfg in src/daemon/ names a file that is not test support (*_tests.rs, *_props.rs, model_*.rs, scripted.rs; an inline mod tests is no file), no cfg under src/daemon/ joins test with a platform but the one condition of daemon/mod.rs, and src/daemon/ holds a .rs file.
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
      gsub(/^[[:space:]]+|[[:space:]]+$/, "", text)
      return text
    }
    function check_allowances() {
      if (file != "" && joined ~ /(allow|expect)[[:space:]]*\([^)]*(dead_code|unused)/)
        cause(file, "-", "an allow or an expect of dead_code or of an unused lint")
    }
    FNR == 1 {
      check_allowances()
      file = FILENAME
      joined = ""
      pending = 0
    }
    {
      joined = joined " " $0
      line = $0
    }
    FILENAME ~ /\/src\/bin\// { next }
    line ~ /cfg[[:space:]]*\(/ && line ~ /[^a-z_]test[^a-z_]/ &&
      line ~ /(target_os|target_family|target_arch|target_env|target_vendor|[^a-z_]windows[^a-z_]|[^a-z_]unix[^a-z_])/ {
      if (FILENAME != mod_rs) cause(FILENAME, FNR, "a cfg joins test with a platform outside daemon/mod.rs")
      if (condition == "") condition = trimmed(line)
      else if (condition != trimmed(line)) cause(FILENAME, FNR, "daemon/mod.rs joins test with a platform in a second condition")
    }
    line ~ /#\[[[:space:]]*cfg[[:space:]]*\(/ {
      pending = (line ~ /cfg[[:space:]]*\([[:space:]]*test[[:space:]]*\)/ ||
        line ~ /cfg[[:space:]]*\([[:space:]]*all[[:space:]]*\(([^)]*[^a-z_])?test[^a-z_]/)
      if (!pending) next
      rest = line
      sub(/^.*\][[:space:]]*/, "", rest)
      if (rest == "") next
      line = rest
    }
    pending {
      if (line ~ /^[[:space:]]*(pub(\([^)]*\))?[[:space:]]+)?mod[[:space:]]+[A-Za-z0-9_]+[[:space:]]*;/) {
        module = line
        sub(/^[[:space:]]*(pub(\([^)]*\))?[[:space:]]+)?mod[[:space:]]+/, "", module)
        sub(/[[:space:]]*;.*$/, "", module)
        if (module !~ /(_tests|_props)$/ && module !~ /^model_/ && module != "scripted")
          cause(FILENAME, FNR, "mod " module " under a test-only cfg names a file that is not test support")
        pending = 0
      } else if (line !~ /^[[:space:]]*(#|\/\/)/ && trimmed(line) != "") {
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
  fixture test-support-names
  add_line test-support-names daemon/mod.rs '#[cfg(test)]'
  add_line test-support-names daemon/mod.rs 'mod model_clock;'
  add_line test-support-names daemon/mod.rs '#[cfg(all(test, feature = "r2"))]'
  add_line test-support-names daemon/mod.rs 'mod seam_tests;'
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
