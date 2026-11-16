#!/bin/sh
# The U0.3c step of the scratch workflow (never merged). sh .u03c/step.sh PART, PART one of:
#   tests    the profile trial's clippy and whole tests once, each new or changed test 200 times by its
#            exact name and the result line's count, the collector's and the harness's self-tests
#   item3    hosted-win: item 3 with the first and the final flag set, with their egress_denial lines
#   harness  the hosted harness run for real, the state of its settings before and after, then a
#            second run ended by TERM after its first policy is set, over an earlier setting
# Each part prints u03c_ lines and its seconds; CI logs are public, so lines hold counts and words.
set +e
code=0
case "$(uname -s)" in
  Darwin)
    machine=hosted-mac
    chrome="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
    ;;
  *)
    machine=hosted-win
    chrome="C:/Program Files/Google/Chrome/Application/chrome.exe"
    ;;
esac
s0=$(date +%s)
seconds() {
  echo "u03c_seconds machine=$machine part=$1 seconds=$(($(date +%s) - s0))"
}
py=python3
command -v python3 >/dev/null 2>&1 || py=python
[ "$machine" = hosted-win ] && py=python

bin_of() {
  cargo test --locked --manifest-path tools/r3-trials/profile/Cargo.toml "$1" --no-run --message-format=json 2>/dev/null |
    grep '"test":true' | grep "\"name\":\"$2\"" |
    sed -n 's/.*"executable":"\([^"]*\)".*/\1/p' | sed 's|\\\\|/|g' | head -1
}

loop() {
  bin=$1
  name=$2
  want=$3
  n=0
  ok=0
  zero=0
  other=0
  while [ "$n" -lt "$want" ]; do
    out=$("$bin" --exact "$name" --test-threads=1 2>&1)
    case "$out" in
      *"test result: ok. 1 passed; 0 failed;"*) ok=$((ok + 1)) ;;
      *"test result: ok. 0 passed;"*) zero=$((zero + 1)) ;;
      *) other=$((other + 1)) ;;
    esac
    n=$((n + 1))
  done
  echo "u03c_loop machine=$machine test=$name runs=$n passed_with_count_1=$ok ran_no_test=$zero other=$other expected_count=1"
  [ "$ok" -eq "$want" ] || code=1
}

tests_part() {
  m=tools/r3-trials/profile/Cargo.toml
  cargo clippy --locked --quiet --manifest-path "$m" --all-targets -- -D warnings
  e=$?
  echo "u03c_clippy machine=$machine exit=$e"
  [ "$e" -eq 0 ] || code=1
  seconds clippy
  s0=$(date +%s)
  cargo test --locked --manifest-path "$m" --no-fail-fast >"$RUNNER_TEMP/u03c-test.log" 2>&1
  e=$?
  grep -E '^test result:' "$RUNNER_TEMP/u03c-test.log" | awk '{ p += $4; f += $6 } END { print "u03c_tests passed=" p " failed=" f }'
  grep -E '^test .* FAILED$' "$RUNNER_TEMP/u03c-test.log"
  echo "u03c_tests machine=$machine exit=$e"
  [ "$e" -eq 0 ] || code=1
  seconds tests
  s0=$(date +%s)
  lib=$(bin_of --lib r3_profile)
  pbin=$(bin_of --bin=profile-trial profile-trial)
  sbin=$(bin_of --test=stand_in_item3 stand_in_item3)
  rbin=$(bin_of --test=run_sh run_sh)
  echo "u03c_bins lib=$([ -x "$lib" ] && echo yes || echo no) program=$([ -x "$pbin" ] && echo yes || echo no) stand_in=$([ -x "$sbin" ] && echo yes || echo no) run_sh=$([ -x "$rbin" ] && echo yes || echo no)"
  loop "$lib" product::tests::each_denial_prints_its_protocol_its_port_and_whether_its_host_is_loopback 200
  loop "$lib" tests::every_fault_of_the_start_itself_is_a_browser_that_did_not_start 200
  loop "$pbin" item3_end_tests::each_inner_line_reaches_its_printed_line_through_the_end_of_item_3 200
  loop "$sbin" the_whole_program_prints_each_line_of_item_3_through_a_stand_in_check 200
  loop "$rbin" the_argument_reader_follows_the_usage_line_of_a13_3 200
  loop "$rbin" the_reader_hands_each_argument_on_to_the_program 200
  loop "$pbin" item3_end_tests::the_control_alone_starts_the_browser_with_its_own_sandbox_off 200
  loop "$lib" product::tests::the_outer_item_3_line_follows_the_inner_line_by_its_keys 200
  loop "$lib" product::tests::a_last_line_with_no_newline_still_prints_its_denial 200
  if [ "$machine" = hosted-mac ]; then
    cbin=$(bin_of --test=stand_in_control stand_in_control)
    loop "$cbin" the_control_alone_starts_its_browser_with_the_sandbox_off 200
    loop "$cbin" the_task_shape_reads_a_start_that_never_answers_as_browser_start 200
    loop "$cbin" items_1_and_2_read_a_start_that_never_answers_as_browser_start 20
  fi
  seconds loops
  s0=$(date +%s)
  sh tools/r3-trials/web/hosted/harness.sh --self-test >"$RUNNER_TEMP/u03c-harness.log" 2>&1
  e=$?
  grep ' ok=no' "$RUNNER_TEMP/u03c-harness.log"
  echo "u03c_harness_self_test machine=$machine exit=$e $(tail -1 "$RUNNER_TEMP/u03c-harness.log")"
  [ "$e" -eq 0 ] || code=1
  "$py" -I tools/r3-trials/web/hosted/collect.py --self-test >"$RUNNER_TEMP/u03c-collect.log" 2>&1
  e=$?
  grep ' ok=no' "$RUNNER_TEMP/u03c-collect.log"
  echo "u03c_collect_self_test machine=$machine exit=$e $(tail -1 "$RUNNER_TEMP/u03c-collect.log")"
  [ "$e" -eq 0 ] || code=1
  seconds self_tests
}

