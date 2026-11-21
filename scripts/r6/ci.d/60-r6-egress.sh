#!/bin/sh
# jobs: macos windows
# rule: No code of halvo-route, halvo-route-gate or halvo-route-wire that a job compiles names a listed type or method of a process or a socket or uses unsafe code, by a clippy run in the dev and in the release profile whose list the script writes and whose canary proves it in the run, over a closed graph of packages; a cfg predicate may name test, unix, windows, any, all and not, and a rule of text refuses any other name where the text cfg, cfg! or cfg_attr stands before its parenthesis, in the tracked files of the three crate directories and in every file that a forbid run's dep-info names, in comments and strings too; it does not see a predicate brought by a macro variable named like an allowed word or one split by a comment; for those spellings the dev and the release forbid runs hold what they compile, and the rule on the word clippy holds the one value that no forbid run has in common with a build; no attribute path comes from a macro variable and no flags or wrapper variable lifts a lint; once one of these holds has refused, the check builds and runs nothing of route-gate; and every command of the first list is run, and a run that is missing fails the check; each, run with its arguments under scripts/egress/check.sh with the commit's list behind a control of the wrap, exits 0 with denied=0, the commands come from route-gate's printing of the one table that dispatches them, in both jobs, and a printed command on neither list fails, every file under scripts/r6/egress/ in the commit holds no entry line and the run leaves no tracked file changed and no untracked path outside target/ new or gone; it holds no more than --help says.
# lane: alone
set -u
name=60-r6-egress
list=scripts/r6/egress/gate.txt
trial=docs/r2/trials/egress.md
nl='
'
w_rust=RUST
w_rustc=${w_rust}C
w_cargo=CARGO
v_flags=${w_rust}FLAGS
v_encoded=${w_cargo}_ENCODED_$v_flags
v_build=${w_cargo}_BUILD_$v_flags
v_clippy=CLIPPY_ARGS
v_wrappers="${w_rustc}_WRAPPER ${w_rustc}_WORKSPACE_WRAPPER ${w_cargo}_BUILD_${w_rustc}_WRAPPER ${w_cargo}_BUILD_${w_rustc}_WORKSPACE_WRAPPER $w_rustc ${w_cargo}_BUILD_$w_rustc"
case $0 in
  /*) self=$0 ;;
  *) self=$(pwd)/$0 ;;
esac

help() {
  cat <<'EOF'
sh scripts/r6/ci.d/60-r6-egress.sh [--self-test | --help]
The forbid runs: cargo clippy with --locked over the lib and bin targets of halvo-route,
halvo-route-gate and halvo-route-wire, once in the dev and once in the release profile, in the macos
job and in the windows job, with clippy::disallowed_types, clippy::disallowed_methods and unsafe_code
held by -F, so that no allow in the source lifts them. The list of types and methods of a process
start or a socket is written by this script for the job under its artifacts directory, and an entry
of the other system carries allow-invalid. A finding of the three codes at any level, a non-zero
exit and an entry that refers to nothing fail the check; each finding prints
"forbid <dev|release> <path>:<line> <code>", with the path as the compiler names it (a file that a
module declaration resolved by another letter case prints by the name that the compiler built).
The canary: a crate of one file under the artifacts directory names each entry of the list once and
goes through the same cargo call, in each profile, before any finding of the three crates counts; one finding per
entry of the job's system is required, else "the forbid run let the canary through". It holds a cap
on lints and a wrapper that silence the canary as they silence the crates. The environment rule reads
no cargo configuration file: the compiler flags keys of its build and target tables and its wrapper
keys reach the forbid runs and the build alike.
From the first wrapped call on, the check decides nothing from a file that a wrapped program can
write: the run list with its arguments, the list's blob id, the hashes of the tracked files, git's
configuration and the untracked paths of the checkout are read into the shell before the control,
before any run of the built route-gate; the printed names of the commands are read into the shell
once, right after the printing's own wrapped call, and from nowhere after it; the egress check's
last line comes through a pipe; and a command's --out directory is one of its own, outside the
checkout. After every wrapped call the check reads the
tracked files, git's configuration and the untracked paths outside target/ again (git ls-files --others
--directory, which names a directory that holds no tracked file by its name alone) and fails on any
difference: an untracked path that is new by "a wrapped run left a new path: <path>", and one that is
gone by "a wrapped run took away a path: <path>". A file written under target/, or inside a directory
that held no tracked file before the first call, is not seen there.
Every command of the first list is run, and a run that is missing fails the check: checks must equal
the number of entries of the first list that the printing names, and each command whose run was not
made and read is named by "egress run-missing=<word>".
The closed graph: the path packages of cargo tree -e normal,build are the three crates, each at
crates/<name> of the repository where the check runs, the linked
third-party packages are itoa, memchr, serde, serde_core, serde_json and zmij, the build-time ones are
proc-macro2, quote, serde_derive, syn and unicode-ident, and the three crates have no build script, no
proc-macro target and no features, and the root manifest no [patch] table; another name fails.
cargo tree and cargo metadata, which the graph reads, compile nothing, and they come before every
cargo call that compiles the three crates; a graph that refuses leaves the forbid runs unmade, so the
check runs no build script or proc macro that the graph refused.
Once the environment rules, the graph, a canary, a forbid run or a rule of text has refused, the
check prints the findings, then "egress runs=not-made (a hold refused)", and fails: it builds no
route-gate and runs no command of it, no printing and no run of the first list.
The rules for what no compiler pass reads, over every tracked file of the three crate directories and
every file that a forbid run's dep-info names: a cfg predicate may name test, unix, windows, any, all
and not, and a rule of text refuses any other name where the text cfg, cfg! or cfg_attr stands before its
parenthesis, in the tracked files of the three crate directories and in every file that a forbid
run's dep-info names, in comments and strings too; it does not see a predicate brought by a macro
variable named like an allowed word or one split by a comment; for those spellings the dev and the
release forbid runs hold what they compile, and the rule on the word clippy holds the one value that
no forbid run has in common with a build; the word clippy, the identifier with no path separator
after it, is refused; an attribute's path may not be a macro variable followed by an opening
parenthesis, an equals sign or a path separator; and before its first cargo command the check
refuses a flags variable of the compiler (plain, encoded, build or per-target) or the clippy
arguments variable that holds an allow flag, a cap on lints, a cfg flag or a word that starts with @
(an argument file, which the rule cannot read), and a compiler or wrapper
variable that is set; it reads a name without regard to letter case, as Windows does. Text in a
comment or a string is read as code, so a doc comment that writes cfg(feature) is refused: write such
text without the parenthesis after cfg.
The printing: after the control, in the macos job and in the windows job, the check runs the built
route-gate under the egress check as "route-gate commands", a wrapped call like every other, with its
own line "egress printing mode=<mode> command=<n> denied=<n> exit=<n>", whose denied adds to denied;
route-gate prints the names of its commands from the one table that dispatches them, one per line in
the table's order, so that a name that it does not print is a command that route-gate does not
dispatch; a printing that does not exit 0 fails the check by "egress printing exit=<n>".
The two lists: the commands that the check runs under the egress check, each with its arguments,
and the commands that it cannot run on files of the repository alone, each with its reason. At this
commit every command stands on the first list, import among them on the repository's two trial files
of docs/r3/trials/ with its --out in the check's own directory, and the second list has no entry; the
count line prints cannot=0. A printed command on neither list fails the check, and so does a list
entry that the printing does not name. Every printed line counts as a name, an empty one and one
that reads --found-- among them, and the check fails where the count of printed lines is not the
count of the first list's commands plus the count of the second list's entries, by "egress
commands=<n> is not run=<n> plus cannot=<n>".
The control: before the first wrapped run, a listener on 127.0.0.1:47201 and one wrapped run of the
egress run's loopback probe; a status of 0 with denied=0 is "the egress check let the control
through". The control holds against an egress check that wraps nothing; an egress check changed in
the same commit can pass it, and scripts/egress/ is held by its own run's checks and by the landing's
readers. Every wrapped run must name a new log directory whose denied.log holds as many lines as its
denied. A port of the control that another process holds fails the check by "refused=port"; on a
machine that other runs share, this is a false FAIL.
The import listing: the check prints the count of the undefined or imported symbols of the dev
route-gate binary that it built (nm -u on macOS, llvm-readobj --coff-imports on Windows where the
runner has it, else one line that it has none) and keeps the listing under its artifacts directory;
it is a measurement and refuses nothing. On Windows it looks for llvm-readobj under the sysroot that
rustc --print sysroot names, or under R6_EGRESS_SYSROOT when that variable is set: the self-test sets
it for its cases, so that one self-test asks rustc for the sysroot once.
A signal INT, TERM or HUP stops the control's listener and ends the check with FAIL 60-r6-egress
(signal).
When a rule or a forbid run has refused before a later step fails to set up, the cause is check.
Prints: egress checks=<n> denied=<n> entries=<n>, with checks the number of wrapped runs of the first
list that were made and whose egress line was read; a job with none fails.
The limits that stay:
1. a socket or a process start inside a third-party crate, also when code of the three crates calls
   it; the check prints their names from the lock on its line "egress limit third-party crates=".
2. code under a value that no forbid run has: the processor of an Intel Mac or of Windows on ARM,
   another target_env, target_abi or target_vendor, a target feature or a flag that a person sets
   for their own build, and a combination of unix and windows that neither job compiles.
3. what the system does with a path that it opens over a network.
4. what a process did after the command ended, in the egress check's own words.
5. the egress check's writes under target/egress/.
6. an untracked cargo configuration, in the checkout, above it or in cargo's home, whose wrapper tells
   the canary from the crates passes this check; a hosted job has none, a tracked one is
   40-r6-policy's refusal, and on a person's own machine nothing holds it.
7. the check and the egress check find their own programs through PATH, whose first directory a
   wrapped program can write; a program planted there runs in the check after the next call; a diff
   shows it and the landing's readers hold it.
8. the forbid list and its canary come from one table of this script: a commit that changes
   60-r6-egress.sh can take an entry out of the list and its canary line together, and only the
   readers of that guarded script see it.
9. the check takes the commands from the binary's own printing and tries no other word on the built
   binary; what holds the printing to the dispatch is one test of the crate,
   the_printing_names_exactly_the_commands_that_the_dispatch_runs, which calls dispatch in-process; a
   commit whose main or whose handler runs a word outside the table passes the check, a diff shows
   it, and the landing's readers hold it.
With --self-test, R6_EGRESS_CASES, a list of case names, runs those cases alone, for a mutant runner;
a list that names no case fails the self-test. On a Windows host the self-test runs the cases of its
Windows list alone: one case for each rule of the generic code, by the table rule_cases, and every
case of a windows path or a path form; each case of its macos list prints one line there, and the
macos job's self-test runs every case. A case runs in the host's own job unless it names its job. A
case on neither list, a listed case that neither ran nor printed its line, and a rule whose case is
not on the Windows list fail the self-test. R6_EGRESS_SELFTEST_HOST=macos|windows chooses the cases
as on that host, for a fixture.
EOF
}

run_list() {
  cat <<'EOF'
generate --seed 1 --out @/table.tsv
import --driver docs/r3/trials/driver.md --cells docs/r3/trials/model-cells.md --date s1=2026-11-10 --date s2=2026-11-10 --out @/
derive --table @/table.tsv --out @/policy.tsv
score --table @/table.tsv --router product --policy @/policy.tsv
self-test --tables 20 --seed 1
spend-sim --seed 1
probe --rounds 1000
commands
EOF
}

cannot_list() {
  cat <<'EOF'
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
  cause=$1
  [ "$cause" != setup ] || [ "${holds:-yes}" != no ] || cause=check
  printf 'FAIL %s (%s)\n' "$name" "$cause"
  exit 1
}

enter_repo() {
  if [ -n "${CI_D_REPO:-}" ]; then
    repo=$CI_D_REPO
  else
    repo=$(git rev-parse --show-toplevel 2>/dev/null) || fail setup
  fi
  if ! cd "$repo" 2>/dev/null; then
    printf 'egress setup repository=unreadable\n'
    fail setup
  fi
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
  CLIPPY_CONF_DIR=$conf_abs cargo clippy --locked "$@" --message-format=json -- \
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
  flag=
  [ "$1" != release ] || flag=--release
  forbid "$artifacts/canary-$1" --manifest-path "$canary/Cargo.toml" --lib --bins ${flag:+"$flag"}
  canary_exit=$?
  seconds_line="$seconds_line canary-$1=$(($(date +%s) - started))"
  read_forbid "$artifacts/canary-$1.json" >"$work/canary-read.txt" || fail setup
  wanted=0
  found=0
  refers=0
  LC_ALL=C awk -v profile="$1" -v read="$work/canary-read.txt" '
    BEGIN {
      while ((getline line < read) > 0) seen[line] = 1
      close(read)
    }
    {
      line = $0
      sub(/^ +/, "", line)
      code = line
      at = ""
      k = index(line, " ")
      if (k > 0) {
        code = substr(line, 1, k - 1)
        at = substr(line, k + 1)
        sub(/^ +/, "", at)
        sub(/ +$/, "", at)
      }
      wanted++
      if (("finding " code " src/lib.rs " at) in seen) found++
      else if (wanted - found <= 5) printf "egress canary profile=%s missing=%s:%s\n", profile, code, at
    }
    END { printf "counts %d %d %d\n", wanted, found, ("refer" in seen) ? 1 : 0 }
  ' "$canary/expected.txt" >"$work/canary-match.txt"
  while IFS= read -r line; do
    case $line in
      'counts '*)
        rest=${line#counts }
        wanted=${rest%% *}
        rest=${rest#* }
        found=${rest%% *}
        refers=${rest#* }
        ;;
      *) printf '%s\n' "$line" ;;
    esac
  done <"$work/canary-match.txt"
  printf 'egress canary profile=%s entries=%s found=%s exit=%s\n' "$1" "$wanted" "$found" "$canary_exit"
  if [ "$found" -ne "$wanted" ] || [ "$wanted" -eq 0 ] || [ "$refers" -ne 0 ]; then
    printf 'egress canary: the forbid run let the canary through\n'
    holds=no
  fi
}

env_rules() {
  env | sed -n 's/^\([A-Za-z_][A-Za-z0-9_]*\)=.*/\1/p' | sort -u >"$work/env-names.txt" || fail setup
  LC_ALL=C tr '[:lower:]' '[:upper:]' <"$work/env-names.txt" >"$work/env-upper.txt" || fail setup
  while IFS= read -r var && IFS= read -r upper <&3; do
    case " $v_wrappers " in
      *" $upper "*)
        printf 'rule env %s is set\n' "$var"
        holds=no
        continue
        ;;
    esac
    case $upper in
      "$v_flags" | "$v_encoded" | "$v_build" | "$v_clippy" | "${w_cargo}_TARGET_"*"_$v_flags") ;;
      *) continue ;;
    esac
    printenv "$var" >"$work/env-value.txt" || continue
    if tr '\037' ' ' <"$work/env-value.txt" | awk '
      {
        for (i = 1; i <= NF; i++) {
          if ($i ~ /^-A/ || $i ~ /^--allow/ || $i ~ /cap[-_]lints/ || $i ~ /^--cfg/ || $i ~ /^@/) bad = 1
        }
      }
      END { exit bad ? 0 : 1 }
    '; then
      printf 'rule env %s holds an allow, a cap on lints or a cfg\n' "$var"
      holds=no
    fi
  done <"$work/env-names.txt" 3<"$work/env-upper.txt"
}

