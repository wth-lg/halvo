#!/bin/sh
# jobs: macos windows
# rule: Every vendored file of account/schema/ has its recorded sha256, no cargo configuration file sits at the repository root or under account/ and no symbolic link under account/, account/deny.toml is the configuration that this script admits with at least the 27 bans of unit 7.1, and, offline with the cargo-deny version and the packages that scripts/prerequisites.sh installs and fetches, cargo's metadata of account/ holds no package from a source outside crates.io, no local path outside account/ on the disk or by its text and no package named with sqlite or sqlcipher, no tracked file lies under target/account, and after cargo clean of the packages of account/ cargo deny check bans licenses sources over the four targets that Halvo is built for, build, test, fmt --check and clippy with all features pass for account/, the packages of account/Cargo.lock that no such target compiles and whose licence is outside the allow list are the ones that this script lists, the dep-info files that this build wrote for the packages of account/, each package with at least one, name no file with a tab or another control character, which the reader cannot split without doubt and so refuses, and list no file that, made real on the disk, is a symbolic link, is not there, or lies outside account/ and outside the out directory that the same dep-info file names for its build script under target/account, and no tracked file differs from HEAD after cargo ran; on a GitHub-hosted runner the hosted test of halvo-secrets stores, reads and deletes an item and prints store=keychain and store=file in the macos job and store=dpapi in the windows job. Its self-test runs the real builds path-attribute-outside, include-outside, include-str-outside, include-bytes-outside, include-from-the-target-directory, moved-tree, moved-tree-with-an-outside-include, source-put-right, member-named-with-a-hash, directory-name-with-a-tab in the macos job alone and includes-inside in both jobs, fails when the real builds of its job are none, and fails in a whole run, one without R7_ACCOUNT_CASES, when none ran. Two lists of the script name every case of the self-test, 95 and 72: the macos job runs both, and the windows job runs the first alone and prints one line for each case of the second, because the windows job is held under ST-22's line. The first holds the cases that end before the first cargo fetch, the cases whose subject differs between the two systems (line ends, the tracked-file guard, paths that leave account/, the drive and cygpath spellings, names whose bytes the tools of the windows runner read in their own way, includes-inside and the hosted store of the windows job), the controls unchanged, git-source-in-lock, banned-package-in-graph, dep-info-path-outside and tracked-file-under-the-target, and the harness cases. The self-test fails when a case of its job's list did not run (the link case alone may print that its system made no symbolic link), when a case stands on no list, when the count of cases left to the macos job is wrong or when the job that it read is not the driver's, a verdict that one function gives from the five values that the run holds, read by one more function, and that harness cases check with values of their own at both. The windows job therefore no longer shows that the script refuses the left-out cases under the shell tools of the windows runner; the macos job of the same commit does, and a commit needs both jobs green. Limits: what a build script or a proc macro of account/ reads, copies or writes while it runs is not seen. A doc example is compiled by rustdoc, which writes no dep-info. Code under a cfg that neither job compiles is not read. Code of account/ that runs in the job (a build script, a proc macro, a test) can change the target before the check reads it. A source replacement in the cargo home of the machine is outside the check.
set -u
LC_ALL=C
export LC_ALL
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
outside_licences='webpki-root-certs 1.0.11'
windows_cases='no-repository unchanged hosted-windows hosted-line-inside-another hosted-windows-line-missing
deny-other-version deny-absent vendored-byte-schema.v1.json recorded-sha256-schema.v1.json
vendored-byte-routes.v1.json recorded-sha256-routes.v1.json vendored-byte-samples.v1.json
recorded-sha256-samples.v1.json vendored-byte-prompt-budget.json
recorded-sha256-prompt-budget.json record-without-a-file record-with-another-commit-line
commit-of-41-digits commit-in-upper-case sum-of-65-digits sum-in-upper-case
record-with-a-fifth-line fifth-line-without-newline config-source-replacement
config-patch-under-account config-name-in-another-case link-out-of-account escaped-path-key
escaped-build-key path-outside-account git-source-in-lock crlf-line-ends licence-added-to-list
licence-exception git-allowed unknown-git-warns extra-line-without-newline ban-with-wrappers
ban-name-with-other-characters ban-reason-with-a-quote ban-deleted ban-renamed
deny-targets-fifth deny-targets-lack-one deny-targets-gone tracked-unchanged
tracked-file-changed untracked-file-written no-deny-pin pin-with-other-characters
banned-package-in-graph edit-that-changes-nothing crlf-without-carriage-returns job-read-once
no-job-runs-every-case windows-job-runs-its-list-alone macos-job-misses-a-case
windows-job-misses-a-case case-on-no-list verdict-names-a-case-not-run
verdict-names-a-case-on-no-list verdict-names-a-left-out-count verdict-names-a-job-not-read
verdict-of-right-values-has-no-word record-line-with-another-name record-line-with-another-path
manifest-outside-account dep-info-path-outside dep-info-windows-separators
dep-info-windows-drive dep-info-name-starting-with-a-hash tracked-file-under-the-target
metadata-path-in-a-sibling-of-account dep-info-escaped-space
dep-info-name-with-a-colon-and-a-newline target-in-another-spelling target-in-the-drive-spelling
dep-info-name-with-a-newline includes-inside ban-with-a-third-key ban-reason-with-a-backslash
ban-line-cut-after-its-reason macos-list-emptied windows-list-emptied
whole-run-without-a-real-build windows-job-runs-a-macos-build macos-job-misses-a-left-out-case
windows-job-misses-a-case-of-its-list windows-job-runs-a-left-out-case
dep-info-name-with-a-tab dep-info-name-with-a-control-character
end-verdict-reads-the-drivers-job end-verdict-reads-the-cases-that-ran
end-verdict-reads-the-cases-on-no-list end-verdict-reads-the-left-out-count'
macos_cases='tests-fail no-test-ran test-count-not-a-number build-fails deny-fails fmt-fails clippy-fails
self-hosted-runner actions-not-true hosted-macos hosted-test-fails hosted-line-missing
hosted-line-of-another-store hosted-file-line-missing hosted-other-job empty-registry
respelled-lock-entry metadata-source-outside-crates-io metadata-name-with-sqlite-in-mixed-case
metadata-name-with-sqlcipher metadata-without-a-package sqlite-in-lock sqlite-in-lock-mixed-case
sqlcipher-in-lock git-source-in-lock-without-spaces git-source-in-manifest registry-in-manifest
patch-in-workspace patch-as-dotted-key patch-in-quoted-header path-in-literal-string
path-absolute multi-line-string member-outside-crates outside-licence-another
outside-licence-unnamed outside-licence-exit outside-licence-one-less licence-outside-list
dep-info-absent dep-info-absolute-path-outside dep-info-missing-file dep-info-missing-directory
dep-info-head-elsewhere dep-info-unopenable member-without-dep-info dep-info-link-then-dot-dot
dep-info-target-outside-out dep-info-file-beside-out dep-info-in-another-out-directory
dep-info-out-directory-without-build dep-info-out-directory-not-named-out
dep-info-out-directory-two-levels-deep dep-info-head-names-a-missing-file
dep-info-in-a-sibling-of-account repository-through-a-link path-attribute-outside
include-outside include-str-outside include-bytes-outside include-from-the-target-directory
moved-tree moved-tree-with-an-outside-include source-put-right member-named-with-a-hash
target-through-a-link link-as-account-target dep-info-link-in-a-source-directory
dep-info-link-in-an-out-directory dep-info-backslash-in-a-directory-name
dep-info-letter-and-colon-directory directory-name-with-a-tab'
real_builds_macos='path-attribute-outside include-outside include-str-outside include-bytes-outside include-from-the-target-directory includes-inside moved-tree moved-tree-with-an-outside-include source-put-right member-named-with-a-hash directory-name-with-a-tab'
real_builds_windows='includes-inside'

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
  find account -path account/target -type d -prune -o -type l -print >"$work/links.txt" 2>/dev/null
  [ ! -s "$work/links.txt" ] || fail 'account/ holds a symbolic link'
}

