#!/bin/sh
# jobs: macos
# rule: cargo deny accepts the licences, advisories and sources of the shipped graph against the committed deny.toml, and npm audit reports no high or critical advisory.
set -u
name=23-licences

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

deny_run() {
  deny_manifest=$1; deny_config=$2; deny_out=$3; shift 3
  if [ "${CI:-}" = true ]; then
    cargo deny --log-level error --manifest-path "$deny_manifest" --config "$deny_config" --locked check "$@" >"$deny_out" 2>&1
  else
    cargo deny --log-level error --manifest-path "$deny_manifest" --config "$deny_config" --locked --offline check "$@" >"$deny_out" 2>&1
  fi
}

measure() {
  cd "$CI_D_REPO" || exit 2
  have_deny || { printf 'SKIP %s (cargo-deny is not installed)\n' "$name"; exit 77; }
  deny_version=$(cargo deny --version) || fail "cargo deny --version failed"
  printf 'cargo-deny version: %s\n' "${deny_version#cargo-deny }"
  deny_run Cargo.toml deny.toml "$CI_D_ARTIFACTS/deny.txt" licenses advisories sources
  code=$?
  rejected=$(grep -c '^error\[' "$CI_D_ARTIFACTS/deny.txt")
  printf 'cargo deny exit %s, %s errors\n' "$code" "$rejected"
  [ "$code" -eq 0 ] || fail "cargo deny check licenses advisories sources exited $code with $rejected errors"
  if [ "${CI:-}" != true ]; then
    printf 'SKIP %s (outside CI: npm audit was not run and cargo deny read the local advisory database)\n' "$name"
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
