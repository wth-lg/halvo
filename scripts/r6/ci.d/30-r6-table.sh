#!/bin/sh
# jobs: macos windows
# rule: The router's tests pass, each test of scripts/r6/required-tests.txt prints ok under its own target, stands in its test binary's list and not among its ignored tests and passes in a run of those tests alone, each of the five pinned files hashes by the system's own tool to its entry in PINNED of tests/no_grant_in_release.rs, the list, PINNED and the files are read from the commit before any cargo command and no cargo command leaves the tree changed, the gate tool's self-test fails the three bad routers and passes the derived one on 20 generated tables, and the tool regenerates the committed specimen.
set -u
name=30-r6-table
case $0 in
  /*) self=$0 ;;
  *) self=$(pwd)/$0 ;;
esac
specimen=crates/halvo-route-gate/tests/data/specimen.tsv
required=scripts/r6/required-tests.txt
fixed_tests='halvo-route tests/no_grant_in_release.rs no_grant_in_release
halvo-route tests/no_grant_in_release.rs pinned_files_hold_their_hashes'
floor_tests='halvo-route tests/no_grant_in_release.rs no_grant_in_release
halvo-route tests/no_grant_in_release.rs pinned_files_hold_their_hashes
halvo-route tests/refusal_spend.rs refusal_no_hold
halvo-route tests/refusal_spend.rs refusal_over_room
halvo-route tests/refusal_spend.rs refusal_second_dispatch
halvo-route tests/refusal_spend.rs refusal_ruling_none
halvo-route tests/refusal_spend.rs refusal_no_cap
halvo-route tests/refusal_spend.rs refusal_not_active
halvo-route tests/refusal_spend.rs refusal_zero_amount
halvo-route tests/refusal_spend.rs refusal_final_state
halvo-route src/lib.rs admit::tests::refusal_admit_ruling
halvo-route src/lib.rs admit::tests::refusal_admit_no_cap
halvo-route src/lib.rs admit::tests::refusal_admit_over_room
halvo-route src/lib.rs admit::tests::refusal_admit_sum
halvo-route src/lib.rs admit::tests::refusal_admit_overflow
halvo-route src/lib.rs leave::tests::refusal_leave_ruling
halvo-route src/lib.rs leave::tests::refusal_leave_local
halvo-route src/lib.rs leave::tests::refusal_leave_ceiling
halvo-route src/lib.rs leave::tests::refusal_leave_part
halvo-route src/lib.rs leave::tests::refusal_leave_origin
halvo-route src/lib.rs leave::tests::refusal_leave_consent
halvo-route tests/no_grant_in_release.rs the_manifest_holds_the_package_and_the_dev_dependencies_alone
halvo-route tests/formal_traces.rs formal_traces'
pin_file=crates/halvo-route/tests/no_grant_in_release.rs
want_line='selftest tables=20 derived_pass=20 product_pass=- one_tier_fail=20 random_fail=20 peek_fail=20'
want_product='selftest tables=20 derived_pass=20 product_pass=20 one_tier_fail=20 random_fail=20 peek_fail=20'

enter_repo() {
  if [ -n "${CI_D_REPO:-}" ]; then
    repo=$CI_D_REPO
  else
    repo=$(git rev-parse --show-toplevel) || repo=
  fi
  if [ -z "$repo" ] || ! cd "$repo" 2>/dev/null; then
    printf 'FAIL %s (cannot enter the repository)\n' "$name"
    exit 2
  fi
}

fail_now() {
  printf 'FAIL %s (tests)\n' "$name"
  exit 1
}

tree_state() {
  git -c core.fsmonitor=false -c core.untrackedCache=false status --porcelain=v2 --branch --untracked-files=all --ignored=matching &&
    git ls-files -v
}

tree_same() {
  tree_now=$(tree_state 2>&1) || tree_now="unreadable $tree_now"
  [ "$tree_now" = "$tree_before" ] && return 0
  R6_TREE_BEFORE=$tree_before R6_TREE_NOW=$tree_now awk -v when="$1" '
    function report(line,    field, count) {
      if (shown++ >= 5) return
      count = split(line, field, " ")
      printf "tree changed=%s %s %s\n", when, field[1], field[count]
    }
    BEGIN {
      n = split(ENVIRON["R6_TREE_BEFORE"], before, "\n")
      for (i = 1; i <= n; i++) was[before[i]] = 1
      m = split(ENVIRON["R6_TREE_NOW"], now, "\n")
      for (i = 1; i <= m; i++) is[now[i]] = 1
      for (i = 1; i <= m; i++) if (!(now[i] in was)) report(now[i])
      for (i = 1; i <= n; i++) if (!(before[i] in is)) report(before[i])
    }
  '
  fail_now
}

read_commit() {
  commit=$(git rev-parse --verify --quiet 'HEAD^{commit}') || commit=
  tree_before=$(tree_state 2>&1) || tree_before=
  if [ -z "$tree_before" ]; then
    printf 'tree commit=unreadable\n'
    fail_now
  fi
  R6_TREE_BEFORE=$tree_before awk '
    BEGIN {
      n = split(ENVIRON["R6_TREE_BEFORE"], line, "\n")
      for (i = 1; i <= n; i++) {
        count = split(line[i], field, " ")
        if (count && field[1] !~ /^(#|\?|!|H)$/) {
          printf "tree changed=before %s %s\n", field[1], field[count]
          bad = 1
        }
      }
      exit bad
    }
  ' || fail_now
  committed=$scratch/commit
  mkdir "$committed" || exit 2
  if ! git -c core.autocrlf=false archive --format=tar -o "$scratch/commit.tar" "$commit" -- crates/halvo-route scripts/r6 \
    crates/halvo-route-gate/tests/data || ! tar -xf "$scratch/commit.tar" -C "$committed"; then
    printf 'tree commit=unreadable\n'
    fail_now
  fi
  wanted_text=
  if [ -s "$committed/$required" ] && wanted_text=$(printf '%s\n' "$fixed_tests" && cat "$committed/$required") &&
    R6_WANTED=$wanted_text awk '
      BEGIN {
        n = split(ENVIRON["R6_WANTED"], line, "\n")
        for (i = 1; i <= n; i++) if (line[i] !~ /^halvo-route (src\/lib|tests\/[a-z0-9_]+)\.rs [A-Za-z0-9_:]+$/) bad = 1
        exit bad
      }'; then
    list_ok=yes
  else
    list_ok=no
  fi
}

run_targets() {
  target_sources=$(R6_WANTED=$wanted_text awk 'BEGIN { n = split(ENVIRON["R6_WANTED"], line, "\n"); for (i = 1; i <= n; i++) { split(line[i], field, " "); if (!seen[field[2]]++) print field[2] } }') || exit 2
  target_names=$(R6_WANTED=$wanted_text awk 'BEGIN { n = split(ENVIRON["R6_WANTED"], line, "\n"); for (i = 1; i <= n; i++) { split(line[i], field, " "); if (!seen[field[3]]++) print field[3] } }') || exit 2
  set --
  while read -r target_source; do
    case $target_source in
      src/lib.rs) set -- "$@" --lib ;;
      *)
        target_stem=${target_source#tests/}
        set -- "$@" --test "${target_stem%.rs}"
        ;;
    esac
  done <<EOF
$target_sources
EOF
  cargo test --locked -p halvo-route "$@" -- --list >"$artifacts/list.txt" 2>&1 </dev/null
  list_code=$?
  tree_same after-list
  cargo test --locked -p halvo-route "$@" -- --list --ignored >"$artifacts/list-ignored.txt" 2>&1 </dev/null
  ignored_code=$?
  tree_same after-ignored
  set -- "$@" -- --exact
  while read -r test_name; do
    set -- "$@" "$test_name"
  done <<EOF
$target_names
EOF
  cargo test --locked -p halvo-route "$@" >"$artifacts/exact-run.txt" 2>&1 </dev/null
  run_code=$?
  tree_same after-exact
}

require_tests() {
  if [ "$list_ok" != yes ]; then
    printf 'required list=bad\n'
    return 1
  fi
  run_targets
  awk -v list_code="$list_code" -v ignored_code="$ignored_code" -v run_code="$run_code" '
    function target_of(line,    paren, bin) {
      sub(/^ *Running /, "", line)
      gsub(/\\/, "/", line)
      paren = index(line, " (")
      if (paren == 0 || line !~ /\)$/) return ""
      bin = substr(line, paren + 2, length(line) - paren - 2)
      sub(/.*\//, "", bin)
      sub(/\.exe$/, "", bin)
      sub(/-[0-9a-f]+$/, "", bin)
      return bin " " substr(line, 1, paren - 1)
    }
    FILENAME == ARGV[1] || FILENAME == ARGV[2] || FILENAME == ARGV[3] || FILENAME == ARGV[4] {
      sub(/\r$/, "")
      if ($0 ~ /^ *Doc-tests /) {
        target = ""
      } else if ($0 ~ /^ *Running /) {
        target = target_of($0)
      } else {
        if (FILENAME == ARGV[1] && $0 ~ /^test [^ ]+ \.\.\. ok$/) shared[target " " substr($0, 6, length($0) - 12)] = 1
        if (FILENAME == ARGV[2] && $0 ~ /: test$/) listed[target " " substr($0, 1, length($0) - 6)] = 1
        if (FILENAME == ARGV[3] && $0 ~ /: test$/) ignored[target " " substr($0, 1, length($0) - 6)] = 1
        if (FILENAME == ARGV[4] && $0 ~ /^test [^ ]+ \.\.\. ok$/) passed[target " " substr($0, 6, length($0) - 12)] = 1
        if (FILENAME == ARGV[4] && $0 ~ /^test result: ok\. /) summary[target] = 1
      }
      next
    }
    $0 in wanted { next }
    {
      wanted[$0] = 1
      tests++
      if ($2 == "src/lib.rs") {
        bin = $1
        gsub(/-/, "_", bin)
        key = bin " unittests src/lib.rs"
      } else {
        bin = $2
        sub(/^tests\//, "", bin)
        sub(/\.rs$/, "", bin)
        key = bin " " $2
      }
      if ((key " " $3) in shared) ok++
      else print "required not_ok=" $0
      if (list_code != 0) print "required no_list=" $0
      else if ((key " " $3) in listed) listed_count++
      else print "required not_listed=" $0
      if (ignored_code != 0) print "required no_ignored_list=" $0
      else if ((key " " $3) in ignored) print "required ignored=" $0
      else clean++
      if (run_code == 0 && (key in summary) && ((key " " $3) in passed)) ran++
      else print "required not_run=" $0
    }
    END {
      printf "required tests=%d ok=%d listed=%d ignored=%d ran=%d\n", tests, ok, listed_count, tests - clean, ran
      exit (ok != tests || listed_count != tests || clean != tests || ran != tests)
    }
  ' "$artifacts/cargo-test.txt" "$artifacts/list.txt" "$artifacts/list-ignored.txt" "$artifacts/exact-run.txt" - <<EOF
$wanted_text
EOF
}

hold_floor() {
  floor_list=$committed/$required
  [ -f "$floor_list" ] || floor_list=/dev/null
  R6_FLOOR=$floor_tests awk '
    { sub(/\r$/, ""); have[$0] = 1 }
    END {
      n = split(ENVIRON["R6_FLOOR"], line, "\n")
      for (i = 1; i <= n; i++) {
        if (!(line[i] in have)) {
          print "required dropped=" line[i]
          bad = 1
        }
      }
      exit bad
    }' "$floor_list"
}

pinned() {
  "$@" crates/halvo-route/src/lib.rs crates/halvo-route/src/leave.rs crates/halvo-route/src/ruling.rs \
    crates/halvo-route/src/tier.rs crates/halvo-route/Cargo.toml
}

hold_pins() {
  if command -v sha256sum >/dev/null 2>&1; then
    tool=sha256sum
  elif command -v shasum >/dev/null 2>&1; then
    tool=shasum
  else
    printf 'pins tool=none files=0 same=0\n'
    return 1
  fi
  if [ "$tool" = sha256sum ]; then
    (cd "$committed" && pinned sha256sum -b) >"$scratch/hashes.txt" 2>"$artifacts/hashes.err"
  else
    (cd "$committed" && pinned shasum -a 256 -b) >"$scratch/hashes.txt" 2>"$artifacts/hashes.err"
  fi
  paths=$(pinned echo) || return 1
  pins_source=$committed/$pin_file
  if [ ! -f "$pins_source" ]; then
    pins_source=$scratch/no-pins.txt
    : >"$pins_source"
  fi
  awk -v paths="$paths" -v tool="$tool" '
    FILENAME == ARGV[1] {
      file = $2
      sub(/^\*/, "", file)
      hash[file] = $1
      next
    }
    /^[ \t]*(pub(\([^)]*\))? )?(const|static) PINNED[^A-Za-z0-9_]/ { declared++ }
    /^const PINNED/ {
      blocks++
      inside = 1
      if (attribute) broken = 1
    }
    /^[ \t]*(#|\/\/\/|\/\/!|\/\*\*|\/\*!)/ { attribute = 1 }
    $0 !~ /^[ \t]*(#|\/\/|\/\*|\*|$)/ { attribute = 0 }
    inside {
      line = $0
      sub(/\/\/.*/, "", line)
      text = text line
      if ($0 ~ /^\];/) inside = 0
    }
    END {
      count = split(paths, path, " ")
      gsub(/[ \t]/, "", text)
      if (match(text, /^constPINNED:\[\(&str,&str\);[0-9]+\]=\[/)) {
        size = substr(text, 26, RLENGTH - 28) + 0
        rest = substr(text, RLENGTH + 1)
      } else {
        broken = 1
      }
      while (!broken && match(rest, /^\("[^"]*","[^"]*",?\),?/)) {
        entry = substr(rest, 3, RLENGTH - 2)
        rest = substr(rest, RLENGTH + 1)
        sub(/,$/, "", entry)
        sub(/\)$/, "", entry)
        sub(/,$/, "", entry)
        sub(/"$/, "", entry)
        split(entry, part, "\",\"")
        entries++
        name[entries] = part[1]
        value[entries] = part[2]
      }
      if (rest != "];" || entries != size) broken = 1
      if (declared != 1 || broken) {
        print "pins unreadable=PINNED"
        printf "pins tool=%s files=%d same=0\n", tool, count
        exit 1
      }
      for (i = 1; i <= count; i++) {
        have = (path[i] in hash)
        relative = path[i]
        sub(/^crates\/halvo-route\//, "", relative)
        found = 0
        wrong = 0
        for (k = 1; k <= entries; k++) {
          if (name[k] != relative) continue
          found++
          if (!have || value[k] != hash[path[i]]) wrong++
        }
        if (!found) print "pins missing=" path[i]
        else if (wrong) print "pins differs=" path[i]
        else same++
      }
      printf "pins tool=%s files=%d same=%d\n", tool, count, same
      exit (same != count)
    }
  ' "$scratch/hashes.txt" "$pins_source"
}

measure() {
  enter_repo
  artifacts=${CI_D_ARTIFACTS:-target/ci-artifacts/$name}
  if ! mkdir -p "$artifacts" 2>/dev/null || ! scratch=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX" 2>/dev/null); then
    printf 'FAIL %s (cannot make a directory)\n' "$name"
    exit 2
  fi
  CARGO_TERM_QUIET=false
  CARGO_TERM_VERBOSE=false
  CARGO_TERM_COLOR=never
  export CARGO_TERM_QUIET CARGO_TERM_VERBOSE CARGO_TERM_COLOR
  cause=

  read_commit
  hold_floor || cause=tests
  hold_pins || cause=tests
  if ! cargo test --locked -p halvo-route -p halvo-route-gate >"$artifacts/cargo-test.txt" 2>&1; then
    cause=tests
  fi
  tree_same after-tests
  require_tests || cause=tests

  cargo run -q --locked -p halvo-route-gate --bin route-gate -- self-test --tables 20 --seed 1 \
    >"$artifacts/selftest.out" 2>"$artifacts/selftest.err"
  selftest_code=$?
  tree_same after-selftest
  grep '^selftest ' "$artifacts/selftest.out"
  last=$(tail -n 1 "$artifacts/selftest.out")
  if [ "$selftest_code" -ne 0 ] || { [ "$last" != "$want_line" ] && [ "$last" != "$want_product" ]; }; then
    cause=${cause:-selftest}
  fi

  fresh=$scratch/specimen.tsv
  cargo run -q --locked -p halvo-route-gate --bin route-gate -- generate --seed 1 --out "$fresh" \
    >"$artifacts/generate.out" 2>"$artifacts/generate.err"
  tree_same after-generate
  rows=0
  same=no
  if [ -f "$fresh" ]; then
    lines=$(grep -c '' "$fresh")
    [ "$lines" -gt 0 ] && rows=$((lines - 1))
    cmp -s "$fresh" "$committed/$specimen" && same=yes
  fi
  printf 'specimen rows=%s same=%s\n' "$rows" "$same"
  [ "$same" = yes ] || cause=${cause:-specimen}

  if [ -n "$cause" ]; then
    printf 'FAIL %s (%s)\n' "$name" "$cause"
    exit 1
  fi
  printf 'PASS %s\n' "$name"
}

check() {
  out=$(
    PATH=$work/bin:${STUB_PATH:-$PATH}
    CI_D_REPO=$work/repo
    CI_D_ARTIFACTS=$work/artifacts/$1
    export PATH CI_D_REPO CI_D_ARTIFACTS STUB_TEST_EXIT STUB_SELFTEST STUB_SELFTEST_EXIT STUB_FRESH
    export STUB_DIR STUB_NO_TARGETS STUB_NO_IGNORED STUB_NOT_RUN STUB_NO_SUMMARY STUB_RUN_EXIT
    export STUB_TREE STUB_CHANGE_AT STUB_CHANGE_FILE STUB_CHANGE_LINE STUB_COMMITTED STUB_DROP STUB_NO_COMMIT
    if [ -n "$STUB_QUIET" ]; then
      CARGO_TERM_QUIET=$STUB_QUIET
      export CARGO_TERM_QUIET
    fi
    if [ -n "$STUB_COLOR" ]; then
      CARGO_TERM_COLOR=$STUB_COLOR
      export CARGO_TERM_COLOR
    fi
    sh "$self" 2>&1
  )
  code=$?
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  case_name=$1
  want=$2
  shift 2
  for text in "$@"; do
    case $out in
      *"$text"*) ;;
      *) got=$got-without-the-text ;;
    esac
  done
  printf 'case %s/%s want=%s got=%s\n' "$name" "$case_name" "$want" "$got"
  if [ "$got" != "$want" ]; then
    wrong="$wrong $case_name"
  elif [ "$want" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
  STUB_TEST_EXIT=0
  STUB_SELFTEST=$want_line
  STUB_SELFTEST_EXIT=0
  STUB_FRESH=$work/fresh.tsv
  STUB_DIR=$work/stub
  STUB_NO_TARGETS=
  STUB_NO_IGNORED=
  STUB_PATH=
  STUB_NOT_RUN=
  STUB_NO_SUMMARY=
  STUB_RUN_EXIT=
  STUB_CHANGE_AT=
  STUB_CHANGE_FILE=status
  STUB_CHANGE_LINE=
  STUB_COMMITTED=
  STUB_DROP=
  STUB_NO_COMMIT=
  STUB_QUIET=
  STUB_COLOR=
  : >"$STUB_TREE/status" || exit 2
  : >"$STUB_TREE/flags" || exit 2
}

stub_case() {
  STUB_DIR=$work/stub-$1
  mkdir "$STUB_DIR" || exit 2
  cp "$work/stub/"* "$STUB_DIR/" || exit 2
}

restore_repo() {
  cp -R "$work/pristine/." "$work/repo/" || exit 2
}

required_text() {
  required_count=$(LC_ALL=C sort -u "$work/wanted.txt" | grep -c '') || exit 2
  printf 'required tests=%s ok=%s listed=%s ignored=0 ran=%s\n' \
    "$required_count" "$required_count" "$required_count" "$required_count"
}

make_stub() {
  mkdir -p "$work/stub" "$work/pristine/scripts/r6" "$work/pristine/crates/halvo-route/src" \
    "$work/pristine/crates/halvo-route/tests" "$work/pristine/crates/halvo-route-gate/tests/data" "$work/tree" || exit 2
  cp "$specimen" "$work/pristine/$specimen" || exit 2
  STUB_TREE=$work/tree
  : >"$STUB_TREE/status" || exit 2
  : >"$STUB_TREE/flags" || exit 2
  cp "$required" "$work/pristine/$required" || exit 2
  cp "$pin_file" "$work/pristine/$pin_file" || exit 2
  pinned_paths=$(pinned echo) || exit 2
  for path in $pinned_paths; do
    cp "$path" "$work/pristine/$path" || exit 2
  done
  { printf '%s\n' "$fixed_tests" && cat "$required"; } >"$work/wanted.txt" || exit 2
  awk -v dir="$work/stub" '
    seen[$0]++ { next }
    {
      if ($2 == "src/lib.rs") {
        stem = "lib"
        bin = $1
        gsub(/-/, "_", bin)
        printed = "unittests src/lib.rs"
      } else {
        stem = $2
        sub(/^tests\//, "", stem)
        sub(/\.rs$/, "", stem)
        bin = stem
        printed = $2
      }
      target = $1 " " $2
      if (!(target in order)) {
        order[target] = ++n
        head[n] = "     Running " printed " (target/debug/deps/" bin "-0123456789abcdef)"
        listed[n] = dir "/list-" $1 "-" stem ".txt"
        ignored[n] = dir "/ignored-" $1 "-" stem ".txt"
      }
      k = order[target]
      oks[k] = oks[k] "test " $3 " ... ok\n"
      names[k] = names[k] $3 ": test\n"
    }
    END {
      out = dir "/test.out"
      for (k = 1; k <= n; k++) {
        printf "%s\n\nrunning tests\n%s\n", head[k], oks[k] >out
        printf "%s\n", names[k] >listed[k]
        printf "" >ignored[k]
      }
      printf "     Running tests/pick_rules.rs (target/debug/deps/pick_rules-0123456789abcdef)\n\ntest pick_rule ... ok\n\n" >out
      printf "     Running unittests src/lib.rs (target/debug/deps/halvo_route_gate-0123456789abcdef)\n\ntest gate_rule ... ok\n\n" >out
      printf "   Doc-tests halvo_route\n\nrunning 0 tests\n" >out
    }
  ' "$work/wanted.txt" || exit 2
  STUB_DIR=$work/stub
  STUB_NO_TARGETS=
  STUB_NO_IGNORED=
  STUB_PATH=
  STUB_NOT_RUN=
  STUB_NO_SUMMARY=
  STUB_RUN_EXIT=
  STUB_CHANGE_AT=
  STUB_CHANGE_FILE=status
  STUB_CHANGE_LINE=
  STUB_COMMITTED=
  STUB_DROP=
  STUB_NO_COMMIT=
  STUB_QUIET=
  STUB_COLOR=
}

tree_cases() {
  t=halvo-route
  STUB_CHANGE_AT=tests
  STUB_CHANGE_LINE='1 .M N... 100644 100644 100644 0000 0000 scripts/r6/required-tests.txt'
  check test-rewrites-the-list FAIL 'tree changed=after-tests 1 scripts/r6/required-tests.txt'

  grep -v 'refusal_admit_sum$' "$work/pristine/$required" >"$work/repo/$required"
  stub_case list-trimmed-in-the-work-tree
  awk '$0 != "test admit::tests::refusal_admit_sum ... ok"' "$work/stub/test.out" >"$STUB_DIR/test.out" || exit 2
  printf 'admit::tests::refusal_admit_sum: test\n' >"$STUB_DIR/ignored-$t-lib.txt"
  STUB_DROP=admit::tests::refusal_admit_sum
  STUB_COMMITTED=$work/pristine
  check list-trimmed-in-the-work-tree FAIL "required ignored=$t src/lib.rs admit::tests::refusal_admit_sum"
  restore_repo

  mkdir "$work/committed-short" || exit 2
  cp -R "$work/pristine/." "$work/committed-short/" || exit 2
  grep -v ' refusal_zero_amount$' "$work/pristine/$required" >"$work/committed-short/$required"
  STUB_COMMITTED=$work/committed-short
  check list-of-the-commit-lost-a-line FAIL "required dropped=$t tests/refusal_spend.rs refusal_zero_amount" \
    "FAIL $name (tests)"

  mkdir "$work/committed-longer" || exit 2
  cp -R "$work/pristine/." "$work/committed-longer/" || exit 2
  printf '%s tests/formal_traces.rs the_floor_refuses_199_drawn_traces\n' "$t" >>"$work/committed-longer/$required"
  longer_count=$(LC_ALL=C sort -u "$work/wanted.txt" | grep -c '') || exit 2
  longer_count=$((longer_count + 1))
  stub_case list-of-the-commit-gained-a-line
  awk '{ print } $0 == "test formal_traces ... ok" { print "test the_floor_refuses_199_drawn_traces ... ok" }' \
    "$work/stub/test.out" >"$STUB_DIR/test.out" || exit 2
  printf 'the_floor_refuses_199_drawn_traces: test\n' >>"$STUB_DIR/list-$t-formal_traces.txt"
  STUB_COMMITTED=$work/committed-longer
  check list-of-the-commit-gained-a-line PASS "required tests=$longer_count ok=$longer_count" \
    "PASS $name"

  mkdir "$work/committed-e1" || exit 2
  cp -R "$work/pristine/." "$work/committed-e1/" || exit 2
  printf '\n' >>"$work/committed-e1/crates/halvo-route/src/ruling.rs"
  STUB_COMMITTED=$work/committed-e1
  check pinned-file-restored-in-the-work-tree FAIL 'pins differs=crates/halvo-route/src/ruling.rs'

  mkdir "$work/committed-pins" || exit 2
  cp -R "$work/pristine/." "$work/committed-pins/" || exit 2
  awk '/^const PINNED/ { sub(/; 5\]/, "; 4]") } $0 == "    (" { held = $0 "\n"; inside = 1; next } inside { held = held $0 "\n"; if ($0 == "    ),") { inside = 0; if (index(held, "\"src/tier.rs\"") == 0) printf "%s", held }; next } { print }' "$work/pristine/$pin_file" >"$work/committed-pins/$pin_file" || exit 2
  STUB_COMMITTED=$work/committed-pins
  check pinned-row-taken-out-in-the-commit-only FAIL 'pins missing=crates/halvo-route/src/tier.rs'

  mkdir "$work/committed-specimen" || exit 2
  cp -R "$work/pristine/." "$work/committed-specimen/" || exit 2
  cp "$work/short.tsv" "$work/committed-specimen/$specimen" || exit 2
  STUB_COMMITTED=$work/committed-specimen
  check specimen-changed-in-the-commit-only FAIL "specimen rows=540 same=no
FAIL $name (specimen)"

  STUB_NO_COMMIT=1
  check no-commit FAIL 'tree commit=unreadable'

  printf '? notes.txt\n' >"$STUB_TREE/status"
  STUB_CHANGE_AT=tests
  STUB_CHANGE_FILE=clear
  check untracked-file-removed-by-a-test FAIL 'tree changed=after-tests ? notes.txt'

  stub_case un-ignored-at-run-time-with-a-written-line
  printf '     Running unittests src/lib.rs (target/debug/deps/halvo_route-0123456789abcdef)\ntest admit::tests::refusal_admit_sum ... ok\n' >>"$STUB_DIR/test.out"
  STUB_CHANGE_AT=tests
  STUB_CHANGE_LINE='1 .M N... 100644 100644 100644 0000 0000 crates/halvo-route/src/admit.rs'
  check un-ignored-at-run-time-with-a-written-line FAIL 'tree changed=after-tests 1 crates/halvo-route/src/admit.rs'

  for kind in list ignored exact selftest generate; do
    STUB_CHANGE_AT=$kind
    STUB_CHANGE_LINE='? .cargo/config.toml'
    check "untracked-file-after-$kind" FAIL "tree changed=after-$kind ? .cargo/config.toml"
  done
  STUB_CHANGE_AT=tests
  STUB_CHANGE_FILE=flags
  STUB_CHANGE_LINE='S crates/halvo-route/src/admit.rs'
  check skip-worktree-set-by-a-test FAIL 'tree changed=after-tests S crates/halvo-route/src/admit.rs'
  STUB_CHANGE_AT=tests
  STUB_CHANGE_LINE='# branch.oid 2222222222222222222222222222222222222222'
  check head-moved-by-a-test FAIL 'tree changed=after-tests # 2222222222222222222222222222222222222222'

  printf '1 .M N... 100644 100644 100644 0000 0000 crates/halvo-route/src/admit.rs\n' >"$STUB_TREE/status"
  check changed-file-before-the-run FAIL 'tree changed=before 1 crates/halvo-route/src/admit.rs'
  printf 'h crates/halvo-route/src/admit.rs\n' >"$STUB_TREE/flags"
  check flagged-file-before-the-run FAIL 'tree changed=before h crates/halvo-route/src/admit.rs'
  printf '? notes.txt\n! target/\n' >"$STUB_TREE/status"
  check untracked-and-ignored-before-the-run PASS "$want_required"

  STUB_QUIET=true
  check job-sets-cargo-quiet PASS "$want_required"
  STUB_COLOR=always
  check job-sets-cargo-colour PASS "$want_required"

  unentered=$(CI_D_REPO=$work/no-such-directory sh "$self" 2>&1)
  code=$?
  outside "$code" "$unentered" 'FAIL 30-r6-table (cannot enter the repository)' cannot-enter-the-repository
  : >"$work/a-file" || exit 2
  unmade=$(CI_D_REPO=$work/repo CI_D_ARTIFACTS=$work/a-file/artifacts sh "$self" 2>&1)
  code=$?
  outside "$code" "$unmade" 'FAIL 30-r6-table (cannot make a directory)' cannot-make-a-directory
}

outside() {
  if [ "$1:$2" = "2:$3" ]; then
    printf 'case %s/%s want=FAIL got=FAIL\n' "$name" "$4"
    rejected=$((rejected + 1))
  else
    printf 'case %s/%s want=FAIL got=exit-%s\n' "$name" "$4" "$1"
    wrong="$wrong $4"
  fi
}

required_cases() {
  t=halvo-route
  stub_case ignored-required-test
  awk '$0 == "test refusal_no_hold ... ok" { $0 = "test refusal_no_hold ... ignored" } { print }' "$work/stub/test.out" >"$STUB_DIR/test.out" || exit 2
  printf 'refusal_no_hold: test\n' >"$STUB_DIR/ignored-$t-refusal_spend.txt"
  check ignored-required-test FAIL "required not_ok=$t tests/refusal_spend.rs refusal_no_hold" \
    "required ignored=$t tests/refusal_spend.rs refusal_no_hold"

  stub_case test-table-gives-a-name-to-another-file
  awk '/Running tests\/refusal_spend\.rs / { sub(/tests\/refusal_spend\.rs/, "tests/spend_table.rs") } { print }' "$work/stub/test.out" >"$STUB_DIR/test.out" || exit 2
  check test-table-gives-a-name-to-another-file FAIL "required not_ok=$t tests/refusal_spend.rs refusal_no_hold" \
    "required not_ok=$t tests/refusal_spend.rs refusal_final_state"

  stub_case autotests-false
  awk '/^ *(Running|Doc-tests) / { skip = ($0 ~ /Running tests\//) } !skip { print }' "$work/stub/test.out" >"$STUB_DIR/test.out" || exit 2
  STUB_NO_TARGETS='no_grant_in_release refusal_spend pick_rules'
  check autotests-false FAIL "required not_ok=$t tests/no_grant_in_release.rs no_grant_in_release" \
    "required no_list=$t tests/no_grant_in_release.rs pinned_files_hold_their_hashes" \
    "required no_ignored_list=$t tests/refusal_spend.rs refusal_no_hold"

  stub_case test-writes-the-ok-lines-itself
  awk '{ print } /Running tests\/no_grant_in_release\.rs / { print "test forge ... ok" }' "$work/stub/test.out" >"$STUB_DIR/test.out" || exit 2
  printf 'forge: test\nlib_rs_is_the_forbid_line_and_the_module_lines: test\n' >"$STUB_DIR/list-$t-no_grant_in_release.txt"
  check test-writes-the-ok-lines-itself FAIL "required not_listed=$t tests/no_grant_in_release.rs no_grant_in_release" \
    "required not_listed=$t tests/no_grant_in_release.rs pinned_files_hold_their_hashes"

  stub_case other-target-prints-the-first-targets-line
  awk '/Running tests\/no_grant_in_release\.rs / { print "     Running tests/pick_rules.rs (target/debug/deps/pick_rules-0123456789abcdef)" } { print }' "$work/stub/test.out" >"$STUB_DIR/test.out" || exit 2
  STUB_NO_TARGETS=no_grant_in_release
  check other-target-prints-the-first-targets-line FAIL "required no_list=$t tests/no_grant_in_release.rs no_grant_in_release" \
    "required no_ignored_list=$t tests/no_grant_in_release.rs pinned_files_hold_their_hashes"

  stub_case ignored-test-with-a-written-ok-line
  printf 'refusal_no_hold: test\n' >"$STUB_DIR/ignored-$t-refusal_spend.txt"
  check ignored-test-with-a-written-ok-line FAIL "required ignored=$t tests/refusal_spend.rs refusal_no_hold"

  stub_case ok-lines-under-the-next-target
  awk '/^test (no_grant_in_release|pinned_files_hold_their_hashes) \.\.\. ok$/ { held = held $0 "\n"; next } { print } $0 == "test pick_rule ... ok" { printf "%s", held }' "$work/stub/test.out" >"$STUB_DIR/test.out" || exit 2
  check ok-lines-under-the-next-target FAIL "required not_ok=$t tests/no_grant_in_release.rs no_grant_in_release" \
    "required not_ok=$t tests/no_grant_in_release.rs pinned_files_hold_their_hashes"

  stub_case ok-line-after-a-doc-tests-line
  awk '$0 == "test admit::tests::refusal_admit_ruling ... ok" { print "   Doc-tests halvo_route" } { print }' "$work/stub/test.out" >"$STUB_DIR/test.out" || exit 2
  check ok-line-after-a-doc-tests-line FAIL "required not_ok=$t src/lib.rs admit::tests::refusal_admit_ruling"

  stub_case should-panic-ok-line
  awk '$0 == "test pinned_files_hold_their_hashes ... ok" { $0 = "test pinned_files_hold_their_hashes - should panic ... ok" } { print }' "$work/stub/test.out" >"$STUB_DIR/test.out" || exit 2
  check should-panic-ok-line FAIL "required not_ok=$t tests/no_grant_in_release.rs pinned_files_hold_their_hashes"

  stub_case ok-line-under-the-gate-library
  awk '$0 == "test admit::tests::refusal_admit_ruling ... ok" { next } { print } $0 == "test gate_rule ... ok" { print "test admit::tests::refusal_admit_ruling ... ok" }' "$work/stub/test.out" >"$STUB_DIR/test.out" || exit 2
  check ok-line-under-the-gate-library FAIL "required not_ok=$t src/lib.rs admit::tests::refusal_admit_ruling"

  stub_case ignored-listing-fails
  STUB_NO_IGNORED=refusal_spend
  check ignored-listing-fails FAIL "required no_ignored_list=$t tests/refusal_spend.rs refusal_no_hold"

  stub_case list-without-the-pin-tests
  awk '/^test (no_grant_in_release|pinned_files_hold_their_hashes) \.\.\. ok$/ { next } { print }' "$work/stub/test.out" >"$STUB_DIR/test.out" || exit 2
  printf 'lib_rs_is_the_forbid_line_and_the_module_lines: test\n' >"$STUB_DIR/list-$t-no_grant_in_release.txt"
  grep -v ' tests/no_grant_in_release\.rs ' "$work/pristine/$required" >"$work/repo/$required"
  check list-without-the-pin-tests FAIL "required not_ok=$t tests/no_grant_in_release.rs no_grant_in_release" \
    "required not_listed=$t tests/no_grant_in_release.rs pinned_files_hold_their_hashes"
  restore_repo

  printf '%s tests/refusal_spend.rs\n' "$t" >>"$work/repo/$required"
  check list-with-a-line-of-two-fields FAIL 'required list=bad'
  restore_repo
  : >"$work/repo/$required"
  check empty-list FAIL 'required list=bad'
  restore_repo
  printf '%s tests/refusal_spend.rs refusal no_hold\n' "$t" >>"$work/repo/$required"
  check list-with-a-name-of-two-words FAIL 'required list=bad'
  mv "$work/repo/$required" "$work/repo/$required.away" || exit 2
  check no-list FAIL 'required list=bad'
  restore_repo

  stub_case carriage-returns
  awk '{ printf "%s\r\n", $0 }' "$work/stub/test.out" >"$STUB_DIR/test.out" || exit 2
  check carriage-returns PASS "$want_required"

  stub_case windows-paths
  awk '/^ *Running / { n = split($0, part, "/"); $0 = part[1]; for (i = 2; i <= n; i++) $0 = $0 "\\" part[i]; sub(/\)$/, ".exe)") } { print }' "$work/stub/test.out" >"$STUB_DIR/test.out" || exit 2
  check windows-paths PASS "$want_required"

  stub_case running-line-without-a-binary
  awk '{ print } /Running tests\/no_grant_in_release\.rs / { print "     Running tests/forge.rs" }' "$work/stub/test.out" >"$STUB_DIR/test.out" || exit 2
  check running-line-without-a-binary FAIL "required not_ok=$t tests/no_grant_in_release.rs no_grant_in_release"

  stub_case running-line-without-its-closing-paren
  awk '/Running tests\/no_grant_in_release\.rs / { sub(/\)$/, "0") } { print }' "$work/stub/test.out" >"$STUB_DIR/test.out" || exit 2
  check running-line-without-its-closing-paren FAIL "required not_ok=$t tests/no_grant_in_release.rs no_grant_in_release"

  stub_case ok-line-with-another-ending
  awk '$0 == "test no_grant_in_release ... ok" { $0 = "test no_grant_in_release ... no" } { print }' "$work/stub/test.out" >"$STUB_DIR/test.out" || exit 2
  check ok-line-with-another-ending FAIL "required not_ok=$t tests/no_grant_in_release.rs no_grant_in_release"

  STUB_NOT_RUN=refusal_spend
  check written-ok-lines-and-an-early-exit FAIL "required not_run=$t tests/refusal_spend.rs refusal_no_hold"
  STUB_NOT_RUN=lib
  check library-lines-from-another-binary FAIL "required not_run=$t src/lib.rs leave::tests::refusal_leave_part"
  STUB_NO_SUMMARY=no_grant_in_release
  check exact-run-without-its-summary FAIL "required not_run=$t tests/no_grant_in_release.rs no_grant_in_release"
  STUB_RUN_EXIT=101
  check exact-run-that-fails FAIL "required not_run=$t tests/refusal_spend.rs refusal_final_state"
  STUB_DROP=refusal_no_hold
  check exact-run-without-one-ok-line FAIL "required not_run=$t tests/refusal_spend.rs refusal_no_hold"
}

pin_cases() {
  pins=$work/pristine/$pin_file
  awk '/^const PINNED/ { sub(/; 5\]/, "; 4]") } $0 == "    (" { held = $0 "\n"; inside = 1; next } inside { held = held $0 "\n"; if ($0 == "    ),") { inside = 0; if (index(held, "\"src/tier.rs\"") == 0) printf "%s", held }; next } { print }' "$pins" >"$work/repo/$pin_file" || exit 2
  check pinned-row-taken-out FAIL 'pins missing=crates/halvo-route/src/tier.rs'
  restore_repo

  awk '/^const PINNED/ { sub(/; 5\]/, "; 4]") } $0 == "    (" { held = $0 "\n"; inside = 1; next } inside { held = held $0 "\n"; if ($0 == "    ),") { inside = 0; if (index(held, "\"Cargo.toml\"") == 0) printf "%s", held }; next } { print }' "$pins" >"$work/repo/$pin_file" || exit 2
  check manifest-entry-dropped FAIL 'pins missing=crates/halvo-route/Cargo.toml'
  restore_repo

  awk '{ sub(/changed\.is_empty\(\),/, "true,") } { print }' "$pins" >"$work/repo/$pin_file" || exit 2
  printf '\n' >>"$work/repo/crates/halvo-route/src/leave.rs"
  check always-holding-assertion-beside-a-changed-file FAIL 'pins differs=crates/halvo-route/src/leave.rs' 'pins tool='
  restore_repo

  printf '\n' >>"$work/repo/crates/halvo-route/src/ruling.rs"
  check changed-ruling-rs FAIL 'pins differs=crates/halvo-route/src/ruling.rs'
  restore_repo
  printf '\n' >>"$work/repo/crates/halvo-route/src/lib.rs"
  check changed-lib-rs FAIL 'pins differs=crates/halvo-route/src/lib.rs'
  restore_repo

  awk '/^const PINNED/ { sub(/; 5\]/, "; 4]") } $0 == "    (" { held = $0 "\n"; inside = 1; next } inside { held = held $0 "\n"; if ($0 == "    ),") { inside = 0; if (index(held, "\"src/tier.rs\"") > 0) { gsub(/\n */, " ", held); sub(/^    \( /, "    // (", held); sub(/, \),? *$/, "),", held); held = held "\n" }; printf "%s", held }; next } { print }' "$pins" >"$work/repo/$pin_file" || exit 2
  check pinned-row-commented-out FAIL 'pins missing=crates/halvo-route/src/tier.rs'
  restore_repo

  awk '$0 == "    (" { held = $0 "\n"; inside = 1; next } inside { held = held $0 "\n"; if ($0 == "    ),") { inside = 0; if (index(held, "\"src/tier.rs\"") > 0) held = "    /*\n" held "    */\n"; printf "%s", held }; next } { print }' "$pins" >"$work/repo/$pin_file" || exit 2
  check pinned-row-in-a-block-comment FAIL 'pins unreadable=PINNED'
  restore_repo

  awk '{ gsub(/PINNED/, "PINS") } { print }' "$pins" >"$work/repo/$pin_file" || exit 2
  check no-pinned-list FAIL 'pins unreadable=PINNED'
  restore_repo

  awk '{ print } /^const PINNED/ { inside = 1; held = "" } inside { held = held $0 "\n" } inside && /^\];/ { inside = 0; printf "%s", held }' "$pins" >"$work/repo/$pin_file" || exit 2
  check pinned-list-twice FAIL 'pins unreadable=PINNED'
  restore_repo

  awk '/^const PINNED/ { sub(/; 5\]/, "; 6]") } { print } $0 == "        \"src/tier.rs\"," { stale = 1 } stale && $0 == "    )," { stale = 0; print "    (\n        \"src/tier.rs\",\n        \"0000000000000000000000000000000000000000000000000000000000000000\",\n    )," }' "$pins" >"$work/repo/$pin_file" || exit 2
  check pinned-path-twice-one-stale FAIL 'pins differs=crates/halvo-route/src/tier.rs'
  restore_repo

  awk '/^const PINNED/ { print "#[cfg(any())]" } { print }' "$pins" >"$work/repo/$pin_file" || exit 2
  check attribute-above-the-pinned-list FAIL 'pins unreadable=PINNED'
  restore_repo

  awk '{ print } /^fn pinned_files_hold_their_hashes/ { print "    const PINNED: [(&str, &str); 1] = [(\"Cargo.toml\", \"x\")];" }' "$pins" >"$work/repo/$pin_file" || exit 2
  check pinned-list-shadowed-in-the-test FAIL 'pins unreadable=PINNED'
  restore_repo

  awk '/^const PINNED/ { sub(/= \[$/, "= pins![") } { print }' "$pins" >"$work/repo/$pin_file" || exit 2
  check pinned-list-from-a-macro FAIL 'pins unreadable=PINNED'
  restore_repo

  awk '/^\];/ { $0 = $0 " const _: (&str, &str) = (\"src/tier.rs\", \"x\");" } { print }' "$pins" >"$work/repo/$pin_file" || exit 2
  check item-after-the-closing-line FAIL 'pins unreadable=PINNED'
  restore_repo

  awk '/^const PINNED/ { sub(/; 5\]/, "; 6]") } { print }' "$pins" >"$work/repo/$pin_file" || exit 2
  check pinned-size-not-its-rows FAIL 'pins unreadable=PINNED'
  restore_repo

  awk '{ print } /^fn pinned_files_hold_their_hashes/ { print "    pub const PINNED: [(&str, &str); 0] = [];" }' "$pins" >"$work/repo/$pin_file" || exit 2
  check public-pinned-list-in-the-test FAIL 'pins unreadable=PINNED'
  restore_repo

  awk '{ print } /^fn pinned_files_hold_their_hashes/ { print "    static PINNED: [(&str, &str); 0] = [];" }' "$pins" >"$work/repo/$pin_file" || exit 2
  check static-pinned-list-in-the-test FAIL 'pins unreadable=PINNED'
  restore_repo

  awk '/^const PINNED/ { print "#[cfg(any())]"; print "" } { print }' "$pins" >"$work/repo/$pin_file" || exit 2
  check attribute-and-a-blank-line-above FAIL 'pins unreadable=PINNED'
  restore_repo

  awk '/^const PINNED/ { print "#[cfg(any())]"; print "// the pins" } { print }' "$pins" >"$work/repo/$pin_file" || exit 2
  check attribute-and-a-comment-above FAIL 'pins unreadable=PINNED'
  restore_repo

  awk '/^const PINNED/ { print "/// The pinned files." } { print }' "$pins" >"$work/repo/$pin_file" || exit 2
  check doc-comment-above-the-pinned-list FAIL 'pins unreadable=PINNED'
  restore_repo

  awk '/^const PINNED/ { print "// The pinned files." } { print }' "$pins" >"$work/repo/$pin_file" || exit 2
  check plain-comment-above-the-pinned-list PASS ' files=5 same=5'
  restore_repo

  awk 'blank { $0 = "        \"\","; blank = 0 } $0 == "        \"src/tier.rs\"," { blank = 1 } { print }' "$pins" >"$work/repo/$pin_file" || exit 2
  mv "$work/repo/crates/halvo-route/src/tier.rs" "$work/repo/crates/halvo-route/src/tier.rs.away" || exit 2
  check missing-file-with-an-empty-entry FAIL 'pins differs=crates/halvo-route/src/tier.rs'
  restore_repo

  awk '{ print } END { print "const OLD: [(&str, &str); 1] = [(\"src/tier.rs\", \"0000000000000000000000000000000000000000000000000000000000000000\")];" }' "$pins" >"$work/repo/$pin_file" || exit 2
  check tuple-after-the-pinned-list PASS 'pins tool=' ' files=5 same=5'
  restore_repo

  mv "$work/repo/$pin_file" "$work/repo/$pin_file.away" || exit 2
  check no-pin-test-file FAIL 'pins unreadable=PINNED'
  restore_repo

  mkdir "$work/bare" || exit 2
  for tool in awk cat cmp cp grep mkdir mktemp sh tail tar; do
    real=$(command -v "$tool") || exit 2
    printf '#!/bin/sh\nexec %s "$@"\n' "$real" >"$work/bare/$tool" || exit 2
    chmod +x "$work/bare/$tool" || exit 2
  done
  STUB_PATH=$work/bare
  check no-hash-tool FAIL 'pins tool=none'
  if real=$(command -v shasum); then
    printf '#!/bin/sh\nexec %s "$@"\n' "$real" >"$work/bare/shasum" || exit 2
    chmod +x "$work/bare/shasum" || exit 2
    STUB_PATH=$work/bare
    check only-shasum PASS 'pins tool=shasum files=5 same=5'
  fi
}

self_test() {
  enter_repo
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  mkdir -p "$work/bin" "$work/repo/crates/halvo-route-gate/tests/data" "$work/artifacts" || exit 2
  cp "$specimen" "$work/fresh.tsv" || exit 2
  cp "$specimen" "$work/repo/$specimen" || exit 2
  sed '2d' "$specimen" >"$work/short.tsv" || exit 2
  tab=$(printf '\t')
  sed "2s/${tab}yes${tab}/${tab}no${tab}/" "$specimen" >"$work/flipped.tsv" || exit 2
  cmp -s "$work/flipped.tsv" "$work/fresh.tsv" && exit 2
  cat >"$work/bin/git" <<'EOF'
#!/bin/sh
case $* in
  'rev-parse --verify --quiet HEAD^{commit}')
    [ -z "$STUB_NO_COMMIT" ] || exit 1
    printf '%s\n' 1111111111111111111111111111111111111111
    ;;
  '-c core.autocrlf=false archive --format=tar -o '*' 1111111111111111111111111111111111111111 -- crates/halvo-route scripts/r6 crates/halvo-route-gate/tests/data')
    tar -cf "$6" -C "${STUB_COMMITTED:-.}" crates/halvo-route scripts/r6 crates/halvo-route-gate/tests/data
    ;;
  '-c core.fsmonitor=false -c core.untrackedCache=false status --porcelain=v2 --branch --untracked-files=all --ignored=matching')
    printf '# branch.oid %s\n' 1111111111111111111111111111111111111111
    cat "$STUB_TREE/status"
    ;;
  'ls-files -v')
    printf 'H scripts/r6/required-tests.txt\n'
    cat "$STUB_TREE/flags"
    ;;
  *)
    printf 'stub git: got the arguments: %s\n' "$*" >&2
    exit 97
    ;;
