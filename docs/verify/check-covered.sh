#!/bin/sh
set -u
name=check-covered
doc_path=docs/verify/store-shred.md
tests_path=crates/halvo-capture/src/store/formal_tests.rs
tests_module=store::formal_tests

usage() {
  cat <<'EOF'
Audit of the covered table in docs/verify/store-shred.md. It reads the doc, every
mutants.rows file and crates/halvo-capture/src/store/formal_tests.rs. It builds nothing.

  sh docs/verify/check-covered.sh [--root DIR] [--as NAME]
      Fail unless every line of the table under "## Covered code" names a row that
      - starts with formal- and stands once in a mutants.rows file,
      - has the state required,
      - mutates the file of its line,
      - has the verdict "test store::formal_tests::<test>", where <test> is a function
        of formal_tests.rs and the table under "## The checks" lists it for the check
        of the line.
      It also fails when no row starts with formal-, when the doc names a formal- row
      that no row file holds, and when a row file holds a formal- row that the doc
      does not name. The doc names a row in backticks: `formal-<name>`. Each problem
      is one line, and a problem of the doc names its line.
      --root DIR   audit the tree DIR (default: the repository of this script)
      --as NAME    print NAME in the last line (the ci.d script passes its own name)
  sh docs/verify/check-covered.sh --self-test [-v] [--as NAME]
      Prove the audit on fixtures that it carries.

The last line is "PASS <name>" or "FAIL <name> (<cause>)". Exit status: 0 on PASS,
1 on FAIL, 2 when the audit could not run, 64 on bad usage.
EOF
}

audit() {
  root=$1
  [ -f "$root/$doc_path" ] || {
    printf 'FAIL %s (%s is missing)\n' "$name" "$doc_path"
    return 1
  }
  [ -f "$root/$tests_path" ] || {
    printf 'FAIL %s (%s is missing)\n' "$name" "$tests_path"
    return 1
  }
  scratch=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || return 2
  find "$root" -name mutants.rows -type f ! -path '*/target/*' >"$scratch/found" || return 2
  LC_ALL=C sort "$scratch/found" >"$scratch/files" || return 2
  : >"$scratch/rows"
  while IFS= read -r rows_file; do
    rows_name=${rows_file#"$root"/}
    rows_dir=${rows_name%/*}
    [ "$rows_dir" != "$rows_name" ] || rows_dir=.
    tr -d '\r' <"$rows_file" |
      awk -F '\t' -v dir="$rows_dir" -v from="$rows_name" 'NF == 6 { print $1 "\t" dir "/" $2 "\t" $4 "\t" $5 "\t" from }' >>"$scratch/rows" || return 2
  done <"$scratch/files"
  tr -d '\r' <"$root/$tests_path" | sed -n 's/^[[:space:]]*fn \([a-z0-9_]*\)(.*/\1/p' >"$scratch/tests" || return 2
  tr -d '\r' <"$root/$doc_path" >"$scratch/doc" || return 2
  awk -F '|' -v rows="$scratch/rows" -v tests="$scratch/tests" -v doc="$doc_path" -v module="$tests_module" '
    function problem(text) {
      print "problem: " text
      problems++
    }
    function cell(text) {
      gsub(/^[ \t`]+|[ \t`]+$/, "", text)
      return text
    }
    BEGIN {
      while ((getline line < rows) > 0) {
        split(line, part, "\t")
        held[part[1]]++
        row_file[part[1]] = part[2]
        row_verdict[part[1]] = part[3]
        row_state[part[1]] = part[4]
        row_from[part[1]] = part[5]
        if (index(part[1], "formal-") == 1) formal_rows++
      }
      while ((getline line < tests) > 0) is_test[line] = 1
    }
    /^#/ {
      section = $0
      sub(/^#+[ \t]*/, "", section)
      table_line = 0
    }
    {
      text = $0
      while (match(text, /`formal-[a-z0-9-]+`/)) {
        named = substr(text, RSTART + 1, RLENGTH - 2)
        text = substr(text, RSTART + RLENGTH)
        in_doc[named] = 1
        if (!(named in held) && !(named in told)) {
          told[named] = 1
          problem("line " NR " of " doc " names the row " named ", which no mutants.rows file holds")
        }
      }
    }
    !/^\|/ { table_line = 0; next }
    { table_line++ }
    table_line < 3 { next }
    section == "The checks" {
      test_of[cell($2), cell($3)] = 1
      next
    }
    section != "Covered code" { next }
    {
      covered_lines++
      if (NF != 6) {
        problem("line " NR " of " doc " is a line of the covered table without its four cells: check, file, functions, rows")
        next
      }
      check = cell($2)
      file = cell($3)
      text = $5
      rows_named = 0
      while (match(text, /`formal-[a-z0-9-]+`/)) {
        row = substr(text, RSTART + 1, RLENGTH - 2)
        text = substr(text, RSTART + RLENGTH)
        rows_named++
        if (!(row in held)) continue
        where = "line " NR " of " doc " names the row " row ", "
        if (held[row] > 1) problem(where "which stands in more than one mutants.rows file")
        if (row_state[row] != "required") problem(where "whose state is " row_state[row] ", not required")
        if (row_file[row] != file) problem(where "which mutates " row_file[row] ", not the file of this line")
        verdict = row_verdict[row]
        if (index(verdict, "test " module "::") != 1) {
          problem(where "whose verdict is " verdict ", not a test of " module)
          continue
        }
        test = substr(verdict, length("test " module "::") + 1)
        if (!(test in is_test)) problem(where "whose verdict test " test " is not a function of formal_tests.rs")
        if (!((check, test) in test_of)) problem(where "whose verdict test " test " is not a test of check " check " in the table under The checks")
      }
      if (rows_named == 0) problem("line " NR " of " doc ", the covered-table line of " file ", names no formal- row: name the row that fails through it, or move the line to Not covered with the reason")
    }
    END {
      if (formal_rows == 0) problem("no mutants.rows file holds a row whose name starts with formal-")
      if (covered_lines == 0) problem("the table under Covered code in " doc " holds no line")
      for (row in held) {
        if (index(row, "formal-") == 1 && !(row in in_doc)) problem("the row " row " stands in " row_from[row] ", and " doc " does not name it")
      }
      printf "covered lines %d, formal rows %d, problems %d\n", covered_lines, formal_rows, problems
      exit (problems > 0)
    }
  ' "$scratch/doc"
  status=$?
  case $status in
    0)
      printf 'PASS %s\n' "$name"
      return 0
      ;;
    1)
      printf 'FAIL %s (the covered table and the mutant rows disagree; fix each line that starts with problem)\n' "$name"
      return 1
      ;;
    *) return 2 ;;
  esac
}

