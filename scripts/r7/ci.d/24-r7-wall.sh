#!/bin/sh
# jobs: macos windows
# rule: A tracked file under backend/ reads no tracked file outside backend/ and formal/r7-ledger/, and a tracked file of the app (every file outside those two roots but .md files and the files under docs/ that are no script) reads no file under backend/, but for the files under scripts/r7/ that --help lists, the step of scripts/prerequisites.sh that installs backend/'s packages and the four files of the wire contract named whole under account/schema/, and a tree that the check cannot read fails, with the 11 named limits of --help: names-v0, computed-path, bare-module, file-or-environment, nul-byte, model-side, absolute-path, both-sides-path, unread-variable, bare-operand, bracket-glob.
# shellcheck disable=SC2016
set -u
name=24-r7-wall
tab=$(printf '\t')
contract='backend/contract/schema/v1.json backend/contract/routes/v1.json backend/contract/samples/v1.json backend/gate/prompt-budget.json'
limits='names-v0 computed-path bare-module file-or-environment nul-byte model-side absolute-path both-sides-path unread-variable bare-operand bracket-glob'
r7_files='scripts/r7/ci.d/10-r7-layout.sh scripts/r7/ci.d/20-r7-backend-static.sh scripts/r7/ci.d/21-r7-backend-test.sh scripts/r7/ci.d/22-r7-backend-records.sh scripts/r7/ci.d/23-r7-machine-paths.sh scripts/r7/ci.d/24-r7-wall.sh scripts/r7/ci.d/25-r7-backend-licenses.sh scripts/r7/layout.txt scripts/r7/stamp-gate-records.sh'
nlimits=$(awk -v l="$limits" 'BEGIN { print split(l, w, " ") }')