esac
EOF
  chmod +x "$work/bin/git"
  cat >"$work/bin/cargo" <<'EOF'
#!/bin/sh
kind=other
case $* in
  'test --locked -p halvo-route -p halvo-route-gate') kind=tests ;;
  'test --locked -p halvo-route '*' -- --list --ignored') kind=ignored ;;
  'test --locked -p halvo-route '*' -- --list') kind=list ;;
  'test --locked -p halvo-route '*' -- --exact '*) kind=exact ;;
  'run -q --locked -p halvo-route-gate --bin route-gate -- self-test --tables 20 --seed 1') kind=selftest ;;
  'run -q --locked -p halvo-route-gate --bin route-gate -- generate --seed 1 --out '*) kind=generate ;;
esac
if [ "$kind" = "${STUB_CHANGE_AT:-none}" ]; then
  if [ "${STUB_CHANGE_FILE:-status}" = clear ]; then
    : >"$STUB_TREE/status"
  else
    printf '%s\n' "$STUB_CHANGE_LINE" >>"$STUB_TREE/${STUB_CHANGE_FILE:-status}"
  fi
fi
if [ "${CARGO_TERM_QUIET:-false}" = true ] || [ "${CARGO_TERM_COLOR:-auto}" = always ]; then
  printf '\033[1mcargo setting\033[0m quiet=%s color=%s\n' "${CARGO_TERM_QUIET:-}" "${CARGO_TERM_COLOR:-}"
  exit 0
