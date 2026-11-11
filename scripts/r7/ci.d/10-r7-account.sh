#!/bin/sh
# jobs: macos windows
# rule: Every vendored file of account/schema/ has its recorded sha256, no cargo configuration file sits at the repository root or under account/ and no symbolic link under account/, account/deny.toml is the configuration that this script admits with at least the 27 bans of unit 7.1, and, offline with the cargo-deny version and the packages that scripts/prerequisites.sh installs and fetches, cargo's metadata of account/ holds no package from a source outside crates.io, no local path outside account/ and no package named with sqlite or sqlcipher, and cargo deny check bans licenses sources, build, test, fmt --check and clippy pass for account/.
set -u
name=10-r7-account
case $0 in
  /*) self=$0 ;;
  *) self=$(pwd)/$0 ;;
esac
nl='
'
tab=$(printf '\t')
files='schema.v1.json contract/schema/v1.json
routes.v1.json contract/routes/v1.json
samples.v1.json contract/samples/v1.json
prompt-budget.json gate/prompt-budget.json'
required_bans='rusqlite libsqlite3-sys sqlite sqlite3-sys sqlite3-src sqlx-sqlite libsql libsql-sys libsql-ffi limbo limbo_core turso turso_core sentry sentry-core sentry-types sentry-rust-minidump tauri-plugin-sentry bugsnag rollbar honeybadger airbrake posthog-rs segment aptabase tauri-plugin-aptabase opentelemetry-otlp'
crates_io='registry+https://github.com/rust-lang/crates.io-index'

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

enter_repository() {
  if [ -z "${CI_D_REPO:-}" ] || ! cd "$CI_D_REPO"; then
    fail 'cannot enter the repository that CI_D_REPO names'
  fi
  repo=$(pwd)
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sum=$(sha256sum <"$1") || return 1
  else
    sum=$(shasum -a 256 <"$1") || return 1
  fi
  printf '%s\n' "${sum%% *}"
}

without_cr() {
  tr -d '\r' <"$1" >"$work/$2" || fail "$1 cannot be read"
}

lower_hex() {
  [ "${#1}" -eq "$2" ] || return 1
  case $1 in
    *[!0-9a-f]*) return 1 ;;
  esac
}

check_vendored() {
  record=account/schema/vendored.txt
  [ -f "$record" ] || fail 'account/schema/vendored.txt is missing'
  without_cr "$record" record.txt
  exec 3<"$work/record.txt"
  IFS= read -r first <&3 || first=
  case $first in
    'commit '*) commit=${first#'commit '} ;;
    *) commit= ;;
  esac
  lower_hex "$commit" 40 || fail 'account/schema/vendored.txt does not start with the line of its commit'
  compared=0
  while read -r want_name want_path; do
    IFS= read -r line <&3 || line=
    recorded=${line%% *}
    if ! lower_hex "$recorded" 64 || [ "$line" != "$recorded $want_name $want_path" ]; then
      fail 'account/schema/vendored.txt does not name the four vendored files'
    fi
    actual=$(sha256_of "account/schema/$want_name") || fail "account/schema/$want_name cannot be read"
    [ "$actual" = "$recorded" ] || fail "account/schema/$want_name differs from its recorded sha256"
    compared=$((compared + 1))
  done <<EOF
$files
EOF
  if IFS= read -r line <&3 || [ -n "$line" ]; then
    fail 'account/schema/vendored.txt does not name the four vendored files'
  fi
  exec 3<&-
}

is_cargo_config_name() {
  case $1 in
    [cC][oO][nN][fF][iI][gG] | [cC][oO][nN][fF][iI][gG].[tT][oO][mM][lL]) return 0 ;;
  esac
  return 1
}

check_config_files() {
  {
    find . -maxdepth 1 -iname .cargo
    find account -path account/target -prune -o -iname .cargo -print
  } >"$work/cargo-dirs.txt" 2>/dev/null
  while IFS= read -r dir || [ -n "$dir" ]; do
    [ -d "$dir" ] || continue
    for entry in "$dir"/*; do
      if is_cargo_config_name "${entry##*/}"; then
        fail 'a cargo configuration file sits at the repository root or under account/'
      fi
    done
  done <"$work/cargo-dirs.txt"
}

check_links() {
  find account -path account/target -prune -o -type l -print >"$work/links.txt" 2>/dev/null
  [ ! -s "$work/links.txt" ] || fail 'account/ holds a symbolic link'
}

admitted_deny_config() {
  cat <<'EOF'
[graph]
all-features = true

[licenses]
allow = [
    "MIT",
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "Zlib",
    "Unicode-3.0",
]
confidence-threshold = 0.93
unused-allowed-license = "allow"

[licenses.private]
ignore = true

[bans]
multiple-versions = "allow"
deny = [
]

[sources]
unknown-registry = "deny"
unknown-git = "deny"
allow-registry = ["https://github.com/rust-lang/crates.io-index"]
allow-git = []
EOF
}

