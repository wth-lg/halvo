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
which are the two mechanisms that refuse a second writer. A loom model would prove
only that a stand-in lock is a lock. The process-level test runs the real
mechanisms instead.

## Covered code

| Check | File | Functions |
|---|---|---|
| 1 | `crates/halvo-capture/src/store/expiry.rs` | `segment_ids_with_keys`, `destroy_segment_keys`, `remove_keyless_files`, `shred_segments`, `destroy_store_key`, `crypto_shred_expired`, `crypto_shred_all` |
| 1 | `crates/halvo-capture/src/store/db.rs` | `Db::open` (the `secure_delete`, `locking_mode`, `journal_mode` pragmas), `Db::checkpoint_truncate`, `reserve_segment_key`, `commit_segment_key`, `KeyFile::destroy`, `Db::destroy`, `EncryptedStore::{create, open, seal, read_segment, write_segment_with_expiry}`, `WriterLock`'s `Drop` (the `Reopen` and `Recreate` ops call `open` or `create` right after a drop) |
| 1 | `crates/halvo-capture/src/store/segment.rs` | `wrap_segment_key`, `unwrap_segment_key`, `write_segment_file`, `read_segment_file`, `decrypt_segment_bytes`, `segment_files`, `remove_segment_file` |
| 1 | `crates/halvo-capture/src/custody/dev_fallback.rs` | `DevFallbackCustody::{wrap_store_key, unwrap_store_key, destroy_root}`, `aead_seal`, `aead_open` |
| 2 | `crates/halvo-capture/src/store/db.rs` | `WriterLock::take`, `WriterLock`'s `Drop`, `Db::open` (the `locking_mode = EXCLUSIVE` pragma and `require_exclusive_locking`), `verify_key` (its `DatabaseBusy`/`DatabaseLocked` to `Locked` mapping), `EncryptedStore::open` |

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
| `Recreate` | after delete-all, drop the sealed store and `create` under a fresh root; otherwise `create` must refuse with `Locked` or `AlreadyExists` |

`Reopen`, and `Recreate` after delete-all, call `open` or `create` right
after they drop the store, so both need the drop to have released the file
lock. `WriterLock`'s `Drop` unlocks `writer.lock` before its descriptor
closes. On macOS and Linux the lock belongs to the open file, and a child
process shares that file from its spawn until its exec, so a lock that only
the close released would stay held for that time. Check 2 spawns its
contenders from the same test process. Before the unlock, check 1 failed
that way on the macOS CI job: "reopen failed: another writer holds the
store".

"Keys remaining in the store" means every non-NULL `segment_keys.wrapped_key`
row the live connection returns, plus every such row a copy of `store.db`
taken without its WAL returns under the store key, each unwrapped with the
store key. Every one of them is tried against every shredded segment's saved
ciphertext.

Check 2 (`at_most_one_process_holds_the_store_at_a_time`) creates a store
and drops it, then spawns the test binary two to five times as contending
processes
(`contend_for_store_for_parent`). Each contender sleeps a random delay, then
one to three times: retries `EncryptedStore::open` while the platform refuses it
(see the platform note below), writes a witness file, logs `enter`, writes a segment, sleeps the hold time,
logs `exit`, removes the witness, drops the store. The parent meanwhile
unlinks `writer.lock` on a random cadence of 1 to 8 ms, or never. The check
fails if any contender found the witness present on entry, if two logged hold
intervals overlap, if any contender exited non-zero, or if a contender held
the store a different number of times than planned.

## How each platform refuses a second writer

The contender retries `open` on any error until its deadline, because the
refusal reaches it in a platform-specific shape:

| Platform | Refusal a losing contender sees | Where it comes from |
|---|---|---|
| macOS, Linux | `Locked` | `WriterLock::take` gets `WouldBlock` from the file lock, or `verify_key` gets `SQLITE_BUSY` from the holder's exclusive database lock once `writer.lock` has been unlinked |
| Windows | `Locked`, or `Io("Access is denied. (os error 5)")` | the same two paths, plus: unlinking `writer.lock` while the holder keeps it open leaves the name delete-pending, and Windows refuses every open of that path until the holder closes it |

