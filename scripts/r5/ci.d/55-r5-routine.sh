#!/bin/sh
# jobs: macos windows
# rule: learn-gate routine prints routine trains=135 plays=270 same=270 result=ok on the memory store, alone on standard output and with exit 0; learn-gate routine --store derived prints routine trains=5 plays=10 same=10 store=derived result=ok, or derived=not-run for the build's own refusal line alone (status 2, nothing on standard output, and the one line REFUSED store: the derived store is not in this build on standard error); before its first cargo command the check fails when a tracked file differs from HEAD (changed or staged, its own file among them) or carries an index flag, prints for the changed ones the one line tree changed=before files=<n> with their count and no path, and keeps git's status and index list in tree-before.txt of its artifacts, and a file that git does not track is allowed; and no cargo command or binary of the check changes the tree.
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
exec 9>&1
trap 'printf "FAIL %s (signal)\n" "$name" >&9; exit 1' HUP INT TERM

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
  else
    passed=$((passed + 1))
  fi
}

# tree_git ARGS: the stand-in for git in the tree cases: the two reads of the tree, from the case's status file.
tree_git() {
  case $* in
    '-c core.fsmonitor=false -c core.untrackedCache=false status --porcelain=v2 --branch --untracked-files=all')
      cat "$TREE_DIR/status"
      ;;
    'ls-files -v')
      printf 'H %s\nH fixtures/r5/trains/sample.jsonl\n' "$script_path"
      ;;
    *) return 97 ;;
  esac
}

# tree_cargo ARGS: the stand-in for cargo in the tree cases: records the call, changes the tree when the case asks it
# to at the build, and fails when the case's build fails.
tree_cargo() {
  printf '%s\n' "$*" >>"$TREE_DIR/cargo.calls"
  if [ "$TREE_CHANGE_AT" = build ]; then
    printf '%s\n' "$TREE_CHANGE_LINE" >>"$TREE_DIR/status"
  fi
  [ "$TREE_BUILD" = ok ] || return 101
}

tree_gate() {
  cat <<'EOF'
#!/bin/sh
printf '%s\n' "$*" >>"$TREE_DIR/gate.calls"
if [ "$TREE_CHANGE_AT" = gate ]; then
  printf '%s\n' "$TREE_CHANGE_LINE" >>"$TREE_DIR/status"
fi
case $* in
  routine) printf 'routine trains=135 plays=270 same=270 result=ok\n' ;;
  *) printf 'REFUSED store: the derived store is not in this build\n' >&2; exit 2 ;;
esac
EOF
}

# tree_case LABEL WANT TEXT LINE_BEFORE CHANGE_AT CHANGE_LINE BUILD CARGO [HIDDEN]: runs measure, the function that the
# check runs, in a subshell with the stand-ins for git and for cargo (or no cargo when CARGO is no) and with the stand-in
# gate where the lookup finds the built binary, and records whether the last line, the text and the calls are the
# wanted; with HIDDEN, also that the output does not hold that word and the artifacts' tree-before.txt does.
tree_case() {
  label=$1
  want=$2
  text=$3
  hidden=${9:-}
  TREE_DIR=$work/tree-$label
  TREE_CHANGE_AT=$5
  TREE_CHANGE_LINE=$6
  TREE_BUILD=$7
  export TREE_DIR TREE_CHANGE_AT TREE_CHANGE_LINE TREE_BUILD
  mkdir -p "$TREE_DIR/repo/target/debug" "$TREE_DIR/art" "$work/no-cargo" || broken
  printf '# branch.oid 1111111111111111111111111111111111111111\n# branch.head r5\n' >"$TREE_DIR/status" || broken
  if [ -n "$4" ]; then
    printf '%s\n' "$4" >>"$TREE_DIR/status" || broken
  fi
  tree_gate >"$TREE_DIR/repo/target/debug/learn-gate" || broken
  chmod +x "$TREE_DIR/repo/target/debug/learn-gate" || broken
  out=$(
    git() {
      tree_git "$@"
    }
    if [ "$8" = yes ]; then
      cargo() {
        tree_cargo "$@"
      }
    else
      PATH=$work/no-cargo:/usr/bin:/bin
    fi
    CI_D_REPO=$TREE_DIR/repo
    CI_D_ARTIFACTS=$TREE_DIR/art
    CARGO_TARGET_DIR=$TREE_DIR/repo/target
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
  if [ -n "$hidden" ]; then
    case $out in
      *"$hidden"*) got=$got-showing-the-name ;;
    esac
    grep -q "$hidden" "$TREE_DIR/art/tree-before.txt" 2>/dev/null || got=$got-without-the-artifact
  fi
  case $label in
    changed-before | staged-before | sample-before | name-not-shown | heldout-name-not-shown)
      [ ! -e "$TREE_DIR/cargo.calls" ] || got=$got-after-a-cargo-call
      ;;
    build-fails)
      [ ! -e "$TREE_DIR/gate.calls" ] || got=$got-after-a-gate-call
      ;;
  esac
  printf 'case %s want=%s got=%s\n' "$label" "$want" "$got" >>"$work/tree-cases.txt"
  if [ "$got" != "$want" ]; then
    wrong="$wrong tree-$label:$got"
  elif [ "$want" = PASS ]; then
    tree_passed=$((tree_passed + 1))
  else
    tree_rejected=$((tree_rejected + 1))
  fi
}

