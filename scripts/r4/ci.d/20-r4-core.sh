#!/bin/sh
# jobs: windows
# rule: Every test of halvo-coord, and of halvo-coord-app once that crate is in the tree, passes with the committed lockfile, and every refusal test of I3.6, I4.8 and I5.11 that the crates hold ran and passed.
set -u
name=20-r4-core

cases='kind_reason kind_class kind_frame second_item items_budget ask_again check_again model_text
answer_twice answer_early answer_page_native timer_allow no_record digest_shown guard_unmatched
guard_digest guard_money guard_releasing_other guard_repeat guard_no_approval_confirm
guard_no_approval_run mixed_request changed_line
two_models two_runs_agent hold_start unrecorded_start second_slot_run auto_needs_person
over_budget queue_full outlives_app stop_tree deleted_workflow interrupted_no_retry
store_not_ended reap_alive after_failed_step7 after_failed_step8 slot_gone agents_unread
check_raised create_again active_tick setup_owed bind_stored gone_outlives_app setup_refused'

mode=measure
case $# in
  0) ;;
  1)
    case $1 in
      --self-test) mode=self_test ;;
      *) mode=usage ;;
    esac
    ;;
  *) mode=usage ;;
esac

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

case $mode in
  usage) fail usage ;;
  *) ;;
esac

