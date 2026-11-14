#!/bin/sh
# jobs: macos windows
# rule: the normal dependency tree of halvo, on every target and with all features, holds halvo-control and none of halvo-capture, halvo-sqlcipher, rusqlite, libsqlite3-sys
set -u
name=20-app-links-no-store
required='halvo-control'
forbidden='halvo-capture halvo-sqlcipher rusqlite libsqlite3-sys'
banned='^(halvo-capture|halvo-sqlcipher|rusqlite|libsqlite3-sys) v'

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

classify() {
  printf '%s\n' "$1" | grep -q '^halvo-control v' || {
    printf 'the app does not depend on halvo-control'
    return 1
  }
  matches=$(printf '%s\n' "$1" | grep -E "$banned")
  status=$?
  case $status in
    1) ;;
    0)
      first=$(printf '%s\n' "$matches" | sed 1q)
      first=${first%% (*}
      printf 'src-tauri must reach capture data only through halvo-control and the daemon: %s' "$first"
      return 1
      ;;
    *)
      printf 'grep exited %s' "$status"
      return 1
      ;;
  esac
  cause=$(predicate_tree_lines "$1" "$required" "$forbidden") || {
    printf '%s' "${cause:-predicate_tree_lines of scripts/r1-gate/lib.sh gave no cause}"
    return 1
  }
  return 0
}

load_predicate() {
  [ -f "$1/scripts/r1-gate/lib.sh" ] || fail "scripts/r1-gate/lib.sh is missing"
  . "$1/scripts/r1-gate/lib.sh"
  command -v predicate_tree_lines >/dev/null 2>&1 || fail "scripts/r1-gate/lib.sh defines no predicate_tree_lines"
}

measure() {
  repo=${CI_D_REPO:-$(git rev-parse --show-toplevel)} || exit 2
  cd "$repo" || exit 2
  load_predicate "$repo"
  tree=$(cargo tree -p halvo -e normal --prefix none --locked --target all --all-features) || fail "cargo tree failed"
  if [ -n "${CI_D_ARTIFACTS:-}" ]; then
    printf '%s\n' "$tree" >"$CI_D_ARTIFACTS/halvo-tree.txt" || fail "cannot write the artifact"
  fi
  lines=$(printf '%s\n' "$tree" | grep -c '')
  printf 'halvo tree: %s lines\n' "$lines"
  cause=$(classify "$tree") || fail "$cause"
  printf 'PASS %s\n' "$name"
}

case_of() {
  cause=$(classify "$2")
  status=$?
  if [ "$status" -eq 0 ]; then got=PASS; else got=FAIL; fi
  printf 'case %s/%s want=%s got=%s %s\n' "$name" "$1" "$3" "$got" "$cause"
  if [ "$got" != "$3" ]; then
    wrong="$wrong $1"
  elif [ "$got" = FAIL ]; then
    trees_rejected=$((trees_rejected + 1))
  else
    trees_accepted=$((trees_accepted + 1))
  fi
}

measure_case() {
  out=$(
    CARGO_HOME=$work
    PATH="$work/bin:$PATH"
    STUB_SHOWS=$2
    CI_D_ARTIFACTS=
    export CARGO_HOME PATH STUB_SHOWS
    measure 2>&1
  )
  status=$?
  last=$(printf '%s\n' "$out" | sed -n '$p')
  printf 'case %s/measure-%s want=exit %s got=exit %s %s\n' "$name" "$1" "$3" "$status" "$last"
  if [ "$status" != "$3" ] || [ "$last" != "$4" ]; then
    wrong="$wrong measure-$1"
  elif [ "$status" = 1 ]; then
    runs_rejected=$((runs_rejected + 1))
  fi
}

self_test() {
  repo=${CI_D_REPO:-$(git rev-parse --show-toplevel)} || exit 2
  load_predicate "$repo"
  trees_rejected=0
  trees_accepted=0
  runs_rejected=0
  wrong=
  clean='halvo v0.0.0 (/w/src-tauri)
halvo-control v0.0.0 (/w/crates/halvo-control)
serde v1.0.229
tauri v2.12.5'
  case_of clean "$clean" PASS
  case_of rusqlite "$clean
rusqlite v0.41.1" FAIL
  case_of no-control-line 'halvo v0.0.0 (/w/src-tauri)
serde v1.0.229' FAIL
  case_of empty '' FAIL
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  mkdir "$work/bin" || exit 2
  cat >"$work/bin/cargo" <<'EOF'
#!/bin/sh
case $STUB_SHOWS in
  cargo-fails)
    printf 'halvo-control v0.0.0 (/w/crates/halvo-control)\n'
    exit 101
    ;;
esac
printf 'halvo v0.0.0 (/w/src-tauri)\nhalvo-control v0.0.0 (/w/crates/halvo-control)\nserde v1.0.229\n'
exit 0
EOF
  chmod +x "$work/bin/cargo" || exit 2
  measure_case cargo-fails cargo-fails 1 "FAIL $name (cargo tree failed)"
  measure_case clean clean 0 "PASS $name"
  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'trees: %s failing rejected, %s clean accepted; cargo runs: %s failing rejected\n' "$trees_rejected" "$trees_accepted" "$runs_rejected"
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$((trees_rejected + runs_rejected))"
}

case ${1:-} in
  --self-test) self_test ;;
  '') measure ;;
  *) exit 64 ;;
esac