A refusal of either shape is the single-writer property holding, not a
failure, so the contender keeps retrying and the check stays the same on both
platforms: it fails only if a second process is admitted (the witness or an
overlapping interval), if an admitted writer cannot write, if a contender never
gets in before its deadline, or if one holds the store fewer times than
planned. The deadline message names the last refusal, so a misconfigured
custody or a lock that is never released reads as what it is. Both checks run
in CI on the macOS and Windows jobs; the break proofs below were run on macOS.

A refusal ends when the holder drops the store. The database connection
closes first, which releases the exclusive database lock. Then `WriterLock`'s
`Drop` unlocks `writer.lock`, and its descriptor closes. An `open` or
`create` that fails after it took the lock releases it the same way before
it returns. Check 2 cannot tell the unlock from the close. A contender
spawns no child, so nothing else shares its descriptor.

## Bounds and assumptions

| Bound | Value | Why |
|---|---|---|
| Check 1 cases | 48 (`SHRED_CASES`) | each case creates a keychain root and a store; 48 runs in about 5 s on a laptop |
| Ops per case | 1 to 12 (`MAX_OPS`) | every reachable state needs at most a write, a shred and a reopen or recreate; 12 leaves room for several of each |
| Segments per case | at most 12 | one per write; bounded by the ops |
| Segment sizes | 0 to 512 bytes, one chunk and one byte either side of it, two chunks and seven bytes | the empty, the one-chunk boundary and the multi-chunk cases of the 64 KiB stream |
| Check 2 cases | 10 (`RACE_CASES`) | each case spawns processes; ten runs in about 4 s |
| Contenders | 2 to 5 (`MAX_CONTENDERS`) | two is the minimal race; five covers a queue of waiters |
| Acquisitions per contender | 1 to 3 | re-acquisition after a release, with the lock file possibly recreated |
| Hold | 5 to 30 ms | long enough that a second admission falls inside the hold |
| Child deadline | 60 s per contender process, for all of its acquisitions | a contender that never gets the store is a liveness failure |

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
  still unwraps. "reopen failed: another writer holds the store", or a panic
  "create on a free directory: Locked", means that a lock outlived the store
  that the op had just dropped: the file lock if `WriterLock`'s `Drop` did
  not release it, or the database lock.
- **Check 2.** The input is the contender plans, the hold and the unlink
  cadence. A contender that is refused and retries is not a failure. "found another contender inside the store" or "held the store at
  the same time" means two processes were admitted at once. "contender N
  exited with exit status: 101" with `database is locked` on stderr means a
  second process reached the database while the first held it, and SQLite
  refused one of them mid-operation: both were inside. "never got the store; last
  refusal: ..." means the lock was never released to a waiter, or the named
  refusal was not a lock at all.

A step that cannot happen in the code marks a modeling error in the harness:
for example an op the harness applies to a state the daemon can never reach.
If every step can happen, it is a bug in the store. Do not weaken the
property or the harness; report the counterexample.

## Break proofs

Each check has been seen to fail on a broken store. The patches below apply
at the sync commit. They and their output are in the run evidence
(`~/.wstack/runs/20261109-halvo-fix-writer-lock/evidence/formal-verification-1/`).
The patches `mutation-unlock-at-once.patch`, `m7-success-only.patch` and
`m8-store-drop.patch` and their output are in
`~/.wstack/runs/20261109-halvo-fix-writer-lock/evidence/builder-3/`. They
apply to the `db.rs` whose blob is in the table under Sync commit. That file
has the two tests of `db::tests` that their rows name, and the sync commit
does not. Every result below that names a test of `db::tests` is from that
file. The reviewer's and the verifier's runs of this branch are in
`~/.wstack/runs/20261109-halvo-fix-writer-lock/evidence/pr-review-2/` and
`verifier-2/` beside it.
The first proofs, taken at `478fbb5`, are in
`~/.wstack/runs/20261108-halvo-r1-capture/evidence/formal-verification-1/`.
The two patches there that change `WriterLock::take` no longer apply, because
one of their context lines has changed since.

