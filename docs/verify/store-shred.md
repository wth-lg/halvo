---
name: verify-store-shred
description: Run and maintain the formal checks of the encrypted store's two safety properties, crypto-shred non-decryptability and the single writer, when a change touches crates/halvo-capture/src/store/ or src/custody/.
---

# Verify: store crypto-shred and single writer

Two property checks run the real `EncryptedStore` under random inputs and
fail on the first input that breaks a property. They live in
`crates/halvo-capture/src/store/formal_tests.rs`, compile only under
`cfg(test)`, and need only the `proptest` dev-dependency.

## When to run it

Run the check when a change touches any of the covered code below, and before
moving the sync commit. A change elsewhere in the crate does not need it.

## The properties

1. **Crypto-shred non-decryptability.** Once `crypto_shred_expired` or
   `crypto_shred_all` has shredded a segment, no key remaining in the store
   decrypts that segment's ciphertext, the segment reads as `Sealed`, and its
   file is gone. Every segment that was not shredded still reads back its
   bytes. After delete-all the directory holds no store and the wrapped store
   key retained from before no longer unwraps through its custody.
   Harm if broken: data the owner believes destroyed is still recoverable
   from the store by anyone who later obtains the store key.
2. **Single writer.** At most one process holds the store at a time, even
   while `writer.lock` is unlinked under the running processes. Every
   contender that waits eventually gets the store, holds it, writes a segment,
   and exits clean.
   Harm if broken: two writers corrupt the metadata database, or a reader
   keeps a snapshot alive that preserves a shredded key.

## Tool

Both checks are `proptest` property tests over the real store, not a model.
A verifier that runs on the code is the faithful tool here: the properties
are about bytes on disk, SQLCipher's locking and the OS file lock, none of
which a hand model would carry. `loom` was considered for property 2 and
rejected: it explores interleavings of its own atomics and mutexes inside one
process, and cannot intercept `File::try_lock` or SQLite's exclusive locking,
which are the two mechanisms the property rests on. A loom model would prove
only that a stand-in lock is a lock. The process-level test runs the real
mechanisms instead.

## Covered code

| Check | File | Functions |
|---|---|---|
| 1 | `crates/halvo-capture/src/store/expiry.rs` | `segment_ids_with_keys`, `destroy_segment_keys`, `remove_keyless_files`, `shred_segments`, `destroy_store_key`, `crypto_shred_expired`, `crypto_shred_all` |
| 1 | `crates/halvo-capture/src/store/db.rs` | `Db::open` (the `secure_delete`, `locking_mode`, `journal_mode` pragmas), `Db::checkpoint_truncate`, `reserve_segment_key`, `commit_segment_key`, `KeyFile::destroy`, `Db::destroy`, `EncryptedStore::{create, open, read_segment, write_segment_with_expiry}` |
| 1 | `crates/halvo-capture/src/store/segment.rs` | `wrap_segment_key`, `unwrap_segment_key`, `write_segment_file`, `read_segment_file`, `decrypt_segment_bytes`, `segment_files`, `remove_segment_file` |
| 1 | `crates/halvo-capture/src/custody/dev_fallback.rs` | `DevFallbackCustody::{wrap_store_key, unwrap_store_key, destroy_root}`, `aead_seal`, `aead_open` |
| 2 | `crates/halvo-capture/src/store/db.rs` | `WriterLock::take`, `Db::open` (the `locking_mode = EXCLUSIVE` pragma and `require_exclusive_locking`), `verify_key` (its `DatabaseBusy`/`DatabaseLocked` to `Locked` mapping), `EncryptedStore::open` |

A change to any function in the table, or to the schema of `segment_keys`,
touches verified code.

## What the harness does

Check 1 (`a_shredded_segment_is_unrecoverable_with_every_remaining_key`)
creates a store in a temp directory with a real dev-fallback root and replays
a random sequence of these operations, checking the invariants after each:

| Op | What it does to the real store |
|---|---|
| `Write { len, expiry }` | `write_segment_with_expiry` with `expires_at` in the past, at the write's own second (the `expires_at <= now` boundary), or one retention window ahead; the ciphertext file is copied aside |
| `ShredExpired` | `crypto_shred_expired`; the report must count exactly the live segments past their bound |
| `ShredAll` | `crypto_shred_all`; the report must count every live segment plus the store key |
| `Reopen` | drop the store and `EncryptedStore::open`; must succeed unless delete-all ran, then must be `Sealed` |
| `Recreate` | after delete-all, `create` under a fresh root; otherwise `create` must refuse with `Locked` or `AlreadyExists` |

"Keys remaining in the store" means every non-NULL `segment_keys.wrapped_key`
row the live connection returns, plus every such row a copy of `store.db`
taken without its WAL returns under the store key, each unwrapped with the
store key. Every one of them is tried against every shredded segment's saved
ciphertext.

Check 2 (`at_most_one_process_holds_the_store_at_a_time`) creates a store,
then spawns the test binary two to five times as contending processes
(`contend_for_store_for_parent`). Each contender sleeps a random delay, then
one to three times: retries `EncryptedStore::open` while it returns `Locked`,
writes a witness file, logs `enter`, writes a segment, sleeps the hold time,
logs `exit`, removes the witness, drops the store. The parent meanwhile
unlinks `writer.lock` on a random cadence of 1 to 8 ms, or never. The check
fails if any contender found the witness present on entry, if two logged hold
intervals overlap, if any contender exited non-zero, or if a contender held
the store a different number of times than planned.

## Bounds and assumptions

