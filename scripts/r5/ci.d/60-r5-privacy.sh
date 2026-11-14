#!/bin/sh
# jobs: macos windows
# rule: learn-gate privacy --data-dir DIR, run with TMPDIR, TMP and TEMP set to an empty directory of the check's own, prints the six lines of contracts.md L11.4 in their order, learn_deleted and learn_outside result=ok on the memory store and learn_listed, learn_scrub, learn_delete_all and learn_expired result=not-run, alone on standard output with exit 0; learn-gate delete --what workflow --data-dir DIR --store memory prints delete what=workflow store=memory rows_left=0 other_rows=<n above 0> other_served=yes result=ok with exit 0, and learn-gate delete --what all --data-dir DIR --store memory prints delete what=all store=memory exit=- files_left=- result=not-run with exit 0; each command gets a data directory of its own, and one that ends with status 0 or 1 writes nothing on standard error; before its first cargo command the check fails when a tracked file differs from HEAD (changed or staged, its own file among them) or carries an index flag, prints for the changed ones the one line tree changed=before files=<n> with their count and no path, and keeps git's status and index list in tree-before.txt of its artifacts, and a file that git does not track is allowed; and no cargo command or binary of the check changes the tree.
set -u
LC_ALL=C
export LC_ALL
name=60-r5-privacy
script_path=scripts/r5/ci.d/60-r5-privacy.sh
lfx=$(printf '\nx')
nl=${lfx%x}
want_privacy="privacy check=learn_listed result=not-run
privacy check=learn_deleted result=ok
privacy check=learn_scrub result=not-run
privacy check=learn_delete_all result=not-run
privacy check=learn_outside result=ok
privacy check=learn_expired result=not-run"
want_workflow='^delete what=workflow store=memory rows_left=0 other_rows=[1-9][0-9]* other_served=yes result=ok$'
want_all='delete what=all store=memory exit=- files_left=- result=not-run'

finish() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

made=
cleanup() {
  case $made in
    */60-r5-privacy.??????) rm -rf "$made" ;;
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

# run_gate TEMP ARGS: runs the gate host, or the self-test's stand-in for it, with ARGS, and with TMPDIR, TMP and TEMP
# set to TEMP for that one command when TEMP is not empty.
run_gate() {
  temp=$1
  shift
  if [ -n "$gate_script" ]; then
    set -- sh "$gate_script" "$@"
  else
    set -- "$gate" "$@"
  fi
  if [ -n "$temp" ]; then
    env TMPDIR="$temp" TMP="$temp" TEMP="$temp" "$@"
  else
    "$@"
  fi
}

# play OUT SCRATCH: runs the three commands, each with a data directory of its own under SCRATCH and privacy with an
# empty temporary directory under SCRATCH, keeps their output under OUT, and sets privacy_exit, workflow_exit and
# all_exit; a directory that cannot be made fails the check with the cause scratch.
play() {
  mkdir "$2/privacy-data" "$2/privacy-tmp" "$2/workflow-data" "$2/all-data" || finish scratch
  run_gate "$2/privacy-tmp" privacy --data-dir "$2/privacy-data" \
    >"$1/privacy.out" 2>"$1/privacy.err" </dev/null
  privacy_exit=$?
  run_gate '' delete --what workflow --data-dir "$2/workflow-data" --store memory \
    >"$1/workflow.out" 2>"$1/workflow.err" </dev/null
  workflow_exit=$?
  run_gate '' delete --what all --data-dir "$2/all-data" --store memory \
    >"$1/all.out" 2>"$1/all.err" </dev/null
  all_exit=$?
}

# shown FILE PATTERN WORD: prints the lines of FILE when it holds from one to six lines and each matches the extended
# pattern, which admits counts and fixed tokens alone; else prints WORD=unreadable.
shown() {
  lines=$(awk 'END { print NR }' "$1") || lines=0
  other=$(grep -Evc "$2" "$1") || other=${other:-unreadable}
  if [ "$lines" -ge 1 ] && [ "$lines" -le 6 ] && [ "$other" = 0 ]; then
    awk '{ print }' "$1"
  else
    printf '%s=unreadable\n' "$3"
  fi
}