To rerun a row, run `git apply <patch>`, then the check that the row names
(`cargo test -p halvo-capture --lib formal_tests::a_shredded_segment` or
`formal_tests::at_most_one`, and for the rows of
`mutation-unlock-at-once.patch`, `m7-success-only.patch` and
`m8-store-drop.patch`
`cargo test -p halvo-capture --lib <the test that the row names>`), then
`git apply -R <patch>`, and confirm that `git status --porcelain` prints
nothing. The same command runs a test that another row names. `mutations.sh`
in the `builder-3` directory runs those three patches. It applies each
patch, runs the two tests of `db::tests` alone and then the whole lib suite,
and reverts the patch. Before the first patch it stops when a file under
`crates/` is changed. After each revert it prints the number of changed
files under `crates/` and does not check it: it goes on when the number is
not 0, and it does not look at the rest of the tree. In the saved run the
number was 0 after each of the three patches (`mutations-summary.txt`).

| Patch | What it breaks | Result |
|---|---|---|
| `break-crypto-shred.patch` | `destroy_segment_keys` no longer sets `wrapped_key` to NULL | check 1 FAILS: `[Write { len: 0, expiry: Past }, ShredExpired]`, "decrypts with a key remaining in the store" |
| `break-single-writer.patch` | `Db::open` no longer sets `locking_mode = EXCLUSIVE` | check 2 FAILS: two contenders, `writer.lock` unlinked every 1 ms |
| `break-single-writer-both-locks.patch` | the above plus `WriterLock::take` never locks | check 2 FAILS with the lock file left alone. `db::tests::a_second_take_of_the_writer_lock_is_refused_until_the_first_is_dropped` FAILS |
| `break-single-writer-flock-only.patch` | `WriterLock::take` never locks, exclusive mode kept | check 2 still PASSES. `db::tests::a_second_take_of_the_writer_lock_is_refused_until_the_first_is_dropped` FAILS |
| `break-writer-lock-outlives-store.patch` | `WriterLock`'s `Drop` does not unlock and leaks a duplicate of its descriptor, so the lock outlives the dropped store | check 1 FAILS: "reopen failed: another writer holds the store". Check 2 FAILS with the lock file left alone: "never got the store; last refusal: another writer holds the store". `db::tests::a_second_take_of_the_writer_lock_is_refused_until_the_first_is_dropped` and `db::tests::a_dropped_writer_lock_is_released_while_a_duplicate_of_its_descriptor_stays_open` FAIL |
| `break-writer-lock-unlock.patch` | `WriterLock`'s `Drop` does not unlock, so only the close releases the lock | check 2 PASSES. Check 1 PASSES unless a drop and the next `open` or `create` both fall between a child's spawn and its exec: at `d7b245a`, which has no unlock, check 1 failed in 4 of 180 runs and check 2 in 0 of 180. `hardening_tests::a_dropped_store_releases_the_writer_lock_while_a_duplicate_of_its_descriptor_stays_open`, `hardening_tests::no_open_is_refused_as_locked_while_other_threads_spawn_children` and `db::tests::a_dropped_writer_lock_is_released_while_a_duplicate_of_its_descriptor_stays_open` FAIL |
| `mutation-unlock-at-once.patch` | `WriterLock::take` unlocks the file right after it locks it, so no store holds the file lock | both checks PASS. `db::tests::a_second_take_of_the_writer_lock_is_refused_until_the_first_is_dropped` FAILS |
| `m7-success-only.patch`, `m8-store-drop.patch` | only a lock that reached a store is unlocked. In m7 the unlock stays in `WriterLock`'s `Drop`, behind a flag that `open` and `create` set when they build the store that they return. In m8 the unlock is in a `Drop` of the store | both checks PASS in every run taken with each patch. The paragraph on where the unlock is lists the runs: with each patch four runs of both checks on the `db.rs` of the blob table, at the sync commit 21 runs of the lib suite with m7 and 8 with m8, and with m7 more runs of check 1 beside spawned children. Check 1 can fail when a child is spawned while an `open` that fails holds its lock, and the child has not reached its exec at the next `open` or `create`. No run has shown that failure of check 1. A probe outside check 1 has shown by run that the `open` after a failed `open` can be refused as `Locked`, and the paragraph gives its counts. `db::tests::a_dropped_writer_lock_is_released_while_a_duplicate_of_its_descriptor_stays_open` FAILS |

