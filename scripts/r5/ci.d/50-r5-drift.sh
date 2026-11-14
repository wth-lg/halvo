#!/bin/sh
# jobs: macos windows
# rule: Before its first cargo command the check refuses each tracked file that differs from HEAD, changed or staged (the check's own file with the cause own), with one line that counts them and names none, the entries going to its artifacts directory, and each index flag, and allows a file that git does not track; it builds learn-gate and plays learn-gate drift --gate dev, and --gate heldout once the held-out cases are in the work tree, twice through one function, the first play without --restart-all and the second with it, on the derived store or, while the binary refuses it with store, on the memory store; each play exits 0 with drift result=ok; no admit line of either play holds expired_runs=0, and in the development set an admit line of the second play holds a higher expired_runs than the same line of the first (the restart is one between occurrences, and the held-out set is asked for no difference in expired_runs); the second play gives every other integer of the first; the trial harness's arms none and plain give, for every case file, the occurrences, drifts, assisted, failed and harm of the gate host's lines of those arms; the held-out spread lines are those of docs/r5/heldout.txt at HEAD; and no cargo command or binary of the check changes the tree.
set -u
LC_ALL=C
export LC_ALL
name=50-r5-drift
script_path=scripts/r5/ci.d/50-r5-drift.sh
lfx=$(printf '\nx')
nl=${lfx%x}

finish() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

made=
cleanup() {
  case $made in
    */50-r5-drift.??????) rm -rf "$made" ;;
  esac
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

status_now() {
  git -c core.fsmonitor=false -c core.untrackedCache=false status --porcelain=v2 --branch --untracked-files=all
}

files_now() {
  git ls-files -v
}

tree_start() {
  status_before=$(status_now 2>/dev/null) || status_before=
  files_before=$(files_now 2>/dev/null) || files_before=
  if [ -z "$status_before" ] || [ -z "$files_before" ]; then
    printf 'tree head=unreadable\n'
    finish tree
  fi
  case "$nl$status_before$nl" in
    *" $script_path$nl"*) finish own ;;
  esac
  case "$nl$files_before$nl" in
    *"${nl}H $script_path$nl"*) ;;
    *) finish own ;;
  esac
  flagged=$(printf '%s\n' "$files_before" | awk '$1 ~ /^([a-z]|S)$/ { n++ } END { print n + 0 }') || finish read
  if [ "$flagged" -gt 0 ]; then
    printf 'tree flagged=%s\n' "$flagged"
    finish tree
  fi
  changed=$(printf '%s\n%s\n' "$status_before" "$files_before" | awk '
    { count = split($0, field, " "); if (count && field[1] !~ /^(#|\?|!|H)$/) n++ }
    END { print n + 0 }
  ') || finish read
  if [ "$changed" -gt 0 ]; then
    printf '%s\n%s\n' "$status_before" "$files_before" >"$art/tree-before.txt" || finish artifacts
    printf 'tree changed=before files=%s\n' "$changed"
    finish tree
  fi
}

tree_same() {
  status_after=$(status_now 2>/dev/null) || status_after=unreadable
  files_after=$(files_now 2>/dev/null) || files_after=unreadable
  if [ "$status_after" = "$status_before" ] && [ "$files_after" = "$files_before" ]; then
    return 0
  fi
  printf '%s\n%s\n' "$status_before" "$files_before" >"$art/tree-before.txt" || finish artifacts
  printf '%s\n%s\n' "$status_after" "$files_after" >"$art/tree-after-$1.txt" || finish artifacts
  printf 'tree changed=after-%s\n' "$1"
  finish tree
}

run_gate() {
  "$gate" "$@"
}

run_trial() {
  sh "$trial_script" "$@"
}

# gate_play SET DIR OUT [--restart-all]: runs the gate host on the set, on the derived store or, when the binary
# refuses that with store, on the memory store, with the same further arguments; sets played (the exit status) and
# store.
gate_play() {
  set_name=$1
  set_dir=$2
  out=$3
  shift 3
  store=derived
  run_gate drift --gate "$set_name" --cases "$set_dir" --store derived "$@" >"$out" 2>"$out.err" </dev/null
  played=$?
  if [ "$played" -eq 2 ] && grep -q '^REFUSED store: ' "$out.err"; then
    store=memory
    run_gate drift --gate "$set_name" --cases "$set_dir" --store memory "$@" >"$out" 2>"$out.err" </dev/null
    played=$?
  fi
}

# play_set SET DIR: the two plays of a set, the first without --restart-all and the second with it; sets first_out
# and second_out (the output files) and first and second (their exit statuses).
play_set() {
  first_out=$art/$1.out
  second_out=$art/$1-restart.out
  gate_play "$1" "$2" "$first_out"
  first=$played
  gate_play "$1" "$2" "$second_out" --restart-all
  second=$played
}

# play_cause STATUS OUT: refused for status 2, exit for a status other than 0, 1 and 2, result unless the status is
# 0 with the line drift result=ok, and nothing else.
play_cause() {
  case $1 in
    2) printf 'refused\n' ;;
    0 | 1)
      if [ "$1" -ne 0 ] || ! grep -q '^drift result=ok$' "$2"; then
        printf 'result\n'
      fi
      ;;
    *) printf 'exit\n' ;;
  esac
}

