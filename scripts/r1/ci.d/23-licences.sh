#!/bin/sh
# jobs: macos
# rule: cargo deny accepts, with the owner's seven licences allowed and every ignored advisory present in the lock, the licences, advisories and sources of the shipped graph against the committed deny.toml, every licence exception of deny.toml has one row with its reason in scripts/r1-gate/licence-exceptions.tsv and every row its exception, and npm audit reports no high or critical advisory.
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

audit_defect() {
  [ -s "$1" ] || { printf 'npm audit wrote no report\n'; return 1; }
  counts=$(node -e '
    let report;
    try { report = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")); } catch (e) { process.exit(3); }
    const v = report && report.metadata && report.metadata.vulnerabilities;
    if (!v || typeof v.high !== "number" || typeof v.critical !== "number") process.exit(3);
    console.log(v.high + " " + v.critical);
  ' "$1") || { printf 'the npm audit report holds no metadata.vulnerabilities counts\n'; return 1; }
  [ "$counts" = '0 0' ] && return 0
  printf 'npm audit reports %s high and %s critical advisories\n' "${counts% *}" "${counts#* }"
  return 1
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
    cargo deny --log-level error --manifest-path "$deny_manifest" --config "$deny_config" --locked --offline check -W advisory-not-detected "$@" >"$deny_out" 2>&1
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

local_database_note() {
  newest=0
  for database in "${CARGO_HOME:-$HOME/.cargo}"/advisory-dbs/*/; do
    [ -d "$database" ] || continue
    stamp=$(git -C "$database" log -1 --format=%ct 2>/dev/null) || continue
    [ "${stamp:-0}" -le "$newest" ] || newest=$stamp
  done
  [ "$newest" -gt 0 ] || { printf 'no local advisory database was found'; return 0; }
  now=$(date +%s)
  printf 'cargo deny read the local advisory database, whose newest commit is %s days old, and an ignore that it does not know is not judged outside CI' "$(((now - newest) / 86400))"
}

measure() {
  cd "$CI_D_REPO" || exit 2
  have_deny || { printf 'SKIP %s (cargo-deny is not installed)\n' "$name"; exit 77; }
  deny_version=$(cargo deny --version) || fail "cargo deny --version failed"
  printf 'cargo-deny version: %s\n' "${deny_version#cargo-deny }"
  found=$(allow_defect deny.toml) || fail "$found"
  found=$(exceptions_defect deny.toml "$table" "$table") || fail "$found"
  printf 'licence exceptions: each of deny.toml has its row in %s, and each row its exception\n' "$table"
  deny_run Cargo.toml deny.toml "$CI_D_ARTIFACTS/deny.txt" licenses advisories sources
  code=$?
  rejected=$(grep -c '^error\[' "$CI_D_ARTIFACTS/deny.txt")
  printf 'cargo deny exit %s, %s errors\n' "$code" "$rejected"
  found=$(deny_result_defect "$code" "$CI_D_ARTIFACTS/deny.txt") || fail "cargo deny check licenses advisories sources: $found"
  if [ "${CI:-}" != true ]; then
    note=$(local_database_note)
    printf 'SKIP %s (outside CI: npm audit was not run; %s)\n' "$name" "$note"
    exit 77
  fi
  npm audit --json --audit-level=high >"$CI_D_ARTIFACTS/npm-audit.json" 2>"$CI_D_ARTIFACTS/npm-audit.err"
  found=$(audit_defect "$CI_D_ARTIFACTS/npm-audit.json") || fail "$found"
  printf 'npm audit: 0 high and 0 critical advisories\n'
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

audit_case() {
  printf '%s\n' "$3" >"$work/$1.json"
  audit_defect "$work/$1.json" >/dev/null
  verdict "$1" "$?" "$2"
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
  printf 'advisories ok, licenses ok, sources ok\n' >"$work/deny-clean.txt"
  printf 'error[vulnerability]: sqlcipher-src via C SQLite CVE-2026-119375\n    ├ ID: RUSTSEC-2026-0502\n    ├ Solution: Upgrade to >=420.0.0\n' >"$work/deny-advisory.txt"
  deny_result_defect 0 "$work/deny-clean.txt" >/dev/null
  verdict deny-clean "$?" PASS
  deny_result_defect 1 "$work/deny-advisory.txt" >/dev/null
  verdict cargo-advisory "$?" FAIL
  deny_result_defect 0 "$work/deny-advisory.txt" >/dev/null
  verdict error-line-with-exit-0 "$?" FAIL
  printf '[advisories]\nunused-ignored-advisory = "deny"\nignore = [\n  { id = "RUSTSEC-2020-0001", reason = "fixture: an advisory that the lock does not have" },\n]\n' >"$work/deny-unused-ignore.toml"
  if [ "${CI:-}" = true ]; then
    cargo deny --log-level error --manifest-path "$work/mit/Cargo.toml" --config "$work/deny-unused-ignore.toml" --locked check advisories >"$work/unused.out" 2>&1
  else
    cargo deny --log-level error --manifest-path "$work/mit/Cargo.toml" --config "$work/deny-unused-ignore.toml" --locked --offline check advisories >"$work/unused.out" 2>&1
  fi
  code=$?
  grep -q 'advisory-not-detected' "$work/unused.out" || code=0
  verdict ignore-of-an-advisory-the-lock-does-not-have "$code" FAIL
  audit_case npm-no-advisory PASS '{"auditReportVersion":2,"vulnerabilities":{},"metadata":{"vulnerabilities":{"info":0,"low":2,"moderate":1,"high":0,"critical":0,"total":3}}}'
  audit_case npm-high-advisory FAIL '{"auditReportVersion":2,"vulnerabilities":{"planted":{"severity":"high"}},"metadata":{"vulnerabilities":{"info":0,"low":0,"moderate":0,"high":1,"critical":0,"total":1}}}'
  audit_case npm-critical-advisory FAIL '{"auditReportVersion":2,"vulnerabilities":{"planted":{"severity":"critical"}},"metadata":{"vulnerabilities":{"info":0,"low":0,"moderate":0,"high":0,"critical":1,"total":1}}}'
  audit_case npm-report-without-counts FAIL '{"error":{"code":"ENOLOCK"}}'
  audit_case npm-report-not-json FAIL 'npm ERR! network'
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
