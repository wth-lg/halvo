#!/bin/sh
# The last step of the one scratch run of verifier 122 (unit app-1c), and the runner of the same
# changed copies on the Mac. It never fails its caller: every result is a line that starts with "gate:".
# usage: sh step.sh <macos|windows> all
#        sh step.sh <macos|windows> control <tag> | copy <id> | check36 <id> | b01 | links | plain | release
# Start it at the root of the checkout. A changed copy is a whole file under copies/<id>/, named in
# copies.list; it is put in place with cp and taken out again with git checkout.
set -u
job=${1:-}
mode=${2:-}
item=${3:-}
here=$(cd "$(dirname "$0")" && pwd)
out=${GATE_OUT:-${RUNNER_TEMP:-${TMPDIR:-/tmp}}/gate-122}
mkdir -p "$out" || exit 0
t0=$(date +%s)
five='a_child_web_view_of_the_window_main_gets_no_command_of_the_unit_and_deletes_nothing
a_call_whose_origin_header_is_no_address_is_refused
an_address_of_the_app_host_with_a_password_and_no_user_name_is_refused
the_web_view_main_moved_into_another_window_gets_no_command_of_the_unit_and_deletes_nothing
a_capability_handed_to_the_context_macro_reaches_no_command_of_the_unit_from_another_window'
fourteen='E1-empty-list E1-no-delete_all E1-m27 E2-B-1 E3-m32 E3-A-2bw_and_m32 E4-A-g3 E4-window-01 A-1a A-1b A-2a A-2b A-2bw A-3'
applied=
l_limit=600
l_code=0

say() {
  printf 'gate: %s\n' "$*"
}

elapsed() {
  echo $(($(date +%s) - t0))
}

over() {
  if [ "$(elapsed)" -gt 3000 ]; then
    say "skipped $1: the step had run for more than 3000 s"
    return 0
  fi
  return 1
}

differ() {
  git status --porcelain --untracked-files=no | grep -c ''
}

bounded() {
  b_limit=$1
  shift
  "$@" &
  b_pid=$!
  (
    b_n=0
    while [ "$b_n" -lt "$b_limit" ] && kill -0 "$b_pid" 2>/dev/null; do
      sleep 2
      b_n=$((b_n + 2))
    done
    if kill -0 "$b_pid" 2>/dev/null; then
      kill "$b_pid" 2>/dev/null
    fi
  ) &
  b_dog=$!
  wait "$b_pid"
  b_code=$?
  wait "$b_dog" 2>/dev/null
  return "$b_code"
}

apply_copy() {
  applied=
  while read -r c_id c_path; do
    [ "$c_id" = "$1" ] || continue
    if cp "$here/copies/$1/${c_path##*/}.copy" "$c_path"; then
      applied="$applied $c_path"
    else
      say "$1: the copy of $c_path could not be put in place"
    fi
  done <"$here/copies.list"
  [ -n "$applied" ] || say "$1: copies.list names no file for it"
  a_differ=$(differ)
}

restore() {
  for r_path in $applied; do
    git checkout -- "$r_path" || say "git checkout of $r_path failed"
  done
  applied=
  r_differ=$(differ)
}

tests() {
  t_tag=$1
  bounded 900 cargo test -p halvo --locked --test page_guard --no-fail-fast >"$out/$t_tag.txt" 2>&1
  t_code=$?
  tr -d '\r' <"$out/$t_tag.txt" >"$out/$t_tag.lf"
  t_ok=0
  for t_name in $five; do
    if grep -q -x "test $t_name \.\.\. ok" "$out/$t_tag.lf"; then
      t_ok=$((t_ok + 1))
    fi
  done
  t_failed=$(sed -n 's/^test \([^ ]*\) \.\.\. FAILED.*$/\1/p' "$out/$t_tag.lf" | sort -u | tr '\n' ' ')
  t_running=$(grep -E '^running [0-9]+ tests?$' "$out/$t_tag.lf" | tr '\n' ',')
  t_errors=$(grep -c -E '^error' "$out/$t_tag.lf")
  t_entry=$(grep -c '0xc0000139' "$out/$t_tag.lf")
}

