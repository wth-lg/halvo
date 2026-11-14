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
A word is a command when the binary does not answer "REFUSED usage: unknown command" to it alone
or to it with one argument that no command takes; the words asked are the string literals with no
space, quote or backslash of the tracked Rust files outside tests/ of the crates that the binary
links, read from the commit, and they are asked outside the egress check.
The check holds the runs it makes and no more: a command that the binary answers to only with other
arguments, a branch of a command that only other arguments than those of the first list reach, and
a command whose name is no such string literal are not run under the egress check.
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

forbid_table() {
  cat <<'EOF'
type all std::process::Command|pub fn t01(_: Option<&std::process::Command>) {}
type all std::process::Child|pub fn t02(_: Option<&std::process::Child>) {}
type all std::process::Stdio|pub fn t03(_: Option<&std::process::Stdio>) {}
type all std::process::ChildStdin|pub fn t04(_: Option<&std::process::ChildStdin>) {}
type all std::process::ChildStdout|pub fn t05(_: Option<&std::process::ChildStdout>) {}
type all std::process::ChildStderr|pub fn t06(_: Option<&std::process::ChildStderr>) {}
type all std::process::Output|pub fn t07(_: Option<&std::process::Output>) {}
type all std::net::TcpStream|pub fn t08(_: Option<&std::net::TcpStream>) {}
type all std::net::TcpListener|pub fn t09(_: Option<&std::net::TcpListener>) {}
type all std::net::UdpSocket|pub fn t10(_: Option<&std::net::UdpSocket>) {}
type unix std::os::unix::net::UnixStream|pub fn t11(_: Option<&std::os::unix::net::UnixStream>) {}
type unix std::os::unix::net::UnixListener|pub fn t12(_: Option<&std::os::unix::net::UnixListener>) {}
type unix std::os::unix::net::UnixDatagram|pub fn t13(_: Option<&std::os::unix::net::UnixDatagram>) {}
method all std::process::Command::new|pub fn m01() { let _ = std::process::Command::new("x"); }
method all std::process::Command::spawn|pub fn m02(c: &mut std::process::Command) { let _ = c.spawn(); }
method all std::process::Command::output|pub fn m03(c: &mut std::process::Command) { let _ = c.output(); }
method all std::process::Command::status|pub fn m04(c: &mut std::process::Command) { let _ = c.status(); }
method unix std::os::unix::process::CommandExt::exec|pub fn m05(c: &mut std::process::Command) { let _ = std::os::unix::process::CommandExt::exec(c); }
method all std::net::TcpStream::connect|pub fn m06() { let _ = std::net::TcpStream::connect("192.0.2.1:9"); }
method all std::net::TcpStream::connect_timeout|pub fn m07(a: &std::net::SocketAddr) { let _ = std::net::TcpStream::connect_timeout(a, std::time::Duration::from_secs(1)); }
method all std::net::TcpListener::bind|pub fn m08() { let _ = std::net::TcpListener::bind("127.0.0.1:0"); }
method all std::net::UdpSocket::bind|pub fn m09() { let _ = std::net::UdpSocket::bind("127.0.0.1:0"); }
method all std::net::ToSocketAddrs::to_socket_addrs|pub fn m10() { let _ = std::net::ToSocketAddrs::to_socket_addrs("x.invalid:1"); }
method unix std::os::unix::net::UnixStream::connect|pub fn m11() { let _ = std::os::unix::net::UnixStream::connect("/x"); }
method unix std::os::unix::net::UnixStream::connect_addr|pub fn m12(a: &std::os::unix::net::SocketAddr) { let _ = std::os::unix::net::UnixStream::connect_addr(a); }
method unix std::os::unix::net::UnixStream::pair|pub fn m13() { let _ = std::os::unix::net::UnixStream::pair(); }
method unix std::os::unix::net::UnixListener::bind|pub fn m14() { let _ = std::os::unix::net::UnixListener::bind("/x"); }
method unix std::os::unix::net::UnixListener::bind_addr|pub fn m15(a: &std::os::unix::net::SocketAddr) { let _ = std::os::unix::net::UnixListener::bind_addr(a); }
method unix std::os::unix::net::UnixDatagram::bind|pub fn m16() { let _ = std::os::unix::net::UnixDatagram::bind("/x"); }
method unix std::os::unix::net::UnixDatagram::bind_addr|pub fn m17(a: &std::os::unix::net::SocketAddr) { let _ = std::os::unix::net::UnixDatagram::bind_addr(a); }
method unix std::os::unix::net::UnixDatagram::pair|pub fn m18() { let _ = std::os::unix::net::UnixDatagram::pair(); }
method unix std::os::unix::net::UnixDatagram::connect|pub fn m19(s: &std::os::unix::net::UnixDatagram) { let _ = s.connect("/x"); }
method unix std::os::unix::net::UnixDatagram::connect_addr|pub fn m20(s: &std::os::unix::net::UnixDatagram, a: &std::os::unix::net::SocketAddr) { let _ = s.connect_addr(a); }
method unix std::os::fd::FromRawFd::from_raw_fd|pub fn m21() -> std::fs::File { unsafe { std::os::fd::FromRawFd::from_raw_fd(3) } }
method windows std::os::windows::io::FromRawSocket::from_raw_socket|pub fn m22() -> std::net::TcpStream { unsafe { std::os::windows::io::FromRawSocket::from_raw_socket(0) } }
method windows std::os::windows::io::FromRawHandle::from_raw_handle|pub fn m23() -> std::fs::File { unsafe { std::os::windows::io::FromRawHandle::from_raw_handle(std::ptr::null_mut()) } }
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

job_of() {
  job=${CI_D_JOB:-}
  if [ -z "$job" ]; then
    case $system in
      Darwin) job=macos ;;
      MINGW* | MSYS* | CYGWIN*) job=windows ;;
    esac
  fi
  case $job in
    macos) job_system=unix ;;
    windows) job_system=windows ;;
    *) fail setup ;;
  esac
}

