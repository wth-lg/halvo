# local storage, encryption and deletion design

decision: 2026-11-09T07:51:36Z
status: accepted
decided by: planner-1, run 20261108-halvo-r1-capture
evidence: row 12 of decisions.tsv of run 20261108-halvo-r1-capture; docs/privacy-limits.md

## Context

Halvo records activity on the person's machine and keeps it there, encrypted, for
the observation window. The person can delete all of it at any time. Row 12 chose
how: SQLCipher for the metadata, one file for each segment under a random key with
XChaCha20-Poly1305, a store key wrapped by a custody of each system, and deletion by
crypto-shred with zero-fill.

## Decision

The store encrypts by a key chain of three levels:

1. The custody root wraps the store key (crates/halvo-capture/src/custody/mod.rs:82-83
   at 685d846).
2. The store key wraps each segment key (crates/halvo-capture/src/store/segment.rs:48-57
   at 685d846).
3. Each segment key encrypts one segment file. A store has one store key, drawn once
   when the store is created (crates/halvo-capture/src/store/db.rs:444 at 685d846) and
   never rotated.

Window expiry and cap eviction destroy a segment key. Delete-all destroys the root.

## Consequences

- The limit, in one sentence: window expiry does not reach an earlier copy of the
  store files or free blocks of the volume; delete-all does, unless the copy holds
  the root. docs/privacy-limits.md holds the coverage table and the measured bounds,
  and the README and the view quote that file.
- R1 records the limit and builds no per-day key (the default answer of RD-03, an
  owner ask; another answer is a dated line here). The Time Machine mark on the data
  directory stays as a mitigation: it stops Time Machine from copying the data
  directory and stops no other copy.
