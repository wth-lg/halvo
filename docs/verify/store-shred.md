---
name: verify-store-shred
description: Run and maintain the formal checks of the encrypted store's two safety properties, crypto-shred non-decryptability and the single writer, when a change touches crates/halvo-capture/src/store/, crates/halvo-capture/src/custody/ or crates/halvo-sqlcipher/.
---

# Verify: store crypto-shred and single writer

Six tests check two properties of the real `EncryptedStore`. Three of them
run the store under random inputs and fail on the first input that breaks a
property. Two run the file scan of check 1 on fixed inputs of 1,200
segments, and one makes every move that fails or is refused in a fixed
order. They live in `crates/halvo-capture/src/store/formal_tests.rs`,
compile only under `cfg(test)`, and need only the `proptest` dev-dependency.

## When to run it

Run the checks when a change touches `crates/halvo-capture/src/store/`,
`crates/halvo-capture/src/custody/` or `crates/halvo-sqlcipher/`. The crate
`halvo-sqlcipher` builds SQLCipher for the store: the page layout, the AES
code and the entropy hook that the store and the page reader of check 1
link come from it. A change elsewhere in the workspace does not need them.

## The properties

1. **Crypto-shred non-decryptability.** Once `crypto_shred_expired` or
   `crypto_shred_all` has shredded a segment, no key remaining in the store
   decrypts that segment's ciphertext, the wrapped key that the store held
   for it is in no page of `store.db`, no frame of `store.db-wal` and no
   record of `store.db-journal`, the segment reads as `Sealed`, and its
   file is gone. Every segment that was not shredded still reads back its
   bytes. After delete-all the directory holds no store, and the unwrap of
   the wrapped store key retained from before fails because its root is
   missing.
   A move that fails, or that the store refuses, leaves no half store. A
   create that fails after it made its database leaves no file and no
   root. A create under a root that exists is refused, leaves no file, and
   leaves that root to the store that has it. A create whose root step
   fails, whatever the custody answers, leaves no file and destroys no
   root, because it has made none. A delete-all that stops after
   it destroyed the root leaves a store that the next delete-all on the
   same handle finishes. An append whose index row fails leaves no event
   row, and a shred removes the event rows of its segment.
   Harm if broken: data the owner believes destroyed is still recoverable
   from the store by anyone who later obtains the store key. A create that
   takes or destroys the root of another store loses that store. A
   directory that a failed move left half made takes no new store.
2. **Single writer.** At most one process holds the store at a time, even
   while `writer.lock` is unlinked under the running processes. Every
   contender that waits eventually gets the store, holds it, writes a segment,
   and exits clean.
   Harm if broken: two writers corrupt the metadata database, or a reader
   keeps a snapshot alive that preserves a shredded key.

## The checks

| Check | Test of `store::formal_tests` |
|---|---|
| 1 | `a_shredded_segment_is_unrecoverable_with_every_remaining_key` |
| 1 | `no_key_of_a_shredded_segment_is_left_when_three_in_four_expire_in_one_pass` |
| 1 | `no_key_of_a_shredded_segment_is_left_when_segments_expire_oldest_first` |
| 1 | `no_failed_or_refused_move_leaves_a_half_store` |
| 1 | `no_failed_or_refused_move_leaves_a_half_store_in_a_fixed_order` |
| 2 | `at_most_one_process_holds_the_store_at_a_time` |

Check 1 is the first test, over random sequences of at most 12 operations,
its two scaled cases, which have a fixed shape each, and two tests of the
moves that fail or are refused: one over random sequences of at most 12
operations that hold those moves, and one that makes each of them in a
fixed order. Check 2 is one test over random races. `docs/verify/check-covered.sh` reads this table and the
table under Covered code.

## Tool

The three random tests are `proptest` property tests over the real store, not
a model.
A verifier that runs on the code is the faithful tool here: the properties
are about bytes on disk, SQLCipher's locking and the OS file lock, none of
which a hand model would carry. `loom` was considered for property 2 and
rejected: it explores interleavings of its own atomics and mutexes inside one
process, and cannot intercept `File::try_lock` or SQLite's exclusive locking,
which are the two mechanisms that refuse a second writer. A loom model would prove
only that a stand-in lock is a lock. The process-level test runs the real
mechanisms instead.

## Covered code

A line of this table names code through which a check has been seen to
fail. Each name in its last cell is a row of a `mutants.rows` file beside
the code: `sh scripts/r1-gate/mutation-test.sh` applies the row's
substitution to a copy of HEAD and requires that the one test of the row's
verdict fails. `sh docs/verify/check-covered.sh` fails when a line names no
row, or names a row that is missing, not `required`, on another file, or
judged by a test that is not a test of the line's check. Code that the
checks call and that no row fails through stands under Not covered.

