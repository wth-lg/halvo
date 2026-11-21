#!/bin/sh
# jobs: macos windows
# rule: On a hosted runner the job's own run of the dialog targets of halvo-coord-app prints no inconclusive line of guard_hosted (its app did not become active within 20 s, failed with the cause inconclusive), no line of a target's start that ended it before its first case (a line start FAIL of either target or the inconclusive line of dialog_hosted, failed with the cause start) and each target's own end line with every case of that system run and none failed (dialog_hosted 4 of 4 on macOS and 5 of 5 on Windows, guard_hosted 7 of 7 on macOS and 8 of 8 on Windows, with no help, no report taken at a press and no report made before its window was visible, and its count of slipped schedules, each case run again by the target at most three times in all) and no case of guard_hosted whose schedule slipped in each of its runs (too few runs kept the schedule) and no line of the loop mode (a case SLIPPED), the targets run with GUARD_HOSTED_SCHEDULE and GUARD_HOSTED_TALLY unset and no line that a target was not run, cargo exits 0 and no tracked file differs from HEAD, and it prints into the job's log, above its verdict, the own lines of each failed case and of each start, watchdog or cut-short run that ended a target, with their line numbers in dialog-targets.txt; off a hosted runner it skips.
# lane: alone
set -u
name=46-r4-dialog-ran
off='hosted alone: not run on this machine'
targets='dialog_hosted 4 5
guard_hosted 7 8'

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

broke() {
  printf '%s\n' "$1"
  broken="$broken${broken:+; }$1"
}

wanted() {
  printf '%s\n' "$targets" | while read -r target macos windows; do
    case $1 in
      macos) printf '%s %s\n' "$target" "$macos" ;;
      *) printf '%s %s\n' "$target" "$windows" ;;
    esac
  done
}

decide() {
  log=$1
  code=$2
  want=$3
  [ "$code" = 0 ] || broke "cargo test -p halvo-coord-app --locked --no-fail-fast --test dialog_hosted --test guard_hosted exited $code"
  offs=$(grep -c -x -F "$off" "$log")
  [ "$offs" = 0 ] || broke "a target printed the line that it was not run, $offs times"
  while read -r target count; do
    line="$target end: cases ran $count of $count, failed 0"
    case $target in
      guard_hosted) tail=', schedule slipped [0-9]+, helped 0, moments at a press 0, retries [0-9]+, reports before visible 0' ;;
      *) tail= ;;
    esac
    ends=$(grep -c "^$target end: " "$log")
    exact=$(grep -c -x -E "$line$tail" "$log")
    printf '%s: end lines %s, of them the line of %s of %s with none failed %s\n' "$target" "$ends" "$count" "$count" "$exact"
    if [ "$ends" != 1 ]; then
      broke "$target printed $ends end lines, not one"
    elif [ "$exact" != 1 ]; then
      found=$(grep "^$target end: " "$log")
      broke "$target printed \"$found\", not \"$line$tail\""
    fi
  done <<EOF
$want
EOF
  few=$(grep -c '^guard_hosted case .* FAIL: .*too few runs kept the schedule' "$log")
  [ "$few" = 0 ] || broke "too few runs kept the schedule: in $few cases of guard_hosted the schedule slipped in each of the runs that the target makes"
  looped=$(grep -c '^guard_hosted case .* SLIPPED: ' "$log")
  [ "$looped" = 0 ] || broke "guard_hosted printed $looped lines of the loop mode (a case SLIPPED) in this check's own run, which runs each case once and again after a slip; GUARD_HOSTED_SCHEDULE reached the run"
}

told() {
  awk '
    function flush(what, i) {
      printf "%s %s: its own lines, from line %d to line %d of dialog-targets.txt\n", t, what, at[1], at[n]
      for (i = 1; i <= n; i++) printf "  %s\n", kept[i]
      n = 0
    }
    /^(dialog_hosted|guard_hosted) / {
      if ($1 != t) {
        if (n > 0) flush("printed no end line after these lines")
        t = $1
      }
      n++
      at[n] = NR
      kept[n] = $0
      s = substr($0, length(t) + 2)
      if (s ~ /^end: / || s ~ /^((bound|ink) )?case [^ ]+ [^ ]+ (PASS$|SLIPPED: )/) n = 0
      else if (s ~ /^((bound|ink) )?case [^ ]+ [^ ]+ FAIL: /) flush("failed " substr(s, 1, index(s, " FAIL: ") - 1))
      else if (s ~ /^(start FAIL|inconclusive): /) flush("start ended before its first case")
      else if (s ~ /^watchdog FAIL: /) flush("ended at its watchdog")
    }
    END { if (n > 0) flush("printed no end line after these lines") }
  ' "$1"
}

