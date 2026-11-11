#!/bin/sh
# Tests of the egress check that send nothing off the machine: sh scripts/egress/test.sh [probe|lists|sandbox|wfp|wfp-rules]...
set -u
LC_ALL=C
export LC_ALL
root=$(git rev-parse --show-toplevel) || {
  printf 'FAIL test (not in a work tree)\n'
  exit 1
}
cd "$root" || exit 1
work=$(mktemp -d "${TMPDIR:-/tmp}/egress-test.XXXXXX") || exit 1
failed=0
passed=0
serves=0
bin=target/egress/bin
exe=
case $(uname -s) in
  MINGW* | MSYS* | CYGWIN*) exe=.exe ;;
esac

result() {
  if [ "$2" = "$3" ]; then
    passed=$((passed + 1))
    printf 'ok %s\n' "$1"
  else
    failed=$((failed + 1))
    printf 'not ok %s: want %s, got %s\n' "$1" "$2" "$3"
  fi
}

expect_exit() {
  label=$1
  want=$2
  shift 2
  "$@" >"$work/out" 2>"$work/err"
  result "$label" "$want" "$?"
}

expect_refusal() {
  label=$1
  cause=$2
  shift 2
  "$@" >"$work/out" 2>"$work/err"
  code=$?
  line=$(cat "$work/err")
  case $code:$line in
    "2:REFUSED $cause: "*) got=refused-$cause ;;
    *) got="exit-$code:$line" ;;
  esac
  if [ -s "$work/out" ]; then
    got=$got-with-stdout
  fi
  result "$label" "refused-$cause" "$got"
}

serve() {
  serves=$((serves + 1))
  stop=$work/stop-$serves
  "$bin/egress-spawn$exe" serve "$1" "$stop" "$work/ready-$serves" &
  served=$!
  n=0
  while [ ! -f "$work/ready-$serves" ] && [ $n -lt 100 ]; do
    sleep 0.05
    n=$((n + 1))
  done
}

unserve() {
  : >"$stop"
  wait "$served"
}

test_probe() {
  expect_exit probe/none-exits-0 0 sh scripts/egress/probe.sh none
  for name in egress-spawn egress-probe; do
    if [ -x "$bin/$name$exe" ]; then
      result "probe/binary-$name-built" yes yes
    else
      result "probe/binary-$name-built" yes no
    fi
  done
  expect_refusal probe/unknown-name usage sh scripts/egress/probe.sh nothing
  expect_refusal probe/no-name usage sh scripts/egress/probe.sh
  expect_refusal probe/two-names usage sh scripts/egress/probe.sh none none
  expect_exit probe/loopback-without-listener-exits-1 1 sh scripts/egress/probe.sh loopback
  serve 47201
  expect_exit probe/loopback-with-listener-exits-0 0 sh scripts/egress/probe.sh loopback
  unserve
  serve 47202
  expect_exit probe/loopback2-with-listener-exits-0 0 sh scripts/egress/probe.sh loopback2
  unserve
  expect_exit probe/listen-on-a-free-port-exits-0 0 sh scripts/egress/probe.sh listen
  serve 47201
  expect_exit probe/listen-on-a-taken-port-exits-1 1 sh scripts/egress/probe.sh listen
  unserve
  expect_exit probe/serve-on-a-taken-port-exits-3 3 sh -c "\"$bin/egress-spawn$exe\" serve 47201 '$work/s1' '$work/r1' & first=\$!; sleep 0.5; \"$bin/egress-spawn$exe\" serve 47201 '$work/s2' '$work/r2'; code=\$?; : >'$work/s1'; wait \$first; exit \$code"
  printf '%s\n' 'exit 7' >"$work/seven.sh"
  "$bin/egress-spawn$exe" run "$work/status" -- sh "$work/seven.sh"
  code=$?
  got=missing
  read -r got <"$work/status"
  result probe/run-records-exit-code "0:7" "$code:$got"
  "$bin/egress-spawn$exe" run "$work/status2" -- "$work/no-such-binary"
  code=$?
  got=missing
  read -r got <"$work/status2"
  result probe/run-records-not-started "0:not-started" "$code:$got"
  if [ -z "$exe" ]; then
    printf '%s\n' 'kill -9 $$' >"$work/nine.sh"
    "$bin/egress-spawn$exe" run "$work/status3" -- sh "$work/nine.sh"
    code=$?
    got=missing
    read -r got <"$work/status3"
    result probe/run-records-signal "0:signal-9" "$code:$got"
    got=$("$bin/egress-spawn$exe" run fd3 -- sh -c 'printf 7 >&3 2>/dev/null || printf closed' 3>"$work/fd3-status")
    code=$?
    result probe/run-fd3-not-inherited "0:closed:0" "$code:$got:$(cat "$work/fd3-status")"
  fi
}

other_mode() {
  if [ -n "$exe" ]; then
    printf '%s\n' sandbox
  else
    printf '%s\n' pf
  fi
}