| Check | File | Functions | Rows that fail through it |
|---|---|---|---|
| 1 | `crates/halvo-capture/src/store/expiry.rs` | `destroy_segment_keys` (the `UPDATE` that sets a key to NULL, and the checkpoint after its commit), `destroy_store_key`, `delete_events_and_index` (the `DELETE` of the event rows), `crypto_shred_all` (its branch for a handle that delete-all sealed) | `formal-shredded-key-survives`, `formal-key-row-deleted`, `formal-key-row-deleted-oldest-first`, `formal-checkpoint-removed`, `formal-delete-all-destroys-nothing`, `formal-shred-leaves-the-event-rows`, `formal-second-delete-all-is-refused`, `formal-second-delete-all-finishes-nothing` |
| 1 | `crates/halvo-capture/src/store/db.rs` | `Db::open` (the `secure_delete` pragma with `require_secure_delete`, and the `journal_mode` pragma), `Db::checkpoint_truncate`, `EncryptedStore::write_segment_with_expiry` (where it puts the wrapped key), `KeyFile` and `EncryptedStore::write_key_file` (what `store.key` holds), `EncryptedStore::create` (the removal of the database when `Db::open` fails, and the database before the root step), `EncryptedStore::under_a_new_root` (the root step of a create, which calls `create_root`, and where its error goes), `Db::insert_event` (the rollback to its savepoint) | `formal-secure-delete-off`, `formal-journal-mode-not-wal`, `formal-checkpoint-not-verified`, `formal-wrapped-key-in-a-side-file`, `formal-second-wrap-in-another-table`, `formal-raw-store-key-in-key-file`, `formal-failed-create-leaves-the-database`, `formal-create-makes-its-root-first`, `formal-create-takes-an-existing-root`, `formal-refused-create-destroys-the-root`, `formal-create-spares-only-a-root-it-can-read`, `formal-failed-append-leaves-the-event-row` |
| 1 | `crates/halvo-capture/src/custody/dev_fallback.rs` | the macOS root store: `seal`, which `wrap` calls, and `unwrap` (the `aead_seal` and `aead_open` calls under the root), and `make_root` (the random fill of a new root), which `create_root` and `existing_or_new_root` call | `formal-custody-wrap-under-constant-key`, `formal-custody-root-is-all-zero` |
| 2 | `crates/halvo-capture/src/store/db.rs` | `Db::open` (the `locking_mode = EXCLUSIVE` pragma and `require_exclusive_locking`), `verify_key` (the mapping of `DatabaseBusy` to `Locked`) | `formal-single-writer`, `formal-single-writer-both-locks`, `formal-busy-not-mapped-to-locked` |

A change to any function in the table, or to the schema of `segment_keys`,
touches verified code. A change to the text that a row's substitution
matches changes the row in the same pull request: the runner fails a row
whose substitution no longer applies.

## What the harness does

Check 1 (`a_shredded_segment_is_unrecoverable_with_every_remaining_key`)
creates a store in a temp directory with a real dev-fallback root and replays
a random sequence of these operations, checking the invariants after each:

| Op | What it does to the real store |
|---|---|
| `Write { len, expiry }` | `write_segment_with_expiry` with `expires_at` in the past, at the write's own second (the `expires_at <= now` boundary), or one retention window ahead; the ciphertext file is copied aside, and the harness records the wrapped key that the segment's key row holds when the write returns |
| `ShredExpired` | `crypto_shred_expired`; the report must count exactly the live segments past their bound |
| `ShredAll` | `crypto_shred_all`; the report of the first call must count every live segment plus the store key, and a later call on the same handle must return `Ok` with a report that counts nothing |
| `Reopen` | drop the store and `EncryptedStore::open`; must succeed unless delete-all ran, then must be `Sealed` |
| `ReopenUnderAnotherRoot` | drop the store, flip one bit of the root's 32 bytes under the same root id, and `open`, which must answer `Custody(WrapAuthFailed)`; put the bytes back, and `open` must succeed. Nothing after delete-all |
| `Recreate` | after delete-all, drop the sealed store and `create` under a fresh root; otherwise `create` must refuse with `Locked` or `AlreadyExists` |

The second random test (`no_failed_or_refused_move_leaves_a_half_store`)
replays sequences that also hold the moves of the next table, with the same
checks after each. `no_failed_or_refused_move_leaves_a_half_store_in_a_fixed_order`
makes each of them at least once in one list of 24 moves, and fails when a
move of the list does not apply to the state that the moves before it left.

| Op | What it does to the real store |
|---|---|
| `CreateThatFailsAfterItsDatabase` | after a delete-all that finished: `create` under a fresh root with the failpoint `db_open` armed, which fails `Db::open` once after it made `store.db`. The create must return that error, no root with the id of the create may exist, and the directory must hold nothing but `writer.lock` |
| `CreateUnderTheRootOfThisStore` | on a live store: `create` in a second temp directory under the root id of the live store. It must answer `Custody(RootExists)` and leave nothing but `writer.lock` in that directory. Then the harness drops the live store, and `open` must succeed |
| `CreateWhoseRootStepFails` | in any state, five times: `create` in a second temp directory with a stand-in custody under the newest root id, whose `create_root` makes no root and answers `RootLocked`, `RootDenied`, `RootCorrupt`, `Crypto` or `Os` with the code of a file that exists, one answer for each create. Each create must return `Custody` with that answer, must leave nothing but `writer.lock` in its directory, and must not call `destroy_root`, which the stand-in counts and does not pass on |
| `ShredAllThatStopsAfterItsRootStep` | on a live store: `crypto_shred_all` with the failpoint `key_file_destroy` armed, which fails `KeyFile::destroy` once. The call must return that error. From then on the root must be destroyed and every segment must read as `Sealed`; the directory may hold the files of a store; `Recreate` must be refused and `Reopen` must answer `Custody(RootMissing)`. The next `ShredAll` on the same handle must return `Ok` with a report that counts nothing, and after it the directory must hold nothing but `writer.lock` |
| `DestroyTheRootAgain` | after a delete-all, finished or stopped: `destroy_root` of the dev-fallback custody on the root that delete-all destroyed; it must return `Ok` |
| `Append { index_row_fails }` | on a live store with a live segment: `append_event` for the newest live segment. With `index_row_fails` the harness renames the table `events_fts` away for the call and back after it, so that the second of the two inserts fails; the append must return an error |

After every operation on a live store, the three tests that replay
operations count the rows of `events` and of `events_fts` through the
store's connection. Each count must equal the appends that returned `Ok`
for the segments that are live: a failed append leaves no row, and a shred
removes the rows of its segment.

The two failpoints are the store's own (`store::db::failpoint`, compiled
under `cfg(test)`), one shot each and local to the thread. After each of the
two moves the harness requires that the failpoint was reached, so a create
that opens no database, or a delete-all that never comes to its key file,
fails the move.

At every create the harness reads `store.key` and requires that the wrapped
store key does not open under an all-zero key with the root id as
associated data, and that `store.key` does not hold the raw store key in a
form that the at-rest scan reads: as bytes, as hex, as base64 or as a list
of decimal numbers (`store::verify::raw_key_at_rest`, the scan of
`--verify-store`, on a copy of the file; the entry on a copy of the store
key under Not covered gives each form). With `ReopenUnderAnotherRoot` this is what
check 1 tests of the custody wrap: the store key is sealed under the root's
bytes and stands nowhere beside its sealed form, the unwrap fails under
other bytes, and it fails with `RootMissing` once delete-all has destroyed
the root.

