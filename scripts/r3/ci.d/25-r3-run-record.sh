#!/bin/sh
# jobs: macos windows
# rule: No tracked .rs file outside crates/halvo-schema and crates/halvo-schema-core builds the stored agent_run record by hand, read lexically: a place is a string literal whose text holds the keys outcome and earlier_page, or a json! map that holds both as keys, counted per file, and each file's count equals its row in known(), whose rows, places and sorted path and count pairs equal the pins beside it, crates/halvo-schema/tests/later_kinds.rs read on its frozen form by the keys outcome and handbacks: the list may shrink with its pins and may not grow, and new code builds the record with AgentRun::new (U1.10).
# lane: free
set -u
LC_ALL=C
export LC_ALL
name=25-r3-run-record
nl='
'
frozen=crates/halvo-schema/tests/later_kinds.rs
case $0 in
  /*) self=$0 ;;
  *) self=$(pwd)/$0 ;;
esac

known() {
  cat <<'EOF'
crates/halvo-learn-gate/src/stub.rs 1 learning run, until its unit calls the maker
crates/halvo-learn-store/src/tests/host.rs 1 learning run, until its unit calls the maker
crates/halvo-learn-store/src/tests/pass.rs 1 learning run, until its unit calls the maker
crates/halvo-learn-store/src/tests/ruled.rs 1 learning run, until its unit calls the maker
crates/halvo-schema/tests/later_kinds.rs 2 workflow run, its frozen entry of agent_run checked against itself
EOF
}

pinned_rows=5
pinned_places=6
pinned_paths=489507615

program='
function keys(text, k,    n, re) {
  n = 0
  re = "\"" k "\"[ \t\r\n]*:"
  while (match(text, re)) {
    n++
    text = substr(text, RSTART + RLENGTH)
  }
  return n
}
function least(a, b) {
  return a < b ? a : b
}
function ident(c) {
  return c ~ /[A-Za-z0-9_]/
}
function literal_end(text, at,    n) {
  n = least(keys(text, ka), keys(text, kb))
  if (n) {
    count += n
    where = where (where == "" ? "" : ",") at
  }
  if (jdepth) {
    pend = text
    pending = 1
  }
}
function scan(path,    file, line, r, i, n, c, d, q, j, h, lineno) {
  file = root "/" path
  ka = "outcome"
  kb = (path == frozen) ? "handbacks" : "earlier_page"
  count = 0
  where = ""
  state = 0
  jdepth = 0
  jwait = 0
  pending = 0
  word = ""
  last = ""
  lineno = 0
  while ((r = (getline line < file)) > 0) {
    lineno++
    sub(/\r$/, "", line)
    n = length(line)
    i = 1
    if (state == 1 || state == 2) text = text "\n"
    while (i <= n) {
      c = substr(line, i, 1)
      if (state == 1) {
        if (c == "\\") {
          d = substr(line, i + 1, 1)
          if (d == "\"" || d == "\\") text = text d
          else if (d != "") text = text c d
          i += 2
          continue
        }
        if (c == "\"") {
          state = 0
          literal_end(text, start)
        } else text = text c
        i++
        continue
      }
      if (state == 2) {
        if (c == "\"" && substr(line, i + 1, h) == substr(hashes, 1, h)) {
          state = 0
          i += 1 + h
          literal_end(text, start)
          continue
        }
        text = text c
        i++
        continue
      }
      if (state == 3) {
        d = substr(line, i, 2)
        if (d == "/*") {
          depth++
          i += 2
        } else if (d == "*/") {
          depth--
          i += 2
          if (!depth) state = 0
        } else i++
        continue
      }
      d = substr(line, i, 2)
      if (d == "//") break
      if (d == "/*") {
        state = 3
        depth = 1
        i += 2
        continue
      }
      if (c ~ /[ \t]/) {
        if (word != "") last = word
        word = ""
        i++
        continue
      }
      if (pending) {
        pending = 0
        if (c == ":" && substr(line, i + 1, 1) != ":") {
          if (pend == ka) jka++
          if (pend == kb) jkb++
        }
      }
      if (jwait && c != "!") {
        jwait = 0
        if (c == "(" || c == "[" || c == "{") {
          jdepth = 1
          jka = 0
          jkb = 0
          jstart = lineno
          i++
          continue
        }
      }
      if (c == "\"") {
        if (word != "") last = word
        word = ""
        state = 1
        text = ""
        start = lineno
        i++
        continue
      }
      if ((c == "r" || ((c == "b" || c == "c") && substr(line, i + 1, 1) == "r")) && !ident(substr(line, i - 1, 1)) && match(substr(line, i + (c == "r" ? 1 : 2)), /^#*"/)) {
        h = RLENGTH - 1
        state = 2
        text = ""
        start = lineno
        if (word != "") last = word
        word = ""
        i += (c == "r" ? 1 : 2) + RLENGTH
        continue
      }
      if (c == "'\''") {
        d = substr(line, i + 1, 1)
        if (d == "\\") {
          j = index(substr(line, i + 3), "'\''")
          i += j ? j + 3 : 3
        } else if (substr(line, i + 2, 1) == "'\''") {
          i += 3
        } else if (d > "~") {
          for (j = 2; j <= 5 && substr(line, i + j, 1) != "'\''"; j++) {}
          i += j <= 5 ? j + 1 : 1
        } else i++
        word = ""
        last = ""
        continue
      }
      if (ident(c)) {
        word = word c
        i++
        continue
      }
      if (word != "") last = word
      word = ""
      if (c == "!" && last == "json" && !jdepth) jwait = 1
      else last = ""
      if (jdepth) {
        if (c == "(" || c == "[" || c == "{") jdepth++
        else if (c == ")" || c == "]" || c == "}") {
          jdepth--
          if (!jdepth) {
            q = least(jka, jkb)
            if (q) {
              count += q
              where = where (where == "" ? "" : ",") jstart
            }
          }
        }
      }
      i++
    }
    if (word != "") last = word
    word = ""
  }
  close(file)
  if (r < 0) {
    printf "run-record file=%s unread\n", path
    faults++
  }
  if (count) printf "%s %d %s\n", path, count, where >> placelist
  return count
}
BEGIN {
  hashes = "################################################################"
  k = 0
  while ((r = (getline line < knownlist)) > 0) {
    k++
    if (split(line, w, " ") == 0) continue
    if (w[1] in want || w[2] !~ /^[1-9]/ || w[2] ~ /[^0-9]/) {
      printf "run-record known-line=%d\n", k
      faults++
      continue
    }
    want[w[1]] = w[2]
    order[++nk] = w[1]
    kp += w[2]
  }
  if (r < 0) {
    printf "run-record known-list=unread\n"
    faults++
  }
  close(knownlist)
  if (nk > pin_rows) {
    printf "run-record pin=rows known=%d pinned=%d: the known list may not grow; a new row needs a ruling, else build the record with AgentRun::new at the new place\n", nk, pin_rows
    faults++
  } else if (nk < pin_rows) {
    printf "run-record pin=rows known=%d pinned=%d: set pinned_rows=%d and pinned_paths=%s beside known() in scripts/r3/ci.d/25-r3-run-record.sh, in the commit that moves the place to the maker\n", nk, pin_rows, nk, paths
    faults++
  }
  if (kp > pin_places) {
    printf "run-record pin=places known=%d pinned=%d: the known list may not grow; a higher count needs a ruling, else build the record with AgentRun::new at the new place\n", kp, pin_places
    faults++
  } else if (kp < pin_places) {
    printf "run-record pin=places known=%d pinned=%d: set pinned_places=%d and pinned_paths=%s beside known() in scripts/r3/ci.d/25-r3-run-record.sh, in the commit that moves the place to the maker\n", kp, pin_places, kp, paths
    faults++
  }
  if (nk == pin_rows && kp == pin_places && paths != pin_paths) {
    printf "run-record pin=paths: known() names a file or a count that pinned_paths does not allow; a new row, a raised count or a count moved to another row needs a ruling (after a move that lowered pinned_rows or pinned_places, pinned_paths=%s)\n", paths
    faults++
  }
  while ((r = (getline path < filelist)) > 0) {
    sub(/\r$/, "", path)
    if (substr(path, 1, 1) == "\"") {
      printf "run-record name=quoted\n"
      faults++
      continue
    }
    if (path != frozen && (substr(path, 1, 20) == "crates/halvo-schema/" || substr(path, 1, 25) == "crates/halvo-schema-core/")) continue
    files++
    inscope[path] = 1
  }
  if (r < 0) {
    printf "run-record file-list=unread\n"
    faults++
  }
  close(filelist)
  while ((r = (getline path < readlist)) > 0) {
    sub(/\r$/, "", path)
    if (!(path in inscope) || (path in wasread)) continue
    wasread[path] = 1
    reads++
    c = scan(path)
    if (c) {
      got[path] = c
      lines[path] = where
      seen[++ns] = path
      total += c
    }
  }
  if (r < 0) {
    printf "run-record read-list=unread\n"
    faults++
  }
  close(readlist)
  for (i = 1; i <= ns; i++) {
    p = seen[i]
    if (!(p in want)) {
      printf "run-record unknown=%s places=%d lines=%s: build the record with halvo_schema::executable::record::AgentRun::new there; the known list in scripts/r3/ci.d/25-r3-run-record.sh may not grow\n", p, got[p], lines[p]
      faults++
    } else if (got[p] > want[p]) {
      printf "run-record count=%s places=%d known=%d lines=%s: build the record with AgentRun::new at the new place; the known list in scripts/r3/ci.d/25-r3-run-record.sh may not grow\n", p, got[p], want[p], lines[p]
      faults++
    } else if (got[p] < want[p]) {
      printf "run-record count=%s places=%d known=%d: set its row in known() to %d and lower pinned_places and pinned_paths beside it in scripts/r3/ci.d/25-r3-run-record.sh\n", p, got[p], want[p], got[p]
      faults++
    }
  }
  for (i = 1; i <= nk; i++) {
    p = order[i]
    if (!(p in got)) {
      printf "run-record gone=%s known=%d: take its row out of known() and lower pinned_rows, pinned_places and pinned_paths beside it in scripts/r3/ci.d/25-r3-run-record.sh\n", p, want[p]
      faults++
    }
  }
  printf "run-record files=%d read=%d places=%d known=%d faults=%d\n", files, reads, total, nk, faults
  exit faults ? 1 : 0
}
'

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

