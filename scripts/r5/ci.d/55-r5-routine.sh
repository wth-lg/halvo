#!/bin/sh
# jobs: macos windows
# rule: learn-gate routine prints routine trains=135 plays=270 same=270 result=ok on the memory store, alone and with exit 0; learn-gate routine --store derived prints routine trains=5 plays=10 same=10 store=derived result=ok, or derived=not-run while the binary refuses it with store; and no cargo command or binary of the check changes the tree.
set -u
LC_ALL=C
export LC_ALL
name=55-r5-routine
script_path=scripts/r5/ci.d/55-r5-routine.sh
lfx=$(printf '\nx')
nl=${lfx%x}
want_memory='routine trains=135 plays=270 same=270 result=ok'
want_derived='routine trains=5 plays=10 same=10 store=derived result=ok'
not_built='REFUSED store: the derived store is not in this build'

finish() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

made=
cleanup() {
  case $made in
    */55-r5-routine.??????) rm -rf "$made" ;;
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

# play DIR: runs learn-gate routine and learn-gate routine --store derived with their output under DIR, and sets
# memory_exit and derived_exit.
play() {
  run_gate routine >"$1/memory.out" 2>"$1/memory.err" </dev/null
  memory_exit=$?
  run_gate routine --store derived >"$1/derived.out" 2>"$1/derived.err" </dev/null
  derived_exit=$?
}

# shown FILE PATTERN: prints the one line of FILE when FILE holds exactly one line and it matches the extended
# pattern, which admits counts and fixed tokens alone; else prints the token unreadable.
shown() {
  lines=$(awk 'END { print NR }' "$1") || lines=0
  if [ "$lines" -eq 1 ] && grep -Eq "$2" "$1"; then
    cat "$1"
  else
    printf 'unreadable\n'
  fi
}

# judge DIR: prints the routine line and the derived line or derived=not-run from the outputs under DIR, and sets
# cause to the first FAIL cause that holds, or to nothing.
judge() {
  cause=
  memory_line=$(shown "$1/memory.out" '^routine trains=[0-9]+ plays=[0-9]+ same=[0-9]+ result=(ok|miss)$') || memory_line=unreadable
  if [ "$memory_line" = unreadable ]; then
    printf 'routine=unreadable exit=%s\n' "$memory_exit"
  else
    printf '%s\n' "$memory_line"
  fi
  err_lines=$(awk 'END { print NR }' "$1/derived.err") || err_lines=0
  err_text=$(cat "$1/derived.err") || err_text=
  if [ "$derived_exit" -eq 2 ] && [ ! -s "$1/derived.out" ] && [ "$err_lines" -eq 1 ] && [ "$err_text" = "$not_built" ]; then
    derived_line=derived=not-run
    printf '%s\n' "$derived_line"
  else
    derived_line=$(shown "$1/derived.out" '^routine trains=[0-9]+ plays=[0-9]+ same=[0-9]+ store=[a-z]+ result=(ok|miss)$') || derived_line=unreadable
    if [ "$derived_line" = unreadable ]; then
      printf 'derived=unreadable exit=%s\n' "$derived_exit"
    else
      printf '%s\n' "$derived_line"
    fi
  fi
  if [ "$memory_exit" -eq 2 ]; then
    cause=refused
  elif [ "$memory_exit" -ne 0 ] || [ "$memory_line" != "$want_memory" ]; then
    cause=result
  elif [ "$derived_line" = derived=not-run ]; then
    :
  elif [ "$derived_exit" -ne 0 ] || [ "$derived_line" != "$want_derived" ]; then
    cause=derived
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
  play "$art"
  tree_same gate
  judge "$art"
  [ -z "$cause" ] || finish "$cause"
  printf 'PASS %s\n' "$name"
}

fixture_gate() {
  cat <<'EOF'
#!/bin/sh
case $* in
  routine) cat "$FIXTURE_DIR/fix-memory"; exit "$(cat "$FIXTURE_DIR/fix-memory.exit")" ;;
  *) cat "$FIXTURE_DIR/fix-derived"; cat "$FIXTURE_DIR/fix-derived.err" >&2; exit "$(cat "$FIXTURE_DIR/fix-derived.exit")" ;;
esac
EOF
}

broken() {
  printf 'FAIL self-test:%s (scratch)\n' "$name"
  exit 1
}

