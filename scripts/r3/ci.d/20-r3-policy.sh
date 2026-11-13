#!/bin/sh
# jobs: macos windows
# rule: The check builds the crates of its second list (halvo-policy, halvo-agent-store and halvo-agent-gate) with all their test targets and the target r3_config of halvo-schema, refuses a second build call that compiles anything, and starts every binary itself from the paths in cargo's messages: agent-config presets prints ten valid presets and none unattended; agent-gate configs prints one passing line (stored=yes state=approved agent=set result=valid) per seed category in order and configs seeds=10 valid=10; each of the 19 tests of its first list stands in the list of its own target's binary, is not ignored, and passes in a run of that name alone with --exact; each of the 26 test targets of its second list has exactly one test binary, and every test binary of those crates passes whole; and no tracked file differs from HEAD before or after a cargo command or a run. It does not hold against code of the job that writes a test binary or a tool on the PATH before the check reads them, a test that keeps its name with another body, a required target with its test harness switched off, code that runs before main in a required test file, or a compiler wrapper or a cargo configuration that changes what is built (ruling S-53), nor against a test that stands in no list and is removed from its target, which no list holds without naming every test.
set -u
LC_ALL=C
export LC_ALL
name=20-r3-policy
list_count=19
target_count=26
whole=
crates='halvo-policy halvo-agent-store halvo-agent-gate halvo-schema'
categories='utility_bill_payment tenant_rent lease_renewal_or_change tenant_application_review email spreadsheet_fill shop_order calendar_cleanup ride_order daily_chart_check'
want_presets='presets seeds=10 valid=10 unattended=0'
tab=$(printf '\t')

finish() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

test_list() {
  cat <<'EOF'
halvo-policy tests/policy_table.rs policy_table
halvo-policy tests/refusal.rs refusal_no_approval
halvo-policy tests/refusal.rs refusal_digest
halvo-policy tests/refusal.rs refusal_reuse
halvo-policy tests/refusal.rs refusal_retry_unknown
halvo-policy tests/refusal.rs refusal_observe_effect
halvo-policy tests/refusal.rs refusal_allow_split
halvo-policy tests/refusal.rs refusal_no_checkpoint
halvo-policy tests/refusal.rs refusal_unattended_default
halvo-policy tests/refusal.rs refusal_changed_after_approval
halvo-policy tests/refusal.rs refusal_begin_after_end
halvo-agent-store tests/refusal.rs refusal_confirm_as_run
halvo-agent-store tests/refusal.rs refusal_propose_after_end
halvo-agent-store tests/refusal.rs refusal_end_run_loses
halvo-agent-store tests/refusal.rs refusal_repeat_begin
halvo-agent-store tests/memory.rs own_kind_roles
halvo-agent-store tests/coordinator.rs create_agent_stores_the_preset_and_makes_the_state_call
halvo-agent-gate tests/bind.rs every_runner_body_of_an_ended_run_is_refused
halvo-schema tests/r3_config.rs refusal_allow_money
EOF
}

target_want() {
  printf '%s\n' "$target_count"
}