fi
case $kind in
  tests)
    cat "$STUB_DIR/test.out"
    exit "$STUB_TEST_EXIT"
    ;;
  list | ignored | exact)
    shift 4
    stems=
    while [ "$1" != -- ]; do
      case $1 in
        --lib) stems="$stems lib" ;;
        --test)
          shift
          stems="$stems $1"
          ;;
      esac
      shift
    done
    shift
    for stem in $stems; do
      case " $STUB_NO_TARGETS " in
        *" $stem "*)
          printf 'error: no test target named %s\n' "$stem"
          exit 101
          ;;
      esac
      if [ "$kind" = ignored ]; then
        case " $STUB_NO_IGNORED " in
          *" $stem "*) exit 101 ;;
        esac
      fi
    done
    [ "$kind" = exact ] && shift
    awk -v kind="$kind" -v dir="$STUB_DIR" -v stems="$stems" -v names="$*" -v not_run=" $STUB_NOT_RUN " \
      -v no_summary=" $STUB_NO_SUMMARY " -v drop=" $STUB_DROP " '
      BEGIN {
        n = split(stems, stem, " ")
        m = split(names, name, " ")
        for (i = 1; i <= m; i++) wanted[name[i]] = 1
        for (i = 1; i <= n; i++) {
          s = stem[i]
          if (s == "lib") printf "     Running unittests src/lib.rs (target/debug/deps/halvo_route-0123456789abcdef)\n"
          else printf "     Running tests/%s.rs (target/debug/deps/%s-0123456789abcdef)\n", s, s
          file = dir "/" (kind == "ignored" ? "ignored" : "list") "-halvo-route-" s ".txt"
          if (kind != "exact") {
            while ((getline line < file) > 0) print line
            close(file)
            continue
          }
          if (index(not_run, " " s " ")) continue
          count = 0
          while ((getline line < file) > 0) {
            if (line !~ /: test$/) continue
            test = substr(line, 1, length(line) - 6)
            if ((test in wanted) && !index(drop, " " test " ")) {
              print "test " test " ... ok"
              count++
            }
          }
          close(file)
          if (!index(no_summary, " " s " ")) printf "\ntest result: ok. %d passed; 0 failed\n", count
        }
      }'
    [ "$kind" = exact ] && exit "${STUB_RUN_EXIT:-0}"
    exit 0
    ;;
  selftest)
    [ -z "$STUB_SELFTEST" ] || printf '%s\n' "$STUB_SELFTEST"
    exit "$STUB_SELFTEST_EXIT"
    ;;
  generate)
    [ -f "$STUB_FRESH" ] || exit 2
    shift 12
    cp "$STUB_FRESH" "$1"
    ;;
  *)
    printf 'stub cargo: got the arguments: %s\n' "$*"
    exit 97
    ;;
