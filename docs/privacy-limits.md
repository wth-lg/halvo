# What expiry and delete-all cover, and what capture stores

This file states the limits of Halvo's privacy promises in plain words. The README,
the data view and the consent text quote it and do not restate it. ADR 0005 states
the key chain behind it.

A log named in this file is a path under the ledger of run 20261108-halvo-r1-capture
(`~/.wstack/runs/20261108-halvo-r1-capture/`). A cell that says "not run" is a
record of what nobody has measured yet, not a pass.

## What expiry and delete-all cover against an earlier copy

Halvo keeps what it records in an encrypted store in its data directory. Window
expiry and the storage cap destroy the key of a segment when the segment is older
than the window or the store is over the cap. Delete-all destroys the custody root,
which every other key of the store depends on. The table says what a reader who
acts as the same user can still decrypt after each of them.

| What ran | The reader has the live store files | The reader also has a copy of the store files made earlier | The earlier copy also holds the custody root |
|---|---|---|---|
| Window expiry or cap eviction of a segment | The segment does not decrypt: 0 of 1,125 shredded keys were left in the raw pages of store.db (evidence/interrogate-rr-store-1/08-raw-pages-pinned.log), and none after any oldest-first pass (evidence/interrogate-rr-store-1/12-oldest-first-pinned.log). Note of 2026-11-11: until store-4 removes the per-event rows (ADR 0009), rows of an expired segment can stay in freed page space of store.db (RR-store-08); docs-2 deletes this note. | The segment decrypts: the older files gave back 66,313 bytes equal to the original plaintext, with the Time Machine exclusion attribute set (evidence/interrogate-rr-store-1/09-rollback.log). | The segment decrypts, as in the column before: expiry destroys no root, and that run read the older files with the root that was still in place (evidence/interrogate-rr-store-1/09-rollback.log). |
| Delete-all | Nothing decrypts: delete-all destroys the root first (RD-20; store-3 lands the order) and then removes the key file, the database and the segments, and after it even the older files of the next column open Sealed (evidence/interrogate-rr-store-1/09-rollback.log, control line). | Nothing decrypts: the older files open Sealed, because the root they need is gone (evidence/interrogate-rr-store-1/09-rollback.log, control line). | Everything decrypts, by reading the code on both systems, because both use the same root-file code. Not run: store-6 ports the rollback test and win-5 runs the restore test on Windows. |

Where the custody root lives decides the last column. An unsigned build keeps the
root in a per-user root file in the `roots` directory of the data directory, on both
systems: on macOS under `~/Library/Application Support/Halvo/roots/` (store-3 and
daemon-3 land it), on Windows under `%LOCALAPPDATA%\com.halvo.desktop\data\roots\`
(control-1 lands the data directory). So a copy of the whole data directory holds
the root, and a copy of the store directory alone does not. Builds before store-3
kept the macOS root in the login keychain, which the Time Machine exclusion does not
cover (`tmutil isexcluded` prints `[Included]` for the login keychain).

R1 records this limit and builds no per-day key. Halvo sets the Time Machine
exclusion mark on the data directory. The mark stops Time Machine from copying the
data directory and stops no other copy.

A Secure Enclave custody would keep the root outside every copy of the directory.
R1 ships none: a build with the `secure-enclave` feature creates no store and opens
an existing one for maintenance only, and no R1 installer is such a build.

## What else stays after a shred

- Free blocks of the volume. Right after a shred the wrapped key of the segment came
  back from a freed WAL frame of the raw volume image in 6 of 6 runs on APFS, and it
  was gone after 5 MiB of further capture
  (evidence/interrogate-rr-store-3/it14_raw_volume_apfs_run1.log to
  it14_raw_volume_apfs_run3.log, and the three logs of the same names under
  rerun/). A reader of those blocks also needs the owner's root.
- The Time Machine mark. Time Machine skips the data directory. A sync client,
  another backup tool, Migration Assistant and a copy made by hand are not covered.
  Whether a local APFS snapshot holds the directory is not verified.
- Segment files. From store-4 on, a file name carries no time, because file ids are
  random. The size of each file, its file times and the segment's number in the
  clear header remain, and they show when the person was active. R1 accepts this
  and pads nothing (RR-store-25).
- Key rows. The ids and times of shredded segments stay in store.db until
  delete-all, about 124 bytes a row (evidence/interrogate-rr-store-3/it03_what_a_shred_leaves_inside_store_db.log).
- Rows of a segment shredded out of order (RR-store-08). store-4 closes this when it
  removes the per-event rows; until then the note in the table applies.