tests_line() {
  say "$1: cargo exit $t_code; ${t_running:-no line running N tests,} ok of the five $t_ok; FAILED: ${t_failed:-none}; lines that start with error: $t_errors; lines with 0xc0000139: $t_entry$2"
  grep -A 1 -E 'panicked at ' "$out/$t_tag.lf" | grep -v -E '^--$' | head -8 | cut -c1-260 | sed 's/^/gate:   /'
  if [ "$t_errors" != 0 ]; then
    grep -E '^error|0xc0000139' "$out/$t_tag.lf" | head -3 | cut -c1-300 | sed 's/^/gate:   /'
  fi
  if [ -z "$t_failed" ] && [ "$t_code" != 0 ]; then
    say "$1: cargo failed and no test printed FAILED; the last lines of its output:"
    tail -15 "$out/$t_tag.lf" | cut -c1-300 | sed 's/^/gate:   | /'
  fi
}

control() {
  tests "$1"
  tests_line "control $1 (no copy applied; tracked files that differ from HEAD: $(differ))" ""
  for c_name in $five; do
    if grep -q -x "test $c_name \.\.\. ok" "$out/$1.lf"; then
      say "control $1: test $c_name ... ok"
    else
      say "control $1: NOT ok: $c_name"
    fi
  done
}

copy() {
  apply_copy "$1"
  tests "copy-$1"
  c_applied=$applied
  restore
  tests_line "copy $1 (applied to$c_applied; $a_differ tracked files differed)" "; after git checkout $r_differ tracked files differ"
}

check36() {
  bounded 1200 sh scripts/ci/run-ci-d.sh "$job" 36-app-guard-tests >"$out/$1.txt" 2>&1
  k_code=$?
  say "$1: sh scripts/ci/run-ci-d.sh $job 36-app-guard-tests exit $k_code"
  tr -d '\r' <"$out/$1.txt" | grep -E '^(FAIL|PASS|SKIP) |passed by name|^running [0-9]|^ci\.d ' | awk '!seen[$0]++' | cut -c1-1500 | sed 's/^/gate:   /'
}

check36_copy() {
  apply_copy "$1"
  c_applied=$applied
  say "check 36 with the copy $1 applied to$c_applied and not committed ($a_differ tracked files differ)"
  check36 "check36-$1"
  restore
  say "check 36 with the copy $1: after git checkout $r_differ tracked files differ"
}

b01() {
  apply_copy b01
  tests b01
  tests_line "b01 (os != \"windows\" in build.rs; applied to$applied)" ""
  check36 check36-b01
  restore
  say "b01: after git checkout $r_differ tracked files differ"
}

words() {
  tr -d '\r' <"$1" | awk '{ n = split($0, w, "\" \""); if (n == 0) print ""; for (i = 1; i <= n; i++) print w[i] }' | sed -E 's/rustc[A-Za-z0-9]{6}/rustcTMP/g'
}

link() {
  l_tag=$1
  shift
  touch src-tauri/src/main.rs src-tauri/tests/page_guard.rs
  bounded "$l_limit" cargo rustc -p halvo --locked "$@" -- --print link-args >"$out/link-$l_tag.txt" 2>"$out/link-$l_tag.err"
  l_code=$?
  words "$out/link-$l_tag.txt" >"$out/link-$l_tag.words"
  say "link $l_tag (cargo rustc -p halvo --locked $* -- --print link-args): cargo exit $l_code; words $(grep -c '' <"$out/link-$l_tag.words"); /MANIFEST words $(grep -c -i '/MANIFEST' <"$out/link-$l_tag.words")"
  grep -i '/MANIFEST' "$out/link-$l_tag.words" | head -6 | cut -c1-300 | sed 's/^/gate:   manifest word: /'
  if [ "$l_code" != 0 ]; then
    tr -d '\r' <"$out/link-$l_tag.err" | grep -E 'LNK[0-9]|CVT[0-9]|^error|fatal' | head -6 | cut -c1-400 | sed 's/^/gate:   /'
  fi
}

mask() {
  sed -E 's/\.[a-z0-9]{7}\.rcgu\.o/.SESSION.rcgu.o/g' "$1"
}

same() {
  if cmp -s "$out/link-$1.words" "$out/link-$2.words"; then
    say "link words of $1 equal those of $2, in order: true"
  else
    say "link words of $1 equal those of $2, in order: false"
    mask "$out/link-$1.words" >"$out/link-$1.masked"
    mask "$out/link-$2.words" >"$out/link-$2.masked"
    if cmp -s "$out/link-$1.masked" "$out/link-$2.masked"; then
      say "link words of $1 equal those of $2, in order, with the session part of each object name masked (.xxxxxxx.rcgu.o, which an incremental build writes): true"
    else
      say "link words of $1 equal those of $2, in order, with the session part of each object name masked: false"
      diff "$out/link-$1.masked" "$out/link-$2.masked" | head -8 | cut -c1-300 | sed 's/^/gate:   /'
    fi
  fi
}

