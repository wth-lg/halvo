#!/bin/sh
# jobs: macos windows
# rule: No tracked file under backend/ but a .md file reads or imports a file outside backend/ and formal/r7-ledger/, and no tracked file of the app (outside those two roots, docs/ and .md files) reads a file under backend/, but scripts/r7/, the step of scripts/prerequisites.sh that installs backend/'s packages, and account/schema/ for the four files of the wire contract that it copies.
# shellcheck disable=SC2016
set -u
name=24-r7-wall
tab=$(printf '\t')
contract='backend/contract/schema/v1.json backend/contract/routes/v1.json backend/contract/samples/v1.json backend/gate/prompt-budget.json'

step_lines() {
  awk '
    /^ensure_pnpm\(\) \{$/ || /^ensure_backend\(\) \{$/ { in_function = 1 }
    in_function { print NR; if ($0 == "}") in_function = 0; next }
    /^  new_backend_case\(\) \{$/ || /^  run_ensure_backend\(\) \{$/ { in_helper = 1 }
    in_helper { print NR; if ($0 == "  }") in_helper = 0; next }
    /^  name=/ { held = 0; in_case = 1; of_the_step = 0 }
    in_case {
      line[++held] = NR
      if ($0 ~ /^  new_backend_case /) of_the_step = 1
      if ($0 == "  verdict \"$name\"") {
        if (of_the_step) for (i = 1; i <= held; i++) print line[i]
        in_case = 0
      }
    }
  '
}

judge() {
  awk -F "$tab" -v q="'" -v contract="$contract" -v problems="$1" -v tally="$2" '
    function problem(where, why) { printf "%s: %s\n", where, why >>problems; refused[side]++ }
    function lower(s) { return tolower(s) }
    function inside(r,   l) {
      l = lower(r)
      return l == "backend" || index(l, "backend/") == 1 || l == "formal/r7-ledger" || index(l, "formal/r7-ledger/") == 1
    }
    function under_backend(r,   l) { l = lower(r); return l == "backend" || index(l, "backend/") == 1 }
    function dir_of(p,   k) { k = match(p, /\/[^\/]*$/); return k ? substr(p, 1, k - 1) : "" }
    function side_of(p) {
      if (index(p, "backend/") == 1) return "backend"
      if (index(p, "formal/r7-ledger/") == 1) return "model"
      return "app"
    }
    function resolve(base, sg, from, n,   st, d, i, k, out) {
      d = base == "" ? 0 : split(base, st, "/")
      for (i = from; i <= n; i++) {
        if (sg[i] == "" || sg[i] == ".") continue
        if (sg[i] == "..") { if (d == 0) return "^"; d--; continue }
        st[++d] = sg[i]
      }
      out = ""
      for (k = 1; k <= d; k++) out = out (k > 1 ? "/" : "") st[k]
      return out
    }
    function glob_matches(word, glob,   re) {
      if (glob !~ /[*?[]/ || glob !~ /[a-z]/) return 0
      re = glob
      gsub(/[.+^$(){}|\\]/, "\\\\&", re)
      gsub(/\*+/, ".*", re)
      gsub(/\?/, ".", re)
      return word ~ ("^" re "$")
    }
    function normal(s,   k, m) {
      gsub(/\\\\/, "/", s)
      while (k = match(s, /\\[A-Za-z0-9_.-]/)) s = substr(s, 1, k - 1) "/" substr(s, k + 1)
      while (match(s, dot_chain)) {
        m = substr(s, RSTART, RLENGTH)
        sub(sep_at_end, "/", m)
        s = substr(s, 1, RSTART - 1) m substr(s, RSTART + RLENGTH)
      }
      return s
    }
    function anchor_of(left,   l) {
      l = left
      sub(quotes_at_end, "", l)
      if (l ~ /dirname[ \t]+(--[ \t]+)?"?\$\{?0\}?"?\)$/) return "file"
      if (l ~ /\$\{0%\/\*\}$/) return "file"
      if (l ~ /(__dirname|import\.meta\.dirname)[ \t]*\+[ \t]*$/) return "file"
      if (l ~ /[)}]$/ || l ~ /\$[A-Za-z_][A-Za-z0-9_]*$/ || l ~ /\$[0-9@*]$/ || l ~ /\+[ \t]*$/) return "root"
      return "none"
    }
    function token(tok, s, pos,   c, kind, base, sg, n, i, from, r, climbs, prev, w, tail, k) {
      sub(/^\\+/, "", tok)
      if (match(tok, /^(file|link|portal|workspace):/)) tok = substr(tok, RLENGTH + 1)
      kind = ""
      if (match(tok, /^[A-Za-z0-9_~^@]*:[^\/]/) && index(substr(tok, RLENGTH), "/")) {
        RLENGTH--
        w = substr(tok, 1, RLENGTH - 1)
        sub(/[~^][0-9~^]*$/, "", w)
        if (w == "" || w == "HEAD" || w == "@" || (w ~ /^[0-9a-fA-F]+$/ && length(w) >= 7)) {
          tok = substr(tok, RLENGTH + 1)
          kind = "top"
        }
      }
      if (tok == "") return
      c = substr(tok, 1, 1)
      if (c == "~") return
      if (side == "backend" && kind == "" && !index(tok, "..") && !((c == "/" || c == "$") && index(substr(tok, 2), "/"))) return
      if (side == "app" && lower(tok) !~ /backend|[*?]/) return
      n = split(tok, sg, "/")
      from = 1
      if (kind == "top") from = 1
      else if (c == "/") {
        kind = anchor_of(substr(s, 1, pos - 1))
        if (kind == "none") kind = "absolute"
        from = 2
      } else if (c == "$") {
        kind = "root"
        from = 2
      } else kind = "plain"
      if (kind == "file" || kind == "plain") base = dir_of(path)
      else if (kind == "top") base = ""
      else base = "backend"
      climbs = kind == "top"
      for (i = from; i <= n; i++) if (sg[i] == "..") climbs = 1
      if (side == "backend") {
        if (kind == "absolute") return
        if (climbs) {
          r = resolve(base, sg, from, n)
          if (r == "^") { problem(where, "climbs above the repository: " tok); return }
          if (r == "") { problem(where, "reads the top of the repository: " tok); return }
          if (!inside(r)) { problem(where, "reads " r " outside backend/ and formal/r7-ledger/: " tok); return }
        }
        prev = kind == "plain" ? "plain" : ""
        for (i = from; i <= n; i++) {
          if (sg[i] == "" || sg[i] == ".") continue
          if ((prev == "" || prev == "..") && (lower(sg[i]) in app_only)) {
            if (lower(sg[i]) == "formal" && i < n && lower(sg[i + 1]) == "r7-ledger") { prev = sg[i]; continue }
            problem(where, "names " sg[i] ", a top-level entry of the app: " tok)
            return
          }
          prev = sg[i]
        }
        return
      }
      prev = ""
      for (i = 1; i <= n; i++) {
        w = lower(sg[i])
        if (w == "backend" && (n >= 2 || kind != "plain")) break
        if (kind != "absolute" && i < n && sg[i + 1] != "" && sg[i + 1] != "." && sg[i + 1] != ".." && w != "." && w != ".." && (prev == "" || prev == "..") && glob_matches("backend", w)) break
        if (w != "" && w != ".") prev = w
      }
      if (i > n) return
      tail = sg[i]
      for (k = i + 1; k <= n; k++) tail = tail "/" sg[k]
      if (index(path, "account/schema/") == 1 && (tail in copied)) return
      problem(where, "reads under backend/: " tok)
    }
    function words(s,   rest, off, k, w, left) {
      rest = s
      off = 0
      while (match(rest, quoted_word)) {
        w = substr(rest, RSTART + 1, RLENGTH - 2)
        left = substr(s, 1, off + RSTART - 1)
        off += RSTART + RLENGTH - 1
        rest = substr(rest, RSTART + RLENGTH)
        if (left !~ /(,|\/|\+|join\(|push\()[ \t]*$/) continue
        if (side == "app" && lower(w) == "backend") { problem(where, "joins the word " w " to a path"); return }
      }
    }
    function line_of(s,   rest, off, tok, low) {
      s = normal(s)
      if (side == "app") {
        low = lower(s)
        if (low ~ enters) {
          problem(where, "enters backend/ as a directory")
          return
        }
      }
      words(s)
      rest = s
      off = 0
      while (match(rest, token_chars)) {
        tok = substr(rest, RSTART, RLENGTH)
        pos = off + RSTART
        off += RSTART + RLENGTH - 1
        rest = substr(rest, RSTART + RLENGTH)
        before = refused[side] + 0
        token(tok, s, pos)
        if (refused[side] + 0 > before) return
      }
    }
    BEGIN {
      qs = "[\"" q "`]"
      token_chars = "[^][ \t\"" q "`()<>,;{}|&=]+"
      quoted_word = qs "[A-Za-z0-9_.-]+" qs
      quotes_at_end = qs "+$"
      sep = qs "[ \t]*(,|\\)[ \t]*\\.[ \t]*join[ \t]*\\(|/)[ \t]*" qs
      sep_at_end = sep "$"
      dot_chain = "\\.\\.(/[^\"" q "`]*)?" sep
      enters = "(^|[^a-z0-9_-])(cd|pushd|-c|--cwd|--dir|--prefix|--manifest-path|working-directory:)[ \t=]+" qs "?(\\./)?backend/?" qs "?([^a-z0-9_./-]|$)"
      split(contract, list, " ")
      for (k in list) copied[list[k]] = 1
    }
    FILENAME == ARGV[1] { app_only[$0] = 1; next }
    FILENAME == ARGV[2] { step[$0] = 1; steps++; next }
    FILENAME == ARGV[3] {
      path = $1
      links++
      side = side_of(path)
      if (side == "model" || (side == "app" && index(path, "scripts/r7/") == 1)) next
      where = path " (a link)"
      n = split($2, sg, "/")
      if (side == "backend") {
        if ($2 ~ /^\// || $2 ~ /^[A-Za-z]:/) { problem(where, "points to an absolute path"); next }
        r = resolve(dir_of(path), sg, 1, n)
        if (r == "^" || !inside(r)) problem(where, "points outside backend/ and formal/r7-ledger/")
        next
      }
      r = resolve(dir_of(path), sg, 1, n)
      if (r != "^" && under_backend(r)) { problem(where, "points under backend/"); next }
      for (k = 1; k <= n; k++) if (lower(sg[k]) == "backend") { problem(where, "points to a path through backend"); next }
      next
    }
    FILENAME == ARGV[4] { known[$0] = 1; files++; if (index($0, "backend/") == 1) backend_files++; next }
    {
      line = $0
      path = ""
      for (start = 1; (k = index(substr(line, start), ":")) > 0; start += k) {
        candidate = substr(line, 1, start + k - 2)
        rest = substr(line, start + k)
        if ((candidate in known) && match(rest, /^[0-9]+:/)) {
          path = candidate
          number = substr(rest, 1, RLENGTH - 1)
          text = substr(rest, RLENGTH + 1)
          break
        }
      }
      if (path == "") { unparsed++; next }
      side = side_of(path)
      if (side == "model") next
      if (lower(path) ~ /\.md$/) next
      if (side == "app") {
        if (index(path, "docs/") == 1 || index(path, "scripts/r7/") == 1) next
        if (path == "scripts/prerequisites.sh" && (number in step)) next
      }
      lines++
      where = path ":" number
      line_of(text)
    }
    END {
      printf "files=%d backend-files=%d links=%d step-lines=%d lines=%d backend-refused=%d app-refused=%d unparsed=%d\n", files, backend_files, links, steps, lines, refused["backend"], refused["app"], unparsed >tally
      if (unparsed) printf "git grep printed %d lines that name no tracked path\n", unparsed >>problems
      if (!files) printf "no tracked file\n" >>problems
    }
  ' "$3" "$4" "$5" "$6" "$7"
}

check_in() {
  repo=$1
  work=$2
  problems=$3
  : >"$problems" || return 2
  cd "$repo" || return 2
  git -c core.quotepath=off ls-files -s >"$work/index" || return 2
  awk -F "$tab" -v work="$work" -v problems="$problems" '
    substr($2, 1, 1) == "\"" { quoted = 1 }
    {
      print $2 >(work "/paths")
      split($1, m, " ")
      if (m[1] == "120000") print m[2] "\t" $2 >(work "/link-ids")
      n = split($2, seg, "/")
      if (seg[1] == "backend") for (i = 2; i <= n; i++) inner[tolower(seg[i])] = 1
      else top[tolower(seg[1])] = 1
    }
    END {
      for (t in top) if (t != "backend" && !(t in inner)) print t >(work "/app-only")
      if (quoted) printf "git quotes a tracked path, so the check cannot read it\n" >>problems
    }
  ' "$work/index" || return 2
  : >>"$work/paths"
  : >>"$work/link-ids"
  : >>"$work/app-only"
  : >"$work/links"
  while IFS="$tab" read -r id path; do
    target=$(git cat-file blob "$id") || return 2
    printf '%s\t%s\n' "$path" "$target" >>"$work/links"
  done <"$work/link-ids"
  : >"$work/step"
  if git cat-file blob :scripts/prerequisites.sh >"$work/prerequisites" 2>/dev/null; then
    step_lines <"$work/prerequisites" >"$work/step" || return 2
  fi
  pattern=$(awk '{ gsub(/[][.+*?(){}|^$\\]/, "\\\\&"); printf "|(^|[^a-z0-9_-])%s", $0 }' "$work/app-only") || return 2
  git -c core.quotepath=off grep --cached -I -n -i -E -e "[.][.]$pattern" -- backend/ >"$work/lines"
  found=$?
  [ "$found" -le 1 ] || return 2
  git -c core.quotepath=off grep --cached -I -n -i -E -e 'backend|[*?][^[:space:]]*/' -- . ':!backend/' ':!formal/r7-ledger/' >>"$work/lines"
  found=$?
  [ "$found" -le 1 ] || return 2
  judge "$problems" "$work/tally" "$work/app-only" "$work/step" "$work/links" "$work/paths" "$work/lines" || return 2
  read -r counts <"$work/tally" || return 2
  [ ! -s "$problems" ]
}

measure() {
  repo=${CI_D_REPO:-$(git rev-parse --show-toplevel)} || exit 2
  artifacts=${CI_D_ARTIFACTS:-$repo/target/ci-artifacts/$name}
  mkdir -p "$artifacts" || exit 2
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  check_in "$repo" "$work" "$artifacts/problems.txt"
  code=$?
  found=$(awk 'END { print NR }' "$artifacts/problems.txt")
  printf '%s problems=%s\n' "${counts:-unmeasured}" "$found"
  case $code in
    0) printf 'PASS %s\n' "$name" ;;
    1) printf 'FAIL %s (%s reads across the wall; the list is problems.txt among the artifacts)\n' "$name" "$found"; exit 1 ;;
    *) printf 'FAIL %s (setup)\n' "$name"; exit 1 ;;
  esac
}

put() {
  case $1 in
    */*) mkdir -p "$dir/${1%/*}" || exit 2 ;;
  esac
  printf '%s\n' "$2" >"$dir/$1" || exit 2
}

fixture() {
  dir=$scratch/$1
  cp -R "$scratch/base" "$dir" || exit 2
}

base() {
  dir=$scratch/base
  mkdir -p "$dir" || exit 2
  git -C "$dir" init -q || exit 2
  put backend/package.json '{ "name": "halvo-backend", "devDependencies": { "wrangler": "4.0.0" } }'
  put backend/tsconfig.json '{ "compilerOptions": { "strict": true }, "include": ["src", "test", "../formal/r7-ledger/check.mjs"] }'
  put backend/src/types.ts 'export type Id = string;'
  put backend/test/ledger/cases.ts 'import type { Id } from "../../src/types.ts";
import { join } from "node:path";
const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const model = join(root, "..", "formal", "r7-ledger");
const same = new URL("../../", import.meta.url);'
  put backend/gate/checks/95-model.sh 'ROOT=${HALVO_ROOT:-$(cd "$(dirname "$0")/../.." && pwd -P)}
HALVO_ROOT=$ROOT node "$ROOT/../formal/r7-ledger/check.mjs"
(cd "$ROOT/../formal/r7-ledger" && git ls-files)
cd ..'
  put backend/README.md 'An example: import { x } from "../../crates/halvo-capture/src/lib.rs"; it reads ../src-tauri/ too.'
  put backend/contract/names/forbidden-names.json '{ "source": "app@685d846:crates/halvo-capture/src/event.rs:25" }'
  put backend/scripts/spread.mjs 'const all = [...one, ...two]; const range = "1..10"; const parent = "..";'
  put formal/r7-ledger/check.mjs 'import x from "../../src-tauri/src/main.rs";'
  put formal/r3-confirm/README.md 'the backend repository'
  put src-tauri/src/main.rs 'store_backend: unwired_store::DESCRIPTION,'
  put crates/halvo-capture/src/lib.rs '//! Per-OS backends live under macos/ and windows/. A size is 4 * 1024 / 2.'
  put backend/scripts/names-v0.sh 'ENUM_PATH=crates/halvo-capture/src/event.rs'
  put backend/test/billing/accounts.ts 'const ACCOUNT = "account"; const where = join(dir, "account");'
  put ui/index.html '<span id="backend"></span><script>$("backend").textContent = s.store_backend;</script>'
  put tools/run.sh 'echo "runtime backend=cpu"'
  put account/schema/vendored.txt 'f382 schema.v1.json backend/contract/schema/v1.json
09a4 routes.v1.json backend/contract/routes/v1.json'
  put account/schema/vendor.sh 'cp "$root/backend/contract/samples/v1.json" samples.v1.json
cp ../../backend/gate/prompt-budget.json prompt-budget.json'
  put scripts/r7/ci.d/21-r7-backend-test.sh 'cd backend && pnpm test
sh backend/scripts/stack.sh up'
  put scripts/prerequisites.sh 'ensure_pnpm() {
  field=$(sed -n p backend/package.json)
}

ensure_backend() {
  (cd backend && pnpm install --frozen-lockfile --ignore-scripts)
  sh backend/scripts/fetch-tools.sh
}

self_test() {
  name=a-case-of-cargo
  verdict "$name"

  new_backend_case() {
    mkdir -p "$case_dir/repo/backend/scripts"
  }

  name=a-case-of-the-step
  new_backend_case backend-no-pnpm
  [ -f "$case_dir/repo/backend/package.json" ]
  verdict "$name"
}

ensure_backend "$HOME/.local"'
  put docs/plan.md 'backend/src/index.ts holds the routes.'
  put docs/r7/gate/10-subscription.txt 'tree of backend/ at HEAD'
  put CHANGELOG.md '- Added: backend/ under the app repository.'
  stage
}

link() {
  blob=$(printf '%s' "$2" | git -C "$dir" hash-object -w --stdin) || exit 2
  git -C "$dir" update-index --add --cacheinfo "120000,$blob,$1" || exit 2
}

stage() {
  git -C "$dir" add -A . || exit 2
}

expect() {
  want=$1
  case_name=$2
  work=$(mktemp -d "$scratch/work.XXXXXX") || exit 2
  (check_in "$scratch/$case_name" "$work" "$work/problems") >/dev/null 2>&1
  code=$?
  case $code in
    0) got=PASS ;;
    1) got=FAIL ;;
    *) got=exit-$code ;;
  esac
  if [ "$got" = FAIL ] && [ -n "${3:-}" ] && ! grep -q -F -e "$3" "$work/problems"; then
    got=FAIL-for-another-cause
  fi
  printf 'case %s/%s want=%s got=%s\n' "$name" "$case_name" "$want" "$got"
  if [ "$got" != "$want" ]; then
    wrong="$wrong $case_name"
  elif [ "$want" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

case_with() {
  fixture "$1"
  put "$2" "$3"
  stage
  expect FAIL "$1" "$4"
}

self_test() {
  scratch=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  rejected=0
  wrong=
  base

  fixture good
  link backend/gate/fixtures/licenses/node_modules/native .pnpm/native@1.0.0/node_modules/native
  link backend/gate/fixtures/licenses/node_modules/.pnpm/native@1.0.0/node_modules/absent ../../../../linked/absent
  link scripts/r7/backend-link ../../backend/src
  link backend/model-link ../formal/r7-ledger/check.mjs
  expect PASS good

  fixture untracked-file-is-not-read
  put scripts/loose.sh 'cat backend/src/types.ts'
  expect PASS untracked-file-is-not-read

  case_with app-script-outside-r7-reads-backend scripts/check-anything.sh 'node backend/scripts/names-v0.mjs' 'scripts/check-anything.sh:1: reads under backend/'
  case_with app-script-in-a-look-alike-of-r7 scripts/r7x/run.sh 'cat backend/src/types.ts' 'scripts/r7x/run.sh:1: reads under backend/'
  case_with app-script-reads-backend-by-a-root-variable scripts/check-two.sh 'cat "$repo/backend/src/types.ts"' 'scripts/check-two.sh:1: reads under backend/'
  case_with app-script-enters-backend scripts/check-three.sh 'cd backend && pnpm test' 'scripts/check-three.sh:1: enters backend/'
  case_with app-workflow-runs-in-backend .github/workflows/ci.yml '      - run: pnpm test
        working-directory: backend' '.github/workflows/ci.yml:2: enters backend/'
  case_with app-crate-includes-a-contract-file crates/halvo-x/build.rs 'const S: &str = include_str!("../../backend/contract/schema/v1.json");' 'crates/halvo-x/build.rs:1: reads under backend/'
  case_with app-page-loads-a-backend-file ui/index.html '<script src="../backend/src/app.js"></script>' 'ui/index.html:1: reads under backend/'
  case_with app-joins-backend-to-a-path src-tauri/build.rs 'let p = root.join("backend").join("contract");' 'src-tauri/build.rs:1: joins the word backend'
  case_with app-names-backend-in-another-case tools/x.sh 'cat Backend/src/types.ts' 'tools/x.sh:1: reads under backend/'
  case_with app-names-backend-with-windows-separators tools/x.ps1 'Get-Content ..\backend\src\types.ts' 'tools/x.ps1:1: reads under backend/'
  case_with app-names-backend-by-a-glob Cargo.toml 'members = ["src-tauri", "b*/spikes/s3/crate"]' 'Cargo.toml:1: reads under backend/'
  case_with account-schema-reads-another-backend-file account/schema/vendor.sh 'cp backend/src/types.ts types.ts' 'account/schema/vendor.sh:1: reads under backend/'
  case_with account-crate-reads-the-contract-file account/crates/halvo-wire/build.rs 'let s = "../../../backend/contract/schema/v1.json";' 'account/crates/halvo-wire/build.rs:1: reads under backend/'

  fixture prerequisites-outside-the-step
  put scripts/prerequisites.sh 'ensure_backend() {
  (cd backend && pnpm install --frozen-lockfile --ignore-scripts)
}

ensure_tools() {
  sh backend/scripts/fetch-tools.sh
}'
  stage
  expect FAIL prerequisites-outside-the-step 'scripts/prerequisites.sh:6: reads under backend/'

  fixture prerequisites-case-of-another-step
  put scripts/prerequisites.sh 'self_test() {
  name=a-case-of-cargo
  cat backend/package.json
  verdict "$name"
}'
  stage
  expect FAIL prerequisites-case-of-another-step 'scripts/prerequisites.sh:3: reads under backend/'

  fixture app-link-into-backend
  link ui/contract ../backend/contract
  expect FAIL app-link-into-backend 'ui/contract (a link): points under backend/'

  case_with backend-imports-a-crate backend/src/capture.ts 'import { read } from "../../crates/halvo-capture/src/lib.rs";' 'backend/src/capture.ts:1: reads crates/halvo-capture/src/lib.rs outside'
  case_with backend-script-climbs-from-its-directory backend/scripts/peek.sh 'cat "$(dirname "$0")/../../src-tauri/tauri.conf.json"' 'backend/scripts/peek.sh:1: reads src-tauri/tauri.conf.json outside'
  case_with backend-script-climbs-from-a-root-variable backend/gate/checks/99-peek.sh 'cat "$ROOT/../ui/index.html"' 'backend/gate/checks/99-peek.sh:1: reads ui/index.html outside'
  case_with backend-script-climbs-from-a-braced-variable backend/gate/checks/98-peek.sh 'cat "${ROOT}/../ui/index.html"' 'backend/gate/checks/98-peek.sh:1: reads ui/index.html outside'
  case_with backend-reads-another-model backend/gate/checks/97-peek.sh 'node "$ROOT/../formal/r3-confirm/export.sh"' 'backend/gate/checks/97-peek.sh:1: reads formal/r3-confirm/export.sh outside'
  case_with backend-joins-climbs-and-a-name backend/src/peek.ts 'const p = join(ROOT, "..", "src-tauri", "src");' 'backend/src/peek.ts:1: names src-tauri'
  case_with backend-names-the-top-of-the-work-tree backend/scripts/peek.sh 'cat "$(git rev-parse --show-toplevel)/crates/halvo-capture/src/lib.rs"' 'backend/scripts/peek.sh:1: names crates'
  case_with backend-reads-a-tracked-file-by-git backend/scripts/peek.sh 'git show HEAD:src-tauri/tauri.conf.json' 'backend/scripts/peek.sh:1: reads src-tauri/tauri.conf.json outside'
  case_with backend-climbs-with-windows-separators backend/scripts/peek.ps1 'Get-Content ..\..\account\schema\vendor.sh' 'backend/scripts/peek.ps1:1: reads account/schema/vendor.sh outside'
  case_with backend-config-extends-the-root backend/biome.json '{ "extends": ["../biome.json"] }' 'backend/biome.json:1: reads biome.json outside'
  case_with backend-package-from-the-app backend/package.json '{ "dependencies": { "ui": "file:../ui" } }' 'backend/package.json:1: reads ui outside'
  case_with backend-climbs-above-the-repository backend/src/peek.ts 'import x from "../../../x.js";' 'backend/src/peek.ts:1: climbs above the repository'
  case_with backend-climbs-to-the-root backend/scripts/peek.sh 'cd ../.. && cat Cargo.toml' 'backend/scripts/peek.sh:1: reads the top of the repository'

  fixture backend-link-out
  link backend/src/capture ../../crates/halvo-capture
  expect FAIL backend-link-out 'backend/src/capture (a link): points outside'

  fixture backend-link-absolute
  link backend/src/capture /etc
  expect FAIL backend-link-absolute 'backend/src/capture (a link): points to an absolute path'

  dir=$scratch/no-tracked-file
  mkdir -p "$dir" || exit 2
  git -C "$dir" init -q || exit 2
  put backend/src/types.ts 'export type Id = string;'
  expect FAIL no-tracked-file 'no tracked file'

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  '') measure ;;
  *) printf 'FAIL %s (usage)\n' "$name"; exit 1 ;;
esac