# quiet FILE EXIT: whether a command that ended with EXIT wrote nothing to FILE, its standard error, at status 0 or 1.
quiet() {
  case $2 in
    0 | 1) [ ! -s "$1" ] ;;
    *) return 0 ;;
  esac
}

# judge DIR: prints the lines of the three commands from the outputs under DIR, each in the form that the log may
# hold, and sets cause to the first FAIL cause that holds, or to nothing.
judge() {
  cause=
  shown "$1/privacy.out" '^privacy check=learn_[a-z_]+ result=(ok|miss|not-run)$' privacy
  shown "$1/workflow.out" '^delete what=workflow store=[a-z]+ rows_left=[0-9]+ other_rows=[0-9]+ other_served=(yes|no) result=(ok|miss)$' workflow
  shown "$1/all.out" '^delete what=all store=[a-z]+ exit=([0-9]+|-) files_left=([0-9]+|-) result=(ok|miss|not-run)$' all
  printf '%s\n' "$want_privacy" >"$1/privacy.want" || finish artifacts
  printf '%s\n' "$want_all" >"$1/all.want" || finish artifacts
  workflow_lines=$(awk 'END { print NR }' "$1/workflow.out") || workflow_lines=0
  if [ "$privacy_exit" -eq 2 ] || [ "$workflow_exit" -eq 2 ] || [ "$all_exit" -eq 2 ]; then
    cause=refused
  elif [ "$privacy_exit" -ne 0 ] || ! cmp -s "$1/privacy.want" "$1/privacy.out" || ! quiet "$1/privacy.err" "$privacy_exit"; then
    cause=privacy
  elif [ "$workflow_exit" -ne 0 ] || [ "$workflow_lines" -ne 1 ] || ! grep -Eq "$want_workflow" "$1/workflow.out" || ! quiet "$1/workflow.err" "$workflow_exit"; then
    cause=delete
  elif [ "$all_exit" -ne 0 ] || ! cmp -s "$1/all.want" "$1/all.out" || ! quiet "$1/all.err" "$all_exit"; then
    cause=all
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
  made=$(mktemp -d "${tmp_parent:-${TMPDIR:-/tmp}}/60-r5-privacy.XXXXXX") || finish scratch
  play "$art" "$made"
  tree_same gate
  judge "$art"
  [ -z "$cause" ] || finish "$cause"
  printf 'PASS %s\n' "$name"
}

fixture_gate() {
  cat <<'EOF'
#!/bin/sh
case $1 in
  privacy)
    if [ -d "$3" ] && [ -d "$TMPDIR" ] && [ "$TMP" = "$TMPDIR" ] && [ "$TEMP" = "$TMPDIR" ] && [ -z "$(ls -A "$TMPDIR")" ]; then
      printf 'privacy --data-dir DIR tmp=own\n' >>"$FIXTURE_DIR/calls"
    else
      printf 'privacy %s\n' "$*" >>"$FIXTURE_DIR/calls"
    fi
    fix=privacy
    ;;
  *)
    if [ -d "$5" ]; then
      printf '%s %s %s %s DIR %s %s\n' "$1" "$2" "$3" "$4" "$6" "$7" >>"$FIXTURE_DIR/calls"
    else
      printf 'delete %s\n' "$*" >>"$FIXTURE_DIR/calls"
    fi
    fix=$3
    ;;
esac
cat "$FIXTURE_DIR/fix-$fix"
cat "$FIXTURE_DIR/fix-$fix.err" >&2
exit "$(cat "$FIXTURE_DIR/fix-$fix.exit")"
EOF
}

broken() {
  printf 'FAIL self-test:%s (scratch)\n' "$name"
  exit 1
}

# changed SCRIPT: prints the six lines that the check wants, changed by the sed script SCRIPT.
changed() {
  printf '%s\n' "$want_privacy" | sed "$1"
}