started() {
  awk '
    /^(dialog_hosted|guard_hosted) start FAIL: / { ended = $1 "\047s start ended before its first case: " substr($0, length($1) + 14) }
    /^dialog_hosted inconclusive: / { ended = $1 "\047s start ended before its first case: " substr($0, length($1) + 2) }
    ended != "" { all = all (all == "" ? "" : "; ") ended; ended = "" }
    END { if (all != "") print all }
  ' "$1"
}

tracked() {
  status=$(git status --porcelain --untracked-files=no) || fail "git status failed $1 cargo ran"
  [ -z "$status" ] && return 0
  case $1 in
    before) fail "a tracked file differs from HEAD before cargo ran, so commit or stash the change, then run the check again" ;;
    *) fail "a tracked file differs from HEAD after cargo ran: a test or a build of the run wrote a tracked file, so read git status and mend what wrote it" ;;
  esac
}

measure() {
  case ${CI_D_JOB:-} in
    macos | windows) ;;
    *) fail "the job is ${CI_D_JOB:-not set}, not macos or windows" ;;
  esac
  want=$(wanted "$CI_D_JOB")
  if [ "${GITHUB_ACTIONS:-}" != true ] || [ "${RUNNER_ENVIRONMENT:-}" != github-hosted ]; then
    printf 'SKIP %s (the dialog targets show their dialogs on a hosted runner alone)\n' "$name"
    exit 77
  fi
  [ -n "${CI_D_REPO:-}" ] || fail "CI_D_REPO is not set"
  [ -n "${CI_D_ARTIFACTS:-}" ] || fail "CI_D_ARTIFACTS is not set"
  cd "$CI_D_REPO" || fail "the repository directory cannot be entered"
  [ -d "$CI_D_ARTIFACTS" ] || mkdir -p "$CI_D_ARTIFACTS" || fail "the artifacts directory cannot be made"
  tracked before
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail "mktemp failed"
  env -u GUARD_HOSTED_SCHEDULE -u GUARD_HOSTED_TALLY cargo test -p halvo-coord-app --locked --no-fail-fast --test dialog_hosted --test guard_hosted >"$work/out" 2>&1
  code=$?
  tracked after
  file=$CI_D_ARTIFACTS/dialog-targets.txt
  tr -d '\r' <"$work/out" >"$file" || fail "the output of the run cannot be written to the artifacts directory"
  broken=
  decide "$file" "$code" "$want"
  told "$file" || fail "the lines of the targets cannot be read from the artifacts directory"
  if grep -q '^guard_hosted inconclusive: ' "$file"; then
    fail "inconclusive: guard_hosted's app did not become active within 20 s on this runner, so the run is no result; run the job again"
  fi
  ended=$(started "$file") || fail "the lines of the targets' starts cannot be read"
  [ -z "$ended" ] || fail "start: $ended"
  [ -z "$broken" ] || fail "$broken"
  printf 'PASS %s\n' "$name"
  exit 0
}

check() {
  label=$1
  want=$2
  text=$3
  job=$4
  shift 4
  if [ "$job" = --bogus ]; then
    out=$(sh "$self" --bogus 2>&1)
  else
    out=$(env PATH="$work/bin:$PATH" STUB_CALLS="$work/calls.$label" STUB_OUT="$STUB_OUT" \
      STUB_EXIT="$STUB_EXIT" STUB_WRITES="$STUB_WRITES" CI_D_REPO="$work/repo" \
      CI_D_ARTIFACTS="$work/artifacts" CI_D_JOB="$job" GITHUB_ACTIONS=true \
      RUNNER_ENVIRONMENT=github-hosted "$@" sh "$self" 2>&1)
  fi
  code=$?
  last=${out##*"
"}
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    "77:SKIP $name ("*")") got=SKIP ;;
    *) got=exit-$code ;;
  esac
  case $out in
    *"$text"*) ;;
    *) got=$got-without-the-text ;;
  esac
  if [ -n "$absent" ]; then
    case $out in
      *"$absent"*) got=$got-with-the-absent-text ;;
    esac
  fi
  if [ "$want" = SKIP ] && [ -s "$work/calls.$label" ]; then
    got=$got-after-cargo-ran
  fi
  printf 'case %s/%s want=%s got=%s\n' "$name" "$label" "$want" "$got"
  if [ "$got" != "$want" ]; then
    wrong="$wrong $label"
  elif [ "$want" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
  STUB_EXIT=0
  STUB_WRITES=no
  absent=
}

