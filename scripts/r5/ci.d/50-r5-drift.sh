#!/bin/sh
# jobs: macos windows
# rule: learn-gate drift --gate dev, and --gate heldout once the held-out cases are in the work tree, prints drift result=ok on the derived store or, while the binary refuses it with store, on the memory store; a second play with every occurrence restarted gives every integer of the first but expired_runs; the trial harness's arms none and plain give, for every case file, the occurrences, drifts, assisted, failed and harm of the gate host's lines of those arms; the held-out spread lines are those of docs/r5/heldout.txt; and no cargo command or binary of the check changes the tree.
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
  if [ -n "$gate_script" ]; then
    sh "$gate_script" "$@"
  else
    "$gate" "$@"
  fi
}

run_trial() {
  sh "$trial_script" "$@"
}

# gate_play SET DIR OUT [--restart-all]: runs the gate host on the set, on the derived store or, when the binary
# refuses that with store, on the memory store; sets played (the exit status) and store.
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

# judge SET OUT RESTART_OUT SPREAD_FILE: prints the lines of one set from the two plays and the readers' counts,
# and sets cause to the first FAIL cause that holds, or to nothing.
judge() {
  cause=
  grep -E '^(improve|rollback|drift result)' "$2" || true
  word=$(restart_word "$2" "$3") || word=differ
  printf 'restart=%s\n' "$word"
  printf 'readers same=%s of=%s\n' "$same" "$of"
  printf 'store=%s\n' "$store"
  spread=same
  if [ "$1" = heldout ]; then
    grep '^spread ' "$2" >"$art/spread-gate.txt" || true
    if ! cmp -s "$4" "$art/spread-gate.txt"; then
      spread=differ
    fi
    printf 'spread=%s\n' "$spread"
  fi
  result=$(grep -c '^drift result=ok$' "$2") || result=0
  if [ "$result" -ne 1 ]; then
    cause=result
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
  gate_script=
  trial_script=tools/r5-trials/drift/run.sh
  status=pass
  for set_name in $sets; do
    gate_play "$set_name" "fixtures/r5/cases/$set_name" "$art/$set_name.out"
    first=$played
    gate_play "$set_name" "fixtures/r5/cases/$set_name" "$art/$set_name-restart.out" --restart-all
    second=$played
    tree_same "gate-$set_name"
    case "$first $second" in
      *2*) finish refused ;;
    esac
    readers "fixtures/r5/cases/$set_name" "$art/$set_name.out"
    tree_same "trial-$set_name"
    judge "$set_name" "$art/$set_name.out" "$art/$set_name-restart.out" "$art/spread-file.txt"
    if [ -n "$cause" ] && [ "$status" = pass ]; then
      status=$cause
    fi
  done
  [ "$status" = pass ] || finish "$status"
  printf 'PASS %s\n' "$name"
}

fixture_line() {
  printf 'case name=%s kind=%s form=%s arm=%s store=memory occurrences=8 drifts=1 assisted=%s failed=0 harm=%s reverted_good=0 edit_rows=%s expired_runs=%s\n' \
    "$1" "$2" "$3" "$4" "$5" "$6" "$7" "$8"
}

fixture_play() {
  fixture_line renamed-a benign renamed none 3 0 0 0
  fixture_line renamed-a benign renamed plain 1 0 0 0
  fixture_line renamed-a benign renamed admit 1 "${3:-0}" 2 "$1"
  printf 'improve cases=1 admit=1 none=3 bound=1 worse=0\n'
  printf 'rollback cases=0 wins=0 losses=0 critical=0 cost=0 budget=1\n'
  printf 'drift result=%s\n' "$2"
}

fixture_trial() {
  cat <<'EOF'
#!/bin/sh
arm=$4
case $arm in
  none) assisted=3 ;;
  *) assisted=${PLAIN_ASSISTED:-1} ;;
esac
printf 'case name=renamed-a kind=benign form=renamed arm=%s sort=- occurrences=8 drifts=1 assisted=%s failed=0 harm=0 reverted_good=0\n' "$arm" "$assisted"
EOF
}

fixture_gate() {
  cat <<'EOF'
#!/bin/sh
for arg in "$@"; do
  if [ "$arg" = derived ]; then
    printf 'REFUSED store: the derived store is not in this build\n' >&2
    exit 2
  fi
done
cat "$FIXTURE_PLAY"
EOF
}

# self_case LABEL WANT_CAUSE PLAY RESTART_PLAY [PLAIN_ASSISTED]: judges one fixture set, with the trial's arm plain
# printing PLAIN_ASSISTED when it is given, and records whether the cause is the one wanted.
self_case() {
  label=$1
  want=$2
  play=$3
  again=$4
  plain=${5:-}
  art=$work/art-$label
  mkdir -p "$art" || broken
  FIXTURE_PLAY=$play
  export FIXTURE_PLAY
  gate_play dev "$work/cases" "$art/dev.out"
  first=$played
  FIXTURE_PLAY=$again
  gate_play dev "$work/cases" "$art/dev-restart.out" --restart-all
  if [ "$first $played" != "0 0" ]; then
    wrong="$wrong $label:exit"
    return
  fi
  if [ -n "$plain" ]; then
    PLAIN_ASSISTED=$plain
    export PLAIN_ASSISTED
  fi
  readers "$work/cases" "$art/dev.out"
  unset PLAIN_ASSISTED
  judge dev "$art/dev.out" "$art/dev-restart.out" "$work/none" >"$art/lines.txt"
  if [ "$cause" != "$want" ]; then
    wrong="$wrong $label:${cause:-pass}"
  elif [ -n "$want" ]; then
    rejected=$((rejected + 1))
  fi
}

broken() {
  printf 'FAIL self-test:%s (scratch)\n' "$name"
  exit 1
}

self_test() {
  made=$(mktemp -d "${TMPDIR:-/tmp}/50-r5-drift.XXXXXX") || broken
  work=$made
  mkdir -p "$work/cases" || broken
  : >"$work/cases/renamed-a.jsonl" || broken
  fixture_trial >"$work/trial.sh" || broken
  fixture_gate >"$work/gate.sh" || broken
  trial_script=$work/trial.sh
  gate_script=$work/gate.sh
  fixture_play 1 ok >"$work/good" || broken
  fixture_play 3 ok >"$work/good-restart" || broken
  fixture_play 1 miss >"$work/miss" || broken
  fixture_play 3 ok 1 >"$work/differ-restart" || broken
  wrong=
  rejected=0
  self_case good "" "$work/good" "$work/good-restart"
  self_case readers-below readers "$work/good" "$work/good-restart" 2
  self_case result-miss result "$work/miss" "$work/miss"
  self_case restart-differ restart "$work/good" "$work/differ-restart"
  if [ "$store" != memory ]; then
    wrong="$wrong store:$store"
  fi
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
