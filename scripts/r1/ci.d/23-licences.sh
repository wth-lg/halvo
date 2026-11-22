#!/bin/sh
# jobs: macos
# rule: cargo deny accepts, with the owner's seven licences allowed, the licences and sources of the shipped graph against the committed deny.toml, and every licence exception of deny.toml has one row with its reason in scripts/r1-gate/licence-exceptions.tsv and every row its exception; it reads no advisory database, which 25-advisories reads.
set -u
name=23-licences
table=scripts/r1-gate/licence-exceptions.tsv

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

have_deny() {
  if ! command -v cargo-deny >/dev/null 2>&1 && [ "${CI:-}" = true ]; then
    HOMEBREW_NO_AUTO_UPDATE=1 HOMEBREW_NO_INSTALL_CLEANUP=1 brew install cargo-deny >"$CI_D_ARTIFACTS/brew-install.txt" 2>&1
  fi
  command -v cargo-deny >/dev/null 2>&1
}

exception_pairs() {
  awk '
    /^exceptions = \[/ { inside = 1; next }
    inside && /^\]/ { inside = 0 }
    inside && /crate = "/ {
      crate = $0; sub(/.*crate = "/, "", crate); sub(/".*/, "", crate)
      list = $0; sub(/.*allow = \[/, "", list); sub(/\].*/, "", list)
      n = split(list, names, ",")
      for (i = 1; i <= n; i++) { name = names[i]; gsub(/[" ]/, "", name); if (name != "") print crate "\t" name }
    }' "$1" | sort
}

row_pairs() {
  awk -F '\t' 'NR > 1 && NF > 0 { if (NF != 4 || $3 == "" || $4 == "") print "bad\t" NR; else print $1 "\t" $2 }' "$1" | sort
}

exceptions_defect() {
  [ -f "$2" ] || { printf 'the table of licence exceptions %s is missing\n' "$3"; return 1; }
  deny_pairs=$(exception_pairs "$1")
  table_pairs=$(row_pairs "$2")
  bad_row=$(printf '%s\n' "$table_pairs" | sed -n 's/^bad\t//p' | head -1)
  [ -z "$bad_row" ] || { printf 'line %s of %s does not hold crate, licence, reason and who confirmed it\n' "$bad_row" "$3"; return 1; }
  [ -n "$deny_pairs" ] || { printf 'deny.toml holds no licence exception that the reader can see\n'; return 1; }
  for pair in $(printf '%s\n' "$deny_pairs" | tr '\t' ':'); do
    printf '%s\n' "$table_pairs" | tr '\t' ':' | grep -q -x -F -e "$pair" || { printf 'the exception %s (%s) of deny.toml has no row in %s\n' "${pair%%:*}" "${pair#*:}" "$3"; return 1; }
  done
  for pair in $(printf '%s\n' "$table_pairs" | tr '\t' ':'); do
    printf '%s\n' "$deny_pairs" | tr '\t' ':' | grep -q -x -F -e "$pair" || { printf 'the row %s (%s) of %s names no exception of deny.toml\n' "${pair%%:*}" "${pair#*:}" "$3"; return 1; }
  done
}

deny_run() {
  deny_manifest=$1; deny_config=$2; deny_out=$3; shift 3
  if [ "${CI:-}" = true ]; then
    cargo deny --log-level error --manifest-path "$deny_manifest" --config "$deny_config" --locked check "$@" >"$deny_out" 2>&1
  else
    cargo deny --log-level error --manifest-path "$deny_manifest" --config "$deny_config" --locked --offline check "$@" >"$deny_out" 2>&1
  fi
}

allow_defect() {
  allowed=$(awk '/^\[licenses\]/ { inside = 1; next } /^\[/ { inside = 0 } inside && /^allow = \[/ { line = $0; sub(/^allow = \[/, "", line); sub(/\].*/, "", line); gsub(/[" ]/, "", line); n = split(line, names, ","); for (i = 1; i <= n; i++) if (names[i] != "") print names[i] }' "$1" | sort | tr '\n' ' ')
  [ "$allowed" = 'Apache-2.0 BSD-2-Clause BSD-3-Clause ISC MIT Unicode-3.0 Zlib ' ] && return 0
  printf 'deny.toml allows [%s], and P4 allows the owner'"'"'s seven: MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Zlib and Unicode-3.0; any other licence is an exception for one crate with its row\n' "${allowed% }"
  return 1
}

deny_result_defect() {
  first_error=$(grep -m 1 '^error\[' "$2")
  [ "$1" -eq 0 ] && [ -z "$first_error" ] && return 0
  printf 'cargo deny exited %s: %s\n' "$1" "${first_error:-no error line}"
  return 1
}

measure() {
  cd "$CI_D_REPO" || exit 2
  have_deny || { printf 'SKIP %s (cargo-deny is not installed)\n' "$name"; exit 77; }
  deny_version=$(cargo deny --version) || fail "cargo deny --version failed"
  printf 'cargo-deny version: %s\n' "${deny_version#cargo-deny }"
  found=$(allow_defect deny.toml) || fail "$found"
  found=$(exceptions_defect deny.toml "$table" "$table") || fail "$found"
  printf 'licence exceptions: each of deny.toml has its row in %s, and each row its exception\n' "$table"
  deny_run Cargo.toml deny.toml "$CI_D_ARTIFACTS/deny.txt" licenses sources
  code=$?
  rejected=$(grep -c '^error\[' "$CI_D_ARTIFACTS/deny.txt")
  printf 'cargo deny exit %s, %s errors\n' "$code" "$rejected"
  found=$(deny_result_defect "$code" "$CI_D_ARTIFACTS/deny.txt") || fail "cargo deny check licenses sources: $found"
  printf 'PASS %s\n' "$name"
}

verdict() {
  got=PASS
  [ "$2" -eq 0 ] || got=FAIL
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$3" "$got"
  if [ "$got" != "$3" ]; then wrong="$wrong $1"; elif [ "$3" = FAIL ]; then rejected=$((rejected + 1)); fi
}

crate() {
  mkdir -p "$work/$1/src" || exit 2
  printf '[package]\nname = "deny-fixture-%s"\nversion = "0.1.0"\nedition = "2021"\nlicense = "%s"\ndescription = "fixture"\nrepository = "https://example.invalid/fixture"\n\n[workspace]\n' "$1" "$2" >"$work/$1/Cargo.toml"
  printf 'pub fn f() {}\n' >"$work/$1/src/lib.rs"
  (cd "$work/$1" && cargo generate-lockfile --offline >/dev/null 2>&1) || exit 2
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  rejected=0
  wrong=
  have_deny || { printf 'FAIL self-test:%s (cargo-deny is not installed)\n' "$name"; exit 1; }
  crate mit MIT
  crate gpl GPL-3.0-only
  deny_run "$work/mit/Cargo.toml" "$CI_D_REPO/deny.toml" "$work/mit.out" licenses
  verdict mit-crate-is-accepted "$?" PASS
  deny_run "$work/gpl/Cargo.toml" "$CI_D_REPO/deny.toml" "$work/gpl.out" licenses
  code=$?
  grep -q 'rejected' "$work/gpl.out" || code=0
  verdict licence-outside-the-list "$code" FAIL
  printf '[licenses]\nexceptions = [\n  { crate = "alpha", allow = ["MPL-2.0"] },\n  { crate = "beta", allow = ["blessing", "WTFPL"] },\n]\n' >"$work/deny-two.toml"
  printf '[licenses]\nexceptions = [\n  { crate = "alpha", allow = ["MPL-2.0"] },\n  { crate = "beta", allow = ["blessing", "WTFPL"] },\n  { crate = "gamma", allow = ["MPL-2.0"] },\n]\n' >"$work/deny-three.toml"
  printf 'crate\tlicence\treason\tconfirmed\nalpha\tMPL-2.0\ta reason\topen\nbeta\tblessing\ta reason\topen\nbeta\tWTFPL\ta reason\topen\n' >"$work/rows-two.tsv"
  cp "$work/rows-two.tsv" "$work/rows-extra.tsv"
  printf 'delta\tMPL-2.0\ta reason\topen\n' >>"$work/rows-extra.tsv"
  printf 'crate\tlicence\treason\tconfirmed\nalpha\tMPL-2.0\ta reason\topen\nbeta\tblessing\t\topen\nbeta\tWTFPL\ta reason\topen\n' >"$work/rows-no-reason.tsv"
  exceptions_defect "$work/deny-two.toml" "$work/rows-two.tsv" table >/dev/null
  verdict every-exception-has-its-row "$?" PASS
  exceptions_defect "$work/deny-three.toml" "$work/rows-two.tsv" table >/dev/null
  verdict exception-without-a-row "$?" FAIL
  exceptions_defect "$work/deny-two.toml" "$work/rows-extra.tsv" table >/dev/null
  verdict row-without-an-exception "$?" FAIL
  exceptions_defect "$work/deny-two.toml" "$work/rows-no-reason.tsv" table >/dev/null
  verdict row-without-a-reason "$?" FAIL
  exceptions_defect "$CI_D_REPO/deny.toml" "$CI_D_REPO/$table" "$table" >/dev/null
  verdict the-committed-files-agree "$?" PASS
  printf '[licenses]\nallow = ["MIT", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Zlib", "Unicode-3.0"]\n' >"$work/allow-seven.toml"
  printf '[licenses]\nallow = ["MIT", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Zlib", "Unicode-3.0", "BSL-1.0"]\n' >"$work/allow-eight.toml"
  allow_defect "$work/allow-seven.toml" >/dev/null
  verdict allow-list-of-the-owner "$?" PASS
  allow_defect "$work/allow-eight.toml" >/dev/null
  verdict allow-list-with-an-eighth-licence "$?" FAIL
  allow_defect "$CI_D_REPO/deny.toml" >/dev/null
  verdict the-committed-allow-list "$?" PASS
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
