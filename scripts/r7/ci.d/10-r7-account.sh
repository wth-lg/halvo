#!/bin/sh
# jobs: macos windows
# rule: Every vendored file of account/schema/ has its recorded sha256, account/Cargo.lock holds no package named with sqlite or sqlcipher and no source outside crates.io, no manifest of account/ names another source or a path outside account/, account/deny.toml is the configuration that this script admits, and cargo deny check bans licenses sources, cargo build --locked and cargo test --locked pass for account/.
set -u
name=10-r7-account
case $0 in
  /*) self=$0 ;;
  *) self=$(pwd)/$0 ;;
esac
nl='
'
cr=$(printf '\r')
files='schema.v1.json contract/schema/v1.json
routes.v1.json contract/routes/v1.json
samples.v1.json contract/samples/v1.json
prompt-budget.json gate/prompt-budget.json'

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

lower_hex() {
  [ "${#1}" -eq "$2" ] || return 1
  case $1 in
    *[!0-9a-f]*) return 1 ;;
  esac
}

check_vendored() {
  record=account/schema/vendored.txt
  [ -f "$record" ] || fail 'account/schema/vendored.txt is missing'
  exec 3<"$record"
  IFS= read -r first <&3 || first=
  first=${first%"$cr"}
  case $first in
    'commit '*) commit=${first#'commit '} ;;
    *) commit= ;;
  esac
  lower_hex "$commit" 40 || fail 'account/schema/vendored.txt does not start with the line of its commit'
  compared=0
  while read -r want_name want_path; do
    IFS= read -r line <&3 || line=
    line=${line%"$cr"}
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

check_lock() {
  [ -f account/Cargo.lock ] || fail 'account/Cargo.lock is missing'
  packages=0
  sqlite=0
  outside=0
  while IFS= read -r line || [ -n "$line" ]; do
    squeeze "${line%"$cr"}"
    case $bare in
      '[[package]]') packages=$((packages + 1)) ;;
      'name="'*[sS][qQ][lL][iI][tT][eE]* | 'name="'*[sS][qQ][lL][cC][iI][pP][hH][eE][rR]*) sqlite=$((sqlite + 1)) ;;
      'source="registry+https://github.com/rust-lang/crates.io-index"') ;;
      'source='*) outside=$((outside + 1)) ;;
    esac
  done <account/Cargo.lock
  [ "$sqlite" -eq 0 ] || fail "account/Cargo.lock holds $sqlite packages whose names contain sqlite or sqlcipher"
  [ "$outside" -eq 0 ] || fail "account/Cargo.lock holds $outside packages from a source outside crates.io"
  [ "$packages" -gt 0 ] || fail 'account/Cargo.lock holds no package'
}

squeeze() {
  bare=$1
  while :; do
    case $bare in
      *[' 	']*) bare=${bare%%[' 	']*}${bare#*[' 	']} ;;
      *) return 0 ;;
    esac
  done
}

path_inside() {
  level=$1
  case $2 in
    '' | /* | [A-Za-z]:* | *\\*) return 1 ;;
  esac
  rest=$2/
  while [ -n "$rest" ]; do
    segment=${rest%%/*}
    rest=${rest#*/}
    case $segment in
      '' | .) ;;
      ..)
        level=$((level - 1))
        [ "$level" -ge 0 ] || return 1
        ;;
      *) level=$((level + 1)) ;;
    esac
  done
}

check_paths_of() {
  rest=$3
  while :; do
    case $rest in
      *"$2="*)
        rest=${rest#*"$2="}
        value=${rest%%[,\}]*}
        path_inside "$1" "$value" || fail "$manifest names a path outside account/"
        ;;
      *) return 0 ;;
    esac
  done
}