target_list() {
  cat <<'EOF'
halvo-policy src/lib.rs
halvo-policy src/bin/agent-config.rs
halvo-policy tests/admit.rs
halvo-policy tests/agent_config.rs
halvo-policy tests/class.rs
halvo-policy tests/config.rs
halvo-policy tests/decide.rs
halvo-policy tests/digest.rs
halvo-policy tests/effect.rs
halvo-policy tests/manifest.rs
halvo-policy tests/policy_table.rs
halvo-policy tests/refusal.rs
halvo-agent-store src/lib.rs
halvo-agent-store tests/after_end.rs
halvo-agent-store tests/bindings.rs
halvo-agent-store tests/checkpoint_bodies.rs
halvo-agent-store tests/coordinator.rs
halvo-agent-store tests/effect.rs
halvo-agent-store tests/end.rs
halvo-agent-store tests/memory.rs
halvo-agent-store tests/refusal.rs
halvo-agent-store tests/runner.rs
halvo-agent-gate src/lib.rs
halvo-agent-gate src/bin/agent-gate.rs
halvo-agent-gate tests/bind.rs
halvo-agent-gate tests/configs.rs
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

check_list() {
  awk -v want="$list_count" -v crates="$crates" '
    BEGIN { n = split(crates, c, " "); for (i = 1; i <= n; i++) known[c[i]] = 1 }
    { sub(/\r$/, "") }
    NF == 0 { next }
    {
      lines++
      if (NF != 3 || !($1 in known) || $2 !~ /^(src\/lib\.rs|tests\/[a-z0-9_]+\.rs)$/ || $3 !~ /^[a-z0-9_]+$/) bad = bad " form:" lines
      if ($3 in named) bad = bad " twice:" $3
      named[$3] = 1
    }
    END {
      if (lines != want) bad = bad " count:" lines
      if (bad != "") { print "list" bad; exit 1 }
      print "list lines=" lines
    }
  ' "$1"
}

check_targets() {
  awk -v want="$targets_want" -v crates="$crates" '
    BEGIN { n = split(crates, c, " "); for (i = 1; i <= n; i++) known[c[i]] = 1 }
    { sub(/\r$/, "") }
    NF == 0 { next }
    {
      lines++
      if (NF != 2 || !($1 in known) || $2 !~ /^(src\/lib\.rs|src\/bin\/[a-z0-9_-]+\.rs|tests\/[a-z0-9_]+\.rs)$/) bad = bad " form:" lines
      if (($1 "/" $2) in seen) bad = bad " twice:" $1 "/" $2
      seen[$1 "/" $2] = 1
    }
    END {
      if (lines != want) bad = bad " count:" lines
      if (bad != "") { print "targets" bad; exit 1 }
      print "targets lines=" lines
    }
  ' "$1"
}

read_messages() {
  awk -v crate="$2" '
    function field(key,    v) {
      if (!match($0, "\"" key "\":\"[^\"]*\"")) return ""
      v = substr($0, RSTART + length(key) + 4, RLENGTH - length(key) - 5)
      gsub(/\\\\/, "/", v)
      return v
    }
    index($0, "{\"reason\":\"compiler-artifact\",") != 1 { next }
    {
      dir = field("manifest_path")
      sub(/\/[^\/]*$/, "", dir)
      if (stale == "" && index($0, "\"fresh\":true") == 0) {
        owner = dir
        sub(/.*\//, "", owner)
        stale = field("name") "@" owner
      }
      if (dir !~ ("/crates/" crate "$")) next
      exe = field("executable")
      if (exe == "") next
      src = field("src_path")
      if (substr(src, 1, length(dir) + 1) != dir "/") next
      src = substr(src, length(dir) + 2)
      profile = ""
      if (match($0, /"profile":\{[^}]*\}/)) profile = substr($0, RSTART, RLENGTH)
      if (index(profile, "\"test\":true")) kind = "test"
      else if (index($0, "\"kind\":[\"bin\"]")) kind = "bin"
      else next
      out[++n] = kind "\t" src "\t" field("name") "\t" exe
    }
    END {
      if (stale != "") { print "stale\t" stale; exit }
      for (i = 1; i <= n; i++) print out[i]
    }
  ' "$1"
}

binary_of() {
  awk -F "$tab" -v kind="$2" -v key="$3" -v col="$4" '
    $1 == kind && $col == key { n++; exe = $4 }
    END { if (n == 1) print exe; else { print "found=" (n + 0); exit 1 } }
  ' "$1"
}

crate_args() {
  case " $whole " in
    *" $1 "*)
      printf '%s\n' --tests
      return 0
      ;;
  esac
  awk -v crate="$1" '
    $1 == crate && !seen[$2]++ {
      if ($2 == "src/lib.rs") print "--lib"
      else { t = $2; sub(/^tests\//, "", t); sub(/\.rs$/, "", t); print "--test"; print t }
    }
  ' "$art/list.txt"
}

build_crate() {
  crate=$1
  args=$(crate_args "$crate") || finish read
  set --
  for arg in $args; do
    set -- "$@" "$arg"
  done
  cargo test --locked -p "$crate" "$@" --no-run >"$art/build-$crate.txt" 2>&1 </dev/null
  code=$?
  tree_check "after-build-$crate"
  if [ "$code" -ne 0 ]; then
    printf 'policy build=%s exit=%s\n' "$crate" "$code"
    finish build
  fi
  cargo test --locked -p "$crate" "$@" --no-run --message-format=json >"$art/messages-$crate.json" 2>"$art/messages-$crate.err" </dev/null
  code=$?
  tree_check "after-messages-$crate"
  if [ "$code" -ne 0 ]; then
    printf 'policy build=%s exit=%s\n' "$crate" "$code"
    finish build
  fi
  read_messages "$art/messages-$crate.json" "$crate" >"$art/artifacts-$crate.txt" || finish read
  first=$(awk 'NR == 1' "$art/artifacts-$crate.txt") || finish read
  case $first in
    stale"$tab"*)
      printf 'policy stale=%s crate=%s\n' "${first#stale"$tab"}" "$crate"
      finish stale
      ;;
  esac
}

run_binary() {
  (
    cd "$repo/crates/$1" || exit 98
    CARGO_MANIFEST_DIR=$repo/crates/$1
    export CARGO_MANIFEST_DIR
    shift
    exec "$@"
  )
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

map_listed() {
  set -- "$art/list.txt"
  for crate in $crates; do
    set -- "$@" "$art/artifacts-$crate.txt"
  done
  awk '
    FNR == 1 { file++ }
    file == 1 {
      sub(/\r$/, "")
      if (NF == 3) { n++; crate[n] = $1; path[n] = $2; test[n] = $3 }
      next
    }
    {
      owner = FILENAME
      sub(/.*artifacts-/, "", owner)
      sub(/\.txt$/, "", owner)
      split($0, a, "\t")
      if (a[1] != "test") next
      key = owner SUBSEP a[2]
      count[key]++
      exe[key] = a[4]
    }
    END {
      for (i = 1; i <= n; i++) {
        key = crate[i] SUBSEP path[i]
        if (count[key] != 1) { print "missing\t" crate[i] "/" path[i] "\tfound=" (count[key] + 0); exit 1 }
        e = exe[key]
        if (!(e in index_of)) { index_of[e] = ++b; print "bin\t" b "\t" crate[i] "\t" e }
        print "test\t" i "\t" crate[i] "\t" test[i] "\t" index_of[e] "\t" e
      }
    }
  ' "$@"
}

listed_states() {
  awk -v art="$art" '
    function slurp(file, kind,    line) {
      while ((getline line < file) > 0) {
        sub(/\r$/, "", line)
        lines[kind, ++count[kind]] = line
      }
      close(file)
    }
    FNR == 1 { file++ }
    file == 1 {
      split($0, a, "\t")
      if (a[1] == "test") { n++; num[n] = a[2]; test[n] = a[4]; bin[n] = a[5] }
      next
    }
    file == 2 { bad_list[$1] = ($2 != 0 || $3 != 0); next }
    file == 3 { code[$1] = $2; next }
    END {
      for (i = 1; i <= n; i++) {
        t = test[i]
        b = bin[i]
        if (bad_list[b]) { print t, "unlisted"; continue }
        delete count
        slurp(art "/bin-" b "-list.txt", "all")
        slurp(art "/bin-" b "-ignored.txt", "ignored")
        slurp(art "/run-" num[i] ".txt", "run")
        listed = skipped = ok = other = summaries = one = 0
        for (k = 1; k <= count["all"]; k++) if (lines["all", k] == t ": test") listed = 1
        for (k = 1; k <= count["ignored"]; k++) if (lines["ignored", k] == t ": test") skipped = 1
        for (k = 1; k <= count["run"]; k++) {
          line = lines["run", k]
          if (line == "test " t " ... ok") ok++
          else if (line ~ /^test result: /) { summaries++; if (line ~ /^test result: ok\. 1 passed; 0 failed; 0 ignored; /) one = 1 }
          else if (line ~ /^test /) other++
        }
        if (!listed) print t, "absent"
        else if (skipped) print t, "ignored"
        else if (ok == 1 && other == 0 && summaries == 1 && one && code[num[i]] == 0) print t, "ok"
        else print t, "failed"
      }
    }
  ' "$art/map.txt" "$art/bin-codes.txt" "$art/run-codes.txt"
}

run_listed() {
  map_listed >"$art/map.txt"
  code=$?
  if [ "$code" -ne 0 ]; then
    missing=$(awk -F "$tab" 'END { print $2 " " $3 }' "$art/map.txt") || finish read
    printf 'policy binary=%s\n' "$missing"
    finish binary
  fi
  : >"$art/bin-codes.txt" || finish artifacts
  : >"$art/run-codes.txt" || finish artifacts
  while IFS="$tab" read -r kind num crate a rest; do
    if [ "$kind" = bin ]; then
      run_binary "$crate" "$a" --list >"$art/bin-$num-list.txt" 2>&1 </dev/null
      list_code=$?
      run_binary "$crate" "$a" --list --ignored >"$art/bin-$num-ignored.txt" 2>&1 </dev/null
      printf '%s %s %s\n' "$num" "$list_code" "$?" >>"$art/bin-codes.txt" || finish artifacts
      tree_check "after-list-$crate-$num"
    else
      run_binary "$crate" "${rest#*"$tab"}" --exact "$a" >"$art/run-$num.txt" 2>&1 </dev/null
      printf '%s %s\n' "$num" "$?" >>"$art/run-codes.txt" || finish artifacts
      tree_check "after-$crate-$a"
    fi
  done <"$art/map.txt"
  listed_states >>"$art/results.txt" || finish read
}

whole_gaps() {
  set -- "$art/targets.txt"
  for crate in $whole; do
    set -- "$@" "$art/artifacts-$crate.txt"
  done
  awk '
    FNR == 1 { file++ }
    file == 1 { sub(/\r$/, ""); if (NF == 2) { n++; key[n] = $1 "/" $2 }; next }
    {
      owner = FILENAME
      sub(/.*artifacts-/, "", owner)
      sub(/\.txt$/, "", owner)
      split($0, a, "\t")
      if (a[1] == "test") count[owner "/" a[2]]++
    }
    END { for (i = 1; i <= n; i++) if (count[key[i]] != 1) print "policy whole_target=" key[i] " found=" (count[key[i]] + 0) }
  ' "$@"
}

run_whole() {
  : >"$art/whole-codes.txt" || finish artifacts
  whole_gaps >"$art/whole-gaps.txt" || finish read
  whole_missing=$(awk 'END { print NR }' "$art/whole-gaps.txt") || finish read
  while IFS= read -r line; do
    printf '%s\n' "$line"
  done <"$art/whole-gaps.txt"
  for crate in $whole; do
    i=0
    while IFS="$tab" read -r kind src target exe; do
      [ "$kind" = test ] || continue
      i=$((i + 1))
      run_binary "$crate" "$exe" >"$art/whole-$crate-$i.txt" 2>&1 </dev/null
      printf '%s\t%s\t%s\t%s\n' "$crate" "$target" "$?" "$art/whole-$crate-$i.txt" >>"$art/whole-codes.txt" || finish artifacts
      tree_check "after-whole-$crate-$i"
    done <"$art/artifacts-$crate.txt"
  done
  awk -F "\t" '
    {
      file = $4
      summaries = good = failed = 0
      while ((getline line < file) > 0) {
        sub(/\r$/, "", line)
        if (line ~ /^test result: /) { summaries++; if (line ~ /^test result: ok\. [0-9]+ passed; 0 failed; /) good++ }
        if (line ~ / \.\.\. FAILED$/) failed++
      }
      close(file)
      n++
      if (!($3 == 0 && summaries == 1 && good == 1 && failed == 0)) { bad++; print "policy whole=" $1 " target=" $2 " exit=" $3 }
    }
    END { print "whole " (n + 0) " " (bad + 0) }
  ' "$art/whole-codes.txt" >"$art/whole-states.txt" || finish read
  while IFS=' ' read -r first second third; do
    if [ "$first" = whole ]; then
      whole_binaries=$second
      whole_failed=$third
    else
      printf '%s %s %s\n' "$first" "$second" "$third"
    fi
  done <"$art/whole-states.txt"
}

measure() {
  art=$1
  mkdir -p "$art" || finish artifacts
  tree_check before
  test_list >"$art/list.txt" || finish list
  listed=$(check_list "$art/list.txt")
  code=$?
  printf 'policy %s\n' "$listed"
  [ "$code" -eq 0 ] || finish list
  target_list >"$art/targets.txt" || finish list
  targets_want=$(target_want) || finish read
  targets=$(check_targets "$art/targets.txt")
  code=$?
  printf 'policy %s\n' "$targets"
  [ "$code" -eq 0 ] || finish list
  whole=$(awk '!seen[$1]++ { printf "%s ", $1 }' "$art/targets.txt") || finish read

  for crate in $crates; do
    build_crate "$crate"
  done

  exe=$(binary_of "$art/artifacts-halvo-policy.txt" bin agent-config 3) || {
    printf 'policy binary=agent-config %s\n' "$exe"
    finish binary
  }
  run_binary halvo-policy "$exe" presets >"$art/presets.txt" 2>"$art/presets.err" </dev/null
  presets_code=$?
  tree_check after-presets
  presets=miss
  if [ "$presets_code" -eq 0 ] && read_presets "$art/presets.txt"; then
    presets=ok
  fi

  exe=$(binary_of "$art/artifacts-halvo-agent-gate.txt" bin agent-gate 3) || {
    printf 'policy binary=agent-gate %s\n' "$exe"
    finish binary
  }
  run_binary halvo-agent-gate "$exe" configs >"$art/configs.txt" 2>"$art/configs.err" </dev/null
  configs_code=$?
  tree_check after-configs
  show_configs "$art/configs.txt" || finish read
  configs=miss
  if [ "$configs_code" -eq 0 ] && read_configs "$art/configs.txt"; then
    configs=ok
  fi

  : >"$art/results.txt" || finish artifacts
  run_listed
  run_whole

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
  printf 'policy presets=%s configs=%s tests=%s ok=%s miss=%s whole_targets=%s whole_binaries=%s whole_failed=%s\n' \
    "$presets" "$configs" "$total" "$ok" "$miss" "$targets_want" "$whole_binaries" "$whole_failed"
  [ "$total" -eq "$list_count" ] || finish read
  [ "$presets" = ok ] || finish presets
  [ "$configs" = ok ] || finish configs
  [ "$miss" -eq 0 ] || finish miss
  [ "$whole_failed" -eq 0 ] || finish whole
  [ "$whole_missing" -eq 0 ] || finish whole
  printf 'PASS %s\n' "$name"
}

say() {
  printf '%s%s\n' "$1" "$eol"
}

json_path() {
  if [ "$fx_backslash" = yes ]; then
    printf '%s\n' "$1" | sed 's|/|\\\\|g'
  else
    printf '%s\n' "$1"
  fi
}

stub_test_binary() {
  cat >"$1" <<'EOF' || return 1
#!/bin/sh
eol=
IFS= read -r eol <"$0.eol" || :
say() { printf '%s%s\n' "$1" "$eol"; }
total=0
while read -r t r; do total=$((total + 1)); done <"$0.tests"
case $* in
  --list)
    if [ -f "$0.list-exit" ]; then
      read -r code <"$0.list-exit" || :
      exit "$code"
    fi
    if [ -f "$0.dirty-list" ]; then
      IFS= read -r dirty <"$0.dirty-list" || :
      : >"$dirty"
    fi
    while read -r t r; do say "$t: test"; done <"$0.tests"
    say ""
    say "$total tests, 0 benchmarks"
    ;;
  '--list --ignored')
    while read -r t r; do [ "$r" = ignored ] && say "$t: test"; done <"$0.tests"
    ;;
  '--exact '*)
    want=$2
    if [ -f "$0.dirty-exact" ]; then
      IFS= read -r dirty <"$0.dirty-exact" || :
      : >"$dirty"
    fi
    [ -f "$0.fake" ] && while read -r line; do say "$line"; done <"$0.fake"
    found=
    while read -r t r; do [ "$t" = "$want" ] && found=$r; done <"$0.tests"
    case $found in
      '')
        say "running 0 tests"
        say "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; $total filtered out; finished in 0.00s"
        ;;
      ok)
        say "running 1 test"
        say "test $want ... ok"
        [ -f "$0.twice" ] && say "test $want ... ok"
        [ -f "$0.exact-extra" ] && say "test another_test ... ok"
        say ""
        passed=1
        [ -f "$0.zero" ] && passed=0
        say "test result: ok. $passed passed; 0 failed; 0 ignored; 0 measured; $((total - 1)) filtered out; finished in 0.00s"
        [ -f "$0.exact-exit" ] && exit 101
        ;;
      ignored)
        say "running 1 test"
        say "test $want ... ignored"
        say "test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; $((total - 1)) filtered out; finished in 0.00s"
        ;;
      *)
        say "running 1 test"
        say "test $want ... FAILED"
        say "test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; $((total - 1)) filtered out; finished in 0.00s"
        exit 101
        ;;
    esac
    ;;
  '')
    if [ -f "$0.dirty-whole" ]; then
      IFS= read -r dirty <"$0.dirty-whole" || :
      : >"$dirty"
    fi
    passed=0
    failed=0
    while read -r t r; do
      [ "$r" = ignored ] && continue
      if [ "$r" = ok ]; then passed=$((passed + 1)); else failed=$((failed + 1)); fi
      say "test $t ... $r"
    done <"$0.tests"
    if [ -f "$0.whole" ]; then
      failed=$((failed + 1))
      say "test whole_only ... FAILED"
    fi
    [ -f "$0.no-summary" ] && exit 0
    if [ "$failed" -gt 0 ]; then
      say "test result: FAILED. $passed passed; $failed failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s"
      exit 101
    fi
    [ -f "$0.whole-failed-line" ] && say "test hidden_failure ... FAILED"
    say "test result: ok. $passed passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s"
    [ -f "$0.whole-two-summaries" ] && say "test result: ok. $passed passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s"
    [ -f "$0.whole-exit" ] && exit 101
    ;;
  *) exit 97 ;;