admitted_deny_config() {
  cat <<'EOF'
[graph]
all-features = true
targets = ["aarch64-apple-darwin", "x86_64-apple-darwin", "x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc"]

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
  sed 's/,/,\
/g' | awk '
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
    print "member\t" package "\t" manifest
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
BEGIN { d = 0; in_string = 0 }
{
  text = $0; n = length(text); i = 1
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
    dir=$place
    leaf=
  else
    dir=$(dirname "$place")
    leaf=/${place##*/}
  fi
  both=$(cd "$dir" 2>/dev/null && pwd -L && pwd -P) || return 1
  [ "${both%%"$nl"*}" = "${both#*"$nl"}" ] || return 1
  resolved=${both#*"$nl"}$leaf
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
  members=
  : >"$work/members.tsv" || fail 'cannot write in the temp directory'
  while IFS="$tab" read -r kind value manifest || [ -n "$kind" ]; do
    case $kind in
      member)
        members="$members -p $value"
        printf '%s\t%s\n' "$value" "$manifest" >>"$work/members.tsv"
        ;;
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
  [ -n "$members" ] || fail "account/'s metadata holds no package of account/"
}

check_tracked_target() {
  git -C "$repo" rev-parse --git-dir >/dev/null 2>&1 || return 0
  tracked=$(git -C "$repo" ls-files -- target/account) || fail 'git ls-files cannot read the index'
  [ -z "$tracked" ] || fail 'a tracked file lies under the build target directory target/account'
}

clear_members() {
  set --
  while IFS="$tab" read -r member manifest || [ -n "$member" ]; do
    set -- "$@" -p "$member"
  done <"$work/members.tsv"
  CARGO_TARGET_DIR=$target cargo clean --offline "$@" --manifest-path account/Cargo.toml
  code=$?
  [ "$code" -eq 0 ] || fail "cargo clean --offline of the packages of account/ exited $code"
}

dep_info_read() {
  awk '
function add(k, i, v, unescape,  c) {
  if (unescape) gsub(/\\ /, " ", v)
  for (c = 1; c <= 32; c++) if (index(v, ctrl[c])) { control++; break }
  n++; kind[n] = k; idx[n] = i; value[n] = v
}
function whole(p) { return substr(p, 1, 1) == "/" ? p : account "/" p }
function parent(p) { sub(/\/[^\/]*$/, "", p); return p == "" ? "/" : p }
BEGIN {
  account = ENVIRON["DEP_INFO_ACCOUNT"]; names = ENVIRON["DEP_INFO_NAMES"]
  list = ENVIRON["DEP_INFO_LIST"]; members = ENVIRON["DEP_INFO_MEMBERS"]
  n = 0; files = 0; unopenable = 0; control = 0
  for (c = 1; c < 32; c++) ctrl[c] = sprintf("%c", c)
  ctrl[32] = sprintf("%c", 127)
  while ((getline file < list) > 0) {
    files++
    status = (getline line < file)
    if (status < 0) { unopenable++; continue }
    while (status > 0) {
      if (index(line, "# env-dep:OUT_DIR=") == 1) add("outdir", files, substr(line, 19), 1)
      else if (index(line, "# env-dep:") == 1 || line == "") { }
      else if (line ~ /:$/) add("path", files, substr(line, 1, length(line) - 1), 1)
      else if (index(line, ": ") > 1) add("head", files, substr(line, 1, index(line, ": ") - 1), 1)
      else bad[files] = 1
      status = (getline line < file)
    }
    close(file)
  }
  close(list)
  while ((getline line < members) > 0) {
    split(line, part, "\t")
    add("member", part[1], part[2], 0)
  }
  close(members)
  if (control) { print "control\t" control "\t-\t-"; exit }
  if (ENVIRON["DEP_INFO_CONVERT"] == "1") {
    for (i = 1; i <= n; i++) print value[i] > names
    close(names)
    command = "cygpath -u -f \"" names "\""
    converted = 0
    while ((command | getline line) > 0) value[++converted] = line
    close(command)
    if (converted != n) { print "unconverted\t0\t-\t-"; exit }
  }
  for (i = 1; i <= n; i++) if (kind[i] == "path" && substr(value[i], 1, 1) != "/") local[idx[i]] = 1
  count = 0; unread = 0
  for (f in local) { count++; if (f in bad) unread++ }
  print "local\t" count "\t-\t-"
  print "unopenable\t" unopenable "\t-\t-"
  print "unread\t" unread "\t-\t-"
  for (i = 1; i <= n; i++) if (kind[i] == "outdir" && idx[i] in local) print "outdir\t" idx[i] "\t" whole(value[i]) "\t."
  dirs = 0
  for (i = 1; i <= n; i++) {
    if ((kind[i] != "path" && kind[i] != "head") || !(idx[i] in local)) continue
    p = whole(value[i]); dir = parent(p)
    if (!(dir in group)) order[++dirs] = dir
    group[dir] = group[dir] kind[i] "\t" idx[i] "\t" dir "\t" substr(p, length(dir) + 2) "\n"
  }
  for (d = 1; d <= dirs; d++) printf "%s", group[order[d]]
  for (i = 1; i <= n; i++) if (kind[i] == "member") print "member\t" idx[i] "\t" parent(whole(value[i])) "\t."
}'
}

place_of() {
  place=$1
  while :; do
    if [ "$place" -ef "$account_dir" ]; then
      place=account
      return 0
    fi
    if [ "$place" -ef "$target" ]; then
      place=target
      return 0
    fi
    case $place in
      */*) place=${place%/*} ;;
      *) break ;;
    esac
    [ -n "$place" ] || break
  done
  place=other
}

below() {
  inner=$1
  while :; do
    [ "$inner" -ef "$2" ] && return 0
    case $inner in
      */*) inner=${inner%/*} ;;
      *) return 1 ;;
    esac
    [ -n "$inner" ] || return 1
  done
}

out_dir_of() {
  cd -P "$1" 2>/dev/null || return 1
  out_real=$PWD
  [ "${out_real##*/}" = out ] || return 1
  build=${out_real%/*}
  build=${build%/*}
  [ "${build##*/}" = build ] || return 1
  [ "${build%/*/*}" -ef "$target" ]
}

read_dir_below() {
  found=no
  old_ifs=$IFS
  IFS=$nl
  set -f
  for read_dir in $read_dirs; do
    if below "$read_dir" "$1"; then
      found=yes
      break
    fi
  done
  set +f
  IFS=$old_ifs
  [ "$found" = yes ]
}

check_sources() {
  stale_fix='cargo clean --manifest-path account/Cargo.toml --target-dir target/account clears it'
  find "$target" -name '*.d' -type f >"$work/dep-info.txt" || fail "the search for dep-info files under target/account failed; $stale_fix"
  convert=0
  if command -v cygpath >/dev/null 2>&1; then
    convert=1
  fi
  DEP_INFO_LIST=$work/dep-info.txt DEP_INFO_MEMBERS=$work/members.tsv DEP_INFO_ACCOUNT=$account_dir DEP_INFO_NAMES=$work/dep-info.names DEP_INFO_CONVERT=$convert dep_info_read >"$work/dep-info.read" || fail 'awk cannot read the dep-info files'
  here=$PWD
  dep_info=0
  unopenable=0
  unread=0
  elsewhere=0
  missing=0
  links=0
  outside=0
  member_missing=
  outs=$nl
  read_dirs=
  last_dir=
  real=
  place=other
  while IFS="$tab" read -r kind idx dir leaf || [ -n "$kind" ]; do
    case $kind in
      control) fail 'a dep-info file of account/ names a file with a tab or another control character, which this check cannot read without doubt' ;;
      local) dep_info=$idx ;;
      unconverted) fail 'cygpath gave fewer paths than the dep-info files hold' ;;
      unopenable) unopenable=$idx ;;
      unread)
        unread=$idx
        [ "$unread" -eq 0 ] || printf 'a dep-info file of account/ holds a line that names no file whole\n'
        ;;
      outdir) ! out_dir_of "$dir" || outs=$outs$idx$tab$out_real$nl ;;
      member)
        cd -P "$dir" 2>/dev/null || fail "cannot enter the directory of the package $idx"
        if [ -z "$member_missing" ] && ! read_dir_below "$PWD"; then
          printf 'no dep-info file names a source of the package %s\n' "$idx"
          member_missing=$idx
        fi
        ;;
      head | path)
        if [ "$dir" != "$last_dir" ]; then
          last_dir=$dir
          if cd -P "$dir" 2>/dev/null; then
            real=$PWD
            place_of "$real"
            [ "$place" != account ] || read_dirs=$read_dirs$real$nl
          else
            real=
            place=other
          fi
        fi
        if [ "$kind" = head ]; then
          if [ -z "$real" ] || [ ! -f "$real/$leaf" ] || [ "$place" != target ]; then
            elsewhere=$((elsewhere + 1))
          fi
        elif [ -z "$real" ] || { [ ! -e "$real/$leaf" ] && [ ! -L "$real/$leaf" ]; }; then
          missing=$((missing + 1))
        elif [ -L "$real/$leaf" ]; then
          links=$((links + 1))
          printf 'read through a symbolic link: %s\n' "$leaf"
        elif [ "$place" = target ]; then
          admitted=no
          case $outs in
            *"$nl$idx$tab"*)
              out=${outs#*"$nl$idx$tab"}
              out=${out%%"$nl"*}
              ! below "$real" "$out" || admitted=yes
              ;;
          esac
          if [ "$admitted" = no ]; then
            outside=$((outside + 1))
            printf 'read outside account/: a file under target/account outside the out directory of its build script\n'
          fi
        elif [ "$place" != account ]; then
          outside=$((outside + 1))
          case $real/$leaf in
            "${account_dir%/*}"/*) printf 'read outside account/: %s\n' "${real#"${account_dir%/*}"/}/$leaf" ;;
            *) printf 'read outside account/: a file outside the repository\n' ;;
          esac
        fi
        ;;
    esac
  done <"$work/dep-info.read"
  cd "$here" || fail 'cannot return to the repository'
  [ "$unopenable" -eq 0 ] || fail "the target holds a dep-info file that this check cannot open; $stale_fix"
  [ "$unread" -eq 0 ] || fail 'a source of account/ reads a file outside account/'
  [ "$elsewhere" -eq 0 ] || fail "the target holds dep-info that this build did not write, whose rule names another place; $stale_fix"
  [ "$missing" -eq 0 ] || fail "the target holds dep-info that this build did not write, which lists a file that is not there; $stale_fix"
  [ "$links" -eq 0 ] || fail 'a source of account/ reads a file through a symbolic link'
  [ "$outside" -eq 0 ] || fail 'a source of account/ reads a file outside account/'
  [ "$dep_info" -gt 0 ] || fail 'cargo wrote no dep-info file of a package of account/'
  [ -z "$member_missing" ] || fail 'a package of account/ has no dep-info file that this check read'
}

outside_names() {
  awk '
/"type":"diagnostic"/ && /"severity":"error"/ {
  line = $0
  at = index(line, "\"Krate\":{\"name\":\"")
  if (at == 0) { print "unnamed"; next }
  rest = substr(line, at + 17)
  name = substr(rest, 1, index(rest, "\"") - 1)
  rest = substr(rest, length(name) + 1)
  if (name == "" || substr(rest, 1, 13) != "\",\"version\":\"") { print "unnamed"; next }
  rest = substr(rest, 14)
  version = substr(rest, 1, index(rest, "\"") - 1)
  licence = "unknown"
  at = index(line, "\"span\":\"")
  if (at > 0) { rest = substr(line, at + 8); licence = substr(rest, 1, index(rest, "\"") - 1) }
  print name " " version "\t" licence
}' "$1"
}

check_outside_licences() {
  grep -v '^targets = \[' "$work/deny.txt" >"$work/deny-all-targets.toml" || fail 'cannot write in the temp directory'
  CARGO_TARGET_DIR=$target cargo deny --format json --color never --config "$work/deny-all-targets.toml" --offline --locked --manifest-path account/Cargo.toml check licenses >"$work/outside.out" 2>"$work/outside.json"
  code=$?
  case $code in
    0 | 4) ;;
    *) fail "cargo deny check licenses over every target exited $code" ;;
  esac
  outside_names "$work/outside.json" | sort -u >"$work/outside.found" || fail 'cannot read the output of cargo deny'
  if grep -q '^unnamed$' "$work/outside.found"; then
    fail 'cargo deny refused a licence that names no package'
  fi
  cut -f1 "$work/outside.found" >"$work/outside.names"
  : >"$work/outside.listed"
  if [ -n "$outside_licences" ]; then
    printf '%s\n' "$outside_licences" | sort -u >"$work/outside.listed"
  fi
  outside=$(awk 'END { print NR }' "$work/outside.found")
  named=$(awk -F '\t' '{ printf "%s%s %s", (NR > 1 ? "; " : ""), $1, $2 }' "$work/outside.found")
  if [ "$outside" -eq 0 ]; then
    printf 'licences outside the four targets: 0\n'
  else
    printf 'licences outside the four targets: %s (%s)\n' "$outside" "$named"
  fi
  cmp -s "$work/outside.names" "$work/outside.listed" || fail "the packages of account/Cargo.lock that no target of Halvo compiles and whose licence is outside the allow list differ from outside_licences of this script; read each and list it there"
}

check_tracked() {
  command -v git >/dev/null 2>&1 || fail 'git is not on the PATH'
  inside=$(git rev-parse --is-inside-work-tree 2>/dev/null) || inside=false
  prefix=$(git rev-parse --show-prefix 2>/dev/null) || prefix=
  if [ "$inside" != true ] || [ -n "$prefix" ]; then
    printf 'tracked files not compared: the repository is no git work tree of its own\n'
    return 0
  fi
  if git rev-parse -q --verify HEAD >/dev/null 2>&1; then
    git diff --name-only HEAD -- >"$work/tracked.changed" 2>"$work/tracked.err" || fail 'git diff against HEAD failed'
  else
    git diff --name-only -- >"$work/tracked.changed" 2>"$work/tracked.err" || fail 'git diff against the index failed'
  fi
  changed=$(awk 'END { print NR }' "$work/tracked.changed")
  [ "$changed" -eq 0 ] || fail "$changed tracked files differ from HEAD after cargo ran"
  printf 'tracked files changed by the check: 0\n'
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

hosted_store() {
  log=${CI_D_ARTIFACTS:-$work}/hosted-store-$1.log
  CARGO_TARGET_DIR=$target cargo test --manifest-path account/Cargo.toml --locked --offline -p halvo-secrets --features "$2" --test hosted -- --test-threads=1 --nocapture >"$log" 2>&1
  code=$?
  cat "$log"
  [ "$code" -eq 0 ] || fail "the hosted test of store=$1 exited $code"
  without_cr "$log" hosted-store.txt
  grep -qx "store=$1" "$work/hosted-store.txt" || fail "the hosted test printed no line store=$1"
  stores="$stores store=$1"
}

hosted_stores() {
  if [ "${GITHUB_ACTIONS:-}" != true ] || [ "${RUNNER_ENVIRONMENT:-}" != github-hosted ]; then
    printf 'hosted stores not run: not a GitHub-hosted runner\n'
    return 0
  fi
  stores=
  case ${CI_D_JOB:-} in
    macos)
      hosted_store keychain hosted-store-tests,keychain
      hosted_store file hosted-store-tests
      ;;
    windows) hosted_store dpapi hosted-store-tests ;;
    *) fail 'CI_D_JOB names neither macos nor windows' ;;
  esac
  printf 'hosted stores:%s\n' "$stores"
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
  check_tracked_target
  CARGO_TARGET_DIR=$target cargo deny --offline --locked --manifest-path account/Cargo.toml check bans licenses sources
  code=$?
  [ "$code" -eq 0 ] || fail "cargo deny check bans licenses sources exited $code"
  check_outside_licences
  clear_members
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
  hosted_stores
  cargo fmt --manifest-path account/Cargo.toml --all --check
  code=$?
  [ "$code" -eq 0 ] || fail "cargo fmt --check exited $code"
  CARGO_TARGET_DIR=$target cargo clippy --manifest-path account/Cargo.toml --all-targets --all-features --locked --offline -- -D warnings
  code=$?
  [ "$code" -eq 0 ] || fail "cargo clippy --all-targets --locked --offline exited $code"
  check_sources
  check_tracked
  printf 'account packages=%s vendored=%s of 4 bans=%s tests=%s cargo-deny=%s %s dep-info=%s\n' "$packages" "$compared" "$bans" "$tests" "$pin" "$tool" "$dep_info"
  printf 'PASS %s\n' "$name"
}

write_stub() {
  mkdir "$1" || return 1
  cat >"$1/cargo" <<'EOF' || return 1
#!/bin/sh
printf '%s\n' "$*" | sed 's|--config [^ ]*|--config CONFIG|' >>"$STUB_LOG"
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
  deny)
    case " $* " in
      *' --format json '*)
        [ -z "${STUB_DENY_OUTSIDE:-}" ] || cat "$STUB_DENY_OUTSIDE" >&2
        exit "${STUB_OUTSIDE_EXIT:-0}"
        ;;
    esac
    exit "${STUB_DENY_EXIT:-0}"
    ;;
  clean)
    [ -z "${STUB_BUILD_REAL:-}" ] || exec "$STUB_REAL_CARGO" "$@"
    exit "${STUB_CLEAN_EXIT:-0}"
    ;;
  build)
    [ -z "${STUB_BUILD_REAL:-}" ] || exec "$STUB_REAL_CARGO" "$@"
    t=$CARGO_TARGET_DIR
    mkdir -p "$t/debug/deps" || exit 98
    if [ -z "${STUB_NO_DEP_INFO:-}" ]; then
      s=${STUB_TARGET_SPELLING:-$t}
      out=${STUB_OUT_DIR:-debug/build/halvo-wire-0/out}
      mkdir -p "$t/$out" && printf 'pub const X: u8 = 0;\n' >"$t/$out/bounds.rs" || exit 98
      extra=${STUB_DEP:+$STUB_DEP:}
      if [ -n "${STUB_TARGET_DEP:-}" ]; then
        mkdir -p "$t/${STUB_TARGET_DEP%/*}" && printf 'x\n' >"$t/$STUB_TARGET_DEP" || exit 98
        extra=$s/$STUB_TARGET_DEP:
      fi
      case ${STUB_LINK_IN:-} in
        src) ln -s ../../../../outside/code.rs account/crates/halvo-wire/src/linked.rs && extra='crates/halvo-wire/src/linked.rs:' || exit 98 ;;
        out) ln -s "$(pwd -P)/outside/code.rs" "$t/$out/linked.rs" && extra=$s/$out/linked.rs: || exit 98 ;;
      esac
      printf '%s: crates/halvo-wire/src/lib.rs\n\ncrates/halvo-wire/src/lib.rs:\ncrates/halvo-wire/src/../build.rs:\n%s/%s/bounds.rs:\n%s\n# env-dep:OUT_DIR=%s/%s\n# env-dep:STUB=../../../../../../../..\n' "${STUB_HEAD:-$s/debug/deps/${STUB_HEAD_IN_TARGET:-halvo_wire-0.d}}" "$s" "$out" "$extra" "$s" "$out" >"$t/debug/deps/halvo_wire-0.d" || exit 98
      if [ -z "${STUB_SKIP_SECRETS:-}" ]; then
        printf '%s/debug/deps/halvo_secrets-0.d: crates/halvo-secrets/src/lib.rs\n\ncrates/halvo-secrets/src/lib.rs:\n' "$s" >"$t/debug/deps/halvo_secrets-0.d" || exit 98
      fi
      printf '%s/debug/deps/halvo_account-0.d: crates/halvo-account/src/lib.rs\n\ncrates/halvo-account/src/lib.rs:\n' "$s" >"$t/debug/deps/halvo_account-0.d" || exit 98
      printf '%s/debug/deps/itoa-0.d: /registry/itoa/src/lib.rs\n\n/registry/itoa/src/lib.rs:\n' "$t" >"$t/debug/deps/itoa-0.d" || exit 98
      [ -z "${STUB_UNREADABLE:-}" ] || chmod 000 "$t/debug/deps/halvo_wire-0.d" || exit 98
    fi
    exit "${STUB_BUILD_EXIT:-0}"
    ;;
  test)
    case " $* " in
      *' --test hosted '*)
        case $* in
          *hosted-store-tests,keychain*) store=keychain ;;
          *)
            if [ "$CI_D_JOB" = windows ]; then
              store=dpapi
            else
              store=file
            fi
            ;;
        esac
        store=${STUB_HOSTED_STORE:-$store}
        [ -n "${STUB_HOSTED_SILENT:-}" ] || printf 'test the_store_of_this_build ... \n%sstore=%s\nok\n' "${STUB_HOSTED_PREFIX:-}" "$store"
        exit "${STUB_HOSTED_EXIT:-0}"
        ;;
    esac
    [ -z "${STUB_TEST_WRITES:-}" ] || printf 'x\n' >>"$STUB_TEST_WRITES"
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

deny_json() {
  printf '%s\n' "$outside_licences" "$@" | while IFS=' ' read -r crate version; do
    [ -n "$crate" ] || continue
    printf '{"fields":{"code":"rejected","graphs":[{"Krate":{"name":"%s","version":"%s"},"parents":[]}],"labels":[{"column":12,"line":26,"message":"","span":"LicenseRef-fixture"}],"message":"failed to satisfy license requirements","severity":"error"},"type":"diagnostic"}\n' "$crate" "$version"
  done
  printf '{"fields":{"licenses":{"errors":0,"helps":0,"notes":0,"warnings":0}},"type":"summary"}\n'
}

git_fixture() {
  git -C "$fixture" init -q >/dev/null 2>&1 && git -C "$fixture" add -A >/dev/null 2>&1
}

make_fixture() {
  fixture=$work/$1
  if ! takes_case "$1"; then
    left_out "$1"
    return 1
  fi
  mkdir "$fixture" "$fixture/account" || return 1
  for entry in Cargo.toml Cargo.lock deny.toml schema crates; do
    cp -R "$repo/account/$entry" "$fixture/account/$entry" || return 1
  done
  mkdir "$fixture/scripts" && cp "$repo/scripts/prerequisites.sh" "$fixture/scripts/prerequisites.sh" || return 1
  mkdir "$fixture/outside" && printf 'pub fn included() {}\n' >"$fixture/outside/code.rs"
}

write_stand_in() {
  mkdir "$1" || return 1
  cat >"$1/cygpath" <<'EOF' || return 1
#!/bin/sh
case $1 in
  -u)
    [ "$2" = -f ] || exit 2
    awk 'BEGIN { root = ENVIRON["STAND_IN_DRIVE"] } { line = $0; gsub(/\\/, "/", line); if (line ~ /^[Zz]:\//) line = root substr(line, 3); print line }' "$3"
    ;;
  -w)
    rest=${2#"$STAND_IN_DRIVE"}
    [ "$rest" != "$2" ] || exit 1
    printf 'Z:%s\n' "$(printf '%s' "$rest" | tr '/' '\\')"
    ;;
  *) exit 2 ;;
esac
EOF
  chmod +x "$1/cygpath"
}

windows_name() {
  if command -v cygpath >/dev/null 2>&1; then
    cygpath -w "$1"
  else
    STAND_IN_DRIVE=$work "$work/stand-in/cygpath" -w "$1"
  fi
}

edit() {
  file=$fixture/account/$1
  shift
  if ! "$@" <"$file" >"$file.edited"; then
    wrong="$wrong ${fixture##*/}"
    return 1
  fi
  if cmp -s "$file" "$file.edited"; then
    printf 'the edit of %s changed nothing\n' "${file#"$work"/}"
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