text_rules() {
  awk -v root="$root_abs" '
    function lineof(text, at,   head) {
      head = substr(text, 1, at)
      return gsub(/\n/, "\n", head) + 1
    }
    function shown(path) {
      gsub(/\\/, "/", path)
      if (root != "" && index(path, root "/") == 1) path = substr(path, length(root) + 2)
      if (path ~ /^\// || path ~ /^[A-Za-z]:\//) path = "outside-the-repository"
      return path
    }
    function scan(path, text,   off, rest, at, attr, i, c, depth, pred, quoted, word) {
      off = 0
      rest = text
      while (match(rest, /(^|[^A-Za-z0-9_])cfg(_attr)?[ \t\r\n]*!?[ \t\r\n]*\(/)) {
        at = off + RSTART
        attr = index(substr(rest, RSTART, RLENGTH), "cfg_attr") > 0
        i = off + RSTART + RLENGTH
        depth = 1
        pred = ""
        quoted = 0
        while (i <= length(text) && depth > 0) {
          c = substr(text, i, 1)
          if (quoted) {
            if (c == "\\") i++
            else if (c == "\"") quoted = 0
            i++
            continue
          }
          if (c == "\"") quoted = 1
          else if (c == "(") depth++
          else if (c == ")") depth--
          else if (c == "," && depth == 1 && attr) break
          if (depth > 0) pred = pred c
          i++
        }
        while (match(pred, /[A-Za-z_][A-Za-z0-9_]*/)) {
          word = substr(pred, RSTART, RLENGTH)
          if (word != "test" && word != "unix" && word != "windows" && word != "any" && word != "all" && word != "not") {
            printf "rule cfg %s:%d %s\n", shown(path), lineof(text, at), word
          }
          pred = substr(pred, RSTART + RLENGTH)
        }
        off = i
        rest = substr(text, i + 1)
      }
      off = 0
      rest = text
      while (match(rest, /#[ \t\r\n]*!?[ \t\r\n]*\[[ \t\r\n]*\$[A-Za-z_][A-Za-z0-9_]*[ \t\r\n]*(\(|=|::)/)) {
        printf "rule attribute %s:%d\n", shown(path), lineof(text, off + RSTART)
        off += RSTART + RLENGTH - 1
        rest = substr(rest, RSTART + RLENGTH)
      }
      off = 0
      rest = text
      while ((k = index(rest, "clippy")) > 0) {
        at = off + k
        before = at > 1 ? substr(text, at - 1, 1) : ""
        after = substr(text, at + 6)
        off = at + 5
        rest = substr(text, off + 1)
        if (before ~ /[A-Za-z0-9_]/ || after ~ /^[A-Za-z0-9_]/) continue
        sub(/^[ \t\r\n]*/, "", after)
        if (substr(after, 1, 2) == "::") continue
        printf "rule clippy %s:%d\n", shown(path), lineof(text, at)
      }
    }
    {
      path = $0
      text = ""
      first = 1
      while ((got = (getline line < path)) > 0) {
        text = text (first ? "" : "\n") line
        first = 0
      }
      if (got < 0) printf "rule unread %s\n", shown(path)
      close(path)
      scan(path, text)
      files++
    }
    END { printf "files %d\n", files }
  ' "$1"
}

dep_info_files() {
  awk '
    index($0, "{\"reason\":\"compiler-artifact\"") == 1 && index($0, "\"package_id\":\"path+file") > 0 {
      if (!match($0, /"filenames":\["[^"]*"/)) next
      file = substr($0, RSTART + 14, RLENGTH - 15)
      gsub(/\\\\/, "/", file)
      dir = file
      sub(/\/[^\/]*$/, "", dir)
      stem = substr(file, length(dir) + 2)
      sub(/^lib/, "", stem)
      sub(/\.[^.]*$/, "", stem)
      print dir "/" stem ".d"
    }
  ' "$artifacts/forbid-dev.json" "$artifacts/forbid-release.json" | sort -u >"$work/dep-info-files.txt" || fail setup
  : >"$work/dep-info-named.txt"
  while IFS= read -r depfile; do
    if [ ! -f "$depfile" ]; then
      printf 'rule unread %s\n' "${depfile##*/}"
      holds=no
      continue
    fi
    awk '
      NR == 1 {
        line = $0
        sub(/^.*\.d: /, "", line)
        gsub(/\\ /, "\001", line)
        n = split(line, part, " ")
        for (i = 1; i <= n; i++) {
          if (part[i] == "") continue
          gsub(/\001/, " ", part[i])
          print part[i]
        }
      }
    ' "$depfile" >>"$work/dep-info-named.txt" || fail setup
  done <"$work/dep-info-files.txt"
}

all_text_rules() {
  git ls-tree -r -z --name-only "$tree" -- crates/halvo-route crates/halvo-route-gate crates/halvo-route-wire \
    >"$work/crate-files.z" || fail setup
  tr '\000' '\n' <"$work/crate-files.z" >"$work/crate-files.txt" || fail setup
  dep_info_files
  sort -u "$work/crate-files.txt" "$work/dep-info-named.txt" >"$work/text-files.txt" || fail setup
  text_rules "$work/text-files.txt" >"$work/text-rules.txt" || fail setup
  refused=0
  while IFS= read -r line; do
    case $line in
      'files '*) read_files=${line#files } ;;
      *)
        printf '%s\n' "$line"
        refused=$((refused + 1))
        ;;
    esac
  done <"$work/text-rules.txt"
  printf 'egress rules files=%s refused=%s\n' "${read_files:-0}" "$refused"
  [ "$refused" -eq 0 ] || holds=no
}

forbid_crates() {
  started=$(date +%s)
  flag=
  [ "$1" != release ] || flag=--release
  forbid "$artifacts/forbid-$1" -p halvo-route -p halvo-route-gate -p halvo-route-wire --lib --bins ${flag:+"$flag"}
  forbid_exit=$?
  seconds_line="$seconds_line forbid-$1=$(($(date +%s) - started))"
  read_forbid "$artifacts/forbid-$1.json" >"$work/forbid-read.txt" || fail setup
  findings=0
  while IFS=' ' read -r kind code where at; do
    case $kind in
      finding)
        findings=$((findings + 1))
        printf 'forbid %s %s:%s %s\n' "$1" "$where" "$at" "$code"
        ;;
      refer)
        printf 'egress forbid list-entry=refers-to-nothing\n'
        holds=no
        ;;
    esac
  done <"$work/forbid-read.txt"
  printf 'egress forbid profile=%s findings=%s exit=%s\n' "$1" "$findings" "$forbid_exit"
  if [ "$findings" -ne 0 ] || [ "$forbid_exit" -ne 0 ]; then
    holds=no
  fi
}

import_listing() {
  listing=$artifacts/route-gate.imports
  if [ "$job_system" = unix ]; then
    if command -v nm >/dev/null 2>&1 && nm -u "$exe" >"$listing" 2>"$listing.err"; then
      count=$(awk 'END { print NR }' "$listing") || count=-
      printf 'egress imports binary=dev reader=nm count=%s\n' "$count"
    else
      printf 'egress imports binary=dev reader=nm count=- (nm could not read the binary)\n'
    fi
    return
  fi
  reader=
  if [ -n "${R6_EGRESS_SYSROOT+set}" ]; then
    sysroot=$R6_EGRESS_SYSROOT
  else
    sysroot=$(rustc --print sysroot 2>/dev/null) || sysroot=
  fi
  for found in "$sysroot"/lib/rustlib/*/bin/llvm-readobj.exe "$sysroot"/lib/rustlib/*/bin/llvm-readobj; do
    if [ -n "$sysroot" ] && [ -f "$found" ]; then
      reader=$found
      break
    fi
  done
  if [ -z "$reader" ]; then
    printf 'egress imports binary=dev reader=none (llvm-readobj of rustup llvm-tools is not on this runner)\n'
    return
  fi
  if "$reader" --coff-imports "$exe" >"$listing" 2>"$listing.err"; then
    count=$(grep -c 'Symbol: ' "$listing") || count=0
    printf 'egress imports binary=dev reader=llvm-readobj count=%s\n' "$count"
  else
    printf 'egress imports binary=dev reader=llvm-readobj count=- (llvm-readobj could not read the binary)\n'
  fi
}

tree_changes() {
  git -c core.fsmonitor=false -c core.safecrlf=false diff --no-renames --name-status "$tree" -- >"$1" 2>"$1.err"
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

closed_graph() {
  started=$(date +%s)
  cargo tree --locked -e normal,build --target all -p halvo-route-gate --prefix none --format '{p}' \
    >"$artifacts/cargo-tree-build.txt" 2>"$artifacts/cargo-tree-build.err" || fail setup
  cargo tree --locked -e normal,no-proc-macro --target all -p halvo-route-gate --prefix none --format '{p}' \
    >"$artifacts/cargo-tree.txt" 2>"$artifacts/cargo-tree.err" || fail setup
  cargo metadata --locked --format-version 1 --no-deps >"$artifacts/cargo-metadata.json" 2>"$artifacts/cargo-metadata.err" ||
    fail setup
  graph_root=$({ pwd -W 2>/dev/null || pwd -P; }) || fail setup
  awk -v linked_file="$artifacts/cargo-tree.txt" -v root="$graph_root" '
    function name_of(line) {
      sub(/ \(\*\)$/, "", line)
      sub(/ \(proc-macro\)$/, "", line)
      return line
    }
    BEGIN {
      split("halvo-route halvo-route-gate halvo-route-wire", w, " ")
      for (i in w) path_ok[w[i]] = 1
      split("itoa memchr serde serde_core serde_json zmij", w, " ")
      for (i in w) linked_ok[w[i]] = 1
      split("proc-macro2 quote serde_derive syn unicode-ident", w, " ")
      for (i in w) build_ok[w[i]] = 1
      while ((getline line < linked_file) > 0) {
        line = name_of(line)
        if (line ~ / \(.*\)$/) continue
        split(line, f, " ")
        if (f[1] != "") linked[f[1]] = 1
      }
    }
    {
      line = name_of($0)
      split(line, f, " ")
      if (f[1] == "") next
      if (line ~ / \(.*\)$/) {
        path = line
        sub(/^[^(]*\(/, "", path)
        sub(/\)$/, "", path)
        gsub(/\\/, "/", path)
        if (!(f[1] in path_ok) || tolower(path) != tolower(root "/crates/" f[1])) print "graph unknown=" f[1]
        else paths[f[1]] = 1
        next
      }
      if (f[1] in linked) {
        if (!(f[1] in linked_ok)) print "graph unknown=" f[1]
        else seen_linked[f[1]] = 1
      } else if (!(f[1] in build_ok)) print "graph unknown=" f[1]
      else seen_build[f[1]] = 1
    }
    END {
      for (k in path_ok) if (!(k in paths)) print "graph missing=" k
      for (k in linked_ok) if (!(k in seen_linked)) print "graph missing=" k
      for (k in build_ok) if (!(k in seen_build)) print "graph missing=" k
    }
  ' "$artifacts/cargo-tree-build.txt" >"$work/graph-read.txt" || fail setup
  awk '
    {
      n = split($0, part, "\"manifest_path\":\"")
      for (i = 2; i <= n; i++) {
        path = substr(part[i], 1, index(part[i], "\"") - 1)
        gsub(/\\\\/, "/", path)
        crate = ""
        if (path ~ /\/crates\/halvo-route\/Cargo\.toml$/) crate = "halvo-route"
        if (path ~ /\/crates\/halvo-route-gate\/Cargo\.toml$/) crate = "halvo-route-gate"
        if (path ~ /\/crates\/halvo-route-wire\/Cargo\.toml$/) crate = "halvo-route-wire"
        if (crate == "") continue
        body = part[i - 1]
        print "graph seen=" crate
        rest = body
        while (match(rest, /"kind":\[[^]]*\]/)) {
          kinds = substr(rest, RSTART, RLENGTH)
          if (kinds ~ /"custom-build"/) print "graph target=" crate " kind=custom-build"
          if (kinds ~ /"proc-macro"/) print "graph target=" crate " kind=proc-macro"
          rest = substr(rest, RSTART + RLENGTH)
        }
        last = 0
        rest = body
        while ((k = index(rest, "\"features\":{")) > 0) {
          last += k
          rest = substr(rest, k + 1)
        }
        if (last == 0 || substr(body, last + 11, 2) != "{}") print "graph features=" crate
      }
    }
  ' "$artifacts/cargo-metadata.json" >>"$work/graph-read.txt" || fail setup
  for crate in halvo-route halvo-route-gate halvo-route-wire; do
    grep -q -x -F -e "graph seen=$crate" "$work/graph-read.txt" || printf 'graph unread=%s\n' "$crate" >>"$work/graph-read.txt"
    if grep -q -E '^[[:space:]]*\[features' "crates/$crate/Cargo.toml" 2>/dev/null; then
      printf 'graph features=%s\n' "$crate" >>"$work/graph-read.txt"
    fi
  done
  if grep -q -E '^[[:space:]]*\[patch' Cargo.toml 2>/dev/null; then
    printf 'graph patch=Cargo.toml\n' >>"$work/graph-read.txt"
  fi
  graph_seconds=$(($(date +%s) - started))
  graph_holds=yes
  while IFS= read -r line; do
    case $line in
      'graph seen='*) ;;
      *)
        printf 'egress %s\n' "$line"
        graph_holds=no
        ;;
    esac
  done <"$work/graph-read.txt"
  printf 'egress graph paths=3 linked=6 build=5 holds=%s\n' "$graph_holds"
  [ "$graph_holds" = yes ] || holds=no
}

read_graph() {
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

list_copy() {
  calls=$((calls + 1))
  list_given=$work/list-$calls.txt
  list_given_git=$work_git/list-$calls.txt
  printf '%s' "$list_text" >"$list_given" || fail setup
}

fast_view() {
  att=
  [ ! -f "$fast_attributes" ] || att=$fast_attributes$nl
  git -c core.quotePath=false hash-object --no-filters --stdin-paths 2>/dev/null <<EOF
$1
$att$tracked_paths
EOF
  printf '%s\n' --kinds--
  set -f
  IFS=$nl
  # shellcheck disable=SC2086
  printf '%s\000' $tracked_all | xargs -0 ls -ldn 2>&1
  printf '%s\n' --config--
  git config --list --show-origin 2>&1
  printf '%s\n' --untracked--
  git -c core.fsmonitor=false -c core.untrackedCache=false -c core.quotePath=false ls-files --others --directory
}

fast_state() {
  fast_now=$(fast_view "$1" && printf x) || fast_now=-
  fast_now=${fast_now%x}
  case $fast_now in
    *"${nl}target/$nl"*) fast_now=${fast_now%%"${nl}target/$nl"*}$nl${fast_now#*"${nl}target/$nl"} ;;
  esac
}

list_after() {
  fast_state "$list_given_git"
  [ "$fast_now" != "$fast_reference" ] || return 0
  copy_id=$(git hash-object --no-filters -- "$list_given" 2>/dev/null) || copy_id=-
  if [ "$copy_id" != "$list_copy_id" ]; then
    printf 'egress %s: a wrapped run changed the list\n' "$1"
    changed=yes
  fi
  tree_after "$1"
}

tree_hashes() {
  printf '%s\n' "$tracked_paths" | git -c core.quotePath=false hash-object --no-filters --stdin-paths 2>/dev/null
  printf '%s\n' "$tracked_all" | tr '\n' '\000' | xargs -0 ls -ldn 2>&1 | awk '
    {
      kind = substr($1, 1, 1) substr($1, 4, 1)
      at = index($0, " -> ")
      print "kind " kind (at > 0 ? " " substr($0, at + 4) : "") " " NR
    }
  '
}

git_view() {
  git config --list --show-origin 2>&1
  attributes=$(git rev-parse --git-path info/attributes) || return 0
  [ ! -f "$attributes" ] || cat "$attributes"
}

tree_after() {
  hashes_now=$(tree_hashes) || hashes_now=-
  if [ "$hashes_now" != "$tracked_hashes" ]; then
    changed=yes
    tree_report=$({
      printf '%s\n' "$tracked_paths"
      printf '%s\n' '--reference--'
      printf '%s\n' "$tracked_hashes"
      printf '%s\n' '--now--'
      printf '%s\n' "$hashes_now"
    } | awk -v list="$list" '
      /^kind / { kind[$0] += (part == 1 ? 1 : (part == 2 ? -1 : 0)); next }
      $0 == "--reference--" { part = 1; next }
      $0 == "--now--" { part = 2; next }
      part == 0 { path[++n] = $0; next }
      part == 1 { ref[++r] = $0; next }
      { now[++m] = $0 }
      END {
        shown = 0
        for (i = 1; i <= n; i++) {
          if (ref[i] == now[i]) continue
          if (path[i] == list) print "list"
          if (shown < 5) print "tree changed=after M " path[i]
          shown++
        }
        for (k in kind) if (kind[k] != 0) print "tree changed=after " k
      }
    ')
    case $nl$tree_report$nl in
      *"${nl}list$nl"*) printf 'egress %s: a wrapped run changed the list\n' "$1" ;;
    esac
    printf '%s\n' "$tree_report" | grep -v '^list$'
  fi
  view_now=$(git_view) || view_now=-
  if [ "$view_now" != "$git_reference" ]; then
    printf "egress %s: a wrapped run changed git's configuration\n" "$1"
    changed=yes
  fi
  untracked_now=$(untracked_view) || untracked_now=-
  if [ "$untracked_now" != "$untracked_reference" ]; then
    changed=yes
    {
      printf '%s\n' "$untracked_reference"
      printf '%s\n' '--now--'
      printf '%s\n' "$untracked_now"
    } | awk -v who="$1" '
      $0 == "--now--" { part = 1; next }
      $0 == "" { next }
      part == 0 { before[$0] = 1; next }
      {
        now[$0] = 1
        if (!($0 in before) && shown++ < 5) printf "egress %s: a wrapped run left a new path: %s\n", who, $0
      }
      END { for (p in before) if (!(p in now) && gone++ < 5) printf "egress %s: a wrapped run took away a path: %s\n", who, p }
    '
  fi
}

untracked_view() {
  untracked_listing=$(git -c core.fsmonitor=false -c core.untrackedCache=false -c core.quotePath=false \
    ls-files --others --directory) || return 1
  printf '%s\n' "$untracked_listing" | awk '$0 != "target/"'
}

wrapped_call() {
  call_out=$1
  shift
  list_copy
  egress_numbers
  call_err=$(sh scripts/egress/check.sh "$list_given" -- "$@" 2>&1 >"$call_out" </dev/null)
  code=$?
  printf '%s\n' "$call_err" >"$call_out.err"
  last=${call_err##*"$nl"}
}

read_wrapped() {
  rest=${last#"egress mode="}
  run_mode=${rest%% *}
  rest=${rest#*" list=$list_given command="}
  status=${rest%% *}
  rest=${rest#*" denied="}
  count=${rest%% *}
  log=${rest#*" log="}
  wrapped=line
  case $last in
    "REFUSED mode: "*)
      wrapped=refused
      return
      ;;
    "egress mode=$run_mode list=$list_given command=$status denied=$count log=$log") ;;
    *)
      wrapped=missing
      return
      ;;
  esac
  case $count in
    '' | *[!0-9]*) wrapped=missing ;;
  esac
}

wrapped_holds() {
  if [ "$status" != 0 ]; then
    return 1
  elif [ "$count" -eq 0 ] && [ "$1" -ne 0 ]; then
    return 1
  elif [ "$count" -ne 0 ] && [ "$1" -ne 1 ]; then
    return 1
  fi
  return 0
}

egress_numbers() {
  case $- in
    *f*) noglob=yes ;;
    *) noglob=no ;;
  esac
  set +f
  before_numbers=
  for egress_dir in target/egress/*/; do
    egress_number=${egress_dir#target/egress/}
    egress_number=${egress_number%/}
    case $egress_number in
      '' | *[!0-9]*) continue ;;
    esac
    [ ! -h "target/egress/$egress_number" ] || continue
    before_numbers=$before_numbers$egress_number$nl
  done
  [ "$noglob" = no ] || set -f
}

log_holds() {
  case $log in
    target/egress/*/denied.log) number=${log#target/egress/} ;;
    *) return 1 ;;
  esac
  number=${number%/denied.log}
  case $number in
    '' | *[!0-9]*) return 1 ;;
  esac
  case $nl$before_numbers$nl in
    *"$nl$number$nl"*) return 1 ;;
  esac
  [ -f "$log" ] && [ ! -h "$log" ] || return 1
  lines=0
  while IFS= read -r line || [ -n "$line" ]; do
    lines=$((lines + 1))
  done <"$log" || return 1
  [ "$lines" = "$count" ]
}

start_listener() {
  "$spawn" serve "$1" "$work/$2-stop" "$work/$2-ready" &
  listener=$!
  waited=0
  while [ ! -f "$work/$2-ready" ] && [ "$waited" -lt 100 ]; do
    kill -0 "$listener" 2>/dev/null || break
    sleep 0.05
    waited=$((waited + 1))
  done
  [ -f "$work/$2-ready" ] && return 0
  : >"$work/$2-stop"
  wait "$listener"
  listener=
  printf 'egress %s refused=port 127.0.0.1:%s\n' "$2" "$1"
  holds=no
  return 1
}

