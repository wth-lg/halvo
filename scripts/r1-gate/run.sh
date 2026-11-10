#!/bin/sh
set -u
DIR=$(cd "$(dirname "$0")" && pwd)
. "$DIR/lib.sh"
name=run

CHECKS="a:timed-run b:store b:deletion c:secure-input d:cost e:memory f:service-run g:service-memory h:service-expiry i:service-pause j:service-delete k:content-floor l:login-start m:app-memory n:ci-results -:windows-ci -:no-ll-hook"

checks_help=''
for entry in $CHECKS; do
  checks_help="$checks_help
  $entry"
done

usage() {
  cat <<EOF
R1 gate driver. One line for each sub-check of the done-when.

  sh scripts/r1-gate/run.sh --self-test [-v] [--group NAME [--case CASE]]
      Prove the gate's own logic on fixtures and on stand-in programs. Needs no
      capture crate, cargo, gh or network. -v prints one line for each case:
      case <check>/<name> want=<PASS|FAIL|NO-INPUT> got=<PASS|FAIL|NO-INPUT>.
      --group runs one group of the driver's own cases and no check script.

  sh scripts/r1-gate/run.sh [--duration N] [--input none|operator] [--strict] [--allow-dirty] [--remove-scratch]
      Measure the built binary of HEAD. The first line names the commit, the
      tree id, the state of the work tree, the input and the machine. The tree
      id is the hash of the listing of HEAD without docs/baseline, so a squash
      and the commit that adds the record keep it:
        git ls-tree -r HEAD | awk -F '\t' 'index(\$2, "docs/baseline/") != 1' | git hash-object --stdin
      A dirty tree is refused unless --allow-dirty is given,
      and then every line says so. N is the length of each timed run in seconds,
      a positive integer (default $GATE_DURATION). --input says who gives keyboard and
      pointer input during the timed runs: none (the default) or operator, a
      person. With none the lines that need input print their figures and end
      NO-INPUT, never PASS. Exit status: 0 when every line is PASS, 1 on any
      FAIL, 3 on no FAIL and at least one NO-INPUT. --strict fails a cost or memory
      line on a machine under memory pressure or beside another halvo-capture.
      Scratch directories stay in place and their paths go to stderr, unless
      --remove-scratch is given.

Sub-checks, as letter:name (a letter of the done-when, or - for a line outside
the letters). A name with no check script prints FAIL <name> (check script missing).$checks_help
EOF
}

run_check() {
  if [ ! -f "$DIR/check-$1.sh" ]; then
    emit_fail "$1" "check script missing"
    return 1
  fi
  check_name=$1; shift
  sh "$DIR/check-$check_name.sh" "$@"
}

missing_script_line_is() {
  out=$(run_check no-such-check --self-test)
  [ "$out" = "$1" ]
}

scratch_trees() {
  trees=$gate_scratch/trees
  git init -q "$trees" > /dev/null 2>&1 || return 1
  mkdir -p "$trees/docs/baseline" "$trees/src"
  printf 'one\n' > "$trees/src/a.txt"
  printf 'record one\n' > "$trees/docs/baseline/record.txt"
  git -C "$trees" add -A || return 1
  tree_first=$(git -C "$trees" write-tree) || return 1
  printf 'record two\n' > "$trees/docs/baseline/record.txt"
  git -C "$trees" add -A || return 1
  tree_new_record=$(git -C "$trees" write-tree) || return 1
  printf 'two\n' > "$trees/src/a.txt"
  git -C "$trees" add -A || return 1
  tree_new_source=$(git -C "$trees" write-tree)
}

tree_ids() {
  id_one=$(tree_id "$trees" "$2") || return 1
  id_two=$(tree_id "$trees" "$3") || return 1
  case $1 in
    same) [ "$id_one" = "$id_two" ] ;;
    differ) [ "$id_one" != "$id_two" ] ;;
  esac
}

head_of() {
  GATE_ROOT=$1
  head_read
}

status_is() {
  status=$(worst_status "$2" "$3")
  [ "$status" = "$1" ]
}

input_accepted() {
  sh "$DIR/run.sh" --input "$1" --help
}

duration_accepted() {
  sh "$DIR/run.sh" --duration "$1" --help
}

standin_helper() {
  printf '#!/bin/sh\n%s\n' "$2" > "$gate_scratch/$1"
  chmod 755 "$gate_scratch/$1"
}

helper_starts() {
  helper_start 5 "$gate_scratch/$1" 10 || return 1
  helper_stop
}

helper_defect_is() {
  helper_start 5 "$gate_scratch/$2" 10 && return 1
  [ "$gate_defect" = "$1" ]
}

cases_helper() {
  standin_helper helper-good 'printf "helper visible pid=1\\n"; exec sleep 5'
  standin_helper helper-silent 'exec sleep 5'
  standin_helper helper-gone 'printf "window-helper: no screen\\n" >&2; exit 1'
  t starts-and-stops PASS helper_starts helper-good
  t missing FAIL helper_starts no-such-helper
  t ends-before-it-is-visible FAIL helper_starts helper-gone
  t never-visible FAIL helper_starts helper-silent
  t ended-is-named PASS helper_defect_is "the helper window ended before it was on screen: window-helper: no screen" helper-gone
  t not-in-time-is-named PASS helper_defect_is "the helper window was not on screen in time" helper-silent
}

cases() {
  case $name in
    helper) cases_helper ;;
    run) cases_run ;;
  esac
}