list_case() {
  printf '%b' "$3" >"$work/list-$1.txt"
  expect_refusal "lists/$1" "$2" sh scripts/egress/check.sh --mode "$mode" "$work/list-$1.txt" -- sh scripts/egress/probe.sh none
  if [ -n "${4:-}" ]; then
    detail=$(cat "$work/err")
    case $detail in
      *"$4") result "lists/$1-detail" yes yes ;;
      *) result "lists/$1-detail" "...$4" "$detail" ;;
    esac
  fi
}

test_lists() {
  mode=$(other_mode)
  expect_refusal args/none usage sh scripts/egress/check.sh
  expect_refusal args/unknown-option usage sh scripts/egress/check.sh --verbose scripts/egress/lists/empty.txt -- true
  expect_refusal args/unknown-mode usage sh scripts/egress/check.sh --mode iptables scripts/egress/lists/empty.txt -- true
  expect_refusal args/mode-without-value usage sh scripts/egress/check.sh --mode
  expect_refusal args/mode-twice usage sh scripts/egress/check.sh --mode "$mode" --mode "$mode" scripts/egress/lists/empty.txt -- true
  expect_refusal args/list-without-dashes usage sh scripts/egress/check.sh scripts/egress/lists/empty.txt true
  expect_refusal args/no-command usage sh scripts/egress/check.sh scripts/egress/lists/empty.txt --
  expect_refusal args/dashes-first usage sh scripts/egress/check.sh -- true
  expect_refusal args/control-alone usage sh scripts/egress/check.sh --control-no-deny
  expect_refusal args/control-off-a-runner usage env -u GITHUB_ACTIONS sh scripts/egress/check.sh --self-test --control-no-deny
  expect_refusal args/self-test-with-list usage sh scripts/egress/check.sh --self-test scripts/egress/lists/empty.txt -- true
  sh scripts/egress/check.sh --help >"$work/help.out" 2>"$work/help.err"
  code=$?
  got=$(grep -c -e '^sh scripts/egress/check.sh \[--mode MODE\] LISTFILE -- COMMAND \[ARG\.\.\.\]$' -e '^sh scripts/egress/check.sh \[--mode MODE\] --self-test \[--control-no-deny\]$' -e '^sh scripts/egress/probe.sh NAME$' "$work/help.out")
  result args/help "0 3" "$code $got"
  cp scripts/egress/lists/empty.txt "$work/a
b.txt"
  expect_refusal args/list-path-with-a-newline usage sh scripts/egress/check.sh "$work/a
b.txt" -- true
  cp scripts/egress/lists/empty.txt "$work/a	b.txt"
  expect_refusal args/list-path-with-a-tab usage sh scripts/egress/check.sh "$work/a	b.txt" -- true
  for byte in 001 037; do
    odd=$(printf "a\\${byte}b.txt")
    cp scripts/egress/lists/empty.txt "$work/$odd"
    expect_refusal "args/list-path-with-byte-$byte" usage sh scripts/egress/check.sh "$work/$odd" -- true
  done
  (cd "$work" && expect_refusal args/outside-a-repository no-repository sh "$root/scripts/egress/check.sh" "$root/scripts/egress/lists/empty.txt" -- true)
  expect_refusal lists/committed-empty mode sh scripts/egress/check.sh --mode "$mode" scripts/egress/lists/empty.txt -- sh scripts/egress/probe.sh none
  expect_refusal lists/committed-loopback mode sh scripts/egress/check.sh --mode "$mode" scripts/egress/lists/loopback.txt -- sh scripts/egress/probe.sh none
  size=$(wc -c <scripts/egress/lists/empty.txt)
  result lists/empty-is-16-bytes 16 "$((size))"
  list_case ordered mode '[fixed]\n127.0.0.1:47201\n127.0.0.1:47202\n[::1]:80\n[agent]\n192.0.2.1:443\nexample.com:443\n'
  list_case ipv6-forms mode '[fixed]\n[2001:db8:0:1:1:1:1:1]:443\n[2001:db8::1:0:0:1]:1\n[2001:db8::1]:443\n[::]:1\n[agent]\n'
  list_case name-forms mode '[fixed]\na-b.example:1\nlocalhost:1\nx.0y:2\n[agent]\n'
  expect_refusal lists/missing-file list sh scripts/egress/check.sh --mode "$mode" "$work/no-such-list.txt" -- sh scripts/egress/probe.sh none
  list_case zero-bytes list ''
  list_case fixed-only list '[fixed]\n'
  list_case agent-first list '[agent]\n[fixed]\n' 'line 1'
  list_case agent-twice list '[fixed]\n[agent]\n[agent]\n' 'line 3'
  list_case fixed-twice list '[fixed]\n[fixed]\n[agent]\n' 'line 2'
  list_case entry-before-fixed list '127.0.0.1:1\n[fixed]\n[agent]\n' 'line 1'
  list_case blank-line list '[fixed]\n\n[agent]\n' 'line 2'
  list_case trailing-space list '[fixed]\n127.0.0.1:1 \n[agent]\n' 'line 2'
  list_case comment list '[fixed]\n# allowed\n[agent]\n' 'line 2'
  list_case crlf list '[fixed]\r\n[agent]\r\n' 'line 1'
  list_case no-final-newline list '[fixed]\n[agent]'
  list_case bom list '\0357\0273\0277[fixed]\n[agent]\n' 'line 1'
  list_case nul list '[fixed]\n[agent]\n\0000\n'
  list_case non-ascii list '[fixed]\nexampl\0303\0251.com:1\n[agent]\n' 'line 2'
  list_case ipv4-leading-zero list '[fixed]\n127.0.0.01:1\n[agent]\n' 'line 2'
  list_case ipv4-octet-256 list '[fixed]\n127.0.0.256:1\n[agent]\n' 'line 2'
  list_case ipv4-three-parts list '[fixed]\n127.0.1:1\n[agent]\n' 'line 2'
  list_case port-zero list '[fixed]\n127.0.0.1:0\n[agent]\n' 'line 2'
  list_case port-65536 list '[fixed]\n127.0.0.1:65536\n[agent]\n' 'line 2'
  list_case port-leading-zero list '[fixed]\n127.0.0.1:080\n[agent]\n' 'line 2'
  list_case no-port list '[fixed]\n127.0.0.1\n[agent]\n' 'line 2'
  list_case wildcard list '[fixed]\n*.example.com:443\n[agent]\n' 'line 2'
  list_case ipv6-upper list '[fixed]\n[2001:DB8::1]:443\n[agent]\n' 'line 2'
  list_case ipv6-uncompressed list '[fixed]\n[2001:db8:0:0:0:0:0:1]:443\n[agent]\n' 'line 2'
  list_case ipv6-one-group-compressed list '[fixed]\n[2001:db8::1:1:1:1:1]:443\n[agent]\n' 'line 2'
  list_case ipv6-shorter-run-compressed list '[fixed]\n[2001::1:0:0:0:1]:443\n[agent]\n' 'line 2'
  list_case ipv6-leading-zero list '[fixed]\n[2001:0db8::1]:443\n[agent]\n' 'line 2'
  list_case ipv6-ipv4-part list '[fixed]\n[::ffff:1.2.3.4]:443\n[agent]\n' 'line 2'
  list_case ipv6-zone list '[fixed]\n[fe80::1%en0]:443\n[agent]\n' 'line 2'
  list_case ipv6-unbracketed list '[fixed]\n::1:443\n[agent]\n' 'line 2'
  list_case name-upper list '[fixed]\nExample.com:443\n[agent]\n' 'line 2'
  list_case name-digit-last-label list '[fixed]\na.1:80\n[agent]\n' 'line 2'
  list_case name-hex-last-label list '[fixed]\na.0x1f:80\n[agent]\n' 'line 2'
  list_case name-hyphen-start list '[fixed]\n-a.example:80\n[agent]\n' 'line 2'
  list_case name-empty-label list '[fixed]\na..example:80\n[agent]\n' 'line 2'
  list_case out-of-order list '[fixed]\n127.0.0.1:2\n127.0.0.1:1\n[agent]\n' 'line 3'
  list_case twice list '[fixed]\n127.0.0.1:1\n127.0.0.1:1\n[agent]\n' 'line 3'
  list_case order-per-heading mode '[fixed]\n127.0.0.1:2\n[agent]\n127.0.0.1:1\n'
}