control() {
  sh scripts/egress/probe.sh none >"$artifacts/control-build.out" 2>"$artifacts/control-build.err" || fail setup
  spawn=target/egress/bin/egress-spawn
  [ ! -f "$spawn.exe" ] || spawn=$spawn.exe
  [ -f "$spawn" ] || fail setup
  start_listener 47201 control || return
  try=0
  while [ "$try" -lt 4 ]; do
    try=$((try + 1))
    wrapped_call "$artifacts/control-$try.out" sh scripts/egress/probe.sh loopback
    list_after control
    read_wrapped
    case $wrapped in
      refused)
        refused_mode=yes
        ran=no
        printf 'egress control refused=mode\n'
        break
        ;;
      missing)
        holds=no
        printf 'egress control line=missing exit=%s\n' "$code"
        break
        ;;
    esac
    if [ "$status" = 0 ] && [ "$count" -eq 0 ]; then
      holds=no
      printf 'egress control: the egress check let the control through\n'
      break
    fi
    if ! log_holds; then
      holds=no
      printf 'egress control log=not-new-or-not-its-own\n'
      break
    fi
    if [ "$count" -gt 0 ]; then
      break
    fi
    if [ "$try" -eq 4 ]; then
      holds=no
      printf 'egress control: the probe was stopped in 4 tries and no report of a denial arrived\n'
    fi
  done
  : >"$work/control-stop"
  wait "$listener"
  printf 'egress control denied=%s log=%s\n' "${count:--}" "${log:--}"
}

printing() {
  wrapped_call "$work/printing.out" "$exe" commands
  found_commands=$(cat "$work/printing.out" && printf x) || found_commands=x
  found_commands=${found_commands%x}
  printf '%s' "$found_commands" >"$artifacts/printing.out" || fail setup
  list_after printing
  read_wrapped
  case $wrapped in
    refused)
      refused_mode=yes
      ran=no
      printf 'egress printing refused=mode\n'
      return
      ;;
    missing)
      ran=no
      printf 'egress printing line=missing exit=%s\n' "$code"
      return
      ;;
  esac
  denied=$((denied + count))
  printf 'egress printing mode=%s command=%s denied=%s exit=%s\n' "$run_mode" "$status" "$count" "$code"
  if [ "$status" != 0 ]; then
    printf 'egress printing exit=%s (route-gate commands printed no names)\n' "$status"
    fail check
  fi
  wrapped_holds "$code" || ran=no
  if ! log_holds; then
    printf 'egress printing log=not-new-or-not-its-own\n'
    ran=no
  fi
}

state_before_calls() {
  run_entries=$(run_list) || fail setup
  cannot_entries=$(cannot_list) || fail setup
  list_blob=$(git rev-parse --verify --quiet "$tree:$list") || fail setup
  tracked_paths=$(git -c core.quotePath=false ls-tree -r "$tree" | awk -F '\t' '$1 ~ /^100/ { print $2 }') || fail setup
  tracked_all=$(git -c core.quotePath=false ls-tree -r "$tree" | awk -F '\t' '$1 ~ /^1[02]0/ { print $2 }') || fail setup
  list_text=$(git cat-file blob "$list_blob" && printf x) || fail setup
  list_text=${list_text%x}
  printf '%s' "$list_text" >"$work/list-0.txt" || fail setup
  list_copy_id=$(git hash-object --no-filters -- "$work/list-0.txt") || fail setup
  tree_changes "$work/before-calls.txt" || fail setup
  if [ -s "$work/before-calls.txt" ]; then
    report_changes before-calls "$work/before-calls.txt"
    changed=yes
  fi
  tracked_hashes=$(tree_hashes) || fail setup
  git_reference=$(git_view) || fail setup
  out_dir=$(mktemp -d "${TMPDIR:-/tmp}/$name-out.XXXXXX") || fail setup
  untracked_reference=$(untracked_view) || fail setup
  work_git=$(absolute_dir "$work") || fail setup
  [ -n "$work_git" ] || fail setup
  fast_attributes=$(git rev-parse --git-path info/attributes 2>/dev/null) || fast_attributes=
  list_given_git=$work_git/list-0.txt
  fast_state "$list_given_git"
  fast_reference=$fast_now
}

compare_lists() {
  run_words=$(printf '%s\n' "$run_entries" | awk 'NF { print $1 }') || fail setup
  cannot_words=$(printf '%s\n' "$cannot_entries" | awk 'NF { print $1 }') || fail setup
  listed=$(printf '%s\n%s\n' "$run_words" "$cannot_words" | awk 'NF' | sort) || fail setup
  run_names=$(printf '%s\n' "$run_words" | sort -u | awk 'NF { n++ } END { print n + 0 }') || fail setup
  matched=yes
  twice=$(printf '%s\n' "$listed" | awk 'seen[$0]++ == 1') || fail setup
  for word in $twice; do
    printf 'egress listed-twice=%s\n' "$word"
    matched=no
  done
  commands=$(printf '%s' "$found_commands" | awk 'END { print NR }') || fail setup
  mismatch=$({
    printf '%s\n' "$listed" | awk 'NF { print "L " $0 }'
    printf '%s' "$found_commands" | awk '{ print "P " $0 }'
  } | awk '
    substr($0, 1, 2) == "L " { listed[substr($0, 3)] = 1; next }
    {
      printed++
      name = substr($0, 3)
      found[name] = 1
      if (name in listed) next
      if (name == "") printf "egress on-neither-list= (an empty name, printed line %d)\n", printed
      else print "egress on-neither-list=" name
    }
    END { for (word in listed) if (!(word in found)) print "egress no-command=" word }
  ') || fail setup
  if [ -n "$mismatch" ]; then
    printf '%s\n' "$mismatch"
    matched=no
  fi
  cannot=$(printf '%s\n' "$cannot_words" | awk 'NF { n++ } END { print n + 0 }') || fail setup
  if [ "$commands" -ne $((run_names + cannot)) ]; then
    printf 'egress commands=%s is not run=%s plus cannot=%s\n' "$commands" "$run_names" "$cannot"
    matched=no
  fi
  printf 'egress commands=%s run=%s cannot=%s crates=%s\n' "$commands" "$run_names" "$cannot" "$crates"
  printf '%s\n' "$cannot_entries" | while IFS=' ' read -r word reason; do
    [ -z "$word" ] || printf 'egress cannot=%s reason=%s\n' "$word" "$reason"
  done
}

run_checks() {
  i=0
  checks=0
  expected=0
  made=
  saved_ifs=$IFS
  set -f
  IFS=$nl
  for entry in $run_entries; do
    IFS=$saved_ifs
    i=$((i + 1))
    word=${entry%% *}
    case $nl$found_commands$nl in
      *"$nl$word$nl"*) ;;
      *)
        IFS=$nl
        continue
        ;;
    esac
    expected=$((expected + 1))
    # shellcheck disable=SC2086
    set -- $entry
    shift
    for arg do
      shift
      case $arg in
        @/*) arg=$out_dir${arg#@} ;;
      esac
      set -- "$@" "$arg"
    done
    wrapped_call "$artifacts/run-$i.out" "$exe" "$word" "$@"
    list_after "run=$word"
    read_wrapped
    case $wrapped in
      refused)
        refused_mode=yes
        ran=no
        printf 'egress run=%s refused=mode\n' "$word"
        IFS=$nl
        continue
        ;;
      missing)
        ran=no
        printf 'egress run=%s line=missing exit=%s\n' "$word" "$code"
        IFS=$nl
        continue
        ;;
    esac
    checks=$((checks + 1))
    made=$made$word$nl
    denied=$((denied + count))
    printf 'egress run=%s mode=%s command=%s denied=%s exit=%s\n' "$word" "$run_mode" "$status" "$count" "$code"
    wrapped_holds "$code" || ran=no
    if ! log_holds; then
      printf 'egress run=%s log=not-new-or-not-its-own\n' "$word"
      ran=no
    fi
    IFS=$nl
  done
  IFS=$saved_ifs
  set +f
  if [ "$checks" -eq 0 ]; then
    printf 'egress checks=0: no command of the first list ran\n'
    ran=no
  fi
  if [ "$checks" -ne "$expected" ]; then
    ran=no
    for word in $run_words; do
      case $nl$found_commands$nl in
        *"$nl$word$nl"*) ;;
        *) continue ;;
      esac
      case $nl$made in
        *"$nl$word$nl"*) ;;
        *) printf 'egress run-missing=%s (no run of it was made and read)\n' "$word" ;;
      esac
    done
  fi
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

stopped() {
  trap '' INT TERM HUP
  : >"$work/control-stop" 2>/dev/null
  [ -z "$listener" ] || wait "$listener" 2>/dev/null
  printf 'egress stopped by a signal\n'
  fail signal
}

check() {
  enter_repo
  artifacts=${CI_D_ARTIFACTS:-target/ci-artifacts/$name}
  [ -d "$artifacts" ] || mkdir -p "$artifacts" 2>/dev/null || fail setup
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail setup
  listener=
  trap stopped INT TERM HUP
  GIT_NO_REPLACE_OBJECTS=1
  export GIT_NO_REPLACE_OBJECTS
  R6_EGRESS_CHECK_PID=$$
  export R6_EGRESS_CHECK_PID
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
  env_rules
  count_entries
  calls=0
  changed=no
  not_observed

  closed_graph
  write_forbid_list
  write_canary
  seconds_line=
  run_canary dev
  run_canary release
  if [ "$graph_holds" = yes ]; then
    forbid_crates dev
    forbid_crates release
  else
    : >"$artifacts/forbid-dev.json" || fail setup
    : >"$artifacts/forbid-release.json" || fail setup
    printf 'egress forbid=not-made (the graph refused, so no cargo call compiles the three crates)\n'
  fi
  all_text_rules
  printf 'egress seconds graph=%s%s\n' "$graph_seconds" "$seconds_line"
  if [ "$holds" = no ]; then
    printf 'egress runs=not-made (a hold refused)\n'
    fail check
  fi

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
  import_listing

  denied=0
  ran=yes
  refused_mode=no
  read_graph
  state_before_calls
  control
  printing
  compare_lists
  run_checks

  fast_state "$list_given_git"
  [ "$fast_now" = "$fast_reference" ] || tree_after end

  if [ "$checks" -gt 0 ] && [ "$checks" -eq "$expected" ]; then
    printf 'egress checks=%s denied=%s entries=%s\n' "$checks" "$denied" "$entries"
  fi
  if [ "$ran" = yes ] && [ "$denied" -eq 0 ] && [ "$checks" -eq "$expected" ]; then
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
  printf 'FAIL self-test:%s (%s)\n' "$name" "$1" >&2
  exit 1
}

write_stubs() {
  real_git=$(command -v git) || self_failed "setup at line $LINENO"
  cat >"$work/bin/git" <<EOF || self_failed "setup at line $LINENO"
#!/bin/sh
if [ "\$*" = 'rev-parse --verify --quiet HEAD^{tree}' ]; then
  [ -z "\${STUB_INPUT:-}" ] || . "\$STUB_INPUT"
  [ -n "\${STUB_TREE:-}" ] || exit 1
  printf '%s\n' "\$STUB_TREE"
  exit 0
fi
exec '$real_git' "\$@"
EOF
  real_rustc=$(command -v rustc) || self_failed "no rustc"
  stub_sysroot=$(rustc --print sysroot) || self_failed "setup at line $LINENO"
  cat >"$work/bin/rustc" <<EOF || self_failed "setup at line $LINENO"
#!/bin/sh
if [ "\$*" = '--print sysroot' ]; then
  printf 'x\n' >>'$work/sysroot-asks.txt'
  printf '%s\n' '$stub_sysroot'
  exit 0
fi
exec '$real_rustc' "\$@"
EOF
  chmod +x "$work/bin/rustc" || self_failed "setup at line $LINENO"
  cat >"$work/bin/cargo" <<'EOF' || self_failed "setup at line $LINENO"
#!/bin/sh
[ -z "${STUB_INPUT:-}" ] || . "$STUB_INPUT"
paths() {
  repo_w=$(cd "$CI_D_REPO" && { pwd -W 2>/dev/null || pwd -P; }) || exit 96
  for crate in halvo-route-gate halvo-route halvo-route-wire; do
    at=crates/$crate
    [ "$STUB_GRAPH:$crate" != moved:halvo-route ] || at=vendor/crates/$crate
    printf '%s v0.0.0 (%s/%s)\n' "$crate" "$repo_w" "$at"
  done
  [ "$STUB_GRAPH" != extra-path ] || printf 'halvo-cap v0.0.0 (%s/crates/halvo-cap)\n' "$CI_D_REPO"
}
linked_rest() {
  printf 'serde_json v1.0.154\nitoa v1.0.19\nmemchr v2.8.3\nserde v1.0.229 (*)\n'
  [ "$STUB_GRAPH" = missing ] || printf 'zmij v1.0.23\n'
  [ "$STUB_GRAPH" != extra-third ] || printf 'libc v0.2.180\n'
  [ "$STUB_GRAPH" != linked-syn ] || printf 'syn v3.0.10\n'
}
case $* in
  'build --locked -p halvo-route-gate --bin route-gate')
    [ -z "$STUB_BUILD_TOUCH" ] || printf 'x\n' >>"$STUB_BUILD_TOUCH"
    exit 0
    ;;
  'build --locked -p halvo-route-gate --bin route-gate --message-format=json')
    printf '{"reason":"compiler-artifact","package_id":"path+file://%s/crates/halvo-route-gate#0.0.0","manifest_path":"%s/crates/halvo-route-gate/Cargo.toml","target":{"kind":["bin"],"name":"route-gate","src_path":"%s/crates/halvo-route-gate/src/main.rs"},"executable":"%s","fresh":%s}\n' \
      "$CI_D_REPO" "$CI_D_REPO" "$CI_D_REPO" "$STUB_EXE" "$STUB_FRESH"
    exit 0
    ;;
  "clippy --locked --manifest-path $CI_D_ARTIFACTS/canary/Cargo.toml --lib --bins --message-format=json -- -F clippy::disallowed_types -F clippy::disallowed_methods -F unsafe_code" | \
    "clippy --locked --manifest-path $CI_D_ARTIFACTS/canary/Cargo.toml --lib --bins --release --message-format=json -- -F clippy::disallowed_types -F clippy::disallowed_methods -F unsafe_code")
    profile=dev
    case $* in *' --release '*) profile=release ;; esac
    want=$(cd "$CI_D_ARTIFACTS/forbid-list" && { pwd -W 2>/dev/null || pwd; }) || exit 96
    got=$(cd "$CLIPPY_CONF_DIR" && { pwd -W 2>/dev/null || pwd; }) || exit 96
    [ "$got" = "$want" ] || exit 96
    if [ "$STUB_CANARY" = real ]; then
      exec "$REAL_CARGO" "$@"
    fi
    sys=unix
    [ "$CI_D_JOB" != windows ] || sys=windows
    awk -v sys="$sys" '
      /^#\[cfg\(/ {
        c = $0
        sub(/^#\[cfg\(/, "", c)
        sub(/\)\]$/, "", c)
        skip = (c != sys)
        next
      }
      /^pub fn / {
        entry = $3
        sub(/\(.*/, "", entry)
        if (skip) {
          skip = 0
          next
        }
        code = "unsafe_code"
        if (entry ~ /^t/) code = "clippy::disallowed_types"
        if (entry ~ /^m/) code = "clippy::disallowed_methods"
        print entry, code, NR
      }
    ' "$CI_D_ARTIFACTS/canary/src/lib.rs" >"$CI_D_ARTIFACTS/stub-canary-$profile.txt" || exit 96
    while IFS=' ' read -r entry code at; do
      if [ "$profile" = "${STUB_CANARY_DROP_PROFILE:-dev}" ] && [ "$entry" = "$STUB_CANARY_DROP" ]; then
        continue
      fi
      printf '{"reason":"compiler-message","message":{"rendered":"error: x\\n  --> src/lib.rs:%s:1\\n","children":[],"code":{"code":"%s","explanation":null},"level":"error","spans":[]}}\n' "$at" "$code"
    done <"$CI_D_ARTIFACTS/stub-canary-$profile.txt"
    if [ "$STUB_CANARY" = refer ]; then
      printf '{"reason":"compiler-message","message":{"rendered":"warning: `std::x` does not refer to a reachable type\\n","children":[],"code":null,"level":"warning","spans":[]}}\n'
    fi
    exit 101
    ;;
  'clippy --locked -p halvo-route -p halvo-route-gate -p halvo-route-wire --lib --bins --message-format=json -- -F clippy::disallowed_types -F clippy::disallowed_methods -F unsafe_code' | \
    'clippy --locked -p halvo-route -p halvo-route-gate -p halvo-route-wire --lib --bins --release --message-format=json -- -F clippy::disallowed_types -F clippy::disallowed_methods -F unsafe_code')
    json=$STUB_CLIPPY_JSON
    status=$STUB_CLIPPY_EXIT
    case $* in
      *' --release '*)
        json=$STUB_CLIPPY_RELEASE_JSON
        status=$STUB_CLIPPY_RELEASE_EXIT
        ;;
    esac
    want=$(cd "$CI_D_ARTIFACTS/forbid-list" && { pwd -W 2>/dev/null || pwd; }) || exit 96
    got=$(cd "$CLIPPY_CONF_DIR" && { pwd -W 2>/dev/null || pwd; }) || exit 96
    [ "$got" = "$want" ] || exit 96
    invalid=$(grep -c 'allow-invalid = true' "$CLIPPY_CONF_DIR/clippy.toml")
    case $CI_D_JOB:$invalid in
      macos:2 | windows:15) ;;
      *) exit 95 ;;
    esac
    if [ -f "$json" ]; then
      while IFS= read -r line || [ -n "$line" ]; do printf '%s\n' "$line"; done <"$json"
    fi
    exit "$status"
    ;;
  "tree --locked -e normal,build --target all -p halvo-route-gate --prefix none --format {p}")
    paths
    printf 'serde v1.0.229\nserde_core v1.0.229\nserde_derive v1.0.229 (proc-macro)\nproc-macro2 v1.0.107\nunicode-ident v1.0.27\nquote v1.0.47\nsyn v3.0.10\nproc-macro2 v1.0.107 (*)\n'
    [ "$STUB_GRAPH" != extra-build ] || printf 'thiserror-impl v2.0.0 (proc-macro)\n'
    linked_rest
    exit 0
    ;;
  "tree --locked -e normal,no-proc-macro --target all -p halvo-route-gate --prefix none --format {p}")
    paths
    printf 'serde v1.0.229\nserde_core v1.0.229\n'
    linked_rest
    exit 0
    ;;
  'metadata --locked --format-version 1 --no-deps')
    printf '{"packages":['
    sep=
    for crate in halvo-route halvo-route-gate halvo-route-wire; do
      kind=lib
      features={}
      [ "$STUB_GRAPH:$crate" != build-target:halvo-route ] || kind=custom-build
      [ "$STUB_GRAPH:$crate" != proc-macro-target:halvo-route-wire ] || kind=proc-macro
      [ "$STUB_GRAPH:$crate" != features:halvo-route-gate ] || features='{"net":[]}'
      printf '%s{"name":"%s","dependencies":[{"name":"serde","kind":null}],"targets":[{"kind":["lib"],"crate_types":["lib"],"name":"x","src_path":"%s/crates/%s/src/lib.rs"},{"kind":["%s"],"crate_types":["lib"],"name":"y","src_path":"%s/crates/%s/build.rs"}],"features":%s,"manifest_path":"%s/crates/%s/Cargo.toml","metadata":null}' \
        "$sep" "$crate" "$CI_D_REPO" "$crate" "$kind" "$CI_D_REPO" "$crate" "$features" "$CI_D_REPO" "$crate"
      sep=,
    done
    printf '],"workspace_root":"%s"}\n' "$CI_D_REPO"
    exit 0
    ;;