make_probe_fixture() {
  fixture=$work/$1
  mkdir "$fixture" "$fixture/account" "$fixture/outside" "$fixture/scripts" || return 1
  for entry in Cargo.toml deny.toml schema; do
    cp -R "$repo/account/$entry" "$fixture/account/$entry" || return 1
  done
  cp "$repo/scripts/prerequisites.sh" "$fixture/scripts/prerequisites.sh" || return 1
  write_crate "$fixture/account/crates/probe" probe || return 1
  mkdir "$fixture/account/crates/probe/inner" || return 1
  printf 'pub fn outside() {}\n' >"$fixture/outside/module.rs" || return 1
  printf 'pub fn included() {}\n' >"$fixture/outside/code.rs" || return 1
  printf 'text\n' >"$fixture/outside/text.txt" || return 1
  printf 'bytes\n' >"$fixture/outside/bytes.bin" || return 1
  printf 'pub fn inner() {}\n' >"$fixture/account/crates/probe/inner/module.rs" || return 1
  printf 'text\n' >"$fixture/account/crates/probe/inner/text.txt" || return 1
  printf '%s\n' "$2" >"$fixture/account/crates/probe/src/lib.rs" || return 1
  relock
}

write_probe_build_script() {
  printf 'fn main() {\n    let out = std::env::var("OUT_DIR").unwrap();\n    std::fs::write(std::path::Path::new(&out).join("generated.rs"), "pub fn generated() {}\\n").unwrap();\n}\n' >"$fixture/account/crates/probe/build.rs"
}