step_lines() {
  awk -v tab="$tab" '
    function header(s) { return s ~ /^[ \t]*[A-Za-z_][A-Za-z0-9_]*[ \t]*\(\)/ }
    function name_of(s,   t) { t = s; sub(/^[ \t]*/, "", t); sub(/[ \t]*\(\).*$/, "", t); return t }
    function indent_of(s) { match(s, /^[ \t]*/); return substr(s, 1, RLENGTH) }
    function bad(why) { printf "error%s%d%s%s\n", tab, NR, tab, why }
    BEGIN { step_fn["ensure_pnpm"] = 1; step_fn["ensure_backend"] = 1; step_fn["new_backend_case"] = 1 }
    {
      if (open) {
        if (substr($0, 1, length(ind) + 1) == ind "}") { print NR; open = 0; next }
        if (!header($0)) { print NR; next }
        bad("the step function " fn " has no end before the next function")
        open = 0
      }
      if (header($0)) {
        f = name_of($0)
        if (f in step_fn) {
          defined[f]++
          if (defined[f] > 1) { bad("the step function " f " is defined twice"); next }
          if ($0 !~ /^[ \t]*[A-Za-z_][A-Za-z0-9_]*\(\)[ \t]*\{[ \t]*$/) { bad("the step function " f " is not written as " f "() { on a line of its own"); next }
          open = 1
          fn = f
          ind = indent_of($0)
          print NR
          next
        }
      }
      if ($0 ~ /^  name=/) { held = 0; in_case = 1; of_the_step = 0 }
      if (in_case) {
        line[++held] = NR
        if ($0 ~ /^  new_backend_case /) of_the_step = 1
        if ($0 == "  verdict \"$name\"") {
          if (of_the_step) for (i = 1; i <= held; i++) print line[i]
          in_case = 0
        }
      }
    }
    END { if (open) bad("the step function " fn " has no end") }
  '
}

judge() {
  awk -F "$tab" -v q="'" -v contract="$contract" -v r7="$r7_files" -v nlimits="$nlimits" -v problems="$1" -v tally="$2" '
    function problem(where, why) { printf "%s: %s\n", where, why >>problems; refused[side]++ }
    function lower(s) { return tolower(s) }
    function inside(r,   l) {
      l = lower(r)
      return l == "backend" || index(l, "backend/") == 1 || l == "formal/r7-ledger" || index(l, "formal/r7-ledger/") == 1
    }
    function under_backend(r,   l) { l = lower(r); return l == "backend" || index(l, "backend/") == 1 }
    function tracked(r) { return r != "" && ((r in known) || (r in tracked_dir) || (r in stem)) }
    function tracked_file(r) { return r != "" && ((r in known) || (r in stem)) }
    function dir_of(p,   k) { k = match(p, /\/[^\/]*$/); return k ? substr(p, 1, k - 1) : "" }
    function side_of(p,   l) {
      l = lower(p)
      if (index(l, "backend/") == 1) return "backend"
      if (index(l, "formal/r7-ledger/") == 1) return "model"
      return "app"
    }
    function manifest_of(p,   d) {
      d = dir_of(p)
      while (1) {
        if (((d == "") ? "Cargo.toml" : d "/Cargo.toml") in known) return d
        if (d == "") return "^"
        d = dir_of(d)
      }
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
    function parse(line,   start, k, candidate, rest) {
      for (start = 1; (k = index(substr(line, start), ":")) > 0; start += k) {
        candidate = substr(line, 1, start + k - 2)
        rest = substr(line, start + k)
        if ((candidate in known) && match(rest, /^[0-9]+:/)) {
          gpath = candidate
          gnumber = substr(rest, 1, RLENGTH - 1)
          gtext = substr(rest, RLENGTH + 1)
          return 1
        }
      }
      return 0
    }
    function anchor_line(p, t,   v, nm, rel, rs, rn, d) {
      sub(/\r$/, "", t)
      if (!match(t, assignment)) return
      nm = substr(t, 1, RLENGTH)
      v = substr(t, RLENGTH + 1)
      sub(/[ \t]*[+]?=$/, "", nm)
      sub(/^[ \t]*(export|const|let|var|readonly|local)?[ \t]*/, "", nm)
      sub(/^[ \t]*/, "", v)
      sub(/[ \t]*;?[ \t]*$/, "", v)
      if (v ~ /^".*"$/) v = substr(v, 2, length(v) - 2)
      rel = ""
      if (v == "dirname(fileURLToPath(import.meta.url))" || v == "__dirname" || v == "import.meta.dirname" || v == "$(dirname \"$0\")") rel = ""
      else if (v ~ /^\$\(cd "\$\(dirname "\$0"\)(\/[A-Za-z0-9_.-]+)*" && pwd( -P)?\)$/) {
        rel = v
        sub(/^\$\(cd "\$\(dirname "\$0"\)/, "", rel)
        sub(/" && pwd( -P)?\)$/, "", rel)
      } else return
      if (assigned[p SUBSEP nm] != 1) return
      rn = split(rel, rs, "/")
      d = walk(dir_of(p), rs, 1, rn)
      if (d == "^" || d == "@" || d == "~") return
      anchor[p SUBSEP nm] = d
    }
    function rest_index(sg, from, n,   i) {
      for (i = from; i <= n && (sg[i] == "" || sg[i] == "." || sg[i] == ".."); i++) continue
      return i
    }
    function rest_from_top(sg, from, n,   i) {
      for (i = from; i <= n && (sg[i] == "" || sg[i] == "." || sg[i] == ".."); i++) continue
      return walk("", sg, i, n)
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
      return under != "backend" && ((under in backend_dir) || (!tok_quoted && (under in backend_file)))
    }
    function through_link(sg, n,   k) {
      for (k = 1; k <= n; k++) if (sg[k] in link_name) return 1
      return 0
    }
    function normal(s,   k, m, id, qc) {
      sub(/\r$/, "", s)
      gsub(/\\\\/, "/", s)
      while (k = match(s, /\\[A-Za-z0-9_.-]/)) s = substr(s, 1, k - 1) "/" substr(s, k + 1)
      while (match(s, join_var)) {
        m = substr(s, RSTART, RLENGTH)
        qc = substr(m, length(m) - 2, 1)
        id = m
        sub(/^[A-Za-z]+\([ \t]*/, "", id)
        sub(/[ \t]*,.*$/, "", id)
        if (id == "__dirname" || id == "import.meta.dirname") break
        k = index(m, "(")
        s = substr(s, 1, RSTART - 1) substr(m, 1, k) qc "$" id "/.." substr(s, RSTART + RLENGTH)
      }
      return s
    }
    function split_defaults(s,   m) {
      while (match(s, /\$\{[A-Za-z_][A-Za-z0-9_]*:?[-=?+]/)) {
        m = substr(s, RSTART, RLENGTH)
        sub(/:?[-=?+]$/, "} ", m)
        s = substr(s, 1, RSTART - 1) m substr(s, RSTART + RLENGTH)
      }
      return s
    }
    function anchor_of(left,   l, nm) {
      l = left
      sub(quotes_at_end, "", l)
      if (l ~ /dirname[ \t]+(--[ \t]+)?"?\$\{?0\}?"?\)$/) return "file"
      if (l ~ /\$\{0%\/\*\}$/) return "file"
      if (l ~ /(__dirname|import\.meta\.dirname)[ \t]*\+[ \t]*$/) return "file"
      if (l ~ /git[ \t]+(-C[ \t]+[^ \t]+[ \t]+)?rev-parse[ \t]+--show-toplevel[ \t]*\)$/) return "top"
      if (l ~ /CARGO_MANIFEST_DIR"?[ \t]*\)?[ \t]*,[ \t]*$/) return "manifest"
      if (match(l, /\$\{?[A-Za-z_][A-Za-z0-9_]*\}?$/) || match(l, /[A-Za-z_][A-Za-z0-9_]*[ \t]*\+[ \t]*$/)) {
        nm = substr(l, RSTART)
        sub(/[ \t]*\+[ \t]*$/, "", nm)
        gsub(/[${}]/, "", nm)
        if ((path SUBSEP nm) in anchor) { fbase = anchor[path SUBSEP nm]; return "file" }
      }
      if (l ~ /[)}]$/ || l ~ /\$[A-Za-z_][A-Za-z0-9_]*$/ || l ~ /\$[0-9@*]$/ || l ~ /\+[ \t]*$/) return "root"
      return "none"
    }
    function rev_name(w, rest) {
      sub(/[~^][0-9~^]*$/, "", w)
      if (w == "") return rest ~ /^[A-Za-z0-9_.][^\/]*\//
      if (w == "HEAD" || w == "@") return 1
      if (w ~ /^[0-9a-fA-F]+$/ && length(w) >= 7) return 1
      return w ~ /^((refs\/)?((heads|remotes|tags)\/)?([A-Za-z0-9_.-]+\/)*(main|master|HEAD))$/
    }
    function backend_token(tok, sg, n, from, kind, climbs, linked,   base, r, rb, i, prev, fix) {
      if (kind == "absolute") return
      fix = (kind == "root" || kind == "manifest") ? fix_dir : fix_md
      if (kind == "manifest") { base = manifest_of(path); if (base == "^") kind = "root" }
      if (kind == "file") base = fbase
      else if (kind == "plain") base = dir_of(path)
      else if (kind == "top") base = ""
      if (kind == "root") {
        i = rest_index(sg, from, n)
        r = walk("", sg, i, n)
        rb = walk("backend", sg, i, n)
        if (!inside(r) && (chain_mode ? tracked_file(r) : tracked(r)) && !tracked(rb)) problem(where, "reads " r ", a tracked path of the app, after a variable or a command substitution: " tok fix_dir)
        return
      }
      if (climbs || linked || kind == "top" || kind == "manifest") {
        r = walk(base, sg, from, n)
        if (chain_mode) {
          if (r != "^" && r != "@" && r != "~" && r != "" && !inside(r) && tracked_file(r)) problem(where, "reads " r " outside backend/ and formal/r7-ledger/: " tok fix)
          return
        }
        if (r == "^") { problem(where, "climbs above the repository: " tok fix); return }
        if (r == "@") { problem(where, "reaches an absolute path through a link: " tok fix_ask); return }
        if (r == "~") { problem(where, "follows a loop of links: " tok fix_ask); return }
        if (r == "") { problem(where, "reads the top of the repository: " tok fix); return }
        if (substr(r, 1, 1) == "$" && (kind == "top" || kind == "file")) { problem(where, "names a path from the top whose next part the check cannot read: " tok "; write the path from backend/'"'"'s own directory, or ask for a ruling"); return }
        if (!inside(r)) { problem(where, "reads " r " outside backend/ and formal/r7-ledger/: " tok fix); return }
      }
      if (kind == "plain" && climbs && !chain_mode) {
        r = walk(base, sg, from, n)
        prev = ""
        for (i = from; i <= n; i++) {
          if (prev == ".." && (lower(sg[i]) in app_only) && !tracked(r)) {
            if (lower(sg[i]) == "formal" && i < n && lower(sg[i + 1]) == "r7-ledger") { prev = sg[i]; continue }
            problem(where, "names " sg[i] ", a top-level entry of the app, after a climb: " tok fix_md)
            return
          }
          if (sg[i] != "" && sg[i] != ".") prev = sg[i]
        }
      }
    }
    function app_token(tok, sg, n, from, kind, c, low,   r, r1, r2, hit, i, dotted, base) {
      if (low ~ /(^|[^:]):backend(\/|$)/) { problem(where, "reads under backend/ by a rev:path: " tok (index(path, "account/schema/") == 1 ? fix_four : fix_md)); return }
      if (low in backend_package) { problem(where, "names the package of backend/: " tok fix_md); return }
      if (kind == "absolute") return
      hit = ""
      if (kind == "top") {
        r = walk("", sg, from, n)
        if (under_backend(r)) hit = r
      } else if (kind == "root" || kind == "manifest" || kind == "file") {
        r1 = kind == "file" ? "" : walk("", sg, from, n)
        base = kind == "manifest" ? manifest_of(path) : fbase
        if (base == "^") base = dir_of(path)
        r2 = walk(base, sg, from, n)
        if (under_backend(r1)) hit = r1
        else if (under_backend(r2)) hit = r2
        else if (r1 == "^" || r2 == "^") { r = rest_from_top(sg, from, n); if (under_backend(r)) hit = r }
      } else {
        dotted = c == "."
        if (n == 1 && !dotted) return
        r1 = walk(dir_of(path), sg, 1, n)
        if (under_backend(r1)) hit = r1
        else if (r1 == "^") { r = rest_from_top(sg, 1, n); if (under_backend(r)) hit = r }
        else if (!(n == 1 && !dotted)) {
          r2 = walk("", sg, 1, n)
          if (under_backend(r2) && !(dotted && tracked(r1))) hit = r2
        }
      }
      if (hit != "" && chain_mode && !tracked_file(hit)) hit = ""
      if (chain_mode) {
        if (hit != "" && !(index(path, "account/schema/") == 1 && (hit in copied))) problem(where, "reads under backend/: " hit " by " tok fix_md)
        return
      }
      if (hit != "") {
        if (index(path, "account/schema/") == 1 && (hit in copied)) return
        problem(where, "reads under backend/: " hit " by " tok (index(path, "account/schema/") == 1 ? fix_four : fix_md))
        return
      }
      for (i = from; i <= n; i++) if (sg[i] != "" && sg[i] != "." && sg[i] != "..") break
      if (i < n && tok_pos > pattern_end && glob_hit(lower(sg[i]), sg, i, n)) problem(where, "reads under backend/ by a glob: " tok fix_md)
    }
    function token(tok, s, pos,   c, kind, sg, n, i, from, climbs, low, linked) {
      tok_pos = pos
      tok_quoted = pos > 1 && index("\"" q, substr(s, pos - 1, 1)) > 0
      sub(/^\\+/, "", tok)
      if (match(tok, /^(file|link|portal|workspace):/)) tok = substr(tok, RLENGTH + 1)
      kind = ""
      if (match(tok, /^[A-Za-z0-9_.~^@\/-]*:[^\/:]/) && rev_name(substr(tok, 1, RLENGTH - 2), substr(tok, RLENGTH))) {
        tok = substr(tok, RLENGTH)
        kind = "top"
      }
      if (tok == "") return
      c = substr(tok, 1, 1)
      if (c == "~") return
      if (kind == "" && tok ~ /^[A-Za-z]:\//) kind = "absolute"
      low = lower(tok)
      linked = link_re != "" && tok ~ link_re
      if (side == "backend" && kind == "" && !linked && !index(tok, "..") && !((c == "/" && length(tok) > 1) || (c == "$" && index(tok, "/")))) return
      if (side == "app" && kind == "" && !linked && low !~ /backend|[*?]/) return
      n = split(tok, sg, "/")
      if (linked) linked = through_link(sg, n)
      climbs = 0
      for (i = 1; i <= n; i++) if (sg[i] == "..") climbs = 1
      from = 1
      fbase = dir_of(path)
      if (kind == "") {
        if (c == "/") {
          kind = anchor_of(substr(s, 1, pos - 1))
          if (kind == "none") kind = climbs ? "root" : "absolute"
          from = 2
        } else if (c == "$") {
          kind = "root"
          from = 2
          if ((path SUBSEP substr(sg[1], 2)) in anchor) { kind = "file"; fbase = anchor[path SUBSEP substr(sg[1], 2)] }
        } else kind = "plain"
      }
      if (side == "backend") backend_token(tok, sg, n, from, kind, climbs, linked)
      else app_token(tok, sg, n, from, kind, c, low)
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
        if (lower(w) == "backend") { problem(where, "joins the word " w " to a path" fix_md); return }
      }
    }
    function app_line(s,   low) {
      low = lower(s)
      if (s ~ enters_word || s ~ enters_mark || s ~ enters_tool) { problem(where, "enters backend/ as a directory or a project" fix_md); return }
      if (s ~ archive) { problem(where, "gives backend to tar or zip on one line, as a command or as a sentence; reword a sentence, or ask for a ruling"); return }
      if (s ~ operand) { problem(where, "gives backend to a command of the closed list as its first path operand" fix_md); return }
      if (low ~ defaults) { problem(where, "takes the word backend as a parameter default" fix_md); return }
      if (low ~ braces) { problem(where, "names backend/ in a brace group" fix_md); return }
      if (low ~ assigns) { problem(where, "assigns the word backend to a name, a label or an attribute" fix_name); return }
      if (low ~ listed) { problem(where, "lists backend as a member" fix_md); return }
      words(s)
    }
    function case_line(raw,   t, d, k, opens) {
      if (!parse(raw)) return
      t = gtext
      sub(/\r$/, "", t)
      if (t ~ /^[ \t]*#/) return
      if (t ~ arm_ends) arm_end_at[gpath, gnumber + 0] = 1
      if (t ~ arm_cont) cont_at[gpath, gnumber + 0] = 1
      d = 0
      while (match(t, case_open)) { d++; t = substr(t, RSTART + RLENGTH) }
      opens = d
      if (opens) open_at[gpath, gnumber + 0] = 1
      t = gtext
      while (match(t, case_close)) { d--; t = substr(t, RSTART + RLENGTH) }
      if (d == 0) return
      k = ++case_n[gpath]
      case_at[gpath, k] = gnumber + 0
      case_d[gpath, k] = d
    }
    function case_depth(p, num,   k, d) {
      d = 0
      for (k = 1; k <= case_n[p]; k++) {
        if (case_at[p, k] >= num) break
        d += case_d[p, k]
        if (d < 0) d = 0
      }
      return d
    }
    function pattern_place(p, n,   k) {
      for (k = n - 1; k > 0; k--) {
        if (((p, k) in open_at) || ((p, k) in arm_end_at)) return 1
        if (!((p, k) in cont_at)) return 0
      }
      return 0
    }
    function arm_end(s) {
      if (path !~ /\.(sh|bash)$/ && index(path, ".githooks/") != 1) return 0
      if (case_depth(path, number + 0) < 1) return 0
      if (!pattern_place(path, number + 0)) return 0
      if (match(s, arm_cont)) return length(s)
      if (match(s, arm_re)) return RLENGTH
      return 0
    }
    function line_of(s,   rest, off, tok, before, pos) {
      chain_mode = 0
      pattern_end = 0
      s = normal(s)
      if (side == "app") {
        app_line(s)
        if (refused[side] + 0 > seen_refused) return
      }
      s = split_defaults(s)
      pattern_end = arm_end(s)
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
      before = refused[side] + 0
      chains(s)
      if (refused[side] + 0 > before) return
      from_top(s)
    }
    function top_path(c,   n, sg, i, d, kp, out) {
      n = split(c, sg, "/")
      d = 0
      for (i = 1; i <= n; i++) {
        if (sg[i] == "" || sg[i] == ".") continue
        if (i == 1 && substr(sg[i], 1, 1) == "$") continue
        if (sg[i] == "..") { if (d > 0) d--; continue }
        kp[++d] = sg[i]
      }
      out = ""
      for (i = 1; i <= d; i++) out = out (i > 1 ? "/" : "") kp[i]
      return out
    }
    function top_hit(c,   r) {
      if (c ~ /^[A-Za-z]:[\/\\]/) return 0
      r = top_path(c)
      if (!(r in known) || !index(r, "/") || path == "backend/scripts/names-v0.sh") return 0
      if (side == "backend" && tracked("backend/" r)) return 0
      if (side == "backend" && !inside(r)) problem(where, "names " r ", a tracked file of the app, read from the top of the repository: " c fix_top)
      else if (side == "app" && inside(r)) problem(where, "names " r ", a tracked file of backend/ or formal/r7-ledger/, read from the top of the repository: " c fix_top)
      else return 0
      return 1
    }
    function from_top(s,   rest, off, n, i, j, st, en, sv, c) {
      rest = s
      while (match(rest, token_chars)) {
        if (top_hit(substr(rest, RSTART, RLENGTH))) return
        rest = substr(rest, RSTART + RLENGTH)
      }
      n = 0
      rest = s
      off = 0
      while (match(rest, string_re)) {
        n++
        st[n] = off + RSTART
        en[n] = off + RSTART + RLENGTH - 1
        sv[n] = substr(rest, RSTART + 1, RLENGTH - 2)
        off += RSTART + RLENGTH - 1
        rest = substr(rest, RSTART + RLENGTH)
      }
      for (i = 1; i <= n; i++) {
        c = sv[i]
        if (top_hit(c)) return
        for (j = i + 1; j <= n; j++) {
          if (substr(s, en[j - 1] + 1, st[j] - en[j - 1] - 1) !~ sep_gap) break
          c = c "/" sv[j]
          if (top_hit(c)) return
        }
      }
    }
    function chains(s,   rest, off, n, i, j, st, en, sv, c, last, before) {
      n = 0
      rest = s
      off = 0
      while (match(rest, string_re)) {
        n++
        st[n] = off + RSTART
        en[n] = off + RSTART + RLENGTH - 1
        sv[n] = substr(rest, RSTART + 1, RLENGTH - 2)
        off += RSTART + RLENGTH - 1
        rest = substr(rest, RSTART + RLENGTH)
      }
      for (i = 1; i < n; i++) {
        if (sv[i] !~ /(^|\/)\.\.(\/|$)/) continue
        c = sv[i]
        for (j = i + 1; j <= n; j++) {
          if (substr(s, en[j - 1] + 1, st[j] - en[j - 1] - 1) !~ sep_gap) break
          c = c "/" sv[j]
          last = j == n || substr(s, en[j] + 1, st[j + 1] - en[j] - 1) !~ sep_gap
          chain_mode = !last
          before = refused[side] + 0
          token(c, s, st[i] + 1)
          chain_mode = 0
          if (refused[side] + 0 > before) return
        }
      }
    }
    BEGIN {
      qs = "[\"" q "`]"
      token_chars = "[^][ \t\"" q "`()<>,;{}|&=]+"
      quoted_word = qs "[A-Za-z0-9_.-]+" qs
      quotes_at_end = qs "+$"
      sep = qs "[ \t]*(\\)[ \t]*\\.[ \t]*join[ \t]*\\(|/)[ \t]*" qs
      comma_sep = qs "[ \t]*,[ \t]*" qs
      comma_at_end = comma_sep "$"
      join_call = "(join|resolve)[ \t]*\\(([^()]|\\(([^()]|\\([^()]*\\))*\\))*\\)"
      sep_at_end = sep "$"
      dot_chain = "\\.\\.(/[^\"" q "`]*)?" sep
      comma_chain = "\\.\\.(/[^\"" q "`]*)?" comma_sep
      join_var = "(join|resolve)\\([ \t]*[A-Za-z_][A-Za-z0-9_.]*[ \t]*,[ \t]*" qs "\\.\\."
      word = qs "?(\\./)?backend/?" qs "?"
      bk = qs "?(\\./)?[Bb][Aa][Cc][Kk][Ee][Nn][Dd]/?" qs "?"
      enters_word = "(^|[^A-Za-z0-9_-])(cd|pushd|-C|--cwd|--dir|--prefix|--manifest-path|--project|--workspace)([ \t]+|[ \t]*=[ \t]*)" bk "([^A-Za-z0-9_./-]|$)"
      enters_tool = "(^|[^A-Za-z0-9_-])((tsc|vue-tsc)([ \t]+[^ \t;&|]+)*[ \t]+-p|(npm|pnpm|yarn)([ \t]+[^ \t;&|]+)*[ \t]+-w)([ \t]+|[ \t]*=[ \t]*)" bk "([^A-Za-z0-9_./-]|$)"
      enters_mark = "(working-directory:|cwd:|cwd=|current_dir\\()[ \t]*" bk "([^A-Za-z0-9_./-]|$)"
      operand = "(^|[;&|(`$ \t])(cat|cp|mv|ln|ls|find|rsync|du)([ \t]+-[A-Za-z0-9-]+)*[ \t]+" bk "([ \t;&|)]|$)"
      archive = "(^|[;&|(`$ \t])(tar|zip)([ \t]+[^ \t;&|]+)*[ \t]+" bk "([ \t;&|)]|$)"
      string_re = "\"[^\"]*\"|" q "[^" q "]*" q "|`[^`]*`"
      case_open = "(^|[;&|({ \t])case[ \t]+[^ \t;&|]+[ \t]+in([ \t;]|$)"
      arm_ends = "(;;&?|;&)[ \t]*(#.*)?$"
      case_close = "(^|[;&|( \t])esac([ \t;&|)]|$)"
      arm_word = "[^ \t()|;&<>$`]+"
      arm_re = "^[ \t]*" arm_word "([ \t]*[|][ \t]*" arm_word ")*[ \t]*[)]"
      arm_cont = "^[ \t]*" arm_word "([ \t]*[|][ \t]*" arm_word ")*[ \t]*[|][ \t]*\\\\[ \t]*\r?$"
      fix_top = "; rename the file, or reword the string so that it spells no path of the other side, or ask for a ruling"
      sep_gap = "^[ \t]*(,|\\)[ \t]*\\.[ \t]*join[ \t]*\\(|/)[ \t]*$"
      defaults = "\\$\\{[a-z_][a-z0-9_]*:?[-=?+][ \t]*" word "\\}"
      braces = "\\{([^{}]*,)?[ \t]*backend[ \t]*(,[^{}]*)?\\}/"
      assigns = "(^|[^a-z0-9_])[a-z_][a-z0-9_]*[ \t]*(:[^=]*)?=[ \t]*" word "[ \t]*(;|,|$|&&|\\|\\||\\))"
      assignment = "^[ \t]*(export[ \t]+|const[ \t]+|let[ \t]+|var[ \t]+|readonly[ \t]+|local[ \t]+)?[A-Za-z_][A-Za-z0-9_]*[ \t]*[+]?="
      listed = "^[ \t]*-[ \t]+" word "[ \t]*$"
      fix_md = "; write the path in a .md file, or ask for a ruling"
      fix_dir = "; write the file'"'"'s own directory out, or ask for a ruling"
      fix_four = "; name the four files whole in account/schema/, or ask for a ruling"
      fix_name = "; give it another name, or ask for a ruling"
      fix_ask = "; ask for a ruling"
      split(contract, list, " ")
      for (k in list) copied[list[k]] = 1
      nr7 = split(r7, r7list, " ")
      for (k = 1; k <= nr7; k++) exempt_r7[r7list[k]] = 1
    }
    FILENAME == ARGV[1] { app_only[$0] = 1; next }
    FILENAME == ARGV[2] {
      if ($1 == "error") step_error[++step_errors] = $2 "\t" $3
      else { step[$0] = 1; steps++ }
      next
    }
    FILENAME == ARGV[3] {
      target[$1] = $2
      k = split($1, part, "/")
      link_name[part[k]] = 1
      name = part[k]
      gsub(/[]+.*?(){}|^$\\[]/, "\\\\&", name)
      link_re = link_re (link_re == "" ? "" : "|") "(^|/)" name "(/|$)"
      link_path[++links] = $1
      next
    }
    FILENAME == ARGV[4] {
      known[$0] = 1
      files++
      if (index(lower($0), "backend/") == 1) { backend_files++; backend_file[lower($0)] = 1 }
      d = $0
      while ((k = match(d, /\/[^\/]*$/)) > 0) { d = substr(d, 1, k - 1); tracked_dir[d] = 1 }
      st = $0
      if (sub(/\.[^.\/]*$/, "", st)) stem[st] = 1
      next
    }
    FILENAME == ARGV[6] { if (parse($0) && match(gtext, assignment)) { v = substr(gtext, 1, RLENGTH); sub(/[ \t]*[+]?=$/, "", v); sub(/^[ \t]*(export|const|let|var|readonly|local)?[ \t]*/, "", v); assigned[gpath SUBSEP v]++ }; next }
    FILENAME == ARGV[7] { if (parse($0)) anchor_line(gpath, gtext); next }
    FILENAME == ARGV[5] {
      if ($1 == "dir") backend_dir[$2] = 1
      else if ($1 == "package") backend_package[$2] = 1
      else if ($1 == "run") run_able[$2] = 1
      else if ($1 == "quoted") quoted[++quoted_paths] = $2
      else if ($1 == "caseline") case_line(substr($0, 10))
      next
    }
    {
      if (!parse($0)) { unparsed++; next }
      path = gpath
      number = gnumber
      text = gtext
      side = side_of(path)
      if (side == "model") next
      if (lower(path) ~ /\.md$/) next
      if (side == "app") {
        if (path in exempt_r7) next
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
        if (side == "model" || (side == "app" && (path in exempt_r7))) continue
        where = path " (a link)"
        n = split(path, sg, "/")
        r = walk("", sg, 1, n)
        if (r == "@") problem(where, "points to an absolute path, which lies outside the checkout on another machine" fix_ask)
        else if (r == "~") problem(where, "is part of a loop of links" fix_ask)
        else if (side == "backend" && (r == "^" || !inside(r))) problem(where, "points outside backend/ and formal/r7-ledger/" fix_ask)
        else if (side == "app" && r != "^" && under_backend(r)) problem(where, "points under backend/" fix_ask)
      }
      side = "tree"
      for (k = 1; k <= step_errors; k++) {
        split(step_error[k], part, "\t")
        problem("scripts/prerequisites.sh:" part[1], "the step cannot be read: " part[2] "; write it as NAME() { with its body below and its closing brace at the start of a line of its own, or ask for a ruling")
      }
      for (k = 1; k <= quoted_paths; k++) problem(quoted[k], "git quotes this tracked path, so the check cannot read it; rename the file, or ask for a ruling")
      if (unparsed) problem("git grep", "printed " unparsed " lines that name no tracked path, so the check cannot read them; ask for a ruling")
      if (!files) problem("the index", "holds no tracked file")
      printf "files=%d backend-files=%d links=%d step-lines=%d lines=%d limits=%d backend-refused=%d app-refused=%d tree-refused=%d unparsed=%d\n", files, backend_files, links, steps, lines, nlimits, refused["backend"], refused["app"], refused["tree"], unparsed >tally
    }
  ' "$3" "$4" "$5" "$6" "$7" "$8" "$9" "${10}"
}

check_in() {
  repo=$1
  work=$2
  problems=$3
  : >"$problems" || return 2
  cd "$repo" || return 2
  git -c core.quotepath=off ls-files -s >"$work/index" || return 2
  : >"$work/facts"
  awk -F "$tab" -v work="$work" '
    {
      if (substr($2, 1, 1) == "\"") { print "quoted\t" $2 >>(work "/facts"); next }
      print $2 >(work "/paths")
      split($1, m, " ")
      if (m[1] == "120000") print m[2] "\t" $2 >(work "/link-ids")
      if ($2 == "scripts/prerequisites.sh" || $2 == "backend/package.json") print m[2] "\t" $2 >(work "/blob-ids")
      if (m[1] == "100755" || tolower($2) ~ /\.(sh|bash|mjs|cjs|js|ts|py|rs|ps1|bat|cmd)$/) print "run\t" $2 >>(work "/facts")
      n = split($2, seg, "/")
      top_name = tolower(seg[1])
      if (top_name != "backend") top[top_name] = 1
      else {
        under = "backend"
        for (i = 2; i < n; i++) { under = under "/" tolower(seg[i]); dirs[under] = 1 }
      }
    }
    END {
      for (t in top) print t >(work "/app-only")
      for (t in dirs) print "dir\t" t >>(work "/facts")
    }
  ' "$work/index" || return 2
  for file in paths link-ids app-only blob-ids; do
    : >>"$work/$file"
  done
  : >"$work/links"
  while IFS="$tab" read -r id path; do
    target=$(git cat-file blob "$id") || return 2
    printf '%s\t%s\n' "$path" "$target" >>"$work/links"
  done <"$work/link-ids"
  : >"$work/step"
  while IFS="$tab" read -r id path; do
    case $path in
      scripts/prerequisites.sh)
        git cat-file blob "$id" >"$work/prerequisites" || return 2
        step_lines <"$work/prerequisites" >"$work/step" || return 2
        ;;
      *)
        git cat-file blob "$id" >"$work/package.json" || return 2
        awk 'match($0, /^[ \t]*\{?[ \t]*"name"[ \t]*:[ \t]*"[^"]+"/) { v = substr($0, RSTART, RLENGTH); sub(/^[^:]*:[ \t]*"/, "", v); sub(/"$/, "", v); print "package\t" tolower(v); exit }' "$work/package.json" >>"$work/facts" || return 2
        ;;
    esac
  done <"$work/blob-ids"
  awk -F "$tab" '
    FILENAME == ARGV[1] { gsub(/[]+.*?(){}|^$\\[]/, "\\\\&"); names = names "|(^|[^a-z0-9_-])" $0; next }
    { n = split($1, part, "/"); name = part[n]; gsub(/[]+.*?(){}|^$\\[]/, "\\\\&", name); linked = linked "|" name }
    END { print names; print linked }
  ' "$work/app-only" "$work/links" >"$work/patterns" || return 2
  { read -r names; read -r linked; } <"$work/patterns" || return 2
  anchors='|[$]|[A-Za-z0-9_~^@/-]:[^/ ]|show-toplevel|CARGO_MANIFEST_DIR|from[ \t]*["'"'"']|require[ \t]*[(]|import[ \t]*[(]'
  git -c core.quotepath=off grep --cached -I -n -i -E -e "[.][.]$anchors$names$linked" -- ':(icase)backend/' >"$work/lines"
  found=$?
  [ "$found" -le 1 ] || return 2
  git -c core.quotepath=off grep --cached -I -n -i -E -e "ackend|[*?][^[:space:]]*/$linked" -- . ':(exclude,icase)backend/' ':(exclude,icase)formal/r7-ledger/' >>"$work/lines"
  found=$?
  [ "$found" -le 1 ] || return 2
  git -c core.quotepath=off grep --cached -I -n -E -e '(^|[;&|({[:space:]])(case[[:space:]].*[[:space:]]in|esac)([[:space:];&|)]|$)' -e '(;;&?|;&)[[:space:]]*(#.*)?$' -e '[|][[:space:]]*\\[[:space:]]*$' -- '*.sh' '*.bash' '.githooks/' >"$work/case-lines"
  found=$?
  [ "$found" -le 1 ] || return 2
  awk -v t="$tab" '{ print "caseline" t $0 }' "$work/case-lines" >>"$work/facts" || return 2
  git -c core.quotepath=off grep --cached -I -n -E -e 'dirname[(]fileURLToPath[(]import[.]meta[.]url[)][)]|__dirname|import[.]meta[.]dirname|[$][(]dirname "[$]0"[)]' -- . >"$work/anchor-lines"
  found=$?
  [ "$found" -le 1 ] || return 2
  anchor_names=$(awk '
    match($0, /:[ \t]*(export[ \t]+|const[ \t]+|let[ \t]+|var[ \t]+|readonly[ \t]+|local[ \t]+)?[A-Za-z_][A-Za-z0-9_]*[ \t]*=/) {
      v = substr($0, RSTART + 1, RLENGTH - 2)
      sub(/^[ \t]*(export|const|let|var|readonly|local)?[ \t]*/, "", v)
      sub(/[ \t]*$/, "", v)
      if (!(v in seen)) { seen[v] = 1; out = out (out == "" ? "" : "|") v }
    }
    END { print out }
  ' "$work/anchor-lines") || return 2
  : >"$work/assignments"
  if [ -n "$anchor_names" ]; then
    git -c core.quotepath=off grep --cached -I -n -E -e "^[ $tab]*(export[ $tab]+|const[ $tab]+|let[ $tab]+|var[ $tab]+|readonly[ $tab]+|local[ $tab]+)?($anchor_names)[ $tab]*[+]?=([^=]|\$)" -- . >"$work/assignments"
    found=$?
    [ "$found" -le 1 ] || return 2
  fi
  git -c core.quotepath=off check-attr --cached --stdin diff <"$work/paths" >"$work/attributes" || return 2
  awk '{ k = length($0) - length(": diff: unset"); if (k > 0 && substr($0, k + 1) == ": diff: unset") print substr($0, 1, k) }' "$work/attributes" >"$work/hidden" || return 2
  while IFS= read -r path; do
    size=$(git cat-file -s ":$path") || return 2
    git cat-file blob ":$path" >"$work/blob" || return 2
    tr -d '\000' <"$work/blob" >"$work/blob-text" || return 2
    text=$(wc -c <"$work/blob-text") || return 2
    [ "$size" -eq "$((text))" ] || continue
    git -c core.quotepath=off grep --cached -a -n -i -E -e "[.][.]|ackend|[*?][^[:space:]]*/$anchors$names$linked" -- ":(literal)$path" >>"$work/lines"
    found=$?
    [ "$found" -le 1 ] || return 2
  done <"$work/hidden"
  judge "$problems" "$work/tally" "$work/app-only" "$work/step" "$work/links" "$work/paths" "$work/facts" "$work/assignments" "$work/anchor-lines" "$work/lines" || return 2
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
    1) printf 'FAIL %s (%s reads across the wall or parts that the check cannot read; the list, with what to do for each, is problems.txt among the artifacts)\n' "$name" "$found"; exit 1 ;;
    *) printf 'FAIL %s (setup)\n' "$name"; exit 1 ;;
  esac
}

usage() {
  cat <<'EOF'
usage: sh scripts/r7/ci.d/24-r7-wall.sh [--self-test | --help]

24-r7-wall holds the wall between backend/ and the app (rulings P7-23, P7-36 and
P7-60 of the billing run). It reads the git index alone, so it also runs in the
windows job, which checks out one commit.

What it reads. Every tracked file but a .md file and a file under docs/ that is
no script (a script has mode 100755 or the extension .sh, .bash, .mjs, .cjs,
.js, .ts, .py, .rs, .ps1, .bat or .cmd). It refuses a tree that it cannot read:
a tracked file whose name git quotes, a line of git grep that names no tracked
path, and a step of scripts/prerequisites.sh that it cannot read.

How it places a path. A path is walked from where it starts, through the
tracked links: from the file's own directory for a relative path, for
$(dirname "$0"), ${0%/*}, __dirname + and import.meta.dirname +; from the top
for a rev:path (HEAD:, @:, a commit id, a branch named main or master, or an
index path such as :x/y) and for $(git rev-parse --show-toplevel); from the
crate's directory for concat!(env!("CARGO_MANIFEST_DIR"), ...); and from the
directory that a variable names when the file assigns it exactly once, on a
line of its own, from one of five forms: dirname(fileURLToPath(import.meta.url)),
__dirname, import.meta.dirname, $(dirname "$0") or
$(cd "$(dirname "$0")[/relative path]" && pwd [-P]). A path whose anchor it
cannot read (another variable, a command substitution, $PSScriptRoot, a
dirname of a realpath) is walked in the app from the top and from the file's
own directory, and when a walk climbs above the top, what follows the climbs
is read from the top. In backend/ such a path, with its climbs taken off, is
placed by the index: when it names a tracked path of the app from the top and
no tracked path from backend/, it is refused. A token ends at a quote, a space, a bracket, a comma, a
semicolon, a pipe, an ampersand or an equals sign, at :- or := of a parameter
default, and at a carriage return at the end of a line.

It fails when:
- a file under backend/ (in any letter case) climbs, links or follows a chain
  of links out of backend/ and formal/r7-ledger/, reads the top, or names a
  top-level entry of the app after a climb to a path that backend/ does not
  track;
- a file of the app names a path that lands under backend/ by any walk above,
  a dotted path such as ./backend/x included unless it names a tracked path
  from the file's own directory; reads under backend/ by a rev:path on any
  branch or by a glob such as b*/x or */spikes/x; names the package of
  backend/package.json; joins, lists or assigns the word backend, also as a
  label or an attribute value; names it in a brace group ({ui,backend}/x) or as
  a parameter default (${X:-backend}); enters it with cd, pushd, -C, --cwd,
  --dir, --prefix, --manifest-path, --project, --workspace,
  working-directory:, cwd:, cwd= or current_dir(, with -p of tsc or vue-tsc,
  or with -w of npm, pnpm or yarn, each option in its own letter case; gives
  it as the first path operand of cat, cp, mv, ln, ls, find, rsync or du; or
  names tar or zip before the word backend on one line, which is refused as a
  command or as a sentence alike (reword such a sentence). A word that stands alone is no
  path: a comment or a sentence that names the backend is read and passes;
- a tracked link of the app points under backend/, a tracked link of either
  side points to an absolute or a drive-letter path, or a link is part of a
  loop.

Its exceptions:
- the files under scripts/r7/ in this list, the backend's tooling (P7-36):
EOF
  for file in $r7_files; do
    printf '    %s\n' "$file"
  done
  cat <<'EOF'
  A file under scripts/r7/ that is not on the list is read like every other
  file of the app. A later unit that adds a file there which must read
  backend/ adds its line to the list in its own landing.
- the step of scripts/prerequisites.sh that installs backend/'s packages: the
  functions ensure_pnpm and ensure_backend, the self-test helper
  new_backend_case, and the self-test cases from name= to verdict "$name" that
  call new_backend_case. A step function is one definition: its header
  NAME() { on a line of its own, its end the first later line that starts with
  } at the same indentation. A second definition, a header with no end before
  the next function, and a definition on one line are refused.
- account/schema/, for the four files of the wire contract named whole:
  backend/contract/schema/v1.json, backend/contract/routes/v1.json,
  backend/contract/samples/v1.json and backend/gate/prompt-budget.json.

Readings that refuse an honest line, with the form to write in its place:
- in backend/, a variable that starts a path stands for backend/: write
  $(dirname "$0"), ${0%/*}, __dirname + or import.meta.url in its place;
- in backend/, a cd or a git -C to the top is refused: stay inside backend/,
  for example cd "$(dirname "$0")/..";
- in backend/, a path from the top whose next part is a variable, as
  "$(git rev-parse --show-toplevel)/$a/$b", is refused, because the variable
  may name the app: write the path from backend/'s own directory, as
  "$(dirname "$0")/../gate/prompt-budget.json";
- in the app, a comment that names a path under backend/ is refused: write it
  in a .md file;
- on either side, the strings of one line are placed each by itself, as the
  chain of a run of strings joined by a comma, ").join(" or "/", and as each
  shorter chain of that run (a shorter chain is refused when it names a
  tracked file of the other side), so two unrelated strings that chain into
  a path of the other side are refused: put the strings on two lines;
- on either side, every token and string of a line, and every run of strings
  joined by a comma, ").join(" or "/", is also read from the top of the
  repository, whatever stands before it and whatever the file's own
  directory is, so a text in a file of one side that spells in full, with at
  least one slash, a tracked file of the other side from the top is refused:
  rename the file or reword the text. This reading has three limits, each
  held by a passing fixture p785-limit-<kind>: a lone name of a top-level
  file (join(dir, "Cargo.toml")) is not read from the top; a path without its
  extension (crates/x/src/lib) is not read from the top; a path that the
  file's own side also tracks is read as its own (scripts/prerequisites.sh in
  backend/, which is backend/scripts/prerequisites.sh too). A drive-letter
  path is not read from the top (the limit absolute-path). As part of the
  limit names-v0, backend/scripts/names-v0.sh is exempt from it;
- in the app, the word backend assigned to a name, as a label or as an
  attribute value is refused: give it another name.

What it does not refuse, the 11 named limits, each held by a passing fixture
known-limit-<name> of the self-test:
  known limit names-v0: backend/scripts/names-v0.sh reads crates/halvo-capture/src/event.rs of the app's tree through the variable HALVO_APP_REPO, as ruled.
  known limit computed-path: a path that code computes, such as a climb that code repeats ("../".repeat(2)), a dirname of a dirname, pathlib's parents[n], a variable joined on a later line, or a word that code or the shell joins from pieces ("back" + "end", back""end).
  known limit bare-module: a bare module name that Node resolves from the node_modules of the root.
  known limit file-or-environment: a path read from a file or from the environment, such as "$BACKEND_SRC/types.ts".
  known limit nul-byte: a file with a NUL byte, which git grep does not read.
  known limit model-side: the files under formal/r7-ledger/, which neither side's rule reads.
  known limit absolute-path: an absolute or a drive-letter path in text, on either side, because the check cannot know which directory of a machine is the checkout.
  known limit both-sides-path: in backend/, a path after a variable that names a tracked path from the top and from backend/ alike.
  known limit unread-variable: in backend/, a variable that the file assigns twice or from another form than the five: a climb after it is placed by the index alone, so a path that the index does not track passes.
  known limit bare-operand: the bare word backend as an operand of a command outside the list above, such as stat backend.
  known limit bracket-glob: a bracket glob such as [b]ackend/x.
A glob without a letter, as */test/x, is counted when it can stand for
backend and the path after it is a directory of backend/ or, in a word that
the shell opens (no quote before it), a tracked file of backend/ (P7-89).
A quoted glob that a program opens by itself (a pathspec given to a git
command that reads contents, a pattern given to find or to a search tool) is
not counted when the path after it is a file of backend/: that is a limit of
the wall, not a rule of it, held by the passing fixture
p789-limit-a-quoted-glob-that-a-program-opens.
A case arm's pattern is not read as a glob (P7-88, P7-89): the shell holds
the pattern against a word and never opens it against the file system, so a
glob in it reads no file. In a shell script (.sh, .bash, or a file under
.githooks/), a line between a case word's "in" and its "esac", with blocks
inside blocks counted, that starts with words joined by | and closed by ),
is an arm, and a line of such words that ends in | and a backslash is the
first line of a pattern. The glob rule passes over the pattern of an arm;
every other rule reads it as before, and everything after the parenthesis is
read as code by every rule. A line outside a case block is never read as an
arm. Inside a case block a line is read as a pattern only where a pattern
can stand (P7-92): on the line after the line that holds case ... in, on the
line after a line that ends an arm (;;, ;& or ;;&), and on the line after a
pattern line that ends in | and a backslash; every other line of the block,
as a line of an arm's body, is code for every rule. A line in a pattern's
place that only looks like an arm is read as one, and a case or esac in a
string or a here-document is counted as one.

Exit status: 0 PASS, 1 FAIL. --self-test runs its cases on throwaway
repositories under TMPDIR and removes them when it ends.
EOF
}

put() {
  if [ "$in_base" = 1 ]; then
    put_tree "$1" "$2"
    return
  fi
  printf '%s\n' "$2" >"$scratch/blob.txt" || exit 2
  index_put "$1" 100644
}

put_raw() {
  printf '%b' "$2" >"$scratch/blob.txt" || exit 2
  index_put "$1" 100644
}

put_tree() {
  case $1 in
    */*) mkdir -p "$case_repo/${1%/*}" || exit 2 ;;
  esac
  printf '%s\n' "$2" >"$case_repo/$1" || exit 2
}

index_put() {
  blob=$(git -C "$scratch/base" hash-object -w --no-filters "$scratch/blob.txt") || exit 2
  GIT_INDEX_FILE=$case_index git -C "$case_repo" -c core.protectNTFS=false update-index --add --cacheinfo "$2,$blob,$1" || exit 2
}

fixture() {
  case_repo=$scratch/base
  case_index=$scratch/index.$1
  cp "$scratch/base.index" "$case_index" || exit 2
}

stage() {
  [ "$in_base" = 1 ] || return 0
  git -C "$case_repo" add -A . || exit 2
}

index_file() {
  printf '%s\n' "$2" >"$scratch/blob.txt" || exit 2
  index_put "$1" "${3:-100644}"
}

link() {
  printf '%s' "$2" >"$scratch/blob.txt" || exit 2
  index_put "$1" 120000
}

base() {
  in_base=1
  case_repo=$scratch/base
  case_index=$case_repo/.git/index
  mkdir -p "$case_repo" || exit 2
  git -C "$case_repo" init -q || exit 2
  git -C "$case_repo" config core.autocrlf false || exit 2
  git -C "$case_repo" config core.safecrlf false || exit 2
  put backend/package.json '{ "name": "halvo-backend", "devDependencies": { "wrangler": "4.0.0" } }'
  put backend/tsconfig.json '{ "compilerOptions": { "strict": true }, "include": ["src", "test", "../formal/r7-ledger/check.mjs"] }'
  put backend/src/types.ts 'export type Id = string;'
  put backend/test/ledger/cases.ts 'import type { Id } from "../../src/types.ts";
import { join } from "node:path";
const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const model = join(root, "..", "formal", "r7-ledger");
const same = new URL("../../", import.meta.url);
const here = __dirname + "/../../src/types.ts";
const there = import.meta.dirname + "/../../src/types.ts";'
  put backend/gate/checks/95-model.sh 'ROOT=${HALVO_ROOT:-$(cd "$(dirname "$0")/../.." && pwd -P)}
HALVO_ROOT=$ROOT node "$ROOT/../formal/r7-ledger/check.mjs"
(cd "$ROOT/../formal/r7-ledger" && git ls-files)
cd ..
cat "$ROOT/package.json" "$ROOT/scripts/stack.sh" "${0%/*}/../../src/types.ts"'
  put backend/scripts/stack.sh 'echo up'
  put backend/scripts/prerequisites.sh 'echo backend prerequisites'
  put backend/docs/baseline/a.txt 'baseline'
  put backend/fixtures/f.json '{}'
  put backend/README.md 'An example: import { x } from "../../crates/halvo-capture/src/lib.rs"; it reads ../src-tauri/ too.'
  put backend/contract/names/forbidden-names.json '{ "source": "app@685d846:crates/halvo-capture/src/event.rs:25" }'
  put backend/scripts/spread.mjs 'const all = [...one, ...two]; const range = "1..10"; const parent = "..";'
  put backend/scripts/names-v0.sh 'ENUM_PATH=crates/halvo-capture/src/event.rs'
  put backend/spikes/s3/run.sh 'ROOT=$(cd "$(dirname "$0")/../.." && pwd -P)'
  put formal/r7-ledger/check.mjs 'import x from "../../src-tauri/src/main.rs";'
  put formal/r3-confirm/README.md 'the backend repository'
  put src-tauri/src/main.rs 'store_backend: unwired_store::DESCRIPTION,'
  put src-tauri/tauri.conf.json '{}'
  put Cargo.toml '[workspace]'
  put package.json '{ "name": "halvo" }'
  put crates/halvo-capture/src/lib.rs '//! Per-OS backends live under macos/ and windows/. A size is 4 * 1024 / 2.'
  put ui/src/status.ts 'import { client } from "./backend/client.ts";'
  put ui/src/backend/client.ts 'export const client = 1;'
  put docs/verify/check-covered.sh 'git ls-files formal'
  put backend/test/billing/accounts.ts 'const ACCOUNT = "account"; const where = join(dir, "account");'
  put ui/index.html '<span id="backend"></span><script>$("backend").textContent = s.store_backend;</script>'
  put tools/run.sh 'echo "runtime backend=cpu"
cat t*/x.txt
grep -c backend target/log.txt'
  put scripts/egress/probe.sh 'dir=$(dirname "$0")
root=$(cd "$dir/../.." && pwd)'
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
} # the end of ensure_backend

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
  cp "$case_index" "$scratch/base.index" || exit 2
  in_base=0
}

fake_git() {
  real=$(command -v git) || exit 2
  mkdir -p "$scratch/$1" || exit 2
  printf '#!/bin/sh\nreal=%s\nmode=%s\n' "'$real'" "$1" >"$scratch/$1/git" || exit 2
  cat >>"$scratch/$1/git" <<'EOF' || exit 2
case $mode in
  fail-blob)
    if [ "$1" = cat-file ] && [ "$2" = blob ] && [ "$3" = :scripts/gen/pull.sh ]; then
      exit 1
    fi
    ;;
  bogus-grep)
    for a in "$@"; do
      if [ "$a" = grep ]; then
        "$real" "$@"
        s=$?
        printf '%s\n' 'no/such/path.sh:1:cat backend/x'
        exit "$s"
      fi
    done
    ;;
esac
exec "$real" "$@"
EOF
  chmod +x "$scratch/$1/git" || exit 2
}

keep_case() {
  [ -n "${CI_D_ARTIFACTS:-}" ] || return 0
  mkdir -p "$CI_D_ARTIFACTS/case-$1" || exit 2
  cp "$work/problems" "$work/stderr" "$CI_D_ARTIFACTS/case-$1/" 2>/dev/null
  cp "$work/lines" "$CI_D_ARTIFACTS/case-$1/" 2>/dev/null
  return 0
}

expect() {
  cases=$((cases + 1))
  work=$scratch/work.$cases
  mkdir "$work" || exit 2
  printf '%s\t%s\t%s\t%s\n' "$cases" "$1" "$2" "${3:-}" >>"$scratch/pending" || exit 2
  (
    if [ -n "$case_bin" ]; then
      PATH="$case_bin:$PATH"
      export PATH
    fi
    GIT_INDEX_FILE=$case_index
    export GIT_INDEX_FILE
    check_in "$case_repo" "$work" "$work/problems" >/dev/null 2>"$work/stderr"
    printf '%s\n' "$?" >"$work/code"
  ) &
  case_bin=
  running=$((running + 1))
  if [ "$running" -ge "$jobs" ]; then
    wait
    running=0
  fi
}

verdict_of() {
  want=$1
  case_name=$2
  code=
  read -r code <"$work/code" || code=missing
  case $code in
    0) got=PASS ;;
    1) got=FAIL ;;
    2) got=SETUP ;;
    *) got=exit-$code ;;
  esac
  case $3 in
    read:*)
      if [ "$got" = PASS ] && ! grep -q -F -e "${3#read:}:" "$work/lines"; then
        got=PASS-without-reading-the-line
      fi
      ;;
    unread:*)
      if [ "$got" = PASS ] && grep -q -F -e "${3#unread:}:" "$work/lines"; then
        got=PASS-after-reading-the-line
      fi
      ;;
    '') ;;
    *)
      if [ "$got" = FAIL ] && ! grep -q -F -e "$3" "$work/problems"; then
        got=FAIL-for-another-cause
      fi
      ;;
  esac
  case $case_name in
    known-limit-*) seen_limits="$seen_limits ${case_name#known-limit-}" ;;
  esac
  printf 'case %s/%s want=%s got=%s\n' "$name" "$case_name" "$want" "$got"
  if [ "$got" != "$want" ]; then
    wrong="$wrong $case_name"
    keep_case "$case_name"
  elif [ "$want" != PASS ]; then
    rejected=$((rejected + 1))
  fi
}