`Reopen`, and `Recreate` after delete-all, call `open` or `create` right
after they drop the store, so both need the drop to have released the file
lock. `WriterLock`'s `Drop` unlocks `writer.lock` before its descriptor
closes. On macOS and Linux the lock belongs to the open file, and a child
process shares that file from its spawn until its exec, so a lock that only
the close released would stay held for that time. Check 2 spawns its
contenders from the same test process. Before the unlock, check 1 failed
that way on the macOS CI job: "reopen failed: another writer holds the
store".

After every operation the store directory may hold only the files of a
store: `store.key`, `store.db`, `store.db-wal`, `store.db-shm`,
`store.db-journal`, `writer.lock` and `segments`. Any other name fails the
case, so a file that the check does not read cannot hold a key or a copy of
a segment.

After every operation the check also asks which keys remain, in two ways.

Through SQL: every blob of every table that the live connection returns,
in whatever column it stands, is a key that remains when it unwraps under
the store key, and each such key is tried against the saved ciphertext of
every shredded segment. Every non-NULL `segment_keys.wrapped_key` must
unwrap.

In the store files: the page reader of the store's test support
(`store/test_support/pages.rs`) decrypts every page of `store.db`, every
frame slot of `store.db-wal` and every record of `store.db-journal` with
the store key. The wrapped key that the harness recorded for a shredded
segment must occur in none of them, whole (72 bytes) or as its middle 32
bytes, which are the encrypted segment key without its nonce and its tag.
The recorded key of every live segment must occur in at least one of them,
and the reader fails unless page 1 of `store.db` decrypts to a SQLite
header: a reader that sees nothing fails the check. The scan reads bytes
and does not ask `dbstat`, so a free page counts like any other page.

The reader opens the three files while the store is open. On macOS and
Linux the close of a second descriptor of `store.db` drops the SQLite lock
that the process holds on that file, so after the first scan the harness's
store works without it. Check 1 runs in one process and does not rely on
that lock. The contenders of check 2 never call the reader.

After delete-all the check asks the classifier that `--verify-deletion`
uses, `store::verify::root_state`, with the wrapped store key retained from
the create: the root counts as destroyed only when the unwrap fails with
`RootMissing`. An unwrap that succeeds, or that fails with any other error,
fails the case. Then the store directory must hold nothing but
`writer.lock`, and every segment must read as `Sealed`.

The two scaled cases run the same harness on a fixed input, without the
random operations:

- `no_key_of_a_shredded_segment_is_left_when_three_in_four_expire_in_one_pass`
  writes 1,200 segments of 40 bytes, 900 of them past their bound, and
  calls `crypto_shred_expired` once.
- `no_key_of_a_shredded_segment_is_left_when_segments_expire_oldest_first`
  writes 1,200 segments with one retention for all, and after every third
  write one whose source dies after 100 bytes, which leaves a key row that
  never gets a key. Then it makes 50 passes: the harness moves the expiry
  of the 7 oldest live segments into the past, through the store's own
  connection in place of the clock, calls `crypto_shred_expired` and scans
  the store files.

Both list the store directory, scan the store files and read every segment
before the first pass and
after the last. They skip the trial of every remaining key against every
shredded segment, which the random test makes on its small stores and which
would be 270,000 trials after the one pass.

Check 2 (`at_most_one_process_holds_the_store_at_a_time`) creates a store
and drops it, then spawns the test binary two to five times as contending
processes
(`contend_for_store_for_parent`). Each contender sleeps a random delay, then
one to three times: retries `EncryptedStore::open` while the store refuses a
second writer (see the platform note below), writes a witness file, logs
`enter`, writes a segment, sleeps the hold time, logs `exit`, removes the
witness, drops the store. It logs every refusal with its time. The parent
meanwhile unlinks `writer.lock` on a random cadence of 1 to 8 ms, or never.
A case fails if any contender found the witness present on entry, if two
logged hold intervals overlap, if any contender exited non-zero, or if a
contender held the store a different number of times than planned. The run
fails if, over all its cases, no refusal falls inside the enter-to-exit
interval of another contender.

## How each platform refuses a second writer

The contender takes an error of `open` as a refusal only when
`is_writer_refusal` says so, and retries after it until its deadline:

| Platform | Refusal that the contender accepts | Where it comes from |
|---|---|---|
| macOS, Linux | `Locked` | `WriterLock::take` gets `WouldBlock` from the file lock, or `verify_key` gets `SQLITE_BUSY` from the holder's exclusive database lock once `writer.lock` has been unlinked |
| Windows | `Locked`, or `Io(PermissionDenied)` | the same two paths, plus: unlinking `writer.lock` while the holder keeps it open leaves the name delete-pending, and Windows refuses every open of that path with access denied (os error 5) until the holder closes it |

Any other error ends the contender at once, with the error in its message,
and the case fails with "contender N exited with exit status: 101". So an
`open` that answers a second writer with anything but these refusals fails
the check, on either platform. The check also fails if a second process is
admitted (the witness or an overlapping interval), if an admitted writer
cannot write, if a contender never gets in before its deadline, or if one
holds the store fewer times than planned.

Two contenders can finish a case without meeting: they start 0 to 20 ms
apart and hold the store 5 to 30 ms. A run in which no contender is ever
refused would pass whatever the lock does, so the check counts the
refusals of the whole run and requires one that falls inside another
contender's hold. It asks this of the run and not of each case. On a pass
it writes one line to standard error, past the capture of the test harness,
so the line is in a CI log too:
`check 2 refusals in <cases> cases: locked=<n> access_denied=<n> while_another_contender_held_the_store=<n>`.
Both checks run in CI on the macOS and Windows jobs. The rows under Break
proofs run on macOS.

