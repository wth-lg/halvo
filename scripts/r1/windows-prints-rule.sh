#!/bin/sh

wpr_fixed_words() {
  cat <<'EOF'
about English word of a fixed message of a test
allocated English word of a fixed message of a test
ambient a window class of the hosted runner's desktop
application an image name and a window class of Windows (ApplicationFrameHost.exe, ApplicationFrameWindow)
applicationframehost the image name ApplicationFrameHost.exe in a key of a functional test's line
autofocus the HTML attribute autofocus in a fixed message of a page test
backtrace libtest: the line that names RUST_BACKTRACE
been libtest: a test that has been running for over 60 seconds
but English word of the left_out lines of the stall tests
cascadia a window class of Windows Terminal (CASCADIA_HOSTING_WINDOW_CLASS)
come English word of a fixed message of a page test
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
fg the key fg (the foreground flag) of the stall tests' lines, a word of Windows code only inside string literals
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
imagename the filter word IMAGENAME of tasklist.exe in a format string of the tests
inconclusive the word INCONCLUSIVE of a test that could not run its case
lay English word of a fixed message of a test
loops English word of the left_out lines of the stall tests
mb a unit of a size
mean English word of a fixed message of a test
measured libtest: the summary line
missed English word of a fixed message of a test
monitors a key of a functional test's line
mozilla a window class of Firefox (MozillaWindowClass)
msedge an image name of Edge (msedge.exe)
multiline an ARIA property name that a page test prints
multiselectable an ARIA property name that a page test prints
nocapture the libtest flag --nocapture in the command line that a test builds
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
silently the value SilentlyContinue of PowerShell in a format string of the print tool
slip a word of the label NAME-after-slip-N of a control of a page test
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

wpr_forms() {
  cat <<'EOF'
A printed line is split at blanks into tokens. A token passes when it is one of these forms, and the line passes when every token passes.
Every byte is printable ASCII, and the punctuation of a token is drawn from = - ( ) , ; [ ] < > + # ' ` " { } . : / \ @ alone.
Number forms, each a run of letters and digits of its own:
  integer           digits: 42
  decimal           digits, one dot, digits, with no other dot beside them: 6.01
  hex               0x and hex digits alone: 0x80070578
  duration or size  an integer or a decimal and one unit of ns us ms s mb: 150ms 6.01s
  screen size       digits x digits: 1024x768
  hash              7 or more of 0-9 and a-f with at least one digit and one letter: 38082c0e
A word: a run whose parts, split at _ and at capital letters, are each a word of the vocabulary, or such a word with digits at its end.
  The vocabulary holds the parts of the identifiers of the Rust code of crates/halvo-capture at HEAD outside string literals and
  comments, and the parts of its file paths, both read in the Windows view that the ledger of check 50 binds (no path that the
  list macos_only_paths of check 50 names), and the fixed list of wpr_fixed_words, each word with its reason.
A lone letter passes only as: an upper-case letter right after the word phase, or after phases and then after and once more
  (at most two for one phase word); the token a; the s of 's; the f of the token fixture-native-f; the t of t+Ns; and the
  letters p and t of the value of landings.
Punctuation between runs: = - ( ) , ; [ ] < > + # ' ` " { } join anything. The others pass only so:
  .   inside a decimal, at the end of a token, in a program's image name NAME.exe, and in a class name Windows.Name.Name
  :   at the end of a token, before a run that starts with a letter (one : or ):), as :: between two runs that start with a
      letter, as ): or : before a comma, and as one : between two integers that no other : stands beside (a wait in ms and
      its count of rounds)
  /   between two integers (a count of a count), or before a key that = follows
  @   between a run or a ) and an integer (an event and its time)
  \   only in the panic hook's place token crates\PATH.rs:LINE:COLUMN: whose PATH is a .rs path of the Windows view
  %   never
A token with no letter and no digit passes only as [] - ... { or })), and the value after an = with no letter and no digit
  only as [] or - or nothing.
