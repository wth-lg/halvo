#!/bin/sh
# jobs: macos windows
# rule: Every view file keeps to the rules of a view file; the macos job makes the whole run of the six checks, the status-region rule and page.mjs on every state of ui/tests/fixtures/coord/ again and requires the committed record to be its record byte for byte, and the windows job requires the record to name the stamp of the files that the run reads and to be the record of a passing run, and runs again the states that its commit draws.
set -u
name=40-r4-page
nested=self-test-of-$name
own_per_push=4
needs_per_push=4

usage() {
  printf 'FAIL %s (usage)\n' "$name"
  exit 1
}

help() {
  cat <<'EOF'
usage: sh scripts/r4/ci.d/40-r4-page.sh [--self-test | --record | --help]

Through the driver: sh scripts/ci/run-ci-d.sh <macos|windows> 40-r4-page.
The job macos, and a call without the driver's job name, makes the whole run
again from the commit's files and compares its record with the committed
ui/tests/r4/page/record.txt byte for byte. The job windows checks the stamp,
reads the record and runs again the 4 states and 4 page.mjs names that the
SHA-256 of the commit draws, in Chromium. --record writes the committed record
and is run through the helper for timed runs.

The stamp holds every tracked file under ui/ and tools/r4-page/,
docs/r4/page-features.txt and this script, as the index holds them. Before a
run the check compares the copies of the harness's manifest and lock in the
browsers directory with the committed ones, and each installed package's
version with the lock.

Limits, each with its manual step:
- The hosted image, its system and its fonts, and the Node version beyond the
  check of its major version, are in no stamp.
- An unpacked browser build and the files of the installed packages are
  trusted by fetch.sh's marker and by their versions alone.
  Step for both: fetch into a new empty directory with
  sh tools/r4-page/fetch.sh DIR, run this script with --record through the
  helper for timed runs, and read the difference of the record. In WebKit the
  whole run of the macos job shows such a change at once; in Chromium it shows
  on the states that a push draws.
- The rules of a view file read the text, decoded names and string literals,
  a member named write or writeln, a with statement and a computed key that
  joins string literals. A key built at run time, an alias of window, eval,
  Function, atob, String.fromCharCode, Reflect, a dynamic import, a worker or
  a script element pass them; the six checks still run on the page.
EOF
}

mode=measure
case $# in
  0) ;;
  1)
    case $1 in
      --self-test) mode=self_test ;;
      --record) mode=record ;;
      --help)
        help
        exit 0
        ;;
      *) usage ;;
    esac
    ;;
  *) usage ;;
esac

prefix=
[ "$mode" = self_test ] && prefix=self-test:

fail() {
  printf 'FAIL %s%s (%s)\n' "$prefix" "$name" "$1"
  exit 1
}

child=
work=
cleanup() {
  if [ -n "$child" ]; then
    kill -TERM "$child" 2>/dev/null
    wait "$child" 2>/dev/null
    child=
  fi
  if [ -n "$work" ] && [ -d "$work" ]; then
    rm -rf -- "$work"
  fi
  work=
}
trap cleanup EXIT
trap 'cleanup; trap - EXIT; exit 129' HUP
trap 'cleanup; trap - EXIT; exit 130' INT
trap 'cleanup; trap - EXIT; exit 143' TERM

