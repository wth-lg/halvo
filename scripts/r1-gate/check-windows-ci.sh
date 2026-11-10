#!/bin/sh
set -u
DIR=$(cd "$(dirname "$0")" && pwd)
. "$DIR/lib.sh"
name=windows-ci

cases() {
  t success PASS predicate_ci_success success
  t failure FAIL predicate_ci_success failure
  t empty FAIL predicate_ci_success ""
}

measure() {
  sha=$(git -C "$GATE_ROOT" rev-parse HEAD 2>/dev/null || echo "")
  if ! command -v gh >/dev/null 2>&1; then
    emit_fail "$name" "gh not available to read CI for ${sha:-HEAD}"; return 1
  fi
  concl=$(gh run list -R "$GATE_REPO" -w ci -c "$sha" -L1 --json conclusion -q '.[0].conclusion' 2>/dev/null || echo "")
  if predicate_ci_success "$concl"; then emit_pass "$name"; return 0; fi
  emit_fail "$name" "no successful ci run for ${sha:-HEAD} (conclusion='${concl:-none}')"; return 1
}

check_main "$@"
