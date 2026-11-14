# halvo's build of the database zeroes the gap of a page that it builds again or edits in place

decision: 2026-11-13T20:53:00Z
status: accepted
decided by: the coordinator (eng-manager) of runs 20261109-halvo-r2-workflows and 20261108-halvo-r1-capture, as the default that the team builds to; the owner may turn it through the entry in owner-asks.md of run 20261108-halvo-r1-capture
evidence: rulings S-82 and S-83 of run 20261109-halvo-r2-workflows (rulings-on-u52-gates.md) and rulings ST-40 and ST-42 of run 20261108-halvo-r1-capture (rulings-on-store2.md); Addition 2 of reports/verifier-37.md of run 20261109-halvo-r2-workflows; unit sqlcipher-1 of run 20261108-halvo-r1-capture and its report builder-67.md

## Context

Both stores open the one SQLCipher build of crates/halvo-sqlcipher with secure
delete on, and promise that what they remove leaves no text in their files. In
the pinned source (sqlcipher-src 419.0.6, SQLCipher 4.19.0 on SQLite 3.53.4),
secure delete zeroes a freed cell, an emptied page, a freed page and the gap of
a page that the database defragments. Two places leave text of a row in the
gap of a page that is in use, where a later removal of that row zeroes its
cell and not the copy:

- rebuildPage (lines 81205 to 81272 of sqlcipher.c), when a balance builds a
  page again: the earlier places of the cells that the page keeps, and cells
  that a failed edit in place had already copied there. The workflow run's
  verifier found whole text of removed rows there in derived.db in 15 of 16
  drawn stores.
- editPage, line 81461, when cells leave the start of a page that is edited in
  place: the pointer array is moved with the count from before the shift, so
  its last bytes take the first bytes of the cell at the content start. Found
  in the review of this unit, in test tables of this crate.

Version 420.0.0 of the crate holds rebuildPage byte for byte as 419.0.6 does;
editPage of that version was not read.

## Decision

Halvo's build of SQLCipher differs from the pinned source in two statements of
two functions, rebuildPage and editPage, and in one added function.

- crates/halvo-sqlcipher/build.rs reads the pinned sqlcipher.c, writes a copy
  under the build's own output directory and compiles that copy. No file of
  the cargo registry is written, and no copy of the source is in the
  repository.
- At the end of the comment line 81262 of rebuildPage, under secure delete,
  the bytes between the new pointer array and the new content are set to
  zero.
- At the end of line 81517 of editPage (`pPg->nCell = (u16)nNew;`), on an edit
  that succeeded in place, under secure delete, the bytes between the end of
  the new pointer array and the content start are set to zero.
- Neither statement adds a line, so every line number of the compiled file,
  among them those that the database logs for a corrupt page, is that of the
  pinned file.