check_deny_config() {
  [ -f account/deny.toml ] || fail 'account/deny.toml is missing'
  bans=0
  banned=' '
  : >"$work/deny.kept" || fail 'cannot write in the temp directory'
  without_cr account/deny.toml deny.txt
  while IFS= read -r line || [ -n "$line" ]; do
    case $line in
      '    { crate = "'*)
        rest=${line#'    { crate = "'}
        crate=${rest%%\"*}
        rest=${rest#"$crate"}
        case $crate in
          '' | *[!a-z0-9_-]*) fail 'account/deny.toml holds a ban that this script does not admit' ;;
        esac
        case $rest in
          '", reason = "'*'" },') reason=${rest#'", reason = "'} ;;
          *) fail 'account/deny.toml holds a ban that this script does not admit' ;;
        esac
        reason=${reason%'" },'}
        case $reason in
          '' | *\"* | *\\*) fail 'account/deny.toml holds a ban that this script does not admit' ;;
        esac
        bans=$((bans + 1))
        banned="$banned$crate "
        ;;
      *) printf '%s\n' "$line" >>"$work/deny.kept" ;;
    esac
  done <"$work/deny.txt"
  admitted_deny_config >"$work/deny.admitted" || fail 'cannot write in the temp directory'
  cmp -s "$work/deny.kept" "$work/deny.admitted" || fail 'account/deny.toml differs from the configuration that this script admits'
  for crate in $required_bans; do
    case $banned in
      *" $crate "*) ;;
      *) fail "account/deny.toml lacks the ban of $crate" ;;
    esac
  done
}

deny_pin() {
  pin=
  without_cr scripts/prerequisites.sh prerequisites.txt
  while IFS= read -r line || [ -n "$line" ]; do
    case $line in
      cargo_deny_version=*) pin=${line#cargo_deny_version=} ;;
    esac
  done <"$work/prerequisites.txt"
  case $pin in
    '' | *[!0-9.]*) fail 'scripts/prerequisites.sh pins no cargo-deny version' ;;
  esac
}

ensure_deny() {
  deny_pin
  have=$(cargo deny --version 2>/dev/null)
  [ "$have" = "cargo-deny $pin" ] || fail "cargo deny --version is not cargo-deny $pin; sh scripts/prerequisites.sh installs it"
  tool=found
  printf 'cargo-deny %s found\n' "$pin"
}

check_fetched() {
  cargo fetch --locked --offline --manifest-path account/Cargo.toml >"$work/fetch.txt" 2>&1
  code=$?
  [ "$code" -eq 0 ] || fail "cargo fetch --locked --offline exited $code; sh scripts/prerequisites.sh fetches the packages of account/Cargo.lock"
}

json_pairs() {
  awk '
function opened() {
  if (d == 1) where[d] = "root"
  else if (type[d - 1] == "o") where[d] = where[d - 1] "." key[d - 1]
  else where[d] = where[d - 1] "[]"
  if (where[d] == "root.packages[]") {
    package = ""; source = ""; null_source = 0; manifest = ""; paths = 0
  }
}
function closed(   j) {
  if (where[d] != "root.packages[]") return
  print "name\t" package
  if (null_source) {
    print "local\t" manifest
    print "path\t" manifest
    for (j = 1; j <= paths; j++) print "path\t" path_of[j]
  } else {
    print "source\t" source
  }
}
function value(v, is_string,   k, w) {
  k = key[d]; w = where[d]
  if (w == "root.packages[]") {
    if (k == "name" && is_string) package = v
    else if (k == "source") { if (is_string) source = v; else null_source = 1 }
    else if (k == "manifest_path" && is_string) manifest = v
  } else if (w == "root.packages[].targets[]" && k == "src_path" && is_string) {
    path_of[++paths] = v
  }
}
BEGIN {
  while ((getline line) > 0) text = text line "\n"
  n = length(text); i = 1; d = 0; in_string = 0
  while (i <= n) {
    c = substr(text, i, 1)
    if (in_string) {
      if (c == "\\") {
        e = substr(text, i + 1, 1)
        if (e == "u") { s = s "\\u" substr(text, i + 2, 4); i += 6; continue }
        if (e == "\"" || e == "\\" || e == "/") s = s e
        else s = s "\\" e
        i += 2
        continue
      }
      if (c == "\"") {
        in_string = 0; i++
        if (type[d] == "o" && want_key[d]) { key[d] = s; want_key[d] = 0 }
        else value(s, 1)
        continue
      }
      s = s c; i++
      continue
    }
    if (c == "\"") { in_string = 1; s = ""; i++; continue }
    if (c == "{" || c == "[") {
      d++; type[d] = (c == "{") ? "o" : "a"; key[d] = ""; want_key[d] = (c == "{"); opened(); i++
      continue
    }
    if (c == "}" || c == "]") { closed(); d--; i++; continue }
    if (c == ",") { if (type[d] == "o") want_key[d] = 1; i++; continue }
    if (c == ":" || c == " " || c == "\t" || c == "\n" || c == "\r") { i++; continue }
    literal = ""
    while (i <= n) {
      c = substr(text, i, 1)
      if (c == "," || c == "}" || c == "]" || c == " " || c == "\n") break
      literal = literal c; i++
    }
    value(literal, 0)
  }
}'
}