collect() {
  wait
  while IFS="$tab" read -r index want case_name arg; do
    work=$scratch/work.$index
    verdict_of "$want" "$case_name" "$arg"
  done <"$scratch/pending"
}

case_with() {
  fixture "$1"
  put "$2" "$3"
  stage
  expect FAIL "$1" "$4"
}

pass_with() {
  fixture "$1"
  put "$2" "$3"
  stage
  expect PASS "$1" "${4:-}"
}

list_of() {
  awk -v t="$1" 'BEGIN { if (match(t, /the [0-9]+ named limits of --help: [a-z0-9, -]+\./)) { m = substr(t, RSTART + 4, RLENGTH - 5); sub(/ named limits of --help: /, " ", m); print m } }'
}

limits_agree() {
  seen=$4
  for text in "$1" "$2" "$3"; do
    count=${text%% *}
    words=$(awk -v t="${text#* }" 'BEGIN { gsub(/,/, "", t); print t }') || return 1
    total=$(awk -v t="$words" 'BEGIN { print split(t, w, " ") }') || return 1
    [ "$count" = "$total" ] || return 1
    [ "$words" = "$limits" ] || return 1
  done
  [ "$seen" = " $limits" ]
}

texts_case() {
  want=$1
  case_name=$2
  rule=$(sed -n 's/^# rule: //p' "$0") || exit 2
  changelog=$(grep -F 'scripts/r7/ci.d/24-r7-wall.sh' "${CI_D_REPO:-.}/CHANGELOG.md") || changelog=
  usage >"$scratch/help.txt" || exit 2
  sed -n 's/^  known limit \([a-z0-9-]*\): .*/\1/p' "$scratch/help.txt" >"$scratch/help-names.txt" || exit 2
  help_names=$(tr '\n' ' ' <"$scratch/help-names.txt") || exit 2
  help_names=${help_names% }
  help_count=$(awk -v t="$help_names" 'BEGIN { print split(t, w, " ") }') || exit 2
  help_list="$help_count $help_names"
  rule_list=$(list_of "$rule")
  changelog_list=$(list_of "$changelog")
  if [ -n "${3:-}" ]; then
    rule_list=$(awk -v t="$rule_list" -v d=", $3" 'BEGIN { k = index(t, d); if (k) t = substr(t, 1, k - 1) substr(t, k + length(d)); print t }') || exit 2
  fi
  if limits_agree "$rule_list" "$changelog_list" "$help_list" "$seen_limits"; then got=PASS; else got=FAIL; fi
  printf 'case %s/%s want=%s got=%s\n' "$name" "$case_name" "$want" "$got"
  if [ "$got" != "$want" ]; then
    wrong="$wrong $case_name"
  elif [ "$want" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

remove_scratch() {
  [ -z "${scratch:-}" ] || rm -rf "$scratch"
}

self_test() {
  scratch=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  trap remove_scratch EXIT
  trap 'exit 130' INT
  trap 'exit 143' TERM
  trap 'exit 129' HUP
  rejected=0
  cases=0
  running=0
  jobs=1
  [ "${CI:-}" != true ] || jobs=4
  : >"$scratch/pending" || exit 2
  wrong=
  seen_limits=
  case_bin=
  in_base=0
  base

  fixture good
  link backend/gate/fixtures/licenses/node_modules/native .pnpm/native@1.0.0/node_modules/native
  link backend/gate/fixtures/licenses/node_modules/.pnpm/native@1.0.0/node_modules/absent ../../../../linked/absent
  link backend/model-link ../formal/r7-ledger/check.mjs
  expect PASS good

  fixture untracked-file-is-not-read
  put_tree scripts/loose.sh 'cat backend/src/types.ts'
  expect PASS untracked-file-is-not-read

  fixture the-index-decides-a-crossing-line
  put scripts/peek-a.sh 'cat backend/src/types.ts'
  stage
  put_tree scripts/peek-a.sh 'echo honest'
  expect FAIL the-index-decides-a-crossing-line 'scripts/peek-a.sh:1: reads under backend/'

  fixture the-index-decides-an-honest-line
  put scripts/peek-b.sh 'echo honest'
  stage
  put_tree scripts/peek-b.sh 'cat backend/src/types.ts'
  expect PASS the-index-decides-an-honest-line

  case_with app-script-outside-r7-reads-backend scripts/check-anything.sh 'node backend/scripts/names-v0.mjs' 'scripts/check-anything.sh:1: reads under backend/'
  case_with app-script-in-a-look-alike-of-r7 scripts/r7x/run.sh 'cat backend/src/types.ts' 'scripts/r7x/run.sh:1: reads under backend/'
  case_with app-script-under-r7-off-the-list scripts/r7/ci.d/99-r7-new.sh 'cd backend && pnpm test' 'scripts/r7/ci.d/99-r7-new.sh:1: enters backend/'
  fixture app-crate-under-r7-includes-backend
  put scripts/r7/wire/Cargo.toml '[package]
name = "wire"'
  put scripts/r7/wire/src/lib.rs 'pub const T: &str = include_str!("../../../../backend/src/types.ts");'
  put src-tauri/Cargo.toml '[dependencies]
wire = { path = "../scripts/r7/wire" }'
  stage
  expect FAIL app-crate-under-r7-includes-backend 'scripts/r7/wire/src/lib.rs:1: reads under backend/: backend/src/types.ts'
  case_with app-script-reads-backend-by-a-root-variable scripts/check-two.sh 'cat "$repo/backend/src/types.ts"' 'scripts/check-two.sh:1: reads under backend/'
  case_with app-climbs-after-a-variable scripts/check-routes.sh 'here=$(cd "$(dirname "$0")" && pwd)
cat "$here/../backend/contract/routes/v1.json"' 'scripts/check-routes.sh:2: reads under backend/: backend/contract/routes/v1.json'
  case_with app-climbs-above-the-top-after-a-variable scripts/x.sh 'cat "$here/../../../backend/src/types.ts"' 'scripts/x.sh:1: reads under backend/: backend/src/types.ts'
  case_with app-climbs-after-a-script-dir-variable scripts/x/check.sh 'cat "$SCRIPT_DIR/../../backend/src/types.ts"' 'scripts/x/check.sh:1: reads under backend/: backend/src/types.ts'
  case_with app-crate-climbs-from-the-manifest-directory crates/halvo-x/build.rs 'const S: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../backend/contract/schema/v1.json"));' 'crates/halvo-x/build.rs:1: reads under backend/: backend/contract/schema/v1.json'
  case_with app-powershell-climbs-from-its-root tools/peek.ps1 'Get-Content "$PSScriptRoot\..\backend\contract\schema\v1.json"' 'tools/peek.ps1:1: reads under backend/: backend/contract/schema/v1.json'
  case_with app-climbs-after-a-dirname-of-a-realpath scripts/peek.sh 'cat "$(dirname "$(realpath "$0")")/../backend/gate/prompt-budget.json"' 'scripts/peek.sh:1: reads under backend/: backend/gate/prompt-budget.json'
  case_with app-script-runs-a-dotted-backend-path scripts/ci-stack.sh 'sh ./backend/scripts/stack.sh up' 'scripts/ci-stack.sh:1: reads under backend/: backend/scripts/stack.sh'
  case_with app-workflow-uses-a-dotted-backend-path .github/workflows/stack.yml '      - uses: ./backend/.github/actions/setup
      - run: ./backend/scripts/stack.sh up' '.github/workflows/stack.yml:1: reads under backend/'
  case_with app-script-enters-backend scripts/check-three.sh 'cd backend && pnpm test' 'scripts/check-three.sh:1: enters backend/'
  case_with app-workflow-runs-in-backend .github/workflows/ci.yml '      - run: pnpm test
        working-directory: backend' '.github/workflows/ci.yml:2: enters backend/'
  case_with app-runs-pnpm-in-backend package.json '{ "scripts": { "t": "pnpm -C backend test" } }' 'package.json:1: enters backend/'
  case_with app-runs-npm-with-the-backend-prefix package.json '{ "scripts": { "b": "npm --prefix backend run build" } }' 'package.json:1: enters backend/'
  case_with app-builds-the-backend-workspace package.json '{ "scripts": { "b": "npm run build --workspace backend" } }' 'package.json:1: enters backend/'
  case_with app-builds-the-backend-workspace-short scripts/b.sh 'npm run build -w backend' 'scripts/b.sh:1: enters backend/'
  case_with app-type-checks-the-backend-project scripts/t.sh 'tsc -p backend --noEmit' 'scripts/t.sh:1: enters backend/'
  case_with app-type-checks-the-backend-project-long scripts/t.sh 'tsc --project backend' 'scripts/t.sh:1: enters backend/'
  case_with app-build-script-runs-in-backend src-tauri/build.rs 'Command::new("pnpm").arg("build").current_dir("backend").status().unwrap();' 'src-tauri/build.rs:1: enters backend/'
  case_with app-spawns-in-backend scripts/spawn.mjs 'spawnSync("pnpm", ["test"], { cwd: "backend" });' 'scripts/spawn.mjs:1: enters backend/'
  case_with app-finds-in-backend scripts/find.sh 'find backend -name "*.json" -exec cat {} +' 'scripts/find.sh:1: gives backend to a command'
  case_with app-copies-backend scripts/copy.sh 'cp -R backend "$out/"' 'scripts/copy.sh:1: gives backend to a command'
  case_with app-globs-backend-in-braces biome.json '{ "files": { "include": ["{ui,backend}/src/**/*.ts"] } }' 'biome.json:1: names backend/ in a brace group'
  case_with app-defaults-a-variable-to-backend scripts/types.sh 'dir=${SERVER_DIR:-backend}
cp "$dir/src/types.ts" ui/src/types.ts' 'scripts/types.sh:1: takes the word backend as a parameter default'
  case_with app-crate-includes-a-contract-file crates/halvo-x/build.rs 'const S: &str = include_str!("../../backend/contract/schema/v1.json");' 'crates/halvo-x/build.rs:1: reads under backend/'
  case_with app-crate-includes-by-doubled-backslashes crates/halvo-x/build.rs 'const S: &str = include_str!("..\\..\\backend\\contract\\schema\\v1.json");' 'crates/halvo-x/build.rs:1: reads under backend/'
  case_with app-page-loads-a-backend-file ui/index.html '<script src="../backend/src/app.js"></script>' 'ui/index.html:1: reads under backend/'
  case_with app-joins-backend-to-a-path src-tauri/build.rs 'let p = root.join("backend").join("contract");' 'src-tauri/build.rs:1: joins the word backend'
  case_with app-joins-backend-as-a-second-argument scripts/j.mjs 'const p = path.join(root, "backend");' 'scripts/j.mjs:1: joins the word backend'
  case_with app-pushes-backend-to-a-path scripts/j.mjs 'parts.push("backend");' 'scripts/j.mjs:1: joins the word backend'
  case_with app-joins-a-climb-to-backend scripts/j.mjs 'const p = join(root, "..", "backend", "src");' 'scripts/j.mjs:1: joins the word backend'
  case_with app-names-backend-in-another-case tools/x.sh 'cat Backend/src/types.ts' 'tools/x.sh:1: reads under backend/'
  case_with app-names-backend-with-windows-separators tools/x.ps1 'Get-Content ..\backend\src\types.ts' 'tools/x.ps1:1: reads under backend/'
  case_with app-names-backend-by-a-glob Cargo.toml 'members = ["src-tauri", "b*/spikes/s3/crate"]' 'Cargo.toml:1: reads under backend/'
  case_with app-globs-a-directory-only-backend-has Cargo.toml 'members = ["src-tauri", "*/spikes/*"]' 'Cargo.toml:1: reads under backend/ by a glob'
  case_with app-assigns-backend-in-a-script scripts/check-routes.sh 'server_dir=backend
cat "$server_dir/contract/routes/v1.json"' 'scripts/check-routes.sh:1: assigns the word backend'
  case_with app-assigns-backend-in-a-crate src-tauri/build.rs 'const SERVER_DIR: &str = "backend";' 'src-tauri/build.rs:1: assigns the word backend'
  case_with app-gives-backend-as-an-attribute-value src-tauri/src/model.rs '#[serde(rename = "backend")]' 'src-tauri/src/model.rs:1: assigns the word backend'
  case_with app-gives-backend-as-a-label ui/src/tabs.ts 'const tab = "backend";' 'ui/src/tabs.ts:1: assigns the word backend'
  case_with app-lists-backend-as-a-workspace pnpm-workspace.yaml 'packages:
  - backend' 'pnpm-workspace.yaml:2: lists backend as a member'
  case_with app-lists-backend-in-a-json-array package.json '{ "workspaces": ["backend"] }' 'package.json:1: joins the word backend'
  case_with app-runs-the-backend-package package.json '{ "scripts": { "dev": "pnpm --filter halvo-backend dev" } }' 'package.json:1: names the package of backend/'
  case_with app-reads-backend-by-a-branch-rev-path scripts/peek.sh 'git show origin/main:backend/src/types.ts' 'scripts/peek.sh:1: reads under backend/: backend/src/types.ts'
  case_with app-reads-backend-by-another-branch scripts/peek.sh 'git show feature-x:backend/src/types.ts' 'scripts/peek.sh:1: reads under backend/ by a rev:path'
  case_with app-script-under-docs-reads-backend docs/verify/check-budget.sh 'cat backend/gate/prompt-budget.json' 'docs/verify/check-budget.sh:1: reads under backend/: backend/gate/prompt-budget.json'
  fixture app-executable-under-docs-reads-backend
  index_file docs/tools/sync 'cat backend/src/types.ts' 100755
  expect FAIL app-executable-under-docs-reads-backend 'docs/tools/sync:1: reads under backend/'
  case_with account-schema-reads-another-backend-file account/schema/vendor.sh 'cp backend/src/types.ts types.ts' 'account/schema/vendor.sh:1: reads under backend/: backend/src/types.ts by backend/src/types.ts; name the four files whole'
  case_with account-crate-reads-the-contract-file account/crates/halvo-wire/build.rs 'let s = "../../../backend/contract/schema/v1.json";' 'account/crates/halvo-wire/build.rs:1: reads under backend/'
  pass_with app-climbs-after-a-variable-and-names-no-backend scripts/egress/probe2.sh 'root=$(cd "$dir/../.." && pwd) # the root, not the backend' 'read:scripts/egress/probe2.sh'
  pass_with app-counts-the-word-backend-with-grep scripts/count.sh 'grep -c backend target/log.txt' 'read:scripts/count.sh'

  fixture app-reads-a-file-that-the-diff-attribute-hides
  put .gitattributes 'scripts/gen/*.sh -diff'
  put scripts/gen/pull.sh 'cat backend/contract/schema/v1.json'
  stage
  expect FAIL app-reads-a-file-that-the-diff-attribute-hides 'scripts/gen/pull.sh:1: reads under backend/: backend/contract/schema/v1.json'

  fixture a-hidden-file-that-git-cannot-read
  put .gitattributes 'scripts/gen/*.sh -diff'
  put scripts/gen/pull.sh 'cat backend/contract/schema/v1.json'
  stage
  fake_git fail-blob
  case_bin=$scratch/fail-blob
  expect SETUP a-hidden-file-that-git-cannot-read

  fixture a-grep-line-of-no-tracked-path
  fake_git bogus-grep
  case_bin=$scratch/bogus-grep
  expect FAIL a-grep-line-of-no-tracked-path 'lines that name no tracked path'

  for reserved in scripts/nul.sh scripts/con.sh scripts/aux.txt scripts/ends-in-a-dot.; do
    case_name=a-windows-reserved-name-is-read-from-the-index-${reserved#scripts/}
    case_name=${case_name%.}
    fixture "$case_name"
    index_file "$reserved" 'cat backend/src/types.ts'
    expect FAIL "$case_name" "$reserved:1: reads under backend/"
  done

  fixture a-path-that-git-quotes
  index_file 'say "hi".txt' 'cat backend/src/types.ts'
  expect FAIL a-path-that-git-quotes 'git quotes this tracked path'

  fixture app-reads-through-a-link-under-r7
  put crates/halvo-wire/src/lib.rs 'const S: &str = include_str!("../../../scripts/r7/wire/schema/v1.json");'
  stage
  link scripts/r7/wire ../../backend/contract
  expect FAIL app-reads-through-a-link-under-r7 'crates/halvo-wire/src/lib.rs:1: reads under backend/: backend/contract/schema/v1.json'

  fixture app-link-into-backend
  link ui/contract ../backend/contract
  expect FAIL app-link-into-backend 'ui/contract (a link): points under backend/'

  fixture app-link-to-an-absolute-backend
  link ui/contract /srv/checkout/backend/src
  expect FAIL app-link-to-an-absolute-backend 'ui/contract (a link): points to an absolute path'

  fixture app-link-to-a-drive-letter
  link ui/contract D:/checkout/backend/src
  expect FAIL app-link-to-a-drive-letter 'ui/contract (a link): points to an absolute path'

  fixture prerequisites-outside-the-step
  put scripts/prerequisites.sh 'ensure_backend() {
  (cd backend && pnpm install --frozen-lockfile --ignore-scripts)
}

ensure_tools() {
  sh backend/scripts/fetch-tools.sh
}'
  stage
  expect FAIL prerequisites-outside-the-step 'scripts/prerequisites.sh:6: reads under backend/'

  fixture prerequisites-after-a-closing-line-with-a-comment
  put scripts/prerequisites.sh 'ensure_backend() {
  (cd backend && pnpm install --frozen-lockfile --ignore-scripts)
} # ensure_backend
show_contract() { cat backend/src/index.ts; }'
  stage
  expect FAIL prerequisites-after-a-closing-line-with-a-comment 'scripts/prerequisites.sh:4: reads under backend/'

  fixture prerequisites-after-a-plain-closing-line
  put scripts/prerequisites.sh 'ensure_backend() {
  (cd backend && pnpm install --frozen-lockfile --ignore-scripts)
}
show_contract() { cat backend/src/index.ts; }'
  stage
  expect FAIL prerequisites-after-a-plain-closing-line 'scripts/prerequisites.sh:4: reads under backend/'

  fixture prerequisites-step-defined-twice
  put scripts/prerequisites.sh 'ensure_backend() {
  (cd backend && pnpm install --frozen-lockfile --ignore-scripts)
}

