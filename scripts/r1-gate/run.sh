#!/bin/sh
set -u
DIR=$(cd "$(dirname "$0")" && pwd)
. "$DIR/lib.sh"
name=run

CHECKS="a:timed-run:here b:store:here b:deletion:here b:data-dir-scan:service-run c:secure-input:here d:cost:here e:memory:here f:service-run:here g:service-memory:here h:service-expiry:here i:service-pause:here j:service-delete:service-memory k:content-floor:here l:login-start:later m:app-memory:later n:ci-results:later -:windows-ci:here -:no-ll-hook:here"

check_of() {
  check_rest=${1#*:}
  entry_check=${check_rest%:*}
}

table_defect() {
  for entry in $CHECKS; do
    check_of "$entry"
    table_check=$entry_check
    table_state=${entry##*:}
    case $table_state in
      here|later) ;;
      *)
        [ ! -f "$1/check-$table_check.sh" ] || { printf 'check-%s.sh is in %s and the table has check-%s.sh print its line\n' "$table_check" "$1" "$table_state"; return 1; }
        case " $CHECKS " in *":$table_state:here "*) ;; *) printf 'the line %s is printed by check-%s.sh, which the table does not mark here\n' "$table_check" "$table_state"; return 1 ;; esac
        [ -f "$1/check-$table_state.sh" ] || { printf 'check-%s.sh, which prints the line %s, is missing from %s\n' "$table_state" "$table_check" "$1"; return 1; }
        continue ;;
    esac
    if [ -f "$1/check-$table_check.sh" ]; then
      [ "$table_state" = here ] || { printf 'check-%s.sh is in %s and the table marks it later: mark it here\n' "$table_check" "$1"; return 1; }
    else
      [ "$table_state" = later ] || { printf 'check-%s.sh is missing from %s and the table marks it here\n' "$table_check" "$1"; return 1; }
    fi
  done
}

lines_of() {
  printf '%s' "$1"
  for line_entry in $CHECKS; do
    [ "${line_entry##*:}" = "$1" ] || continue
    line_name=${line_entry#*:}
    printf ' %s' "${line_name%:*}"
  done
  printf '\n'
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
      a positive integer with no leading zero (default $GATE_DURATION, which
      GATE_DURATION sets and the same rule checks). --input says who gives keyboard and
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

measured_check() {
  check_out=$(run_check "$@")
  check_status=$?
  [ -z "$check_out" ] || printf '%s\n' "$check_out"
  case $check_status in
    0|1|3) ;;
    *) emit_fail "$1" "the check exited $check_status, and a check exits 0, 1 or 3"; return 1 ;;
  esac
  check_owned=$(lines_of "$1")
  check_foreign=$(printf '%s\n' "$check_out" | awk -v owned=" $check_owned " '($1 == "PASS" || $1 == "FAIL" || $1 == "NO-INPUT") && index(owned, " " $2 " ") == 0 { print $1, $2; exit }')
  if [ -n "$check_foreign" ]; then
    emit_fail "$1" "the check printed a line under a name that it does not own: $check_foreign"
    return 1
  fi
  check_worst=0
  for check_line in $check_owned; do
    check_found=$(printf '%s\n' "$check_out" | awk -v n="$check_line" '($1 == "PASS" || $1 == "FAIL" || $1 == "NO-INPUT") && $2 == n { c++; w = $1 } END { print c + 0, w }')
    check_count=${check_found%% *}
    check_word=${check_found#* }
    if [ "$check_count" != 1 ]; then
      emit_fail "$check_line" "the check exited $check_status and printed $check_count lines PASS $check_line, FAIL $check_line or NO-INPUT $check_line; it must print exactly one, and its word must match the exit status"
      return 1
    fi
    case $check_word in PASS) check_code=0 ;; NO-INPUT) check_code=3 ;; *) check_code=1 ;; esac
    check_worst=$(worst_status "$check_worst" "$check_code")
  done
  if [ "$check_worst" != "$check_status" ]; then
    check_said=$(printf '%s\n' "$check_out" | awk -v n="$1" '$2 == n && ($1 == "PASS" || $1 == "FAIL" || $1 == "NO-INPUT") { print $1; exit }')
    emit_fail "$1" "the check printed $check_said $1 and exited $check_status"
    return 1
  fi
  return "$check_status"
}