esac
exit 0
EOF
  chmod +x "$1" || return 1
  printf '%s' "$eol" >"$1.eol" || return 1
}

stub_output_binary() {
  cat >"$1" <<'EOF' || return 1
#!/bin/sh
if [ -f "$0.dirty" ]; then
  IFS= read -r dirty <"$0.dirty" || :
  : >"$dirty"
fi
while IFS= read -r line; do printf '%s\n' "$line"; done <"$0.out"
read -r code <"$0.code" || :
exit "$code"
EOF
  chmod +x "$1" || return 1
}

artifact_message() {
  dir=$(json_path "@W@/repo/crates/$1") || return 1
  exe=null
  if [ "$4" != null ]; then
    exe=$(json_path "$4") || return 1
    exe="\"$exe\""
  fi
  case $2 in
    test) kind='"test"' profile=true ;;
    unit) kind='"lib"' profile=true ;;
    bin) kind='"bin"' profile=false ;;
    *) kind='"lib"' profile=false ;;
  esac
  sep=/
  [ "$fx_backslash" = yes ] && sep="\\\\"
  src=$(json_path "$3") || return 1
  printf '{"reason":"compiler-artifact","package_id":"path+file://%s#0.0.0","manifest_path":"%s%sCargo.toml","target":{"kind":[%s],"crate_types":["bin"],"name":"%s","src_path":"%s%s%s","edition":"2021","doc":false,"doctest":false,"test":true},"profile":{"opt_level":"0","debuginfo":2,"debug_assertions":true,"overflow_checks":true,"test":%s},"features":[],"filenames":[],"executable":%s,"fresh":%s}\n' \
    "$dir" "$dir" "$sep" "$kind" "$5" "$dir" "$sep" "$src" "$profile" "$exe" "$6" >>"$world/messages-$1.json"
}