ensure_backend() {
  cat backend/src/types.ts
}'
  stage
  expect FAIL prerequisites-step-defined-twice 'scripts/prerequisites.sh:5: the step cannot be read: the step function ensure_backend is defined twice'

  fixture prerequisites-step-closed-at-another-indentation
  put scripts/prerequisites.sh 'ensure_backend() {
  (cd backend && pnpm install --frozen-lockfile --ignore-scripts)
  }
show_contract() {
  cat backend/src/index.ts
}'
  stage
  expect FAIL prerequisites-step-closed-at-another-indentation 'scripts/prerequisites.sh:4: the step cannot be read: the step function ensure_backend has no end'

  fixture prerequisites-step-on-one-line
  put scripts/prerequisites.sh 'ensure_backend() { (cd backend && pnpm install --frozen-lockfile --ignore-scripts); }'
  stage
  expect FAIL prerequisites-step-on-one-line 'scripts/prerequisites.sh:1: the step cannot be read: the step function ensure_backend is not written'

  fixture prerequisites-case-of-another-step
  put scripts/prerequisites.sh 'self_test() {
  name=a-case-of-cargo
  cat backend/package.json
  verdict "$name"
}'
  stage
  expect FAIL prerequisites-case-of-another-step 'scripts/prerequisites.sh:3: reads under backend/'

  fixture prerequisites-case-of-another-step-after-a-case-of-the-step
  put scripts/prerequisites.sh 'self_test() {
  name=a-case-of-the-step
  new_backend_case backend-no-pnpm
  verdict "$name"
  name=a-case-of-cargo
  cat backend/package.json
  verdict "$name"
}'
  stage
  expect FAIL prerequisites-case-of-another-step-after-a-case-of-the-step 'scripts/prerequisites.sh:6: reads under backend/'

  case_with backend-climbs-out-by-the-script-path backend/scripts/sp.sh 'cat "${0%/*}/../../x.txt"' 'backend/scripts/sp.sh:1: reads x.txt outside'
  case_with backend-climbs-out-by-dirname-plus backend/src/dp.mjs 'const p = __dirname + "/../../x.txt";' 'backend/src/dp.mjs:1: reads x.txt outside'
  case_with backend-climbs-out-by-import-meta-dirname-plus backend/src/dp.mjs 'const p = import.meta.dirname + "/../../x.txt";' 'backend/src/dp.mjs:1: reads x.txt outside'
  case_with backend-imports-a-crate backend/src/capture.ts 'import { read } from "../../crates/halvo-capture/src/lib.rs";' 'backend/src/capture.ts:1: reads crates/halvo-capture/src/lib.rs outside'
  case_with backend-script-climbs-from-its-directory backend/scripts/peek.sh 'cat "$(dirname "$0")/../../src-tauri/tauri.conf.json"' 'backend/scripts/peek.sh:1: reads src-tauri/tauri.conf.json outside'
  case_with backend-script-climbs-from-a-root-variable backend/gate/checks/99-peek.sh 'cat "$ROOT/../ui/index.html"' 'backend/gate/checks/99-peek.sh:1: reads ui/index.html, a tracked path of the app'
  case_with backend-script-climbs-from-a-braced-variable backend/gate/checks/98-peek.sh 'cat "${ROOT}/../ui/index.html"' 'backend/gate/checks/98-peek.sh:1: reads ui/index.html, a tracked path of the app'
  case_with backend-reads-another-model backend/gate/checks/97-peek.sh 'node "$ROOT/../formal/r3-confirm/README.md"' 'backend/gate/checks/97-peek.sh:1: reads formal/r3-confirm/README.md, a tracked path of the app'
  case_with backend-joins-climbs-and-a-name backend/src/peek.ts 'const p = join(ROOT, "..", "src-tauri", "src");' 'backend/src/peek.ts:1: reads src-tauri/src, a tracked path of the app'
  case_with backend-joins-a-variable-and-a-climb-to-a-shared-name backend/gate/checks/96-peek.mjs 'const p = readFileSync(join(ROOT, "..", "Cargo.toml"));' 'backend/gate/checks/96-peek.mjs:1: reads Cargo.toml, a tracked path of the app'
  case_with backend-joins-a-variable-and-a-climb-to-the-app-scripts backend/gate/checks/96-peek.mjs 'const p = join(ROOT, "..", "scripts", "egress", "probe.sh");' 'backend/gate/checks/96-peek.mjs:1: reads scripts/egress/probe.sh, a tracked path of the app'
  case_with backend-rust-joins-a-climb backend/spikes/s3/src/peek.rs 'let p = root.join("..").join("src-tauri");' 'backend/spikes/s3/src/peek.rs:1: names src-tauri'
  case_with backend-names-the-top-of-the-work-tree backend/scripts/peek.sh 'cat "$(git rev-parse --show-toplevel)/crates/halvo-capture/src/lib.rs"' 'backend/scripts/peek.sh:1: reads crates/halvo-capture/src/lib.rs outside'
  case_with backend-reads-the-app-scripts-from-the-top backend/scripts/peek.sh 'cat "$(git rev-parse --show-toplevel)/scripts/prerequisites.sh"' 'backend/scripts/peek.sh:1: reads scripts/prerequisites.sh outside'
  case_with backend-reads-a-top-level-file-from-the-top backend/scripts/peek.sh 'cat "$(git rev-parse --show-toplevel)/package.json"' 'backend/scripts/peek.sh:1: reads package.json outside'
  case_with backend-reads-a-tracked-file-by-git backend/scripts/peek.sh 'git show HEAD:src-tauri/tauri.conf.json' 'backend/scripts/peek.sh:1: reads src-tauri/tauri.conf.json outside'
  case_with backend-reads-the-app-scripts-by-git backend/scripts/peek.sh 'git show HEAD:scripts/prerequisites.sh' 'backend/scripts/peek.sh:1: reads scripts/prerequisites.sh outside'
  case_with backend-reads-a-top-level-file-by-git backend/scripts/peek.sh 'git show HEAD:CHANGELOG.md' 'backend/scripts/peek.sh:1: reads CHANGELOG.md outside'
  case_with backend-lists-an-app-tree-by-git backend/scripts/peek.sh 'git ls-tree -r --name-only HEAD:src-tauri' 'backend/scripts/peek.sh:1: reads src-tauri outside'
  case_with backend-reads-the-root-manifest-on-main backend/scripts/peek.sh 'git show main:package.json' 'backend/scripts/peek.sh:1: reads package.json outside'
  case_with backend-reads-a-workflow-by-git backend/scripts/peek.sh 'git show HEAD:.github/workflows/ci.yml' 'backend/scripts/peek.sh:1: reads .github/workflows/ci.yml outside'
  case_with backend-reads-by-the-index-path backend/scripts/peek.sh 'git show :src-tauri/tauri.conf.json' 'backend/scripts/peek.sh:1: reads src-tauri/tauri.conf.json outside'
  case_with backend-reads-a-branch-rev-path backend/scripts/peek.sh 'git show main:src-tauri/tauri.conf.json' 'backend/scripts/peek.sh:1: reads src-tauri/tauri.conf.json outside'
  case_with backend-reads-by-a-commit-rev-path backend/scripts/peek.sh 'git show 0123abcd:src-tauri/tauri.conf.json' 'backend/scripts/peek.sh:1: reads src-tauri/tauri.conf.json outside'
  case_with backend-names-an-app-path-after-a-variable backend/scripts/peek.sh 'cat "$X/crates/halvo-capture/src/lib.rs"' 'backend/scripts/peek.sh:1: reads crates/halvo-capture/src/lib.rs, a tracked path of the app'
  case_with backend-defaults-a-root-to-a-climb backend/scripts/j6.sh 'ROOT=${HALVO_ROOT:-../..}
