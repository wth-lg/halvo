# Sheet trial (U0.4)

This trial decides which Rust library unit U2.3 uses to change cells in a
workbook that the user already has, and which kinds of workbook part the
executor must refuse (contracts.md A13.4, A11.3). The rule was fixed before
the run, and the verdict line at the end of the block follows from the
`sheet` lines by that rule.

## What ran

- Machine: `mac`, on 2026-11-10, with `sh tools/r3-trials/sheet/run.sh`
  and no argument, so both candidates ran over the eight workbooks. The run
  records no time and no peak.
- Candidates, in this order: `umya` (umya-spreadsheet 3.1.1 reads the
  workbook, changes the cell and writes it) and `rewrite` (calamine 0.38.0
  reads the workbook, and rust_xlsxwriter 0.102.0 writes a new one). Both are
  read back with calamine 0.38.0 and as zip archives.
- The trial is the crate in `tools/r3-trials/sheet/`. It has its own
  `[workspace]` table and its own `Cargo.lock`, and no product build reads it.
  `cargo test` in that directory runs the reader's failing cases and the
  checks of the corpus.

## The corpus

`r3-sheet-trial generate --corpus DIR` writes the eight workbooks with
rust_xlsxwriter 0.102.0. The committed files in
`tools/r3-trials/sheet/corpus/` are byte for byte the generator's output: the
creation date is fixed at 2026-01-01, and a test generates the corpus again
and compares the bytes.

| workbook | sheets | contents | part kinds |
|---|---|---|---|
| `values.xlsx` | `Values` | a header and 20 rows of text, whole numbers, decimals and booleans, with an empty cell in every third row of column D | none |
| `formulas.xlsx` | `Ledger` | the header `Id`, `Date`, `Payee`, `Amount` in A1 to D1, the ids `INV-1` to `INV-5` in A2 to A6, B2 to D6 empty, `=SUM(D2:D6)` in D7 | none |
| `formats.xlsx` | `Formats` | a bold header with a fill, amounts in `#,##0.00`, dates in `yyyy-mm-dd`, shares in `0.0%` in italics | `format` |
| `merged.xlsx` | `Merged` | a title merged over A1 to D1, six rows of numbers, `Notes` merged over A10 to B10 | `merged` |
| `validation.xlsx` | `Validation` | a whole-number rule (0 to 24) on B2 to B11 and a list rule (`yes`, `no`) on C2 to C11 | `validation` |
| `chart.xlsx` | `Sales` | the table `SalesTable` over A1 to B7, a column chart of B2 to B7, `=SUM(B2:B7)` in B9 and a comment on A9 | `chart`, `drawing`, `table`, `comment` |
| `two-sheets.xlsx` | `Input`, `Summary` | four values and a rate on `Input`, the defined name `Rate` (`=Input!$B$6`), `=SUM(Input!B2:B5)` in `Summary!B1` and `=B1*Rate` in `Summary!B2` | `defined_name` |
| `rows-5000.xlsx` | `Rows` | a header and 5,000 rows of an id, a name and an amount, `=SUM(C2:C5001)` in F1 | none |

The comment of `chart.xlsx` brings a VML part under `xl/drawings/`, so the
kind `drawing` there counts the chart's drawing and the comment's VML part.

## The changed cells

Each run copies the workbook to `target/r3-trials/sheet/out/<candidate>/`
under the repository root, changes one cell in the copy and saves it.

| workbook | cell | old value | new value | formula cells that read it: old result, new result |
|---|---|---|---|---|
| `values.xlsx` | `Values!B5` | 12 | 77 | none |
| `formulas.xlsx` | `Ledger!D2` | empty | 42 | `Ledger!D7`: 0, 42 |
| `formats.xlsx` | `Formats!B3` | 2000.25 | 2500.5 | none |
| `merged.xlsx` | `Merged!B4` | 210 | 99 | none |
| `validation.xlsx` | `Validation!B3` | 2 | 7 | none |
| `chart.xlsx` | `Sales!B3` | 135 | 640 | `Sales!B9`: 845, 1350 |
| `two-sheets.xlsx` | `Input!B3` | 20 | 25 | `Summary!B1`: 100, 105; `Summary!B2`: 300, 315 |
| `rows-5000.xlsx` | `Rows!C2501` | 500 | 123456 | `Rows!F1`: 2497500, 2620456 |

