#!/bin/sh
# jobs: macos windows
# rule: the three page suites of the agents page (ui/tests/r4/agents/test.mjs, ui/tests/r4/confirm/test.mjs and ui/tests/r4/needs/needs.test.mjs) and the page test under the app's own policy (ui/tests/page/policy.test.mjs) each run at least one test and pass, headless, in the browser that tools/r4-page/browser.txt names for the runner, which tools/r4-page/fetch.sh gets with its packages by the pins of tools/r4-page/files.tsv and tools/r4-page/package-lock.json
# lane: alone
set -u
name=30-r4-page-suites
suites='agents=ui/tests/r4/agents/test.mjs confirm=ui/tests/r4/confirm/test.mjs needs=ui/tests/r4/needs/needs.test.mjs policy=ui/tests/page/policy.test.mjs'

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

# The path in the form that node reads on each system.
native() {
  if command -v cygpath >/dev/null 2>&1; then
    cygpath -w "$1"
  else
    printf '%s\n' "$1"
  fi
}

# Prints "<tests> <failed>" from the last line of a suite's standard output, its count line
# (`agents tests passed=<p> failed=<f> states=<n>`, `confirm tests passed=<p> failed=<f>`,
# `needs tests=<t> passed=<p> states=<n>`, `policy tests passed=<p> failed=<f> browser=<b>`); exits 1
# when that line is no count line of the suite.
counts() {
  awk -v suite="$1" '
    { sub(/\r$/, ""); last = $0 }
    END {
      n = split(last, w, " ")
      if (n < 2 || w[1] != suite || (w[2] != "tests" && w[2] !~ /^tests=[0-9]+$/)) exit 1
      t = -1; p = -1; f = -1
      for (i = 2; i <= n; i++) {
        if (w[i] ~ /^tests=[0-9]+$/) t = substr(w[i], 7) + 0
        else if (w[i] ~ /^passed=[0-9]+$/) p = substr(w[i], 8) + 0
        else if (w[i] ~ /^failed=[0-9]+$/) f = substr(w[i], 8) + 0
      }
      if (p < 0) exit 1
      if (t < 0) { if (f < 0) exit 1; t = p + f }
      else if (f < 0) f = t - p
      print t, f
    }' "$2"
}

# Runs one suite: $1 its label, $2 its file, $3 the output directory, $4 the browsers directory,
# $5 its TMPDIR and $6 the same directory in the form that node reads; writes "<exit> <seconds>"
# into $3/$1.status.
run_one() {
  start=$(date +%s)
  R4_PAGE_BROWSERS=$4 TMPDIR=$5 TEMP=$6 TMP=$6 node "$2" </dev/null >"$3/$1.out" 2>"$3/$1.err"
  status=$?
  end=$(date +%s)
  printf '%s %s\n' "$status" "$((end - start))" >"$3/$1.status"
}

# Runs the check: $1 is the fetch program, run by sh with the browsers directory, or the word
# `kept` to use R4_PAGE_BROWSERS as it is; $2 the suites, `label=file` with blanks between them;
# $3 the directory for the outputs. Ends with the PASS line or the FAIL line.
run_check() {
  fetcher=$1
  list=$2
  out=$3
  work=$(mktemp -d "${TMPDIR:-/tmp}/r4-page-suites.XXXXXX") || fail "mktemp failed"
  mkdir "$work/tmp" || fail "mkdir failed"
  if [ "$fetcher" = kept ]; then
    browsers=$R4_PAGE_BROWSERS
    printf 'fetch: not run, R4_PAGE_BROWSERS names the browsers directory outside CI\n'
  else
    browsers=$(native "$work/browsers")
    start=$(date +%s)
    sh "$fetcher" "$browsers" >"$out/fetch.out" 2>"$out/fetch.err" </dev/null
    status=$?
    end=$(date +%s)
    line=$(sed -n '$p' "$out/fetch.out")
    case $status:$line in
      '0:fetch browser='*) printf '%s seconds=%s\n' "$line" "$((end - start))" ;;
      *)
        why=$line
        [ -n "$why" ] || why=$(sed -n '$p' "$out/fetch.err")
        fail "the page tool cannot get its files: exit $status, ${why:-no line}"
        ;;
    esac
  fi
  tmp=$(native "$work/tmp")
  # The suites run at the same time, each with its own server on a free port and its own browser.
  for item in $list; do
    run_one "${item%%=*}" "${item#*=}" "$out" "$browsers" "$work/tmp" "$tmp" &
  done
  wait
  bad=
  for item in $list; do
    label=${item%%=*}
    read -r status seconds <"$out/$label.status" || {
      status=-
      seconds=-
    }
    if c=$(counts "$label" "$out/$label.out"); then
      tests=${c% *}
      failed=${c#* }
    else
      tests=
      failed=
    fi
    printf 'suite %s tests=%s failed=%s exit=%s seconds=%s\n' "$label" "${tests:--}" "${failed:--}" "$status" "$seconds"
    if [ -z "$tests" ]; then
      why="$label printed no count line, exit $status"
    elif [ "$tests" -eq 0 ]; then
      why="$label ran no test, exit $status"
    elif [ "$failed" -ne 0 ]; then
      why="$label: $failed of $tests tests failed, exit $status"
    elif [ "$status" != 0 ]; then
      why="$label exited $status after $tests tests passed"
    else
      continue
    fi
    awk -v suite="$label" '
      $1 == suite && $2 ~ /^test=/ && $3 == "fail" { print "  failed: " substr($2, 6); n++ }
      $1 == "not" && $2 == "ok" { print "  failed: " $3; n++ }
      n == 20 { exit }' "$out/$label.out"
    bad="${bad:+$bad; }$why"
  done
  [ -z "$bad" ] || fail "$bad"
  printf 'PASS %s\n' "$name"
}