paths_of() {
  awk 'NF { print $1, $2 }' "$1" | sort | cksum | awk '{ print $1 }'
}

measure() {
  repo=$1
  list=$2
  out=$3
  read -r pin_rows pin_places pin_paths <"$4" || fail pins
  paths=$(paths_of "$list") || fail setup
  git -C "$repo" -c core.quotePath=false ls-files -- '*.rs' >"$out/files" || fail git
  git -C "$repo" -c core.quotePath=false grep -l -I -F --all-match -e outcome -e earlier_page -- '*.rs' >"$out/read"
  case $? in
    0 | 1) ;;
    *) fail git ;;
  esac
  printf '%s\n' "$frozen" >>"$out/read" || fail setup
  : >"$out/places.txt" || fail setup
  awk -v root="$repo" -v frozen="$frozen" -v placelist="$out/places.txt" \
    -v knownlist="$list" -v filelist="$out/files" -v readlist="$out/read" \
    -v pin_rows="$pin_rows" -v pin_places="$pin_places" -v pin_paths="$pin_paths" \
    -v paths="$paths" "$program"
  case $? in
    0) printf 'PASS %s\n' "$name" ;;
    1) fail places ;;
    *) fail awk ;;
  esac
}

main() {
  repo=${CI_D_REPO:-.}
  scratch=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail setup
  artifacts=${CI_D_ARTIFACTS:-$scratch}
  known >"$scratch/known" || fail setup
  printf '%s %s %s\n' "$pinned_rows" "$pinned_places" "$pinned_paths" >"$scratch/pins" || fail setup
  measure "$repo" "$scratch/known" "$artifacts" "$scratch/pins"
}