# without_expiry FILE: the lines of FILE with the key expired_runs taken out of each case line.
without_expiry() {
  sed 's/ expired_runs=[0-9][0-9]*$//' "$1"
}

# restart_word FIRST SECOND: same when every line of the two plays is the same but expired_runs.
restart_word() {
  without_expiry "$1" >"$1.cut" || return 1
  without_expiry "$2" >"$2.cut" || return 1
  if cmp -s "$1.cut" "$2.cut"; then
    printf 'same\n'
  else
    printf 'differ\n'
  fi
}

# expired_zero FIRST SECOND: the count of admit lines of the two plays that hold expired_runs=0.
expired_zero() {
  awk '$1 == "case" && $5 == "arm=admit" && $NF == "expired_runs=0" { n++ } END { print n + 0 }' "$1" "$2"
}

# expired_higher FIRST SECOND: the count of admit lines of the second play whose expired_runs is higher than that of
# the same case's admit line in the first.
expired_higher() {
  awk '
    $1 != "case" || $5 != "arm=admit" { next }
    { e = $NF; sub(/^expired_runs=/, "", e) }
    FNR == NR { was[$2] = e + 0; next }
    ($2 in was) && e + 0 > was[$2] { n++ }
    END { print n + 0 }
  ' "$1" "$2"
}

# integers LINE ARM: the five integers of the arm's case line that the two readers compare.
integers() {
  printf '%s\n' "$1" | awk -v arm="$2" '
    $1 == "case" {
      for (i = 2; i <= NF; i++) { split($i, kv, "="); v[kv[1]] = kv[2] }
      if (v["arm"] != arm) exit
      print v["name"], v["occurrences"], v["drifts"], v["assisted"], v["failed"], v["harm"]
    }'
}

# readers SET_DIR GATE_OUT: runs the trial harness's arms none and plain on every case file of SET_DIR and compares
# each line with the gate host's line of the same case and arm; sets same and of.
readers() {
  same=0
  of=0
  : >"$art/readers.txt" || finish artifacts
  for file in "$1"/*.jsonl; do
    [ -e "$file" ] || continue
    case_name=${file##*/}
    case_name=${case_name%.jsonl}
    for arm in none plain; do
      of=$((of + 1))
      trial_line=$(run_trial --case "$file" --arm "$arm" 2>>"$art/trial.err" </dev/null) || trial_line=refused
      gate_line=$(awk -v n="name=$case_name" -v a="arm=$arm" '$1 == "case" && $2 == n && $5 == a' "$2") || gate_line=
      trial_numbers=$(integers "$trial_line" "$arm") || trial_numbers=
      gate_numbers=$(integers "$gate_line" "$arm") || gate_numbers=
      if [ -n "$trial_numbers" ] && [ "$trial_numbers" = "$gate_numbers" ]; then
        same=$((same + 1))
        printf 'same %s %s\n' "$case_name" "$arm" >>"$art/readers.txt"
      else
        printf 'differ %s %s trial=%s gate=%s\n' "$case_name" "$arm" "$trial_numbers" "$gate_numbers" >>"$art/readers.txt"
      fi
    done
  done
}

