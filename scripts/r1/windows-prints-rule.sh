#!/bin/sh

wpr_fixed_words() {
  cat <<'EOF'
about English word of a fixed message of a test
ambient a window class of the hosted runner's desktop
application an image name and a window class of Windows (ApplicationFrameHost.exe, ApplicationFrameWindow)
applicationframehost the image name ApplicationFrameHost.exe in a key of a functional test's line
autofocus the HTML attribute autofocus in a fixed message of a page test
backtrace libtest: the line that names RUST_BACKTRACE
been libtest: a test that has been running for over 60 seconds
cascadia a window class of Windows Terminal (CASCADIA_HOSTING_WINDOW_CLASS)
compares English word of a fixed message of a test
could English word of a fixed message of a test
covering English word of a fixed message of a test
crates the panic hook: the path of the source file in a panic line
deduplicated a key of a functional test's line
defects English word of a fixed message of a test
draining a key of a functional test's line
environment libtest: the line that names RUST_BACKTRACE
exist English word of a fixed message of a test
expanded an ARIA property name that a page test prints
explorer an image name of Windows (explorer.exe)
failures libtest: the list of failed tests
fewer English word of a fixed message of a test
filtered libtest: the summary line
finished libtest: the summary line
fixtures English word of a fixed message of a test
folded English word of a fixed message of a test
gave English word of a fixed message of a test
gecko a window class of Firefox (MozillaWindowClass, Gecko)
hasactions an ARIA property name that a page test prints
have English word of a fixed message of a test
hosting a window class of Windows Terminal (CASCADIA_HOSTING_WINDOW_CLASS)
ignored libtest: the summary line
inconclusive the word INCONCLUSIVE of a test that could not run its case
lay English word of a fixed message of a test
mb a unit of a size
mean English word of a fixed message of a test
measured libtest: the summary line
missed English word of a fixed message of a test
monitors a key of a functional test's line
mozilla a window class of Firefox (MozillaWindowClass)
msedge an image name of Edge (msedge.exe)
multiline an ARIA property name that a page test prints
multiselectable an ARIA property name that a page test prints
note libtest: the line that names RUST_BACKTRACE
panicked the panic hook: thread NAME (ID) panicked at
passed libtest: the summary line
physical a key of a functional test's line
printc the prefix of a line of the print tool's child
remote a key of a functional test's line
running libtest: running N tests
rust libtest: the line that names RUST_BACKTRACE
scanned a key of a functional test's line
search English word of a fixed message of a test
seconds libtest: a test that has been running for over 60 seconds
sendinput the Windows call SendInput, in keys of the functional tests
something English word of a fixed message of a test
sta the window station WinSta0, split at its capital letters
stallc the prefix of the lines of the stall fixture's child
stayed English word of a fixed message of a test
summary English word of a fixed message of a test
systemsettings an image name of Windows (SystemSettings.exe)
teardown a key of a functional test's line
textbox an ARIA role that a page test prints
tf the prefix of a line of the functional tests
tmpdir the variable CARGO_TARGET_TMPDIR in a fixed message
variable libtest: the line that names RUST_BACKTRACE
wandered a key of a functional test's line
widget a window class of the hosted runner's desktop
winsta the window station WinSta0, the one of an interactive session
xaml a window class of a XAML app of Windows
EOF
}

