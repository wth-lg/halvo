#!/bin/sh
# jobs: macos windows
# rule: The router's tests pass, each test of scripts/r6/required-tests.txt prints ok under its own target and stands in its test binary's list and not among its ignored tests, each of the five pinned files hashes by the system's own tool to its entry in PINNED of tests/no_grant_in_release.rs, the gate tool's self-test fails the three bad routers and passes the derived one on 20 generated tables, and the tool regenerates the committed specimen.
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
pin_file=crates/halvo-route/tests/no_grant_in_release.rs
want_line='selftest tables=20 derived_pass=20 product_pass=- one_tier_fail=20 random_fail=20 peek_fail=20'
want_product='selftest tables=20 derived_pass=20 product_pass=20 one_tier_fail=20 random_fail=20 peek_fail=20'

enter_repo() {
  if [ -n "${CI_D_REPO:-}" ]; then
    repo=$CI_D_REPO
  else
    repo=$(git rev-parse --show-toplevel) || exit 2
  fi
  cd "$repo" || exit 2
}

list_target() {
  package=$1
  source=$2
  stem=${source#tests/}
  stem=${stem%.rs}
  if [ "$source" = src/lib.rs ]; then
    stem=lib
    set -- --lib
  else
    set -- --test "$stem"
  fi
  listing=$artifacts/list-$package-$stem
  cargo test --locked -p "$package" "$@" -- --list >"$listing.txt" 2>"$listing.err" </dev/null
  list_code=$?
  cargo test --locked -p "$package" "$@" -- --list --ignored >"$listing-ignored.txt" 2>>"$listing.err" </dev/null
  ignored_code=$?
  awk -v target="$package $source" '($1 " " $2) == target && !seen[$3]++ { print $3 }' "$wanted" >"$listing-names.txt"
  set -- "$@" -- --exact
  while read -r test_name; do
    set -- "$@" "$test_name"
  done <"$listing-names.txt"
  cargo test --locked -p "$package" "$@" >"$listing-run.txt" 2>&1 </dev/null
  run_code=$?
  awk -v target="$package $source" -v run_code="$run_code" '
    FILENAME == ARGV[1] {
      sub(/\r$/, "")
      if ($0 ~ /^test [^ ]+ \.\.\. ok$/) passed[substr($0, 6, length($0) - 12)] = 1
      if ($0 ~ /^test result: ok\. /) summary = 1
      next
    }
    ($1 " " $2) != target || seen[$0]++ { next }
    run_code == 0 && summary && ($3 in passed) { print "ran " $0 }
  ' "$listing-run.txt" "$wanted" >>"$result"
  awk -v target="$package $source" -v list_code="$list_code" -v ignored_code="$ignored_code" '
    FILENAME == ARGV[1] {
      if ($0 ~ /: test$/) listed[substr($0, 1, length($0) - 6)] = 1
      next
    }
    FILENAME == ARGV[2] {
      if ($0 ~ /: test$/) ignored[substr($0, 1, length($0) - 6)] = 1
      next
    }
    ($1 " " $2) != target || seen[$0]++ { next }
    {
      if (list_code == 0) print (($3 in listed) ? "listed " : "not_listed ") $0
      if (ignored_code == 0) print (($3 in ignored) ? "ignored " : "clean ") $0
    }
  ' "$listing.txt" "$listing-ignored.txt" "$wanted" >>"$result"
}

require_tests() {
  wanted=$scratch/required.txt
  result=$scratch/required-result.txt
  if [ ! -s "$required" ] || ! { printf '%s\n' "$fixed_tests" && cat "$required"; } >"$wanted" 2>/dev/null ||
    ! awk '!/^halvo-route(-gate)? (src\/lib|tests\/[a-z0-9_]+)\.rs [A-Za-z0-9_:]+$/ { bad = 1 } END { exit bad }' "$wanted"; then
    printf 'required list=bad\n'
    return 1
  fi
  awk '
    FILENAME == ARGV[1] {
      sub(/\r$/, "")
      if ($0 ~ /^ *Doc-tests /) {
        target = ""
      } else if ($0 ~ /^ *Running /) {
        line = $0
        sub(/^ *Running /, "", line)
        gsub(/\\/, "/", line)
        target = ""
        paren = index(line, " (")
        if (paren > 0 && line ~ /\)$/) {
          bin = substr(line, paren + 2, length(line) - paren - 2)
          sub(/.*\//, "", bin)
          sub(/\.exe$/, "", bin)
          sub(/-[0-9a-f]+$/, "", bin)
          target = bin " " substr(line, 1, paren - 1)
        }
      } else if (target != "" && $0 ~ /^test [^ ]+ \.\.\. ok$/) {
        ran[target " " substr($0, 6, length($0) - 12)] = 1
      }
      next
    }
    {
      if ($2 == "src/lib.rs") {
        bin = $1
        gsub(/-/, "_", bin)
        printed = "unittests src/lib.rs"
      } else {
        bin = $2
        sub(/^tests\//, "", bin)
        sub(/\.rs$/, "", bin)
        printed = $2
      }
      if ((bin " " printed " " $3) in ran) print "ok " $0
    }
  ' "$artifacts/cargo-test.txt" "$wanted" >"$result"
  awk '!seen[$1 " " $2]++ { print $1, $2 }' "$wanted" >"$scratch/required-targets.txt"
  while read -r package source; do
    list_target "$package" "$source"
  done <"$scratch/required-targets.txt"
  awk '
    FILENAME == ARGV[1] {
      if (!($0 in wanted)) {
        wanted[$0] = 1
        order[++tests] = $0
      }
      next
    }
    { got[$0] = 1 }
    END {
      for (i = 1; i <= tests; i++) {
        entry = order[i]
        if (("ok " entry) in got) ok++
        else print "required not_ok=" entry
        if (("listed " entry) in got) listed++
        else if (("not_listed " entry) in got) print "required not_listed=" entry
        else print "required no_list=" entry
        if (("clean " entry) in got) clean++
        else if (("ignored " entry) in got) print "required ignored=" entry
        else print "required no_ignored_list=" entry
        if (("ran " entry) in got) ran++
        else print "required not_run=" entry
      }
      printf "required tests=%d ok=%d listed=%d ignored=%d ran=%d\n", tests, ok, listed, tests - clean, ran
      exit (ok != tests || listed != tests || clean != tests || ran != tests)
    }
  ' "$wanted" "$result"
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
    pinned sha256sum -b >"$scratch/hashes.txt" 2>"$artifacts/hashes.err"
  else
    pinned shasum -a 256 -b >"$scratch/hashes.txt" 2>"$artifacts/hashes.err"
  fi
  paths=$(pinned echo) || return 1
  pins_source=$pin_file
  if [ ! -f "$pins_source" ]; then
    pins_source=$scratch/no-pins.txt
    : >"$pins_source"
  fi
  awk -v paths="$paths" -v tool="$tool" '
    FILENAME == ARGV[1] {
      file = $2
      sub(/^\*/, "", file)
      if (length($1) == 64 && $1 ~ /^[0-9a-f]+$/) hash[file] = $1
      next
    }
    /^[ \t]*(pub(\([^)]*\))? )?(const|static) PINNED[^A-Za-z0-9_]/ { declared++ }
    /^const PINNED/ {
      blocks++
      inside = 1
      if (previous ~ /^[ \t]*#/) broken = 1
    }
    $0 !~ /^[ \t]*$/ { previous = $0 }
    inside {
      line = $0
      sub(/\/\/.*/, "", line)
      if (line ~ /\/\*/) broken = 1
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
      if (declared != 1 || blocks != 1 || broken) {
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
  mkdir -p "$artifacts" || exit 2
  scratch=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  cause=

  if ! cargo test --locked -p halvo-route -p halvo-route-gate >"$artifacts/cargo-test.txt" 2>&1; then
    cause=tests
  fi
  require_tests || cause=tests
  hold_pins || cause=tests

  cargo run -q --locked -p halvo-route-gate --bin route-gate -- self-test --tables 20 --seed 1 \
    >"$artifacts/selftest.out" 2>"$artifacts/selftest.err"
  selftest_code=$?
  grep '^selftest ' "$artifacts/selftest.out"
  last=$(tail -n 1 "$artifacts/selftest.out")
  if [ "$selftest_code" -ne 0 ] || { [ "$last" != "$want_line" ] && [ "$last" != "$want_product" ]; }; then
    cause=${cause:-selftest}
  fi

  fresh=$scratch/specimen.tsv
  cargo run -q --locked -p halvo-route-gate --bin route-gate -- generate --seed 1 --out "$fresh" \
    >"$artifacts/generate.out" 2>"$artifacts/generate.err"
  rows=0
  same=no
  if [ -f "$fresh" ]; then
    lines=$(grep -c '' "$fresh")
    [ "$lines" -gt 0 ] && rows=$((lines - 1))
    cmp -s "$fresh" "$specimen" && same=yes
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
}

stub_case() {
  STUB_DIR=$work/stub-$1
  mkdir "$STUB_DIR" || exit 2
  cp "$work/stub/"* "$STUB_DIR/" || exit 2
}

copy_pristine() {
  for path in "$@"; do
    cp "$path" "$work/pristine/$path" || exit 2
  done
}

restore_repo() {
  cp -R "$work/pristine/." "$work/repo/" || exit 2
}

make_stub() {
  mkdir -p "$work/stub" "$work/pristine/scripts/r6" "$work/pristine/crates/halvo-route/src" \
    "$work/pristine/crates/halvo-route/tests" || exit 2
  cp "$required" "$work/pristine/$required" || exit 2
  cp "$pin_file" "$work/pristine/$pin_file" || exit 2
  pinned copy_pristine
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
  mv "$work/repo/$required" "$work/repo/$required.away" || exit 2
  check no-list FAIL 'required list=bad'
  restore_repo

  stub_case carriage-returns
  awk '{ printf "%s\r\n", $0 }' "$work/stub/test.out" >"$STUB_DIR/test.out" || exit 2
  check carriage-returns PASS 'required tests=21 ok=21 listed=21 ignored=0 ran=21'

  stub_case windows-paths
  awk '/^ *Running / { n = split($0, part, "/"); $0 = part[1]; for (i = 2; i <= n; i++) $0 = $0 "\\" part[i]; sub(/\)$/, ".exe)") } { print }' "$work/stub/test.out" >"$STUB_DIR/test.out" || exit 2
  check windows-paths PASS 'required tests=21 ok=21 listed=21 ignored=0 ran=21'

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

  awk '{ print } END { print "const OLD: [(&str, &str); 1] = [(\"src/tier.rs\", \"0000000000000000000000000000000000000000000000000000000000000000\")];" }' "$pins" >"$work/repo/$pin_file" || exit 2
  check tuple-after-the-pinned-list PASS 'pins tool=' ' files=5 same=5'
  restore_repo

  mv "$work/repo/$pin_file" "$work/repo/$pin_file.away" || exit 2
  check no-pin-test-file FAIL 'pins unreadable=PINNED'
  restore_repo

  mkdir "$work/bare" || exit 2
  for tool in awk cat cmp cp grep mkdir mktemp sh tail; do
    real=$(command -v "$tool") || exit 2
    printf '#!/bin/sh\nexec %s "$@"\n' "$real" >"$work/bare/$tool" || exit 2
    chmod +x "$work/bare/$tool" || exit 2
  done
  STUB_PATH=$work/bare
  check no-hash-tool FAIL 'pins tool=none'
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
  cat >"$work/bin/cargo" <<'EOF'
#!/bin/sh
case $* in
  'test --locked -p halvo-route -p halvo-route-gate')
    cat "$STUB_DIR/test.out"
    exit "$STUB_TEST_EXIT"
    ;;
  'test --locked -p '*' --lib -- --exact '* | 'test --locked -p '*' --test '*' -- --exact '*)
    stem=lib
    [ "$5" = --test ] && stem=$6
    case " $STUB_NOT_RUN " in
      *" $stem "*) exit 0 ;;
    esac
    while [ "$1" != --exact ]; do
      shift
    done
    shift
    for test_name in "$@"; do
      printf 'test %s ... ok\n' "$test_name"
    done
    case " $STUB_NO_SUMMARY " in
      *" $stem "*) ;;
      *) printf '\ntest result: ok. %s passed; 0 failed\n' "$#" ;;
    esac
    exit "${STUB_RUN_EXIT:-0}"
    ;;
  'test --locked -p '*' --lib -- --list' | 'test --locked -p '*' --lib -- --list --ignored' | \
    'test --locked -p '*' --test '*' -- --list' | 'test --locked -p '*' --test '*' -- --list --ignored')
    package=$4
    stem=lib
    [ "$5" = --test ] && stem=$6
    case " $STUB_NO_TARGETS " in
      *" $stem "*)
        printf 'error: no test target named %s\n' "$stem" >&2
        exit 101
        ;;
    esac
    case $* in
      *' --ignored')
        case " $STUB_NO_IGNORED " in
          *" $stem "*) exit 101 ;;
        esac
        cat "$STUB_DIR/ignored-$package-$stem.txt"
        ;;
      *) cat "$STUB_DIR/list-$package-$stem.txt" ;;
    esac
    ;;
  'run -q --locked -p halvo-route-gate --bin route-gate -- self-test --tables 20 --seed 1')
    [ -z "$STUB_SELFTEST" ] || printf '%s\n' "$STUB_SELFTEST"
    exit "$STUB_SELFTEST_EXIT"
    ;;
  'run -q --locked -p halvo-route-gate --bin route-gate -- generate --seed 1 --out '*)
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
  restore_repo
  rejected=0
  wrong=
  STUB_TEST_EXIT=0
  STUB_SELFTEST=$want_line
  STUB_SELFTEST_EXIT=0
  STUB_FRESH=$work/fresh.tsv

  check all-hold PASS 'required tests=21 ok=21 listed=21 ignored=0 ran=21' ' files=5 same=5' "$want_line
specimen rows=540 same=yes
PASS $name"
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

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  '') measure ;;
  *)
    printf 'FAIL %s (usage)\n' "$name"
    exit 1
    ;;
esac