fixture() {
  tree=$work/$1
  mkdir -p "$tree/docs/verify" "$tree/crates/halvo-capture/src/store" "$tree/crates/halvo-capture/src/custody" || exit 2
  printf '%s\n' "$good_doc" >"$tree/$doc_path" || exit 2
  printf '%s\n' "$good_tests" >"$tree/$tests_path" || exit 2
  : >"$tree/crates/halvo-capture/src/store/mutants.rows"
  row "$1" store formal-keys expiry.rs "test $tests_module::shred_check" required
  row "$1" store formal-lock db.rs "test $tests_module::race_check" required
  row "$1" store key-row-deleted expiry.rs 'test store::hardening_tests::raw_pages' required
}

row() {
  printf '%s\t%s\ts/a/b/\t%s\t%s\tX-1\n' "$3" "$4" "$5" "$6" >>"$work/$1/crates/halvo-capture/src/$2/mutants.rows" || exit 2
}

edit_doc() {
  sed "$2" "$work/$1/$doc_path" >"$work/$1/doc.edited" || exit 2
  cp "$work/$1/doc.edited" "$work/$1/$doc_path" || exit 2
}

check() {
  out=$(sh "$0" --as "$name" --root "$work/$1" 2>&1)
  code=$?
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  case $out in
    *"${3:-}"*) ;;
    *) got=$got-without-the-text ;;
  esac
  [ -z "$verbose" ] || printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$got"
  if [ "$got" != "$2" ]; then
    wrong="$wrong $1"
  elif [ "$2" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

# shellcheck disable=SC2016
good_doc='# A fixture of the verify doc

## The checks

| Check | Test of `store::formal_tests` |
|---|---|
| 1 | `shred_check` |
| 2 | `race_check` |

## Covered code

| Check | File | Functions | Rows that fail through it |
|---|---|---|---|
| 1 | `crates/halvo-capture/src/store/expiry.rs` | `destroy_segment_keys` | `formal-keys` |
| 2 | `crates/halvo-capture/src/store/db.rs` | `Db::open` | `formal-lock` |

## Not covered

- The segment format, which `scripts/30-formal-other.sh` checks.'

good_tests='fn helper() {}
proptest! {
    #[test]
    fn shred_check(ops in ops()) {}
}
#[test]
fn race_check() {}'

# shellcheck disable=SC2016
self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  rejected=0
  wrong=

  fixture holds
  check holds PASS 'covered lines 2, formal rows 2, problems 0'

  fixture holds-with-crlf
  awk '{ printf "%s\r\n", $0 }' "$work/holds-with-crlf/$doc_path" >"$work/holds-with-crlf/doc.crlf" || exit 2
  cp "$work/holds-with-crlf/doc.crlf" "$work/holds-with-crlf/$doc_path" || exit 2
  check holds-with-crlf PASS 'covered lines 2, formal rows 2, problems 0'

  fixture row-beside-custody-code
  row row-beside-custody-code custody formal-wrap dev_fallback.rs "test $tests_module::shred_check" required
  edit_doc row-beside-custody-code 's/^| 2 .*/&\
| 1 | `crates\/halvo-capture\/src\/custody\/dev_fallback.rs` | `aead_seal` | `formal-wrap` |/'
  check row-beside-custody-code PASS 'covered lines 3, formal rows 3, problems 0'

  fixture line-names-no-row
  edit_doc line-names-no-row 's/ `formal-keys` |$/ none |/'
  check line-names-no-row FAIL "line 14 of $doc_path, the covered-table line of crates/halvo-capture/src/store/expiry.rs, names no formal- row"

  fixture row-missing
  edit_doc row-missing 's/`formal-lock`/`formal-gone`/'
  check row-missing FAIL "line 15 of $doc_path names the row formal-gone, which no mutants.rows file holds"

  fixture row-pending
  row row-pending store formal-later db.rs "test $tests_module::race_check" 'pending store-3'
  edit_doc row-pending 's/`formal-lock`/`formal-lock`, `formal-later`/'
  check row-pending FAIL 'names the row formal-later, whose state is pending store-3, not required'

  fixture verdict-is-a-gate-line
  row verdict-is-a-gate-line store formal-gate db.rs 'measured decryptable' required
  edit_doc verdict-is-a-gate-line 's/`formal-lock`/`formal-lock`, `formal-gate`/'
  check verdict-is-a-gate-line FAIL "names the row formal-gate, whose verdict is measured decryptable, not a test of $tests_module"

  fixture verdict-is-another-test
  row verdict-is-another-test store formal-other db.rs 'test store::hardening_tests::raw_pages' required
  edit_doc verdict-is-another-test 's/`formal-lock`/`formal-lock`, `formal-other`/'
  check verdict-is-another-test FAIL "whose verdict is test store::hardening_tests::raw_pages, not a test of $tests_module"

  fixture verdict-of-the-other-check
  row verdict-of-the-other-check store formal-crossed expiry.rs "test $tests_module::race_check" required
  edit_doc verdict-of-the-other-check 's/`formal-keys`/`formal-keys`, `formal-crossed`/'
  check verdict-of-the-other-check FAIL 'names the row formal-crossed, whose verdict test race_check is not a test of check 1'

  fixture test-not-in-the-file
  row test-not-in-the-file store formal-renamed db.rs "test $tests_module::renamed_check" required
  edit_doc test-not-in-the-file 's/`formal-lock`/`formal-lock`, `formal-renamed`/'
  check test-not-in-the-file FAIL 'whose verdict test renamed_check is not a function of formal_tests.rs'

  fixture row-on-another-file
  row row-on-another-file store formal-elsewhere segment.rs "test $tests_module::race_check" required
  edit_doc row-on-another-file 's/`formal-lock`/`formal-lock`, `formal-elsewhere`/'
  check row-on-another-file FAIL 'names the row formal-elsewhere, which mutates crates/halvo-capture/src/store/segment.rs, not the file of this line'

  fixture row-not-in-the-doc
  row row-not-in-the-doc store formal-unnamed db.rs "test $tests_module::race_check" required
  check row-not-in-the-doc FAIL "the row formal-unnamed stands in crates/halvo-capture/src/store/mutants.rows, and $doc_path does not name it"

  fixture name-twice
  row name-twice custody formal-lock db.rs "test $tests_module::race_check" required
  check name-twice FAIL 'names the row formal-lock, which stands in more than one mutants.rows file'

  fixture no-formal-row
  grep -v '^formal-' "$work/no-formal-row/crates/halvo-capture/src/store/mutants.rows" >"$work/no-formal-row/rows.kept"
  cp "$work/no-formal-row/rows.kept" "$work/no-formal-row/crates/halvo-capture/src/store/mutants.rows" || exit 2
  check no-formal-row FAIL 'no mutants.rows file holds a row whose name starts with formal-'

  fixture no-covered-table
  edit_doc no-covered-table '/^## Covered code$/,/^## Not covered$/d'
  check no-covered-table FAIL "the table under Covered code in $doc_path holds no line"

  fixture line-with-three-cells
  edit_doc line-with-three-cells 's/ `Db::open` |//'
  check line-with-three-cells FAIL "line 15 of $doc_path is a line of the covered table without its four cells"

  fixture doc-missing
  mv "$work/doc-missing/$doc_path" "$work/doc-missing/doc.moved" || exit 2
  check doc-missing FAIL "$doc_path is missing"

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

mode=audit
verbose=
root=
while [ $# -gt 0 ]; do
  case $1 in
    --self-test) mode=self-test ;;
    -v) verbose=yes ;;
    --root)
      shift
      root=${1:-}
      [ -n "$root" ] || {
        usage >&2
        exit 64
      }
      ;;
    --as)
      shift
      name=${1:-}
      [ -n "$name" ] || {
        usage >&2
        exit 64
      }
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      printf '%s: unknown argument: %s\n' "$name" "$1" >&2
      usage >&2
      exit 64
      ;;
  esac
  shift
done

if [ "$mode" = self-test ]; then
  self_test
  exit 0
fi
if [ -z "$root" ]; then
  root=$(cd "$(dirname "$0")/../.." && pwd) || exit 2
fi
audit "$root"