wpr_words() {
  LC_ALL=C awk '
    function emit(atom,    n, parts, i, p, out) {
      n = split(atom, parts, "_")
      for (i = 1; i <= n; i++) {
        p = parts[i]
        if (p == "") continue
        out = ""
        while (match(p, /[a-z0-9][A-Z]|[A-Z][A-Z][a-z]/)) {
          out = out substr(p, 1, RSTART) "\n"
          p = substr(p, RSTART + 1)
        }
        print tolower(out p)
      }
    }
    $0 == "//wpr-blob" { mode = ""; next }
    {
      line = $0
      len = length(line)
      i = 1
      while (i <= len) {
        if (mode == "block") {
          if (substr(line, i, 2) == "/*") { depth++; i += 2; continue }
          if (substr(line, i, 2) == "*/") { depth--; i += 2; if (depth == 0) mode = ""; continue }
          i++
          continue
        }
        if (mode == "str") {
          c = substr(line, i, 1)
          if (c == "\\") { i += 2; continue }
          if (c == "\"") mode = ""
          i++
          continue
        }
        if (mode == "raw") {
          if (substr(line, i, length(endq)) == endq) { i += length(endq); mode = ""; continue }
          i++
          continue
        }
        rest = substr(line, i)
        if (match(rest, /^[^A-Za-z0-9_\/"\047]+/)) { i += RLENGTH; continue }
        if (match(rest, /^[A-Za-z0-9_]+/)) {
          n = RLENGTH
          atom = substr(rest, 1, n)
          after = substr(rest, n + 1)
          if ((atom == "r" || atom == "br") && match(after, /^#*"/)) {
            endq = "\"" substr(after, 1, RLENGTH - 1)
            mode = "raw"
            i += n + RLENGTH
            continue
          }
          if (atom == "b" && substr(after, 1, 1) == "\"") { mode = "str"; i += 2; continue }
          if (atom !~ /^[0-9]/) emit(atom)
          i += n
          continue
        }
        if (substr(rest, 1, 2) == "//") break
        if (substr(rest, 1, 2) == "/*") { mode = "block"; depth = 1; i += 2; continue }
        c = substr(rest, 1, 1)
        if (c == "\"") { mode = "str"; i++; continue }
        if (c == "\047" && match(rest, /^\047(\\[^\047]+|[^\\\047])\047/)) { i += RLENGTH; continue }
        i++
      }
    }
  '
}

wpr_vocabulary() {
  wpr_out=$1
  git ls-tree -r HEAD -- crates/halvo-capture >"$wpr_out.tree" || return 2
  LC_ALL=C awk -F '\t' '$1 ~ / blob / && $2 ~ /\.rs$/ { split($1, f, " "); print f[3] }' "$wpr_out.tree" >"$wpr_out.blobs" || return 2
  [ -s "$wpr_out.blobs" ] || return 2
  git cat-file --batch='//wpr-blob' <"$wpr_out.blobs" >"$wpr_out.src" || return 2
  wpr_words <"$wpr_out.src" >"$wpr_out.words" || return 2
  LC_ALL=C awk -F '\t' '{ gsub(/[^A-Za-z0-9_]/, " ", $2); print $2 }' "$wpr_out.tree" >"$wpr_out.paths" || return 2
  wpr_words <"$wpr_out.paths" >>"$wpr_out.words" || return 2
  wpr_fixed_words >"$wpr_out.fixed" || return 2
  LC_ALL=C awk '{ print $1 }' "$wpr_out.fixed" >>"$wpr_out.words" || return 2
  LC_ALL=C sort -u "$wpr_out.words" >"$wpr_out" || return 2
  [ -s "$wpr_out" ] || return 2
}

wpr_check_lines() {
  LC_ALL=C awk -v name="$2" '
    function stem_ok(p,    s) {
      if (p in vocab) return 1
      if (p ~ /^[0-9]+(ms|us|ns|s|mb)?$/) return 1
      s = p
      sub(/[0-9]+$/, "", s)
      if (s != p && length(s) >= 2 && (s in vocab)) return 1
      return 0
    }
    function part_ok(p,    out, n, subs, j) {
      out = ""
      while (match(p, /[a-z0-9][A-Z]|[A-Z][A-Z][a-z]/)) {
        out = out substr(p, 1, RSTART) " "
        p = substr(p, RSTART + 1)
      }
      n = split(tolower(out p), subs, " ")
      for (j = 1; j <= n; j++) if (!stem_ok(subs[j])) return 0
      return 1
    }
    function letter_ok(a, before) {
      if (prev == "phase" || prev == "phases") return 1
      if (prev == "and" && prev2 ~ /^[a-z]$/) return 1
      if (tok == "a") return 1
      if (a == "s" && before == "\047") return 1
      if (tok == "fixture-native-f") return 1
      if (a == "t" && tok ~ /^t\+[0-9]+s$/) return 1
      return 0
    }
    function atom_ok(a, before,    n, parts, i) {
      if (a ~ /^[0-9]+$/ || a ~ /^0x[0-9A-Fa-f]+$/ || a ~ /^[0-9]+(ms|us|ns|s|mb)$/ || a ~ /^[0-9]+x[0-9]+$/) return 1
      if (a ~ /^[0-9a-f]+$/ && length(a) >= 7 && a ~ /[0-9]/ && a ~ /[a-f]/) return 1
      if (prev == "landings" && a ~ /^[pt]+$/) return 1
      if (a ~ /^[A-Za-z]$/) return letter_ok(a, before)
      n = split(a, parts, "_")
      for (i = 1; i <= n; i++) {
        if (parts[i] == "" || parts[i] ~ /^[0-9]+$/) continue
        if (!part_ok(parts[i])) return 0
      }
      return 1
    }
    function token_ok(    rest, a, before) {
      if (tok ~ /[^ -~]/) return 0
      if (k == 2 && toks[1] == "KNOWN-DEFECT" && tok ~ /^[A-Z][A-Za-z0-9]*(-[A-Za-z0-9]+)+$/) return 1
      rest = tok
      while (match(rest, /[A-Za-z0-9_]+/)) {
        a = substr(rest, RSTART, RLENGTH)
        before = RSTART > 1 ? substr(rest, RSTART - 1, 1) : ""
        rest = substr(rest, RSTART + RLENGTH)
        if (!atom_ok(a, before)) return 0
        prev2 = prev
        prev = tolower(a)
      }
      return 1
    }
    FNR == NR { vocab[$1] = 1; next }
    {
      line = $0
      gsub(/\r/, "", line)
      n = split(line, toks, /[ \t]+/)
      prev = ""
      prev2 = ""
      for (k = 1; k <= n; k++) {
        tok = toks[k]
        if (!token_ok()) {
          print "prints-rule " name " line=" FNR " length=" length(line) " token=" k
          bad++
          break
        }
      }
    }
    END { exit bad > 0 }
  ' "$1" -
}

wpr_print_literals() {
  LC_ALL=C awk -v file="$1" '
    function decode(s,    out, i, c, d) {
      out = ""
      i = 1
      while (i <= length(s)) {
        c = substr(s, i, 1)
        if (c != "\\") { out = out c; i++; continue }
        d = substr(s, i + 1, 1)
        if (d == "n") out = out "\n"
        else if (d == "t") out = out "\t"
        else if (d == "r") out = out ""
        else if (d == "\\" || d == "\"" || d == "\047") out = out d
        else if (d == "x" && substr(s, i + 2, 2) ~ /^([2-6][0-9A-Fa-f]|7[0-9A-Ea-e])$/) { out = out sprintf("%c", hexval(substr(s, i + 2, 2))); i += 2 }
        else if (d == "u" && match(substr(s, i + 2), /^\{[0-9A-Fa-f_]*\}/)) { out = out "\001"; i += RLENGTH }
        else out = out "\001"
        i += 2
      }
      return out
    }
    function hexval(h,    v, k) {
      v = 0
      for (k = 1; k <= 2; k++) v = v * 16 + index("0123456789abcdef", tolower(substr(h, k, 1))) - 1
      return v
    }
    function fields(s,    out, f) {
      out = ""
      while (match(s, /\{\{|\}\}|\{[^{}]*\}/)) {
        f = substr(s, RSTART, RLENGTH)
        out = out substr(s, 1, RSTART - 1) (f == "{{" ? "{" : f == "}}" ? "}" : "0")
        s = substr(s, RSTART + RLENGTH)
      }
      return out s
    }
    function flush(raw,    text, n, parts, k) {
      text = fields(raw ? lit : decode(lit))
      if (!sink && text !~ /^(TF|STALLC|PRINT|PRINTC) /) return
      n = split(text, parts, "\n")
      for (k = 1; k <= n; k++) print file ":" start "\t" parts[k]
    }
    {
      line = $0
      sub(/\r$/, "", line)
      len = length(line)
      i = 1
      if (mode == "str") {
        if (skipws) { sub(/^[ \t]*/, "", line); len = length(line); skipws = 0 }
        else lit = lit "\n"
      } else if (mode == "raw") lit = lit "\n"
      while (i <= len) {
        if (mode == "block") {
          if (substr(line, i, 2) == "/*") { comments++; i += 2; continue }
          if (substr(line, i, 2) == "*/") { comments--; i += 2; if (comments == 0) mode = ""; continue }
          i++
          continue
        }
        if (mode == "str") {
          c = substr(line, i, 1)
          if (c == "\\") {
            if (i == len) { skipws = 1; i++; continue }
            lit = lit substr(line, i, 2)
            i += 2
            continue
          }
          if (c == "\"") { mode = ""; flush(0); i++; continue }
          lit = lit c
          i++
          continue
        }
        if (mode == "raw") {
          if (substr(line, i, length(endq)) == endq) { i += length(endq); mode = ""; flush(1); continue }
          lit = lit substr(line, i, 1)
          i++
          continue
        }
        rest = substr(line, i)
        if (match(rest, /^[ \t]+/)) { i += RLENGTH; continue }
        if (match(rest, /^[A-Za-z0-9_]+/)) {
          n = RLENGTH
          atom = substr(rest, 1, n)
          after = substr(rest, n + 1)
          if ((atom == "r" || atom == "br") && match(after, /^#*"/)) {
            endq = "\"" substr(after, 1, RLENGTH - 1)
            mode = "raw"
            lit = ""
            start = FNR
            i += n + RLENGTH
            continue
          }
          if (atom == "b" && substr(after, 1, 1) == "\"") { mode = "str"; lit = ""; start = FNR; i += 2; continue }
          if (!sink && (atom ~ /^(e?print(ln)?|panic|(debug_)?assert(_eq|_ne)?|unreachable|todo|unimplemented)$/ && after ~ /^![ \t]*[([{]/ ||
              (atom == "say" || atom == "expect" && last == ".") && after ~ /^[ \t]*\(/)) {
            pending = 1
          }
          last = atom
          i += n
          continue
        }
        if (substr(rest, 1, 2) == "//") break
        if (substr(rest, 1, 2) == "/*") { mode = "block"; comments = 1; i += 2; continue }
        c = substr(rest, 1, 1)
        if (c == "\"") { mode = "str"; lit = ""; start = FNR; i++; last = c; continue }
        if (c == "\047" && match(rest, /^\047(\\[^\047]+|[^\\\047])\047/)) { i += RLENGTH; last = "char"; continue }
        if (c == "(" || c == "[" || c == "{") {
          depth++
          if (pending) { sink = depth; pending = 0 }
        } else if (c == ")" || c == "]" || c == "}") {
          if (sink == depth) sink = 0
          depth--
        } else if (c != "!") pending = 0
        last = c
        i++
      }
    }
  '
}
