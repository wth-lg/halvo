#!/bin/sh
# jobs: macos windows
# rule: No code of halvo-route, halvo-route-gate or halvo-route-wire that a job compiles names a listed type or method of a process or a socket or uses unsafe code, by a clippy run in the dev and in the release profile whose list the script writes and whose canary proves it in the run, over a closed graph of packages; a cfg predicate may name test, unix, windows, any, all and not, and a rule of text refuses any other name where the text cfg, cfg! or cfg_attr stands before its parenthesis, in the tracked files of the three crate directories and in every file that a forbid run's dep-info names, in comments and strings too; it does not see a predicate brought by a macro variable named like an allowed word or one split by a comment; for those spellings the dev and the release forbid runs hold what they compile, and the rule on the word clippy holds the one value that no forbid run has in common with a build; no attribute path comes from a macro variable and no flags or wrapper variable lifts a lint; once one of these holds has refused, the check builds and runs nothing of route-gate; and every command of the first list, run with its arguments under scripts/egress/check.sh with the commit's list behind a control of the wrap, exits 0 with denied=0, the asking runs wrapped in the macos job, every file under scripts/r6/egress/ in the commit holds no entry line and the run leaves no tracked file changed; it holds no more than --help says.
set -u
name=60-r6-egress
list=scripts/r6/egress/gate.txt
trial=docs/r2/trials/egress.md
probe_word=--r6-egress-no-such-argument
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
entry of the job's system is required, else "the forbid run let the canary through". It holds an
untracked cargo configuration, a cap on lints and a wrapper, which silence the canary too. The
environment rule reads no cargo configuration file: the compiler flags keys of its build and target
tables and its wrapper keys reach the forbid runs and the build alike, and the canary fails a cap and
a wrapper set there.
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
route-gate and runs no command of it, no asking and no run of the first list.
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
The asking: the asking asks each one-word string literal with no space, quote or backslash of the
tracked files of the crates that route-gate links, outside tests/, benches/ and examples/; a command
that it does not find, by any form, runs under no egress check, and the forbid run alone holds it, in
code of the three crates. A word is a command when the binary does not answer "REFUSED usage: unknown
command", with exit 2 and nothing on standard output, to it alone or to it with one argument that no
command takes. In the macos job the whole asking runs in one call of scripts/egress/check.sh with
the list, under one deadline (240 s, or R6_EGRESS_ASKING_SECONDS from 1 to 240). The asker asks the
egress run's own loopback probe first and last, with a listener of its own on 127.0.0.1:47202, and the
check counts those two reports apart from the words' reports; an asking that ends with fewer than two
of them is a lost report, asked again up to three times, and then fails by the words "the asking's
report was lost". It prints "egress asking denied=<n> probes=<n> tries=<n>", and its denied adds to
denied. In the words of E-16: denied counts the reports that arrived, and a reach by an asked word is
stopped by the mode whether or not its report arrives; on this Mac 3 of 20 wrapped askings over all
words lost the report of a reach and 0 of 20 over one word, and on the hosted macos-15 runner 1 of 20
over all words and 0 of 50 over one word (run 43680812402) (SG-64 item 4, round 3 of builder-26). The
asking holds nothing that the forbid run does not. The asker reads its words before the first ask, keeps
its record of the found words in its own memory and prints it to standard output, into a file under
target/egress/ where the egress check lets no wrapped program write; the count of words that it asked
must equal the count of words that it was given, or the check fails by "egress asking asked=<n> of <n>
words". A literal on a line whose quotes do not pair, as with a raw string or a comment mark, may be
missed. The windows job does not ask: there the egress check's rule
names the wrapped program, so a reach by a child of a wrapped shell is logged and failed, not
stopped; the forbid run holds the code that the windows job compiles, and every command of the first
list runs.
The two lists: the commands that the check runs under the egress check, each with its arguments,
and the commands that it cannot run on files of the repository alone, each with its reason. A found
command on neither list fails the check, and so does a list entry that is no command.
The control: before the first wrapped run, a listener on 127.0.0.1:47201 and one wrapped run of the
egress run's loopback probe; a status of 0 with denied=0 is "the egress check let the control
through". The control holds against an egress check that wraps nothing; an egress check changed in
the same commit can pass it, and scripts/egress/ is held by its own run's checks and by the landing's
readers. Every wrapped run must name a new log directory whose denied.log holds as many lines as its
denied. A port of the control or of the asking that another process holds fails the check by
"refused=port"; on a machine that other runs share, this is a false FAIL.
The import listing: the check prints the count of the undefined or imported symbols of the dev
route-gate binary that it built (nm -u on macOS, llvm-readobj --coff-imports on Windows where the
runner has it, else one line that it has none) and keeps the listing under its artifacts directory;
it is a measurement and refuses nothing.
A signal INT, TERM or HUP stops the check's listeners and ends it with FAIL 60-r6-egress (signal).
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
  while IFS=' ' read -r code at; do
    wanted=$((wanted + 1))
    if grep -q -x -F -e "finding $code src/lib.rs $at" "$work/canary-read.txt"; then
      found=$((found + 1))
    else
      [ "$((wanted - found))" -gt 5 ] || printf 'egress canary profile=%s missing=%s:%s\n' "$1" "$code" "$at"
    fi
  done <"$canary/expected.txt"
  if grep -q -x -F -e refer "$work/canary-read.txt"; then
    refers=1
  fi
  printf 'egress canary profile=%s entries=%s found=%s exit=%s\n' "$1" "$wanted" "$found" "$canary_exit"
  if [ "$found" -ne "$wanted" ] || [ "$wanted" -eq 0 ] || [ "$refers" -ne 0 ]; then
    printf 'egress canary: the forbid run let the canary through\n'
    fail check
  fi
}