cases_run() {
  sha=0123456789abcdef0123456789abcdef01234567
  t run-unchanged PASS predicate_run_unchanged "$sha" clean "$sha" clean
  t sha-changed FAIL predicate_run_unchanged "$sha" clean 1123456789abcdef0123456789abcdef01234567 clean
  t tree-dirty FAIL predicate_run_unchanged "$sha" clean "$sha" dirty
  t sha-empty FAIL predicate_run_unchanged "" clean "" clean
  t sha-39-digits FAIL predicate_sha 0123456789abcdef0123456789abcdef0123456
  t sha-41-digits FAIL predicate_sha 0123456789abcdef0123456789abcdef012345678
  t sha-is-the-word-HEAD FAIL predicate_sha HEAD
  t sha-40-hex PASS predicate_sha "$sha"
  t porcelain-empty PASS predicate_tree_clean ""
  t porcelain-holds-a-change FAIL predicate_tree_clean " M scripts/r1-gate/run.sh"
  t porcelain-holds-an-untracked-file FAIL predicate_tree_clean "?? notes.txt"
  t head-of-no-repository FAIL head_of "$gate_scratch"
  scratch_trees || { emit_fail "self-test:run" "git could not write the scratch trees"; return 1; }
  t tree-id-moves-with-a-source-file PASS tree_ids differ "$tree_new_record" "$tree_new_source"
  t tree-id-holds-under-docs-baseline PASS tree_ids same "$tree_first" "$tree_new_record"
  t tree-id-of-no-tree FAIL tree_id "$trees" no-such-tree
  t check-script-missing FAIL run_check no-such-check --self-test
  t check-script-missing-is-named PASS missing_script_line_is "FAIL no-such-check (check script missing)"
  t check-script-present PASS run_check windows-ci --self-test
  t exit-0-when-every-line-passes PASS status_is 0 0 0
  t exit-3-on-a-no-input-line PASS status_is 3 0 3
  t exit-3-stays-after-a-pass PASS status_is 3 3 0
  t exit-1-on-a-fail PASS status_is 1 0 1
  t exit-1-beats-no-input PASS status_is 1 3 1
  t exit-1-stays-after-no-input PASS status_is 1 1 3
  t exit-1-stays-after-a-pass PASS status_is 1 1 0
  t exit-1-on-an-unknown-status PASS status_is 1 0 64
  t input-none PASS input_accepted none
  t input-operator PASS input_accepted operator
  t input-scripted-arrives-later FAIL input_accepted scripted
  t input-empty FAIL input_accepted ""
  t duration-600 PASS duration_accepted 600
  t duration-0 FAIL duration_accepted 0
  t duration-letters FAIL duration_accepted abc
  t duration-empty FAIL duration_accepted ""
  t duration-negative FAIL duration_accepted -5
}


main() {
  mode=measure; verbose=''; strict=''; remove=''; allow_dirty=no; input=none; group=''; duration=$GATE_DURATION
  while [ $# -gt 0 ]; do
    case "$1" in
      --self-test) mode=self-test ;;
      -v) verbose=-v ;;
      --strict) strict=--strict ;;
      --remove-scratch) remove=--remove-scratch ;;
      --allow-dirty) allow_dirty=yes ;;
      --group) shift; group=${1:-} ;;
      --case) shift; gate_case=${1:-} ;;
      --input)
        shift
        case ${1:-} in
          none|operator) input=$1 ;;
          *) usage >&2; return 64 ;;
        esac ;;
      --duration)
        shift
        case ${1:-} in
          ''|*[!0-9]*|0) usage >&2; return 64 ;;
        esac
        duration=$1 ;;
      -h|--help) usage; return 0 ;;
      *) printf 'run.sh: unknown argument: %s\n' "$1" >&2; usage >&2; return 64 ;;
    esac
    shift
  done
  rc=0
  if [ "$mode" = self-test ]; then
    gate_verbose=${verbose:+yes}
    name=run
    scratch_root
    gate_traps
    [ -z "$remove" ] || gate_remove_scratch=yes
    if [ -n "$group" ]; then
      case $group in
        run|helper) name=$group; selftest_run; return ;;
        *) printf 'run.sh: --group takes run or helper\n' >&2; return 64 ;;
      esac
    fi
    for entry in $CHECKS; do
      check=${entry#*:}
      [ -f "$DIR/check-$check.sh" ] || continue
      run_check "$check" --self-test ${verbose:+"$verbose"} ${remove:+"$remove"} || rc=1
    done
    for name in run helper; do
      selftest_run || rc=1
    done
    return "$rc"
  fi
  start=$(head_read) || { emit_fail run "HEAD of $GATE_ROOT is not readable: the gate measures a commit"; return 1; }
  read -r start_sha start_tree start_state <<HEAD
$start
HEAD
  machine=$(machine_kind)
  printf 'gate head=%s tree=%s state=%s input=%s machine=%s\n' "$start_sha" "$start_tree" "$start_state" "$input" "$machine"
  if [ "$start_state" = dirty ]; then
    if [ "$allow_dirty" = no ]; then
      emit_fail run "the work tree is dirty: commit first, or pass --allow-dirty for a run that is no done-when run"
      return 1
    fi
    export GATE_TREE_STATE=dirty
  fi
  for entry in $CHECKS; do
    run_check "${entry#*:}" --measure "$duration" --input "$input" --sha "$start_sha" ${strict:+"$strict"} ${remove:+"$remove"}
    rc=$(worst_status "$rc" "$?")
  done
  printf 'OPEN second-account-read (owner step; no test bed)\n'
  end=$(head_read) || end="unreadable unreadable unreadable"
  read -r end_sha end_tree end_state <<HEAD
$end
HEAD
  if ! predicate_run_unchanged "$start_sha" "$start_state" "$end_sha" "$end_state"; then
    emit_fail run "the commit or the state of the work tree changed during the run: head $start_sha to $end_sha, tree $start_tree to $end_tree, state $start_state to $end_state"
    rc=1
  fi
  return "$rc"
}

main "$@"