# self_case LABEL WANT_CAUSE MEMORY_LINES MEMORY_EXIT DERIVED_LINES DERIVED_ERR DERIVED_EXIT: judges the outputs that
# the fixture gate prints from these values, and records whether the cause is the one wanted.
self_case() {
  label=$1
  want=$2
  FIXTURE_DIR=$work/$label
  export FIXTURE_DIR
  mkdir -p "$FIXTURE_DIR" || broken
  printf '%s' "$3" >"$FIXTURE_DIR/fix-memory" || broken
  printf '%s\n' "$4" >"$FIXTURE_DIR/fix-memory.exit" || broken
  printf '%s' "$5" >"$FIXTURE_DIR/fix-derived" || broken
  printf '%s' "$6" >"$FIXTURE_DIR/fix-derived.err" || broken
  printf '%s\n' "$7" >"$FIXTURE_DIR/fix-derived.exit" || broken
  play "$FIXTURE_DIR"
  judge "$FIXTURE_DIR" >"$FIXTURE_DIR/lines.txt"
  printed=$(awk 'END { print NR }' "$FIXTURE_DIR/lines.txt") || printed=0
  allowed=$(grep -Ec "$log_line" "$FIXTURE_DIR/lines.txt") || allowed=0
  if [ "$cause" != "$want" ]; then
    wrong="$wrong $label:${cause:-pass}"
  elif [ "$printed" -ne 2 ] || [ "$allowed" -ne 2 ]; then
    wrong="$wrong $label:log"
  elif [ -n "$want" ]; then
    rejected=$((rejected + 1))
  fi
}

self_test() {
  made=$(mktemp -d "${TMPDIR:-/tmp}/55-r5-routine.XXXXXX") || broken
  work=$made
  fixture_gate >"$work/gate.sh" || broken
  gate_script=$work/gate.sh
  ok="$want_memory$nl"
  store_refusal="$not_built$nl"
  log_line='^(routine trains=[0-9]+ plays=[0-9]+ same=[0-9]+( store=[a-z]+)? result=(ok|miss)|(routine|derived)=unreadable exit=[0-9]+|derived=not-run)$'
  failing_fixtures=18
  wrong=
  rejected=0
  self_case good "" "$ok" 0 "" "$store_refusal" 2
  self_case good-derived "" "$ok" 0 "$want_derived$nl" "" 0
  self_case one-train-differs result "routine trains=135 plays=270 same=268 result=miss$nl" 1 "" "$store_refusal" 2
  self_case one-play-differs result "routine trains=135 plays=270 same=269 result=miss$nl" 1 "" "$store_refusal" 2
  self_case miss-read-as-ok result "routine trains=135 plays=270 same=269 result=ok$nl" 0 "" "$store_refusal" 2
  self_case fewer-trains result "routine trains=134 plays=268 same=268 result=ok$nl" 0 "" "$store_refusal" 2
  self_case ok-with-exit-1 result "$ok" 1 "" "$store_refusal" 2
  self_case second-line result "$ok${ok}" 0 "" "$store_refusal" 2
  self_case no-line result "" 0 "" "$store_refusal" 2
  self_case line-with-a-path result "routine trains=135 path=/x/sample.jsonl result=ok$nl" 0 "" "$store_refusal" 2
  self_case refused refused "" 2 "" "$store_refusal" 2
  self_case derived-miss derived "$ok" 0 "routine trains=5 plays=10 same=9 store=derived result=miss$nl" "" 1
  self_case derived-miss-read-as-ok derived "$ok" 0 "routine trains=5 plays=10 same=9 store=derived result=ok$nl" "" 0
  self_case derived-ok-with-exit-1 derived "$ok" 0 "$want_derived$nl" "" 1
  self_case derived-on-memory derived "$ok" 0 "routine trains=5 plays=10 same=10 store=memory result=ok$nl" "" 0
  self_case derived-usage derived "$ok" 0 "" "REFUSED usage: an argument that the command does not take$nl" 2
  self_case derived-store-other-detail derived "$ok" 0 "" "REFUSED store: train 3: the replay run of day 150 did not complete$nl" 2
  self_case derived-store-second-line derived "$ok" 0 "" "a line before it$nl$store_refusal" 2
  self_case derived-store-beside-a-miss derived "$ok" 0 "routine trains=5 plays=10 same=9 store=derived result=miss$nl" "$store_refusal" 2
  self_case derived-store-status-1 derived "$ok" 0 "" "$store_refusal" 1
  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  if [ "$rejected" -ne "$failing_fixtures" ]; then
    printf 'FAIL self-test:%s (%s failing fixtures rejected, not %s)\n' "$name" "$rejected" "$failing_fixtures"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  '') measure ;;
  --self-test) self_test ;;
  *) finish usage ;;
esac
