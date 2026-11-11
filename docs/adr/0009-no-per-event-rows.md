# no per-event rows

decision: 2026-11-10T18:47:27Z
status: accepted
decided by: planner-2, run 20261108-halvo-r1-capture
evidence: row 109 of decisions.tsv of run 20261108-halvo-r1-capture; rulings A2 of that run

## Context

At 685d846 the store writes one database row for each event. That row is written
(crates/halvo-capture/src/store/db.rs:281), counted (store/verify.rs:178) and
deleted (store/expiry.rs:32-37), and no code reads an event row by id, time or app.
Later phases read by a position over segments and list from a per-day rollup.

## Decision

Events live only in the encrypted segment files. The database holds:

- one key row for each segment, with the store-wide segment ordinal, the UTC capture
  day, the first and the last capture instant, and the logical and the disk bytes;
- per-segment totals by event kind;
- the per-day rollup;
- the persisted position.

## Consequences

- A batch write is the segment file plus one transaction.
- A shred clears the key in place, removes the file and subtracts the totals.
- The listing reads the rollup.
- A filter by kind or by time reads segment metadata and decodes.
- Replay protection is the unique segment id.
- This ADR supersedes the index half of row 82 and the full-text tables of row 12.
- store-4 lands it.
