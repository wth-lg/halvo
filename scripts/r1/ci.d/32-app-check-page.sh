#!/bin/sh
# jobs: macos windows
# rule: the built app loads its page and its read commands answer through the capability, and it prints when its event loop was ready; on Windows its window's display affinity is 17, WDA_EXCLUDEFROMCAPTURE, so that the system keeps the window out of screenshots, screen shares and screen recordings that are made through Windows' own capture features, Halvo's own screen record among them, and its web view's general autofill is off
# lane: alone
set -u
name=32-app-check-page
APP_CHECK_PAGE_MAX_S=60
expected_commands_ok=4
grace_s=30
shown_lines=20
shown_entries=40
shown_depth=3
shown_width=200

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

expected_service() {
  case ${CI_D_JOB:-} in
    macos) printf 'not_running\n' ;;
    windows) printf 'not_running\n' ;;
    *)
      case $(uname -s) in
        MINGW* | MSYS* | CYGWIN*) printf 'not_running\n' ;;
        *) printf 'not_running\n' ;;
      esac
      ;;
  esac
}

# What the line ends with in this job: on Windows the window's display affinity, 17, which keeps it out of
# screenshots, screen shares and screen recordings that are made through Windows' own capture features,
# Halvo's own screen record among them, and the web view's general autofill, off.
expected_tail() {
  case ${CI_D_JOB:-} in
    windows) printf ' display_affinity=17 general_autofill=off\n' ;;
    macos) printf '\n' ;;
    *)
      case $(uname -s) in
        MINGW* | MSYS* | CYGWIN*) printf ' display_affinity=17 general_autofill=off\n' ;;
        *) printf '\n' ;;
      esac
      ;;
  esac
}

run_check() {
  exe=$1
  data_dir=$2
  max_s=$3
  limit_s=$4
  out=$5
  err=$6
  dir_arg=$(native_path "$data_dir") || fail "the data directory has no native path"
  started=$(date +%s) || fail "the clock cannot be read"
  "$exe" --check-page --data-dir "$dir_arg" --within-s "$max_s" >"$out" 2>"$err" &
  pid=$!
  waited=0
  ended=false
  while kill -0 "$pid" 2>/dev/null; do
    if [ "$waited" -ge "$limit_s" ]; then
      kill "$pid" 2>/dev/null
      ended=true
      break
    fi
    sleep 1
    waited=$((waited + 1))
  done
  wait "$pid"
  check_code=$?
  check_seconds=$waited
  stopped=$(date +%s) || fail "the clock cannot be read"
  check_clock=$((stopped - started))
}

plain() {
  LC_ALL=C tr -c ' -~\n' '?' | LC_ALL=C tr '#' '?'
}

show_err() {
  plain <"$1" | awk -v keep="$shown_lines" -v width="$shown_width" '
    { line = substr($0, 1, width) }
    NR <= keep { first[NR] = line; next }
    { last[NR % keep] = line }
    END {
      printf "check-page err lines: %d\n", NR
      for (i = 1; i <= NR && i <= keep; i++) printf "check-page err: %s\n", first[i]
      from = keep + 1
      if (NR > 2 * keep) {
        printf "check-page err: (%d lines not shown)\n", NR - 2 * keep
        from = NR - keep + 1
      }
      for (i = from; i <= NR; i++) printf "check-page err: %s\n", last[i % keep]
    }' || fail "the error output of halvo --check-page cannot be shown"
}

show_data() {
  files=$(find "$1" -type f 2>/dev/null | wc -l | tr -d ' ') || fail "the data directory cannot be read"
  directories=$(find "$1" -mindepth 1 -type d 2>/dev/null | wc -l | tr -d ' ') || fail "the data directory cannot be read"
  dumps=$(find "$1" -type f -name '*.dmp' 2>/dev/null | wc -l | tr -d ' ') || fail "the data directory cannot be read"
  bytes=$(find "$1" -type f -exec wc -c {} + 2>/dev/null | awk 'NF == 2 && $2 == "total" { next } { sum += $1 } END { printf "%d\n", sum }') || fail "the data directory cannot be read"
  printf 'check-page data: %s files, %s directories, %s bytes, %s dump files\n' "$files" "$directories" "$bytes" "$dumps"
  (cd "$1" && find . -mindepth 1 -maxdepth "$shown_depth" 2>/dev/null) | LC_ALL=C sort | plain | awk -v keep="$shown_entries" -v width="$shown_width" '
    NR <= keep { printf "check-page data entry: %s\n", substr($0, 3, width) }
    END { if (NR > keep) printf "check-page data: (%d entries not shown)\n", NR - keep }' || fail "the data directory cannot be shown"
}

