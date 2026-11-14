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
  SQLITE_ENABLE_STAT4 is defined, SQLITE_TEMP_STORE is not 2,
  SQLITE_DEFAULT_AUTOVACUUM is not 0, BTS_FAST_SECURE is not defined (the
  database's code is not compiled in this file, as under an outside
  -DSQLITE_AMALGAMATION), SQLITE_HAS_CODEC is not defined,
  SQLITE_DEFAULT_SYNCHRONOUS or SQLITE_DEFAULT_WAL_SYNCHRONOUS is not 2,
  SQLITE_DEBUG is defined, or SQLITE_DEFAULT_PAGE_SIZE is not 4096, whether
  build.rs or the environment (CFLAGS) set it. A flag that redefines an
  identifier of the source is not read. The test
  `page_gap::pins::the_compile_options_of_this_build_are_its_closed_list`
  reads PRAGMA compile_options, less the compiler's line, against a closed
  list for each system. The function halvo_sqlcipher_page_gap_mend answers 2
  and touches no structure of the database. `halvo_sqlcipher::page_gap_mend_linked`
  is true only for that answer, so a program that calls it does not link
  another SQLCipher and refuses a library of the earlier one-statement build.
  The open of each store is to require it, in the next unit that changes that
  open.
- The build has no SQLITE_ENABLE_STAT4, and the compiler stops if a flag
  defines it: with it, ANALYZE and PRAGMA optimize copy whole index keys into
  sqlite_stat4, where they outlive the rows. No store runs either.
- No cargo feature and no switch of the environment or of the build takes a
  statement out.

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
  layout after each round and requires zero in every byte that holds no cell,
  no header and no pointer, a cell of fewer than 4 bytes included. Its rules
  stand in tests/page_gap/rules.rs, which uses rusqlite alone so that another
  crate's tests can include it; its `plants` function plants a byte in each
  place that it reads in a copy of a caller's own image. Its allowances are
  its limit: a run of stale 2-byte offsets right after the pointer array whose
  values are below the usable size, and 4-byte values no larger than the page
  count in the tail of an overflow page and in a freelist trunk past its
  count. A leftover made only of such values is not seen. Because a larger
  page lets more text pass as offsets, the check stops by name on a page size
  above 4,096 bytes, which both stores use.
- The keyed draws get no byte search and no structural check: they read the
  rows back against the model and run the integrity check alone. The stores'
  own checks read the keyed layout (rulings ST-42 (2) and S-83 7 (b)).
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
  auto-vacuum field) is the same pin on every drawn file. The fifteen rows of
  auto-vacuum code in the table run in no draw under that pin, and the row of
  line 77342 in none because a file that this build made does not reach it.