| Bound | Value | Why |
|---|---|---|
| Check 1 cases | 48 (`SHRED_CASES`) | each case creates a keychain root and a store; 48 runs in about 5 s on a laptop |
| Ops per case | 1 to 12 (`MAX_OPS`) | every reachable state needs at most a write, a shred and a reopen or recreate; 12 leaves room for several of each |
| Segments per case | at most 12 | one per write; bounded by the ops |
| Segment sizes | 0 to 512 bytes, one chunk minus or plus one byte, two chunks and seven bytes | the empty, the one-chunk boundary and the multi-chunk cases of the 64 KiB stream |
| Check 2 cases | 10 (`RACE_CASES`) | each case spawns processes; ten runs in about 4 s |
| Contenders | 2 to 5 (`MAX_CONTENDERS`) | two is the minimal race; five covers a queue of waiters |
| Acquisitions per contender | 1 to 3 | re-acquisition after a release, with the lock file possibly recreated |
| Hold | 5 to 30 ms | long enough that a second admission falls inside the hold |
| Child deadline | 60 s to acquire | a contender that never gets the store is a liveness failure |

`PROPTEST_CASES=<n>` raises either count for a longer run.

Assumptions: the dev-fallback custody (macOS login keychain on this machine)
is available to the test process and its children; the checks run on one
machine, so the contenders' `SystemTime` clocks are comparable; proptest's
shrinking makes a reported counterexample minimal but not unique. The checks
do not run the Secure Enclave or DPAPI custodies.

## Command

```sh
cargo test -p halvo-capture --lib formal_tests
```

Both checks and the contender stub run; three tests, about 8 s. A single
check: append `::a_shredded_segment` or `::at_most_one`.

## Reading a failure

Proptest prints `minimal failing input:` followed by the shrunk input, then
the assertion message. Replay the input against the code, step by step, in
the words of the code:

- **Check 1.** The input is a list of ops. Walk the list by hand through
  `write_segment_with_expiry`, `crypto_shred_expired`, `crypto_shred_all`,
  `open` and `create`, tracking which `segment_keys` rows are non-NULL. The
  message names the segment and the invariant: "decrypts with a key remaining
  in the store" means a non-NULL row's key opened the saved ciphertext;
  "reads as ... instead of Sealed" means `read_segment` found a key; "still
  has its file" means `remove_keyless_files` left it; "does not read" or
  "read back different bytes" means a live segment was damaged; the
  delete-all messages name the file left behind or the retained key that
  still unwraps.
- **Check 2.** The input is the contender plans, the hold and the unlink
  cadence. "found another contender inside the store" or "held the store at
  the same time" means two processes were admitted at once. "contender N
  exited with exit status: 101" with `database is locked` on stderr means a
  second process reached the database while the first held it, and SQLite
  refused one of them mid-operation: both were inside. "never got the store"
  means the lock was never released to a waiter.

A step that cannot happen in the code marks a modeling error in the harness:
for example an op the harness applies to a state the daemon can never reach.
If every step can happen, it is a bug in the store. Do not weaken the
property or the harness; report the counterexample.

## Break proofs

Each check has been seen to fail on a broken store. The patches and failing
output are in the run evidence
(`~/.wstack/runs/20261108-halvo-r1-capture/evidence/formal-verification-1/`):

| Patch | What it breaks | Result |
|---|---|---|
| `break-crypto-shred.patch` | `destroy_segment_keys` no longer sets `wrapped_key` to NULL | check 1 FAILS: `[Write { len: 0, expiry: Past }, ShredExpired]`, "decrypts with a key remaining in the store" |
| `break-single-writer.patch` | `Db::open` no longer sets `locking_mode = EXCLUSIVE` | check 2 FAILS: two contenders, `writer.lock` unlinked every 1 ms |
| `break-single-writer-both-locks.patch` | the above plus `WriterLock::take` never locks | check 2 FAILS with the lock file left alone |
| `break-single-writer-flock-only.patch` | `WriterLock::take` never locks, exclusive mode kept | check 2 still PASSES |

Which message the two failing rows print depends on timing: "a contender found
another contender inside the store" when the second process reaches the
witness, or "contender N exited with exit status: 101" with `database is
locked` on stderr when SQLite refuses one of the two mid-operation first.
Both were seen for both patches across runs; either is the property failing.

The last row is the point of the check: the file lock only serializes
`create`; the single-writer guarantee is SQLite's exclusive lock, which
unlinking `writer.lock` cannot release. A change that removes the file lock
alone does not fail the check, and is not a safety regression.

## Updating the verification

When a change to covered code is intended:

1. Run the command. A pass with no harness change means the properties still
   describe the code.
2. If the store's operations change shape (a new operation, a new key table,
   a new lock), add the op to `Op` and its effect to `Harness::apply` and
   `Harness::check`, or extend the contender, so one step in the harness is
   still one step in the code.
3. Re-run a break proof: apply one of the patches above (or a new one for the
   new code), confirm the check fails, revert, confirm `git status` is clean.
4. Move the sync commit below to the new commit only after steps 1 to 3 pass.

If the property itself is meant to change (for example, a shredded segment
may stay readable until a later sweep), that is a product decision; record
it in the plan before weakening an assertion.

## Not covered

- The Secure Enclave and DPAPI custodies; only the dev-fallback root runs.
- A process crash between `destroy_segment_keys`'s commit and its checkpoint
  (verifier-store-4's `V4-CRASHWINDOW`): the harness never kills a process
  mid-shred.
- Keys in freed database pages or filesystem blocks; the harness reads keys
  through SQLite, so `secure_delete` is assumed, not checked, here.
- Two writers in different user accounts, or over a network filesystem.

## Sync commit

`478fbb542b5eb02b5fd79b442ccf8c0c0f7df5e8` (main, "R1 P2: encrypted on-device
store and key custody, hardened by adversarial review (#5)"). Both checks
pass at this commit and both break proofs fail. Outcome: in sync.