item3_part() {
  for set in first final; do
    scratch=$(mktemp -d "$RUNNER_TEMP/u03c-item3.XXXXXX")
    extra=
    [ "$set" = first ] && extra="--flags first"
    # shellcheck disable=SC2086
    sh tools/r3-trials/profile/run.sh --item 3 $extra --browser "$chrome" --port 18090 --scratch "$scratch"
    e=$?
    echo "u03c_item3 machine=$machine set=$set exit=$e"
  done
  seconds item3
}

mac_state() {
  d="/Library/Managed Preferences"
  if [ -d "$d" ]; then
    n=$(sudo find "$d" -mindepth 1 -maxdepth 1 | wc -l | tr -d ' ')
    c=no
    [ -e "$d/com.google.Chrome.plist" ] && c=$(sudo shasum -a 256 "$d/com.google.Chrome.plist" | cut -c1-12)
    e=no
    [ -e "$d/com.microsoft.Edge.plist" ] && e=yes
    echo "u03c_state machine=$machine at=$1 managed_dir=yes entries=$n chrome_plist=$c edge_plist=$e"
  else
    echo "u03c_state machine=$machine at=$1 managed_dir=no"
  fi
  echo "u03c_procs machine=$machine at=$1 naming_org_work=$(pgrep -f -- "$RUNNER_TEMP/org" | wc -l | tr -d ' ')"
}

win_state() {
  for k in 'Google' 'Google\Chrome' 'Microsoft\Edge'; do
    if MSYS_NO_PATHCONV=1 reg query "HKLM\\SOFTWARE\\Policies\\$k" >/dev/null 2>&1; then x=yes; else x=no; fi
    v=$(MSYS_NO_PATHCONV=1 reg query "HKLM\\SOFTWARE\\Policies\\$k" /v RemoteDebuggingAllowed 2>/dev/null | tr -d '\r' | awk '$1 == "RemoteDebuggingAllowed" { print $NF }')
    p=$(MSYS_NO_PATHCONV=1 reg query "HKLM\\SOFTWARE\\Policies\\$k" /v ProxyMode 2>/dev/null | tr -d '\r' | awk '$1 == "ProxyMode" { print $NF }')
    echo "u03c_state machine=$machine at=$1 key=$(printf '%s' "$k" | tr '\134' '/') exists=$x remote_debugging=${v:--} proxy_mode=${p:--}"
  done
  n=$(powershell -NoProfile -Command "@(Get-CimInstance Win32_Process | Where-Object { \$_.CommandLine -like '*org.*' -and \$_.CommandLine -like '*user-data-dir*' }).Count" 2>/dev/null | tr -d '\r')
  echo "u03c_procs machine=$machine at=$1 browsers_with_a_harness_profile=${n:-unread}"
}

state() {
  if [ "$machine" = hosted-mac ]; then mac_state "$1"; else win_state "$1"; fi
}