help() {
  cat <<'EOF'
usage: sh scripts/r3/ci.d/25-r3-run-record.sh [--self-test | --help]

The stored agent_run record (contracts A7.4) has one maker, AgentRun::new in
crates/halvo-schema/src/executable/record.rs. A tool or a test outside the schema crate
builds the record with it and writes its text with to_json; a caller that needs a JSON
value parses that text and may then change keys of it.

The check is lexical: it reads tracked .rs files of the work tree as Rust tokens (line
and block comments, string literals with escapes, raw strings, char literals), not
Rust's syntax. It reads only the files in which git grep finds both words outcome and
earlier_page, and crates/halvo-schema/tests/later_kinds.rs.

A place:
  a string literal (plain, raw or byte, also a format! string) whose text holds the key
  outcome and the key earlier_page, each a quoted name followed by a colon; a literal
  with n of each counts n places;
  a json! call whose keys, a string literal followed by a colon anywhere inside it,
  name outcome and earlier_page; a call with n of each counts n places.
Not a place: a struct literal of AgentRun, a list of keys (a key followed by anything but
a colon), a record made by AgentRun::new or by a helper that calls it and then changed
by its keys, and any text in a comment.

Not read: crates/halvo-schema and crates/halvo-schema-core, the form's own source (the
record's type, its tests, its golden files and the kinds' entry), but
crates/halvo-schema/tests/later_kinds.rs, which holds the workflow run's frozen entry of
agent_run from before the run's list: there a place holds the keys outcome and
handbacks.