A refusal ends when the holder drops the store. The database connection
closes first, which releases the exclusive database lock. Then `WriterLock`'s
`Drop` unlocks `writer.lock`, and its descriptor closes. An `open` or
`create` that fails after it took the lock releases it the same way before
it returns. Check 2 cannot tell the unlock from the close. A contender
spawns no child, so nothing else shares its descriptor.

## Bounds and assumptions

| Bound | Value | Why |
|---|---|---|
| Check 1 cases | 48 (`SHRED_CASES`) | each case creates a keychain root and a store; 48 run in about 10 s on a laptop |
| Ops per case | 1 to 12 (`MAX_OPS`) | every reachable state needs at most a write, a shred and a reopen or recreate; 12 leaves room for several of each |
| Segments per case | at most 12 | one per write; bounded by the ops |
| Cases with moves that fail | 32 (`FAILING_MOVE_CASES`), 1 to 12 ops each | the second random test; it interleaves the moves that fail or are refused with the others, and no row depends on a case that it draws |
| The fixed order | 24 moves | each move that fails or is refused at least once, in the state that it needs; the rows of these moves are judged by it |
| Segment sizes | 0 to 512 bytes, one chunk and one byte either side of it, two chunks and seven bytes | the empty, the one-chunk boundary and the multi-chunk cases of the 64 KiB stream |
| Scaled cases, segments | 1,200 of 40 bytes (`SCALED_SEGMENTS`, `SCALED_SEGMENT_LEN`) | the size at which a key row that is deleted, not set to NULL, leaves keys in `store.db`: 3 of 900 after the one pass |
| Oldest first | 7 segments a pass, 50 passes, a failed write after every third write (`OLDEST_FIRST_PER_PASS`, `OLDEST_FIRST_PASSES`, `A_SOURCE_DIES_AFTER_EVERY`) | with deleted key rows a key is left after passes 17 to 22 and again after passes 49 and 50 of 150; the case ends at 50 |
| Check 2 cases | 30 (`RACE_CASES`) | each case spawns processes, and a row of check 2 fails only in a case in which two contenders meet; 30 follows from the measured share of such cases, under Break proofs |
| Contenders | 2 to 5 (`MAX_CONTENDERS`) | two is the minimal race; five covers a queue of waiters |
| Acquisitions per contender | 1 to 3 | re-acquisition after a release, with the lock file possibly recreated |
| Hold | 5 to 30 ms | long enough that a second admission falls inside the hold |
| Child deadline | 60 s per contender process, for all of its acquisitions | a contender that never gets the store is a liveness failure |

`PROPTEST_CASES=<n>` sets either count of cases for one run. Check 2 asks
for a refusal inside another contender's hold only when the run has at
least `RACE_CASES` cases.

A row of check 1 needs a case that holds its pattern, and the random test
draws its cases anew in each run. A simulation of the generator (400,000
cases) gives the share of cases with each pattern and, from it, the chance
that a run of 48 cases holds none: a write that is due, then
`ShredExpired`: 0.31 and 2 in 100 million; a `ShredAll`: 0.43 and 2 in a
million million; a `ReopenUnderAnotherRoot` on a live store: 0.35 and 1 in
a thousand million.

Assumptions: the dev-fallback custody (macOS login keychain on this machine)
is available to the test process and its children; the checks run on one
machine, so the contenders' `SystemTime` clocks are comparable; proptest's
shrinking makes a reported counterexample minimal but not unique. The checks
do not run the Secure Enclave or DPAPI custodies.

## Command

```sh
cargo test -p halvo-capture --lib formal_tests
```

The six tests, the contender stub and two tests of what the harness takes
as a destroyed root run; nine tests. On a 16-core Mac whose 1-minute load was 100 to 400 from other
work, the command took 31 s in one run, and alone the random test of check 1
took 19 s, the one-pass case 12 to 15 s, the oldest-first case 34 to 47 s and
check 2 14 s. At a load near 10 the random test took 10 s and the one-pass
case 11 s. Those times are of the four tests that the command ran before
the moves that fail or are refused had theirs. With the nine tests of
today the command took 77 to 82 s in three runs at a load of 180 to 480,
and the two tests of those moves with the two tests of a destroyed root
took 13 s alone at a load of 342. A single test: append `::` and the start of its
name, for example `::a_shredded_segment` or `::at_most_one`.

## Reading a failure

Proptest prints `minimal failing input:` followed by the shrunk input, then
the assertion message. Replay the input against the code, step by step, in
the words of the code:

- **Check 1.** The input is a list of ops. Walk the list by hand through
  `write_segment_with_expiry`, `crypto_shred_expired`, `crypto_shred_all`,
  `open` and `create`, tracking which `segment_keys` rows are non-NULL and
  which files hold which page. The message names the segment and the
  invariant: "decrypts with a key remaining in the store" means a non-NULL
  row's key opened the saved ciphertext; "is still in the store files" means
  the bytes of the segment's wrapped key are in the named file, in the page,
  frame or record of the page number given; "is in no page of store.db, no
  frame of store.db-wal and no record of store.db-journal" and a panic "page
  1 does not decrypt to a SQLite header" mean that the reader no longer sees
  the store's pages, which is a defect of the harness or a change of the
  page format; "reads as ... instead of Sealed" means `read_segment` found a
  key; "still has its file" means `remove_keyless_files` left it; "does not
  read" or "read back different bytes" means a live segment was damaged;
  "opens under an all-zero key" means that no root seals the store key; "store.key
  of the fresh store holds the raw store key" means what it says; "the
  store directory holds ..., which is no file of a store" means that the
  store wrote a file that the check does not read;
  "with other bytes under the id of its root, the store reopened" means that
  the unwrap does not depend on the root's bytes; the delete-all messages
  say that the root is alive, that its state is unknown with the error of
  the unwrap, or name the file left behind. "reopen failed: another writer
  holds the store", or a panic "create on a free directory: Locked", means
  that a lock outlived the store that the op had just dropped: the file lock
  if `WriterLock`'s `Drop` did not release it, or the database lock.
