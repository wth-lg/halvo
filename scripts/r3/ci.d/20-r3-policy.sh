#!/bin/sh
# jobs: macos windows
# rule: agent-config presets prints ten valid presets and none unattended, agent-gate configs prints one line stored=yes state=approved agent=set result=valid per seed category in order and configs seeds=10 valid=10, each test of this script's list (policy_table, own_kind_roles and the refusal tests of A5.4 in halvo-policy, halvo-agent-store and halvo-schema) prints ok in cargo test --locked, and no tracked file differs from HEAD before or after a cargo command.
set -u
LC_ALL=C
export LC_ALL
name=20-r3-policy
crates='halvo-policy halvo-agent-store halvo-schema'
categories='utility_bill_payment tenant_rent lease_renewal_or_change tenant_application_review email spreadsheet_fill shop_order calendar_cleanup ride_order daily_chart_check'
want_presets='presets seeds=10 valid=10 unattended=0'

finish() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

test_list() {
  cat <<'EOF'
halvo-policy policy_table policy_table
halvo-policy refusal refusal_no_approval
halvo-policy refusal refusal_digest
halvo-policy refusal refusal_reuse
halvo-policy refusal refusal_retry_unknown
halvo-policy refusal refusal_observe_effect
halvo-policy refusal refusal_allow_split
halvo-policy refusal refusal_no_checkpoint
halvo-policy refusal refusal_unattended_default
halvo-policy refusal refusal_changed_after_approval
halvo-policy refusal refusal_begin_after_end
halvo-agent-store refusal refusal_confirm_as_run
halvo-agent-store refusal refusal_propose_after_end
halvo-agent-store refusal refusal_end_run_loses
halvo-agent-store refusal refusal_repeat_begin
halvo-agent-store memory own_kind_roles
halvo-schema r3_config refusal_allow_money
EOF
}

tree_state() {
  git -c core.fsmonitor=false -c core.untrackedCache=false status --porcelain=v1 --untracked-files=no >"$1" 2>&1 ||
    printf 'status failed\n' >>"$1"
  git ls-files -v >"$1.flags" 2>&1 || printf 'ls-files failed\n' >>"$1"
  awk '$1 != "H" { print "flag " $1 }' "$1.flags" >>"$1" || printf 'awk failed\n' >>"$1"
}

tree_check() {
  tree_state "$art/tree-$1.txt"
  if [ -s "$art/tree-$1.txt" ]; then
    lines=$(awk 'END { print NR }' "$art/tree-$1.txt") || lines=unreadable
    printf 'policy tree=%s differs=%s\n' "$1" "$lines"
    finish tree
  fi
}

read_presets() {
  awk -v want="$want_presets" '
    { sub(/\r$/, "") }
    /^preset / { n++; if ($0 ~ / result=valid$/ && $0 !~ / autonomy=unattended /) good++ }
    { last = $0 }
    END { exit !(n == 10 && good == 10 && last == want) }
  ' "$1"
}

read_configs() {
  awk -v categories="$categories" '
    BEGIN { n = split(categories, category, " ") }
    { sub(/\r$/, "") }
    /^config / {
      lines++
      if (lines <= n && $0 == "config category=" category[lines] " stored=yes state=approved agent=set result=valid") good++
      next
    }
    /^configs / { counts++; last = $0; next }
    { other++ }
    END {
      ok = lines == n && good == n && counts == 1 && other == 0 &&
        (last == "configs seeds=10 valid=10 store=memory" || last == "configs seeds=10 valid=10 store=derived")
      exit !ok
    }
  ' "$1"
}

show_configs() {
  awk '
    { sub(/\r$/, "") }
    /^configs? [a-z0-9_= ]+$/ { print; next }
    { other++ }
    END { if (other) print "policy configs other_lines=" other }
  ' "$1"
}

read_results() {
  awk -v names="$1" '
    BEGIN {
      n = split(names, list, " ")
      for (i = 1; i <= n; i++) state[list[i]] = "absent"
    }
    {
      sub(/\r$/, "")
      if ($0 !~ /^test [^ ]+ \.\.\. /) next
      if (!($2 in state)) next
      result = substr($0, index($0, " ... ") + 5)
      if (result != "ok") state[$2] = "failed"
      else if (state[$2] == "absent") state[$2] = "ok"
      else if (state[$2] == "ok") state[$2] = "twice"
    }
    END { for (i = 1; i <= n; i++) print list[i], state[list[i]] }
  ' "$2"
}

run_crate() {
  crate=$1
  targets=
  names=
  while IFS=' ' read -r c target test; do
    [ "$c" = "$crate" ] || continue
    case " $targets " in
      *" $target "*) ;;
      *) targets="$targets $target" ;;
    esac
    names="$names $test"
  done <"$art/list.txt"
  set --
  for target in $targets; do
    set -- "$@" --test "$target"
  done
  set -- "$@" --
  for test in $names; do
    set -- "$@" "$test"
  done
  cargo test --locked -p "$crate" "$@" --exact >"$art/test-$crate.txt" 2>&1 </dev/null
  code=$?
  [ "$code" -eq 0 ] || cargo_failed=yes
  tree_check "after-test-$crate"
  read_results "${names# }" "$art/test-$crate.txt" >>"$art/results.txt" || finish read
}