absolute_dir() {
  (cd "$1" 2>/dev/null && { pwd -W 2>/dev/null || pwd; })
}

write_forbid_list() {
  conf=$artifacts/forbid-list
  mkdir -p "$conf" || fail setup
  conf_abs=$(absolute_dir "$conf") || fail setup
  [ -n "$conf_abs" ] || fail setup
  forbid_table >"$work/forbid-table.txt" || fail setup
  awk -v own="$job_system" '
    {
      split($0, half, "|")
      split(half[1], part, " ")
      other = (part[2] != "all" && part[2] != own) ? ", allow-invalid = true" : ""
      line = "  { path = \"" part[3] "\"" other " },"
      if (part[1] == "type") types = types line "\n"
      else methods = methods line "\n"
    }
    END {
      printf "disallowed-types = [\n%s]\ndisallowed-methods = [\n%s]\n", types, methods
    }
  ' "$work/forbid-table.txt" >"$conf/clippy.toml" || fail setup
}

forbid() {
  out=$1
  shift
  CLIPPY_CONF_DIR=$conf_abs cargo clippy --locked "$@" --lib --bins --message-format=json -- \
    -F clippy::disallowed_types -F clippy::disallowed_methods -F unsafe_code >"$out.json" 2>"$out.err"
}

read_forbid() {
  awk -v root="$root_abs" '
    function last(text, word,   base, k, at) {
      base = 0
      at = 0
      while ((k = index(substr(text, base + 1), word)) > 0) {
        at = base + k
        base = at
      }
      return at
    }
    index($0, "{\"reason\":\"compiler-message\"") != 1 { next }
    {
      line = $0
      if (index(line, " does not refer to ") > 0) print "refer"
      named = last(line, "\"code\":{\"code\":\"")
      code = ""
      if (named > last(line, "\"code\":null")) {
        tail = substr(line, named + 16)
        code = substr(tail, 1, index(tail, "\"") - 1)
      }
      if (code != "clippy::disallowed_types" && code != "clippy::disallowed_methods" && code != "unsafe_code") next
      where = "-"
      at = 0
      if (match(line, /--> [^ ]+:[0-9]+:[0-9]+\\n/)) {
        loc = substr(line, RSTART + 4, RLENGTH - 6)
        sub(/:[0-9]+$/, "", loc)
        at = loc
        sub(/^.*:/, "", at)
        where = substr(loc, 1, length(loc) - length(at) - 1)
        gsub(/\\\\/, "/", where)
        if (root != "" && index(where, root "/") == 1) where = substr(where, length(root) + 2)
        if (where ~ /^\// || where ~ /^[A-Za-z]:\//) where = "outside-the-repository"
      }
      print "finding", code, where, at
    }
  ' "$1"
}

write_canary() {
  canary=$artifacts/canary
  mkdir -p "$canary/src" || fail setup
  printf '[package]\nname = "canary"\nversion = "0.0.0"\nedition = "2021"\npublish = false\n\n[workspace]\n' \
    >"$canary/Cargo.toml" || fail setup
  printf '# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\nversion = 4\n\n[[package]]\nname = "canary"\nversion = "0.0.0"\n' \
    >"$canary/Cargo.lock" || fail setup
  git show "$tree:rust-toolchain.toml" >"$canary/rust-toolchain.toml" 2>/dev/null || fail setup
  awk -v own="$job_system" -v expected="$canary/expected.txt" '
    {
      split($0, half, "|")
      split(half[1], part, " ")
      if (part[2] != "all") {
        print "#[cfg(" part[2] ")]"
        n++
      }
      print half[2]
      n++
      if (part[2] == "all" || part[2] == own) {
        print (part[1] == "type" ? "clippy::disallowed_types" : "clippy::disallowed_methods"), n > expected
      }
    }
    END {
      print "pub fn u01() { unsafe {} }"
      n++
      print "unsafe_code", n > expected
    }
  ' "$work/forbid-table.txt" >"$canary/src/lib.rs" || fail setup
}

run_canary() {
  started=$(date +%s)
  forbid "$artifacts/canary-run" --manifest-path "$canary/Cargo.toml"
  canary_exit=$?
  canary_seconds=$(($(date +%s) - started))
  read_forbid "$artifacts/canary-run.json" >"$work/canary-read.txt" || fail setup
  wanted=0
  found=0
  refers=0
  while IFS=' ' read -r code at; do
    wanted=$((wanted + 1))
    if grep -q -x -F -e "finding $code src/lib.rs $at" "$work/canary-read.txt"; then
      found=$((found + 1))
    else
      printf 'egress canary missing=%s:%s\n' "$code" "$at"
    fi
  done <"$canary/expected.txt"
  if grep -q -x -F -e refer "$work/canary-read.txt"; then
    refers=1
  fi
  printf 'egress canary entries=%s found=%s exit=%s\n' "$wanted" "$found" "$canary_exit"
  if [ "$found" -ne "$wanted" ] || [ "$wanted" -eq 0 ] || [ "$refers" -ne 0 ]; then
    printf 'egress canary: the forbid run let the canary through\n'
    fail check
  fi
}

forbid_crates() {
  started=$(date +%s)
  forbid "$artifacts/forbid" -p halvo-route -p halvo-route-gate -p halvo-route-wire
  forbid_exit=$?
  forbid_seconds=$(($(date +%s) - started))
  read_forbid "$artifacts/forbid.json" >"$work/forbid-read.txt" || fail setup
  findings=0
  while IFS=' ' read -r kind code where at; do
    case $kind in
      finding)
        findings=$((findings + 1))
        printf 'forbid %s:%s %s\n' "$where" "$at" "$code"
        ;;
      refer)
        printf 'egress forbid list-entry=refers-to-nothing\n'
        holds=no
        ;;
    esac
  done <"$work/forbid-read.txt"
  printf 'egress forbid findings=%s exit=%s\n' "$findings" "$forbid_exit"
  if [ "$findings" -ne 0 ] || [ "$forbid_exit" -ne 0 ]; then
    holds=no
  fi
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
    git grep -I -o -E '"[^" \\]+"' "$tree" -- "$value" >>"$work/grep.txt" 2>"$work/grep.err"
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

