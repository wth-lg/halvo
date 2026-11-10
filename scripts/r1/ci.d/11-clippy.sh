#!/bin/sh
# jobs: macos windows
# rule: Clippy reports no warning for any target of the workspace, built with the committed lockfile.
set -u
name=11-clippy

measure() {
  cd "$CI_D_REPO" || exit 2
  cargo clippy --workspace --all-targets --locked -- -D warnings
  code=$?
  if [ "$code" -ne 0 ]; then
    printf 'FAIL %s (cargo clippy exited %s)\n' "$name" "$code"
    exit 1
  fi
  printf 'PASS %s\n' "$name"
}

check() {
  out=$(
    PATH=$work/bin:$PATH
    CI_D_REPO=$work/repo
    export PATH CI_D_REPO STUB_EXIT
    sh "$0" 2>&1
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
  mkdir -p "$work/bin" "$work/repo"
  cat >"$work/bin/cargo" <<'EOF'
#!/bin/sh
[ -f fixture-root ] || { printf 'stub cargo: started outside the repo root\n'; exit 98; }
[ "$*" = 'clippy --workspace --all-targets --locked -- -D warnings' ] || { printf 'stub cargo: got the arguments: %s\n' "$*"; exit 97; }
exit "$STUB_EXIT"
EOF
  chmod +x "$work/bin/cargo"
  : >"$work/repo/fixture-root"
  rejected=0
  wrong=

  STUB_EXIT=0
  check no-warning PASS
  STUB_EXIT=101
  check warning-found FAIL 'cargo clippy exited 101)'
  STUB_EXIT=1
  check cargo-clippy-fails FAIL 'cargo clippy exited 1)'

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