watch_app() {
  limit_s=$(($3 + grace_s))
  run_check "$1" "$2" "$3" "$limit_s" "$4" "$5"
  printf 'check-page seconds: %s, bound: %s\n' "$check_seconds" "$3"
  printf 'check-page clock seconds: %s\n' "$check_clock"
  ready_ms=$(plain <"$5" | sed -n 's/^halvo: --check-page: the event loop was ready at \([0-9][0-9]*\) ms$/\1/p' | head -n 1) || fail "the error output of halvo --check-page cannot be read"
  printf 'check-page ready ms: %s\n' "${ready_ms:-none}"
  show_err "$5"
  show_data "$2"
  if [ -n "$ready_ms" ]; then
    place="its event loop ready at $ready_ms ms"
  else
    place="its event loop not ready"
  fi
  [ "$ended" = false ] || fail "halvo --check-page ran past $limit_s s and was ended, $place"
}

verdict() {
  out=$1
  code=$2
  version=$3
  service=$4
  lines=$(grep -c '' "$out")
  content=$(cat "$out") || fail "the output of halvo --check-page cannot be read"
  tail=${6:-}
  expected="page=loaded commands_ok=$expected_commands_ok version=$version service=$service guard=installed$tail"
  first=$(head -n 1 "$out" | cut -c1-120 | LC_ALL=C tr -c 'A-Za-z0-9=._ \n-' '?') || fail "the first line of halvo --check-page cannot be read"
  [ -n "$first" ] || first='(none)'
  matched=false
  if [ "$lines" -eq 1 ] && [ "$content" = "$expected" ]; then
    matched=true
  fi
  printf 'check-page exit: %s, lines: %s, line matched: %s, first line: %s\n' "$code" "$lines" "$matched" "$first"
  [ "$code" -eq 0 ] || fail "halvo --check-page exited $code"
  [ "$lines" -eq 1 ] || fail "halvo --check-page printed $lines lines"
  [ "$matched" = true ] || fail "halvo --check-page printed another line than page=loaded commands_ok=$expected_commands_ok version=<the version> service=$service guard=installed$tail"
  [ -n "$5" ] || fail "halvo --check-page printed no time of the Ready of its event loop"
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
  data_dir=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail "mktemp failed"
  watch_app "$exe" "$data_dir" "$APP_CHECK_PAGE_MAX_S" "$artifacts/check-page.out" "$artifacts/check-page.err"
  service=$(expected_service) || fail "the expected service cannot be named"
  tail=$(expected_tail) || fail "the expected end of the line cannot be named"
  verdict "$artifacts/check-page.out" "$check_code" "$version" "$service" "$ready_ms" "$tail"
  git diff --quiet HEAD -- || fail "a tracked file differs from HEAD after the build and the check"
  printf 'PASS %s\n' "$name"
}

fixture() {
  exe=$1
  root=$2
  grace_s=2
  turn_now=1000
  sleep() {
    while kill -0 "$pid" 2>/dev/null && [ ! -f "$root/held" ]; do
      env true
    done
    turn_now=$((turn_now + 1))
  }
  date() {
    printf '%s\n' "$turn_now"
  }
  watch_app "$exe" "$root/data" 2 "$root/out" "$root/err"
  verdict "$root/out" "$check_code" 0.0.0 not_running "$ready_ms"
  printf 'PASS %s\n' "$name"
}