esac
printf 'stub cargo: %s\n' "$*" >&2
exit 97
EOF
  cat >"$work/route-gate" <<'EOF' || self_failed "setup at line $LINENO"
#!/bin/sh
[ -z "${STUB_INPUT:-}" ] || . "$STUB_INPUT"
[ -z "$STUB_MARKER" ] || printf 'run %s\n' "$*" >>"$STUB_MARKER"
if [ -n "$STUB_LEAVE" ] && [ "$1" = "$STUB_LEAVE" ]; then
  if [ -n "$STUB_LEAVE_AT" ]; then
    printf 'x\n' >>"$STUB_LEAVE_COUNT"
    [ "$(awk 'END { print NR }' "$STUB_LEAVE_COUNT")" != "$STUB_LEAVE_AT" ] || printf 'x\n' >left-behind.txt
  else
    printf 'x\n' >left-behind.txt
  fi
fi
if [ "$#" -eq 1 ] && [ "$1" = commands ]; then
  if [ -n "$STUB_PRINT_EXIT" ]; then
    printf 'REFUSED usage: unknown command\n' >&2
    exit "$STUB_PRINT_EXIT"
  fi
  if [ -n "$STUB_PRINT_FILTER" ]; then
    trial=docs/r2/trials/egress.md
    blob=$(git ls-files -s -- "$trial" | awk '{ print $2 }')
    git config filter.r6same.clean "cat >/dev/null; git cat-file blob $blob"
    attributes=$(git rev-parse --git-path info/attributes)
    mkdir -p "${attributes%/*}"
    printf '%s filter=r6same\n' "$trial" >"$attributes"
    tr o O <"$trial" >"$trial.new" && mv "$trial.new" "$trial"
  fi
  for known in $STUB_COMMANDS; do
    printf '%s\n' "$known"
  done
  [ -z "$STUB_PRINT_HIDDEN" ] || printf '%s\n' '--found--' ''
  exit 0
fi
for known in $STUB_COMMANDS; do
  if [ "$1" = "$known" ]; then
    [ "$1" != "$STUB_TAKE" ] || mv untracked-before.txt target/untracked-before.txt
    exit "$STUB_RUN_EXIT"
  fi
done
printf 'REFUSED usage: unknown command\n' >&2
exit 2
EOF
  chmod +x "$work/bin/git" "$work/bin/cargo" "$work/route-gate" || self_failed "setup at line $LINENO"
}