inside_account() {
  place=$1
  case $place in
    [A-Za-z]:*) if command -v cygpath >/dev/null 2>&1; then place=$(cygpath -u "$place") || return 1; fi ;;
  esac
  if [ -d "$place" ]; then
    resolved=$(cd "$place" 2>/dev/null && pwd -P) || return 1
  else
    parent=$(dirname "$place")
    resolved=$(cd "$parent" 2>/dev/null && pwd -P) || return 1
    resolved=$resolved/${place##*/}
  fi
  case $resolved/ in
    "$account_dir"/*) return 0 ;;
  esac
  return 1
}

check_metadata() {
  cargo metadata --locked --offline --format-version 1 --manifest-path account/Cargo.toml >"$work/metadata.json" 2>"$work/metadata.err"
  code=$?
  [ "$code" -eq 0 ] || fail "cargo metadata --locked --offline exited $code"
  json_pairs <"$work/metadata.json" >"$work/metadata.pairs" || fail 'cargo metadata printed nothing that this script reads'
  account_dir=$(cd account && pwd -P) || fail 'cannot enter account/'
  packages=0
  while IFS="$tab" read -r kind value || [ -n "$kind" ]; do
    case $kind in
      name)
        case $value in
          *[sS][qQ][lL][iI][tT][eE]* | *[sS][qQ][lL][cC][iI][pP][hH][eE][rR]*) fail "account/'s metadata holds a package whose name contains sqlite or sqlcipher" ;;
        esac
        ;;
      local) packages=$((packages + 1)) ;;
      source)
        packages=$((packages + 1))
        [ "$value" = "$crates_io" ] || fail "account/'s metadata holds a package from a source outside crates.io"
        ;;
      path) inside_account "$value" || fail "account/'s metadata holds a path outside account/" ;;
    esac
  done <"$work/metadata.pairs"
  [ "$packages" -gt 0 ] || fail "account/'s metadata holds no package"
}

count_tests() {
  tests=0
  without_cr "$1" test-log.txt
  while IFS= read -r line || [ -n "$line" ]; do
    case $line in
      'test result: ok. '*' passed; '*)
        passed=${line#'test result: ok. '}
        passed=${passed%%' '*}
        case $passed in
          '' | *[!0-9]*) ;;
          *) tests=$((tests + passed)) ;;
        esac
        ;;
    esac
  done <"$work/test-log.txt"
}

measure() {
  enter_repository
  work=$(mktemp -d "${TMPDIR:-/tmp}/r7-account.XXXXXX") || fail 'mktemp -d failed'
  export CARGO_NET_OFFLINE=true
  check_vendored
  check_config_files
  check_links
  check_deny_config
  ensure_deny
  check_fetched
  check_metadata
  target=$repo/target/account
  CARGO_TARGET_DIR=$target cargo deny --offline --locked --manifest-path account/Cargo.toml check bans licenses sources
  code=$?
  [ "$code" -eq 0 ] || fail "cargo deny check bans licenses sources exited $code"
  CARGO_TARGET_DIR=$target cargo build --manifest-path account/Cargo.toml --locked --offline
  code=$?
  [ "$code" -eq 0 ] || fail "cargo build --locked --offline exited $code"
  log=${CI_D_ARTIFACTS:-$work}/cargo-test.log
  CARGO_TARGET_DIR=$target cargo test --manifest-path account/Cargo.toml --locked --offline >"$log" 2>&1
  code=$?
  cat "$log"
  [ "$code" -eq 0 ] || fail "cargo test --locked --offline exited $code"
  count_tests "$log"
  [ "$tests" -gt 0 ] || fail 'cargo test --locked --offline ran no test'
  cargo fmt --manifest-path account/Cargo.toml --check
  code=$?
  [ "$code" -eq 0 ] || fail "cargo fmt --check exited $code"
  CARGO_TARGET_DIR=$target cargo clippy --manifest-path account/Cargo.toml --all-targets --locked --offline -- -D warnings
  code=$?
  [ "$code" -eq 0 ] || fail "cargo clippy --all-targets --locked --offline exited $code"
  printf 'account packages=%s vendored=%s of 4 bans=%s tests=%s cargo-deny=%s %s\n' "$packages" "$compared" "$bans" "$tests" "$pin" "$tool"
  printf 'PASS %s\n' "$name"
}

write_stub() {
  mkdir "$1" || return 1
  cat >"$1/cargo" <<'EOF' || return 1
#!/bin/sh
printf '%s\n' "$*" >>"$STUB_LOG"
if [ "$1" = deny ] && [ "$2" = --version ]; then
  [ -z "${STUB_DENY_ABSENT:-}" ] || exit 101
  printf 'cargo-deny %s\n' "$STUB_DENY_VERSION"
  exit 0
fi
case $1 in
  fetch) exec "$STUB_REAL_CARGO" "$@" ;;
  metadata)
    if [ -n "${STUB_METADATA:-}" ]; then
      cat "$STUB_METADATA"
      exit 0
    fi
    exec "$STUB_REAL_CARGO" "$@"
    ;;
  deny) exit "${STUB_DENY_EXIT:-0}" ;;
  build) exit "${STUB_BUILD_EXIT:-0}" ;;
  test)
    printf 'running 3 tests\ntest result: ok. %s passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n' "${STUB_TESTS:-3}"
    exit "${STUB_TEST_EXIT:-0}"
    ;;
  fmt) exit "${STUB_FMT_EXIT:-0}" ;;
  clippy) exit "${STUB_CLIPPY_EXIT:-0}" ;;
esac
exit 99
EOF
  chmod +x "$1/cargo"
}

make_fixture() {
  fixture=$work/$1
  mkdir "$fixture" "$fixture/account" || return 1
  for entry in Cargo.toml Cargo.lock deny.toml schema crates; do
    cp -R "$repo/account/$entry" "$fixture/account/$entry" || return 1
  done
  mkdir "$fixture/scripts" && cp "$repo/scripts/prerequisites.sh" "$fixture/scripts/prerequisites.sh"
}

edit() {
  file=$fixture/account/$1
  shift
  if ! "$@" <"$file" >"$file.edited"; then
    wrong="$wrong ${fixture##*/}"
    return 1
  fi
  if cmp -s "$file" "$file.edited"; then
    printf 'the edit of %s changed nothing\n' "$file"
    wrong="$wrong ${fixture##*/}"
    return 1
  fi
  mv "$file.edited" "$file"
}