Forms that it does not read (each builds the record by hand and passes):
  a key written with an escape (earlier_page) or split between literals (concat!);
  a key that a const or a variable names inside json!; a map built by insert calls; a
  record in a file that is not .rs, untracked or outside the work tree; json! under
  another name by use ... as; serde_json::Map or a struct of another crate that
  serializes to the record.

The known list, known() in this script, holds each file of another run with its count of
places. A file's count must equal its row: a new place builds the record with the maker.
Beside it, pinned_rows, pinned_places and pinned_paths (a cksum of the sorted pairs of
path and count) pin the list: it may shrink with its pins and may not grow, and a count
may not move from one row to another without a ruling. A raised pin passes, because the
check cannot know a ruling: the review of the diff is the gate.
EOF
}

broken() {
  printf 'FAIL self-test:%s (fixture %s)\n' "$name" "$1"
  exit 1
}

repo() {
  label=$1
  git init -q "$work/$label" >/dev/null 2>&1 || broken "$label: git init"
  : >"$work/$label.known" || broken "$label: known"
}

put() {
  file=$1
  shift
  mkdir -p "$work/$label/${file%/*}" || broken "$label: mkdir"
  printf '%s\n' "$@" >"$work/$label/$file" || broken "$label: write"
  git -C "$work/$label" add -- "$file" || broken "$label: add"
}

put_crlf() {
  file=$1
  shift
  mkdir -p "$work/$label/${file%/*}" || broken "$label: mkdir"
  printf '%s\r\n' "$@" >"$work/$label/$file" || broken "$label: write"
  git -C "$work/$label" -c core.autocrlf=false add -- "$file" || broken "$label: add"
}

put_from_head() {
  file=$1
  mkdir -p "$work/$label/${file%/*}" || broken "$label: mkdir"
  git -C "$source" show "HEAD:$file" >"$work/$label/$file" || broken "$label: $file is not in HEAD of the checkout"
  git -C "$work/$label" add -- "$file" || broken "$label: add"
}

row() {
  printf '%s %s a run\n' "$1" "$2" >>"$work/$label.known" || broken "$label: known"
}

