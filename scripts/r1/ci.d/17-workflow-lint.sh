#!/bin/sh
# jobs: macos
# rule: ci.yml is the only workflow file, actionlint at its pinned version accepts it, every action is pinned to a 40-hex commit, also behind a uses key written with escape sequences, the permissions at the top level grant contents: read and nothing else, no job grants more, the jobs are macos and windows, the timeout of each job is the bound ci_job_timeout_min, and ci.yml holds no byte other than the line feed and printable ASCII, no explicit key (? and a blank), no document marker or directive, no node that begins with &, * or ! (an anchor, an alias or a tag; a plain line that begins with one of the three reads so too), and no line that the lint reads as ending inside a quoted scalar or a flow collection (a block header alone on the next line, and a plain line that begins with a quote or a bracket, read so too). Push ignores landing branches at any depth and accepts every tag; pull_request and the pinned schedule are the other events. The pin of cache-1 reads one form and refuses every other at its first line, quoting it and naming the line to write: one line on: alone in column 1, under it the line '  schedule:' alone with the pinned cron line below it and no other line under it and no other line of the block that names the schedule or a cron line; under jobs: each job's key is '  macos:' or '  windows:' alone, a job's own keys are runs-on, timeout-minutes and steps at four blanks, none begins a block scalar, '    steps:' alone is the last of them, and each step begins at six blanks, a dash and a blank; each job holds one Rust cache step that is exactly the four pinned lines, and the letters rust-cache stand on no other line. A step condition must be always() and a step must have no continue-on-error key. Review each changed step command in the diff; this pin does not read what a step runs. The push filters are pin_branches and pin_tags. The schedule and cache lines are pin_cron, pin_uses, pin_with, pin_prefix and pin_save below; on the event schedule they make the step find no saved Rust build cache and save none.
set -u
name=17-workflow-lint
pinned_version=1.7.12
nl='
'
pin_branches="    branches-ignore: ['land-*', 'land-*/**']"
pin_tags="    tags: ['**']"
pin_cron="    - cron: '17 3 * * 0'"
pin_uses="      - uses: Swatinem/rust-cache@d1527c8f67ef9af357290475d4af4b0fab8f12e0 # v2.9.3"
pin_with="        with:"
pin_prefix="          prefix-key: \${{ github.event_name == 'schedule' && 'v0-rust-cold' || 'v0-rust' }}"
pin_save="          save-if: \${{ github.ref == 'refs/heads/main' && github.event_name != 'schedule' }}"

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
  byte=$(perl -e 'open(my $f, "<:raw", $ARGV[0]) or exit 2; local $/; my $t = <$f>; exit 0 unless defined $t && $t =~ /[^\x0A\x20-\x7E]/; my ($at, $b) = ($-[0], ord($&)); my $head = substr($t, 0, $at); printf("%d %02X\n", 1 + ($head =~ tr/\n//), $b)' "$file") ||
    fail "perl could not read the bytes of ci.yml"
  [ -z "$byte" ] ||
    finding "ci.yml line ${byte% *} holds the byte 0x${byte#* }, and this lint takes the line feed and printable ASCII (0x20 to 0x7E) alone; end each line with a line feed alone, write blanks for a tab, and write ASCII text (in a double-quoted value, an escape such as \\u00e9 for a character outside ASCII)"
  document=$(awk '/^(---|\.\.\.)( |$)/ || /^%/ { print NR; exit }' "$file")
  [ -z "$document" ] ||
    finding "ci.yml line $document begins a YAML document or a directive (---, ... or % in column 1), and this lint reads one document alone; write one document with no marker and no directive"
  span=$(awk -v q="'" '
    function past(line, i, c,    n, d) {
      for (n = length(line); i <= n; i++) {
        d = substr(line, i, 1)
        if (c == "\"" && d == "\\") i++
        else if (d == c) return i
      }
      return 0
    }
    function scan(line,    i, n, c, nx, start, prev, after, first, dash, depth) {
      n = length(line); start = 1; prev = " "
      for (i = 1; i <= n; i++) {
        c = substr(line, i, 1); nx = substr(line, i + 1, 1)
        if (c == " ") { prev = " "; continue }
        if (c == "#" && prev == " ") break
        prev = c
        if (start && c == "-" && nx == " ") { dash = i; continue }
        if (start && c == "?" && (nx == "" || nx == " ")) return "explicit"
        if (start && (c == "&" || c == "*" || c == "!")) return "sign"
        if (start && (c == "|" || c == ">")) { block = first ? first : dash ? dash : i; continue }
        if (!first) first = i
        if (start && (c == "\"" || c == q)) {
          i = past(line, i + 1, c)
          if (!i) return "quote"
          after = 1; continue
        }
        if (c == ":" && (after || nx == " ")) { start = 1; continue }
        if (start && (c == "[" || c == "{")) { depth++; continue }
        if (depth && (c == "]" || c == "}")) { depth--; continue }
        if (depth && c == ",") { start = 1; continue }
        start = 0
        after = 0
      }
      return depth ? "flow" : ""
    }
    block && /[^ ]/ && match($0, /[^ ]/) <= block { block = 0 }
    !block && (kind = scan($0)) != "" { print kind, NR; exit }' "$file")
  case $span in
    quote*) finding "ci.yml line ${span#quote } has a quote that this lint reads as running past the line's end; write the value on one line, or as a block scalar with | or > on the key's line and its text indented below the key (a block header alone on the next line, and a plain value continued onto a line that starts with a quote, read so too)" ;;
    flow*) finding "ci.yml line ${span#flow } has a [ or { that this lint reads as running past the line's end; write it on one line, or in block form with one item or key on each line (a plain value continued onto a line that starts with [ or { reads so too)" ;;
    explicit*) finding "ci.yml line ${span#explicit } starts a key with ? and a blank, one of the keys in the explicit form of YAML, which this lint does not read; write the key in the plain form key: value" ;;
    sign*) finding "ci.yml line ${span#sign } starts a node with &, * or !, which this lint reads as an anchor, an alias or a tag and does not read; take an anchor, an alias or a tag out and write the value itself, and put a value that begins with &, * or ! as text in quotes, as in '!cancelled()' (a plain value continued onto a line that starts with &, * or ! reads so too: write it on one line, in quotes, or as a block scalar with | or > on the key's line)" ;;
  esac
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
    BEGIN { key = "^[\"" q "]?jobs[\"" q "]?[[:space:]]*:" }
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

  pins=$(LC_ALL=C PIN_BRANCHES=$pin_branches PIN_TAGS=$pin_tags PIN_CRON=$pin_cron PIN_USES=$pin_uses PIN_WITH=$pin_with PIN_PREFIX=$pin_prefix PIN_SAVE=$pin_save awk -v q="'" '
    function decode(s,    out, h, n, k) {
      out = ""
      while (match(s, /\\(x[0-9A-Fa-f][0-9A-Fa-f]|u[0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f]|U[0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f])/)) {
        h = substr(s, RSTART + 2, RLENGTH - 2)
        n = 0
        for (k = 1; k <= length(h); k++) n = n * 16 + index("0123456789abcdef", tolower(substr(h, k, 1))) - 1
        out = out substr(s, 1, RSTART - 1) ((n >= 32 && n < 127) ? sprintf("%c", n) : "?")
        s = substr(s, RSTART + RLENGTH)
      }
      return tolower(out s)
    }
    function indent(s) { match(s, /^ */); return RLENGTH }
    function shown(s) { return q s q }
    function word(c) { return c == "" ? "the line" q "s end" : c == " " ? "a blank" : "the sign " c }
    function differ(found, want,    k, a, b) {
      for (k = 1; k <= length(found) || k <= length(want); k++) {
        a = substr(found, k, 1)
        b = substr(want, k, 1)
        if (a != b) return "it differs from the line to write first at column " k ", which holds " word(a) " where the line to write holds " word(b)
      }
      return ""
    }
    function held(v) { return "; the pin holds the " pinned[v] " in the variable " v " at the top of scripts/r1/ci.d/17-workflow-lint.sh, and a change of it is a ruled change" }
    function add(n, text) {
      nf++
      at[nf] = n
      tx[nf] = file (n ? " line " n : "") ": " text
    }
    function bad() {
      jbad[jid] = 1
      quiet = 1
      cstate = 0
    }
    function endjob() {
      if (job != "" && !jbad[jid]) {
        if (!stepsseen) add(jline, "the job " job " has no line " shown("    steps:") "; this lint reads the steps of a job under that line alone, as the job" q "s last own key; write it")
        else if (!nc[jid] && !e17[jid]) add(jline, "the job " job " holds no Rust cache step; this lint reads it as the four lines " shown(PIN_USES) ", " shown(PIN_WITH) ", " shown(PIN_PREFIX) " and " shown(PIN_SAVE) ", one after another in the steps; write them; the pin holds these lines in the variables pin_uses, pin_with, pin_prefix and pin_save at the top of scripts/r1/ci.d/17-workflow-lint.sh, and a change of one is a ruled change")
        else if (nc[jid] > 1) add(cl[jid, 2], "the job " job " holds a second Rust cache step, beside the one at line " cl[jid, 1] "; this lint reads one Rust cache step in a job; take one out")
      }
      job = ""
      quiet = 0
      cstate = 0
    }
    function endon() {
      if (sec == "on" && !onbad && !sched && !schedother) add(onstart, "the block on: holds no line " shown("  schedule:") "; this lint reads the schedule as that line alone and under it the line " shown(PIN_CRON) "; write both lines under " shown("on:") held("pin_cron"))
    }
    function checkname() {
      if (low !~ /rust-cache/ || firstofstep || quiet) return
      add(NR, shown(line) " names the action rust-cache; this lint reads the action" q "s name on the first line of a job" q "s Rust cache step alone, the line " shown(PIN_USES) "; take the name out of this line" held("pin_uses"))
      if (job != "") e17[jid] = 1
    }
    function col1() {
      endjob()
      endon()
      sec = ""
      quiet = 0
      if (line == "on:") {
        ons++
        if (ons > 1) add(NR, "a second line " shown("on:") "; this lint reads one block on:, the one at line " onstart "; write the events of this block under that one")
        else { sec = "on"; onstart = NR; onfirst = 1 }
        return
      }
      if (low ~ ("^[\"" q "]?on[\"" q "]? *:")) {
        add(NR, shown(line) " holds the key on in another form; this lint reads the key on as the line " shown("on:") " alone in column 1, with no quotes, capital letters, escapes, blank before the colon, value or comment; write " shown("on:"))
        onother = 1
        return
      }
      if (line == "jobs:") { sec = "jobs"; jfirstall = 1; return }
      if (low ~ ("^[\"" q "]?jobs[\"" q "]? *:")) {
        add(NR, shown(line) " holds the key jobs in another form; this lint reads the jobs under the line " shown("jobs:") " alone in column 1; write " shown("jobs:"))
        sec = "jobsbad"
        quiet = 1
      }
    }
    function onl() {
      if (onbad || blank) return
      if (comment) {
        if (low ~ /schedule|cron/) {
          add(NR, shown(line) " names the schedule or a cron line in the block on:; this lint reads the schedule as the line " shown("  schedule:") " alone and under it the one line " shown(PIN_CRON) "; take this line out" held("pin_cron"))
          schedother = 1
        }
        return
      }
      if (onfirst) {
        onfirst = 0
        if (i != 2) {
          add(NR, shown(line) " is the first line under " shown("on:") " and stands at " i " blanks; this lint reads the events of on: at two blanks; write each event at two blanks")
          onbad = 1
          quiet = 1
          return
        }
      }
      if (aftercron) {
        aftercron = 0
        if (i > 2) {
          add(NR, shown(line) " stands under the schedule below its cron line; this lint reads the cron line and nothing else under " shown("  schedule:") "; take it out" held("pin_cron"))
          return
        }
      }
      if (line == "  schedule:") {
        sched++
        if (sched > 1) add(NR, "a second line " shown("  schedule:") "; this lint reads one, the one at line " schedat "; take this one out")
        else { schedat = NR; wantcron = NR + 1 }
        return
      }
      if (low ~ /schedule|cron/) {
        add(NR, shown(line) " names the schedule or a cron line in the block on:; this lint reads the schedule as the line " shown("  schedule:") " alone and under it the one line " shown(PIN_CRON) "; write it so or take this line out" held("pin_cron"))
        schedother = 1
      }
    }
    function jobl() {
      if (blank || comment) return
      if (jfirstall) {
        jfirstall = 0
        if (i != 2) {
          add(NR, shown(line) " is the first line under " shown("jobs:") " and stands at " i " blanks; this lint reads a job" q "s key at two blanks, the line " shown("  macos:") " or " shown("  windows:") "; write the jobs so")
          sec = "jobsbad"
          quiet = 1
          return
        }
      }
      if (i == 2) {
        endjob()
        jid++
        if (line == "  macos:" || line == "  windows:") {
          job = substr(line, 3, length(line) - 3)
          jline = NR
          jfirst = 1
          insteps = 0
          stepsseen = 0
          return
        }
        job = ""
        quiet = 1
        if (line ~ /^  [A-Za-z0-9_-]+:$/) return
        add(NR, shown(line) " stands at two blanks under " shown("jobs:") ", where this lint reads a job" q "s key; it reads that key as the line " shown("  macos:") " or " shown("  windows:") " alone, with no comment, no quotes and no blank before the colon; write one of those lines")
        return
      }
      if (job == "" || jbad[jid]) return
      if (i == 1 || i == 3 || i == 5 || (jfirst && i != 4)) {
        if (jfirst) add(NR, shown(line) " is the first line of the job " job " and stands at " i " blanks; this lint reads a job" q "s own keys at four blanks; write them at four blanks")
        else add(NR, shown(line) " stands at " i " blanks in the job " job "; this lint reads a job" q "s own keys at four blanks and its steps at six; write the line at one of those")
        bad()
        return
      }
      jfirst = 0
      if (i == 4) {
        if (insteps) {
          add(NR, shown(line) " stands at four blanks after " shown("    steps:") " in the job " job "; this lint reads " shown("    steps:") " as the job" q "s last own key and each step at six blanks, a dash and a blank; write the steps so and the job" q "s other keys above " shown("    steps:"))
          bad()
          return
        }
        if (line !~ /^    [A-Za-z0-9_-]+:( |$)/) {
          add(NR, shown(line) " stands at four blanks in the job " job ", where this lint reads an own key of the job; it reads such a key as a plain word of letters, digits, - and _, a colon, and then the line" q "s end or a blank and the value; write the key so")
          bad()
          return
        }
        if (line ~ /^    steps:/) {
          if (line != "    steps:") {
            add(NR, shown(line) " is the steps key of the job " job " with something behind its colon; this lint reads the steps as the line " shown("    steps:") " alone and below it each step at six blanks, a dash and a blank; write them so")
            bad()
            return
          }
          insteps = 1
          stepsfirst = 1
          stepsseen = 1
          return
        }
        v = line
        sub(/^    [A-Za-z0-9_-]+: */, "", v)
        if (v ~ /^[|>]/) {
          add(NR, shown(line) " is an own key of the job " job " whose value begins a block scalar; this lint reads no block scalar on a job" q "s own key; write the value on its line")
          bad()
        }
        return
      }
      if (!insteps) return
      if (stepsfirst && i != 6) {
        add(NR, shown(line) " is the first line in the steps of the job " job " and stands at " i " blanks; this lint reads a step as beginning at six blanks, a dash and a blank; write the step so")
        bad()
        return
      }
      stepsfirst = 0
      if (i != 6) return
      if (line ~ /^      - *$/) {
        add(NR, shown(line) " is a dash alone in the steps of the job " job "; this lint reads a step as beginning at six blanks, a dash, a blank and the step" q "s first key; write that key on the dash" q "s line")
        bad()
        return
      }
      if (line !~ /^      - /) {
        add(NR, shown(line) " stands at six blanks in the steps of the job " job " and is no step" q "s first line; this lint reads a line at six blanks there as the first line of a step, with a dash and a blank; write it so")
        bad()
        return
      }
      firstofstep = 1
      if (low !~ /rust-cache/) return
      code = line
      sub(/ #.*$/, "", code)
      if (decode(code) !~ /rust-cache/) {
        add(NR, shown(line) " is the first line of a step and names the action rust-cache in its comment; this lint reads the action" q "s name on the first line of a job" q "s Rust cache step alone, the line " shown(PIN_USES) ", and in no comment; take the name out of the comment" held("pin_uses"))
        if (job != "") e17[jid] = 1
        return
      }
      nc[jid]++
      cl[jid, nc[jid]] = NR
      if (line == PIN_USES) {
        cstate = 1
        cstart = NR
      } else add(NR, shown(line) " stands where this lint reads line 1 of the Rust cache step of the job " job ", " shown(PIN_USES) "; " differ(line, PIN_USES) "; write " shown(PIN_USES) held("pin_uses"))
    }
    BEGIN {
      file = ".github/workflows/ci.yml"
      want[1] = ENVIRON["PIN_USES"]; var[1] = "pin_uses"
      want[2] = ENVIRON["PIN_WITH"]; var[2] = "pin_with"
      want[3] = ENVIRON["PIN_PREFIX"]; var[3] = "pin_prefix"
      want[4] = ENVIRON["PIN_SAVE"]; var[4] = "pin_save"
      PIN_CRON = ENVIRON["PIN_CRON"]
      on_line[++on_count] = "  push:"
      on_line[++on_count] = ENVIRON["PIN_BRANCHES"]
      on_line[++on_count] = ENVIRON["PIN_TAGS"]
      on_line[++on_count] = "  pull_request:"
      on_line[++on_count] = "  schedule:"
      on_line[++on_count] = PIN_CRON
      pinned["pin_cron"] = "cron line"
      pinned["pin_uses"] = "uses line of the Rust cache step"
      pinned["pin_with"] = "with line of the Rust cache step"
      pinned["pin_prefix"] = "prefix-key line"
      pinned["pin_save"] = "save-if line"
      PIN_USES = want[1]; PIN_WITH = want[2]; PIN_PREFIX = want[3]; PIN_SAVE = want[4]
    }
    {
      line = $0
      low = decode(line)
      i = indent(line)
      blank = (line ~ /^ *$/)
      comment = (line ~ /^ *#/)
      firstofstep = 0
      if (sec == "on" && (i || blank || comment)) {
        row = NR - onstart
        if (row in on_line) {
          onread = row
          if (line != on_line[row] && !onchange) onchange = NR
        } else if (line != "" && i && !onchange) onchange = NR
      }
      if (cstate == 1) {
        k = NR - cstart + 1
        if (line == want[k]) {
          if (k == 4) cstate = 2
          next
        }
        add(NR, shown(line) " stands where this lint reads line " k " of the Rust cache step of the job " job ", " shown(want[k]) "; " differ(line, want[k]) "; write " shown(want[k]) held(var[k]))
        cstate = 0
        if (blank || comment || line !~ /^[^ ]/ && i != 2 && line !~ /^      - /) { checkname(); next }
      } else if (cstate == 2 && !blank && !comment) {
        cstate = 0
        if (line !~ /^[^ ]/ && i != 2 && line !~ /^      - /) {
          add(NR, shown(line) " follows the four lines of the Rust cache step of the job " job "; this lint reads that step as those four lines alone, so the next line begins a step, is a job" q "s key or stands in column 1; take it out or begin a step there")
          checkname()
          next
        }
      }
      if (NR == wantcron) {
        wantcron = 0
        if (line == PIN_CRON) { aftercron = 1; next }
        add(NR, shown(line) " stands under " shown("  schedule:") ", where this lint reads the cron line; " differ(line, PIN_CRON) "; write " shown(PIN_CRON) held("pin_cron"))
        if (blank || comment || line !~ /^[^ ]/) { checkname(); next }
      }
      if (!blank && !comment && line ~ /^[^ ]/) col1()
      else if (sec == "on") onl()
      else if (sec == "jobs") jobl()
      checkname()
    }
    END {
      if (cstate == 1) add(cstart, "the Rust cache step of the job " job " ends with the file before its line " (NR - cstart + 2) "; write after its last line the line " shown(want[NR - cstart + 2]) held(var[NR - cstart + 2]))
      if (wantcron) add(wantcron - 1, "the file ends where this lint reads the cron line under " shown("  schedule:") "; write " shown(PIN_CRON) held("pin_cron"))
      endjob()
      endon()
      if (!ons && !onother) add(0, "no line of the file is " shown("on:") " alone in column 1; this lint reads the events under that line, with the line " shown("  schedule:") " and under it the line " shown(PIN_CRON) "; write them" held("pin_cron"))
      if (!nf && (onchange || onread != on_count))
        add(onchange ? onchange : onstart, "write the push filters and the pull_request and schedule events exactly as pinned, in order, under " shown("on:") ": under " shown("  push:") " write " shown(ENVIRON["PIN_BRANCHES"]) " and " shown(ENVIRON["PIN_TAGS"]) "; write " shown("  pull_request:") " without filters and keep the weekly schedule")
      for (a = 2; a <= nf; a++)
        for (b = a; b > 1 && at[b - 1] > at[b]; b--) {
          t = at[b]; at[b] = at[b - 1]; at[b - 1] = t
          t = tx[b]; tx[b] = tx[b - 1]; tx[b - 1] = t
        }
      for (a = 1; a <= nf; a++) print tx[a]
    }' "$file") || fail "awk could not read ci.yml for the pin of the schedule and the Rust cache step"
  while IFS= read -r pin; do
    [ -z "$pin" ] || finding "$pin"
  done <<EOF
$pins
EOF

  if [ "$findings" -eq 0 ]; then
    cache_action=${pin_uses#*uses: }
    policy=$(ruby -rpsych - "$file" "$bound" "${cache_action%% *}" "${pin_prefix#*: }" "${pin_save#*: }" <<'RUBY'
      def decode(node)
        case node
        when Psych::Nodes::Scalar
          node.value
        when Psych::Nodes::Sequence
          node.children.map { |child| decode(child) }
        when Psych::Nodes::Mapping
          node.children.each_slice(2).each_with_object({}) do |(key, value), fields|
            abort "workflow mapping key must be scalar" unless key.is_a?(Psych::Nodes::Scalar)
            abort "duplicate workflow key #{key.value.inspect}" if fields.key?(key.value)
            fields[key.value] = decode(value)
          end
        else
          abort "unsupported workflow YAML node #{node.class}"
        end
      end

      def display(value)
        case value
        when :command then "a run command"
        when :absent then "absent"
        else value.inspect
        end
      end

      def compare(path, actual, expected)
        return if expected == :command && actual.is_a?(String)
        return if actual == expected
        if actual.is_a?(Hash) && (expected.is_a?(Hash) || expected == :absent)
          fields = expected == :absent ? {} : expected
          puts "#{path} must be absent, read {}" if actual.empty? && expected == :absent
          (actual.keys | fields.keys).each do |key|
            compare("#{path}.#{key}", actual.fetch(key, :absent), fields.fetch(key, :absent))
          end
        else
          puts "#{path} must be #{display(expected)}, read #{display(actual)}"
        end
      end

      workflow = decode(Psych.parse_file(ARGV.fetch(0)).root)
      workflow_keys = %w[name on permissions concurrency jobs]
      unless workflow.keys.sort == workflow_keys.sort
        puts "workflow keys must be exactly #{workflow_keys.join(", ")}"
      end
      puts "workflow name must be ci" unless workflow["name"] == "ci"
      concurrency = {
        "group" => "ci-${{ github.event_name }}-${{ github.event_name == 'pull_request' && github.ref || github.sha }}",
        "cancel-in-progress" => "${{ github.event_name == 'pull_request' }}"
      }
      compare("workflow concurrency", workflow.fetch("concurrency", :absent), concurrency)
      compare("workflow permissions", workflow.fetch("permissions", :absent), {"contents" => "read"})

      runners = {"macos" => "macos-15", "windows" => "windows-2025"}
      checkout = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1"
      node = "actions/setup-node@949feb2413d6458794dcd2491c4babbbce0c15c1"
      cache = ARGV.fetch(2)
      upload = "actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9"
      steps = {
        "macos" => [
          checkout, "Commit messages", "Self-tests that commit in throwaway repositories", node,
          "Prerequisites", "Changelog", "Line endings", "Own-source reads", cache, "ci.d self-test",
          "Processor", "ci.d", "Lockfile unchanged", upload
        ],
        "windows" => [
          checkout, node, "Prerequisites", cache, "ci.d self-test", "Processor", "ci.d",
          "Scripts and hook in a core.autocrlf=true clone", "Lockfile unchanged", upload
        ]
      }
      conditions = {upload => "always()"}
      identity = {
        "GIT_AUTHOR_NAME" => "Halvo CI", "GIT_AUTHOR_EMAIL" => "ci@halvo.invalid",
        "GIT_COMMITTER_NAME" => "Halvo CI", "GIT_COMMITTER_EMAIL" => "ci@halvo.invalid"
      }
      environments = {
        "Commit messages" => {
          "DEFAULT_BRANCH" => "${{ github.event.repository.default_branch }}",
          "BEFORE" => "${{ github.event.before }}",
          "AFTER" => "${{ github.event.pull_request.head.sha || github.sha }}",
          "PR_TITLE" => "${{ github.event.pull_request.title }}",
          "PR_BODY" => "${{ github.event.pull_request.body }}"
        },
        "Self-tests that commit in throwaway repositories" => identity,
        "ci.d self-test" => identity
      }

      workflow.fetch("jobs").each do |job, fields|
        allowed = %w[runs-on timeout-minutes steps]
        unless fields.keys.sort == allowed.sort
          puts "job #{job} keys: use only runs-on, timeout-minutes and steps"
        end
        compare("job #{job} runs-on", fields.fetch("runs-on", :absent), runners.fetch(job))
        compare("job #{job} timeout-minutes", fields.fetch("timeout-minutes", :absent), ARGV.fetch(1))
        inputs = {
          checkout => {"fetch-depth" => job == "macos" ? "0" : "2"},
          node => {"node-version" => "26", "cache" => "npm"},
          cache => {"prefix-key" => ARGV.fetch(3), "save-if" => ARGV.fetch(4)},
          upload => {"name" => "ci-artifacts-#{job}", "path" => "target/ci-artifacts/"}
        }
        roster = steps.fetch(job)
        actual = fields.fetch("steps")
        unless actual.map { |step| step["uses"] || step["name"] } == roster
          puts "job #{job} steps must list the pinned actions and named commands in order"
        end
        actual.each_with_index do |step, index|
          id = roster[index]
          expected = if inputs.key?(id)
            {"uses" => id, "with" => inputs.fetch(id)}
          elsif id
            {"name" => id, "shell" => "bash", "run" => :command}
          else
            {}
          end
          expected["env"] = environments.fetch(id) if environments.key?(id)
          expected["if"] = conditions.fetch(id) if conditions.key?(id)
          if step.key?("if") && step["if"] != expected["if"]
            puts "job #{job} step #{index + 1}: use only #{conditions.values.uniq.join(" or ")} as a step condition on its listed steps"
          end
          if step.key?("continue-on-error")
            puts "job #{job} step #{index + 1}: take continue-on-error out of the step"
          end
          compare("job #{job} step #{index + 1}", step, expected)
        end
      end
RUBY
    ) || fail "could not read the workflow policy with Ruby's YAML parser"
    while IFS= read -r violation; do
      [ -z "$violation" ] || finding "$violation"
    done <<EOF
$policy
EOF
  fi

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
    *) [ "$got" != "$2" ] || got=$got-without-the-text ;;
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
  perl -pi -e 's/\@CR\@/\r/g; s/\@LS\@/\xE2\x80\xA8/g; s/\@NEL\@/\xC2\x85/g; s/\@PS\@/\xE2\x80\xA9/g; s/\@TAB\@/\t/g; s/\@FF\@/\xFF/g; s/\@NUL\@/\x00/g; s/\@BOM\@/\xEF\xBB\xBF/g; s/\@EACUTE\@/\xC3\xA9/g' "$repo/.github/workflows/ci.yml"
  cp "$work/lib.sh" "$repo/scripts/r1-gate/lib.sh"
}

lines() {
  repo=$work/$1
  shift
  mkdir -p "$repo/.github/workflows" "$repo/scripts/r1-gate" || exit 2
  cp "$work/ci.yml" "$repo/.github/workflows/ci.yml"
  while [ "$#" -ge 2 ]; do
    LINE_TEXT=$2 awk -v n="$1" 'NR == n { if (ENVIRON["LINE_TEXT"] != "-") print ENVIRON["LINE_TEXT"]; next } { print }' \
      "$repo/.github/workflows/ci.yml" >"$repo/.github/workflows/ci.yml.next" || exit 2
    mv "$repo/.github/workflows/ci.yml.next" "$repo/.github/workflows/ci.yml" || exit 2
    shift 2
  done
  cp "$work/lib.sh" "$repo/scripts/r1-gate/lib.sh"
}

full() {
  repo=$work/$1
  mkdir -p "$repo/.github/workflows" "$repo/scripts/r1-gate" || exit 2
  ruby - "$work/full.yml" "$2" "${3:-}" "${4:-}" >"$repo/.github/workflows/ci.yml" <<'RUBY' || exit 2
source, job, before, after = ARGV
parts = File.read(source).split(/^(?=  (?:macos|windows):$)/)
part = job == "workflow" ? parts.first : parts.find { |text| text.start_with?("  #{job}:\n") }
abort "missing fixture job #{job}" unless part
unless before.empty?
  abort "expected one occurrence in #{job}: #{before.inspect}" unless part.scan(before).size == 1
  part.sub!(before) { after }
end
print parts.join
RUBY
  mkdir -p "$repo/docs/baseline" || exit 2
  cp "$CI_D_REPO/scripts/r1-gate/lib.sh" "$repo/scripts/r1-gate/lib.sh" || exit 2
  cp "$CI_D_REPO/docs/baseline/r1-gate-bounds.txt" "$repo/docs/baseline/" || exit 2
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
  checkout='      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1'
  printf '%s\n' 'on:' '  push:' "    branches-ignore: ['land-*', 'land-*/**']" "    tags: ['**']" '  pull_request:' '  schedule:' "$pin_cron" 'jobs:' \
    '  macos:' '    runs-on: macos-15' '    timeout-minutes: 30' '    steps:' "$checkout" "$pin_uses" "$pin_with" "$pin_prefix" "$pin_save" \
    '  windows:' '    runs-on: windows-2025' '    timeout-minutes: 30' '    steps:' "$checkout" "$pin_uses" "$pin_with" "$pin_prefix" "$pin_save" \
    'permissions:' '  contents: read' 'name: ci' \
    >"$work/ci.yml"
  cp "$CI_D_REPO/.github/workflows/ci.yml" "$work/full.yml" || exit 2
  rejected=0
  wrong=
  STUB_BOUND=30
  path=$PATH
  ci=

  full top-display-name workflow 'name: ci' 'name: CI checks'
  check top-display-name FAIL 'workflow name'
  full top-name-absent workflow "name: ci${nl}" ''
  check top-name-absent FAIL 'workflow keys'
  full top-run-name workflow 'name: ci' "name: ci${nl}run-name: checks"
  check top-run-name FAIL 'workflow keys'
  full top-default-shell workflow 'name: ci' "name: ci${nl}defaults: {run: {shell: bash}}"
  check top-default-shell FAIL 'workflow keys'
  full top-env workflow 'name: ci' "name: ci${nl}env: {TEST_LABEL: plain}"
  check top-env FAIL 'workflow keys'
  full top-concurrency-absent workflow "concurrency:${nl}  group: ci-\${{ github.event_name }}-\${{ github.event_name == 'pull_request' && github.ref || github.sha }}${nl}  cancel-in-progress: \${{ github.event_name == 'pull_request' }}${nl}" ''
  check top-concurrency-absent FAIL 'workflow keys'
  full concurrency-other-group workflow "  group: ci-\${{ github.event_name }}-\${{ github.event_name == 'pull_request' && github.ref || github.sha }}" '  group: ci-fixture'
  check concurrency-other-group FAIL 'workflow concurrency'
  full concurrency-cancel-false workflow "  cancel-in-progress: \${{ github.event_name == 'pull_request' }}" '  cancel-in-progress: false'
  check concurrency-cancel-false FAIL 'workflow concurrency'
  full concurrency-no-cancel workflow "  cancel-in-progress: \${{ github.event_name == 'pull_request' }}" ''
  check concurrency-no-cancel FAIL 'workflow concurrency'
  full concurrency-scalar workflow "concurrency:${nl}  group: ci-\${{ github.event_name }}-\${{ github.event_name == 'pull_request' && github.ref || github.sha }}${nl}  cancel-in-progress: \${{ github.event_name == 'pull_request' }}${nl}" "concurrency: ci-fixture${nl}"
  check concurrency-scalar FAIL 'workflow concurrency'

  full quoted-step-command macos 'run: sh scripts/prerequisites.sh' 'run: "sh scripts/prerequisites.sh"'
  check quoted-step-command PASS 'jobs: 2, uses keys: 8, findings: 0'
  full apostrophe-in-command macos 'run: sh scripts/prerequisites.sh' "run: echo \"it's\""
  check apostrophe-in-command PASS 'jobs: 2, uses keys: 8, findings: 0'
  full dash-in-command macos 'run: sh scripts/prerequisites.sh' "run: echo '-\"x'"
  check dash-in-command PASS 'jobs: 2, uses keys: 8, findings: 0'
  full comma-and-comment-in-command macos 'run: sh scripts/prerequisites.sh' "run: echo 'a, \"y' # c: 'z"
  check comma-and-comment-in-command PASS 'jobs: 2, uses keys: 8, findings: 0'
  full open-bracket-in-command macos 'run: sh scripts/prerequisites.sh' "run: echo 'a[b, \"y'"
  check open-bracket-in-command PASS 'jobs: 2, uses keys: 8, findings: 0'
  full closing-bracket-in-command macos 'run: sh scripts/prerequisites.sh' "run: echo 'a], \"z'"
  check closing-bracket-in-command PASS 'jobs: 2, uses keys: 8, findings: 0'
  full quoted-run-key-and-colon macos 'run: sh scripts/prerequisites.sh' "\"run\": echo 'a:\"b'"
  check quoted-run-key-and-colon PASS 'jobs: 2, uses keys: 8, findings: 0'
  full folded-command-with-an-open-quote macos 'run: sh scripts/prerequisites.sh' "run: >-${nl}          printf '%s' \"step: 'one\""
  check folded-command-with-an-open-quote PASS 'jobs: 2, uses keys: 8, findings: 0'
  full question-mark-in-command macos 'run: sh scripts/prerequisites.sh' 'run: echo is it ? yes'
  check question-mark-in-command PASS 'jobs: 2, uses keys: 8, findings: 0'
  full signs-in-command macos 'run: sh scripts/prerequisites.sh' "run: echo 'a && b * c!'"
  check signs-in-command PASS 'jobs: 2, uses keys: 8, findings: 0'
  full flow-env-and-comma-in-command windows "env:${nl}          GIT_AUTHOR_NAME: Halvo CI${nl}          GIT_AUTHOR_EMAIL: ci@halvo.invalid${nl}          GIT_COMMITTER_NAME: Halvo CI${nl}          GIT_COMMITTER_EMAIL: ci@halvo.invalid${nl}        run: sh scripts/ci/run-ci-d.sh windows --self-test" "env: {GIT_AUTHOR_NAME: Halvo CI, GIT_AUTHOR_EMAIL: ci@halvo.invalid, GIT_COMMITTER_NAME: Halvo CI, GIT_COMMITTER_EMAIL: ci@halvo.invalid}${nl}        run: echo a, b"
  check flow-env-and-comma-in-command PASS 'jobs: 2, uses keys: 8, findings: 0'
  full on-add-workflow-dispatch workflow '  pull_request:' "  pull_request:${nl}  workflow_dispatch:"
  check on-add-workflow-dispatch FAIL 'write the push filters and the pull_request and schedule events'
  full on-comment-naming-dispatch workflow '  pull_request:' "  # workflow_dispatch${nl}  pull_request:"
  check on-comment-naming-dispatch FAIL 'write the push filters and the pull_request and schedule events'
  full on-push-filters-reversed workflow "$pin_branches${nl}$pin_tags" "$pin_tags${nl}$pin_branches"
  check on-push-filters-reversed FAIL 'write the push filters and the pull_request and schedule events'
  full on-extra-blank-line workflow '  pull_request:' "${nl}  pull_request:"
  check on-extra-blank-line FAIL 'write the push filters and the pull_request and schedule events'
  full on-comment-after-cron workflow "$pin_cron" "$pin_cron${nl}    # weekly"
  check on-comment-after-cron FAIL 'write the push filters and the pull_request and schedule events'
  for minutes in 90 5 expression; do
    value=$minutes
    [ "$minutes" != expression ] || value='${{ 90 }}'
    full "step-timeout-$minutes" macos "      - name: ci.d${nl}        shell: bash" \
      "      - name: ci.d${nl}        shell: bash${nl}        timeout-minutes: $value"
    check "step-timeout-$minutes" FAIL 'step 12.timeout-minutes'
  done

  full runner-expression-macos macos '    runs-on: macos-15' "    runs-on: \${{ github.event_name == 'push' && 'macos-15' || 'macos-15' }}"
  check runner-expression-macos FAIL 'job macos runs-on'
  full runner-other-macos macos '    runs-on: macos-15' '    runs-on: ubuntu-24.04'
  check runner-other-macos FAIL 'job macos runs-on'
  full runner-expression-windows windows '    runs-on: windows-2025' "    runs-on: \${{ github.event_name == 'push' && 'windows-2025' || 'windows-2025' }}"
  check runner-expression-windows FAIL 'job windows runs-on'
  full runner-other-windows windows '    runs-on: windows-2025' '    runs-on: ubuntu-24.04'
  check runner-other-windows FAIL 'job windows runs-on'
  full step-shell-template macos "      - name: ci.d${nl}        shell: bash" "      - name: ci.d${nl}        shell: bash -e {0}"
  check step-shell-template FAIL 'step 12.shell'
  full step-shell-missing macos "      - name: ci.d${nl}        shell: bash" '      - name: ci.d'
  check step-shell-missing FAIL 'step 12.shell'
  full step-env-ci-d macos "      - name: ci.d${nl}        shell: bash" "      - name: ci.d${nl}        shell: bash${nl}        env: {FIXTURE_LABEL: plain}"
  check step-env-ci-d FAIL 'step 12.env.FIXTURE_LABEL'
  full step-quoted-working-directory macos "      - name: ci.d${nl}        shell: bash" "      - name: ci.d${nl}        shell: bash${nl}        \"working-directory\": ."
  check step-quoted-working-directory FAIL 'step 12.working-directory'
  full step-escaped-working-directory macos "      - name: ci.d${nl}        shell: bash" "      - name: ci.d${nl}        shell: bash${nl}        \"working-\\u0064irectory\": ."
  check step-escaped-working-directory FAIL 'step 12.working-directory'
  full step-unknown-id macos "      - name: ci.d${nl}        shell: bash" "      - name: ci.d${nl}        shell: bash${nl}        id: build_checks"
  check step-unknown-id FAIL 'step 12.id'
  full step-timeout macos "      - name: ci.d${nl}        shell: bash" "      - name: ci.d${nl}        shell: bash${nl}        timeout-minutes: 120"
  check step-timeout FAIL 'step 12.timeout-minutes'
  full step-condition-misplaced macos "      - name: ci.d${nl}        shell: bash" "      - name: ci.d${nl}        shell: bash${nl}        if: always()"
  check step-condition-misplaced FAIL 'step 12.if'
  full step-continue-false macos "      - name: ci.d${nl}        shell: bash" "      - name: ci.d${nl}        shell: bash${nl}        continue-on-error: false"
  check step-continue-false FAIL 'take continue-on-error out of the step'
  full step-anchored-directory macos "      - name: ci.d${nl}        shell: bash" "      - name: ci.d${nl}        shell: bash${nl}        working-directory: &directory ."
  check step-anchored-directory FAIL 'starts a node with &, * or !'
  full step-merged-env macos "      - name: ci.d${nl}        shell: bash" "      - name: ci.d${nl}        shell: bash${nl}        env: {<<: {FIXTURE_LABEL: plain}}"
  check step-merged-env FAIL
  full flow-step-extra-key windows "      - name: Prerequisites${nl}        shell: bash${nl}        run: sh scripts/prerequisites.sh" '      - {name: Prerequisites, shell: bash, id: verify, run: sh scripts/prerequisites.sh}'
  check flow-step-extra-key FAIL 'step 3.id'
  full job-disallows-container macos '    runs-on: macos-15' "    runs-on: macos-15${nl}    container: alpine"
  check job-disallows-container FAIL
  full job-disallows-services macos '    runs-on: macos-15' "    runs-on: macos-15${nl}    services: {data: {image: redis}}"
  check job-disallows-services FAIL
  full job-disallows-strategy macos '    runs-on: macos-15' "    runs-on: macos-15${nl}    strategy: {max-parallel: 1}"
  check job-disallows-strategy FAIL
  full job-disallows-env macos '    runs-on: macos-15' "    runs-on: macos-15${nl}    env: {FIXTURE_LABEL: plain}"
  check job-disallows-env FAIL
  full job-disallows-permissions macos '    runs-on: macos-15' "    runs-on: macos-15${nl}    permissions: {contents: read}"
  check job-disallows-permissions FAIL
  full job-disallows-defaults macos '    runs-on: macos-15' "    runs-on: macos-15${nl}    defaults: {run: {shell: bash}}"
  check job-disallows-defaults FAIL
  full action-other-checkout-sha macos 3d3c42e5aac5ba805825da76410c181273ba90b1 3d3c42e5aac5ba805825da76410c181273ba90b2
  check action-other-checkout-sha FAIL 'step 1.uses'
  full action-other-setup-sha windows 949feb2413d6458794dcd2491c4babbbce0c15c1 949feb2413d6458794dcd2491c4babbbce0c15c2
  check action-other-setup-sha FAIL 'step 2.uses'
  full action-other-artifact-sha windows cf430e030ddbb5b0abf93d22962f4752f3646cd9 cf430e030ddbb5b0abf93d22962f4752f3646cd8
  check action-other-artifact-sha FAIL 'step 10.uses'
  full action-other-checkout-depth windows 'fetch-depth: 2' 'fetch-depth: 3'
  check action-other-checkout-depth FAIL 'step 1.with.fetch-depth'
  full action-checkout-depth-missing macos "        with:${nl}          fetch-depth: 0${nl}" ''
  check action-checkout-depth-missing FAIL 'step 1.with'
  full action-extra-checkout-input macos "          fetch-depth: 0${nl}" "          fetch-depth: 0${nl}          fetch-tags: true${nl}"
  check action-extra-checkout-input FAIL 'step 1.with.fetch-tags'
  full action-other-node-version windows 'node-version: 26' 'node-version: 25'
  check action-other-node-version FAIL 'step 2.with.node-version'
  full action-other-node-cache windows 'cache: npm' 'cache: yarn'
  check action-other-node-cache FAIL 'step 2.with.cache'
  full action-other-artifact-name windows 'name: ci-artifacts-windows' 'name: ci-artifacts-other'
  check action-other-artifact-name FAIL 'step 10.with.name'
  full action-other-artifact-path windows 'path: target/ci-artifacts/' 'path: target/reports/'
  check action-other-artifact-path FAIL 'step 10.with.path'
  full action-upload-if-missing windows "        if: always()${nl}" ''
  check action-upload-if-missing FAIL 'step 10.if'
  full env-commit-before-value macos "BEFORE: \${{ github.event.before }}" 'BEFORE: sample'
  check env-commit-before-value FAIL 'step 2.env.BEFORE'
  full env-commit-title-missing macos "          PR_TITLE: \${{ github.event.pull_request.title }}${nl}" ''
  check env-commit-title-missing FAIL 'step 2.env.PR_TITLE'
  full env-ci-author-value windows 'GIT_AUTHOR_NAME: Halvo CI' 'GIT_AUTHOR_NAME: Fixture CI'
  check env-ci-author-value FAIL 'step 5.env.GIT_AUTHOR_NAME'
  full env-ci-email-value windows 'GIT_COMMITTER_EMAIL: ci@halvo.invalid' 'GIT_COMMITTER_EMAIL: fixture@example.invalid'
  check env-ci-email-value FAIL 'step 5.env.GIT_COMMITTER_EMAIL'
  full roster-drop-step windows "      - name: Prerequisites${nl}        shell: bash${nl}        run: sh scripts/prerequisites.sh${nl}" ''
  check roster-drop-step FAIL 'steps must list'
  full roster-repeat-step windows "      - name: Prerequisites${nl}        shell: bash${nl}        run: sh scripts/prerequisites.sh" "      - name: Prerequisites${nl}        shell: bash${nl}        run: sh scripts/prerequisites.sh${nl}      - name: Prerequisites${nl}        shell: bash${nl}        run: sh scripts/prerequisites.sh${nl}"
  check roster-repeat-step FAIL 'steps must list'
  full name-of-step windows '      - name: Processor' '      - name: Processor information'
  check name-of-step FAIL 'step 6.name'
  full runner-block-literal macos '    runs-on: macos-15' "    runs-on:${nl}      |-${nl}        macos-15"
  check runner-block-literal PASS
  full env-and-upload-condition macos '          PR_TITLE:' '          "PR_TITLE":'
  check env-and-upload-condition PASS

  lines job-if-pr-only 18 "  windows:${nl}    if: github.event_name == 'pull_request'" 9 "  macos:${nl}    if: github.event_name == 'pull_request'"
  check job-if-pr-only FAIL 'use only runs-on, timeout-minutes and steps'
  lines job-if-skip-tags 18 "  windows:${nl}    if: \"!startsWith(github.ref, 'refs/tags/')\"" 9 "  macos:${nl}    if: \"!startsWith(github.ref, 'refs/tags/')\""
  check job-if-skip-tags FAIL 'use only runs-on, timeout-minutes and steps'
  lines job-if-skip-main 18 "  windows:${nl}    if: github.ref != 'refs/heads/main'" 9 "  macos:${nl}    if: github.ref != 'refs/heads/main'"
  check job-if-skip-main FAIL 'use only runs-on, timeout-minutes and steps'
  lines job-if-skip-schedule 18 "  windows:${nl}    if: github.event_name != 'schedule'" 9 "  macos:${nl}    if: github.event_name != 'schedule'"
  check job-if-skip-schedule FAIL 'use only runs-on, timeout-minutes and steps'
  lines job-if-skip-scratch 18 "  windows:${nl}    if: github.event_name != 'push' || github.ref == 'refs/heads/main'" 9 "  macos:${nl}    if: github.event_name != 'push' || github.ref == 'refs/heads/main'"
  check job-if-skip-scratch FAIL 'use only runs-on, timeout-minutes and steps'
  lines job-if-push-only 9 "  macos:${nl}    if: github.event_name == 'push'"
  check job-if-push-only FAIL 'use only runs-on, timeout-minutes and steps'
  lines job-key-continue-on-error 9 "  macos:${nl}    continue-on-error: true"
  check job-key-continue-on-error FAIL 'use only runs-on, timeout-minutes and steps'
  lines job-key-strategy 9 "  macos:${nl}    strategy: {matrix: {build: [check]}}"
  check job-key-strategy FAIL 'use only runs-on, timeout-minutes and steps'
  lines job-key-needs 9 "  macos:${nl}    needs: windows"
  check job-key-needs FAIL 'use only runs-on, timeout-minutes and steps'
  lines job-key-name 9 "  macos:${nl}    name: test job"
  check job-key-name FAIL 'use only runs-on, timeout-minutes and steps'
  lines job-key-env 9 "  macos:${nl}    env: {BUILD: live}"
  check job-key-env FAIL 'use only runs-on, timeout-minutes and steps'
  lines job-key-environment 9 "  macos:${nl}    environment: testing"
  check job-key-environment FAIL 'use only runs-on, timeout-minutes and steps'
  lines job-key-defaults 9 "  macos:${nl}    defaults: {run: {shell: bash}}"
  check job-key-defaults FAIL 'use only runs-on, timeout-minutes and steps'
  lines job-key-concurrency 9 "  macos:${nl}    concurrency: group-a"
  check job-key-concurrency FAIL 'use only runs-on, timeout-minutes and steps'
  lines job-key-outputs 9 "  macos:${nl}    outputs: {result: ready}"
  check job-key-outputs FAIL 'use only runs-on, timeout-minutes and steps'
  step_event="if: github.event_name == 'pull_request'"
  lines step-event-on-all-steps 17 "$pin_save${nl}        $step_event" 13 "$checkout${nl}        $step_event"
  check step-event-on-all-steps FAIL 'four lines of the Rust cache step'
  lines step-event-on-work-steps 13 "$checkout${nl}        $step_event${nl}      - name: ci.d${nl}        $step_event${nl}        run: echo check"
  check step-event-on-work-steps FAIL 'use only always() as a step condition'
  lines step-event-on-ci-d 22 "$checkout${nl}      - name: ci.d${nl}        $step_event${nl}        run: echo check"
  check step-event-on-ci-d FAIL 'use only always() as a step condition'
  lines step-if-ref 13 "$checkout${nl}        if: github.ref == 'refs/heads/main'"
  check step-if-ref FAIL 'use only always() as a step condition'
  lines step-if-actor 13 "$checkout${nl}      - if: github.actor == 'octocat'${nl}        run: echo check"
  check step-if-actor FAIL 'use only always() as a step condition'
  lines step-continue-on-error 22 "$checkout${nl}        continue-on-error: true"
  check step-continue-on-error FAIL 'take continue-on-error out of the step'
  lines step-flow-continue-on-error 13 "$checkout${nl}      - {run: echo, \"continue-on-error\": true}"
  check step-flow-continue-on-error FAIL 'take continue-on-error out of the step'
  lines step-always-and-input-named-if 13 "$checkout${nl}        if: always()${nl}        env:${nl}          if: local"
  check step-always-and-input-named-if FAIL 'step 1.env.if must be absent, read "local"'

  full pinned-and-timed macos '' ''
  check pinned-and-timed PASS 'jobs: 2, uses keys: 8, findings: 0'
  tree action-on-a-tag '13s/@[0-9a-f]* # v7.0.1/@v7/'
  check action-on-a-tag FAIL '1 of 4 uses keys of ci.yml take no 40-hex commit'
  tree action-on-a-short-commit '22s/@[0-9a-f]\{33\}/@/'
  check action-on-a-short-commit FAIL '1 of 4 uses keys of ci.yml take no 40-hex commit'
  tree action-on-41-hex '22s/ # v7.0.1/a # v7.0.1/'
  check action-on-41-hex FAIL '1 of 4 uses keys of ci.yml take no 40-hex commit'
  tree named-step-on-a-tag "13s/.*/      - name: Checkout\\${nl}        uses: actions\\/checkout@v7/"
  check named-step-on-a-tag FAIL '1 of 4 uses keys of ci.yml take no 40-hex commit'
  tree no-uses-line '13s/.*/      - run: "true"/;14,17d;22s/.*/      - run: "true"/;23,26d'
  check no-uses-line FAIL 'FAIL 17-workflow-lint (ci.yml holds no uses line;'
  tree second-workflow-file ''
  cp "$work/ci.yml" "$work/second-workflow-file/.github/workflows/package.yml"
  check second-workflow-file FAIL 'add a script under scripts/<run>/ci.d/, not a workflow file)'
  tree hidden-second-workflow-file ''
  cp "$work/ci.yml" "$work/hidden-second-workflow-file/.github/workflows/.package.yml"
  check hidden-second-workflow-file FAIL 'add a script under scripts/<run>/ci.d/, not a workflow file)'
  tree no-permissions '27,28d'
  check no-permissions FAIL 'ci.yml must set permissions once at the top level, and it does so 0 times)'
  tree permissions-at-job-level-only "27,28d;10s/\$/\\${nl}    permissions: {}/"
  check permissions-at-job-level-only FAIL 'ci.yml must set permissions once at the top level, and it does so 0 times)'
  tree job-without-timeout '20d'
  check job-without-timeout FAIL '1 of 2 jobs of ci.yml set timeout-minutes: 30, the bound ci_job_timeout_min)'
  tree step-timeout-in-place-of-the-job-timeout "20d;22s/\$/\\${nl}        timeout-minutes: 30/"
  check step-timeout-in-place-of-the-job-timeout FAIL '1 of 2 jobs of ci.yml set timeout-minutes: 30, the bound ci_job_timeout_min)'
  tree job-keys-at-another-indent '9,26s/^/  /;11d;20d'
  check job-keys-at-another-indent FAIL 'FAIL 17-workflow-lint (ci.yml holds no job key at two spaces under jobs:;'
  tree timeout-above-the-bound '11s/30/31/'
  check timeout-above-the-bound FAIL '1 of 2 jobs of ci.yml set timeout-minutes: 30, the bound ci_job_timeout_min)'
  tree timeout-ten-times-the-bound '11s/30/300/'
  check timeout-ten-times-the-bound FAIL '1 of 2 jobs of ci.yml set timeout-minutes: 30, the bound ci_job_timeout_min)'
  tree bound-moved ''
  STUB_BOUND=45
  check bound-moved FAIL '0 of 2 jobs of ci.yml set timeout-minutes: 45, the bound ci_job_timeout_min)'
  tree bound-is-no-number ''
  STUB_BOUND=30m
  check bound-is-no-number FAIL 'the bound ci_job_timeout_min is no number)'
  tree bound-zero '11s/30/0/;20s/30/0/'
  STUB_BOUND=0
  check bound-zero FAIL 'actionlint reports a finding in ci.yml)'
  tree bound-unreadable ''
  printf 'gate_bound() {\n  return 1\n}\n' >"$work/bound-unreadable/scripts/r1-gate/lib.sh"
  check bound-unreadable FAIL 'the bound ci_job_timeout_min cannot be read with gate_bound of scripts/r1-gate/lib.sh)'
  tree actionlint-finding "13s/\$/\\${nl}        bogus: true/"
  check actionlint-finding FAIL 'actionlint reports a finding in ci.yml)'
  tree two-findings '13s/@[0-9a-f]* # v7.0.1/@v7/;27,28d'
  check two-findings FAIL '1 of 4 uses keys of ci.yml take no 40-hex commit; pin the action to a commit and keep the version in a comment; and 1 more above)'
  tree quoted-uses-key '13s/.*/      - "uses": actions\/checkout@v7/'
  check quoted-uses-key FAIL '1 of 4 uses keys of ci.yml take no 40-hex commit'
  tree flow-steps-with-an-unpinned-action '12s/.*/    steps: [{uses: actions\/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1}, {uses: actions\/setup-node@v4}]/;13,17d'
  check flow-steps-with-an-unpinned-action FAIL '1 of 4 uses keys of ci.yml take no 40-hex commit'
  tree runs-on-block-list-without-timeout "19s/.*/    runs-on:\\${nl}      - windows-2025/;20d"
  check runs-on-block-list-without-timeout FAIL '1 of 2 jobs of ci.yml set timeout-minutes: 30, the bound ci_job_timeout_min)'
  tree third-job "26s/\$/\\${nl}  linux:\\${nl}    runs-on: ubuntu-24.04\\${nl}    timeout-minutes: 30\\${nl}    steps:\\${nl}      - run: \"true\"/"
  check third-job FAIL 'the jobs of ci.yml must be exactly macos and windows, and it holds: macos windows linux)'
  tree permissions-write-all '27s/.*/permissions: write-all/;28d'
  check permissions-write-all FAIL 'the top-level permissions of ci.yml must grant contents: read and nothing else)'
  tree permissions-contents-write '28s/read/write/'
  check permissions-contents-write FAIL 'the top-level permissions of ci.yml must grant contents: read and nothing else)'
  tree permissions-read-and-more "28s/\$/\\${nl}  actions: write/"
  check permissions-read-and-more FAIL 'the top-level permissions of ci.yml must grant contents: read and nothing else)'
  tree uses-key-with-an-escape '13s/.*/      - "u\\x73es": actions\/checkout@v7/'
  check uses-key-with-an-escape FAIL '1 of 4 uses keys of ci.yml take no 40-hex commit'
  tree uses-key-with-a-unicode-escape '13s/.*/      - "\\u0075ses": actions\/checkout@v7/'
  check uses-key-with-a-unicode-escape FAIL '1 of 4 uses keys of ci.yml take no 40-hex commit'
  tree job-permissions-write-all "10s/\$/\\${nl}    permissions: write-all/"
  check job-permissions-write-all FAIL 'a job of ci.yml sets permissions that grant more than contents: read'
  tree job-permissions-contents-write "10s/\$/\\${nl}    permissions:\\${nl}      contents: write/"
  check job-permissions-contents-write FAIL 'a job of ci.yml sets permissions that grant more than contents: read'
  tree job-permissions-contents-read "10s/\$/\\${nl}    permissions:\\${nl}      contents: read/"
  check job-permissions-contents-read FAIL 'use only runs-on, timeout-minutes and steps'
  tree job-permissions-empty "10s/\$/\\${nl}    permissions: {}/"
  check job-permissions-empty FAIL 'use only runs-on, timeout-minutes and steps'
  tree explicit-uses-key "13s/.*/      - ? uses\\${nl}        : actions\/checkout@v4/"
  check explicit-uses-key FAIL 'ci.yml line 13 starts a key with ? and a blank, one of the keys in the explicit form of YAML'
  tree explicit-job-permissions "10s/\$/\\${nl}    ? permissions\\${nl}    : write-all/"
  check explicit-job-permissions FAIL 'ci.yml line 11 starts a key with ? and a blank, one of the keys in the explicit form of YAML'
  tree timeout-in-a-double-quoted-name "11s/.*/    name: \"macos\\${nl}    timeout-minutes: 30\\${nl}    \"/"
  check timeout-in-a-double-quoted-name FAIL 'ci.yml line 11 has a quote that this lint reads as running past the line'"'"'s end'
  tree timeout-in-a-single-quoted-name "11s/.*/    name: 'macos\\${nl}    timeout-minutes: 30\\${nl}    '/"
  check timeout-in-a-single-quoted-name FAIL 'ci.yml line 11 has a quote that this lint reads as running past the line'"'"'s end'
  tree longer-timeout-beside-a-quoted-name "11s/30/600/;11s/\$/\\${nl}    name: \"macos\\${nl}    timeout-minutes: 30\\${nl}    \"/"
  check longer-timeout-beside-a-quoted-name FAIL 'ci.yml line 12 has a quote that this lint reads as running past the line'"'"'s end'
  tree timeout-after-an-escaped-double-quote "11s/.*/    name: \"say \\\\\"hi\\${nl}    timeout-minutes: 30\\${nl}    \"/"
  check timeout-after-an-escaped-double-quote FAIL 'ci.yml line 11 has a quote that this lint reads as running past the line'"'"'s end'
  tree timeout-after-a-doubled-single-quote "11s/.*/    name: 'macos\\${nl}    it''s\\${nl}    timeout-minutes: 30\\${nl}    '/"
  check timeout-after-a-doubled-single-quote FAIL 'ci.yml line 11 has a quote that this lint reads as running past the line'"'"'s end'
  tree timeout-after-a-hash-in-a-quoted-name "11s/.*/    name: \"macos #\\${nl}    timeout-minutes: 30\\${nl}    \"/"
  check timeout-after-a-hash-in-a-quoted-name FAIL 'ci.yml line 11 has a quote that this lint reads as running past the line'"'"'s end'
  tree timeout-in-a-tagged-quoted-name "11s/.*/    name: \&n !!str \"macos\\${nl}    timeout-minutes: 30\\${nl}    \"/;20s/\$/\\${nl}    name: *n/"
  check timeout-in-a-tagged-quoted-name FAIL 'ci.yml line 11 starts a node with &, * or !'
  tree timeout-in-a-quoted-flow-value "11s/.*/    env: { A: a#b, NAME: \"macos\\${nl}    timeout-minutes: 30\\${nl}    \" }/"
  check timeout-in-a-quoted-flow-value FAIL 'ci.yml line 11 has a quote that this lint reads as running past the line'"'"'s end'
  tree timeout-in-a-json-like-flow-value "11s/.*/    env: {\"NAME\":\"macos\\${nl}    timeout-minutes: 30\\${nl}    \"}/"
  check timeout-in-a-json-like-flow-value FAIL 'ci.yml line 11 has a quote that this lint reads as running past the line'"'"'s end'
  tree timeout-in-a-quoted-item-of-a-flow-list-over-lines "11s/.*/    strategy:\\${nl}      matrix:\\${nl}        os: [a,\\${nl}      b, \"c\\${nl}    timeout-minutes: 30\\${nl}    \"]/"
  check timeout-in-a-quoted-item-of-a-flow-list-over-lines FAIL 'ci.yml line 13 has a [ or { that this lint reads as running past the line'"'"'s end'
  tree timeout-in-a-quoted-step-value "11d;13s/.*/      - run: \"true\\${nl}    timeout-minutes: 30\\${nl}      \"/"
  check timeout-in-a-quoted-step-value FAIL 'ci.yml line 12 has a quote that this lint reads as running past the line'"'"'s end'
  tree timeout-in-a-step-name-after-a-run-block "11d;13s/.*/      - run: |\\${nl}          true\\${nl}        name: \"step\\${nl}    timeout-minutes: 30\\${nl}        \"/"
  check timeout-in-a-step-name-after-a-run-block FAIL 'ci.yml line 14 has a quote that this lint reads as running past the line'"'"'s end'
  tree job-in-a-quoted-name "11s/\$/\\${nl}    name: \"macos\\${nl}  windows:\\${nl}    runs-on: windows-2025\\${nl}    timeout-minutes: 30\\${nl}    \"/;18,26d"
  check job-in-a-quoted-name FAIL 'ci.yml line 12 has a quote that this lint reads as running past the line'"'"'s end'
  tree tab-before-a-quote-in-a-flow-value "11s/.*/    env: {NAME:@TAB@!!str@TAB@\"macos\\${nl}    timeout-minutes: 30\\${nl}    \"}/"
  check tab-before-a-quote-in-a-flow-value FAIL 'ci.yml line 11 holds the byte 0x09'
  tree flow-list-over-lines-holds-the-limit "11s/30/600/;11s/\$/\\${nl}    strategy:\\${nl}      matrix:\\${nl}        os: [a, {b: c},\\${nl}    timeout-minutes: 30\\${nl}    ]/"
  check flow-list-over-lines-holds-the-limit FAIL 'ci.yml line 14 has a [ or { that this lint reads as running past the line'"'"'s end'
  tree quote-in-a-plain-continuation-hides-no-job "10s/\$/\\${nl}    env:\\${nl}      NOTE: a\\${nl}        \"b/;26s/\$/\\${nl}  extra:\\${nl}    runs-on: ubuntu-24.04\\${nl}    timeout-minutes: 600\\${nl}    steps:\\${nl}      - run: echo \"hi\"/"
  check quote-in-a-plain-continuation-hides-no-job FAIL 'the jobs of ci.yml must be exactly macos and windows, and it holds: macos windows extra'
  tree closed-quotes-before-the-timeout "13s/\$/\\${nl}        name: \"macos \\\\\" # job\"/;22s/\$/\\${nl}        name: 'windows\\\\' # y/"
  check closed-quotes-before-the-timeout FAIL 'step 1.name must be absent'
  full comment-at-column-one-with-a-quote macos '    runs-on: macos-15' "    runs-on: macos-15${nl}# a: \"x"
  check comment-at-column-one-with-a-quote PASS 'jobs: 2, uses keys: 8, findings: 0'
  tree apostrophe-in-a-plain-value "13s/\$/\\${nl}        env:\\${nl}          F: it's/"
  check apostrophe-in-a-plain-value FAIL 'step 1.env.F'
  tree dash-before-a-quote-in-a-plain-value "13s/\$/\\${nl}        env:\\${nl}          A: -\"x/"
  check dash-before-a-quote-in-a-plain-value FAIL 'step 1.env.A'
  tree comma-and-comment-in-a-plain-value "13s/\$/\\${nl}        env:\\${nl}          B: a, \"y # c: 'z/"
  check comma-and-comment-in-a-plain-value FAIL 'step 1.env.B'
  tree open-bracket-in-a-plain-value "13s/\$/\\${nl}        env:\\${nl}          C: a[b, \"y/"
  check open-bracket-in-a-plain-value FAIL 'step 1.env.C'
  tree closing-bracket-in-a-plain-value "13s/\$/\\${nl}        env:\\${nl}          E: a], \"z/"
  check closing-bracket-in-a-plain-value FAIL 'step 1.env.E'
  lines comma-after-a-closed-flow-map 13 "$checkout${nl}        env: {A: b}${nl}        name: a, \"y"
  check comma-after-a-closed-flow-map FAIL 'step 1.env.A'
  tree colon-in-a-plain-value-after-a-quoted-key "13s/\$/\\${nl}        env:\\${nl}          \"D\": a:\"b/"
  check colon-in-a-plain-value-after-a-quoted-key FAIL 'step 1.env.D'
  tree quoted-names-over-two-lines-that-close-before-the-timeout "10s/\$/\\${nl}    name: \"macos\\${nl}      job\"/;19s/\$/\\${nl}    name: 'windows\\${nl}      job'/"
  check quoted-names-over-two-lines-that-close-before-the-timeout FAIL 'ci.yml line 11 has a quote that this lint reads as running past the line'"'"'s end'
  full run-block-with-a-blank-line-and-an-open-quote macos 'run: sh scripts/prerequisites.sh' "run: |${nl}${nl}          printf '%s' \"x: 'y\""
  check run-block-with-a-blank-line-and-an-open-quote PASS 'jobs: 2, uses keys: 8, findings: 0'
  tree folded-block-with-an-open-quote "13s/\$/\\${nl}      - name: >-\\${nl}          step: 'one\\${nl}        run: echo/"
  check folded-block-with-an-open-quote FAIL 'step 2.name'
  tree block-scalar-under-a-tagged-key "13s/\$/\\${nl}      - !!str run: |\\${nl}          printf '%s' \"x: 'y\"/"
  check block-scalar-under-a-tagged-key FAIL 'ci.yml line 14 starts a node with &, * or !'
  full block-scalar-under-a-quoted-key macos 'run: sh scripts/prerequisites.sh' "\"run\": |${nl}          printf '%s' \"x: 'y\""
  check block-scalar-under-a-quoted-key PASS 'jobs: 2, uses keys: 8, findings: 0'
  lines block-scalar-item-of-a-list 10 "    runs-on:${nl}      - |-${nl}        macos-15"
  check block-scalar-item-of-a-list FAIL 'job macos runs-on must be "macos-15", read ["macos-15"]'
  tree cr-before-a-quoted-list-item "11s/.*/    strategy:\\${nl}      matrix:\\${nl}        os:\\${nl}          - a@CR@          - \"b\\${nl}    timeout-minutes: 30\\${nl}            c\"/"
  check cr-before-a-quoted-list-item FAIL 'ci.yml line 14 holds the byte 0x0D'
  tree ls-before-a-quoted-list-item "11s/.*/    strategy:\\${nl}      matrix:\\${nl}        os:\\${nl}          - a@LS@          - \"b\\${nl}    timeout-minutes: 30\\${nl}            c\"/"
  check ls-before-a-quoted-list-item FAIL 'ci.yml line 14 holds the byte 0xE2'
  tree nel-before-a-quoted-list-item "11s/.*/    strategy:\\${nl}      matrix:\\${nl}        os:\\${nl}          - a@NEL@          - \"b\\${nl}    timeout-minutes: 30\\${nl}            c\"/"
  check nel-before-a-quoted-list-item FAIL 'ci.yml line 14 holds the byte 0xC2'
  tree ps-before-a-quoted-list-item "11s/.*/    strategy:\\${nl}      matrix:\\${nl}        os:\\${nl}          - a@PS@          - \"b\\${nl}    timeout-minutes: 30\\${nl}            c\"/"
  check ps-before-a-quoted-list-item FAIL 'ci.yml line 14 holds the byte 0xE2'
  tree cr-inside-a-block-scalar-line "11s/.*/    env:\\${nl}      NOTE: |\\${nl}        text@CR@      LABEL: \"x\\${nl}    timeout-minutes: 30\\${nl}        y\"/"
  check cr-inside-a-block-scalar-line FAIL 'ci.yml line 13 holds the byte 0x0D'
  tree cr-in-a-plain-value-hides-a-third-job "26s/\$/\\${nl}      - run: echo a@CR@  extra:@CR@    runs-on: ubuntu-24.04@CR@    timeout-minutes: 600@CR@    steps:@CR@      - run: echo x/"
  check cr-in-a-plain-value-hides-a-third-job FAIL 'ci.yml line 27 holds the byte 0x0D'
  tree ls-after-a-comment-hides-a-third-job "26s/\$/\\${nl}    # note@LS@  extra:@LS@    runs-on: ubuntu-24.04@LS@    timeout-minutes: 600@LS@    steps:@LS@      - run: echo x/"
  check ls-after-a-comment-hides-a-third-job FAIL 'ci.yml line 27 holds the byte 0xE2'
  tree nel-in-a-plain-value-hides-a-third-job "26s/\$/\\${nl}      - run: echo a@NEL@  extra:@NEL@    runs-on: ubuntu-24.04@NEL@    timeout-minutes: 600@NEL@    steps:@NEL@      - run: echo x/"
  check nel-in-a-plain-value-hides-a-third-job FAIL 'ci.yml line 27 holds the byte 0xC2'
  tree cr-before-a-quoted-name-hides-the-permissions "29s/.*/name:@CR@  \"ci\\${nl}permissions:\\${nl}  contents: read\\${nl}x\"/;27,28d"
  check cr-before-a-quoted-name-hides-the-permissions FAIL 'ci.yml line 27 holds the byte 0x0D'
  tree cr-after-the-last-line-hides-a-third-job "1s/^/permissions:\\${nl}  contents: read\\${nl}/;27,29d;26s/\$/@CR@  linux:@CR@    runs-on: ubuntu-24.04@CR@    timeout-minutes: 600@CR@    steps:@CR@      - run: echo x/"
  check cr-after-the-last-line-hides-a-third-job FAIL 'ci.yml line 28 holds the byte 0x0D'
  tree tab-before-a-comment "10s/\$/@TAB@# note/"
  check tab-before-a-comment FAIL 'ci.yml line 10 holds the byte 0x09'
  tree crlf-line-ends ""
  perl -pi -e 's/\n/\r\n/' "$work/crlf-line-ends/.github/workflows/ci.yml"
  check crlf-line-ends FAIL 'ci.yml line 1 holds the byte 0x0D'
  tree byte-ff-in-a-comment "2s/\$/ # @FF@/"
  check byte-ff-in-a-comment FAIL 'ci.yml line 2 holds the byte 0xFF'
  tree nul-in-a-comment "2s/\$/ # a@NUL@b/"
  check nul-in-a-comment FAIL 'ci.yml line 2 holds the byte 0x00'
  tree byte-order-mark "1s/^/@BOM@/"
  check byte-order-mark FAIL 'ci.yml line 1 holds the byte 0xEF'
  tree character-outside-ascii-in-a-name "10s/\$/\\${nl}    name: caf@EACUTE@/"
  check character-outside-ascii-in-a-name FAIL 'ci.yml line 11 holds the byte 0xC3'
  tree explicit-key-behind-two-dashes "11s/.*/    strategy:\\${nl}      matrix:\\${nl}        os:\\${nl}          - - ? \"a\\${nl}    timeout-minutes: 30\\${nl}              x\"\\${nl}              : b/"
  check explicit-key-behind-two-dashes FAIL 'ci.yml line 14 starts a key with ? and a blank, one of the keys in the explicit form of YAML'
  tree explicit-key-in-a-flow-map-of-with "11d;13s/.*/      - uses: foo\\/bar@3d3c42e5aac5ba805825da76410c181273ba90b1\\${nl}        with: { ? \"a}\\${nl}    timeout-minutes: 30\\${nl}        x\" : b }/"
  check explicit-key-in-a-flow-map-of-with FAIL 'ci.yml line 13 starts a key with ? and a blank, one of the keys in the explicit form of YAML'
  tree explicit-key-behind-two-dashes-closing-with-a-quote "11s/.*/    strategy:\\${nl}      matrix:\\${nl}        os:\\${nl}          - - ? \"a\\${nl}    timeout-minutes: 30\\${nl}              \"\\${nl}              : b/"
  check explicit-key-behind-two-dashes-closing-with-a-quote FAIL 'ci.yml line 14 starts a key with ? and a blank, one of the keys in the explicit form of YAML'
  tree explicit-key-in-a-flow-map-of-with-closing-with-a-quote "11d;13s/.*/      - uses: foo\\/bar@3d3c42e5aac5ba805825da76410c181273ba90b1\\${nl}        with: { ? \"a}\\${nl}    timeout-minutes: 30\\${nl}        \" : b }/"
  check explicit-key-in-a-flow-map-of-with-closing-with-a-quote FAIL 'ci.yml line 13 starts a key with ? and a blank, one of the keys in the explicit form of YAML'
  tree explicit-key-in-a-flow-list "11s/.*/    strategy:\\${nl}      matrix:\\${nl}        os: [ ? 'a]\\${nl}    timeout-minutes: 30\\${nl}        x' ]/"
  check explicit-key-in-a-flow-list FAIL 'ci.yml line 13 starts a key with ? and a blank, one of the keys in the explicit form of YAML'
  tree explicit-key-in-a-flow-list-closing-with-a-quote "11s/.*/    strategy:\\${nl}      matrix:\\${nl}        os: [ ? 'a]\\${nl}    timeout-minutes: 30\\${nl}        ' ]/"
  check explicit-key-in-a-flow-list-closing-with-a-quote FAIL 'ci.yml line 13 starts a key with ? and a blank, one of the keys in the explicit form of YAML'
  tree explicit-key-beside-a-longer-limit "11s/30/600/;13s/.*/      - uses: foo\\/bar@3d3c42e5aac5ba805825da76410c181273ba90b1\\${nl}        with: { ? \"a}\\${nl}    timeout-minutes: 30\\${nl}        x\" : b }/"
  check explicit-key-beside-a-longer-limit FAIL 'ci.yml line 14 starts a key with ? and a blank, one of the keys in the explicit form of YAML'
  tree explicit-key-hides-a-later-job "22s/.*/      - uses: foo\\/bar@3d3c42e5aac5ba805825da76410c181273ba90b1\\${nl}        with: { ? \"a}\\${nl}x\\${nl}        y\" : b }\\${nl}  linux:\\${nl}    runs-on: ubuntu-24.04\\${nl}    timeout-minutes: 600\\${nl}    steps:\\${nl}      - run: echo x/"
  check explicit-key-hides-a-later-job FAIL 'ci.yml line 23 starts a key with ? and a blank, one of the keys in the explicit form of YAML'
  tree second-document-holds-the-permissions "27,28d;\$s/\$/\\${nl}---\\${nl}permissions:\\${nl}  contents: read/"
  check second-document-holds-the-permissions FAIL 'ci.yml line 28 begins a YAML document or a directive'
  tree end-marker-then-a-document "27,28d;\$s/\$/\\${nl}...\\${nl}---\\${nl}permissions:\\${nl}  contents: read/"
  check end-marker-then-a-document FAIL 'ci.yml line 28 begins a YAML document or a directive'
  tree tagged-jobs-key-and-a-second-document "1s/^/permissions:\\${nl}  contents: read\\${nl}/;27,29d;8s/.*/!!str jobs:/;11d;26s/\$/\\${nl}  linux:\\${nl}    runs-on: ubuntu-24.04\\${nl}    timeout-minutes: 600\\${nl}    steps:\\${nl}      - run: echo x\\${nl}---\\${nl}jobs:\\${nl}  macos:\\${nl}    runs-on: macos-15\\${nl}    timeout-minutes: 30\\${nl}  windows:\\${nl}    runs-on: windows-2025\\${nl}    timeout-minutes: 30/"
  check tagged-jobs-key-and-a-second-document FAIL 'ci.yml line 33 begins a YAML document or a directive'
  tree quote-starts-a-second-document "27,28d;\$s/\$/\\${nl}--- \"x\\${nl}permissions:\\${nl}  contents: read\\${nl}\"/"
  check quote-starts-a-second-document FAIL 'ci.yml line 28 begins a YAML document or a directive'
  tree directive-line "1s/^/%YAML 1.2\\${nl}---\\${nl}/"
  check directive-line FAIL 'ci.yml line 1 begins a YAML document or a directive'
  tree anchor-and-alias-as-a-uses-key "11s/\$/\\${nl}    env:\\${nl}      K: \\&k uses/;13s/\$/\\${nl}      - *k : actions\/setup-node@v4/"
  check anchor-and-alias-as-a-uses-key FAIL 'ci.yml line 13 starts a node with &, * or !'
  tree alias-as-a-uses-key-in-a-flow-step "11s/\$/\\${nl}    env:\\${nl}      K: \\&k uses/;13s/\$/\\${nl}      - { *k : actions\/setup-node@v4 }/"
  check alias-as-a-uses-key-in-a-flow-step FAIL 'ci.yml line 13 starts a node with &, * or !'
  tree job-that-is-an-alias "9s/\$/ \\&m/;18,26d;17s/\$/\\${nl}  windows: *m/"
  check job-that-is-an-alias FAIL 'ci.yml line 9 starts a node with &, * or !'
  tree tagged-jobs-key "8s/.*/!!str jobs:/"
  check tagged-jobs-key FAIL 'ci.yml line 8 starts a node with &, * or !'
  tree tag-before-a-quote-over-lines "11s/.*/    name: !!str 'x\\${nl}    timeout-minutes: 30\\${nl}    '/"
  check tag-before-a-quote-over-lines FAIL 'ci.yml line 11 starts a node with &, * or !'
  tree alias-with-no-anchor "10s/\$/\\${nl}    name: *n/"
  check alias-with-no-anchor FAIL 'ci.yml line 11 starts a node with &, * or !'
  tree plain-continuation-that-starts-with-ampersands "10s/\$/\\${nl}    env:\\${nl}      CMD: a\\${nl}        \\&\\& b/"
  check plain-continuation-that-starts-with-ampersands FAIL 'ci.yml line 13 starts a node with &, * or !'
  tree if-that-begins-with-a-bang "13s/\$/\\${nl}      - if: !cancelled()\\${nl}        run: echo x/"
  check if-that-begins-with-a-bang FAIL 'ci.yml line 14 starts a node with &, * or !, which this lint reads as an anchor, an alias or a tag and does not read; take an anchor, an alias or a tag out and write the value itself, and put a value that begins with &, * or ! as text in quotes'
  tree if-that-begins-with-a-bang-in-quotes "13s/\$/\\${nl}      - if: '!cancelled()'\\${nl}        run: echo x/"
  check if-that-begins-with-a-bang-in-quotes FAIL 'use only always() as a step condition'
  tree plain-continuation-that-starts-with-an-asterisk "10s/\$/\\${nl}    env:\\${nl}      CMD: a\\${nl}        * b/"
  check plain-continuation-that-starts-with-an-asterisk FAIL 'ci.yml line 13 starts a node with &, * or !'
  tree plain-continuation-that-starts-with-a-bang "10s/\$/\\${nl}    env:\\${nl}      CMD: a\\${nl}        ! b/"
  check plain-continuation-that-starts-with-a-bang FAIL 'ci.yml line 13 starts a node with &, * or !'
  tree quoted-name-holds-the-permissions "29s/.*/name: \"ci\\${nl}permissions:\\${nl}  contents: read\\${nl}\"/;27,28d"
  check quoted-name-holds-the-permissions FAIL 'ci.yml line 27 has a quote that this lint reads as running past the line'"'"'s end'
  tree open-flow-map-with-no-quote "11s/.*/    env: { A: b,\\${nl}    timeout-minutes: 30\\${nl}    }/"
  check open-flow-map-with-no-quote FAIL 'ci.yml line 11 has a [ or { that this lint reads as running past the line'"'"'s end'
  tree explicit-key-mark-at-the-line-end "10s/\$/\\${nl}    ?\\${nl}      name\\${nl}    : x/"
  check explicit-key-mark-at-the-line-end FAIL 'ci.yml line 11 starts a key with ? and a blank, one of the keys in the explicit form of YAML'
  tree question-mark-inside-a-plain-value "13s/\$/\\${nl}        name: is it ? yes/"
  check question-mark-inside-a-plain-value FAIL 'step 1.name must be absent'
  tree signs-inside-a-plain-value "13s/\$/\\${nl}        name: a \\&\\& b * c!/"
  check signs-inside-a-plain-value FAIL 'step 1.name must be absent'
  base_save="          save-if: \${{ github.ref == 'refs/heads/main' }}"
  lines r1-v11-macos-save-if-of-the-base 17 "$base_save"
  check r1-v11-macos-save-if-of-the-base FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 17: '          save-if: \${{ github.ref == 'refs/heads/main' }}' stands where this lint reads line 4 of the Rust cache step of the job macos, '${pin_save}'; it differs from the line to write first at column 56, which holds the sign } where the line to write holds the sign &; write '${pin_save}'; the pin holds the save-if line in the variable pin_save at the top of scripts/r1/ci.d/17-workflow-lint.sh, and a change of it is a ruled change)"
  lines r2-v12-windows-save-if-of-the-base 26 "$base_save"
  check r2-v12-windows-save-if-of-the-base FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 26: '          save-if: \${{ github.ref == 'refs/heads/main' }}' stands where this lint reads line 4 of the Rust cache step of the job windows, '${pin_save}'; it differs from the line to write first at column 56, which holds the sign } where the line to write holds the sign &; write '${pin_save}'; the pin holds the save-if line in the variable pin_save at the top of scripts/r1/ci.d/17-workflow-lint.sh, and a change of it is a ruled change)"
  lines r3-v06-macos-prefix-key-line-gone 16 -
  check r3-v06-macos-prefix-key-line-gone FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 16: '${pin_save}' stands where this lint reads line 3 of the Rust cache step of the job macos, '${pin_prefix}'; it differs from the line to write first at column 11, which holds the sign s where the line to write holds the sign p; write '${pin_prefix}'; the pin holds the prefix-key line in the variable pin_prefix at the top of scripts/r1/ci.d/17-workflow-lint.sh, and a change of it is a ruled change)"
  lines r4-v07-windows-prefix-key-line-gone 25 -
  check r4-v07-windows-prefix-key-line-gone FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 25: '${pin_save}' stands where this lint reads line 3 of the Rust cache step of the job windows, '${pin_prefix}'; it differs from the line to write first at column 11, which holds the sign s where the line to write holds the sign p; write '${pin_prefix}'; the pin holds the prefix-key line in the variable pin_prefix at the top of scripts/r1/ci.d/17-workflow-lint.sh, and a change of it is a ruled change)"
  lines r5-macos-both-lines-of-the-base 17 "$base_save" 16 -
  check r5-macos-both-lines-of-the-base FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 16: '          save-if: \${{ github.ref == 'refs/heads/main' }}' stands where this lint reads line 3 of the Rust cache step of the job macos, '${pin_prefix}'; it differs from the line to write first at column 11, which holds the sign s where the line to write holds the sign p; write '${pin_prefix}'; the pin holds the prefix-key line in the variable pin_prefix at the top of scripts/r1/ci.d/17-workflow-lint.sh, and a change of it is a ruled change)"
  lines v01-no-schedule-trigger 7 - 6 -
  check v01-no-schedule-trigger FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 1: the block on: holds no line '  schedule:'; this lint reads the schedule as that line alone and under it the line '${pin_cron}'; write both lines under 'on:'; the pin holds the cron line in the variable pin_cron at the top of scripts/r1/ci.d/17-workflow-lint.sh, and a change of it is a ruled change)"
  lines v02-cron-daily 7 "    - cron: '17 3 * * *'"
  check v02-cron-daily FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 7: '    - cron: '17 3 * * *'' stands under '  schedule:', where this lint reads the cron line; it differs from the line to write first at column 23, which holds the sign * where the line to write holds the sign 0; write '${pin_cron}'; the pin holds the cron line in the variable pin_cron at the top of scripts/r1/ci.d/17-workflow-lint.sh, and a change of it is a ruled change)"
  lines v08-macos-cold-prefix-is-v0-rust 16 "          prefix-key: \${{ github.event_name == 'schedule' && 'v0-rust' || 'v0-rust' }}"
  check v08-macos-cold-prefix-is-v0-rust FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 16: '          prefix-key: \${{ github.event_name == 'schedule' && 'v0-rust' || 'v0-rust' }}' stands where this lint reads line 3 of the Rust cache step of the job macos, '${pin_prefix}'; it differs from the line to write first at column 70, which holds the sign ' where the line to write holds the sign -; write '${pin_prefix}'; the pin holds the prefix-key line in the variable pin_prefix at the top of scripts/r1/ci.d/17-workflow-lint.sh, and a change of it is a ruled change)"
  lines v09-windows-prefix-inverted 25 "          prefix-key: \${{ github.event_name != 'schedule' && 'v0-rust-cold' || 'v0-rust' }}"
  check v09-windows-prefix-inverted FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 25: '          prefix-key: \${{ github.event_name != 'schedule' && 'v0-rust-cold' || 'v0-rust' }}' stands where this lint reads line 3 of the Rust cache step of the job windows, '${pin_prefix}'; it differs from the line to write first at column 45, which holds the sign ! where the line to write holds the sign =; write '${pin_prefix}'; the pin holds the prefix-key line in the variable pin_prefix at the top of scripts/r1/ci.d/17-workflow-lint.sh, and a change of it is a ruled change)"
  lines v10-macos-cold-prefix-empty 16 "          prefix-key: \${{ github.event_name == 'schedule' && '' || 'v0-rust' }}"
  check v10-macos-cold-prefix-empty FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 16: '          prefix-key: \${{ github.event_name == 'schedule' && '' || 'v0-rust' }}' stands where this lint reads line 3 of the Rust cache step of the job macos, '${pin_prefix}'; it differs from the line to write first at column 63, which holds the sign ' where the line to write holds the sign v; write '${pin_prefix}'; the pin holds the prefix-key line in the variable pin_prefix at the top of scripts/r1/ci.d/17-workflow-lint.sh, and a change of it is a ruled change)"
  lines v13-windows-save-if-or 26 "          save-if: \${{ github.ref == 'refs/heads/main' || github.event_name != 'schedule' }}"
  check v13-windows-save-if-or FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 26: '          save-if: \${{ github.ref == 'refs/heads/main' || github.event_name != 'schedule' }}' stands where this lint reads line 4 of the Rust cache step of the job windows, '${pin_save}'; it differs from the line to write first at column 56, which holds the sign | where the line to write holds the sign &; write '${pin_save}'; the pin holds the save-if line in the variable pin_save at the top of scripts/r1/ci.d/17-workflow-lint.sh, and a change of it is a ruled change)"
  lines v14-macos-save-if-master 17 "          save-if: \${{ github.ref == 'refs/heads/master' && github.event_name != 'schedule' }}"
  check v14-macos-save-if-master FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 17: '          save-if: \${{ github.ref == 'refs/heads/master' && github.event_name != 'schedule' }}' stands where this lint reads line 4 of the Rust cache step of the job macos, '${pin_save}'; it differs from the line to write first at column 52, which holds the sign s where the line to write holds the sign i; write '${pin_save}'; the pin holds the save-if line in the variable pin_save at the top of scripts/r1/ci.d/17-workflow-lint.sh, and a change of it is a ruled change)"
  lines v15-windows-save-if-true 26 "          save-if: true"
  check v15-windows-save-if-true FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 26: '          save-if: true' stands where this lint reads line 4 of the Rust cache step of the job windows, '${pin_save}'; it differs from the line to write first at column 20, which holds the sign t where the line to write holds the sign \$; write '${pin_save}'; the pin holds the save-if line in the variable pin_save at the top of scripts/r1/ci.d/17-workflow-lint.sh, and a change of it is a ruled change)"
  lines v18-macos-cold-prefix-v0-rus 16 "          prefix-key: \${{ github.event_name == 'schedule' && 'v0-rus' || 'v0-rust' }}"
  check v18-macos-cold-prefix-v0-rus FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 16: '          prefix-key: \${{ github.event_name == 'schedule' && 'v0-rus' || 'v0-rust' }}' stands where this lint reads line 3 of the Rust cache step of the job macos, '${pin_prefix}'; it differs from the line to write first at column 69, which holds the sign ' where the line to write holds the sign t; write '${pin_prefix}'; the pin holds the prefix-key line in the variable pin_prefix at the top of scripts/r1/ci.d/17-workflow-lint.sh, and a change of it is a ruled change)"
  lines pinned-lines-inside-a-quoted-scalar 17 "$base_save" 16 "          key: \"weekly${nl}$pin_prefix${nl}$pin_save${nl}          \""
  check pinned-lines-inside-a-quoted-scalar FAIL "' stands where this lint reads line 3 of the Rust cache step of the job macos, '${pin_prefix}'; it differs from the line to write first at column 11, which holds the sign k where the line to write holds the sign p"
  lines g13-macos-decoy-in-job-name-real-step-indentless 17 "        save-if: true" 16 - 15 "      with:" 14 "    - uses: Swatinem/rust-cache@d1527c8f67ef9af357290475d4af4b0fab8f12e0 # v2.9.3" 13 "    - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1" 9 "  macos:${nl}    name: |${nl}$pin_uses${nl}$pin_with${nl}$pin_prefix${nl}$pin_save"
  check g13-macos-decoy-in-job-name-real-step-indentless FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 10: '    name: |' is an own key of the job macos whose value begins a block scalar; this lint reads no block scalar on a job's own key; write the value on its line)"
  lines x02-windows-decoy-in-job-name-real-step-indentless 26 "        save-if: true" 25 - 24 "      with:" 23 "    - uses: Swatinem/rust-cache@d1527c8f67ef9af357290475d4af4b0fab8f12e0 # v2.9.3" 22 "    - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1" 18 "  windows:${nl}    name: |${nl}$pin_uses${nl}$pin_with${nl}$pin_prefix${nl}$pin_save"
  check x02-windows-decoy-in-job-name-real-step-indentless FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 19: '    name: |' is an own key of the job windows whose value begins a block scalar; this lint reads no block scalar on a job's own key; write the value on its line)"
  lines x04-macos-decoy-in-job-name-steps-in-flow 17 - 16 - 15 - 14 - 13 - 12 "    steps: [{uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1}, {uses: Swatinem/rust-cache@d1527c8f67ef9af357290475d4af4b0fab8f12e0, with: {save-if: true}}]" 9 "  macos:${nl}    name: |${nl}$pin_uses${nl}$pin_with${nl}$pin_prefix${nl}$pin_save"
  check x04-macos-decoy-in-job-name-steps-in-flow FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 10: '    name: |' is an own key of the job macos whose value begins a block scalar; this lint reads no block scalar on a job's own key; write the value on its line)"
  lines x05-macos-decoy-and-real-step-both-normal 9 "  macos:${nl}    name: |${nl}$pin_uses${nl}$pin_with${nl}$pin_prefix${nl}$pin_save"
  check x05-macos-decoy-and-real-step-both-normal FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 10: '    name: |' is an own key of the job macos whose value begins a block scalar; this lint reads no block scalar on a job's own key; write the value on its line)"
  lines e14-macos-steps-indentless 17 "${pin_save#  }" 16 "${pin_prefix#  }" 15 "${pin_with#  }" 14 "${pin_uses#  }" 13 "${checkout#  }"
  check e14-macos-steps-indentless FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 13: '    - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1' stands at four blanks after '    steps:' in the job macos; this lint reads '    steps:' as the job's last own key and each step at six blanks, a dash and a blank; write the steps so and the job's other keys above '    steps:')"
  lines x06-macos-only-the-real-step-indentless-save-true 17 "        save-if: true" 16 "${pin_prefix#  }" 15 "${pin_with#  }" 14 "${pin_uses#  }" 13 "${checkout#  }"
  check x06-macos-only-the-real-step-indentless-save-true FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 13: '    - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1' stands at four blanks after '    steps:' in the job macos; this lint reads '    steps:' as the job's last own key and each step at six blanks, a dash and a blank; write the steps so and the job's other keys above '    steps:')"
  lines h19-windows-steps-at-four-blanks 26 "${pin_save#  }" 25 "${pin_prefix#  }" 24 "${pin_with#  }" 23 "${pin_uses#  }" 22 "${checkout#  }"
  check h19-windows-steps-at-four-blanks FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 22: '    - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1' stands at four blanks after '    steps:' in the job windows; this lint reads '    steps:' as the job's last own key and each step at six blanks, a dash and a blank; write the steps so and the job's other keys above '    steps:')"
  lines steps-in-a-flow-list-with-no-decoy 17 - 16 - 15 - 14 - 13 - 12 "    steps: [{uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1}, {uses: Swatinem/rust-cache@d1527c8f67ef9af357290475d4af4b0fab8f12e0}]"
  check steps-in-a-flow-list-with-no-decoy FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 12: '    steps: [{uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1}, {uses: Swatinem/rust-cache@d1527c8f67ef9af357290475d4af4b0fab8f12e0}]' is the steps key of the job macos with something behind its colon; this lint reads the steps as the line '    steps:' alone and below it each step at six blanks, a dash and a blank; write them so)"
  lines h16-comment-on-the-macos-job-key 9 "  macos: # the macOS job"
  check h16-comment-on-the-macos-job-key FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 9: '  macos: # the macOS job' stands at two blanks under 'jobs:', where this lint reads a job's key; it reads that key as the line '  macos:' or '  windows:' alone, with no comment, no quotes and no blank before the colon; write one of those lines)"
  lines h17-quoted-on-key 1 '"on":'
  check h17-quoted-on-key FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 1: '\"on\":' holds the key on in another form; this lint reads the key on as the line 'on:' alone in column 1, with no quotes, capital letters, escapes, blank before the colon, value or comment; write 'on:')"
  full h18-escape-x0a-in-a-run-block-above-the-cache-step macos 'run: sh scripts/prerequisites.sh' "run: |${nl}          printf 'done\\x0a'"
  check h18-escape-x0a-in-a-run-block-above-the-cache-step PASS 'jobs: 2, uses keys: 8, findings: 0'
  lines h22-trailing-blank-after-with 15 "$pin_with "
  check h22-trailing-blank-after-with FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 15: '        with: ' stands where this lint reads line 2 of the Rust cache step of the job macos, '${pin_with}'; it differs from the line to write first at column 14, which holds a blank where the line to write holds the line's end; write '${pin_with}'; the pin holds the with line of the Rust cache step in the variable pin_with at the top of scripts/r1/ci.d/17-workflow-lint.sh, and a change of it is a ruled change)"
  lines pr-x03-timezone-under-the-cron-item 7 "$pin_cron${nl}      timezone: Asia/Tokyo"
  check pr-x03-timezone-under-the-cron-item FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 8: '      timezone: Asia/Tokyo' stands under the schedule below its cron line; this lint reads the cron line and nothing else under '  schedule:'; take it out; the pin holds the cron line in the variable pin_cron at the top of scripts/r1/ci.d/17-workflow-lint.sh, and a change of it is a ruled change)"
  lines pr-x01-second-cache-step-with-defaults-in-macos 17 "$pin_save${nl}$pin_uses"
  check pr-x01-second-cache-step-with-defaults-in-macos FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 18: the job macos holds a second Rust cache step, beside the one at line 14; this lint reads one Rust cache step in a job; take one out; and 1 more above)"
  lines pr-x02-f03-pinned-lines-under-env-and-a-with-of-its-own 17 "$pin_save${nl}        with:${nl}          cache-on-failure: true" 15 "        env:"
  check pr-x02-f03-pinned-lines-under-env-and-a-with-of-its-own FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 15: '        env:' stands where this lint reads line 2 of the Rust cache step of the job macos, '${pin_with}'; it differs from the line to write first at column 9, which holds the sign e where the line to write holds the sign w; write '${pin_with}'; the pin holds the with line of the Rust cache step in the variable pin_with at the top of scripts/r1/ci.d/17-workflow-lint.sh, and a change of it is a ruled change)"
  lines g10-macos-second-step-subpath 17 "$pin_save${nl}      - uses: Swatinem/rust-cache/sub@d1527c8f67ef9af357290475d4af4b0fab8f12e0 # v2.9.3"
  check g10-macos-second-step-subpath FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 18: '      - uses: Swatinem/rust-cache/sub@d1527c8f67ef9af357290475d4af4b0fab8f12e0 # v2.9.3' stands where this lint reads line 1 of the Rust cache step of the job macos, '${pin_uses}'; it differs from the line to write first at column 34, which holds the sign / where the line to write holds the sign @; write '${pin_uses}'; the pin holds the uses line of the Rust cache step in the variable pin_uses at the top of scripts/r1/ci.d/17-workflow-lint.sh, and a change of it is a ruled change; and 1 more above)"
  lines x03-macos-second-step-trailing-slash 17 "$pin_save${nl}      - uses: Swatinem/rust-cache/@d1527c8f67ef9af357290475d4af4b0fab8f12e0 # v2.9.3"
  check x03-macos-second-step-trailing-slash FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 18: '      - uses: Swatinem/rust-cache/@d1527c8f67ef9af357290475d4af4b0fab8f12e0 # v2.9.3' stands where this lint reads line 1 of the Rust cache step of the job macos, '${pin_uses}'; it differs from the line to write first at column 34, which holds the sign / where the line to write holds the sign @; write '${pin_uses}'; the pin holds the uses line of the Rust cache step in the variable pin_uses at the top of scripts/r1/ci.d/17-workflow-lint.sh, and a change of it is a ruled change; and 1 more above)"
  lines x07-macos-step-named-with-a-path 14 "      - uses: Swatinem/rust-cache/sub@d1527c8f67ef9af357290475d4af4b0fab8f12e0 # v2.9.3"
  check x07-macos-step-named-with-a-path FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 14: '      - uses: Swatinem/rust-cache/sub@d1527c8f67ef9af357290475d4af4b0fab8f12e0 # v2.9.3' stands where this lint reads line 1 of the Rust cache step of the job macos, '${pin_uses}'; it differs from the line to write first at column 34, which holds the sign / where the line to write holds the sign @; write '${pin_uses}'; the pin holds the uses line of the Rust cache step in the variable pin_uses at the top of scripts/r1/ci.d/17-workflow-lint.sh, and a change of it is a ruled change)"
  lines g03-windows-second-step-other-case 26 "$pin_save${nl}      - uses: SWATINEM/RUST-CACHE@d1527c8f67ef9af357290475d4af4b0fab8f12e0 # v2.9.3"
  check g03-windows-second-step-other-case FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 27: '      - uses: SWATINEM/RUST-CACHE@d1527c8f67ef9af357290475d4af4b0fab8f12e0 # v2.9.3' stands where this lint reads line 1 of the Rust cache step of the job windows, '${pin_uses}'; it differs from the line to write first at column 16, which holds the sign W where the line to write holds the sign w; write '${pin_uses}'; the pin holds the uses line of the Rust cache step in the variable pin_uses at the top of scripts/r1/ci.d/17-workflow-lint.sh, and a change of it is a ruled change; and 1 more above)"
  lines e18-comment-names-the-action-in-another-step 13 "$checkout${nl}        # Swatinem/rust-cache comes next"
  check e18-comment-names-the-action-in-another-step FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 14: '        # Swatinem/rust-cache comes next' names the action rust-cache; this lint reads the action's name on the first line of a job's Rust cache step alone, the line '${pin_uses}'; take the name out of this line; the pin holds the uses line of the Rust cache step in the variable pin_uses at the top of scripts/r1/ci.d/17-workflow-lint.sh, and a change of it is a ruled change)"
  lines e07-macos-save-if-trailing-blank 17 "$pin_save "
  check e07-macos-save-if-trailing-blank FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 17: '          save-if: \${{ github.ref == 'refs/heads/main' && github.event_name != 'schedule' }} ' stands where this lint reads line 4 of the Rust cache step of the job macos, '${pin_save}'; it differs from the line to write first at column 93, which holds a blank where the line to write holds the line's end; write '${pin_save}'; the pin holds the save-if line in the variable pin_save at the top of scripts/r1/ci.d/17-workflow-lint.sh, and a change of it is a ruled change)"
  lines f05-macos-quoted-job-key 9 '  "macos":'
  check f05-macos-quoted-job-key FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 9: '  \"macos\":' stands at two blanks under 'jobs:', where this lint reads a job's key; it reads that key as the line '  macos:' or '  windows:' alone, with no comment, no quotes and no blank before the colon; write one of those lines)"
  lines i07-macos-input-cache-on-failure 17 "$pin_save${nl}          cache-on-failure: true"
  check i07-macos-input-cache-on-failure FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 18: '          cache-on-failure: true' follows the four lines of the Rust cache step of the job macos; this lint reads that step as those four lines alone, so the next line begins a step, is a job's key or stands in column 1; take it out or begin a step there)"
  lines c01-macos-step-if-not-pull-request 17 "$pin_save${nl}        if: github.event_name != 'pull_request'"
  check c01-macos-step-if-not-pull-request FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 18: '        if: github.event_name != 'pull_request'' follows the four lines of the Rust cache step of the job macos; this lint reads that step as those four lines alone, so the next line begins a step, is a job's key or stands in column 1; take it out or begin a step there)"
  lines c07-other-commit-of-the-action 14 "      - uses: Swatinem/rust-cache@0123456789abcdef0123456789abcdef01234567 # v2.9.3"
  check c07-other-commit-of-the-action FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 14: '      - uses: Swatinem/rust-cache@0123456789abcdef0123456789abcdef01234567 # v2.9.3' stands where this lint reads line 1 of the Rust cache step of the job macos, '${pin_uses}'; it differs from the line to write first at column 35, which holds the sign 0 where the line to write holds the sign d; write '${pin_uses}'; the pin holds the uses line of the Rust cache step in the variable pin_uses at the top of scripts/r1/ci.d/17-workflow-lint.sh, and a change of it is a ruled change)"
  lines j1-macos-keys-at-three-blanks-with-a-timeout-in-a-name-block 17 - 16 - 15 - 14 - 13 - 12 - 11 - 10 - 9 "  macos:${nl}   name: |${nl}    timeout-minutes: 30${nl}   runs-on: macos-15${nl}   timeout-minutes: 600${nl}   steps:${nl}     - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1${nl}${pin_uses# }${nl}${pin_with# }${nl}${pin_prefix# }${nl}${pin_save# }"
  check j1-macos-keys-at-three-blanks-with-a-timeout-in-a-name-block FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 10: '   name: |' is the first line of the job macos and stands at 3 blanks; this lint reads a job's own keys at four blanks; write them at four blanks)"
  lines byte-ff-beside-the-save-if-of-the-base 17 "$base_save" 2 "  push: # $(printf '\377')"
  if locale -a 2>/dev/null | grep -q -x 'en_US.UTF-8'; then
    lc_before=${LC_ALL-unset}
    LC_ALL=en_US.UTF-8
    export LC_ALL
    check byte-ff-beside-the-save-if-of-the-base FAIL ".github/workflows/ci.yml line 17: '${base_save}' stands where this lint reads line 4 of the Rust cache step of the job macos"
    if [ "$lc_before" = unset ]; then unset LC_ALL; else LC_ALL=$lc_before; export LC_ALL; fi
  else
    printf 'case %s/%s want=FAIL got=no-locale (en_US.UTF-8, the locale of the hosted Mac, is not on this machine)\n' "$name" byte-ff-beside-the-save-if-of-the-base
    wrong="$wrong byte-ff-beside-the-save-if-of-the-base"
  fi
  lines g06-macos-second-step-hex-escape 17 "$pin_save${nl}      - uses: \"Swatinem/rust\\x2dcache@d1527c8f67ef9af357290475d4af4b0fab8f12e0\""
  check g06-macos-second-step-hex-escape FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 18: '      - uses: \"Swatinem/rust\x2dcache@d1527c8f67ef9af357290475d4af4b0fab8f12e0\"' stands where this lint reads line 1 of the Rust cache step of the job macos, '${pin_uses}'; it differs from the line to write first at column 15, which holds the sign \" where the line to write holds the sign S; write '${pin_uses}'; the pin holds the uses line of the Rust cache step in the variable pin_uses at the top of scripts/r1/ci.d/17-workflow-lint.sh, and a change of it is a ruled change; and 1 more above)"
  lines two-escapes-in-the-name-of-a-second-step 17 "$pin_save${nl}      - uses: \"Swatinem\\x2frust\\x2dcache@d1527c8f67ef9af357290475d4af4b0fab8f12e0\""
  check two-escapes-in-the-name-of-a-second-step FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 18: '      - uses: \"Swatinem\x2frust\x2dcache@d1527c8f67ef9af357290475d4af4b0fab8f12e0\"' stands where this lint reads line 1 of the Rust cache step of the job macos, '${pin_uses}'; it differs from the line to write first at column 15, which holds the sign \" where the line to write holds the sign S; write '${pin_uses}'; the pin holds the uses line of the Rust cache step in the variable pin_uses at the top of scripts/r1/ci.d/17-workflow-lint.sh, and a change of it is a ruled change; and 1 more above)"
  lines h06-renamed-step 14 "      - name: Rust build cache${nl}        uses: Swatinem/rust-cache@d1527c8f67ef9af357290475d4af4b0fab8f12e0 # v2.9.3"
  check h06-renamed-step FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 15: '        uses: Swatinem/rust-cache@d1527c8f67ef9af357290475d4af4b0fab8f12e0 # v2.9.3' names the action rust-cache; this lint reads the action's name on the first line of a job's Rust cache step alone, the line '${pin_uses}'; take the name out of this line; the pin holds the uses line of the Rust cache step in the variable pin_uses at the top of scripts/r1/ci.d/17-workflow-lint.sh, and a change of it is a ruled change)"
  lines schedule-key-with-a-comment 6 "  schedule: # weekly"
  check schedule-key-with-a-comment FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 6: '  schedule: # weekly' names the schedule or a cron line in the block on:; this lint reads the schedule as the line '  schedule:' alone and under it the one line '${pin_cron}'; write it so or take this line out; the pin holds the cron line in the variable pin_cron at the top of scripts/r1/ci.d/17-workflow-lint.sh, and a change of it is a ruled change; and 1 more above)"
  lines first-event-at-four-blanks 7 "      - cron: '17 3 * * 0'" 6 "    schedule:" 5 "    pull_request:" 4 "      tags: ['**']" 3 "      branches-ignore: ['land-*', 'land-*/**']" 2 "    push:"
  check first-event-at-four-blanks FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 2: '    push:' is the first line under 'on:' and stands at 4 blanks; this lint reads the events of on: at two blanks; write each event at two blanks)"
  lines second-on-line 28 "  contents: read${nl}on:${nl}  workflow_dispatch:"
  check second-on-line FAIL ".github/workflows/ci.yml line 29: a second line 'on:'; this lint reads one block on:, the one at line 1; write the events of this block under that one"
  lines jobs-key-with-a-comment 8 "jobs: # the two jobs"
  check jobs-key-with-a-comment FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 8: 'jobs: # the two jobs' holds the key jobs in another form; this lint reads the jobs under the line 'jobs:' alone in column 1; write 'jobs:')"
  lines comment-in-the-on-block-names-the-schedule 2 "  push:${nl}  # the weekly schedule is below"
  check comment-in-the-on-block-names-the-schedule FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 3: '  # the weekly schedule is below' names the schedule or a cron line in the block on:; this lint reads the schedule as the line '  schedule:' alone and under it the one line '${pin_cron}'; take this line out; the pin holds the cron line in the variable pin_cron at the top of scripts/r1/ci.d/17-workflow-lint.sh, and a change of it is a ruled change)"
  lines s01-second-cron-line 7 "$pin_cron${nl}    - cron: '0 4 * * 3'"
  check s01-second-cron-line FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 8: '    - cron: '0 4 * * 3'' stands under the schedule below its cron line; this lint reads the cron line and nothing else under '  schedule:'; take it out; the pin holds the cron line in the variable pin_cron at the top of scripts/r1/ci.d/17-workflow-lint.sh, and a change of it is a ruled change)"
  lines s02-second-schedule-key 7 "$pin_cron${nl}  schedule:${nl}$pin_cron"
  check s02-second-schedule-key FAIL ".github/workflows/ci.yml line 8: a second line '  schedule:'; this lint reads one, the one at line 6; take this one out"
  lines line-at-three-blanks-in-a-job 10 "    runs-on: macos-15${nl}   env: {}"
  check line-at-three-blanks-in-a-job FAIL ".github/workflows/ci.yml line 11: '   env: {}' stands at 3 blanks in the job macos; this lint reads a job's own keys at four blanks and its steps at six; write the line at one of those"
  lines line-at-five-blanks-in-the-steps 13 "$checkout${nl}     name: x"
  check line-at-five-blanks-in-the-steps FAIL ".github/workflows/ci.yml line 14: '     name: x' stands at 5 blanks in the job macos; this lint reads a job's own keys at four blanks and its steps at six; write the line at one of those"
  lines first-line-of-a-job-at-six-blanks 10 "      runs-on: macos-15"
  check first-line-of-a-job-at-six-blanks FAIL ".github/workflows/ci.yml line 10: '      runs-on: macos-15' is the first line of the job macos and stands at 6 blanks; this lint reads a job's own keys at four blanks; write them at four blanks"
  lines quoted-own-key-of-a-job 10 "    runs-on: macos-15${nl}    \"name\": macos"
  check quoted-own-key-of-a-job FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 11: '    \"name\": macos' stands at four blanks in the job macos, where this lint reads an own key of the job; it reads such a key as a plain word of letters, digits, - and _, a colon, and then the line's end or a blank and the value; write the key so)"
  lines dash-alone-on-its-line 13 "      -${nl}        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1"
  check dash-alone-on-its-line FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 13: '      -' is a dash alone in the steps of the job macos; this lint reads a step as beginning at six blanks, a dash, a blank and the step's first key; write that key on the dash's line)"
  lines line-at-six-blanks-that-is-no-step 13 "$checkout${nl}      name: x"
  check line-at-six-blanks-that-is-no-step FAIL ".github/workflows/ci.yml line 14: '      name: x' stands at six blanks in the steps of the job macos and is no step's first line; this lint reads a line at six blanks there as the first line of a step, with a dash and a blank; write it so"
  lines job-key-where-the-with-line-stands 17 - 16 - 15 "    continue-on-error: true"
  check job-key-where-the-with-line-stands FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 15: '    continue-on-error: true' stands where this lint reads line 2 of the Rust cache step of the job macos, '${pin_with}'; it differs from the line to write first at column 5, which holds the sign c where the line to write holds a blank; write '${pin_with}'; the pin holds the with line of the Rust cache step in the variable pin_with at the top of scripts/r1/ci.d/17-workflow-lint.sh, and a change of it is a ruled change)"
  lines cache-step-cut-by-the-end-of-the-file 29 - 28 - 27 - 26 - 25 - 1 "permissions:${nl}  contents: read${nl}on:"
  check cache-step-cut-by-the-end-of-the-file FAIL ".github/workflows/ci.yml line 25: the Rust cache step of the job windows ends with the file before its line 3; write after its last line the line '${pin_prefix}'; the pin holds the prefix-key line in the variable pin_prefix at the top of scripts/r1/ci.d/17-workflow-lint.sh, and a change of it is a ruled change"
  lines schedule-line-at-the-end-of-the-file 29 "name: ci${nl}on:${nl}  push:${nl}  schedule:" 7 - 6 - 5 - 4 - 3 - 2 - 1 -
  check schedule-line-at-the-end-of-the-file FAIL ".github/workflows/ci.yml line 25: the file ends where this lint reads the cron line under '  schedule:'; write '${pin_cron}'; the pin holds the cron line in the variable pin_cron at the top of scripts/r1/ci.d/17-workflow-lint.sh, and a change of it is a ruled change"
  lines first-step-at-eight-blanks 13 "        - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1"
  check first-step-at-eight-blanks FAIL ".github/workflows/ci.yml line 13: '        - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1' is the first line in the steps of the job macos and stands at 8 blanks; this lint reads a step as beginning at six blanks, a dash and a blank; write the step so"
  lines no-on-block 7 - 6 - 5 - 4 - 3 - 2 - 1 -
  check no-on-block FAIL ".github/workflows/ci.yml: no line of the file is 'on:' alone in column 1; this lint reads the events under that line, with the line '  schedule:' and under it the line '${pin_cron}'; write them; the pin holds the cron line in the variable pin_cron at the top of scripts/r1/ci.d/17-workflow-lint.sh, and a change of it is a ruled change"
  lines n27-checkout-comment-names-the-action 13 "      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # before rust-cache"
  check n27-checkout-comment-names-the-action FAIL "FAIL 17-workflow-lint (.github/workflows/ci.yml line 13: '      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # before rust-cache' is the first line of a step and names the action rust-cache in its comment; this lint reads the action's name on the first line of a job's Rust cache step alone, the line '${pin_uses}', and in no comment; take the name out of the comment; the pin holds the uses line of the Rust cache step in the variable pin_uses at the top of scripts/r1/ci.d/17-workflow-lint.sh, and a change of it is a ruled change)"
  lines push-filter-missing 3 -
  check push-filter-missing FAIL 'write the push filters and the pull_request and schedule events'
  lines main-also-ignored 3 "    branches-ignore: ['land-*', 'land-*/**', 'main']"
  check main-also-ignored FAIL 'write the push filters and the pull_request and schedule events'
  lines scratch-branch-also-ignored 3 "    branches-ignore: ['land-*', 'land-*/**', 'scratch/**']"
  check scratch-branch-also-ignored FAIL 'write the push filters and the pull_request and schedule events'
  lines nested-landing-branch-tested 3 "    branches-ignore: ['land-*']"
  check nested-landing-branch-tested FAIL 'write the push filters and the pull_request and schedule events'
  lines only-nested-landing-branches-ignored 3 "    branches-ignore: ['land-*/**']"
  check only-nested-landing-branches-ignored FAIL 'write the push filters and the pull_request and schedule events'
  lines no-tag-filter 4 -
  check no-tag-filter FAIL 'write the push filters and the pull_request and schedule events'
  lines only-top-level-tags 4 "    tags: ['*']"
  check only-top-level-tags FAIL 'write the push filters and the pull_request and schedule events'
  lines push-event-missing 4 - 3 - 2 -
  check push-event-missing FAIL 'write the push filters and the pull_request and schedule events'
  lines pull-request-event-missing 5 -
  check pull-request-event-missing FAIL 'write the push filters and the pull_request and schedule events'
  lines pull-request-limited-to-main 5 "  pull_request:${nl}    branches: [main]"
  check pull-request-limited-to-main FAIL 'write the push filters and the pull_request and schedule events'
  lines extra-workflow-event 5 "  pull_request:${nl}  workflow_dispatch:"
  check extra-workflow-event FAIL 'write the push filters and the pull_request and schedule events'
  lines push-limited-to-paths 4 "    tags: ['**']${nl}    paths: [README.md]"
  check push-limited-to-paths FAIL 'write the push filters and the pull_request and schedule events'
  tree another-actionlint-version ''
  printf '#!/bin/sh\ncase "%s1" in -version) printf "1.6.0\\n" ;; *) exec "%s" "%s@" ;; esac\n' "\$" "$STUB_REAL" "\$" >"$work/old-tools/actionlint"
  chmod +x "$work/old-tools/actionlint"
  path=$work/old-tools:$PATH
  check another-actionlint-version FAIL 'actionlint 1.6.0 runs here, and the pinned version is 1.7.12'
  tree actionlint-missing ''
  path=$work/no-tools
  check actionlint-missing SKIP 'SKIP 17-workflow-lint (actionlint is not installed)'
  full actionlint-installed-in-ci macos
  path=$work/tools:/usr/bin:/bin
  ci=true
  check actionlint-installed-in-ci PASS 'jobs: 2, uses keys: 8, findings: 0'

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