env_rules() {
  env | sed -n 's/^\([A-Za-z_][A-Za-z0-9_]*\)=.*/\1/p' | sort -u >"$work/env-names.txt" || fail setup
  while IFS= read -r var; do
    upper=$(printf '%s\n' "$var" | tr '[:lower:]' '[:upper:]') || fail setup
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
  done <"$work/env-names.txt"
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
  sysroot=$(rustc --print sysroot 2>/dev/null) || sysroot=
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

read_candidates() {
  : >"$work/grep.z"
  while IFS=' ' read -r kind value; do
    [ "$kind" = crate ] || continue
    git -c core.quotePath=off grep -a -z -e '"' "$tree" -- "$value" >>"$work/grep.z" 2>"$work/grep.err"
    case $? in
      0 | 1) ;;
      *) fail setup ;;
    esac
  done <"$work/graph.txt"
  tr '\000' '\001' <"$work/grep.z" >"$work/grep.txt" || fail setup
  awk '
    {
      cut = index($0, "\001")
      if (cut == 0) next
      path = substr($0, 1, cut - 1)
      path = substr(path, index(path, ":") + 1)
      if (path ~ /^crates\/[^\/]+\/(tests|benches|examples)\//) next
      line = substr($0, cut + 1)
      n = length(line)
      i = 1
      while (i <= n) {
        c = substr(line, i, 1)
        if (c == "\047" && substr(line, i + 2, 1) == "\047") {
          i += 3
          continue
        }
        if (c == "\047" && substr(line, i + 1, 1) == "\\" && substr(line, i + 3, 1) == "\047") {
          i += 4
          continue
        }
        if (c != "\"") {
          i++
          continue
        }
        word = ""
        closed = 0
        i++
        while (i <= n) {
          c = substr(line, i, 1)
          if (c == "\\") {
            word = word c substr(line, i + 1, 1)
            i += 2
            continue
          }
          if (c == "\"") {
            closed = 1
            i++
            break
          }
          word = word c
          i++
        }
        if (closed && word != "" && word !~ /[ "\\]/) print word
      }
    }
  ' "$work/grep.txt" | sort -u >"$work/candidates.txt" || fail setup
}

asker_program() {
  cat <<'EOF'
#!/bin/sh
exe=$1
words=$2
answers=$3
seconds=$4
probe=$5
loopback=$6
unknown='REFUSED usage: unknown command'
nl='
'
all=$words
found=
sh "$loopback" loopback2 >/dev/null 2>&1 </dev/null
mkdir -p "$answers/discover" || exit 2
cd "$answers/discover" || exit 2
(
  trap 'kill "$nap" 2>/dev/null; exit 0' TERM
  sleep "$seconds" &
  nap=$!
  wait "$nap"
  : >"$answers/timeout"
  if read -r child <"$answers/child"; then
    kill "$child" 2>/dev/null
  fi
  kill "$$" 2>/dev/null
) &
watchdog=$!
answers_unknown() {
  "$exe" "$@" >"$answers/ask.out" 2>"$answers/ask.err" </dev/null &
  printf '%s\n' "$!" >"$answers/child"
  wait "$!"
  code=$?
  answer=
  while IFS= read -r line || [ -n "$line" ]; do
    answer=$answer$line$nl
  done <"$answers/ask.err"
  [ "$code" -eq 2 ] && [ ! -s "$answers/ask.out" ] && [ "$answer" = "$unknown$nl" ]
}
asked=0
while IFS= read -r word; do
  [ -n "$word" ] || continue
  asked=$((asked + 1))
  printf '%s\n' "$word" >"$answers/asking"
  if ! answers_unknown "$word" || ! answers_unknown "$word" "$probe"; then
    found=$found$word$nl
  fi
done <<WORDS
$all
WORDS
sh "$loopback" loopback2 >/dev/null 2>&1 </dev/null
kill "$watchdog" 2>/dev/null
wait "$watchdog"
printf 'asked %s\n' "$asked"
while IFS= read -r word; do
  [ -z "$word" ] || printf 'found %s\n' "$word"
done <<FOUND
$found
FOUND
exit 0
EOF
}

list_copy() {
  calls=$((calls + 1))
  list_given=$work/list-$calls.txt
  git cat-file blob "$list_blob" >"$list_given" || fail setup
}

list_after() {
  copy_id=$(git hash-object --no-filters -- "$list_given" 2>/dev/null) || copy_id=-
  if [ "$copy_id" != "$list_blob" ]; then
    printf 'egress %s: a wrapped run changed the list\n' "$1"
    changed=yes
  fi
  tree_after "$1"
}

tree_hashes() {
  printf '%s\n' "$tracked_paths" | git -c core.quotePath=false hash-object --stdin-paths 2>/dev/null
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
}

wrapped_call() {
  call_out=$1
  shift
  list_copy
  before_numbers=$(egress_numbers)
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
  find target/egress -mindepth 1 -maxdepth 1 -type d 2>/dev/null | sed 's|.*/||' | awk '/^[0-9]+$/'
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
  lines=$(awk 'END { print NR }' "$log") || return 1
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

asking_seconds() {
  seconds=${R6_EGRESS_ASKING_SECONDS:-240}
  case $seconds in
    '' | 0* | *[!0-9]*) fail usage ;;
  esac
  [ "$seconds" -le 240 ] || fail usage
}

state_before_calls() {
  run_entries=$(run_list) || fail setup
  cannot_entries=$(cannot_list) || fail setup
  list_blob=$(git rev-parse --verify --quiet "$tree:$list") || fail setup
  candidate_words=$(cat "$work/candidates.txt") || fail setup
  candidate_count=$(printf '%s\n' "$candidate_words" | awk 'NF { n++ } END { print n + 0 }') || fail setup
  tracked_paths=$(git -c core.quotePath=false ls-tree -r "$tree" | awk -F '\t' '$1 ~ /^100/ { print $2 }') || fail setup
  tracked_hashes=$(tree_hashes) || fail setup
  git_reference=$(git_view) || fail setup
  out_dir=$(mktemp -d "${TMPDIR:-/tmp}/$name-out.XXXXXX") || fail setup
}

discover() {
  asked=0
  if [ "$job" = windows ]; then
    printf 'egress asking=not-run (in the windows job the egress check names the wrapped program alone, so a reach by a child of a wrapped shell is logged and failed, not stopped; the forbid run holds the code that windows compiles)\n'
    return
  fi
  asker_text=$(asker_program) || fail setup
  asking_seconds
  if ! start_listener 47202 asking; then
    found_commands=
    return
  fi
  asking_listener=$listener
  try=0
  probes=0
  while [ "$try" -lt 4 ]; do
    try=$((try + 1))
    answers=$work/answers-$try
    mkdir -p "$answers" || fail setup
    record=target/egress/r6-asking-$$-$try.out
    mkdir -p target/egress || fail setup
    wrapped_call "$record" sh -c "$asker_text" asker "$exe" "$candidate_words" "$answers" "$seconds" \
      "$probe_word" "$repo_abs/scripts/egress/probe.sh"
    list_after asking
    read_wrapped
    [ "$wrapped" = line ] || break
    probes=0
    if [ -f "$log" ] && [ ! -h "$log" ]; then
      probes=$(grep -c 'dest=127\.0\.0\.1:47202 ' "$log") || probes=0
    fi
    [ "$status" = 0 ] && [ "$probes" -lt 2 ] || break
  done
  : >"$work/asking-stop"
  wait "$asking_listener"
  if [ -f "$answers/timeout" ]; then
    IFS= read -r word <"$answers/asking" || word=-
    printf 'egress asking timeout=%s word=%s\n' "$seconds" "$word"
    ran=no
  fi
  case $wrapped in
    refused)
      refused_mode=yes
      ran=no
      printf 'egress asking refused=mode\n'
      ;;
    missing)
      ran=no
      printf 'egress asking line=missing exit=%s\n' "$code"
      ;;
    line)
      reached=$((count - probes))
      [ "$reached" -ge 0 ] || reached=0
      denied=$((denied + reached))
      printf 'egress asking denied=%s probes=%s tries=%s command=%s exit=%s\n' "$reached" "$probes" "$try" "$status" "$code"
      wrapped_holds "$code" || ran=no
      if ! log_holds; then
        printf 'egress asking log=not-new-or-not-its-own\n'
        ran=no
      fi
      if [ "$status" = 0 ] && [ "$probes" -lt 2 ]; then
        printf "egress asking: the asking's report was lost in %s tries\n" "$try"
        ran=no
      fi
      ;;
  esac
  record_text=$(cat "$record") || record_text=
  printf '%s\n' "$record_text" >"$artifacts/asking-$try.out" || fail setup
  found_commands=$(printf '%s\n' "$record_text" | sed -n 's/^found //p') || fail setup
  asked=$(printf '%s\n' "$record_text" | sed -n 's/^asked //p') || fail setup
  if [ "$asked" != "$candidate_count" ]; then
    printf 'egress asking asked=%s of %s words\n' "${asked:--}" "$candidate_count"
    asked=0
    ran=no
  fi
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
  if [ "$job" = windows ]; then
    found_commands=$run_words
    commands=-
  else
    commands=$(printf '%s\n' "$found_commands" | awk 'NF { n++ } END { print n + 0 }') || fail setup
    mismatch=$({
      printf '%s\n' "$listed"
      printf '%s\n' '--found--'
      printf '%s\n' "$found_commands"
    } | awk '
      $0 == "--found--" { part = 1; next }
      $0 == "" { next }
      part == 0 { listed[$0] = 1; next }
      { found[$0] = 1 }
      part == 1 && !($0 in listed) { print "egress on-neither-list=" $0 }
      END { for (word in listed) if (!(word in found)) print "egress no-command=" word }
    ') || fail setup
    if [ -n "$mismatch" ]; then
      printf '%s\n' "$mismatch"
      matched=no
    fi
  fi
  cannot=$(printf '%s\n' "$cannot_words" | awk 'NF { n++ } END { print n + 0 }') || fail setup
  printf 'egress commands=%s asked=%s run=%s cannot=%s crates=%s\n' "$commands" "$asked" "$run_names" "$cannot" "$crates"
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
  for who in control asking; do
    : >"$work/$who-stop" 2>/dev/null
  done
  [ -z "$listener" ] || wait "$listener" 2>/dev/null
  [ -z "$asking_listener" ] || wait "$asking_listener" 2>/dev/null
  printf 'egress stopped by a signal\n'
  fail signal
}