cat "$ROOT/Cargo.toml"' 'backend/scripts/j6.sh:1: reads the top of the repository'
  fixture backend-climbs-at-the-end-of-a-crlf-line
  put_raw backend/scripts/crlf.sh 'echo x\r\ncd ../..\r\n'
  stage
  expect FAIL backend-climbs-at-the-end-of-a-crlf-line 'backend/scripts/crlf.sh:2: reads the top of the repository'
  fixture backend-root-at-the-end-of-a-crlf-line
  put_raw backend/scripts/crlf.sh 'ROOT=$(dirname "$0")/../..\r\n'
  stage
  expect FAIL backend-root-at-the-end-of-a-crlf-line 'backend/scripts/crlf.sh:1: reads the top of the repository'
  case_with backend-climbs-with-windows-separators backend/scripts/peek.ps1 'Get-Content ..\..\account\schema\vendor.sh' 'backend/scripts/peek.ps1:1: reads account/schema/vendor.sh outside'
  case_with backend-config-extends-the-root backend/biome.json '{ "extends": ["../biome.json"] }' 'backend/biome.json:1: reads biome.json outside'
  case_with backend-package-from-the-app backend/package.json '{ "dependencies": { "ui": "file:../ui" } }' 'backend/package.json:1: reads ui outside'
  case_with backend-climbs-above-the-repository backend/src/peek.ts 'import x from "../../../x.js";' 'backend/src/peek.ts:1: climbs above the repository'
  case_with backend-climbs-to-the-root backend/scripts/peek.sh 'cd ../.. && cat Cargo.toml' 'backend/scripts/peek.sh:1: reads the top of the repository'
  fixture backend-includes-from-the-manifest-directory
  put backend/spikes/peek/Cargo.toml '[package]