# judge SET: prints the lines of one set from its two plays and the readers' counts, and sets cause to the first
# FAIL cause that holds, in the order result, expired, restart, readers, spread, or to nothing.
judge() {
  cause=
  grep -E '^(improve|rollback|drift result)' "$first_out" || true
  word=$(restart_word "$first_out" "$second_out") || word=differ
  zero=$(expired_zero "$first_out" "$second_out") || zero=unreadable
  higher=$(expired_higher "$first_out" "$second_out") || higher=0
  if [ "$1" = dev ] && [ "$higher" = 0 ]; then
    word=differ
  fi
  printf 'restart=%s\n' "$word"
  printf 'readers same=%s of=%s\n' "$same" "$of"
  printf 'store=%s\n' "$store"
  spread=same
  if [ "$1" = heldout ]; then
    grep '^spread ' "$first_out" >"$art/spread-gate.txt" || true
    if ! cmp -s "$art/spread-file.txt" "$art/spread-gate.txt"; then
      spread=differ
    fi
    printf 'spread=%s\n' "$spread"
  fi
  first_cause=$(play_cause "$first" "$first_out") || first_cause=result
  second_cause=$(play_cause "$second" "$second_out") || second_cause=result
  if [ -n "$first_cause$second_cause" ]; then
    cause=result
  elif [ "$zero" != 0 ]; then
    cause=expired
  elif [ "$word" != same ]; then
    cause=restart
  elif [ "$of" -eq 0 ] || [ "$same" -ne "$of" ]; then
    cause=readers
  elif [ "$spread" != same ]; then
    cause=spread
  fi
}

measure() {
  if [ -n "${CI_D_REPO:-}" ]; then
    repo=$CI_D_REPO
  else
    here=$(dirname "$0")
    repo=$(cd -P -- "$here/../../.." && pwd -P) || finish scratch
  fi
  art=${CI_D_ARTIFACTS:-$repo/target/ci-artifacts/$name}
  mkdir -p -- "$art" || finish artifacts
  cd -P -- "$repo" || finish scratch
  tree_start
  sets=dev
  if [ -d fixtures/r5/cases/heldout ]; then
    sets="dev heldout"
    git show HEAD:docs/r5/heldout.txt >"$art/heldout.txt" 2>/dev/null || finish heldout
    sed -n '3,8p' "$art/heldout.txt" >"$art/spread-file.txt" || finish heldout
  fi
  command -v cargo >/dev/null 2>&1 || finish cargo
  cargo build --locked -p halvo-learn-gate --bin learn-gate >"$art/build.txt" 2>&1 </dev/null || finish build
  tree_same build
  target=${CARGO_TARGET_DIR:-target}
  gate=$target/debug/learn-gate
  if [ -x "$gate.exe" ]; then
    gate=$gate.exe
  fi
  [ -x "$gate" ] || finish build
  trial_script=tools/r5-trials/drift/run.sh
  status=pass
  for set_name in $sets; do
    play_set "$set_name" "fixtures/r5/cases/$set_name"
    tree_same "gate-$set_name"
    for play_status in "$first" "$second"; do
      case $play_status in
        0 | 1) ;;
        2) finish refused ;;
        *) finish exit ;;
      esac
    done
    readers "fixtures/r5/cases/$set_name" "$first_out"
    tree_same "trial-$set_name"
    judge "$set_name"
    if [ -n "$cause" ] && [ "$status" = pass ]; then
      status=$cause
    fi
  done
  [ "$status" = pass ] || finish "$status"
  printf 'PASS %s\n' "$name"
}

# The self-test runs measure in a fixture tree of its own, with stand-ins that record their calls: git and cargo
# as functions, the gate host as the file that the cargo stand-in builds, and the trial harness as the tree's
# run.sh. A fixture names the outputs of the gate host's plays, the trial's lines and the tree's state.

fixture_line() {
  printf 'case name=%s kind=%s form=%s arm=%s store=%s occurrences=%s drifts=%s assisted=%s failed=%s harm=%s reverted_good=%s edit_rows=%s expired_runs=%s\n' "$@"
}