Which message the two failing rows of check 2 print depends on timing: "a
contender found another contender inside the store" when the second process
reaches the witness, or "contender N exited with exit status: 101" with
`database is locked` on stderr when SQLite refuses one of the two
mid-operation first. Both were seen for both patches across runs at
`478fbb5`; the runs at the sync commit printed the first. Either is the
property failing. So is a panic "unknown race log event", which one run at
the sync commit printed while shrinking: two contenders inside the store
wrote the race log at the same moment.

The `flock-only` row is the point of check 2: the file lock only serializes
`create`; the single-writer guarantee is SQLite's exclusive lock, which
unlinking `writer.lock` cannot release. A change that removes the file lock
alone does not fail the check, and is not a safety regression. It does fail
a test of the lib suite.
`db::tests::a_second_take_of_the_writer_lock_is_refused_until_the_first_is_dropped`
fails for `break-single-writer-flock-only.patch` and for
`break-single-writer-both-locks.patch`, because a second take beside a live
lock answers `Ok`. For `break-writer-lock-outlives-store.patch` that test
and
`db::tests::a_dropped_writer_lock_is_released_while_a_duplicate_of_its_descriptor_stays_open`
fail, because the take after the drop is still refused. In the reviewer's
one run of the lib suite with the `flock-only` patch,
`db::tests::a_second_take_of_the_writer_lock_is_refused_until_the_first_is_dropped`
failed and the other 102 tests that ran passed, check 2 among them
(`02-mutations.log` in `pr-review-2`). On macOS one more test of the binary,
`hardening_tests::peak_rss_stays_bounded_for_a_512_mib_segment`, is ignored
and does not run in the lib suite. With each of the other two patches the
reviewer ran the two tests alone, once (`13-extra-mutants.log`).

The two `break-writer-lock` rows are about the release of the file lock,
which the `Reopen` and `Recreate` ops of check 1 need. A lock that outlives
its dropped store makes `open` and `create` answer `Locked` with no other
writer, and check 1 fails. Check 2 fails too when the lock file is left
alone: the parent's dropped store keeps the lock, and no contender gets in
before its deadline.
Run that row for check 2 with `PROPTEST_MAX_SHRINK_ITERS=0`, because each
failing case waits 60 s. When only the unlock is missing, check 1 fails in
few runs, because the close still releases the lock unless a child shares
the descriptor at that moment. At `d7b245a`, which has no unlock, check 1 failed
in 4 of 180 runs of the command on a 16-core Mac under load. At the sync
commit it failed in 0 of 180. Three tests outside the formal checks test the
unlock. Two of them failed in every run without the unlock:
`hardening_tests::a_dropped_store_releases_the_writer_lock_while_a_duplicate_of_its_descriptor_stays_open`
and `hardening_tests::no_open_is_refused_as_locked_while_other_threads_spawn_children`.
`break-writer-lock-unlock.txt` holds one run of both beside the formal
checks, and its second line is the command. The third is
`db::tests::a_dropped_writer_lock_is_released_while_a_duplicate_of_its_descriptor_stays_open`,
also on Unix only. It has no timing in it, because it spawns nothing and
waits for nothing. In the reviewer's one run of the lib suite with
`break-writer-lock-unlock.patch`, these three tests failed and the other 100
passed, both formal checks among them (`02-mutations.log` in `pr-review-2`).