case $0 in
  */*) here=${0%/*} ;;
  *) here=. ;;
esac
here=$(cd "$here" 2>/dev/null && pwd) || fail setup
repo=${CI_D_REPO:-}
if [ -z "$repo" ]; then
  case $here in
    *?/scripts/r4/ci.d) repo=${here%/scripts/r4/ci.d} ;;
    *) fail setup ;;
  esac
fi
artifacts=${CI_D_ARTIFACTS:-$repo/target/ci-artifacts/$name}

measure() {
  cd "$repo" 2>/dev/null || fail setup
  [ -d "$artifacts" ] || mkdir -p "$artifacts" 2>/dev/null || fail setup
  log=$artifacts/cargo-test.txt
  lines=$artifacts/cargo-test-lines.txt
  sources=$artifacts/refusal-sources.txt
  : >"$log" 2>/dev/null || fail setup
  app=no
  if [ -f crates/halvo-coord-app/Cargo.toml ]; then
    app=yes
    cargo test -p halvo-coord -p halvo-coord-app --locked >"$log" 2>&1
  else
    cargo test -p halvo-coord --locked >"$log" 2>&1
  fi
  code=$?
  printf 'cargo test exit=%s halvo-coord-app=%s\n' "$code" "$app"
  [ "$code" -eq 0 ] || fail cargo
  tr -d '\r' <"$log" >"$lines" || fail setup
  git -C "$repo" grep -h -o -E 'fn refusal_[a-z0-9_]+' -- crates/halvo-coord crates/halvo-coord-app >"$sources"
  code=$?
  case $code in
    0 | 1) ;;
    *) fail setup ;;
  esac
  listed=0
  held=0
  passed=0
  missing=0
  for c in $cases; do
    listed=$((listed + 1))
    in_source=no
    if grep -q -x "fn refusal_$c" "$sources"; then
      in_source=yes
      held=$((held + 1))
    fi
    if grep -q -x "test refusal_$c \.\.\. ok" "$lines"; then
      passed=$((passed + 1))
    elif [ "$in_source" = yes ]; then
      missing=$((missing + 1))
    fi
  done
  printf 'refusal cases listed=%s in_sources=%s passed=%s missing=%s\n' "$listed" "$held" "$passed" "$missing"
  printf 'refusals=%s\n' "$passed"
  printf 'refusals=%s\n' "$passed" >"$artifacts/result.txt" || fail setup
  [ "$missing" -eq 0 ] || fail missing
  printf 'PASS %s\n' "$name"
}

check() {
  label=$1
  want=$2
  text=$3
  shift 3
  out=$(
    PATH=$work/bin:$PATH
    export PATH STUB_OUT STUB_EXIT STUB_ARGS
    "$@" 2>&1
  )
  code=$?
  last=${out##*"
"}
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  case $out in
    *"$text"*) ;;
    *) got=$got-without-the-text ;;
  esac
  printf 'case %s/%s want=%s got=%s\n' "$name" "$label" "$want" "$got"
  if [ "$got" != "$want" ]; then
    wrong="$wrong $label"
  elif [ "$want" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
  STUB_EXIT=0
}

fixture() {
  dir=$work/$1
  mkdir -p "$dir/crates/halvo-coord/tests" || exit 2
  printf '#[test]\nfn refusal_two_models() {}\n#[test]\nfn refusal_queue_full() {}\n#[test]\nfn refusal_not_a_case() {}\n' >"$dir/crates/halvo-coord/tests/refusal.rs" || exit 2
  git -C "$dir" init -q >/dev/null 2>&1 || exit 2
  git -C "$dir" add . >/dev/null 2>&1 || exit 2
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
  trap 'rm -rf "$work"' EXIT
  trap 'rm -rf "$work"; exit 2' HUP INT TERM
  mkdir -p "$work/bin" || exit 2
  cat >"$work/bin/cargo" <<'EOF'
#!/bin/sh
printf '%s\n' "$*" >"$STUB_ARGS"
case " $* " in
  *' --locked '*) ;;
  *) exit 97 ;;
esac
if [ -f crates/halvo-coord-app/Cargo.toml ]; then
  [ "$*" = 'test -p halvo-coord -p halvo-coord-app --locked' ] || exit 96
else
  [ "$*" = 'test -p halvo-coord --locked' ] || exit 95
fi
cat "$STUB_OUT"
exit "$STUB_EXIT"
EOF
  chmod +x "$work/bin/cargo" || exit 2
  STUB_ARGS=$work/args
  STUB_EXIT=0
  rejected=0
  wrong=
  fixture one
  fixture app
  mkdir -p "$work/app/crates/halvo-coord-app" || exit 2
  : >"$work/app/crates/halvo-coord-app/Cargo.toml" || exit 2
  : >"$work/plain-file" || exit 2
  ok_both='test refusal_two_models ... ok'
  ok_queue='test refusal_queue_full ... ok'
  ok_other='test refusal_not_a_case ... ok'
  done_line='test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out'
  run_one() {
    CI_D_REPO=$work/one CI_D_ARTIFACTS=$work/artifacts-one CI_D_JOB=self-test sh "$here/$name.sh"
  }

  output pass "$ok_both" "$ok_queue" "$ok_other" "$done_line"
  check pass PASS 'refusals=2' run_one
  check app-crate-in-the-tree PASS 'halvo-coord-app=yes' \
    env CI_D_REPO="$work/app" CI_D_ARTIFACTS="$work/artifacts-app" sh "$here/$name.sh"
  check usage FAIL "FAIL $name (usage)" sh "$here/$name.sh" --bogus
  check setup FAIL "FAIL $name (setup)" \
    env CI_D_REPO="$work/one" CI_D_ARTIFACTS="$work/plain-file/artifacts" sh "$here/$name.sh"
  STUB_EXIT=101
  check cargo FAIL "FAIL $name (cargo)" run_one
  output ignored "$ok_both" 'test refusal_queue_full ... ignored' "$done_line"
  check a-refusal-test-that-did-not-run FAIL "FAIL $name (missing)" run_one
  output absent "$ok_queue" "$done_line"
  check a-refusal-test-that-cargo-did-not-list FAIL "FAIL $name (missing)" run_one
  output failed "$ok_both" 'test refusal_queue_full ... FAILED' "$done_line"
  STUB_EXIT=101
  check a-refusal-test-that-failed FAIL "FAIL $name (cargo)" run_one
  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case $mode in
  self_test) self_test ;;
  *) measure ;;
esac
