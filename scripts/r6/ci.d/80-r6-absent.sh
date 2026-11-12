#!/bin/sh
# jobs: macos windows
# rule: Nothing of a model request can leave the machine in this build: the ruling file of the commit reads ruling=none, no row of the policy or of the recorded table names a hosted tier, cargo tree of the run's three crates names no network package, the egress lists hold no entry, cargo test of halvo-route in the debug and the release profile prints the ok lines of no_grant_in_release and pinned_files_hold_their_hashes under the target tests/no_grant_in_release.rs, and no cargo command leaves the tree changed.
set -u
name=80-r6-absent
case $0 in
  /*) self=$0 ;;
  *) self=$(pwd)/$0 ;;
esac
tab=$(printf '\t')
policy_header="category${tab}kind${tab}pick${tab}level${tab}basis${tab}k${tab}n"
recorded_header="task${tab}category${tab}kind${tab}tier${tab}model${tab}machine${tab}seed${tab}split${tab}attempt${tab}pass${tab}steps${tab}wall_s${tab}tokens_in${tab}tokens_out${tab}level${tab}source"
net_packages='reqwest hyper ureq rustls ring aws-lc-rs native-tls openssl-sys tokio socket2 mio'
control_want='control tier=low_cost_hosted answer=ruling
control tier=frontier answer=ruling
control cfg_test=yes'

setup_fail() {
  printf 'FAIL %s (setup)\n' "$name"
  exit 1
}

enter_repo() {
  if [ -n "${CI_D_REPO:-}" ]; then
    repo=$CI_D_REPO
  else
    repo=$(git rev-parse --show-toplevel 2>/dev/null) || repo=
  fi
  if [ -z "$repo" ] || ! cd "$repo" 2>/dev/null; then
    setup_fail
  fi
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
  printf 'FAIL %s (%s)\n' "$name" "$2"
  exit 1
}

read_commit() {
  commit=$(git rev-parse --verify --quiet 'HEAD^{commit}') || commit=
  tree_before=$(tree_state 2>&1) || tree_before=
  if [ -z "$commit" ] || [ -z "$tree_before" ]; then
    printf 'tree commit=unreadable\n'
    setup_fail
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
  ' || setup_fail
  committed=$scratch/commit
  mkdir "$committed" || setup_fail
  if ! git -c core.autocrlf=false archive --format=tar -o "$scratch/commit.tar" "$commit" -- crates/halvo-route/data scripts/r6 docs/r6 ||
    ! tar -xf "$scratch/commit.tar" -C "$committed"; then
    printf 'tree commit=unreadable\n'
    setup_fail
  fi
}

ruling_of() {
  printf 'ruling=none\n' >"$scratch/ruling-none"
  if [ -f "$1" ] && cmp -s "$1" "$scratch/ruling-none"; then
    printf 'none\n'
  else
    printf 'other\n'
  fi
}

hosted_rows() {
  if [ ! -e "$1" ] && [ ! -h "$1" ]; then
    printf '%s\n' "$5"
    return
  fi
  if [ ! -f "$1" ] || [ ! -r "$1" ]; then
    printf '1\n'
    return
  fi
  awk -F "$tab" -v header="$2" -v column="$3" -v allowed="$4" '
    BEGIN {
      n = split(allowed, word, " ")
      for (i = 1; i <= n; i++) ok[word[i]] = 1
    }
    NR == 1 && $0 == header { next }
    {
      hosted = !($column in ok)
      for (i = 1; i <= NF; i++) {
        cell = $i
        sub(/\r$/, "", cell)
        if (cell == "low_cost_hosted" || cell == "frontier") hosted = 1
      }
      count += hosted
    }
    END { print count + 0 }
  ' "$1"
}

egress_entries() {
  if [ ! -e "$1" ] && [ ! -h "$1" ]; then
    printf '0\n'
    return
  fi
  find "$1" ! -type d >"$scratch/egress-files.txt" 2>/dev/null || printf '%s\n' "$1/unreadable" >>"$scratch/egress-files.txt"
  awk '
    {
      path = $0
      while ((got = (getline line < path)) > 0) if (line != "[fixed]" && line != "[agent]") count++
      if (got < 0) count++
      close(path)
    }
    END { print count + 0 }
  ' "$scratch/egress-files.txt"
}

net_crates() {
  awk -v packages="$net_packages" '
    BEGIN {
      n = split(packages, word, " ")
      for (i = 1; i <= n; i++) net[word[i]] = 1
    }
    {
      sub(/\r$/, "")
      for (i = 2; i <= NF; i++) {
        if ($i !~ /^v[0-9]/) continue
        package = tolower($(i - 1))
        gsub(/_/, "-", package)
        if (package in net) count++
        break
      }
    }
    END { print count + 0 }
  ' "$1"
}

grant_lines_hold() {
  awk '
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
    { sub(/\r$/, "") }
    /^ *Doc-tests / { target = ""; next }
    /^ *Running / {
      target = target_of($0)
      if (target == "no_grant_in_release tests/no_grant_in_release.rs") lines++
      next
    }
    target != "no_grant_in_release tests/no_grant_in_release.rs" { next }
    $0 == "test no_grant_in_release ... ok" { grant_ok++ }
    $0 == "test pinned_files_hold_their_hashes ... ok" { pins_ok++ }
    END { exit !(lines == 1 && grant_ok == 1 && pins_ok == 1) }
  ' "$1"
}

grants() {
  held=0
  for profile in debug release; do
    started=$(date +%s)
    if [ "$profile" = release ]; then
      cargo test --locked -p halvo-route --release >"$artifacts/test-$profile.txt" 2>&1 </dev/null
    else
      cargo test --locked -p halvo-route >"$artifacts/test-$profile.txt" 2>&1 </dev/null
    fi
    code=$?
    ended=$(date +%s)
    printf '%s %s\n' "$profile" "$((ended - started))" >>"$artifacts/seconds.txt"
    tree_same "after-$profile" grants
    [ "$code" -eq 0 ] && grant_lines_hold "$artifacts/test-$profile.txt" && held=$((held + 1))
  done
  if [ "$held" -eq 2 ]; then
    grant_count=0
  else
    grant_count=1
  fi
}

measure() {
  enter_repo
  artifacts=${CI_D_ARTIFACTS:-target/ci-artifacts/$name}
  if ! mkdir -p "$artifacts" 2>/dev/null || ! scratch=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX" 2>/dev/null); then
    setup_fail
  fi
  : >"$artifacts/seconds.txt" || setup_fail
  CARGO_TERM_QUIET=false
  CARGO_TERM_VERBOSE=false
  CARGO_TERM_COLOR=never
  export CARGO_TERM_QUIET CARGO_TERM_VERBOSE CARGO_TERM_COLOR

  read_commit
  ruling=$(ruling_of "$committed/crates/halvo-route/data/ruling.txt")
  picks=$(hosted_rows "$committed/crates/halvo-route/data/policy.tsv" "$policy_header" 3 'none hand_back on_device' 1)
  rows=$(hosted_rows "$committed/docs/r6/outcomes/recorded.tsv" "$recorded_header" 4 on_device 0)
  entries=$(egress_entries "$committed/scripts/r6/egress")

  cargo tree --locked -e normal --target all --all-features -p halvo-route -p halvo-route-gate -p halvo-route-wire \
    >"$artifacts/tree.txt" 2>"$artifacts/tree.err" </dev/null
  tree_code=$?
  tree_same after-tree crates
  crates=$(net_crates "$artifacts/tree.txt")
  [ "$tree_code" -eq 0 ] || [ "$crates" -gt 0 ] || crates=1

  grants
  value_count=-

  printf 'absent ruling=%s hosted_picks=%s hosted_rows=%s net_crates=%s entries=%s grants=%s test_value=%s\n' \
    "$ruling" "$picks" "$rows" "$crates" "$entries" "$grant_count" "$value_count"
  cause=
  [ "$ruling" = none ] || cause=ruling
  [ "$picks" = 0 ] || cause=${cause:-picks}
  [ "$rows" = 0 ] || cause=${cause:-rows}
  [ "$crates" = 0 ] || cause=${cause:-crates}
  [ "$entries" = 0 ] || cause=${cause:-entries}
  [ "$grant_count" = 0 ] || cause=${cause:-grants}
  if [ -n "$cause" ]; then
    printf 'FAIL %s (%s)\n' "$name" "$cause"
    exit 1
  fi
  printf 'PASS %s\n' "$name"
}

verdict() {
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$3"
  if [ "$3" != "$2" ]; then
    wrong="$wrong $1"
  elif [ "$4" = failing ]; then
    rejected=$((rejected + 1))
  fi
}

reset_stubs() {
  STUB_COMMIT=$work/commit
  STUB_NO_COMMIT=
  STUB_CHANGE_AT=
  STUB_CHANGE_LINE=
  STUB_TREE_OUT=$work/stub/tree.out
  STUB_TREE_EXIT=0
  STUB_DEBUG_OUT=$work/stub/debug.out
  STUB_DEBUG_EXIT=0
  STUB_RELEASE_OUT=$work/stub/release.out
  STUB_RELEASE_EXIT=0
  STUB_PROBE_OUT=$work/stub/probe.out
  STUB_PROBE_EXIT=101
  STUB_CONTROL_OUT=$work/stub/control.out
  STUB_CONTROL_EXIT=0
  STUB_JOB_FLAG=
  CASE_RUSTFLAGS=
  CASE_ENCODED=
  CASE_QUIET=
  : >"$work/tree/status" || exit 2
  : >"$work/tree/flags" || exit 2
}

check() {
  case_name=$1
  want=$2
  shift 2
  out=$(
    CI_D_REPO=$work/repo
    CI_D_ARTIFACTS=$work/artifacts/$case_name
    STUB_TREE=$work/tree
    export CI_D_REPO CI_D_ARTIFACTS STUB_TREE STUB_COMMIT STUB_NO_COMMIT STUB_CHANGE_AT STUB_CHANGE_LINE
    export STUB_TREE_OUT STUB_TREE_EXIT STUB_DEBUG_OUT STUB_DEBUG_EXIT STUB_RELEASE_OUT STUB_RELEASE_EXIT
    export STUB_PROBE_OUT STUB_PROBE_EXIT STUB_CONTROL_OUT STUB_CONTROL_EXIT STUB_JOB_FLAG
    unset RUSTFLAGS CARGO_ENCODED_RUSTFLAGS
    if [ -n "$CASE_RUSTFLAGS" ]; then
      RUSTFLAGS=$CASE_RUSTFLAGS
      export RUSTFLAGS
    fi
    if [ -n "$CASE_ENCODED" ]; then
      CARGO_ENCODED_RUSTFLAGS=$CASE_ENCODED
      export CARGO_ENCODED_RUSTFLAGS
    fi
    if [ -n "$CASE_QUIET" ]; then
      CARGO_TERM_QUIET=$CASE_QUIET
      export CARGO_TERM_QUIET
    fi
    sh "$self" 2>&1
  )
  code=$?
  last=${out##*"$newline"}
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  for text in "$@"; do
    case $out in
      *"$text"*) ;;
      *) got=$got-without-the-text ;;
    esac
  done
  if [ "$want" = FAIL ]; then
    verdict "$case_name" "$want" "$got" failing
  else
    verdict "$case_name" "$want" "$got" passing
  fi
  reset_stubs
}

unit() {
  unit_name=$1
  unit_want=$2
  unit_kind=$3
  shift 3
  unit_got=$("$@") || unit_got=exit-status
  verdict "$unit_name" "$unit_want" "$unit_got" "$unit_kind"
}

direct() {
  if [ "$1:$2" = "1:FAIL $name ($3)" ]; then
    verdict "$4" FAIL FAIL failing
  else
    verdict "$4" FAIL "exit-$1" failing
  fi
}

write_stubs() {
  cat >"$work/bin/git" <<'EOF'
#!/bin/sh
head=1111111111111111111111111111111111111111
case $* in
  'rev-parse --verify --quiet HEAD^{commit}')
    [ -z "$STUB_NO_COMMIT" ] || exit 1
    printf '%s\n' "$head"
    ;;
  "-c core.autocrlf=false archive --format=tar -o "*" $head -- crates/halvo-route/data scripts/r6 docs/r6")
    tar -cf "$6" -C "$STUB_COMMIT" crates/halvo-route/data scripts/r6 docs/r6
    ;;
  '-c core.fsmonitor=false -c core.untrackedCache=false status --porcelain=v2 --branch --untracked-files=all --ignored=matching')
    printf '# branch.oid %s\n' "$head"
    cat "$STUB_TREE/status"
    ;;
  'ls-files -v')
    printf 'H crates/halvo-route/data/ruling.txt\n'
    cat "$STUB_TREE/flags"
    ;;
  *)
    printf 'stub git: got the arguments: %s\n' "$*" >&2
    exit 97
    ;;
esac
EOF
  cat >"$work/bin/cargo" <<'EOF'
#!/bin/sh
kind=other
case $* in
  'tree --locked -e normal --target all --all-features -p halvo-route -p halvo-route-gate -p halvo-route-wire') kind=tree ;;
  'test --locked -p halvo-route') kind=debug ;;
  'test --locked -p halvo-route --release') kind=release ;;
  'generate-lockfile --offline --manifest-path '*'/pair/Cargo.toml') kind=lock ;;
  'run --locked --offline --manifest-path '*'/pair/Cargo.toml --target-dir '*' --bin control'*) kind=control ;;
  'build --locked --offline --manifest-path '*'/pair/Cargo.toml --target-dir '*' --message-format short --bin probe'*) kind=probe ;;
esac
if [ -n "$STUB_CHANGE_AT" ] && [ "$kind" = "$STUB_CHANGE_AT" ]; then
  printf '%s\n' "$STUB_CHANGE_LINE" >>"$STUB_TREE/status"
fi
case $kind in
  tree)
    cat "$STUB_TREE_OUT"
    exit "$STUB_TREE_EXIT"
    ;;
  debug | release)
    [ "${CARGO_TERM_QUIET:-false}" = true ] && exit 0
    if [ "$kind" = debug ]; then
      cat "$STUB_DEBUG_OUT"
      exit "$STUB_DEBUG_EXIT"
    fi
    cat "$STUB_RELEASE_OUT"
    exit "$STUB_RELEASE_EXIT"
    ;;
  lock) exit 0 ;;
  probe | control)
    if [ "${CARGO_ENCODED_RUSTFLAGS+set}" = set ]; then
      flags=$(printf '%s' "$CARGO_ENCODED_RUSTFLAGS" | tr '\037' ' ')
    elif [ "${RUSTFLAGS+set}" = set ]; then
      flags=$RUSTFLAGS
    else
      flags=
      case $* in
        *' --config build.rustflags=["--cfg", "test"]') flags='--cfg test' ;;
      esac
    fi
    reached=yes
    case " $flags " in
      *' --cfg test '*) ;;
      *) reached=no ;;
    esac
    if [ -n "$STUB_JOB_FLAG" ]; then
      case " $flags " in
        *" $STUB_JOB_FLAG "*) ;;
        *) reached=no ;;
      esac
    fi
    if [ "$kind" = probe ]; then
      if [ "$reached" = no ]; then
        printf 'src/bin/probe.rs:4:26: error[E0599]: no function or associated item named test_hosted found for enum `Ruling`\n'
        exit 101
      fi
      cat "$STUB_PROBE_OUT"
      exit "$STUB_PROBE_EXIT"
    fi
    if [ "$reached" = no ]; then
      printf 'control tier=low_cost_hosted answer=ruling\ncontrol tier=frontier answer=ruling\ncontrol cfg_test=no\n'
      exit 0
    fi
    cat "$STUB_CONTROL_OUT"
    exit "$STUB_CONTROL_EXIT"
    ;;
  *)
    printf 'stub cargo: got the arguments: %s\n' "$*"
    exit 97
    ;;
esac
EOF
  chmod +x "$work/bin/git" "$work/bin/cargo" || exit 2
}

write_outputs() {
  stub=$work/stub
  cat >"$stub/debug.out" <<'EOF'
   Compiling halvo-route v0.0.0 (crates/halvo-route)
    Finished test profile [unoptimized + debuginfo] target(s) in 1.00s
     Running unittests src/lib.rs (target/debug/deps/halvo_route-0123456789abcdef)

running 1 test
test ruling::tests::ruling_test_value ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/no_grant_in_release.rs (target/debug/deps/no_grant_in_release-0123456789abcdef)

running 3 tests
test lib_rs_is_the_forbid_line_and_the_module_lines ... ok
test no_grant_in_release ... ok
test pinned_files_hold_their_hashes ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/pick_rules.rs (target/debug/deps/pick_rules-0123456789abcdef)

running 1 test
test pick_rule ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

   Doc-tests halvo_route

running 0 tests
EOF
  sed 's|target/debug/|target/release/|; s|test profile \[unoptimized + debuginfo\]|release profile [optimized]|' "$stub/debug.out" >"$stub/release.out" || exit 2
  printf 'halvo-route v0.0.0 (crates/halvo-route)\n\nhalvo-route-wire v0.0.0 (crates/halvo-route-wire)\n\342\224\234\342\224\200\342\224\200 serde v1.0.229\n\342\224\224\342\224\200\342\224\200 serde_json v1.0.154\n' >"$stub/tree.out" || exit 2
  printf '   Compiling r6-absent-pair v0.0.0 (pair)\nsrc/bin/probe.rs:4:26: error[E0624]: associated function test_hosted is private: private associated function\nerror: could not compile r6-absent-pair (bin "probe") due to 1 previous error\n' >"$stub/probe.out" || exit 2
  printf '%s\n' "$control_want" >"$stub/control.out" || exit 2
  printf 'halvo-route-wire v0.0.0 (crates/halvo-route-wire)\n\342\224\224\342\224\200\342\224\200 reqwest v0.12.0\n' >"$stub/tree-reqwest.out" || exit 2
  awk '$0 != "test no_grant_in_release ... ok"' "$stub/debug.out" >"$stub/debug-no-grant.out" || exit 2
  awk '$0 != "test no_grant_in_release ... ok"' "$stub/release.out" >"$stub/release-no-grant.out" || exit 2
  printf '' >"$stub/empty.out" || exit 2
  printf 'src/bin/probe.rs:4:26: error[E0624]: associated function test_hosted is private\n' >"$stub/probe-text.out" || exit 2
  sed 's/answer=ruling$/answer=grant/' "$stub/control.out" >"$stub/control-grant.out" || exit 2
}

make_commit() {
  mkdir -p "$1/crates/halvo-route/data" "$1/scripts/r6" "$1/docs/r6" || exit 2
  printf 'ruling=none\n' >"$1/crates/halvo-route/data/ruling.txt" || exit 2
  cp crates/halvo-route/data/policy.tsv "$1/crates/halvo-route/data/policy.tsv" || exit 2
}

ruling_units() {
  u=$work/u
  for fixture in other-line:'ruling=hosted\n' without-its-lf:'ruling=none' line-twice:'ruling=none\nruling=none\n' \
    carriage-return:'ruling=none\r\n' leading-space:' ruling=none\n'; do
    printf '%b' "${fixture#*:}" >"$u/ruling-${fixture%%:*}" || exit 2
    unit "ruling-${fixture%%:*}" other failing ruling_of "$u/ruling-${fixture%%:*}"
  done
  unit ruling-missing other failing ruling_of "$u/no-such-file"
  mkdir "$u/ruling-directory" || exit 2
  unit ruling-directory other failing ruling_of "$u/ruling-directory"
  printf 'ruling=none\n' >"$u/ruling-none" || exit 2
  unit ruling-none none passing ruling_of "$u/ruling-none"
}

row_units() {
  u=$work/u
  policy=crates/halvo-route/data/policy.tsv
  awk -F "$tab" -v OFS="$tab" 'NR == 2 { $3 = "frontier"; $4 = "high" } { print }' "$policy" >"$u/policy-frontier" || exit 2
  awk -F "$tab" -v OFS="$tab" 'NR == 3 { $3 = "low_cost_hosted"; $4 = "low" } { print }' "$policy" >"$u/policy-low-cost" || exit 2
  awk -F "$tab" -v OFS="$tab" 'NR == 2 { $3 = "Frontier" } { print }' "$policy" >"$u/policy-unknown-word" || exit 2
  awk -F "$tab" -v OFS="$tab" 'NR == 2 { $4 = "frontier" } { print }' "$policy" >"$u/policy-hosted-level" || exit 2
  awk -F "$tab" -v OFS="$tab" 'NR == 2 { $7 = $7 "\r"; $3 = "frontier\r" } { print }' "$policy" >"$u/policy-hosted-cr" || exit 2
  awk 'NR == 1 { $0 = $0 "\r" } { print }' "$policy" >"$u/policy-header-cr" || exit 2
  awk -F "$tab" -v OFS="$tab" 'NR == 2 { $3 = "on_device"; $4 = "low" } { print }' "$policy" >"$u/policy-on-device" || exit 2
  for fixture in frontier low-cost unknown-word hosted-level hosted-cr header-cr; do
    unit "policy-$fixture" 1 failing hosted_rows "$u/policy-$fixture" "$policy_header" 3 'none hand_back on_device' 1
  done
  unit policy-missing 1 failing hosted_rows "$u/no-such-file" "$policy_header" 3 'none hand_back on_device' 1
  mkdir "$u/policy-directory" || exit 2
  unit policy-directory 1 failing hosted_rows "$u/policy-directory" "$policy_header" 3 'none hand_back on_device' 0
  unit policy-on-device 0 passing hosted_rows "$u/policy-on-device" "$policy_header" 3 'none hand_back on_device' 1
  unit policy-committed 0 passing hosted_rows "$policy" "$policy_header" 3 'none hand_back on_device' 1

  row="bill${tab}utility_bill_payment${tab}compile${tab}on_device${tab}m1${tab}mac${tab}1${tab}dev${tab}1${tab}yes${tab}3${tab}10${tab}5${tab}5${tab}low${tab}s1"
  printf '%s\n%s\n' "$recorded_header" "$row" >"$u/recorded-on-device" || exit 2
  printf '%s\n%s\n' "$recorded_header" "$row" | sed "2s/${tab}on_device${tab}/${tab}frontier${tab}/" >"$u/recorded-frontier" || exit 2
  printf '%s\n%s\n%s\n' "$recorded_header" "$row" "$row" | sed "3s/${tab}on_device${tab}/${tab}low_cost_hosted${tab}/" >"$u/recorded-low-cost" || exit 2
  unit recorded-frontier 1 failing hosted_rows "$u/recorded-frontier" "$recorded_header" 4 on_device 0
  unit recorded-low-cost 1 failing hosted_rows "$u/recorded-low-cost" "$recorded_header" 4 on_device 0
  mkdir "$u/recorded-directory" || exit 2
  unit recorded-directory 1 failing hosted_rows "$u/recorded-directory" "$recorded_header" 4 on_device 0
  unit recorded-on-device 0 passing hosted_rows "$u/recorded-on-device" "$recorded_header" 4 on_device 0
  unit recorded-missing 0 passing hosted_rows "$u/no-such-file" "$recorded_header" 4 on_device 0
}

entry_units() {
  u=$work/u
  for fixture in entry:'[fixed]\n127.0.0.1:1\n[agent]\n' blank-line:'[fixed]\n\n[agent]\n' \
    last-line-without-lf:'[fixed]\n[agent]\nexample.invalid:443' carriage-returns:'[fixed]\r\n[agent]\r\n'; do
    mkdir -p "$u/egress-${fixture%%:*}" || exit 2
    printf '%b' "${fixture#*:}" >"$u/egress-${fixture%%:*}/gate.txt" || exit 2
  done
  unit egress-entry 1 failing egress_entries "$u/egress-entry"
  unit egress-blank-line 1 failing egress_entries "$u/egress-blank-line"
  unit egress-last-line-without-lf 1 failing egress_entries "$u/egress-last-line-without-lf"
  unit egress-carriage-returns 2 failing egress_entries "$u/egress-carriage-returns"
  mkdir -p "$u/egress-nested/agent" || exit 2
  printf '[fixed]\n[agent]\n' >"$u/egress-nested/gate.txt" || exit 2
  printf '[fixed]\n[agent]\nexample.invalid:443\n' >"$u/egress-nested/agent/list.txt" || exit 2
  unit egress-entry-in-a-subdirectory 1 failing egress_entries "$u/egress-nested"
  printf '[fixed]\n[agent]\n127.0.0.1:1\n' >"$u/egress-as-a-file" || exit 2
  unit egress-path-is-a-file 1 failing egress_entries "$u/egress-as-a-file"
  mkdir -p "$u/egress-headings" || exit 2
  printf '[fixed]\n[agent]\n' >"$u/egress-headings/gate.txt" || exit 2
  unit egress-headings-only 0 passing egress_entries "$u/egress-headings"
  unit egress-missing 0 passing egress_entries "$u/no-such-directory"
}

crate_units() {
  u=$work/u
  printf '%s\r\n' 'reqwest v0.12.0' >"$u/tree-all" || exit 2
  printf '%s\n' '|-- hyper v1.0.0' "$(printf '\342\224\234\342\224\200\342\224\200') ureq v2.0.0" '|   `-- rustls v0.23.0 (*)' \
    '    ring v0.17.0' 'aws_lc_rs v1.0.0' 'native-tls v0.2.0' 'openssl-sys v0.9.0' 'Tokio v1.0.0' 'socket2 v0.5.0' \
    'mio v1.0.0 (registry+https://example.invalid)' >>"$u/tree-all" || exit 2
  unit tree-every-listed-package 11 failing net_crates "$u/tree-all"
  printf '%s\n' 'halvo-route-wire v0.0.0 (crates/reqwest)' '|-- tokio-util v0.7.0' '|-- hyper-util v0.1.0' \
    '|-- rustls-pemfile v2.0.0' '|-- ringbuf v0.4.0' '`-- mio-extras v2.0.0' >"$u/tree-near" || exit 2
  unit tree-near-names 0 passing net_crates "$u/tree-near"
  unit tree-committed-form 0 passing net_crates "$work/stub/tree.out"
}

grant_units() {
  u=$work/u
  base=$work/stub/debug.out
  grant_case() {
    if grant_lines_hold "$u/grants-$1"; then
      verdict "grants-$1" "$2" hold "$3"
    else
      verdict "grants-$1" "$2" miss "$3"
    fi
  }
  awk '$0 != "test pinned_files_hold_their_hashes ... ok"' "$base" >"$u/grants-without-the-pins-ok" || exit 2
  awk '$0 != "test no_grant_in_release ... ok"' "$base" >"$u/grants-without-the-grant-ok" || exit 2
  awk '/^test (no_grant_in_release|pinned_files_hold_their_hashes) \.\.\. ok$/ { held = held $0 "\n"; next } { print } $0 == "test pick_rule ... ok" { printf "%s", held }' "$base" >"$u/grants-under-the-next-target" || exit 2
  awk '!/Running tests\/no_grant_in_release\.rs /' "$base" >"$u/grants-without-the-target-line" || exit 2
  awk '{ print } /Running tests\/no_grant_in_release\.rs / { print }' "$base" >"$u/grants-target-line-twice" || exit 2
  awk '{ sub(/deps\/no_grant_in_release-/, "deps/pick_rules-") } { print }' "$base" >"$u/grants-other-binary" || exit 2
  awk '{ sub(/Running tests\/no_grant_in_release\.rs /, "Running tests/forge.rs ") } { print }' "$base" >"$u/grants-other-source" || exit 2
  awk '$0 == "test no_grant_in_release ... ok" { print "   Doc-tests halvo_route" } { print }' "$base" >"$u/grants-after-a-doc-tests-line" || exit 2
  awk '$0 == "test pinned_files_hold_their_hashes ... ok" { $0 = "test pinned_files_hold_their_hashes - should panic ... ok" } { print }' "$base" >"$u/grants-should-panic" || exit 2
  awk '{ print } $0 == "test no_grant_in_release ... ok" { print }' "$base" >"$u/grants-ok-line-twice" || exit 2
  awk '/Running tests\/no_grant_in_release\.rs / { sub(/\)$/, "") } { print }' "$base" >"$u/grants-target-line-without-its-paren" || exit 2
  for fixture in without-the-pins-ok without-the-grant-ok under-the-next-target without-the-target-line target-line-twice \
    other-binary other-source after-a-doc-tests-line should-panic ok-line-twice target-line-without-its-paren; do
    grant_case "$fixture" miss failing
  done
  awk '/^ *Running / { gsub(/\//, "\\"); sub(/\)$/, ".exe)") } { printf "%s\r\n", $0 }' "$base" >"$u/grants-windows-form" || exit 2
  grant_case windows-form hold passing
  cp "$work/stub/release.out" "$u/grants-release-form" || exit 2
  grant_case release-form hold passing
}

full_cases() {
  check all-hold PASS 'absent ruling=none hosted_picks=0 hosted_rows=0 net_crates=0 entries=0 grants=0 test_value=-'

  for variant in ruling policy recorded egress; do
    mkdir "$work/commit-$variant" || exit 2
    make_commit "$work/commit-$variant"
  done
  printf 'ruling=hosted\n' >"$work/commit-ruling/crates/halvo-route/data/ruling.txt" || exit 2
  STUB_COMMIT=$work/commit-ruling
  check ruling-file-with-another-line FAIL 'absent ruling=other hosted_picks=0 ' "FAIL $name (ruling)"
  cp "$work/u/policy-frontier" "$work/commit-policy/crates/halvo-route/data/policy.tsv" || exit 2
  STUB_COMMIT=$work/commit-policy
  check policy-pick-raised-to-frontier FAIL ' hosted_picks=1 hosted_rows=0 ' "FAIL $name (picks)"
  mkdir -p "$work/commit-recorded/docs/r6/outcomes" || exit 2
  cp "$work/u/recorded-frontier" "$work/commit-recorded/docs/r6/outcomes/recorded.tsv" || exit 2
  STUB_COMMIT=$work/commit-recorded
  check recorded-row-of-a-hosted-tier FAIL ' hosted_rows=1 net_crates=0 ' "FAIL $name (rows)"
  mkdir -p "$work/commit-egress/scripts/r6/egress" || exit 2
  printf '[fixed]\n127.0.0.1:1\n[agent]\n' >"$work/commit-egress/scripts/r6/egress/gate.txt" || exit 2
  STUB_COMMIT=$work/commit-egress
  check egress-list-with-an-entry FAIL ' entries=1 grants=0 ' "FAIL $name (entries)"

  STUB_TREE_OUT=$work/stub/tree-reqwest.out
  check tree-names-a-network-package FAIL ' net_crates=1 entries=0 ' "FAIL $name (crates)"
  STUB_TREE_EXIT=101
  STUB_TREE_OUT=$work/stub/empty.out
  check tree-fails FAIL ' net_crates=1 entries=0 ' "FAIL $name (crates)"

  STUB_DEBUG_OUT=$work/stub/debug-no-grant.out
  check debug-without-the-grant-ok-line FAIL ' grants=1 test_value=-' "FAIL $name (grants)"
  STUB_RELEASE_OUT=$work/stub/release-no-grant.out
  check release-without-the-grant-ok-line FAIL ' grants=1 test_value=-' "FAIL $name (grants)"
  STUB_DEBUG_EXIT=101
  check debug-run-fails FAIL ' grants=1 test_value=-' "FAIL $name (grants)"
  STUB_RELEASE_EXIT=101
  check release-run-fails FAIL ' grants=1 test_value=-' "FAIL $name (grants)"

  CASE_QUIET=true
  check job-sets-cargo-quiet PASS ' grants=0 test_value=-'

  for kind in tree:crates debug:grants release:grants; do
    STUB_CHANGE_AT=${kind%%:*}
    STUB_CHANGE_LINE='? .cargo/config.toml'
    case ${kind%%:*} in
      *) when=${kind%%:*} ;;
    esac
    check "tree-changed-by-$STUB_CHANGE_AT" FAIL "tree changed=after-$when ? .cargo/config.toml" "FAIL $name (${kind#*:})"
  done
  printf '1 .M N... 100644 100644 100644 0000 0000 crates/halvo-route/data/ruling.txt\n' >"$work/tree/status" || exit 2
  check changed-file-before-the-run FAIL 'tree changed=before 1 crates/halvo-route/data/ruling.txt' "FAIL $name (setup)"
  printf '? notes.txt\n! target/\n' >"$work/tree/status" || exit 2
  check untracked-and-ignored-before-the-run PASS ' test_value=-'
  STUB_NO_COMMIT=1
  check no-commit FAIL 'tree commit=unreadable' "FAIL $name (setup)"
  STUB_COMMIT=$work/commit-ruling
  STUB_RELEASE_EXIT=101
  check first-cause-named FAIL 'absent ruling=other ' ' grants=1 test_value=-' "FAIL $name (ruling)"

  usage=$(sh "$self" --all 2>&1)
  direct "$?" "$usage" usage unknown-argument
  unentered=$(CI_D_REPO=$work/no-such-directory sh "$self" 2>&1)
  direct "$?" "$unentered" setup cannot-enter-the-repository
  : >"$work/a-file" || exit 2
  unmade=$(CI_D_REPO=$work/repo CI_D_ARTIFACTS=$work/a-file/artifacts sh "$self" 2>&1)
  direct "$?" "$unmade" setup cannot-make-a-directory
}

self_test() {
  enter_repo
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || setup_fail
  newline='
'
  scratch=$work/u
  mkdir -p "$work/bin" "$work/repo" "$work/artifacts" "$work/stub" "$work/tree" "$work/u" "$work/commit" || setup_fail
  make_commit "$work/commit"
  write_stubs
  PATH=$work/bin:$PATH
  export PATH
  write_outputs
  reset_stubs
  rejected=0
  wrong=
  ruling_units
  row_units
  entry_units
  crate_units
  grant_units
  full_cases
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