answers_unknown() {
  (cd "$work/discover" && "$exe" "$@" >"$work/ask.out" 2>"$work/ask.err" </dev/null)
  code=$?
  answer=
  while IFS= read -r line || [ -n "$line" ]; do
    answer=$answer$line$nl
  done <"$work/ask.err"
  [ "$code" -eq 2 ] && [ ! -s "$work/ask.out" ] && [ "$answer" = "$unknown$nl" ]
}

discover() {
  : >"$work/commands.txt"
  asked=0
  while IFS= read -r word || [ -n "$word" ]; do
    asked=$((asked + 1))
    if ! answers_unknown "$word" || ! answers_unknown "$word" "$probe_word"; then
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
  job_of
  root_abs=$(absolute_dir .) || fail setup
  holds=yes

  tree=$(git rev-parse --verify --quiet 'HEAD^{tree}' 2>/dev/null) || fail setup
  [ -n "$tree" ] || fail setup
  tree_changes "$work/before.txt" || fail setup
  if [ -s "$work/before.txt" ]; then
    report_changes before "$work/before.txt"
    fail setup
  fi
  count_entries
  not_observed

  write_forbid_list
  write_canary
  run_canary
  forbid_crates
  printf 'egress seconds canary=%s forbid=%s\n' "$canary_seconds" "$forbid_seconds"

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
  if [ "$holds" = no ] || [ "$matched" = no ] || [ "$ran" = no ] || [ "$changed" = yes ]; then
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
  "clippy --locked --manifest-path $CI_D_ARTIFACTS/canary/Cargo.toml --lib --bins --message-format=json -- -F clippy::disallowed_types -F clippy::disallowed_methods -F unsafe_code")
    want=$(cd "$CI_D_ARTIFACTS/forbid-list" && pwd) || exit 96
    got=$(cd "$CLIPPY_CONF_DIR" && pwd) || exit 96
    [ "$got" = "$want" ] || exit 96
    if [ "$STUB_CANARY" = real ]; then
      exec "$REAL_CARGO" "$@"
    fi
    n=0
    while IFS=' ' read -r code at; do
      n=$((n + 1))
      [ "$n" = "$STUB_CANARY_DROP" ] && continue
      printf '{"reason":"compiler-message","message":{"rendered":"error: x\\n  --> src/lib.rs:%s:1\\n","children":[],"code":{"code":"%s","explanation":null},"level":"error","spans":[]}}\n' "$at" "$code"
    done <"$CI_D_ARTIFACTS/canary/expected.txt"
    exit 101
    ;;
  'clippy --locked -p halvo-route -p halvo-route-gate -p halvo-route-wire --lib --bins --message-format=json -- -F clippy::disallowed_types -F clippy::disallowed_methods -F unsafe_code')
    want=$(cd "$CI_D_ARTIFACTS/forbid-list" && pwd) || exit 96
    got=$(cd "$CLIPPY_CONF_DIR" && pwd) || exit 96
    [ "$got" = "$want" ] || exit 96
    invalid=$(grep -c 'allow-invalid = true' "$CLIPPY_CONF_DIR/clippy.toml")
    case $CI_D_JOB:$invalid in
      macos:2 | windows:15) ;;
      *) exit 95 ;;
    esac
    if [ -f "$STUB_CLIPPY_JSON" ]; then
      while IFS= read -r line || [ -n "$line" ]; do printf '%s\n' "$line"; done <"$STUB_CLIPPY_JSON"
    fi
    exit "$STUB_CLIPPY_EXIT"
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
for bare in $STUB_BARE; do
  if [ "$1" = "$bare" ] && [ "$#" -eq 1 ]; then
    exit 0
  fi