The rows of `mutation-unlock-at-once.patch` and of `m7-success-only.patch`
and `m8-store-drop.patch` are about where the unlock is. No formal check
failed for them in the runs taken. With each of the three patches, on the
`db.rs` of the blob table, the builder and the reviewer each ran the lib
suite once and the verifier ran the two checks once (`mutations-summary.txt`
in `builder-3`, `02-mutations.log` in `pr-review-2`, and the
`m-*-4-formal-checks.txt` files in `verifier-2`). A second verifier ran both
checks once more with m7 and with m8 on that file (`p5-m7-tests-lines.txt`
and `p6-m8-tests-lines.txt` in `verifier-3`). The first and the third
adversary of this branch's adversarial review ran the patches earlier, on a
`db.rs` without the two tests of `db::tests` (`interrogate-1` and
`interrogate-3` in the same evidence directory). The first ran the lib suite
twice with `mutation-unlock-at-once.patch`, and both checks passed in both
runs. The third ran m7 and m8 at the sync commit: the lib suite 21 times
with m7 and 8 times with m8, and check 1 beside a test that spawns children
from four threads 8 times with m7. All 37 of those runs passed (`extra.tsv`,
`survivors.tsv` and `survivors-run1.tsv` in `interrogate-3`, the m7 and m8
`whole-suite` rows of `logs/rerun/runs.tsv`, and line 109 of
`logs/final-chain.log`, which is all that is saved of one run of the lib
suite with m7). With m7 or m8, check 1 can still fail in one case, which
this paragraph gives further on. With the unlock right after the take, the
database lock still refuses a second writer. With the unlock only for a lock
that reached a store, a lock that reached none is closed without an unlock,
and the close releases it unless something shares its descriptor. One test
of `db::tests` fails
for each row. The first requires `Locked` from a second `WriterLock::take`
in a directory with no database, where only the file lock can give it. The
second, on Unix only, drops a `WriterLock` that never reached a store while
a duplicate of its descriptor is open, and requires that the next take
succeeds. That is the lock of an `open` or `create` that fails: the lock is
a local of both until they return the store. Check 1 drives that failure
through `open` itself. After delete-all, its `Reopen` op calls an `open`
that takes the lock and then fails as `Sealed`, and the next `Reopen` or
`Recreate` takes the lock again. So check 1 can catch a lock that a failed
`open` never releases. It does not catch one in every run, because it draws
its cases at random. With a patch that leaks the lock on the error returns
of `open`, check 1 failed in the verifier's one run with "reopen failed:
another writer holds the store" (`s-hard-2-check1.txt` in `verifier-2`).
With the same patch, check 1 failed in 8 of the second verifier's 11 runs:
in 8 of 10 runs alone, and not in the one run beside four other tests
(`r1-s-hard-check1-x10.txt` and `q2-s-hard-tests-lines.txt` in
`verifier-3`). With the same patch and a fixed seed the result was the same
in every run of that seed, 6 seeds failing twice each and 2 seeds passing
three times each (`r1-leak-patch-seeds.txt`, `r5-leak-patch-seeds-17-40.txt`
and `r6-leak-patch-passing-seeds-again.txt` in `verifier-4`).
Check 1 does not catch a lock that a failed `open` closes without the
unlock, as m7 and m8 do, because the close releases the lock. The exception
is that of the `break-writer-lock-unlock.patch` row. If a child is spawned
while the failing `open` holds the lock, and has not reached its exec when
the next `Reopen` or `Recreate` runs, that op gets `Locked` and check 1
fails. No run has shown this failure. The reviewer ran check 1 16 times in
one process with m7, beside four threads that spawned 669,458 children, and
it passed each time (`14-check1-beside-spawns.log`). Check 1 also passed in
the third adversary's 8 runs beside spawned children, which are counted
above. A run has shown the mechanism outside check 1. A probe of that
adversary, which is not in the repository, calls an `open` that fails as
`Sealed` again and again for 1.5 s beside four threads that spawn children,
and counts the opens that are refused as `Locked`. In the first of the three
runs of its matrix with each binary, 7,634 of 83,096 opens were refused with
m7 and 7,398 of 84,372 with m8, and 0 of 92,756 at the sync commit without a
patch
(`matrix.tsv` in `interrogate-3`, lines 54, 61 and 5; the probe is in
`i3_probe_tests.rs`).
The second of the two tests of `db::tests` catches m7 and m8 because they
skip the unlock for every lock that never reached a store, and the test
drops such a lock itself. Apart from check 1 in the case of the spawned
child above, no test
catches a patch that skips the unlock only on the error returns of `open`
and `create` and leaves a bare `WriterLock` alone. With such a patch the lib
suite passed 103 tests of 103 in one run of each verifier
(`s-soft-2-lib-suite.txt` in `verifier-2` and `q1-s-soft-tests-lines.txt` in
`verifier-3`). A test at the level of the store that holds a duplicate of a
store's descriptor and spawns no child does not tell m7 or m8 from the fix
either. The sketch
`sketch_an_open_that_fails_beside_a_duplicate_of_a_store_descriptor` holds a
duplicate of a dropped store's descriptor open, calls an `open` that must
fail as `Sealed`, and requires that the `open` after it is not refused as
`Locked`. It passed without a patch, with m7 and with m8, in one run each of
the builder and of the reviewer (`sketch-store-level.patch` and the
`sketch-*.log` files in the `builder-3` directory, and `04-sketch.log` in
`pr-review-2`), because the failing `open` locks an open file of its own.

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
- The unlock call in `WriterLock`'s `Drop`, as distinct from the release of
  the lock. Check 1 fails when the lock outlives the store, and check 2
  fails when a case leaves the lock file alone. Only check 1 notices a
  missing unlock, and only when a drop and the next `open` or `create` both
  fall between a child's spawn and its exec. Three tests named under the
  break proofs test the unlock itself, on Unix only: the two
  `hardening_tests`, and
  `db::tests::a_dropped_writer_lock_is_released_while_a_duplicate_of_its_descriptor_stays_open`,
  which has no timing in it.