The generator holds the values that it writes as Rust data and computes each
formula's result from them, before and after the change, with the same sums
and products as the formula. It writes the old result as the formula's
cached value. All values are whole numbers or exact binary fractions, so the
comparison of a cached result with the computed one is exact.

## How the trial reads the saved file

- `other_cells_same`: calamine reads every sheet of both files. The sheet
  names and their order must be equal. Every cell but the changed one must
  hold the same value in both files; a cell that holds a formula in the
  original must hold the same formula text, and its cached result is not
  compared. A cell that is empty in one file and not in the other differs.
  calamine names no formula for the other cells of an array formula, so the
  trial also reads each sheet part and requires the same array formulas
  (sheet, cell and range) in both files.
- `lost`: each kind of the closed list is found by the rule of A13.4: an
  archive entry under `xl/charts/`, `xl/drawings/`, `xl/pivotTables/`,
  `xl/pivotCache/` or `xl/tables/`, the entry `xl/vbaProject.bin`, an entry
  `xl/comments*.xml`, a `dataValidations` element in a sheet part, a
  `mergeCells` element in a sheet part, a `numFmts` element in
  `xl/styles.xml`, a `definedNames` element in `xl/workbook.xml`. Entry names
  are compared without regard to case, also where the linked-entry count
  below looks up the target of a relationship, and a directory entry counts
  for no kind. An element counts only with at least one child (`dataValidation`,
  `mergeCell`, `numFmt`, `definedName`). A kind is lost when the original has
  it and the saved file has not. The trial reads the rule more strictly than
  that sentence of A13.4, and also counts a kind as lost when the saved file
  keeps less of it:
  - fewer archive entries of the kind that the workbook still links to. An
    entry counts only when a relationship reaches it from the package root,
    and only when the part that holds the relationship names its id (as
    `<drawing r:id>`, `<legacyDrawing r:id>`, `<tablePart r:id>` or
    `<c:chart r:id>` do) or the relationship is of a type that no element
    names (comments, styles, theme, shared strings and a few more, listed in
    `IMPLICIT` of `parts.rs`). So a chart whose sheet no longer names its
    drawing is lost, although its entry is still in the archive.
  - a child of the original that the saved file does not hold, compared by
    `sqref`, `type`, `operator`, `formula1` and `formula2` for a validation,
    by `ref` for a merged range, by `formatCode` for a number format, and by
    `name`, `localSheetId` and the reference text for a defined name.
  The facts in the evidence print, for each file, what the presence rule
  alone would give (`lost_by_presence_alone`).
- `cached_results`: calamine reads the cached result of each formula cell in
  the table above. A cell that holds the new result is `fresh`, one with no
  cached result is `absent`, and one with any other number is `stale`.
  A13.4 defines `stale` as the old result; in this run every `stale` cell
  holds exactly the old result (the facts show each `<v>`). With
  two such cells, as in `two-sheets.xlsx`, the line names the worse of the
  two, in the order `fresh`, `absent`, `stale`.
- A cell could not run, and `run.sh` exits 1, when a candidate fails or the
  saved file does not hold the new value in the changed cell.

`tools/r3-trials/sheet/src/parts.rs` holds the rule of each kind as code
that U2.3 can take over.

## The rewrite candidate

calamine reads the values, the formula texts with their cached results, the
merged ranges, the tables and the defined names, and rust_xlsxwriter writes
them into a new workbook. calamine does not read charts, drawings, comments,
validations, number formats or cell styles, so the rewrite cannot carry
them. A cell that calamine reads as a date is written with the format
`yyyy-mm-dd`, which keeps it a date. The rewrite writes each formula's old
cached result.