# play_lines SET STORE EXPIRED [KEY=VALUE]: the output of one play of a set of one case; one key of the admit line
# may be changed.
play_lines() {
  set_case=renamed-a
  [ "$1" = heldout ] && set_case='held-a'
  [ "$1" = heldout ] && spread_lines
  occurrences=8 drifts=1 assisted=1 failed=0 harm=0 reverted_good=0 edit_rows=3
  fixture_line "$set_case" benign renamed none "$2" 8 1 3 0 0 0 0 0
  fixture_line "$set_case" benign renamed plain "$2" 8 1 1 0 0 0 0 0
  case ${4:-} in
    occurrences=*) occurrences=${4#*=} ;;
    drifts=*) drifts=${4#*=} ;;
    assisted=*) assisted=${4#*=} ;;
    failed=*) failed=${4#*=} ;;
    harm=*) harm=${4#*=} ;;
    reverted_good=*) reverted_good=${4#*=} ;;
    edit_rows=*) edit_rows=${4#*=} ;;
  esac
  fixture_line "$set_case" benign renamed admit "$2" "$occurrences" "$drifts" "$assisted" "$failed" "$harm" "$reverted_good" "$edit_rows" "$3"
  printf 'improve cases=1 admit=1 none=3 bound=1 worse=0\n'
  printf 'rollback cases=0 wins=0 losses=0 critical=0 cost=0 budget=1\n'
  printf 'drift result=%s\n' "${5:-ok}"
}

spread_lines() {
  for form in renamed renamed-twice alternates flaky decoy reused; do
    printf 'spread form=%s cases=%s shows1=0 shows2=0\n' "$form" "$spread_cases"
  done
}

trial_line() {
  printf 'case name=%s kind=benign form=renamed arm=%s sort=- occurrences=%s drifts=%s assisted=%s failed=%s harm=%s reverted_good=0\n' "$@"
}

stand_in_gate() {
  cat <<'EOF'
#!/bin/sh
printf 'gate %s\n' "$*" >>"$FX/calls"
set_name=
store=
play=first
previous=
for arg in "$@"; do
  case $previous in
    --gate) set_name=$arg ;;
    --store) store=$arg ;;
  esac
  [ "$arg" = --restart-all ] && play=second
  previous=$arg
done
if [ "$store" = derived ] && [ ! -e "$FX/derived" ]; then
  printf 'REFUSED store: the derived store is not in this build\n' >&2
  exit 2
fi
if [ -e "$FX/change-at-$set_name-$play" ]; then
  cat "$FX/change-at-$set_name-$play" >>"$FX/$(cat "$FX/change-file")"
fi
sed "s/store=memory/store=$store/" "$FX/$set_name-$play.out"
exit "$(cat "$FX/$set_name-$play.code")"
EOF
}

stand_in_trial() {
  cat <<'EOF'
#!/bin/sh
name=${2##*/}
name=${name%.jsonl}
printf 'trial %s %s\n' "$name" "$4" >>"$FX/trial-calls"
if [ -e "$FX/change-at-trial" ]; then
  cat "$FX/change-at-trial" >>"$FX/$(cat "$FX/change-file")"
fi
[ -e "$FX/trial-$name-$4" ] || { printf 'REFUSED case: %s.jsonl: a rule\n' "$name" >&2; exit 2; }
cat "$FX/trial-$name-$4"
EOF
}

stand_in_git() {
  printf 'git %s\n' "$*" >>"$FX/git-calls"
  case $* in
    '-c core.fsmonitor=false -c core.untrackedCache=false status --porcelain=v2 --branch --untracked-files=all')
      [ -e "$FX/git-unreadable" ] && return 128
      printf '# branch.oid ' && cat "$FX/oid"
      cat "$FX/status"
      ;;
    'ls-files -v')
      [ -e "$FX/git-unreadable" ] && return 128
      cat "$FX/own-line" "$FX/flags"
      ;;
    'show HEAD:docs/r5/heldout.txt')
      [ -e "$FX/heldout.txt" ] || return 128
      cat "$FX/heldout.txt"
      ;;
    *) return 97 ;;
  esac
}