tree_cases() {
  oid=e69de29bb2d1d6434b8b29ae775ad8c2e48c5391
  changed="1 .M N... 100644 100644 100644 $oid $oid crates/halvo-learn-gate/src/routine.rs"
  staged="1 M. N... 100644 100644 100644 $oid $oid crates/halvo-learn-gate/src/routine.rs"
  sample="1 .M N... 100644 100644 100644 $oid $oid fixtures/r5/trains/sample.jsonl"
  later="1 .M N... 100644 100644 100644 $oid $oid crates/halvo-learn-gate/src/main.rs"
  word="1 .M N... 100644 100644 100644 $oid $oid crates/halvo-learn-gate/src/quillwortpenny.rs"
  heldout="1 M. N... 100644 100644 100644 $oid $oid fixtures/r5/cases/heldout/zephyrcalyx-kite.jsonl"
  tree_passed=0
  tree_rejected=0
  tree_case changed-before FAIL-tree 'tree changed=before files=1' "$changed" "" "" ok yes
  tree_case staged-before FAIL-tree 'tree changed=before files=1' "$staged" "" "" ok yes
  tree_case sample-before FAIL-tree 'tree changed=before files=1' "$sample" "" "" ok yes
  tree_case name-not-shown FAIL-tree 'tree changed=before files=1' "$word" "" "" ok yes quillwortpenny
  tree_case heldout-name-not-shown FAIL-tree 'tree changed=before files=1' "$heldout" "" "" ok yes zephyrcalyx
  tree_case untracked-before PASS "PASS $name" '? notes.txt' "" "" ok yes
  tree_case build-changes FAIL-tree 'tree changed=after-build' "" build "$later" ok yes
  tree_case play-changes FAIL-tree 'tree changed=after-gate' "" gate "$later" ok yes
  tree_case no-cargo FAIL-cargo "FAIL $name (cargo)" "" "" "" ok no
  tree_case build-fails FAIL-build "FAIL $name (build)" "" "" "" fails yes
}

self_test() {
  made=$(mktemp -d "${TMPDIR:-/tmp}/55-r5-routine.XXXXXX") || broken
  work=$made
  fixture_gate >"$work/gate.sh" || broken
  gate_script=$work/gate.sh
  ok="$want_memory$nl"
  store_refusal="$not_built$nl"
  log_line='^(routine trains=[0-9]+ plays=[0-9]+ same=[0-9]+( store=[a-z]+)? result=(ok|miss)|(routine|derived)=unreadable exit=[0-9]+|derived=not-run)$'
  failing_fixtures=19
  passing_fixtures=2
  wrong=
  rejected=0
  passed=0
  self_case good "" "$ok" 0 "" "$store_refusal" 2
  self_case good-derived "" "$ok" 0 "$want_derived$nl" "" 0
  self_case one-train-differs result "routine trains=135 plays=270 same=268 result=miss$nl" 1 "" "$store_refusal" 2
  self_case one-play-differs result "routine trains=135 plays=270 same=269 result=miss$nl" 1 "" "$store_refusal" 2
  self_case miss-read-as-ok result "routine trains=135 plays=270 same=269 result=ok$nl" 0 "" "$store_refusal" 2
  self_case trains-134-plays-270 result "routine trains=134 plays=270 same=270 result=ok$nl" 0 "" "$store_refusal" 2
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
  tree_cases
  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  if [ "$tree_passed" -ne 1 ] || [ "$tree_rejected" -ne 9 ]; then
    printf 'FAIL self-test:%s (tree cases passed %s, not 1, and rejected %s, not 9)\n' "$name" "$tree_passed" "$tree_rejected"
    exit 1
  fi
  if [ "$passed" -ne "$passing_fixtures" ]; then
    printf 'FAIL self-test:%s (%s passing fixtures passed, not %s)\n' "$name" "$passed" "$passing_fixtures"
    exit 1
  fi
  if [ "$rejected" -ne "$failing_fixtures" ]; then
    printf 'FAIL self-test:%s (%s failing fixtures rejected, not %s)\n' "$name" "$rejected" "$failing_fixtures"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$((rejected + tree_rejected))"
}

case ${1:-} in
  '') measure ;;
  --self-test) self_test ;;
  *) finish usage ;;
esac
