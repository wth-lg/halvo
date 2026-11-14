#!/bin/sh
set -u

usage() {
  cat <<'EOF'
Usage: sh tools/r3-trials/web/hosted/harness.sh [--guard]
       sh tools/r3-trials/web/hosted/harness.sh --self-test
       sh tools/r3-trials/web/hosted/harness.sh --help

The hosted harness of the org_ cells of A13.7 (U0.7c; ruling CP-133 (i)). On a GitHub-hosted
runner it sets each of the two organisation settings (RemoteDebuggingAllowed off, ProxyMode
direct) for Chrome and Edge as the system's policy, reads the browser's own policy page back, runs
the cell with sh tools/r3-trials/web/run.sh --rows org --round 3 --relay, and takes the setting
away: on macOS a property list in /Library/Managed Preferences installed with sudo (mac.sh, with
the page reader reader.swift, which swiftc builds on the runner), on Windows a value under
HKLM\SOFTWARE\Policies (win.sh). It runs from the repository's root, after a release build of the
web trial, and prints counts, booleans and closed-list words alone. collect.py builds the block of
docs/r3/trials/web-isolation.md from the lines.

The harness changes the machine's policy, so it refuses to run, with one line
REFUSED host: github_actions=<yes|no> runner_environment=<yes|no> image_os=<yes|no>
on standard error and exit 2, before any privileged call, unless it finds itself on a hosted
runner by three facts that a person's Mac lacks: GITHUB_ACTIONS is true, RUNNER_ENVIRONMENT is
github-hosted and ImageOS is set. A person can set the three by hand; the guard keeps the harness
from running by accident, and no check and no test of the repository calls the harness.

  --guard      runs the guard alone: prints "guard passed" and exits 0, or refuses.
  --self-test  runs the refusal with no privilege: the harness under each environment that lacks a
               fact, with stand-ins for sudo, reg, powershell, killall, install, defaults and
               swiftc on the PATH that record any call, must exit 2 with the one refused line and
               no call recorded; the guard with the three facts passes. Prints one line per case
               and "self-test cases=<n> failed=<n>"; exit 0 when none failed.
Exit 0 when every cell ran, 1 when a cell or a step failed, 2 for a refusal.
EOF
}

yes_no() {
  if "$@"; then echo yes; else echo no; fi
}

facts() {
  a=$(yes_no [ "${GITHUB_ACTIONS:-}" = true ])
  r=$(yes_no [ "${RUNNER_ENVIRONMENT:-}" = github-hosted ])
  i=$(yes_no [ -n "${ImageOS:-}" ])
}

guard() {
  facts
  if [ "$a$r$i" != yesyesyes ]; then
    echo "REFUSED host: github_actions=$a runner_environment=$r image_os=$i" >&2
    exit 2
  fi
}

here=$(cd "$(dirname "$0")" && pwd)

self_case() {
  name=$1
  shift
  log="$work/calls.$name"
  : >"$log"
  env -i PATH="$work/bin:/usr/bin:/bin" HOME="$work" R3_STUB_LOG="$log" "$@" \
    sh "$here/harness.sh" >"$work/out.$name" 2>"$work/err.$name"
  e=$?
  lines=$(wc -l <"$work/err.$name" | tr -d ' ')
  refused=$(grep -c '^REFUSED host: github_actions=[a-z]* runner_environment=[a-z]* image_os=[a-z]*$' "$work/err.$name")
  out=$(wc -c <"$work/out.$name" | tr -d ' ')
  calls=$(wc -l <"$log" | tr -d ' ')
  ok=no
  [ "$e" -eq 2 ] && [ "$lines" -eq 1 ] && [ "$refused" -eq 1 ] && [ "$out" -eq 0 ] && [ "$calls" -eq 0 ] && ok=yes
  echo "case $name exit=$e refused_lines=$refused stderr_lines=$lines stdout_bytes=$out privileged_calls=$calls ok=$ok"
  cases=$((cases + 1))
  [ "$ok" = yes ] || failed=$((failed + 1))
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/r3-hosted-harness.XXXXXX") || exit 2
  mkdir -p "$work/bin"
  # shellcheck disable=SC2016
  for t in sudo reg powershell killall install defaults swiftc plutil; do
    printf '#!/bin/sh\necho "%s $*" >>"$R3_STUB_LOG"\nexit 0\n' "$t" >"$work/bin/$t"
    chmod 755 "$work/bin/$t"
  done
  cases=0
  failed=0
  self_case none
  self_case actions_only GITHUB_ACTIONS=true
  self_case no_image GITHUB_ACTIONS=true RUNNER_ENVIRONMENT=github-hosted
  self_case empty_image GITHUB_ACTIONS=true RUNNER_ENVIRONMENT=github-hosted ImageOS=
  self_case actions_one GITHUB_ACTIONS=1 RUNNER_ENVIRONMENT=github-hosted ImageOS=macos15
  self_case self_hosted GITHUB_ACTIONS=true RUNNER_ENVIRONMENT=self-hosted ImageOS=macos15
  self_case no_actions RUNNER_ENVIRONMENT=github-hosted ImageOS=macos15
  : >"$work/calls.control"
  R3_STUB_LOG="$work/calls.control" "$work/bin/sudo" true
  seen=$(wc -l <"$work/calls.control" | tr -d ' ')
  ok=no
  [ "$seen" -eq 1 ] && ok=yes
  echo "case stub_records_a_call calls=$seen ok=$ok"
  cases=$((cases + 1))
  [ "$ok" = yes ] || failed=$((failed + 1))
  passed=$(env -i PATH="$work/bin:/usr/bin:/bin" R3_STUB_LOG="$work/calls.guard" GITHUB_ACTIONS=true \
    RUNNER_ENVIRONMENT=github-hosted ImageOS=macos15 sh "$here/harness.sh" --guard 2>/dev/null)
  e=$?
  ok=no
  [ "$e" -eq 0 ] && [ "$passed" = "guard passed" ] && ok=yes
  echo "case three_facts_pass_the_guard exit=$e ok=$ok"
  cases=$((cases + 1))
  [ "$ok" = yes ] || failed=$((failed + 1))
  echo "self-test cases=$cases failed=$failed"
  [ "$failed" -eq 0 ]
}

case ${1:-} in
  --help)
    usage
    exit 0
    ;;
  --self-test)
    [ $# -eq 1 ] || {
      echo "REFUSED usage: --self-test takes no argument" >&2
      exit 2
    }
    self_test
    exit $?
    ;;
  --guard)
    guard
    echo "guard passed"
    exit 0
    ;;
  '') ;;
  *)
    echo "REFUSED usage: unknown argument; see --help" >&2
    exit 2
    ;;
esac

guard
case $(uname -s) in
  Darwin)
    . "$here/mac.sh"
    mac_main "$here"
    ;;
  MINGW* | MSYS* | CYGWIN*)
    . "$here/win.sh"
    win_main
    ;;
  *)
    echo "REFUSED host: no harness for $(uname -s)" >&2
    exit 2
    ;;
esac