- After the last line of the pinned file, each on lines of its own, the copy
  holds eight compile-time stops, and then it ends with the function
  halvo_sqlcipher_page_gap_mend. The compiler refuses the file by name when
  BTS_FAST_SECURE is not defined (the database's code is not compiled in
  this file, as under an outside -DSQLITE_AMALGAMATION),
  SQLITE_ENABLE_STAT4 is defined, SQLITE_TEMP_STORE is not 2,
  SQLITE_DEFAULT_AUTOVACUUM is not 0, SQLITE_HAS_CODEC is not defined,
  SQLITE_DEFAULT_SYNCHRONOUS or SQLITE_DEFAULT_WAL_SYNCHRONOUS is not 2,
  SQLITE_DEBUG is defined, or SQLITE_DEFAULT_PAGE_SIZE is not 4096, whether
  build.rs or the environment sets it (CFLAGS, CFLAGS_<target>, TARGET_CFLAGS
  or a define of build.rs; the #error lines name the macro, not its source). The stop on BTS_FAST_SECURE
  stands first: MSVC's compiler ends at the first #error, and with the
  database's code compiled out the macros of the later stops are undefined
  too (the builder's hosted run 43582177071 at c57666d5 named
  SQLITE_DEFAULT_AUTOVACUUM on Windows when that stop stood fourth). A flag
  that redefines an identifier of the source is not read. The test
  `page_gap::pins::the_compile_options_of_this_build_are_its_closed_list`
  reads PRAGMA compile_options, less the compiler's line, against a closed
  list for each system. The function halvo_sqlcipher_page_gap_mend answers 2
  and touches no structure of the database. `halvo_sqlcipher::page_gap_mend_linked`
  is true only for that answer, so a program that calls it links that object
  or does not link at all, and a library of the earlier build with one
  statement, which answers 1, reads false.
  The open of each store is to require it, in the next unit that changes that
  open.
- The build has no SQLITE_ENABLE_STAT4, and the compiler stops if a flag
  defines it: with it, ANALYZE and PRAGMA optimize copy whole index keys into
  sqlite_stat4, where they outlive the rows. No store runs either, by a search
  of the stores' sources today; no check holds it.
- No cargo feature and no switch of the environment or of the build takes a
  statement out, within the limit above: a flag that redefines an identifier
  of the source is not read. The sha256 reads sqlcipher.c alone: the headers
  that the same file includes, the other sources of sqlcipher-src, a forced
  include and the compiler's own headers are not hashed. tomcrypt.h is
  included at line 115040, after the whole of btree.c, so a patched header
  cannot change the page code's text; a blanket redefinition is the limit
  above. Cargo.lock pins the crate's archive, not an edited registry copy.

## How it is held

Reading cannot show that no third place exists, so the claim is "no leftover
in the draws of the tests", with the limits below.

- The tests page_gap of the crate draw a table and a table WITHOUT ROWID, each
  with an index, bodies of 8 to 40 bytes, of 20 to 950 bytes and larger than a
  page, deletes one at a time with a truncating checkpoint after each,
  replacements in place, mass deletes that take the index from depth 3 back
  to depth 1, and a plain table of small rows (seeds 1 and 8). After each step
  they search every byte of every file for the text of removed rows, count
  each stored row's places and read every row back against a model. Tables of
  the FTS5 form with their own secure-delete option are drawn apart: after
  merges that end, they leave no removed token, searched whole and by its
  tail, in any page that dbstat does not name the table's _idx tree.
- FTS5's own code keeps the token of a removed row, and neither statement
  makes or mends it: as a key of a live row of the table's _idx tree (the
  head of the token, whole when its neighbour differs in the last byte alone)
  until that leaf loses its last term, its segment is merged away, or a
  'rebuild' runs; and as whole tokens in the _data tree when a merge stops
  partway. The test
  `page_gap::fts5::the_fts5_idx_key_of_a_removed_row_stays_until_a_rebuild`
  pins the first. Only the capture store has such a table (events_fts); its
  unit store-4 removes it (rulings A2 and E11 of that run's contract).
- The structural check reads every page of the file by the database's own
  layout after each round and requires zero in every byte up to the usable
  size that holds no cell, no header and no pointer, a cell of fewer than 4
  bytes included (the reserved tail of a page is the codec's and is not
  read), bytes 52 to 55 and 72 to 91 of the file header, that every page that
  dbstat names overflow is walked by exactly one chain of the cells that it
  reads, that the pages it read as a b-tree page, an overflow page, a freelist
  trunk or a freelist leaf add up to the page count, and that the log of the
  connection's file holds no frame. Its rules stand in
  tests/page_gap/rules.rs, which uses rusqlite alone so that another crate's
  tests can include it; its `plants` function plants a byte in the places
  that its doc lists, in a copy of a caller's own image, and the controls of
  the other rules stand in this crate's tests, because they need pages that a
  caller's image may lack. Its allowances are
  its limit: a run of stale 2-byte offsets right after the pointer array whose
  values are at least the slot's own offset plus 2 and at most the usable size
  less 4, and 4-byte values no larger than the page count in the tail of an
  overflow page and in a freelist trunk past its count. A leftover made only
  of such values is not seen. Because a larger page lets more text pass as
  offsets, the check stops by name on a page size above 4,096 bytes, which
  both stores use. At a usable size of 4,096 a pair passes when its first
  byte is below 0x10, that is a tab, a newline or a carriage return followed
  by any byte, or a zero byte followed by a non-zero byte (text in a 2-byte
  encoding of Latin letters, small numbers); under a key the bound is the
  smaller usable size. It is a limit of the check and not of the build: both
  statements zero the gap after every edit and rebuild, the byte search sees
  whole markers wherever they stand, and in the runs of the stress step's
  fourth adversary (interrogate-sqlcipher1-adversary-4 of the capture run,
  part 2) a count over the images of its draw r03 read 508,159 stale slots
  with no printable pair, and two draws never formed such a leftover even
  without the rebuildPage statement; no test holds those counts. The bound of the slot's
  offset plus 2 and the usable size less 4 was tried on every drawn file and
  kept because each passes it; the test
  `page_gap::layout::a_stale_offset_passes_only_between_its_slot_plus_2_and_the_usable_size_less_4`
  plants the values on both sides of each end (ruling S-86 E6).
- The keyed draws get no byte search and no structural check: they read the
  rows back against the model and run the integrity check alone. The stores'
  own checks read the keyed layout (rulings ST-42 (2) and S-83 7 (b)).
- The look after a close is the byte search alone: the structural check runs
  only while the store is open, and the draws close a store after a
  truncating checkpoint, which leaves nothing for the close to write.
- A copy of a table by `INSERT INTO b SELECT * FROM a` takes the transfer path
  (sqlite3BtreeTransferRow, lines 83309 and 83341, which write the payload
  into the new cell and its overflow pages). VACUUM attaches an empty file
  name (line 167703), which in this build is a temporary database held in
  memory (SQLITE_TEMP_STORE is 2, and the stores set temp_store to MEMORY),
  with the main database's secure delete (lines 131727 and 131728); it copies
  each table into it by `INSERT ... SELECT` (line 167806), so line 83341 writes
  into that database in memory, and then writes its pages into the file by
  sqlite3BtreeCopyFile (line 167871). The database in memory is freed when
  VACUUM ends and leaves no file of its own; the file then holds the pages
  that it built. The tests page_gap::rewrites draw both, each with a control
  with secure delete off, and search every file after each and after each
  single removal of the copied or kept rows.
- A database of this build has no auto-vacuum: the test
  `page_gap::pins::a_new_database_of_this_build_reads_auto_vacuum_0_after_its_first_table`
  requires `PRAGMA auto_vacuum` to read 0 on a new file after its first table,
  and the structural check's rule on bytes 52 to 55 of the file header (the
  auto-vacuum field) is the same pin on every drawn file. The sixteen rows of
  auto-vacuum code in the table (74665 among them) run in no draw under that
  pin, and the row of line 77342 in none because a file that this build made
  does not reach it; with the five rows that write into memory alone, 22 rows
  read 0 in both columns.
