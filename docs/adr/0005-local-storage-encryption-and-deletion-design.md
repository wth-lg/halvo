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
  and it is decided that the README (docs-2) and the view (app-3 and app-4) quote
  that file.
- R1 records the limit and builds no per-day key (the default answer of RD-03, an
  owner ask; another answer is a dated line here). The Time Machine mark on the data
  directory is the mitigation, and daemon-3a sets it when the daemon starts
  (contract C3.2 item 7); until then Time Machine copies the data directory like
  any other. The mark stops Time Machine from copying the data directory and stops
  no other copy.
- The storage cap counts disk bytes of the capture store: its database pages, its
  WAL and the segment ciphertext. The inventory labels the logical and the disk
  totals apart (DR-03, which row 76 folds into the units).
- Delete-all runs in this order, attempts every step and reports once after all of
  them ran: destroy the root, seal the store against writes, destroy the key file,
  destroy the database, remove the segments. When the root cannot be destroyed, the
  store seals, the rest still runs and the report names the root that survived
  (RD-20; store-3 lands the order).
- A shred keeps the key row of the segment and clears its key in place; nothing
  deletes a key row before delete-all. The ids and times of shredded segments stay
  in store.db until delete-all, about 124 bytes a row (RD-21).
- A store that an older build wrote, of any schema before store-4's, is refused: it
  reports `store=unsupported_version` with `store_error=older`, and delete-all is
  accepted (RD-22).
- ADR 0009 removes the per-event rows and the full-text tables of row 12. ADR 0010
  replaces the Secure Enclave custody of row 12 for every R1 build.