build_probe() {
  (cd "$fixture" && CARGO_TARGET_DIR=$fixture/target/account "$real_cargo" build --offline --manifest-path account/Cargo.toml >/dev/null 2>&1)
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

in_list() {
  for listed in $2; do
    [ "$listed" = "$1" ] && return 0
  done
  return 1
}

takes_case() {
  in_list "$1" "$windows_cases" && return 0
  [ "${2-$job}" != windows ]
}

all_taken() {
  for listed_case in $2; do
    takes_case "$listed_case" "$1" || return 1
  done
}

none_taken() {
  for listed_case in $2; do
    takes_case "$listed_case" "$1" && return 1
  done
  return 0
}

left_out() {
  selected "$1" || return 0
  printf 'case %s/%s not run: the windows job leaves it to the macos job\n' "$name" "$1"
  left=$((left + 1))
}

note_case() {
  ran="$ran $1"
  in_list "$1" "$windows_cases $macos_cases" || unlisted="$unlisted $1"
}

cases_missing() {
  case $1 in
    windows) required=$windows_cases ;;
    macos) required="$windows_cases $macos_cases" ;;
    *) required= ;;
  esac
  for required_case in $required; do
    [ "$1" = windows ] && [ "$required_case" = link-out-of-account ] && continue
    selected "$required_case" || continue
    in_list "$required_case" "$2" || printf ' %s' "$required_case"
  done
}

split_verdict() {
  # $1 the job that the self-test read, $2 the driver's job, $3 the cases that ran, $4 the
  # cases noted on no list, $5 the count of cases left out. Prints one word after a space for
  # each fault and nothing for right values.
  missing=$(cases_missing "$1" "$3") || {
    printf ' cases-missing-failed'
    return 1
  }
  for missing_case in $missing; do
    printf ' not-run:%s' "$missing_case"
  done
  for unlisted_case in $4; do
    printf ' on-no-list:%s' "$unlisted_case"
  done
  expected_left=0
  if [ "$1" = windows ]; then
    for listed_case in $macos_cases; do
      selected "$listed_case" && expected_left=$((expected_left + 1))
    done
  fi
  [ "$5" -eq "$expected_left" ] || printf ' left-out-count:%s-not-%s' "$5" "$expected_left"
  [ "$1" = "$2" ] || printf ' job-not-read'
}

end_verdict() {
  # The verdict of this run's split from the values that the run holds: the job read at the
  # start, the driver's job, the cases that ran, the cases noted on no list and the count left
  # out. Harness cases call it through end_verdict_of with values of their own.
  split_verdict "$job" "${CI_D_JOB:-}" "$ran" "$unlisted" "$left"
}

end_verdict_of() {
  # Calls end_verdict with the five values of a harness case in place of the run's own, in
  # this shell, and puts the run's own back. The words land in verdict.
  held_job=$job
  held_driver=${CI_D_JOB:-}
  held_ran=$ran
  held_unlisted=$unlisted
  held_left=$left
  job=$1
  CI_D_JOB=$2
  ran=$3
  unlisted=$4
  left=$5
  verdict=$(R7_ACCOUNT_CASES='' end_verdict)
  code=$?
  job=$held_job
  CI_D_JOB=$held_driver
  ran=$held_ran
  unlisted=$held_unlisted
  left=$held_left
  return "$code"
}

runs_here() {
  selected "$1" || return 1
  takes_case "$1" && return 0
  left_out "$1"
  return 1
}

split_holds() {
  case $1 in
    macos | windows) ;;
    *) return 1 ;;
  esac
  missing=$(cases_missing "$1" "$2") || return 1
  [ -z "$missing" ] && return 0
  printf 'the %s job did not run%s\n' "$1" "$missing"
  return 1
}

job_real_builds() {
  case $1 in
    macos) real_builds=$real_builds_macos ;;
    windows) real_builds=$real_builds_windows ;;
    *) real_builds= ;;
  esac
  [ -n "$real_builds" ]
}

no_real_build() {
  [ "$1" = whole ] && [ "$2" -eq 0 ]
}

real_build() {
  case " $real_builds " in
    *" $1 "*) return 0 ;;
  esac
  left_out "$1"
  return 1
}