- Its controls fail by name: each statement taken out, secure delete off, and
  4 bytes of text planted in a gap of a copy of the file. The mutant rows
  rebuilt-page-gap-not-zeroed and edited-page-gap-not-zeroed take each
  statement out. A narrowing of the editPage statement to the bytes of line
  81461 is caught by the structural check alone: the byte search does not see
  it in the draws. For each line that the structural check or a search can
  print, a test of page_gap makes the fault in a copy or in a file and
  requires that line, and a test fails when a line of rules.rs has no control
  (`page_gap::lines::every_line_that_rules_rs_can_print_has_a_control_in_the_closed_list`,
  a closed list as the table below has). The parts of rules that have no
  control: page 1 read as a b-tree page (every plant is on a later page), the
  last 1 to 3 bytes of a range of words that is no multiple of 4, a cell whose
  payload or rowid takes a varint of 9 bytes, bytes 0 to 3 of the last
  overflow page of a chain and the bytes of the file past its last whole
  page, which no rule reads, and the three walks that stop without a line of
  their own (the trunks past the page count or seen twice, a chain that leaves
  the file before its payload ends, a freeblock chain longer than the page),
  whose pages the line of a page in no tree and the sum of the four kinds
  then name. The tests page_gap::controls plant the four rules of a tree page
  on an interior index page, an interior table page, an index leaf and a
  table leaf with a byte of 0x01 and one of 0x80 (the last byte of a gap
  among them), a low and a high byte in a freelist leaf and in the header,
  the page count and the count plus 1 in a trunk and in a tail, a cell that
  takes the overflow chain of another (a page of 2 chains and one of 0), a
  leaf whose type byte is 0 on disk (dbstat reads it corrupted), a log with
  frames, and a draw that frees a second freelist trunk, with plants in its
  last leaf, past its count and in its first entry; `plants` holds a page
  appended to the file, a b-tree page that a trunk names, a trunk that names
  page 0, a page past the count and itself, and a leaf whose type byte is 0.
  The rule on a log is one function that the three searches call, each with a
  control, and the search of rewrites has a control of its structural check.
  The tests page_gap::holes hold the controls of S-86 E5: a checkpoint that
  does not truncate (the log's own line), 4 bytes of text planted past the
  stale run of a gap with the store closed and with it open, a removed marker
  written after a close and one in a file of another name, a stored marker
  that no file holds, a marker that the test never wrote, a byte planted at
  each end of the places that the check reads (a freelist trunk past its
  count, an overflow tail, the first bytes of a freelist leaf, header byte 55,
  the last byte of a freeblock), and two index records at the bound of the
  local payload that dbstat reads as such (a local part of 1,002 bytes, and a
  record of 1,003 bytes that spills). The secure-delete-off control requires
  that for at least one seed the first line is found after a single delete.
- In every draw of page_gap the rows that the test did not remove read back
  equal to its model, by table and by index, and the integrity check passes,
  keyless and under a key (`Run::model_and_integrity`, before and after each
  close). Every result was held against the pinned source without the
  statements on 64 drawn workloads at the stress step and at the gates; no
  tracked test holds that. The reading against the model has no failing
  control of its own: a mutant row that widened the editPage statement's
  zeroing by 1, 2, 4 or 8 bytes ended the main test and the keyed test at
  their first statement with "database disk image is malformed", before any
  line of the model or of the integrity check, so no such row was kept
  (ruling S-87 G2 K 4). On a page that is malformed before the edit, the
  results can differ: when the content-start field of a leaf lies above live
  cells, the editPage statement zeroes those cells on a delete, where the
  pinned source keeps them until a row of that page grows or is inserted, and
  then loses the same rows by its own code (seen by run); pageInsertArray
  (lines 81324 to 81326) and defragmentPage (line 75289) lose them too (by
  reading). The integrity check refuses such a page by name: a cell below the
  content start at line 84530 (`Offset ... out of range`), such a freeblock at
  line 75689. The test
  `page_gap::malformed::on_a_leaf_whose_content_start_lies_above_live_cells_the_integrity_check_refuses_it_and_deletes_lose_those_cells`
  pins what this build does on such a page, beside what the integrity check
  prints before the first delete. No sequence of this build's writes is known
  to make such a page. A keyed store refuses a page changed on disk without
  the key: the test
  `page_gap::malformed::under_a_key_one_byte_changed_on_disk_in_a_b_tree_page_fails_the_read`
  changes one byte of a b-tree page and requires that the read fails (by run);
  that a torn checkpoint write together with lost log frames is what this
  build's own writes would need is by reading and not run. Not covered by the
  keyed refusal: a page changed in the process's memory before the codec
  writes it. The path where the two builds differ, deletes alone on an old
  page, is how the capture store's expiry treats its pages, so the limit is
  not nothing. The fall-back, recorded and not built: on the same line, the
  upper bound of the zeroing becomes the lower of pData and the lowest offset
  in the new pointer array (one pass over nNew entries); it would need its own
  three readers and new text in both MENDS lists and in the row
  edited-page-gap-not-zeroed (ruling S-86 E2).
- The table below has one row for every line of btree.c in the pinned file
  (between its marker lines "Begin file btree.c" and "End of btree.c", lines
  73561 to 85152, each found exactly once by the test) that holds memcpy,
  memmove, memset, put2byte or put4byte, and a test of the crate fails when
  such a line has no row or a row names no such line. The five words do not
  match a header byte or a varint written in another form (a byte stored
  through an index, putVarint, a 4-byte copy by assignment): those lines have
  no row. Its columns are the builder's reading of each line, not
  a proof: what it writes and where, whether its source or destination can
  pass its object, what it can leave in the file, what zeroes or writes over
  that, and how often the tests' draws ran it, from line counts of the
  compiled copy under clang's coverage with llvm-cov's rounding, made at
  dcd4c79b of the branch, in two
  columns: the secured keyless draws alone (the 24 tests of page_gap that
  run with secure delete on and without a key on a well-formed file and
  search the files or check their pages: the main test, the three of
  second_place, the three of rewrites without their controls, the three
  plants and the two draws of layout, the two of fts5 with FTS5's own
  secure-delete on, and the ten of holes), and all the tests of the crate,
  the draws with secure delete off, the keyed draws and the test of a
  malformed page among them.
  A line that no draw ran is named "0: not tested".
- The table does not cover FTS5's own code, the pager and the log, the
  codec's buffers and the keyed form's bytes.

## Consequences

- A move of the pin owes a new reading of the page code. The build stops, and
  names what it looked for, when the sha256 of sqlcipher.c is not the one that
  was read, or when either changed line is not found exactly once, or its
  function's opening (`static int rebuildPage(`, `static int editPage(`) is not
  found exactly once before it with no function end between, or when a
  preprocessor branch opened after that opening is still open at the line or
  an #else or #elif stands outside such a branch (a directive is a line whose
  first non-blank character is #). The check does not count a branch opened
  before the function's opening (one is open around the whole file, line 54;
  the compile-time stop on BTS_FAST_SECURE covers it), and it does not see an
  anchor inside a comment or a memset renamed by a macro. Read
  rebuildPage, editPage, pageInsertArray, pageFreeArray, balance_quick and
  balance_nonroot from its divider loop again in the new source, with every
  row of the table, move or delete the statements, write the new sha256 into
  build.rs, and run the tests page_gap with the statements and without them.