check_manifests() {
  manifests=0
  for manifest in account/Cargo.toml account/crates/*/Cargo.toml; do
    [ -f "$manifest" ] || continue
    manifests=$((manifests + 1))
    case $manifest in
      account/Cargo.toml) depth=0 ;;
      *) depth=2 ;;
    esac
    while IFS= read -r line || [ -n "$line" ]; do
      case $line in
        *'"""'* | *"'''"*) fail "$manifest holds a multi-line string, which this script does not read" ;;
      esac
      squeeze "${line%"$cr"}"
      while :; do
        case $bare in
          *[\"\']*) bare=${bare%%[\"\']*}${bare#*[\"\']} ;;
          *) break ;;
        esac
      done
      case $bare in
        '[patch'* | patch.* | '[replace'* | replace.* | '[source'* | source.* | git=* | *'{git='* | *',git='* | registry=* | *'{registry='* | *',registry='* | registry-index=* | *'{registry-index='* | *',registry-index='*)
          fail "$manifest names a source outside crates.io"
          ;;
      esac
      if [ "$depth" -eq 0 ]; then
        case $bare in
          'members=[crates/*]') ;;
          members=* | default-members=* | *.members=*) fail 'account/Cargo.toml names a workspace member outside account/crates/' ;;
        esac
      fi
      check_paths_of "$depth" path "$bare"
      check_paths_of "$depth" build "$bare"
    done <"$manifest"
  done
  [ "$manifests" -gt 1 ] || fail 'account/ holds no crate manifest under crates/'
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
  : >"$work/deny.kept" || fail 'cannot write in the temp directory'
  while IFS= read -r line || [ -n "$line" ]; do
    line=${line%"$cr"}
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
        ;;
      *) printf '%s\n' "$line" >>"$work/deny.kept" ;;
    esac
  done <account/deny.toml
  admitted_deny_config >"$work/deny.admitted" || fail 'cannot write in the temp directory'
  cmp -s "$work/deny.kept" "$work/deny.admitted" || fail 'account/deny.toml differs from the configuration that this script admits'
  [ "$bans" -gt 0 ] || fail 'account/deny.toml bans no package'
}

deny_pin() {
  pin=
  while IFS= read -r line || [ -n "$line" ]; do
    line=${line%"$cr"}
    case $line in
      'cargo-deny = "'*'"')
        pin=${line#'cargo-deny = "'}
        pin=${pin%\"}
        ;;
    esac
  done <account/Cargo.toml
  case $pin in
    '' | *[!0-9.]*) fail 'account/Cargo.toml names no cargo-deny version' ;;
  esac
}

ensure_deny() {
  deny_pin
  have=$(cargo deny --version 2>/dev/null)
  if [ "$have" = "cargo-deny $pin" ]; then
    tool=found
    printf 'cargo-deny %s found\n' "$pin"
    return 0
  fi
  if [ "${CI:-}" != true ]; then
    printf 'SKIP %s (cargo-deny %s is not installed, and outside CI this script installs nothing)\n' "$name" "$pin"
    exit 77
  fi
  start=$(date +%s)
  cargo install --locked cargo-deny --version "$pin"
  code=$?
  end=$(date +%s)
  [ "$code" -eq 0 ] || fail "cargo install --locked cargo-deny --version $pin exited $code"
  have=$(cargo deny --version 2>/dev/null)
  [ "$have" = "cargo-deny $pin" ] || fail "cargo-deny $pin is not on the PATH after its install"
  tool=installed
  printf 'cargo-deny %s installed in %s s\n' "$pin" "$((end - start))"
}

count_tests() {
  tests=0
  while IFS= read -r line || [ -n "$line" ]; do
    line=${line%"$cr"}
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
  done <"$1"
}

measure() {
  enter_repository
  work=$(mktemp -d "${TMPDIR:-/tmp}/r7-account.XXXXXX") || fail 'mktemp -d failed'
  check_vendored
  check_lock
  check_manifests
  check_deny_config
  ensure_deny
  target=$repo/target/account
  CARGO_TARGET_DIR=$target cargo deny --locked --manifest-path account/Cargo.toml check bans licenses sources
  code=$?
  [ "$code" -eq 0 ] || fail "cargo deny check bans licenses sources exited $code"
  CARGO_TARGET_DIR=$target cargo build --manifest-path account/Cargo.toml --locked
  code=$?
  [ "$code" -eq 0 ] || fail "cargo build --locked exited $code"
  log=${CI_D_ARTIFACTS:-$work}/cargo-test.log
  CARGO_TARGET_DIR=$target cargo test --manifest-path account/Cargo.toml --locked >"$log" 2>&1
  code=$?
  cat "$log"
  [ "$code" -eq 0 ] || fail "cargo test --locked exited $code"
  count_tests "$log"
  [ "$tests" -gt 0 ] || fail 'cargo test --locked ran no test'
  printf 'account packages=%s vendored=%s of 4 bans=%s tests=%s cargo-deny=%s %s\n' "$packages" "$compared" "$bans" "$tests" "$pin" "$tool"
  printf 'PASS %s\n' "$name"
}

write_stub() {
  mkdir "$1" || return 1
  cat >"$1/cargo" <<'EOF' || return 1
#!/bin/sh
printf '%s\n' "$*" >>"$STUB_LOG"
if [ "$1" = deny ] && [ "$2" = --version ]; then
  printf 'cargo-deny %s\n' "$STUB_DENY_VERSION"
  exit 0
fi
case $1 in
  deny) exit "${STUB_DENY_EXIT:-0}" ;;
  build) exit "${STUB_BUILD_EXIT:-0}" ;;
  test)
    printf 'running 3 tests\ntest result: ok. %s passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n' "${STUB_TESTS:-3}"
    exit "${STUB_TEST_EXIT:-0}"
    ;;
  install) exit "${STUB_INSTALL_EXIT:-1}" ;;
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

run_case() {
  case=$1
  want=$2
  mode=$3
  shift 3
  if [ "$mode" = stub ]; then
    path=$stub:$PATH
  else
    path=$PATH
  fi
  out=$(env PATH="$path" CI_D_REPO="$fixture" CI_D_ARTIFACTS="$fixture" STUB_DENY_VERSION="$pin" STUB_LOG="$fixture/cargo-calls.txt" "$@" sh "$self" 2>&1 </dev/null)
  code=$?
  last=${out##*"$nl"}
  case $want in
    PASS*) want_code=0 ;;
    SKIP*) want_code=77 ;;
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

self_test() {
  enter_repository
  work=$(mktemp -d "${TMPDIR:-/tmp}/r7-account-self-test.XXXXXX") || fail 'mktemp -d failed'
  stub=$work/stub
  write_stub "$stub" || fail 'cannot write the stub cargo'
  check_vendored
  check_lock
  check_deny_config
  deny_pin
  rejected=0
  wrong=
  want_calls="deny --version${nl}deny --locked --manifest-path account/Cargo.toml check bans licenses sources${nl}build --manifest-path account/Cargo.toml --locked${nl}test --manifest-path account/Cargo.toml --locked"
  fixture=$work/no-repository
  run_case no-repository "FAIL $name (cannot enter the repository that CI_D_REPO names)" stub
  stub_case unchanged "PASS $name" stub
  stub_case tests-fail "FAIL $name (cargo test --locked exited 101)" stub STUB_TEST_EXIT=101
  stub_case no-test-ran "FAIL $name (cargo test --locked ran no test)" stub STUB_TESTS=0
  stub_case build-fails "FAIL $name (cargo build --locked exited 101)" stub STUB_BUILD_EXIT=101
  stub_case deny-fails "FAIL $name (cargo deny check bans licenses sources exited 2)" stub STUB_DENY_EXIT=2
  stub_case deny-absent-outside-ci "SKIP $name (cargo-deny $pin is not installed, and outside CI this script installs nothing)" stub STUB_DENY_VERSION=0.0.1 CI=
  stub_case deny-install-fails-in-ci "FAIL $name (cargo install --locked cargo-deny --version $pin exited 1)" stub STUB_DENY_VERSION=0.0.1 CI=true
  stub_case deny-absent-after-install "FAIL $name (cargo-deny $pin is not on the PATH after its install)" stub STUB_DENY_VERSION=0.0.1 STUB_INSTALL_EXIT=0 CI=true

  make_fixture vendored-byte && edit schema/routes.v1.json sed '1s/{/[/' &&
    run_case vendored-byte "FAIL $name (account/schema/routes.v1.json differs from its recorded sha256)" stub
  make_fixture recorded-sha256 && edit schema/vendored.txt sed '/ samples\.v1\.json /s/^0/g/; / samples\.v1\.json /s/^[1-9a-f]/0/; / samples\.v1\.json /s/^g/1/' &&
    run_case recorded-sha256 "FAIL $name (account/schema/samples.v1.json differs from its recorded sha256)" stub
  make_fixture record-without-a-file && edit schema/vendored.txt sed '/ prompt-budget.json /d' &&
    run_case record-without-a-file "FAIL $name (account/schema/vendored.txt does not name the four vendored files)" stub
  make_fixture record-with-another-commit-line && edit schema/vendored.txt sed '1s/^commit /commit: /' &&
    run_case record-with-another-commit-line "FAIL $name (account/schema/vendored.txt does not start with the line of its commit)" stub
  make_fixture record-with-a-fifth-line && edit schema/vendored.txt awk '{ print } END { print "0000000000000000000000000000000000000000000000000000000000000000 names.json contract/names/forbidden-names.json" }' &&
    run_case record-with-a-fifth-line "FAIL $name (account/schema/vendored.txt does not name the four vendored files)" stub
  make_fixture sqlite-in-lock && edit Cargo.lock awk '{ print } END { print ""; print "[[package]]"; print "name = \"libsqlite3-sys\""; print "version = \"0.35.0\""; print "source = \"registry+https://github.com/rust-lang/crates.io-index\"" }' &&
    run_case sqlite-in-lock "FAIL $name (account/Cargo.lock holds 1 packages whose names contain sqlite or sqlcipher)" stub
  make_fixture sqlite-in-lock-mixed-case && edit Cargo.lock sed 's/^name = "itoa"$/name = "LibSQLite3-ffi"/' &&
    run_case sqlite-in-lock-mixed-case "FAIL $name (account/Cargo.lock holds 1 packages whose names contain sqlite or sqlcipher)" stub
  make_fixture sqlcipher-in-lock && edit Cargo.lock sed 's/^name = "itoa"$/name = "SQLCipher-sys"/' &&
    run_case sqlcipher-in-lock "FAIL $name (account/Cargo.lock holds 1 packages whose names contain sqlite or sqlcipher)" stub
  make_fixture git-source-in-lock && edit Cargo.lock sed '/^name = "itoa"$/{n;n;s|^source = .*|source = "git+https://example.invalid/itoa.git#0000000"|;}' &&
    run_case git-source-in-lock "FAIL $name (account/Cargo.lock holds 1 packages from a source outside crates.io)" stub
  make_fixture git-source-in-lock-without-spaces && edit Cargo.lock sed '/^name = "itoa"$/{n;n;s|^source = .*|source="git+https://example.invalid/itoa.git#0000000"|;}' &&
    run_case git-source-in-lock-without-spaces "FAIL $name (account/Cargo.lock holds 1 packages from a source outside crates.io)" stub
  make_fixture git-source-in-manifest && edit crates/halvo-wire/Cargo.toml sed 's|^serde_json = "1"$|serde_json = { git = "https://example.invalid/serde_json.git" }|' &&
    run_case git-source-in-manifest "FAIL $name (account/crates/halvo-wire/Cargo.toml names a source outside crates.io)" stub
  make_fixture registry-in-manifest && edit crates/halvo-wire/Cargo.toml sed 's|^serde_json = "1"$|serde_json = { version = "1", registry = "other" }|' &&
    run_case registry-in-manifest "FAIL $name (account/crates/halvo-wire/Cargo.toml names a source outside crates.io)" stub
  make_fixture patch-in-workspace && edit Cargo.toml awk '{ print } END { print "[patch.crates-io]" }' &&
    run_case patch-in-workspace "FAIL $name (account/Cargo.toml names a source outside crates.io)" stub
  make_fixture patch-as-dotted-key && edit Cargo.toml awk '{ print } END { print "patch.crates-io.itoa = { path = \"crates/halvo-wire\" }" }' &&
    run_case patch-as-dotted-key "FAIL $name (account/Cargo.toml names a source outside crates.io)" stub
  make_fixture patch-in-quoted-header && edit Cargo.toml awk '{ print } END { print "[ \"patch\" . crates-io ]" }' &&
    run_case patch-in-quoted-header "FAIL $name (account/Cargo.toml names a source outside crates.io)" stub
  make_fixture path-in-literal-string && edit crates/halvo-wire/Cargo.toml sed "s|^serde_json = \"1\"$|serde_json = { 'path' = '../../../crates/halvo-schema' }|" &&
    run_case path-in-literal-string "FAIL $name (account/crates/halvo-wire/Cargo.toml names a path outside account/)" stub
  make_fixture path-absolute && edit crates/halvo-wire/Cargo.toml sed 's|^serde_json = "1"$|serde_json = { path = "/opt/halvo-schema" }|' &&
    run_case path-absolute "FAIL $name (account/crates/halvo-wire/Cargo.toml names a path outside account/)" stub
  make_fixture multi-line-string && edit crates/halvo-wire/Cargo.toml sed 's|^serde_json = "1"$|serde_json = { path = """../../../crates/halvo-schema""" }|' &&
    run_case multi-line-string "FAIL $name (account/crates/halvo-wire/Cargo.toml holds a multi-line string, which this script does not read)" stub
  make_fixture member-outside-crates && edit Cargo.toml sed 's|^members = \["crates/\*"\]$|members = ["crates/*", "../tools/x"]|' &&
    run_case member-outside-crates "FAIL $name (account/Cargo.toml names a workspace member outside account/crates/)" stub
  make_fixture crlf-line-ends && crlf=yes &&
    for toml in Cargo.toml Cargo.lock deny.toml crates/halvo-wire/Cargo.toml; do edit "$toml" sed "s/\$/$cr/" || crlf=no; done &&
    [ "$crlf" = yes ] && run_case crlf-line-ends "PASS $name" stub
  make_fixture path-outside-account && edit crates/halvo-wire/Cargo.toml sed 's|^serde_json = "1"$|serde_json = { path = "../../../crates/halvo-schema" }|' &&
    run_case path-outside-account "FAIL $name (account/crates/halvo-wire/Cargo.toml names a path outside account/)" stub
  make_fixture licence-added-to-list && edit deny.toml sed 's/^    "Zlib",$/    "Zlib",\
    "GPL-3.0-only",/' &&
    run_case licence-added-to-list "FAIL $name (account/deny.toml differs from the configuration that this script admits)" stub
  make_fixture licence-exception && edit deny.toml sed 's/^unused-allowed-license = "allow"$/&\
exceptions = [{ allow = ["GPL-3.0-only"], crate = "serde" }]/' &&
    run_case licence-exception "FAIL $name (account/deny.toml differs from the configuration that this script admits)" stub
  make_fixture git-allowed && edit deny.toml sed 's|^allow-git = \[\]$|allow-git = ["https://example.invalid/x.git"]|' &&
    run_case git-allowed "FAIL $name (account/deny.toml differs from the configuration that this script admits)" stub
  make_fixture unknown-git-warns && edit deny.toml sed 's/^unknown-git = "deny"$/unknown-git = "warn"/' &&
    run_case unknown-git-warns "FAIL $name (account/deny.toml differs from the configuration that this script admits)" stub
  make_fixture extra-line-without-newline && edit deny.toml awk '{ print } END { printf "%s", "exclude-dev = true" }' &&
    run_case extra-line-without-newline "FAIL $name (account/deny.toml differs from the configuration that this script admits)" stub
  make_fixture ban-with-wrappers && edit deny.toml sed 's/^    { crate = "rusqlite", reason = \(.*\) },$/    { crate = "rusqlite", reason = \1, wrappers = ["halvo-wire"] },/' &&
    run_case ban-with-wrappers "FAIL $name (account/deny.toml holds a ban that this script does not admit)" stub
  make_fixture no-deny-pin && edit Cargo.toml sed '/^cargo-deny = /d' &&
    run_case no-deny-pin "FAIL $name (account/Cargo.toml names no cargo-deny version)" stub

  have=$(cargo deny --version 2>/dev/null)
  if [ "$have" = "cargo-deny $pin" ] || [ "${CI:-}" = true ]; then
    make_fixture licence-outside-list && edit crates/halvo-wire/Cargo.toml sed 's/^publish = false$/license = "GPL-3.0-only"/' &&
      run_case licence-outside-list "FAIL $name (cargo deny check bans licenses sources exited 4)" real
    make_fixture banned-package-in-graph && edit deny.toml sed 's/^deny = \[$/&\
    { crate = "itoa", reason = "a fixture of the self-test" },/' &&
      run_case banned-package-in-graph "FAIL $name (cargo deny check bans licenses sources exited 2)" real
  else
    printf 'case %s/licence-outside-list and banned-package-in-graph not run: cargo-deny %s is not installed\n' "$name" "$pin"
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