check() {
  enter_repo
  artifacts=${CI_D_ARTIFACTS:-target/ci-artifacts/$name}
  [ -d "$artifacts" ] || mkdir -p "$artifacts" 2>/dev/null || fail setup
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail setup
  mkdir -p "$work/discover" || fail setup
  listener=
  asking_listener=
  trap stopped INT TERM HUP
  R6_EGRESS_CHECK_PID=$$
  export R6_EGRESS_CHECK_PID
  platform_of
  job_of
  root_abs=$(absolute_dir .) || fail setup
  repo_abs=$(pwd) || fail setup
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
  read_candidates
  state_before_calls
  control
  discover
  compare_lists
  run_checks

  tree_after end

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
  printf '%s\n' '$stub_sysroot'
  exit 0
fi
exec '$real_rustc' "\$@"
EOF
  chmod +x "$work/bin/rustc" || self_failed "setup at line $LINENO"
  cat >"$work/bin/cargo" <<'EOF' || self_failed "setup at line $LINENO"
#!/bin/sh
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
  'build --locked -p halvo-route-gate --bin route-gate') exit 0 ;;
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
[ -z "$STUB_MARKER" ] || printf 'run %s\n' "$*" >>"$STUB_MARKER"
if [ "$#" -eq 1 ] && [ "$1" = reachword ] && [ -n "$STUB_REACH" ]; then
  printf 'reach\n' >>"$STUB_REACH"
