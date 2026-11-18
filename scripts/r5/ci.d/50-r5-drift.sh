#!/bin/sh
# jobs: macos windows
# rule: Before its first cargo command the check refuses each tracked file that differs from HEAD, changed or staged (the check's own file with the cause own), with one line that counts them and names none, the entries going to its artifacts directory, and each index flag, and allows a file that git does not track; it removes the built learn-gate at its two names under the build directory, builds it with cargo build --locked -p halvo-learn-gate --bin learn-gate, first and apart, takes the one of the two names that then exists (none or both is the cause build) and runs that file directly; it plays learn-gate drift --gate dev, and --gate heldout once the held-out cases are in the work tree, printing one line set=dev or set=heldout before the lines of that set, so that every line up to the next set= line belongs to that set, twice through one function, the first play without --restart-all and the second with it, on the derived store or, only when the derived call ends with status 2, nothing on standard output and exactly the one line REFUSED store: the derived store is not in this build on standard error, byte for byte with its final LF, on the memory store, every other status 2 of the derived call failing the check with the cause refused and no memory play following; every case line of a play names the store of its call and every play of the run is on the store of the first, else the check fails with the cause store, tested after stderr and before result; each play exits 0 with drift result=ok, and a play that ended with status 0 or 1 and wrote on standard error fails the check with the cause stderr; no admit line of either play holds expired_runs=0, and in the development set an admit line of the second play holds a higher expired_runs than the same line of the first (the restart is one between occurrences, and the held-out set is asked for no difference in expired_runs); the second play gives every other integer of the first; the trial harness's arms none and plain give, for every case file, the occurrences, drifts, assisted, failed and harm of the gate host's lines of those arms; the held-out spread lines are those of docs/r5/heldout.txt at HEAD; and no cargo command or binary of the check changes the tree.
set -u
LC_ALL=C
export LC_ALL
name=50-r5-drift
script_path=scripts/r5/ci.d/50-r5-drift.sh
not_built='REFUSED store: the derived store is not in this build'
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

# build_gate: removes the built learn-gate at both names under the build directory, builds it, checks the tree, and
# sets gate to the one of the two names that the build directory then lists; ends the check with build when the build
# fails or when it lists none or both.
build_gate() {
  built=${CARGO_TARGET_DIR:-target}/debug/learn-gate
  rm -f -- "$built" "$built.exe" || finish build
  cargo build --locked -p halvo-learn-gate --bin learn-gate >"$art/build.txt" 2>&1 </dev/null || finish build
  tree_same build
  listed=$(ls -1a -- "${built%/*}") || finish build
  plain=no
  exe=no
  case "$nl$listed$nl" in
    *"${nl}learn-gate$nl"*) plain=yes ;;
  esac
  case "$nl$listed$nl" in
    *"${nl}learn-gate.exe$nl"*) exe=yes ;;
  esac
  case $plain-$exe in
    yes-no) gate=$built ;;
    no-yes) gate=$built.exe ;;
    *) finish build ;;
  esac
}

run_trial() {
  sh "$trial_script" "$@"
}

# not_built_refusal OUT: the derived call wrote nothing to OUT, and OUT.err holds byte for byte the build's own
# refusal line and its final LF, the file not_built_file.
not_built_refusal() {
  [ ! -s "$1" ] && cmp -s "$not_built_file" "$1.err"
}

# gate_play SET DIR OUT [--restart-all]: runs the gate host on the set on the derived store and, only when the call
# gives the build's own refusal (status 2, nothing on standard output and the one line not_built on standard error),
# on the memory store, with the same further arguments; ends the check with refused at any other status 2 of the
# derived call; sets played (the exit status) and store; sets store_fault to yes when a case line of the play names
# another store than its call or when the play is on another store than the first play of the run (run_store).
gate_play() {
  set_name=$1
  set_dir=$2
  out=$3
  shift 3
  store=derived
  run_gate drift --gate "$set_name" --cases "$set_dir" --store derived "$@" >"$out" 2>"$out.err" </dev/null
  played=$?
  if [ "$played" -eq 2 ]; then
    not_built_refusal "$out" || finish refused
    store=memory
    run_gate drift --gate "$set_name" --cases "$set_dir" --store memory "$@" >"$out" 2>"$out.err" </dev/null
    played=$?
  fi
  lines_name_store "$out" "$store" || store_fault=yes
  if [ -z "$run_store" ]; then
    run_store=$store
  elif [ "$run_store" != "$store" ]; then
    store_fault=yes
  fi
}