done
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
    let more = ["fetch", "other", "Upper", "--flag", "post_route", "push"];
}
EOF
  printf '%s\n' 'const WORD: &str = "testonly";' >"$dir/crates/halvo-route-gate/tests/t.rs" || self_failed setup
  cp "$work/rust-toolchain.toml" "$dir/rust-toolchain.toml" || self_failed setup
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
  STUB_BARE=
  STUB_CHECK=pass
  STUB_TOUCH=
  STUB_FRESH=true
  STUB_CLIPPY_JSON=$work/clippy-clean.json
  STUB_CLIPPY_EXIT=0
  STUB_CANARY=synth
  STUB_CANARY_DROP=0
  case_job=$own_job
  case_actions=
  case_arg=
}

case_run() {
  PATH=$work/bin:$PATH CI_D_REPO=$case_repo CI_D_ARTIFACTS=$work/artifacts TMPDIR=$work/tmp \
    STUB_TREE=$case_tree STUB_EXE=$work/route-gate STUB_COMMANDS=$STUB_COMMANDS STUB_BARE=$STUB_BARE \
    STUB_RUN_EXIT=$STUB_RUN_EXIT STUB_CHECK=$STUB_CHECK STUB_TOUCH=$STUB_TOUCH STUB_FRESH=$STUB_FRESH \
    STUB_CLIPPY_JSON=$STUB_CLIPPY_JSON STUB_CLIPPY_EXIT=$STUB_CLIPPY_EXIT STUB_CANARY=$STUB_CANARY \
    STUB_CANARY_DROP=$STUB_CANARY_DROP REAL_CARGO=$real_cargo CI_D_JOB=$case_job GITHUB_ACTIONS=$case_actions sh "$self" ${case_arg:+"$case_arg"} >"$work/case.txt" 2>&1 </dev/null
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

finding_json() {
  # shellcheck disable=SC2016
  printf '{"reason":"compiler-message","package_id":"path+file:///x#0.0.0","message":{"rendered":"error: a use\\n  --> %s:%s:5\\n","$message_type":"diagnostic","children":[{"children":[],"code":null,"level":"note","message":"n","rendered":null,"spans":[]}],"code":{"code":"%s","explanation":null},"level":"error","message":"m","spans":[]}}\n' \
    "$3" "$4" "$2" >"$1"
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || self_failed setup
  case $work in
    /*) ;;
    *) work=$(pwd)/$work ;;
  esac
  mkdir -p "$work/bin" "$work/artifacts" "$work/tmp" || self_failed setup
  real_cargo=$(command -v cargo) || self_failed "no cargo"
  own_repo=${CI_D_REPO:-$(git rev-parse --show-toplevel)} || self_failed setup
  git -C "$own_repo" show HEAD:rust-toolchain.toml >"$work/rust-toolchain.toml" || self_failed setup
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
  case $system in
    Darwin) own_job=macos ;;
    *) own_job=windows ;;
  esac
  : >"$work/clippy-clean.json" || self_failed setup
  finding_json "$work/clippy-finding.json" clippy::disallowed_methods 'crates\\halvo-route-gate\\src\\net.txt' 2 || self_failed setup
  finding_json "$work/clippy-unsafe.json" unsafe_code 'crates/halvo-route/src/x.rs' 7 || self_failed setup
  # shellcheck disable=SC2016
  printf '%s\n' '{"reason":"compiler-message","message":{"rendered":"warning: `std::nope` does not refer to a reachable type\n","$message_type":"diagnostic","children":[],"code":null,"level":"warning","message":"x","spans":[]}}' \
    >"$work/clippy-refer.json" || self_failed setup
  wrong=
  rejected=0
  case_reset

  case_run pass PASS - "egress checks=6 denied=0 entries=0${nl}denied=0 proves that no process"
  case_run sentence PASS - "while the command ran on $p; not verified: path-1-$p, path-2-$p, and what a process did after the command ended.${nl}A connection that the mode stops"
  case_actions=true
  case_run sentence-on-a-hosted-runner PASS - "while the command ran on $hosted; not verified: path-1-$hosted, path-2-$hosted, and what"
  case_run commands PASS - 'egress commands=7 asked=13 run=6 cannot=1 crates=1'
  case_run third-party PASS - 'egress limit third-party crates=1 names=serde'
  case_run forbid-clean PASS - 'egress forbid findings=0 exit=0'
  STUB_CANARY=real
  case_run canary-on-the-real-cargo PASS - 'egress canary entries='
  STUB_CANARY_DROP=1
  case_run canary-let-through FAIL check "egress canary missing=clippy::disallowed_types:1${nl}egress canary entries="
  case_job=windows
  case_run forbid-list-of-the-windows-job PASS - 'egress forbid findings=0 exit=0'
  STUB_CLIPPY_JSON=$work/clippy-finding.json
  STUB_CLIPPY_EXIT=101
  case_run forbid-finding FAIL check "forbid crates/halvo-route-gate/src/net.txt:2 clippy::disallowed_methods${nl}egress forbid findings=1 exit=101"
  STUB_CLIPPY_JSON=$work/clippy-unsafe.json
  case_run forbid-finding-with-exit-0 FAIL check 'forbid crates/halvo-route/src/x.rs:7 unsafe_code'
  STUB_CLIPPY_EXIT=101
  case_run forbid-exit-without-a-finding FAIL check 'egress forbid findings=0 exit=101'
  STUB_CLIPPY_JSON=$work/clippy-refer.json
  case_run forbid-list-entry-refers-to-nothing FAIL check 'egress forbid list-entry=refers-to-nothing'
  case_repo=$work/repo-entry
  case_tree=$entry_tree
  case_run list-file-with-one-entry FAIL entries 'egress checks=6 denied=0 entries=1'
  STUB_COMMANDS="$STUB_COMMANDS fetch"
  case_run command-on-neither-list FAIL check 'egress on-neither-list=fetch'
  STUB_COMMANDS="$STUB_COMMANDS post_route"
  case_run command-with-an-underscore FAIL check 'egress on-neither-list=post_route'
  STUB_BARE=push
  case_run command-alone-without-arguments FAIL check 'egress on-neither-list=push'
  STUB_COMMANDS='generate import derive score self-test spend-sim'
  case_run entry-that-is-no-command FAIL check 'egress no-command=probe'
  STUB_COMMANDS='testonly generate import derive score self-test spend-sim probe'
  case_run word-of-tests-not-asked PASS - 'egress commands=7 asked=13'
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