- **The moves that fail or are refused.** The fixed order prints "at move N
  of 24" and the move in front of the message. "a create that failed left
  ... in the store directory" or "left the root that it made" names what
  the create did not clean up. "a create under the root of another store
  returned ... in place of RootExists" means that a create took a root that
  exists; "after a create under its root was refused, the store did not
  reopen" means that the refused create destroyed or changed that root. "a
  create whose root step answered ... returned ..." names what the create
  returned in place of the custody's answer; "... had made no root and
  called destroy_root N times" means that the clean-up of the create
  destroys a root that the call did not make; "... left ... in its
  directory" names a file that it did not remove. "a
  delete-all whose key file could not be removed returned ..." names what
  the stopped call returned in place of the failpoint's error. After the
  `ShredAll` that follows a stopped one, "a second delete-all did not
  finish with nothing left to shred" names what the second call returned,
  and "delete-all left ... in the store directory" means that it returned
  `Ok` and finished nothing. "destroy_root on a root that delete-all had
  destroyed failed" means that the custody takes a missing root as an
  error. "an append whose index row could not be written returned Ok" and
  "events holds N rows, and M appends returned Ok for the segments that are
  live" mean that an append left one row of two, or that a shred left the
  event rows of its segment.
- **The scaled cases.** They print no input. The message names the segment
  and the place of its key, and the oldest-first case names the pass.
- **Check 2.** The input is the contender plans, the hold and the unlink
  cadence. A contender that is refused and retries is not a failure. "found
  another contender inside the store" or "held the store at the same time"
  means two processes were admitted at once. "contender N exited with exit
  status: 101" comes with the contender's own message on stderr: "open
  failed with ..., which is no refusal of a second writer" names the error
  that `open` gave in place of a refusal, and "write failed with ..." with
  `database is locked` means that a second process reached the database
  while the first held it. "never got the store; last refusal: ..." means
  the lock was never released to a waiter. "no contender was refused while
  another held the store" means that the run saw no contention at all.

A step that cannot happen in the code marks a modeling error in the harness:
for example an op the harness applies to a state the daemon can never reach.
If every step can happen, it is a bug in the store. Do not weaken the
property or the harness; report the counterexample.

## Break proofs

Each check has been seen to fail on a broken store, and each break is a row
of a `mutants.rows` file: `crates/halvo-capture/src/store/mutants.rows` for
store code and `crates/halvo-capture/src/custody/mutants.rows` for custody
code. The name of such a row starts with `formal-`, its verdict is `test`
with one of the six tests of the table under The checks, and its state is
`required`.

```sh
sh scripts/r1-gate/mutation-test.sh --only formal-shredded-key-survives
```

The runner copies HEAD, requires that the test passes unmutated, applies
the substitution, builds and requires that the test fails:
`PASS mutation:<row> (test <name>: the one test failed)`. Without `--only`
it runs every row. A row whose substitution matches nothing is a FAIL, so a
row cannot go stale unseen.

| Row | What it breaks | Test | Message |
|---|---|---|---|
| `formal-shredded-key-survives` | `destroy_segment_keys` writes each key back to itself and sets none to NULL | the random test of check 1 | "shredded segment ... decrypts with a key remaining in the store" |
| `formal-key-row-deleted` | `destroy_segment_keys` deletes the key row, where it must set the key to NULL in place | the one-pass scaled case | "the wrapped key of shredded segment ... is still in the store files", in a page of `store.db` |
| `formal-key-row-deleted-oldest-first` | the same | the oldest-first scaled case | "after pass 17 of 50", then the same |
| `formal-checkpoint-removed` | `destroy_segment_keys` does not checkpoint after its commit | the random test of check 1 | the same, in a frame of `store.db-wal` |
| `formal-checkpoint-not-verified` | `Db::checkpoint_truncate` runs a PASSIVE checkpoint and accepts a WAL that still holds frames | the random test of check 1 | the same, in a frame of `store.db-wal` |
| `formal-journal-mode-not-wal` | `Db::open` sets `journal_mode = DELETE` | the random test of check 1 | the same, in a record of `store.db-journal` |
| `formal-secure-delete-off` | `Db::open` switches `secure_delete` off and does not read it back | the one-pass scaled case | the same, in a page of `store.db` |
| `formal-wrapped-key-in-a-side-file` | `write_segment_with_expiry` also writes the wrapped key into `keys.log` in the store directory | the random test of check 1 | "the store directory holds keys.log, which is no file of a store" |
| `formal-second-wrap-in-another-table` | `write_segment_with_expiry` wraps the segment key a second time and keeps that wrap in a table that no shred clears | the random test of check 1 | "shredded segment ... decrypts with a key remaining in the store" |
| `formal-reader-keyed-with-zeros` | the harness gives the page reader an all-zero key | the random test of check 1 | panic "page 1 does not decrypt to a SQLite header" |
| `formal-raw-store-key-in-key-file` | `store.key` also holds the raw store key, as base64 | the random test of check 1 | "store.key of the fresh store holds the raw store key" |
| `formal-delete-all-destroys-nothing` | `destroy_store_key` returns at once | the random test of check 1 | "after delete-all the root is alive: the retained wrapped store key still unwraps" |
| `formal-custody-wrap-under-constant-key` | the macOS root store seals and opens the store key under an all-zero key, not under the root | the random test of check 1 | "the wrapped store key of a fresh store opens under an all-zero key, so no root seals it" |
| `formal-custody-root-is-all-zero` | a new root is never filled with random bytes | the random test of check 1 | the same |
| `formal-single-writer` | `Db::open` neither sets `locking_mode = EXCLUSIVE` nor requires it | check 2 | "a contender found another contender inside the store", "held the store at the same time", or "contender N exited with exit status: 101" |
| `formal-single-writer-both-locks` | the same, and `WriterLock::take` never locks | check 2 | the same three, or "no contender was refused while another held the store" |
| `formal-busy-not-mapped-to-locked` | `verify_key` does not map `DatabaseBusy` and `DatabaseLocked` to `Locked` | check 2 | "contender N exited with exit status: 101", and on stderr "open failed with Db(5), which is no refusal of a second writer" |
| `formal-failed-create-leaves-the-database` | `create` returns the error of `Db::open` and does not remove the database that the call made | the fixed order | "at move 15 of 24, CreateThatFailsAfterItsDatabase: ... a create that failed left store.db in the store directory" |
| `formal-create-makes-its-root-first` | `create` makes its root with `create_root` before it opens the database, so a failure of `Db::open` leaves that root | the fixed order | "at move 15 of 24, CreateThatFailsAfterItsDatabase: ... a create that failed left the root that it made" |
| `formal-create-takes-an-existing-root` | `create` skips its root step, `under_a_new_root`, so `write_key_file` wraps under a root that exists | the fixed order | "at move 6 of 24, CreateUnderTheRootOfThisStore: ... a create under the root of another store returned None in place of RootExists" |
| `formal-refused-create-destroys-the-root` | `under_a_new_root` sends every error of the root step, `RootExists` among them, to `abandon_create`, which destroys the root | the fixed order | "at move 6 of 24, CreateUnderTheRootOfThisStore: ... after a create under its root was refused, the store did not reopen" |
| `formal-create-spares-only-a-root-it-can-read` | `under_a_new_root` keeps the clean-up that removes the database alone for `RootExists`, and sends every other error of the root step to `abandon_create`, as `create` did before its root step stood alone | the fixed order | "at move 24 of 24, CreateWhoseRootStepFails: ... a create whose root step answered Custody(RootLocked(-25308)) had made no root and called destroy_root 1 times" |
| `formal-failed-append-leaves-the-event-row` | `insert_event` releases its savepoint without the rollback | the fixed order | "at move 3 of 24, Append { index_row_fails: true }: ... events holds 2 rows, and 1 appends returned Ok for the segments that are live" |
| `formal-shred-leaves-the-event-rows` | `delete_events_and_index` deletes the index rows and leaves the event rows | the fixed order | "at move 7 of 24, ShredExpired: ... events holds 2 rows, and 1 appends returned Ok for the segments that are live" |
| `formal-second-delete-all-is-refused` | `crypto_shred_all` answers `Sealed` on a handle that delete-all sealed, and finishes nothing | the fixed order | "at move 12 of 24, ShredAll: ... a second delete-all did not finish with nothing left to shred: Err(Sealed)" |
| `formal-second-delete-all-finishes-nothing` | `crypto_shred_all` returns `Ok` with an empty report on a handle that delete-all sealed, and removes nothing | the fixed order | "at move 12 of 24, ShredAll: ... delete-all left segments in the store directory" |