check() {
  mode=$1
  want=$2
  mkdir -p "$work/$mode/data"
  out=$(PATH=$work/realbin:$PATH REAL_SLEEP=$real_sleep REAL_TURNS=$work/$mode/real-turns STUB_MODE=$mode STUB_DATA=$work/$mode/data sh "$self" --fixture "$work/bin/halvo" "$work/$mode" 2>&1)
  code=$?
  real_turns=0
  [ -f "$work/$mode/real-turns" ] && real_turns=$(wc -l <"$work/$mode/real-turns" | tr -d ' ')
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
  if [ -n "$4" ]; then
    case $out in
      *"first line: $4"*) ;;
      *) got=$got-without-the-first-line ;;
    esac
  fi
  turns=$(printf '%s\n' "$out" | sed -n 's/^check-page seconds: \([0-9][0-9]*\), bound: 2$/\1/p')
  clock=$(printf '%s\n' "$out" | sed -n 's/^check-page clock seconds: \([0-9][0-9]*\)$/\1/p')
  if [ -z "$turns" ] || [ -z "$clock" ] || [ "$clock" -lt $((turns - 1)) ] || [ "$clock" -gt $((turns + 30)) ]; then
    got=$got-without-the-seconds
  fi
  shift 4
  printed=true
  for text in "$@"; do
    case $out in
      *"$text"*) ;;
      *) printed=false ;;
    esac
  done
  [ "$printed" = true ] || got=$got-without-the-print
  [ "$real_turns" = 0 ] || got=$got-with-a-real-turn
  case $out in
    *'##['*) got=$got-with-a-runner-command ;;
  esac
  printf 'case %s/%s want=%s got=%s\n' "$name" "$mode" "$want" "$got"
  if [ "$got" != "$want" ]; then
    wrong="$wrong $mode"
  elif [ "$want" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

self_test() {
  dir=$(cd "$(dirname "$0")" && pwd) || exit 2
  self=$dir/$(basename "$0")
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  mkdir -p "$work/bin" "$work/realbin"
  real_sleep=$(command -v sleep) || exit 2
  cat >"$work/realbin/sleep" <<'EOF'
#!/bin/sh
if [ "$1" = 1 ]; then
  printf 'turn\n' >>"$REAL_TURNS"
fi
exec "$REAL_SLEEP" "$@"
EOF
  chmod +x "$work/realbin/sleep"
  cat >"$work/bin/halvo" <<'EOF'
#!/bin/sh
[ "$1" = --check-page ] && [ "$2" = --data-dir ] && [ -n "$3" ] && [ "$4" = --within-s ] && [ "$5" = 2 ] || exit 98
case $STUB_MODE in
  loaded)
    printf 'halvo: --check-page: the event loop was ready at 15 ms\n' >&2
    printf 'halvo: --check-page: done in 19 ms\n' >&2
    printf 'page=loaded commands_ok=4 version=0.0.0 service=not_running guard=installed\n'
    ;;
  two-commands) printf 'page=loaded commands_ok=2 version=0.0.0 service=not_running guard=installed\n' ;;
  other-version) printf 'page=loaded commands_ok=4 version=9.9.9 service=not_running guard=installed\n' ;;
  extra-word) printf 'page=loaded commands_ok=4 version=0.0.0 service=not_running guard=installed extra\n' ;;
  other-service) printf 'page=loaded commands_ok=4 version=0.0.0 service=held_unreachable guard=installed\n' ;;
  no-service) printf 'page=loaded commands_ok=4 version=0.0.0\n' ;;
  long-line) printf 'page=loaded commands_ok=4 version=0.0.0 service=not_running guard=installed %s\n' 'abcdefghijklmnopqrstuvwxyz0123456789abcdefghijklmnopqrstuvwxyz0123456789' ;;
  odd-bytes) printf 'page=loaded\033[31m commands_ok=4 version=0.0.0\r\n' ;;
  two-lines) printf 'page=loaded commands_ok=4 version=0.0.0 service=not_running guard=installed\npage=loaded commands_ok=4 version=0.0.0 service=not_running guard=installed\n' ;;
  no-guard) printf 'page=loaded commands_ok=4 version=0.0.0 service=not_running\n' ;;
  guard-not-installed)
    printf 'halvo: the window main is not opened, because its engine guard is not installed: the rule list did not compile\n' >&2
    exit 3
    ;;
  no-line) ;;
  times-out)
    mkdir -p "$STUB_DATA/engine-rules" "$STUB_DATA/webview/EBWebView/Crashpad/reports"
    printf 'rules\n' >"$STUB_DATA/engine-rules/rules.txt"
    printf 'dump\n' >"$STUB_DATA/webview/EBWebView/Crashpad/reports/one.dmp"
    printf 'halvo: --check-page: the event loop was ready at 3 ms\n' >&2
    printf 'halvo: --check-page: the page did not have the reply of its 4 read commands within 2 s after the event loop was ready\n' >&2
    exit 1
    ;;
  odd-err)
    printf 'halvo: \033[31mred\351\r\na\000b\n%0300d\nno end' 0 >&2
    exit 1
    ;;
  many-err)
    i=1
    while [ "$i" -le 50 ]; do
      printf 'halvo: line %s\n' "$i" >&2
      i=$((i + 1))
    done
    exit 1
    ;;
  many-entries)
    i=10
    while [ "$i" -le 54 ]; do
      : >"$STUB_DATA/f$i"
      i=$((i + 1))
    done
    printf 'halvo: --check-page: the event loop was ready at 6 ms\n' >&2
    printf 'page=loaded commands_ok=4 version=0.0.0 service=not_running guard=installed\n'
    ;;
  edge)
    printf 'halvo: --check-page: the event loop was ready at 4 ms\n' >&2
    : >"$STUB_DATA/f10"
    i=11
    while [ "$i" -le 49 ]; do
      printf 'halvo: line %s\n' $((i - 9)) >&2
      : >"$STUB_DATA/f$i"
      i=$((i + 1))
    done
    printf 'page=loaded commands_ok=4 version=0.0.0 service=not_running guard=installed\n'
    ;;
  no-ready) printf 'page=loaded commands_ok=4 version=0.0.0 service=not_running guard=installed\n' ;;
  crlf-ready)
    printf 'halvo: --check-page: the event loop was ready at 12 ms\r\n' >&2
    printf 'page=loaded commands_ok=4 version=0.0.0 service=not_running guard=installed\n'
    ;;
  runner-commands)
    printf 'halvo: --check-page: the event loop was ready at 4 ms\n##[error]forged by the app\n::error::forged by the app\n' >&2
    : >"$STUB_DATA/##[warning]name"
    printf 'page=loaded commands_ok=4 version=0.0.0 service=not_running guard=installed\n'
    ;;
  setup-fails)
    printf 'page=loaded commands_ok=4 version=0.0.0 service=not_running guard=installed\n'
    exit 101
    ;;
  engine-start-past-the-limit)
    printf 'halvo: --check-page: the stub waits\n' >&2
    : >"$STUB_DATA/../held"
    exec sleep 30
    ;;
  page-past-the-limit)
    printf 'halvo: --check-page: the event loop was ready at 7 ms\n' >&2
    : >"$STUB_DATA/../held"
    exec sleep 30
    ;;
  odd-ready)
    printf 'noise halvo: --check-page: the event loop was ready at 1 ms\nhalvo: --check-page: the event loop was ready at 7x ms\nhalvo: --check-page: the event loop was ready at 8 ms\nhalvo: --check-page: the event loop was ready at 9 ms\n' >&2
    exit 1
    ;;
