#!/bin/sh
# jobs: macos
# self-test: alone
# rule: cargo deny check advisories accepts the shipped graph against the committed deny.toml, with no yanked crate and every ignored advisory present in the lock, and npm audit reports no high or critical advisory; both read advisory databases that change with no commit, so the check stands in each daily slice of the macos job and not in the merge gate.
set -u
name=25-advisories
deny_fix='in a pull request of its own, update Cargo.lock to the version that the advisory names as mended, or add { id = "<id>", reason = "<why it does not reach Halvo>, <date>" } to the ignore list of [advisories] in deny.toml; delete an ignore that cargo deny calls advisory-not-detected'
npm_fix='in a pull request of its own, update package-lock.json to the version that mends the advisory (npm audit fix)'

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
    const bad = (s) => s === "high" || s === "critical";
    const ids = new Set();
    for (const [pkg, entry] of Object.entries(report.vulnerabilities || {})) {
      if (!entry || !bad(entry.severity)) continue;
      const own = (Array.isArray(entry.via) ? entry.via : []).filter((via) => via && typeof via === "object" && bad(via.severity));
      for (const via of own) ids.add(String(via.url || via.source || "no id").replace(/.*\//, "") + " in " + pkg);
      if (own.length === 0 && !(Array.isArray(entry.via) && entry.via.length)) ids.add("no id in " + pkg);
    }
    const named = [...ids].sort().join(", ").replace(/[^A-Za-z0-9 ._@\/,-]/g, "?");
    console.log(v.high + " " + v.critical + " " + (named || "none named"));
  ' "$1") || { printf 'the npm audit report holds no metadata.vulnerabilities counts\n'; return 1; }
  high=${counts%% *}
  rest=${counts#* }
  critical=${rest%% *}
  [ "$high $critical" = '0 0' ] && return 0
  printf 'npm audit reports %s high and %s critical advisories: %s\n' "$high" "$critical" "${rest#* }"
  return 1
}

crate() {
  mkdir -p "$work/$1/src" || exit 2
  printf '[package]\nname = "deny-fixture-%s"\nversion = "0.1.0"\nedition = "2021"\nlicense = "%s"\ndescription = "fixture"\nrepository = "https://example.invalid/fixture"\n\n[workspace]\n' "$1" "$2" >"$work/$1/Cargo.toml"
  printf 'pub fn f() {}\n' >"$work/$1/src/lib.rs"
  (cd "$work/$1" && cargo generate-lockfile --offline >/dev/null 2>&1) || exit 2
}

deny_run() {
  if [ "${CI:-}" = true ]; then
    cargo deny --log-level error --manifest-path "$1" --config "$2" --locked check advisories >"$3" 2>&1
  else
    cargo deny --log-level error --manifest-path "$1" --config "$2" --locked --offline check -W advisory-not-detected advisories >"$3" 2>&1
  fi
}

deny_result_defect() {
  first_error=$(grep -m 1 '^error\[' "$2")
  [ "$1" -eq 0 ] && [ -z "$first_error" ] && return 0
  ids=$(grep -o -E 'RUSTSEC-[0-9]{4}-[0-9]{4}' "$2" | sort -u | tr '\n' ' ')
  ids=${ids% }
  printf 'cargo deny exited %s: %s; advisories: %s\n' "$1" "${first_error:-no error line}" "${ids:-none named}"
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
  deny_run Cargo.toml deny.toml "$CI_D_ARTIFACTS/deny.txt"
  code=$?
  rejected=$(grep -c '^error\[' "$CI_D_ARTIFACTS/deny.txt")
  printf 'cargo deny exit %s, %s errors\n' "$code" "$rejected"
  found=$(deny_result_defect "$code" "$CI_D_ARTIFACTS/deny.txt") || fail "cargo deny check advisories: $found. Fix: $deny_fix"
  if [ "${CI:-}" != true ]; then
    note=$(local_database_note)
    printf 'SKIP %s (outside CI: npm audit was not run; %s)\n' "$name" "$note"
    exit 77
  fi
  npm audit --json --audit-level=high >"$CI_D_ARTIFACTS/npm-audit.json" 2>"$CI_D_ARTIFACTS/npm-audit.err"
  found=$(audit_defect "$CI_D_ARTIFACTS/npm-audit.json") || fail "$found. Fix: $npm_fix"
  printf 'npm audit: 0 high and 0 critical advisories\n'
  printf 'PASS %s\n' "$name"
}

verdict() {
  got=PASS
  [ "$2" -eq 0 ] || got=FAIL
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$3" "$got"
  if [ "$got" != "$3" ]; then wrong="$wrong $1"; elif [ "$3" = FAIL ]; then rejected=$((rejected + 1)); fi
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
