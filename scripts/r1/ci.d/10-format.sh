#!/bin/sh
# jobs: macos windows
# rule: Rust code is formatted as cargo fmt writes it.
set -u
name=10-format

measure() {
  cd "$CI_D_REPO" || exit 2
  cargo fmt --check
  code=$?
  if [ "$code" -ne 0 ]; then
    printf 'FAIL %s (cargo fmt --check exited %s; run cargo fmt and commit the result)\n' "$name" "$code"
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
[ "$*" = 'fmt --check' ] || { printf 'stub cargo: got the arguments: %s\n' "$*"; exit 97; }
exit "$STUB_EXIT"
EOF
  chmod +x "$work/bin/cargo"
  : >"$work/repo/fixture-root"
  rejected=0
  wrong=

  STUB_EXIT=0
  check formatted PASS
  STUB_EXIT=1
  check difference-found FAIL 'cargo fmt --check exited 1;'
  STUB_EXIT=101
  check cargo-fmt-fails FAIL 'cargo fmt --check exited 101;'

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