self_test() {
  st=$(mktemp -d "${TMPDIR:-/tmp}/r4-page-suites-st.XXXXXX") || {
    printf 'FAIL self-test:%s (mktemp failed)\n' "$name"
    exit 1
  }
  for f in ui/tests/r4/agents/test.mjs ui/tests/r4/confirm/test.mjs ui/tests/r4/needs/needs.test.mjs ui/tests/page/policy.test.mjs tools/r4-page/fetch.sh; do
    [ -f "$f" ] || {
      printf 'FAIL self-test:%s (%s is missing)\n' "$name" "$f"
      exit 1
    }
  done
  # Stand-in fetch programs: each records its call, then prints and exits as its name says.
  cat >"$st/fetch-ok.sh" <<'EOF'
echo called >>"$(dirname "$0")/calls"
printf 'fetch browser=webkit build=webkit-0 system=stand-in bytes=1 archive=downloaded extracted=yes package=installed\n'
EOF
  printf 'printf "fetch failed step=download status=404\\n"\nexit 1\n' >"$st/fetch-failed.sh"
  printf 'printf "REFUSED hash: a stand-in archive has another SHA-256\\n" >&2\nexit 2\n' >"$st/fetch-refused.sh"
  printf 'printf "something else\\n"\n' >"$st/fetch-no-line.sh"
  # Stand-in suites: each records that it ran, then prints its lines and exits with its status.
  suite() {
    {
      printf "import { appendFileSync } from 'node:fs';\n"
      printf "appendFileSync(process.argv[1] + '.ran', 'ran\\\\n');\n"
      printf '%s\n' "$2"
    } >"$st/$1.mjs"
  }
  suite agents-pass "console.log('agents test=a ok'); console.log('agents tests passed=2 failed=0 states=3'); process.exit(0);"
  suite confirm-pass "console.log('confirm test=a ok'); console.log('confirm tests passed=1 failed=0'); process.exit(0);"
  suite needs-pass "console.log('ok lint'); console.log('needs tests=4 passed=4 states=29'); process.exit(0);"
  suite agents-failed "console.log('agents test=b fail detail=1'); console.log('agents tests passed=1 failed=1 states=3'); process.exit(1);"
  suite needs-failed "console.log('not ok render x'); console.log('needs tests=4 passed=3 states=29'); process.exit(1);"
  suite needs-none "console.log('needs tests=0 passed=0 states=29'); process.exit(0);"
  suite agents-none "console.log('agents tests passed=0 failed=0 states=0'); process.exit(0);"
  suite confirm-silent "process.exit(0);"
  suite confirm-count-says-failed "console.log('confirm tests passed=1 failed=1'); process.exit(0);"
  suite confirm-exit-says-failed "console.log('confirm tests passed=1 failed=0'); process.exit(1);"
  suite agents-threw "throw new Error('x');"
  suite needs-refused "process.stderr.write('REFUSED browser: x\\n'); process.exit(2);"
  suite needs-line-before-last "console.log('needs tests=4 passed=4 states=29'); console.log('one more line'); process.exit(0);"
  rejected=0
  n=0
  # fixture <fetch> <agents> <confirm> <needs> <last line wanted> <all: each stand-in suite ran | none: none ran>
  fixture() {
    n=$((n + 1))
    d=$st/case-$n
    mkdir "$d" "$d/out" || exit 1
    for f in "$st"/*.ran "$st/calls"; do
      [ -e "$f" ] && mv "$f" "$d/" 2>/dev/null
    done
    fetcher=$st/$1.sh
    [ "$1" = kept ] && fetcher=kept
    (R4_PAGE_BROWSERS=$st/kept-browsers TMPDIR=$d run_check "$fetcher" "agents=$st/$2.mjs confirm=$st/$3.mjs needs=$st/$4.mjs" "$d/out") >"$d/stdout" 2>"$d/stderr"
    status=$?
    last=$(sed -n '$p' "$d/stdout")
    if [ "$last" != "$5" ]; then
      printf 'FAIL self-test:%s (case %s: last line %s, wanted %s)\n' "$name" "$n" "$last" "$5"
      exit 1
    fi
    case $5 in
      PASS*) want=0 ;;
      *) want=1 ;;
    esac
    if [ "$status" != "$want" ]; then
      printf 'FAIL self-test:%s (case %s: exit %s, wanted %s)\n' "$name" "$n" "$status" "$want"
      exit 1
    fi
    if [ "${6:-}" = all ]; then
      for s in "$2" "$3" "$4"; do
        [ -e "$st/$s.mjs.ran" ] || {
          printf 'FAIL self-test:%s (case %s: the stand-in %s did not run)\n' "$name" "$n" "$s"
          exit 1
        }
      done
    elif [ "${6:-}" = none ]; then
      for s in "$2" "$3" "$4"; do
        [ ! -e "$st/$s.mjs.ran" ] || {
          printf 'FAIL self-test:%s (case %s: the stand-in %s ran after a failed fetch)\n' "$name" "$n" "$s"
          exit 1
        }
      done
    fi
    [ "$want" = 1 ] && rejected=$((rejected + 1))
    printf 'case %s: %s\n' "$n" "$5"
  }
  fixture fetch-ok agents-pass confirm-pass needs-pass "PASS $name" all
  [ -e "$st/calls" ] || {
    printf 'FAIL self-test:%s (the passing case called no fetch)\n' "$name"
    exit 1
  }
  fixture kept agents-pass confirm-pass needs-pass "PASS $name" all
  [ ! -e "$st/calls" ] || {
    printf 'FAIL self-test:%s (the case with kept browsers called the fetch)\n' "$name"
    exit 1
  }
  fixture fetch-ok agents-failed confirm-pass needs-pass "FAIL $name (agents: 1 of 2 tests failed, exit 1)" all
  fixture fetch-ok agents-pass confirm-pass needs-failed "FAIL $name (needs: 1 of 4 tests failed, exit 1)" all
  fixture fetch-ok agents-pass confirm-pass needs-none "FAIL $name (needs ran no test, exit 0)" all
  fixture fetch-ok agents-none confirm-pass needs-pass "FAIL $name (agents ran no test, exit 0)" all
  fixture fetch-ok agents-pass confirm-silent needs-pass "FAIL $name (confirm printed no count line, exit 0)" all
  fixture fetch-ok agents-pass confirm-count-says-failed needs-pass "FAIL $name (confirm: 1 of 2 tests failed, exit 0)" all
  fixture fetch-ok agents-pass confirm-exit-says-failed needs-pass "FAIL $name (confirm exited 1 after 1 tests passed)" all
  fixture fetch-ok agents-threw confirm-pass needs-refused "FAIL $name (agents printed no count line, exit 1; needs printed no count line, exit 2)" all
  fixture fetch-ok agents-pass confirm-pass needs-line-before-last "FAIL $name (needs printed no count line, exit 0)" all
  fixture fetch-failed agents-pass confirm-pass needs-pass "FAIL $name (the page tool cannot get its files: exit 1, fetch failed step=download status=404)" none
  fixture fetch-refused agents-pass confirm-pass needs-pass "FAIL $name (the page tool cannot get its files: exit 2, REFUSED hash: a stand-in archive has another SHA-256)" none
  fixture fetch-no-line agents-pass confirm-pass needs-pass "FAIL $name (the page tool cannot get its files: exit 0, something else)" none
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test)
    self_test
    exit 0
    ;;
  '') ;;
  *)
    printf 'usage: sh scripts/r4/ci.d/30-r4-page-suites.sh [--self-test]\n' >&2
    exit 64
    ;;
esac

out=${CI_D_ARTIFACTS:-}
if [ -z "$out" ]; then
  out=$(mktemp -d "${TMPDIR:-/tmp}/r4-page-suites-out.XXXXXX") || fail "mktemp failed"
fi
if [ "${CI:-}" != true ] && [ -n "${R4_PAGE_BROWSERS:-}" ]; then
  run_check kept "$suites" "$out"
else
  run_check tools/r4-page/fetch.sh "$suites" "$out"
fi