name = "peek"'
  put backend/spikes/peek/src/main.rs 'const E: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../crates/halvo-capture/src/lib.rs"));'
  stage
  expect FAIL backend-includes-from-the-manifest-directory 'backend/spikes/peek/src/main.rs:1: reads crates/halvo-capture/src/lib.rs outside'
  fixture backend-reads-crates-at-the-top-beside-its-own-crates
  put backend/crates/x.txt 'x'
  put backend/scripts/peek.sh 'git show HEAD:crates/halvo-capture/src/lib.rs'
  stage
  expect FAIL backend-reads-crates-at-the-top-beside-its-own-crates 'backend/scripts/peek.sh:1: reads crates/halvo-capture/src/lib.rs outside'
  pass_with backend-names-its-own-scripts backend/test/x/a.ts 'const s = "../../scripts/stack.sh";' 'read:backend/test/x/a.ts'
  pass_with backend-names-its-own-docs backend/test/x/a.ts 'const s = "../../docs/baseline/a.txt";' 'read:backend/test/x/a.ts'
  pass_with backend-names-its-own-fixtures backend/test/x/a.ts 'const s = "../../fixtures/f.json";' 'read:backend/test/x/a.ts'
  pass_with backend-names-its-own-package-json backend/test/x/a.ts 'const s = "../../package.json";' 'read:backend/test/x/a.ts'
  pass_with backend-names-its-own-readme backend/test/x/a.ts 'const s = "../../README.md";' 'read:backend/test/x/a.ts'

  fixture backend-link-out
  link backend/src/capture ../../crates/halvo-capture
  expect FAIL backend-link-out 'backend/src/capture (a link): points outside'

  fixture backend-link-absolute
  link backend/src/capture /etc
  expect FAIL backend-link-absolute 'backend/src/capture (a link): points to an absolute path'

  fixture backend-link-to-a-drive-letter
  link backend/src/capture C:/checkout/crates
  expect FAIL backend-link-to-a-drive-letter 'backend/src/capture (a link): points to an absolute path'

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

  fixture backend-file-in-another-letter-case
  index_file Backend/src/peek.ts 'import { read } from "../../crates/halvo-capture/src/lib.rs";'
  expect FAIL backend-file-in-another-letter-case 'Backend/src/peek.ts:1: reads crates/halvo-capture/src/lib.rs outside'

  pass_with backend-reads-its-root-from-a-file-anchor backend/spikes/s1/run.mjs 'const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = join(HERE, "..", "..");' 'read:backend/spikes/s1/run.mjs'
  pass_with backend-reads-a-deeper-root-from-a-file-anchor backend/test/ledger/probes/after-client/probe.mjs 'const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = join(HERE, "..", "..", "..", "..");' 'read:backend/test/ledger/probes/after-client/probe.mjs'
  case_with backend-climbs-out-from-a-url-anchor backend/spikes/s1/peek.mjs 'const HERE = dirname(fileURLToPath(import.meta.url));