fi
if [ "$#" -eq 1 ] && [ "$1" = hangword ] && [ -n "$STUB_HANG" ]; then
  printf '%s\n' "$$" >"$STUB_HANG"
  exec sleep 30
fi
if [ "$1" = eraser ] && [ "$STUB_ERASER" = killer ]; then
  kill -9 "$PPID"
  exit 0
fi
if [ "$1" = eraser ] && [ -n "$STUB_ERASER" ]; then
  if [ -f ../commands.txt ]; then
    awk '$0 != "eraser"' ../commands.txt >../commands.kept && mv ../commands.kept ../commands.txt
  fi
  exit 0
fi
if [ "$1" = argword ] && [ -n "$STUB_ARGONLY" ]; then
  [ "$#" -ge 2 ] && exit 0
fi
for odd in $STUB_ODD; do
  [ "$1" = "${odd%%:*}" ] || continue
  case ${odd#*:} in
    stdout)
      printf 'x\n'
      printf 'REFUSED usage: unknown command\n' >&2
      exit 2
      ;;
    exit3)
      printf 'REFUSED usage: unknown command\n' >&2
      exit 3
      ;;
    text)
      printf 'REFUSED usage: unknown commands\n' >&2
      exit 2
      ;;
  esac
done
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
  chmod +x "$work/bin/git" "$work/bin/cargo" "$work/route-gate" || self_failed "setup at line $LINENO"
}