run_check() {
  label=$1
  shift
  "$@" >"$work/out" 2>"$work/err"
  code=$?
  last=$(tail -n 1 "$work/err")
  log=${last##* log=}
  case $last in
    "egress mode="*) ;;
    *) log=/nonexistent ;;
  esac
}

field_of() {
  case " $last " in
    *" $1="*)
      value=${last#* "$1"=}
      printf '%s\n' "${value%% *}"
      ;;
    *) printf '%s\n' missing ;;
  esac
}

expect_run() {
  label=$1
  want_code=$2
  want_status=$3
  want_denied=$4
  shift 4
  run_check "$label" "$@"
  status=$(field_of command)
  denied=$(field_of denied)
  if [ "$want_denied" != 0 ] && [ "$denied" = 0 ]; then
    printf 'retry %s: the run logged no line\n' "$label"
    run_check "$label" "$@"
    status=$(field_of command)
    denied=$(field_of denied)
  fi
  result "$label" "$want_code $want_status $want_denied" "$code $status $denied"
}

logged() {
  status=$(field_of command)
  denied=$(field_of denied)
  case $denied in
    '' | 0 | *[!0-9]*) result "$1" "$2 denied>0" "$code $status denied=$denied" ;;
    *) result "$1" "$2 denied>0" "$code $status denied>0" ;;
  esac
}

expect_logged() {
  if [ -f "$log" ] && grep -q "$2" "$log"; then
    result "$1" yes yes
  else
    result "$1" "a line with $2" "no such line"
  fi
}