measure_all() {
  all_status=0
  for entry in $CHECKS; do
    case ${entry##*:} in here|later) ;; *) continue ;; esac
    check_of "$entry"
    measured_check "$entry_check" --measure "$1" --input "$2" --sha "$3" ${strict:+"$strict"} ${remove:+"$remove"}
    all_status=$(worst_status "$all_status" "$?")
  done
  return "$all_status"
}

stub_checks() {
  stubs=$gate_scratch/stubs-$st_case
  mkdir -p "$stubs"
  for entry in $CHECKS; do
    case ${entry##*:} in here|later) ;; *) continue ;; esac
    check_of "$entry"
    lines_of "$entry_check" > "$stubs/check-$entry_check.sh.lines"
    cat > "$stubs/check-$entry_check.sh" <<'STUB'
#!/bin/sh
printf '%s\n' "$*" > "$0.args"
read -r verdict < "$0.verdict"
stub=${0##*/check-}
stub=${stub%.sh}
case $verdict in
  SILENT) exit 0 ;;
  TWICE) printf 'PASS %s\nPASS %s\n' "$stub" "$stub"; exit 0 ;;
  MISMATCH) printf 'FAIL %s (a stub)\n' "$stub"; exit 0 ;;
  DIRTY) : > "$GATE_ROOT/changed-during-the-run"; printf 'PASS %s\n' "$stub"; exit 0 ;;
  FOREIGN) printf 'PASS %s\nFAIL cost (a stub)\n' "$stub"; exit 0 ;;
  ONLY) printf 'PASS %s\n' "$stub"; exit 0 ;;
esac
read -r lines < "$0.lines"
for line in $lines; do printf '%s %s\n' "$verdict" "$line"; done
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

