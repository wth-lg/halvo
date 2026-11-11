#!/bin/sh
set -u
DIR=$(cd "$(dirname "$0")" && pwd)
. "$DIR/lib.sh"
name=run

CHECKS="a:timed-run:here b:store:here b:deletion:here c:secure-input:here d:cost:here e:memory:here f:service-run:later g:service-memory:later h:service-expiry:later i:service-pause:later j:service-delete:later k:content-floor:later l:login-start:later m:app-memory:later n:ci-results:later -:windows-ci:here -:no-ll-hook:here"

check_of() {
  check_rest=${1#*:}
  entry_check=${check_rest%:*}
}

table_defect() {
  for entry in $CHECKS; do
    check_of "$entry"
    table_check=$entry_check
    if [ -f "$1/check-$table_check.sh" ]; then
      [ "${entry##*:}" = here ] || { printf 'check-%s.sh is in %s and the table marks it later: mark it here\n' "$table_check" "$1"; return 1; }
    else
      [ "${entry##*:}" = later ] || { printf 'check-%s.sh is missing from %s and the table marks it here\n' "$table_check" "$1"; return 1; }
    fi
  done
}

predicate_table() {
  table_defect "$1" > /dev/null
}

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

Sub-checks, as letter:name:state (a letter of the done-when, or - for a line
outside the letters). The state is here when check-<name>.sh is at this commit,
or later when a later unit adds it. --self-test fails when a script marked here
is missing or a script marked later is present. A name with no check script
prints FAIL <name> (check script missing).$checks_help
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

measure_all() {
  all_status=0
  for entry in $CHECKS; do
    check_of "$entry"
    run_check "$entry_check" --measure "$1" --input "$2" --sha "$3" ${strict:+"$strict"} ${remove:+"$remove"}
    all_status=$(worst_status "$all_status" "$?")
  done
  return "$all_status"
}

stub_checks() {
  stubs=$gate_scratch/stubs-$st_case
  mkdir -p "$stubs"
  for entry in $CHECKS; do
    check_of "$entry"
    cat > "$stubs/check-$entry_check.sh" <<'STUB'
#!/bin/sh
printf '%s\n' "$*" > "$0.args"
read -r verdict < "$0.verdict"
stub=${0##*/check-}
printf '%s %s\n' "$verdict" "${stub%.sh}"
case $verdict in PASS) exit 0 ;; NO-INPUT) exit 3 ;; esac
exit 1
STUB
    printf 'PASS\n' > "$stubs/check-$entry_check.sh.verdict"
  done
  for pair in "$@"; do
    printf '%s\n' "${pair#*=}" > "$stubs/check-${pair%%=*}.sh.verdict"
  done
  DIR=$stubs; strict=''; remove=''
}

all_checks_exit() {
  all_want=$1; shift
  stub_checks "$@"
  measure_all 5 operator 0123456789abcdef0123456789abcdef01234567 > /dev/null
  [ "$?" -eq "$all_want" ]
}

checks_get_their_arguments() {
  stub_checks
  strict=--strict
  measure_all 5 operator 0123456789abcdef0123456789abcdef01234567 > /dev/null
  read -r stub_args < "$stubs/check-memory.sh.args"
  [ "$stub_args" = "--measure 5 --input operator --sha 0123456789abcdef0123456789abcdef01234567 --strict" ]
}

copy_gate() {
  mkdir -p "$gate_scratch/$1" && cp "$DIR"/check-*.sh "$gate_scratch/$1/"
}