write_repo() {
  dir=$1
  mkdir -p "$dir/scripts/r6/egress" "$dir/scripts/egress" "$dir/docs/r2/trials" "$dir/crates/halvo-route-gate/src" ||
    self_failed "setup at line $LINENO"
  printf '%s\n' '[fixed]' "$2" '[agent]' | awk 'NF' >"$dir/scripts/r6/egress/gate.txt" || self_failed "setup at line $LINENO"
  cat >"$dir/docs/r2/trials/egress.md" <<'EOF' || self_failed "setup at line $LINENO"
cells: this-mac.sandbox
not-observable this-mac sandbox path-1-this-mac: one
not-observed this-mac sandbox path-2-this-mac: two
not-observable macos-15 sandbox path-1-macos-15: one
not-observed macos-15 sandbox path-2-macos-15: two
not-observable windows-2025 wfp path-1-windows-2025: one
not-observed windows-2025 wfp path-2-windows-2025: two
EOF
  printf '%s\n' 'fn main() {}' >"$dir/crates/halvo-route-gate/src/main.rs" || self_failed "setup at line $LINENO"
  cp "$work/rust-toolchain.toml" "$dir/rust-toolchain.toml" || self_failed "setup at line $LINENO"
  printf '[workspace]\nmembers = ["crates/*"]\n%s' "${3:-}" >"$dir/Cargo.toml" || self_failed "setup at line $LINENO"
  printf '[package]\nname = "halvo-route-gate"\n%s' "${4:-}" >"$dir/crates/halvo-route-gate/Cargo.toml" || self_failed "setup at line $LINENO"
  cat >"$dir/crates/halvo-route-gate/src/allowed.rs" <<'EOF' || self_failed "setup at line $LINENO"
#[cfg(all(unix, not(test)))]
pub fn a() {}
#[cfg_attr(not(test), allow(unused_variables))]
pub fn b(x: u8) {}
#[allow(clippy::needless_return)]
pub fn d() {}
#[cfg(any(windows, test))]
pub fn c() {}
macro_rules! named {
    ($name:ident) => {
        #[$name]
        fn f() {}
    };
}
EOF
  if [ -n "${5:-}" ]; then
    mkdir -p "$dir/${5%/*}" || self_failed "setup at line $LINENO"
    printf '%s\n' "$6" >"$dir/$5" || self_failed "setup at line $LINENO"
  fi
  cat >"$dir/scripts/egress/check.sh" <<'EOF' || self_failed "setup at line $LINENO"
#!/bin/sh
[ -z "${STUB_INPUT:-}" ] || . "$STUB_INPUT"
list=$1
shift 2
empty=no
for arg in "$@"; do
  [ -n "$arg" ] || empty=yes
done
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
if [ "$STUB_CHECK:${2:-}" = noline-derive:derive ]; then
  "$@"
  exit
fi
case $STUB_CHECK:${2:-} in
  rewrite-record:generate) printf 'generate\n' >"$CI_D_ARTIFACTS/printing.out" ;;
  cut-run-list:generate)
    out=
    for arg in "$@"; do
      [ "$prev" != --out ] || out=$arg
      prev=$arg
    done
    parent=${out%/*}
    parent=${parent%/*}
    for checkdir in "$parent" "$parent"/60-r6-egress.*; do
      [ -d "$checkdir" ] || continue
      : >"$checkdir/run-list.txt"
    done
    ;;
  replace-list:generate)
    listed=$(git rev-parse "$STUB_TREE:scripts/r6/egress/gate.txt") || exit 97
    wide=$({ git --no-replace-objects cat-file blob "$listed"; printf '127.0.0.1:47391\n'; } | git hash-object -w --stdin) || exit 97
    git replace -f "$listed" "$wide" || exit 97
    ;;
  replace-list:*)
    if grep -q -F -e '127.0.0.1:47391' "$list"; then
      printf 'widened %s\n' "$2" >>"$STUB_MARKER.widened"
      listed=$(git rev-parse "$STUB_TREE:scripts/r6/egress/gate.txt") || exit 97
      git --no-replace-objects cat-file blob "$listed" >"$list.true" && mv "$list.true" "$list"
    fi
    ;;
  symlink-tree:generate)
    cp docs/r2/trials/egress.md "$STUB_MARKER.copy" && ln -sf "$STUB_MARKER.copy" docs/r2/trials/egress.md
    ;;
  exec-bit:generate)
    chmod +x docs/r2/trials/egress.md
    ;;
  git-config:generate)
    git config --local r6.egress.planted generate || exit 97
    ;;
  in-place:generate)
    tr o O <docs/r2/trials/egress.md >docs/r2/trials/egress.md.new &&
      touch -r docs/r2/trials/egress.md docs/r2/trials/egress.md.new &&
      mv docs/r2/trials/egress.md.new docs/r2/trials/egress.md || exit 97
    ;;
  attributes:generate)
    attributes=$(git rev-parse --git-path info/attributes) || exit 97
    mkdir -p "${attributes%/*}" && printf 'r6-planted r6=1\n' >"$attributes" || exit 97
    ;;
  widen:generate) printf '127.0.0.1:47391\n' >>scripts/r6/egress/gate.txt ;;
  widen:probe) printf '[fixed]\n[agent]\n' >scripts/r6/egress/gate.txt ;;
  change-copy:generate) printf '127.0.0.1:47391\n' >>"$list" ;;
esac
"$@"
code=$?
[ "$empty" = no ] || code=9
[ -z "$STUB_TOUCH" ] || printf 'x\n' >>"$STUB_TOUCH"
denied=0
[ "$STUB_CHECK" != denied ] || denied=$((denied + 1))
[ "$STUB_CHECK" != denied0 ] || denied=$((denied + 1))
if [ "$1 $2" = "sh scripts/egress/probe.sh" ]; then
  case $STUB_CONTROL in
    through) denied=0 ;;
    lost) denied=0 ;;
    *) denied=1 ;;
  esac
fi
n=1
while [ -e "target/egress/$n" ]; do
  n=$((n + 1))
done
mkdir -p "target/egress/$n"
lines=$denied
[ "$STUB_CHECK" != short-log ] || lines=$((denied + 1))
[ "$STUB_CHECK:$1 $2" != "noline-control:sh scripts/egress/probe.sh" ] || exit 1
[ "$STUB_CHECK:$1 $2" != "signal:sh scripts/egress/probe.sh" ] || kill -TERM "$R6_EGRESS_CHECK_PID"
{
  i=0
  while [ "$i" -lt "$lines" ]; do
    printf '%s\n' 'denied at=- proto=tcp dest=127.0.0.1:9 pid=1 image=x'
    i=$((i + 1))
  done
} >"target/egress/$n/denied.log"
if [ "$STUB_CHECK" = link-log ]; then
  mv "target/egress/$n/denied.log" "target/egress/$n/denied.real" && ln -s denied.real "target/egress/$n/denied.log"
fi
case $STUB_CHECK:$1:$2 in
  reuse-log:*) n=1 ;;
  reuse-log-run:sh:*) ;;
  reuse-log-run:*) n=1 ;;
  reuse-previous:sh:*) ;;
  reuse-previous:*) n=$((n - 1)) ;;
esac
[ "$STUB_CHECK" != wrong-list ] || list=scripts/egress/lists/empty.txt
printf 'egress mode=sandbox list=%s command=%s denied=%s log=target/egress/%s/denied.log\n' "$list" "$code" "$denied" "$n" >&2
[ "$STUB_CHECK" != exit3 ] || exit 3
[ "$STUB_CHECK" != denied0 ] || exit 0
[ "$code" = 0 ] && [ "$denied" = 0 ]
EOF
  cat >"$dir/scripts/egress/probe.sh" <<'EOF' || self_failed "setup at line $LINENO"
#!/bin/sh
[ -z "${STUB_INPUT:-}" ] || . "$STUB_INPUT"
case $1 in
  none)
    [ ! -f target/egress/bin/egress-spawn ] || exit 0
    mkdir -p target/egress/bin || exit 2
    for spawn in target/egress/bin/egress-spawn; do
      cat >"$spawn" <<'SPAWN' || exit 2
#!/bin/sh
[ -z "${STUB_INPUT:-}" ] || . "$STUB_INPUT"
[ "$1" = serve ] || exit 2
[ "$STUB_CONTROL" != port ] || exit 3
[ "$STUB_HELD_PORT" != "$2" ] || exit 3
[ -z "$STUB_SPAWN_PIDS" ] || printf '%s\n' "$$" >>"$STUB_SPAWN_PIDS"
: >"$4"
waited=0
while [ ! -f "$3" ] && [ "$waited" -lt 600 ]; do
  sleep 0.1
  waited=$((waited + 1))
done
exit 0
SPAWN
      chmod +x "$spawn" || exit 2
    done
    exit 0
    ;;
  loopback)
    [ -z "$STUB_CONTROL_LEAVE" ] || printf 'x\n' >left-behind.txt
    [ "$STUB_CONTROL" = through ] && exit 0
    exit 1
    ;;
esac
exit 2
EOF
  git init -q "$dir" >/dev/null 2>&1 || self_failed "setup at line $LINENO"
  if ! git -C "$dir" add -A >"$dir.add.txt" 2>&1; then
    cat "$dir.add.txt" >&2
    self_failed "setup at line $LINENO"
  fi
  git -C "$dir" write-tree 2>/dev/null || self_failed "setup at line $LINENO"
}

case_reset() {
  case_repo=$work/repo
  case_tree=$good_tree
  STUB_COMMANDS='generate import derive score self-test spend-sim probe commands'
  STUB_RUN_EXIT=0
  STUB_PRINT_EXIT=
  STUB_MARKER=
  STUB_BUILD_TOUCH=
  STUB_CONTROL=
  STUB_HELD_PORT=
  STUB_LEAVE=
  STUB_TAKE=
  STUB_PRINT_FILTER=
  STUB_PRINT_HIDDEN=
  STUB_LEAVE_AT=
  STUB_CONTROL_LEAVE=
  STUB_CHECK=pass
  STUB_TOUCH=
  STUB_FRESH=true
  STUB_CLIPPY_JSON=$work/clippy-clean.json
  STUB_CLIPPY_EXIT=0
  STUB_CANARY=synth
  STUB_CANARY_DROP=0
  STUB_CANARY_DROP_PROFILE=dev
  STUB_CLIPPY_RELEASE_JSON=$work/clippy-clean.json
  STUB_CLIPPY_RELEASE_EXIT=0
  STUB_GRAPH=good
  case_job=
  case_actions=
  case_arg=
  case_env=
  case_self=
  case_sysroot=given
}

rule_cases() {
  cat <<'EOF'
forbid-reader forbid-finding
forbid-release forbid-finding-in-release
forbid-outside-path forbid-finding-outside-the-repository-on-a-drive
canary-own-entries canary-requires-a-windows-entry-in-the-windows-job
canary-refers canary-list-entry-refers-to-nothing
canary-real-cargo canary-on-the-real-cargo
graph-tree graph-path-package-moved
graph-metadata graph-features-in-metadata
graph-tables graph-patch-table
refusal-stops-the-runs runs-not-made-after-a-text-rule
canary-refusal canary-refusal-ends-as-a-hold
list-from-memory run-plants-a-replace-ref-for-the-list
tree-after-builds build-changes-a-tracked-file
git-configuration run-changes-git-configuration
rule-cfg runs-not-made-after-a-text-rule
rule-attribute rule-attribute-path-from-a-macro-variable
rule-clippy rule-clippy-behind-a-comment
rule-dep-info rule-file-named-by-dep-info
env-rules env-lowercase-flags
entries list-file-with-one-entry
entries-link list-directory-holds-a-link
entries-submodule list-directory-holds-a-submodule
compare-lists word-on-both-lists
control control-let-through
new-log log-of-the-run-before
log-link log-that-is-a-link
list-in-force run-widens-the-list-and-a-later-run-restores-it
state-in-memory-record run-rewrites-the-record-of-found-commands
state-in-memory-run-list run-cuts-the-run-list
checks-equal-found run-without-its-line-names-the-missing-run
wrapped-verdict command-exits-1
run-arguments entry-with-no-arguments
tree-check tracked-file-changed-by-a-run
cause env-refused-then-a-stale-build
signal-trap signal-stops-the-listeners
windows-job pass
windows-imports import-listing-printed
printing-names printed-command-on-neither-list
printing-every-line printed-name-that-the-list-rule-skips
printing-entries entry-that-is-no-command
printing-exit printing-that-does-not-exit-0
printing-wrapped printing-leaves-a-file-in-the-checkout
printing-state-first printing-rewrites-a-file-behind-a-clean-filter
tree-new-path import-leaves-a-file-in-the-checkout
checks-count job-with-no-run
windows-sentence sentence-on-a-hosted-runner
forbid-exit-status forbid-exit-without-a-finding
forbid-refers forbid-list-entry-refers-to-nothing
env-wrapper env-wrapper-set
env-argument-file env-flags-argument-file
graph-unknown graph-third-party-package-added
graph-build-time graph-build-time-package-added
graph-build-script graph-build-script
graph-proc-macro graph-proc-macro
wrapped-missing-line no-egress-line
wrapped-refused-mode refusal-mode
log-count log-with-another-count
wrapped-exit-0-with-a-denial check-exits-0-with-a-denial
wrapped-exit-1-with-a-denial command-exits-1-with-a-denial
denials-summed denied-in-each-run
stale-build stale-build
usage usage
no-tree no-tree
tree-before tracked-file-changed-before
EOF
}

windows_cases() {
  cat <<'EOF'
forbid-finding
forbid-finding-in-release
forbid-finding-outside-the-repository-on-a-drive
canary-requires-a-windows-entry-in-the-windows-job
canary-list-entry-refers-to-nothing
canary-on-the-real-cargo
graph-path-package-moved
graph-features-in-metadata
graph-patch-table
runs-not-made-after-a-text-rule
rule-attribute-path-from-a-macro-variable
rule-clippy-behind-a-comment
rule-file-named-by-dep-info
env-lowercase-flags
list-file-with-one-entry
list-directory-holds-a-link
list-directory-holds-a-submodule
word-on-both-lists
control-let-through
log-of-the-run-before
log-that-is-a-link
run-widens-the-list-and-a-later-run-restores-it
run-rewrites-the-record-of-found-commands
run-cuts-the-run-list
run-without-its-line-names-the-missing-run
canary-refusal-ends-as-a-hold
run-plants-a-replace-ref-for-the-list
build-changes-a-tracked-file
run-changes-git-configuration
command-exits-1
entry-with-no-arguments
tracked-file-changed-by-a-run
env-refused-then-a-stale-build
signal-stops-the-listeners
pass
import-listing-printed
printed-command-on-neither-list
printed-name-that-the-list-rule-skips
entry-that-is-no-command
printing-that-does-not-exit-0
printing-leaves-a-file-in-the-checkout
printing-rewrites-a-file-behind-a-clean-filter
import-leaves-a-file-in-the-checkout
job-with-no-run
sentence-on-a-hosted-runner
forbid-exit-without-a-finding
forbid-list-entry-refers-to-nothing
env-wrapper-set
env-flags-argument-file
graph-third-party-package-added
graph-build-time-package-added
graph-build-script
graph-proc-macro
no-egress-line
refusal-mode
log-with-another-count
check-exits-0-with-a-denial
command-exits-1-with-a-denial
denied-in-each-run
stale-build
usage
no-tree
tracked-file-changed-before
EOF
}

macos_cases() {
  cat <<'EOF'
run-swaps-a-tracked-file-for-a-link
run-sets-an-exec-bit
canary-refusal-in-release-ends-as-a-hold
sentence
commands
third-party
forbid-clean
control-denied
control-report-lost-four-times
control-port-in-use
log-of-another-run
graph-closed
rules-allowed-forms
env-deny-flag
env-allow-flag
env-encoded-allow
env-per-target-cfg
env-clippy-arguments
env-plain-wrapper-set
env-build-flags-allow
env-lowercase-wrapper
env-long-allow-flag
rule-cfg-target-os
runs-not-made-after-a-forbid-finding
runs-not-made-after-a-graph-refusal
rule-cfg-nested
rule-attribute-from-a-macro-variable
rule-cfg-macro
rule-cfg-spaced
rule-clippy-through-a-macro-variable
rule-clippy-on-a-hidden-module
rule-outside-the-crates-not-named
rule-dep-info-missing
graph-build-time-package-linked
graph-path-package-added
graph-package-gone
graph-features-table
canary-let-through-in-release
canary-without-its-unsafe-finding
canary-let-through
canary-entries-of-the-macos-job
canary-requires-a-unix-entry-in-the-macos-job
canary-entries-of-the-windows-job
windows-job-runs-every-command
forbid-list-of-the-windows-job
forbid-finding-outside-the-repository
forbid-finding-with-exit-0
forbid-child-code-is-no-finding
repository-missing
line-of-another-list
log-of-another-run-of-the-gate-tool
run-changes-its-copy-of-the-list
check-exits-3-with-a-clean-line
forbid-finding-then-a-stale-build
control-without-an-egress-line
port-47202-held-by-another-process
untracked-path-before-the-run
import-takes-away-an-untracked-path
derive-leaves-a-file-in-the-checkout
control-leaves-a-file-in-the-checkout
last-entry-leaves-a-file-in-the-checkout
shared-stand-ins-first-input
shared-stand-ins-second-input
run-writes-info-attributes
run-rewrites-a-tracked-file-in-place
EOF
}

case_named() {
  [ -z "${R6_EGRESS_CASES:-}" ] && return 0
  case " $R6_EGRESS_CASES " in
    *" $1 "*) return 0 ;;
  esac
  return 1
}

case_wanted() {
  [ -z "${R6_EGRESS_CASES:-}" ] && return 0
  case " $R6_EGRESS_CASES " in
    *" $1 "*)
      cases_matched=yes
      return 0
      ;;
  esac
  return 1
}

case_environment() {
  set -- STUB_TREE "$case_tree" STUB_EXE "$work/route-gate" STUB_COMMANDS "$STUB_COMMANDS" STUB_PRINT_EXIT "$STUB_PRINT_EXIT" \
    STUB_MARKER "$STUB_MARKER" STUB_BUILD_TOUCH "$STUB_BUILD_TOUCH" STUB_CONTROL "$STUB_CONTROL" STUB_HELD_PORT "$STUB_HELD_PORT" \
    STUB_LEAVE "$STUB_LEAVE" STUB_TAKE "$STUB_TAKE" STUB_PRINT_FILTER "$STUB_PRINT_FILTER" STUB_PRINT_HIDDEN "$STUB_PRINT_HIDDEN" \
    STUB_LEAVE_AT "$STUB_LEAVE_AT" STUB_LEAVE_COUNT "$work/leave-count" STUB_CONTROL_LEAVE "$STUB_CONTROL_LEAVE" \
    STUB_RUN_EXIT "$STUB_RUN_EXIT" STUB_CHECK "$STUB_CHECK" STUB_TOUCH "$STUB_TOUCH" STUB_FRESH "$STUB_FRESH" \
    STUB_CLIPPY_JSON "$STUB_CLIPPY_JSON" STUB_CLIPPY_EXIT "$STUB_CLIPPY_EXIT" STUB_CANARY "$STUB_CANARY" \
    STUB_CANARY_DROP "$STUB_CANARY_DROP" STUB_CANARY_DROP_PROFILE "$STUB_CANARY_DROP_PROFILE" \
    STUB_CLIPPY_RELEASE_JSON "$STUB_CLIPPY_RELEASE_JSON" STUB_CLIPPY_RELEASE_EXIT "$STUB_CLIPPY_RELEASE_EXIT" \
    STUB_GRAPH "$STUB_GRAPH" REAL_CARGO "$real_cargo" STUB_SPAWN_PIDS "$work/spawn.pids"
  case "$*" in
    *"'"*) self_failed "a stand-in input holds a quote" ;;
  esac
  printf "%s='%s'\n" "$@" >"$work/stub-input.sh" || self_failed "setup at line $LINENO"
  PATH=$work/bin:$PATH CI_D_REPO=$case_repo CI_D_ARTIFACTS=$work/artifacts TMPDIR=$work/tmp
  STUB_INPUT=$work/stub-input.sh CI_D_JOB=$case_job GITHUB_ACTIONS=$case_actions
  export PATH CI_D_REPO CI_D_ARTIFACTS TMPDIR STUB_INPUT CI_D_JOB GITHUB_ACTIONS
  if [ "$case_sysroot" != asked ]; then
    R6_EGRESS_SYSROOT=$stub_sysroot
    export R6_EGRESS_SYSROOT
  else
    unset R6_EGRESS_SYSROOT
  fi
}

case_run() {
  if ! case_wanted "$1"; then
    case_reset
    return
  fi
  if grep -q -x -F -e "$1" "$work/macos-cases.txt"; then
    printf '%s\n' "$1" >>"$work/macos-seen.txt"
    if [ "$host_job" != macos ]; then
      printf 'case %s/%s not-run: the macos job'"'"'s self-test runs it; on Windows one case of each rule runs (rule_cases)\n' "$name" "$1"
      case_reset
      return
    fi
    [ -n "$case_job" ] || case_job=$host_job
  elif grep -q -x -F -e "$1" "$work/windows-cases.txt"; then
    printf '%s\n' "$1" >>"$work/windows-seen.txt"
    [ -n "$case_job" ] || case_job=$host_job
  else
    printf 'case %s/%s want=%s got=in-neither-list\n' "$name" "$1" "$2"
    wrong="$wrong $1"
    case_reset
    return
  fi
  case_started=$(date +%s)
  (
    case_environment
    exec env ${case_env:+"$case_env"} sh "${case_self:-$self}" ${case_arg:+"$case_arg"}
  ) >"$work/case.txt" 2>&1 </dev/null
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
  if [ -n "${6:-}" ]; then
    case $out in
      *"$6"*) got=$got-with-the-second-other-text ;;
    esac
  fi
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$got"
  case_ended=$(date +%s) || case_ended=$case_started
  printf 'case-seconds %s %s job=%s\n' "$1" "$((case_ended - case_started))" "$case_job"
  if [ "$got" != "$2" ]; then
    printf 'case %s/%s exit=%s, its last lines:\n' "$name" "$1" "$code"
    tail -n 12 "$work/case.txt" | awk -v work="$work" '
      {
        line = $0
        while ((at = index(line, work)) > 0) line = substr(line, 1, at - 1) "<work>" substr(line, at + length(work))
        print "  | " line
      }
    '
    wrong="$wrong $1"
  elif [ "$2" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
  case_reset
}

part_seconds() {
  case_wanted part-seconds || return 0
  case_job=$host_job
  part_names=$(
    case_environment
    env_rules >/dev/null 2>&1
    awk 'END { print NR }' "$work/env-names.txt"
  ) || part_names=-
  printf 'part-names env-rules %s names in the environment of a case\n' "$part_names"
  (
    case_environment
    cd "$work/repo" || exit 1
    artifacts=$work/artifacts
    tree=$good_tree
    job_system=unix
    [ "$host_job" != windows ] || job_system=windows
    forbid_table >"$work/forbid-table.txt" || exit 1
    write_forbid_list
    write_canary
    forbid "$work/part-canary" --manifest-path "$canary/Cargo.toml" --lib --bins
  ) >/dev/null 2>&1
  for part in stand-in-start forbid-stub wrapped-call list-checks import-listing env-rules canary-match; do
    if [ "$part" = list-checks ]; then
      steps_part
      continue
    fi
    reps=10
    case $part in
      env-rules) reps=2 ;;
      canary-match) reps=5 ;;
    esac
    started=$(date +%s) || started=0
    i=0
    while [ "$i" -lt "$reps" ]; do
      case $part in
        stand-in-start)
          (cd "$work/repo" && PATH=$work/bin:$PATH CI_D_REPO=$work/repo STUB_GRAPH=good \
            sh "$work/bin/cargo" metadata --locked --format-version 1 --no-deps >/dev/null 2>&1) ;;
        forbid-stub)
          (cd "$work/repo" && PATH=$work/bin:$PATH CI_D_REPO=$work/repo CI_D_ARTIFACTS=$work/artifacts CI_D_JOB=$host_job \
            CLIPPY_CONF_DIR=$work/artifacts/forbid-list STUB_CLIPPY_JSON=$work/clippy-clean.json STUB_CLIPPY_EXIT=0 \
            sh "$work/bin/cargo" clippy --locked -p halvo-route -p halvo-route-gate -p halvo-route-wire --lib --bins \
            --message-format=json -- -F clippy::disallowed_types -F clippy::disallowed_methods -F unsafe_code >/dev/null 2>&1) ;;
        wrapped-call)
          (cd "$work/repo" && STUB_COMMANDS=probe \
            sh scripts/egress/check.sh scripts/r6/egress/gate.txt -- "$work/route-gate" probe --rounds 1 >/dev/null 2>&1) ;;
        import-listing)
          (
            case_environment
            job_system=unix
            [ "$host_job" != windows ] || job_system=windows
            exe=$work/route-gate
            artifacts=$work/artifacts
            import_listing >/dev/null 2>&1
          ) ;;
        env-rules)
          (case_environment && env_rules >/dev/null 2>&1) ;;
        canary-match)
          (
            case_environment
            artifacts=$work/artifacts
            canary=$artifacts/canary
            root_abs=$work/repo
            seconds_line=
            forbid() {
              cp "$work/part-canary.json" "$1.json" && : >"$1.err"
              return 101
            }
            run_canary dev >/dev/null 2>&1
          ) ;;
      esac
      i=$((i + 1))
    done
    ended=$(date +%s) || ended=$started
    printf 'part-seconds %s %s for %s calls\n' "$part" "$((ended - started))" "$reps"
  done
  case_job=
}

call_steps() {
  list_copy
  egress_numbers
  list_after part >/dev/null 2>&1
  before_numbers=
  log_holds
}

steps_part() {
  (
    case_environment
    cd "$work/repo" || exit 1
    tree=$good_tree
    calls=0
    changed=no
    state_before_calls >/dev/null 2>&1
    log=target/egress/1/denied.log
    count=0
    started=$(date +%s) || started=0
    i=0
    while [ "$i" -lt 10 ]; do
      call_steps
      i=$((i + 1))
    done
    ended=$(date +%s) || ended=$started
    printf 'part-seconds list-checks %s for 10 calls\n' "$((ended - started))"
    mkdir -p "$work/count-bin" || exit 1
    for program in awk cat cmp cp find git grep head ls mkdir sed sort tail tr wc xargs; do
      real=$(command -v "$program") || continue
      # shellcheck disable=SC2016
      printf '#!/bin/sh\nprintf '\''%s\\n'\'' >>"$R6_COUNT_FILE"\nexec '\''%s'\'' "$@"\n' "$program" "$real" \
        >"$work/count-bin/$program" || exit 1
    done
    chmod +x "$work"/count-bin/* || exit 1
    : >"$work/count.txt" || exit 1
    PATH=$work/count-bin:$PATH
    R6_COUNT_FILE=$work/count.txt
    export PATH R6_COUNT_FILE
    call_steps
    programs=0
    while IFS= read -r line; do
      programs=$((programs + 1))
    done <"$work/count.txt"
    printf 'part-programs list-checks %s for 1 call\n' "$programs"
  )
}

direct_verdict() {
  got=PASS
  case $nl$2 in
    *"${nl}holds=no"*) got=FAIL ;;
  esac
  case $2 in
    *"$4"*) ;;
    *) got=$got-without-the-text ;;
  esac
  if [ -n "$5" ]; then
    case $2 in
      *"$5"*) got=$got-with-the-other-text ;;
    esac
  fi
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$3" "$got"
  if [ "$got" != "$3" ]; then
    printf '%s\n' "$2" | sed 's/^/  | /'
    wrong="$wrong $1"
  elif [ "$3" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

env_case() {
  case_wanted "$1" || return 0
  env_out=$(
    holds=yes
    # shellcheck disable=SC2163
    export "$5" || exit 1
    env_rules
    printf 'holds=%s\n' "$holds"
  ) || env_out=exit-$?
  direct_verdict "$1" "$env_out" "$2" "$3" "$4"
}

canary_case() {
  case_wanted "$1" || return 0
  mkdir -p "$work/canary-case" || self_failed "setup at line $LINENO"
  printf '%s' "$5" >"$work/canary-case/expected.txt" || self_failed "setup at line $LINENO"
  printf '%s' "$6" >"$work/canary-case/read.txt" || self_failed "setup at line $LINENO"
  canary_out=$(
    artifacts=$work/canary-case
    canary=$work/canary-case
    seconds_line=
    holds=yes
    forbid() {
      : >"$1.json"
      return 101
    }
    read_forbid() {
      cat "$work/canary-case/read.txt"
    }
    run_canary dev
    printf 'holds=%s\n' "$holds"
  ) || canary_out=exit-$?
  direct_verdict "$1" "$canary_out" "$2" "$3" "$4"
}

marker_absent() {
  case_wanted "$1" || return 0
  if [ -s "$2" ]; then
    printf 'case %s/%s want=PASS got=route-gate-run-after-a-refusal\n' "$name" "$1"
    wrong="$wrong $1-marker"
  fi
}

finding_json() {
  # shellcheck disable=SC2016
  printf '{"reason":"compiler-message","package_id":"path+file:///x#0.0.0","message":{"rendered":"error: a use\\n  --> %s:%s:5\\n","$message_type":"diagnostic","children":[{"children":[],"code":null,"level":"note","message":"n","rendered":null,"spans":[]}],"code":{"code":"%s","explanation":null},"level":"error","message":"m","spans":[]}}\n' \
    "$3" "$4" "$2" >"$1"
}

write_forms() {
  dir=$1
  start=$2
  mkdir -p "$dir/tests" "$dir/examples" || self_failed "setup at line $LINENO"
  printf '[package]\nname = "forms"\nversion = "0.0.0"\nedition = "2021"\npublish = false\nautotests = false\nautoexamples = false\n\n[lib]\npath = "tests/lib.rs"\n\n[[bin]]\nname = "forms-bin"\npath = "examples/bin.rs"\n\n[workspace]\n' \
    >"$dir/Cargo.toml" || self_failed "setup at line $LINENO"
  printf '# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\nversion = 4\n\n[[package]]\nname = "forms"\nversion = "0.0.0"\n' \
    >"$dir/Cargo.lock" || self_failed "setup at line $LINENO"
  if [ "$3" = forms ]; then
    debug='#[cfg(not(debug_assertions))]'
    feature='#[cfg(not(feature = "x"))]'
    linux='#[cfg(target_os = "linux")]'
    extern1='unsafe extern "C" {'
    extern2='    pub safe fn getpid() -> i32;'
    extern3='}'
  else
    debug='#[cfg(not(test))]'
    feature='#[cfg(not(test))]'
    linux='#[cfg(not(test))]'
    extern1='pub fn e1() {}'
    extern2='pub fn e2() {}'
    extern3='pub fn e3() {}'
  fi
  cat >"$dir/tests/lib.rs" <<EOF || self_failed "setup at line $LINENO"
pub mod shout;
#[cfg(test)] const A: u8 = 1; pub fn one_line() -> bool { $start }
include! {"inc.txt"}
#[cfg_attr(all(), path = "cfgpath.txt")]
pub mod cfgpath;
#[ path = "spaced.txt" ]
pub mod spaced;
/* #[cfg(test)]
mod hidden; */
pub mod hidden;
pub mod shell;
pub mod sub {
    #[cfg(test)]
    mod shell;
}
#[path = "plain.txt"]
pub mod plain;
#[cfg(not(test))]
pub mod nottest;
$extern1
$extern2
$extern3
$debug
pub fn release_only() -> bool { $start }
$feature
pub fn no_feature() -> bool { $start }
$linux
pub fn linux_only() -> bool { $start }
macro_rules! gate {
    (\$test:meta) => {
        #[cfg(\$test)]
        pub fn gated() -> bool { $start }
    };
}
gate!(not(debug_assertions));
#[cfg/**/(not(debug_assertions))]
pub fn commented() -> bool { $start }
#[cfg(windows)]
pub fn windows_only() -> bool { $start }
EOF
  for file in shout.RS inc.txt cfgpath.txt spaced.txt hidden.rs shell.rs plain.txt nottest.rs; do
    printf 'pub fn f() -> bool { %s }\n' "$start" >"$dir/tests/$file" || self_failed "setup at line $LINENO"
  done
  printf 'fn main() { let _ = %s; }\n' "$start" >"$dir/examples/bin.rs" || self_failed "setup at line $LINENO"
  mkdir -p "$dir-bin/examples" || self_failed "setup at line $LINENO"
  printf '[package]\nname = "forms-bin"\nversion = "0.0.0"\nedition = "2021"\npublish = false\nautobins = false\nautoexamples = false\n\n[[bin]]\nname = "forms-bin"\npath = "examples/bin.rs"\n\n[workspace]\n' \
    >"$dir-bin/Cargo.toml" || self_failed "setup at line $LINENO"
  printf '# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\nversion = 4\n\n[[package]]\nname = "forms-bin"\nversion = "0.0.0"\n' \
    >"$dir-bin/Cargo.lock" || self_failed "setup at line $LINENO"
  cp "$dir/examples/bin.rs" "$dir-bin/examples/bin.rs" || self_failed "setup at line $LINENO"
}

forms_table() {
  cat <<'EOF'
an-upper-case-rs-file-by-mod forbid tests/shout.rs 1
a-one-line-cfg-test-item-before-a-function forbid tests/lib.rs 2
include-with-braces forbid tests/inc.txt 1
cfg-attr-path forbid tests/cfgpath.txt 1
a-spaced-path-attribute forbid tests/spaced.txt 1
a-lib-path-under-tests forbid tests/lib.rs 2
a-bin-path-under-examples forbid examples/bin.rs 1 bin
a-comment-that-hides-a-mod-line forbid tests/hidden.rs 1
an-inline-module-with-a-cfg-test-mod-of-the-same-name forbid tests/shell.rs 1
a-non-rs-file-by-path forbid tests/plain.txt 1
a-module-under-cfg-not-test forbid tests/nottest.rs 1
a-safe-function-in-an-unsafe-extern-block forbid tests/lib.rs 20
a-start-under-cfg-not-debug-assertions rule tests/lib.rs 23 debug_assertions
a-start-under-cfg-not-feature rule tests/lib.rs 25 feature
a-start-under-cfg-target-os rule tests/lib.rs 27 target_os
f31-a-macro-variable-cfg-under-not-debug-assertions forbid-release tests/lib.rs 32
f32-a-comment-between-cfg-and-its-predicate forbid-release tests/lib.rs 37
a-start-under-cfg-windows forbid-windows tests/lib.rs 39
EOF
}