esac
EOF
  chmod +x "$work/bin/halvo"
  rejected=0
  wrong=
  zeros=$(printf '%0200d' 0)
  check loaded PASS 'check-page exit: 0, lines: 1, line matched: true' 'page=loaded commands_ok=4 version=0.0.0 service=not_running guard=installed
' 'check-page ready ms: 15
check-page err lines: 2
check-page err: halvo: --check-page: the event loop was ready at 15 ms
check-page err: halvo: --check-page: done in 19 ms
check-page data: 0 files, 0 directories, 0 bytes, 0 dump files
check-page exit: 0'
  check two-commands FAIL 'printed another line than' 'page=loaded commands_ok=2 version=0.0.0 service=not_running guard=installed
'
  check other-version FAIL 'printed another line than' 'page=loaded commands_ok=4 version=9.9.9 service=not_running guard=installed
'
  check extra-word FAIL 'printed another line than' 'page=loaded commands_ok=4 version=0.0.0 service=not_running guard=installed extra
'
  check odd-bytes FAIL 'printed another line than' 'page=loaded??31m commands_ok=4 version=0.0.0?
'
  check other-service FAIL 'printed another line than' 'page=loaded commands_ok=4 version=0.0.0 service=held_unreachable guard=installed
'
  check no-service FAIL 'printed another line than' 'page=loaded commands_ok=4 version=0.0.0
'
  check long-line FAIL 'printed another line than' 'page=loaded commands_ok=4 version=0.0.0 service=not_running guard=installed abcdefghijklmnopqrstuvwxyz0123456789abcdefgh
'
  check two-lines FAIL 'printed 2 lines)' 'page=loaded commands_ok=4 version=0.0.0 service=not_running guard=installed
'
  check no-guard FAIL 'printed another line than' 'page=loaded commands_ok=4 version=0.0.0 service=not_running
'
  check guard-not-installed FAIL 'halvo --check-page exited 3)' '(none)
' 'check-page err: halvo: the window main is not opened, because its engine guard is not installed: the rule list did not compile
'
  check no-line FAIL 'printed 0 lines)' '(none)
