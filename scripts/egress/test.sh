#!/bin/sh
# Tests of the egress check that send nothing off the machine: sh scripts/egress/test.sh [probe|lists|sandbox|wfp]...
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
  "$bin/egress-spawn$exe" run "$work/status" "$work/pid" -- sh "$work/seven.sh"
  code=$?
  got=missing
  read -r got <"$work/status"
  result probe/run-records-exit-code "0:7" "$code:$got"
  "$bin/egress-spawn$exe" run "$work/status2" "$work/pid2" -- "$work/no-such-binary"
  code=$?
  got=missing
  read -r got <"$work/status2"
  result probe/run-records-not-started "0:not-started" "$code:$got"
  if [ -z "$exe" ]; then
    printf '%s\n' 'kill -9 $$' >"$work/nine.sh"
    "$bin/egress-spawn$exe" run "$work/status3" "$work/pid3" -- sh "$work/nine.sh"
    code=$?
    got=missing
    read -r got <"$work/status3"
    result probe/run-records-signal "0:signal-9" "$code:$got"
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
  result "$label" "$want_code $want_status $want_denied" "$code $status $denied"
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
  printf '%s\n' "for d in target/egress/[0-9]*; do" \
    "if [ -f \$d/state ] && ! grep -q '^done\$' \$d/state; then" \
    ": >\$d/stream.ndjson 2>/dev/null && exit 9; : >\$d/denied.log 2>/dev/null && exit 8; fi; done" >"$work/erase.sh"
  expect_run sandbox/tree-cannot-erase-its-log 1 1 1 sh scripts/egress/check.sh "$e" -- sh -c "sh scripts/egress/probe.sh loopback; sh '$work/erase.sh' && exit 1"
  printf '%s\n' '{"processImagePath":"/usr/bin/logger","senderImagePath":"/usr/bin/logger","eventMessage":"Sandbox: x(1) deny(1) network-outbound remote:*:1\ntok mark-end","timestamp":"2026-11-11 03:06:50.956934-0800"}' \
    '{"processImagePath":"/kernel","senderImagePath":"/System/Library/Extensions/Sandbox.kext/Contents/MacOS/Sandbox","eventMessage":"Sandbox: x(1) deny(1) network-outbound remote:*:1\ntok mark-start","timestamp":"2026-11-11 03:06:50.956934-0800"}' \
    '{"processImagePath":"/usr/bin/logger","senderImagePath":"/usr/bin/logger","eventMessage":"Sandbox: x(1) deny(1) network-outbound remote:*:1\ntok mark-end","timestamp":"2026-11-11 03:06:50.956934-0800"}' \
    '{"processImagePath":"/kernel","senderImagePath":"/System/Library/Extensions/Sandbox.kext/Contents/MacOS/Sandbox","eventMessage":"Sandbox: egress-probe(7) deny(1) network-outbound remote:*:443\ntok tcp4 any","timestamp":"2026-11-11 03:06:50.956934-0800"}' >"$work/forged.ndjson"
  got=$(awk -v token=tok -f scripts/egress/sandbox.awk "$work/forged.ndjson") || got=
  result sandbox/forged-markers-ignored 'denied at=1794395210956934000 proto=tcp dest=-:443 pid=7 image=egress-probe' "$got"
  first_log=$log
  expect_run sandbox/second-run 0 0 0 sh scripts/egress/check.sh "$e" -- sh "$pr" none
  if [ "$first_log" != "$log" ]; then
    result sandbox/runs-get-new-directories yes yes
  else
    result sandbox/runs-get-new-directories yes no
  fi
  serve 47201
  expect_refusal sandbox/self-test-port-taken port sh scripts/egress/check.sh --self-test
  unserve
  expect_refusal sandbox/self-test-unknown-argument usage sh scripts/egress/check.sh --self-test --verbose
  expect_refusal sandbox/pf-off-a-runner mode env -u GITHUB_ACTIONS sh scripts/egress/check.sh --mode pf "$e" -- sh "$pr" none
  expect_refusal sandbox/wfp-on-macos mode sh scripts/egress/check.sh --mode wfp "$e" -- sh "$pr" none
}

test_wfp() {
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
  expect_run wfp/exit-code-passes 1 3 0 sh scripts/egress/check.sh "$e" -- sh -c 'exit 3'
  expect_run wfp/not-started 1 not-started 0 sh scripts/egress/check.sh "$e" -- "$work/no-such-command"
  printf '[fixed]\n192.0.2.1:443\n[agent]\n' >"$work/routable.txt"
  expect_run wfp/routable-entry-taken 0 0 0 sh scripts/egress/check.sh "$work/routable.txt" -- sh "$pr" none
  printf '[fixed]\n[agent]\nexample.com:443\n' >"$work/name.txt"
  expect_refusal wfp/name-entry entry sh scripts/egress/check.sh "$work/name.txt" -- sh "$pr" none
  expect_refusal wfp/sandbox-on-windows mode sh scripts/egress/check.sh --mode sandbox "$e" -- sh "$pr" none
}

[ $# -gt 0 ] || set -- probe lists
for part in "$@"; do
  case $part in
    probe) test_probe ;;
    lists) test_lists ;;
    sandbox) test_sandbox ;;
    wfp) test_wfp ;;
    *)
      printf 'FAIL test (unknown part %s)\n' "$part"
      exit 1
      ;;
  esac
done
printf 'tests passed=%s failed=%s\n' "$passed" "$failed"
[ "$failed" -eq 0 ]
