#!/bin/sh
# Sub-check: the windows-2025 CI job for the head SHA concluded success.
set -u
DIR=$(cd "$(dirname "$0")" && pwd)
. "$DIR/lib.sh"
name=windows-ci

self_test() {
  if predicate_ci_success success && ! predicate_ci_success failure && ! predicate_ci_success ""; then
    emit_pass "self-test:$name"; return 0
  fi
  emit_fail "self-test:$name" "predicate misclassified a fixture"; return 1
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

case "${1:-}" in
  --self-test) self_test ;;
  --measure) shift; measure ;;
  *) measure ;;
esac