const p = join(HERE, "..", "..", "..", "src-tauri", "tauri.conf.json");' 'backend/spikes/s1/peek.mjs:2: reads src-tauri/tauri.conf.json outside'
  case_with backend-climbs-out-from-a-dirname-anchor backend/spikes/s1/peek.cjs 'const HERE = __dirname;
const p = join(HERE, "..", "..", "..", "src-tauri", "tauri.conf.json");' 'backend/spikes/s1/peek.cjs:2: reads src-tauri/tauri.conf.json outside'
  case_with backend-climbs-out-from-an-import-meta-anchor backend/spikes/s1/peek.mjs 'const HERE = import.meta.dirname;
const p = join(HERE, "..", "..", "..", "src-tauri", "tauri.conf.json");' 'backend/spikes/s1/peek.mjs:2: reads src-tauri/tauri.conf.json outside'
  case_with backend-climbs-out-from-a-shell-dirname-anchor backend/spikes/s1/peek.sh 'HERE=$(dirname "$0")
cat "$HERE/../../../src-tauri/tauri.conf.json"' 'backend/spikes/s1/peek.sh:2: reads src-tauri/tauri.conf.json outside'
  case_with backend-climbs-out-from-a-shell-cd-anchor backend/spikes/s1/peek.sh 'HERE=$(cd "$(dirname "$0")/.." && pwd -P)
cat "$HERE/../../src-tauri/tauri.conf.json"' 'backend/spikes/s1/peek.sh:2: reads src-tauri/tauri.conf.json outside'
  case_with backend-climbs-after-a-variable-assigned-twice backend/spikes/s1/peek.sh 'HERE=$(dirname "$0")
HERE=$(pick_a_directory)
cat "$HERE/../src-tauri/tauri.conf.json"' 'backend/spikes/s1/peek.sh:3: reads src-tauri/tauri.conf.json, a tracked path of the app'
  pass_with backend-climbs-after-an-unread-variable-to-no-tracked-path backend/stress/driver.mjs 'ctx.registry ??= join(ctx.outDir, "..", "started.jsonl");' 'read:backend/stress/driver.mjs'
  case_with backend-climbs-after-an-unread-variable-to-the-app backend/stress/driver.mjs 'ctx.registry ??= join(ctx.outDir, "..", "src-tauri", "tauri.conf.json");' 'backend/stress/driver.mjs:1: reads src-tauri/tauri.conf.json, a tracked path of the app'
  case_with backend-climbs-after-an-unread-variable-to-the-root-manifest backend/stress/driver.mjs 'ctx.registry ??= join(ctx.outDir, "..", "Cargo.toml");' 'backend/stress/driver.mjs:1: reads Cargo.toml, a tracked path of the app'

  case_with app-archives-backend-with-tar scripts/pack.sh 'tar -czf dist/app.tgz backend' 'scripts/pack.sh:1: gives backend to tar or zip'
  case_with app-archives-backend-with-zip scripts/pack.sh 'zip -r dist/app.zip backend' 'scripts/pack.sh:1: gives backend to tar or zip'
  case_with app-cats-backend scripts/list.sh 'cat backend' 'scripts/list.sh:1: gives backend to a command'
  case_with app-moves-backend scripts/list.sh 'mv backend old-server' 'scripts/list.sh:1: gives backend to a command'
  case_with app-links-backend scripts/list.sh 'ln -s backend server' 'scripts/list.sh:1: gives backend to a command'
  case_with app-lists-backend scripts/list.sh 'ls backend' 'scripts/list.sh:1: gives backend to a command'
  case_with app-syncs-backend scripts/list.sh 'rsync -a backend out' 'scripts/list.sh:1: gives backend to a command'
  case_with app-sizes-backend scripts/list.sh 'du -sh backend' 'scripts/list.sh:1: gives backend to a command'
  pass_with a-root-manifest-comment-names-the-backend Cargo.toml '[workspace]
# The billing backend is TypeScript; it has no crate in this workspace.' 'read:Cargo.toml'
  pass_with a-root-text-file-names-the-backend notes.txt 'The backend lives in its own directory.' 'read:notes.txt'
  pass_with a-root-package-description-names-the-backend package.json '{ "description": "Tools of the app; the backend has its own package." }' 'read:package.json'
  pass_with app-greps-the-whole-word-backend scripts/count-words.sh 'grep -w backend target/log.txt' 'read:scripts/count-words.sh'
  pass_with app-makes-a-directory-named-backend scripts/make.sh 'mkdir -p backend' 'read:scripts/make.sh'
  case_with backend-names-a-path-from-the-top-by-variables backend/scripts/ab.sh 'a=backend; b=gate/prompt-budget.json; cat "$(git rev-parse --show-toplevel)/$a/$b"' 'backend/scripts/ab.sh:1: names a path from the top whose next part the check cannot read'
  case_with backend-climbs-after-a-variable-extended-with-plus-equals backend/src/pe.mjs 'let here = dirname(fileURLToPath(import.meta.url));
here += "/../..";
const p = here + "/../crates/halvo-capture/src/lib.rs";' 'backend/src/pe.mjs:3: reads crates/halvo-capture/src/lib.rs, a tracked path of the app'

  case_with backend-reads-the-app-after-cwd-with-an-encoding backend/scripts/peek-cwd.mjs 'const text = readFileSync(process.cwd() + "/../crates/halvo-capture/src/lib.rs", "utf8");' 'backend/scripts/peek-cwd.mjs:1: reads crates/halvo-capture/src/lib.rs, a tracked path of the app'
  case_with backend-reads-the-app-after-cwd backend/scripts/peek-cwd.mjs 'const path = process.cwd() + "/../crates/halvo-capture/src/lib.rs";' 'backend/scripts/peek-cwd.mjs:1: reads crates/halvo-capture/src/lib.rs, a tracked path of the app'
  case_with backend-reads-the-app-after-an-environment-root-with-an-encoding backend/scripts/peek-env.mjs 'const root = process.env.HALVO_ROOT ?? ".";
const text = readFileSync(root + "/../crates/halvo-capture/src/lib.rs", "utf8");' 'backend/scripts/peek-env.mjs:2: reads crates/halvo-capture/src/lib.rs, a tracked path of the app'
  case_with backend-reads-the-app-after-plus-equals-with-an-encoding backend/scripts/peek-plus.mjs 'let here = dirname(fileURLToPath(import.meta.url));