The shortest failing input of the four rows on the key statement, the
checkpoint and the journal mode is
`[Write { len: 0, expiry: Past }, ShredExpired]`.

Ten rows share their substitution with a `formal-` row of the same file
that has another verdict: `shredded-key-survives`, `key-row-deleted`,
`checkpoint-not-verified`, `raw-store-key-in-key-file`,
`delete-all-destroys-nothing`, `custody-wrap-under-constant-key`,
`custody-root-is-all-zero`, and the three rows of the hardening tests on a
create that destroys a root, `create-destroys-a-root-it-cannot-read`,
`create-destroys-a-root-file-it-cannot-read` and
`create-destroys-the-root-of-a-create-at-the-same-instant`. A row has
one verdict, and the covered table needs the formal check to fail, so each
of these substitutions stands under more than one name: that of
`key-row-deleted` under three, once for each scaled case, and that of the
three rows on a create under four, with
`formal-refused-create-destroys-the-root`.

The rows of check 1 on the random test need a case with their pattern (see
Bounds). The three rows of check 2 depend on timing: a case shows
`formal-single-writer` or `formal-busy-not-mapped-to-locked` only when two
contenders meet while the lock file is unlinked. On the Mac of this pass,
runs of these two rows were given up to 200 cases, so that each run ended
at its first failing case, and the share of failing cases followed the load
of the machine. `formal-single-writer`, with the contender as it was before
this pass: 30 failing cases among 49 at a 1-minute load of 9 to 15. The
same row in 30 pairs of runs at a load of 24 to 128: 30 among 38 with the
contender as it was, 30 among 42 with the contender of today.
`formal-busy-not-mapped-to-locked`, which passed the contender as it was:
30 among 38 at a load of 114 to 402. `RACE_CASES` is 30 for the lowest
share measured, 0.61 a case: 30 such cases all pass with a chance below 1
in a million million, and at half that share once in about 55,000 runs. A
machine on which contenders rarely meet needs more cases, and the line
that check 2 prints on a pass shows it: its last count falls towards 0.

`formal-secure-delete-off` is judged by the one-pass case, because the
random test shows it only in a case with two due writes before one
`ShredExpired`: with one, the store writes the row without its key over a
part of the freed key. A simulation gives such a case a share of 0.14, so 1
run in about 1,300 would hold none.

`formal-reader-keyed-with-zeros` mutates the harness, not the store: it is
the control that the file scan cannot pass blind.

The rows of the moves that fail or are refused are judged by the fixed
order, which draws nothing: each fails at the same move in every run.
`DestroyTheRootAgain` has no row. A mutant of `destroy_root` leaves real
roots in the login keychain of the Mac that runs it, so no row and no probe
mutates it. Its read has a control in place of a row:
`a_destroyed_root_destroys_again_only_when_its_custody_returns_ok` gives
the read a stand-in custody whose `destroy_root` answers `RootMissing` and
requires the message of the failure.

The read of a second delete-all has two rows, because it has two halves:
what the call returns, and what it finishes. The first random test reads
the first half. It reports `formal-second-delete-all-is-refused` in a case
with two `ShredAll` and no `Recreate` between them; three runs with three
seeds each ended at such a case, the shortest `[ShredAll, ShredAll]`. It
passes `formal-second-delete-all-finishes-nothing` (three seeds, 48 cases
each), because in its cases the first delete-all has left nothing to
finish. A second call that returns `Ok` and removes nothing shows only
after a delete-all that stopped, and the fixed order holds that pair at its
moves 9 and 12.

