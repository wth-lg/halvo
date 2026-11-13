# halvo's build of the database zeroes the gap of a page that it builds again

decision: 2026-11-13T20:53:00Z
status: accepted
decided by: the coordinator (eng-manager) of runs 20261109-halvo-r2-workflows and 20261108-halvo-r1-capture, as the default that the team builds to; the owner may turn it through the entry in owner-asks.md of run 20261108-halvo-r1-capture
evidence: ruling S-82 of run 20261109-halvo-r2-workflows (rulings-on-u52-gates.md) and ruling ST-40 of run 20261108-halvo-r1-capture (rulings-on-store2.md); Addition 2 of reports/verifier-37.md of run 20261109-halvo-r2-workflows; unit sqlcipher-1 of run 20261108-halvo-r1-capture

## Context

Both stores open the one SQLCipher build of crates/halvo-sqlcipher with secure
delete on, and promise that what they remove is in no byte of their files. In
the pinned source (sqlcipher-src 419.0.6, SQLCipher 4.19.0 on SQLite 3.53.4),
secure delete zeroes a freed cell, an emptied page, a freed page and the gap of
a page that the database defragments. It does not zero the gap of a page that
a balance builds again (rebuildPage, lines 81205 to 81272 of sqlcipher.c).
Earlier copies of the cells that the page keeps stay in that gap, and so can
cells that a failed edit in place had already copied there, and a later
removal of such a row zeroes its cell and not the copy. The workflow run's
verifier found whole text of removed rows in that gap in derived.db in 15 of
16 drawn stores. Version 420.0.0 of the crate holds rebuildPage byte for byte
as 419.0.6 does.

## Decision

Halvo's build of SQLCipher differs from the pinned source in one function,
rebuildPage, and adds one function of its own.

- crates/halvo-sqlcipher/build.rs reads the pinned sqlcipher.c, writes a copy
  under the build's own output directory and compiles that copy. No file of
  the cargo registry is written, and no copy of the source is in the
  repository.
- The copy holds one statement more, at the end of the comment line 81262 of
  rebuildPage: under secure delete, the bytes between the new cell pointer
  array and the new cell content are set to zero. No line is added, so every
  line number of the compiled file, among them those that the database logs
  for a corrupt page, is that of the pinned file.
- The copy ends with the function halvo_sqlcipher_page_gap_mend, which answers
  a constant and touches no structure of the database.
  `halvo_sqlcipher::page_gap_mend_linked` calls it, so a program that links
  another SQLCipher does not link. The open of each store is to require its
  answer, in the next unit that changes that open.
- No cargo feature and no switch of the environment or of the build takes the
  change out.
- The test page_gap of the crate draws four fixed seeds of a table and a table
  WITHOUT ROWID, each with an index, and finds no byte of a removed or replaced
  row in the files, where the build without the change leaves such bytes for
  each seed. The row rebuilt-page-gap-not-zeroed of
  crates/halvo-sqlcipher/mutants.rows takes the statement out and names that
  test as the one that fails.

## Consequences

- A move of the pin owes a new reading of the page code. The build stops, and
  names what it looked for, when the sha256 of sqlcipher.c is not the one that
  was read, when the comment line is not found exactly once, or when the text
  `static int rebuildPage(` is not found exactly once before it with no
  function end between. Read rebuildPage, editPage, pageInsertArray,
  pageFreeArray, balance_quick and balance_nonroot from its divider loop again
  in the new source, move or delete the change, write the new sha256 into
  build.rs, and run the test page_gap with the change and without it.
- The notices of a bundle owe one line for it: the SQLCipher in Halvo is
  SQLCipher 4.19.0 with one change of Halvo's own in rebuildPage.
- The change cleans no file that a build without it wrote. Such stores are
  removed whole.
- ADR 0006's "the shared SQLCipher build" stands: crates/halvo-sqlcipher is
  still the one build of the database.
