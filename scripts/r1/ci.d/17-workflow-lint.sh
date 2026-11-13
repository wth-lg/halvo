#!/bin/sh
# jobs: macos
# rule: ci.yml is the only workflow file, actionlint at its pinned version accepts it, every action is pinned to a 40-hex commit, also behind a uses key written with escape sequences, the permissions at the top level grant contents: read and nothing else, no job grants more, the jobs are macos and windows, and the timeout of each job is the bound ci_job_timeout_min.
set -u
name=17-workflow-lint
pinned_version=1.7.12
nl='
'

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

have_actionlint() {
  if ! command -v actionlint >/dev/null 2>&1 && [ "${CI:-}" = true ]; then
    HOMEBREW_NO_AUTO_UPDATE=1 HOMEBREW_NO_INSTALL_CLEANUP=1 brew install actionlint >"$CI_D_ARTIFACTS/brew-install.txt" 2>&1
  fi
  command -v actionlint >/dev/null 2>&1
}

finding() {
  findings=$((findings + 1))
  [ -n "$first" ] || first=$1
  printf 'finding: %s\n' "$1"
}

measure() {
  cd "$CI_D_REPO" || exit 2
  if ! have_actionlint; then
    printf 'SKIP %s (actionlint is not installed)\n' "$name"
    exit 77
  fi
  version=$(actionlint -version) || fail "actionlint -version failed"
  version=$(printf '%s\n' "$version" | sed 1q)
  printf 'actionlint version: %s\n' "$version"
  [ "$version" = "$pinned_version" ] ||
    fail "actionlint $version runs here, and the pinned version is $pinned_version: the runner image changed the tool, so read its release notes, then move the pin in scripts/r1/ci.d/17-workflow-lint.sh"
  findings=0
  first=

  file=.github/workflows/ci.yml
  entries=$(ls -A .github/workflows) || fail ".github/workflows cannot be listed"
  [ "$entries" = ci.yml ] ||
    finding ".github/workflows must hold ci.yml and no other file; to add a check, add a script under scripts/<run>/ci.d/, not a workflow file"
  actionlint "$file" || finding "actionlint reports a finding in ci.yml"

  uses_key="(^|[[:space:]{,-])[\"']?uses[\"']?[[:space:]]*:"
  decoded=$CI_D_ARTIFACTS/ci.decoded.yml
  perl -pe 's/\\x([0-9A-Fa-f]{2})/chr(hex($1))/ge; s/\\u([0-9A-Fa-f]{4})/chr(hex($1))/ge; s/\\U([0-9A-Fa-f]{8})/chr(hex($1))/ge' "$file" >"$decoded" ||
    fail "perl could not decode the escape sequences of ci.yml"
  uses=$(grep -o -E "$uses_key" "$decoded" | wc -l | tr -d ' ')
  pinned=$(grep -o -E "${uses_key}[[:space:]]*[\"']?[^ \"',}]+@[0-9a-f]{40}[\"']?([[:space:],}]|\$)" "$decoded" | wc -l | tr -d ' ')
  explicit=$(grep -c -E '^[[:space:]]*(-[[:space:]]+)?[?]([[:space:]]|$)' "$decoded")
  [ "$explicit" -eq 0 ] || finding "ci.yml writes $explicit keys in the explicit form of YAML (?), which this lint does not read; write each key in the plain form key: value"
  [ "$uses" -gt 0 ] || finding "ci.yml holds no uses line"
  [ "$uses" -eq "$pinned" ] ||
    finding "$((uses - pinned)) of $uses uses keys of ci.yml take no 40-hex commit; pin the action to a commit and keep the version in a comment"

  permissions=$(awk -v q="'" '
    BEGIN { key = "^[\"" q "]?permissions[\"" q "]?[[:space:]]*:" }
    in_block && /^[^[:space:]#]/ { in_block = 0 }
    in_block && /^[[:space:]]+[^[:space:]#]/ { grants++; if ($0 != "  contents: read") other = 1; next }
    $0 ~ key { tops++; rest = $0; sub(key, "", rest); if (rest !~ /^[[:space:]]*(#.*)?$/) other = 1; in_block = 1 }
    END { print tops + 0, grants + 0, other + 0 }' "$file")
  read -r tops grants other <<EOF
$permissions
EOF
  job_grants=$(awk -v q="'" '
    BEGIN { key = "^[[:space:]]+[\"" q "]?permissions[\"" q "]?[[:space:]]*:" }
    in_block && match($0, /^[[:space:]]*/) && RLENGTH <= depth && /[^[:space:]]/ { in_block = 0 }
    in_block && /^[[:space:]]+[^[:space:]#]/ { line = $0; sub(/^[[:space:]]+/, "", line); sub(/[[:space:]]*(#.*)?$/, "", line); if (line != "contents: read") bad = 1; next }
    $0 ~ key {
      match($0, /^[[:space:]]*/); depth = RLENGTH
      rest = $0; sub(key, "", rest); sub(/[[:space:]]*(#.*)?$/, "", rest); sub(/^[[:space:]]*/, "", rest)
      if (rest == "") in_block = 1
      else if (rest != "{}" && rest != "{ contents: read }" && rest != "{contents: read}") bad = 1
    }
    END { print bad + 0 }' "$decoded")
  [ "$job_grants" -eq 0 ] || finding "a job of ci.yml sets permissions that grant more than contents: read"
  if [ "$tops" -ne 1 ]; then
    finding "ci.yml must set permissions once at the top level, and it does so $tops times"
  elif [ "$other" -ne 0 ] || [ "$grants" -ne 1 ]; then
    finding "the top-level permissions of ci.yml must grant contents: read and nothing else"
  fi

  [ -f scripts/r1-gate/lib.sh ] || fail "scripts/r1-gate/lib.sh is missing"
  GATE_ROOT=$CI_D_REPO
  export GATE_ROOT
  . scripts/r1-gate/lib.sh
  bound=$(gate_bound ci_job_timeout_min) || bound=
  job_facts=$(awk -v q="'" -v bound="$bound" '
    function finish() {
      if (job == "") return
      jobs++; names = names " " job
      if (job == "macos") macos++; else if (job == "windows") windows++; else others++
      if (timed == 1) good++
      job = ""
    }
    function past(line, i, c,    n, d) {
      for (n = length(line); i <= n; i++) {
        d = substr(line, i, 1)
        if (c == "\"" && d == "\\") i++
        else if (d == q && substr(line, i + 1, 1) == q) i++
        else if (d == c) return i
      }
      return 0
    }
    function scan(line,    i, n, c, nx, start, prev, after, tok, keycol) {
      n = length(line); i = 1; start = 1; prev = " "
      if (quote != "") {
        i = past(line, 1, quote)
        if (!i) return
        quote = ""; start = 0
      }
      for (; i <= n; i++) {
        c = substr(line, i, 1); nx = substr(line, i + 1, 1)
        if (c ~ /[ \t]/) { prev = " "; continue }
        if (c == "#" && prev == " ") return
        prev = c
        if (start && (c == "\"" || c == q)) {
          tok = i; i = past(line, i + 1, c)
          if (!i) { quote = c; return }
          start = 0; after = 1; continue
        }
        if (c == ":" && (after || nx ~ /[ \t]/)) { keycol = tok; start = 1; continue }
        if (start && c == "-" && nx ~ /[ \t]/) { keycol = i; continue }
        if (start && (c == "[" || c == "{")) { depth++; continue }
        if (depth && (c == "]" || c == "}")) { depth--; continue }
        if (depth && c == ",") { start = 1; continue }
        if (start && (c == "|" || c == ">")) { block = keycol; return }
        if (start && (c == "&" || c == "!")) { while (i < n && substr(line, i + 1, 1) !~ /[ \t]/) i++; continue }
        if (start) { start = 0; tok = i }
        after = 0
      }
    }
    BEGIN { key = "^[\"" q "]?jobs[\"" q "]?[[:space:]]*:" }
    { inside = quote != "" }
    block && /[^ \t]/ && match($0, /[^ \t]/) <= block { block = 0 }
    !block { scan($0) }
    inside { next }
    in_jobs && /^[^[:space:]#]/ { finish(); in_jobs = 0 }
    $0 ~ key { blocks++; rest = $0; sub(key, "", rest); if (rest !~ /^[[:space:]]*(#.*)?$/) inline = 1; in_jobs = 1; next }
    in_jobs && /^  [^[:space:]#]/ { finish(); job = $0; sub(/^  /, "", job); sub(/[[:space:]]*:.*$/, "", job); gsub(q, "", job); gsub(/"/, "", job); timed = 0; next }
    in_jobs && job != "" && bound != "" && $0 == "    timeout-minutes: " bound { timed++ }
    END { finish(); print blocks + 0, inline + 0, jobs + 0, good + 0, macos + 0, windows + 0, others + 0, names }' "$file")
  read -r blocks inline jobs timed macos windows others names <<EOF
$job_facts
EOF
  if [ "$blocks" -ne 1 ] || [ "$inline" -ne 0 ]; then
    finding "ci.yml must hold one block of jobs under a jobs key at the top level"
  elif [ "$jobs" -eq 0 ]; then
    finding "ci.yml holds no job key at two spaces under jobs:"
  elif [ "$macos" -ne 1 ] || [ "$windows" -ne 1 ] || [ "$others" -ne 0 ]; then
    finding "the jobs of ci.yml must be exactly macos and windows, and it holds: $names"
  fi
  case $bound in
    '') finding "the bound ci_job_timeout_min cannot be read with gate_bound of scripts/r1-gate/lib.sh" ;;
    *[!0-9]*) finding "the bound ci_job_timeout_min is no number" ;;
    *)
      [ "$jobs" -eq "$timed" ] ||
        finding "$timed of $jobs jobs of ci.yml set timeout-minutes: $bound, the bound ci_job_timeout_min"
      ;;
  esac

  printf 'jobs: %s, uses keys: %s, findings: %s\n' "$jobs" "$uses" "$findings"
  [ "$findings" -le 1 ] || first="$first; and $((findings - 1)) more above"
  [ "$findings" -eq 0 ] || fail "$first"
  printf 'PASS %s\n' "$name"
}

check() {
  out=$(PATH=$path CI=$ci CI_D_REPO=$work/$1 CI_D_ARTIFACTS=$work/artifacts "$shell" "$0" 2>&1)
  code=$?
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    "77:SKIP $name ("*")") got=SKIP ;;
    *) got=exit-$code ;;
  esac
  case $out in
    *"${3:-}"*) ;;
    *) got=$got-without-the-text ;;
  esac
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$got"
  if [ "$got" != "$2" ]; then
    wrong="$wrong $1"
  elif [ "$2" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
  STUB_BOUND=30
  path=$PATH
  ci=
}

tree() {
  repo=$work/$1
  mkdir -p "$repo/.github/workflows" "$repo/scripts/r1-gate" || exit 2
  sed "$2" "$work/ci.yml" >"$repo/.github/workflows/ci.yml"
  cp "$work/lib.sh" "$repo/scripts/r1-gate/lib.sh"
}

self_test() {
  if ! have_actionlint; then
    printf 'FAIL self-test:%s (actionlint is not installed)\n' "$name"
    exit 1
  fi
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  mkdir -p "$work/artifacts" "$work/no-tools" "$work/tools" "$work/old-tools"
  shell=$(command -v sh) || exit 2
  STUB_REAL=$(command -v actionlint) || exit 2
  STUB_TOOLS=$work/tools
  STUB_BOUND=30
  export STUB_BOUND STUB_REAL STUB_TOOLS
  cat >"$work/tools/brew" <<'EOF'
#!/bin/sh
[ "$*" = 'install actionlint' ] || exit 1
printf '#!/bin/sh\nexec "%s" "$@"\n' "$STUB_REAL" >"$STUB_TOOLS/actionlint"
chmod +x "$STUB_TOOLS/actionlint"
EOF
  chmod +x "$work/tools/brew"
  cat >"$work/lib.sh" <<'EOF'
gate_bound() {
  [ "${GATE_ROOT:-}" = "$CI_D_REPO" ] || return 1
  [ "$1" = ci_job_timeout_min ] && printf '%s\n' "$STUB_BOUND"
}
EOF
  cat >"$work/ci.yml" <<'EOF'
name: ci
on: push
permissions:
  contents: read
jobs:
  macos:
    runs-on: macos-15
    timeout-minutes: 30
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
  windows:
    runs-on: windows-2025
    timeout-minutes: 30
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
EOF
  rejected=0
  wrong=
  STUB_BOUND=30
  path=$PATH
  ci=

  tree pinned-and-timed ''
  check pinned-and-timed PASS 'jobs: 2, uses keys: 2, findings: 0'
  tree action-on-a-tag '10s/@[0-9a-f]* # v7.0.1/@v7/'
  check action-on-a-tag FAIL '1 of 2 uses keys of ci.yml take no 40-hex commit'
  tree action-on-a-short-commit '15s/@[0-9a-f]\{33\}/@/'
  check action-on-a-short-commit FAIL '1 of 2 uses keys of ci.yml take no 40-hex commit'
  tree action-on-41-hex '15s/ # v7.0.1/a # v7.0.1/'
  check action-on-41-hex FAIL '1 of 2 uses keys of ci.yml take no 40-hex commit'
  tree named-step-on-a-tag "10s/.*/      - name: Checkout\\${nl}        uses: actions\\/checkout@v7/"
  check named-step-on-a-tag FAIL '1 of 2 uses keys of ci.yml take no 40-hex commit'
  tree no-uses-line '10s/.*/      - run: "true"/;15s/.*/      - run: "true"/'
  check no-uses-line FAIL 'FAIL 17-workflow-lint (ci.yml holds no uses line)'
  tree second-workflow-file ''
  cp "$work/ci.yml" "$work/second-workflow-file/.github/workflows/package.yml"
  check second-workflow-file FAIL 'add a script under scripts/<run>/ci.d/, not a workflow file)'
  tree hidden-second-workflow-file ''
  cp "$work/ci.yml" "$work/hidden-second-workflow-file/.github/workflows/.package.yml"
  check hidden-second-workflow-file FAIL 'add a script under scripts/<run>/ci.d/, not a workflow file)'
  tree no-permissions '3,4d'
  check no-permissions FAIL 'ci.yml must set permissions once at the top level, and it does so 0 times)'
  tree permissions-at-job-level-only "3,4d;7s/\$/\\${nl}    permissions: {}/"
  check permissions-at-job-level-only FAIL 'ci.yml must set permissions once at the top level, and it does so 0 times)'
  tree job-without-timeout '13d'
  check job-without-timeout FAIL '1 of 2 jobs of ci.yml set timeout-minutes: 30, the bound ci_job_timeout_min)'
  tree step-timeout-in-place-of-the-job-timeout "13d;15s/\$/\\${nl}        timeout-minutes: 30/"
  check step-timeout-in-place-of-the-job-timeout FAIL '1 of 2 jobs of ci.yml set timeout-minutes: 30, the bound ci_job_timeout_min)'
  tree job-keys-at-another-indent '6,15s/^/  /;8d;13d'
  check job-keys-at-another-indent FAIL 'FAIL 17-workflow-lint (ci.yml holds no job key at two spaces under jobs:)'
  tree timeout-above-the-bound '8s/30/31/'
  check timeout-above-the-bound FAIL '1 of 2 jobs of ci.yml set timeout-minutes: 30, the bound ci_job_timeout_min)'
  tree timeout-ten-times-the-bound '8s/30/300/'
  check timeout-ten-times-the-bound FAIL '1 of 2 jobs of ci.yml set timeout-minutes: 30, the bound ci_job_timeout_min)'
  tree bound-moved ''
  STUB_BOUND=45
  check bound-moved FAIL '0 of 2 jobs of ci.yml set timeout-minutes: 45, the bound ci_job_timeout_min)'
  tree bound-is-no-number ''
  STUB_BOUND=30m
  check bound-is-no-number FAIL 'the bound ci_job_timeout_min is no number)'
  tree bound-zero '8s/30/0/;13s/30/0/'
  STUB_BOUND=0
  check bound-zero FAIL 'actionlint reports a finding in ci.yml)'
  tree bound-unreadable ''
  printf 'gate_bound() {\n  return 1\n}\n' >"$work/bound-unreadable/scripts/r1-gate/lib.sh"
  check bound-unreadable FAIL 'the bound ci_job_timeout_min cannot be read with gate_bound of scripts/r1-gate/lib.sh)'
  tree actionlint-finding '2s/push/pushed/'
  check actionlint-finding FAIL 'actionlint reports a finding in ci.yml)'
  tree two-findings '10s/@[0-9a-f]* # v7.0.1/@v7/;3,4d'
  check two-findings FAIL '1 of 2 uses keys of ci.yml take no 40-hex commit; pin the action to a commit and keep the version in a comment; and 1 more above)'
  tree quoted-uses-key '10s/.*/      - "uses": actions\/checkout@v7/'
  check quoted-uses-key FAIL '1 of 2 uses keys of ci.yml take no 40-hex commit'
  tree flow-steps-with-an-unpinned-action '9s/.*/    steps: [{uses: actions\/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1}, {uses: actions\/setup-node@v4}]/;10d'
  check flow-steps-with-an-unpinned-action FAIL '1 of 3 uses keys of ci.yml take no 40-hex commit'
  tree runs-on-block-list-without-timeout "12s/.*/    runs-on:\\${nl}      - windows-2025/;13d"
  check runs-on-block-list-without-timeout FAIL '1 of 2 jobs of ci.yml set timeout-minutes: 30, the bound ci_job_timeout_min)'
  tree third-job "\$s/\$/\\${nl}  linux:\\${nl}    runs-on: ubuntu-24.04\\${nl}    timeout-minutes: 30\\${nl}    steps:\\${nl}      - run: \"true\"/"
  check third-job FAIL 'the jobs of ci.yml must be exactly macos and windows, and it holds: macos windows linux)'
  tree permissions-write-all '3s/.*/permissions: write-all/;4d'
  check permissions-write-all FAIL 'the top-level permissions of ci.yml must grant contents: read and nothing else)'
  tree permissions-contents-write '4s/read/write/'
  check permissions-contents-write FAIL 'the top-level permissions of ci.yml must grant contents: read and nothing else)'
  tree permissions-read-and-more "4s/\$/\\${nl}  actions: write/"
  check permissions-read-and-more FAIL 'the top-level permissions of ci.yml must grant contents: read and nothing else)'
  tree uses-key-with-an-escape '10s/.*/      - "u\\x73es": actions\/checkout@v7/'
  check uses-key-with-an-escape FAIL '1 of 2 uses keys of ci.yml take no 40-hex commit'
  tree uses-key-with-a-unicode-escape '10s/.*/      - "\\u0075ses": actions\/checkout@v7/'
  check uses-key-with-a-unicode-escape FAIL '1 of 2 uses keys of ci.yml take no 40-hex commit'
  tree job-permissions-write-all "7s/\$/\\${nl}    permissions: write-all/"
  check job-permissions-write-all FAIL 'a job of ci.yml sets permissions that grant more than contents: read'
  tree job-permissions-contents-write "7s/\$/\\${nl}    permissions:\\${nl}      contents: write/"
  check job-permissions-contents-write FAIL 'a job of ci.yml sets permissions that grant more than contents: read'
  tree job-permissions-contents-read "7s/\$/\\${nl}    permissions:\\${nl}      contents: read/"
  check job-permissions-contents-read PASS 'jobs: 2, uses keys: 2, findings: 0'
  tree job-permissions-empty "7s/\$/\\${nl}    permissions: {}/"
  check job-permissions-empty PASS 'jobs: 2, uses keys: 2, findings: 0'
  tree explicit-uses-key "10s/.*/      - ? uses\\${nl}        : actions\/checkout@v4/"
  check explicit-uses-key FAIL 'keys in the explicit form of YAML'
  tree explicit-job-permissions "7s/\$/\\${nl}    ? permissions\\${nl}    : write-all/"
  check explicit-job-permissions FAIL 'keys in the explicit form of YAML'
  tree timeout-in-a-double-quoted-name "8s/.*/    name: \"macos\\${nl}    timeout-minutes: 30\\${nl}    \"/"
  check timeout-in-a-double-quoted-name FAIL '1 of 2 jobs of ci.yml set timeout-minutes: 30, the bound ci_job_timeout_min)'
  tree timeout-in-a-single-quoted-name "8s/.*/    name: 'macos\\${nl}    timeout-minutes: 30\\${nl}    '/"
  check timeout-in-a-single-quoted-name FAIL '1 of 2 jobs of ci.yml set timeout-minutes: 30, the bound ci_job_timeout_min)'
  tree longer-timeout-beside-a-quoted-name "8s/30/600/;8s/\$/\\${nl}    name: \"macos\\${nl}    timeout-minutes: 30\\${nl}    \"/"
  check longer-timeout-beside-a-quoted-name FAIL '1 of 2 jobs of ci.yml set timeout-minutes: 30, the bound ci_job_timeout_min)'
  tree timeout-after-an-escaped-double-quote "8s/.*/    name: \"say \\\\\"hi\\${nl}    timeout-minutes: 30\\${nl}    \"/"
  check timeout-after-an-escaped-double-quote FAIL '1 of 2 jobs of ci.yml set timeout-minutes: 30, the bound ci_job_timeout_min)'
  tree timeout-after-a-doubled-single-quote "8s/.*/    name: 'it''s\\${nl}    timeout-minutes: 30\\${nl}    '/"
  check timeout-after-a-doubled-single-quote FAIL '1 of 2 jobs of ci.yml set timeout-minutes: 30, the bound ci_job_timeout_min)'
  tree timeout-after-a-hash-in-a-quoted-name "8s/.*/    name: \"macos #\\${nl}    timeout-minutes: 30\\${nl}    \"/"
  check timeout-after-a-hash-in-a-quoted-name FAIL '1 of 2 jobs of ci.yml set timeout-minutes: 30, the bound ci_job_timeout_min)'
  tree timeout-in-a-tagged-quoted-name "8s/.*/    name: \&n !!str \"macos\\${nl}    timeout-minutes: 30\\${nl}    \"/;13s/\$/\\${nl}    name: *n/"
  check timeout-in-a-tagged-quoted-name FAIL '1 of 2 jobs of ci.yml set timeout-minutes: 30, the bound ci_job_timeout_min)'
  tree timeout-in-a-quoted-flow-value "8s/.*/    env: { A: a#b, NAME: \"macos\\${nl}    timeout-minutes: 30\\${nl}    \" }/"
  check timeout-in-a-quoted-flow-value FAIL '1 of 2 jobs of ci.yml set timeout-minutes: 30, the bound ci_job_timeout_min)'
  tree timeout-in-a-json-like-flow-value "8s/.*/    env: {\"NAME\":\"macos\\${nl}    timeout-minutes: 30\\${nl}    \"}/"
  check timeout-in-a-json-like-flow-value FAIL '1 of 2 jobs of ci.yml set timeout-minutes: 30, the bound ci_job_timeout_min)'
  tree timeout-in-a-quoted-item-of-a-flow-list-over-lines "8s/.*/    strategy:\\${nl}      matrix:\\${nl}        os: [a,\\${nl}      b, \"c\\${nl}    timeout-minutes: 30\\${nl}    \"]/"
  check timeout-in-a-quoted-item-of-a-flow-list-over-lines FAIL '1 of 2 jobs of ci.yml set timeout-minutes: 30, the bound ci_job_timeout_min)'
  tree timeout-in-a-quoted-step-value "8d;10s/.*/      - run: \"true\\${nl}    timeout-minutes: 30\\${nl}      \"/"
  check timeout-in-a-quoted-step-value FAIL '1 of 2 jobs of ci.yml set timeout-minutes: 30, the bound ci_job_timeout_min)'
  tree timeout-in-a-step-name-after-a-run-block "8d;10s/.*/      - run: |\\${nl}          true\\${nl}        name: \"step\\${nl}    timeout-minutes: 30\\${nl}        \"/"
  check timeout-in-a-step-name-after-a-run-block FAIL '1 of 2 jobs of ci.yml set timeout-minutes: 30, the bound ci_job_timeout_min)'
  tree job-in-a-quoted-name "8s/\$/\\${nl}    name: \"macos\\${nl}  windows:\\${nl}    runs-on: windows-2025\\${nl}    timeout-minutes: 30\\${nl}    \"/;11,15d"
  check job-in-a-quoted-name FAIL 'the jobs of ci.yml must be exactly macos and windows, and it holds: macos)'
  tree quotes-that-close-or-do-not-open "7s/\$/\\${nl}    name: \"macos \\\\\" # job\"\\${nl}    permissions: {}\\${nl}    env:\\${nl}      A: -\"x\\${nl}      B: it's, \"y # c: 'z\\${nl}      C: a[b, \"y\\${nl}      E: a], \"z\\${nl}      \"D\": a:\"b/;12s/\$/\\${nl}    name: 'windows\\\\' # y/"
  check quotes-that-close-or-do-not-open PASS 'jobs: 2, uses keys: 2, findings: 0'
  tree quoted-names-over-two-lines-before-the-timeout "7s/\$/\\${nl}    name: \"macos\\${nl}      job\"/;12s/\$/\\${nl}    name: 'windows\\${nl}      job'/"
  check quoted-names-over-two-lines-before-the-timeout PASS 'jobs: 2, uses keys: 2, findings: 0'
  tree run-block-with-an-open-quote "10s/\$/\\${nl}      - run: |\\${nl}\\${nl}          printf '%s' \"x: 'y\"\\${nl}      - name: >-\\${nl}          step: 'one\\${nl}        run: echo/"
  check run-block-with-an-open-quote PASS 'jobs: 2, uses keys: 2, findings: 0'
  tree another-actionlint-version ''
  printf '#!/bin/sh\ncase "%s1" in -version) printf "1.6.0\\n" ;; *) exec "%s" "%s@" ;; esac\n' "\$" "$STUB_REAL" "\$" >"$work/old-tools/actionlint"
  chmod +x "$work/old-tools/actionlint"
  path=$work/old-tools:$PATH
  check another-actionlint-version FAIL 'actionlint 1.6.0 runs here, and the pinned version is 1.7.12'
  tree actionlint-missing ''
  path=$work/no-tools
  check actionlint-missing SKIP 'SKIP 17-workflow-lint (actionlint is not installed)'
  tree actionlint-installed-in-ci ''
  path=$work/tools:/usr/bin:/bin
  ci=true
  check actionlint-installed-in-ci PASS 'jobs: 2, uses keys: 2, findings: 0'

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  '') measure ;;
  *) exit 64 ;;
esac