here += "/../..";
const text = readFileSync(here + "/../crates/halvo-capture/src/lib.rs", "utf8");' 'backend/scripts/peek-plus.mjs:3: reads crates/halvo-capture/src/lib.rs, a tracked path of the app'
  pass_with backend-joins-pieces-to-its-own-path backend/src/own.mjs 'const p = join(dirname(fileURLToPath(import.meta.url)), "..", "scripts", "stack.sh");' 'read:backend/src/own.mjs'
  case_with app-comment-names-zip-and-then-the-backend crates/halvo-x/src/lib.rs '// Releases ship the app as one zip file and the backend as a Worker' 'crates/halvo-x/src/lib.rs:1: gives backend to tar or zip'
  case_with a-root-text-names-tar-and-then-the-backend RELEASE-NOTES.txt 'Each tar of a release leaves out the backend' 'RELEASE-NOTES.txt:1: gives backend to tar or zip'
  case_with app-archives-backend-with-old-style-tar scripts/pack.sh 'tar czf dist/app.tgz backend' 'scripts/pack.sh:1: gives backend to tar or zip'
  case_with app-archives-backend-to-standard-output scripts/pack.sh 'tar cf - backend | gzip > dist/app.tgz' 'scripts/pack.sh:1: gives backend to tar or zip'
  case_with app-archives-backend-with-a-long-option scripts/pack.sh 'tar --exclude=node_modules -czf dist/app.tgz backend' 'scripts/pack.sh:1: gives backend to tar or zip'
  pass_with app-archives-the-ui scripts/pack.sh 'tar -czf dist/app.tgz ui/dist'
  case_with backend-joins-three-levels-of-calls backend/src/v.mjs 'const p = join(dirname(fileURLToPath(new URL(import.meta.url))), "..", "..", "crates", "halvo-capture", "src", "lib.rs");' 'backend/src/v.mjs:1: reads crates'
  case_with backend-joins-an-array-of-pieces backend/src/v.mjs 'const p = ["..", "..", "crates", "halvo-capture", "src", "lib.rs"].join("/");' 'backend/src/v.mjs:1: reads crates'
  case_with backend-reads-the-app-by-pieces-with-a-tail backend/src/v.mjs 'const t = read(process.env.ROOT + "/..", "crates", "halvo-capture", "src", "lib.rs", "utf8");' 'backend/src/v.mjs:1: reads crates/halvo-capture/src/lib.rs, a tracked path of the app'
  pass_with backend-passes-two-strings-that-name-nothing-outside backend/src/log.mjs 'log("../x", "y");' 'read:backend/src/log.mjs'

  case_with t1-app-tar-dashless-options scripts/pack1.sh 'tar czf dist/app.tgz backend' 'scripts/pack1.sh:1: gives backend to tar or zip'
  case_with t2-app-tar-long-option-with-equals scripts/pack2.sh 'tar --exclude=node_modules -czf dist/app.tgz backend' 'scripts/pack2.sh:1: gives backend to tar or zip'
  case_with t3-app-tar-to-standard-output scripts/pack3.sh 'tar -cf - backend | gzip > dist/app.tgz' 'scripts/pack3.sh:1: gives backend to tar or zip'
  case_with j1-backend-python-path-pieces backend/scripts/peek_path.py 'TEXT = Path("..", "..", "crates", "halvo-capture", "src", "lib.rs").read_text()' 'backend/scripts/peek_path.py:1: reads crates'
  case_with j2-backend-python-joinpath backend/scripts/peek_joinpath.py 'TEXT = Path(__file__).parent.joinpath("..", "..", "crates", "halvo-capture", "src", "lib.rs").read_text()' 'backend/scripts/peek_joinpath.py:1: reads crates'

  case_with p785-a-backend-joins-the-repository-root-and-app-pieces backend/scripts/root.mjs 'const src = readFileSync(join(execSync("git rev-parse --show-toplevel").toString().trim(), "crates", "halvo-capture", "src", "lib.rs"), "utf8");' 'backend/scripts/root.mjs:1: names crates/halvo-capture/src/lib.rs, a tracked file of the app, read from the top'
  case_with p785-a-backend-joins-a-root-variable-and-app-pieces backend/scripts/root2.mjs 'const repoRoot = execSync("git rev-parse --show-toplevel", { encoding: "utf8" }).trim();
const src = readFileSync(join(repoRoot, "crates", "halvo-capture", "src", "lib.rs"), "utf8");' 'backend/scripts/root2.mjs:2: names crates/halvo-capture/src/lib.rs, a tracked file of the app, read from the top'
  case_with p785-b-backend-joins-os-pardir-and-app-pieces backend/scripts/pd.py 'text = open(os.path.join(os.path.dirname(__file__), os.pardir, os.pardir, "crates", "halvo-capture", "src", "lib.rs")).read()' 'backend/scripts/pd.py:1: names crates/halvo-capture/src/lib.rs, a tracked file of the app, read from the top'
  case_with p785-c-backend-cds-up-twice-and-reads-a-plain-app-path backend/scripts/cd2.sh 'cd .. && cd .. && cat crates/halvo-capture/src/lib.rs' 'backend/scripts/cd2.sh:1: names crates/halvo-capture/src/lib.rs, a tracked file of the app, read from the top'
  case_with p785-a-shell-twin-reads-the-app-after-the-root backend/scripts/top.sh 'TOP=$(git rev-parse --show-toplevel)
cat "$TOP/crates/halvo-capture/src/lib.rs"' 'backend/scripts/top.sh:2: reads crates/halvo-capture/src/lib.rs, a tracked path of the app'
  pass_with p785-backend-joins-the-repository-root-and-its-own-pieces backend/scripts/budget.mjs 'const budget = readFileSync(join(execSync("git rev-parse --show-toplevel").toString().trim(), "backend", "gate", "prompt-budget.json"), "utf8");' 'read:backend/scripts/budget.mjs'

  pass_with p785-names-v0-is-exempt-from-the-reading-from-the-top backend/scripts/names-v0.sh 'ENUM_PATH=crates/halvo-capture/src/lib.rs' 'read:backend/scripts/names-v0.sh'
  case_with p785-a-copy-of-the-names-v0-line-in-another-file backend/scripts/names-v1.sh 'ENUM_PATH=crates/halvo-capture/src/lib.rs' 'backend/scripts/names-v1.sh:1: names crates/halvo-capture/src/lib.rs, a tracked file of the app, read from the top'
  pass_with p785-limit-a-lone-top-level-file-name backend/gate/lib/manifest.mjs 'const manifest = readFileSync(join(directory, "Cargo.toml"), "utf8");' 'read:backend/gate/lib/manifest.mjs'
  pass_with p785-limit-a-path-without-its-extension backend/src/peek.ts 'import { x } from "crates/halvo-capture/src/lib";' 'read:backend/src/peek.ts'
  pass_with p785-limit-a-path-that-backend-also-tracks backend/spikes/s4/run.sh 'echo "packages are missing: run sh scripts/prerequisites.sh first" >&2' 'read:backend/spikes/s4/run.sh'

  pass_with p788-the-line-of-script-31-passes scripts/r2/ci.d/31-fixture-server.sh 'kind_of() {
  case $1 in
    */test/*) return 0 ;;
  esac
  return 1
}' 'read:scripts/r2/ci.d/31-fixture-server.sh'
  case_with p788-an-arm-with-a-read-after-its-parenthesis scripts/sort.sh 'case $1 in
  */test/*) cat backend/test/ledger/cases.ts ;;
esac' 'scripts/sort.sh:2: reads under backend/: backend/test/ledger/cases.ts by backend/test/ledger/cases.ts'
  case_with p788-a-for-loop-over-the-glob scripts/count.sh 'for f in */test/*; do wc -l "$f"; done' 'scripts/count.sh:1: reads under backend/'
  case_with p788-a-command-with-the-glob-as-an-argument scripts/count2.sh 'wc -l */test/*' 'scripts/count2.sh:1: reads under backend/'
  pass_with p788-an-arm-whose-pattern-spans-two-lines scripts/sort2.sh 'case $1 in
  */test/* | \
  */spec/*) return 0 ;;
esac' 'read:scripts/sort2.sh'
  case_with p788-a-parenthesis-in-a-command-substitution-is-code scripts/count3.sh 'files=$(ls */test/*)' 'scripts/count3.sh:1: reads under backend/'
  case_with p788-a-case-word-under-backend-is-read scripts/word.sh 'case backend/test/ledger/cases.ts in' 'scripts/word.sh:1: '

  case_with p789-e1-a-literal-pattern-under-backend-is-read scripts/pick.sh 'git ls-files | while read -r f; do
  case $f in
    backend/*) cat "$f" ;;
  esac
done' 'scripts/pick.sh:3: reads under backend/: backend/* by backend/*'
  case_with p789-e2-a-wrapped-pipeline-line-is-code scripts/wrap.sh 'printf "%s\n" \
  */test/ledger/*.ts | \
  xargs cat' 'scripts/wrap.sh:2: reads under backend/ by a glob'
  case_with p789-e2-a-one-word-glob-command-before-a-pipe-is-code scripts/run1.sh 'b*/scripts/stack.sh | \
  tee out.txt' 'scripts/run1.sh:1: reads under backend/ by a glob'
  case_with p789-e2-a-subshell-closing-line-is-code scripts/sub.sh '(cd "$here" &&
  b*/scripts/stack.sh)' 'scripts/sub.sh:2: reads under backend/ by a glob'
  case_with p789-e2-an-array-closed-on-the-glob-line-is-code scripts/arr.sh 'files=(
  */test/ledger/*.ts)
cat "${files[@]}"' 'scripts/arr.sh:2: reads under backend/ by a glob'
  pass_with p789-e2-an-outer-arm-after-a-nested-case-is-an-arm scripts/nested.sh 'case $1 in
  a)
    case $2 in
      x) : ;;
    esac
    ;;
  */test/*) return 0 ;;
esac' 'read:scripts/nested.sh'
  case_with p789-e3-a-glob-that-ends-in-a-backend-file scripts/e3.sh 'cat */test/ledger/cases.ts' 'scripts/e3.sh:1: reads under backend/ by a glob'

  pass_with p789-limit-a-quoted-glob-that-a-program-opens scripts/attrs.sh 'hits=$(git grep -n -e TODO -- "*/test/ledger/cases.ts")' 'read:scripts/attrs.sh'

  case_with p792-k1-a-wrapped-pipeline-in-an-arm-body scripts/k1.sh 'case "$1" in
  list)
    printf "%s\n" \
      */test/ledger/*.ts | \
      xargs cat
    ;;
esac' 'scripts/k1.sh:4: reads under backend/ by a glob'
  case_with p792-k2-an-array-closed-on-the-glob-in-an-arm-body scripts/k2.sh 'case "$1" in
  show)
    files=(
      */test/ledger/*.ts)
    cat "${files[@]}"
    ;;
esac' 'scripts/k2.sh:4: reads under backend/ by a glob'
  case_with p792-k3-a-subshell-closed-on-the-glob-in-an-arm-body scripts/k3.sh 'case "$1" in
  show)
    (cat \
      */test/ledger/*.ts)
    ;;
esac' 'scripts/k3.sh:4: reads under backend/ by a glob'
  case_with p792-n7-a-one-word-glob-command-before-a-pipe-in-an-arm-body scripts/n7.sh 'case $mode in
  run)
    b*/scripts/stack.sh | \
      tee out.txt
    ;;
esac' 'scripts/n7.sh:3: reads under backend/ by a glob'
  pass_with p792-k8-a-pattern-list-over-three-lines scripts/k8.sh 'case $1 in
  */test/* | \
  */spec/* | \
  */bench/*) return 0 ;;
esac' 'read:scripts/k8.sh'
  pass_with p792-k10-a-case-inside-a-case scripts/k10.sh 'kind_of() {
  case $1 in
    a)
      case $2 in
        x) : ;;
      esac
      ;;
    */test/*) return 0 ;;
  esac
}' 'read:scripts/k10.sh'

  fixture known-limit-names-v0
  put backend/scripts/names-v0.sh 'APP_COMMIT=685d84637c302976956d56e6bbb5c18a4503e7c8
ENUM_PATH=crates/halvo-capture/src/event.rs
git -C "$HALVO_APP_REPO" show "$APP_COMMIT:$ENUM_PATH"'
  stage
  expect PASS known-limit-names-v0 'read:backend/scripts/names-v0.sh'

  pass_with known-limit-computed-path backend/src/computed.ts 'const url = new URL("../".repeat(2) + "crates/halvo-capture/Cargo.toml", import.meta.url);
const up = dirname(dirname(import.meta.dirname)) + "/crates/halvo-capture/Cargo.toml";
p=crates; cat "$top/$p/halvo-capture/Cargo.toml"' 'read:backend/src/computed.ts'
  pass_with computed-path-joined-from-pieces-in-the-app scripts/joined.sh 'cat "$(git rev-parse --show-toplevel)/back""end/src/types.ts"
node -e '"'"'join(root, "back" + "end", "src")'"'"'' 'unread:scripts/joined.sh'
  pass_with known-limit-bare-module backend/scripts/tauri-info.mjs 'import { run } from "@tauri-apps/cli";' 'read:backend/scripts/tauri-info.mjs'
  pass_with known-limit-file-or-environment scripts/env.sh 'cat "$BACKEND_SRC/types.ts"' 'read:scripts/env.sh'
  fixture known-limit-nul-byte
  put_raw scripts/zero-byte.sh 'cat backend/src/types.ts\000\n'
  stage
  expect PASS known-limit-nul-byte 'unread:scripts/zero-byte.sh'
  pass_with known-limit-model-side formal/r7-ledger/peek.mjs 'import a from "../../src-tauri/src/main.rs"; import b from "../../backend/src/types.ts";' 'unread:formal/r7-ledger/peek.mjs'
  fixture known-limit-absolute-path
  put scripts/abs.sh 'cat /srv/checkout/backend/src/types.ts'
  put backend/scripts/abs.ps1 'Get-Content D:\checkout\..\..\..\crates\halvo-capture\src\lib.rs'
  stage
  expect PASS known-limit-absolute-path 'read:backend/scripts/abs.ps1'
  pass_with known-limit-both-sides-path backend/scripts/both.sh 'cat "$X/scripts/prerequisites.sh"' 'read:backend/scripts/both.sh'
  pass_with known-limit-unread-variable backend/scripts/out.sh 'OUT=$(mktemp -d)
OUT=$(dirname "$0")
cat "$OUT/../started.jsonl"' 'read:backend/scripts/out.sh'
  pass_with known-limit-bare-operand scripts/stat.sh 'stat backend' 'read:scripts/stat.sh'
  pass_with known-limit-bracket-glob scripts/bracket.sh 'cat [b]ackend/contract/routes/v1.json' 'read:scripts/bracket.sh'

  case_repo=$scratch/no-tracked-file
  case_index=$case_repo/.git/index
  mkdir -p "$case_repo" || exit 2
  git -C "$case_repo" init -q || exit 2
  put_tree backend/src/types.ts 'export type Id = string;'
  expect FAIL no-tracked-file 'holds no tracked file'

  collect
  texts_case PASS the-three-texts-and-the-fixtures-name-the-same-limits
  texts_case FAIL a-text-that-drops-a-limit-is-refused bracket-glob

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