output() {
  STUB_OUT=$work/out.$1
  shift
  : >"$STUB_OUT" || exit 2
  for line in "$@"; do
    printf '%s\r\n' "$line" >>"$STUB_OUT" || exit 2
  done
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  mkdir -p "$work/bin" "$work/repo" || exit 2
  cat >"$work/bin/cargo" <<'EOF'
#!/bin/sh
printf '%s\n' "$*" >>"$STUB_CALLS"
[ "$*" = 'test -p halvo-coord-app --locked --no-fail-fast --test dialog_hosted --test guard_hosted' ] || exit 97
[ "$STUB_WRITES" = no ] || printf 'written by a test\n' >>tracked.txt
cat "$STUB_OUT"
exit "$STUB_EXIT"
EOF
  chmod +x "$work/bin/cargo" || exit 2
  found=$(env PATH="$work/bin:$PATH" sh -c 'command -v cargo') || exit 2
  if [ "$found" != "$work/bin/cargo" ]; then
    printf 'FAIL self-test:%s (cargo is not the stub of the self-test)\n' "$name"
    exit 1
  fi
  printf 'tracked\n' >"$work/repo/tracked.txt" || exit 2
  git -C "$work/repo" init -q || exit 2
  git -C "$work/repo" add tracked.txt || exit 2
  git -C "$work/repo" commit -q -m fixture || exit 2
  STUB_EXIT=0
  STUB_WRITES=no
  absent=
  rejected=0
  wrong=
  running='     Running tests/dialog_hosted.rs (target/debug/deps/dialog_hosted-0123456789abcdef)'
  mac_dialog='dialog_hosted end: cases ran 4 of 4, failed 0'
  counts=', schedule slipped 0, helped 0, moments at a press 0, retries'
  mac_guard="guard_hosted end: cases ran 7 of 7, failed 0$counts 0, reports before visible 0"
  win_dialog='dialog_hosted end: cases ran 5 of 5, failed 0'
  win_guard="guard_hosted end: cases ran 8 of 8, failed 0$counts 2, reports before visible 0"

  output macos "$running" 'dialog_hosted case macos press_allow PASS' "$mac_dialog" "$mac_guard"
  absent='its own lines'
  check macos-own-lines PASS 'guard_hosted: end lines 1, of them the line of 7 of 7 with none failed 1' macos
  output windows "$running" "$win_dialog" "$win_guard"
  check windows-own-lines PASS 'dialog_hosted: end lines 1, of them the line of 5 of 5 with none failed 1' windows
  output off "$off" "$mac_guard"
  check macos-hosted-alone FAIL 'a target printed the line that it was not run, 1 times' macos
  output off-both "$off" "$off"
  check macos-both-hosted-alone FAIL 'dialog_hosted printed 0 end lines, not one' macos
  output off-beside "$off" "$mac_dialog" "$mac_guard"
  check macos-hosted-alone-beside-the-lines FAIL 'a target printed the line that it was not run, 1 times' macos
  output short 'dialog_hosted end: cases ran 3 of 4, failed 0' "$mac_guard"
  check macos-short-count FAIL 'dialog_hosted printed "dialog_hosted end: cases ran 3 of 4, failed 0", not "dialog_hosted end: cases ran 4 of 4, failed 0"' macos
  output short-flag 'dialog_hosted end: cases ran 3 of 4, failed 0, FAIL: not every case ran' "$mac_guard"
  check macos-short-count-with-its-mark FAIL 'not "dialog_hosted end: cases ran 4 of 4, failed 0"' macos
  output win-short "$win_dialog" "guard_hosted end: cases ran 7 of 8, failed 0$counts 0"
  check windows-short-count FAIL 'not "guard_hosted end: cases ran 8 of 8, failed 0, schedule slipped [0-9]+, helped 0' windows
  output win-count "$win_dialog" "$win_guard"
  check macos-job-with-the-windows-count FAIL 'not "dialog_hosted end: cases ran 4 of 4, failed 0"' macos
  output mac-count "$mac_dialog" "$mac_guard"
  check windows-job-with-the-macos-count FAIL 'not "dialog_hosted end: cases ran 5 of 5, failed 0"' windows
  output zero 'dialog_hosted end: cases ran 0 of 0, failed 0' "$mac_guard"
  check macos-zero-of-zero FAIL 'not "dialog_hosted end: cases ran 4 of 4, failed 0"' macos
  output failed 'dialog_hosted end: cases ran 4 of 4, failed 1' "$mac_guard"
  check macos-a-failed-case FAIL 'not "dialog_hosted end: cases ran 4 of 4, failed 0"' macos
  output inconclusive "$mac_dialog" 'guard_hosted inconclusive: the app did not become active within 20 s; active=false key_window=false asks=21'
  STUB_EXIT=101
  check macos-inconclusive FAIL "FAIL $name (inconclusive: guard_hosted's app did not become active within 20 s" macos
  output inconclusive-in-a-line "$mac_dialog" "x guard_hosted inconclusive: is no line of the target" "$mac_guard"
  check macos-inconclusive-inside-another-line PASS 'the line of 7 of 7 with none failed 1' macos
  bound_line="the test runtime's run loop gave no sign that it runs within 10 s, so a show would run off the main thread; no case ran"
  output start-at-the-bound "$running" 'dialog_hosted start: brought its own panel to the front once, the app read active after 2 asks, and the panel is ordered out' "dialog_hosted start FAIL: $bound_line" "$mac_guard"
  STUB_EXIT=101
  check macos-a-start-that-ends-at-its-bound FAIL "FAIL $name (start: dialog_hosted's start ended before its first case: $bound_line)" macos
  output guard-start-at-the-bound "$mac_dialog" "guard_hosted start FAIL: $bound_line"
  STUB_EXIT=101
  check macos-a-guard-start-that-ends-at-its-bound FAIL "FAIL $name (start: guard_hosted's start ended before its first case: $bound_line)" macos
  closed_line="the channel of the run loop's sign closed after 0 ms with no sign: the main thread dropped its end without the sign that the test runtime's run loop runs; no case ran"
  output start-closed "$mac_dialog" "guard_hosted start FAIL: $closed_line"
  STUB_EXIT=101
  check macos-a-start-whose-channel-closed FAIL "(start: guard_hosted's start ended before its first case: $closed_line)" macos
  refused_line='HALVO_HOSTED_LATE_RUN_MS is set and is not a whole number of milliseconds from 1 to 60000; no case ran'
  output start-refused "dialog_hosted start FAIL: $refused_line" "guard_hosted start FAIL: $refused_line"
  STUB_EXIT=101
  check macos-both-starts-refused-the-setting FAIL "(start: dialog_hosted's start ended before its first case: $refused_line; guard_hosted's start ended before its first case: $refused_line)" macos
  output dialog-inconclusive 'dialog_hosted inconclusive: the app did not become active within 20 s; active=false key_window=false asks=21' "$mac_guard"
  STUB_EXIT=101
  check macos-the-inconclusive-start-of-dialog_hosted FAIL "(start: dialog_hosted's start ended before its first case: inconclusive: the app did not become active within 20 s; active=false key_window=false asks=21)" macos
  output start-and-inconclusive "dialog_hosted start FAIL: $bound_line" 'guard_hosted inconclusive: the app did not become active within 20 s; active=false key_window=false asks=21'
  STUB_EXIT=101
  check macos-inconclusive-before-a-start-that-ended FAIL "FAIL $name (inconclusive: guard_hosted's app did not become active within 20 s" macos
  output start-in-a-line "$mac_dialog" "x guard_hosted start FAIL: $bound_line" "$mac_guard"
  check macos-a-start-line-inside-another-line PASS 'the line of 7 of 7 with none failed 1' macos
  sign="guard_hosted start: the test runtime's run loop gave its sign after 1 ms, and the first show goes out after it"
  output first-case "$running" "$mac_dialog" "$sign" \
    'guard_hosted reading macos reject_action_at_100ms: the show returned after 1 ms with no report and answered reject' \
    'guard_hosted reading macos reject_action_at_100ms: report made by no report' \
    'guard_hosted unreported macos reject_action_at_100ms +4024 ms: no report after 2 s; the main thread ran no read in 2 s; the show had returned=true' \
    'guard_hosted reading macos reject_action_at_100ms: reports=0 active_at_report=false visible_at_report=false answer=reject' \
    'guard_hosted case macos reject_action_at_100ms FAIL: macos reject_action_at_100ms: no report in 3 s with no press and no help' \
    'guard_hosted reading macos allow_action_at_100ms: reports=1 active_at_report=true visible_at_report=true answer=allow' \
    'guard_hosted case macos allow_action_at_100ms PASS' \
    'guard_hosted end: cases ran 7 of 7, failed 1, schedule slipped 0, helped 0, moments at a press 0, retries 0, reports before visible 0'
  STUB_EXIT=101
  absent='answer=allow'
  check macos-a-failed-case-prints-its-lines-into-the-log FAIL "guard_hosted failed case macos reject_action_at_100ms: its own lines, from line 3 to line 8 of dialog-targets.txt
  $sign
  guard_hosted reading macos reject_action_at_100ms: the show returned after 1 ms with no report and answered reject" macos
  output first-case-again "$running" "$mac_dialog" "$sign" \
    'guard_hosted unreported macos reject_action_at_100ms +4024 ms: no report after 2 s; the main thread ran no read in 2 s; the show had returned=true' \
    'guard_hosted case macos reject_action_at_100ms FAIL: macos reject_action_at_100ms: no report in 3 s with no press and no help' \
    'guard_hosted end: cases ran 7 of 7, failed 1, schedule slipped 0, helped 0, moments at a press 0, retries 0, reports before visible 0'
  STUB_EXIT=101
  check macos-the-last-line-of-a-failed-case-in-the-log FAIL '  guard_hosted case macos reject_action_at_100ms FAIL: macos reject_action_at_100ms: no report in 3 s with no press and no help
FAIL 46-r4-dialog-ran (' macos
  output win-failed-case "$running" 'dialog_hosted case windows press_reject PASS' \
    'dialog_hosted timing windows press_allow: report_to_visible_ms=2400' \
    'dialog_hosted case windows press_allow FAIL: windows press_allow: the answer was reject, not allow' \
    'dialog_hosted end: cases ran 5 of 5, failed 1' "$win_guard"
  STUB_EXIT=101
  absent='dialog_hosted case windows press_reject PASS'
  check windows-a-failed-case-prints-its-lines-into-the-log FAIL "dialog_hosted failed case windows press_allow: its own lines, from line 3 to line 4 of dialog-targets.txt
  dialog_hosted timing windows press_allow: report_to_visible_ms=2400" windows
  output bound-case "$running" 'dialog_hosted case macos press_allow PASS' \
    'dialog_hosted bound reading narrow: lines=13 window=520x615 seen_h=480 controls=2' \
    'dialog_hosted bound case macos narrow FAIL: narrow: the field was cut' \
    'dialog_hosted bounds: cases ran 3 of 3, failed 1' "$mac_dialog" "$mac_guard"
  STUB_EXIT=101
  check macos-a-failed-bound-case-prints-its-lines-into-the-log FAIL "dialog_hosted failed bound case macos narrow: its own lines, from line 3 to line 4 of dialog-targets.txt
  dialog_hosted bound reading narrow: lines=13 window=520x615 seen_h=480 controls=2" macos
  output watchdog "$mac_dialog" "$sign" 'guard_hosted case macos reject_action_at_100ms PASS' \
    'guard_hosted notice macos allow_action_at_100ms +1104 ms: became key active=true key=true modal_window=true object_is_the_alert=true' \
    'guard_hosted watchdog FAIL: the cases did not end in time'
  STUB_EXIT=101
  check macos-a-run-that-its-watchdog-ended-prints-its-last-lines FAIL "guard_hosted ended at its watchdog: its own lines, from line 4 to line 5 of dialog-targets.txt
  guard_hosted notice macos allow_action_at_100ms +1104 ms" macos
  output cut-short "$mac_dialog" "$sign" 'guard_hosted notice macos reject_action_at_100ms +1129 ms: became key active=true key=true modal_window=true object_is_the_alert=true'
  STUB_EXIT=101
  check macos-a-run-cut-short-prints-its-last-lines FAIL "guard_hosted printed no end line after these lines: its own lines, from line 2 to line 3 of dialog-targets.txt" macos
  output dialog-cut-short "$running" 'dialog_hosted timing macos press_allow: report_to_visible_ms=913' "$mac_guard"
  STUB_EXIT=101
  check macos-a-target-cut-short-before-the-next-target FAIL "dialog_hosted printed no end line after these lines: its own lines, from line 2 to line 2 of dialog-targets.txt" macos
  output start-lines "$running" 'dialog_hosted start: brought its own panel to the front once, the app read active after 2 asks, and the panel is ordered out' "dialog_hosted start FAIL: $bound_line" "$mac_guard"
  STUB_EXIT=101
  check macos-the-lines-of-a-start-that-ended-in-the-log FAIL "dialog_hosted start ended before its first case: its own lines, from line 2 to line 3 of dialog-targets.txt
  dialog_hosted start: brought its own panel" macos
  output tail "$mac_dialog, FAIL: not every case ran" "$mac_guard"
  check macos-own-line-with-a-tail FAIL 'not "dialog_hosted end: cases ran 4 of 4, failed 0"' macos
  output helped "$mac_dialog" 'guard_hosted end: cases ran 7 of 7, failed 0, schedule slipped 0, helped 1, moments at a press 0, retries 0'
  check macos-a-helped-show FAIL 'guard_hosted printed "guard_hosted end: cases ran 7 of 7, failed 0, schedule slipped 0, helped 1' macos
  output press-moment "$mac_dialog" 'guard_hosted end: cases ran 7 of 7, failed 0, schedule slipped 0, helped 0, moments at a press 2, retries 0'
  check macos-a-report-taken-at-a-press FAIL 'moments at a press 2, retries 0", not' macos
  output retries "$mac_dialog" 'guard_hosted end: cases ran 7 of 7, failed 0, schedule slipped 0, helped 0, moments at a press 0, retries 13, reports before visible 0'
  check macos-retries-of-any-count PASS 'the line of 7 of 7 with none failed 1' macos
  output no-slipped-count "$mac_dialog" 'guard_hosted end: cases ran 7 of 7, failed 0, helped 0, moments at a press 0, retries 0'
  check macos-a-guard-line-with-no-count-of-slipped-schedules FAIL 'guard_hosted printed "guard_hosted end: cases ran 7 of 7, failed 0, helped 0' macos
  output slipped-twice "$mac_dialog" 'guard_hosted case macos allow_click_at_300ms_and_250ms_after_it schedule slipped in run 1 of 3: the schedule slipped: the press ran 528 ms after the press before it, and arm_ms is 500; the case runs again' 'guard_hosted end: cases ran 7 of 7, failed 0, schedule slipped 2, helped 0, moments at a press 0, retries 0, reports before visible 0'
  absent='its own lines'
  check macos-slipped-twice-then-kept-and-passed PASS 'the line of 7 of 7 with none failed 1' macos
  output too-few-kept "$mac_dialog" 'guard_hosted case macos allow_click_at_300ms_and_250ms_after_it FAIL: too few runs kept the schedule: the schedule slipped in 3 runs of 3; the last: macos allow_click_at_300ms_and_250ms_after_it: the schedule slipped: the press ran 539 ms after the press before it, and arm_ms is 500' 'guard_hosted end: cases ran 7 of 7, failed 1, schedule slipped 3, helped 0, moments at a press 0, retries 0'
  STUB_EXIT=101
  check macos-too-few-runs-kept-the-schedule FAIL 'too few runs kept the schedule: in 1 cases of guard_hosted' macos
  output win-too-few-kept "$win_dialog" 'guard_hosted case windows click_100_at_300ms_and_250ms_after_it FAIL: too few runs kept the schedule: the schedule slipped in 3 runs of 3' 'guard_hosted end: cases ran 8 of 8, failed 1, schedule slipped 3, helped 0, moments at a press 0, retries 0'
  STUB_EXIT=101
  check windows-too-few-runs-kept-the-schedule FAIL 'too few runs kept the schedule: in 1 cases of guard_hosted' windows
  output kept-and-failed "$mac_dialog" 'guard_hosted case macos allow_click_at_300ms_and_250ms_after_it FAIL: macos allow_click_at_300ms_and_250ms_after_it: the answer was reject, not allow' 'guard_hosted end: cases ran 7 of 7, failed 1, schedule slipped 0, helped 0, moments at a press 0, retries 0'
  STUB_EXIT=101
  check macos-a-run-that-kept-the-schedule-and-failed FAIL 'guard_hosted printed "guard_hosted end: cases ran 7 of 7, failed 1, schedule slipped 0' macos
  output loop-mode "$mac_dialog" 'guard_hosted case macos allow_click_at_300ms_and_250ms_after_it SLIPPED: the schedule slipped: the press ran 541 ms after the press before it, and arm_ms is 500; not a pass and not a miss' 'guard_hosted end: cases ran 7 of 7, failed 0, schedule slipped 1, helped 0, moments at a press 0, retries 0'
  check macos-a-line-of-the-loop-mode-in-the-checks-own-run FAIL 'guard_hosted printed 1 lines of the loop mode (a case SLIPPED)' macos
  output win-loop-mode "$win_dialog" 'guard_hosted case windows click_100_at_300ms_and_250ms_after_it SLIPPED: the schedule slipped: the press ran 512 ms after the press before it, and arm_ms is 500; not a pass and not a miss' 'guard_hosted end: cases ran 8 of 8, failed 0, schedule slipped 1, helped 0, moments at a press 0, retries 0'
  check windows-a-line-of-the-loop-mode-in-the-checks-own-run FAIL 'guard_hosted printed 1 lines of the loop mode (a case SLIPPED)' windows
  output win-before-visible "$win_dialog" 'guard_hosted case windows click_100_at_100ms FAIL: windows click_100_at_100ms: the window was not visible at the report' 'guard_hosted end: cases ran 8 of 8, failed 1, schedule slipped 0, helped 0, moments at a press 0, retries 0, reports before visible 1'
  STUB_EXIT=101
  check windows-a-report-before-the-dialog-was-visible FAIL 'reports before visible 1", not' windows
  output win-before-visible-and-passed "$win_dialog" 'guard_hosted end: cases ran 8 of 8, failed 0, schedule slipped 0, helped 0, moments at a press 0, retries 0, reports before visible 2'
  check windows-reports-before-visible-with-no-failed-case FAIL 'reports before visible 2", not' windows
  output no-visible-count "$win_dialog" 'guard_hosted end: cases ran 8 of 8, failed 0, schedule slipped 0, helped 0, moments at a press 0, retries 0'
  check windows-a-guard-line-with-no-count-of-reports-before-visible FAIL 'retries 0", not' windows
  output loop-mode-inside-a-line "$mac_dialog" 'x guard_hosted case macos allow_click_at_300ms_and_250ms_after_it SLIPPED: is no line of the target' "$mac_guard"
  check macos-the-loop-mode-text-inside-another-line PASS 'the line of 7 of 7 with none failed 1' macos
  output no-counts "$mac_dialog" 'guard_hosted end: cases ran 7 of 7, failed 0'
  check macos-a-guard-line-with-no-counts FAIL 'guard_hosted printed "guard_hosted end: cases ran 7 of 7, failed 0", not' macos
  output no-guard "$mac_dialog"
  check macos-no-guard-line FAIL 'guard_hosted printed 0 end lines, not one' macos
  output two "$mac_dialog" "$mac_dialog" "$mac_guard"
  check macos-two-end-lines FAIL 'dialog_hosted printed 2 end lines, not one' macos
  output prefix "x $mac_dialog" "$mac_guard"
  check macos-line-inside-another FAIL 'dialog_hosted printed 0 end lines, not one' macos
  output cargo "$mac_dialog" "$mac_guard"
  STUB_EXIT=101
  check macos-cargo-failed FAIL 'exited 101' macos
  STUB_WRITES=yes
  check macos-a-test-writes-the-checkout FAIL 'a tracked file differs from HEAD after cargo ran' macos
  git -C "$work/repo" checkout -q -- tracked.txt || exit 2
  printf 'changed before the check\n' >>"$work/repo/tracked.txt" || exit 2
  check macos-on-a-changed-checkout FAIL 'a tracked file differs from HEAD before cargo ran' macos
  git -C "$work/repo" checkout -q -- tracked.txt || exit 2
  check macos-after-the-checkout-is-clean-again PASS 'PASS' macos
  check not-in-actions SKIP 'on a hosted runner alone' macos GITHUB_ACTIONS=
  check self-hosted-runner SKIP 'on a hosted runner alone' windows RUNNER_ENVIRONMENT=self-hosted
  check another-job FAIL 'the job is linux, not macos or windows' linux
  check an-argument FAIL "FAIL $name (usage)" --bogus

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
  exit 0
}

main() {
  self=$0
  case $# in
    0) measure ;;
    1)
      case $1 in
        --self-test) self_test ;;
        *) fail usage ;;
      esac
      ;;
    *) fail usage ;;
  esac
}

main "$@"