## Licences

The licence lines name every crate that either candidate would put into a
shipped bundle: the crates that `cargo tree` reaches from umya-spreadsheet,
calamine and rust_xlsxwriter by normal edges for every target, without
proc-macro crates and their own dependencies, which run only in the build.
`python3 -I tools/r3-trials/sheet/licences.py --read 2026-11-10` prints them
from the `license` field of each crate's manifest in the lock file's
versions. For a crate with several licences joined by OR the line names the
first of A0.8's list; a crate with an AND gets one line for each part. The
registry pages of the three named crates were read on 2026-11-10 before they
were added. Every item has `ok=yes`.

## Result

The block holds the licence lines, the 16 `sheet` lines as the run printed
them, and the verdict. umya keeps every other cell in all eight workbooks
and loses no part in `values.xlsx` and `formulas.xlsx`, so it is the
verdict. It loses one thing: in `validation.xlsx` it writes the whole-number
rule on B2 to B11 without its bounds (`formula1` 0 and `formula2` 24), while
it keeps the list rule on C2 to C11 whole. So its `refuses` list holds
`validation` beside `pivot` and `macro`, which no corpus workbook has. The
rewrite also keeps every other cell, and loses `chart`, `drawing` and
`comment` in `chart.xlsx`, `validation` in `validation.xlsx` and `format` in
`formats.xlsx`.

Under the presence rule alone, umya's `validation.xlsx` line and the
rewrite's `formats.xlsx` line would read `lost=-`, and the verdict would be
`verdict sheet=umya refuses=pivot,macro`. The verdict's library is the same
under both readings.

Neither candidate computes formulas: in each workbook with a formula that
reads the changed cell, both leave the old cached result (`stale`). umya
also drops `fullCalcOnLoad="1"` from the `calcPr` element of
`xl/workbook.xml`, which rust_xlsxwriter writes, and writes `calcId="122211"`.

Hand check of plan.md for the owner: open `target/r3-trials/sheet/out/umya/formulas.xlsx`
and `target/r3-trials/sheet/out/umya/chart.xlsx`, under the repository root after a run, in
Numbers.