table_defect_is() {
  defect=$(table_defect "$gate_scratch/$2")
  [ "$defect" = "$1" ]
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

tree_cause_is() {
  cause=$(predicate_tree_lines "$2" "$3" "$4")
  [ "$cause" = "$1" ]
}

cases_tree() {
  good=$(printf 'halvo v0.0.0 (/repo/src-tauri)\nhalvo-capture v0.0.0 (/repo/crates/halvo-capture)\nrusqlite v0.41.1\nserde v1.0.228')
  banned="reqwest hyper ureq rustls ring aws-lc-rs native-tls openssl-sys"
  t tree-of-main PASS predicate_tree_lines "$good" "halvo-capture" "$banned"
  t capture-failed FAIL predicate_tree_lines "" "halvo-capture" "$banned"
  renamed=$(printf 'halvo v0.0.0\nhalvo-capture-core v0.0.0\nserde v1.0.228')
  t package-renamed FAIL predicate_tree_lines "$renamed" "halvo-capture" "$banned"
  t required-line-missing FAIL predicate_tree_lines "$good" "halvo-control halvo-capture" "$banned"
  with_openssl=$(printf '%s\nopenssl-sys v0.9.109' "$good")
  t forbidden-crate FAIL predicate_tree_lines "$with_openssl" "halvo-capture" "$banned"
  with_reqwest=$(printf '%s\nreqwest v0.13.5' "$good")
  t reqwest-line FAIL predicate_tree_lines "$with_reqwest" "halvo-capture" "$banned"
  t forbidden-name-inside-another-name PASS predicate_tree_lines "$good" "halvo-capture" "sqlite serde_json ring"
  t required-name-needs-its-own-line FAIL predicate_tree_lines "$good" "capture" ""
  t required-list-empty-and-nothing-forbidden PASS predicate_tree_lines "$good" "" ""
  t missing-line-is-named PASS tree_cause_is "required line missing: halvo-control v" "$good" "halvo-control" "$banned"
  t forbidden-line-is-named PASS tree_cause_is "forbidden crate line: reqwest v0.13.5" "$with_reqwest" "halvo-capture" "$banned"
}

cases() {
  case $name in
    helper) cases_helper ;;
    tree) cases_tree ;;
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
  t check-script-present PASS run_check no-ll-hook --self-test
  t table-of-this-commit PASS predicate_table "$DIR"
  copy_gate renamed && mv "$gate_scratch/renamed/check-store.sh" "$gate_scratch/renamed/check-store-old.sh"
  t check-script-renamed FAIL predicate_table "$gate_scratch/renamed"
  t check-script-renamed-is-named PASS table_defect_is "check-store.sh is missing from $gate_scratch/renamed and the table marks it here" renamed
  copy_gate early && printf '#!/bin/sh\n' > "$gate_scratch/early/check-service-run.sh"
  t later-script-present FAIL predicate_table "$gate_scratch/early"
  t later-script-present-is-named PASS table_defect_is "check-service-run.sh is in $gate_scratch/early and the table marks it later: mark it here" early
  t every-check-passing-exits-0 PASS all_checks_exit 0
  t a-no-input-line-exits-3 PASS all_checks_exit 3 cost=NO-INPUT timed-run=NO-INPUT
  t a-fail-beside-no-input-exits-1 PASS all_checks_exit 1 cost=NO-INPUT memory=FAIL
  t a-fail-of-the-first-check-exits-1 PASS all_checks_exit 1 timed-run=FAIL
  t a-fail-of-the-last-check-exits-1 PASS all_checks_exit 1 no-ll-hook=FAIL
  t checks-get-duration-input-sha-and-strict PASS checks_get_their_arguments
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
        run|helper|tree) name=$group; selftest_run; return ;;
        mutation) sh "$DIR/mutation-test.sh" --self-test ${verbose:+"$verbose"} ${gate_case:+--case "$gate_case"}; return ;;
        *) printf 'run.sh: --group takes run, helper, tree or mutation\n' >&2; return 64 ;;
      esac
    fi
    if ! table=$(table_defect "$DIR"); then
      emit_fail "self-test:run" "$table"
      return 1
    fi
    for entry in $CHECKS; do
      [ "${entry##*:}" = here ] || continue
      check_of "$entry"
      run_check "$entry_check" --self-test ${verbose:+"$verbose"} ${remove:+"$remove"} || rc=1
    done
    for name in run helper tree; do
      selftest_run || rc=1
    done
    sh "$DIR/mutation-test.sh" --self-test ${verbose:+"$verbose"} ${remove:+"$remove"} || rc=1
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
  measure_all "$duration" "$input" "$start_sha"
  rc=$?
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