- A forked child that runs destructors and drops its copy of the store:
  `WriterLock`'s `Drop` then unlocks the lock file that its parent still
  holds, and the tree has no fork without an exec.

## Sync commit

`46b69b1c2743df14ee8b6d3b02a35f76cc59399c` (branch `halvo-fix-writer-lock`,
"Release the store's writer lock explicitly so a spawned child cannot keep it
held"). Both checks pass at this commit on macOS, where this pass ran them,
and each of the six `break-*` patches, the break proofs of the formal
checks, gives the result that its row states for the checks. The rows of
`mutation-unlock-at-once.patch` and of `m7-success-only.patch` and
`m8-store-drop.patch` give their results on the `db.rs` whose blob is in the
table below, except the runs that those rows and their paragraph give as
taken at the sync commit or on a `db.rs` without the two tests, and so does
each result in another row that names a test of
`db::tests`, because the sync commit does not have those tests. Outcome:
updated, for both checks. The sync commit before this one was
`478fbb542b5eb02b5fd79b442ccf8c0c0f7df5e8`.

A squash merge gives this change another SHA on main. The covered functions
and the harness are unchanged since the sync commit for as long as
`git rev-parse <commit>:<path>` prints these blob ids. The blob of `db.rs`
is the sync commit's file plus the two tests of `db::tests` that the
break-proof rows of `mutation-unlock-at-once.patch` and of
`m7-success-only.patch` and `m8-store-drop.patch` name, and nothing else:

| File | Blob |
|---|---|
| `crates/halvo-capture/src/store/expiry.rs` | `93aaef99dfc743d4ba644d0b088bdd09eeccb428` |
| `crates/halvo-capture/src/store/db.rs` | `0f4912c0556d6693783ffcf8a36a01add19324f2` |
| `crates/halvo-capture/src/store/segment.rs` | `efb61ac9736a2e6e9a636233c8c22fe72aaf96ed` |
| `crates/halvo-capture/src/custody/dev_fallback.rs` | `3a918db8156d23bb37f3a33a804d372ca95aa55b` |
| `crates/halvo-capture/src/store/formal_tests.rs` | `7b556421010ee8a3316bd793311b13766a447db6` |