lint_forms() {
  dir=$1
  flag=
  case $2 in
    *release) flag=--release ;;
  esac
  dir_abs=$(absolute_dir "$dir") || self_failed "setup at line $LINENO"
  case $2 in
    *bin) targets=--bins ;;
    *) targets='--lib --bins' ;;
  esac
  # shellcheck disable=SC2086
  (cd "$dir" && forbid "$work/forms-$2" --manifest-path Cargo.toml $targets ${flag:+"$flag"})
  printf '%s\n' "$?" >"$work/forms-$2.exit"
  root_abs=$dir_abs
  read_forbid "$work/forms-$2.json" | tr '[:upper:]' '[:lower:]' >"$work/forms-$2-found.txt" || self_failed "setup at line $LINENO"
  (cd "$dir" && find . -type f ! -path './target/*' | sed 's|^\./||' | sort >"$work/forms-$2-files.txt") || self_failed "setup at line $LINENO"
  (cd "$dir" && text_rules "$work/forms-$2-files.txt") >"$work/forms-$2-rules.txt" || self_failed "setup at line $LINENO"
}

check_forms() {
  artifacts=$work/forms-artifacts
  mkdir -p "$artifacts" || self_failed "setup at line $LINENO"
  case $system in
    Darwin) job_system=unix ;;
    *) job_system=windows ;;
  esac
  forbid_table >"$work/forbid-table.txt" || self_failed "setup at line $LINENO"
  write_forbid_list
  forms_table >"$work/forms-table.txt" || self_failed "setup at line $LINENO"
  write_forms "$work/forms" 'std::process::Command::new("true").status().is_ok()' forms
  write_forms "$work/forms-control" 'std::path::Path::new("true").exists()' control
  lint_forms "$work/forms" forms
  lint_forms "$work/forms-control" control
  lint_forms "$work/forms-bin" forms-bin
  lint_forms "$work/forms-control-bin" control-bin
  lint_forms "$work/forms" forms-release
  lint_forms "$work/forms-control" control-release
  while IFS=' ' read -r form how path at word; do
    found=$work/forms-forms-found.txt
    [ "$word" != bin ] || found=$work/forms-forms-bin-found.txt
    [ "$how" != forbid-release ] || found=$work/forms-forms-release-found.txt
    if [ "$how" = forbid-windows ] && [ "$job_system" != windows ]; then
      printf 'case %s/form-%s not compiled in this job: the windows job holds it\n' "$name" "$form"
      continue
    fi
    case $how in
      forbid | forbid-release | forbid-windows) grep -q -E "^finding [^ ]+ $path $at\$" "$found" && got=FAIL || got=PASS ;;
      rule) grep -q -x -F -e "rule cfg $path:$at $word" "$work/forms-forms-rules.txt" && got=FAIL || got=PASS ;;
    esac
    printf 'case %s/form-%s want=FAIL got=%s\n' "$name" "$form" "$got"
    if [ "$got" = FAIL ]; then
      rejected=$((rejected + 1))
    else
      wrong="$wrong form-$form"
    fi
  done <"$work/forms-table.txt"
  got=PASS
  for control in control control-bin control-release; do
    [ -s "$work/forms-$control-found.txt" ] && got=FAIL
    grep -q '^rule ' "$work/forms-$control-rules.txt" && got=FAIL
    read -r control_exit <"$work/forms-$control.exit" || control_exit=x
    [ "$control_exit" = 0 ] || got=FAIL-exit-$control_exit
  done
  printf 'case %s/forms-control want=PASS got=%s\n' "$name" "$got"
  [ "$got" = PASS ] || wrong="$wrong forms-control"
}

odd_repo() {
  write_repo "$1" '' >/dev/null || self_failed "setup at line $LINENO"
  case $2 in
    link)
      blob=$(printf 'gate.txt' | git -C "$1" hash-object -w --stdin) || self_failed "setup at line $LINENO"
      if ! ln -s gate.txt "$1/scripts/r6/egress/link" 2>/dev/null || [ ! -h "$1/scripts/r6/egress/link" ]; then
        printf 'gate.txt' >"$1/scripts/r6/egress/link" || self_failed "setup at line $LINENO"
        git -C "$1" config core.symlinks false || self_failed "setup at line $LINENO"
      fi
      git -C "$1" update-index --add --cacheinfo "120000,$blob,scripts/r6/egress/link" || self_failed "setup at line $LINENO"
      ;;
    gitlink)
      mkdir -p "$1/scripts/r6/egress/sub" || self_failed "setup at line $LINENO"
      git -C "$1" update-index --add --cacheinfo "160000,1111111111111111111111111111111111111111,scripts/r6/egress/sub" ||
        self_failed "setup at line $LINENO"
      ;;
  esac
  git -C "$1" update-index -q --refresh >/dev/null 2>&1
  git -C "$1" write-tree 2>/dev/null || self_failed "setup at line $LINENO"
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || self_failed "setup at line $LINENO"
  case $work in
    /*) ;;
    *) work=$(pwd)/$work ;;
  esac
  mkdir -p "$work/bin" "$work/artifacts" "$work/tmp" || self_failed "setup at line $LINENO"
  real_cargo=$(command -v cargo) || self_failed "no cargo"
  own_repo=${CI_D_REPO:-$(git rev-parse --show-toplevel)} || self_failed "setup at line $LINENO"
  git -C "$own_repo" show HEAD:rust-toolchain.toml >"$work/rust-toolchain.toml" || self_failed "setup at line $LINENO"
  write_stubs
  good_tree=$(write_repo "$work/repo" '') || self_failed "setup at line $LINENO"
  entry_tree=$(write_repo "$work/repo-entry" '127.0.0.1:443') || self_failed "setup at line $LINENO"
  patch_tree=$(write_repo "$work/repo-patch" '' '[patch.crates-io]\n') || self_failed "setup at line $LINENO"
  features_tree=$(write_repo "$work/repo-features" '' '' '[features]\nnet = []\n') || self_failed "setup at line $LINENO"
  link_tree=$(odd_repo "$work/repo-link" link) || self_failed "setup at line $LINENO"
  gitlink_tree=$(odd_repo "$work/repo-gitlink" gitlink) || self_failed "setup at line $LINENO"
  leave_tree=$(write_repo "$work/repo-leave" '') || self_failed "setup at line $LINENO"
  take_tree=$(write_repo "$work/repo-take" '') || self_failed "setup at line $LINENO"
  print_leave_tree=$(write_repo "$work/repo-print-leave" '') || self_failed "setup at line $LINENO"
  print_filter_tree=$(write_repo "$work/repo-print-filter" '') || self_failed "setup at line $LINENO"
  leave_derive_tree=$(write_repo "$work/repo-leave-derive" '') || self_failed "setup at line $LINENO"
  leave_control_tree=$(write_repo "$work/repo-leave-control" '') || self_failed "setup at line $LINENO"
  leave_last_tree=$(write_repo "$work/repo-leave-last" '') || self_failed "setup at line $LINENO"
  for dir in repo-leave repo-take; do
    printf 'x\n' >"$work/$dir/untracked-before.txt" || self_failed "setup at line $LINENO"
  done
  awk '/^run_list\(\) \{/ { skip = 1 } skip && /^EOF$/ { skip = 0 } skip && /^[a-z-]+( |$)/ { next } { print }' "$self" \
    >"$work/norun.sh" || self_failed "setup at line $LINENO"
  awk '/^spend-sim --seed 1$/ { print "spend-sim"; next } { print }' "$self" >"$work/bare.sh" || self_failed "setup at line $LINENO"
  awk '{ print } /^cannot_list\(\) \{$/ { getline; print; print "probe a second entry of the same name" }' "$self" >"$work/twice.sh" ||
    self_failed "setup at line $LINENO"
  cfg_tree=$(write_repo "$work/repo-cfg" '' '' '' crates/halvo-route-gate/src/net.rs \
    '#[cfg(target_os = "linux")]') || self_failed "setup at line $LINENO"
  nested_tree=$(write_repo "$work/repo-nested" '' '' '' crates/halvo-route-gate/src/net.rs \
    '#[cfg_attr(test, cfg(debug_assertions))]') || self_failed "setup at line $LINENO"
  # shellcheck disable=SC2016
  attr_tree=$(write_repo "$work/repo-attr" '' '' '' crates/halvo-route-gate/src/net.rs \
    'macro_rules! m { ($a:ident) => { #[ $a ( x ) ] fn f() {} }; }') || self_failed "setup at line $LINENO"
  # shellcheck disable=SC2016
  clippy_macro_tree=$(write_repo "$work/repo-clippy-macro" '' '' '' crates/halvo-route-gate/src/net.rs \
    'macro_rules! gate { ($test:meta) => { #[cfg($test)] pub fn go() { std::process::Command::new("true"); } }; } gate!(not(clippy));') ||
    self_failed "setup at line $LINENO"
  clippy_comment_tree=$(write_repo "$work/repo-clippy-comment" '' '' '' crates/halvo-route-gate/src/net.rs \
    '#[cfg/**/(not(clippy))] pub fn go() { std::process::Command::new("true"); }') || self_failed "setup at line $LINENO"
  clippy_module_tree=$(write_repo "$work/repo-clippy-module" '' '' '' crates/halvo-route-gate/src/lib.rs \
    '#[cfg/**/(not(clippy))] pub mod hidden;') || self_failed "setup at line $LINENO"
  cfg_bang_tree=$(write_repo "$work/repo-cfg-bang" '' '' '' crates/halvo-route-gate/src/net.rs \
    'pub fn f() -> bool { cfg!(target_os = "linux") }') || self_failed "setup at line $LINENO"
  cfg_spaced_tree=$(write_repo "$work/repo-cfg-spaced" '' '' '' crates/halvo-route-gate/src/net.rs \
    '#[cfg (feature = "x")] pub fn g() {}') || self_failed "setup at line $LINENO"
  # shellcheck disable=SC2016
  attr_path_tree=$(write_repo "$work/repo-attr-path" '' '' '' crates/halvo-route-gate/src/net.rs \
    'macro_rules! m { ($a:ident) => { #[$a::b] fn f() {} }; }') || self_failed "setup at line $LINENO"
  outside_tree=$(write_repo "$work/repo-outside" '' '' '' docs/outside.rs '#[cfg(feature = "x")]') || self_failed "setup at line $LINENO"
  mkdir -p "$work/deps" || self_failed "setup at line $LINENO"
  printf '%s/deps/gate-1.d: crates/halvo-route-gate/src/main.rs docs/outside.rs\n' "$work" >"$work/deps/gate-1.d" ||
    self_failed "setup at line $LINENO"
  printf '{"reason":"compiler-artifact","package_id":"path+file:///x#0.0.0","filenames":["%s/deps/libgate-1.rmeta"],"fresh":true}\n' \
    "$work" >"$work/clippy-dep-info.json" || self_failed "setup at line $LINENO"
  printf '{"reason":"compiler-artifact","package_id":"path+file:///x#0.0.0","filenames":["%s/deps/libgone-1.rmeta"],"fresh":true}\n' \
    "$work" >"$work/clippy-dep-info-gone.json" || self_failed "setup at line $LINENO"
  system=$(uname -s) || self_failed "setup at line $LINENO"
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
  host_job=${R6_EGRESS_SELFTEST_HOST:-$own_job}
  case $host_job in
    macos | windows) ;;
    *) self_failed "R6_EGRESS_SELFTEST_HOST is macos or windows" ;;
  esac
  windows_cases >"$work/windows-cases.txt" || self_failed "setup at line $LINENO"
  macos_cases >"$work/macos-cases.txt" || self_failed "setup at line $LINENO"
  rule_cases >"$work/rule-cases.txt" || self_failed "setup at line $LINENO"
  : >"$work/windows-seen.txt" || self_failed "setup at line $LINENO"
  : >"$work/macos-seen.txt" || self_failed "setup at line $LINENO"
  wrong=
  rejected=0
  while IFS=' ' read -r rule ruled; do
    if ! grep -q -x -F -e "$ruled" "$work/windows-cases.txt"; then
      printf 'case %s/rule-table want=PASS got=rule-%s-has-no-case-on-the-windows-list\n' "$name" "$rule"
      wrong="$wrong rule-table"
    fi
  done <"$work/rule-cases.txt"
  : >"$work/clippy-clean.json" || self_failed "setup at line $LINENO"
  finding_json "$work/clippy-finding.json" clippy::disallowed_methods 'crates\\halvo-route-gate\\src\\net.txt' 2 || self_failed "setup at line $LINENO"
  finding_json "$work/clippy-unsafe.json" unsafe_code 'crates/halvo-route/src/x.rs' 7 || self_failed "setup at line $LINENO"
  finding_json "$work/clippy-outside.json" clippy::disallowed_methods '/elsewhere/x.rs' 3 || self_failed "setup at line $LINENO"
  finding_json "$work/clippy-outside-drive.json" clippy::disallowed_methods 'C:\\elsewhere\\x.rs' 4 || self_failed "setup at line $LINENO"
  # shellcheck disable=SC2016
  printf '%s\n' '{"reason":"compiler-message","message":{"rendered":"warning: `std::nope` does not refer to a reachable type\n","$message_type":"diagnostic","children":[],"code":null,"level":"warning","message":"x","spans":[]}}' \
    >"$work/clippy-refer.json" || self_failed "setup at line $LINENO"
  # shellcheck disable=SC2016
  printf '%s\n' '{"reason":"compiler-message","message":{"rendered":"warning: x\n  --> crates/halvo-route/src/x.rs:3:1\n","$message_type":"diagnostic","children":[{"children":[],"code":{"code":"unsafe_code","explanation":null},"level":"note","message":"n","rendered":null,"spans":[]}],"code":null,"level":"warning","message":"m","spans":[]}}' \
    >"$work/clippy-child-code.json" || self_failed "setup at line $LINENO"
  v_lower_flags=$(printf '%s\n' "$v_flags" | tr '[:upper:]' '[:lower:]') || self_failed "setup at line $LINENO"
  v_lower_wrapper=$(printf '%s\n' "${w_rustc}_WRAPPER" | tr '[:upper:]' '[:lower:]') || self_failed "setup at line $LINENO"
  case_reset
  if case_wanted rule-table-pinned; then
    cp "$work/rule-cases.txt" "$work/rule-names.txt" || self_failed "setup at line $LINENO"
    cat >"$work/rule-pin.txt" <<'EOF' || self_failed "setup at line $LINENO"
forbid-reader forbid-finding
forbid-release forbid-finding-in-release
forbid-outside-path forbid-finding-outside-the-repository-on-a-drive
canary-own-entries canary-requires-a-windows-entry-in-the-windows-job
canary-refers canary-list-entry-refers-to-nothing
canary-real-cargo canary-on-the-real-cargo
graph-tree graph-path-package-moved
graph-metadata graph-features-in-metadata
graph-tables graph-patch-table
refusal-stops-the-runs runs-not-made-after-a-text-rule
canary-refusal canary-refusal-ends-as-a-hold
list-from-memory run-plants-a-replace-ref-for-the-list
tree-after-builds build-changes-a-tracked-file
git-configuration run-changes-git-configuration
rule-cfg runs-not-made-after-a-text-rule
rule-attribute rule-attribute-path-from-a-macro-variable
rule-clippy rule-clippy-behind-a-comment
rule-dep-info rule-file-named-by-dep-info
env-rules env-lowercase-flags
entries list-file-with-one-entry
entries-link list-directory-holds-a-link
entries-submodule list-directory-holds-a-submodule
compare-lists word-on-both-lists
control control-let-through
new-log log-of-the-run-before
log-link log-that-is-a-link
list-in-force run-widens-the-list-and-a-later-run-restores-it
state-in-memory-record run-rewrites-the-record-of-found-commands
state-in-memory-run-list run-cuts-the-run-list
checks-equal-found run-without-its-line-names-the-missing-run
wrapped-verdict command-exits-1
run-arguments entry-with-no-arguments
tree-check tracked-file-changed-by-a-run
cause env-refused-then-a-stale-build
signal-trap signal-stops-the-listeners
windows-job pass
windows-imports import-listing-printed
printing-names printed-command-on-neither-list
printing-every-line printed-name-that-the-list-rule-skips
printing-entries entry-that-is-no-command
printing-exit printing-that-does-not-exit-0
printing-wrapped printing-leaves-a-file-in-the-checkout
printing-state-first printing-rewrites-a-file-behind-a-clean-filter
tree-new-path import-leaves-a-file-in-the-checkout
checks-count job-with-no-run
windows-sentence sentence-on-a-hosted-runner
forbid-exit-status forbid-exit-without-a-finding
forbid-refers forbid-list-entry-refers-to-nothing
env-wrapper env-wrapper-set
env-argument-file env-flags-argument-file
graph-unknown graph-third-party-package-added
graph-build-time graph-build-time-package-added
graph-build-script graph-build-script
graph-proc-macro graph-proc-macro
wrapped-missing-line no-egress-line
wrapped-refused-mode refusal-mode
log-count log-with-another-count
wrapped-exit-0-with-a-denial check-exits-0-with-a-denial
wrapped-exit-1-with-a-denial command-exits-1-with-a-denial
denials-summed denied-in-each-run
stale-build stale-build
usage usage
no-tree no-tree
tree-before tracked-file-changed-before
EOF
    if cmp -s "$work/rule-names.txt" "$work/rule-pin.txt"; then
      printf 'case %s/rule-table-pinned want=PASS got=PASS\n' "$name"
    else
      printf 'case %s/rule-table-pinned want=PASS got=rows-differ-from-the-pin\n' "$name"
      wrong="$wrong rule-table-pinned"
    fi
  fi
  if case_wanted forbid-table-pinned; then
    forbid_table | sed 's/|.*//' >"$work/table-rows.txt" || self_failed "setup at line $LINENO"
    cat >"$work/table-pin.txt" <<'EOF' || self_failed "setup at line $LINENO"