test_sandbox() {
  e=scripts/egress/lists/empty.txt
  l=scripts/egress/lists/loopback.txt
  pr=scripts/egress/probe.sh
  expect_run sandbox/quiet 0 0 0 sh scripts/egress/check.sh "$e" -- sh "$pr" none
  got=$(field_of mode) || got=
  size=$(wc -c <"$log") || size=
  result sandbox/quiet-mode "sandbox 0" "$got $((size))"
  expect_run sandbox/loopback-denied 1 1 1 sh scripts/egress/check.sh "$e" -- sh "$pr" loopback
  expect_logged sandbox/loopback-line ' proto=tcp dest=127.0.0.1:47201 pid=[0-9]* image=egress-probe$'
  serve 47201
  serve_first=$served
  stop_first=$stop
  serve 47202
  expect_run sandbox/listed-reached 0 0 0 sh scripts/egress/check.sh "$l" -- sh "$pr" loopback
  expect_run sandbox/unlisted-denied 1 1 1 sh scripts/egress/check.sh "$l" -- sh "$pr" loopback2
  expect_logged sandbox/unlisted-line ' proto=tcp dest=127.0.0.1:47202 pid=[0-9]* image=egress-probe$'
  unserve
  served=$serve_first
  stop=$stop_first
  unserve
  expect_run sandbox/listen-inside 0 0 0 sh scripts/egress/check.sh "$l" -- sh "$pr" listen
  run_check sandbox/other-loopback-host-on-a-listed-port sh scripts/egress/check.sh "$l" -- sh -c '/usr/bin/nc -z -G 2 127.0.0.2 47201; /usr/bin/nc -z -G 2 127.0.0.2 47201; exit 0'
  logged sandbox/other-loopback-host-on-a-listed-port "1 0"
  run_check sandbox/denial-then-exit-0 sh scripts/egress/check.sh "$e" -- sh -c 'sh scripts/egress/probe.sh loopback; sh scripts/egress/probe.sh loopback2; exit 0'
  logged sandbox/denial-then-exit-0 "1 0"
  sh scripts/egress/check.sh "$e" -- sh -c ': >"$1"; sleep 4' sh "$work/stream-started" >"$work/stream.out" 2>"$work/stream.err" &
  stream_check=$!
  n=0
  while [ ! -f "$work/stream-started" ] && [ $n -lt 300 ]; do
    sleep 0.1
    n=$((n + 1))
  done
  stream_state=$(grep -l "^pid=$stream_check\$" target/egress/[0-9]*/state | head -n 1)
  kill "$(sed -n 's/^stream=//p' "$stream_state")"
  wait "$stream_check"
  code=$?
  result sandbox/stream-lost-before-its-end 'exit-2:REFUSED mode: the log stream gave no end marker' "exit-$code:$(tail -n 1 "$work/stream.err")"
  expect_run sandbox/exit-code-passes 1 3 0 sh scripts/egress/check.sh "$e" -- sh -c 'exit 3'
  expect_run sandbox/not-started 1 not-started 0 sh scripts/egress/check.sh "$e" -- "$work/no-such-command"
  expect_run sandbox/signal 1 signal-9 0 sh scripts/egress/check.sh "$e" -- sh -c 'kill -9 $$'
  expect_run sandbox/stdout-passes 0 0 0 sh scripts/egress/check.sh "$e" -- printf 'through\n'
  got=$(cat "$work/out") || got=
  result sandbox/stdout-text through "$got"
  printf 'from-stdin\n' >"$work/in"
  sh scripts/egress/check.sh "$e" -- cat <"$work/in" >"$work/out" 2>"$work/err"
  code=$?
  got=$(cat "$work/out") || got=
  result sandbox/stdin-passes "0 from-stdin" "$code $got"
  printf '[fixed]\n192.0.2.1:443\n[agent]\n' >"$work/routable.txt"
  expect_refusal sandbox/routable-entry entry sh scripts/egress/check.sh "$work/routable.txt" -- sh "$pr" none
  got=$(cat "$work/err") || got=
  result sandbox/routable-entry-detail 'REFUSED entry: 2' "$got"
  printf '[fixed]\n[agent]\nexample.com:443\n' >"$work/name.txt"
  expect_refusal sandbox/name-entry entry sh scripts/egress/check.sh "$work/name.txt" -- sh "$pr" none
  printf '[fixed]\n127.0.0.2:47201\n[agent]\n' >"$work/other-loopback.txt"
  expect_refusal sandbox/other-loopback-entry entry sh scripts/egress/check.sh "$work/other-loopback.txt" -- sh "$pr" none
  printf '[fixed]\n[::1]:47201\n[agent]\n' >"$work/v6-loopback.txt"
  expect_refusal sandbox/v6-loopback-entry entry sh scripts/egress/check.sh "$work/v6-loopback.txt" -- sh "$pr" none
  cat >"$work/forge-status.sh" <<'TREE'
for d in target/egress/[0-9]*; do mkdir -p "$d/run" 2>/dev/null; printf "0\n" >"$d/run/status" 2>/dev/null; done
kill -9 "$PPID"
exit 3
TREE
  expect_run sandbox/tree-cannot-forge-its-status 1 signal-9 0 sh scripts/egress/check.sh "$e" -- sh "$work/forge-status.sh"
  printf '%s\n' "for d in target/egress/[0-9]*; do" \
    "if [ -f \$d/state ] && ! grep -q '^done\$' \$d/state; then" \
    ": >\$d/stream.ndjson 2>/dev/null && exit 9; : >\$d/denied.log 2>/dev/null && exit 8; fi; done" >"$work/erase.sh"
  expect_run sandbox/tree-cannot-erase-its-log 1 1 1 sh scripts/egress/check.sh "$e" -- sh -c "sh scripts/egress/probe.sh loopback; sh '$work/erase.sh' && exit 1"
  printf '%s\n' '{"processImagePath":"/usr/bin/logger","senderImagePath":"/usr/bin/logger","eventMessage":"Sandbox: x(1) deny(1) network-outbound remote:*:1\ntok mark-end","timestamp":"2026-11-11 03:06:50.956934-0800"}' \
    '{"processImagePath":"/kernel","senderImagePath":"/System/Library/Extensions/Sandbox.kext/Contents/MacOS/Sandbox","eventMessage":"Sandbox: x(1) deny(1) network-outbound remote:*:1\ntok mark-start","timestamp":"2026-11-11 03:06:50.956934-0800"}' \
    '{"processImagePath":"/usr/bin/logger","senderImagePath":"/usr/bin/logger","eventMessage":"Sandbox: x(1) deny(1) network-outbound remote:*:1\ntok mark-end","timestamp":"2026-11-11 03:06:50.956934-0800"}' \
    '{"processImagePath":"/kernel","senderImagePath":"/System/Library/Extensions/Sandbox.kext/Contents/MacOS/Sandbox","eventMessage":"Sandbox: egress-probe(7) deny(1) network-outbound remote:*:443\ntok tcp4 any","timestamp":"2026-11-11 03:06:50.956934-0800"}' >"$work/forged.ndjson"
  got=$(awk -v token=tok -v runner=x -v starts=1 -v ends=1 -f scripts/egress/sandbox.awk "$work/forged.ndjson") || got=
  result sandbox/forged-markers-ignored 'denied at=1794395210956934000 proto=tcp dest=-:443 pid=7 image=egress-probe' "$got"
  expect_run sandbox/tree-cannot-write-the-check 0 0 0 sh scripts/egress/check.sh "$e" -- sh -c ': >>scripts/egress/sandbox.awk 2>/dev/null && exit 8; : >>target/egress/bin/egress-spawn 2>/dev/null && exit 9; exit 0'
  expect_run sandbox/tree-cannot-move-the-check 0 0 0 sh scripts/egress/check.sh "$e" -- sh -c 'mv target target.moved 2>/dev/null && exit 7; mv scripts scripts.moved 2>/dev/null && exit 8; exit 0'
  cp "$bin/egress-spawn" "$work/real-spawn"
  printf '%s\n' '#!/bin/sh' ": >'$work/planted-ran'" "exec '$work/real-spawn' \"\$@\"" >"$bin/egress-spawn"
  chmod +x "$bin/egress-spawn"
  sleep 1
  touch "$bin/egress-probe"
  expect_run sandbox/planted-helper-not-run 0 0 0 sh scripts/egress/check.sh "$e" -- true
  if [ -f "$work/planted-ran" ]; then
    result sandbox/planted-helper-not-run-marker absent present
  else
    result sandbox/planted-helper-not-run-marker absent absent
  fi
  copy=$work/copy
  mkdir -p "$copy/scripts/egress/lists"
  cp scripts/egress/check.sh scripts/egress/list.awk scripts/egress/sandbox.awk scripts/egress/probe.sh scripts/egress/probe.rs scripts/egress/wfp.ps1 "$copy/scripts/egress/"
  cp scripts/egress/lists/empty.txt "$copy/scripts/egress/lists/"
  git init -q "$copy"
  (cd "$copy" && sh scripts/egress/check.sh scripts/egress/lists/empty.txt -- sh -c ': >"$1"; sleep 4' sh "$work/copy-started" >"$work/copy.out" 2>"$work/copy.err") &
  copy_check=$!
  n=0
  while [ ! -f "$work/copy-started" ] && [ $n -lt 300 ]; do
    sleep 0.1
    n=$((n + 1))
  done
  printf '%s\n' '# changed' >>"$copy/scripts/egress/sandbox.awk"
  wait "$copy_check"
  code=$?
  got=$(tail -n 1 "$work/copy.err")
  result sandbox/script-changed-during-run 'exit-2:REFUSED mode: a script of the check changed' "exit-$code:$got"
  cat >"$work/forge-kill.sh" <<'TREE'
state=$(ls -t target/egress/[0-9]*/state | head -n 1)
token=$(sed -n "s/^token=//p" "$state")
stream=$(sed -n "s/^stream=//p" "$state")
/usr/bin/logger "$token mark-end"
sleep 1
kill "$stream" 2>/dev/null && exit 7
exit 0
TREE
  expect_run sandbox/tree-cannot-end-the-stream 0 0 0 sh scripts/egress/check.sh "$e" -- sh "$work/forge-kill.sh"
  head -n 1 "$work/forged.ndjson" >"$work/forged-end.ndjson"
  awk -v token=tok -v mark=end -v runner=x -v ends=1 -f scripts/egress/sandbox.awk "$work/forged-end.ndjson" >/dev/null 2>&1
  result sandbox/forged-end-marker-not-taken 1 "$?"
  sed -n '2p' "$work/forged.ndjson" >"$work/kernel-start.ndjson"
  awk -v token=tok -v mark=start -v runner=x -v starts=1 -f scripts/egress/sandbox.awk "$work/kernel-start.ndjson" >/dev/null 2>&1
  awk -v token=tok -v mark=start -v runner=x -v starts=2 -f scripts/egress/sandbox.awk "$work/kernel-start.ndjson" >/dev/null 2>&1
  result sandbox/start-marker-of-another-process-not-taken 1 "$?"
  kernel='"processImagePath":"/kernel","senderImagePath":"/System/Library/Extensions/Sandbox.kext/Contents/MacOS/Sandbox"'
  kernel_fields=$kernel
  printf '%s\n' "{$kernel,\"eventMessage\":\"Sandbox: x(1) deny(1) network-outbound remote:*:47202\\ntok mark-start\",\"timestamp\":\"2026-11-11 03:06:50.956934-0800\"}" \
    "{$kernel,\"eventMessage\":\"a record of another form tok\",\"timestamp\":\"2026-11-11 03:06:51.956934-0800\"}" >"$work/unreadable.ndjson"
  got=$(awk -v token=tok -v runner=x -v starts=1 -v ends=1 -f scripts/egress/sandbox.awk "$work/unreadable.ndjson" | grep -c 'proto=- dest=- pid=- image=-')
  result sandbox/unreadable-record-counts 1 "$got"
  result sandbox/kernel-start-marker-taken 0 "$?"
  nl='
'
  cp "$bin/egress-probe" "$work/a${nl}b"
  expect_run sandbox/newline-in-a-process-name 1 1 1 sh scripts/egress/check.sh "$e" -- "$work/a${nl}b" connect loopback
  cat >"$work/forge-name.sh" <<'TREE'
state=$(ls -t target/egress/[0-9]*/state | head -n 1)
token=$(sed -n "s/^token=//p" "$state")
nl='
'
/usr/bin/touch "target/egress/x${nl}${token} mark-end" 2>/dev/null
sleep 1
sh scripts/egress/probe.sh loopback
TREE
  run_check sandbox/resolver-service-line sh scripts/egress/check.sh "$e" -- sh "$pr" resolver
  if [ -f "$log" ] && grep -q ' proto=dns dest=resolver ' "$log" && ! grep -q ' proto=- dest=- ' "$log"; then
    result sandbox/resolver-service-line yes yes
  else
    result sandbox/resolver-service-line "a proto=dns line and no proto=- dest=- line" "$(tr '\n' ';' <"$log" 2>/dev/null)"
  fi
  printf '%s\n' "{$kernel_fields,\"eventMessage\":\"Sandbox: x(1) deny(1) network-outbound remote:*:47202\\ntok mark-start\",\"timestamp\":\"2026-11-11 03:06:50.956934-0800\"}" \
    '{"processImagePath":"/kernel","senderImagePath":"/usr/bin/logger","eventMessage":"Sandbox: y(5) deny(1) network-outbound remote:*:443\ntok tcp4 any","timestamp":"2026-11-11 03:06:51.956934-0800"}' \
    '{"processImagePath":"/usr/bin/logger","senderImagePath":"/System/Library/Extensions/Sandbox.kext/Contents/MacOS/Sandbox","eventMessage":"Sandbox: y(6) deny(1) network-outbound remote:*:443\ntok tcp4 any","timestamp":"2026-11-11 03:06:51.956934-0800"}' >"$work/one-field.ndjson"
  got=$(awk -v token=tok -v runner=x -v starts=1 -v ends=1 -f scripts/egress/sandbox.awk "$work/one-field.ndjson" | grep -c '')
  result sandbox/only-kernel-sandbox-records 0 "$got"
  run_check sandbox/end-marker-in-a-file-name sh scripts/egress/check.sh "$e" -- sh "$work/forge-name.sh"
  logged sandbox/end-marker-in-a-file-name "1 1"
  (cd "$copy" && sh scripts/egress/check.sh scripts/egress/lists/empty.txt -- sh -c ': >"$1"; sleep 4' sh "$work/moved-started" >"$work/moved.out" 2>"$work/moved.err") &
  moved_check=$!
  n=0
  while [ ! -f "$work/moved-started" ] && [ $n -lt 300 ]; do
    sleep 0.1
    n=$((n + 1))
  done
  mv "$copy/scripts/egress" "$copy/scripts/egress-moved"
  cp -R "$copy/scripts/egress-moved" "$copy/scripts/egress"
  wait "$moved_check"
  code=$?
  result sandbox/directory-moved-during-run 'exit-2:REFUSED mode: a directory of the check was moved during the run' "exit-$code:$(tail -n 1 "$work/moved.err")"
  first_log=$log
  expect_run sandbox/second-run 0 0 0 sh scripts/egress/check.sh "$e" -- sh "$pr" none
  if [ "$first_log" != "$log" ]; then
    result sandbox/runs-get-new-directories yes yes
  else
    result sandbox/runs-get-new-directories yes no
  fi
  sh scripts/egress/check.sh --self-test >"$work/self-test.out" 2>"$work/self-test.err"
  code=$?
  got=$(awk '/^(probe|unlisted) / && $NF !~ /^lost=[0-9]+$/ { bad++ } /^checks / { checks = $0 } END { printf "%d %s", bad, checks }' "$work/self-test.out")
  result sandbox/self-test-lines-carry-lost "0 0 checks probes=ok quiet=ok loopback=ok default=ok" "$code $got"
  serve 47201
  expect_refusal sandbox/self-test-port-taken port sh scripts/egress/check.sh --self-test
  unserve
  expect_refusal sandbox/self-test-unknown-argument usage sh scripts/egress/check.sh --self-test --verbose
  expect_refusal sandbox/pf-off-a-runner mode env -u GITHUB_ACTIONS sh scripts/egress/check.sh --mode pf "$e" -- sh "$pr" none
  expect_refusal sandbox/wfp-on-macos mode sh scripts/egress/check.sh --mode wfp "$e" -- sh "$pr" none
}