run_case() {
  case=$1
  runs_here "$case" || return 0
  note_case "$case"
  want=$2
  mode=$3
  shift 3
  case " $* " in
    *' STUB_BUILD_REAL=1 '*) real_runs=$((real_runs + 1)) ;;
  esac
  if [ "$mode" = stub ]; then
    path=$stub:$PATH
  else
    path=$PATH
  fi
  case " $* " in
    *' STAND_IN_CYGPATH=1 '*) command -v cygpath >/dev/null 2>&1 || path=$work/stand-in:$path ;;
  esac
  out=$(env -u GITHUB_ACTIONS -u RUNNER_ENVIRONMENT PATH="$path" CI_D_REPO="$fixture" CI_D_ARTIFACTS="$fixture" STUB_DENY_VERSION="$pin" STUB_REAL_CARGO="$real_cargo" STUB_DENY_OUTSIDE="$outside_default" STUB_LOG="$fixture/cargo-calls.txt" STAND_IN_DRIVE="$work" "$@" sh "$self" 2>&1 </dev/null)
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
      *"${nl}account packages=${want_packages:-$packages} vendored=4 of 4 bans=$bans tests=3 cargo-deny=$pin found dep-info=${want_dep_info:-3}$nl"*) ;;
      *) got="$got, without its counts line" ;;
    esac
    calls=$(cat "$fixture/cargo-calls.txt" 2>/dev/null)
    [ "$calls" = "$want_calls" ] || got="$got, with other cargo calls"
  fi
  printf 'case %s/%s want=%s got=%s\n' "$name" "$case" "$want" "$got"
  if [ "$got" != "$want" ]; then
    wrong="$wrong $case"
    printf '%s\n' "$out" | grep -E '^(read outside account/|a dep-info file of account/)'
  elif [ "$want" != "PASS $name" ]; then
    rejected=$((rejected + 1))
  fi
}

stub_case() {
  runs_here "$1" || return 0
  make_fixture "$1" || fail "cannot make the fixture $1"
  run_case "$@"
}

harness_case() {
  selected "$1" || return 0
  if ! takes_case "$1"; then
    left_out "$1"
    return 0
  fi
  note_case "$1"
  printf 'case %s/%s want=refused got=%s\n' "$name" "$1" "$2"
  if [ "$2" = refused ]; then
    rejected=$((rejected + 1))
  else
    wrong="$wrong $1"
  fi
}

self_test() {
  case ${CI_D_JOB:-} in
    macos | windows) ;;
    *) fail 'CI_D_JOB names neither macos nor windows' ;;
  esac
  job=$CI_D_JOB
  split=' split-not-checked'
  ran=
  left=0
  unlisted=
  enter_repository
  real_runs=0
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
  write_stand_in "$work/stand-in" || fail 'cannot write the stand-in cygpath'
  calls_before="deny --version
fetch --locked --offline --manifest-path account/Cargo.toml
metadata --locked --offline --format-version 1 --manifest-path account/Cargo.toml
deny --offline --locked --manifest-path account/Cargo.toml check bans licenses sources
deny --format json --color never --config CONFIG --offline --locked --manifest-path account/Cargo.toml check licenses
clean --offline$members --manifest-path account/Cargo.toml
build --manifest-path account/Cargo.toml --locked --offline
test --manifest-path account/Cargo.toml --locked --offline"
  calls_after="fmt --manifest-path account/Cargo.toml --all --check
clippy --manifest-path account/Cargo.toml --all-targets --all-features --locked --offline -- -D warnings"
  keychain_call='test --manifest-path account/Cargo.toml --locked --offline -p halvo-secrets --features hosted-store-tests,keychain --test hosted -- --test-threads=1 --nocapture'
  file_call='test --manifest-path account/Cargo.toml --locked --offline -p halvo-secrets --features hosted-store-tests --test hosted -- --test-threads=1 --nocapture'
  want_calls="$calls_before$nl$calls_after"
  cause_deny="cargo deny --version is not cargo-deny $pin; sh scripts/prerequisites.sh installs it"
  cause_fetch="cargo fetch --locked --offline exited 101; sh scripts/prerequisites.sh fetches the packages of account/Cargo.lock"
  cause_record='account/schema/vendored.txt does not name the four vendored files'
  cause_commit='account/schema/vendored.txt does not start with the line of its commit'
  cause_config='a cargo configuration file sits at the repository root or under account/'
  cause_admitted='account/deny.toml differs from the configuration that this script admits'
  cause_ban='account/deny.toml holds a ban that this script does not admit'
  cause_path="account/'s metadata holds a path outside account/"
  cause_name="account/'s metadata holds a package whose name contains sqlite or sqlcipher"
  cause_outside='the packages of account/Cargo.lock that no target of Halvo compiles and whose licence is outside the allow list differ from outside_licences of this script; read each and list it there'
  outside_default=$work/outside-default.json
  deny_json >"$outside_default" || fail 'cannot write in the temp directory'
  cause_source='a source of account/ reads a file outside account/'
  cause_no_dep_info='cargo wrote no dep-info file of a package of account/'
  stale_fix='cargo clean --manifest-path account/Cargo.toml --target-dir target/account clears it'
  cause_unopenable="the target holds a dep-info file that this check cannot open; $stale_fix"
  cause_elsewhere="the target holds dep-info that this build did not write, whose rule names another place; $stale_fix"
  cause_missing="the target holds dep-info that this build did not write, which lists a file that is not there; $stale_fix"
  cause_link='a source of account/ reads a file through a symbolic link'
  cause_member='a package of account/ has no dep-info file that this check read'
  cause_tracked='a tracked file lies under the build target directory target/account'
  cause_control='a dep-info file of account/ names a file with a tab or another control character, which this check cannot read without doubt'
  escape=$(printf '\033')
  inside_source='include!(concat!(env!("OUT_DIR"), "/generated.rs"));
#[path = "../inner/module.rs"]
pub mod inner;
pub const TEXT: &str = include_str!("../inner/text.txt");'
  probe_calls="deny --version