type all std::process::Command
type all std::process::Child
type all std::process::Stdio
type all std::process::ChildStdin
type all std::process::ChildStdout
type all std::process::ChildStderr
type all std::process::Output
type all std::net::TcpStream
type all std::net::TcpListener
type all std::net::UdpSocket
type unix std::os::unix::net::UnixStream
type unix std::os::unix::net::UnixListener
type unix std::os::unix::net::UnixDatagram
method all std::process::Command::new
method all std::process::Command::spawn
method all std::process::Command::output
method all std::process::Command::status
method unix std::os::unix::process::CommandExt::exec
method all std::net::TcpStream::connect
method all std::net::TcpStream::connect_timeout
method all std::net::TcpListener::bind
method all std::net::UdpSocket::bind
method all std::net::ToSocketAddrs::to_socket_addrs
method unix std::os::unix::net::UnixStream::connect
method unix std::os::unix::net::UnixStream::connect_addr
method unix std::os::unix::net::UnixStream::pair
method unix std::os::unix::net::UnixListener::bind
method unix std::os::unix::net::UnixListener::bind_addr
method unix std::os::unix::net::UnixDatagram::bind
method unix std::os::unix::net::UnixDatagram::bind_addr
method unix std::os::unix::net::UnixDatagram::pair
method unix std::os::unix::net::UnixDatagram::connect
method unix std::os::unix::net::UnixDatagram::connect_addr
method unix std::os::fd::FromRawFd::from_raw_fd
method windows std::os::windows::io::FromRawSocket::from_raw_socket
method windows std::os::windows::io::FromRawHandle::from_raw_handle
EOF
    if cmp -s "$work/table-rows.txt" "$work/table-pin.txt"; then
      printf 'case %s/forbid-table-pinned want=PASS got=PASS\n' "$name"
    else
      printf 'case %s/forbid-table-pinned want=PASS got=rows-differ-from-the-pin\n' "$name"
      wrong="$wrong forbid-table-pinned"
    fi
  fi

  case_run pass PASS - "egress checks=8 denied=0 entries=0${nl}denied=0 proves that no process"
  part_seconds
  case_sysroot=asked
  case_run import-listing-printed PASS - 'egress imports binary=dev reader='
  case_run sentence PASS - "while the command ran on $p; not verified: path-1-$p, path-2-$p, and what a process did after the command ended.${nl}A connection that the mode stops"
  case_actions=true
  case_run sentence-on-a-hosted-runner PASS - "while the command ran on $hosted; not verified: path-1-$hosted, path-2-$hosted, and what"
  case_run commands PASS - 'egress commands=8 run=8 cannot=0 crates=3'
  case_run third-party PASS - 'egress limit third-party crates=6 names=itoa,memchr,serde,serde_core,serde_json,zmij'
  case_run forbid-clean PASS - 'egress forbid profile=dev findings=0 exit=0'
  case_run control-denied PASS - 'egress control denied=1 log=target/egress/'
  STUB_CONTROL=through
  case_run control-let-through FAIL check 'egress control: the egress check let the control through'
  STUB_CONTROL=lost
  case_run control-report-lost-four-times FAIL check 'egress control: the probe was stopped in 4 tries and no report of a denial arrived'
  STUB_CONTROL=port
  case_run control-port-in-use FAIL check 'egress control refused=port 127.0.0.1:47201'
  STUB_CHECK=reuse-log
  case_run log-of-another-run FAIL check 'log=not-new-or-not-its-own'
  STUB_CHECK=short-log
  case_run log-with-another-count FAIL check 'log=not-new-or-not-its-own'
  STUB_HELD_PORT=47202
  case_job=macos
  case_run port-47202-held-by-another-process PASS - 'egress checks=8 denied=0 entries=0' 'egress asking'
  STUB_COMMANDS="$STUB_COMMANDS export"
  case_run printed-command-on-neither-list FAIL check 'egress on-neither-list=export'
  STUB_PRINT_HIDDEN=1
  case_run printed-name-that-the-list-rule-skips FAIL check "egress on-neither-list=--found--${nl}egress on-neither-list= (an empty name, printed line 10)${nl}egress commands=10 is not run=8 plus cannot=0"
  STUB_PRINT_EXIT=2
  case_run printing-that-does-not-exit-0 FAIL check 'egress printing exit=2 (route-gate commands printed no names)' 'egress run='
  case_repo=$work/repo-print-leave
  case_tree=$print_leave_tree
  STUB_LEAVE=commands
  case_run printing-leaves-a-file-in-the-checkout FAIL check 'egress printing: a wrapped run left a new path: left-behind.txt'
  case_repo=$work/repo-print-filter
  case_tree=$print_filter_tree
  STUB_PRINT_FILTER=1
  case_run printing-rewrites-a-file-behind-a-clean-filter FAIL check "tree changed=after M docs/r2/trials/egress.md${nl}egress printing: a wrapped run changed git's configuration"
  case_repo=$work/repo-leave
  case_tree=$leave_tree
  STUB_LEAVE=import
  case_run import-leaves-a-file-in-the-checkout FAIL check 'egress run=import: a wrapped run left a new path: left-behind.txt'
  case_repo=$work/repo-leave
  case_tree=$leave_tree
  case_run untracked-path-before-the-run PASS - 'egress checks=8 denied=0 entries=0' 'a wrapped run left a new path'
  case_repo=$work/repo-take
  case_tree=$take_tree
  STUB_TAKE=import
  case_run import-takes-away-an-untracked-path FAIL check 'egress run=import: a wrapped run took away a path: untracked-before.txt'
  case_repo=$work/repo-leave-derive
  case_tree=$leave_derive_tree
  STUB_LEAVE=derive
  case_run derive-leaves-a-file-in-the-checkout FAIL check 'egress run=derive: a wrapped run left a new path: left-behind.txt'
  case_repo=$work/repo-leave-control
  case_tree=$leave_control_tree
  STUB_CONTROL_LEAVE=1
  case_run control-leaves-a-file-in-the-checkout FAIL check 'egress control: a wrapped run left a new path: left-behind.txt'
  case_repo=$work/repo-leave-last
  case_tree=$leave_last_tree
  STUB_LEAVE=commands
  STUB_LEAVE_AT=2
  case_run last-entry-leaves-a-file-in-the-checkout FAIL check 'egress run=commands: a wrapped run left a new path: left-behind.txt'
  case_run graph-closed PASS - 'egress graph paths=3 linked=6 build=5 holds=yes'
  case_run rules-allowed-forms PASS - 'egress rules files='
  case_env="$v_flags=-Dwarnings"
  case_run env-deny-flag PASS - 'egress rules files='
  case_env="$v_flags=-Aclippy::disallowed_types"
  case_run env-allow-flag FAIL check "rule env $v_flags holds an allow, a cap on lints or a cfg"
  case_env="$v_encoded=-Dwarnings$(printf '\037')-Aunsafe_code"
  case_run env-encoded-allow FAIL check "rule env $v_encoded holds"
  case_env="${w_cargo}_TARGET_X86_64_UNKNOWN_LINUX_GNU_$v_flags=--cfg=nonet"
  case_run env-per-target-cfg FAIL check "rule env ${w_cargo}_TARGET_X86_64_UNKNOWN_LINUX_GNU_$v_flags holds"
  case_env="$v_clippy=--cap-lints=allow"
  case_run env-clippy-arguments FAIL check "rule env $v_clippy holds"
  case_env="${w_rustc}_WORKSPACE_WRAPPER=/x"
  case_run env-wrapper-set FAIL check "rule env ${w_rustc}_WORKSPACE_WRAPPER is set"
  case_env="${w_rustc}_WRAPPER=/x"
  case_run env-plain-wrapper-set FAIL check "rule env ${w_rustc}_WRAPPER is set"
  case_env="$v_build=-Aunsafe_code"
  case_run env-build-flags-allow FAIL check "rule env $v_build holds"
  case_env="$v_flags=@args.txt"
  case_run env-flags-argument-file FAIL check "rule env $v_flags holds"
  case_env="$v_lower_flags=-Aunsafe_code"
  case_run env-lowercase-flags FAIL check "rule env $v_lower_flags holds"
  case_env="$v_lower_wrapper=/x"
  case_run env-lowercase-wrapper FAIL check "rule env $v_lower_wrapper is set"
  case_env="$v_flags=--allow=unsafe_code"
  case_run env-long-allow-flag FAIL check "rule env $v_flags holds"
  env_case env-rules-reads-a-lower-case-flags-name FAIL "rule env $v_lower_flags holds an allow, a cap on lints or a cfg" '' \
    "$v_lower_flags=-Aunsafe_code"
  env_case env-rules-reads-a-mixed-case-flags-name FAIL "rule env ${w_rust}Flags holds an allow, a cap on lints or a cfg" '' \
    "${w_rust}Flags=-Aunsafe_code"
  env_case env-rules-reads-a-lower-case-wrapper-name FAIL "rule env $v_lower_wrapper is set" '' "$v_lower_wrapper=/x"
  env_case env-rules-reads-a-mixed-case-wrapper-name FAIL "rule env ${w_rustc}_Wrapper is set" '' "${w_rustc}_Wrapper=/x"
  env_case env-rules-skips-a-flags-name-with-one-more-sign PASS 'holds=yes' "rule env ${v_flags}X" "${v_flags}X=-Aunsafe_code"
  env_case env-rules-skips-a-wrapper-name-with-one-more-sign PASS 'holds=yes' "rule env ${w_rustc}_WRAPPERX" "${w_rustc}_WRAPPERX=/x"
  env_case env-rules-skips-a-wrapper-name-with-one-sign-less PASS 'holds=yes' "rule env ${w_rustc}_WRAPPE" "${w_rustc}_WRAPPE=/x"
  case_repo=$work/repo-cfg
  case_tree=$cfg_tree
  case_run rule-cfg-target-os FAIL check 'rule cfg crates/halvo-route-gate/src/net.rs:1 target_os'
  case_repo=$work/repo-cfg
  case_tree=$cfg_tree
  STUB_MARKER=$work/marker-text-rule
  case_run runs-not-made-after-a-text-rule FAIL check "egress runs=not-made (a hold refused)${nl}FAIL 60-r6-egress (check)" 'egress checks='
  marker_absent runs-not-made-after-a-text-rule "$work/marker-text-rule"
  STUB_CLIPPY_JSON=$work/clippy-finding.json
  STUB_CLIPPY_EXIT=101
  STUB_MARKER=$work/marker-forbid
  case_run runs-not-made-after-a-forbid-finding FAIL check "egress runs=not-made (a hold refused)${nl}FAIL 60-r6-egress (check)" 'egress checks='
  marker_absent runs-not-made-after-a-forbid-finding "$work/marker-forbid"
  STUB_GRAPH=extra-third
  STUB_MARKER=$work/marker-graph
  case_run runs-not-made-after-a-graph-refusal FAIL check "egress forbid=not-made (the graph refused, so no cargo call compiles the three crates)" 'egress checks='
  marker_absent runs-not-made-after-a-graph-refusal "$work/marker-graph"
  case_repo=$work/repo-nested
  case_tree=$nested_tree
  case_run rule-cfg-nested FAIL check 'rule cfg crates/halvo-route-gate/src/net.rs:1 debug_assertions'
  case_repo=$work/repo-attr
  case_tree=$attr_tree
  case_run rule-attribute-from-a-macro-variable FAIL check 'rule attribute crates/halvo-route-gate/src/net.rs:1'
  case_repo=$work/repo-attr-path
  case_tree=$attr_path_tree
  case_run rule-attribute-path-from-a-macro-variable FAIL check 'rule attribute crates/halvo-route-gate/src/net.rs:1'
  case_repo=$work/repo-cfg-bang
  case_tree=$cfg_bang_tree
  case_run rule-cfg-macro FAIL check 'rule cfg crates/halvo-route-gate/src/net.rs:1 target_os'
  case_repo=$work/repo-cfg-spaced
  case_tree=$cfg_spaced_tree
  case_run rule-cfg-spaced FAIL check 'rule cfg crates/halvo-route-gate/src/net.rs:1 feature'
  case_repo=$work/repo-clippy-macro
  case_tree=$clippy_macro_tree
  case_run rule-clippy-through-a-macro-variable FAIL check 'rule clippy crates/halvo-route-gate/src/net.rs:1'
  case_repo=$work/repo-clippy-comment
  case_tree=$clippy_comment_tree
  case_run rule-clippy-behind-a-comment FAIL check 'rule clippy crates/halvo-route-gate/src/net.rs:1'
  case_repo=$work/repo-clippy-module
  case_tree=$clippy_module_tree
  case_run rule-clippy-on-a-hidden-module FAIL check 'rule clippy crates/halvo-route-gate/src/lib.rs:1'
  case_repo=$work/repo-outside
  case_tree=$outside_tree
  case_run rule-outside-the-crates-not-named PASS - 'egress rules files='
  STUB_CLIPPY_JSON=$work/clippy-dep-info.json
  case_repo=$work/repo-outside
  case_tree=$outside_tree
  case_run rule-file-named-by-dep-info FAIL check 'rule cfg docs/outside.rs:1 feature'
  STUB_CLIPPY_JSON=$work/clippy-dep-info-gone.json
  case_run rule-dep-info-missing FAIL check 'rule unread gone-1.d'
  STUB_GRAPH=extra-third
  case_run graph-third-party-package-added FAIL check 'egress graph unknown=libc'
  STUB_GRAPH=extra-build
  case_run graph-build-time-package-added FAIL check 'egress graph unknown=thiserror-impl'
  STUB_GRAPH=linked-syn
  case_run graph-build-time-package-linked FAIL check 'egress graph unknown=syn'
  STUB_GRAPH=moved
  case_run graph-path-package-moved FAIL check 'egress graph unknown=halvo-route'
  STUB_GRAPH=extra-path
  case_run graph-path-package-added FAIL check 'egress graph unknown=halvo-cap'
  STUB_GRAPH=missing
  case_run graph-package-gone FAIL check 'egress graph missing=zmij'
  STUB_GRAPH=build-target
  case_run graph-build-script FAIL check 'egress graph target=halvo-route kind=custom-build'
  STUB_GRAPH=proc-macro-target
  case_run graph-proc-macro FAIL check 'egress graph target=halvo-route-wire kind=proc-macro'
  STUB_GRAPH=features
  case_run graph-features-in-metadata FAIL check 'egress graph features=halvo-route-gate'
  case_repo=$work/repo-features
  case_tree=$features_tree
  case_run graph-features-table FAIL check 'egress graph features=halvo-route-gate'
  case_repo=$work/repo-patch
  case_tree=$patch_tree
  case_run graph-patch-table FAIL check 'egress graph patch=Cargo.toml'
  STUB_GRAPH=missing
  case_run shared-stand-ins-first-input FAIL check 'egress graph missing=zmij' 'egress graph unknown=libc'
  STUB_GRAPH=extra-third
  case_run shared-stand-ins-second-input FAIL check 'egress graph unknown=libc' 'egress graph missing=zmij'
  STUB_CANARY=real
  case_job=$own_job
  case_run canary-on-the-real-cargo PASS - 'egress canary profile=dev entries='
  STUB_CLIPPY_RELEASE_JSON=$work/clippy-finding.json
  STUB_CLIPPY_RELEASE_EXIT=101
  case_run forbid-finding-in-release FAIL check "forbid release crates/halvo-route-gate/src/net.txt:2 clippy::disallowed_methods${nl}egress forbid profile=release findings=1 exit=101"
  STUB_CANARY_DROP=t01
  STUB_CANARY_DROP_PROFILE=release
  case_run canary-let-through-in-release FAIL check 'egress canary profile=release missing=clippy::disallowed_types:1'
  STUB_CANARY_DROP=t01
  case_run canary-refusal-ends-as-a-hold FAIL check "egress runs=not-made (a hold refused)${nl}FAIL 60-r6-egress (check)"
  STUB_CANARY_DROP=t01
  STUB_CANARY_DROP_PROFILE=release
  case_run canary-refusal-in-release-ends-as-a-hold FAIL check "egress runs=not-made (a hold refused)${nl}FAIL 60-r6-egress (check)"
  STUB_CANARY_DROP=u01
  case_run canary-without-its-unsafe-finding FAIL check 'egress canary profile=dev missing=unsafe_code:'
  STUB_CANARY_DROP=t01
  case_run canary-let-through FAIL check "egress canary profile=dev missing=clippy::disallowed_types:1${nl}egress canary profile=dev entries="
  case_run canary-entries-of-the-macos-job PASS - 'egress canary profile=dev entries=35 found=35 exit=101'
  STUB_CANARY_DROP=t11
  case_run canary-requires-a-unix-entry-in-the-macos-job FAIL check 'egress canary profile=dev missing=clippy::disallowed_types:12'
  case_job=windows
  case_run canary-entries-of-the-windows-job PASS - 'egress canary profile=dev entries=22 found=22 exit=101'
  case_job=windows
  STUB_CANARY_DROP=m22
  case_run canary-requires-a-windows-entry-in-the-windows-job FAIL check 'egress canary profile=dev missing=clippy::disallowed_methods:51'
  canary_case canary-match-splits-a-place-with-a-space PASS 'egress canary profile=dev entries=1 found=1 exit=101' 'missing=' \
    "clippy::disallowed_types 1 2$nl" "finding clippy::disallowed_types src/lib.rs 1 2$nl"
  canary_case canary-match-strips-leading-spaces PASS 'egress canary profile=dev entries=1 found=1 exit=101' 'missing=' \
    "  unsafe_code 1$nl" "finding unsafe_code src/lib.rs 1$nl"
  canary_case canary-match-reads-the-whole-line FAIL 'egress canary profile=dev missing=clippy::disallowed_types:1' 'found=1' \
    "clippy::disallowed_types 1$nl" "finding clippy::disallowed_types src/lib.rs 12$nl"
  canary_case canary-match-reads-text-not-a-pattern FAIL 'egress canary profile=dev missing=clippy::disallowed.types[1]:3' 'found=1' \
    "clippy::disallowed.types[1] 3$nl" "finding clippy::disallowedXtypes1 src/lib.rs 3$nl"
  canary_case canary-match-refer-line FAIL "egress canary profile=dev entries=1 found=1 exit=101${nl}egress canary: the forbid run let the canary through" '' \
    "unsafe_code 1$nl" "finding unsafe_code src/lib.rs 1${nl}refer$nl"
  canary_case canary-match-no-entry FAIL "egress canary profile=dev entries=0 found=0 exit=101${nl}egress canary: the forbid run let the canary through" '' \
    '' "finding unsafe_code src/lib.rs 1$nl"
  canary_case canary-match-more-than-five-missing FAIL "missing=unsafe_code:5${nl}egress canary profile=dev entries=7 found=0 exit=101" 'missing=unsafe_code:6' \
    "unsafe_code 1${nl}unsafe_code 2${nl}unsafe_code 3${nl}unsafe_code 4${nl}unsafe_code 5${nl}unsafe_code 6${nl}unsafe_code 7$nl" "other$nl"
  case_job=windows
  case_run windows-job-runs-every-command PASS - 'egress checks=8 denied=0 entries=0'
  case_job=windows
  case_self=$work/norun.sh
  STUB_COMMANDS=
  case_run job-with-no-run FAIL check 'egress checks=0: no command of the first list ran' 'egress no-command=' 'egress on-neither-list='
  case_job=windows
  case_run forbid-list-of-the-windows-job PASS - 'egress forbid profile=dev findings=0 exit=0'
  STUB_CLIPPY_JSON=$work/clippy-finding.json
  STUB_CLIPPY_EXIT=101
  case_run forbid-finding FAIL check "forbid dev crates/halvo-route-gate/src/net.txt:2 clippy::disallowed_methods${nl}egress forbid profile=dev findings=1 exit=101"
  STUB_CLIPPY_JSON=$work/clippy-outside.json
  STUB_CLIPPY_EXIT=101
  case_run forbid-finding-outside-the-repository FAIL check 'forbid dev outside-the-repository:3 clippy::disallowed_methods'
  STUB_CLIPPY_JSON=$work/clippy-outside-drive.json
  STUB_CLIPPY_EXIT=101
  case_run forbid-finding-outside-the-repository-on-a-drive FAIL check 'forbid dev outside-the-repository:4 clippy::disallowed_methods'
  STUB_CLIPPY_JSON=$work/clippy-unsafe.json
  case_run forbid-finding-with-exit-0 FAIL check 'forbid dev crates/halvo-route/src/x.rs:7 unsafe_code'
  STUB_CLIPPY_EXIT=101
  case_run forbid-exit-without-a-finding FAIL check 'egress forbid profile=dev findings=0 exit=101'
  STUB_CLIPPY_JSON=$work/clippy-child-code.json
  case_run forbid-child-code-is-no-finding PASS - 'egress forbid profile=dev findings=0 exit=0'
  STUB_CLIPPY_JSON=$work/clippy-refer.json
  case_run forbid-list-entry-refers-to-nothing FAIL check 'egress forbid list-entry=refers-to-nothing'
  case_repo=$work/repo-entry
  case_tree=$entry_tree
  case_run list-file-with-one-entry FAIL entries 'egress checks=8 denied=0 entries=1'
  case_self=$work/bare.sh
  case_run entry-with-no-arguments PASS - 'egress run=spend-sim mode=sandbox command=0 denied=0 exit=0'
  case_self=$work/twice.sh
  case_run word-on-both-lists FAIL check 'egress listed-twice=probe'
  case_repo=$work/no-such-repository
  case_run repository-missing FAIL setup 'egress setup repository=unreadable'
  STUB_CHECK='wrong-list'
  case_run line-of-another-list FAIL check 'egress run=probe line=missing'
  case_repo=$work/repo-link
  case_tree=$link_tree
  case_run list-directory-holds-a-link FAIL entries 'egress checks=8 denied=0 entries=1'
  case_repo=$work/repo-gitlink
  case_tree=$gitlink_tree
  case_run list-directory-holds-a-submodule FAIL entries 'egress checks=8 denied=0 entries=1'
  STUB_CANARY=refer
  case_run canary-list-entry-refers-to-nothing FAIL check 'egress canary: the forbid run let the canary through'
  STUB_CHECK=reuse-log-run
  case_run log-of-another-run-of-the-gate-tool FAIL check 'egress run=generate log=not-new-or-not-its-own'
  STUB_CHECK=rewrite-record
  case_run run-rewrites-the-record-of-found-commands PASS - 'egress checks=8 denied=0 entries=0'
  STUB_CHECK=cut-run-list
  case_run run-cuts-the-run-list PASS - 'egress checks=8 denied=0 entries=0'
  STUB_CHECK=widen
  case_run run-widens-the-list-and-a-later-run-restores-it FAIL check 'egress run=generate: a wrapped run changed the list'
  STUB_CHECK=change-copy
  case_run run-changes-its-copy-of-the-list FAIL check 'egress run=generate: a wrapped run changed the list'
  STUB_CHECK=reuse-previous
  case_run log-of-the-run-before FAIL check 'egress run=generate log=not-new-or-not-its-own'
  STUB_COMMANDS='generate import derive score self-test spend-sim commands'
  case_run entry-that-is-no-command FAIL check 'egress no-command=probe'
  STUB_CHECK=denied
  case_run denied-in-each-run FAIL denied 'egress checks=8 denied=9 entries=0' 'denied=0 proves'
  STUB_CHECK=denied0
  case_run check-exits-0-with-a-denial FAIL check 'egress run=probe mode=sandbox command=0 denied=1 exit=0'
  STUB_CHECK=denied
  STUB_RUN_EXIT=1
  case_run command-exits-1-with-a-denial FAIL check 'egress run=probe mode=sandbox command=1 denied=1 exit=1'
  STUB_CHECK=mode
  case_run refusal-mode FAIL check 'NOT VERIFIED egress requirement=W2'
  STUB_RUN_EXIT=1
  case_run command-exits-1 FAIL check 'egress run=probe mode=sandbox command=1 denied=0 exit=1' 'denied=0 proves'
  STUB_CHECK=exit3
  case_run check-exits-3-with-a-clean-line FAIL check 'egress run=probe mode=sandbox command=0 denied=0 exit=3'
  STUB_FRESH=false
  case_run stale-build FAIL setup 'FAIL'
  STUB_FRESH=false
  case_env="${w_rustc}_WORKSPACE_WRAPPER=/x"
  case_run env-refused-then-a-stale-build FAIL check "rule env ${w_rustc}_WORKSPACE_WRAPPER is set"
  STUB_FRESH=false
  STUB_CLIPPY_JSON=$work/clippy-finding.json
  STUB_CLIPPY_EXIT=101
  case_run forbid-finding-then-a-stale-build FAIL check 'forbid dev crates/halvo-route-gate/src/net.txt:2'
  STUB_CHECK=noline
  case_run no-egress-line FAIL check 'egress run=probe line=missing exit=0'
  : >"$work/spawn.pids"
  STUB_CHECK=signal
  case_run signal-stops-the-listeners FAIL signal 'egress stopped by a signal'
  if case_wanted signal-stops-the-listeners; then
    while IFS= read -r spawned; do
      if kill -0 "$spawned" 2>/dev/null; then
        printf 'case %s/signal-stops-the-listeners want=PASS got=listener-alive\n' "$name"
        wrong="$wrong listener-alive"
      fi
    done <"$work/spawn.pids"
  fi
  STUB_CHECK=noline-derive
  case_run run-without-its-line-names-the-missing-run FAIL check "egress run-missing=derive" 'egress checks='
  STUB_CHECK=noline-control
  case_run control-without-an-egress-line FAIL check 'egress control line=missing exit=1'
  if [ "$own_job" = macos ] && [ "$host_job" = macos ]; then
    STUB_CHECK=link-log
    case_run log-that-is-a-link FAIL check 'egress control log=not-new-or-not-its-own'
  elif case_wanted log-that-is-a-link; then
    printf '%s\n' log-that-is-a-link >>"$work/windows-seen.txt"
    printf 'case %s/log-that-is-a-link not-run: Git for Windows makes a copy for ln -s, so the stub cannot make a link here\n' "$name"
  fi
  STUB_CHECK=replace-list
  STUB_MARKER=$work/marker-replace
  case_run run-plants-a-replace-ref-for-the-list PASS - 'egress checks=8 denied=0 entries=0'
  marker_absent run-plants-a-replace-ref-for-the-list "$work/marker-replace.widened"
  listed=$(git -C "$work/repo" rev-parse "$good_tree:scripts/r6/egress/gate.txt") || self_failed "setup at line $LINENO"
  git -C "$work/repo" replace -d "$listed" >/dev/null 2>&1
  STUB_CHECK=symlink-tree
  STUB_MARKER=$work/marker-link
  case_run run-swaps-a-tracked-file-for-a-link FAIL check 'tree changed=after kind l'
  git -C "$work/repo" checkout -q -- docs/r2/trials/egress.md 2>/dev/null || self_failed "setup at line $LINENO"
  STUB_CHECK=exec-bit
  case_run run-sets-an-exec-bit FAIL check 'tree changed=after kind -x'
  chmod -x "$work/repo/docs/r2/trials/egress.md" || self_failed "setup at line $LINENO"
  STUB_CHECK=git-config
  case_run run-changes-git-configuration FAIL check "egress run=generate: a wrapped run changed git's configuration"
  git -C "$work/repo" config --local --unset r6.egress.planted >/dev/null 2>&1
  if [ "$host_job" = macos ] && case_wanted run-writes-info-attributes; then
    attributes_tree=$(write_repo "$work/repo-attributes" '') || self_failed "setup at line $LINENO"
    case_repo=$work/repo-attributes
    case_tree=$attributes_tree
  fi
  STUB_CHECK=attributes
  case_run run-writes-info-attributes FAIL check "egress run=generate: a wrapped run changed git's configuration"
  STUB_CHECK=in-place
  case_run run-rewrites-a-tracked-file-in-place FAIL check 'tree changed=after M docs/r2/trials/egress.md' 'tree changed=after kind'
  git -C "$work/repo" checkout -q -- docs/r2/trials/egress.md 2>/dev/null || self_failed "setup at line $LINENO"
  STUB_BUILD_TOUCH=$work/repo/docs/r2/trials/egress.md
  case_run build-changes-a-tracked-file FAIL check 'tree changed=before-calls M docs/r2/trials/egress.md'
  git -C "$work/repo" checkout -q -- docs/r2/trials/egress.md 2>/dev/null || self_failed "setup at line $LINENO"
  STUB_TOUCH=$work/repo/docs/r2/trials/egress.md
  case_run tracked-file-changed-by-a-run FAIL check 'tree changed=after M docs/r2/trials/egress.md'
  case_run tracked-file-changed-before FAIL setup 'tree changed=before M docs/r2/trials/egress.md'
  git -C "$work/repo" checkout -q -- docs/r2/trials/egress.md 2>/dev/null || self_failed "setup at line $LINENO"
  case_tree=
  case_run no-tree FAIL setup 'FAIL'
  case_arg=--bogus
  case_run usage FAIL usage 'FAIL'

  if [ "$host_job" != macos ]; then
    for nested in case-in-neither-list skipped-names-differ-from-the-macos-list rule-with-no-case-on-the-windows-list cases-filter-matches-nothing; do
      if case_wanted "$nested"; then
        printf 'case %s/%s not-run: a nested self-test; the macos job'"'"'s self-test runs it\n' "$name" "$nested"
      fi
    done
  fi
  if [ "$host_job" = macos ] && case_wanted case-in-neither-list; then
    awk '/^macos_cases\(\) \{/ { in_list = 1 } in_list && /^EOF$/ { in_list = 0 } in_list && $0 == "forbid-clean" { next } { print }' \
      "$self" >"$work/neither.sh" || self_failed "setup at line $LINENO"
    R6_EGRESS_CASES=forbid-clean sh "$work/neither.sh" --self-test >"$work/neither.txt" 2>&1 </dev/null
    code=$?
    if [ "$code" = 1 ] && grep -q -F -e 'forbid-clean want=PASS got=in-neither-list' "$work/neither.txt"; then
      printf 'case %s/case-in-neither-list want=FAIL got=FAIL\n' "$name"
      rejected=$((rejected + 1))
    else
      printf 'case %s/case-in-neither-list want=FAIL got=exit-%s\n' "$name" "$code"
      wrong="$wrong case-in-neither-list"
    fi
  fi
  if [ "$host_job" = macos ] && case_wanted skipped-names-differ-from-the-macos-list; then
    awk '{ print } /^macos_cases\(\) \{/ { getline; print; print "macos-case-with-no-run" }' \
      "$self" >"$work/extra-macos.sh" || self_failed "setup at line $LINENO"
    R6_EGRESS_SELFTEST_HOST=windows R6_EGRESS_CASES='macos-case-with-no-run commands' \
      sh "$work/extra-macos.sh" --self-test >"$work/extra-macos.txt" 2>&1 </dev/null
    code=$?
    if [ "$code" = 1 ] && grep -q -F -e 'commands not-run:' "$work/extra-macos.txt" &&
      grep -q -F -e 'macos-case-with-no-run want=PASS got=on-the-macos-list-and-neither-run-nor-skipped' "$work/extra-macos.txt"; then
      printf 'case %s/skipped-names-differ-from-the-macos-list want=FAIL got=FAIL\n' "$name"
      rejected=$((rejected + 1))
    else
      printf 'case %s/skipped-names-differ-from-the-macos-list want=FAIL got=exit-%s\n' "$name" "$code"
      wrong="$wrong skipped-names-differ-from-the-macos-list"
    fi
  fi
  if [ "$host_job" = macos ] && case_wanted rule-with-no-case-on-the-windows-list; then
    awk '/^rule_cases\(\) \{/ { in_table = 1 } in_table && $1 == "entries" { print "entries list-file-with-no-run-on-windows"; next } /^EOF$/ { in_table = 0 } { print }' \
      "$self" >"$work/rule-table.sh" || self_failed "setup at line $LINENO"
    R6_EGRESS_CASES=forbid-clean sh "$work/rule-table.sh" --self-test >"$work/rule-table.txt" 2>&1 </dev/null
    code=$?
    if [ "$code" = 1 ] && grep -q -F -e 'rule-table want=PASS got=rule-entries-has-no-case-on-the-windows-list' "$work/rule-table.txt" &&
      grep -q -F -e 'FAIL self-test:60-r6-egress (wrong: rule-table)' "$work/rule-table.txt"; then
      printf 'case %s/rule-with-no-case-on-the-windows-list want=FAIL got=FAIL\n' "$name"
      rejected=$((rejected + 1))
    else
      printf 'case %s/rule-with-no-case-on-the-windows-list want=FAIL got=exit-%s\n' "$name" "$code"
      wrong="$wrong rule-with-no-case-on-the-windows-list"
    fi
  fi
  if [ "$host_job" = macos ] && case_wanted cases-filter-matches-nothing; then
    R6_EGRESS_CASES=no-such-case sh "$self" --self-test >"$work/no-case.txt" 2>&1 </dev/null
    code=$?
    if [ "$code" = 1 ] && grep -q -F -e 'R6_EGRESS_CASES matched no case' "$work/no-case.txt"; then
      printf 'case %s/cases-filter-matches-nothing want=FAIL got=FAIL\n' "$name"
      rejected=$((rejected + 1))
    else
      printf 'case %s/cases-filter-matches-nothing want=FAIL got=exit-%s\n' "$name" "$code"
      wrong="$wrong cases-filter-matches-nothing"
    fi
  fi
  if case_wanted help; then
    help_sentences
  fi
  self_test_end
}