The root step of a create has three rows. `formal-create-takes-an-existing-root`
and `formal-refused-create-destroys-the-root` fail at move 6, where the
custody answers `RootExists` for the root of the live store.
`formal-create-spares-only-a-root-it-can-read` is the store as it stood
before the root step became its own: it spares a root for which the custody
answers `RootExists` and destroys one for which the custody answers
anything else. The fixed order passed that copy while its last move was
move 23 (one run of the third formal step on the changed copy), because
`RootExists` was the only answer that it made the root step give. Move 24
makes it give five others. The stand-in custody of move 24 counts the
calls of `destroy_root` and does not pass them on, so this changed copy
destroys no root of the keychain.

### What patches showed about the file lock, before the rows

The results of this section were taken with patch files, on the branch
`halvo-fix-writer-lock`, with the checks as they stood there: check 1 asked
SQL which keys remain, and the contender of check 2 retried on any error.
In this section "the sync commit" is
`46b69b1c2743df14ee8b6d3b02a35f76cc59399c` of that branch, and "the blob
table" is the table at the end of the section. The patches and their output
are in the evidence of the run 20261109-halvo-fix-writer-lock, in the
directories that the paragraphs name (`builder-3`, `pr-review-2`,
`verifier-2` to `verifier-4`, `interrogate-1`, `interrogate-3`, and the
directory of that run's own formal pass), not in the repository, and no row
runs them: most of them pass both checks, which is what they record.
Three of them are rows now, with the checks of today:
`break-crypto-shred.patch` is `formal-shredded-key-survives` (the patch
removed the statement, the row writes the key back to itself),
`break-single-writer.patch` is `formal-single-writer` and
`break-single-writer-both-locks.patch` is `formal-single-writer-both-locks`.

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

The `flock-only` row is the point of check 2: the file lock serializes
`create` and `open`; the single-writer guarantee is SQLite's exclusive lock, which
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

The results of this section were taken on these blobs. The blob of `db.rs`
is the sync commit's file plus the two tests of `db::tests` that the rows of
`mutation-unlock-at-once.patch` and of `m7-success-only.patch` and
`m8-store-drop.patch` name, and nothing else:

| File | Blob |
|---|---|
| `crates/halvo-capture/src/store/expiry.rs` | `93aaef99dfc743d4ba644d0b088bdd09eeccb428` |
| `crates/halvo-capture/src/store/db.rs` | `0f4912c0556d6693783ffcf8a36a01add19324f2` |
| `crates/halvo-capture/src/store/segment.rs` | `efb61ac9736a2e6e9a636233c8c22fe72aaf96ed` |
| `crates/halvo-capture/src/custody/dev_fallback.rs` | `3a918db8156d23bb37f3a33a804d372ca95aa55b` |
| `crates/halvo-capture/src/store/formal_tests.rs` | `7b556421010ee8a3316bd793311b13766a447db6` |

## Updating the verification

When a change to covered code is intended:

1. Run the command. A pass with no harness change means the properties still
   describe the code.
2. If the store's operations change shape (a new operation, a new key table,
   a new lock, a new file beside `store.db`), add the op to `Op` and its
   effect to `Harness::apply` and `Harness::check`, extend the page reader,
   or extend the contender, so one step in the harness is still one step in
   the code.
3. If the change moves the text that a row's substitution matches, change
   the `formal-` row, and the row of the same substitution with another
   verdict, in the same pull request.
4. Run the rows: `sh scripts/r1-gate/mutation-test.sh --only <row>` for each
   row of the changed file, or every row with no argument. Each must print
   `PASS mutation`. For new covered code, add a row that breaks it, see the
   check fail, and name the row on the code's line of the covered table.
5. Run `sh docs/verify/check-covered.sh`.

If the property itself is meant to change (for example, a shredded segment
may stay readable until a later sweep), that is a product decision; record
it in the plan before weakening an assertion.

## Not covered

- The Secure Enclave and DPAPI custodies; only the dev-fallback root runs.
  On Windows that root is a root file under a DPAPI protector
  (`custody/file_root.rs`, `custody/windows.rs`): check 1 runs there on the
  unmutated code, and no row mutates those two files, because the runner
  runs on macOS.