# lines_name_store OUT STORE: every case line of OUT names the store STORE.
lines_name_store() {
  awk -v want="store=$2" '$1 == "case" && $6 != want { bad = 1 } END { exit bad }' "$1"
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
# FAIL cause that holds, in the order stderr, store, result, expired, restart, readers, spread, or to nothing.
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
  if wrote_at_0_or_1 "$first" "$first_out.err" || wrote_at_0_or_1 "$second" "$second_out.err"; then
    cause=stderr
  elif [ "$store_fault" = yes ]; then
    cause=store
  elif [ -n "$first_cause$second_cause" ]; then
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

# wrote_at_0_or_1 STATUS ERR: a play that ended with status 0 or 1 wrote to its standard error, the file ERR.
wrote_at_0_or_1() {
  case $1 in
    0 | 1) [ -s "$2" ] ;;
    *) return 1 ;;
  esac
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
  not_built_file=$art/not-built.expected
  printf '%s\n' "$not_built" >"$not_built_file" || finish artifacts
  build_gate
  trial_script=tools/r5-trials/drift/run.sh
  status=pass
  run_store=
  store_fault=no
  for set_name in $sets; do
    printf 'set=%s\n' "$set_name"
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
if [ "$store" = derived ] && [ ! -e "$FX/derived" ] && [ ! -e "$FX/derived-$set_name-$play" ]; then
  if [ -e "$FX/derived-$play.code" ]; then
    cat "$FX/derived-$play.out"
    cat "$FX/derived-$play.err" >&2
    exit "$(cat "$FX/derived-$play.code")"
  fi
  printf 'REFUSED store: the derived store is not in this build\n' >&2
  exit 2
fi
if [ -e "$FX/change-at-$set_name-$play" ]; then
  cat "$FX/change-at-$set_name-$play" >>"$FX/$(cat "$FX/change-file")"
fi
word=$store
if [ -e "$FX/lines-store" ]; then
  word=$(cat "$FX/lines-store")
fi
sed "s/store=memory/store=$word/" "$FX/$set_name-$play.out"
if [ -e "$FX/$set_name-$play.err" ]; then
  cat "$FX/$set_name-$play.err" >&2
fi
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
  [ ! -e "$FX/cargo-warns" ] || printf 'warning: unused variable: x\n' >&2
  case $* in
    'build --locked -p halvo-learn-gate --bin learn-gate')
      printf 'cargo %s\n' "$*" >>"$FX/calls"
      [ -e "$FX/build-fails" ] && return 101
      if [ -e "$FX/change-at-build" ]; then
        cat "$FX/change-at-build" >>"$FX/$(cat "$FX/change-file")"
      fi
      mkdir -p "${CARGO_TARGET_DIR:-target}/debug" || return 101
      while read -r made; do
        cp "$FX/gate.sh" "${CARGO_TARGET_DIR:-target}/debug/$made" && chmod +x "${CARGO_TARGET_DIR:-target}/debug/$made" || return 101
      done <"$FX/built-names"
      ;;
    'run --locked -q -p halvo-learn-gate --bin learn-gate -- '*)
      shift 8
      sh "$FX/gate.sh" "$@"
      ;;
    *)
      printf 'cargo %s\n' "$*" >>"$FX/calls"
      return 97
      ;;
  esac
}