case $0 in
  */*) here=${0%/*} ;;
  *) here=. ;;
esac
here=$(cd "$here" 2>/dev/null && pwd) || fail setup
repo=${CI_D_REPO:-}
if [ -z "$repo" ]; then
  case $here in
    *?/scripts/r4/ci.d) repo=${here%/scripts/r4/ci.d} ;;
    *) fail setup ;;
  esac
fi
artifacts=${CI_D_ARTIFACTS:-$repo/target/ci-artifacts/$name}
[ -d "$artifacts" ] || mkdir -p "$artifacts" 2>/dev/null || fail setup
page=$repo/ui/tests/r4/page
work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX" 2>/dev/null) || fail setup
mkdir -p "$work/tmp" || fail setup

node_ok() {
  command -v node >/dev/null 2>&1 || return 1
  version=$(node --version 2>/dev/null) || return 1
  major=${version#v}
  major=${major%%.*}
  case $major in
    '' | *[!0123456789]*) return 1 ;;
  esac
  [ "$major" -ge 22 ]
}

inputs() {
  git -C "$repo" status --porcelain -- ui tools/r4-page docs/r4/page-features.txt "scripts/r4/ci.d/$name.sh" >"$artifacts/inputs-$1.txt" 2>/dev/null || return 1
  [ ! -s "$artifacts/inputs-$1.txt" ]
}

tracked_same() {
  git -C "$repo" diff --quiet HEAD -- 2>/dev/null
}

acorn() {
  dir=$artifacts/acorn
  mkdir -p "$dir" || fail setup
  if [ -f "$dir/node_modules/acorn/package.json" ] && cmp -s "$page/package-lock.json" "$dir/package-lock.json"; then
    return 0
  fi
  cp "$page/package.json" "$page/package-lock.json" "$dir/" || fail setup
  (cd "$dir" && npm ci --ignore-scripts --no-audit --no-fund --no-update-notifier --cache "$dir/npm-cache" >"$artifacts/npm.log" 2>&1) || fail acorn
}

browsers() {
  if [ -z "${R4_PAGE_BROWSERS:-}" ] && [ ! -d "$repo/tools/r4-page/.browsers" ]; then
    R4_PAGE_BROWSERS=$artifacts/browsers
    export R4_PAGE_BROWSERS
    sh "$repo/tools/r4-page/fetch.sh" "$R4_PAGE_BROWSERS" >"$artifacts/fetch.log" 2>&1 || fail browser
  fi
}

last_of() {
  sed -n '$p' "$1"
}

node_to() {
  out=$1
  shift
  TMPDIR=$work/tmp node "$@" >"$out" 2>"$out.stderr" &
  child=$!
  wait "$child"
  code=$?
  child=
}

result_of() {
  cat "$1"
  last=$(last_of "$1")
  case $code:$last in
    "0:check result=ok") ;;
    "1:check result="*) fail "${last#check result=}" ;;
    *) fail "$2" ;;
  esac
}

files_step() {
  acorn
  node_to "$artifacts/files.txt" "$page/check.mjs" files --repo "$repo" --acorn "$artifacts/acorn"
  result_of "$artifacts/files.txt" files
}

measure() {
  node_ok || fail node
  git -C "$repo" rev-parse --is-inside-work-tree >/dev/null 2>&1 || fail setup
  inputs before || fail dirty
  files_step
  browsers
  if [ "${CI_D_JOB:-}" = windows ]; then
    commit=$(git -C "$repo" rev-parse HEAD 2>/dev/null) || fail setup
    node_to "$artifacts/page.txt" "$page/check.mjs" push --repo "$repo" --commit "$commit" --own "$own_per_push" --needs "$needs_per_push"
  else
    node_to "$artifacts/page.txt" "$page/check.mjs" whole --repo "$repo" --out "$artifacts/record-fresh.txt"
  fi
  tracked_same || fail dirty
  inputs after || fail dirty
  result_of "$artifacts/page.txt" page
  printf 'PASS %s\n' "$name"
}

record() {
  node_ok || fail node
  git -C "$repo" rev-parse --is-inside-work-tree >/dev/null 2>&1 || fail setup
  inputs before || fail dirty
  browsers
  node_to "$artifacts/record.txt" "$page/check.mjs" record --repo "$repo"
  result_of "$artifacts/record.txt" page
  printf 'PASS %s\n' "$name"
}

fake_node() {
  mkdir -p "$work/$1" || fail setup
  if [ -n "$2" ]; then
    printf '#!/bin/sh\necho %s\n' "$2" >"$work/$1/node" || fail setup
    chmod +x "$work/$1/node" || fail setup
  fi
}

stub_node() {
  mkdir -p "$work/bin-stub" || fail setup
  cat >"$work/bin-stub/node" <<'EOF'
#!/bin/sh
case ${1:-} in
  --version)
    echo v26.0.0
    exit 0
    ;;
esac
case " $* " in
  *" files "*)
    echo 'check result=ok'
    exit 0
    ;;
esac
case ${R4_STUB:-} in
  dirty)
    printf 'x\n' >>"$CI_D_REPO/ui/tests/r4/page/acts.json"
    echo 'check result=ok'
    exit 0
    ;;
  dirty-outside)
    printf 'x\n' >>"$CI_D_REPO/CHANGELOG.md"
    echo 'check result=ok'
    exit 0
    ;;
  whole-record | push-record)
    case "${R4_STUB%-record} $* " in
      "whole "*" whole "* | "push "*" push "*)
        echo 'page record fault=stamp'
        echo 'check result=record'
        exit 1
        ;;
    esac
    echo 'check result=ok'
    exit 0
    ;;
  pass)
    echo 'check result=ok'
    exit 0
    ;;
  hang)
    printf '%s\n' "$$" >"$R4_STUB_PID"
    exec sleep 60
    ;;
esac
exit 3
EOF
  chmod +x "$work/bin-stub/node" || fail setup
}

fixture() {
  fx=$work/$1
  git init -q "$fx" >/dev/null 2>&1 || fail setup
  objects=$(git -C "$repo" rev-parse --path-format=absolute --git-common-dir 2>/dev/null) || fail setup
  printf '%s/objects\n' "$objects" >"$fx/.git/objects/info/alternates" || fail setup
  head=$(git -C "$repo" rev-parse HEAD 2>/dev/null) || fail setup
  git -C "$fx" update-ref HEAD "$head" || fail setup
  git -C "$fx" config core.sparseCheckout true || fail setup
  printf '/ui/tests/r4/page/\n/CHANGELOG.md\n' >"$fx/.git/info/sparse-checkout" || fail setup
  git -C "$fx" read-tree -mu HEAD >/dev/null 2>&1 || fail setup
}

script_case() {
  label=$1
  want=$2
  shift 2
  out=$(CI_D_JOB=$nested "$@" 2>/dev/null)
  code=$?
  last=${out##*
}
  got=exit-$code
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")")
      got=${last#"FAIL $name ("}
      got=FAIL:${got%)}
      ;;
    "1:FAIL self-test:$name ("*")")
      got=${last#"FAIL self-test:$name ("}
      got=FAIL:${got%)}
      ;;
  esac
  line="case $label want=$want got=$got"
  if [ "$got" = "$want" ]; then
    [ "$want" = PASS ] || rejected=$((rejected + 1))
  else
    line="$line wrong"
    wrong=$((wrong + 1))
  fi
  printf '%s\n' "$line"
}

case_line() {
  if [ "$2" = "$3" ]; then
    printf 'case %s want=%s got=%s\n' "$1" "$2" "$3"
    [ "$2" = pass ] || rejected=$((rejected + 1))
  else
    printf 'case %s want=%s got=%s wrong\n' "$1" "$2" "$3"
    wrong=$((wrong + 1))
  fi
}

empty_after() {
  label=$1
  mkdir -p "$work/$2" || fail setup
  t=$work/$2
  shift 2
  env TMPDIR="$t" CI_D_JOB="$nested" "$@" "$shell" "$me" >/dev/null 2>&1
  left=$(find "$t" -mindepth 1 -maxdepth 1 | wc -l | tr -d ' ')
  got=left-$left
  [ "$left" = 0 ] && got=pass
  case_line "$label" pass "$got"
}

signal_case() {
  mkdir -p "$work/sig-tmp" || fail setup
  env TMPDIR="$work/sig-tmp" CI_D_JOB="$nested" R4_STUB=hang R4_STUB_PID="$work/sig.pid" R4_PAGE_BROWSERS="$work" PATH="$work/bin-stub:$PATH" CI_D_REPO="$work/fx-pass" CI_D_ARTIFACTS="$artifacts" "$shell" "$me" >/dev/null 2>&1 &
  script=$!
  n=0
  while [ ! -s "$work/sig.pid" ] && [ "$n" -lt 60 ]; do
    sleep 1
    n=$((n + 1))
  done
  stub=$(cat "$work/sig.pid" 2>/dev/null)
  started=$(date +%s)
  kill -TERM "$script" 2>/dev/null
  wait "$script"
  scode=$?
  took=$(($(date +%s) - started))
  alive=no
  [ -n "$stub" ] && kill -0 "$stub" 2>/dev/null && alive=yes
  left=$(find "$work/sig-tmp" -mindepth 1 -maxdepth 1 | wc -l | tr -d ' ')
  got=exit-$scode-alive-$alive-left-$left-seconds-$took
  if [ -n "$stub" ] && [ "$scode" = 143 ] && [ "$alive" = no ] && [ "$left" = 0 ] && [ "$took" -lt 20 ]; then
    got=caught
  fi
  case_line a-term-ends-the-node-child-and-removes-the-temporary-directory caught "$got"
}

self_test() {
  [ "${CI_D_JOB:-}" = "$nested" ] && fail nested
  node_ok || fail node
  acorn
  browsers
  node_to "$work/page-cases.txt" "$page/check.mjs" selftest --repo "$repo" --acorn "$artifacts/acorn" --work "$work/cases"
  cat "$work/page-cases.txt"
  cp "$work/page-cases.txt.stderr" "$artifacts/selftest-stderr.txt" 2>/dev/null
  last=$(last_of "$work/page-cases.txt")
  case $code:$last in
    "0:self-test rejected="*" wrong=0") ;;
    *) fail cases ;;
  esac
  k=${last#self-test rejected=}
  k=${k%% *}
  counted=$(grep -c ' want=[a-z_]* got=[a-z_]*$' "$work/page-cases.txt")
  passing=$(grep -c ' want=pass got=pass$' "$work/page-cases.txt")
  [ "$((counted - passing))" = "$k" ] || fail count
  rejected=0
  wrong=0
  shell=$(command -v sh) || fail setup
  printf 'x\n' >"$work/blocker" || fail setup
  fake_node bin-none ''
  fake_node bin-21 v21.7.0
  stub_node
  mkdir -p "$work/ok-artifacts" "$work/dirty/ui" || fail setup
  git -C "$work/dirty" init -q >/dev/null 2>&1 || fail setup
  printf 'x\n' >"$work/dirty/ui/untracked.txt" || fail setup
  fixture fx-pass
  fixture fx-dirty
  fixture fx-dirty-outside
  fixture fx-record
  fixture fx-record-windows
  me="$here/$name.sh"
  helped=$("$shell" "$me" --help 2>/dev/null)
  hcode=$?
  case $hcode:$helped in
    "0:usage: "*"Limits, each with its manual step:"*) printf 'case help-names-the-limits want=pass got=pass\n' ;;
    *)
      printf 'case help-names-the-limits want=pass got=exit-%s wrong\n' "$hcode"
      wrong=$((wrong + 1))
      ;;
  esac
  script_case usage-an-unknown-argument FAIL:usage env CI_D_ARTIFACTS="$work/ok-artifacts" "$shell" "$me" --bogus
  script_case usage-a-second-argument FAIL:usage env CI_D_ARTIFACTS="$work/ok-artifacts" "$shell" "$me" --self-test extra
  script_case setup-artifacts-under-a-file FAIL:setup env CI_D_ARTIFACTS="$work/blocker/a" "$shell" "$me"
  script_case setup-no-temporary-directory FAIL:setup env TMPDIR="$work/blocker" "$shell" "$me" --self-test
  script_case a-self-test-inside-the-self-test FAIL:nested env CI_D_ARTIFACTS="$work/ok-artifacts" "$shell" "$me" --self-test
  as0="f=\$1; shift; . \"\$f\""
  script_case setup-no-repository-above-the-script FAIL:setup env CI_D_REPO= CI_D_ARTIFACTS="$work/ok-artifacts" "$shell" -c "$as0" "$work/$name.sh" "$me"
  script_case no-node FAIL:node env PATH="$work/bin-none:/usr/bin:/bin" CI_D_ARTIFACTS="$work/ok-artifacts" "$shell" "$me"
  script_case an-input-that-differs-from-the-commit FAIL:dirty env CI_D_REPO="$work/dirty" CI_D_ARTIFACTS="$work/ok-artifacts" "$shell" "$me"
  script_case node-21 FAIL:node env PATH="$work/bin-21:$PATH" CI_D_ARTIFACTS="$work/ok-artifacts" "$shell" "$me"
  script_case a-run-that-passes-on-a-fixture PASS env R4_STUB=pass R4_PAGE_BROWSERS="$work" PATH="$work/bin-stub:$PATH" CI_D_REPO="$work/fx-pass" CI_D_ARTIFACTS="$artifacts" "$shell" "$me"
  script_case a-run-that-changes-a-tracked-file FAIL:dirty env R4_STUB=dirty R4_PAGE_BROWSERS="$work" PATH="$work/bin-stub:$PATH" CI_D_REPO="$work/fx-dirty" CI_D_ARTIFACTS="$artifacts" "$shell" "$me"
  script_case a-run-that-changes-a-tracked-file-outside-the-inputs FAIL:dirty env R4_STUB=dirty-outside R4_PAGE_BROWSERS="$work" PATH="$work/bin-stub:$PATH" CI_D_REPO="$work/fx-dirty-outside" CI_D_ARTIFACTS="$artifacts" "$shell" "$me"
  script_case a-whole-run-that-fails-on-the-record FAIL:record env R4_STUB=whole-record R4_PAGE_BROWSERS="$work" PATH="$work/bin-stub:$PATH" CI_D_REPO="$work/fx-record" CI_D_ARTIFACTS="$artifacts" "$shell" "$me"
  script_case a-windows-push-that-fails-on-the-record FAIL:record env CI_D_JOB=windows R4_STUB=push-record R4_PAGE_BROWSERS="$work" PATH="$work/bin-stub:$PATH" CI_D_REPO="$work/fx-record-windows" CI_D_ARTIFACTS="$artifacts" "$shell" "$me"
  empty_after a-run-that-passes-leaves-no-temporary-directory pass-tmp R4_STUB=pass R4_PAGE_BROWSERS="$work" PATH="$work/bin-stub:$PATH" CI_D_REPO="$work/fx-pass" CI_D_ARTIFACTS="$artifacts"
  signal_case
  [ "$wrong" -eq 0 ] || fail cases
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$((k + rejected))"
}

"$mode"