make_base() {
  world=$self_art/base-$1
  mkdir -p "$world/bin" || return 1
  test_list >"$world/list.txt" || return 1
  for crate in $crates; do
    printf '{"reason":"compiler-artifact","package_id":"registry+https://github.com/rust-lang/crates.io-index#serde@1.0.229","manifest_path":"/c/serde/Cargo.toml","target":{"kind":["lib"],"crate_types":["lib"],"name":"serde","src_path":"/c/serde/src/lib.rs","edition":"2021","doc":true,"doctest":true,"test":true},"profile":{"opt_level":"0","debuginfo":2,"debug_assertions":true,"overflow_checks":true,"test":false},"features":[],"filenames":[],"executable":null,"fresh":true}\n' >"$world/messages-$crate.json" || return 1
    artifact_message "$crate" lib src/lib.rs null "$crate" true || return 1
    if [ "$crate" != halvo-schema ]; then
      stub_test_binary "$world/bin/$crate-lib" || return 1
      printf 'lib_other ok\n' >"$world/bin/$crate-lib.tests" || return 1
      artifact_message "$crate" unit src/lib.rs "@W@/bin/$crate-lib" "$crate" true || return 1
    fi
    paths=$(awk -v crate="$crate" '$1 == crate && !seen[$2]++ { print $2 }' "$world/list.txt") || return 1
    for path in $paths; do
      stem=${path#tests/}
      stem=${stem%.rs}
      exe=$world/bin/$crate-$stem
      stub_test_binary "$exe" || return 1
      awk -v crate="$crate" -v path="$path" '$1 == crate && $2 == path { print $3 " ok" }' "$world/list.txt" >"$exe.tests" || return 1
      printf '%s_other ok\n' "$stem" >>"$exe.tests" || return 1
      artifact_message "$crate" test "$path" "@W@/bin/$crate-$stem" "$stem" true || return 1
    done
  done
  stub_output_binary "$world/bin/agent-config" || return 1
  artifact_message halvo-policy bin src/bin/agent-config.rs @W@/bin/agent-config agent-config true || return 1
  stub_output_binary "$world/bin/agent-gate" || return 1
  artifact_message halvo-agent-gate bin src/bin/agent-gate.rs @W@/bin/agent-gate agent-gate true || return 1
  for crate in $crates; do
    printf '{"reason":"build-finished","success":true}\n' >>"$world/messages-$crate.json" || return 1
  done
}

make_world() {
  variant=plain
  [ -z "$eol" ] || variant=crlf
  [ "$fx_backslash" = no ] || variant=backslash
  base=$self_art/base-$variant
  world=$self_art/$case_name
  cp -R "$base" "$world" || return 1
  json_path "$world" >"$world/at.txt" || return 1
  at=$(sed 's/\\/\\\\/g' "$world/at.txt") || return 1
  for crate in $crates; do
    sed "s|@W@|$at|g" "$world/messages-$crate.json" >"$world/messages.tmp" || return 1
    if [ "$fx_no_gate_binary" = yes ]; then
      grep -v '"kind":\["bin"\],"crate_types":\["bin"\],"name":"agent-gate"' "$world/messages.tmp" >"$world/messages-$crate.json"
      [ $? -le 1 ] || return 1
    else
      mv "$world/messages.tmp" "$world/messages-$crate.json" || return 1
    fi
  done
  printf '%s\n' "$fx_presets_code" >"$world/bin/agent-config.code" || return 1
  printf '%s\n' "$fx_configs_code" >"$world/bin/agent-gate.code" || return 1
  for category in $categories; do
    result=valid
    autonomy=draft_and_confirm
    if [ "$category" = email ]; then
      result=$fx_preset_email
      autonomy=$fx_preset_email_autonomy
    fi
    say "preset category=$category runtime=web autonomy=$autonomy result=$result"
  done >"$world/bin/agent-config.out" || return 1
  if [ -n "$fx_presets_extra" ]; then
    say "$fx_presets_extra" >>"$world/bin/agent-config.out" || return 1
  fi
  say "$fx_presets_last" >>"$world/bin/agent-config.out" || return 1
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
    say "config category=$shown stored=$stored state=approved agent=set result=valid"
  done >"$world/bin/agent-gate.out" || return 1
  if [ -n "$fx_configs_extra" ]; then
    say "$fx_configs_extra" >>"$world/bin/agent-gate.out" || return 1
  fi
  say "$fx_configs_last" >>"$world/bin/agent-gate.out" || return 1
  if [ -n "$fx_configs_after" ]; then
    say "$fx_configs_after" >>"$world/bin/agent-gate.out" || return 1
  fi
}

stub_cargo() {
  case $* in
    'test --locked -p '*' --no-run')
      [ "$fx_build_fails" = "$4" ] && return 101
      [ "$fx_dirty_by_build" = "$4" ] && : >"$world/dirty"
      printf '   Compiling %s v0.0.0\n' "$4"
      ;;
    'test --locked -p '*' --no-run --message-format=json')
      [ "$fx_dirty_by_messages" = "$4" ] && : >"$world/dirty"
      cat "$world/messages-$4.json"
      if [ "$fx_messages_fails" = "$4" ]; then
        return 101
      fi
      ;;
    'test --locked -p '*)
      printf 'running 1 test\ntest %s ... ok\ntest result: ok. 1 passed; 0 failed\n' "$4"
      ;;
    *) return 97 ;;
  esac
}