# self_case LABEL WANT_CAUSE PRIVACY_TEXT PRIVACY_ERR PRIVACY_EXIT WORKFLOW_TEXT WORKFLOW_ERR WORKFLOW_EXIT ALL_TEXT
# ALL_ERR ALL_EXIT: plays the three commands through the fixture gate, which prints these values and records its
# calls, judges the outputs with the functions of the check, and records whether the cause is the one wanted, the
# calls are the check's three calls with the directories of their own, and the log holds no line outside its form.
self_case() {
  label=$1
  want=$2
  FIXTURE_DIR=$work/$label
  export FIXTURE_DIR
  mkdir -p "$FIXTURE_DIR/scratch" || broken
  printf '%s' "$3" >"$FIXTURE_DIR/fix-privacy" || broken
  printf '%s' "$4" >"$FIXTURE_DIR/fix-privacy.err" || broken
  printf '%s\n' "$5" >"$FIXTURE_DIR/fix-privacy.exit" || broken
  printf '%s' "$6" >"$FIXTURE_DIR/fix-workflow" || broken
  printf '%s' "$7" >"$FIXTURE_DIR/fix-workflow.err" || broken
  printf '%s\n' "$8" >"$FIXTURE_DIR/fix-workflow.exit" || broken
  printf '%s' "$9" >"$FIXTURE_DIR/fix-all" || broken
  printf '%s' "${10}" >"$FIXTURE_DIR/fix-all.err" || broken
  printf '%s\n' "${11}" >"$FIXTURE_DIR/fix-all.exit" || broken
  play "$FIXTURE_DIR" "$FIXTURE_DIR/scratch"
  judge "$FIXTURE_DIR" >"$FIXTURE_DIR/lines.txt"
  printed=$(awk 'END { print NR }' "$FIXTURE_DIR/lines.txt") || printed=0
  allowed=$(grep -Ec "$log_line" "$FIXTURE_DIR/lines.txt") || allowed=0
  calls=$(cat "$FIXTURE_DIR/calls") || calls=
  if [ "$cause" != "$want" ]; then
    wrong="$wrong $label:${cause:-pass}"
  elif [ "$calls" != "$want_calls" ]; then
    wrong="$wrong $label:calls"
  elif [ "$printed" -lt 3 ] || [ "$allowed" -ne "$printed" ]; then
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
      printf 'H %s\nH crates/halvo-learn-gate/src/privacy.rs\n' "$script_path"
      if [ "$label" = flagged-before ]; then
        printf 'S crates/halvo-learn-gate/src/main.rs\n'
      fi
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
printf '%s\n' "$1" >>"$TREE_DIR/gate.calls"
if [ "$TREE_CHANGE_AT" = gate ]; then
  printf '%s\n' "$TREE_CHANGE_LINE" >>"$TREE_DIR/status"
fi
case $1:$3 in
  privacy:*) printf '%s\n' "$WANT_PRIVACY" ;;
  delete:workflow) printf 'delete what=workflow store=memory rows_left=0 other_rows=9 other_served=yes result=ok\n' ;;
  *) printf 'delete what=all store=memory exit=- files_left=- result=not-run\n' ;;
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
  WANT_PRIVACY=$want_privacy
  export TREE_DIR TREE_CHANGE_AT TREE_CHANGE_LINE TREE_BUILD WANT_PRIVACY
  mkdir -p "$TREE_DIR/repo/target/debug" "$TREE_DIR/art" "$TREE_DIR/tmp" "$work/no-cargo" || broken
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
    tmp_parent=$TREE_DIR/tmp
    made=
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
    changed-before | staged-before | name-not-shown | heldout-name-not-shown | flagged-before)
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
  changed="1 .M N... 100644 100644 100644 $oid $oid crates/halvo-learn-gate/src/privacy.rs"
  staged="1 M. N... 100644 100644 100644 $oid $oid crates/halvo-learn-gate/src/privacy.rs"
  later="1 .M N... 100644 100644 100644 $oid $oid crates/halvo-learn-gate/src/main.rs"
  word="1 .M N... 100644 100644 100644 $oid $oid crates/halvo-learn-gate/src/marrowbillet.rs"
  heldout="1 M. N... 100644 100644 100644 $oid $oid fixtures/r5/cases/heldout/quintelfern-moss.jsonl"
  own="1 .M N... 100644 100644 100644 $oid $oid $script_path"
  tree_passed=0
  tree_rejected=0
  tree_case changed-before FAIL-tree 'tree changed=before files=1' "$changed" "" "" ok yes
  tree_case staged-before FAIL-tree 'tree changed=before files=1' "$staged" "" "" ok yes
  tree_case name-not-shown FAIL-tree 'tree changed=before files=1' "$word" "" "" ok yes marrowbillet
  tree_case heldout-name-not-shown FAIL-tree 'tree changed=before files=1' "$heldout" "" "" ok yes quintelfern
  tree_case flagged-before FAIL-tree 'tree flagged=1' "" "" "" ok yes
  tree_case own-changed FAIL-own "FAIL $name (own)" "$own" "" "" ok yes
  tree_case untracked-before PASS "PASS $name" '? notes.txt' "" "" ok yes
  tree_case build-changes FAIL-tree 'tree changed=after-build' "" build "$later" ok yes
  tree_case play-changes FAIL-tree 'tree changed=after-gate' "" gate "$later" ok yes
  tree_case no-cargo FAIL-cargo "FAIL $name (cargo)" "" "" "" ok no
  tree_case build-fails FAIL-build "FAIL $name (build)" "" "" "" fails yes
}