links() {
  l_limit=600
  link head-bin --bin halvo
  link head-harness --bin halvo --profile test
  link head-page_guard --test page_guard
  apply_copy b-base-build
  say "build.rs of the base applied to$applied ($a_differ tracked files differ)"
  link base-bin --bin halvo
  link base-harness --bin halvo --profile test
  link base-page_guard --test page_guard
  restore
  say "build.rs restored: after git checkout $r_differ tracked files differ"
  same head-bin base-bin
  same head-harness base-harness
}

plain() {
  l_limit=600
  apply_copy b-plain-key
  say "plain link key (cargo:rustc-link-arg in both lines of build.rs) applied to$applied ($a_differ tracked files differ)"
  link plain-bin --bin halvo
  link plain-harness --bin halvo --profile test
  link plain-page_guard --test page_guard
  bounded 900 cargo test -p halvo --locked --no-run >"$out/plain-no-run.txt" 2>&1
  p_code=$?
  say "plain link key: cargo test -p halvo --locked --no-run exit $p_code"
  tr -d '\r' <"$out/plain-no-run.txt" | grep -E 'LNK[0-9]|CVT[0-9]|^error|fatal' | head -6 | cut -c1-400 | sed 's/^/gate:   /'
  if [ "$p_code" = 0 ]; then
    bounded 900 cargo test -p halvo --locked --no-fail-fast >"$out/plain-run.txt" 2>&1
    say "plain link key: cargo test -p halvo --locked --no-fail-fast exit $?; lines with 0xc0000139: $(grep -c '0xc0000139' "$out/plain-run.txt")"
    tr -d '\r' <"$out/plain-run.txt" | grep -E '^running [0-9]|^test result|^error|exit code' | head -8 | cut -c1-300 | sed 's/^/gate:   /'
  fi
  restore
  say "plain link key: after git checkout $r_differ tracked files differ"
}

release() {
  if [ "$job" != windows ] && [ "${GATE_RELEASE:-0}" != 1 ]; then
    say "release link: not run on this system"
    return
  fi
  if [ "$(elapsed)" -gt 1500 ]; then
    say "release link: not run, the step had run for $(elapsed) s and it starts only before 1500 s"
    return
  fi
  r_start=$(date +%s)
  l_limit=1200
  link release-head-bin --bin halvo --release
  say "release link at the head: $(($(date +%s) - r_start)) s with its build"
  if [ "$l_code" = 0 ]; then
    apply_copy b-base-build
    l_limit=420
    link release-base-bin --bin halvo --release
    restore
    say "release link with the base's build.rs: after git checkout $r_differ tracked files differ"
    same release-head-bin release-base-bin
  fi
  l_limit=600
}

all() {
  say "start: job $job; HEAD $(git rev-parse HEAD); $(cargo --version 2>&1); lines of git status --porcelain: $(git status --porcelain | grep -c '')"
  control control-before
  for a_id in $fourteen; do
    over "copy $a_id" || copy "$a_id"
  done
  over "control after" || control control-after
  if [ "$job" = windows ]; then
    a_checks='c-ignored c-renamed c-cfg-not-windows c-line1-cfg-not-windows c4-early-return-windows'
  else
    a_checks='c-ignored c-renamed c-cfg-not-macos c-line1-cfg-windows c4-early-return'
  fi
  for a_id in $a_checks; do
    over "check 36 with $a_id" || check36_copy "$a_id"
  done
  over b01 || b01
  over links || links
  over "plain link key" || plain
  over "release link" || release
  say "end: lines of git status --porcelain: $(git status --porcelain | grep -c ''); the step ran $(elapsed) s"
  git status --porcelain | head -5 | sed 's/^/gate:   /'
}

case $mode in
  all) all ;;
  control) control "$item" ;;
  copy) copy "$item" ;;
  check36) check36_copy "$item" ;;
  b01) b01 ;;
  links) links ;;
  plain) plain ;;
  release) release ;;
  same) same "$item" "${4:-}" ;;
  *) say "usage: sh step.sh <macos|windows> all | control <tag> | copy <id> | check36 <id> | b01 | links | plain | release" ;;
esac
exit 0