fetch --locked --offline --manifest-path account/Cargo.toml
metadata --locked --offline --format-version 1 --manifest-path account/Cargo.toml
deny --offline --locked --manifest-path account/Cargo.toml check bans licenses sources
deny --format json --color never --config CONFIG --offline --locked --manifest-path account/Cargo.toml check licenses
clean --offline -p probe --manifest-path account/Cargo.toml
build --manifest-path account/Cargo.toml --locked --offline
test --manifest-path account/Cargo.toml --locked --offline
$calls_after"

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
  want_calls="$calls_before$nl$calls_after"
  stub_case self-hosted-runner "PASS $name" stub GITHUB_ACTIONS=true RUNNER_ENVIRONMENT=self-hosted CI_D_JOB=macos
  stub_case actions-not-true "PASS $name" stub GITHUB_ACTIONS=1 RUNNER_ENVIRONMENT=github-hosted CI_D_JOB=macos
  want_calls="$calls_before$nl$keychain_call$nl$file_call$nl$calls_after"
  stub_case hosted-macos "PASS $name" stub GITHUB_ACTIONS=true RUNNER_ENVIRONMENT=github-hosted CI_D_JOB=macos
  want_calls="$calls_before$nl$file_call$nl$calls_after"
  stub_case hosted-windows "PASS $name" stub GITHUB_ACTIONS=true RUNNER_ENVIRONMENT=github-hosted CI_D_JOB=windows
  want_calls="$calls_before$nl$calls_after"
  stub_case hosted-test-fails "FAIL $name (the hosted test of store=keychain exited 101)" stub GITHUB_ACTIONS=true RUNNER_ENVIRONMENT=github-hosted CI_D_JOB=macos STUB_HOSTED_EXIT=101
  stub_case hosted-line-missing "FAIL $name (the hosted test printed no line store=keychain)" stub GITHUB_ACTIONS=true RUNNER_ENVIRONMENT=github-hosted CI_D_JOB=macos STUB_HOSTED_SILENT=1
  stub_case hosted-line-of-another-store "FAIL $name (the hosted test printed no line store=keychain)" stub GITHUB_ACTIONS=true RUNNER_ENVIRONMENT=github-hosted CI_D_JOB=macos STUB_HOSTED_STORE=file
  stub_case hosted-file-line-missing "FAIL $name (the hosted test printed no line store=file)" stub GITHUB_ACTIONS=true RUNNER_ENVIRONMENT=github-hosted CI_D_JOB=macos STUB_HOSTED_STORE=keychain
  stub_case hosted-line-inside-another "FAIL $name (the hosted test printed no line store=dpapi)" stub GITHUB_ACTIONS=true RUNNER_ENVIRONMENT=github-hosted CI_D_JOB=windows STUB_HOSTED_PREFIX=x
  stub_case hosted-windows-line-missing "FAIL $name (the hosted test printed no line store=dpapi)" stub GITHUB_ACTIONS=true RUNNER_ENVIRONMENT=github-hosted CI_D_JOB=windows STUB_HOSTED_SILENT=1
  stub_case hosted-other-job "FAIL $name (CI_D_JOB names neither macos nor windows)" stub GITHUB_ACTIONS=true RUNNER_ENVIRONMENT=github-hosted CI_D_JOB=linux
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
  make_fixture record-line-with-another-name && edit schema/vendored.txt sed 's/ schema\.v1\.json / schema.v2.json /' &&
    run_case record-line-with-another-name "FAIL $name ($cause_record)" stub
  make_fixture record-line-with-another-path && edit schema/vendored.txt sed 's| contract/routes/v1\.json$| contract/routes/v2.json|' &&
    run_case record-line-with-another-path "FAIL $name ($cause_record)" stub

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
  make_fixture escaped-build-key && printf 'fn main() {}\n' >"$fixture/outside/build.rs" &&
    edit crates/halvo-wire/Cargo.toml awk 'BEGIN { while ((getline line) > 0) { print line; if (line == "publish = false") print "\"\\u0062uild\" = \"../../../outside/build.rs\"" } }' &&
    run_case escaped-build-key "FAIL $name ($cause_path)" stub
  make_fixture path-outside-account && write_crate "$fixture/outside-crate" outside &&
    edit crates/halvo-wire/Cargo.toml awk 'BEGIN { while ((getline line) > 0) { print line; if (line == "[dependencies]") print "outside = { path = \"../../../outside-crate\" }" } }' &&
    relock && run_case path-outside-account "FAIL $name ($cause_path)" stub
  make_fixture manifest-outside-account && write_crate "$fixture/outside-crate" outside &&
    printf '[lib]\npath = "../account/sources/lib.rs"\n' >>"$fixture/outside-crate/Cargo.toml" && mkdir "$fixture/account/sources" && : >"$fixture/account/sources/lib.rs" &&
    edit crates/halvo-wire/Cargo.toml awk 'BEGIN { while ((getline line) > 0) { print line; if (line == "[dependencies]") print "outside = { path = \"../../../outside-crate\" }" } }' &&
    relock && run_case manifest-outside-account "FAIL $name ($cause_path)" stub
  if runs_here target-through-a-link; then
    make_fixture target-through-a-link && ln -s account "$fixture/link-to-account" 2>/dev/null
    if [ -L "$fixture/link-to-account" ]; then
      edit crates/halvo-wire/Cargo.toml awk '{ print } END { print ""; print "[[test]]"; print "name = \"through_a_link\""; print "path = \"../../../link-to-account/crates/halvo-wire/tests/request.rs\"" }' &&
        relock && run_case target-through-a-link "FAIL $name ($cause_path)" stub
    else
      printf 'case %s/target-through-a-link not run: this system made no symbolic link\n' "$name"
    fi
  fi
  make_fixture respelled-lock-entry && write_crate "$fixture/account/crates/sqlite-shim" sqlite-shim && relock &&
    edit Cargo.lock sed "s/^name = \"sqlite-shim\"\$/name = 'sqlite-shim'/" &&
    run_case respelled-lock-entry "FAIL $name ($cause_name)" stub

  if takes_case metadata-source-outside-crates-io; then
    make_fixture metadata-base
    base=$work/metadata-base.json
    (cd "$fixture" && "$real_cargo" metadata --locked --offline --format-version 1 --manifest-path account/Cargo.toml >"$base" 2>/dev/null)
    sed 's|"source":"registry+https://github.com/rust-lang/crates.io-index"|"source":"git+https://example.invalid/x.git#0"|' <"$base" >"$work/source.json"
    sed 's|"name":"itoa","version":|"name":"LibSQLite3-ffi","version":|' <"$base" >"$work/sqlite.json"
    sed 's|"name":"itoa","version":|"name":"SQLCipher-sys","version":|' <"$base" >"$work/sqlcipher.json"
    printf '{"packages":[],"workspace_members":[],"resolve":null,"version":1}\n' >"$work/empty.json"
    for crafted in source sqlite sqlcipher; do
      cmp -s "$base" "$work/$crafted.json" && wrong="$wrong metadata-$crafted"
    done
  fi
  run_case metadata-source-outside-crates-io "FAIL $name (account/'s metadata holds a package from a source outside crates.io)" stub STUB_METADATA="$work/source.json"
  run_case metadata-name-with-sqlite-in-mixed-case "FAIL $name ($cause_name)" stub STUB_METADATA="$work/sqlite.json"
  run_case metadata-name-with-sqlcipher "FAIL $name ($cause_name)" stub STUB_METADATA="$work/sqlcipher.json"
  run_case metadata-without-a-package "FAIL $name (account/'s metadata holds no package)" stub STUB_METADATA="$work/empty.json"

  stub_case dep-info-absent "FAIL $name ($cause_no_dep_info)" stub STUB_NO_DEP_INFO=1
  stub_case dep-info-path-outside "FAIL $name ($cause_source)" stub STUB_DEP='crates/halvo-wire/src/../../../../outside/code.rs'
  runs_here dep-info-absolute-path-outside && make_fixture dep-info-absolute-path-outside &&
    run_case dep-info-absolute-path-outside "FAIL $name ($cause_source)" stub STUB_DEP="$fixture/outside/code.rs"
  stub_case dep-info-windows-separators "FAIL $name ($cause_source)" stub STUB_DEP='crates\halvo-wire\src\..\..\..\..\outside\code.rs' STAND_IN_CYGPATH=1
  make_fixture dep-info-windows-drive && drive_name=$(windows_name "$fixture/outside/code.rs") &&
    run_case dep-info-windows-drive "FAIL $name ($cause_source)" stub STUB_DEP="$drive_name" STAND_IN_CYGPATH=1
  stub_case dep-info-missing-file "FAIL $name ($cause_missing)" stub STUB_DEP='crates/halvo-wire/src/gone.rs'
  stub_case dep-info-missing-directory "FAIL $name ($cause_missing)" stub STUB_DEP='crates/halvo-wire/gone/lib.rs'
  stub_case dep-info-head-elsewhere "FAIL $name ($cause_elsewhere)" stub STUB_HEAD=crates/halvo-wire/Cargo.toml
  if runs_here dep-info-unopenable; then
    make_fixture dep-info-unopenable && probe_file=$fixture/mode-000 && : >"$probe_file" && chmod 000 "$probe_file"
    if [ -r "$probe_file" ]; then
      printf 'case %s/dep-info-unopenable not run: this system reads a file of mode 000\n' "$name"
    else
      run_case dep-info-unopenable "FAIL $name ($cause_unopenable)" stub STUB_UNREADABLE=1
    fi
  fi
  stub_case member-without-dep-info "FAIL $name ($cause_member)" stub STUB_SKIP_SECRETS=1
  runs_here dep-info-name-starting-with-a-hash && make_fixture dep-info-name-starting-with-a-hash && mkdir -p "$fixture/account/#m/src" &&
    run_case dep-info-name-starting-with-a-hash "FAIL $name ($cause_source)" stub STUB_DEP='#m/src/../../../outside/code.rs'
  if runs_here dep-info-link-then-dot-dot; then
    make_fixture dep-info-link-then-dot-dot && mkdir -p "$fixture/elsewhere/d" "$fixture/elsewhere/account/crates/halvo-wire/src" &&
      : >"$fixture/elsewhere/account/crates/halvo-wire/src/lib.rs" && ln -s elsewhere/d "$fixture/sneak" 2>/dev/null
    if [ -L "$fixture/sneak" ]; then
      run_case dep-info-link-then-dot-dot "FAIL $name ($cause_source)" stub STUB_DEP='crates/halvo-wire/src/../../../../sneak/../account/crates/halvo-wire/src/lib.rs'
    else
      printf 'case %s/dep-info-link-then-dot-dot not run: this system made no symbolic link\n' "$name"
    fi
  fi
  if runs_here link-as-account-target; then
    make_fixture link-as-account-target && mkdir "$fixture/elsewhere" && ln -s ../elsewhere "$fixture/account/target" 2>/dev/null
    if [ -L "$fixture/account/target" ]; then
      run_case link-as-account-target "FAIL $name (account/ holds a symbolic link)" stub
    else
      printf 'case %s/link-as-account-target not run: this system made no symbolic link\n' "$name"
    fi
  fi
  stub_case dep-info-link-in-a-source-directory "FAIL $name ($cause_link)" stub STUB_LINK_IN=src
  stub_case dep-info-link-in-an-out-directory "FAIL $name ($cause_link)" stub STUB_LINK_IN=out
  make_fixture dep-info-backslash-in-a-directory-name && mkdir "$fixture/account/crates/halvo-wire/src/a\\b" &&
    run_case dep-info-backslash-in-a-directory-name "FAIL $name ($cause_source)" stub STUB_DEP='crates/halvo-wire/src/a\b/../../../../../outside/code.rs'
  make_fixture dep-info-letter-and-colon-directory && mkdir -p "$fixture/account/c:x/src" &&
    run_case dep-info-letter-and-colon-directory "FAIL $name ($cause_source)" stub STUB_DEP='c:x/src/../../../outside/code.rs'
  stub_case dep-info-target-outside-out "FAIL $name ($cause_source)" stub STUB_TARGET_DEP=debug/deps/code.rs
  stub_case dep-info-file-beside-out "FAIL $name ($cause_source)" stub STUB_TARGET_DEP=debug/build/halvo-wire-0/other.rs
  stub_case dep-info-in-another-out-directory "FAIL $name ($cause_source)" stub STUB_TARGET_DEP=debug/build/fake-0/out/code.rs
  stub_case dep-info-out-directory-without-build "FAIL $name ($cause_source)" stub STUB_OUT_DIR=debug/notbuild/halvo-wire-0/out
  stub_case dep-info-out-directory-not-named-out "FAIL $name ($cause_source)" stub STUB_OUT_DIR=debug/build/halvo-wire-0/notout
  stub_case dep-info-out-directory-two-levels-deep "FAIL $name ($cause_source)" stub STUB_OUT_DIR=debug/extra/build/halvo-wire-0/out
  stub_case dep-info-head-names-a-missing-file "FAIL $name ($cause_elsewhere)" stub STUB_HEAD_IN_TARGET=gone.d
  make_fixture tracked-file-under-the-target && mkdir -p "$fixture/target/account/debug/build/fake-0/out" &&
    cp "$fixture/outside/code.rs" "$fixture/target/account/debug/build/fake-0/out/code.rs" &&
    git -C "$fixture" init -q >/dev/null 2>&1 && git -C "$fixture" add -f target/account/debug/build/fake-0/out/code.rs >/dev/null 2>&1 &&
    run_case tracked-file-under-the-target "FAIL $name ($cause_tracked)" stub
  runs_here dep-info-in-a-sibling-of-account && make_fixture dep-info-in-a-sibling-of-account && mkdir "$fixture/account-tools" && cp "$fixture/outside/code.rs" "$fixture/account-tools/code.rs" &&
    run_case dep-info-in-a-sibling-of-account "FAIL $name ($cause_source)" stub STUB_DEP='crates/halvo-wire/src/../../../../account-tools/code.rs'
  make_fixture metadata-path-in-a-sibling-of-account && write_crate "$fixture/account-tools/sibling" sibling &&
    edit crates/halvo-wire/Cargo.toml awk 'BEGIN { while ((getline line) > 0) { print line; if (line == "[dependencies]") print "sibling = { path = \"../../../account-tools/sibling\" }" } }' &&
    relock && run_case metadata-path-in-a-sibling-of-account "FAIL $name ($cause_path)" stub
  runs_here dep-info-escaped-space && make_fixture dep-info-escaped-space && mkdir "$fixture/account/crates/halvo-wire/s " &&
    run_case dep-info-escaped-space "FAIL $name ($cause_source)" stub STUB_DEP='crates/halvo-wire/s\ /../../../../outside/code.rs'
  stub_case dep-info-name-with-a-colon-and-a-newline "FAIL $name ($cause_elsewhere)" stub STUB_DEP="crates/halvo-wire/src/../../../../x: y${nl}z.txt"
  if runs_here repository-through-a-link; then
    make_fixture repository-through-a-link && mkdir -p "$fixture/target/account/debug/build/halvo-wire-0/out" && ln -s "${fixture##*/}" "$work/link-to-the-repository" 2>/dev/null
    if [ -L "$work/link-to-the-repository" ]; then
      run_case repository-through-a-link "PASS $name" stub CI_D_REPO="$work/link-to-the-repository"
    else
      printf 'case %s/repository-through-a-link not run: this system made no symbolic link\n' "$name"
    fi
  fi
  make_fixture target-in-another-spelling && mkdir -p "$fixture/target/account/debug/build/halvo-wire-0/out"
  spelling=
  if command -v cygpath >/dev/null 2>&1; then
    spelling=$(cygpath -m "$fixture/target/account") || spelling=
  elif ln -s "${fixture##*/}/target/account" "$work/another-spelling-of-the-target" 2>/dev/null; then
    spelling=$work/another-spelling-of-the-target
  fi
  if [ -n "$spelling" ]; then
    run_case target-in-another-spelling "PASS $name" stub STUB_TARGET_SPELLING="$spelling"
  else
    printf 'case %s/target-in-another-spelling could not name the target in another spelling\n' "$name"
    wrong="$wrong target-in-another-spelling"
  fi
  make_fixture target-in-the-drive-spelling && mkdir -p "$fixture/target/account" && drive_target=$(windows_name "$fixture/target/account") &&
    run_case target-in-the-drive-spelling "PASS $name" stub STUB_TARGET_SPELLING="$drive_target" STAND_IN_CYGPATH=1
  stub_case dep-info-name-with-a-newline "FAIL $name ($cause_source)" stub STUB_DEP="crates/halvo-wire/src/../../../../x${nl}y.txt"
  stub_case dep-info-name-with-a-tab "FAIL $name ($cause_control)" stub STUB_DEP="crates/halvo-wire/src/a${tab}b/../../../../outside/code.rs"
  stub_case dep-info-name-with-a-control-character "FAIL $name ($cause_control)" stub STUB_DEP="crates/halvo-wire/src/a${escape}b/../../../../outside/code.rs"
  job_real_builds "$job" || wrong="$wrong real-builds-of-the-$job-job"
  want_packages=1
  real_build path-attribute-outside && make_probe_fixture path-attribute-outside '#[path = "../../../../outside/module.rs"]