Two whole values pass as they are: a registry hive and its view (HKLM or HKCU, then /, then native or wow6432node), and
  the counts file_reads=ok:N,failed:N,not_read:N,no_element:N of the page tests.
The token after KNOWN-DEFECT, in the second place of its line alone, passes only as an id that the list expected_tests of
  check 50 names.
So an IPv4 or IPv6 address, a Unix or a Windows path and a URL fail, also when they are made of source words and numbers.
The source read of check 53 reads the format strings with each placeholder as a field that matches a letter or a digit, does
  not hold the punctuation at the two ends of a literal to the end rules, and passes a literal of one or two punctuation marks
  alone (a separator of a join or a replace).
EOF
}

wpr_view_tree() {
  wpr_tree=$1
  wpr_skip=$2
  LC_ALL=C awk -F '\t' 'NR == FNR { skip[$1] = 1; next }
    {
      path = $2
      for (prefix in skip) if (path == prefix || index(path, prefix "/") == 1) next
      print
    }' "$wpr_skip" "$wpr_tree"
}

wpr_vocabulary() {
  wpr_out=$1
  git show HEAD:scripts/r1/ci.d/50-windows-functional.sh >"$wpr_out.check50" || return 2
  sed -n '/^macos_only_paths() {$/,/^}$/p' "$wpr_out.check50" >"$wpr_out.macos-lines" || return 2
  LC_ALL=C awk -F '\t' '$1 ~ /^crates\/halvo-capture\// { print $1 }' "$wpr_out.macos-lines" >"$wpr_out.macos" || return 2
  [ -s "$wpr_out.macos" ] || return 2
  sed -n '/^expected_tests() {$/,/^EOF$/p' "$wpr_out.check50" >"$wpr_out.expected" || return 2
  LC_ALL=C awk '/^EOF$/ { inside = 0 } inside { for (i = 2; i <= NF; i++) if ($i !~ /^(group|scope)=/) print "id:" $i } /<<.EOF.$/ { inside = 1 }' "$wpr_out.expected" >"$wpr_out.ids" || return 2
  git ls-tree -r HEAD -- crates/halvo-capture >"$wpr_out.all" || return 2
  wpr_view_tree "$wpr_out.all" "$wpr_out.macos" >"$wpr_out.tree" || return 2
  LC_ALL=C awk -F '\t' '$1 ~ / blob / && $2 ~ /\.rs$/ { split($1, f, " "); print f[3] }' "$wpr_out.tree" >"$wpr_out.blobs" || return 2
  [ -s "$wpr_out.blobs" ] || return 2
  git cat-file --batch='//wpr-blob' <"$wpr_out.blobs" >"$wpr_out.src" || return 2
  wpr_words <"$wpr_out.src" >"$wpr_out.words" || return 2
  LC_ALL=C awk -F '\t' '{ gsub(/[^A-Za-z0-9_]/, " ", $2); print $2 }' "$wpr_out.tree" >"$wpr_out.paths" || return 2
  wpr_words <"$wpr_out.paths" >>"$wpr_out.words" || return 2
  wpr_fixed_words >"$wpr_out.fixed" || return 2
  LC_ALL=C awk '{ print $1 }' "$wpr_out.fixed" >>"$wpr_out.words" || return 2
  LC_ALL=C awk -F '\t' '$2 ~ /\.rs$/ { print "path:" $2 }' "$wpr_out.tree" >>"$wpr_out.words" || return 2
  cat "$wpr_out.ids" >>"$wpr_out.words" || return 2
  LC_ALL=C sort -u "$wpr_out.words" >"$wpr_out" || return 2
  [ -s "$wpr_out" ] || return 2
}