write_repo() {
  dir=$1
  mkdir -p "$dir/scripts/r6/egress" "$dir/scripts/egress" "$dir/docs/r2/trials" "$dir/crates/halvo-route-gate/src" \
    "$dir/crates/halvo-route-gate/tests" || self_failed "setup at line $LINENO"
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
  cat >"$dir/crates/halvo-route-gate/src/main.rs" <<'EOF' || self_failed "setup at line $LINENO"
fn main() {
    let words = ["generate", "import", "derive", "score", "self-test", "spend-sim", "probe"];
    let more = ["fetch", "other", "Upper", "--flag", "post_route", "push"];
    let adjacent = words! {"see help","adjword"};
    let odd = ["stdoutword", "exitword", "textword"];
    let wrapped = ["reachword", "hangword", "argword", "eraser"];
    let q = '"'; let w = "quoteword";
}
EOF
  printf '// one byte that git takes for binary: \000\npub const W: &str = "nulword";\n' \
    >"$dir/crates/halvo-route-gate/src/zerobyte.rs" || self_failed "setup at line $LINENO"
  printf '%s\n' 'const WORD: &str = "testonly";' >"$dir/crates/halvo-route-gate/tests/t.rs" || self_failed "setup at line $LINENO"
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
  rewrite-record:generate | cut-run-list:generate)
    out=
    for arg in "$@"; do
      [ "$prev" != --out ] || out=$arg
      prev=$arg
    done
    parent=${out%/*}
    parent=${parent%/*}
    if [ "$STUB_CHECK" = rewrite-record ]; then
      printf 'generate\n' >"$parent/commands.txt"
    else
      : >"$parent/run-list.txt"
    fi
    ;;
  widen:generate) printf '127.0.0.1:47391\n' >>scripts/r6/egress/gate.txt ;;
  widen:probe) printf '[fixed]\n[agent]\n' >scripts/r6/egress/gate.txt ;;
  change-copy:generate) printf '127.0.0.1:47391\n' >>"$list" ;;
esac
count_lines() {
  counted=0
  [ -n "$1" ] && [ -f "$1" ] || return 0
  while IFS= read -r counted_line || [ -n "$counted_line" ]; do
    counted=$((counted + 1))
  done <"$1"
}
count_lines "$STUB_REACH"
reached=$counted
count_lines "$STUB_PROBE_LOG"
probed=$counted
"$@"
code=$?
[ "$empty" = no ] || code=9
count_lines "$STUB_PROBE_LOG"
probes=$((counted - probed))
[ -z "$STUB_TOUCH" ] || printf 'x\n' >>"$STUB_TOUCH"
count_lines "$STUB_REACH"
denied=$((counted - reached))
[ "$STUB_CHECK" != denied ] || denied=$((denied + 1))
[ "$STUB_CHECK" != denied0 ] || denied=$((denied + 1))
[ "$STUB_CHECK:$1" != asking-status3:sh ] || code=3
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
  i=0
  while [ "$i" -lt "$probes" ]; do
    printf '%s\n' 'denied at=- proto=tcp dest=127.0.0.1:47202 pid=1 image=egress-probe'
    i=$((i + 1))
  done
} >"target/egress/$n/denied.log"
if [ "$STUB_CHECK" = link-log ]; then
  mv "target/egress/$n/denied.log" "target/egress/$n/denied.real" && ln -s denied.real "target/egress/$n/denied.log"
fi
denied=$((denied + probes))
case $STUB_CHECK:$1:$2 in
  reuse-log:*) n=1 ;;
  reuse-log-run:sh:*) ;;
  reuse-log-run:*) n=1 ;;
  reuse-log-asking:sh:-c) n=1 ;;
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
case $1 in
  none)
    mkdir -p target/egress/bin || exit 2
    for spawn in target/egress/bin/egress-spawn; do
      cat >"$spawn" <<'SPAWN' || exit 2
#!/bin/sh
[ "$1" = serve ] || exit 2
[ "$STUB_CONTROL" != port ] || exit 3
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
    [ "$STUB_CONTROL" = through ] && exit 0
    exit 1
    ;;
  loopback2)
    case $STUB_PROBE in
      lost) ;;
      lost-once)
        if [ -f "$STUB_PROBE_LOG.once" ]; then
          printf 'probe\n' >>"$STUB_PROBE_LOG"
        else
          : >"$STUB_PROBE_LOG.once"
        fi
        ;;
      *) printf 'probe\n' >>"$STUB_PROBE_LOG" ;;
    esac
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
  STUB_COMMANDS='generate import derive score self-test spend-sim probe'
  STUB_RUN_EXIT=0
  STUB_REACH=
  STUB_MARKER=
  STUB_ERASER=
  STUB_PROBE=
  STUB_HANG=
  STUB_CONTROL=
  STUB_ODD=
  STUB_BARE=
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
  STUB_ARGONLY=
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
windows-asking-line asking-not-run-in-the-windows-job
checks-count job-with-no-run
windows-sentence sentence-on-a-hosted-runner
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
command-exits-1
entry-with-no-arguments
tracked-file-changed-by-a-run
env-refused-then-a-stale-build
signal-stops-the-listeners
pass
import-listing-printed
asking-not-run-in-the-windows-job
job-with-no-run
sentence-on-a-hosted-runner
EOF
}

macos_cases() {
  cat <<'EOF'
sentence
commands
third-party
forbid-clean
asking-wrapped
control-denied
control-report-lost-four-times
control-port-in-use
log-of-another-run
log-with-another-count
asking-reaches-the-network-when-asked-bare
asking-hangs-past-its-deadline
asking-deadline-above-240
asking-deadline-of-240
found-command-that-erases-its-record
asked-command-that-kills-the-asker
asking-report-lost-four-times
asking-report-lost-once-and-asked-again
asking-status-read
graph-closed
rules-allowed-forms
env-deny-flag
env-allow-flag
env-encoded-allow
env-per-target-cfg
env-clippy-arguments
env-wrapper-set
env-plain-wrapper-set
env-build-flags-allow
env-flags-argument-file
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
graph-third-party-package-added
graph-build-time-package-added
graph-build-time-package-linked
graph-path-package-added
graph-package-gone
graph-build-script
graph-proc-macro
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
forbid-exit-without-a-finding
forbid-child-code-is-no-finding
forbid-list-entry-refers-to-nothing
command-on-neither-list
command-with-an-underscore
command-alone-without-arguments
command-in-a-file-with-a-nul-byte
command-behind-adjacent-literals
command-in-a-non-rs-file-with-a-non-ascii-name
command-after-a-quote-character
command-only-with-an-argument
repository-missing
line-of-another-list
log-of-another-run-of-the-gate-tool
run-changes-its-copy-of-the-list
log-of-another-asking
answer-on-standard-output
answer-with-exit-3
answer-with-other-words
entry-that-is-no-command
word-of-tests-not-asked
denied-in-each-run
check-exits-0-with-a-denial
command-exits-1-with-a-denial
refusal-mode
check-exits-3-with-a-clean-line
stale-build
forbid-finding-then-a-stale-build
no-egress-line
control-without-an-egress-line
tracked-file-changed-before
no-tree
usage
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
  PATH=$work/bin:$PATH CI_D_REPO=$case_repo CI_D_ARTIFACTS=$work/artifacts TMPDIR=$work/tmp \
    STUB_TREE=$case_tree STUB_EXE=$work/route-gate STUB_COMMANDS=$STUB_COMMANDS STUB_BARE=$STUB_BARE STUB_ODD=$STUB_ODD STUB_REACH=$STUB_REACH STUB_MARKER=$STUB_MARKER STUB_ERASER=$STUB_ERASER STUB_PROBE=$STUB_PROBE STUB_PROBE_LOG=$work/probe.log STUB_HANG=$STUB_HANG STUB_CONTROL=$STUB_CONTROL \
    STUB_RUN_EXIT=$STUB_RUN_EXIT STUB_CHECK=$STUB_CHECK STUB_TOUCH=$STUB_TOUCH STUB_FRESH=$STUB_FRESH \
    STUB_CLIPPY_JSON=$STUB_CLIPPY_JSON STUB_CLIPPY_EXIT=$STUB_CLIPPY_EXIT STUB_CANARY=$STUB_CANARY \
    STUB_CANARY_DROP=$STUB_CANARY_DROP STUB_CANARY_DROP_PROFILE=$STUB_CANARY_DROP_PROFILE \
    STUB_CLIPPY_RELEASE_JSON=$STUB_CLIPPY_RELEASE_JSON STUB_CLIPPY_RELEASE_EXIT=$STUB_CLIPPY_RELEASE_EXIT STUB_GRAPH=$STUB_GRAPH REAL_CARGO=$real_cargo CI_D_JOB=$case_job GITHUB_ACTIONS=$case_actions \
    STUB_ARGONLY=$STUB_ARGONLY STUB_SPAWN_PIDS=$work/spawn.pids env ${case_env:+"$case_env"} sh "${case_self:-$self}" ${case_arg:+"$case_arg"} >"$work/case.txt" 2>&1 </dev/null
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
  case_ended=$(date +%s) || case_ended=$case_started
  printf 'case-seconds %s %s job=%s\n' "$1" "$((case_ended - case_started))" "$case_job"
  if [ "$got" != "$2" ]; then
    wrong="$wrong $1"
  elif [ "$2" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
  case_reset
}

part_seconds() {
  case_wanted part-seconds || return 0
  reps=10
  for part in stand-in-start forbid-stub wrapped-call list-checks import-listing; do
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
          (cd "$work/repo" && STUB_COMMANDS=probe STUB_PROBE_LOG=$work/part-probe.log \
            sh scripts/egress/check.sh scripts/r6/egress/gate.txt -- "$work/route-gate" probe --rounds 1 >/dev/null 2>&1) ;;
        list-checks)
          (cd "$work/repo" && cp scripts/r6/egress/gate.txt "$work/part-list.txt" && cmp -s "$work/part-list.txt" scripts/r6/egress/gate.txt &&
            cmp -s scripts/r6/egress/gate.txt "$work/part-list.txt" &&
            git -c core.fsmonitor=false diff --no-renames --name-status "$good_tree" -- >/dev/null 2>&1) ;;
        import-listing)
          (PATH=$work/bin:$PATH rustc --print sysroot >/dev/null 2>&1) ;;
      esac
      i=$((i + 1))
    done
    ended=$(date +%s) || ended=$started
    printf 'part-seconds %s %s for %s calls\n' "$part" "$((ended - started))" "$reps"
  done
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
  cp "$work/rust-toolchain.toml" "$dir/rust-toolchain.toml" || self_failed "setup at line $LINENO"
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
  cp "$work/rust-toolchain.toml" "$dir-bin/rust-toolchain.toml" || self_failed "setup at line $LINENO"
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

non_ascii_repo() {
  write_repo "$1" '' >/dev/null || self_failed "setup at line $LINENO"
  plain=$(printf 'pub const W: &str = "plainword";\n' | git -C "$1" hash-object -w --stdin) || self_failed "setup at line $LINENO"
  printf '100644 %s\tcrates/halvo-route-gate/src/upl\303\251.txt\n' "$plain" | git -C "$1" update-index --index-info ||
    self_failed "setup at line $LINENO"
  printf 'crates/halvo-route-gate/src/upl\303\251.txt\n' | git -C "$1" update-index --skip-worktree --stdin ||
    self_failed "setup at line $LINENO"
  accented=$(printf 'upl\303\251.txt') || self_failed "setup at line $LINENO"
  printf 'pub const W: &str = "plainword";\n' >"$1/crates/halvo-route-gate/src/$accented" 2>/dev/null || :
  git -C "$1" write-tree 2>/dev/null || self_failed "setup at line $LINENO"
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
  non_ascii_tree=$(non_ascii_repo "$work/repo-non-ascii") || self_failed "setup at line $LINENO"
  gitlink_tree=$(odd_repo "$work/repo-gitlink" gitlink) || self_failed "setup at line $LINENO"
  awk '/^run_list\(\) \{/ { skip = 1 } skip && /^EOF$/ { skip = 0 } skip && /^[a-z-]+ / { next } { print }' "$self" \
    >"$work/norun.sh" || self_failed "setup at line $LINENO"
  awk '/^spend-sim --seed 1$/ { print "spend-sim"; next } { print }' "$self" >"$work/bare.sh" || self_failed "setup at line $LINENO"
  awk '{ print } /^import its inputs/ { print "probe a second entry of the same name" }' "$self" >"$work/twice.sh" ||
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
  wrong=
  rejected=0
  v_lower_flags=$(printf '%s\n' "$v_flags" | tr '[:upper:]' '[:lower:]') || self_failed "setup at line $LINENO"
  v_lower_wrapper=$(printf '%s\n' "${w_rustc}_WRAPPER" | tr '[:upper:]' '[:lower:]') || self_failed "setup at line $LINENO"
  case_reset
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

  case_run pass PASS - "egress checks=6 denied=0 entries=0${nl}denied=0 proves that no process"
  part_seconds
  case_run import-listing-printed PASS - 'egress imports binary=dev reader=' 
  case_run sentence PASS - "while the command ran on $p; not verified: path-1-$p, path-2-$p, and what a process did after the command ended.${nl}A connection that the mode stops"
  case_actions=true
  case_run sentence-on-a-hosted-runner PASS - "while the command ran on $hosted; not verified: path-1-$hosted, path-2-$hosted, and what"
  case_run commands PASS - 'egress commands=7 asked=24 run=6 cannot=1 crates=3'
  case_run third-party PASS - 'egress limit third-party crates=6 names=itoa,memchr,serde,serde_core,serde_json,zmij'
  case_run forbid-clean PASS - 'egress forbid profile=dev findings=0 exit=0'
  case_run asking-wrapped PASS - 'egress asking denied=0 probes=2 tries=1 command=0 exit=1'
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
  STUB_REACH=$work/reach.log
  case_run asking-reaches-the-network-when-asked-bare FAIL denied 'egress asking denied=1 probes=2 tries=1 command=0 exit=1'
  STUB_HANG=$work/hang.pid
  case_env=R6_EGRESS_ASKING_SECONDS=2
  case_run asking-hangs-past-its-deadline FAIL check 'egress asking timeout=2 word=hangword'
  if [ "$host_job" = macos ] && case_wanted asking-hangs-past-its-deadline; then
    hung=
    [ ! -s "$work/hang.pid" ] || read -r hung <"$work/hang.pid"
    if [ -z "$hung" ]; then
      printf 'case %s/asking-hangs-past-its-deadline want=PASS got=no-hung-child\n' "$name"
      wrong="$wrong hung-child"
    elif kill -0 "$hung" 2>/dev/null; then
      printf 'case %s/asking-hangs-past-its-deadline want=PASS got=hung-child-alive\n' "$name"
      wrong="$wrong hung-child"
      kill "$hung" 2>/dev/null
    fi
  fi
  case_env=R6_EGRESS_ASKING_SECONDS=241
  case_run asking-deadline-above-240 FAIL usage 'FAIL'
  case_env=R6_EGRESS_ASKING_SECONDS=240
  case_run asking-deadline-of-240 PASS - 'egress asking denied=0'
  STUB_ERASER=erase
  case_run found-command-that-erases-its-record FAIL check 'egress on-neither-list=eraser'
  STUB_ERASER=killer
  case_run asked-command-that-kills-the-asker FAIL check 'egress asking asked=- of 24 words'
  STUB_PROBE=lost
  case_run asking-report-lost-four-times FAIL check "egress asking: the asking's report was lost in 4 tries"
  STUB_PROBE=lost-once
  case_run asking-report-lost-once-and-asked-again PASS - 'egress asking denied=0 probes=2 tries=2 command=0 exit=1'
  STUB_CHECK=asking-status3
  case_run asking-status-read FAIL check 'egress asking denied=0 probes=2 tries=1 command=3 exit=1'
  case_job=windows
  case_run asking-not-run-in-the-windows-job PASS - 'egress asking=not-run (in the windows job'
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
  STUB_CANARY=real
  case_job=$own_job
  case_run canary-on-the-real-cargo PASS - 'egress canary profile=dev entries='
  STUB_CLIPPY_RELEASE_JSON=$work/clippy-finding.json
  STUB_CLIPPY_RELEASE_EXIT=101
  case_run forbid-finding-in-release FAIL check "forbid release crates/halvo-route-gate/src/net.txt:2 clippy::disallowed_methods${nl}egress forbid profile=release findings=1 exit=101"
  STUB_CANARY_DROP=t01
  STUB_CANARY_DROP_PROFILE=release
  case_run canary-let-through-in-release FAIL check 'egress canary profile=release missing=clippy::disallowed_types:1'
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
  case_job=windows
  case_run windows-job-runs-every-command PASS - 'egress checks=6 denied=0 entries=0'
  case_job=windows
  case_self=$work/norun.sh
  case_run job-with-no-run FAIL check 'egress checks=0: no command of the first list ran'
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
  case_run list-file-with-one-entry FAIL entries 'egress checks=6 denied=0 entries=1'
  STUB_COMMANDS="$STUB_COMMANDS fetch"
  case_run command-on-neither-list FAIL check 'egress on-neither-list=fetch'
  STUB_COMMANDS="$STUB_COMMANDS post_route"
  case_run command-with-an-underscore FAIL check 'egress on-neither-list=post_route'
  STUB_BARE=push
  case_run command-alone-without-arguments FAIL check 'egress on-neither-list=push'
  STUB_COMMANDS="$STUB_COMMANDS nulword"
  case_run command-in-a-file-with-a-nul-byte FAIL check 'egress on-neither-list=nulword'
  STUB_COMMANDS="$STUB_COMMANDS adjword"
  case_run command-behind-adjacent-literals FAIL check 'egress on-neither-list=adjword'
  STUB_COMMANDS="$STUB_COMMANDS plainword"
  case_repo=$work/repo-non-ascii
  case_tree=$non_ascii_tree
  case_run command-in-a-non-rs-file-with-a-non-ascii-name FAIL check 'egress on-neither-list=plainword'
  STUB_COMMANDS="$STUB_COMMANDS quoteword"
  case_run command-after-a-quote-character FAIL check 'egress on-neither-list=quoteword'
  STUB_ARGONLY=1
  case_run command-only-with-an-argument FAIL check 'egress on-neither-list=argword'
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
  case_run list-directory-holds-a-link FAIL entries 'egress checks=6 denied=0 entries=1'
  case_repo=$work/repo-gitlink
  case_tree=$gitlink_tree
  case_run list-directory-holds-a-submodule FAIL entries 'egress checks=6 denied=0 entries=1'
  STUB_CANARY=refer
  case_run canary-list-entry-refers-to-nothing FAIL check 'egress canary: the forbid run let the canary through'
  STUB_CHECK=reuse-log-run
  case_run log-of-another-run-of-the-gate-tool FAIL check 'egress run=generate log=not-new-or-not-its-own'
  STUB_CHECK=rewrite-record
  case_run run-rewrites-the-record-of-found-commands PASS - 'egress checks=6 denied=0 entries=0'
  STUB_CHECK=cut-run-list
  case_run run-cuts-the-run-list PASS - 'egress checks=6 denied=0 entries=0'
  STUB_CHECK=widen
  case_run run-widens-the-list-and-a-later-run-restores-it FAIL check 'egress run=generate: a wrapped run changed the list'
  STUB_CHECK=change-copy
  case_run run-changes-its-copy-of-the-list FAIL check 'egress run=generate: a wrapped run changed the list'
  STUB_CHECK=reuse-previous
  case_run log-of-the-run-before FAIL check 'egress run=generate log=not-new-or-not-its-own'
  STUB_CHECK=reuse-log-asking
  case_run log-of-another-asking FAIL check 'egress asking log=not-new-or-not-its-own'
  STUB_ODD=stdoutword:stdout
  case_run answer-on-standard-output FAIL check 'egress on-neither-list=stdoutword'
  STUB_ODD=exitword:exit3
  case_run answer-with-exit-3 FAIL check 'egress on-neither-list=exitword'
  STUB_ODD=textword:text
  case_run answer-with-other-words FAIL check 'egress on-neither-list=textword'
  STUB_COMMANDS='generate import derive score self-test spend-sim'
  case_run entry-that-is-no-command FAIL check 'egress no-command=probe'
  STUB_COMMANDS='testonly generate import derive score self-test spend-sim probe'
  case_run word-of-tests-not-asked PASS - 'egress commands=7 asked=24'
  STUB_CHECK=denied
  all_denied=6
  [ "$host_job" != macos ] || all_denied=7
  case_run denied-in-each-run FAIL denied "egress checks=6 denied=$all_denied entries=0" 'denied=0 proves'
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
    R6_EGRESS_SELFTEST_HOST=windows R6_EGRESS_CASES='macos-case-with-no-run asking-wrapped' \
      sh "$work/extra-macos.sh" --self-test >"$work/extra-macos.txt" 2>&1 </dev/null
    code=$?
    if [ "$code" = 1 ] && grep -q -F -e 'asking-wrapped not-run:' "$work/extra-macos.txt" &&
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
    if [ "$code" = 1 ] && grep -q -F -e 'rule-table want=PASS got=rule-entries-has-no-case-on-the-windows-list' "$work/rule-table.txt"; then
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
    'the asking asks each one-word string literal with no space, quote or backslash of the tracked files of the crates that route-gate links, outside tests/, benches/ and examples/; a command that it does not find, by any form, runs under no egress check, and the forbid run alone holds it, in code of the three crates' \
    'the control holds against an egress check that wraps nothing; an egress check changed in the same commit can pass it, and scripts/egress/ is held by its own run'"'"'s checks and by the landing'"'"'s readers'; do
    if ! grep -q -i -F -e "$sentence" "$work/help-line.txt"; then
      printf 'case %s/help want=PASS got=without-a-sentence\n' "$name"
      wrong="$wrong help"
    fi
  done
}

self_test_end() {
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