- Its controls fail by name: each statement taken out, secure delete off, and
  4 bytes of text planted in a gap of a copy of the file. The mutant rows
  rebuilt-page-gap-not-zeroed and edited-page-gap-not-zeroed take each
  statement out. A narrowing of the editPage statement to the bytes of line
  81461 is caught by the structural check alone: the byte search does not see
  it in the draws.
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
  compiled copy under clang's coverage with llvm-cov's rounding, made at the
  head ab8282d6 of the branch (the lines of this round's new rows read at the
  same head), in two
  columns: the secured keyless draws alone (the eleven tests of page_gap that
  run with secure delete on and without a key and search the files or check
  their pages: the main test, the three of second_place, the three of
  rewrites without their controls, and the plants and the two draws of
  layout), and all the tests of the crate, the draws with secure delete off
  and the keyed draws among them.
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
| 74297 | saveCursorKey | a buffer of the process | no | nothing in the file (a saved key) | - | 112 | 112 |
| 74563 | sqlite3BtreeCursorHint | a buffer of the process | no | nothing in the file | - | not compiled | not compiled |
| 74665 | ptrmapPut | a 4-byte page number or count into a pointer-map page (auto-vacuum) | no | nothing; pointer-map pages exist only under auto-vacuum | - | 0: not tested | 0: not tested |
| 75205 | defragmentPage | comment | - | - | - | comment | comment |
| 75227 | defragmentPage | cells between two freeblocks, moved up inside the content area | no: both inside [top, usableSize) by the corruption checks before it | the old bytes of the moved cells below the new content start | the zeroing of [iCellFirst, cbrk) at 75289 | 212 | 390 |
| 75235 | defragmentPage | the content block [top, iFree) moved up to cbrk | no: top < iFree and iFree+sz <= usableSize are checked before it | the old bytes of the block below cbrk | 75289 | 397 | 732 |
| 75238 | defragmentPage | a 2-byte offset or count into the pointer array | no | nothing | - | 5.11k | 8.83k |
| 75239 | defragmentPage | a 2-byte offset or count into the pointer array | no | nothing | - | 3.27k | 5.57k |
| 75251 | defragmentPage | the page, copied whole to the pager's temp space | no | a copy in the process's memory, not in the file | - | 323 | 578 |
| 75274 | defragmentPage | a 2-byte offset or count into the pointer array | no | nothing | - | 5.73k | 8.73k |
| 75275 | defragmentPage | a cell from the temp copy to its new place cbrk | no: cbrk >= iCellStart and pc+size <= usableSize are checked before it | the old place of the cell below cbrk | 75289 | 5.73k | 8.73k |
| 75286 | defragmentPage | a 2-byte offset or count into the header (content start) | no | nothing | - | 720 | 1.31k |
| 75289 | defragmentPage | zeros over the gap [iCellFirst, cbrk) | no | nothing: it is a zeroing | - | 720 | 1.31k |
| 75335 | pageFindSlot | the 2-byte next pointer of a freeblock into its predecessor | no | nothing | - | 1.48k | 2.21k |
| 75345 | pageFindSlot | the 2-byte size of what stays of a freeblock | no | nothing | - | 16.1k | 27.8k |
| 75460 | allocateSpace | a 2-byte offset or count into the header (content start) | no | nothing | - | 11.8k | 19.0k |
| 75556 | freeSpace | zeros over a freed cell and the freeblocks that it joins | no | nothing: it is a zeroing | - | 24.2k | 39.5k |
| 75564 | freeSpace | a 2-byte offset or count into the header (first freeblock) | no | nothing | - | 5.03k | 9.12k |
| 75565 | freeSpace | a 2-byte offset or count into the header (content start) | no | nothing | - | 5.03k | 9.12k |
| 75568 | freeSpace | a 2-byte offset or count into the freeblock chain | no | the 4 header bytes of a freeblock, which the structural check allows | - | 19.2k | 33.1k |
| 75569 | freeSpace | a 2-byte offset or count into the freeblock chain | no | as 75568 | - | 19.2k | 33.1k |
| 75571 | freeSpace | a 2-byte offset or count (the size of a freeblock) | no | as 75568 | - | 19.2k | 33.1k |
| 75844 | zeroPage | zeros over a page from its header to its end | no | nothing: it is a zeroing | - | 1.01k | 1.65k |
| 75848 | zeroPage | zeros over 4 header bytes | no | nothing | - | 1.01k | 1.87k |
| 75850 | zeroPage | a 2-byte offset or count into the header (content start) | no | nothing | - | 1.01k | 1.87k |
| 76168 | sqlite3BtreeOpen | a buffer of the process | no | nothing in the file | - | 0: not tested | 0: not tested |
| 76236 | sqlite3BtreeOpen | a buffer of the process | no | nothing in the file | - | 199 | 391 |
| 76444 | allocateTempSpace | a buffer of the process | no | nothing in the file | - | 0: not tested | 0: not tested |
| 76463 | allocateTempSpace | a buffer of the process | no | nothing in the file | - | 101 | 198 |
| 77085 | newDatabase | the magic string into the file header of a new database | no | a fixed string, no row | - | 17 | 31 |
| 77096 | newDatabase | zeros over bytes 24 to 99 of the file header | no | nothing | - | 17 | 31 |
| 77102 | newDatabase | a 4-byte page number or count into the file header | no | nothing | - | 17 | 31 |
| 77103 | newDatabase | a 4-byte page number or count into the file header | no | nothing | - | 17 | 31 |
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
| 78384 | sqlite3BtreeCursorZero | a buffer of the process | no | nothing in the file | - | 34.9k | 52.6k |
| 78445 | sqlite3BtreeCloseCursor | a buffer of the process | no | nothing in the file (debug build only) | - | not compiled | not compiled |
| 78649 | copyPayload | new bytes of an incremental blob write over the payload of the row in place | no: bounded by the payload, checked by the caller | nothing: the old bytes are written over | - | 1 | 1 |
| 78652 | copyPayload | a buffer of the process | no | nothing in the file (a read) | - | 30.1k | 71.8k |
| 78771 | accessPayload | a buffer of the process | no | nothing in the file | - | 16.0k | 31.5k |
| 78847 | accessPayload | a buffer of the process | no | nothing in the file (4 bytes saved before a direct read) | - | 20.5k | 20.6k |
| 78850 | accessPayload | a buffer of the process | no | nothing in the file (the same 4 bytes put back) | - | 20.5k | 20.6k |
| 79751 | sqlite3BtreeIndexMoveto | a buffer of the process | no | nothing in the file | - | 11.9k | 23.3k |
| 80133 | allocateBtreePage | a 4-byte page number or count into the file header (freelist count) | no | nothing | - | 670 | 1.31k |
| 80178 | allocateBtreePage | 4 bytes (a page number) from a trunk into the file header | no | nothing | - | 23 | 45 |
| 80202 | allocateBtreePage | 4 bytes (a page number) from a trunk into the file header (auto-vacuum) | no | nothing; auto-vacuum only: the branch of lines 80186 to 80248 runs under searchList, which only the auto-vacuum code sets (77632, 77664, 83666) | - | 0: not tested | 0: not tested |
| 80208 | allocateBtreePage | 4 bytes (a page number) between two trunks (auto-vacuum) | no | nothing; auto-vacuum only: the branch of lines 80186 to 80248 runs under searchList, which only the auto-vacuum code sets (77632, 77664, 83666) | - | 0: not tested | 0: not tested |
| 80231 | allocateBtreePage | 4 bytes (a page number) into a new trunk (auto-vacuum) | no | nothing; auto-vacuum only: the branch of lines 80186 to 80248 runs under searchList, which only the auto-vacuum code sets (77632, 77664, 83666) | - | 0: not tested | 0: not tested |
| 80232 | allocateBtreePage | a 4-byte page number or count into a new trunk (auto-vacuum) | no | nothing; auto-vacuum only: the branch of lines 80186 to 80248 runs under searchList, which only the auto-vacuum code sets (77632, 77664, 83666) | - | 0: not tested | 0: not tested |
| 80233 | allocateBtreePage | (k-1) leaf page numbers into a new trunk (auto-vacuum) | no | page numbers past the count of the old trunk, which the structural check allows; auto-vacuum only: the branch of lines 80186 to 80248 runs under searchList, which only the auto-vacuum code sets (77632, 77664, 83666) | - | 0: not tested | 0: not tested |
| 80237 | allocateBtreePage | a 4-byte page number or count into the file header (auto-vacuum) | no | nothing; auto-vacuum only: the branch of lines 80186 to 80248 runs under searchList, which only the auto-vacuum code sets (77632, 77664, 83666) | - | 0: not tested | 0: not tested |
| 80243 | allocateBtreePage | a 4-byte page number or count into a trunk (auto-vacuum) | no | nothing; auto-vacuum only: the branch of lines 80186 to 80248 runs under searchList, which only the auto-vacuum code sets (77632, 77664, 83666) | - | 0: not tested | 0: not tested |
| 80298 | allocateBtreePage | a leaf page number moved inside a trunk | no | a page number past the trunk's count, which the structural check allows | - | 594 | 1.15k |
| 80300 | allocateBtreePage | a 4-byte page number or count into a trunk (its count) | no | nothing | - | 647 | 1.26k |
| 80361 | allocateBtreePage | a 4-byte page number or count into the file header (page count) | no | nothing | - | 1.08k | 2.01k |
| 80423 | freePage2 | a 4-byte page number or count into the file header (freelist count) | no | nothing | - | 1.45k | 2.63k |
| 80434 | freePage2 | zeros over a freed page, under secure delete | no | nothing: it is a zeroing | - | 1.45k | 2.39k |
| 80493 | freePage2 | a 4-byte page number or count into a trunk (its count) | no | nothing | - | 1.41k | 2.56k |
| 80494 | freePage2 | a 4-byte page number or count into a trunk (a leaf) | no | nothing | - | 1.41k | 2.56k |
| 80518 | freePage2 | a 4-byte page number or count into a freed page that becomes a trunk | no | nothing | - | 38 | 68 |
| 80519 | freePage2 | a 4-byte page number or count into a new trunk | no | nothing | - | 38 | 68 |
| 80520 | freePage2 | a 4-byte page number or count into the file header | no | nothing | - | 38 | 68 |
| 80690 | fillInCell | the payload of a new cell into the cell buffer or an overflow page | no: nSrc <= nPayload, asserted | nothing: the rest of the payload is zeroed at 80691 | - | 15.5k | 24.2k |
| 80691 | fillInCell | zeros over the rest of a payload | no | nothing: it is a zeroing | - | 15.5k | 24.2k |
| 80748 | fillInCell | a part of a payload into the cell buffer or an overflow page | no | the tail of the last overflow page past the payload keeps the bytes that the page held, which secure delete zeroed when the page was freed | 80434 when the page was freed | 1.11k | 2.18k |
| 80751 | fillInCell | as 80748 | no | as 80748 | 80434 | 10 | 20 |
| 80753 | fillInCell | zeros over a part of a payload | no | nothing: it is a zeroing | - | 20 | 40 |
| 80807 | fillInCell | a 4-byte page number or count (the next overflow page) into a page | no | nothing | - | 691 | 1.35k |
| 80811 | fillInCell | a 4-byte page number or count (zero) into a page | no | nothing | - | 691 | 1.35k |
| 80860 | dropCell | zeros over 4 header bytes of an emptied page | no | nothing | - | 880 | 1.44k |
| 80862 | dropCell | a 2-byte offset or count into the header (content start) | no | nothing | - | 880 | 1.44k |
| 80866 | dropCell | the pointer array moved down one slot | no | a stale 2-byte offset after the new end of the array, which the structural check allows | line 81517 when the page is edited in place later, 81262 when it is built again | 16.4k | 28.1k |
| 80867 | dropCell | a 2-byte offset or count into the header (cell count) | no | nothing | - | 16.4k | 28.1k |
| 80916 | insertCell | a buffer of the process | no | nothing in the file (the cell kept for a balance) | - | 87 | 167 |
| 80919 | insertCell | a 4-byte page number or count (a child page) into a cell buffer | no | nothing | - | 87 | 167 |
| 80956 | insertCell | a cell into its place in an interior page | no: idx+sz <= usableSize, asserted after allocateSpace | nothing: the place came from the gap or a zeroed freeblock | - | 5.76k | 10.4k |
| 80957 | insertCell | a 4-byte page number or count (a child page) into a cell | no | nothing | - | 5.76k | 10.4k |
| 80959 | insertCell | the pointer array moved up one slot | no | nothing | - | 5.76k | 10.4k |
| 80960 | insertCell | a 2-byte offset or count into the pointer array | no | nothing | - | 5.76k | 10.4k |
| 81041 | insertCellFast | a cell into its place in a leaf | no: as 80956 | nothing: as 80956 | - | 13.8k | 21.3k |
| 81043 | insertCellFast | the pointer array moved up one slot | no | nothing | - | 13.8k | 21.3k |
| 81044 | insertCellFast | a 2-byte offset or count into the pointer array | no | nothing | - | 13.8k | 21.3k |
| 81228 | rebuildPage | the content area copied to the pager's temp space | no | a copy in the process's memory, not in the file | - | 1.05k | 1.87k |
| 81249 | rebuildPage | a 2-byte offset or count into the pointer array | no | nothing | - | 11.8k | 20.7k |
| 81252 | rebuildPage | a cell packed at the end of the page being built again | no: line 81251 returns before the cell would pass the pointers | the earlier places of kept cells and cells that a failed edit had copied, in the gap | Halvo's statement at the end of line 81262 | 11.8k | 20.7k |
| 81267 | rebuildPage | zeros into the header (first freeblock) | no | nothing | - | 1.05k | 1.87k |
| 81268 | rebuildPage | a 2-byte offset or count into the header (cell count) | no | nothing | - | 1.05k | 1.87k |
| 81269 | rebuildPage | a 2-byte offset or count into the header (content start) | no | nothing | - | 1.05k | 1.87k |
| 81329 | pageInsertArray | comment | - | - | - | comment | comment |
| 81330 | pageInsertArray | comment | - | - | - | comment | comment |
| 81341 | pageInsertArray | a cell into a freeblock or the end of the gap of a page being edited | no: (pData - pBegin) >= sz is checked before the slot is taken | on a failed edit, the cells copied so far in the gap | 81262 (the fall-back to rebuildPage); on success the copies are cells | 21.1k | 37.3k |
| 81342 | pageInsertArray | a 2-byte offset or count into the pointer array | no | nothing | - | 21.1k | 37.3k |
| 81453 | editPage | a buffer of the process | no | nothing in the file (debug build only) | - | not compiled | not compiled |
| 81461 | editPage | the pointer array moved down by nShift slots, nCell slots long | the source passes the end of the pointer array by nShift*2 bytes, inside the page | the first bytes of the cell at the content start in the new gap | Halvo's statement at the end of line 81517 | 1.57k | 2.81k |
| 81480 | editPage | the pointer array moved up by nAdd slots | no | nothing beyond the new array | 81517 | 2.58k | 4.75k |
| 81494 | editPage | the pointer array moved up one slot | no | nothing beyond the new array | 81517 | 1.39k | 2.49k |
| 81520 | editPage | a 2-byte offset or count into the header (cell count) | no | nothing | - | 7.12k | 13.0k |
| 81521 | editPage | a 2-byte offset or count into the header (content start) | no | nothing | - | 7.12k | 13.0k |
| 81655 | balance_quick | a 4-byte page number or count (the right child) into a page header | no | nothing | - | 184 | 270 |
| 81741 | copyNodeContent | the content area of one page into another (a root that becomes deeper or shallower) | no | the receiving page's own bytes between its new pointer array and the copied content | zeroPage of a new child (75844) or the zeroed cells of a root; read by the structural check | 193 | 390 |
| 81742 | copyNodeContent | the header and pointer array of one page into another | no | as 81741 | as 81741 | 193 | 390 |
| 81840 | balance_nonroot | a buffer of the process | no | nothing in the file | - | 2.81k | 5.17k |
| 81842 | balance_nonroot | a buffer of the process | no | nothing in the file | - | 2.81k | 5.17k |
| 81898 | balance_nonroot | a buffer of the process | no | nothing in the file | - | 0: not tested | 0: not tested |
| 81904 | balance_nonroot | a buffer of the process | no | nothing in the file | - | 0: not tested | 0: not tested |
| 81940 | balance_nonroot | a buffer of the process | no | nothing in the file (a divider kept for the balance) | - | 5.17k | 8.88k |
| 82023 | balance_nonroot | a buffer of the process | no | nothing in the file | - | 7.98k | 14.5k |
| 82060 | balance_nonroot | a buffer of the process | no | nothing in the file (a divider kept for the balance) | - | 3.91k | 7.05k |
| 82069 | balance_nonroot | a buffer of the process | no | nothing in the file | - | 112 | 216 |
| 82317 | balance_nonroot | a 4-byte page number or count (the right child) into the parent | no | nothing | - | 2.81k | 5.17k |
| 82329 | balance_nonroot | 4 bytes (the right child) between two pages | no | nothing | - | 95 | 194 |
| 82405 | balance_nonroot | 4 bytes (a child page) into a new page | no | nothing | - | 113 | 218 |
| 82643 | balance_deeper | a buffer of the process | no | nothing in the file | - | 115 | 233 |
| 82645 | balance_deeper | a buffer of the process | no | nothing in the file | - | 115 | 233 |
| 82651 | balance_deeper | a 4-byte page number or count (the right child) into a root | no | nothing | - | 115 | 233 |
| 82840 | btreeOverwriteContent | zeros over a payload being written over | no | nothing: it is a zeroing | - | 10 | 20 |
| 82857 | btreeOverwriteContent | comment | - | - | - | comment | comment |
| 82858 | btreeOverwriteContent | new bytes over the payload of a row in place, same size | no | nothing: the old bytes are written over | - | 41 | 81 |
| 83181 | sqlite3BtreeInsert | a buffer of the process | no | nothing in the file | - | 3 | 5 |
| 83205 | sqlite3BtreeInsert | a new cell over the old one in place, same size | no: checked against the page before the copy | nothing: the old bytes are written over | - | 71 | 71 |
| 83260 | sqlite3BtreeInsert | a buffer of the process | no | nothing in the file | - | 0: not tested | 0: not tested |
| 83309 | sqlite3BtreeTransferRow | the payload of a row into a new cell buffer (a transfer) | no: aIn+nIn is checked against the page before it | nothing in the file | - | 104 | 208 |
| 83341 | sqlite3BtreeTransferRow | a part of a payload into a cell buffer or an overflow page (a transfer) | no | as 80748 | 80434 | 108 | 216 |
| 83364 | sqlite3BtreeTransferRow | a 4-byte page number or count (the next overflow page of a copied row) into a cell or an overflow page | no | nothing | - | 72 | 144 |
| 83372 | sqlite3BtreeTransferRow | a 4-byte page number or count (zero, the end of a copied row's chain) into an overflow page | no | nothing | - | 72 | 144 |
| 84044 | sqlite3BtreeUpdateMeta | a 4-byte page number or count (a meta value) into the file header | no | nothing | - | 85 | 162 |
| 84738 | sqlite3BtreeIntegrityCheck | a buffer of the process | no | nothing in the file (the integrity check's state) | - | 352 | 684 |

## Amendments

- 2026-11-14: rulings S-83 and ST-42 widen the decision after the review of
  sqlcipher-1 found the second place in editPage: the statement at the end of
  line 81517, the structural check, the table, the answer 2 of the added
  function and the build without SQLITE_ENABLE_STAT4.