measured_says() {
  measured_line=$1; shift
  stub_checks "$@"
  measured_out=$(measure_all 5 operator 0123456789abcdef0123456789abcdef01234567)
  printf '%s\n' "$measured_out" | grep -qxF -e "$measured_line"
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

bounds_blob() {
  git -C "$1" rev-parse --verify -q HEAD:docs/baseline/r1-gate-bounds.txt 2>/dev/null
}

bounds_field() {
  bounds_blob "$gate_scratch/$1" || printf 'none\n'
}

bounds_field_moves_and_tree_holds() {
  real_repository "$1" || return 1
  bounds_one=$(bounds_field "$1"); tree_one=$(tree_id "$gate_scratch/$1" HEAD)
  printf 'x=2\n' > "$gate_scratch/$1/docs/baseline/r1-gate-bounds.txt"
  commit_all "$gate_scratch/$1" || return 1
  bounds_two=$(bounds_field "$1"); tree_two=$(tree_id "$gate_scratch/$1" HEAD)
  [ -n "$bounds_one" ] && [ -n "$bounds_two" ] && [ "$bounds_one" != "$bounds_two" ] && [ "$tree_one" = "$tree_two" ]
}

run_without_bounds_says() {
  real_repository "$1" || return 1
  git -C "$gate_scratch/$1" update-index --force-remove docs/baseline/r1-gate-bounds.txt > /dev/null 2>&1 || return 1
  mv "$gate_scratch/$1/docs/baseline/r1-gate-bounds.txt" "$gate_scratch/$1.bounds" || return 1
  commit_all "$gate_scratch/$1" || return 1
  measured_run_says "FAIL run (HEAD holds no docs/baseline/r1-gate-bounds.txt, and every line reads its bounds from that file)" "$1"
}

row_applies() {
  row_file=$(mktemp "$gate_scratch/row.XXXXXX") || return 2
  printf 'one\n' > "$row_file"
  ROW_SUBST=$1 perl "$DIR/helper/apply-row.pl" "$row_file" > /dev/null
}

quoted_name_tree_holds() {
  quoted=$gate_scratch/quoted
  git init -q "$quoted" > /dev/null 2>&1 || return 1
  mkdir -p "$quoted/docs/baseline" || return 1
  printf 'one\n' > "$quoted/a.txt"
  printf 'record one\n' > "$quoted/docs/baseline/re$(printf '\303\251').txt"
  git -C "$quoted" add -A || return 1
  quoted_one=$(git -C "$quoted" write-tree) || return 1
  printf 'record two\n' > "$quoted/docs/baseline/re$(printf '\303\251').txt"
  git -C "$quoted" add -A || return 1
  quoted_two=$(git -C "$quoted" write-tree) || return 1
  quoted_id_one=$(tree_id "$quoted" "$quoted_one") || return 1
  quoted_id_two=$(tree_id "$quoted" "$quoted_two") || return 1
  [ "$quoted_id_one" = "$quoted_id_two" ]
}

commit_all() {
  git -C "$1" add -A || return 1
  commit_tree=$(git -C "$1" write-tree) || return 1
  commit_parent=$(git -C "$1" rev-parse HEAD) || return 1
  commit_new=$(printf 'tree %s\nparent %s\nauthor gate fixture <fixture@example.invalid> 2 +0000\ncommitter gate fixture <fixture@example.invalid> 2 +0000\n\nfixture\n' "$commit_tree" "$commit_parent" | git -C "$1" hash-object -t commit -w --stdin) || return 1
  git -C "$1" update-ref HEAD "$commit_new"
}

real_repository() {
  repo=$gate_scratch/$1
  git init -q "$repo" > /dev/null 2>&1 || return 1
  mkdir -p "$repo/docs/baseline" || return 1
  printf 'x=1\n' > "$repo/docs/baseline/r1-gate-bounds.txt"
  printf 'one\n' > "$repo/a.txt"
  git -C "$repo" add -A || return 1
  repo_tree=$(git -C "$repo" write-tree) || return 1
  repo_commit=$(printf 'tree %s\nauthor gate fixture <fixture@example.invalid> 1 +0000\ncommitter gate fixture <fixture@example.invalid> 1 +0000\n\nfixture\n' "$repo_tree" | git -C "$repo" hash-object -t commit -w --stdin) || return 1
  git -C "$repo" update-ref HEAD "$repo_commit"
}

measured_run_says() {
  run_line=$1; run_repo=$gate_scratch/$2; shift 2
  gate_dir=$DIR
  stub_checks "$@"
  cp "$gate_dir/run.sh" "$gate_dir/lib.sh" "$stubs/" || return 1
  GATE_ROOT=$run_repo sh "$stubs/run.sh" --duration 1 > "$stubs/run.out" 2> /dev/null
  run_status=$?
  [ "$run_status" -eq 1 ] || return 1
  grep -qF -e "$run_line" "$stubs/run.out"
}

dirty_run_is_refused() {
  measured_run_says "FAIL run (the work tree is dirty: commit first, or pass --allow-dirty for a run that is no done-when run)" "$1" || return 1
  ! grep -q '^PASS ' "$stubs/run.out"
}

head_state_is() {
  head_line=$(head_of "$gate_scratch/$2") || return 1
  [ "${head_line##* }" = "$1" ]
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

duration_from_the_environment() {
  GATE_DURATION=$1 sh "$DIR/run.sh" --help
}

standin_helper() {
  printf '#!/bin/sh\n%s\n' "$2" > "$gate_scratch/$1"
  chmod 755 "$gate_scratch/$1"
}

helper_starts() {
  helper_start 5 "$gate_scratch/$1" 10 || { printf '%s, after %s polls of 0.1 s\n' "$gate_defect" "$helper_polls"; return 1; }
  helper_stop
}

helper_defect_is() {
  helper_start 5 "$gate_scratch/$2" 10 && { printf 'the helper started after %s polls of 0.1 s\n' "$helper_polls"; return 1; }
  [ "$gate_defect" = "$1" ] && return 0
  printf 'the helper said: %s, after %s polls of 0.1 s\n' "$gate_defect" "$helper_polls"
  return 1
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
  real_repository clean-repo || { emit_fail "self-test:run" "git could not make the scratch repository"; return 1; }
  t clean-repository-is-clean PASS head_state_is clean clean-repo
  real_repository hidden-untracked && git -C "$gate_scratch/hidden-untracked" config status.showUntrackedFiles no && printf 'new\n' > "$gate_scratch/hidden-untracked/notes.txt"
  t untracked-file-under-show-untracked-files-no-is-dirty PASS head_state_is dirty hidden-untracked
  real_repository assumed && git -C "$gate_scratch/assumed" update-index --assume-unchanged a.txt && printf 'changed\n' > "$gate_scratch/assumed/a.txt"
  t assume-unchanged-entry-is-dirty PASS head_state_is dirty assumed
  real_repository skipped && git -C "$gate_scratch/skipped" update-index --skip-worktree a.txt
  t skip-worktree-entry-is-dirty PASS head_state_is dirty skipped
  real_repository excluded && printf 'notes.txt\n' >> "$gate_scratch/excluded/.git/info/exclude" && printf 'new\n' > "$gate_scratch/excluded/notes.txt"
  t untracked-file-in-info-exclude-is-dirty PASS head_state_is dirty excluded
  real_repository excludes-file && printf 'notes.txt\n' > "$gate_scratch/excludes-file.list" && git -C "$gate_scratch/excludes-file" config core.excludesFile "$gate_scratch/excludes-file.list" && printf 'new\n' > "$gate_scratch/excludes-file/notes.txt"
  t untracked-file-in-an-excludes-file-is-dirty PASS head_state_is dirty excludes-file
  real_repository mode-changed && git -C "$gate_scratch/mode-changed" config core.fileMode false && chmod 755 "$gate_scratch/mode-changed/a.txt"
  t mode-change-under-core-filemode-false-is-dirty PASS head_state_is dirty mode-changed
  mkdir -p "$gate_scratch/ignored" && printf 'notes.txt\n' > "$gate_scratch/ignored/.gitignore" && real_repository ignored && printf 'new\n' > "$gate_scratch/ignored/notes.txt"
  t file-that-a-tracked-gitignore-names-is-clean PASS head_state_is clean ignored
  real_repository untracked && printf 'new\n' > "$gate_scratch/untracked/notes.txt"
  t untracked-file-makes-the-head-dirty PASS head_state_is dirty untracked
  real_repository self-hidden && mkdir -p "$gate_scratch/self-hidden/new" && printf '*\n' > "$gate_scratch/self-hidden/new/.gitignore" && printf 'fn main() {}\n' > "$gate_scratch/self-hidden/new/code.rs"
  t untracked-directory-that-hides-itself-is-dirty PASS head_state_is dirty self-hidden
  t measured-run-on-a-dirty-tree-is-refused PASS dirty_run_is_refused untracked
  real_repository changing
  t measured-run-whose-tree-changes-fails PASS measured_run_says "FAIL run (the commit or the state of the work tree changed during the run: " changing timed-run=DIRTY
  real_repository steady
  t measured-run-on-a-steady-tree-has-no-change-line FAIL measured_run_says "FAIL run (the commit or the state of the work tree changed during the run: " steady memory=FAIL
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
  copy_gate early && printf '#!/bin/sh\n' > "$gate_scratch/early/check-login-start.sh"
  t later-script-present FAIL predicate_table "$gate_scratch/early"
  t later-script-present-is-named PASS table_defect_is "check-login-start.sh is in $gate_scratch/early and the table marks it later: mark it here" early
  t every-check-passing-exits-0 PASS all_checks_exit 0
  t a-no-input-line-exits-3 PASS all_checks_exit 3 cost=NO-INPUT timed-run=NO-INPUT
  t a-fail-beside-no-input-exits-1 PASS all_checks_exit 1 cost=NO-INPUT memory=FAIL
  t a-fail-of-the-first-check-exits-1 PASS all_checks_exit 1 timed-run=FAIL
  t a-fail-of-the-last-check-exits-1 PASS all_checks_exit 1 no-ll-hook=FAIL
  t checks-get-duration-input-sha-and-strict PASS checks_get_their_arguments
  t a-check-with-no-line-exits-1 PASS all_checks_exit 1 store=SILENT
  t a-check-with-no-line-is-named PASS measured_says "FAIL store (the check exited 0 and printed 0 lines PASS store, FAIL store or NO-INPUT store; it must print exactly one, and its word must match the exit status)" store=SILENT
  t a-check-with-two-lines-exits-1 PASS all_checks_exit 1 store=TWICE
  t a-check-with-two-lines-is-named PASS measured_says "FAIL store (the check exited 0 and printed 2 lines PASS store, FAIL store or NO-INPUT store; it must print exactly one, and its word must match the exit status)" store=TWICE
  t a-line-against-its-exit-status-exits-1 PASS all_checks_exit 1 store=MISMATCH
  t a-line-against-its-exit-status-is-named PASS measured_says "FAIL store (the check printed FAIL store and exited 0)" store=MISMATCH
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
  t duration-00 FAIL duration_accepted 00
  t duration-with-a-leading-zero FAIL duration_accepted 010
  t duration-from-the-environment PASS duration_from_the_environment 600
  t duration-from-the-environment-00 FAIL duration_from_the_environment 00
  t duration-from-the-environment-letters FAIL duration_from_the_environment abc
  t duration-from-the-environment-reaches-a-check FAIL env GATE_DURATION=00 sh "$DIR/check-windows-ci.sh" --self-test --case sha-empty
  t fail-under-another-name-exits-1 PASS all_checks_exit 1 store=FOREIGN
  t fail-under-another-name-is-named PASS measured_says "FAIL store (the check printed a line under a name that it does not own: FAIL cost)" store=FOREIGN
  t co-printed-line-missing-is-named PASS measured_says "FAIL data-dir-scan (the check exited 0 and printed 0 lines PASS data-dir-scan, FAIL data-dir-scan or NO-INPUT data-dir-scan; it must print exactly one, and its word must match the exit status)" service-run=ONLY
  t co-printed-line-fails-the-run PASS all_checks_exit 1 service-memory=ONLY
  copy_gate own-script && printf '#!/bin/sh\n' > "$gate_scratch/own-script/check-data-dir-scan.sh"
  t co-printed-line-with-a-script-of-its-own FAIL predicate_table "$gate_scratch/own-script"
  copy_gate printer-gone && mv "$gate_scratch/printer-gone/check-service-memory.sh" "$gate_scratch/printer-gone/service-memory.old"
  t co-printed-line-whose-printer-is-missing FAIL predicate_table "$gate_scratch/printer-gone"
  t bounds-field-moves-and-tree-id-holds PASS bounds_field_moves_and_tree_holds bounds-moved
  t bounds-file-missing-from-head-fails PASS run_without_bounds_says no-bounds
  t tree-id-holds-under-a-quoted-name PASS quoted_name_tree_holds
  t apply-row-refuses-the-e-modifier FAIL row_applies 's/one/"two"/e'
  t apply-row-applies-a-plain-row PASS row_applies 's/one/two/'
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
        predicate_duration "${1:-}" || { usage >&2; return 64; }
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
  start_bounds=$(bounds_blob "$GATE_ROOT") || start_bounds=none
  printf 'gate head=%s tree=%s bounds=%s state=%s input=%s machine=%s\n' "$start_sha" "$start_tree" "$start_bounds" "$start_state" "$input" "$machine"
  if [ "$start_state" = dirty ]; then
    if [ "$allow_dirty" = no ]; then
      emit_fail run "the work tree is dirty: commit first, or pass --allow-dirty for a run that is no done-when run"
      return 1
    fi
    export GATE_TREE_STATE=dirty
  fi
  if [ "$start_bounds" = none ]; then
    emit_fail run "HEAD holds no docs/baseline/r1-gate-bounds.txt, and every line reads its bounds from that file"
    return 1
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