stub_tree_state() {
  : >"$1"
  [ -f "$world/dirty" ] && printf ' M crates/halvo-policy/src/lib.rs\n' >"$1"
  return 0
}

fixture() {
  case_name=$1
  eol=
  fx_list=
  fx_backslash=no
  fx_preset_email=valid
  fx_preset_email_autonomy=draft_and_confirm
  fx_presets_last=$want_presets
  fx_presets_extra=
  fx_presets_code=0
  fx_configs_drop=0
  fx_configs_swap=0
  fx_configs_unstored=0
  fx_configs_extra=
  fx_configs_after=
  fx_configs_last='configs seeds=10 valid=10 store=memory'
  fx_configs_code=0
  fx_no_gate_binary=no
  fx_build_fails=
  fx_dirty_by_build=
  fx_dirty_by_messages=
  fx_messages_fails=
  fx_targets=$stand_in_targets
}

ready() {
  make_world || self_fail "the world of $case_name cannot be made"
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
    if [ -n "$fx_list" ]; then
      test_list() {
        printf '%s\n' "$fx_list"
      }
    fi
    target_list() {
      printf '%s\n' "$fx_targets"
    }
    target_want() {
      printf '%s\n' "$stand_in_count"
    }
    measure "$self_art/$case_name/art"
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

self_fail() {
  printf 'FAIL self-test:%s (%s)\n' "$name" "$1"
  exit 1
}

flag() {
  printf '%s\n' "$2" >"$world/bin/$1.$3" || self_fail "the flag $3 of $1 cannot be written"
}

edit_tests() {
  sed "$2" "$world/bin/$1.tests" >"$world/bin/$1.tests.new" || self_fail "the tests of $1 cannot be changed"
  mv "$world/bin/$1.tests.new" "$world/bin/$1.tests" || self_fail "the tests of $1 cannot be changed"
}

self_test() {
  self_art=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || self_fail "mktemp failed"
  rejected=0
  wrong=
  [ "$list_count" -eq 19 ] || self_fail "the count of the list is not 19"
  [ "$target_count" -eq 26 ] || self_fail "the count of the list of whole targets is not 26"
  real_targets=$(target_list | awk 'NF { n++ } END { print n + 0 }') || self_fail "the list of whole targets cannot be read"
  [ "$real_targets" -eq "$target_count" ] || self_fail "the list of whole targets holds $real_targets lines, not $target_count"
  stand_in_targets='halvo-policy src/lib.rs
halvo-policy tests/policy_table.rs
halvo-policy tests/refusal.rs
halvo-agent-store src/lib.rs
halvo-agent-store tests/refusal.rs
halvo-agent-store tests/memory.rs
halvo-agent-store tests/coordinator.rs
halvo-agent-gate src/lib.rs
halvo-agent-gate tests/bind.rs'
  stand_in_count=9
  all_ok='policy presets=ok configs=ok tests=19 ok=19 miss=0 whole_targets=9 whole_binaries=9 whole_failed=0'
  eol=
  fx_backslash=no
  make_base plain || self_fail "the plain world cannot be made"
  eol=$cr
  make_base crlf || self_fail "the world with carriage returns cannot be made"
  eol=
  fx_backslash=yes
  make_base backslash || self_fail "the world with backslashes cannot be made"

  fixture all-ok
  ready
  check PASS "$all_ok"

  fixture carriage-returns
  eol=$cr
  ready
  check PASS "$all_ok"

  fixture windows-paths
  fx_backslash=yes
  ready
  check PASS "$all_ok"

  fixture derived-store
  fx_configs_last='configs seeds=10 valid=10 store=derived'
  ready
  check PASS 'configs seeds=10 valid=10 store=derived'

  fixture list-name-dropped
  fx_list=$(test_list | awk '$3 != "refusal_digest"')
  ready
  check FAIL-list 'policy list count:18'

  fixture list-name-doubled
  fx_list=$(test_list && printf 'halvo-policy tests/refusal.rs refusal_digest\n')
  ready
  check FAIL-list 'policy list twice:refusal_digest count:20'

  fixture list-name-in-two-crates
  fx_list=$(test_list && printf 'halvo-agent-store tests/refusal.rs refusal_reuse\n')
  ready
  check FAIL-list 'policy list twice:refusal_reuse count:20'

  fixture list-target-of-another-form
  fx_list=$(test_list | awk '$3 == "policy_table" { $2 = "policy_table" } { print }')
  ready
  check FAIL-list 'policy list form:1'

  fixture preset-not-valid
  fx_preset_email=allow
  ready
  check FAIL-presets 'policy presets=miss configs=ok'

  fixture presets-count-nine
  fx_presets_last='presets seeds=10 valid=9 unattended=0'
  ready
  check FAIL-presets 'policy presets=miss configs=ok'

  fixture presets-unattended
  fx_presets_last='presets seeds=10 valid=10 unattended=1'
  ready
  check FAIL-presets 'policy presets=miss configs=ok'

  fixture preset-unattended-line
  fx_preset_email_autonomy=unattended
  ready
  check FAIL-presets 'policy presets=miss configs=ok'

  fixture presets-eleventh-unattended-line
  fx_presets_extra='preset category=email runtime=web autonomy=unattended result=valid'
  ready
  check FAIL-presets 'policy presets=miss configs=ok'

  fixture presets-exit-1
  fx_presets_code=1
  ready
  check FAIL-presets 'policy presets=miss configs=ok'

  fixture configs-not-stored
  fx_configs_unstored=5
  ready
  check FAIL-configs 'config category=email stored=no state=approved agent=set result=valid'

  fixture configs-line-missing
  fx_configs_drop=10
  ready
  check FAIL-configs 'policy presets=ok configs=miss'

  fixture configs-order
  fx_configs_swap=1
  ready
  check FAIL-configs 'policy presets=ok configs=miss'

  fixture configs-eleventh-line
  fx_configs_extra='config category=email stored=yes state=approved agent=set result=valid'
  ready
  check FAIL-configs 'policy presets=ok configs=miss'

  fixture configs-two-count-lines
  fx_configs_after='configs seeds=10 valid=10 store=memory'
  ready
  check FAIL-configs 'policy presets=ok configs=miss'

  fixture configs-line-of-another-kind
  fx_configs_extra='note gate=memory'
  ready
  check FAIL-configs 'policy configs other_lines=1'

  fixture configs-count-nine
  fx_configs_last='configs seeds=10 valid=9 store=memory'
  ready
  check FAIL-configs 'policy presets=ok configs=miss'

  fixture configs-other-store
  fx_configs_last='configs seeds=10 valid=10 store=owner'
  ready
  check FAIL-configs 'policy presets=ok configs=miss'

  fixture configs-exit-1
  fx_configs_code=1
  ready
  check FAIL-configs 'policy presets=ok configs=miss'

  fixture test-failed
  ready
  edit_tests halvo-agent-store-refusal 's/^refusal_end_run_loses ok$/refusal_end_run_loses FAILED/'
  check FAIL-miss 'policy test=refusal_end_run_loses state=failed'

  fixture test-made-to-panic-under-a-runner-that-prints-ok
  ready
  edit_tests halvo-policy-refusal 's/^refusal_digest ok$/refusal_digest FAILED/'
  check FAIL-miss 'policy test=refusal_digest state=failed'

  fixture test-ignored
  ready
  edit_tests halvo-policy-policy_table 's/^policy_table ok$/policy_table ignored/'
  check FAIL-miss 'policy test=policy_table state=ignored'

  fixture test-missing
  ready
  edit_tests halvo-schema-r3_config '/^refusal_allow_money ok$/d'
  check FAIL-miss 'policy test=refusal_allow_money state=absent'

  fixture test-printed-by-another-test
  ready
  edit_tests halvo-policy-refusal '/^refusal_digest ok$/d'
  flag halvo-policy-refusal 'test refusal_digest ... ok' fake
  check FAIL-miss 'policy test=refusal_digest state=absent'

  fixture test-of-the-same-name-in-another-target
  ready
  edit_tests halvo-agent-store-memory 's/^own_kind_roles ok$/own_kind_roles_old ok/'
  printf 'own_kind_roles ok\n' >>"$world/bin/halvo-agent-store-refusal.tests" || self_fail "a test cannot be added"
  check FAIL-miss 'policy test=own_kind_roles state=absent'

  fixture test-in-a-module
  ready
  edit_tests halvo-policy-refusal 's/^refusal_reuse ok$/tests::refusal_reuse ok/'
  check FAIL-miss 'policy test=refusal_reuse state=absent'

  fixture test-ok-line-twice
  ready
  flag halvo-agent-gate-bind yes twice
  check FAIL-miss 'policy test=every_runner_body_of_an_ended_run_is_refused state=failed'

  fixture test-summary-without-one-passed
  ready
  flag halvo-agent-store-refusal yes zero
  check FAIL-miss 'policy test=refusal_repeat_begin state=failed'

  fixture test-binary-list-fails
  ready
  flag halvo-agent-store-memory 101 list-exit
  check FAIL-miss 'policy test=own_kind_roles state=unlisted'

  fixture whole-run-fails-in-halvo-policy
  ready
  flag halvo-policy-lib yes whole
  check FAIL-whole 'policy whole=halvo-policy target=halvo-policy exit=101'

  fixture whole-run-fails-in-halvo-agent-store
  ready
  flag halvo-agent-store-lib yes whole
  check FAIL-whole 'policy whole=halvo-agent-store target=halvo-agent-store exit=101'

  fixture whole-run-fails-in-halvo-agent-gate
  ready
  flag halvo-agent-gate-lib yes whole
  check FAIL-whole 'policy whole=halvo-agent-gate target=halvo-agent-gate exit=101'

  fixture whole-run-prints-no-summary
  ready
  flag halvo-agent-store-refusal yes no-summary
  check FAIL-whole 'policy whole=halvo-agent-store target=refusal exit=0'

  fixture build-fails
  fx_build_fails=halvo-agent-store
  ready
  check FAIL-build 'policy build=halvo-agent-store exit=101'

  fixture stale-artifact
  ready
  sed 's/"name":"refusal","src_path"\(.*\)"fresh":true}$/"name":"refusal","src_path"\1"fresh":false}/' \
    "$world/messages-halvo-agent-store.json" >"$world/stale.json" || self_fail "the messages cannot be changed"
  mv "$world/stale.json" "$world/messages-halvo-agent-store.json" || self_fail "the messages cannot be changed"
  check FAIL-stale 'policy stale=refusal@halvo-agent-store crate=halvo-agent-store'

  fixture two-binaries-for-one-target
  ready
  grep '"name":"memory"' "$world/messages-halvo-agent-store.json" >"$world/memory-again.json" || self_fail "a message cannot be read"
  cat "$world/memory-again.json" >>"$world/messages-halvo-agent-store.json" || self_fail "a message cannot be written"
  check FAIL-binary 'policy binary=halvo-agent-store/tests/memory.rs found=2'

  fixture no-agent-gate-binary
  fx_no_gate_binary=yes
  ready
  check FAIL-binary 'policy binary=agent-gate found=0'

  fixture tree-dirty-before
  ready
  : >"$world/dirty" || self_fail "a flag cannot be written"
  check FAIL-tree 'policy tree=before differs=1'

  fixture tree-changed-by-a-build
  fx_dirty_by_build=halvo-agent-gate
  ready
  check FAIL-tree 'policy tree=after-build-halvo-agent-gate differs=1'

  fixture tree-changed-by-a-test-run
  ready
  flag halvo-schema-r3_config "$world/dirty" dirty-exact
  check FAIL-tree 'policy tree=after-halvo-schema-refusal_allow_money differs=1'

  fixture whole-target-without-a-binary
  ready
  grep -v '"name":"halvo-agent-store","src_path".*"executable":"' "$world/messages-halvo-agent-store.json" >"$world/no-lib.json"
  [ $? -le 1 ] || self_fail "the messages cannot be changed"
  mv "$world/no-lib.json" "$world/messages-halvo-agent-store.json" || self_fail "the messages cannot be changed"
  check FAIL-whole 'policy whole_target=halvo-agent-store/src/lib.rs found=0'

  fixture targets-line-less
  fx_targets=$(printf '%s\n' "$stand_in_targets" | awk 'NR != 4')
  ready
  check FAIL-list 'policy targets count:8'

  fixture targets-line-twice
  fx_targets="$stand_in_targets
halvo-agent-store src/lib.rs"
  ready
  check FAIL-list 'policy targets twice:halvo-agent-store/src/lib.rs count:10'

  fixture tree-changed-by-presets
  ready
  flag agent-config "$world/dirty" dirty
  check FAIL-tree 'policy tree=after-presets differs=1'

  fixture tree-changed-by-configs
  ready
  flag agent-gate "$world/dirty" dirty
  check FAIL-tree 'policy tree=after-configs differs=1'

  fixture tree-changed-by-a-messages-call
  fx_dirty_by_messages=halvo-agent-store
  ready
  check FAIL-tree 'policy tree=after-messages-halvo-agent-store differs=1'

  fixture tree-changed-by-a-list-call
  ready
  flag halvo-agent-store-memory "$world/dirty" dirty-list
  check FAIL-tree 'policy tree=after-list-halvo-agent-store-4 differs=1'

  fixture tree-changed-by-a-whole-run
  ready
  flag halvo-agent-gate-lib "$world/dirty" dirty-whole
  check FAIL-tree 'policy tree=after-whole-halvo-agent-gate-1 differs=1'

  fixture test-run-exits-101-after-its-summary
  ready
  flag halvo-agent-gate-bind yes exact-exit
  check FAIL-miss 'policy test=every_runner_body_of_an_ended_run_is_refused state=failed'

  fixture whole-run-exits-101-after-an-ok-summary
  ready
  flag halvo-agent-gate-bind yes whole-exit
  check FAIL-whole 'policy whole=halvo-agent-gate target=bind exit=101'

  fixture messages-call-fails
  fx_messages_fails=halvo-agent-gate
  ready
  check FAIL-build 'policy build=halvo-agent-gate exit=101'

  fixture whole-run-with-a-failed-line
  ready
  flag halvo-agent-store-memory yes whole-failed-line
  check FAIL-whole 'policy whole=halvo-agent-store target=memory exit=0'

  fixture test-run-with-another-test-line
  ready
  flag halvo-agent-store-coordinator yes exact-extra
  check FAIL-miss 'policy test=create_agent_stores_the_preset_and_makes_the_state_call state=failed'

  fixture whole-run-with-two-summaries
  ready
  flag halvo-policy-policy_table yes whole-two-summaries
  check FAIL-whole 'policy whole=halvo-policy target=policy_table exit=0'

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
repo=$(pwd) || exit 2
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