esac
EOF
  chmod +x "$work/bin/cargo"
  make_stub
  want_required=$(required_text) || exit 2
  restore_repo
  rejected=0
  wrong=
  STUB_TEST_EXIT=0
  STUB_SELFTEST=$want_line
  STUB_SELFTEST_EXIT=0
  STUB_FRESH=$work/fresh.tsv

  check all-hold PASS "$want_required" ' files=5 same=5' "$want_line
specimen rows=540 same=yes
PASS $name"
  [ -z "${R6_TABLE_NESTED:-}" ] || self_test_end
  tree_cases
  required_cases
  pin_cases
  STUB_SELFTEST=$want_product
  check product-counted PASS 'product_pass=20 one_tier_fail=20'
  STUB_TEST_EXIT=101
  check tests-fail FAIL "FAIL $name (tests)"
  STUB_SELFTEST='selftest miss router=random seed=7
selftest tables=20 derived_pass=20 product_pass=- one_tier_fail=20 random_fail=19 peek_fail=20'
  STUB_SELFTEST_EXIT=1
  check random-passes-once FAIL "random_fail=19 peek_fail=20
specimen rows=540 same=yes
FAIL $name (selftest)"
  STUB_SELFTEST='selftest tables=20 derived_pass=20 product_pass=- one_tier_fail=20 random_fail=20 peek_fail=19'
  check wrong-line-with-exit-0 FAIL "FAIL $name (selftest)"
  STUB_SELFTEST_EXIT=1
  check right-line-with-exit-1 FAIL "FAIL $name (selftest)"
  STUB_SELFTEST='selftest tables=19 derived_pass=19 product_pass=- one_tier_fail=19 random_fail=19 peek_fail=19'
  check nineteen-tables FAIL "FAIL $name (selftest)"
  STUB_SELFTEST='selftest tables=20 derived_pass=20 product_pass=19 one_tier_fail=20 random_fail=20 peek_fail=20'
  check product-misses-once FAIL "FAIL $name (selftest)"
  STUB_SELFTEST="$want_line extra=1"
  check line-with-another-key FAIL "FAIL $name (selftest)"
  STUB_SELFTEST=
  check no-line FAIL "FAIL $name (selftest)"
  STUB_FRESH=$work/short.tsv
  check fresh-output-without-a-row FAIL "specimen rows=539 same=no
