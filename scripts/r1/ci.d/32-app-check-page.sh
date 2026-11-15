#!/bin/sh
# jobs: macos windows
# rule: the built app loads its page and its read commands answer through the capability
# lane: alone
set -u
name=32-app-check-page
APP_CHECK_PAGE_MAX_S=60
expected_commands_ok=3
grace_s=30

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

native_path() {
  if command -v cygpath >/dev/null 2>&1; then
    cygpath -w "$1"
  else
    printf '%s\n' "$1"
  fi
}

run_check() {
  exe=$1
  config_dir=$2
  max_s=$3
  limit_s=$4
  out=$5
  err=$6
  dir_arg=$(native_path "$config_dir") || fail "the config directory has no native path"
  "$exe" --check-page --config-dir "$dir_arg" --within-s "$max_s" >"$out" 2>"$err" &
  pid=$!
  waited=0
  while kill -0 "$pid" 2>/dev/null; do
    if [ "$waited" -ge "$limit_s" ]; then
      kill "$pid" 2>/dev/null
      wait "$pid"
      fail "halvo --check-page ran past $limit_s s and was ended"
    fi
    sleep 1
    waited=$((waited + 1))
  done
  wait "$pid"
  check_code=$?
  check_seconds=$waited
}

verdict() {
  out=$1
  code=$2
  version=$3
  lines=$(grep -c '' "$out")
  content=$(cat "$out") || fail "the output of halvo --check-page cannot be read"
  expected="page=loaded commands_ok=$expected_commands_ok version=$version"
  first=$(head -n 1 "$out" | cut -c1-120 | LC_ALL=C tr -c 'A-Za-z0-9=._ \n-' '?') || fail "the first line of halvo --check-page cannot be read"
  [ -n "$first" ] || first='(none)'
  matched=false
  if [ "$lines" -eq 1 ] && [ "$content" = "$expected" ]; then
    matched=true
  fi
  printf 'check-page exit: %s, lines: %s, line matched: %s, first line: %s\n' "$code" "$lines" "$matched" "$first"
  [ "$code" -eq 0 ] || fail "halvo --check-page exited $code"
  [ "$lines" -eq 1 ] || fail "halvo --check-page printed $lines lines"
  [ "$matched" = true ] || fail "halvo --check-page printed another line than page=loaded commands_ok=$expected_commands_ok version=<the version>"
}

measure() {
  repo=${CI_D_REPO:-$(git rev-parse --show-toplevel)} || exit 2
  cd "$repo" || exit 2
  artifacts=${CI_D_ARTIFACTS:-}
  if [ -z "$artifacts" ]; then
    artifacts=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail "mktemp failed"
  fi
  cargo build -p halvo --locked >"$artifacts/build.txt" 2>&1 || fail "cargo build -p halvo --locked failed; see build.txt"
  exe=target/debug/halvo
  [ -f "$exe.exe" ] && exe=$exe.exe
  [ -f "$exe" ] || fail "cargo build left no $exe"
  version=$("$exe" --version) || fail "halvo --version failed"
  config_dir=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail "mktemp failed"
  run_check "$exe" "$config_dir" "$APP_CHECK_PAGE_MAX_S" $((APP_CHECK_PAGE_MAX_S + grace_s)) "$artifacts/check-page.out" "$artifacts/check-page.err"
  printf 'check-page seconds: %s, bound: %s\n' "$check_seconds" "$APP_CHECK_PAGE_MAX_S"
  verdict "$artifacts/check-page.out" "$check_code" "$version"
  git diff --quiet HEAD -- || fail "a tracked file differs from HEAD after the build and the check"
  printf 'PASS %s\n' "$name"
}

fixture() {
  exe=$1
  root=$2
  run_check "$exe" "$root/config" 2 4 "$root/out" "$root/err"
  verdict "$root/out" "$check_code" 0.0.0
  printf 'PASS %s\n' "$name"
}

check() {
  mkdir -p "$work/$1/config"
  out=$(STUB_MODE=$1 sh "$self" --fixture "$work/bin/halvo" "$work/$1" 2>&1)
  code=$?
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  case $out in
    *"$3"*) ;;
    *) got=$got-without-the-text ;;
  esac
  if [ $# -ge 4 ]; then
    case $out in
      *"first line: $4"*) ;;
      *) got=$got-without-the-first-line ;;
    esac
  fi
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$got"
  if [ "$got" != "$2" ]; then
    wrong="$wrong $1"
  elif [ "$2" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

self_test() {
  dir=$(cd "$(dirname "$0")" && pwd) || exit 2
  self=$dir/$(basename "$0")
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  mkdir -p "$work/bin"
  cat >"$work/bin/halvo" <<'EOF'
#!/bin/sh
[ "$1" = --check-page ] && [ "$2" = --config-dir ] && [ -n "$3" ] && [ "$4" = --within-s ] && [ "$5" = 2 ] || exit 98
case $STUB_MODE in
  loaded) printf 'page=loaded commands_ok=3 version=0.0.0\n' ;;
  two-commands) printf 'page=loaded commands_ok=2 version=0.0.0\n' ;;
  other-version) printf 'page=loaded commands_ok=3 version=9.9.9\n' ;;
  extra-word) printf 'page=loaded commands_ok=3 version=0.0.0 extra\n' ;;
  odd-bytes) printf 'page=loaded\033[31m commands_ok=3 version=0.0.0\r\n' ;;
  two-lines) printf 'page=loaded commands_ok=3 version=0.0.0\npage=loaded commands_ok=3 version=0.0.0\n' ;;
  no-line) ;;
  times-out)
    printf 'halvo: --check-page: the page did not have the reply of its 3 read commands within 2 s\n' >&2
    exit 1
    ;;
  setup-fails)
    printf 'page=loaded commands_ok=3 version=0.0.0\n'
    exit 101
    ;;
  hangs) exec sleep 30 ;;
esac
EOF
  chmod +x "$work/bin/halvo"
  rejected=0
  wrong=
  check loaded PASS 'check-page exit: 0, lines: 1, line matched: true' 'page=loaded commands_ok=3 version=0.0.0
'
  check two-commands FAIL 'printed another line than' 'page=loaded commands_ok=2 version=0.0.0
'
  check other-version FAIL 'printed another line than' 'page=loaded commands_ok=3 version=9.9.9
'
  check extra-word FAIL 'printed another line than' 'page=loaded commands_ok=3 version=0.0.0 extra
'
  check odd-bytes FAIL 'printed another line than' 'page=loaded??31m commands_ok=3 version=0.0.0?
'
  check two-lines FAIL 'printed 2 lines)' 'page=loaded commands_ok=3 version=0.0.0
'
  check no-line FAIL 'printed 0 lines)' '(none)
'
  check times-out FAIL 'halvo --check-page exited 1)' '(none)
'
  check setup-fails FAIL 'halvo --check-page exited 101)' 'page=loaded commands_ok=3 version=0.0.0
'
  check hangs FAIL 'ran past 4 s and was ended)'
  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  --fixture) fixture "$2" "$3" ;;
  '') measure ;;
  *) exit 64 ;;
esac