test_wfp() {
  test_wfp_probes
  test_wfp_rules
}

test_wfp_probes() {
  e=scripts/egress/lists/empty.txt
  l=scripts/egress/lists/loopback.txt
  pr=scripts/egress/probe.sh
  expect_run wfp/quiet 0 0 0 sh scripts/egress/check.sh "$e" -- sh "$pr" none
  got=$(field_of mode) || got=
  result wfp/quiet-mode wfp "$got"
  run_check wfp/loopback sh scripts/egress/check.sh "$e" -- sh "$pr" loopback
  result wfp/loopback-exit 1 "$code"
  expect_logged wfp/loopback-line ' proto=tcp dest=127.0.0.1:47201 pid=[0-9]* image=egress-probe.exe$'
  serve 47201
  serve_first=$served
  stop_first=$stop
  serve 47202
  expect_run wfp/listed-reached 0 0 0 sh scripts/egress/check.sh "$l" -- sh "$pr" loopback
  run_check wfp/unlisted sh scripts/egress/check.sh "$l" -- sh "$pr" loopback2
  result wfp/unlisted-exit 1 "$code"
  expect_logged wfp/unlisted-line ' proto=tcp dest=127.0.0.1:47202 pid=[0-9]* image=egress-probe.exe$'
  unserve
  served=$serve_first
  stop=$stop_first
  unserve
  expect_run wfp/listen-inside 0 0 0 sh scripts/egress/check.sh "$l" -- sh "$pr" listen
}