FAIL $name (specimen)"
  STUB_FRESH=$work/flipped.tsv
  check fresh-output-with-a-pass-flipped FAIL "specimen rows=540 same=no
FAIL $name (specimen)"
  STUB_FRESH=$work/missing.tsv
  check no-fresh-output FAIL "specimen rows=0 same=no
FAIL $name (specimen)"
  cp "$work/short.tsv" "$work/repo/$specimen" || exit 2
  check committed-specimen-without-a-row FAIL "specimen rows=540 same=no
FAIL $name (specimen)"
  STUB_TEST_EXIT=101
  STUB_SELFTEST_EXIT=1
  check three-causes-name-the-first FAIL "FAIL $name (tests)"
  cp "$work/fresh.tsv" "$work/repo/$specimen" || exit 2

  usage=$(sh "$self" --all 2>&1)
  code=$?
  if [ "$code:$usage" = "1:FAIL $name (usage)" ]; then
    printf 'case %s/unknown-argument want=FAIL got=FAIL\n' "$name"
    rejected=$((rejected + 1))
  else
    printf 'case %s/unknown-argument want=FAIL got=exit-%s\n' "$name" "$code"
    wrong="$wrong unknown-argument"
  fi

  if [ -z "${R6_TABLE_NESTED:-}" ]; then
    nested=$(cd crates && R6_TABLE_NESTED=1 sh "../scripts/r6/ci.d/$name.sh" --self-test 2>&1 | tail -n 1)
    case $nested in
      "PASS self-test:$name "*) printf 'case %s/self-test-from-a-subdirectory want=PASS got=PASS\n' "$name" ;;
      *)
        printf 'case %s/self-test-from-a-subdirectory want=PASS got=%s\n' "$name" "$nested"
        wrong="$wrong self-test-from-a-subdirectory"
        ;;
    esac
  fi

  self_test_end
}

self_test_end() {
  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
  exit 0
}

case ${1:-} in
  --self-test) self_test ;;
  '')
    measure
    exit
    ;;
  *)
    printf 'FAIL %s (usage)\n' "$name"
    exit 1
    ;;
esac