wpr_check_lines() {
  LC_ALL=C awk -v name="$2" -v source="${3:-}" '
    function number_ok(p) {
      if (p ~ /^[0-9]+$/ || p ~ /^[0-9]+(ns|us|ms|s|mb)$/ || p ~ /^0x[0-9A-Fa-f]+$/ || p ~ /^[0-9]+x[0-9]+$/) return 1
      return p ~ /^[0-9a-f]+$/ && length(p) >= 7 && p ~ /[0-9]/ && p ~ /[a-f]/
    }
    function stem_ok(p,    s) {
      if (p in vocab || number_ok(p)) return 1
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
    function letter_ok(a, before, after) {
      if (a ~ /^[A-Z]$/ && (prev == "phase" || prev == "phases")) { phase_word = prev; phase_count = 1; by_phase = 1; return 1 }
      if (a ~ /^[A-Z]$/ && prev == "and" && phase2 && phase_word == "phases" && phase_count < 2) { phase_count++; by_phase = 1; return 1 }
      if (tok == "a") return 1
      if (a == "s" && before == "\047") return 1
      if (tok == "fixture-native-f") return 1
      if (a == "t" && (after ~ /^\+[0-9]+s$/ || source != "" && after == "+\003")) return 1
      return 0
    }
    function atom_ok(a, before, after,    n, parts, i) {
      by_phase = 0
      if (source != "" && a == "\003") return 1
      if (prev == "landings" && a ~ /^[pt]+$/) return 1
      if (a ~ /^[A-Za-z]$/) return letter_ok(a, before, after)
      n = split(a, parts, "_")
      for (i = 1; i <= n; i++) {
        if (parts[i] == "" || number_ok(parts[i])) continue
        if (!part_ok(parts[i])) return 0
      }
      return 1
    }
    function digits(x) {
      return x ~ /^[0-9]+$/ || source != "" && x == "\003"
    }
    function sep_ok(s, where, left, right, next_sep, prev_sep) {
      if (s ~ /[^-=(),;\[\]<>+#\047`"{}.:\/\\@]/) return 0
      if (s !~ /[.:\/\\@]/) return 1
      if (source != "" && where != "mid") return 1
      if (where == "end") return s ~ /^[)\]"]*[.:][)\]"`\047,;]*$/
      if (where == "start") return 0
      if (s == ":" && left ~ /^[0-9]+$/ && right ~ /^[0-9]+$/) return prev_sep !~ /:/ && next_sep !~ /:/
      if (s == ":" || s == "):") return right ~ /^[A-Za-z]/ || source != "" && right == "\003"
      if (s == "::") return left ~ /^[A-Za-z]/ && right ~ /^[A-Za-z]/
      if (s == "):," || s == ":,") return 1
      if (s == "/") return digits(left) && digits(right) || next_sep ~ /^=/
      if (s == "@" || s == ")@") return digits(right)
      return 0
    }
    function undot(t, re,    out, m) {
      out = ""
      while (match(t, re)) {
        m = substr(t, RSTART, RLENGTH)
        gsub(/\./, "_", m)
        out = out substr(t, 1, RSTART - 1) m
        t = substr(t, RSTART + RLENGTH)
      }
      return out t
    }
    function token_ok(    t, p, n, parts, i, m, atoms, seps, rest) {
      if (source == "" && tok ~ /[^ -~]/ || source != "" && tok ~ /[^ -~\003]/) return 0
      if (tok !~ /[A-Za-z0-9\003]/) {
        if (source != "" && ntok == 1 && length(tok) <= 2 && tok !~ /[^-=(),;\[\]<>+#\047`"{}.:\/\\@_]/) return 1
        return tok == "[]" || tok == "-" || tok == "..." || tok == "{" || tok == "}))"
      }
      if (k == 2 && toks[1] == "KNOWN-DEFECT") return ("id:" tok) in vocab || source != "" && tok == "\003"
      if (tok ~ /^[a-z_]+=HK(LM|CU)\/(native|wow6432node)$/) { sub(/\//, "_", tok) }
      if (tok ~ /^file_reads=ok:[0-9]+,failed:[0-9]+,not_read:[0-9]+,no_element:[0-9]+$/) return 1
      if (tok ~ /^crates[\\\/][-A-Za-z0-9_.\\\/]+\.rs:[0-9]+:[0-9]+:$/) {
        p = tok
        gsub(/\\/, "/", p)
        sub(/:[0-9]+:[0-9]+:$/, "", p)
        return ("path:" p) in vocab
      }
      t = undot(tok, "(^|[^A-Za-z0-9_.])[0-9]+\\.[0-9]+(ns|us|ms|s|mb)?([^A-Za-z0-9_.]|$)")
      t = undot(t, "[A-Za-z0-9_-]+\\.exe([^A-Za-z0-9_.]|$)")
      t = undot(t, "(^|[^A-Za-z0-9_.])Windows(\\.[A-Z][A-Za-z0-9]*)+([^A-Za-z0-9_.]|$)")
      n = split(t, parts, "=")
      for (i = 2; i <= n; i++) if (parts[i] !~ /[A-Za-z0-9\003]/ && parts[i] != "" && parts[i] != "[]" && parts[i] != "-") return 0
      m = 0
      rest = t
      while (match(rest, /[A-Za-z0-9_]+|\003/)) {
        seps[m] = substr(rest, 1, RSTART - 1)
        m++
        atoms[m] = substr(rest, RSTART, RLENGTH)
        rest = substr(rest, RSTART + RLENGTH)
      }
      seps[m] = rest
      if (seps[0] != "" && !sep_ok(seps[0], "start", "", atoms[1], "")) return 0
      for (i = 1; i <= m; i++) {
        if (source != "" && (i > 1 && atoms[i - 1] == "\003" && seps[i - 1] == "" && atoms[i] ~ /^(ns|us|ms|s|mb|x)$/ || atoms[i] == "0x" && atoms[i + 1] == "\003" && seps[i] == "")) {
          by_phase = 0
        } else if (!atom_ok(atoms[i], substr(seps[i - 1], length(seps[i - 1]), 1), seps[i] atoms[i + 1])) return 0
        prev = tolower(atoms[i])
        phase2 = phase1
        phase1 = by_phase
        if (i < m && !sep_ok(seps[i], "mid", atoms[i], atoms[i + 1], seps[i + 1], seps[i - 1])) return 0
      }
      if (seps[m] != "" && !sep_ok(seps[m], "end", atoms[m], "", "")) return 0
      return 1
    }
    FNR == NR { vocab[$1] = 1; next }
    {
      line = $0
      gsub(/\r/, "", line)
      ntok = split(line, toks, / +/)
      prev = ""
      phase1 = 0
      for (k = 1; k <= ntok; k++) {
        tok = toks[k]
        if (tok == "") continue
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

wpr_check_log() {
  [ -f "$2" ] && [ -r "$2" ] && [ -s "$3" ] || return 2
  wpr_check_lines "$3" "$1" <"$2"
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
        out = out substr(s, 1, RSTART - 1) (f == "{{" ? "{" : f == "}}" ? "}" : "\003")
        s = substr(s, RSTART + RLENGTH)
      }
      return out s
    }
    function flush(raw,    text, n, parts, k) {
      text = fields(raw ? lit : decode(lit))
      if (!sink && !(depth in first)) return
      delete first[depth]
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
          if (!sink && (atom ~ /^(e?print(ln)?|dbg|panic|(debug_)?assert(_eq|_ne)?|unreachable|todo|unimplemented)$/ && after ~ /^[ \t]*!/ ||
              (atom == "say" || atom == "panic_any" || atom == "expect" && (last == "." || last == ":")) && after ~ /^[ \t]*(\(|$)/)) {
            pending = 1
          } else if (!sink && atom ~ /^(format|write(ln)?)$/ && after ~ /^[ \t]*!/) {
            pending = 2
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
          if (pending == 1) sink = depth
          if (pending == 2) first[depth] = 1
          pending = 0
        } else if (c == ")" || c == "]" || c == "}") {
          if (sink == depth) sink = 0
          delete first[depth]
          depth--
        } else if (c != "!") pending = 0
        last = c
        i++
      }
    }
  '
}