self_test() {
  made=$(mktemp -d "${TMPDIR:-/tmp}/60-r5-privacy.XXXXXX") || broken
  work=$made
  fixture_gate >"$work/gate.sh" || broken
  gate_script=$work/gate.sh
  p="$want_privacy$nl"
  wf="delete what=workflow store=memory rows_left=0 other_rows=9 other_served=yes result=ok$nl"
  all="$want_all$nl"
  want_calls="privacy --data-dir DIR tmp=own
delete --what workflow --data-dir DIR --store memory
delete --what all --data-dir DIR --store memory"
  log_line='^(privacy check=learn_[a-z_]+ result=(ok|miss|not-run)|delete what=workflow store=[a-z]+ rows_left=[0-9]+ other_rows=[0-9]+ other_served=(yes|no) result=(ok|miss)|delete what=all store=[a-z]+ exit=([0-9]+|-) files_left=([0-9]+|-) result=(ok|miss|not-run)|(privacy|workflow|all)=unreadable)$'
  pl=privacy
  failing_fixtures=27
  passing_fixtures=2
  wrong=
  rejected=0
  passed=0
  deleted_miss=$(changed 's/learn_deleted result=ok/learn_deleted result=miss/') || broken
  outside_miss=$(changed 's/learn_outside result=ok/learn_outside result=miss/') || broken
  listed_ok=$(changed 's/learn_listed result=not-run/learn_listed result=ok/') || broken
  scrub_ok=$(changed 's/learn_scrub result=not-run/learn_scrub result=ok/') || broken
  delete_all_ok=$(changed 's/learn_delete_all result=not-run/learn_delete_all result=ok/') || broken
  expired_ok=$(changed 's/learn_expired result=not-run/learn_expired result=ok/') || broken
  deleted_not_run=$(changed 's/learn_deleted result=ok/learn_deleted result=not-run/') || broken
  outside_not_run=$(changed 's/learn_outside result=ok/learn_outside result=not-run/') || broken
  swapped=$(changed '1{h;d;};2{G;}') || broken
  five=$(changed "\$d") || broken
  marked=$(changed 's/learn_outside result=ok/learn_outside result=ok Zq7-halvo-marker-learn-a/') || broken
  self_case good "" "$p" "" 0 "$wf" "" 0 "$all" "" 0
  self_case good-other-rows-20 "" "$p" "" 0 "delete what=workflow store=memory rows_left=0 other_rows=20 other_served=yes result=ok$nl" "" 0 "$all" "" 0
  self_case deleted-miss privacy "$deleted_miss$nl" "" 1 "$wf" "" 0 "$all" "" 0
  self_case outside-miss privacy "$outside_miss$nl" "" 1 "$wf" "" 0 "$all" "" 0
  self_case listed-ok privacy "$listed_ok$nl" "" 0 "$wf" "" 0 "$all" "" 0
  self_case scrub-ok privacy "$scrub_ok$nl" "" 0 "$wf" "" 0 "$all" "" 0
  self_case delete-all-ok privacy "$delete_all_ok$nl" "" 0 "$wf" "" 0 "$all" "" 0
  self_case expired-ok privacy "$expired_ok$nl" "" 0 "$wf" "" 0 "$all" "" 0
  self_case deleted-not-run privacy "$deleted_not_run$nl" "" 0 "$wf" "" 0 "$all" "" 0
  self_case outside-not-run privacy "$outside_not_run$nl" "" 0 "$wf" "" 0 "$all" "" 0
  self_case lines-swapped privacy "$swapped$nl" "" 0 "$wf" "" 0 "$all" "" 0
  self_case five-lines privacy "$five$nl" "" 0 "$wf" "" 0 "$all" "" 0
  self_case a-line-twice privacy "$p$pl check=learn_outside result=ok$nl" "" 0 "$wf" "" 0 "$all" "" 0
  self_case last-line-unended privacy "$want_privacy" "" 0 "$wf" "" 0 "$all" "" 0
  self_case ok-with-exit-1 privacy "$p" "" 1 "$wf" "" 0 "$all" "" 0
  self_case privacy-standard-error privacy "$p" "a warning$nl" 0 "$wf" "" 0 "$all" "" 0
  self_case privacy-line-with-a-marker privacy "$marked$nl" "" 0 "$wf" "" 0 "$all" "" 0
  self_case privacy-refused refused "" "REFUSED store: the seed$nl" 2 "$wf" "" 0 "$all" "" 0
  self_case workflow-refused refused "$p" "" 0 "" "REFUSED usage: x$nl" 2 "$all" "" 0
  self_case all-refused refused "$p" "" 0 "$wf" "" 0 "" "REFUSED usage: x$nl" 2
  self_case workflow-rows-left delete "$p" "" 0 "delete what=workflow store=memory rows_left=1 other_rows=9 other_served=yes result=miss$nl" "" 1 "$all" "" 0
  self_case workflow-rows-left-read-as-ok delete "$p" "" 0 "delete what=workflow store=memory rows_left=1 other_rows=9 other_served=yes result=ok$nl" "" 0 "$all" "" 0
  self_case workflow-not-served-read-as-ok delete "$p" "" 0 "delete what=workflow store=memory rows_left=0 other_rows=9 other_served=no result=ok$nl" "" 0 "$all" "" 0
  self_case workflow-no-other-rows delete "$p" "" 0 "delete what=workflow store=memory rows_left=0 other_rows=0 other_served=yes result=ok$nl" "" 0 "$all" "" 0
  self_case workflow-on-derived delete "$p" "" 0 "delete what=workflow store=derived rows_left=0 other_rows=9 other_served=yes result=ok$nl" "" 0 "$all" "" 0
  self_case workflow-ok-with-exit-1 delete "$p" "" 0 "$wf" "" 1 "$all" "" 0
  self_case workflow-two-lines delete "$p" "" 0 "$wf$wf" "" 0 "$all" "" 0
  self_case all-ok all "$p" "" 0 "$wf" "" 0 "delete what=all store=memory exit=0 files_left=0 result=ok$nl" "" 0
  self_case all-standard-error all "$p" "" 0 "$wf" "" 0 "$all" "a warning$nl" 0
  tree_cases
  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  if [ "$tree_passed" -ne 1 ] || [ "$tree_rejected" -ne 10 ]; then
    printf 'FAIL self-test:%s (tree cases passed %s, not 1, and rejected %s, not 10)\n' "$name" "$tree_passed" "$tree_rejected"
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
