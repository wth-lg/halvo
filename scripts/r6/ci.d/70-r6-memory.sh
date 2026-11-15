#!/bin/sh
# jobs: macos windows
# rule: Five runs of route-gate probe --rounds 100000 under measure-peak at a limit of 16777216 bytes each exit 0 with no read above the limit, and the bound of their median, 8 MiB times the ceiling of 5 times the median over 32 MiB, is at or under that limit; a pass says that no read was above the limit and never that the probe held no more, the reading does not count child-process, exec, mapped-read, mapped-written, purgeable, reusable, kernel-message or socket-buffer memory on macOS, or child-process, mapped-file, section or broker-process memory on Windows, the binary measured is the one whose path cargo's artifact line of a fresh build gives, and code that runs while cargo builds and a program named cargo earlier on the PATH are the job's environment, which the check trusts.
# lane: alone
set -u
name=70-r6-memory
limit=16777216
runs=5
rounds=100000
nl='
'
case $0 in
  /*) self=$0 ;;
  *) self=$(pwd)/$0 ;;
esac

setup_failed() {
  printf 'FAIL %s (setup)\n' "$name"
  exit 1
}

run_failed() {
  printf 'FAIL %s (run)\n' "$name"
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

built_route_gate() {
  awk '
    function field(key, value) {
      if (!match($0, "\"" key "\":\"[^\"]*\"")) return ""
      value = substr($0, RSTART + length(key) + 4, RLENGTH - length(key) - 5)
      gsub(/\\\\/, "/", value)
      return value
    }
    index($0, "{\"reason\":\"compiler-artifact\",") != 1 { next }
    {
      if (index($0, "\"fresh\":true") == 0) stale = 1
      dir = field("manifest_path")
      sub(/\/[^\/]*$/, "", dir)
      if (dir !~ /\/crates\/halvo-route-gate$/ || field("src_path") != dir "/src/main.rs") next
      n++
      binary = field("executable")
    }
    END {
      if (stale) print "stale"
      else if (n != 1) print "lines " n + 0
      else print "binary " binary
    }
  ' "$1"
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
      case $platform:$line in
        "macos:measure-peak platform=macos mode=poll limit_bytes=$limit runs=$runs") ;;
        "macos:measure-peak platform=macos mode=taskpolicy limit_bytes=$limit runs=$runs") ;;
        "windows:measure-peak platform=windows mode=job limit_bytes=$limit runs=$runs") ;;
        "windows:measure-peak platform=windows mode=job-commit limit_bytes=$limit runs=$runs") ;;
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
      [ -n "$status" ] || well=no
      [ -n "$start" ] || well=no
      case $end in
        '' | *' '*) well=no ;;
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

  uname -s >"$artifacts/system.txt" 2>/dev/null
  IFS= read -r system <"$artifacts/system.txt" || system=
  case $system:${GITHUB_ACTIONS:-} in
    Darwin:true)
      machine=hosted-mac
      platform=macos
      ;;
    Darwin:*)
      machine=mac
      platform=macos
      ;;
    MINGW*:true | MSYS*:true | CYGWIN*:true)
      machine=hosted-win
      platform=windows
      ;;
    *) run_failed ;;
  esac

  cargo build --locked -p halvo-route-gate --bin route-gate >"$artifacts/cargo-build.txt" 2>&1 || run_failed
  cargo build --locked -p halvo-route-gate --bin route-gate --message-format=json \
    >"$artifacts/cargo-build.json" 2>"$artifacts/cargo-build-json.err" || run_failed
  built_route_gate "$artifacts/cargo-build.json" >"$artifacts/built.txt"
  IFS= read -r built <"$artifacts/built.txt" || built=
  case $built in
    "binary "*) exe=${built#binary } ;;
    *) run_failed ;;
  esac
  [ -f "$exe" ] || run_failed

  out=$artifacts/measure-peak.out
  err=$artifacts/measure-peak.err
  cargo run -q --locked -p halvo-cap --bin measure-peak -- --limit "$limit" --runs "$runs" -- \
    "$exe" probe --rounds "$rounds" >"$out" 2>"$err"
  code=$?

  read_runs "$out"
  [ "$well" = yes ] || run_failed
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

  [ "$ran" = yes ] || run_failed
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
    printf 'run n=%s exit=0 peak_bytes=%s capped=no start=179452604171%s000000 end=179452604174%s000000\n' \
      "$n" "$peak" "$n" "$n" >>"$file"
  done
  printf 'median peak_bytes=%s\nchecks exit=ok cap=ok median=ok\n' "$median" >>"$file"
}

artifact() {
  printf '{"reason":"compiler-artifact","package_id":"path+file://%s#0.0.0","manifest_path":"%s/Cargo.toml","target":{"kind":["%s"],"crate_types":["%s"],"name":"%s","src_path":"%s/%s","edition":"2021","doc":true,"doctest":false,"test":true},"profile":{"opt_level":"0","debuginfo":2,"debug_assertions":true,"overflow_checks":true,"test":false},"features":[],"filenames":[],"executable":%s,"fresh":%s}\n' \
    "$1" "$1" "$2" "$2" "$3" "$1" "$4" "$5" "$6"
}

differ_from_the_first() {
  first=
  while IFS=' ' read -r sum size file; do
    if [ -z "$first" ]; then
      first="$sum $size"
    elif [ "$sum $size" = "$first" ]; then
      return 1
    fi
  done <"$1"
  return 0
}

check_reset() {
  STUB_UNAME=Darwin
  STUB_BUILD_EXIT=0
  STUB_JSON=$work/good.json
  STUB_JSON_EXIT=0
  STUB_EXE=$work/repo/target/debug/route-gate
  STUB_OUT=$work/good.out
  STUB_ERR=$work/good.err
  STUB_CODE=0
  case_actions=
  case_repo=
}

check() {
  PATH=$work/bin:$PATH CI_D_REPO=${case_repo:-$work/repo} CI_D_ARTIFACTS=$work/artifacts \
    GITHUB_ACTIONS=$case_actions STUB_UNAME=$STUB_UNAME STUB_BUILD_EXIT=$STUB_BUILD_EXIT \
    STUB_JSON=$STUB_JSON STUB_JSON_EXIT=$STUB_JSON_EXIT STUB_EXE=$STUB_EXE STUB_OUT=$STUB_OUT \
    STUB_ERR=$STUB_ERR STUB_CODE=$STUB_CODE sh "$self" >"$work/case.txt" 2>&1
  code=$?
  out=
  while IFS= read -r line || [ -n "$line" ]; do
    out=$out$line$nl
  done <"$work/case.txt"
  out=${out%"$nl"}
  last=${out##*"$nl"}
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  case $out in
    *"$3"*) ;;
    *) got=$got-without-the-text ;;
  esac
  if [ -n "${4:-}" ]; then
    case $out in
      *"$4"*) got=$got-with-the-other-text ;;
    esac
  fi
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$got"
  if [ "$got" != "$2" ]; then
    wrong="$wrong $1"
  elif [ "$2" = FAIL ]; then
    rejected=$((rejected + 1))
  else
    accepted=$((accepted + 1))
  fi
  check_reset
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || setup_failed
  case $work in
    /*) ;;
    *) work=$(pwd)/$work ;;
  esac
  enter_repo
  mkdir -p "$work/bin" "$work/repo/target/debug" "$work/k-alt/debug" "$work/elsewhere" "$work/artifacts" || setup_failed
  cat >"$work/bin/cargo" <<'EOF'
#!/bin/sh
case $* in
  'build --locked -p halvo-route-gate --bin route-gate')
    exit "$STUB_BUILD_EXIT"
    ;;
  'build --locked -p halvo-route-gate --bin route-gate --message-format=json')
    if [ -f "$STUB_JSON" ]; then
      while IFS= read -r line || [ -n "$line" ]; do printf '%s\n' "$line"; done <"$STUB_JSON"
    fi
    exit "$STUB_JSON_EXIT"
    ;;
  "run -q --locked -p halvo-cap --bin measure-peak -- --limit 16777216 --runs 5 -- $STUB_EXE probe --rounds 100000")
    if [ -f "$STUB_OUT" ]; then
      while IFS= read -r line || [ -n "$line" ]; do printf '%s\n' "$line"; done <"$STUB_OUT"
    fi
    if [ -f "$STUB_ERR" ]; then
      while IFS= read -r line || [ -n "$line" ]; do printf '%s\n' "$line" >&2; done <"$STUB_ERR"
    fi
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

  gate=$work/repo/crates/halvo-route-gate
  route=$work/repo/crates/halvo-route
  : >"$work/repo/target/debug/route-gate" || setup_failed
  : >"$work/k-alt/debug/route-gate" || setup_failed
  : >"$work/elsewhere/route-gate" || setup_failed
  {
    artifact "$route" lib halvo_route src/lib.rs null true
    artifact "$gate" lib halvo_route_gate src/lib.rs null true
    artifact "$gate" bin route-gate src/main.rs "\"$work/repo/target/debug/route-gate\"" true
    printf '{"reason":"build-finished","success":true}\n'
  } >"$work/good.json" || setup_failed
  sed "s|\"$work/repo/target/debug/route-gate\"|\"$work/k-alt/debug/route-gate\"|" "$work/good.json" >"$work/k-alt.json"
  sed '2s/"fresh":true/"fresh":false/' "$work/good.json" >"$work/stale.json"
  sed '3d' "$work/good.json" >"$work/no-line.json"
  sed '3p' "$work/good.json" >"$work/two-lines.json"
  sed "3s|\"$work/repo/target/debug/route-gate\"|null|" "$work/good.json" >"$work/no-executable.json"
  sed "s|\"$work/repo/target/debug/route-gate\"|\"$work/missing/route-gate\"|" "$work/good.json" >"$work/not-a-file.json"
  {
    artifact "$work/repo/crates/other" bin route-gate src/main.rs "\"$work/elsewhere/route-gate\"" true
    cat "$work/good.json"
  } >"$work/other-package.json" || setup_failed
  sed 's|/|\\\\|g' "$work/good.json" >"$work/windows.json"
  {
    sed -n -e "3s|\"$work/repo/target/debug/route-gate\"|\"$work/elsewhere/route-gate\"|" -e '3s/^/warning: /p' "$work/good.json"
    cat "$work/good.json"
  } >"$work/after-text.json" || setup_failed

  probe="probe rounds=$rounds ok=yes"
  printf '%s\n%s\n%s\n%s\n%s\n' "$probe" "$probe" "$probe" "$probe" "$probe" >"$work/good.err"
  printf '%s\n%s\n%s\n%s\n' "$probe" "$probe" "$probe" "$probe" >"$work/four.err"
  sed 's/ok=yes/ok=no/' "$work/good.err" >"$work/miss.err"
  sed 's/rounds=100000/rounds=1/' "$work/good.err" >"$work/rounds.err"
  write_runs "$work/good.out" 1318912 1310720 1327104 1318912 1302528 1335296
  write_runs "$work/line.out" 13421772 13421772 13421772 13421772 13421772 13421772
  write_runs "$work/above.out" 13421773 13421773 13421773 13421773 13421773 13421773
  write_runs "$work/both.out" 13421773 13421773 13421773 16777217 13421773 13421773
  write_runs "$work/nothing.out" 0 0 0 0 0 0
  write_runs "$work/last-high.out" 1310720 1310720 1310720 1310720 1310720 13421773
  write_runs "$work/second-after.out" 1318912 1318912 1335296 1302528 1310720 1327104
  write_runs "$work/negative.out" -1 -1 -1 -1 -1 -1
  write_runs "$work/long.out" 1000000000000000 1000000000000000 1000000000000000 1000000000000000 1000000000000000 1000000000000000
  sed -e 's/n=3 exit=0 peak_bytes=1318912 capped=no/n=3 exit=signal-9 peak_bytes=16777217 capped=yes/' \
    -e 's/^median peak_bytes=1318912$/median peak_bytes=1327104/' \
    -e 's/^checks exit=ok cap=ok/checks exit=miss cap=miss/' "$work/good.out" >"$work/breach.out"
  sed -e 's/n=3 exit=0 peak_bytes=1318912 capped=no/n=3 exit=signal-9 peak_bytes=1000000000000000 capped=yes/' \
    -e 's/^median peak_bytes=1318912$/median peak_bytes=1327104/' \
    -e 's/^checks exit=ok cap=ok/checks exit=miss cap=miss/' "$work/good.out" >"$work/long-breach.out"
  sed -e 's/n=3 exit=0 peak_bytes=16777217 capped=no/n=3 exit=signal-9 peak_bytes=16777217 capped=yes/' \
    -e 's/^checks exit=ok cap=ok/checks exit=miss cap=miss/' "$work/both.out" >"$work/both-breach.out"
  sed '/^run n=5 /d' "$work/good.out" >"$work/four.out"
  sed 's/^median peak_bytes=1318912$/median peak_bytes=1310720/' "$work/good.out" >"$work/median.out"
  sed 's/^checks exit=ok cap=ok median=ok$/checks exit=ok cap=ok median=miss/' "$work/good.out" >"$work/checks.out"
  sed 's/^checks exit=ok cap=ok median=ok$/checks exit=ok cap=ok/' "$work/good.out" >"$work/checks-form.out"
  sed 's/limit_bytes=16777216/limit_bytes=16777217/' "$work/good.out" >"$work/limit.out"
  sed 's/ limit_bytes=16777216 runs=5$//' "$work/good.out" >"$work/no-suffix.out"
  sed 's/^measure-peak platform=macos /platform=macos /' "$work/good.out" >"$work/no-prefix.out"
  sed 's/^measure-peak platform=macos mode=poll /measure-peak platform=macos mode=taskpolicy /' "$work/good.out" >"$work/taskpolicy.out"
  sed 's/^measure-peak platform=macos mode=poll /measure-peak platform=windows mode=job /' "$work/good.out" >"$work/windows.out"
  sed 's/^measure-peak platform=macos mode=poll /measure-peak platform=windows mode=job-commit /' "$work/good.out" >"$work/job-commit.out"
  sed 's/n=2 exit=0/n=2 exit=1/' "$work/good.out" >"$work/exit.out"
  sed 's/n=2 exit=0 /n=2 exit= /' "$work/good.out" >"$work/empty-exit.out"
  sed 's/peak_bytes=1327104/peak_bytes=01327104/' "$work/good.out" >"$work/zero.out"
  sed 's/peak_bytes=1327104 /peak_bytes= /' "$work/good.out" >"$work/empty-peak.out"
  sed 's/^\(run n=1 .*\) start=[0-9]* /\1 start= /' "$work/good.out" >"$work/empty-start.out"
  sed 's/^\(run n=1 .*\) end=[0-9]*$/\1 end=/' "$work/good.out" >"$work/empty-end.out"
  sed 's/^\(run n=1 .*\) end=\([0-9]*\)$/\1 end=\2 more/' "$work/good.out" >"$work/end-word.out"
  cp "$work/good.out" "$work/extra.out" && printf 'checks exit=ok cap=ok median=ok\n' >>"$work/extra.out"
  sed 's/n=4 exit=0 peak_bytes=1302528 capped=no/n=4 exit=0 peak_bytes=1302528 capped=yes/' "$work/good.out" >"$work/capped.out"
  sed 's/n=4 exit=0 peak_bytes=1302528 capped=no/n=4 exit=0 peak_bytes=1302528 capped=maybe/' "$work/good.out" >"$work/capped-word.out"
  sed 's/^median peak_bytes=1318912$/median peak_bytes=-1318912/' "$work/good.out" >"$work/median-count.out"
  sed '/^checks /d' "$work/good.out" >"$work/short.out"
  sed 's/^median peak_bytes=1318912$/median bytes=1318912/' "$work/good.out" >"$work/median-form.out"
  sed 's/^median peak_bytes=1318912$/1318912/' "$work/good.out" >"$work/median-key.out"
  sed -e 's/^run n=1 /run n=X /' -e 's/^run n=2 /run n=1 /' -e 's/^run n=X /run n=2 /' "$work/good.out" >"$work/order.out"
  sed 's/n=1 exit=0 peak_bytes=1310720 capped=no/n=1 exit=0 peak_bytes=1310720 rss=1 capped=no/' "$work/good.out" >"$work/key.out"
  set -- "$work/good.out"
  for each in breach long-breach four median checks checks-form limit no-suffix no-prefix taskpolicy windows job-commit \
    exit empty-exit zero empty-peak empty-start empty-end end-word extra capped capped-word median-count short median-form median-key order key; do
    set -- "$@" "$work/$each.out"
  done
  cksum "$@" >"$work/out.sums" || setup_failed
  differ_from_the_first "$work/out.sums" || setup_failed
  set -- "$work/good.json"
  for each in k-alt stale no-line two-lines no-executable not-a-file other-package windows after-text; do
    set -- "$@" "$work/$each.json"
  done
  cksum "$@" >"$work/json.sums" || setup_failed
  differ_from_the_first "$work/json.sums" || setup_failed

  rejected=0
  accepted=0
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
  STUB_OUT=$work/windows.out
  STUB_JSON=$work/windows.json
  case_actions=true
  check hosted-win-with-a-path-of-windows PASS 'bound part=router machine=hosted-win bound_bytes=8388608'
  STUB_UNAME=MINGW64_NT-10.0-26100
  STUB_OUT=$work/job-commit.out
  case_actions=true
  check hosted-win-job-commit PASS 'bound part=router machine=hosted-win bound_bytes=8388608'
  STUB_UNAME=MINGW64_NT-10.0-26100
  STUB_OUT=$work/windows.out
  check windows-outside-actions FAIL "FAIL $name (run)"
  STUB_UNAME=Linux
  check other-system FAIL "FAIL $name (run)"
  case_actions=false
  check actions-not-true PASS "bound part=router machine=mac bound_bytes=8388608"
  STUB_OUT=$work/taskpolicy.out
  check mode-taskpolicy PASS "PASS $name"

  STUB_JSON=$work/k-alt.json
  STUB_EXE=$work/k-alt/debug/route-gate
  check artifact-in-another-directory PASS "PASS $name"
  STUB_JSON=$work/other-package.json
  check binary-of-another-package-not-taken PASS "PASS $name"
  STUB_JSON=$work/after-text.json
  check artifact-after-other-text-not-taken PASS "PASS $name"
  STUB_BUILD_EXIT=101
  check build-fails FAIL "FAIL $name (run)"
  STUB_JSON_EXIT=101
  check second-build-fails FAIL "FAIL $name (run)"
  STUB_JSON=$work/stale.json
  check artifact-line-not-fresh FAIL "FAIL $name (run)"
  STUB_JSON=$work/no-line.json
  check no-line-for-the-binary FAIL "FAIL $name (run)"
  STUB_JSON=$work/two-lines.json
  check two-lines-for-the-binary FAIL "FAIL $name (run)"
  STUB_JSON=$work/no-executable.json
  check line-without-an-executable FAIL "FAIL $name (run)"
  STUB_JSON=$work/not-a-file.json
  STUB_EXE=$work/missing/route-gate
  check executable-not-a-file FAIL "FAIL $name (run)"

  STUB_OUT=$work/line.out
  check median-at-the-line PASS 'bound part=router machine=mac bound_bytes=16777216'
  STUB_OUT=$work/above.out
  check peak-above-the-line FAIL "bound part=router machine=mac run=5 peak_bytes=13421773
bound part=router machine=mac bound_bytes=25165824
FAIL $name (bound)"
  STUB_OUT=$work/last-high.out
  check last-run-in-another-step PASS "bound part=router machine=mac run=5 peak_bytes=13421773
bound part=router machine=mac bound_bytes=8388608
PASS $name"
  STUB_OUT=$work/second-after.out
  check second-lowest-after-the-median PASS "PASS $name"
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
  STUB_ERR=$work/rounds.err
  check probe-lines-of-other-rounds FAIL "FAIL $name (run)"
  STUB_OUT=$work/median.out
  check median-not-the-middle FAIL "FAIL $name (run)"
  STUB_OUT=$work/checks.out
  check checks-miss-with-exit-0 FAIL "FAIL $name (run)"
  STUB_OUT=$work/checks-form.out
  check checks-line-of-another-form FAIL "FAIL $name (run)" 'bound part='
  STUB_OUT=$work/limit.out
  check another-limit FAIL "FAIL $name (run)"
  STUB_OUT=$work/no-suffix.out
  check header-without-limit-and-runs FAIL "FAIL $name (run)"
  STUB_OUT=$work/no-prefix.out
  check header-without-its-prefix FAIL "FAIL $name (run)"
  STUB_OUT=$work/windows.out
  check header-of-another-system FAIL "FAIL $name (run)"
  STUB_UNAME=MINGW64_NT-10.0-26100
  STUB_JSON=$work/windows.json
  case_actions=true
  check header-of-macos-on-windows FAIL "FAIL $name (run)"
  STUB_OUT=$work/exit.out
  STUB_CODE=1
  check child-exits-1 FAIL "FAIL $name (run)"
  STUB_OUT=$work/exit.out
  check child-exits-1-with-checks-ok FAIL "FAIL $name (run)"
  STUB_OUT=$work/empty-exit.out
  check empty-exit FAIL "FAIL $name (run)" 'bound part='
  STUB_OUT=$work/zero.out
  check leading-zero FAIL "FAIL $name (run)"
  STUB_OUT=$work/nothing.out
  check peaks-of-0 FAIL "FAIL $name (run)"
  STUB_OUT=$work/empty-peak.out
  check empty-peak FAIL "FAIL $name (run)" 'bound part='
  STUB_OUT=$work/negative.out
  check negative-peaks FAIL "FAIL $name (run)"
  STUB_OUT=$work/long.out
  check peaks-of-16-digits FAIL "FAIL $name (run)" 'bound part='
  STUB_OUT=$work/long-breach.out
  STUB_CODE=1
  check run-of-16-digits-stopped-by-the-cap FAIL "FAIL $name (run)"
  STUB_OUT=$work/empty-start.out
  check empty-start FAIL "FAIL $name (run)"
  STUB_OUT=$work/empty-end.out
  check empty-end FAIL "FAIL $name (run)"
  STUB_OUT=$work/end-word.out
  check word-after-end FAIL "FAIL $name (run)"
  STUB_OUT=$work/extra.out
  check line-after-checks FAIL "FAIL $name (run)"
  STUB_OUT=$work/short.out
  check no-checks-line FAIL "FAIL $name (run)"
  STUB_OUT=$work/median-form.out
  check median-line-of-another-form FAIL "FAIL $name (run)"
  STUB_OUT=$work/median-key.out
  check median-line-without-its-key FAIL "FAIL $name (run)"
  STUB_OUT=$work/order.out
  check run-lines-out-of-order FAIL "FAIL $name (run)"
  STUB_OUT=$work/capped.out
  check capped-with-checks-ok FAIL "FAIL $name (run)"
  STUB_OUT=$work/capped-word.out
  check capped-of-another-word FAIL "FAIL $name (run)"
  STUB_OUT=$work/median-count.out
  check median-not-a-count FAIL "FAIL $name (run)" 'bound part='
  STUB_CODE=1
  check measure-peak-exits-1-alone FAIL "bound_bytes=8388608
FAIL $name (run)"
  STUB_OUT=$work/key.out
  check run-line-with-another-key FAIL "FAIL $name (run)"
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
  printf 'self-test cases passing=%s failing=%s\n' "$accepted" "$rejected"
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