to_crlf() {
  file=$fixture/account/$1
  awk 'BEGIN { ORS = "\r\n" } 1' <"$file" >"$file.crlf" || return 1
  mv "$file.crlf" "$file" || return 1
  crs=$(tr -dc '\r' <"$file" | wc -c)
  [ "$((crs))" -gt 0 ]
}

write_crate() {
  mkdir -p "$1/src" || return 1
  printf '[package]\nname = "%s"\nversion = "0.1.0"\nedition = "2021"\npublish = false\n' "$2" >"$1/Cargo.toml" || return 1
  : >"$1/src/lib.rs"
}

relock() {
  (cd "$fixture" && "$real_cargo" metadata --offline --format-version 1 --manifest-path account/Cargo.toml >/dev/null 2>&1)
}

selected() {
  [ -z "${R7_ACCOUNT_CASES:-}" ] && return 0
  case " $R7_ACCOUNT_CASES " in
    *" $1 "*) return 0 ;;
  esac
  return 1
}

run_case() {
  case=$1
  selected "$case" || return 0
  want=$2
  mode=$3
  shift 3
  if [ "$mode" = stub ]; then
    path=$stub:$PATH
  else
    path=$PATH
  fi
  out=$(env PATH="$path" CI_D_REPO="$fixture" CI_D_ARTIFACTS="$fixture" STUB_DENY_VERSION="$pin" STUB_REAL_CARGO="$real_cargo" STUB_LOG="$fixture/cargo-calls.txt" "$@" sh "$self" 2>&1 </dev/null)
  code=$?
  last=${out##*"$nl"}
  case $want in
    PASS*) want_code=0 ;;
    *) want_code=1 ;;
  esac
  if [ "$last" = "$want" ] && [ "$code" -eq "$want_code" ]; then
    got=$want
  else
    got="exit $code: $last"
  fi
  if [ "$want" = "PASS $name" ]; then
    case $nl$out$nl in
      *"${nl}account packages=$packages vendored=4 of 4 bans=$bans tests=3 cargo-deny=$pin found$nl"*) ;;
      *) got="$got, without its counts line" ;;
    esac
    calls=$(cat "$fixture/cargo-calls.txt" 2>/dev/null)
    [ "$calls" = "$want_calls" ] || got="$got, with other cargo calls"
  fi
  printf 'case %s/%s want=%s got=%s\n' "$name" "$case" "$want" "$got"
  if [ "$got" != "$want" ]; then
    wrong="$wrong $case"
  elif [ "$want" != "PASS $name" ]; then
    rejected=$((rejected + 1))
  fi
}

stub_case() {
  make_fixture "$1" || fail "cannot make the fixture $1"
  run_case "$@"
}