test_wfp_rules() {
  e=scripts/egress/lists/empty.txt
  l=scripts/egress/lists/loopback.txt
  pr=scripts/egress/probe.sh
  ps_run wfp/named-binary-blocked 1 "\$c = New-Object System.Net.Sockets.TcpClient; try { if (\$c.ConnectAsync('192.0.2.1', 443).Wait(5000)) { exit 6 } else { exit 6 } } catch { \$e = \$_.Exception; while (\$null -ne \$e.InnerException) { \$e = \$e.InnerException }; if (\$e -is [System.Net.Sockets.SocketException] -and \$e.NativeErrorCode -eq 10013) { exit 5 } else { exit 6 } }"
  programs=${log%/denied.log}/programs
  if [ -f "$programs" ] && ! grep -qi 'egress-probe' "$programs"; then
    result wfp/no-rule-names-the-probe yes yes
  else
    result wfp/no-rule-names-the-probe yes no
  fi
  expect_run wfp/exit-code-passes 1 3 0 sh scripts/egress/check.sh "$e" -- sh -c 'exit 3'
  expect_run wfp/not-started 1 not-started 0 sh scripts/egress/check.sh "$e" -- "$work/no-such-command"
  printf '[fixed]\n192.0.2.1:443\n[agent]\n' >"$work/routable.txt"
  expect_run wfp/routable-entry-taken 0 0 0 sh scripts/egress/check.sh "$work/routable.txt" -- sh "$pr" none
  printf '[fixed]\n[agent]\nexample.com:443\n' >"$work/name.txt"
  expect_refusal wfp/name-entry entry sh scripts/egress/check.sh "$work/name.txt" -- sh "$pr" none
  expect_refusal wfp/sandbox-on-windows mode sh scripts/egress/check.sh --mode sandbox "$e" -- sh "$pr" none
  ps_run wfp/command-not-administrator 0 "if (([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { exit 0 } else { exit 5 }"
  ps_run wfp/tree-cannot-switch-audit-off 0 "& auditpol.exe /set '/subcategory:{0CCE9226-69AE-11D9-BED3-505054503030}' /success:disable /failure:disable *> \$null; if (\$LASTEXITCODE -eq 0) { exit 0 } else { exit 5 }"
  sh scripts/egress/check.sh "$e" -- powershell.exe -NoProfile -NonInteractive -Command "Start-Sleep -Seconds 12" >"$work/undo.out" 2>"$work/undo.err" &
  undo_check=$!
  undo_token=
  n=0
  while [ -z "$undo_token" ] && [ $n -lt 60 ]; do
    undo_state=$(grep -l "^pid=$undo_check\$" target/egress/[0-9]*/state 2>/dev/null | head -n 1)
    [ -z "$undo_state" ] || undo_token=$(sed -n 's/^token=//p' "$undo_state")
    sleep 0.5
    n=$((n + 1))
  done
  n=0
  while [ "$(wfp_rules "$undo_token")" = 0 ] && [ $n -lt 60 ]; do
    sleep 0.5
    n=$((n + 1))
  done
  powershell.exe -NoProfile -NonInteractive -Command "Get-NetFirewallRule -DisplayName '$undo_token-*' | Remove-NetFirewallRule" >/dev/null 2>&1
  wait "$undo_check"
  code=$?
  got=$(tail -n 1 "$work/undo.err")
  case $code:$got in
    "2:REFUSED mode: "*) got=refused-mode ;;
    *) got="exit-$code:$got" ;;
  esac
  result wfp/rules-removed-during-run-refused refused-mode "$got"
  before=$(wfp_state)
  expect_run wfp/quiet-again 0 0 0 sh scripts/egress/check.sh "$e" -- sh "$pr" none
  result wfp/settings-restored-after-a-run "$before" "$(wfp_state)"
  result wfp/teardown-removes-the-rules 0 "$(wfp_rules)"
  sh scripts/egress/check.sh "$e" -- powershell.exe -NoProfile -NonInteractive -Command "Start-Sleep -Seconds 12" >"$work/term.out" 2>"$work/term.err" &
  term_check=$!
  n=0
  while [ "$(wfp_rules)" = 0 ] && [ $n -lt 60 ]; do
    sleep 0.5
    n=$((n + 1))
  done
  kill -TERM "$term_check"
  wait "$term_check"
  left=$(wfp_rules)
  expect_run wfp/next-run-after-a-signal 0 0 0 sh scripts/egress/check.sh "$e" -- sh "$pr" none
  result wfp/next-run-restores-what-a-signal-left "$before 0 left-before-next=yes" "$(wfp_state) $(wfp_rules) left-before-next=$([ "$left" != 0 ] && printf yes || printf no)"
  wfp_reap_case
}