measure() {
  art=$1
  mkdir -p "$art" || finish artifacts
  tree_check before
  test_list >"$art/list.txt" || finish list

  cargo run --locked -p halvo-policy --bin agent-config -- presets >"$art/presets.txt" 2>"$art/presets.err" </dev/null
  presets_code=$?
  tree_check after-presets
  presets=miss
  if [ "$presets_code" -eq 0 ] && read_presets "$art/presets.txt"; then
    presets=ok
  fi

  cargo run --locked -p halvo-agent-gate --bin agent-gate -- configs >"$art/configs.txt" 2>"$art/configs.err" </dev/null
  configs_code=$?
  tree_check after-configs
  show_configs "$art/configs.txt" || finish read
  configs=miss
  if [ "$configs_code" -eq 0 ] && read_configs "$art/configs.txt"; then
    configs=ok
  fi

  : >"$art/results.txt" || finish artifacts
  cargo_failed=no
  for crate in $crates; do
    run_crate "$crate"
  done

  total=0
  ok=0
  miss=0
  while IFS=' ' read -r test state; do
    total=$((total + 1))
    if [ "$state" = ok ]; then
      ok=$((ok + 1))
    else
      miss=$((miss + 1))
      printf 'policy test=%s state=%s\n' "$test" "$state"
    fi
  done <"$art/results.txt"
  listed=$(awk 'END { print NR }' "$art/list.txt") || finish read
  printf 'policy presets=%s configs=%s tests=%s ok=%s miss=%s\n' "$presets" "$configs" "$total" "$ok" "$miss"
  [ "$total" -eq "$listed" ] || finish read
  [ "$presets" = ok ] || finish presets
  [ "$configs" = ok ] || finish configs
  [ "$miss" -eq 0 ] || finish miss
  [ "$cargo_failed" = no ] || finish cargo
  printf 'PASS %s\n' "$name"
}

stub_presets() {
  for category in $categories; do
    result=valid
    [ "$category" = email ] && result=$fx_preset_email
    printf 'preset category=%s runtime=web autonomy=draft_and_confirm result=%s%s\n' "$category" "$result" "$eol"
  done
  printf '%s%s\n' "$fx_presets_last" "$eol"
  return "$fx_presets_code"
}

stub_configs() {
  i=0
  for category in $categories; do
    i=$((i + 1))
    [ "$i" -eq "$fx_configs_drop" ] && continue
    shown=$category
    if [ "$i" -eq "$fx_configs_swap" ]; then
      shown=tenant_rent
    elif [ "$i" -eq $((fx_configs_swap + 1)) ]; then
      shown=utility_bill_payment
    fi
    stored=yes
    [ "$i" -eq "$fx_configs_unstored" ] && stored=no
    printf 'config category=%s stored=%s state=approved agent=set result=valid%s\n' "$shown" "$stored" "$eol"
  done
  printf '%s%s\n' "$fx_configs_last" "$eol"
  return "$fx_configs_code"
}

stub_test() {
  crate=$1
  shift
  printf '   Compiling %s v0.0.0%s\n' "$crate" "$eol"
  printf 'running 3 tests%s\n' "$eol"
  printf 'test %s_other ... ok%s\n' "$crate" "$eol"
  code=0
  past=no
  for arg in "$@"; do
    if [ "$past" = no ]; then
      [ "$arg" = -- ] && past=yes
      continue
    fi
    [ "$arg" = --exact ] && continue
    test=$arg
    case " $fx_missing " in
      *" $test "*) continue ;;
    esac
    result=ok
    case " $fx_failed " in
      *" $test "*)
        result=FAILED
        code=101
        ;;
    esac
    case " $fx_ignored " in
      *" $test "*) result=ignored ;;
    esac
    case " $fx_renamed " in
      *" $test "*) test=${test}_rows ;;
    esac
    case " $fx_prefixed " in
      *" $test "*) test=tests::$test ;;
    esac
    case " $fx_twice " in
      *" $test "*)
        printf 'test %s ... ok%s\n' "$test" "$eol"
        result=FAILED
        code=101
        ;;
    esac
    case " $fx_ok_twice " in
      *" $test "*) printf 'test %s ... ok%s\n' "$test" "$eol" ;;
    esac
    printf 'test %s ... %s%s\n' "$test" "$result" "$eol"
  done
  case " $fx_cargo_fails " in
    *" $crate "*) code=101 ;;
  esac
  [ -n "$fx_dirty_after" ] && [ "$fx_dirty_after" = "$crate" ] && dirty_now=yes
  return "$code"
}

stub_cargo() {
  case $* in
    'run --locked -p halvo-policy --bin agent-config -- presets') stub_presets ;;
    'run --locked -p halvo-agent-gate --bin agent-gate -- configs')
      [ "$fx_dirty_after" = configs ] && dirty_now=yes
      stub_configs
      ;;
    'test --locked -p '*)
      shift 3
      stub_test "$@"
      ;;
    *) return 97 ;;
  esac
}