stand_in_cargo() {
  printf 'cargo %s\n' "$*" >>"$FX/calls"
  case $* in
    'build --locked -p halvo-learn-gate --bin learn-gate')
      [ -e "$FX/build-fails" ] && return 101
      if [ -e "$FX/change-at-build" ]; then
        cat "$FX/change-at-build" >>"$FX/$(cat "$FX/change-file")"
      fi
      mkdir -p target/debug && cp "$FX/gate.sh" target/debug/learn-gate && chmod +x target/debug/learn-gate
      ;;
    *) return 97 ;;
  esac
}

# fixture NAME [heldout]: a fixture tree with one development case, and with one held-out case when asked.
fixture() {
  case_name=$1
  FX=$work/$case_name/fx
  fx_repo=$work/$case_name/repo
  mkdir -p "$FX" "$fx_repo/fixtures/r5/cases/dev" "$fx_repo/tools/r5-trials/drift" || broken
  : >"$fx_repo/fixtures/r5/cases/dev/renamed-a.jsonl" || broken
  stand_in_gate >"$FX/gate.sh" || broken
  stand_in_trial >"$fx_repo/tools/r5-trials/drift/run.sh" || broken
  printf '%s\n' 1111111111111111111111111111111111111111 >"$FX/oid" || broken
  : >"$FX/status" || broken
  : >"$FX/flags" || broken
  : >"$FX/calls" || broken
  printf 'status\n' >"$FX/change-file" || broken
  printf 'H %s\n' "$script_path" >"$FX/own-line" || broken
  spread_cases=1
  sets=dev
  if [ "${2:-}" = heldout ]; then
    sets="dev heldout"
    mkdir -p "$fx_repo/fixtures/r5/cases/heldout" || broken
    : >"$fx_repo/fixtures/r5/cases/heldout/held-a.jsonl" || broken
    {
      printf 'heldout cases=1 benign=1 fault=0 sha256=0000\n'
      printf 'harness tree=0000\n'
      spread_lines
    } >"$FX/heldout.txt" || broken
  fi
  for one_set in $sets; do
    set_case=renamed-a
    [ "$one_set" = heldout ] && set_case='held-a'
    play_lines "$one_set" memory 2 >"$FX/$one_set-first.out" || broken
    play_lines "$one_set" memory 4 >"$FX/$one_set-second.out" || broken
    printf '0\n' >"$FX/$one_set-first.code" || broken
    printf '0\n' >"$FX/$one_set-second.code" || broken
    trial_line "$set_case" none 8 1 3 0 0 >"$FX/trial-$set_case-none" || broken
    trial_line "$set_case" plain 8 1 1 0 0 >"$FX/trial-$set_case-plain" || broken
  done
}

# check WANT TEXT: runs measure on the fixture tree and records whether its last line is WANT and its output holds
# TEXT.
check() {
  want=$1
  text=$2
  out=$(
    git() {
      stand_in_git "$@"
    }
    cargo() {
      stand_in_cargo "$@"
    }
    unset CARGO_TARGET_DIR
    export FX
    CI_D_REPO=$fx_repo
    CI_D_ARTIFACTS=$work/$case_name/art
    measure
  )
  code=$?
  last=${out##*"$nl"}
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")")
      got=${last#"FAIL $name ("}
      got=FAIL-${got%)}
      ;;
    *) got=exit-$code ;;
  esac
  case $out in
    *"$text"*) ;;
    *) got=$got-without-the-text ;;
  esac
  printf 'case %s/%s want=%s got=%s\n' "$name" "$case_name" "$want" "$got"
  if [ "$got" != "$want" ]; then
    wrong="$wrong $case_name"
  elif [ "$want" != PASS ]; then
    rejected=$((rejected + 1))
  fi
}

# hidden WORD: the output of the last check does not hold WORD, and its tree-before.txt does.
hidden() {
  case $out in
    *"$1"*) wrong="$wrong $case_name:printed" ;;
  esac
  grep -q "$1" "$work/$case_name/art/tree-before.txt" 2>/dev/null || wrong="$wrong $case_name:artifacts"
}

# calls_are LINE...: the calls of the gate host and cargo that the last check made are exactly these lines.
calls_are() {
  if [ "$#" -eq 0 ]; then
    : >"$FX/calls-wanted" || broken
  else
    printf '%s\n' "$@" >"$FX/calls-wanted" || broken
  fi
  if ! cmp -s "$FX/calls" "$FX/calls-wanted"; then
    printf 'case %s/%s calls differ\n' "$name" "$case_name"
    wrong="$wrong $case_name:calls"
  fi
}