# old_gate: an old learn-gate under target/debug, which records that it ran and fails every play.
old_gate() {
  cat <<'EOF'
#!/bin/sh
printf 'old %s\n' "$*" >>"$FX/calls"
exit 3
EOF
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
  printf 'learn-gate\n' >"$FX/built-names" || broken
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
    if [ -e "$FX/no-cargo" ]; then
      mkdir -p "$work/no-cargo-path" || exit 1
      PATH=$work/no-cargo-path:/usr/bin:/bin
    else
      cargo() {
        stand_in_cargo "$@"
      }
    fi
    unset CARGO_TARGET_DIR
    if [ -e "$FX/target-dir" ]; then
      CARGO_TARGET_DIR=$(cat "$FX/target-dir") || exit 1
      export CARGO_TARGET_DIR
    fi
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

# sets_hold FILE SET...: the set= lines of FILE are one for each SET, in that order, and each line of a set's own
# keys stands below a set= line, a spread= line below set=heldout.
sets_hold() {
  file=$1
  shift
  awk -v want="$*" '
    /^set=/ { set = substr($0, 5); got = got (got == "" ? "" : " ") set; next }
    /^(improve|rollback|drift result|restart=|readers |store=|spread=)/ {
      if (set == "" || ($0 ~ /^spread=/ && set != "heldout")) bad = 1
    }
    END { if (bad || got != want) exit 1 }
  ' "$file"
}

# set_rejected LABEL: the output $work/LABEL.out does not hold the sets dev and heldout as sets_hold reads them.
set_rejected() {
  if sets_hold "$work/$1.out" dev heldout; then
    wrong="$wrong $1"
  else
    rejected=$((rejected + 1))
  fi
}

# two_sets_wanted: the lines that the check prints for the fixture two-sets.
two_sets_wanted() {
  for one_set in dev heldout; do
    printf 'set=%s\n' "$one_set"
    printf 'improve cases=1 admit=1 none=3 bound=1 worse=0\n'
    printf 'rollback cases=0 wins=0 losses=0 critical=0 cost=0 budget=1\n'
    printf 'drift result=ok\nrestart=same\nreaders same=2 of=2\nstore=memory\n'
  done
  printf 'spread=same\nPASS %s\n' "$name"
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

  for olds in 'learn-gate learn-gate.exe' learn-gate learn-gate.exe; do
    fixture "old-files-under-target-$(printf '%s' "$olds" | tr ' ' '+')"
    mkdir -p "$fx_repo/target/debug" || broken
    for old in $olds; do
      old_gate >"$fx_repo/target/debug/$old" || broken
      chmod +x "$fx_repo/target/debug/$old" || broken
    done
    check PASS 'readers same=2 of=2'
    calls_are "$build" "gate $dev --store derived" "gate $dev --store memory" \
      "gate $dev --store derived --restart-all" "gate $dev --store memory --restart-all"
  done

  fixture cargo-warns-beside-a-clean-play
  : >"$FX/cargo-warns"
  check PASS 'readers same=2 of=2'
  calls_are "$build" "gate $dev --store derived" "gate $dev --store memory" \
    "gate $dev --store derived --restart-all" "gate $dev --store memory --restart-all"

  fixture build-directory-of-its-own
  printf '%s\n' "$work/$case_name/build" >"$FX/target-dir" || broken
  mkdir -p "$fx_repo/target/debug" || broken
  for old in learn-gate learn-gate.exe; do
    old_gate >"$fx_repo/target/debug/$old" || broken
    chmod +x "$fx_repo/target/debug/$old" || broken
  done
  check PASS 'readers same=2 of=2'
  calls_are "$build" "gate $dev --store derived" "gate $dev --store memory" \
    "gate $dev --store derived --restart-all" "gate $dev --store memory --restart-all"

  fixture none-after-the-build
  : >"$FX/built-names"
  check FAIL-build "FAIL $name (build)"
  calls_are "$build"

  fixture both-after-the-build
  printf 'learn-gate\nlearn-gate.exe\n' >"$FX/built-names"
  check FAIL-build "FAIL $name (build)"
  calls_are "$build"

  fixture no-cargo
  : >"$FX/no-cargo"
  check FAIL-cargo "FAIL $name (cargo)"
  calls_are

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
  printf '%s\n' "$out" >"$work/two-sets.out" || broken
  two_sets_wanted >"$work/two-sets.wanted" || broken
  cmp -s "$work/two-sets.out" "$work/two-sets.wanted" || wrong="$wrong two-sets:lines"
  sets_hold "$work/two-sets.out" dev heldout || wrong="$wrong two-sets:sets"
  grep -v '^set=heldout$' "$work/two-sets.out" >"$work/set-line-missing.out" || broken
  set_rejected set-line-missing
  awk '/^set=heldout$/ { h = 1 } /^PASS / { p = 1 } { if (p) tail = tail $0 "\n"; else if (h) held = held $0 "\n"; else dev = dev $0 "\n" } END { printf "%s%s%s", held, dev, tail }' "$work/two-sets.out" >"$work/sets-in-the-other-order.out" || broken
  set_rejected sets-in-the-other-order
  awk '/^spread=/ { next } { print } /^set=dev$/ { print "spread=same" }' "$work/two-sets.out" >"$work/spread-below-dev.out" || broken
  set_rejected spread-below-dev

  fixture one-set
  check PASS 'readers same=2 of=2'
  printf '%s\n' "$out" >"$work/one-set.out" || broken
  sets_hold "$work/one-set.out" dev || wrong="$wrong one-set:sets"

  fixture two-sets-derived-taken heldout
  : >"$FX/derived"
  check PASS 'spread=same'
  calls_are "$build" "gate $dev --store derived" "gate $dev --store derived --restart-all" \
    "gate $held --store derived" "gate $held --store derived --restart-all"

  fixture derived-lines-name-the-memory-store
  : >"$FX/derived"
  printf 'memory\n' >"$FX/lines-store" || broken
  check FAIL-store "FAIL $name (store)"

  fixture memory-lines-name-the-derived-store
  printf 'derived\n' >"$FX/lines-store" || broken
  check FAIL-store "FAIL $name (store)"

  fixture derived-first-play-alone
  : >"$FX/derived-dev-first"
  check FAIL-store "FAIL $name (store)"
  calls_are "$build" "gate $dev --store derived" "gate $dev --store derived --restart-all" \
    "gate $dev --store memory --restart-all"

  fixture derived-second-play-alone
  : >"$FX/derived-dev-second"
  check FAIL-store "FAIL $name (store)"
  calls_are "$build" "gate $dev --store derived" "gate $dev --store memory" \
    "gate $dev --store derived --restart-all"

  fixture derived-development-set-alone heldout
  : >"$FX/derived-dev-first"
  : >"$FX/derived-dev-second"
  check FAIL-store "FAIL $name (store)"
  calls_are "$build" "gate $dev --store derived" "gate $dev --store derived --restart-all" \
    "gate $held --store derived" "gate $held --store memory" \
    "gate $held --store derived --restart-all" "gate $held --store memory --restart-all"

  for at in first second; do
    for form in other-detail second-line blank-line-after longer-line nul-in-line no-final-lf beside-output blank-output status-1 status-3; do
      fixture "derived-$at-$form"
      code=2
      : >"$FX/derived-$at.out" || broken
      case $form in
        other-detail) printf 'REFUSED store: the data directory cannot be opened\n' ;;
        second-line) printf 'a line before it\n%s\n' "$not_built" ;;
        blank-line-after) printf '%s\n\n' "$not_built" ;;
        longer-line) printf '%s yet\n' "$not_built" ;;
        nul-in-line) printf 'REFUSED store: the derived store\000 is not in this build\n' ;;
        no-final-lf) printf '%s' "$not_built" ;;
        blank-output)
          printf '\n' >"$FX/derived-$at.out" || broken
          printf '%s\n' "$not_built"
          ;;
        status-3)
          code=3
          printf '%s\n' "$not_built"
          ;;
        beside-output)
          printf 'drift result=ok\n' >"$FX/derived-$at.out" || broken
          printf '%s\n' "$not_built"
          ;;
        status-1)
          code=1
          printf '%s\n' "$not_built"
          ;;
      esac >"$FX/derived-$at.err" || broken
      printf '%s\n' "$code" >"$FX/derived-$at.code" || broken
      case $at:$form in
        first:status-1)
          check FAIL-stderr "FAIL $name (stderr)"
          calls_are "$build" "gate $dev --store derived" "gate $dev --store derived --restart-all" \
            "gate $dev --store memory --restart-all"
          ;;
        second:status-1)
          check FAIL-stderr "FAIL $name (stderr)"
          calls_are "$build" "gate $dev --store derived" "gate $dev --store memory" \
            "gate $dev --store derived --restart-all"
          ;;
        first:status-3)
          check FAIL-exit "FAIL $name (exit)"
          calls_are "$build" "gate $dev --store derived" "gate $dev --store derived --restart-all" \
            "gate $dev --store memory --restart-all"
          ;;
        second:status-3)
          check FAIL-exit "FAIL $name (exit)"
          calls_are "$build" "gate $dev --store derived" "gate $dev --store memory" \
            "gate $dev --store derived --restart-all"
          ;;
        first:*)
          check FAIL-refused "FAIL $name (refused)"
          calls_are "$build" "gate $dev --store derived"
          ;;
        second:*)
          check FAIL-refused "FAIL $name (refused)"
          calls_are "$build" "gate $dev --store derived" "gate $dev --store memory" \
            "gate $dev --store derived --restart-all"
          ;;
      esac
    done
  done

  fixture stderr-in-the-first-play-at-status-0
  printf 'x\n' >"$FX/dev-first.err" || broken
  check FAIL-stderr "FAIL $name (stderr)"

  fixture stderr-in-the-second-play-at-status-0
  printf '\n' >"$FX/dev-second.err" || broken
  check FAIL-stderr "FAIL $name (stderr)"

  fixture derived-taken-with-stderr-in-the-first-play
  : >"$FX/derived"
  printf 'x\n' >"$FX/dev-first.err" || broken
  check FAIL-stderr "FAIL $name (stderr)"

  fixture stderr-in-the-first-play-at-status-1
  play_lines dev memory 2 "" miss >"$FX/dev-first.out" || broken
  printf '1\n' >"$FX/dev-first.code" || broken
  printf 'x\n' >"$FX/dev-first.err" || broken
  check FAIL-stderr "FAIL $name (stderr)"

  fixture stderr-in-a-held-out-play-at-status-0 heldout
  printf 'warning: a line of the build\n' >"$FX/heldout-second.err" || broken
  check FAIL-stderr "FAIL $name (stderr)"

  fixture stderr-in-the-development-set-of-two heldout
  printf 'x\n' >"$FX/dev-first.err" || broken
  check FAIL-stderr "FAIL $name (stderr)"

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