pin_file() {
  label=$1
  rows=$2
  places=$3
  shift 3
  : >"$work/$label.pinned-paths" || broken "$label: pinned paths"
  while [ $# -ge 2 ]; do
    printf '%s %s pinned\n' "$1" "$2" >>"$work/$label.pinned-paths" || broken "$label: pinned paths"
    shift 2
  done
  digest=$(paths_of "$work/$label.pinned-paths") || broken "$label: digest"
  printf '%s %s %s\n' "$rows" "$places" "$digest" >"$work/$label.pins" || broken "$label: pins"
}

pins_of() {
  awk 'NF { r++; p += $2 } END { printf "%d %d ", r, p }' "$1" || return 1
  paths_of "$1"
}

case_of() {
  [ -f "$work/$1.pins" ] || pins_of "$work/$1.known" >"$work/$1.pins" || broken "$1: pins"
  out=$(mkdir -p "$work/$1.out" && measure "$work/$1" "$work/$1.known" "$work/$1.out" "$work/$1.pins" 2>&1)
  verdict "$1" "$2" "$3"
}

verdict() {
  code=$?
  last=${out##*"$nl"}
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  case $out in
    *"$3"*) ;;
    *) got=$got-without-the-text ;;
  esac
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$got"
  if [ "$got" != "$2" ]; then
    wrong="$wrong $1"
    printf '%s\n' "$out" >"$artifacts/self-test-$1.txt"
  elif [ "$2" != PASS ]; then
    rejected=$((rejected + 1))
  fi
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || broken mktemp
  artifacts=${CI_D_ARTIFACTS:-$work}
  source=${CI_D_REPO:-.}
  wrong=
  rejected=0
  raw='const RECORD: &str = r#"{"v":1,"outcome":"completed","held_earlier":null,"earlier_page":null}"#;'
  map='    json!({'
  map_keys='        "mode": "replay", "outcome": "completed", "earlier_page": null,'

  repo nothing
  put crates/a/tests/t.rs 'fn f() {}'
  case_of nothing PASS 'run-record files=1 read=0 places=0 known=0 faults=0'

  repo a-new-raw-text
  put crates/a/tests/t.rs "$raw"
  case_of a-new-raw-text FAIL 'run-record unknown=crates/a/tests/t.rs places=1'

  repo a-new-json-map
  put crates/a/tests/common/mod.rs 'pub fn run_record() -> Value {' "$map" "$map_keys" '    })' '}'
  case_of a-new-json-map FAIL 'run-record unknown=crates/a/tests/common/mod.rs places=1'

  repo a-json-map-of-serde-json-by-its-path
  put src/x.rs 'fn f() -> Value {' '    serde_json::json!(' '        {' '            "outcome": "failed",' '            "earlier_page"  :  null' '        }' '    )' '}'
  case_of a-json-map-of-serde-json-by-its-path FAIL 'run-record unknown=src/x.rs places=1'

  repo a-new-format-text
  put crates/a/tests/t.rs 'fn line(run: u32) -> String {' '    format!(' '        r#"{{"run":"5{run:031x}","outcome":"completed","earlier_page":null}}"#' '    )' '}'
  case_of a-new-format-text FAIL 'run-record unknown=crates/a/tests/t.rs places=1'

  repo a-new-escaped-text
  put crates/a/tests/t.rs 'const R: &str = "{\"outcome\":\"failed\",\"earlier_page\":null}";'
  case_of a-new-escaped-text FAIL 'run-record unknown=crates/a/tests/t.rs places=1'

  repo a-text-over-lines-with-hashes
  put crates/a/src/x.rs 'const R: &str = r##"{' '  "outcome": "failed", "x": "#",' '  "earlier_page": null' '}"##;'
  case_of a-text-over-lines-with-hashes FAIL 'run-record unknown=crates/a/src/x.rs places=1'

  repo a-byte-text
  put crates/a/src/x.rs 'const R: &[u8] = br#"{"outcome":"failed","earlier_page":null}"#;'
  case_of a-byte-text FAIL 'run-record unknown=crates/a/src/x.rs places=1'

  repo two-records-in-one-text
  put crates/a/tests/t.rs 'const R: &str = r#"{"outcome":"failed","earlier_page":null}' '{"outcome":"completed","earlier_page":1}"#;'
  case_of two-records-in-one-text FAIL 'run-record unknown=crates/a/tests/t.rs places=2'

  repo two-records-in-one-json-call
  put crates/a/tests/t.rs 'fn f() -> Value {' '    json!([' "        {\"outcome\": 1, \"earlier_page\": null}," "        {\"outcome\": 2, \"earlier_page\": null}" '    ])' '}'
  case_of two-records-in-one-json-call FAIL 'run-record unknown=crates/a/tests/t.rs places=2'

  repo a-char-quote-before-the-text
  put crates/a/tests/t.rs "fn f<'a>(s: &'a str) -> bool { s.ends_with('\"') || s.ends_with('\\'') }" "$raw"
  case_of a-char-quote-before-the-text FAIL 'run-record unknown=crates/a/tests/t.rs places=1'

  repo crlf
  put_crlf crates/a/tests/t.rs 'fn f() -> Value {' "$map" "$map_keys" '    })' '}'
  case_of crlf FAIL 'run-record unknown=crates/a/tests/t.rs places=1'

  repo crlf-before-a-key-s-colon
  put_crlf crates/a/tests/t.rs 'fn f() -> Value {' '    json!({"outcome"' '        : 1, "earlier_page"' '        : null})' '}'
  case_of crlf-before-a-key-s-colon FAIL 'run-record unknown=crates/a/tests/t.rs places=1 lines=2'

  repo a-known-row-rises
  put crates/b/src/stub.rs "$raw" "$raw"
  row crates/b/src/stub.rs 1
  case_of a-known-row-rises FAIL 'run-record count=crates/b/src/stub.rs places=2 known=1'

  repo a-known-row-holds
  put crates/b/src/stub.rs "$raw"
  row crates/b/src/stub.rs 1
  case_of a-known-row-holds PASS 'run-record files=1 read=1 places=1 known=1 faults=0'

  repo a-known-row-falls
  put crates/b/src/stub.rs "$raw"
  row crates/b/src/stub.rs 2
  case_of a-known-row-falls FAIL 'run-record count=crates/b/src/stub.rs places=1 known=2: set its row in known() to 1'

  repo a-known-row-gone
  put crates/b/src/stub.rs 'fn f() {}'
  row crates/b/src/stub.rs 1
  case_of a-known-row-gone FAIL 'run-record gone=crates/b/src/stub.rs known=1: take its row out of known()'

  repo a-known-line-twice
  put crates/b/src/stub.rs "$raw"
  row crates/b/src/stub.rs 1
  row crates/b/src/stub.rs 1
  case_of a-known-line-twice FAIL 'run-record known-line=2'

  repo a-known-count-of-zero
  put crates/b/src/stub.rs 'fn f() {}'
  row crates/b/src/stub.rs 0
  case_of a-known-count-of-zero FAIL 'run-record known-line=1'

  repo a-row-added-with-its-place
  put crates/b/src/stub.rs "$raw"
  row crates/b/src/stub.rs 1
  pin_file a-row-added-with-its-place 0 0
  case_of a-row-added-with-its-place FAIL 'run-record pin=rows known=1 pinned=0'

  repo a-count-raised-with-a-second-place
  put crates/b/src/stub.rs "$raw" "$raw"
  row crates/b/src/stub.rs 2
  pin_file a-count-raised-with-a-second-place 1 1 crates/b/src/stub.rs 1
  case_of a-count-raised-with-a-second-place FAIL 'run-record pin=places known=2 pinned=1'

  repo a-move-with-row-and-pins-lowered
  put crates/b/src/stub.rs 'fn f() {}'
  pin_file a-move-with-row-and-pins-lowered 0 0
  case_of a-move-with-row-and-pins-lowered PASS 'run-record files=1 read=0 places=0 known=0 faults=0'

  repo a-move-with-the-pins-left
  put crates/b/src/stub.rs 'fn f() {}'
  pin_file a-move-with-the-pins-left 1 1 crates/b/src/stub.rs 1
  case_of a-move-with-the-pins-left FAIL 'run-record pin=rows known=0 pinned=1'

  repo a-row-swapped-under-the-pins
  put crates/c/src/x.rs "$raw"
  row crates/c/src/x.rs 1
  pin_file a-row-swapped-under-the-pins 1 1 crates/b/src/stub.rs 1
  case_of a-row-swapped-under-the-pins FAIL 'run-record pin=paths'

  repo the-schema-crates-are-the-form-s-source
  put crates/halvo-schema/tests/r3_bodies.rs "$raw"
  put crates/halvo-schema/src/executable/record.rs "$raw"
  put crates/halvo-schema-core/src/executable.rs "$raw"
  case_of the-schema-crates-are-the-form-s-source PASS 'run-record files=0 read=0 places=0 known=0 faults=0'

  repo a-crate-named-like-the-schema-is-read
  put crates/halvo-schema-x/src/x.rs "$raw"
  case_of a-crate-named-like-the-schema-is-read FAIL 'run-record unknown=crates/halvo-schema-x/src/x.rs places=1'

  repo the-frozen-entry-by-outcome-and-handbacks
  put "$frozen" 'const AGENT_RUN: &str = r#"{"outcome":"done","handbacks":1,"model":"x"}"#;' 'const OTHER: &str = r#"{"outcome":"handed_back","handbacks":0}"#;'
  row "$frozen" 2
  case_of the-frozen-entry-by-outcome-and-handbacks PASS 'run-record files=1 read=1 places=2 known=1 faults=0'

  repo the-frozen-entry-rises
  put "$frozen" 'const AGENT_RUN: &str = r#"{"outcome":"done","handbacks":1}"#;' 'const OTHER: &str = r#"{"outcome":"handed_back","handbacks":0}"#;' 'const THIRD: &str = r#"{"outcome":"failed","handbacks":0}"#;'
  row "$frozen" 2
  case_of the-frozen-entry-rises FAIL "run-record count=$frozen places=3 known=2"

  repo a-struct-literal-and-a-list-of-keys
  put crates/a/src/end.rs 'let run = AgentRun {' '    outcome: Outcome::Failed,' '    earlier_page: None,' '};' 'const KEYS: [&str; 2] = ["outcome", "earlier_page"];' 'let changes = [("outcome", json!("failed")), ("earlier_page", json!(6))];' 'record["earlier_page"] = json!(null); record["outcome"] = json!("failed");'
  case_of a-struct-literal-and-a-list-of-keys PASS 'run-record files=1 read=1 places=0 known=0 faults=0'

  repo keys-in-comments
  put crates/a/src/x.rs '// {"outcome":"failed","earlier_page":null}' '/* json!({"outcome": 1, "earlier_page": 2})' '   /* nested */ r#"{"outcome":1,"earlier_page":1}"# */' '/// r#"{"outcome":1,"earlier_page":1}"#'
  case_of keys-in-comments PASS 'run-record files=1 read=1 places=0 known=0 faults=0'

  repo names-as-values-and-in-lists
  put crates/a/src/x.rs 'fn f() -> Value {' '    json!({"key": "outcome", "other": "earlier_page", "list": ["outcome", "earlier_page"]})' '}' 'const T: &str = r#"{"field":"outcome","then":"earlier_page","list":["outcome","earlier_page"]}"#;'
  case_of names-as-values-and-in-lists PASS 'run-record files=1 read=1 places=0 known=0 faults=0'

  repo one-word-alone-is-not-read
  put crates/a/src/x.rs 'const R: &str = r#"{"outcome":"failed"}"#;'
  put crates/a/src/y.rs 'const E: &str = r#"{"earlier_page":null}"#;'
  case_of one-word-alone-is-not-read PASS 'run-record files=2 read=0 places=0 known=0 faults=0'

  repo outcome-alone
  put crates/a/src/x.rs 'const R: &str = r#"{"outcome":"failed"}"#;' 'const E: &str = r#"{"earlier_page":null}"#;' 'fn f() -> Value { json!({"outcome": 1, "x": {"earlier": 1}}) }'
  case_of outcome-alone PASS 'run-record files=1 read=1 places=0 known=0 faults=0'

  repo untracked
  put crates/a/tests/t.rs 'fn f() {}'
  printf '%s\n' "$raw" >"$work/untracked/crates/a/tests/u.rs" || broken untracked
  case_of untracked PASS 'run-record files=1 read=0 places=0 known=0 faults=0'

  repo not-rust
  put crates/a/tests/t.json '{"outcome":"failed","earlier_page":null}'
  case_of not-rust PASS 'run-record files=0 read=0 places=0 known=0 faults=0'

  repo quoted-name
  git -C "$work/quoted-name" config core.protectNTFS false || broken "quoted-name: config"
  blob=$(git -C "$work/quoted-name" hash-object -w --stdin </dev/null) || broken "quoted-name: blob"
  (cd "$work/quoted-name" && MSYS_NO_PATHCONV=1 MSYS2_ARG_CONV_EXCL='*' git update-index --add --cacheinfo "100644,$blob,crates/a/tests/t\"q.rs") || broken "quoted-name: update-index"
  case_of quoted-name FAIL 'run-record name=quoted'

  for file in crates/halvo-agent-store/src/end.rs crates/halvo-runner/src/run.rs crates/halvo-agent-store/tests/end.rs; do
    case_name=names-the-keys-and-builds-no-record-$(printf '%s' "$file" | tr '/.' '--')
    repo "$case_name"
    put_from_head "$file"
    case_of "$case_name" PASS 'places=0 known=0 faults=0'
  done

  repo the-pinned-list
  put crates/a/tests/t.rs 'fn f() {}'
  mkdir -p "$work/the-pinned-list.out" || broken "the-pinned-list: mkdir"
  out=$(CI_D_REPO=$work/the-pinned-list CI_D_ARTIFACTS=$work/the-pinned-list.out sh "$self" 2>&1)
  verdict the-pinned-list FAIL 'run-record gone=crates/halvo-learn-gate/src/stub.rs known=1'

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case $#:${1:-} in
  0:) main ;;
  1:--self-test) self_test ;;
  1:--help) help ;;
  *) fail usage ;;
esac
