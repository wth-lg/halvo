#!/bin/sh
# jobs: macos windows
# rule: No tracked file under backend/ but a .md file reads or imports a file outside backend/ and formal/r7-ledger/, and no tracked file of the app (outside those two roots, docs/ and .md files) reads a file under backend/, but scripts/r7/, the step of scripts/prerequisites.sh that installs backend/'s packages, and account/schema/ for the four files of the wire contract that it copies. It does not refuse one known read: backend/scripts/names-v0.sh reads a file of the app's tree through the variable HALVO_APP_REPO, because a plain word in an assignment cannot be told from 394 honest lines under backend/ (--help lists every limit).
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
    function side_of(p,   l) {
      l = lower(p)
      if (index(l, "backend/") == 1) return "backend"
      if (index(l, "formal/r7-ledger/") == 1) return "model"
      return "app"
    }
    function joined(st, d,   k, out) {
      out = ""
      for (k = 1; k <= d; k++) out = out (k > 1 ? "/" : "") st[k]
      return out
    }
    function walk(base, sg, from, n,   qu, qn, st, d, i, s, cur, hops, tg, tsg, tn, nq, mq, k) {
      d = base == "" ? 0 : split(base, st, "/")
      qn = 0
      for (i = from; i <= n; i++) qu[++qn] = sg[i]
      i = 1
      hops = 0
      while (i <= qn) {
        s = qu[i++]
        if (s == "" || s == ".") continue
        if (s == "..") { if (d == 0) return "^"; d--; continue }
        st[++d] = s
        cur = joined(st, d)
        if (!(cur in target)) continue
        if (++hops > 40) return "~"
        tg = target[cur]
        if (tg ~ /^\// || tg ~ /^[A-Za-z]:/) return "@"
        d--
        tn = split(tg, tsg, "/")
        nq = 0
        for (k = 1; k <= tn; k++) mq[++nq] = tsg[k]
        for (k = i; k <= qn; k++) mq[++nq] = qu[k]
        for (k = 1; k <= nq; k++) qu[k] = mq[k]
        qn = nq
        i = 1
      }
      return joined(st, d)
    }
    function glob_hit(glob, sg, i, n,   re, k, under) {
      if (glob !~ /[*?[]/) return 0
      re = glob
      gsub(/[.+^$(){}|\\]/, "\\\\&", re)
      gsub(/\*+/, ".*", re)
      gsub(/\?/, ".", re)
      if ("backend" !~ ("^" re "$")) return 0
      if (glob ~ /[a-z]/) return 1
      under = "backend"
      for (k = i + 1; k <= n && sg[k] !~ /[*?[]/ && sg[k] != ""; k++) under = under "/" lower(sg[k])
      return under != "backend" && (under in backend_dir)
    }
    function through_link(sg, n,   k) {
      for (k = 1; k <= n; k++) if (sg[k] in link_name) return 1
      return 0
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
    function rev_name(w) {
      sub(/[~^][0-9~^]*$/, "", w)
      if (w == "" || w == "HEAD" || w == "@") return 1
      if (w ~ /^[0-9a-fA-F]+$/ && length(w) >= 7) return 1
      return w ~ /^((refs\/)?((heads|remotes|tags)\/)?([A-Za-z0-9_.-]+\/)*(main|master|HEAD))$/
    }
    function token(tok, s, pos,   c, kind, base, sg, n, i, from, r, climbs, prev, w, low, linked, dotted) {
      sub(/^\\+/, "", tok)
      if (match(tok, /^(file|link|portal|workspace):/)) tok = substr(tok, RLENGTH + 1)
      kind = ""
      if (match(tok, /^[A-Za-z0-9_.~^@\/-]*:[^\/]/) && index(substr(tok, RLENGTH), "/") && rev_name(substr(tok, 1, RLENGTH - 2))) {
        tok = substr(tok, RLENGTH)
        kind = "top"
      }
      if (tok == "") return
      c = substr(tok, 1, 1)
      if (c == "~") return
      low = lower(tok)
      linked = link_re != "" && tok ~ link_re
      if (side == "backend" && kind == "" && !linked && !index(tok, "..") && !((c == "/" || c == "$") && index(substr(tok, 2), "/"))) return
      if (side == "app" && kind == "" && !linked && low !~ /backend|[*?]/) return
      n = split(tok, sg, "/")
      if (linked) linked = through_link(sg, n)
      climbs = kind == "top"
      for (i = 1; i <= n; i++) if (sg[i] == "..") climbs = 1
      from = 1
      if (kind == "") {
        if (c == "/") {
          kind = anchor_of(substr(s, 1, pos - 1))
          if (kind == "none") kind = climbs ? "root" : "absolute"
          from = 2
        } else if (c == "$") {
          kind = "root"
          from = 2
        } else kind = "plain"
      }
      if (side == "backend") {
        if (kind == "absolute") return
        if (kind == "file" || kind == "plain") base = dir_of(path)
        else if (kind == "top") base = ""
        else base = "backend"
        if (climbs || linked || kind == "top") {
          r = walk(base, sg, from, n)
          if (r == "^") { problem(where, "climbs above the repository: " tok); return }
          if (r == "@") { problem(where, "reaches an absolute path through a link: " tok); return }
          if (r == "~") { problem(where, "follows a loop of links: " tok); return }
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
      if (low ~ /(^|[^:]):backend(\/|$)/) { problem(where, "reads under backend/ by a rev:path: " tok); return }
      if (low in backend_package) { problem(where, "names the package of backend/: " tok); return }
      if (kind == "absolute") return
      r = ""
      if (kind == "top" || kind == "root") r = walk("", sg, from, n)
      else if (kind == "file") r = walk(dir_of(path), sg, from, n)
      else {
        dotted = c == "."
        r = walk(dir_of(path), sg, 1, n)
        if (!under_backend(r) && !dotted) r = walk("", sg, 1, n)
        if (n == 1 && !dotted) r = ""
      }
      if (under_backend(r)) {
        if (index(path, "account/schema/") == 1 && (r in copied)) return
        problem(where, "reads under backend/: " r " by " tok)
        return
      }
      for (i = from; i <= n; i++) if (sg[i] != "" && sg[i] != "." && sg[i] != "..") break
      if (i < n && glob_hit(lower(sg[i]), sg, i, n)) problem(where, "reads under backend/ by a glob: " tok)
    }
    function words(s,   rest, off, w, left) {
      rest = s
      off = 0
      while (match(rest, quoted_word)) {
        w = substr(rest, RSTART + 1, RLENGTH - 2)
        left = substr(s, 1, off + RSTART - 1)
        off += RSTART + RLENGTH - 1
        rest = substr(rest, RSTART + RLENGTH)
        if (left !~ /(,|\/|\+|\[|join\(|push\()[ \t]*$/) continue
        if (lower(w) == "backend") { problem(where, "joins the word " w " to a path"); return }
      }
    }
    function line_of(s,   rest, off, tok, low, before, pos) {
      s = normal(s)
      if (side == "app") {
        low = lower(s)
        if (low ~ enters) { problem(where, "enters backend/ as a directory"); return }
        if (low ~ assigns) { problem(where, "assigns the word backend to a name"); return }
        if (low ~ listed) { problem(where, "lists backend as a member"); return }
        words(s)
        if (refused[side] + 0 > seen_refused) return
      }
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
      word = qs "?(\\./)?backend/?" qs "?"
      enters = "(^|[^a-z0-9_-])(cd|pushd|-c|--cwd|--dir|--prefix|--manifest-path|working-directory:)[ \t=]+" word "([^a-z0-9_./-]|$)"
      assigns = "(^|[^a-z0-9_])[a-z_][a-z0-9_]*[ \t]*(:[^=]*)?=[ \t]*" word "[ \t]*(;|,|$|&&|\\|\\||\\))"
      listed = "^[ \t]*-[ \t]+" word "[ \t]*$"
      split(contract, list, " ")
      for (k in list) copied[list[k]] = 1
    }
    FILENAME == ARGV[1] { app_only[$0] = 1; next }
    FILENAME == ARGV[2] { step[$0] = 1; steps++; next }
    FILENAME == ARGV[3] {
      target[$1] = $2
      k = split($1, part, "/")
      link_name[part[k]] = 1
      name = part[k]
      gsub(/[][.+*?(){}|^$\\]/, "\\\\&", name)
      link_re = link_re (link_re == "" ? "" : "|") "(^|/)" name "(/|$)"
      link_path[++links] = $1
      next
    }
    FILENAME == ARGV[4] { known[$0] = 1; files++; if (index(lower($0), "backend/") == 1) backend_files++; next }
    FILENAME == ARGV[5] {
      if ($1 == "dir") backend_dir[$2] = 1
      else if ($1 == "package") backend_package[$2] = 1
      else if ($1 == "run") run_able[$2] = 1
      next
    }
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
        if (index(path, "scripts/r7/") == 1) next
        if (index(path, "docs/") == 1 && !(path in run_able)) next
        if (path == "scripts/prerequisites.sh" && (number in step)) next
      }
      lines++
      where = path ":" number
      seen_refused = refused[side] + 0
      line_of(text)
    }
    END {
      for (k = 1; k <= links; k++) {
        path = link_path[k]
        side = side_of(path)
        if (side == "model" || (side == "app" && index(path, "scripts/r7/") == 1)) continue
        where = path " (a link)"
        n = split(path, sg, "/")
        r = walk("", sg, 1, n)
        if (side == "backend") {
          if (r == "@") problem(where, "points to an absolute path")
          else if (r == "~") problem(where, "is part of a loop of links")
          else if (r == "^" || !inside(r)) problem(where, "points outside backend/ and formal/r7-ledger/")
        } else if (r != "^" && under_backend(r)) problem(where, "points under backend/")
      }
      printf "files=%d backend-files=%d links=%d step-lines=%d lines=%d backend-refused=%d app-refused=%d unparsed=%d\n", files, backend_files, links, steps, lines, refused["backend"], refused["app"], unparsed >tally
      if (unparsed) printf "git grep printed %d lines that name no tracked path\n", unparsed >>problems
      if (!files) printf "no tracked file\n" >>problems
    }
  ' "$3" "$4" "$5" "$6" "$7" "$8"
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
      if (m[1] == "100755" || tolower($2) ~ /\.(sh|bash|mjs|cjs|js|ts|py|rs|ps1|bat|cmd)$/) print "run\t" $2 >(work "/facts")
      n = split($2, seg, "/")
      top_name = tolower(seg[1])
      if (top_name == "backend") for (i = 2; i <= n; i++) inner[tolower(seg[i])] = 1
      else top[top_name] = 1
      if (top_name == "backend") {
        under = "backend"
        for (i = 2; i < n; i++) { under = under "/" tolower(seg[i]); dirs[under] = 1 }
      }
    }
    END {
      for (t in top) if (t != "backend" && !(t in inner)) print t >(work "/app-only")
      for (t in dirs) print "dir\t" t >(work "/facts")
      if (quoted) printf "git quotes a tracked path, so the check cannot read it\n" >>problems
    }
  ' "$work/index" || return 2
  for file in paths link-ids app-only facts; do
    : >>"$work/$file"
  done
  : >"$work/links"
  while IFS="$tab" read -r id path; do
    target=$(git cat-file blob "$id") || return 2
    printf '%s\t%s\n' "$path" "$target" >>"$work/links"
  done <"$work/link-ids"
  : >"$work/step"
  if git cat-file blob :scripts/prerequisites.sh >"$work/prerequisites" 2>/dev/null; then
    step_lines <"$work/prerequisites" >"$work/step" || return 2
  fi
  if git cat-file blob :backend/package.json >"$work/package.json" 2>/dev/null; then
    awk 'match($0, /^[ \t]*\{?[ \t]*"name"[ \t]*:[ \t]*"[^"]+"/) { v = substr($0, RSTART, RLENGTH); sub(/^[^:]*:[ \t]*"/, "", v); sub(/"$/, "", v); print "package\t" tolower(v); exit }' "$work/package.json" >>"$work/facts" || return 2
  fi
  names=$(awk '{ gsub(/[][.+*?(){}|^$\\]/, "\\\\&"); printf "|(^|[^a-z0-9_-])%s", $0 }' "$work/app-only") || return 2
  linked=$(awk -F "$tab" '{ n = split($1, part, "/"); name = part[n]; gsub(/[][.+*?(){}|^$\\]/, "\\\\&", name); printf "|%s", name }' "$work/links") || return 2
  git -c core.quotepath=off grep --cached -I -n -i -E -e "[.][.]$names$linked" -- ':(icase)backend/' >"$work/lines"
  found=$?
  [ "$found" -le 1 ] || return 2
  git -c core.quotepath=off grep --cached -I -n -i -E -e "backend|[*?][^[:space:]]*/$linked" -- . ':(exclude,icase)backend/' ':(exclude,icase)formal/r7-ledger/' >>"$work/lines"
  found=$?
  [ "$found" -le 1 ] || return 2
  git -c core.quotepath=off check-attr --cached --stdin diff <"$work/paths" >"$work/attributes" || return 2
  awk '{ k = length($0) - length(": diff: unset"); if (k > 0 && substr($0, k + 1) == ": diff: unset") print substr($0, 1, k) }' "$work/attributes" >"$work/hidden" || return 2
  while IFS= read -r path; do
    size=$(git cat-file -s ":$path") || return 2
    text=$(git cat-file blob ":$path" | tr -d '\000' | wc -c) || return 2
    [ "$size" -eq "$((text))" ] || continue
    git -c core.quotepath=off grep --cached -a -n -i -E -e "[.][.]|backend|[*?][^[:space:]]*/$names$linked" -- ":(literal)$path" >>"$work/lines"
    found=$?
    [ "$found" -le 1 ] || return 2
  done <"$work/hidden"
  judge "$problems" "$work/tally" "$work/app-only" "$work/step" "$work/links" "$work/paths" "$work/facts" "$work/lines" || return 2
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

usage() {
  cat <<'EOF'
usage: sh scripts/r7/ci.d/24-r7-wall.sh [--self-test | --help]

24-r7-wall holds the wall between backend/ and the app (rulings P7-23 and P7-36
of the billing run). It reads the git index alone, so it also runs in the
windows job, which checks out one commit.

It fails when:
- a tracked file under backend/ (in any letter case), but a .md file, climbs,
  links or follows a chain of tracked links out of backend/ and
  formal/r7-ledger/, or names a top-level entry of the app right after a climb,
  a variable, a command substitution or a rev:path;
- a tracked file of the app (outside backend/ and formal/r7-ledger/), but a .md
  file and a file under docs/ that is no script, names a path that resolves
  under backend/ (also through tracked links, by a rev:path such as
  main:backend/x, or by a glob such as b*/x or */spikes/x), joins or assigns
  the word backend, lists backend as a member, enters backend/ with cd, -C,
  --prefix or working-directory, or names the package of backend/package.json;
- a tracked link of the app points under backend/.

Its exceptions: the scripts under scripts/r7/; the step of
scripts/prerequisites.sh that installs backend/'s packages (ensure_pnpm,
ensure_backend, the self-test helpers new_backend_case and run_ensure_backend,
and the self-test cases that call new_backend_case); and the files under
account/schema/, for the four files of the wire contract that they copy.

What it does not refuse:
- backend/scripts/names-v0.sh reads crates/halvo-capture/src/event.rs of the
  app's tree through the variable HALVO_APP_REPO. A plain word in an assignment
  (ENUM_PATH=crates/...) cannot be told from 394 honest lines under backend/
  at 52a4687e, which hold words such as account, so the check passes it. The
  self-test holds that form and shows that it passes.
- A path that code computes: a climb that code repeats ("../".repeat(2)), a
  path held in a variable and joined on a later line (but the word backend
  assigned to a name in the app), a path read from a file or the environment.
- A bare module name that Node resolves from the node_modules of the root.
- A file with a NUL byte, a .md file, a file under docs/ that is no script, and
  the files under formal/r7-ledger/, which neither case reads.
- A variable that starts a path stands for the top of the repository in the
  app and for backend/ in backend/; in backend/ a rev:path counts after HEAD,
  @, a commit id or a branch named main or master only.

Exit status: 0 PASS, 1 FAIL. --self-test runs its cases on throwaway
repositories under TMPDIR.
EOF
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
  put backend/spikes/s3/run.sh 'ROOT=$(cd "$(dirname "$0")/../.." && pwd -P)'
  put ui/src/status.ts 'import { client } from "./backend/client.ts";'
  put ui/src/backend/client.ts 'export const client = 1;'
  put docs/verify/check-covered.sh 'git ls-files formal'
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

index_file() {
  blob=$(printf '%s\n' "$2" | git -C "$dir" hash-object -w --stdin) || exit 2
  git -C "$dir" update-index --add --cacheinfo "100644,$blob,$1" || exit 2
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

  fixture known-limit-names-v0-reads-the-app-through-a-variable
  put backend/scripts/names-v0.sh 'APP_COMMIT=685d84637c302976956d56e6bbb5c18a4503e7c8
ENUM_PATH=crates/halvo-capture/src/event.rs
git -C "$HALVO_APP_REPO" show "$APP_COMMIT:$ENUM_PATH"'
  stage
  expect PASS known-limit-names-v0-reads-the-app-through-a-variable

  fixture known-limit-a-climb-that-code-repeats
  put backend/src/peek.ts 'const url = new URL("../".repeat(2) + "crates/halvo-capture/Cargo.toml", import.meta.url);'
  stage
  expect PASS known-limit-a-climb-that-code-repeats

  fixture known-limit-a-bare-module-name
  put backend/scripts/tauri-info.mjs 'import { run } from "@tauri-apps/cli";'
  stage
  expect PASS known-limit-a-bare-module-name

  case_with backend-includes-from-the-manifest-directory backend/spikes/peek/src/main.rs 'const E: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../crates/halvo-capture/src/lib.rs"));' 'backend/spikes/peek/src/main.rs:1: climbs above the repository'
  case_with backend-reads-a-branch-rev-path backend/scripts/peek.sh 'git show main:src-tauri/tauri.conf.json' 'backend/scripts/peek.sh:1: reads src-tauri/tauri.conf.json outside'
  case_with app-assigns-backend-in-a-script scripts/check-routes.sh 'server_dir=backend
cat "$server_dir/contract/routes/v1.json"' 'scripts/check-routes.sh:1: assigns the word backend'
  case_with app-assigns-backend-in-a-crate src-tauri/build.rs 'const SERVER_DIR: &str = "backend";' 'src-tauri/build.rs:1: assigns the word backend'
  case_with app-lists-backend-as-a-workspace pnpm-workspace.yaml 'packages:
  - backend' 'pnpm-workspace.yaml:2: lists backend as a member'
  case_with app-lists-backend-in-a-json-array package.json '{ "workspaces": ["backend"] }' 'package.json:1: joins the word backend'
  case_with app-runs-the-backend-package package.json '{ "scripts": { "dev": "pnpm --filter halvo-backend dev" } }' 'package.json:1: names the package of backend/'
  case_with app-globs-a-directory-only-backend-has Cargo.toml 'members = ["src-tauri", "*/spikes/*"]' 'Cargo.toml:1: reads under backend/ by a glob'
  case_with app-reads-backend-by-a-branch-rev-path scripts/peek.sh 'git show origin/main:backend/src/types.ts' 'scripts/peek.sh:1: reads under backend/: backend/src/types.ts'
  case_with app-script-under-docs-reads-backend docs/verify/check-budget.sh 'cat backend/gate/prompt-budget.json' 'docs/verify/check-budget.sh:1: reads under backend/: backend/gate/prompt-budget.json'

  fixture app-reads-a-file-that-the-diff-attribute-hides
  put .gitattributes 'scripts/gen/*.sh -diff'
  put scripts/gen/pull.sh 'cat backend/contract/schema/v1.json'
  stage
  expect FAIL app-reads-a-file-that-the-diff-attribute-hides 'scripts/gen/pull.sh:1: reads under backend/: backend/contract/schema/v1.json'

  fixture app-reads-through-a-link-under-r7
  put crates/halvo-wire/src/lib.rs 'const S: &str = include_str!("../../../scripts/r7/wire/schema/v1.json");'
  stage
  link scripts/r7/wire ../../backend/contract
  expect FAIL app-reads-through-a-link-under-r7 'crates/halvo-wire/src/lib.rs:1: reads under backend/: backend/contract/schema/v1.json'

  fixture backend-link-climbs-through-another-link
  link backend/src/up ..
  link backend/src/top up/..
  expect FAIL backend-link-climbs-through-another-link 'backend/src/top (a link): points outside'

  fixture backend-link-chain-through-the-model
  link backend/src/model ../../formal/r7-ledger/app
  link formal/r7-ledger/app ../../src-tauri
  expect FAIL backend-link-chain-through-the-model 'backend/src/model (a link): points outside'

  fixture backend-link-loop
  link backend/src/one two
  link backend/src/two one
  expect FAIL backend-link-loop 'backend/src/one (a link): is part of a loop of links'

  fixture backend-reads-through-a-link
  put backend/src/peek.ts 'import { read } from "./up/crates/halvo-capture/src/lib.rs";'
  stage
  link backend/src/up ../..
  expect FAIL backend-reads-through-a-link 'backend/src/peek.ts:1: reads crates/halvo-capture/src/lib.rs outside'

  case_with backend-reads-by-a-commit-rev-path backend/scripts/peek.sh 'git show 0123abcd:src-tauri/tauri.conf.json' 'backend/scripts/peek.sh:1: reads src-tauri/tauri.conf.json outside'
  case_with app-reads-backend-by-another-branch scripts/peek.sh 'git show feature-x:backend/src/types.ts' 'scripts/peek.sh:1: reads under backend/ by a rev:path'

  fixture backend-file-in-another-letter-case
  index_file Backend/src/peek.ts 'import { read } from "../../crates/halvo-capture/src/lib.rs";'
  expect FAIL backend-file-in-another-letter-case 'Backend/src/peek.ts:1: reads crates/halvo-capture/src/lib.rs outside'

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
  --help) usage ;;
  '') measure ;;
  *) printf 'FAIL %s (usage)\n' "$name"; exit 1 ;;
esac
