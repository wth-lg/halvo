#!/bin/sh
# jobs: macos windows
# rule: Every command that the route-gate binary built at the commit answers to stands on one of two lists of this script, and each command of the first list, run with its arguments under scripts/egress/check.sh with scripts/r6/egress/gate.txt, exits 0 with denied=0 while every file under scripts/r6/egress/ in the commit holds no entry line and the run leaves no tracked file changed; a pass says no more than the sentence of the egress check's item 5 that it prints, and it does not hold a process start, which the test probe::tests::the_probe_names_no_process holds by a read of text, nor the three limits that --help names.
set -u
name=60-r6-egress
list=scripts/r6/egress/gate.txt
trial=docs/r2/trials/egress.md
probe_word=--r6-egress-no-such-argument
unknown='REFUSED usage: unknown command'
nl='
'
case $0 in
  /*) self=$0 ;;
  *) self=$(pwd)/$0 ;;
esac

help() {
  cat <<'EOF'
sh scripts/r6/ci.d/60-r6-egress.sh [--self-test | --help]
Builds route-gate at the commit, asks the binary which words it answers to as commands, and holds
each such command on one of two lists of this script: the commands that it runs under
scripts/egress/check.sh with scripts/r6/egress/gate.txt, each with its arguments, and the commands
that it cannot run on files of the repository alone, each with its reason. A command of the binary
on neither list fails the check, and so does a list entry that is no command of the binary.
Prints: egress checks=<n> denied=<n> entries=<n>, with checks the number of commands of the first list.
A word is a command when the binary does not answer it with "REFUSED usage: unknown command"; the
words asked are the string literals of the tracked Rust files outside tests/ of the crates that the
binary links, read from the commit; each is asked with one argument that no command takes, outside
the egress check.
The named limits (contracts.md T6.1, SG-44 (4)), which no run of this check holds:
1. a process start inside a third-party crate that the binary links; the check prints their names
   from the lock at the commit on its line "egress limit third-party crates=";
2. a start by a name that is built at run time;
3. a command of the second list, which the read of text alone holds until its files exist.
A process start is held by no run: the test probe::tests::the_probe_names_no_process reads every
tracked source file of the crates that the binary links, and that read is the hold.
EOF
}

run_list() {
  cat <<'EOF'
generate --seed 1 --out @/table.tsv
derive --table @/table.tsv --out @/policy.tsv
score --table @/table.tsv --router product --policy @/policy.tsv
self-test --tables 20 --seed 1
spend-sim --seed 1
probe --rounds 1000
EOF
}

cannot_list() {
  cat <<'EOF'
import its inputs are the agent-setup run's trial files, which reach the repository with U3.1
EOF
}

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

enter_repo() {
  if [ -n "${CI_D_REPO:-}" ]; then
    repo=$CI_D_REPO
  else
    repo=$(git rev-parse --show-toplevel 2>/dev/null) || fail setup
  fi
  cd "$repo" 2>/dev/null || fail setup
}

platform_of() {
  system=$(uname -s 2>/dev/null) || system=
  case $system in
    Darwin)
      if [ "${GITHUB_ACTIONS:-}" = true ]; then
        platform=macos-15
      else
        platform=this-mac
      fi
      ;;
    MINGW* | MSYS* | CYGWIN*) platform=windows-2025 ;;
    *) platform=none ;;
  esac
}

tree_changes() {
  git -c core.fsmonitor=false diff --no-renames --name-status "$tree" -- >"$1" 2>&1
}

report_changes() {
  awk -v when="$1" 'NR <= 5 { printf "tree changed=%s %s %s\n", when, $1, $NF }' "$2"
}

built_route_gate() {
  awk '
    function field(key, value) {
      if (!match($0, "\"" key "\":\"[^\"]*\"")) return ""
      value = substr($0, RSTART + length(key) + 4, RLENGTH - length(key) - 5)
      gsub(/\\\\/, "/", value)
      return value
    }
    index($0, "{\"reason\":\"compiler-artifact\",") != 1 { next }
    {
      if (index($0, "\"fresh\":true") == 0) stale = 1
      dir = field("manifest_path")
      sub(/\/[^\/]*$/, "", dir)
      if (dir !~ /\/crates\/halvo-route-gate$/ || field("src_path") != dir "/src/main.rs") next
      n++
      binary = field("executable")
    }
    END {
      if (stale) print "stale"
      else if (n != 1) print "lines " n + 0
      else print "binary " binary
    }
  ' "$1"
}

count_entries() {
  : >"$work/entries.txt"
  git ls-tree -r "$tree" -- scripts/r6/egress >"$work/egress-listing.txt" 2>/dev/null || fail setup
  odd=0
  k=0
  tab=$(printf '\t')
  while IFS= read -r line || [ -n "$line" ]; do
    meta=${line%%"$tab"*}
    set -f
    # shellcheck disable=SC2086
    set -- $meta
    set +f
    case $1:$2 in
      100644:blob | 100755:blob)
        k=$((k + 1))
        git cat-file blob "$3" >"$work/egress-$k.txt" 2>/dev/null || fail setup
        printf '%s\n' "$work/egress-$k.txt" >>"$work/entries.txt"
        ;;
      *) odd=$((odd + 1)) ;;
    esac
  done <"$work/egress-listing.txt"
  entries=$(awk '
    {
      path = $0
      while ((got = (getline line < path)) > 0) {
        if (line != "[fixed]" && line != "[agent]") count++
      }
      if (got < 0) count++
      close(path)
    }
    END { print count + 0 }
  ' "$work/entries.txt") || fail setup
  entries=$((entries + odd))
}

read_graph() {
  cargo tree --locked -e normal,no-proc-macro --target all -p halvo-route-gate --prefix none --format '{p}' \
    >"$artifacts/cargo-tree.txt" 2>"$artifacts/cargo-tree.err" || fail setup
  awk '
    / \(\*\)$/ { sub(/ \(\*\)$/, "") }
    / \(.*\)$/ {
      path = $0
      sub(/^[^(]*\(/, "", path)
      sub(/\)$/, "", path)
      gsub(/\\/, "/", path)
      print "crate " path
      next
    }
    NF >= 2 { print "third " $1 }
  ' "$artifacts/cargo-tree.txt" | sort -u >"$work/graph.txt" || fail setup
  crates=0
  third=
  third_count=0
  while IFS=' ' read -r kind value; do
    case $kind in
      crate) crates=$((crates + 1)) ;;
      third)
        third_count=$((third_count + 1))
        third=${third:+$third,}$value
        ;;
    esac
  done <"$work/graph.txt"
  [ "$crates" -gt 0 ] || fail setup
}

read_candidates() {
  : >"$work/grep.txt"
  while IFS=' ' read -r kind value; do
    [ "$kind" = crate ] || continue
    git grep -I -o -E '"[a-z][a-z0-9-]*"' "$tree" -- "$value" >>"$work/grep.txt" 2>"$work/grep.err"
    case $? in
      0 | 1) ;;
      *) fail setup ;;
    esac
  done <"$work/graph.txt"
  awk '
    {
      rest = substr($0, index($0, ":") + 1)
      quote = index(rest, ":\"")
      path = substr(rest, 1, quote - 1)
      word = substr(rest, quote + 2)
      sub(/"$/, "", word)
      if (path !~ /\.rs$/ || path ~ /(^|\/)tests\//) next
      print word
    }
  ' "$work/grep.txt" | sort -u >"$work/candidates.txt" || fail setup
}

discover() {
  : >"$work/commands.txt"
  asked=0
  while IFS= read -r word || [ -n "$word" ]; do
    asked=$((asked + 1))
    (cd "$work/discover" && "$exe" "$word" "$probe_word" >"$work/ask.out" 2>"$work/ask.err" </dev/null)
    code=$?
    answer=
    while IFS= read -r line || [ -n "$line" ]; do
      answer=$answer$line$nl
    done <"$work/ask.err"
    if [ "$code" -ne 2 ] || [ -s "$work/ask.out" ] || [ "$answer" != "$unknown$nl" ]; then
      printf '%s\n' "$word" >>"$work/commands.txt"
    fi
  done <"$work/candidates.txt"
}

compare_lists() {
  run_list >"$work/run-list.txt" || fail setup
  cannot_list >"$work/cannot-list.txt" || fail setup
  awk '{ print $1 }' "$work/run-list.txt" "$work/cannot-list.txt" | sort >"$work/listed.txt" || fail setup
  awk '{ print $1 }' "$work/run-list.txt" | sort -u >"$work/run-names.txt" || fail setup
  checks=$(awk 'END { print NR }' "$work/run-names.txt") || fail setup
  matched=yes
  awk 'seen[$0]++ == 1' "$work/listed.txt" >"$work/twice.txt" || fail setup
  while IFS= read -r word; do
    printf 'egress listed-twice=%s\n' "$word"
    matched=no
  done <"$work/twice.txt"
  sort -u "$work/listed.txt" >"$work/listed-once.txt" || fail setup
  commands=$(awk 'END { print NR }' "$work/commands.txt") || fail setup
  while IFS= read -r word; do
    if ! grep -q -x -F -e "$word" "$work/listed-once.txt"; then
      printf 'egress on-neither-list=%s\n' "$word"
      matched=no
    fi
  done <"$work/commands.txt"
  while IFS= read -r word; do
    if ! grep -q -x -F -e "$word" "$work/commands.txt"; then
      printf 'egress no-command=%s\n' "$word"
      matched=no
    fi
  done <"$work/listed-once.txt"
  cannot=$(awk 'END { print NR }' "$work/cannot-list.txt") || fail setup
  printf 'egress commands=%s asked=%s run=%s cannot=%s crates=%s\n' "$commands" "$asked" "$checks" "$cannot" "$crates"
  while IFS=' ' read -r word reason; do
    printf 'egress cannot=%s reason=%s\n' "$word" "$reason"
  done <"$work/cannot-list.txt"
}

last_line() {
  last=
  while IFS= read -r line || [ -n "$line" ]; do
    last=$line
  done <"$1"
}

run_checks() {
  denied=0
  ran=yes
  refused_mode=no
  i=0
  while IFS= read -r entry || [ -n "$entry" ]; do
    i=$((i + 1))
    word=${entry%% *}
    if ! grep -q -x -F -e "$word" "$work/commands.txt"; then
      continue
    fi
    set -f
    # shellcheck disable=SC2086
    set -- $entry
    set +f
    shift
    words=
    for arg in "$@"; do
      case $arg in
        @/*) arg=$work/out${arg#@} ;;
      esac
      words="$words$arg$nl"
    done
    set --
    while IFS= read -r arg; do
      set -- "$@" "$arg"
    done <<EOF
${words%"$nl"}
EOF
    sh scripts/egress/check.sh "$list" -- "$exe" "$word" "$@" \
      >"$artifacts/run-$i.out" 2>"$artifacts/run-$i.err" </dev/null
    code=$?
    last_line "$artifacts/run-$i.err"
    rest=${last#"egress mode="}
    run_mode=${rest%% *}
    rest=${rest#*" list=$list command="}
    status=${rest%% *}
    rest=${rest#*" denied="}
    count=${rest%% *}
    log=${rest#*" log="}
    case $last in
      "REFUSED mode: "*)
        refused_mode=yes
        ran=no
        printf 'egress run=%s refused=mode\n' "$word"
        continue
        ;;
      "egress mode=$run_mode list=$list command=$status denied=$count log=$log") ;;
      *)
        ran=no
        printf 'egress run=%s line=missing exit=%s\n' "$word" "$code"
        continue
        ;;
    esac
    case $count in
      '' | *[!0-9]*)
        ran=no
        printf 'egress run=%s line=missing exit=%s\n' "$word" "$code"
        continue
        ;;
    esac
    denied=$((denied + count))
    printf 'egress run=%s mode=%s command=%s denied=%s exit=%s\n' "$word" "$run_mode" "$status" "$count" "$code"
    if [ "$status" != 0 ]; then
      ran=no
    elif [ "$count" -eq 0 ] && [ "$code" -ne 0 ]; then
      ran=no
    elif [ "$count" -ne 0 ] && [ "$code" -ne 1 ]; then
      ran=no
    fi
  done <"$work/run-list.txt"
}

not_observed() {
  git show "$tree:$trial" >"$work/trial.txt" 2>/dev/null || fail setup
  paths=$(awk -v platform="$platform" '
    ($1 == "not-observable" || $1 == "not-observed") && $2 == platform {
      path = $4
      sub(/:$/, "", path)
      out = out (out == "" ? "" : ", ") path
    }
    END { print out }
  ' "$work/trial.txt") || fail setup
}

check() {
  enter_repo
  artifacts=${CI_D_ARTIFACTS:-target/ci-artifacts/$name}
  [ -d "$artifacts" ] || mkdir -p "$artifacts" 2>/dev/null || fail setup
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail setup
  mkdir -p "$work/out" "$work/discover" || fail setup
  platform_of

  tree=$(git rev-parse --verify --quiet 'HEAD^{tree}' 2>/dev/null) || fail setup
  [ -n "$tree" ] || fail setup
  tree_changes "$work/before.txt" || fail setup
  if [ -s "$work/before.txt" ]; then
    report_changes before "$work/before.txt"
    fail setup
  fi
  count_entries
  not_observed

  cargo build --locked -p halvo-route-gate --bin route-gate >"$artifacts/cargo-build.txt" 2>&1 || fail setup
  cargo build --locked -p halvo-route-gate --bin route-gate --message-format=json \
    >"$artifacts/cargo-build.json" 2>"$artifacts/cargo-build-json.err" || fail setup
  built_route_gate "$artifacts/cargo-build.json" >"$work/built.txt" || fail setup
  IFS= read -r built <"$work/built.txt" || built=
  case $built in
    "binary "*) exe=${built#binary } ;;
    *) fail setup ;;
  esac
  [ -f "$exe" ] || fail setup

  read_graph
  read_candidates
  discover
  compare_lists
  run_checks

  tree_changes "$work/after.txt" || fail setup
  changed=no
  if [ -s "$work/after.txt" ]; then
    report_changes after "$work/after.txt"
    changed=yes
  fi

  printf 'egress checks=%s denied=%s entries=%s\n' "$checks" "$denied" "$entries"
  if [ "$ran" = yes ] && [ "$denied" -eq 0 ]; then
    printf 'denied=0 proves that no process of the wrapped tree opened a connection, sent a datagram of any IP protocol or resolved a name outside the list while the command ran on %s; not verified: %s%swhat a process did after the command ended.\n' \
      "$platform" "$paths" "${paths:+, and }"
    printf '%s\n' 'A connection that the mode stops is stopped whether or not its report arrives; denied=0 says that no report of a denial arrived, and 1 report in 494 runs of one denied probe was lost (reports/verifier-13.md, claim 3).'
  fi
  printf 'egress limit third-party crates=%s names=%s\n' "$third_count" "${third:--}"
  if [ "$refused_mode" = yes ]; then
    printf 'NOT VERIFIED egress requirement=W2 platform=%s\n' "$platform"
  fi
  if [ "$matched" = no ] || [ "$ran" = no ] || [ "$changed" = yes ]; then
    fail check
  fi
  [ "$denied" -eq 0 ] || fail denied
  [ "$entries" -eq 0 ] || fail entries
  printf 'PASS %s\n' "$name"
}

self_failed() {
  printf 'FAIL self-test:%s (%s)\n' "$name" "$1"
  exit 1
}

write_stubs() {
  real_git=$(command -v git) || self_failed setup
  cat >"$work/bin/git" <<EOF || self_failed setup
#!/bin/sh
if [ "\$*" = 'rev-parse --verify --quiet HEAD^{tree}' ]; then
  [ -n "\${STUB_TREE:-}" ] || exit 1
  printf '%s\n' "\$STUB_TREE"
  exit 0
fi
exec '$real_git' "\$@"
EOF
  cat >"$work/bin/cargo" <<'EOF' || self_failed setup
#!/bin/sh
case $* in
  'build --locked -p halvo-route-gate --bin route-gate') exit 0 ;;
  'build --locked -p halvo-route-gate --bin route-gate --message-format=json')
    printf '{"reason":"compiler-artifact","package_id":"path+file://%s/crates/halvo-route-gate#0.0.0","manifest_path":"%s/crates/halvo-route-gate/Cargo.toml","target":{"kind":["bin"],"name":"route-gate","src_path":"%s/crates/halvo-route-gate/src/main.rs"},"executable":"%s","fresh":%s}\n' \
      "$CI_D_REPO" "$CI_D_REPO" "$CI_D_REPO" "$STUB_EXE" "$STUB_FRESH"
    exit 0
    ;;
  "tree --locked -e normal,no-proc-macro --target all -p halvo-route-gate --prefix none --format {p}")
    printf 'halvo-route-gate v0.0.0 (%s/crates/halvo-route-gate)\nserde v1.0.0\nserde v1.0.0 (*)\n' "$CI_D_REPO"
    exit 0
    ;;
esac
printf 'stub cargo: %s\n' "$*" >&2
exit 97
EOF
  cat >"$work/route-gate" <<'EOF' || self_failed setup
#!/bin/sh
for known in $STUB_COMMANDS; do
  if [ "$1" = "$known" ]; then
    if [ "${2:-}" = --r6-egress-no-such-argument ]; then
      printf 'REFUSED usage: unknown argument --r6-egress-no-such-argument\n' >&2
      exit 2
    fi
    exit "$STUB_RUN_EXIT"
  fi
done
printf 'REFUSED usage: unknown command\n' >&2
exit 2
EOF
  chmod +x "$work/bin/git" "$work/bin/cargo" "$work/route-gate" || self_failed setup
}

write_repo() {
  dir=$1
  mkdir -p "$dir/scripts/r6/egress" "$dir/scripts/egress" "$dir/docs/r2/trials" "$dir/crates/halvo-route-gate/src" \
    "$dir/crates/halvo-route-gate/tests" || self_failed setup
  printf '%s\n' '[fixed]' "$2" '[agent]' | awk 'NF' >"$dir/scripts/r6/egress/gate.txt" || self_failed setup
  cat >"$dir/docs/r2/trials/egress.md" <<'EOF' || self_failed setup
cells: this-mac.sandbox
not-observable this-mac sandbox path-1-this-mac: one
not-observed this-mac sandbox path-2-this-mac: two
not-observable macos-15 sandbox path-1-macos-15: one
not-observed macos-15 sandbox path-2-macos-15: two
not-observable windows-2025 wfp path-1-windows-2025: one
not-observed windows-2025 wfp path-2-windows-2025: two
EOF
  cat >"$dir/crates/halvo-route-gate/src/main.rs" <<'EOF' || self_failed setup
fn main() {
    let words = ["generate", "import", "derive", "score", "self-test", "spend-sim", "probe"];
    let more = ["fetch", "other", "Upper", "--flag"];
}
EOF
  printf '%s\n' 'const WORD: &str = "testonly";' >"$dir/crates/halvo-route-gate/tests/t.rs" || self_failed setup
  cat >"$dir/scripts/egress/check.sh" <<'EOF' || self_failed setup
#!/bin/sh
list=$1
shift 2
case $STUB_CHECK in
  mode)
    printf 'REFUSED mode: stub\n' >&2
    exit 2
    ;;
  noline)
    "$@"
    exit
    ;;
esac
"$@"
code=$?
[ -z "$STUB_TOUCH" ] || printf 'x\n' >>"$STUB_TOUCH"
denied=0
[ "$STUB_CHECK" != denied ] || denied=1
[ "$STUB_CHECK" != denied0 ] || denied=1
printf 'egress mode=sandbox list=%s command=%s denied=%s log=target/egress/1/denied.log\n' "$list" "$code" "$denied" >&2
[ "$STUB_CHECK" != exit3 ] || exit 3
[ "$STUB_CHECK" != denied0 ] || exit 0
[ "$code" = 0 ] && [ "$denied" = 0 ]
EOF
  git init -q "$dir" >/dev/null 2>&1 || self_failed setup
  git -C "$dir" add -A >/dev/null 2>&1 || self_failed setup
  git -C "$dir" write-tree 2>/dev/null || self_failed setup
}

case_reset() {
  case_repo=$work/repo
  case_tree=$good_tree
  STUB_COMMANDS='generate import derive score self-test spend-sim probe'
  STUB_RUN_EXIT=0
  STUB_CHECK=pass
  STUB_TOUCH=
  STUB_FRESH=true
  case_actions=
  case_arg=
}

case_run() {
  PATH=$work/bin:$PATH CI_D_REPO=$case_repo CI_D_ARTIFACTS=$work/artifacts TMPDIR=$work/tmp \
    STUB_TREE=$case_tree STUB_EXE=$work/route-gate STUB_COMMANDS=$STUB_COMMANDS \
    STUB_RUN_EXIT=$STUB_RUN_EXIT STUB_CHECK=$STUB_CHECK STUB_TOUCH=$STUB_TOUCH STUB_FRESH=$STUB_FRESH \
    GITHUB_ACTIONS=$case_actions sh "$self" ${case_arg:+"$case_arg"} >"$work/case.txt" 2>&1 </dev/null
  code=$?
  out=
  while IFS= read -r line || [ -n "$line" ]; do
    out=$out$line$nl
  done <"$work/case.txt"
  out=${out%"$nl"}
  last=${out##*"$nl"}
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ($3)") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  case $out in
    *"$4"*) ;;
    *) got=$got-without-the-text ;;
  esac
  if [ -n "${5:-}" ]; then
    case $out in
      *"$5"*) got=$got-with-the-other-text ;;
    esac
  fi
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$got"
  if [ "$got" != "$2" ]; then
    wrong="$wrong $1"
  elif [ "$2" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
  case_reset
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || self_failed setup
  case $work in
    /*) ;;
    *) work=$(pwd)/$work ;;
  esac
  mkdir -p "$work/bin" "$work/artifacts" "$work/tmp" || self_failed setup
  write_stubs
  good_tree=$(write_repo "$work/repo" '') || self_failed setup
  entry_tree=$(write_repo "$work/repo-entry" '127.0.0.1:443') || self_failed setup
  system=$(uname -s) || self_failed setup
  case $system in
    Darwin)
      p=this-mac
      hosted=macos-15
      ;;
    MINGW* | MSYS* | CYGWIN*)
      p=windows-2025
      hosted=windows-2025
      ;;
    *) self_failed "no platform for $system" ;;
  esac
  wrong=
  rejected=0
  case_reset

  case_run pass PASS - "egress checks=6 denied=0 entries=0${nl}denied=0 proves that no process"
  case_run sentence PASS - "while the command ran on $p; not verified: path-1-$p, path-2-$p, and what a process did after the command ended.${nl}A connection that the mode stops"
  case_actions=true
  case_run sentence-on-a-hosted-runner PASS - "while the command ran on $hosted; not verified: path-1-$hosted, path-2-$hosted, and what"
  case_run commands PASS - 'egress commands=7 asked=9 run=6 cannot=1 crates=1'
  case_run third-party PASS - 'egress limit third-party crates=1 names=serde'
  case_repo=$work/repo-entry
  case_tree=$entry_tree
  case_run list-file-with-one-entry FAIL entries 'egress checks=6 denied=0 entries=1'
  STUB_COMMANDS="$STUB_COMMANDS fetch"
  case_run command-on-neither-list FAIL check 'egress on-neither-list=fetch'
  STUB_COMMANDS='generate import derive score self-test spend-sim'
  case_run entry-that-is-no-command FAIL check 'egress no-command=probe'
  STUB_COMMANDS='testonly generate import derive score self-test spend-sim probe'
  case_run word-of-tests-not-asked PASS - 'egress commands=7 asked=9'
  STUB_CHECK=denied
  case_run denied-in-each-run FAIL denied 'egress checks=6 denied=6 entries=0' 'denied=0 proves'
  STUB_CHECK=denied0
  case_run check-exits-0-with-a-denial FAIL check 'egress run=probe mode=sandbox command=0 denied=1 exit=0'
  STUB_CHECK=denied
  STUB_RUN_EXIT=1
  case_run command-exits-1-with-a-denial FAIL check 'egress run=probe mode=sandbox command=1 denied=1 exit=1'
  STUB_CHECK=mode
  case_run refusal-mode FAIL check 'NOT VERIFIED egress requirement=W2'
  STUB_RUN_EXIT=1
  case_run command-exits-1 FAIL check 'egress run=probe mode=sandbox command=1 denied=0 exit=1'
  STUB_CHECK=exit3
  case_run check-exits-3-with-a-clean-line FAIL check 'egress run=probe mode=sandbox command=0 denied=0 exit=3'
  STUB_FRESH=false
  case_run stale-build FAIL setup 'FAIL'
  STUB_CHECK=noline
  case_run no-egress-line FAIL check 'egress run=probe line=missing exit=0'
  STUB_TOUCH=$work/repo/docs/r2/trials/egress.md
  case_run tracked-file-changed-by-a-run FAIL check 'tree changed=after M docs/r2/trials/egress.md'
  case_run tracked-file-changed-before FAIL setup 'tree changed=before M docs/r2/trials/egress.md'
  git -C "$work/repo" checkout -q -- docs/r2/trials/egress.md 2>/dev/null || self_failed setup
  case_tree=
  case_run no-tree FAIL setup 'FAIL'
  case_arg=--bogus
  case_run usage FAIL usage 'FAIL'

  sh "$self" --help >"$work/help.txt" 2>&1 || wrong="$wrong help-exit"
  for limit in 'a process start inside a third-party crate that the binary links' \
    'a start by a name that is built at run time' \
    'a command of the second list, which the read of text alone holds until its files exist'; do
    if ! grep -q -F -e "$limit" "$work/help.txt"; then
      printf 'case %s/help want=PASS got=without-a-limit\n' "$name"
      wrong="$wrong help"
    fi
  done

  if [ -n "$wrong" ]; then
    self_failed "wrong:$wrong"
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case $# in
  0) check ;;
  1)
    case $1 in
      --self-test) self_test ;;
      --help) help ;;
      *) fail usage ;;
    esac
    ;;
  *) fail usage ;;
esac