harness_case() {
  selected "$1" || return 0
  printf 'case %s/%s want=refused got=%s\n' "$name" "$1" "$2"
  if [ "$2" = refused ]; then
    rejected=$((rejected + 1))
  else
    wrong="$wrong $1"
  fi
}

self_test() {
  enter_repository
  work=$(mktemp -d "${TMPDIR:-/tmp}/r7-account-self-test.XXXXXX") || fail 'mktemp -d failed'
  real_cargo=$(command -v cargo) || fail 'cargo is not on the PATH'
  stub=$work/stub
  write_stub "$stub" || fail 'cannot write the stub cargo'
  check_vendored
  check_deny_config
  deny_pin
  check_metadata
  rejected=0
  wrong=
  want_calls="deny --version
fetch --locked --offline --manifest-path account/Cargo.toml
metadata --locked --offline --format-version 1 --manifest-path account/Cargo.toml
deny --offline --locked --manifest-path account/Cargo.toml check bans licenses sources
build --manifest-path account/Cargo.toml --locked --offline
test --manifest-path account/Cargo.toml --locked --offline
fmt --manifest-path account/Cargo.toml --check
clippy --manifest-path account/Cargo.toml --all-targets --locked --offline -- -D warnings"
  cause_deny="cargo deny --version is not cargo-deny $pin; sh scripts/prerequisites.sh installs it"
  cause_fetch="cargo fetch --locked --offline exited 101; sh scripts/prerequisites.sh fetches the packages of account/Cargo.lock"
  cause_record='account/schema/vendored.txt does not name the four vendored files'
  cause_commit='account/schema/vendored.txt does not start with the line of its commit'
  cause_config='a cargo configuration file sits at the repository root or under account/'
  cause_admitted='account/deny.toml differs from the configuration that this script admits'
  cause_ban='account/deny.toml holds a ban that this script does not admit'
  cause_path="account/'s metadata holds a path outside account/"
  cause_name="account/'s metadata holds a package whose name contains sqlite or sqlcipher"

  fixture=$work/no-repository
  run_case no-repository "FAIL $name (cannot enter the repository that CI_D_REPO names)" stub
  stub_case unchanged "PASS $name" stub
  stub_case tests-fail "FAIL $name (cargo test --locked --offline exited 101)" stub STUB_TEST_EXIT=101
  stub_case no-test-ran "FAIL $name (cargo test --locked --offline ran no test)" stub STUB_TESTS=0
  stub_case test-count-not-a-number "FAIL $name (cargo test --locked --offline ran no test)" stub STUB_TESTS=3x
  stub_case build-fails "FAIL $name (cargo build --locked --offline exited 101)" stub STUB_BUILD_EXIT=101
  stub_case deny-fails "FAIL $name (cargo deny check bans licenses sources exited 2)" stub STUB_DENY_EXIT=2
  stub_case fmt-fails "FAIL $name (cargo fmt --check exited 1)" stub STUB_FMT_EXIT=1
  stub_case clippy-fails "FAIL $name (cargo clippy --all-targets --locked --offline exited 101)" stub STUB_CLIPPY_EXIT=101
  stub_case deny-other-version "FAIL $name ($cause_deny)" stub STUB_DENY_VERSION=0.0.1
  stub_case deny-absent "FAIL $name ($cause_deny)" stub STUB_DENY_ABSENT=1
  make_fixture empty-registry && mkdir "$fixture/empty-cargo-home" &&
    run_case empty-registry "FAIL $name ($cause_fetch)" stub CARGO_HOME="$fixture/empty-cargo-home"

  for vendored in schema.v1.json routes.v1.json samples.v1.json prompt-budget.json; do
    make_fixture "vendored-byte-$vendored" && edit "schema/$vendored" sed '1s/{/[/' &&
      run_case "vendored-byte-$vendored" "FAIL $name (account/schema/$vendored differs from its recorded sha256)" stub
    make_fixture "recorded-sha256-$vendored" && edit schema/vendored.txt sed "/ ${vendored%.json}\\.json /s/^0/g/; / ${vendored%.json}\\.json /s/^[1-9a-f]/0/; / ${vendored%.json}\\.json /s/^g/1/" &&
      run_case "recorded-sha256-$vendored" "FAIL $name (account/schema/$vendored differs from its recorded sha256)" stub
  done
  make_fixture record-without-a-file && edit schema/vendored.txt sed '/ prompt-budget.json /d' &&
    run_case record-without-a-file "FAIL $name ($cause_record)" stub
  make_fixture record-with-another-commit-line && edit schema/vendored.txt sed '1s/^commit /commit: /' &&
    run_case record-with-another-commit-line "FAIL $name ($cause_commit)" stub
  make_fixture commit-of-41-digits && edit schema/vendored.txt sed '1s/$/0/' &&
    run_case commit-of-41-digits "FAIL $name ($cause_commit)" stub
  make_fixture commit-in-upper-case && edit schema/vendored.txt sed '1s/^commit //; 1y/abcdef/ABCDEF/; 1s/^/commit /' &&
    run_case commit-in-upper-case "FAIL $name ($cause_commit)" stub
  make_fixture sum-of-65-digits && edit schema/vendored.txt awk 'BEGIN { while ((getline line) > 0) { if (++n == 2) { split(line, word, " "); line = word[1] "0 " word[2] " " word[3] } print line } }' &&
    run_case sum-of-65-digits "FAIL $name ($cause_record)" stub
  make_fixture sum-in-upper-case && edit schema/vendored.txt awk 'BEGIN { while ((getline line) > 0) { if (++n == 2) { split(line, word, " "); line = toupper(word[1]) " " word[2] " " word[3] } print line } }' &&
    run_case sum-in-upper-case "FAIL $name ($cause_record)" stub
  make_fixture record-with-a-fifth-line && edit schema/vendored.txt awk '{ print } END { print "0000000000000000000000000000000000000000000000000000000000000000 names.json contract/names/forbidden-names.json" }' &&
    run_case record-with-a-fifth-line "FAIL $name ($cause_record)" stub
  make_fixture fifth-line-without-newline && edit schema/vendored.txt awk 'BEGIN { while ((getline line) > 0) print line; printf "%s", "x" }' &&
    run_case fifth-line-without-newline "FAIL $name ($cause_record)" stub

  make_fixture config-source-replacement && mkdir "$fixture/.cargo" &&
    printf '[source.crates-io]\nreplace-with = "vendored"\n\n[source.vendored]\ndirectory = "vendor"\n' >"$fixture/.cargo/config.toml" &&
    run_case config-source-replacement "FAIL $name ($cause_config)" stub
  make_fixture config-patch-under-account && mkdir "$fixture/account/.cargo" &&
    printf '[patch.crates-io]\nitoa = { path = "../outside-itoa" }\n' >"$fixture/account/.cargo/config" &&
    run_case config-patch-under-account "FAIL $name ($cause_config)" stub
  make_fixture config-name-in-another-case && mkdir "$fixture/.CARGO" &&
    printf '[source.crates-io]\nreplace-with = "vendored"\n' >"$fixture/.CARGO/CONFIG.TOML" &&
    run_case config-name-in-another-case "FAIL $name ($cause_config)" stub
  make_fixture link-out-of-account && write_crate "$fixture/outside-crate" outside &&
    ln -s ../../outside-crate "$fixture/account/crates/evil" 2>/dev/null
  if [ -L "$fixture/account/crates/evil" ]; then
    run_case link-out-of-account "FAIL $name (account/ holds a symbolic link)" stub
  else
    printf 'case %s/link-out-of-account not run: this system made no symbolic link\n' "$name"
  fi

  make_fixture escaped-path-key && write_crate "$fixture/outside-crate" outside &&
    edit crates/halvo-wire/Cargo.toml awk 'BEGIN { while ((getline line) > 0) { print line; if (line == "[dependencies]") print "outside = { \"\\u0070ath\" = \"../../../outside-crate\" }" } }' &&
    relock && run_case escaped-path-key "FAIL $name ($cause_path)" stub
  make_fixture escaped-build-key && mkdir "$fixture/outside" && printf 'fn main() {}\n' >"$fixture/outside/build.rs" &&
    edit crates/halvo-wire/Cargo.toml awk 'BEGIN { while ((getline line) > 0) { print line; if (line == "publish = false") print "\"\\u0062uild\" = \"../../../outside/build.rs\"" } }' &&
    run_case escaped-build-key "FAIL $name ($cause_path)" stub
  make_fixture path-outside-account && write_crate "$fixture/outside-crate" outside &&
    edit crates/halvo-wire/Cargo.toml awk 'BEGIN { while ((getline line) > 0) { print line; if (line == "[dependencies]") print "outside = { path = \"../../../outside-crate\" }" } }' &&
    relock && run_case path-outside-account "FAIL $name ($cause_path)" stub
  make_fixture respelled-lock-entry && write_crate "$fixture/account/crates/sqlite-shim" sqlite-shim && relock &&
    edit Cargo.lock sed "s/^name = \"sqlite-shim\"\$/name = 'sqlite-shim'/" &&
    run_case respelled-lock-entry "FAIL $name ($cause_name)" stub

  make_fixture metadata-base
  base=$work/metadata-base.json
  (cd "$fixture" && "$real_cargo" metadata --locked --offline --format-version 1 --manifest-path account/Cargo.toml >"$base" 2>/dev/null)
  sed 's|"source":"registry+https://github.com/rust-lang/crates.io-index"|"source":"git+https://example.invalid/x.git#0"|' <"$base" >"$work/source.json"
  sed 's|"name":"itoa"|"name":"LibSQLite3-ffi"|' <"$base" >"$work/sqlite.json"
  sed 's|"name":"itoa"|"name":"SQLCipher-sys"|' <"$base" >"$work/sqlcipher.json"
  printf '{"packages":[],"workspace_members":[],"resolve":null,"version":1}\n' >"$work/empty.json"
  for crafted in source sqlite sqlcipher; do
    cmp -s "$base" "$work/$crafted.json" && wrong="$wrong metadata-$crafted"
  done
  run_case metadata-source-outside-crates-io "FAIL $name (account/'s metadata holds a package from a source outside crates.io)" stub STUB_METADATA="$work/source.json"
  run_case metadata-name-with-sqlite-in-mixed-case "FAIL $name ($cause_name)" stub STUB_METADATA="$work/sqlite.json"
  run_case metadata-name-with-sqlcipher "FAIL $name ($cause_name)" stub STUB_METADATA="$work/sqlcipher.json"
  run_case metadata-without-a-package "FAIL $name (account/'s metadata holds no package)" stub STUB_METADATA="$work/empty.json"

  make_fixture sqlite-in-lock && edit Cargo.lock awk '{ print } END { print ""; print "[[package]]"; print "name = \"libsqlite3-sys\""; print "version = \"0.35.0\""; print "source = \"registry+https://github.com/rust-lang/crates.io-index\"" }' &&
    run_case sqlite-in-lock "FAIL $name ($cause_fetch)" stub
  make_fixture sqlite-in-lock-mixed-case && edit Cargo.lock sed 's/^name = "itoa"$/name = "LibSQLite3-ffi"/' &&
    run_case sqlite-in-lock-mixed-case "FAIL $name ($cause_fetch)" stub
  make_fixture sqlcipher-in-lock && edit Cargo.lock sed 's/^name = "itoa"$/name = "SQLCipher-sys"/' &&
    run_case sqlcipher-in-lock "FAIL $name ($cause_fetch)" stub
  make_fixture git-source-in-lock && edit Cargo.lock sed '/^name = "itoa"$/{n;n;s|^source = .*|source = "git+https://example.invalid/itoa.git#0000000"|;}' &&
    run_case git-source-in-lock "FAIL $name ($cause_fetch)" stub
  make_fixture git-source-in-lock-without-spaces && edit Cargo.lock sed '/^name = "itoa"$/{n;n;s|^source = .*|source="git+https://example.invalid/itoa.git#0000000"|;}' &&
    run_case git-source-in-lock-without-spaces "FAIL $name ($cause_fetch)" stub
  make_fixture git-source-in-manifest && edit crates/halvo-wire/Cargo.toml sed 's|^serde_json = "1"$|serde_json = { git = "https://example.invalid/serde_json.git" }|' &&
    run_case git-source-in-manifest "FAIL $name ($cause_fetch)" stub
  make_fixture registry-in-manifest && edit crates/halvo-wire/Cargo.toml sed 's|^serde_json = "1"$|serde_json = { version = "1", registry = "other" }|' &&
    run_case registry-in-manifest "FAIL $name ($cause_fetch)" stub
  make_fixture patch-in-workspace && edit Cargo.toml awk '{ print } END { print "[patch.crates-io]" }' &&
    run_case patch-in-workspace "PASS $name" stub
  make_fixture patch-as-dotted-key && edit Cargo.toml awk '{ print } END { print "patch.crates-io.itoa = { path = \"crates/halvo-wire\" }" }' &&
    run_case patch-as-dotted-key "PASS $name" stub
  make_fixture patch-in-quoted-header && edit Cargo.toml awk '{ print } END { print "[ \"patch\" . crates-io ]" }' &&
    run_case patch-in-quoted-header "PASS $name" stub
  make_fixture path-in-literal-string && edit crates/halvo-wire/Cargo.toml sed "s|^serde_json = \"1\"$|serde_json = { 'path' = '../../../crates/halvo-schema' }|" &&
    run_case path-in-literal-string "FAIL $name ($cause_fetch)" stub
  make_fixture path-absolute && edit crates/halvo-wire/Cargo.toml sed 's|^serde_json = "1"$|serde_json = { path = "/opt/halvo-schema" }|' &&
    run_case path-absolute "FAIL $name ($cause_fetch)" stub
  make_fixture multi-line-string && edit crates/halvo-wire/Cargo.toml sed 's|^serde_json = "1"$|serde_json = { path = """../../../crates/halvo-schema""" }|' &&
    run_case multi-line-string "FAIL $name ($cause_fetch)" stub
  make_fixture member-outside-crates && edit Cargo.toml sed 's|^members = \["crates/\*"\]$|members = ["crates/*", "../tools/x"]|' &&
    run_case member-outside-crates "FAIL $name ($cause_fetch)" stub
  crlf=yes
  make_fixture crlf-line-ends || crlf=no
  for toml in Cargo.toml Cargo.lock deny.toml crates/halvo-wire/Cargo.toml; do
    [ "$crlf" = yes ] && to_crlf "$toml" || crlf=no
  done
  if [ "$crlf" = yes ]; then
    run_case crlf-line-ends "PASS $name" stub
  else
    printf 'case %s/crlf-line-ends could not write the CRLF files\n' "$name"
    wrong="$wrong crlf-line-ends"
  fi

  make_fixture licence-added-to-list && edit deny.toml sed 's/^    "Zlib",$/    "Zlib",\
    "GPL-3.0-only",/' &&
    run_case licence-added-to-list "FAIL $name ($cause_admitted)" stub
  make_fixture licence-exception && edit deny.toml sed 's/^unused-allowed-license = "allow"$/&\
exceptions = [{ allow = ["GPL-3.0-only"], crate = "serde" }]/' &&
    run_case licence-exception "FAIL $name ($cause_admitted)" stub
  make_fixture git-allowed && edit deny.toml sed 's|^allow-git = \[\]$|allow-git = ["https://example.invalid/x.git"]|' &&
    run_case git-allowed "FAIL $name ($cause_admitted)" stub
  make_fixture unknown-git-warns && edit deny.toml sed 's/^unknown-git = "deny"$/unknown-git = "warn"/' &&
    run_case unknown-git-warns "FAIL $name ($cause_admitted)" stub
  make_fixture extra-line-without-newline && edit deny.toml awk '{ print } END { printf "%s", "exclude-dev = true" }' &&
    run_case extra-line-without-newline "FAIL $name ($cause_admitted)" stub
  make_fixture ban-with-wrappers && edit deny.toml sed 's/^    { crate = "rusqlite", reason = \(.*\) },$/    { crate = "rusqlite", reason = \1, wrappers = ["halvo-wire"] },/' &&
    run_case ban-with-wrappers "FAIL $name ($cause_ban)" stub
  make_fixture ban-name-with-other-characters && edit deny.toml sed 's/{ crate = "rusqlite"/{ crate = "Rusqlite"/' &&
    run_case ban-name-with-other-characters "FAIL $name ($cause_ban)" stub
  make_fixture ban-reason-with-a-quote && edit deny.toml sed 's/reason = "SQLite: bindings/reason = "SQLite: \\"bindings/' &&
    run_case ban-reason-with-a-quote "FAIL $name ($cause_ban)" stub
  make_fixture ban-deleted && edit deny.toml sed '/{ crate = "sentry", /d' &&
    run_case ban-deleted "FAIL $name (account/deny.toml lacks the ban of sentry)" stub
  make_fixture ban-renamed && edit deny.toml sed 's/{ crate = "libsqlite3-sys"/{ crate = "libsqlite4-sys"/' &&
    run_case ban-renamed "FAIL $name (account/deny.toml lacks the ban of libsqlite3-sys)" stub
  make_fixture no-deny-pin && edit ../scripts/prerequisites.sh sed '/^cargo_deny_version=/d' &&
    run_case no-deny-pin "FAIL $name (scripts/prerequisites.sh pins no cargo-deny version)" stub
  make_fixture pin-with-other-characters && edit ../scripts/prerequisites.sh sed 's/^cargo_deny_version=.*/cargo_deny_version=0.20.2-x/' &&
    run_case pin-with-other-characters "FAIL $name (scripts/prerequisites.sh pins no cargo-deny version)" stub

  make_fixture licence-outside-list && edit crates/halvo-wire/Cargo.toml sed 's/^publish = false$/license = "GPL-3.0-only"/' &&
    run_case licence-outside-list "FAIL $name (cargo deny check bans licenses sources exited 4)" real
  make_fixture banned-package-in-graph && edit deny.toml sed 's/^deny = \[$/&\
    { crate = "itoa", reason = "a fixture of the self-test" },/' &&
    run_case banned-package-in-graph "FAIL $name (cargo deny check bans licenses sources exited 2)" real

  make_fixture edit-that-changes-nothing
  before=$wrong
  if edit Cargo.toml sed 's/^no such line$/x/'; then
    got=applied
  elif [ "$wrong" = "$before edit-that-changes-nothing" ]; then
    got=refused
  else
    got="refused without its case"
  fi
  wrong=$before
  harness_case edit-that-changes-nothing "$got"
  mkdir "$work/awk-that-copies"
  printf '#!/bin/sh\ncat\n' >"$work/awk-that-copies/awk"
  chmod +x "$work/awk-that-copies/awk"
  make_fixture crlf-without-carriage-returns
  if (PATH="$work/awk-that-copies:$PATH" && to_crlf Cargo.toml); then
    harness_case crlf-without-carriage-returns accepted
  else
    harness_case crlf-without-carriage-returns refused
  fi

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