stub_tree_state() {
  : >"$1"
  [ "$dirty_now" = yes ] && printf ' M crates/halvo-policy/src/lib.rs\n' >"$1"
  return 0
}

fixture() {
  case_name=$1
  eol=
  dirty_now=no
  fx_dirty_after=
  fx_preset_email=valid
  fx_presets_last=$want_presets
  fx_presets_code=0
  fx_configs_drop=0
  fx_configs_swap=0
  fx_configs_unstored=0
  fx_configs_last='configs seeds=10 valid=10 store=memory'
  fx_configs_code=0
  fx_missing=
  fx_failed=
  fx_ignored=
  fx_renamed=
  fx_prefixed=
  fx_twice=
  fx_ok_twice=
  fx_cargo_fails=
}

check() {
  want=$1
  text=$2
  out=$(
    cargo() {
      stub_cargo "$@"
    }
    tree_state() {
      stub_tree_state "$@"
    }
    measure "$self_art/$case_name"
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

self_test() {
  self_art=$art/self-test
  rejected=0
  wrong=

  fixture all-ok
  check PASS 'policy presets=ok configs=ok tests=17 ok=17 miss=0'

  fixture carriage-returns
  eol=$cr
  check PASS 'policy presets=ok configs=ok tests=17 ok=17 miss=0'

  fixture derived-store
  fx_configs_last='configs seeds=10 valid=10 store=derived'
  check PASS 'configs seeds=10 valid=10 store=derived'

  fixture preset-not-valid
  fx_preset_email=allow
  check FAIL-presets 'policy presets=miss configs=ok'

  fixture presets-count-nine
  fx_presets_last='presets seeds=10 valid=9 unattended=0'
  check FAIL-presets 'policy presets=miss configs=ok'

  fixture presets-unattended
  fx_presets_last='presets seeds=10 valid=10 unattended=1'
  check FAIL-presets 'policy presets=miss configs=ok'

  fixture presets-exit-1
  fx_presets_code=1
  check FAIL-presets 'policy presets=miss configs=ok'

  fixture configs-not-stored
  fx_configs_unstored=5
  check FAIL-configs 'config category=email stored=no state=approved agent=set result=valid'

  fixture configs-line-missing
  fx_configs_drop=10
  check FAIL-configs 'policy presets=ok configs=miss'

  fixture configs-order
  fx_configs_swap=1
  check FAIL-configs 'policy presets=ok configs=miss'

  fixture configs-count-nine
  fx_configs_last='configs seeds=10 valid=9 store=memory'
  check FAIL-configs 'policy presets=ok configs=miss'

  fixture configs-other-store
  fx_configs_last='configs seeds=10 valid=10 store=owner'
  check FAIL-configs 'policy presets=ok configs=miss'

  fixture configs-exit-1
  fx_configs_code=1
  check FAIL-configs 'policy presets=ok configs=miss'

  fixture test-failed
  fx_failed=refusal_end_run_loses
  check FAIL-miss 'policy test=refusal_end_run_loses state=failed'

  fixture test-ignored
  fx_ignored=policy_table
  check FAIL-miss 'policy test=policy_table state=failed'

  fixture test-missing
  fx_missing=refusal_allow_money
  check FAIL-miss 'policy test=refusal_allow_money state=absent'

  fixture test-twice
  fx_twice=own_kind_roles
  check FAIL-miss 'policy test=own_kind_roles state=failed'

  fixture test-ok-twice
  fx_ok_twice=refusal_digest
  check FAIL-miss 'policy test=refusal_digest state=twice'

  fixture longer-name
  fx_renamed=refusal_digest
  check FAIL-miss 'policy test=refusal_digest state=absent'

  fixture module-path
  fx_prefixed=refusal_reuse
  check FAIL-miss 'policy test=refusal_reuse state=absent'

  fixture cargo-exit-without-a-failed-line
  fx_cargo_fails=halvo-agent-store
  check FAIL-cargo 'policy presets=ok configs=ok tests=17 ok=17 miss=0'

  fixture tree-dirty-before
  dirty_now=yes
  check FAIL-tree 'policy tree=before differs=1'

  fixture tree-changed-by-configs
  fx_dirty_after=configs
  check FAIL-tree 'policy tree=after-configs differs=1'

  fixture tree-changed-by-a-test
  fx_dirty_after=halvo-agent-store
  check FAIL-tree 'policy tree=after-test-halvo-agent-store differs=1'

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

lfx=$(printf '\nx')
nl=${lfx%x}
crx=$(printf '\rx')
cr=${crx%x}
repo=${CI_D_REPO:-}
if [ -z "$repo" ]; then
  repo=$(git rev-parse --show-toplevel) || exit 2
fi
cd "$repo" || exit 2
art=${CI_D_ARTIFACTS:-$repo/target/ci-artifacts/$name}
case ${1:-} in
  --self-test)
    [ $# -eq 1 ] || exit 64
    self_test
    exit 0
    ;;
  '')
    [ $# -eq 0 ] || exit 64
    measure "$art"
    exit 0
    ;;
  *) exit 64 ;;
esac
