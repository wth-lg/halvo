#!/bin/sh
# jobs: macos windows
# rule: Every file under crates/halvo-capture/src/daemon/ but the data file mutants.rows, whatever its name and through a link, and crates/halvo-capture/src/bin/halvo-capture.rs hold no allow or expect of dead_code, of an unused lint or of warnings; no mod line under a cfg that names test, but the one platform condition of daemon/mod.rs, names a file that is not test support (*_tests.rs, *_props.rs, model_*.rs, scripted.rs; an inline mod tests is no file), also written r#name; no cfg under src/daemon/ joins test with a platform but that one condition; an attribute is read whole over its lines; every file under src/daemon/ but the data file is named by exactly one plain mod line in the file where the compiler looks for it (daemon/mod.rs by lib.rs), and the token mod stands in no macro definition and no arguments of a macro call under src/daemon/ or in the bin file; a cfg_attr that names test counts as a cfg that names test; a listing that fails fails the check with its path; src/daemon/ holds a file to read; and no allow or expect of dead_code, of an unused lint or of warnings covers the daemon from outside its files (ruling DM-2): no attribute of the whole crate in crates/halvo-capture/src/lib.rs and no attribute of its item mod daemon, also inside cfg_attr; no lints table of the crate's or the workspace's Cargo.toml that sets dead_code, an unused lint, unused or warnings to allow; no rustflags key of .cargo/config.toml at the root or under crates/halvo-capture, and no line that sets a rustflags variable of cargo in .github/workflows/ci.yml, scripts/ci/ or scripts/r1/ci.d/, that allows those lints or caps the lints; and no .cargo/config with no ending at either place.
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
      if (text !~ /cfg(_attr)?[[:space:]]*\(/) return
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

# Reads the daemon's files ($2...) and the bin file, and fails when the token
# mod stands in a macro definition or in the arguments of a macro call, or when
# a file under the daemon's directory ($1) is named by no plain mod line or by
# more than one. $lib_names_daemon says whether lib.rs names the daemon.
names() {
  awk -v daemon="$1/" -v lib_names_daemon="$lib_names_daemon" '
    function rel(path) {
      sub(/^.*\/crates\/halvo-capture\//, "crates/halvo-capture/", path)
      return path
    }
    function fail(text) {
      print text
      failed = 1
      exit 1
    }
    function macro_holds_mod(text,    rest, off, i, c, depth, instr, esc, span, n) {
      rest = text
      off = 0
      n = length(text)
      while (match(rest, /[A-Za-z_][A-Za-z0-9_]*[ \t\n]*![ \t\n]*([A-Za-z_][A-Za-z0-9_]*[ \t\n]*)?[({[]/)) {
        depth = 1
        instr = 0
        esc = 0
        span = ""
        for (i = off + RSTART + RLENGTH; i <= n && depth > 0; i++) {
          c = substr(text, i, 1)
          if (instr) {
            if (esc) esc = 0
            else if (c == "\\") esc = 1
            else if (c == "\"") instr = 0
            continue
          }
          if (c == "\"") { instr = 1; span = span " "; continue }
          if (c == "(" || c == "[" || c == "{") depth++
          else if (c == ")" || c == "]" || c == "}") depth--
          span = span c
        }
        if (span ~ /(^|[^A-Za-z0-9_])mod([^A-Za-z0-9_]|$)/) return 1
        off = off + RSTART + RLENGTH - 1
        rest = substr(text, off + 1)
      }
      return 0
    }
    function finish(    dir) {
      if (file == "") return
      if (macro_holds_mod(body)) fail(rel(file) ": the token mod stands in a macro definition or in the arguments of a macro call")
    }
    FNR == 1 {
      finish()
      file = FILENAME
      body = ""
      if (index(file, daemon) == 1) daemon_files[file] = 1
    }
    { body = body "\n" $0 }
    index(FILENAME, daemon) == 1 {
      line = $0
      while (line ~ /^[[:space:]]*#!?\[[^]]*\][[:space:]]*/) sub(/^[[:space:]]*#!?\[[^]]*\][[:space:]]*/, "", line)
      if (line ~ /^(pub(\([^)]*\))?[[:space:]]+)?mod[[:space:]]+(r#)?[A-Za-z0-9_]+[[:space:]]*;/) {
        name = line
        sub(/^(pub(\([^)]*\))?[[:space:]]+)?mod[[:space:]]+(r#)?/, "", name)
        sub(/[[:space:]]*;.*$/, "", name)
        dir = FILENAME
        if (dir ~ /\/mod\.rs$/) sub(/\/mod\.rs$/, "", dir)
        else sub(/\.[^.\/]*$/, "", dir)
        named[dir "/" name ".rs"]++
        named[dir "/" name "/mod.rs"]++
      }
    }
    END {
      if (failed) exit 1
      finish()
      for (f in daemon_files) {
        if (f == daemon "mod.rs") count = (lib_names_daemon == "yes") ? 1 : 0
        else count = named[f] + 0
        if (count == 0) fail(rel(f) ": no plain mod line names this file")
        if (count > 1) fail(rel(f) ": " count " plain mod lines name this file")
      }
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
  lib_names_daemon=no
  if grep -q -E '^[[:space:]]*(pub[[:space:]]+)?mod[[:space:]]+daemon[[:space:]]*;' "$root/crates/halvo-capture/src/lib.rs" 2>/dev/null; then
    lib_names_daemon=yes
  fi
  found=$(names "$daemon" "$@")
  status=$?
  if [ "$status" != 0 ]; then
    printf '%s' "${found:-awk exited $status}"
    return 1
  fi
  found=$(scan "$daemon" "$@")
  status=$?
  case $status in
    0)
      outside "$root"
      return
      ;;
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

# Ruling DM-2: an allowance of dead_code, of an unused lint or of warnings that
# covers the daemon's code from outside its files. Reads lib.rs, the crate's and
# the workspace's manifest, and the closed list of files that set the
# compiler's flags under the root $1; prints the first cause and returns 1.
outside() {
  root=$1
  lib=$root/crates/halvo-capture/src/lib.rs
  if [ "$lib_names_daemon" != yes ]; then
    printf 'crates/halvo-capture/src/lib.rs declares no mod daemon, so the files between the crate root and the daemon are not known to this check'
    return 1
  fi
  found=$(tr -d '\r' <"$lib" | awk '
    { text = text " " $0 }
    END {
      allowance = "(allow|expect)[[:space:]]*[(][^)]*(dead_code|unused|warnings)"
      rest = text
      while (match(rest, /#![[:space:]]*[[][^]]*[]]/)) {
        if (substr(rest, RSTART, RLENGTH) ~ allowance) {
          print "an attribute of the whole crate allows or expects dead_code, an unused lint or warnings"
          exit 1
        }
        rest = substr(rest, RSTART + RLENGTH)
      }
      if (match(text, /(pub[[:space:]]*([(][^)]*[)])?[[:space:]]+)?mod[[:space:]]+daemon[[:space:]]*[;{]/)) {
        n = split(substr(text, 1, RSTART - 1), part, "[;}]")
        if (part[n] ~ allowance) {
          print "the item mod daemon carries an allow or an expect of dead_code, of an unused lint or of warnings"
          exit 1
        }
      }
    }')
  status=$?
  if [ "$status" != 0 ]; then
    printf 'crates/halvo-capture/src/lib.rs: %s' "$found"
    return 1
  fi
  for manifest in Cargo.toml crates/halvo-capture/Cargo.toml; do
    [ -f "$root/$manifest" ] || continue
    found=$(tr -d '\r' <"$root/$manifest" | awk '
      function bare(text) {
        gsub(/["'"'"'[:space:]]/, "", text)
        return text
      }
      /^[[:space:]]*[[]/ {
        table = $0
        sub(/^[[:space:]]*[[]+/, "", table)
        sub(/[]].*$/, "", table)
        table = bare(table)
        next
      }
      /=/ {
        key = $0
        sub(/=.*$/, "", key)
        key = bare(key)
        value = $0
        sub(/^[^=]*=/, "", value)
        path = (table == "") ? key : table "." key
        lint = "(dead_code|unused[a-z_]*|warnings)"
        if (path ~ ("^(workspace[.])?lints[.]rust[.]" lint "$") && value ~ /allow/) bad = 1
        if (path ~ /^(workspace[.])?lints([.]rust)?$/ && value ~ (lint "[\"'"'"'[:space:]]*=[[:space:]]*([\"'"'"']allow|[{][^}]*level[[:space:]]*=[[:space:]]*[\"'"'"']allow)")) bad = 1
        if (bad) {
          print "a lints table sets dead_code, an unused lint or warnings to allow at " path
          exit 1
        }
      }')
    status=$?
    if [ "$status" != 0 ]; then
      printf '%s: %s' "$manifest" "$found"
      return 1
    fi
  done
  for config in .cargo crates/halvo-capture/.cargo; do
    if [ -e "$root/$config/config" ] || [ -L "$root/$config/config" ]; then
      printf '%s/config: a cargo config file under its older name, which this check does not read' "$config"
      return 1
    fi
    [ -f "$root/$config/config.toml" ] || continue
    found=$(tr -d '\r' <"$root/$config/config.toml" | awk '
      { text = text "\n" $0 }
      END {
        rest = text
        while (match(rest, /rustflags["'"'"'[:space:]]*=[[:space:]]*/)) {
          rest = substr(rest, RSTART + RLENGTH)
          value = rest
          if (substr(value, 1, 1) == "[") sub(/[]].*$/, "", value)
          else sub(/\n.*$/, "", value)
          if (value ~ /(-A|--allow)([[:space:]"'"'"',=]|\\x1f|\\037)*(dead_code|unused[a-z_]*|warnings)|--cap-lints/) {
            print "a rustflags key allows dead_code, an unused lint or warnings, or caps the lints"
            exit 1
          }
        }
      }')
    status=$?
    if [ "$status" != 0 ]; then
      printf '%s/config.toml: %s' "$config" "$found"
      return 1
    fi
  done
  set --
  [ -f "$root/.github/workflows/ci.yml" ] && set -- "$@" "$root/.github/workflows/ci.yml"
  for dir in scripts/ci scripts/r1/ci.d; do
    [ -d "$root/$dir" ] || continue
    listed=$(find "$root/$dir" ! -type d 2>&1)
    status=$?
    if [ "$status" != 0 ]; then
      printf 'the listing of %s failed' "$dir"
      return 1
    fi
    while IFS= read -r file; do
      [ -n "$file" ] && set -- "$@" "$file"
    done <<EOF
$listed
EOF
  done
  [ "$#" -gt 0 ] || return 0
  found=$(awk -v root="$root/" '
    {
      line = $0
      sub(/\r$/, "", line)
      if (line ~ /(^|[^A-Za-z0-9_])(RUSTFLAGS|CARGO_BUILD_RUSTFLAGS|CARGO_ENCODED_RUSTFLAGS|CARGO_TARGET_[A-Z0-9_]+_RUSTFLAGS)[[:space:]]*[:=]/ &&
        line ~ /(-A|--allow)([[:space:]"'"'"',=]|\\x1f|\\037)*(dead_code|unused[a-z_]*|warnings)|--cap-lints/) {
        file = FILENAME
        if (index(file, root) == 1) file = substr(file, length(root) + 1)
        print file ":" FNR ": a line sets the compiler flags so that dead_code, an unused lint or warnings are allowed, or the lints are capped"
        exit 1
      }
    }' "$@")
  status=$?
  if [ "$status" != 0 ]; then
    printf '%s' "$found"
    return 1
  fi
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
  printf '%s\n' 'pub mod daemon;' >"$tree/crates/halvo-capture/src/lib.rs" || exit 2
}

state_file() {
  printf '%s\n' 'pub fn helper() {}' >"$work/$1/crates/halvo-capture/src/daemon/state.rs" || exit 2
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

# Writes the lines after $2 as the file $2 of the fixture tree $1.
put() {
  file=$work/$1/$2
  shift 2
  mkdir -p "${file%/*}" || exit 2
  printf '%s\n' "$@" >"$file" || exit 2
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
  fixture macro-declares-a-module
  add_line macro-declares-a-module daemon/mod.rs 'macro_rules! test_only {'
  add_line macro-declares-a-module daemon/mod.rs "    (\$name:ident) => {"
  add_line macro-declares-a-module daemon/mod.rs '        #[cfg(test)]'
  add_line macro-declares-a-module daemon/mod.rs "        mod \$name;"
  add_line macro-declares-a-module daemon/mod.rs '    };'
  add_line macro-declares-a-module daemon/mod.rs '}'
  add_line macro-declares-a-module daemon/mod.rs 'test_only!(state);'
  state_file macro-declares-a-module
  case_of macro-declares-a-module FAIL
  fixture macro-gates-a-module
  add_line macro-gates-a-module daemon/mod.rs 'macro_rules! gate {'
  add_line macro-gates-a-module daemon/mod.rs "    (\$(\$item:item)*) => { \$( #[cfg(test)] \$item )* };"
  add_line macro-gates-a-module daemon/mod.rs '}'
  add_line macro-gates-a-module daemon/mod.rs 'gate! { mod state; }'
  state_file macro-gates-a-module
  case_of macro-gates-a-module FAIL
  fixture outside-macro-with-the-name
  add_line outside-macro-with-the-name lib.rs "macro_rules! test_only { (\$name:ident) => { #[cfg(test)] mod \$name; }; }"
  add_line outside-macro-with-the-name daemon/mod.rs 'test_only!(state);'
  state_file outside-macro-with-the-name
  case_of outside-macro-with-the-name FAIL
  fixture outside-macro-gates-a-module
  add_line outside-macro-gates-a-module lib.rs "macro_rules! gate { (\$(\$item:item)*) => { \$( #[cfg(test)] \$item )* }; }"
  add_line outside-macro-gates-a-module daemon/mod.rs 'gate! { mod state; }'
  state_file outside-macro-gates-a-module
  case_of outside-macro-gates-a-module FAIL
  fixture outside-macro-with-no-argument
  add_line outside-macro-with-no-argument lib.rs 'macro_rules! declare_state { () => { #[cfg(test)] mod state; }; }'
  add_line outside-macro-with-no-argument daemon/mod.rs 'declare_state!();'
  state_file outside-macro-with-no-argument
  case_of outside-macro-with-no-argument FAIL
  fixture cfg-attr-test
  add_line cfg-attr-test daemon/mod.rs '#[cfg_attr(not(test), cfg(any()))]'
  add_line cfg-attr-test daemon/mod.rs 'mod state;'
  state_file cfg-attr-test
  case_of cfg-attr-test FAIL
  fixture file-no-mod-line-names
  printf '%s\n' 'pub fn orphan() {}' >"$work/file-no-mod-line-names/crates/halvo-capture/src/daemon/orphan.rs" || exit 2
  case_of file-no-mod-line-names FAIL
  fixture file-named-twice
  add_line file-named-twice daemon/mod.rs 'pub mod owner;'
  case_of file-named-twice FAIL
  fixture product-module-named
  add_line product-module-named daemon/mod.rs 'mod state;'
  state_file product-module-named
  case_of product-module-named PASS
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
  add_line linked daemon/mod.rs '#[cfg(test)]'
  add_line linked daemon/mod.rs 'mod linked_tests;'
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
  fixture parent-allowance
  put parent-allowance crates/halvo-capture/src/lib.rs '#[allow(dead_code)]' 'pub mod daemon;'
  case_of parent-allowance FAIL
  fixture parent-cfg-attr
  put parent-cfg-attr crates/halvo-capture/src/lib.rs '#[cfg_attr(test, allow(unused_imports))]' 'pub mod daemon;'
  case_of parent-cfg-attr FAIL
  fixture parent-split-allowance
  put parent-split-allowance crates/halvo-capture/src/lib.rs '#[allow(' '    unused,' ')]' 'pub mod daemon;'
  case_of parent-split-allowance FAIL
  fixture crate-allowance
  put crate-allowance crates/halvo-capture/src/lib.rs '#![allow(dead_code)]' 'pub mod daemon;'
  case_of crate-allowance FAIL
  fixture crate-cfg-attr-expect
  put crate-cfg-attr-expect crates/halvo-capture/src/lib.rs '#![cfg_attr(debug_assertions, expect(warnings))]' 'pub mod daemon;'
  case_of crate-cfg-attr-expect FAIL
  fixture other-module-allowance
  put other-module-allowance crates/halvo-capture/src/lib.rs '#[allow(dead_code)]' 'mod other;' 'pub mod daemon;'
  case_of other-module-allowance PASS
  fixture crate-lints-table
  put crate-lints-table crates/halvo-capture/Cargo.toml '[lints.rust]' 'dead_code = "allow"'
  case_of crate-lints-table FAIL
  fixture workspace-lints-level
  put workspace-lints-level Cargo.toml '[workspace.lints.rust]' 'unused_imports = { level = "allow", priority = -1 }'
  case_of workspace-lints-level FAIL
  fixture lints-inline-table
  put lints-inline-table Cargo.toml '[lints]' 'rust = { warnings = "allow" }'
  case_of lints-inline-table FAIL
  fixture lints-dotted-key
  put lints-dotted-key crates/halvo-capture/Cargo.toml 'lints.rust.dead_code = "allow"' '[package]' 'name = "x"'
  case_of lints-dotted-key FAIL
  fixture lints-that-deny
  put lints-that-deny Cargo.toml '[workspace.lints.rust]' 'warnings = "deny"' 'unused = { level = "warn", priority = -1 }'
  put lints-that-deny crates/halvo-capture/Cargo.toml '[lints]' 'workspace = true' '[lints.rust]' 'dead_code = "forbid"'
  put lints-that-deny .cargo/config.toml '[build]' 'rustflags = ["-D", "warnings"]'
  case_of lints-that-deny PASS
  fixture config-allow
  put config-allow .cargo/config.toml '[build]' 'rustflags = ["-A", "dead_code"]'
  case_of config-allow FAIL
  fixture config-cap-lints
  put config-cap-lints crates/halvo-capture/.cargo/config.toml '[target.x86_64-pc-windows-msvc]' 'rustflags = ["--cap-lints", "warn"]'
  case_of config-cap-lints FAIL
  fixture config-older-name
  put config-older-name .cargo/config '[build]'
  case_of config-older-name FAIL
  fixture workflow-flags
  mkdir -p "$work/workflow-flags/.github/workflows" || exit 2
  printf '%s%s\n' '      RUSTF' 'LAGS: "-A warnings"' >"$work/workflow-flags/.github/workflows/ci.yml" || exit 2
  case_of workflow-flags FAIL
  fixture script-encoded-flags
  mkdir -p "$work/script-encoded-flags/scripts/r1/ci.d" || exit 2
  printf '%s%s\n' 'export CARGO_ENCODED_RUSTF' 'LAGS="-A\x1fdead_code"' >"$work/script-encoded-flags/scripts/r1/ci.d/40-x.sh" || exit 2
  case_of script-encoded-flags FAIL
  fixture script-target-flags
  mkdir -p "$work/script-target-flags/scripts/ci" || exit 2
  printf '%s%s\n' 'CARGO_TARGET_AARCH64_APPLE_DARWIN_RUSTF' "LAGS='-Aunused_variables' cargo build" >"$work/script-target-flags/scripts/ci/build.sh" || exit 2
  case_of script-target-flags FAIL
  fixture unlisted-dir
  locked=$work/unlisted-dir/crates/halvo-capture/src/daemon/locked
  mkdir -p "$locked" || exit 2
  printf '%s\n' '#![allow(dead_code)]' >"$locked/mod.rs" || exit 2
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

usage() {
  cat <<'EOF'
usage: sh scripts/r1/ci.d/22-daemon-no-dead-allow.sh [--self-test | --help]
Start it at the root of the repository, or run it through
sh scripts/ci/run-ci-d.sh <job> 22-daemon-no-dead-allow.
With no argument it reads every file under crates/halvo-capture/src/daemon/
and crates/halvo-capture/src/bin/halvo-capture.rs as text, and then
crates/halvo-capture/src/lib.rs, the crate's and the workspace's Cargo.toml,
.cargo/config.toml at the root and under crates/halvo-capture,
.github/workflows/ci.yml and the files of scripts/ci/ and scripts/r1/ci.d/
(ruling DM-2), and prints PASS, or FAIL with the file and the cause.
--self-test runs it over fixture trees that must pass or fail. The rule that
it holds is the line "# rule:" of this file, which sh scripts/ci/run-ci-d.sh
--rules prints.

Limits. This check reads text and is the second layer. The compiler holds the
boundary (ruling DM-7): the store's database type cannot be named outside
crate::store, and 30-core-boundary-proofs fails when the lib's test build
reads a file of the daemon that its product build does not read and that is
no test support, whatever spelling declares it. This check refuses the
listed forms of a module of the daemon alone; this form, for one, passes it:
- a line mod state; inside a block comment of daemon/mod.rs, which the check
  counts as the plain mod line that names daemon/state.rs. With it, a macro
  defined in lib.rs that declares the module under cfg(test), called in
  daemon/mod.rs with no argument, builds the module in the test build alone,
  and 30-core-boundary-proofs refuses it.
A flag that a person sets in the shell of their own machine, a cargo config
file above the repository, and a flag that a script builds from parts are
outside what it reads.
EOF
}

case ${1:-} in
  --self-test) self_test ;;
  -h | --help) usage ;;
  '') measure ;;
  *) exit 64 ;;
esac
