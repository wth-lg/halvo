#!/bin/sh
# jobs: macos windows
# rule: Five runs of route-gate probe --rounds 100000 under measure-peak at a limit of 16777216 bytes each exit 0 with no read above the limit, and the bound of their median, 8 MiB times the ceiling of 5 times the median over 32 MiB, is at or under that limit; a pass says that no read was above the limit and never that the probe held no more, and the reading does not count child-process, exec, mapped-read, mapped-written, purgeable, reusable, kernel-message or socket-buffer memory on macOS, or child-process, mapped-file, section or broker-process memory on Windows.
set -u
name=70-r6-memory
limit=16777216
runs=5
rounds=100000
case $0 in
  /*) self=$0 ;;
  *) self=$(pwd)/$0 ;;
esac

setup_failed() {
  printf 'FAIL %s (setup)\n' "$name"
  exit 1
}

enter_repo() {
  if [ -n "${CI_D_REPO:-}" ]; then
    repo=$CI_D_REPO
  else
    repo=$(git rev-parse --show-toplevel 2>/dev/null) || setup_failed
  fi
  cd "$repo" 2>/dev/null || setup_failed
}

is_count() {
  case $1 in
    '' | 0* | *[!0-9]* | ????????????????*) return 1 ;;
  esac
  return 0
}

read_runs() {
  well=yes
  ran=yes
  at=0
  i=0
  peaks=
  while IFS= read -r line || [ -n "$line" ]; do
    at=$((at + 1))
    if [ "$at" -eq 1 ]; then
      mid=${line#measure-peak platform=}
      mid=${mid%" limit_bytes=$limit runs=$runs"}
      case $line:$mid in
        "measure-peak platform="*:'macos mode=poll' | "measure-peak platform="*:'macos mode=taskpolicy') ;;
        "measure-peak platform="*:'windows mode=job' | "measure-peak platform="*:'windows mode=job-commit') ;;
        *) well=no ;;
      esac
    elif [ "$at" -le $((runs + 1)) ]; then
      i=$((i + 1))
      rest=${line#"run n=$i exit="}
      status=${rest%% *}
      rest=${rest#*" peak_bytes="}
      peak=${rest%% *}
      rest=${rest#*" capped="}
      capped=${rest%% *}
      rest=${rest#*" start="}
      start=${rest%% *}
      end=${rest#*" end="}
      if [ "$line" != "run n=$i exit=$status peak_bytes=$peak capped=$capped start=$start end=$end" ]; then
        well=no
      fi
      case $status:$start:$end in
        *' '* | :* | *:: | *:) well=no ;;
      esac
      is_count "$peak" || well=no
      case $capped in
        yes) ran=no ;;
        no) ;;
        *) well=no ;;
      esac
      [ "$status" = 0 ] || ran=no
      peaks="$peaks $peak"
    elif [ "$at" -eq $((runs + 2)) ]; then
      median=${line#median peak_bytes=}
      if [ "$median" = "$line" ] || ! is_count "$median"; then
        well=no
      fi
    elif [ "$at" -eq $((runs + 3)) ]; then
      [ "$line" = 'checks exit=ok cap=ok median=ok' ] || ran=no
      case $line in
        'checks exit='*' cap='*' median='*) ;;
        *) well=no ;;
      esac
    else
      well=no
    fi
  done <"$1"
  [ "$at" -eq $((runs + 3)) ] || well=no
}

middle_of() {
  middle=
  for each in $peaks; do
    below=0
    upto=0
    for other in $peaks; do
      [ "$other" -lt "$each" ] && below=$((below + 1))
      [ "$other" -le "$each" ] && upto=$((upto + 1))
    done
    if [ "$below" -le $((runs / 2)) ] && [ "$upto" -gt $((runs / 2)) ]; then
      middle=$each
    fi
  done
}

count_probes() {
  probes=0
  while IFS= read -r line || [ -n "$line" ]; do
    [ "$line" != "probe rounds=$rounds ok=yes" ] || probes=$((probes + 1))
  done <"$1"
}

measure() {
  enter_repo
  artifacts=${CI_D_ARTIFACTS:-target/ci-artifacts/$name}
  [ -d "$artifacts" ] || mkdir -p "$artifacts" 2>/dev/null || setup_failed

  system=$(uname -s 2>/dev/null)
  exe=${CARGO_TARGET_DIR:-target}/debug/route-gate
  case $system:${GITHUB_ACTIONS:-} in
    Darwin:true) machine=hosted-mac ;;
    Darwin:*) machine=mac ;;
    MINGW*:true | MSYS*:true | CYGWIN*:true)
      machine=hosted-win
      exe=$exe.exe
      ;;
    *)
      printf 'FAIL %s (run)\n' "$name"
      exit 1
      ;;
  esac

  if ! cargo build --locked -p halvo-route-gate --bin route-gate >"$artifacts/cargo-build.txt" 2>&1; then
    printf 'FAIL %s (run)\n' "$name"
    exit 1
  fi
  out=$artifacts/measure-peak.out
  err=$artifacts/measure-peak.err
  cargo run -q --locked -p halvo-cap --bin measure-peak -- --limit "$limit" --runs "$runs" -- \
    "$exe" probe --rounds "$rounds" >"$out" 2>"$err"
  code=$?

  read_runs "$out"
  if [ "$well" = no ]; then
    printf 'FAIL %s (run)\n' "$name"
    exit 1
  fi
  middle_of
  count_probes "$err"
  if [ "$code" -ne 0 ] || [ "$middle" != "$median" ] || [ "$probes" -ne "$runs" ]; then
    ran=no
  fi

  i=0
  for peak in $peaks; do
    i=$((i + 1))
    printf 'bound part=router machine=%s run=%s peak_bytes=%s\n' "$machine" "$i" "$peak"
  done
  bound=$((8 * ((5 * median + 33554431) / 33554432) * 1048576))
  printf 'bound part=router machine=%s bound_bytes=%s\n' "$machine" "$bound"

  if [ "$ran" = no ]; then
    printf 'FAIL %s (run)\n' "$name"
    exit 1
  fi
  if [ "$bound" -gt "$limit" ]; then
    printf 'FAIL %s (bound)\n' "$name"
    exit 1
  fi
  printf 'PASS %s\n' "$name"
}

write_runs() {
  file=$1
  median=$2
  shift 2
  printf 'measure-peak platform=macos mode=poll limit_bytes=%s runs=%s\n' "$limit" "$runs" >"$file"
  n=0
  for peak in "$@"; do
    n=$((n + 1))
    printf 'run n=%s exit=0 peak_bytes=%s capped=no start=2026-11-12T00:00:0%s.000Z end=2026-11-12T00:00:0%s.500Z\n' \
      "$n" "$peak" "$n" "$n" >>"$file"
  done
  printf 'median peak_bytes=%s\nchecks exit=ok cap=ok median=ok\n' "$median" >>"$file"
}

check_reset() {
  STUB_UNAME=Darwin
  STUB_BUILD_EXIT=0
  STUB_OUT=$work/good.out
  STUB_ERR=$work/good.err
  STUB_CODE=0
  case_actions=
  case_ext=
  case_repo=
  case_target=
}

check() {
  out=$(
    PATH=$work/bin:$PATH
    CI_D_REPO=${case_repo:-$work/repo}
    CI_D_ARTIFACTS=$work/artifacts/$1
    STUB_EXE=${case_target:-target}/debug/route-gate$case_ext
    unset CARGO_TARGET_DIR GITHUB_ACTIONS
    [ -z "$case_actions" ] || GITHUB_ACTIONS=$case_actions
    [ -z "$case_target" ] || CARGO_TARGET_DIR=$case_target
    export PATH CI_D_REPO CI_D_ARTIFACTS STUB_EXE GITHUB_ACTIONS CARGO_TARGET_DIR
    export STUB_UNAME STUB_BUILD_EXIT STUB_OUT STUB_ERR STUB_CODE
    sh "$self" 2>&1
  )
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
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$got"
  if [ "$got" != "$2" ]; then
    wrong="$wrong $1"
  elif [ "$2" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
  check_reset
}

self_test() {
  enter_repo
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || setup_failed
  mkdir -p "$work/bin" "$work/repo" "$work/artifacts" || setup_failed
  cat >"$work/bin/cargo" <<'EOF'
#!/bin/sh
case $* in
  'build --locked -p halvo-route-gate --bin route-gate')
    exit "$STUB_BUILD_EXIT"
    ;;
  "run -q --locked -p halvo-cap --bin measure-peak -- --limit 16777216 --runs 5 -- $STUB_EXE probe --rounds 100000")
    [ ! -f "$STUB_OUT" ] || cat "$STUB_OUT"
    [ ! -f "$STUB_ERR" ] || cat "$STUB_ERR" >&2
    exit "$STUB_CODE"
    ;;
  *)
    printf 'stub cargo: got the arguments: %s\n' "$*"
    exit 97
    ;;
esac
EOF
  cat >"$work/bin/uname" <<'EOF'
#!/bin/sh
printf '%s\n' "$STUB_UNAME"
EOF
  chmod +x "$work/bin/cargo" "$work/bin/uname" || setup_failed

  probe="probe rounds=$rounds ok=yes"
  printf '%s\n%s\n%s\n%s\n%s\n' "$probe" "$probe" "$probe" "$probe" "$probe" >"$work/good.err"
  printf '%s\n%s\n%s\n%s\n' "$probe" "$probe" "$probe" "$probe" >"$work/four.err"
  sed 's/ok=yes/ok=no/' "$work/good.err" >"$work/miss.err"
  write_runs "$work/good.out" 1318912 1310720 1327104 1318912 1302528 1335296
  write_runs "$work/line.out" 13421772 13421772 13421772 13421772 13421772 13421772
  write_runs "$work/above.out" 13421773 13421773 13421773 13421773 13421773 13421773
  sed -e 's/n=3 exit=0 peak_bytes=1318912 capped=no/n=3 exit=signal-9 peak_bytes=16777217 capped=yes/' \
    -e 's/^median peak_bytes=1318912$/median peak_bytes=1327104/' \
    -e 's/^checks exit=ok cap=ok/checks exit=miss cap=miss/' "$work/good.out" >"$work/breach.out"
  write_runs "$work/both.out" 13421773 13421773 13421773 16777217 13421773 13421773
  sed -e 's/n=3 exit=0 peak_bytes=16777217 capped=no/n=3 exit=signal-9 peak_bytes=16777217 capped=yes/' \
    -e 's/^checks exit=ok cap=ok/checks exit=miss cap=miss/' "$work/both.out" >"$work/both-breach.out"
  sed '/^run n=5 /d' "$work/good.out" >"$work/four.out"
  sed 's/^median peak_bytes=1318912$/median peak_bytes=1310720/' "$work/good.out" >"$work/median.out"
  sed 's/^checks exit=ok cap=ok median=ok$/checks exit=ok cap=ok median=miss/' "$work/good.out" >"$work/checks.out"
  sed 's/limit_bytes=16777216/limit_bytes=16777217/' "$work/good.out" >"$work/limit.out"
  sed 's/n=2 exit=0/n=2 exit=1/' "$work/good.out" >"$work/exit.out"
  sed 's/peak_bytes=1327104/peak_bytes=01327104/' "$work/good.out" >"$work/zero.out"
  write_runs "$work/nothing.out" 0 0 0 0 0 0
  cp "$work/good.out" "$work/extra.out" && printf 'checks exit=ok cap=ok median=ok\n' >>"$work/extra.out"
  sed 's/n=4 exit=0 peak_bytes=1302528 capped=no/n=4 exit=0 peak_bytes=1302528 capped=yes/' "$work/good.out" >"$work/capped.out"
  sed '/^checks /d' "$work/good.out" >"$work/short.out"
  sed 's/n=1 exit=0 peak_bytes=1310720 capped=no/n=1 exit=0 peak_bytes=1310720 rss=1 capped=no/' "$work/good.out" >"$work/key.out"
  for each in breach four median checks limit exit zero extra capped short key; do
    if cmp -s "$work/good.out" "$work/$each.out"; then
      setup_failed
    fi
  done

  rejected=0
  wrong=
  check_reset

  check all-hold PASS "bound part=router machine=mac run=1 peak_bytes=1310720
bound part=router machine=mac run=2 peak_bytes=1327104
bound part=router machine=mac run=3 peak_bytes=1318912
bound part=router machine=mac run=4 peak_bytes=1302528
bound part=router machine=mac run=5 peak_bytes=1335296
bound part=router machine=mac bound_bytes=8388608
PASS $name"
  case_actions=true
  check hosted-mac PASS 'bound part=router machine=hosted-mac bound_bytes=8388608'
  STUB_UNAME=MINGW64_NT-10.0-26100
  case_actions=true
  case_ext=.exe
  check hosted-win PASS 'bound part=router machine=hosted-win bound_bytes=8388608'
  STUB_UNAME=MINGW64_NT-10.0-26100
  case_ext=.exe
  check windows-outside-actions FAIL "FAIL $name (run)"
  STUB_UNAME=Linux
  check other-system FAIL "FAIL $name (run)"
  STUB_OUT=$work/line.out
  check median-at-the-line PASS 'bound part=router machine=mac bound_bytes=16777216'
  STUB_OUT=$work/above.out
  check peak-above-the-line FAIL "bound part=router machine=mac run=5 peak_bytes=13421773
bound part=router machine=mac bound_bytes=25165824
FAIL $name (bound)"
  STUB_OUT=$work/breach.out
  STUB_CODE=1
  check run-stopped-by-the-cap FAIL "bound part=router machine=mac run=3 peak_bytes=16777217
bound part=router machine=mac run=4 peak_bytes=1302528
bound part=router machine=mac run=5 peak_bytes=1335296
bound part=router machine=mac bound_bytes=8388608
FAIL $name (run)"
  STUB_OUT=$work/both-breach.out
  STUB_CODE=1
  check two-causes-name-the-first FAIL "bound_bytes=25165824
FAIL $name (run)"
  STUB_BUILD_EXIT=101
  check build-fails FAIL "FAIL $name (run)"
  STUB_OUT=$work/missing.out
  STUB_ERR=$work/missing.err
  STUB_CODE=2
  check measure-peak-refuses FAIL "FAIL $name (run)"
  STUB_OUT=$work/four.out
  check four-runs FAIL "FAIL $name (run)"
  STUB_ERR=$work/four.err
  check four-probe-lines FAIL "FAIL $name (run)"
  STUB_ERR=$work/miss.err
  check probe-misses FAIL "FAIL $name (run)"
  STUB_OUT=$work/median.out
  check median-not-the-middle FAIL "FAIL $name (run)"
  STUB_OUT=$work/checks.out
  check checks-miss-with-exit-0 FAIL "FAIL $name (run)"
  STUB_OUT=$work/limit.out
  check another-limit FAIL "FAIL $name (run)"
  STUB_OUT=$work/exit.out
  STUB_CODE=1
  check child-exits-1 FAIL "FAIL $name (run)"
  STUB_OUT=$work/zero.out
  check leading-zero FAIL "FAIL $name (run)"
  STUB_OUT=$work/nothing.out
  check peaks-of-0 FAIL "FAIL $name (run)"
  STUB_OUT=$work/extra.out
  check line-after-checks FAIL "FAIL $name (run)"
  STUB_OUT=$work/short.out
  check no-checks-line FAIL "FAIL $name (run)"
  STUB_OUT=$work/capped.out
  check capped-with-checks-ok FAIL "FAIL $name (run)"
  STUB_OUT=$work/exit.out
  check child-exits-1-with-checks-ok FAIL "FAIL $name (run)"
  STUB_CODE=1
  check measure-peak-exits-1-alone FAIL "bound_bytes=8388608
FAIL $name (run)"
  STUB_OUT=$work/key.out
  check run-line-with-another-key FAIL "FAIL $name (run)"
  case_target=$work/other-target
  check target-dir-of-cargo PASS "PASS $name"
  case_repo=$work/missing
  check repo-missing FAIL "FAIL $name (setup)"

  usage=$(sh "$self" --all 2>&1)
  code=$?
  if [ "$code:$usage" = "1:FAIL $name (usage)" ]; then
    printf 'case %s/unknown-argument want=FAIL got=FAIL\n' "$name"
    rejected=$((rejected + 1))
  else
    printf 'case %s/unknown-argument want=FAIL got=exit-%s\n' "$name" "$code"
    wrong="$wrong unknown-argument"
  fi

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  '') measure ;;
  *)
    printf 'FAIL %s (usage)\n' "$name"
    exit 1
    ;;
esac