help_sentences() {
  sh "$self" --help >"$work/help.txt" 2>&1 || wrong="$wrong help-exit"
  tr '\n' ' ' <"$work/help.txt" | tr -s ' ' >"$work/help-line.txt" || self_failed "setup at line $LINENO"
  grep '^# rule: ' "$self" >"$work/rule-line.txt" || self_failed "setup at line $LINENO"
  cfg_sentence="a rule of text refuses any other name where the text cfg, cfg! or cfg_attr stands before its parenthesis, in the tracked files of the three crate directories and in every file that a forbid run's dep-info names, in comments and strings too; it does not see a predicate brought by a macro variable named like an allowed word or one split by a comment; for those spellings the dev and the release forbid runs hold what they compile, and the rule on the word clippy holds the one value that no forbid run has in common with a build"
  for place in help-line rule-line; do
    if ! grep -q -F -e "$cfg_sentence" "$work/$place.txt"; then
      printf 'case %s/help want=PASS got=without-the-cfg-sentence-in-the-%s\n' "$name" "$place"
      wrong="$wrong help"
    fi
  done
  for sentence in \
    'a socket or a process start inside a third-party crate, also when code of the three crates calls it' \
    'code under a value that no forbid run has: the processor of an Intel Mac or of Windows on ARM, another target_env, target_abi or target_vendor, a target feature or a flag that a person sets for their own build, and a combination of unix and windows that neither job compiles' \
    'what the system does with a path that it opens over a network' \
    'what a process did after the command ended, in the egress check' \
    'the egress check'"'"'s writes under target/egress/' \
    'the forbid list and its canary come from one table of this script: a commit that changes 60-r6-egress.sh can take an entry out of the list and its canary line together, and only the readers of that guarded script see it' \
    'the check takes the commands from the binary'"'"'s own printing and tries no other word on the built binary; what holds the printing to the dispatch is one test of the crate, the_printing_names_exactly_the_commands_that_the_dispatch_runs, which calls dispatch in-process; a commit whose main or whose handler runs a word outside the table passes the check, a diff shows it, and the landing'"'"'s readers hold it' \
    'prints the names of its commands from the one table that dispatches them, one per line in the table'"'"'s order, so that a name that it does not print is a command that route-gate does not dispatch' \
    'and the second list has no entry; the count line prints cannot=0' \
    'the check runs the built route-gate under the egress check as "route-gate commands", a wrapped call like every other' \
    'The printing: after the control, in the macos job and in the windows job, the check runs' \
    'printing that does not exit 0 fails the check by' \
    'A printed command on neither list fails the check, and so does a list entry that the printing does not name' \
    'A file written under target/, or inside a directory that held no tracked file before the first call, is not seen there' \
    'the control holds against an egress check that wraps nothing; an egress check changed in the same commit can pass it, and scripts/egress/ is held by its own run'"'"'s checks and by the landing'"'"'s readers'; do
    if ! grep -q -i -F -e "$sentence" "$work/help-line.txt"; then
      printf 'case %s/help want=PASS got=without-a-sentence\n' "$name"
      wrong="$wrong help"
    fi
  done
}

sysroot_asks() {
  case_wanted sysroot-asked-once || return 0
  asks=0
  if [ -f "$work/sysroot-asks.txt" ]; then
    while IFS= read -r line; do
      asks=$((asks + 1))
    done <"$work/sysroot-asks.txt"
  fi
  want_asks=0
  if [ "$host_job" = windows ] && case_named import-listing-printed; then
    want_asks=1
  fi
  if [ "$asks" -eq "$want_asks" ]; then
    printf 'case %s/sysroot-asked-once want=PASS got=PASS\n' "$name"
  else
    printf 'case %s/sysroot-asked-once want=PASS got=asked-%s-times-of-%s\n' "$name" "$asks" "$want_asks"
    wrong="$wrong sysroot-asked-once"
  fi
}

self_test_end() {
  sysroot_asks
  if case_wanted forms; then
    check_forms
  fi
  for list in windows macos; do
    while IFS= read -r listed; do
      case_named "$listed" || continue
      if ! grep -q -x -F -e "$listed" "$work/$list-seen.txt"; then
        printf 'case %s/%s want=PASS got=on-the-%s-list-and-neither-run-nor-skipped\n' "$name" "$listed" "$list"
        wrong="$wrong $listed"
      fi
    done <"$work/$list-cases.txt"
  done
  if [ -n "${R6_EGRESS_CASES:-}" ] && [ "${cases_matched:-no}" != yes ]; then
    self_failed "R6_EGRESS_CASES matched no case"
  fi
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
