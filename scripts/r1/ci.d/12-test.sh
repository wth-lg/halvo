#!/bin/sh
# jobs: macos windows
# rule: Every test of the workspace passes.
set -u
name=12-test

measure() {
  cd "$CI_D_REPO" || exit 2
  cargo test --workspace
  code=$?
  if [ "$code" -ne 0 ]; then
    printf 'FAIL %s (cargo test exited %s)\n' "$name" "$code"
    exit 1
  fi
  printf 'PASS %s\n' "$name"
}

check() {
  out=$(
    PATH=$work/bin:$PATH
    CI_D_REPO=$work
    export PATH CI_D_REPO
    measure 2>&1
  )
  code=$?
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  case $out in
    *"${3:-}"*) ;;
    *) got=$got-without-the-text ;;
  esac
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$got"
  if [ "$got" != "$2" ]; then
    wrong="$wrong $1"
  elif [ "$2" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  mkdir -p "$work/bin"
  cat >"$work/bin/cargo" <<'EOF'
#!/bin/sh
[ "$*" = 'test --workspace' ] || { printf 'stub cargo: got the arguments: %s\n' "$*"; exit 97; }
exit "$STUB_EXIT"
EOF
  chmod +x "$work/bin/cargo"
  rejected=0
  wrong=
  export STUB_EXIT

  STUB_EXIT=0
  check tests-pass PASS
  STUB_EXIT=101
  check test-fails FAIL 'cargo test exited 101'

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