wfp_reap_case() {
  sh scripts/egress/check.sh "$e" -- powershell.exe -NoProfile -NonInteractive -Command "Start-Sleep -Seconds 12" >"$work/reap.out" 2>"$work/reap.err" &
  reap_check=$!
  n=0
  while [ "$(wfp_rules)" = 0 ] && [ $n -lt 60 ]; do
    sleep 0.5
    n=$((n + 1))
  done
  kill -TERM "$reap_check"
  wait "$reap_check"
  left=$(wfp_rules)
  MSYS2_ARG_CONV_EXCL='*' powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File scripts/egress/wfp.ps1 -Action reap >/dev/null 2>&1
  code=$?
  result wfp/reap-call-restores-what-a-signal-left "$before 0 0 left-before-reap=yes" "$(wfp_state) $(wfp_rules) $code left-before-reap=$([ "$left" != 0 ] && printf yes || printf no)"
}

wfp_state() {
  MSYS2_ARG_CONV_EXCL='*' powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File scripts/egress/wfp.ps1 -Action state | tr -d '\r'
}

wfp_rules() {
  powershell.exe -NoProfile -NonInteractive -Command "@(Get-NetFirewallRule -DisplayName '${1:-halvo-egress}-*' -ErrorAction SilentlyContinue).Count" | tr -d '\r'
}

ps_run() {
  expect_run "$1" 1 5 "$2" sh scripts/egress/check.sh scripts/egress/lists/empty.txt -- powershell.exe -NoProfile -NonInteractive -Command "$3"
}

[ $# -gt 0 ] || set -- probe lists
for part in "$@"; do
  part_start=$((passed + failed))
  case $part in
    probe) test_probe ;;
    lists) test_lists ;;
    sandbox) test_sandbox ;;
    wfp) test_wfp ;;
    wfp-rules) test_wfp_rules ;;
    *)
      printf 'FAIL test (unknown part %s)\n' "$part"
      exit 1
      ;;
  esac
  printf 'part %s ran=%s\n' "$part" "$((passed + failed - part_start))"
done
printf 'tests passed=%s failed=%s\n' "$passed" "$failed"
[ "$failed" -eq 0 ]