```
licence item=aes spdx=MIT read=2026-11-10 ok=yes
licence item=aho-corasick spdx=MIT read=2026-11-10 ok=yes
licence item=android_system_properties spdx=MIT read=2026-11-10 ok=yes
licence item=atoi_simd spdx=MIT read=2026-11-10 ok=yes
licence item=base64 spdx=MIT read=2026-11-10 ok=yes
licence item=bit-set spdx=MIT read=2026-11-10 ok=yes
licence item=bit-vec spdx=MIT read=2026-11-10 ok=yes
licence item=bitflags spdx=MIT read=2026-11-10 ok=yes
licence item=block-buffer spdx=MIT read=2026-11-10 ok=yes
licence item=block-padding spdx=MIT read=2026-11-10 ok=yes
licence item=bumpalo spdx=MIT read=2026-11-10 ok=yes
licence item=bytemuck spdx=MIT read=2026-11-10 ok=yes
licence item=byteorder spdx=MIT read=2026-11-10 ok=yes
licence item=calamine spdx=MIT read=2026-11-10 ok=yes
licence item=cbc spdx=MIT read=2026-11-10 ok=yes
licence item=cfb spdx=MIT read=2026-11-10 ok=yes
licence item=cfg-if spdx=MIT read=2026-11-10 ok=yes
licence item=chacha20 spdx=MIT read=2026-11-10 ok=yes
licence item=chrono spdx=MIT read=2026-11-10 ok=yes
licence item=cipher spdx=MIT read=2026-11-10 ok=yes
licence item=cmov spdx=MIT read=2026-11-10 ok=yes
licence item=codepage spdx=MIT read=2026-11-10 ok=yes
licence item=const-oid spdx=MIT read=2026-11-10 ok=yes
licence item=constant_time_eq spdx=Apache-2.0 read=2026-11-10 ok=yes
licence item=core-foundation-sys spdx=MIT read=2026-11-10 ok=yes
licence item=core_detect spdx=MIT read=2026-11-10 ok=yes
licence item=cpubits spdx=MIT read=2026-11-10 ok=yes
licence item=cpufeatures spdx=MIT read=2026-11-10 ok=yes
licence item=crc32fast spdx=MIT read=2026-11-10 ok=yes
licence item=crypto-common spdx=MIT read=2026-11-10 ok=yes
licence item=ctutils spdx=MIT read=2026-11-10 ok=yes
licence item=debug_unsafe spdx=MIT read=2026-11-10 ok=yes
licence item=digest spdx=MIT read=2026-11-10 ok=yes
licence item=encoding_rs spdx=BSD-3-Clause read=2026-11-10 ok=yes
licence item=encoding_rs spdx=MIT read=2026-11-10 ok=yes
licence item=equivalent spdx=MIT read=2026-11-10 ok=yes
licence item=fancy-regex spdx=MIT read=2026-11-10 ok=yes
licence item=fast-float2 spdx=MIT read=2026-11-10 ok=yes
licence item=flate2 spdx=MIT read=2026-11-10 ok=yes
licence item=fnv spdx=MIT read=2026-11-10 ok=yes
licence item=futures-core spdx=MIT read=2026-11-10 ok=yes
licence item=futures-task spdx=MIT read=2026-11-10 ok=yes
licence item=futures-util spdx=MIT read=2026-11-10 ok=yes
licence item=getrandom spdx=MIT read=2026-11-10 ok=yes
licence item=hashbrown spdx=MIT read=2026-11-10 ok=yes
licence item=hmac spdx=MIT read=2026-11-10 ok=yes
licence item=html5gum spdx=MIT read=2026-11-10 ok=yes
licence item=hybrid-array spdx=MIT read=2026-11-10 ok=yes
licence item=iana-time-zone spdx=MIT read=2026-11-10 ok=yes
licence item=iana-time-zone-haiku spdx=MIT read=2026-11-10 ok=yes
licence item=imagesize spdx=MIT read=2026-11-10 ok=yes
licence item=indexmap spdx=MIT read=2026-11-10 ok=yes
licence item=inout spdx=MIT read=2026-11-10 ok=yes
licence item=jetscii spdx=MIT read=2026-11-10 ok=yes
licence item=jiff spdx=MIT read=2026-11-10 ok=yes
licence item=jiff-core spdx=MIT read=2026-11-10 ok=yes
licence item=js-sys spdx=MIT read=2026-11-10 ok=yes
licence item=libc spdx=MIT read=2026-11-10 ok=yes
licence item=log spdx=MIT read=2026-11-10 ok=yes
licence item=md-5 spdx=MIT read=2026-11-10 ok=yes
licence item=memchr spdx=MIT read=2026-11-10 ok=yes
licence item=nix spdx=MIT read=2026-11-10 ok=yes
licence item=num-traits spdx=MIT read=2026-11-10 ok=yes
licence item=phf spdx=MIT read=2026-11-10 ok=yes
licence item=phf_shared spdx=MIT read=2026-11-10 ok=yes
licence item=pin-project-lite spdx=MIT read=2026-11-10 ok=yes
licence item=portable-atomic spdx=MIT read=2026-11-10 ok=yes
licence item=portable-atomic-util spdx=MIT read=2026-11-10 ok=yes
licence item=quick-xml spdx=MIT read=2026-11-10 ok=yes
licence item=r-efi spdx=MIT read=2026-11-10 ok=yes
licence item=rand spdx=MIT read=2026-11-10 ok=yes
licence item=rand_core spdx=MIT read=2026-11-10 ok=yes
licence item=regex-automata spdx=MIT read=2026-11-10 ok=yes
licence item=regex-syntax spdx=MIT read=2026-11-10 ok=yes
licence item=rgb spdx=MIT read=2026-11-10 ok=yes
licence item=rust_xlsxwriter spdx=MIT read=2026-11-10 ok=yes
licence item=scopeguard spdx=MIT read=2026-11-10 ok=yes
licence item=serde spdx=MIT read=2026-11-10 ok=yes
licence item=serde_core spdx=MIT read=2026-11-10 ok=yes
licence item=sha2 spdx=MIT read=2026-11-10 ok=yes
licence item=simd-adler32 spdx=MIT read=2026-11-10 ok=yes
licence item=simdutf8 spdx=MIT read=2026-11-10 ok=yes
licence item=siphasher spdx=MIT read=2026-11-10 ok=yes
licence item=slab spdx=MIT read=2026-11-10 ok=yes
licence item=thousands spdx=MIT read=2026-11-10 ok=yes
licence item=typed-path spdx=MIT read=2026-11-10 ok=yes
licence item=typenum spdx=MIT read=2026-11-10 ok=yes
licence item=umya-spreadsheet spdx=MIT read=2026-11-10 ok=yes
licence item=unicode-ident spdx=MIT read=2026-11-10 ok=yes
licence item=unicode-ident spdx=Unicode-3.0 read=2026-11-10 ok=yes
licence item=uuid spdx=MIT read=2026-11-10 ok=yes
licence item=wasm-bindgen spdx=MIT read=2026-11-10 ok=yes
licence item=wasm-bindgen-shared spdx=MIT read=2026-11-10 ok=yes
licence item=web-time spdx=MIT read=2026-11-10 ok=yes
licence item=windows-core spdx=MIT read=2026-11-10 ok=yes
licence item=windows-link spdx=MIT read=2026-11-10 ok=yes
licence item=windows-result spdx=MIT read=2026-11-10 ok=yes
licence item=windows-strings spdx=MIT read=2026-11-10 ok=yes
licence item=windows-sys spdx=MIT read=2026-11-10 ok=yes
licence item=zeroize spdx=MIT read=2026-11-10 ok=yes
licence item=zip spdx=MIT read=2026-11-10 ok=yes
licence item=zlib-rs spdx=Zlib read=2026-11-10 ok=yes
licence item=zopfli spdx=Apache-2.0 read=2026-11-10 ok=yes
sheet candidate=umya file=values.xlsx other_cells_same=yes lost=- cached_results=none
sheet candidate=umya file=formulas.xlsx other_cells_same=yes lost=- cached_results=stale
sheet candidate=umya file=formats.xlsx other_cells_same=yes lost=- cached_results=none
sheet candidate=umya file=merged.xlsx other_cells_same=yes lost=- cached_results=none
sheet candidate=umya file=validation.xlsx other_cells_same=yes lost=validation cached_results=none
sheet candidate=umya file=chart.xlsx other_cells_same=yes lost=- cached_results=stale
sheet candidate=umya file=two-sheets.xlsx other_cells_same=yes lost=- cached_results=stale
sheet candidate=umya file=rows-5000.xlsx other_cells_same=yes lost=- cached_results=stale
sheet candidate=rewrite file=values.xlsx other_cells_same=yes lost=- cached_results=none
sheet candidate=rewrite file=formulas.xlsx other_cells_same=yes lost=- cached_results=stale
sheet candidate=rewrite file=formats.xlsx other_cells_same=yes lost=format cached_results=none
sheet candidate=rewrite file=merged.xlsx other_cells_same=yes lost=- cached_results=none
sheet candidate=rewrite file=validation.xlsx other_cells_same=yes lost=validation cached_results=none
sheet candidate=rewrite file=chart.xlsx other_cells_same=yes lost=chart,drawing,comment cached_results=stale
sheet candidate=rewrite file=two-sheets.xlsx other_cells_same=yes lost=- cached_results=stale
sheet candidate=rewrite file=rows-5000.xlsx other_cells_same=yes lost=- cached_results=stale
verdict sheet=umya refuses=pivot,macro,validation
```