harness_part() {
  host=$(rustc -vV | sed -n 's/^host: //p')
  cargo build --release --locked --quiet --target "$host" --target-dir tools/r3-trials/web/target \
    --manifest-path tools/r3-trials/web/Cargo.toml
  echo "u03c_web_build machine=$machine exit=$?"
  state before_run
  RUNS=2 sh tools/r3-trials/web/hosted/harness.sh >"$RUNNER_TEMP/u03c-harness-run.log" 2>&1
  e=$?
  grep -E '^(policy (set|removed|page|value)|prefs_daemon|port_check|harness_end|pypdf_|notrun|call_end|cell |listing|managed_dir|browser |bin built)' "$RUNNER_TEMP/u03c-harness-run.log"
  echo "u03c_harness_run machine=$machine exit=$e"
  state after_run
  seconds harness_run
  s0=$(date +%s)
  google_before=no
  if [ "$machine" = hosted-win ] && MSYS_NO_PATHCONV=1 reg query 'HKLM\SOFTWARE\Policies\Google' >/dev/null 2>&1; then
    google_before=yes
  fi
  if [ "$machine" = hosted-mac ]; then
    d="/Library/Managed Preferences"
    sudo mkdir -p "$d"
    printf '<?xml version="1.0" encoding="UTF-8"?>\n<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">\n<plist version="1.0">\n<dict>\n\t<key>MetricsReportingEnabled</key>\n\t<false/>\n</dict>\n</plist>\n' >"$RUNNER_TEMP/u03c-earlier.plist"
    sudo install -o root -g wheel -m 644 "$RUNNER_TEMP/u03c-earlier.plist" "$d/com.google.Chrome.plist"
    want=$(sudo shasum -a 256 "$d/com.google.Chrome.plist" | cut -c1-12)
    echo "u03c_earlier machine=$machine chrome_plist=$want"
  else
    MSYS_NO_PATHCONV=1 reg add 'HKLM\SOFTWARE\Policies\Google\Chrome' /v RemoteDebuggingAllowed /t REG_DWORD /d 1 /f >/dev/null
    echo "u03c_earlier machine=$machine remote_debugging=1"
  fi
  state before_term
  RUNS=2 sh tools/r3-trials/web/hosted/harness.sh >"$RUNNER_TEMP/u03c-harness-term.log" 2>&1 &
  hp=$!
  i=0
  while [ "$i" -lt 900 ] && ! grep -Eq '^policy (file|set) browser=chrome name=RemoteDebuggingAllowed' "$RUNNER_TEMP/u03c-harness-term.log"; do
    sleep 1
    i=$((i + 1))
  done
  echo "u03c_term machine=$machine waited_s=$i set_seen=$(grep -Ec '^policy (file|set) browser=chrome' "$RUNNER_TEMP/u03c-harness-term.log")"
  t0=$(date +%s)
  kill -TERM "$hp"
  wait "$hp"
  e=$?
  j=$(($(date +%s) - t0))
  grep -E '^(policy (set|removed)|prefs_daemon|harness_end)' "$RUNNER_TEMP/u03c-harness-term.log"
  echo "u03c_term machine=$machine harness_exit=$e seconds_to_end=$j"
  sleep 5
  state after_term
  if [ "$machine" = hosted-mac ]; then
    got=no
    [ -e "/Library/Managed Preferences/com.google.Chrome.plist" ] && got=$(sudo shasum -a 256 "/Library/Managed Preferences/com.google.Chrome.plist" | cut -c1-12)
    echo "u03c_put_back machine=$machine same=$([ "$got" = "$want" ] && echo yes || echo no)"
    [ "$got" = "$want" ] || code=1
    sudo mv "/Library/Managed Preferences/com.google.Chrome.plist" "$RUNNER_TEMP/u03c-earlier-back.plist"
  else
    v=$(MSYS_NO_PATHCONV=1 reg query 'HKLM\SOFTWARE\Policies\Google\Chrome' /v RemoteDebuggingAllowed 2>/dev/null | tr -d '\r' | awk '$1 == "RemoteDebuggingAllowed" { print $NF }')
    echo "u03c_put_back machine=$machine same=$([ "$v" = 0x1 ] && echo yes || echo no)"
    [ "$v" = 0x1 ] || code=1
    if [ "$google_before" = no ]; then
      MSYS_NO_PATHCONV=1 reg delete 'HKLM\SOFTWARE\Policies\Google' /f >/dev/null 2>&1
    else
      MSYS_NO_PATHCONV=1 reg delete 'HKLM\SOFTWARE\Policies\Google\Chrome' /v RemoteDebuggingAllowed /f >/dev/null 2>&1
    fi
  fi
  seconds harness_term
}

echo "u03c machine=$machine part=$1 at=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
case $1 in
  tests) tests_part ;;
  item3) item3_part ;;
  harness) harness_part ;;
  *) echo "u03c unknown part $1" && exit 2 ;;
esac
exit "$code"