- A process crash between `destroy_segment_keys`'s commit and its checkpoint
  (verifier-store-4's `V4-CRASHWINDOW`): the harness never kills a process
  mid-shred.
- Blocks that the file system has freed. A truncated WAL, a removed segment
  file and a scrubbed key file give their old blocks back to the file
  system, and the harness reads files, not the volume. Freed pages inside
  `store.db` are covered: the file scan reads every page of the file.
- A restored copy of a root file. Destroying a root scrubs and removes its
  file, and a copy that a backup or a shadow copy kept unprotects again for the
  same user and revives the key of its store.
- A part of a wrapped key that is shorter than its middle 32 bytes. The
  file scan looks for the whole key and for that middle.
- What the event rows of a shredded segment leave in the store files. The
  file scan looks for wrapped keys only; check 1 counts the event rows
  through SQL.
- A delete-all that stops at another step than the removal of its key
  file, or whose `destroy_root` fails. The store has one failpoint in
  delete-all, `key_file_destroy`.
- A delete-all that stopped and whose handle is gone. Check 1 states what
  the store answers then: `open` gives `Custody(RootMissing)` and `create`
  is refused, so no call of this store finishes that directory. The root is
  destroyed, so nothing in it can be read.
- Two creates under one root id at the same instant.
  `CreateUnderTheRootOfThisStore` runs after the first create has returned.
  The macOS root store reads the keychain and then writes it, in two calls,
  and the write replaces an item that exists. A probe of the second formal
  step, which is not in the repository, started two creates under one root
  id behind a barrier, 200 times: in 115 rounds both returned `Ok`, and in
  each of those one of the two stores answered `Custody(WrapAuthFailed)`
  to the `open` that followed; in the other 85 one create answered
  `Custody(RootExists)`. The file-root store, which both Windows custodies
  use, checks and then writes too. When the probe first ran it, over a temp
  directory with the test protector, its two callers wrote one partial
  file: of 500 rounds of two wraps under one new root id, one wrap returned
  `Ok` in every round, and in 473 of them that wrap no longer opened. Since
  then a root write opens its partial file with `create_new`, so a writer
  that meets the partial file of another fails at once and touches
  nothing. The probe on that code, as two callers that each make the root
  and then wrap: of 500 rounds, one caller returned `Ok` in every round and
  its wrap opened afterwards in every round; the other answered `Os` with
  the code of a file that exists in 499 rounds and `RootExists` in 1. What
  no run of the probe showed and nothing in the root write excludes is the
  other shape: a caller whose check passed before the first root existed,
  and whose own write begins after the first writer's rename, replaces that
  root, as the keychain's write does.
  `store::hardening_tests::two_creates_at_one_instant_under_one_root_id_destroy_no_root_that_a_create_made`
  runs two creates of a store behind a barrier and requires that neither
  destroys the root of the other; it counts the other shape and asserts
  nothing on it. Every caller in the tree makes a fresh root id for
  each create, so the race needs a caller that gives one id to two creates.
  To state it, check 1 would need two creators that share a root id and
  start together, and the read that at most one of them returns `Ok` and
  that a store whose create returned `Ok` opens afterwards.
- A create that fails after its root exists, at the write of the key file,
  where `abandon_create` destroys the root that the call made. No move
  makes that failure.
  `store::hardening_tests::a_create_that_fails_after_wrapping_destroys_the_root_it_made`
  tests it.
- What a root store answers for a root that it cannot read.
  `CreateWhoseRootStepFails` gives `create` a custody that answers so. The
  keychain gives none of those states on demand.
  `store::hardening_tests::a_create_under_a_root_of_another_store_that_it_cannot_read_fails_and_leaves_that_root`
  makes four of them on the file-root store.
- A key file that the store did not write. Check 1 damages no `store.key`,
  so it does not read that a wrapped store key of another length than a
  wrap opens as `KeyFileUnparseable`.
  `store::hardening_tests::a_wrapped_store_key_that_decodes_to_another_length_than_a_wrap_opens_as_key_file_unparseable`
  tests it.
- An append that fails at its first row, or whose savepoint cannot be
  released.
- Code that check 1 calls in every case and that no row fails through. A
  change there can fail the check, and no row shows it: in `expiry.rs`
  `segment_ids_with_keys`, `remove_keyless_files`, `shred_segments`,
  `crypto_shred_expired`, and `crypto_shred_all` outside its branch for a
  sealed handle; in `db.rs`
  `reserve_segment_key`, `commit_segment_key`, `KeyFile::destroy`,
  `Db::destroy` and `EncryptedStore::{open, seal, read_segment}`;
  every function of `segment.rs`; `DevFallbackCustody::destroy_root`, which
  no row may mutate (see Break proofs).
- A key that can be computed from bytes that stay. Two such stores pass
  check 1: one whose root is made of the root id, which `store.key` holds,
  and one whose segment keys are made of the store key and the segment id.
  The checks try the keys that the store holds and look for the bytes of
  keys that it held. They cannot try every rule that gives a key back.
- A copy of a key in a form that neither question reads: as text in a
  table, or in a file below `segments`, where the next shred removes every
  file that is no live segment.
- A copy of the store key that is encrypted or transformed. The at-rest
  scan (`store::verify::raw_key_at_rest`, which `--verify-store`, check 1
  and `no_file_of_a_dropped_store_holds_its_raw_store_key` call) finds the
  key as raw bytes, as hex of either case with or without the padding of a
  byte's two digits, as base64 of either alphabet, also inside a longer
  text, and as a list of decimal numbers such as a debug print or a JSON
  array gives. A copy in any other form passes it.
- The header of a segment file as associated data, and its last-chunk
  flag. With either removed, the formal checks pass.
  `store::hardening_tests::every_modified_segment_file_is_rejected_with_no_plaintext`
  tests both (rows `header-not-authenticated` and `last-chunk-flag-dropped`).
- `WriterLock::take`. A store that takes no file lock and keeps the
  exclusive database lock passes check 2, as the `flock-only` patch under
  Break proofs shows.
  `db::tests::a_second_take_of_the_writer_lock_is_refused_until_the_first_is_dropped`
  fails on it.
- The `DatabaseLocked` half of the mapping in `verify_key`. With that half
  alone removed, check 2 passes: a second process gets `DatabaseBusy`.
- `create` beside a store that another process holds. The contenders of
  check 2 only `open`; check 1 tries `create` beside the live store of its
  own process.
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

## Sync

This file describes the checks of the commit that holds it. Two runs keep
that true for each merged commit, and neither needs a commit id that a
squash merge would change:

- On each pull request and each push, `scripts/r1/ci.d/30-mutants.sh` runs
  `sh scripts/r1-gate/mutation-test.sh --apply-only`, which applies each
  row, the `formal-` rows among them, and builds each mutant; the control
  row must not build, and a transitional row whose name another row file
  holds prints MOVED and is not applied. A covered function that changes
  shape under its row fails that run: the substitution no longer applies.
- On each pull request and each push, `scripts/r1/ci.d/30-formal-covered.sh`
  runs `sh docs/verify/check-covered.sh`, and `12-test` runs the six tests.

On each push to main, `30-mutants.sh` also runs mutants in full, in an
order that the commit id decides. It starts no row once its budget of
seconds (`budget_s`) has passed since the first row started, lets the row
in progress end, and fails when a row that ran is not caught by its
verdict; another commit takes the rows in another order. The whole list,
which requires that the test of every `formal-` row fails, runs on a
person's machine as a line of the phase's gate and of the verifier gates of
the units that name `mutation-test.sh`.

Outcome of the last pass, the third formal step of store-1, over the 15
commits of the store's closed list: updated, for check 1. `create` now
makes its root in a step of its own, `under_a_new_root`, and an error of
that step removes the database of the call and destroys no root. Three
rows moved with that text, and this file follows it. Check 1 gained the
move `CreateWhoseRootStepFails`, with a row for a store that spares only a
root for which its custody answers `RootExists`. Check 2 is in sync: the
changed `create` is on its path, and its three rows fail as before.