- The notices of a bundle owe one line for it: the SQLCipher in Halvo is
  SQLCipher 4.19.0 with two statements of Halvo's own, in rebuildPage and in
  editPage, and without SQLITE_ENABLE_STAT4.
- The statements clean no file that a build without them wrote. Such stores
  are removed whole.
- ADR 0006's "the shared SQLCipher build" stands: crates/halvo-sqlcipher is
  still the one build of the database.

## The table of byte writes in btree.c

| line | function | what it writes, from where to where | can a range pass its object | what it can leave in the file | zeroed or written over by | runs in the secured keyless draws | runs in all draws |
|---|---|---|---|---|---|---|---|
| 74297 | saveCursorKey | a buffer of the process | no | nothing in the file (a saved key) | - | 624 | 740 |
| 74563 | sqlite3BtreeCursorHint | a buffer of the process | no | nothing in the file | - | not compiled | not compiled |
| 74665 | ptrmapPut | a 4-byte page number or count into a pointer-map page (auto-vacuum) | no | nothing; pointer-map pages exist only under auto-vacuum | - | 0: not tested | 0: not tested |
| 75205 | defragmentPage | comment | - | - | - | comment | comment |
| 75227 | defragmentPage | cells between two freeblocks, moved up inside the content area | no: both inside [top, usableSize) by the corruption checks before it | the old bytes of the moved cells below the new content start | the zeroing of [iCellFirst, cbrk) at 75289 | 262 | 441 |
| 75235 | defragmentPage | the content block [top, iFree) moved up to cbrk | no: top < iFree and iFree+sz <= usableSize are checked before it | the old bytes of the block below cbrk | 75289 | 500 | 837 |
| 75238 | defragmentPage | a 2-byte offset or count into the pointer array | no | nothing | - | 7.18k | 10.9k |
| 75239 | defragmentPage | a 2-byte offset or count into the pointer array | no | nothing | - | 4.25k | 6.58k |
| 75251 | defragmentPage | the page, copied whole to the pager's temp space | no | a copy in the process's memory, not in the file | - | 470 | 736 |
| 75274 | defragmentPage | a 2-byte offset or count into the pointer array | no | nothing | - | 10.5k | 13.8k |
| 75275 | defragmentPage | a cell from the temp copy to its new place cbrk | no: cbrk >= iCellStart and pc+size <= usableSize are checked before it | the old place of the cell below cbrk | 75289 | 10.5k | 13.8k |
| 75286 | defragmentPage | a 2-byte offset or count into the header (content start) | no | nothing | - | 970 | 1.57k |
| 75289 | defragmentPage | zeros over the gap [iCellFirst, cbrk) | no | nothing: it is a zeroing | - | 970 | 1.57k |
| 75335 | pageFindSlot | the 2-byte next pointer of a freeblock into its predecessor | no | nothing | - | 3.58k | 4.41k |
| 75345 | pageFindSlot | the 2-byte size of what stays of a freeblock | no | nothing | - | 24.0k | 36.0k |
| 75460 | allocateSpace | a 2-byte offset or count into the header (content start) | no | nothing | - | 30.8k | 39.8k |
| 75556 | freeSpace | zeros over a freed cell and the freeblocks that it joins | no | nothing: it is a zeroing | - | 37.5k | 53.4k |
| 75564 | freeSpace | a 2-byte offset or count into the header (first freeblock) | no | nothing | - | 6.90k | 11.0k |
| 75565 | freeSpace | a 2-byte offset or count into the header (content start) | no | nothing | - | 6.90k | 11.0k |
| 75568 | freeSpace | a 2-byte offset or count into the freeblock chain | no | the 4 header bytes of a freeblock, which the structural check allows | - | 30.6k | 45.1k |
| 75569 | freeSpace | a 2-byte offset or count into the freeblock chain | no | as 75568 | - | 30.6k | 45.1k |
| 75571 | freeSpace | a 2-byte offset or count (the size of a freeblock) | no | as 75568 | - | 30.6k | 45.1k |
| 75844 | zeroPage | zeros over a page from its header to its end | no | nothing: it is a zeroing | - | 1.50k | 2.19k |
| 75848 | zeroPage | zeros over 4 header bytes | no | nothing | - | 1.50k | 2.40k |
| 75850 | zeroPage | a 2-byte offset or count into the header (content start) | no | nothing | - | 1.50k | 2.40k |
| 76168 | sqlite3BtreeOpen | a buffer of the process | no | nothing in the file | - | 0: not tested | 0: not tested |
| 76236 | sqlite3BtreeOpen | a buffer of the process | no | nothing in the file | - | 220 | 420 |
| 76444 | allocateTempSpace | a buffer of the process | no | nothing in the file | - | 0: not tested | 0: not tested |
| 76463 | allocateTempSpace | a buffer of the process | no | nothing in the file | - | 117 | 218 |
| 77085 | newDatabase | the magic string into the file header of a new database | no | a fixed string, no row | - | 33 | 50 |
| 77096 | newDatabase | zeros over bytes 24 to 99 of the file header | no | nothing | - | 33 | 50 |
| 77102 | newDatabase | a 4-byte page number or count into the file header | no | nothing | - | 33 | 50 |
| 77103 | newDatabase | a 4-byte page number or count into the file header | no | nothing | - | 33 | 50 |
| 77342 | btreeBeginTrans | a 4-byte page number or count (the page count) into the file header, when the header's count differs from the file's, as after a writer older than the header format | no | nothing; a file that this build made does not reach it | - | 0: not tested | 0: not tested |
| 77449 | modifyPagePointer | a 4-byte page number or count into an overflow page (auto-vacuum) | no | nothing | - | 0: not tested | 0: not tested |
| 77469 | modifyPagePointer | a 4-byte page number or count into a cell (auto-vacuum) | no | nothing | - | 0: not tested | 0: not tested |
| 77478 | modifyPagePointer | a 4-byte page number or count into a cell (auto-vacuum) | no | nothing | - | 0: not tested | 0: not tested |
| 77489 | modifyPagePointer | a 4-byte page number or count into a page header (auto-vacuum) | no | nothing | - | 0: not tested | 0: not tested |
| 77749 | sqlite3BtreeIncrVacuum | a 4-byte page number or count into the file header (auto-vacuum) | no | nothing | - | 0: not tested | 0: not tested |
| 77828 | autoVacuumCommit | a 4-byte page number or count into the file header (auto-vacuum) | no | nothing | - | 0: not tested | 0: not tested |
| 77829 | autoVacuumCommit | a 4-byte page number or count into the file header (auto-vacuum) | no | nothing | - | 0: not tested | 0: not tested |
| 77831 | autoVacuumCommit | a 4-byte page number or count into the file header (auto-vacuum) | no | nothing | - | 0: not tested | 0: not tested |
| 78378 | sqlite3BtreeClosesWithCursor | comment | - | - | - | comment | comment |
| 78384 | sqlite3BtreeCursorZero | a buffer of the process | no | nothing in the file | - | 98.6k | 119k |
| 78445 | sqlite3BtreeCloseCursor | a buffer of the process | no | nothing in the file (debug build only) | - | not compiled | not compiled |
| 78649 | copyPayload | new bytes of an incremental blob write over the payload of the row in place | no: bounded by the payload, checked by the caller | nothing: the old bytes are written over | - | 6 | 7 |
| 78652 | copyPayload | a buffer of the process | no | nothing in the file (a read) | - | 61.6k | 103k |
| 78771 | accessPayload | a buffer of the process | no | nothing in the file | - | 17.0k | 32.5k |
| 78847 | accessPayload | a buffer of the process | no | nothing in the file (4 bytes saved before a direct read) | - | 20.6k | 20.6k |
| 78850 | accessPayload | a buffer of the process | no | nothing in the file (the same 4 bytes put back) | - | 20.6k | 20.6k |
| 79751 | sqlite3BtreeIndexMoveto | a buffer of the process | no | nothing in the file | - | 12.8k | 24.3k |
| 80133 | allocateBtreePage | a 4-byte page number or count into the file header (freelist count) | no | nothing | - | 816 | 1.46k |
| 80178 | allocateBtreePage | 4 bytes (a page number) from a trunk into the file header | no | nothing | - | 53 | 77 |
| 80202 | allocateBtreePage | 4 bytes (a page number) from a trunk into the file header (auto-vacuum) | no | nothing; auto-vacuum only: the branch of lines 80186 to 80248 runs under searchList, which only the auto-vacuum code sets (77632, 77664, 83666) | - | 0: not tested | 0: not tested |
| 80208 | allocateBtreePage | 4 bytes (a page number) between two trunks (auto-vacuum) | no | nothing; auto-vacuum only: the branch of lines 80186 to 80248 runs under searchList, which only the auto-vacuum code sets (77632, 77664, 83666) | - | 0: not tested | 0: not tested |
| 80231 | allocateBtreePage | 4 bytes (a page number) into a new trunk (auto-vacuum) | no | nothing; auto-vacuum only: the branch of lines 80186 to 80248 runs under searchList, which only the auto-vacuum code sets (77632, 77664, 83666) | - | 0: not tested | 0: not tested |
| 80232 | allocateBtreePage | a 4-byte page number or count into a new trunk (auto-vacuum) | no | nothing; auto-vacuum only: the branch of lines 80186 to 80248 runs under searchList, which only the auto-vacuum code sets (77632, 77664, 83666) | - | 0: not tested | 0: not tested |
| 80233 | allocateBtreePage | (k-1) leaf page numbers into a new trunk (auto-vacuum) | no | page numbers past the count of the old trunk, which the structural check allows; auto-vacuum only: the branch of lines 80186 to 80248 runs under searchList, which only the auto-vacuum code sets (77632, 77664, 83666) | - | 0: not tested | 0: not tested |
| 80237 | allocateBtreePage | a 4-byte page number or count into the file header (auto-vacuum) | no | nothing; auto-vacuum only: the branch of lines 80186 to 80248 runs under searchList, which only the auto-vacuum code sets (77632, 77664, 83666) | - | 0: not tested | 0: not tested |
| 80243 | allocateBtreePage | a 4-byte page number or count into a trunk (auto-vacuum) | no | nothing; auto-vacuum only: the branch of lines 80186 to 80248 runs under searchList, which only the auto-vacuum code sets (77632, 77664, 83666) | - | 0: not tested | 0: not tested |
| 80298 | allocateBtreePage | a leaf page number moved inside a trunk | no | a page number past the trunk's count, which the structural check allows | - | 679 | 1.24k |
| 80300 | allocateBtreePage | a 4-byte page number or count into a trunk (its count) | no | nothing | - | 763 | 1.39k |
| 80361 | allocateBtreePage | a 4-byte page number or count into the file header (page count) | no | nothing | - | 1.59k | 2.55k |
| 80423 | freePage2 | a 4-byte page number or count into the file header (freelist count) | no | nothing | - | 1.68k | 2.88k |
| 80434 | freePage2 | zeros over a freed page, under secure delete | no | nothing: it is a zeroing | - | 1.68k | 2.64k |
| 80493 | freePage2 | a 4-byte page number or count into a trunk (its count) | no | nothing | - | 1.61k | 2.78k |
| 80494 | freePage2 | a 4-byte page number or count into a trunk (a leaf) | no | nothing | - | 1.61k | 2.78k |
| 80518 | freePage2 | a 4-byte page number or count into a freed page that becomes a trunk | no | nothing | - | 74 | 107 |
| 80519 | freePage2 | a 4-byte page number or count into a new trunk | no | nothing | - | 74 | 107 |
| 80520 | freePage2 | a 4-byte page number or count into the file header | no | nothing | - | 74 | 107 |
| 80690 | fillInCell | the payload of a new cell into the cell buffer or an overflow page | no: nSrc <= nPayload, asserted | nothing: the rest of the payload is zeroed at 80691 | - | 42.9k | 53.9k |
| 80691 | fillInCell | zeros over the rest of a payload | no | nothing: it is a zeroing | - | 42.9k | 53.9k |
| 80748 | fillInCell | a part of a payload into the cell buffer or an overflow page | no | the tail of the last overflow page past the payload keeps the bytes that the page held, which secure delete zeroed when the page was freed | 80434 when the page was freed | 1.39k | 2.46k |
| 80751 | fillInCell | as 80748 | no | as 80748 | 80434 | 10 | 20 |
| 80753 | fillInCell | zeros over a part of a payload | no | nothing: it is a zeroing | - | 20 | 40 |
| 80807 | fillInCell | a 4-byte page number or count (the next overflow page) into a page | no | nothing | - | 870 | 1.53k |
| 80811 | fillInCell | a 4-byte page number or count (zero) into a page | no | nothing | - | 870 | 1.53k |
| 80860 | dropCell | zeros over 4 header bytes of an emptied page | no | nothing | - | 1.47k | 2.05k |
| 80862 | dropCell | a 2-byte offset or count into the header (content start) | no | nothing | - | 1.47k | 2.05k |
| 80866 | dropCell | the pointer array moved down one slot | no | a stale 2-byte offset after the new end of the array, which the structural check allows | line 81517 when the page is edited in place later, 81262 when it is built again | 27.0k | 39.4k |
| 80867 | dropCell | a 2-byte offset or count into the header (cell count) | no | nothing | - | 27.0k | 39.4k |
| 80916 | insertCell | a buffer of the process | no | nothing in the file (the cell kept for a balance) | - | 89 | 169 |
| 80919 | insertCell | a 4-byte page number or count (a child page) into a cell buffer | no | nothing | - | 89 | 169 |
| 80956 | insertCell | a cell into its place in an interior page | no: idx+sz <= usableSize, asserted after allocateSpace | nothing: the place came from the gap or a zeroed freeblock | - | 7.38k | 12.0k |
| 80957 | insertCell | a 4-byte page number or count (a child page) into a cell | no | nothing | - | 7.38k | 12.0k |
| 80959 | insertCell | the pointer array moved up one slot | no | nothing | - | 7.38k | 12.0k |
| 80960 | insertCell | a 2-byte offset or count into the pointer array | no | nothing | - | 7.38k | 12.0k |
| 81041 | insertCellFast | a cell into its place in a leaf | no: as 80956 | nothing: as 80956 | - | 38.2k | 47.9k |
| 81043 | insertCellFast | the pointer array moved up one slot | no | nothing | - | 38.2k | 47.9k |
| 81044 | insertCellFast | a 2-byte offset or count into the pointer array | no | nothing | - | 38.2k | 47.9k |
| 81228 | rebuildPage | the content area copied to the pager's temp space | no | a copy in the process's memory, not in the file | - | 1.43k | 2.27k |
| 81249 | rebuildPage | a 2-byte offset or count into the pointer array | no | nothing | - | 15.9k | 24.8k |
| 81252 | rebuildPage | a cell packed at the end of the page being built again | no: line 81251 returns before the cell would pass the pointers | the earlier places of kept cells and cells that a failed edit had copied, in the gap | Halvo's statement at the end of line 81262 | 15.9k | 24.8k |
| 81267 | rebuildPage | zeros into the header (first freeblock) | no | nothing | - | 1.43k | 2.27k |
| 81268 | rebuildPage | a 2-byte offset or count into the header (cell count) | no | nothing | - | 1.43k | 2.27k |
| 81269 | rebuildPage | a 2-byte offset or count into the header (content start) | no | nothing | - | 1.43k | 2.27k |
| 81329 | pageInsertArray | comment | - | - | - | comment | comment |
| 81330 | pageInsertArray | comment | - | - | - | comment | comment |
| 81341 | pageInsertArray | a cell into a freeblock or the end of the gap of a page being edited | no: (pData - pBegin) >= sz is checked before the slot is taken | on a failed edit, the cells copied so far in the gap | 81262 (the fall-back to rebuildPage); on success the copies are cells | 27.9k | 44.1k |
| 81342 | pageInsertArray | a 2-byte offset or count into the pointer array | no | nothing | - | 27.9k | 44.1k |
| 81453 | editPage | a buffer of the process | no | nothing in the file (debug build only) | - | not compiled | not compiled |
| 81461 | editPage | the pointer array moved down by nShift slots, nCell slots long | the source passes the end of the pointer array by nShift*2 bytes, inside the page | the first bytes of the cell at the content start in the new gap | Halvo's statement at the end of line 81517 | 1.93k | 3.18k |
| 81480 | editPage | the pointer array moved up by nAdd slots | no | nothing beyond the new array | 81517 | 3.34k | 5.52k |
| 81494 | editPage | the pointer array moved up one slot | no | nothing beyond the new array | 81517 | 1.77k | 2.87k |
| 81520 | editPage | a 2-byte offset or count into the header (cell count) | no | nothing | - | 9.04k | 14.9k |
| 81521 | editPage | a 2-byte offset or count into the header (content start) | no | nothing | - | 9.04k | 14.9k |
| 81655 | balance_quick | a 4-byte page number or count (the right child) into a page header | no | nothing | - | 345 | 448 |
| 81741 | copyNodeContent | the content area of one page into another (a root that becomes deeper or shallower) | no | the receiving page's own bytes between its new pointer array and the copied content | a child from allocateBtreePage (82625): a page freed under secure delete was zeroed by freePage2 (80434), or the page is new (under secure_delete=FAST a freed page is not zeroed there; both stores use ON); the root is zeroed by zeroPage (82650); read by the structural check | 258 | 470 |
| 81742 | copyNodeContent | the header and pointer array of one page into another | no | as 81741 | as 81741 | 258 | 470 |
| 81840 | balance_nonroot | a buffer of the process | no | nothing in the file | - | 3.56k | 5.92k |
| 81842 | balance_nonroot | a buffer of the process | no | nothing in the file | - | 3.56k | 5.92k |
| 81898 | balance_nonroot | a buffer of the process | no | nothing in the file | - | 0: not tested | 0: not tested |
| 81904 | balance_nonroot | a buffer of the process | no | nothing in the file | - | 0: not tested | 0: not tested |
| 81940 | balance_nonroot | a buffer of the process | no | nothing in the file (a divider kept for the balance) | - | 6.48k | 10.2k |
| 82023 | balance_nonroot | a buffer of the process | no | nothing in the file | - | 10.0k | 16.6k |
| 82060 | balance_nonroot | a buffer of the process | no | nothing in the file (a divider kept for the balance) | - | 4.80k | 7.94k |
| 82069 | balance_nonroot | a buffer of the process | no | nothing in the file | - | 114 | 218 |
| 82317 | balance_nonroot | a 4-byte page number or count (the right child) into the parent | no | nothing | - | 3.56k | 5.92k |
| 82329 | balance_nonroot | 4 bytes (the right child) between two pages | no | nothing | - | 99 | 198 |
| 82405 | balance_nonroot | 4 bytes (a child page) into a new page | no | nothing | - | 115 | 220 |
| 82643 | balance_deeper | a buffer of the process | no | nothing in the file | - | 172 | 299 |
| 82645 | balance_deeper | a buffer of the process | no | nothing in the file | - | 172 | 299 |
| 82651 | balance_deeper | a 4-byte page number or count (the right child) into a root | no | nothing | - | 172 | 299 |
| 82840 | btreeOverwriteContent | zeros over a payload being written over | no | nothing: it is a zeroing | - | 10 | 20 |
| 82857 | btreeOverwriteContent | comment | - | - | - | comment | comment |
| 82858 | btreeOverwriteContent | new bytes over the payload of a row in place, same size | no | nothing: the old bytes are written over | - | 47 | 87 |
| 83181 | sqlite3BtreeInsert | a buffer of the process | no | nothing in the file | - | 3 | 5 |
| 83205 | sqlite3BtreeInsert | a new cell over the old one in place, same size | no: checked against the page before the copy | nothing: the old bytes are written over | - | 2.49k | 2.61k |
| 83260 | sqlite3BtreeInsert | a buffer of the process | no | nothing in the file | - | 0: not tested | 0: not tested |
| 83309 | sqlite3BtreeTransferRow | the payload of a row into a new cell buffer (a transfer) | no: aIn+nIn is checked against the page before it | nothing in the file | - | 104 | 208 |
| 83341 | sqlite3BtreeTransferRow | a part of a payload into a cell buffer or an overflow page (a transfer) | no | as 80748 | 80434 | 108 | 216 |
| 83364 | sqlite3BtreeTransferRow | a 4-byte page number or count (the next overflow page of a copied row) into a cell or an overflow page | no | nothing | - | 72 | 144 |
| 83372 | sqlite3BtreeTransferRow | a 4-byte page number or count (zero, the end of a copied row's chain) into an overflow page | no | nothing | - | 72 | 144 |
| 84044 | sqlite3BtreeUpdateMeta | a 4-byte page number or count (a meta value) into the file header | no | nothing | - | 183 | 274 |
| 84738 | sqlite3BtreeIntegrityCheck | a buffer of the process | no | nothing in the file (the integrity check's state) | - | 354 | 688 |

## Amendments

- 2026-11-14: rulings S-83 and ST-42 widen the decision after the review of
  sqlcipher-1 found the second place in editPage: the statement at the end of
  line 81517, the structural check, the table, the answer 2 of the added
  function and the build without SQLITE_ENABLE_STAT4.