pub mod outside;' && run_case path-attribute-outside "FAIL $name ($cause_source)" stub STUB_BUILD_REAL=1
  real_build include-outside && make_probe_fixture include-outside 'include!("../../../../outside/code.rs");' &&
    run_case include-outside "FAIL $name ($cause_source)" stub STUB_BUILD_REAL=1
  real_build include-str-outside && make_probe_fixture include-str-outside 'pub const TEXT: &str = include_str!("../../../../outside/text.txt");' &&
    run_case include-str-outside "FAIL $name ($cause_source)" stub STUB_BUILD_REAL=1
  real_build include-bytes-outside && make_probe_fixture include-bytes-outside 'pub const BYTES: &[u8] = include_bytes!("../../../../outside/bytes.bin");' &&
    run_case include-bytes-outside "FAIL $name ($cause_source)" stub STUB_BUILD_REAL=1
  real_build include-from-the-target-directory && make_probe_fixture include-from-the-target-directory 'include!("../../../../target/account/code.rs");' &&
    mkdir -p "$fixture/target/account" && cp "$fixture/outside/code.rs" "$fixture/target/account/code.rs" &&
    run_case include-from-the-target-directory "FAIL $name ($cause_source)" stub STUB_BUILD_REAL=1
  want_dep_info=2
  want_calls=$probe_calls
  real_build includes-inside && make_probe_fixture includes-inside "$inside_source" && write_probe_build_script &&
    run_case includes-inside "PASS $name" stub STUB_BUILD_REAL=1
  if real_build moved-tree && make_probe_fixture moved-tree-before-the-move "$inside_source" && write_probe_build_script && build_probe &&
    mv "$fixture" "$work/moved-tree" && fixture=$work/moved-tree; then
    run_case moved-tree "PASS $name" stub STUB_BUILD_REAL=1
    if real_build moved-tree-with-an-outside-include; then
      printf 'include!("../../../../outside/code.rs");\n' >>"$fixture/account/crates/probe/src/lib.rs"
      run_case moved-tree-with-an-outside-include "FAIL $name ($cause_source)" stub STUB_BUILD_REAL=1
    fi
  elif ! takes_case moved-tree-with-an-outside-include; then
    left_out moved-tree-with-an-outside-include
  fi
  want_dep_info=1
  if real_build source-put-right && make_probe_fixture source-put-right 'pub const TEXT: &str = include_str!("../../../../outside/text.txt");' && build_probe &&
    printf 'pub const TEXT: &str = include_str!("../inner/text.txt");\n' >"$fixture/account/crates/probe/src/lib.rs" &&
    edit crates/probe/Cargo.toml sed 's/^version = "0.1.0"$/version = "0.1.1"/' && relock; then
    run_case source-put-right "PASS $name" stub STUB_BUILD_REAL=1
  fi
  want_packages=2
  if real_build member-named-with-a-hash && make_probe_fixture member-named-with-a-hash '' && write_crate "$fixture/account/#m" hashm &&
    printf 'pub const TEXT: &str = include_str!("../../../outside/text.txt");\n' >"$fixture/account/#m/src/lib.rs" &&
    edit Cargo.toml sed 's|^members = \["crates/\*"\]$|members = ["crates/*", "#m"]|' && relock; then
    run_case member-named-with-a-hash "FAIL $name ($cause_source)" stub STUB_BUILD_REAL=1
  fi
  want_packages=1
  real_build directory-name-with-a-tab && make_probe_fixture directory-name-with-a-tab 'pub const TEXT: &str = include_str!("a\tb/../../../../../outside/text.txt");' &&
    mkdir "$fixture/account/crates/probe/src/a${tab}b" && mkdir -p "$fixture/account/crates/probe/src/a/b" &&
    cp "$fixture/outside/text.txt" "$fixture/account/outside${tab}text.txt" &&
    run_case directory-name-with-a-tab "FAIL $name ($cause_control)" stub STUB_BUILD_REAL=1
  want_calls=$calls_before$nl$calls_after
  want_packages=
  want_dep_info=

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
  make_fixture ban-with-a-third-key && edit deny.toml sed 's/^    { crate = "rusqlite", reason = ".*" },$/    { crate = "rusqlite", reason = "SQLite", use-instead = "none" },/' &&
    run_case ban-with-a-third-key "FAIL $name ($cause_ban)" stub
  make_fixture ban-reason-with-a-backslash && edit deny.toml sed 's/{ crate = "rusqlite", reason = "SQLite: bindings/{ crate = "rusqlite", reason = "SQLite:\\tbindings/' &&
    run_case ban-reason-with-a-backslash "FAIL $name ($cause_ban)" stub
  make_fixture ban-line-cut-after-its-reason && edit deny.toml sed 's/^\(    { crate = "rusqlite", reason = ".*\)" },$/\1/' &&
    run_case ban-line-cut-after-its-reason "FAIL $name ($cause_ban)" stub
  make_fixture ban-deleted && edit deny.toml sed '/{ crate = "sentry", /d' &&
    run_case ban-deleted "FAIL $name (account/deny.toml lacks the ban of sentry)" stub
  make_fixture ban-renamed && edit deny.toml sed 's/{ crate = "libsqlite3-sys"/{ crate = "libsqlite4-sys"/' &&
    run_case ban-renamed "FAIL $name (account/deny.toml lacks the ban of libsqlite3-sys)" stub
  make_fixture deny-targets-fifth && edit deny.toml sed 's/"aarch64-pc-windows-msvc"\]$/"aarch64-pc-windows-msvc", "x86_64-unknown-linux-gnu"]/' &&
    run_case deny-targets-fifth "FAIL $name ($cause_admitted)" stub
  make_fixture deny-targets-lack-one && edit deny.toml sed 's/, "aarch64-pc-windows-msvc"\]$/]/' &&
    run_case deny-targets-lack-one "FAIL $name ($cause_admitted)" stub
  make_fixture deny-targets-gone && edit deny.toml sed '/^targets = \[/d' &&
    run_case deny-targets-gone "FAIL $name ($cause_admitted)" stub
  deny_json 'another-crate 1.0.0' >"$work/outside-another.json"
  stub_case outside-licence-another "FAIL $name ($cause_outside)" stub STUB_DENY_OUTSIDE="$work/outside-another.json" STUB_OUTSIDE_EXIT=4
  printf '{"fields":{"code":"rejected","graphs":[],"severity":"error"},"type":"diagnostic"}\n' >"$work/outside-unnamed.json"
  stub_case outside-licence-unnamed "FAIL $name (cargo deny refused a licence that names no package)" stub STUB_DENY_OUTSIDE="$work/outside-unnamed.json" STUB_OUTSIDE_EXIT=4
  stub_case outside-licence-exit "FAIL $name (cargo deny check licenses over every target exited 2)" stub STUB_OUTSIDE_EXIT=2
  if [ -n "$outside_licences" ]; then
    : >"$work/outside-none.json"
    stub_case outside-licence-one-less "FAIL $name ($cause_outside)" stub STUB_DENY_OUTSIDE="$work/outside-none.json"
  fi
  make_fixture tracked-unchanged && git_fixture && run_case tracked-unchanged "PASS $name" stub
  make_fixture tracked-file-changed && git_fixture &&
    run_case tracked-file-changed "FAIL $name (1 tracked files differ from HEAD after cargo ran)" stub STUB_TEST_WRITES=account/Cargo.toml
  make_fixture untracked-file-written && git_fixture &&
    run_case untracked-file-written "PASS $name" stub STUB_TEST_WRITES=account/new-file.txt
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
  if edit Cargo.toml cat; then
    got=applied
  elif [ "$wrong" = "$before edit-that-changes-nothing" ]; then
    got=refused
  else
    got="refused without its case"
  fi
  wrong=$before
  harness_case edit-that-changes-nothing "$got"
  mkdir "$work/awk-that-copies"
  printf '#!/bin/sh\ntr -d %s\n' "'\\r'" >"$work/awk-that-copies/awk"
  chmod +x "$work/awk-that-copies/awk"
  make_fixture crlf-without-carriage-returns
  if (PATH="$work/awk-that-copies:$PATH" && to_crlf Cargo.toml); then
    harness_case crlf-without-carriage-returns accepted
  else
    harness_case crlf-without-carriage-returns refused
  fi
  run=whole
  [ -z "${R7_ACCOUNT_CASES:-}" ] || run=part
  if no_real_build "$run" "$real_runs"; then
    printf 'the %s job ran no real build\n' "$job"
    wrong="$wrong no-real-build"
  fi
  saved_macos=$real_builds_macos
  saved_windows=$real_builds_windows
  real_builds_macos=
  real_builds_windows=
  got=accepted
  job_real_builds macos || got=refused
  harness_case macos-list-emptied "$got"
  got=accepted
  job_real_builds windows || got=refused
  harness_case windows-list-emptied "$got"
  real_builds_macos=$saved_macos
  real_builds_windows=$saved_windows
  got=accepted
  no_real_build whole 0 && got=refused
  harness_case whole-run-without-a-real-build "$got"
  got=accepted
  saved_left=$left
  job_real_builds windows && real_build include-outside >/dev/null || got=refused
  left=$saved_left
  harness_case windows-job-runs-a-macos-build "$got"
  got=accepted
  split_holds macos ' ' >/dev/null || got=refused
  harness_case macos-job-misses-a-left-out-case "$got"
  got=accepted
  split_holds windows ' ' >/dev/null || got=refused
  harness_case windows-job-misses-a-case-of-its-list "$got"
  saved_job=$job
  saved_left=$left
  job=windows
  got=accepted
  runs_here dep-info-absent >/dev/null || got=refused
  job=$saved_job
  left=$saved_left
  harness_case windows-job-runs-a-left-out-case "$got"

  saved_job=${CI_D_JOB:-}
  CI_D_JOB=windows
  if takes_case tests-fail; then first=yes; else first=no; fi
  CI_D_JOB=macos
  if takes_case tests-fail; then second=yes; else second=no; fi
  CI_D_JOB=$saved_job
  if [ "$first" = "$second" ]; then
    harness_case job-read-once refused
  else
    harness_case job-read-once "followed CI_D_JOB after the start"
  fi
  if all_taken '' "$windows_cases $macos_cases"; then
    harness_case no-job-runs-every-case refused
  else
    harness_case no-job-runs-every-case "left a case out with no job named"
  fi
  if all_taken windows "$windows_cases" && none_taken windows "$macos_cases"; then
    harness_case windows-job-runs-its-list-alone refused
  else
    harness_case windows-job-runs-its-list-alone "ran another set"
  fi
  all_cases="$windows_cases $macos_cases"
  without_one=$(for listed_case in $all_cases; do [ "$listed_case" = tests-fail ] || printf '%s ' "$listed_case"; done)
  if missing_one=$(R7_ACCOUNT_CASES='' cases_missing macos "$without_one") &&
    missing_none=$(R7_ACCOUNT_CASES='' cases_missing macos "$all_cases") &&
    [ "$missing_one" = ' tests-fail' ] && [ -z "$missing_none" ]; then
    harness_case macos-job-misses-a-case refused
  else
    harness_case macos-job-misses-a-case "missed no case"
  fi
  without_one=$(for listed_case in $windows_cases; do [ "$listed_case" = unchanged ] || printf '%s ' "$listed_case"; done)
  without_link=$(for listed_case in $windows_cases; do [ "$listed_case" = link-out-of-account ] || printf '%s ' "$listed_case"; done)
  if missing_one=$(R7_ACCOUNT_CASES='' cases_missing windows "$without_one") &&
    missing_none=$(R7_ACCOUNT_CASES='' cases_missing windows "$windows_cases") &&
    missing_link=$(R7_ACCOUNT_CASES='' cases_missing windows "$without_link") &&
    [ "$missing_one" = ' unchanged' ] && [ -z "$missing_none" ] && [ -z "$missing_link" ]; then
    harness_case windows-job-misses-a-case refused
  else
    harness_case windows-job-misses-a-case "missed no case"
  fi
  saved_unlisted=$unlisted
  unlisted=
  note_case case-of-no-list
  noted=$unlisted
  unlisted=$saved_unlisted
  if [ "$noted" = ' case-of-no-list' ]; then
    harness_case case-on-no-list refused
  else
    harness_case case-on-no-list "took a case of no list"
  fi
  macos_count=0
  for listed_case in $macos_cases; do
    macos_count=$((macos_count + 1))
  done
  without_one=$(for listed_case in $all_cases; do [ "$listed_case" = tests-fail ] || printf '%s ' "$listed_case"; done)
  if verdict=$(R7_ACCOUNT_CASES='' split_verdict macos macos "$without_one" '' 0) &&
    [ "$verdict" = ' not-run:tests-fail' ]; then
    harness_case verdict-names-a-case-not-run refused
  else
    harness_case verdict-names-a-case-not-run "gave '$verdict'"
  fi
  if verdict=$(R7_ACCOUNT_CASES='' split_verdict macos macos "$all_cases" ' case-of-no-list' 0) &&
    [ "$verdict" = ' on-no-list:case-of-no-list' ]; then
    harness_case verdict-names-a-case-on-no-list refused
  else
    harness_case verdict-names-a-case-on-no-list "gave '$verdict'"
  fi
  if verdict=$(R7_ACCOUNT_CASES='' split_verdict windows windows "$windows_cases" '' $((macos_count - 1))) &&
    [ "$verdict" = " left-out-count:$((macos_count - 1))-not-$macos_count" ]; then
    harness_case verdict-names-a-left-out-count refused
  else
    harness_case verdict-names-a-left-out-count "gave '$verdict'"
  fi
  if verdict=$(R7_ACCOUNT_CASES='' split_verdict windows macos "$windows_cases" '' "$macos_count") &&
    [ "$verdict" = ' job-not-read' ]; then
    harness_case verdict-names-a-job-not-read refused
  else
    harness_case verdict-names-a-job-not-read "gave '$verdict'"
  fi
  if verdict=$(R7_ACCOUNT_CASES='' split_verdict macos macos "$all_cases" '' 0) &&
    [ -z "$verdict" ] &&
    verdict=$(R7_ACCOUNT_CASES='' split_verdict windows windows "$windows_cases" '' "$macos_count") &&
    [ -z "$verdict" ]; then
    harness_case verdict-of-right-values-has-no-word refused
  else
    harness_case verdict-of-right-values-has-no-word "gave '$verdict'"
  fi
  if end_verdict_of windows macos "$windows_cases" '' "$macos_count" && [ "$verdict" = ' job-not-read' ]; then
    harness_case end-verdict-reads-the-drivers-job refused
  else
    harness_case end-verdict-reads-the-drivers-job "gave '$verdict'"
  fi
  if end_verdict_of macos macos "$without_one" '' 0 && [ "$verdict" = ' not-run:tests-fail' ]; then
    harness_case end-verdict-reads-the-cases-that-ran refused
  else
    harness_case end-verdict-reads-the-cases-that-ran "gave '$verdict'"
  fi
  if end_verdict_of macos macos "$all_cases" ' case-of-no-list' 0 && [ "$verdict" = ' on-no-list:case-of-no-list' ]; then
    harness_case end-verdict-reads-the-cases-on-no-list refused
  else
    harness_case end-verdict-reads-the-cases-on-no-list "gave '$verdict'"
  fi
  if end_verdict_of windows windows "$windows_cases" '' $((macos_count - 1)) &&
    [ "$verdict" = " left-out-count:$((macos_count - 1))-not-$macos_count" ]; then
    harness_case end-verdict-reads-the-left-out-count refused
  else
    harness_case end-verdict-reads-the-left-out-count "gave '$verdict'"
  fi

  split=$(end_verdict) ||
    split="$split split-verdict-failed"
  printf 'cases left to the macos job: %s\n' "$left"
  if [ -n "$split$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s%s)\n' "$name" "$split" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  '') measure ;;
  *) exit 64 ;;
esac
