#!/bin/sh
# jobs: macos windows
# rule: halvo-control links no store code
set -u
name=19-control-links-no-store
banned='^(halvo-capture|halvo-sqlcipher|rusqlite|libsqlite3-sys) v'

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

classify() {
  printf '%s\n' "$1" | grep -q '^halvo-control v' || {
    printf 'the tree has no line halvo-control v'
    return 1
  }
  matches=$(printf '%s\n' "$1" | grep -E "$banned")
  status=$?
  case $status in
    1) return 0 ;;
    0)
      first=$(printf '%s\n' "$matches" | sed 1q)
      printf 'halvo-control must link no store code: %s' "$first"
      return 1
      ;;
    *)
      printf 'grep exited %s' "$status"
      return 1
      ;;
  esac
}

measure() {
  cd "${CI_D_REPO:-.}" || exit 2
  tree=$(cargo tree -p halvo-control -e normal --prefix none --locked --target all --all-features) ||
    fail "cargo tree failed for halvo-control"
  if [ -n "${CI_D_ARTIFACTS:-}" ]; then
    printf '%s\n' "$tree" >"$CI_D_ARTIFACTS/halvo-control-tree.txt" || fail "cannot write the artifact"
  fi
  lines=$(printf '%s\n' "$tree" | grep -c '')
  printf 'halvo-control tree: %s lines\n' "$lines"
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
    rejected=$((rejected + 1))
  fi
}

measure_case() {
  out=$(
    PATH="$work/bin:$PATH"
    STUB_SHOWS=$2
    CI_D_REPO=$work
    CI_D_ARTIFACTS=
    export PATH STUB_SHOWS CI_D_REPO
    measure 2>&1
  )
  status=$?
  last=$(printf '%s\n' "$out" | sed -n '$p')
  printf 'case %s/measure-%s want=exit %s got=exit %s %s\n' "$name" "$1" "$3" "$status" "$last"
  if [ "$status" != "$3" ] || [ "$last" != "$4" ]; then
    wrong="$wrong measure-$1"
  elif [ "$status" = 1 ]; then
    rejected=$((rejected + 1))
  fi
}

self_test() {
  rejected=0
  wrong=
  clean='halvo-control v0.0.0 (/w/crates/halvo-control)
serde v1.0.229
thiserror v2.0.21
libc v0.2.191'
  case_of clean "$clean" PASS
  case_of rusqlite "$clean
rusqlite v0.41.1" FAIL
  case_of libsqlite3-sys "$clean
libsqlite3-sys v0.37.0" FAIL
  case_of halvo-capture "$clean
halvo-capture v0.0.0 (/w/crates/halvo-capture)" FAIL
  case_of halvo-sqlcipher "$clean
halvo-sqlcipher v0.0.0 (/w/crates/halvo-sqlcipher)" FAIL
  case_of no-control-line 'serde v1.0.229
thiserror v2.0.21' FAIL
  case_of empty '' FAIL
  case_of another-crate-of-a-like-name "$clean
rusqlite-macros v0.1.0
halvo-capture-tools v0.0.0 (/w/crates/halvo-capture-tools)" PASS
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  mkdir "$work/bin" || exit 2
  cat >"$work/bin/cargo" <<'EOF'
#!/bin/sh
printf 'halvo-control v0.0.0 (/w/crates/halvo-control)\nserde v1.0.229\n'
case "$STUB_SHOWS: $* " in
  always:* | all-features:*' --all-features '*) printf 'rusqlite v0.41.1\n' ;;
esac
exit 0
EOF
  chmod +x "$work/bin/cargo" || exit 2
  refused="FAIL $name (halvo-control must link no store code: rusqlite v0.41.1)"
  measure_case always always 1 "$refused"
  measure_case all-features all-features 1 "$refused"
  measure_case never never 0 "PASS $name"
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