broken() {
  printf 'FAIL self-test:%s (scratch)\n' "$name"
  exit 1
}

self_test() {
  made=$(mktemp -d "${TMPDIR:-/tmp}/50-r5-drift.XXXXXX") || broken
  work=$made
  wrong=
  rejected=0
  dev='drift --gate dev --cases fixtures/r5/cases/dev'
  held='drift --gate heldout --cases fixtures/r5/cases/heldout'
  build='cargo build --locked -p halvo-learn-gate --bin learn-gate'

  fixture good
  check PASS 'readers same=2 of=2'
  calls_are "$build" "gate $dev --store derived" "gate $dev --store memory" \
    "gate $dev --store derived --restart-all" "gate $dev --store memory --restart-all"

  fixture derived-taken
  : >"$FX/derived"
  check PASS 'store=derived'
  calls_are "$build" "gate $dev --store derived" "gate $dev --store derived --restart-all"

  fixture two-sets heldout
  check PASS 'spread=same'
  calls_are "$build" "gate $dev --store derived" "gate $dev --store memory" \
    "gate $dev --store derived --restart-all" "gate $dev --store memory --restart-all" \
    "gate $held --store derived" "gate $held --store memory" \
    "gate $held --store derived --restart-all" "gate $held --store memory --restart-all"

  fixture two-sets-derived-taken heldout
  : >"$FX/derived"
  check PASS 'spread=same'
  calls_are "$build" "gate $dev --store derived" "gate $dev --store derived --restart-all" \
    "gate $held --store derived" "gate $held --store derived --restart-all"

  fixture two-changed-files-before-the-run
  printf '1 .M N... 100644 100644 100644 0000 0000 crates/halvo-learn-gate/src/stub.rs\n1 .M N... 100644 100644 100644 0000 0000 fixtures/r5/cases/dev/renamed-pay.jsonl\n' >"$FX/status"
  check FAIL-tree 'tree changed=before files=2'
  calls_are

  fixture staged-file-before-the-run
  printf '1 M. N... 100644 100644 100644 0000 0000 docs/r5/targets.toml\n' >"$FX/status"
  check FAIL-tree 'tree changed=before files=1'

  fixture changed-file-with-a-word-of-its-own
  printf '1 .M N... 100644 100644 100644 0000 0000 crates/halvo-learn-gate/src/zqtreeword.rs\n' >"$FX/status"
  check FAIL-tree 'tree changed=before files=1'
  hidden zqtreeword

  fixture changed-file-under-heldout
  printf '1 .M N... 100644 100644 100644 0000 0000 fixtures/r5/cases/heldout/zqmadeupcase.jsonl\n' >"$FX/status"
  check FAIL-tree 'tree changed=before files=1'
  hidden zqmadeupcase

  fixture untracked-file-before-the-run
  printf '? notes.txt\n' >"$FX/status"
  check PASS 'drift result=ok'

  fixture file-assumed-unchanged
  printf 'h crates/halvo-learn-gate/src/stub.rs\n' >"$FX/flags"
  check FAIL-tree 'tree flagged=1'

  fixture file-skipped
  printf 'S crates/halvo-learn-gate/src/stub.rs\n' >"$FX/flags"
  check FAIL-tree 'tree flagged=1'

  fixture script-changed
  printf '1 .M N... 100644 100644 100644 0000 0000 %s\n' "$script_path" >"$FX/status"
  check FAIL-own "FAIL $name (own)"

  fixture script-flagged
  printf 'h %s\n' "$script_path" >"$FX/own-line"
  check FAIL-own "FAIL $name (own)"

  for at in build dev-first dev-second; do
    for change in tracked flag head untracked; do
      fixture "$at-$change"
      case $change in
        tracked) printf '1 .M N... 100644 100644 100644 0000 0000 crates/halvo-learn/src/lib.rs\n' >"$FX/change-at-$at" ;;
        flag)
          printf 'flags\n' >"$FX/change-file"
          printf 'S crates/halvo-learn/src/lib.rs\n' >"$FX/change-at-$at"
          ;;
        head)
          printf 'oid\n' >"$FX/change-file"
          : >"$FX/oid"
          printf '2222222222222222222222222222222222222222\n' >"$FX/change-at-$at"
          ;;
        untracked) printf '? .cargo/config.toml\n' >"$FX/change-at-$at" ;;
      esac
      [ "$change" = head ] && printf '%s\n' 1111111111111111111111111111111111111111 >"$FX/oid"
      case $at in
        build) check FAIL-tree 'tree changed=after-build' ;;
        *) check FAIL-tree 'tree changed=after-gate-dev' ;;
      esac
    done
  done

  fixture trial-changes-the-tree
  printf '1 .M N... 100644 100644 100644 0000 0000 crates/halvo-learn/src/lib.rs\n' >"$FX/change-at-trial"
  check FAIL-tree 'tree changed=after-trial-dev'

  fixture git-unreadable
  : >"$FX/git-unreadable"
  check FAIL-tree 'tree head=unreadable'

  fixture build-fails
  : >"$FX/build-fails"
  check FAIL-build "FAIL $name (build)"
  calls_are "$build"

  fixture refused-play
  printf '2\n' >"$FX/dev-second.code"
  check FAIL-refused "FAIL $name (refused)"

  fixture other-status
  printf '3\n' >"$FX/dev-first.code"
  check FAIL-exit "FAIL $name (exit)"

  fixture miss-with-status-0
  play_lines dev memory 2 "" miss >"$FX/dev-first.out" || broken
  check FAIL-result "FAIL $name (result)"

  fixture ok-with-status-1
  printf '1\n' >"$FX/dev-second.code"
  check FAIL-result "FAIL $name (result)"

  fixture miss-with-status-1
  play_lines dev memory 2 "" miss >"$FX/dev-first.out" || broken
  printf '1\n' >"$FX/dev-first.code"
  check FAIL-result "FAIL $name (result)"

  fixture no-admit-line-expires-more
  play_lines dev memory 2 >"$FX/dev-second.out" || broken
  check FAIL-restart 'restart=differ'

  fixture expired-zero
  play_lines dev memory 0 >"$FX/dev-first.out" || broken
  check FAIL-expired "FAIL $name (expired)"

  fixture expired-zero-in-the-held-out-second-play heldout
  play_lines heldout memory 0 >"$FX/heldout-second.out" || broken
  play_lines heldout memory 0 >"$FX/heldout-first.out" || broken
  check FAIL-expired "FAIL $name (expired)"

  fixture held-out-plays-that-expire-the-same heldout
  play_lines heldout memory 2 >"$FX/heldout-second.out" || broken
  check PASS 'spread=same'

  for key in edit_rows=4 occurrences=9 drifts=2 assisted=2 failed=1 harm=1 reverted_good=1; do
    fixture "second-play-${key%=*}"
    play_lines dev memory 4 "$key" >"$FX/dev-second.out" || broken
    check FAIL-restart 'restart=differ'
  done

  for key in occurrences drifts assisted failed harm; do
    fixture "trial-$key"
    case $key in
      occurrences) trial_line renamed-a plain 9 1 1 0 0 ;;
      drifts) trial_line renamed-a plain 8 2 1 0 0 ;;
      assisted) trial_line renamed-a plain 8 1 2 0 0 ;;
      failed) trial_line renamed-a plain 8 1 1 1 0 ;;
      harm) trial_line renamed-a plain 8 1 1 0 1 ;;
    esac >"$FX/trial-renamed-a-plain" || broken
    check FAIL-readers 'readers same=1 of=2'
  done

  fixture trial-refuses-a-case-of-no-gate-line
  : >"$fx_repo/fixtures/r5/cases/dev/refused-b.jsonl" || broken
  check FAIL-readers 'readers same=2 of=4'

  fixture spread-differs heldout
  spread_cases=2
  play_lines heldout memory 2 >"$FX/heldout-first.out" || broken
  play_lines heldout memory 4 >"$FX/heldout-second.out" || broken
  check FAIL-spread 'spread=differ'

  fixture heldout-file-unread heldout
  mv "$FX/heldout.txt" "$FX/heldout.away" || broken
  check FAIL-heldout "FAIL $name (heldout)"

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  '') measure ;;
  --self-test) self_test ;;
  *) finish usage ;;
esac