' 'check-page ready ms: none
check-page err lines: 0
check-page data: 0 files, 0 directories, 0 bytes, 0 dump files
check-page exit: 0'
  check times-out FAIL 'halvo --check-page exited 1)' '(none)
' 'check-page ready ms: 3
check-page err lines: 2
check-page err: halvo: --check-page: the event loop was ready at 3 ms
check-page err: halvo: --check-page: the page did not have the reply of its 4 read commands within 2 s after the event loop was ready
check-page data: 2 files, 5 directories, 11 bytes, 1 dump files
check-page data entry: engine-rules
check-page data entry: engine-rules/rules.txt
check-page data entry: webview
check-page data entry: webview/EBWebView
check-page data entry: webview/EBWebView/Crashpad
check-page exit: 1, lines: 0, line matched: false, first line: (none)
'
  check odd-err FAIL 'halvo --check-page exited 1)' '(none)
' "check-page err lines: 4
check-page err: halvo: ?[31mred??
check-page err: a?b
check-page err: $zeros
check-page err: no end
check-page data: "
  check many-err FAIL 'halvo --check-page exited 1)' '(none)
' 'check-page err lines: 50
check-page err: halvo: line 1
' 'check-page err: halvo: line 20
check-page err: (10 lines not shown)
check-page err: halvo: line 31
' 'check-page err: halvo: line 50
check-page data: '
  check many-entries PASS 'check-page exit: 0, lines: 1, line matched: true' 'page=loaded commands_ok=4 version=0.0.0 service=not_running guard=installed
' 'check-page data: 45 files, 0 directories, 0 bytes, 0 dump files
check-page data entry: f10
' 'check-page data entry: f49
check-page data: (5 entries not shown)
check-page exit: 0'
  check edge PASS 'check-page exit: 0, lines: 1, line matched: true' 'page=loaded commands_ok=4 version=0.0.0 service=not_running guard=installed
' 'check-page err lines: 40
check-page err: halvo: --check-page: the event loop was ready at 4 ms
check-page err: halvo: line 2
' 'check-page err: halvo: line 20
check-page err: halvo: line 21
' 'check-page err: halvo: line 40
check-page data: 40 files, 0 directories, 0 bytes, 0 dump files
check-page data entry: f10
' 'check-page data entry: f49
check-page exit: 0'
  check no-ready FAIL 'printed no time of the Ready of its event loop)' 'page=loaded commands_ok=4 version=0.0.0 service=not_running guard=installed
' 'check-page ready ms: none
'
  check crlf-ready FAIL 'printed no time of the Ready of its event loop)' 'page=loaded commands_ok=4 version=0.0.0 service=not_running guard=installed
' 'check-page ready ms: none
check-page err lines: 1
check-page err: halvo: --check-page: the event loop was ready at 12 ms?
'
  check runner-commands PASS 'check-page exit: 0, lines: 1, line matched: true' 'page=loaded commands_ok=4 version=0.0.0 service=not_running guard=installed
' 'check-page ready ms: 4
check-page err lines: 3
check-page err: halvo: --check-page: the event loop was ready at 4 ms
check-page err: ??[error]forged by the app
check-page err: ::error::forged by the app
check-page data: 1 files, 0 directories, 0 bytes, 0 dump files
check-page data entry: ??[warning]name
check-page exit: 0'
  check setup-fails FAIL 'halvo --check-page exited 101)' 'page=loaded commands_ok=4 version=0.0.0 service=not_running guard=installed
'
  check engine-start-past-the-limit FAIL 'ran past 4 s and was ended, its event loop not ready)' '' 'check-page seconds: 4, bound: 2
' 'check-page ready ms: none
check-page err lines: 1
check-page err: halvo: --check-page: the stub waits
check-page data: 0 files, 0 directories, 0 bytes, 0 dump files
FAIL 32-app-check-page (halvo --check-page ran past 4 s and was ended, its event loop not ready)'
  check page-past-the-limit FAIL 'ran past 4 s and was ended, its event loop ready at 7 ms)' '' 'check-page seconds: 4, bound: 2
' 'check-page ready ms: 7
check-page err lines: 1
check-page err: halvo: --check-page: the event loop was ready at 7 ms
check-page data: 0 files, 0 directories, 0 bytes, 0 dump files
FAIL 32-app-check-page (halvo --check-page ran past 4 s and was ended, its event loop ready at 7 ms)'
  check odd-ready FAIL 'halvo --check-page exited 1)' '(none)
' 'check-page ready ms: 8
check-page err lines: 4
'
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
