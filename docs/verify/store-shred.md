---
name: verify-store-shred
description: Run and maintain the formal checks of the encrypted store's two safety properties, crypto-shred non-decryptability and the single writer, when a change touches crates/halvo-capture/src/store/, crates/halvo-capture/src/custody/ or crates/halvo-sqlcipher/.
---

# Verify: store crypto-shred and single writer

Eight tests check two properties of the real `EncryptedStore`. Four of them
run the store under random inputs and fail on the first input that breaks a
property. Two run the file scan of check 1 on fixed inputs of 1,200
segments, one makes every move that fails or is refused in a fixed order,
and one sets a process that goes around the directory lock against a
process that holds the store, in two fixed cases. They live in
`crates/halvo-capture/src/store/formal_tests.rs`,
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
   bytes. Delete-all destroys the root first and tries every step after
   it, whatever a step answers. After it returned, with `Ok` or with
   `DeleteIncomplete`, its handle answers a write with `Sealed`, and the
   unwrap of the wrapped store key retained from before fails because its
   root is missing; so does the unwrap of a key file of another label that
   was wrapped under the same root. The one exception is an error whose
   report says that the root step itself failed: the report then names the
   root that stays, every later step was still tried, and the next
   delete-all on the same handle destroys that root. `Ok` means that the
   directory holds no store. An error carries a report that says of the
   root, the key file, the database files and the count of segment files
   what is on disk, and no step after the one that failed was skipped.
   A move that fails, or that the store refuses, leaves no half store. A
   create that fails after it made its database leaves no file and no
   root. A create under a root that exists is refused, leaves no file, and
   leaves that root to the store that has it. A create whose root step
   fails, whatever the custody answers, leaves no file and destroys no
   root, because it has made none. A delete-all that returned an error
   is finished by the next delete-all on the same handle. An append whose
   index row fails leaves no event
   row, and a shred removes the event rows of its segment.
   On macOS every file that the store made has mode 0600 and every
   directory that it made has mode 0700, after every operation, as long as
   no other process widens a mode: the store makes each file at 0600 and
   each directory at 0700. Two modes hold whatever they were before the
   call: the store directory is 0700 after every create and every open,
   which narrow it on its handle, and `store.db` is 0600 after every open,
   which sets that mode on its own descriptor before the connection opens,
   so the `store.db-wal` that SQLite makes with the mode of `store.db` is
   0600 too. Any other entry that exists with a wider mode keeps it (see
   Not covered).
   Harm if broken: data the owner believes destroyed is still recoverable
   from the store by anyone who later obtains the store key. A create that
   takes or destroys the root of another store loses that store. A
   directory that a failed move left half made takes no new store. A file
   that the store made with the mode of the umask shows another account of
   the machine the wrapped keys, the ciphertext and the times of the
   segments.
2. **Single writer.** At most one process holds the store at a time. Two
   layers refuse a second one: the lock on the store directory (on macOS a
   `flock` on the directory itself, on Windows a lock on `writer.lock`,
   which a held handle keeps from being unlinked), and SQLite's exclusive
   lock on `store.db`. Every
   contender that waits eventually gets the store, holds it, writes a segment,
   and exits clean. An `open` that begins and ends while another process
   holds the store is refused before it asks its custody for the store key.
   Of several processes that call `create` on one empty directory at most
   one gets the store, none removes the database of the one that got it,
   and the directory then opens and reads what that one wrote. A process
   that opens `store.db` itself, without the directory lock, neither reads
   nor writes it while another process holds the store, and an `open`
   while such a process holds the database is refused before it asks its
   custody.
   Harm if broken: two writers corrupt the metadata database, or a reader
   keeps a snapshot alive that preserves a shredded key. A creator that
   removes the database of another leaves that one with a store that is
   gone. An `open` that
   asks the custody first puts the store key into a process that gets no
   store, and waits on a custody that can prompt or block for a store that
   it then refuses.

## The checks

| Check | Test of `store::formal_tests` |
|---|---|
| 1 | `a_shredded_segment_is_unrecoverable_with_every_remaining_key` |
| 1 | `no_key_of_a_shredded_segment_is_left_when_three_in_four_expire_in_one_pass` |
| 1 | `no_key_of_a_shredded_segment_is_left_when_segments_expire_oldest_first` |
| 1 | `no_failed_or_refused_move_leaves_a_half_store` |
| 1 | `no_failed_or_refused_move_leaves_a_half_store_in_a_fixed_order` |
| 2 | `at_most_one_process_holds_the_store_at_a_time` |
| 2 | `at_most_one_creator_gets_an_empty_store_directory` |
| 2 | `a_process_that_goes_around_the_directory_lock_is_refused_at_the_database` |

Check 1 is the first test, over random sequences of at most 12 operations,
its two scaled cases, which have a fixed shape each, and two tests of the
moves that fail or are refused: one over random sequences of at most 12
operations that hold those moves, and one that makes each of them in a
fixed order. Check 2 is three tests: one over random races of processes
that open a store, one over random races of processes that create one in
an empty directory, and one of two fixed cases around the directory lock.
`docs/verify/check-covered.sh` reads this table and the table under
Covered code. The delete tail, `EncryptedStore::delete_store_dir`, is held
by tests and rows, not by the formal checks: no move of check 1 calls it,
and a later unit adds it as a move (ruling ST-45 Addition 1).

## Tool

The four random tests are `proptest` property tests over the real store, not
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
| 1 | `crates/halvo-capture/src/store/expiry.rs` | `destroy_segment_keys` (the `UPDATE` that sets a key to NULL, and the checkpoint after its commit), `destroy_store_key`, `delete_events_and_index` (the `DELETE` of the event rows), `EncryptedStore::delete_all` (the root step before every other step, and its steps on a handle that a delete-all sealed) | `formal-shredded-key-survives`, `formal-key-row-deleted`, `formal-key-row-deleted-oldest-first`, `formal-checkpoint-removed`, `formal-delete-all-destroys-nothing`, `formal-delete-all-returns-before-its-root-step`, `formal-shred-leaves-the-event-rows`, `formal-second-delete-all-is-refused`, `formal-second-delete-all-finishes-nothing` |
| 1 | `crates/halvo-capture/src/store/db.rs` | `Db::open_file`, which `Db::open` and `Db::open_existing` call (the `secure_delete` pragma with `require_secure_delete`), `Db::checkpoint_truncate`, `EncryptedStore::write_segment_with_expiry` (where it puts the wrapped key), `KeyFile` and `EncryptedStore::write_key_file` (what `store.key` holds), `EncryptedStore::create` (the removal of the database when `Db::open` fails, and the database before the root step), `EncryptedStore::under_a_new_root` (the root step of a create, which calls `create_root`, and where its error goes), `Db::insert_event` (the rollback to its savepoint), `open_database` (the mode that it sets on `store.db` before the connection opens), `Db::require_file` with its two calls in `EncryptedStore::open_with` and `Db::open_file`, and the one place where `Db::open_file` makes `store.db`, for a create alone, `write_pending` (how it makes `create.pending`) | `formal-secure-delete-off`, `formal-checkpoint-not-verified`, `formal-wrapped-key-in-a-side-file`, `formal-second-wrap-in-another-table`, `formal-raw-store-key-in-key-file`, `formal-failed-create-leaves-the-database`, `formal-create-makes-its-root-first`, `formal-create-takes-an-existing-root`, `formal-refused-create-destroys-the-root`, `formal-create-spares-only-a-root-it-can-read`, `formal-failed-append-leaves-the-event-row`, `formal-store-db-keeps-a-wider-mode`, `formal-open-makes-a-missing-database`, `formal-create-pending-takes-the-umask` |
| 1 | `crates/halvo-capture/src/store/segment.rs` | `remove_entry_below`, which `remove_segments_through` calls for a delete-all (the kind of entry that it lists as a directory) | `formal-delete-walk-takes-a-link-for-a-directory` |
| 1 | `crates/halvo-capture/src/custody/file_root.rs` | the root files of both systems: `FileRoots::wrap` and `FileRoots::unwrap` (the `aead_seal` and `aead_open` calls under the root), and `FileRoots::make_root` (the random fill of a new root), which `FileRoots::create_root` calls | `formal-custody-wrap-under-constant-key`, `formal-custody-root-is-all-zero` |
| 1 | `crates/halvo-capture/src/store/durable.rs` | `Dir::create_new`, which `OsFs::create_new` calls (the mode of a new file), `Dir::create_dir` (the mode of `segments` and of a shard) | `formal-store-files-take-the-umask`, `formal-store-directories-take-the-umask` |
| 2 | `crates/halvo-capture/src/store/db.rs` | `Db::open_file` (the `locking_mode = EXCLUSIVE` pragma and `require_exclusive_locking`), alone, and together with the calls of `DirLock::take` in `EncryptedStore::create_with` and `EncryptedStore::open_with` and with the pragma of `Db::refuse_a_live_holder`; `verify_key`, `code_answer` and `Db::refuse_a_live_holder` (the mapping of `DatabaseBusy` to `Locked`); `EncryptedStore::open_with` (the call of `Db::refuse_a_live_holder` and the place of its answer, before the custody) | `formal-single-writer`, `formal-single-writer-both-locks`, `formal-busy-not-mapped-to-locked`, `formal-held-store-asks-its-custody` |
| 2 | `crates/halvo-capture/src/store/lock.rs` | `DirLock::take` (the `try_lock` on the store directory, on Windows on `writer.lock`) | `formal-directory-lock-never-taken` |

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
| `ShredAll` | `crypto_shred_all`, under the fault that an `ArmFault` before it armed, if one is armed. Whatever it returns, the harness then reads the root, the handle and the disk as the paragraph under the table of faults says. With no fault the first call must return `Ok` with a report that counts every live segment plus the store key, and a later call on the same handle must return `Ok` with a report that counts nothing |
| `Reopen` | drop the store, on macOS set `store.db` to mode 0644 unless delete-all ran, as another process could while no store holds the file, and `EncryptedStore::open`; must succeed unless delete-all ran. After a delete-all it must answer `Sealed`, or `DatabaseMissing` when the delete-all returned an error and left `store.key`: every step was tried, so the database is gone, and `open` reads that before it asks its custody |
| `ReopenUnderAnotherRoot` | drop the store, flip one bit of the root's 32 bytes under the same root id, and `open`, which must answer `Custody(WrapAuthFailed)`; put the bytes back, and `open` must succeed. Nothing after delete-all |
| `Recreate` | after a delete-all that finished, or after one that returned an error, left no `store.key` and whose handle a `Reopen` dropped: drop the store and `create` under a fresh root, which must succeed. Otherwise `create` must refuse with `Locked` or `AlreadyExists` |

The second random test (`no_failed_or_refused_move_leaves_a_half_store`)
replays sequences that also hold the moves of the next table, with the same
checks after each. `no_failed_or_refused_move_leaves_a_half_store_in_a_fixed_order`
makes each of them at least once in one list of 80 moves (68 on Windows,
where the two faults that set a mode and the link below `segments` do
not exist), and fails when a
move of the list does not apply to the state that the moves before it left.

| Op | What it does to the real store |
|---|---|
| `CreateThatFailsAfterItsDatabase` | after a delete-all that finished: `create` under a fresh root with the failpoint `db_open` armed, which fails `Db::open` once after it made `store.db`. The create must return that error, no root with the id of the create may exist, and the directory must hold nothing but `writer.lock` |
| `CreateUnderTheRootOfThisStore` | on a live store: `create` in a second temp directory under the root id of the live store. It must answer `Custody(RootExists)` and leave nothing but `writer.lock` in that directory. Then the harness drops the live store, and `open` must succeed |
| `CreateWhoseRootStepFails` | in any state, five times: `create` in a second temp directory with a stand-in custody under the newest root id, whose `create_root` makes no root and answers `RootLocked`, `RootDenied`, `RootCorrupt`, `Crypto` or `Os` with the code of a file that exists, one answer for each create. Each create must return `Custody` with that answer, must leave nothing but `writer.lock` in its directory, and must not call `destroy_root`, which the stand-in counts and does not pass on |
| `ArmFault(fault)` | arms one fault for the next `ShredAll`, which puts it in the way right before its call and takes it out right after. A fault whose state is not there (the table of faults) arms nothing, and the fixed order fails on such a move |
| `OpenWithoutItsDatabase { cut }` | on a live store: drop the store, take `store.db` out of the directory or, with `cut`, cut it to no bytes, and `open`. It must answer `DatabaseMissing`, and the listing of the store directory, every name below it with its length, must be the same before that call and after it. Then the harness puts the file back, and `open` must succeed |
| `LinkBelowSegments` | on a live store that has a `segments` directory, once in a case, on macOS alone: the harness makes a directory outside the store with one file that has the name of a segment file of the shard `ff`, and puts a symbolic link to that directory at `segments/ff`. From then on, after every operation, the directory must hold that file with the same bytes. A walk that took the link for a shard would take the file for a segment without a key and remove it. Windows has junctions and directory links of its own kind, and this move makes neither |
| `DestroyTheRootAgain` | after a delete-all, finished or stopped, unless its root step failed: `destroy_root` of the dev-fallback custody on the root that delete-all destroyed; it must return `Ok` |
| `Append { index_row_fails }` | on a live store with a live segment: `append_event` for the newest live segment. With `index_row_fails` the harness renames the table `events_fts` away for the call and back after it, so that the second of the two inserts fails; the append must return an error |

After every operation on a live store, the three tests that replay
operations count the rows of `events` and of `events_fts` through the
store's connection. Each count must equal the appends that returned `Ok`
for the segments that are live: a failed append leaves no row, and a shred
removes the rows of its segment.

The faults of `ArmFault`, one for each `ShredAll`. The last cell says what
the harness then requires of the call; what it does not name must be gone
from the disk.

| Fault | How the harness makes it | Needs | What the delete-all must do |
|---|---|---|---|
| `DamagedPage` | it drains the log into `store.db`, drops the store, overwrites 512 bytes of the root page of `events` and opens the store again | a live store | return `Ok`: the delete reads and writes no page of the database |
| `QueryOnly` | `PRAGMA query_only = ON` on the connection of the store | a live store | return `Ok` |
| `KeyAndRootReadOnly` | mode 0400 on `store.key` and on the root file; macOS alone | both files on disk | return `Ok`, with both files removed |
| `RootStep` | it opens the store again under a custody whose `destroy_root` answers `RootDenied` once and which names no roots directory | a live store | return `DeleteIncomplete` whose report names the root of the store as the one that stays; `store.key`, the database and the segments are gone |
| `KeyFileStep` | the failpoint `key_file_destroy`, which fails `KeyFile::destroy` once | `store.key` on disk | return `DeleteIncomplete`; `store.key` stays |
| `DatabaseStep` | the failpoint `database_destroy`, which fails `Db::destroy_through` once | a database file on disk | return `DeleteIncomplete`; the database files stay |
| `BlockedShard` | mode 0500 on the first shard that holds a segment file, and 0700 again after the call; macOS alone | a segment file on disk | return `DeleteIncomplete`; the files of that shard stay, and the report counts them |

After a `ShredAll`, whatever it returned, the harness requires in this
order: that a failpoint which it armed was reached; unless the fault is
`RootStep`, that the root is destroyed, by the classifier named below, and
that the key file of another label does not unwrap; that a write on the
handle answers `Sealed`; then, for `Ok`, that no fault left anything and
that the counts hold; and for `DeleteIncomplete`, that the report says of
the root, the key file, the database and the count of segment files what
a look at the disk finds, that the disk holds what the fault leaves and
nothing else, and under `RootStep` that the report names the root of the
store as the one that stays. From an error until a later `ShredAll`
returns `Ok`, the harness requires after every operation that the disk
still holds exactly that. The `ShredAll` after an error runs with no fault
unless a new `ArmFault` stands before it, and must return `Ok`.

The three failpoints of check 1 (`db_open`, `key_file_destroy`,
`database_destroy`) are the store's own (`store::failpoint`, compiled
under `cfg(test)`), armed by the harness for one shot each on its own
thread. After each move that arms one the harness requires that the
failpoint was reached, so a create that opens no database, or a
delete-all that never comes to its key file or to its database, fails
the move.

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

At every create the harness also wraps a second key under the root of the
store with the label `derived`, writes it through `KeyFile::at` into a key
file in a directory beside the store, and requires that it unwraps. After
a delete-all whose root step did not fail, that unwrap must fail with
`RootMissing` as well.

`Reopen`, and `Recreate` after delete-all, call `open` or `create` right
after they drop the store, so both need the drop to have released the file
lock. `DirLock`'s `Drop` unlocks the directory (on Windows `writer.lock`)
before its descriptor closes. On macOS and Linux the lock belongs to the open file, and a child
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

On macOS the check then walks the store directory, by the type of
each entry itself, and requires mode 0700 of every directory, the store
directory included, and mode 0600 of every file: "... has mode 644 in the
store directory, and a store makes it 600". Every entry of a store of
check 1 is one that the store made, in a directory that the test made at
0700. The one mode that the harness widens is that of `store.db`, before
each `Reopen` of a live store, so the walk after that move reads that
`open` set it back. The walk does not read a symbolic link, which only
the move `LinkBelowSegments` makes.

At every create the harness makes that walk once inside the call as well,
at the seam of the store between its root and its key file
(`SEAM_AFTER_THE_ROOT`), where the directory holds `create.pending`,
`store.db` and its log. A file that a create makes with a wider mode and
removes before it returns fails there with its name and mode: "while a
create stood between its root and its key file: ... create.pending has
mode 644 in the store directory, and a store makes it 600". A create that
never comes to that seam fails as well.

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
fails the case. After a delete-all that returned `Ok` the store directory
must hold nothing but `writer.lock`, which only Windows has, and after
either return every segment must read as `Sealed`.

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
one to three times: retries the open while the store refuses a
second writer (see the platform note below), writes a witness file, logs
`enter`, writes a segment, sleeps the hold time, logs `exit`, removes the
witness, drops the store. The open is `EncryptedStore::open_with`, the body
of `open`, with the custody builder of `open` behind a closure that notes
that it was called. The contender logs every refusal with the time at which
the open began, the time at which it returned, and whether the open had
asked for its custody. The parent
meanwhile unlinks `writer.lock` on a random cadence of 1 to 8 ms, or never.
A case fails if any contender found the witness present on entry, if two
logged hold intervals overlap, if any contender exited non-zero, if a
contender held the store a different number of times than planned, or if an
open that had asked for its custody was refused while one other contender
held the store from before that open began until after it returned. The run
fails if, over all its cases, no refusal falls inside the enter-to-exit
interval of another contender, or if no refused open lies whole inside such
an interval.

The creators' race (`at_most_one_creator_gets_an_empty_store_directory`)
makes an empty store directory and spawns the test binary two to five
times as creators (`create_a_store_for_parent`), each with a fresh root id
under the one roots directory of the case. Each creator calls
`EncryptedStore::create` once. In three cases of four the first creator
waits at a seam of the store (`SEAM_BEFORE_THE_DATABASE_STEP`, in
`create_with` after the lock, the probe for a holder and the look for
`store.key`, and before the removal of the database files) until every
other creator has answered. The others start together at one mark, with
no stagger, and in the fourth case all creators start so. In three cases
of four the parent unlinks every lock path that the platform still has,
the name `writer.lock`, every 200 microseconds: on macOS no such path
exists. A creator that gets the store writes a segment and holds the
store until every creator has answered, then counts its open files that
have lost their name (macOS: it reads `/dev/fd`) and closes. A case fails
if more than one creator got the store; if a creator was answered anything
but `Locked` or `AlreadyExists` (for Windows see the platform note); if
the first creator did not come to its seam; if no creator got the store;
if the one that got it held a file without a name at its close; if the
directory does not open afterwards or does not read back the segment that
the winner wrote; if the roots directory holds another root file than the
one of that store; or if the store directory holds a name that is no file
of a store, `create.pending` among them. The run fails if, over all its
cases, no creator was refused while the first stood at its seam. On a pass
it writes one line to standard error:
`check 2 creators in <cases> cases: created=<n> locked=<n> already_exists=<n> access_denied=<n> other_under_the_unlink=<n> {<answers>} refused_while_the_first_stood_before_its_database_step=<n> cases_with_no_creator=<n>`.

The cases around the directory lock
(`a_process_that_goes_around_the_directory_lock_is_refused_at_the_database`)
are fixed and draw nothing. A second process
(`go_around_the_directory_lock_for_parent`) unwraps the store key through
the custody and opens `store.db` with SQLite itself, taking no directory
lock. In the first case the test holds the store, and the second process
tries one read and one write: both must be refused, and the holder must
still write afterwards. In the second case the second process holds the
database (`locking_mode = EXCLUSIVE`, one read, `BEGIN EXCLUSIVE`), and
the test calls `open_with`: it must answer a refusal that `writer_refusal`
names, without a call of its custody; once the second process has left,
`open` must succeed and write. On a pass the test writes one line to
standard error:
`check 2 around the directory lock: read=<answer> write=<answer> open_while_another_held_the_database=<answer> asked_its_custody=<bool>`.

## How each platform refuses a second writer

The contender takes an error of `open` as a refusal only when
`writer_refusal` names its class, and retries after it until its deadline:

| Platform | Refusal that the contender accepts | Where it comes from |
|---|---|---|
| macOS | `Locked` | `DirLock::take` (store/lock.rs) gets `WouldBlock` from the `flock` on a descriptor of the store directory itself, so no lock file exists to unlink, and every second opener is refused there. While a process holds the database without the directory lock, `open` answers `Locked` from `Db::refuse_a_live_holder`, which reads `SQLITE_BUSY` before `open` asks its custody; the cases around the directory lock make that state, and the open race does not |
| Windows | `Locked`, `Io(PermissionDenied)`, or the `Io` of a sharing violation | the same two paths, with the lock on the file `writer.lock`, which `DirLock` opens without delete sharing, so an unlink of the held file is refused. While the parent's unlink has the lock file open for an instant, the system refuses a contender's own open of it, mostly with a sharing violation, which `open` hands on as `Io` with a kind that the standard library does not name (`Io(Uncategorized)` in a debug print), and now and then with access denied. The contender takes both as a refusal and tries again. Until the sixth formal step it did not take the first: the hosted Windows job of that step failed on it in a case with a cadence of 2 ms, where the job at the head of the unit had passed one run. In the 20 hosted runs after the change, 14 runs counted 31 sharing violations in all and 4 runs counted 5 answers of access denied, beside 154,272 of `Locked`. The contender takes `Io(PermissionDenied)` from any step of `open` |

Any other error ends the contender at once, with the error in its message,
and the case fails with "contender N exited with exit status: 101". So an
`open` that answers a second writer with anything but these refusals fails
the check, on either platform. The check also fails if a second process is
admitted (the witness or an overlapping interval), if an admitted writer
cannot write, if a contender never gets in before its deadline, or if one
holds the store fewer times than planned.

The order of an `open` (`EncryptedStore::open_with`): it takes the handle
of the store directory, narrows the directory on that handle, takes the
directory lock, reads `store.key` relative to the handle, refuses a
`segments` that is a link and a `store.db` that is missing, empty or no
regular file, probes for a holder, asks its custody, and opens the
database file. Only then does it clear what a stopped create left in
`create.pending`, with the root id of the key file that it read: it keeps
that root and destroys another one that `create.pending` names, and a
cleanup that cannot finish leaves the open as it is (ruling ST-43). Beside the
refusals an `open` can answer `Obstructed` (a link or an entry of another
kind at a store name, or a store directory that left its name),
`KeyFileUnparseable`, `KeyFileOfAnotherLabel`, `OlderStore`,
`DatabaseMissing`, `DatabaseUnreadable`, `Custody` and `Io`. None of them
is a class of `writer_refusal`, so each ends a contender and fails the
case, and no step of check 2 makes a link, a move or such a file. A code 5
or 6 of SQLite at any step of an open is answered `Locked` (`code_answer`),
after the custody too, and the contender takes it as a refusal and tries
again. So an `open` that answers `Locked` while no process holds the
store passes until the contender's deadline of 60 s and fails then with
"never got the store"; the open race cannot tell a single such answer from
a refusal at the edge of a hold. On Windows the class
`Io(PermissionDenied)` takes that error from every step of `open`, and
the line of a pass counts it under `access_denied`.

A creator of the creators' race makes one attempt and writes down what
`create` answered, and the parent judges it. On macOS the parent takes the
store, `Locked` and `AlreadyExists`, and every case must end with one
creator that got the store. On Windows it also takes
`Io(PermissionDenied)`, and in a case that unlinks `writer.lock` any `Io`
answer: while no creator holds the lock file yet, the parent's unlink has
that file open for an instant, and a creator's own open of it can be
refused by the system then. Such a case can end with no creator, which
the parent accepts on Windows alone. The line of a pass counts both
(`other_under_the_unlink` with the answers, and `cases_with_no_creator`).
In the 22 hosted Windows runs of the sixth formal step, 264 cases, every
case ended with one creator, and every other creator was answered
`Locked`.

Two contenders can finish a case without meeting: they start 0 to 20 ms
apart and hold the store 5 to 30 ms. A run in which no contender is ever
refused would pass whatever the lock does, so the check counts the
refusals of the whole run and requires one that falls inside another
contender's hold. It asks this of the run and not of each case. On a pass
it writes one line to standard error, past the capture of the test harness,
so the line is in a CI log too:
`check 2 refusals in <cases> cases: locked=<n> access_denied=<n> sharing_violation=<n> while_another_contender_held_the_store=<n> before_the_custody_through_a_whole_hold=<n>`.
The last count is of the refused opens that began and returned inside one
hold of another contender. None of them had asked for its custody, or the
case would have failed, and a run of at least `RACE_CASES` cases needs one:
with none, the run could not tell a refusal before the custody from one
after it. An open that is refused after it asked for its custody is no
failure by itself: two contenders that find the store free both ask, and
`verify_key` refuses the one that comes second to the database.
Both checks run in CI on the macOS and Windows jobs. The rows under Break
proofs run on macOS.

A refusal ends when the holder drops the store. The database connection
closes first, which releases the exclusive database lock. Then `DirLock`'s
`Drop` unlocks the directory (on Windows `writer.lock`), and its descriptor
closes. An `open` or
`create` that fails after it took the lock releases it the same way before
it returns. Check 2 cannot tell the unlock from the close. A contender
spawns no child, so nothing else shares its descriptor.

## Bounds and assumptions

| Bound | Value | Why |
|---|---|---|
| Check 1 cases | 48 (`SHRED_CASES`) | each case creates a root file and a store; 48 run in about 10 s on a laptop |
| Ops per case | 1 to 12 (`MAX_OPS`) | every reachable state needs at most a write, a shred and a reopen or recreate; 12 leaves room for several of each |
| Segments per case | at most 12 | one per write; bounded by the ops |
| Cases with moves that fail | 32 (`FAILING_MOVE_CASES`), 1 to 12 ops each | the second random test; it interleaves the moves that fail or are refused with the others, and no row depends on a case that it draws |
| The fixed order | 80 moves on macOS, 68 on Windows | each move that fails or is refused at least once, in the state that it needs, and each fault of `ArmFault` once before a `ShredAll`; the rows of these moves are judged by it. The two faults that set a mode and the move that makes a link exist on macOS alone |
| Faults | one for each `ShredAll` | one step of a delete-all fails, so the harness knows what may stay |
| Segment sizes | 0 to 512 bytes, one chunk and one byte either side of it, two chunks and seven bytes | the empty, the one-chunk boundary and the multi-chunk cases of the 64 KiB stream |
| Scaled cases, segments | 1,200 of 40 bytes (`SCALED_SEGMENTS`, `SCALED_SEGMENT_LEN`) | the size at which a key row that is deleted, not set to NULL, leaves keys in `store.db`: 3 of 900 after the one pass |
| Oldest first | 7 segments a pass, 50 passes, a failed write after every third write (`OLDEST_FIRST_PER_PASS`, `OLDEST_FIRST_PASSES`, `A_SOURCE_DIES_AFTER_EVERY`) | with deleted key rows a key is left after passes 17 to 22 and again after passes 49 and 50 of 150; the case ends at 50 |
| Check 2 cases | 30 (`RACE_CASES`) | each case spawns processes, and a row of check 2 fails only in a case in which two contenders meet; 30 follows from the measured share of such cases, under Break proofs |
| Contenders | 2 to 5 (`MAX_CONTENDERS`) | two is the minimal race; five covers a queue of waiters |
| Acquisitions per contender | 1 to 3 | re-acquisition after a release, with the lock file possibly recreated |
| Hold | 5 to 30 ms | long enough that a second admission falls inside the hold |
| Child deadline | 60 s per contender process, for all of its acquisitions | a contender that never gets the store is a liveness failure |
| Creators' cases | 12 (`CREATOR_CASES`) | each case spawns two to five processes and waits for each; the row of the directory lock fails in every case |
| Creators | 2 to 5 (`MAX_CONTENDERS`) | two is the minimal race; five gives the one that got the store four that are refused |
| Lock paths unlinked | every 200 microseconds, in three cases of four (`LOCK_PATHS_UNLINKED_EVERY`) | the cadence of the plan of this check; on macOS it removes nothing, because no lock path exists |

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

Assumptions: the dev-fallback custody (a root file under a roots directory of
the test's own, from store-3 on) is available to the test process and its
children; the checks run on one
machine, so the contenders' `SystemTime` clocks are comparable; proptest's
shrinking makes a reported counterexample minimal but not unique. The checks
do not run the Secure Enclave or DPAPI custodies. The read of the modes
takes the umask of the test process as it finds it: the two rows on the
mode of a new file and of a new directory
fail where that umask leaves a bit for the group or for others, as 022 does
on the hosted runners, and under a umask of 077 the store of either row
makes the modes that the unmutated store makes. The crate builds for macOS
and Windows alone: `durable::no_follow` exists for those two targets, and
its callers do not compile for another. So the checks run on those two
systems, and the read of the modes on macOS.

## Command

```sh
cargo test -p halvo-capture --lib formal_tests
```

The eight tests, three stubs that a test starts as a child process
(`contend_for_store_for_parent`, `create_a_store_for_parent`,
`go_around_the_directory_lock_for_parent`) and three tests of what the
harness reads (two of a destroyed root, and on macOS one of an open file
without a name) run: fourteen tests on macOS, thirteen on Windows. On the
Mac of the team the command took 65 s and 102 s in two runs of the sixth
formal step, at a 1-minute load of 46 and of 80. Earlier counts, each of the tests that
the command held then: on a 16-core Mac whose 1-minute load was 100 to 400
from other work, four tests took 31 s in one run, and alone the random
test of check 1 took 19 s, the one-pass case 12 to 15 s, the oldest-first
case 34 to 47 s and the open race of check 2 14 s; at a load near 10 the
random test took 10 s and the one-pass case 11 s; nine tests took 77 to
82 s in three runs at a load of 180 to 480. A single test: append `::`
and the start of its name, for example `::a_shredded_segment` or
`::at_most_one_creator`.

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
  store wrote a file that the check does not read; "... has mode ... in the
  store directory, and a store makes it ..." means that the store made the
  named file or directory with the mode that the umask gives, or left it
  wider than its owner, as an `open` does that sets no mode on a
  `store.db` which the harness widened;
  "with other bytes under the id of its root, the store reopened" means that
  the unwrap does not depend on the root's bytes; the delete-all messages
  say that the root is alive, that its state is unknown with the error of
  the unwrap, or name the file left behind. "the key file of another label
  under the root of the store still unwraps" means that a second key under
  the same root outlives the delete. "a write on its handle answered ... in
  place of Sealed" means that a delete-all returned and left its handle
  open. "a delete-all with ... in its way returned Ok, and the step that
  failed leaves ..." means that the call answered `Ok` over something that
  it left. "reported ..., and a look at the disk finds ..." means that the
  report of an error does not say what is on disk. "left ..., and only ...
  could stay, so it skipped a step after the one that failed" means what
  it says. "does not name the root of the store as the one that stays" is
  the report of a failed root step. "a delete-all that stopped left ...,
  and the store directory now holds ..." means that a later call changed
  what a stopped delete-all had left. "after a delete-all that left ...,
  the store reopened as ..." means that `open` answered a stopped store
  otherwise than `DatabaseMissing` with its key file and `Sealed` without
  it. "reopen failed: another writer
  holds the store", or a panic "create on a free directory: Locked", means
  that a lock outlived the store that the op had just dropped: the lock on
  the directory if `DirLock`'s `Drop` did not release it, or the database
  lock.
- **The moves that fail or are refused.** The fixed order prints "at move N
  of 80" and the move in front of the message. "a create that failed left
  ... in the store directory" or "left the root that it made" names what
  the create did not clean up. "a create under the root of another store
  returned ... in place of RootExists" means that a create took a root that
  exists; "after a create under its root was refused, the store did not
  reopen" means that the refused create destroyed or changed that root. "a
  create whose root step answered ... returned ..." names what the create
  returned in place of the custody's answer; "... had made no root and
  called destroy_root N times" means that the clean-up of the create
  destroys a root that the call did not make; "... left ... in its
  directory" names a file that it did not remove. "delete-all did not
  reach the removal of its key file", or "of its database", means that a
  failpoint which a fault armed was never reached. After the
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
  "contender N asked its custody in an open that was refused, and contender
  Some(M) held the store from before that open to after it" means that
  `open` unwrapped the store key for a store that another process held all
  the while. "no open began and was refused within one hold of another
  contender" means that the run could not read that order.
  "open failed with Io(NotFound), which is no refusal of a second writer"
  had one known cause until store-3, the open of the lock file under an
  unlink (see Sync). On macOS no lock file is opened any more, and the
  message means that the directory of the store is gone.
  In the creators' race, "creators [..] each created a store in the one
  directory" means that two processes both got `Ok` from `create`.
  "creator N got the store and, at its close, held open files that had
  lost their name" means that another creator removed the database under
  the one that got the store. "after the creators the store of creator N
  did not reopen", or "the segment that creator N wrote does not read
  back", means that the store which `create` handed out is not the store
  in the directory. "creator N was answered ..., which is no refusal of a
  second creator" names an answer of `create` that is neither the store
  nor a refusal; with a broken lock it is what a creator gets that ran
  into the files of another. "no creator got the empty directory" means
  that every creator was refused. "the roots directory holds ..." names a
  root file that a refused creator left.
  In the cases around the directory lock, "a process that opened store.db
  itself, without the directory lock, was answered ... while another
  process held the store" gives what the read and the write of that
  process got: `got-in` means that the holder had no exclusive lock on
  the database. "an open while another process held the database itself
  answered ..., which is no refusal of a second writer" and "... had asked
  its custody" are the two halves of the read of `open`.

A step that cannot happen in the code marks a modeling error in the harness:
for example an op the harness applies to a state the daemon can never reach.
If every step can happen, it is a bug in the store. Do not weaken the
property or the harness; report the counterexample.

## Break proofs

Each check has been seen to fail on a broken store, and each break is a row
of a `mutants.rows` file: `crates/halvo-capture/src/store/mutants.rows` for
store code and `crates/halvo-capture/src/custody/mutants.rows` for custody
code. The name of such a row starts with `formal-`, its verdict is `test`
with one of the eight tests of the table under The checks, and its state is
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
| `formal-checkpoint-not-verified` | `Db::checkpoint_truncate` runs a PASSIVE checkpoint and accepts a WAL that still holds frames; the checkpoint of an open (`Db::log_drained`) stays | the random test of check 1 | the same, in a frame of `store.db-wal` |
| `formal-secure-delete-off` | `Db::open_file` switches `secure_delete` off and does not read it back | the one-pass scaled case | the same, in a page of `store.db` |
| `formal-wrapped-key-in-a-side-file` | `write_segment_with_expiry` also writes the wrapped key into `keys.log` in the store directory | the random test of check 1 | "the store directory holds keys.log, which is no file of a store" |
| `formal-second-wrap-in-another-table` | `write_segment_with_expiry` wraps the segment key a second time and keeps that wrap in a table that no shred clears | the random test of check 1 | "shredded segment ... decrypts with a key remaining in the store" |
| `formal-reader-keyed-with-zeros` | the harness gives the page reader an all-zero key | the random test of check 1 | panic "page 1 does not decrypt to a SQLite header" |
| `formal-raw-store-key-in-key-file` | `store.key` also holds the raw store key, as base64 | the random test of check 1 | "store.key of the fresh store holds the raw store key" |
| `formal-delete-all-destroys-nothing` | `destroy_store_key` returns at once, so `delete_all` answers `DeleteIncomplete` with the report of a store that no step touched | the random test of check 1 | "after delete-all the root is alive: the retained wrapped store key still unwraps" |
| `formal-delete-all-returns-before-its-root-step` | `delete_all` shreds every keyed segment (the key statement, the checkpoint and the sweep) before its root step and returns the error of that shred, which is the order that the store had before store-3 | the fixed order | "at move 30 of 80, ShredAll: ... after delete-all the root is alive: the retained wrapped store key still unwraps", under the fault `DamagedPage` |
| `formal-custody-wrap-under-constant-key` | the root file store seals and opens the store key under an all-zero key, not under the root | the random test of check 1 | "the wrapped store key of a fresh store opens under an all-zero key, so no root seals it" |
| `formal-custody-root-is-all-zero` | a new root is never filled with random bytes | the random test of check 1 | panic "create on a free directory: Custody(RootMissing)": since store-3 a root file of zeros reads as a missing root, so the store itself refuses to wrap under it, and the create of the harness fails before the harness reads the wrap |
| `formal-single-writer` | `Db::open_file` neither sets `locking_mode = EXCLUSIVE` nor requires it; the directory lock stays | the cases around the directory lock | "a process that opened store.db itself, without the directory lock, was answered \"read=got-in write=got-in\" while another process held the store" |
| `formal-single-writer-both-locks` | the same, `create` and `open` take no directory lock, and the live-holder probe takes no `locking_mode = EXCLUSIVE` | the open race | "a contender found another contender inside the store", "held the store at the same time", "contender N exited with exit status: 101" with "contender N: write failed with the store database failed with SQLite code 5" on stderr, or "no contender was refused while another held the store" |
| `formal-directory-lock-never-taken` | `DirLock::take` never calls `try_lock`, so no process is refused at the directory; the exclusive mode stays | the creators' race | "creator N got the store and, at its close, held open files that had lost their name: Some(2); another creator removed its database" in a case in which the first creator waits. After proptest shrank the case to two creators that start together: "creator N was answered Io(AlreadyExists), which is no refusal of a second creator", or the same with `Obstructed{name:Database,met:DatabaseOutsideTheDirectory}` |
| `formal-busy-not-mapped-to-locked` | `verify_key`, `code_answer` and the live-holder probe do not map `DatabaseBusy` and `DatabaseLocked` to `Locked`; the directory lock stays | the cases around the directory lock | "an open while another process held the database itself answered Err(Io(Other)), which is no refusal of a second writer" |
| `formal-held-store-asks-its-custody` | `open_with` runs `Db::refuse_a_live_holder` and drops its answer, so an open while another process holds the database is refused by `verify_key`, after the unwrap | the cases around the directory lock | "an open that was refused while another process held the database itself had asked its custody" |
| `formal-store-files-take-the-umask` | `Dir::create_new` makes a new file with the mode of the umask, so `store.key` and each segment file take it; `open` still narrows `store.db` to 0600 on its own descriptor | the fixed order | before its first move: "store.key has mode 644 in the store directory, and a store makes it 600" |
| `formal-store-directories-take-the-umask` | `Dir::create_dir` makes a directory with the mode of the umask, so `segments` and each shard take it | the fixed order | "at move 1 of 80, Write { len: 40, expiry: Past }: ... segments has mode 755 in the store directory, and a store makes it 700" |
| `formal-store-db-keeps-a-wider-mode` | `open_database` reads the metadata of `store.db` on its descriptor and sets no mode, so a `store.db` that was 0644 before `open` stays so, and its log takes that mode | the fixed order | "at move 22 of 80, Reopen: ... store.db has mode 644 in the store directory, and a store makes it 600" |
| `formal-open-makes-a-missing-database` | neither `open_with` nor `Db::open_file` refuses a `store.db` that is missing or empty, and `Db::open_file` makes the file when it is missing | the fixed order | "at move 65 of 80, OpenWithoutItsDatabase { cut: false }: ... an open of a store whose store.db was taken away answered Err(OlderStore { found: 0, supported: 1 }) in place of DatabaseMissing" |
| `formal-delete-walk-takes-a-link-for-a-directory` | `remove_entry_below` lists every entry that is no regular file as a directory, so it counts a link below `segments` as a directory that it could not list; the open through the handle still refuses the link | the fixed order | "at move 67 of 80, ShredAll: ... a delete-all with None in its way returned Err(DeleteIncomplete(DeleteReport { ... files_left: 1, not_followed: 1 }))" |
| `formal-create-pending-takes-the-umask` | `write_pending` writes `create.pending` by its name with the mode of the umask, not through `OsFs::create_new` | the fixed order | before its first move: "while a create stood between its root and its key file: ... create.pending has mode 644 in the store directory, and a store makes it 600" |
| `formal-root-step-moves-the-roots-directory-aside` | `EncryptedStore::root_step` renames the roots directory aside and reads its root as destroyed, the copy F1 of the verify step of ruling ST-44 | the fixed order | apply-only on the builder's Mac; it runs to its end on the hosted macos job alone (ruling ST-44 F7 (e)), and builder-62 reports its line |
| `formal-failed-create-leaves-the-database` | `create` returns the error of `Db::open` and removes neither the database that the call made nor its `create.pending` | the fixed order | "at move 16 of 80, CreateThatFailsAfterItsDatabase: ... a create that failed left create.pending in the store directory"; `store.db` is left as well, and the read names the first name of the listing |
| `formal-create-makes-its-root-first` | `create` makes its root with `create_root` before it opens the database, so a failure of `Db::open` leaves that root | the fixed order | "at move 16 of 80, CreateThatFailsAfterItsDatabase: ... a create that failed left the root that it made" |
| `formal-create-takes-an-existing-root` | `create` neither looks for a root file of its id before its database step nor runs its root step, `under_a_new_root`, so `write_key_file` wraps under a root that exists | the fixed order | "at move 6 of 80, CreateUnderTheRootOfThisStore: ... a create under the root of another store returned None in place of RootExists" |
| `formal-refused-create-destroys-the-root` | `create` does not look for a root file of its id before its database step, and `under_a_new_root` sends every error of the root step, `RootExists` among them, to `abandon_create`, which destroys the root | the fixed order | "at move 6 of 80, CreateUnderTheRootOfThisStore: ... after a create under its root was refused, the store did not reopen" |
| `formal-create-spares-only-a-root-it-can-read` | `under_a_new_root` keeps the clean-up that removes the database alone for `RootExists`, and sends every other error of the root step to `abandon_create`, as `create` did before its root step stood alone | the fixed order | "at move 25 of 80, CreateWhoseRootStepFails: ... a create whose root step answered Custody(RootLocked(-25308)) had made no root and called destroy_root 1 times" |
| `formal-failed-append-leaves-the-event-row` | `insert_event` releases its savepoint without the rollback | the fixed order | "at move 3 of 80, Append { index_row_fails: true }: ... events holds 2 rows, and 1 appends returned Ok for the segments that are live" |
| `formal-shred-leaves-the-event-rows` | `delete_events_and_index` deletes the index rows and leaves the event rows | the fixed order | "at move 7 of 80, ShredExpired: ... events holds 2 rows, and 1 appends returned Ok for the segments that are live" |
| `formal-second-delete-all-is-refused` | `delete_all` answers `Sealed` on a handle that a delete-all sealed, and finishes nothing | the fixed order | "at move 13 of 80, ShredAll: ... a second delete-all did not finish with nothing left to shred: Err(Sealed)" |
| `formal-second-delete-all-finishes-nothing` | `delete_all` runs no step on a handle that a delete-all sealed, and answers with the report of the call before | the fixed order | "at move 13 of 80, ShredAll: ... a second delete-all did not finish with nothing left to shred: Err(DeleteIncomplete(DeleteReport { ... key_file_gone: false ... }))" |

The shortest failing input of the four rows on the key statement, the
checkpoint and the journal mode is
`[Write { len: 0, expiry: Past }, ShredExpired]`.

Thirteen rows share their substitution with a `formal-` row of the same file
that has another verdict: `shredded-key-survives`, `key-row-deleted`,
`checkpoint-not-verified`, `raw-store-key-in-key-file`,
`delete-all-destroys-nothing`, `custody-wrap-under-constant-key`,
`custody-root-is-all-zero`, `store-db-mode-not-narrowed`,
`directory-lock-never-refuses`, `live-holder-busy-not-mapped-to-locked`,
and the three rows of the hardening tests on a create that destroys a
root, `create-destroys-a-root-it-cannot-read`,
`create-destroys-a-root-file-it-cannot-read` and
`create-destroys-the-root-of-a-create-at-the-same-instant`. A row has
one verdict, and the covered table needs the formal check to fail, so each
of these substitutions stands under more than one name: that of
`key-row-deleted` under three, once for each scaled case, and that of the
three rows on a create under four, with
`formal-refused-create-destroys-the-root`. The sixth formal step gave
`checkpoint-not-verified` and the three rows on a create the text of their
formal row again: the first statement of the one had come to land in
`Db::log_drained`, and the three did not build. The copies of these four
reach a call of `destroy_root` or a line that it calls, so they run to
their end on a hosted runner and not on the Mac of the team (ruling
ST-30 (6)).

The formal row of the journal mode, until store-3 took it out of the rows file,
turned two places, the pragma and the mode that `require_write_ahead_log`
requires. The row `journal-mode-not-wal` of
the same file turns the pragma alone, and `db::tests::pragmas_are_set`
judges it. Since `open` and `create` read the journal mode back, a store
with the pragma alone turned opens nowhere: check 1 fails on it at its
first create with `SettingNotInEffect("journal_mode")`, as every test that
opens a store does, and its file scan reads nothing. With both places
turned the store opens with a rollback journal, and the file scan finds the
key of a shredded segment in a record of `store.db-journal`. The rows
`formal-secure-delete-off` and `formal-single-writer` turn their read-back
with their pragma for the same reason. A check on the unchanged store
cannot make a setting fail to take effect, so the proof that each setting
is read back stays `db::tests::pragmas_are_set` with its rows, and a
formal row turns the read-back together with the setting.

The rows of check 1 on the random test need a case with their pattern (see
Bounds). Of the five rows of check 2, one depends on timing:
`formal-single-writer-both-locks` shows only in a case of the open race in
which two contenders meet. The creators' race fails on its row in every
case, and the cases around the directory lock draw nothing. The counts
that follow were taken at the third and fourth formal steps, when
`formal-single-writer` and `formal-busy-not-mapped-to-locked` were judged
by the open race over a lock file that the parent unlinked; they are the
reason for `RACE_CASES`. On the Mac of the third
formal step, runs of these two rows were given up to 200 cases, so that each run ended
at its first failing case, and the share of failing cases followed the load
of the machine. `formal-single-writer`, with the contender as it was before
that step: 30 failing cases among 49 at a 1-minute load of 9 to 15. The
same row in 30 pairs of runs at a load of 24 to 128: 30 among 38 with the
contender as it was, 30 among 42 with the contender of that step.
`formal-busy-not-mapped-to-locked`, which passed the contender as it was:
30 among 38 at a load of 114 to 402. `RACE_CASES` is 30 for the lowest
share measured, 0.61 a case: 30 such cases all pass with a chance below 1
in a million million, and at half that share once in about 55,000 runs. A
machine on which contenders rarely meet needs more cases, and the line
that check 2 prints on a pass shows it: its last two counts fall towards 0.

Since the store refuses a held store at `Db::refuse_a_live_holder`,
`formal-busy-not-mapped-to-locked` shows only when two contenders both
find the store free and one comes second to the database. At the fourth
formal step the substitution of that row failed check 2 in 5 of 5 runs,
each with "open failed with Db(5)". `formal-held-store-asks-its-custody`
shows in a case with an unlink cadence in which a refused open falls whole
inside a hold of another contender: its substitution failed check 2 in 3
of 3 runs, and the check as it stood before that step passed it in 2 of 2.
Two runs of the unmutated store counted 2,083 and 2,400 refused opens
whole inside a hold, none after the custody.

Since store-3 the open race no longer reaches the code that these three
rows turn: on macOS the directory lock refuses every second opener
first, and no unlink takes it away. At the sixth formal step the open race
passed the substitution of `formal-held-store-asks-its-custody` at the
head of the unit, and passed the exclusive mode alone and the mapping
alone (one run each). The three rows are judged by the cases around the
directory lock since then, and each fails there in every run.
`formal-directory-lock-never-taken` passed the open race and the cases
around the lock in the same step (one run each) and fails the creators'
race: five runs of five at the sixth formal step, one of them with
`PROPTEST_MAX_SHRINK_ITERS=0`, which printed the first text of its line in
the table.

`formal-secure-delete-off` is judged by the one-pass case, because the
random test shows it only in a case with two due writes before one
`ShredExpired`: with one, the store writes the row without its key over a
part of the freed key. A simulation gives such a case a share of 0.14, so 1
run in about 1,300 would hold none.

`formal-reader-keyed-with-zeros` mutates the harness, not the store: it is
the control that the file scan cannot pass blind.

The rows of the moves that fail or are refused are judged by the fixed
order, which draws nothing: each fails at the same move in every run.
The two rows on the mode of a new file and of a new directory fail there
too, with a umask that leaves a bit for
the group or for others (see Bounds and assumptions): the one on files at
the read that the fixed order makes before its first move, the one on
directories after the first write, which makes `segments` and a shard.
The row on directories turns `Dir::create_dir` alone. Until the fifth
formal step it also took the mode out of `create_private_dir_all`, which
since ruling ST-26 makes only the store directory: the fixed order passed
a copy with that statement alone (one run), because the test gives
`create` a directory that exists and `create` narrows it on its handle.
`formal-store-db-keeps-a-wider-mode` needs no umask. It fails at move 22,
the first `Reopen` of a live store, before which the harness sets
`store.db` to 0644. Without that step check 1 passed the row's changed
copy, the fixed order and both random tests (one run of the fifth formal
step), because a store of check 1 makes its own `store.db` at 0600.
`DestroyTheRootAgain` has no row. No changed copy of `destroy_root`, or of a
line that it calls, runs on the Mac of the team (ruling ST-30 (6)), so no row
and no probe mutates it. Its read has a control in place of a row:
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
finish. A second call that runs no step shows only
after a delete-all that stopped, and the fixed order holds that pair at its
moves 10 and 13.

The root step of a create has three rows. `formal-create-takes-an-existing-root`
and `formal-refused-create-destroys-the-root` fail at move 6, where the
custody answers `RootExists` for the root of the live store. Since store-3
two places refuse a root that exists: `create` looks for the root file of
its id before its database step, and `create_root` answers `RootExists`
when it comes to that file. Either one alone keeps the root, so both rows
turn the look together with the root step. With the look left in place,
each of the two failed only at move 25, on the stand-in custody, with the
text of the third row (one run each at the sixth formal step).
`formal-create-spares-only-a-root-it-can-read` is the store as it stood
before the root step became its own: it spares a root for which the custody
answers `RootExists` and destroys one for which the custody answers
anything else. The fixed order passed that copy while its last move was
move 23 (one run of the third formal step on the changed copy), because
`RootExists` was the only answer that it made the root step give. The
move after it, move 25 of today's list, makes it give five others. The stand-in custody of move 24 counts the
calls of `destroy_root` and does not pass them on, so this changed copy
destroys no root.

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
A fourth, `break-single-writer-flock-only.patch`, is
`formal-directory-lock-never-taken` since the sixth formal step.

| Patch | What it breaks | Result |
|---|---|---|
| `break-crypto-shred.patch` | `destroy_segment_keys` no longer sets `wrapped_key` to NULL | check 1 FAILS: `[Write { len: 0, expiry: Past }, ShredExpired]`, "decrypts with a key remaining in the store" |
| `break-single-writer.patch` | `Db::open` no longer sets `locking_mode = EXCLUSIVE` | check 2 FAILS: two contenders, `writer.lock` unlinked every 1 ms |
| `break-single-writer-both-locks.patch` | the above plus `WriterLock::take` never locks | check 2 FAILS with the lock file left alone. `db::tests::a_second_take_of_the_writer_lock_is_refused_until_the_first_is_dropped` FAILS |
| `break-single-writer-flock-only.patch` | `WriterLock::take` never locks, exclusive mode kept | check 2 as it stood then PASSED: it raced only `open` on a store that was made beforehand. `db::tests::a_second_take_of_the_writer_lock_is_refused_until_the_first_is_dropped` FAILED. Since the sixth formal step the creators' race of check 2 fails on this break (the row `formal-directory-lock-never-taken`) |
| `break-writer-lock-outlives-store.patch` | `WriterLock`'s `Drop` does not unlock and leaks a duplicate of its descriptor, so the lock outlives the dropped store | check 1 FAILS: "reopen failed: another writer holds the store". Check 2 FAILS with the lock file left alone: "never got the store; last refusal: another writer holds the store". `db::tests::a_second_take_of_the_writer_lock_is_refused_until_the_first_is_dropped` and `db::tests::a_dropped_directory_lock_is_released_while_a_duplicate_of_its_descriptor_stays_open` FAIL |
| `break-writer-lock-unlock.patch` | `WriterLock`'s `Drop` does not unlock, so only the close releases the lock | check 2 PASSES. Check 1 PASSES unless a drop and the next `open` or `create` both fall between a child's spawn and its exec: at `d7b245a`, which has no unlock, check 1 failed in 4 of 180 runs and check 2 in 0 of 180. `hardening_tests::a_dropped_store_releases_the_writer_lock_while_a_duplicate_of_its_descriptor_stays_open`, `hardening_tests::no_open_is_refused_as_locked_while_other_threads_spawn_children` and `db::tests::a_dropped_directory_lock_is_released_while_a_duplicate_of_its_descriptor_stays_open` FAIL |
| `mutation-unlock-at-once.patch` | `WriterLock::take` unlocks the file right after it locks it, so no store holds the file lock | both checks PASS. `db::tests::a_second_take_of_the_writer_lock_is_refused_until_the_first_is_dropped` FAILS |
| `m7-success-only.patch`, `m8-store-drop.patch` | only a lock that reached a store is unlocked. In m7 the unlock stays in `WriterLock`'s `Drop`, behind a flag that `open` and `create` set when they build the store that they return. In m8 the unlock is in a `Drop` of the store | both checks PASS in every run taken with each patch. The paragraph on where the unlock is lists the runs: with each patch four runs of both checks on the `db.rs` of the blob table, at the sync commit 21 runs of the lib suite with m7 and 8 with m8, and with m7 more runs of check 1 beside spawned children. Check 1 can fail when a child is spawned while an `open` that fails holds its lock, and the child has not reached its exec at the next `open` or `create`. No run has shown that failure of check 1. A probe outside check 1 has shown by run that the `open` after a failed `open` can be refused as `Locked`, and the paragraph gives its counts. `db::tests::a_dropped_directory_lock_is_released_while_a_duplicate_of_its_descriptor_stays_open` FAILS |

Which message the two failing rows of check 2 print depends on timing: "a
contender found another contender inside the store" when the second process
reaches the witness, or "contender N exited with exit status: 101" with
`database is locked` on stderr when SQLite refuses one of the two
mid-operation first. Both were seen for both patches across runs at
`478fbb5`; the runs at the sync commit printed the first. Either is the
property failing. So is a panic "unknown race log event", which one run at
the sync commit printed while shrinking: two contenders inside the store
wrote the race log at the same moment.

The `flock-only` row passed check 2 as it stood on that branch, which
raced only `open` on a store that was made beforehand: SQLite's exclusive
lock refused the second opener. For `create` the file lock was the only
guard. At 685d846, with the lock file unlinked, two creators on one
directory both returned `Ok` (the probe it15 of the adversarial review of
that commit). Since store-3 the lock is on the directory, where no unlink
reaches it, and since the sixth formal step the creators' race of check 2
fails on a store that takes no directory lock. On that branch the patch
did fail a test of the lib suite.
`db::tests::a_second_take_of_the_writer_lock_is_refused_until_the_first_is_dropped`
fails for `break-single-writer-flock-only.patch` and for
`break-single-writer-both-locks.patch`, because a second take beside a live
lock answers `Ok`. For `break-writer-lock-outlives-store.patch` that test
and
`db::tests::a_dropped_directory_lock_is_released_while_a_duplicate_of_its_descriptor_stays_open`
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
`db::tests::a_dropped_directory_lock_is_released_while_a_duplicate_of_its_descriptor_stays_open`,
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
either. A sketch test of builder-3, which the code does not keep, held a
duplicate of a dropped store's descriptor open, called an `open` that must
fail as `Sealed`, and required that the `open` after it is not refused as
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

## Delete-all, the create and the open after store-3

What the code does since store-3, each sentence with the test that shows it.

- Delete-all runs the root step first, then seals the handle, then removes
  `store.key` (also when its zero-fill fails), the database files and the
  segments tree, every step attempted, through the store's handle and never
  by its kept name (`EncryptedStore::delete_all`, store/expiry.rs). For a
  custody with a roots directory the root step opens the roots location
  once and lists, destroys and reads again through that one handle; it
  makes no second walk of the path and does not call the custody's destroy
  by path. It destroys there only when the directory holds an entry of the
  root that the handle names (the witness), and then it destroys every root
  file of that directory, the store's own first. Otherwise it destroys
  nothing there and the root reads left: a directory put at the roots
  directory's old name is not touched, and a roots directory that left its
  name is not read as destroyed. Once a call on the handle read its root
  destroyed, no later call on that handle opens the roots path again: the
  later call reads its own root as destroyed from the handle's state and
  the other roots from the listing that the earlier call kept, answers
  `Ok` when that listing showed none left and nothing of the store is left
  and `DeleteIncomplete` with those names otherwise, and destroys nothing
  under the roots path; the first formal check puts a root file of another
  id under the roots path before such a later call and requires that it
  stays (ruling ST-45 H3; tests
  `p2_a_resumed_delete_all_keeps_the_root_of_a_store_made_at_its_old_name`,
  `a_later_delete_all_after_the_root_read_destroyed_does_not_open_the_roots_path_again`). For a custody without a roots directory the custody's
  own answer decides. The report reads the root as destroyed, left (with
  its id when the call knows it) or unnamed, the readings of C11.4; the
  line prints `root_destroyed=yes`, `no` or `unknown`. It answers `Ok`
  with a `DeleteReport` when the root reads destroyed or unnamed, no root
  file that the step counted stays and nothing is left, and
  `DeleteIncomplete` with the same report otherwise; a root that reads left
  never gives `Ok` (ruling ST-44 F1). A call after a delete-all that
  answered `DeleteIncomplete` resumes; once a call answered `Ok`, every call
  but delete-all answers `Sealed`, `size_bytes` among them (also while the
  delete runs), and delete-all answers `Ok` with the kept report of the call
  that finished and touches nothing on disk or in the custody (ruling ST-44
  F6; test `calls_on_a_sealed_handle_answer_sealed_and_touch_nothing`).
  Tests:
  `delete_all_answers_ok_only_when_no_root_unwraps_after_the_roots_directory_moved`,
  `delete_all_answers_ok_only_when_no_root_unwraps_after_an_empty_directory_took_the_roots_name`,
  `delete_all_answers_ok_only_when_no_root_unwraps_after_the_data_directory_moved`,
  `p1_with_the_roots_inside_the_store_directory_leaves_no_root_that_unwraps`,
  `delete_all_of_a_moved_store_keeps_the_root_of_the_store_made_at_its_old_name`,
  `delete_all_after_the_data_directory_moved_keeps_the_root_of_a_store_made_at_the_old_names`,
  `delete_all_destroys_every_root_file_under_its_roots_directory_and_names_one_that_stays` (P4),
  `delete_all_on_a_damaged_page_destroys_the_root_and_the_key_file_in_its_first_call`,
  `delete_all_under_query_only_destroys_the_root_and_the_key_file`,
  `delete_all_with_a_blocked_shard_destroys_the_root_and_the_key_file_and_the_tail_finishes`,
  `delete_all_removes_a_store_key_and_a_root_file_of_mode_0400`.
- A link or an entry of another kind at a store name is removed as the entry
  that it is and counted in `not_followed`; the store directory's own name
  counts when it no longer leads to the handle's directory. Tests:
  `delete_all_acts_through_its_handle_and_leaves_a_directory_put_at_the_store_name_as_it_was` (P1),
  `delete_all_opens_lists_and_changes_nothing_behind_a_link_put_at_the_store_name` (P2),
  `the_delete_tail_removes_a_link_at_the_store_name_and_then_sweeps_every_root_file_under_roots` (P3).
- `EncryptedStore::delete_store_dir(store_dir, roots)` is the tail for a
  directory that no handle holds: it takes the directory lock, runs the
  root step for the root that `store.key` or a `create.pending` that reads
  made names (never one that reads owed), removes the store's files in
  every case, and never opens the database. Named, and no directory or no
  listable directory at the roots location: `DeleteIncomplete`, the root
  left, and no file named for removal. Named, and the directory holds no
  entry of that root: `DeleteIncomplete` with the root left as not found,
  because the tail cannot tell a delete that was stopped after its root
  step from a root file that was moved out. Nothing names a root: `Ok` with
  the reading unnamed, also for a tail that finds neither a store directory
  nor a roots directory, and nothing is destroyed under the roots
  directory. So after a delete that was stopped between its root step and
  its file step, the first tail answers `DeleteIncomplete` (not found) and
  the next `Ok` (unnamed). A key file of version 1, of a newer version
  whose version field parses, or of a custody kind that keeps no root file
  under the roots directory is removed with the rest, and its root is
  reported as not handled and never as destroyed; a later tail, with
  nothing left that names a root, answers `Ok` unnamed (ruling ST-44 F1
  (d), (e) and F5). Tests:
  `the_tail_answers_ok_only_when_no_root_unwraps_after_the_roots_directory_moved`,
  `the_second_tail_after_the_roots_directory_moved_does_not_read_the_root_as_destroyed`,
  `the_tail_with_the_roots_inside_a_moved_store_directory_answers_ok_only_when_the_store_is_gone`,
  `the_tail_after_a_delete_stopped_after_its_root_step_answers_not_found_then_unnamed`,
  `the_tail_on_a_key_file_of_a_custody_with_no_root_file_names_the_root_as_not_handled`,
  `a_second_tail_on_a_key_file_of_version_1_does_not_answer_ok_for_a_root_that_no_call_destroyed`,
  `a_second_tail_on_a_key_file_of_version_3_does_not_read_its_root_as_destroyed`.
- `create` records `create.pending` with the reading owed before its
  root, writes it again with the reading made once the custody's
  `create_root` answered Ok and before the key file is published, and
  removes it after `store.key` is published by a hard link; a second write
  that fails sends the create to its abandon, which destroys the root that
  it made. `create`, an open that succeeds and the tail destroy a root that
  a leftover `create.pending` names only when the mark reads made and no
  key file of its own directory names that root; a mark that reads owed,
  that has no reading or that does not parse is removed and destroys
  nothing (ruling ST-44 F3). The delete on a handle removes
  `create.pending` and its temporary file. Tests:
  `a_create_over_a_stale_pending_mark_destroys_no_root_of_a_live_store`,
  `a_stopped_create_under_a_root_id_that_another_create_then_made_keeps_that_live_root`,
  `a_create_killed_between_its_root_and_its_key_file_leaves_a_root_that_the_next_create_destroys`.
- A create whose link meets an entry at `store.key` that it did not make
  (another writer put it there after the create's look) leaves that entry
  as it found it, zero-fills and removes only its own temporary file
  through the handle that it kept, and destroys its own root unless the
  entry at `store.key` names that root id; it removes `store.key` only when
  its own link answered Ok and the name still leads to its own file. An
  entry at `store.key.tmp` that the create did not make and that has more
  than one name is removed without a zero-fill. The state that can stay is
  another party's key file beside no database: an open there answers
  `DatabaseMissing` (ruling ST-44 F4). Tests:
  `a_create_whose_publish_meets_an_entry_at_store_key_leaves_that_entry`,
  `a_create_whose_key_name_is_taken_after_its_root_leaves_what_stands_there`,
  `a_create_whose_key_name_is_taken_by_a_plain_file_after_its_root_removes_that_file`,
  `a_create_removes_a_hard_link_at_store_key_tmp_without_writing_through_it`.
- The root step destroys, and its last reading counts, every entry of the
  roots directory whose name, with its ASCII letters lowered, is a root name
  or a partial name of a root, on every volume; the report names a file that
  stays by the name it has. The removal of `store.key.tmp` is counted:
  `key_file_gone` reads no while it stays. A directory of the store that
  cannot be removed is counted in `files_left`. The tail on a store
  directory that it cannot open, or whose lock it cannot take for a reason
  other than a holder, runs the root step without the lock and answers
  `DeleteIncomplete` with `key_file_gone` no and the store's files as left;
  its root reads destroyed only when the step itself found and destroyed
  it. `Locked` stays `Locked` with nothing done, and on Windows a sharing
  violation at the open of the store directory or of `writer.lock` is
  `Locked` (ruling ST-44 F2, ST-41 (3)). Tests:
  `a_partial_root_file_with_the_root_bytes_that_stays_leaves_delete_all_ok`,
  `a_store_key_temp_with_the_wrapped_key_that_stays_leaves_delete_all_ok`,
  `the_tail_answers_ok_while_a_partial_root_and_a_store_key_temp_stay`,
  `a_second_name_of_a_root_file_of_mode_0400_is_zeroed_before_delete_all_answers_ok`,
  `the_tail_on_a_store_directory_of_mode_000_destroys_the_root_or_reports_it`,
  `delete_all_with_segments_at_0500_answers_ok_only_when_nothing_of_the_store_is_left`,
  `the_tail_destroys_a_root_file_whose_name_stands_in_upper_case_on_a_volume_that_folds_case`.
- The key file's version is read as a signed 64-bit integer before it is
  sorted: 4294967296 answers `NewerStore`, -1 and 0 `OlderStore`, and a
  number outside that range or one that is not an integer
  `KeyFileUnparseable`. On Windows the tail on a store path of which a part
  ends in a dot or a space removes nothing at or beside that path: it runs
  the root step from the key file that it reads through the verbatim form
  of the path, takes no lock there, counts `not_followed` 1 and answers
  `DeleteIncomplete`; a path that has no verbatim form leaves the root
  unread. A roots path of that form leaves the named root unread and listed
  no (ruling ST-44 F8). Tests:
  `a_key_file_version_past_u32_or_below_zero_answers_newer_or_older_store`,
  `the_tail_given_a_verbatim_name_that_ends_in_a_dot_removes_no_sibling_and_reports_what_stays`.
- An open that is refused destroys no root file, writes nothing under the
  roots directory and leaves `create.pending` as it found it (ruling
  ST-43). Tests, each with a `create.pending` of a killed create that names
  another root:
  `a_refused_open_of_a_stopped_create_answers_sealed_and_the_next_create_clears_it`,
  `a_refused_open_of_a_key_file_of_version_3_keeps_the_pending_root_and_create_pending`,
  `a_refused_open_of_a_key_file_of_version_1_keeps_the_pending_root_and_create_pending`,
  `a_refused_open_of_a_key_file_of_another_label_keeps_the_pending_root_and_create_pending`,
  `a_refused_open_of_a_key_file_that_does_not_parse_keeps_the_pending_root_and_create_pending`,
  `a_refused_open_with_no_store_db_keeps_the_pending_root_and_create_pending`,
  `a_refused_open_at_the_custody_keeps_the_pending_root_and_create_pending`,
  `a_refused_open_in_the_database_keeps_the_pending_root_and_create_pending`,
  `a_refused_open_while_another_process_holds_the_database_keeps_the_pending_root_and_create_pending`,
  `a_refused_open_of_version_3_beside_a_create_pending_of_its_own_root_keeps_both`;
  the open that succeeds:
  `an_open_beside_a_create_pending_of_its_own_root_keeps_the_root_and_removes_create_pending`,
  `an_open_beside_a_create_pending_of_another_root_destroys_that_root_and_keeps_its_own`,
  `an_open_whose_cleanup_of_a_stopped_create_fails_answers_ok_and_delete_all_clears_it`,
  `an_open_answers_a_fifo_or_a_link_at_create_pending_and_a_fifo_at_its_root_within_a_bound`.
- The kill table `a_kill_before_each_file_system_call_of_each_operation_leaves_no_key_or_root_behind`
  ends a child before each counted file-system call of write, write then a
  pass, shred, delete-all, create and open (216 calls in the last run of
  the builder on the Mac of the team) and finds each end state true. It
  reads the end state of the unkilled run of each operation too, and of a
  finished delete-all: `store.key` gone, the named root gone and the old
  wrapped key no longer unwraps. A partial root file counts as a root
  file, and a pending mark that reads owed names no root; a row of a create
  killed between its `create_root` and the second write of its mark is the
  named limit (i) of ruling ST-44 F3 and prints it, and one row that starts
  from such a stopped create asserts that limit by name. P1 and P2 read the
  roots directory on disk, and the first formal check counts a missing
  root as destroyed only while the roots directory still stands at its path
  (ruling ST-44 F7).
- On macOS SQLite opens the database under the name
  `/.vol/<device>/<file number>/store.db` of the handle's directory, with
  the two numbers read from the handle at each open and kept nowhere, after
  up to three stats of `/.vol/<device>/<file number>` that follow no link
  (the first, and two more while the volume reads YES and the handle has
  not left its name, `STATS_AFTER_THE_FIRST`): that
  name with the same identity is used; another identity refuses the open;
  `ENOENT`, `ENOTSUP` or `EOPNOTSUPP` give the kept name only on a volume
  that reads NO before and after the stat (ruling ST-39 (1); see L2). Tests:
  `sqlite_keeps_the_volume_name_of_the_handle_for_an_opened_and_a_created_store` (P9),
  `after_a_move_once_open_a_write_a_checkpoint_and_the_close_act_in_the_moved_directory`,
  `a_move_before_the_first_read_refuses_the_open_and_changes_nothing_at_the_old_name` (P10),
  `what_the_first_read_of_a_database_with_no_pages_removes_lies_in_the_moved_directory` (P11),
  `the_stat_of_the_volume_name_chooses_the_kept_name_or_refuses` (P13),
  `a_no_such_name_answer_chooses_the_kept_name_only_on_a_volume_that_reads_no_both_times`,
  `the_three_readings_of_a_volume_decide_yes_no_and_unknown`,
  `the_volume_of_the_tests_reads_yes_and_a_volume_that_reads_no_gets_the_kept_name`,
  `an_old_log_with_bytes_in_the_directory_while_sqlite_holds_another_log_is_refused`.
- Once SQLite holds its files the store looks through its handle: `store.db`
  is a regular file, `SQLITE_FCNTL_HAS_MOVED` answers 0, the name that
  SQLite keeps leads to the handle's entry, the volume name holds the
  handle's numbers and the directory still stands at its name; after the
  first write the log through the handle holds bytes, and after a create's
  checkpoint `store.db` holds at least one page. Any difference answers
  `Obstructed`. Tests:
  `create_answers_obstructed_when_sqlite_holds_a_database_outside_its_handle_and_clears_the_handle` (P6),
  `a_volume_name_that_leads_to_another_directory_is_refused_and_a_create_leaves_nothing` (P12),
  `a_store_directory_removed_while_open_answers_obstructed_at_the_next_read_and_makes_nothing` (P14).
  So a lookup that landed elsewhere at SQLite's open is refused, unless the
  kernel's lookup lands elsewhere a second time, inside the stat of (b),
  while the stat of (c) lands right. The look closes no window in the
  strict sense.
- A code of SQLite at an open or a create is answered after one more look
  through the handle, by one table on the primary code: 11 and 26
  `DatabaseUnreadable`, 13 `Io(StorageFull)`, 5 and 6 `Locked`, every other
  code `Io(Other)` (`code_answer`, store/db.rs).

The swap race of `the_swap_race_counts_in_both_windows_under_the_volume_name`
(P8) has no pass condition. Its counts on the Mac of the team, 20 rounds for
each arm and each window, a directory swapped in at the seam after the last
look and at the seam before the first read: kept name with plain renames,
volume name with plain renames, and volume name with `renamex_np` and
`RENAME_SWAP` at the parent; every round answered `Obstructed`, no round
changed the directory put at the name, and no open used the kept name (the
test asserts that count as 0). The run prints the three readings of this
Mac's data volume: `MNT_DOVOLFS` set, `VOL_CAP_FMT_PATH_FROM_ID` valid and
set, and the root's own volume name answers the root.

## Not covered

- The roots reached by path (ruling ST-44 F1 (f)). At a create, before the
  root is made, the cleanup of a stopped create, `abandon_create` and the
  look for a live root reach the roots directory by its path. The delete
  does not follow a roots directory that was moved: it says that it is not
  finished, and the root that stands in the moved directory can still
  unwrap a copy of the store. Following a moved roots directory with a
  handle held for the store's life is store-3b's.
- A later call on a handle whose root read destroyed (ruling ST-45 H3). It
  reports the roots from the listing that the earlier call kept and opens
  no roots path, so a root file put under the roots directory after that
  call is neither destroyed nor named by the handle; the tail, run after
  the handle is dropped, finishes what was left. The identity of the roots
  directory goes with the held handle to store-3b.
- The root of a key file that this build does not handle (ruling ST-44
  F5). Its name is given once, by the call that removes its key file; a
  root of a key file of version 1 is an item of the login keychain, and its
  cleanup is the owner's (H-9).
- The window after the store's last look on macOS (ruling ST-33 (3)).
  L1: "Read in the kernel's public source at xnu-12377.121.6 (this Mac runs xnu-13432.41.4, whose source is not published): for /.vol/<device>/<file number>/store.db the kernel gets the directory's node by its number, builds that node's path as a string, adds /store.db and walks the string from the root. It does not check that the walk ended in the directory with that number. It tries again only when the walk answers ENOENT, at most five more times. So a move of the directory with another directory put at its old path, or one exchange of two directories, between the build and the walk can make SQLite's open act in that other directory; the name /.vol/<device>/<file number> alone, with nothing after the number, is checked by the kernel and answers that directory or ENOENT."
  The store's look refuses a single landing elsewhere of the database's
  open and of the log's open (ruling ST-39 (2), W1 to W3 and W6, with the
  log checked in two instants: absent or empty after a checkpoint with
  truncation before the first write, holding bytes after it); two landings
  in the order W4 of ruling ST-39 pass it, and their width is the counts of
  P8 above. L2: a no-such-name answer of the directory's stat alone never
  chooses the kept name: the store asks the volume by its mount flag, its
  capability and the stat of its root under its own number, and takes the
  kept name only on a volume that reads NO before and after that answer,
  where the limit of ruling ST-30 (1)(b) stands; this Mac's data volume
  reads YES on all three, so the NO branch is shown by a seam alone. A
  program of the same user that writes into the store directory can make
  both instants of the log's check read as it likes. L3: a directory that is removed, or moved to
  another volume, while a connection is open answers `Obstructed` for the
  store directory; a move to another volume is a copy and a removal, by
  reasoning and not by a run. L4: on Windows every call is by name under a
  handle that is held without delete sharing. L5: not tried: a device
  number after a restart and a second volume; no stale number can be used,
  because the numbers are read from the handle at each open. L6: P9 holds
  the trial's first item in the suite, so a change of the pinned SQLCipher
  runs it again.
- Codes that read `io` (ruling ST-35 (f)). A code that is neither damage nor
  full reads `io` and is tried again for as long as it comes, with Delete
  everything offered: damage that SQLite answers with another primary code,
  a file with another owner after a restore, and the code of
  `sqlite3_initialize`, which is kept for the life of the process. After a
  page that could not be decrypted a later step can answer 7 or 8, which
  read `io`. An open by the kept name tightens whatever directory stands at
  the store's name before it reads `store.key`.
- A second name of a file (a hard link) is no refusal: another process can
  make one after any check, and a store that a backup tool restored with
  shared names still opens. The zero-fill of `store.key` and of a root file
  reaches every name of the file, so that no second name keeps a key
  (`delete_all_zeroes_the_store_key_file_through_every_name_before_it_removes_it`);
  a file of another program that a process of the same user hard-links at
  a root file's name is zeroed with it (found by the review of store-3 and
  left so). Before the zero-fill the root step opens the root file, keeps
  that handle and makes the file writable (the mode on unix, the read-only
  attribute on Windows). If the zero-fill still fails: with one name, the
  count read through the kept handle, the name is removed, and the root
  reads destroyed only when the count read through that handle after the
  removal is 0; with more than one name, or a count that cannot be read, the
  name is kept, the root is left under its real name, and the next call
  tries again (ruling ST-44 F2 (c)). A name made after the count is read
  stays this limit.
- What the review of store-3 found and left as it is, each by its reason. A
  link at a root file's name under the roots directory is removed and not
  followed, and it is not counted in `not_followed`, which counts the store's
  own names. The delete tail does not ask SQLite whether another program
  holds `store.db`, because it never opens the database: a holder that does
  not hold the directory lock is not refused. The tail on a name where no
  directory stands destroys nothing under the roots directory and answers
  `Ok` with the root unnamed (ruling ST-44 F1 (d), which amends ST-30 (1)
  (d)); the daemon's instance lock on the data directory keeps the tail from
  running beside a live daemon. A
  program that takes `store.db` between the probe and the open makes `open`
  answer `Locked` after it asked its custody. A caller that gives a create
  another roots directory than the one where a stopped create made its root
  leaves that root, because `create.pending` names a root id and no roots
  directory.
- The root of a stopped create (ruling ST-43 (5)). A refused open leaves
  it: the next open that succeeds, the next create, or the delete of the
  data directory, which destroys every root file, removes it. Until then
  a root file stands that wraps no key that any key file names, and it
  protects no capture. One sequence leaves it until a person deletes
  again: a delete whose create of step 6 is killed between its root and
  its key file while the mark of ruling ST-34 is set, so that no life
  makes a store.
- The pending mark (ruling ST-44 F3). (i) A create stopped between its
  `create_root` and the second write of its mark leaves a root file, and
  its partial files, that no mark and no key file names. It wraps no key,
  only a delete destroys it, and while it stands a create under that root
  id answers `RootExists`. (ii) A mark that reads made destroys by id: two
  creates under one root id with a delete between them are not covered;
  the rule is a fresh id for each create, and no caller passes an id
  twice. Binding the made mark to the file's identity is store-3b's.

- On Windows the store sets no access rule on its files and directories,
  syncs no directory after a rename or an unlink, and opens a link at a
  name itself; a link that arrives at the log's name after the look stays a
  limit of that system.
- The roots directory of the root-file custody on macOS protects the root by
  its mode alone: a process of the owner, or an administrator who reads the
  owner's files, can unwrap, and a copy of the whole data directory holds
  the roots with the data.
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
- Faults of a delete-all that no move makes. A segment file that cannot
  be removed on Windows: the fault of check 1 is a shard at mode 0500,
  which macOS alone has, and the failpoint `segment_file_remove` stands in
  the keyless sweep and not in the removal that delete-all makes
  (`segment::remove_segments_through`). A root step that fails under a
  custody that names its roots directory: the stand-in of the fault
  `RootStep` names none, so the sweep of every other root file
  (`file_root::destroy_every_root`) does not run in that move, and no move
  puts a second root file under the roots directory of a store; the tests
  `delete_all_destroys_every_root_file_under_its_roots_directory_and_names_one_that_stays`
  and `a_roots_directory_at_0500_destroys_the_root_by_its_zeros_and_seals_the_handle`
  of `store::hardening_tests` read both. Two steps that fail in one call.
  A fault of the expiry path, the key statement and the checkpoint of
  `destroy_segment_keys`, which delete-all no longer runs.
- A delete-all that stopped and whose handle is gone. Check 1 states what
  the store answers then. With `store.key` left, `open` gives
  `DatabaseMissing` and `create` is refused with `AlreadyExists`, so
  neither finishes that directory. Without it `open` gives `Sealed`, and a
  `create` clears what is left and makes a new store. The tail
  `EncryptedStore::delete_store_dir`, which finishes such a directory, is
  called by no move; tests of `store::hardening_tests` read it. The root
  is destroyed, so nothing in the directory can be read, unless the root
  step itself failed, and then the report of the error named the root.
- Two creates under one root id at the same instant, in check 1. Since
  store-3 a root is written under a partial name of its own and published
  by a hard link, which never replaces an entry, so the second writer
  answers `RootExists` whether its look came before or after the first
  root. `store::hardening_tests::two_root_writes_under_one_id_never_both_answer_ok_and_replace_no_root`
  (500 rounds), `two_creates_under_one_root_id_in_two_directories_never_both_answer_ok`
  (300 rounds) and `a_root_that_appears_between_the_look_and_the_write_is_kept_and_the_create_answers_root_exists`
  test it; each failed at the base of store-3. Check 1 has no move that
  starts two creators together.
- A create that fails after its root exists, at the write of the key file,
  where `abandon_create` destroys the root that the call made. No move
  makes that failure.
  `store::hardening_tests::a_create_that_fails_after_wrapping_destroys_the_root_it_made`
  tests it.
- What a root store answers for a root that it cannot read.
  `CreateWhoseRootStepFails` gives `create` a custody that answers so.
  `store::hardening_tests::a_create_under_a_root_of_another_store_that_it_cannot_read_fails_and_leaves_that_root`
  makes four of them on the file-root store.
- A key file that the store did not write. Check 1 damages no `store.key`,
  so it does not read that a wrapped store key of another length than a
  wrap opens as `KeyFileUnparseable`.
  `store::hardening_tests::a_wrapped_store_key_that_decodes_to_another_length_than_a_wrap_opens_as_key_file_unparseable`
  tests it.
- An append that fails at its first row, or whose savepoint cannot be
  released.
- A shred whose walk fails, and a segment write that fails. Check 1 gives
  the store no file system that fails, so the listings and the opens of
  the sweep, and the last look of a write at its shard, run only where
  they succeed. In a probe of the fifth formal step, which is not in the
  repository, a failure at each of the sweep's four calls (the open and
  the listing of `segments` and of a shard) left what a failed removal
  leaves: the shred answered the error, the key was gone and the segment
  read as `Sealed`, its file was still there, and the next pass removed
  it. A delete-all that failed there had destroyed no root, and the next
  call on the same handle finished it. A write whose rename was refused
  and whose partial file could not be removed answered the error of the
  rename and left the partial file and a key row without a key; the next
  shred pass removed the partial file.
  `store::hardening_tests::a_shred_whose_file_removal_fails_still_destroys_the_key_and_the_next_pass_removes_the_file`
  tests the first of these states for a removal that fails.
- Code that check 1 calls in every case and that no row fails through. A
  change there can fail the check, and no row shows it: in `expiry.rs`
  `segment_ids_with_keys`, `remove_keyless_files`, `shred_segments`,
  `crypto_shred_expired`, and `crypto_shred_all`, which counts what
  `delete_all` reports; in `db.rs`
  `reserve_segment_key`, `commit_segment_key`, `KeyFile::read_in`, which
  `KeyFile::read` calls, with `known_version`, `KeyFilePlace::publish`, `KeyFilePlace::destroy`,
  `Db::destroy_through`, `write_pending`, `clear_pending`, `Db::require_file`,
  `Db::require_schema`, `Db::refuse_a_live_holder` where it answers that no
  process holds the store, the read-backs `require_checkpoint_fullfsync`,
  `require_cipher_log_level_none` and `require_pinned_cipher`,
  `Dir::open_lock_file` on Windows, the flags of the database
  opens (`DATABASE_OPEN`, `PROBE_OPEN`) with `database_open_error`, the
  setting in `Db::open_file` that keeps a log which was there before the
  call, and
  `EncryptedStore::{open_with, read_segment}` outside the one call
  that the row of check 2 turns, and the steps of a delete-all after its
  root step, `EncryptedStore::root_step`, `EncryptedStore::remove_files_after`
  and `remove_the_store_files`, where three changed copies of the sixth
  formal step failed the fixed order and none is a row (a delete that
  stops at its first failed step, at move 10; a report that names no
  database that is left, at move 46; a handle left open after a failed
  root step, at move 40); in `durable.rs` `write_durably`,
  `remove_durably`, `no_follow`, `create_private_dir_all`, and every
  function of a handle but the two of the covered table: `Dir::open`,
  `Dir::open_dir`, `Dir::open_file`, `Dir::entry`, `Dir::entries`,
  `Dir::rename`, `Dir::remove_file`, `Dir::sync`, `Dir::names` and
  `Dir::make_private`;
  every function of `segment.rs`, the walk `sweep_segment_files`, the last
  look of `write_segment_file` at the name of its shard and
  `refuse_a_linked_segments_root` among them;
  `DevFallbackCustody::destroy_root`, with `file_root::destroy_root_at` and
  `file_root::destroy_every_root`, which
  no row may mutate (see Break proofs); and the wrap of a key under
  another label than the store's (`FileRoots::wrap` and `FileRoots::unwrap`),
  where a changed copy that seals every other label under an all-zero key
  failed the fixed order at move 10 with "the key file of another label
  under the root of the store still unwraps", and is no row.
- What `open` answers for a store that is not whole. Check 1 opens stores
  that are whole, deleted, stopped in a delete-all, or without their
  database, missing or cut to nothing, where it reads that `open` makes no
  database, changes no file and answers `DatabaseMissing`. It makes no
  store whose database is unreadable, whose key file or
  database has another format version, or whose settings do not read back.
  So it does not read that `open` changes no file there and
  answers `DatabaseUnreadable`, `NewerStore`,
  `OlderStore` or `SettingNotInEffect`. Nor does it read that an `open`
  which answers `NewerStore` or `OlderStore` leaves a log that was there
  before the call as it found it, whatever its length. Such an `open`
  neither checkpoints nor truncates, so a frame that the log held before
  the call is in it after. A shred that returned `Ok` has left a log of 0
  bytes, so a log that holds a frame of a shredded key's page is the log
  of a shred that stopped between its commit and its checkpoint (the entry
  above on a process crash) or a file that was put there, and property 1
  says nothing of that directory, before the refused `open` or after it.
  Tests of `store::hardening_tests`
  read these, among them
  `open_answers_a_missing_or_empty_database_as_database_missing_and_makes_no_file`,
  `open_answers_a_database_that_its_key_does_not_open_as_database_unreadable`
  and
  `open_refuses_a_format_version_that_this_build_does_not_know_and_changes_no_file`.
  `db::tests::pragmas_are_set` reads the settings (the row
  `journal-mode-not-wal`, and `os-entropy-hook-missing` in
  `crates/halvo-sqlcipher/mutants.rows`, whose verdict is a test of that
  crate).
- Links. One move of check 1 puts a link into the store: a symbolic link
  below `segments`, on macOS, which the sweep and delete-all must not
  walk. No move puts a link at the store directory's own name, at
  `store.key`, at `store.db` or in place of a shard that holds segments,
  so check 1 does not read the handle rule there. Under the link that it
  does make, the handle rule is a second layer: with the test of the entry
  kind turned in the walk of a delete-all, the open of the entry through
  its handle still refuses the link, and nothing outside changes; the row
  of that walk fails because the delete then counts the link as left. A
  changed copy of the sixth formal step whose sweep walks a link by its
  name removed the file outside, and the fixed order failed on it at move
  64 with "the directory outside the store, to which a link below segments
  leads, held ... and now holds []"; it is no row. The store takes one handle of its
  directory at `create` and at `open`, as a directory, and refuses a link
  at the directory's own name; a link in an earlier part of the given name
  is followed there, once. It names the database under the name that the
  system gives for that handle, which holds no link. It opens `segments` relative to that handle, a
  shard relative to `segments`, and `store.key` or a segment file relative
  to its directory, each in one step that refuses a link and, for a file,
  a name that is no regular file; it lists a directory through its handle,
  and creates, renames and removes a segment file, and creates a new
  `store.key` and a new `store.db`, by name relative to the handle that it
  walked or checked. Every connection of the store, the probe for a holder
  among them, is opened by one function, `open_connection`, which carries
  the caller's flags (`SQLITE_OPEN_NOFOLLOW` and no `SQLITE_OPEN_URI`).
  Before a segment write renames its partial file, it reads the shard's
  name relative to `segments` and the partial file's name relative to the
  shard, without following a link, and goes on only when the first names
  the same directory and the second the file that it wrote (device and
  inode, on Windows the volume serial number and the file index);
  otherwise it answers `Corrupt` and tries to remove the partial file
  through the shard's handle. It reports no removal that fails: the
  partial file then stays in that directory until a later sweep removes
  it. Every step of `create` that tests, opens or
  removes an entry acts relative to the handle: the read of whether
  `store.key` is there, which takes any entry at that name, a link among
  them, as a store; the removal of the database files, of a `segments`
  tree and of a `store.key.tmp` that were left; and the removal of the
  database files after a create that fails. Before SQLite opens the
  database by its name under the handle's name, at `create`, at `open` and
  for the probe for a holder, `open_connection` reads `store.db`,
  `store.db-wal`, `store.db-shm` and `store.db-journal` through the handle
  without following a link and without a wait (a link, on Windows also a
  junction or another reparse point, and a name that is no regular file
  answer `Obstructed`), and then reads the handle's name
  without following a link and goes on only when it still stands for the
  handle's own directory (device and inode); otherwise it answers
  `Obstructed`. On Windows it holds `store.db` from that look until the
  connection is open. Since store-3 none of the store's own calls stays
  by name on macOS: the lock is taken on a descriptor of the directory,
  and the removal of `store.key` and every step of delete-all act through
  the handle (see "Delete-all, the create and the open after store-3").
  SQLite's own open of `store.db` and of its log is by a name, on macOS
  the volume name of the handle's directory; the first entry of this
  list gives what a move of the store directory after the last look can
  still do. On Windows a directory of the chain and `store.db`, from its
  checks until the connection is open, are held by a handle opened with the
  reparse-point flag and without delete sharing, so no other entry can take
  their names. The tests of `store::hardening_tests` whose names start with
  `the_keyless_sweep_` read the sweep, two of them with a shard swapped for
  a link inside the walk and after it;
  `a_segment_write_puts_no_file_through_a_shard_that_links_out_of_the_store`
  and the two tests after it read a segment write, with a shard swapped at
  the create of its partial file;
  `delete_all_unlinks_a_store_key_that_is_a_link_and_writes_nothing_through_it`
  and `create_makes_no_file_through_a_writer_lock_that_is_a_link` read the
  other two;
  `create_clears_through_its_handle_and_leaves_the_files_of_a_directory_put_at_its_name`
  reads `create` with its directory moved away between the handle and the
  removals and another directory put at the name, and
  `create_takes_a_link_at_store_key_as_a_store_and_follows_it_nowhere` its
  read of `store.key`; the tests that start with `open_answers_a_store_key_` and
  `a_segment_read_answers_` read a link and a FIFO at `store.key` and at a
  segment file, each within 5 s;
  `open_refuses_a_store_db_that_is_a_link_before_its_custody_and_writes_nothing_through_it`
  reads a link that is there before the custody call, and
  `open_refuses_a_link_put_at_store_db_inside_the_custody_call_and_changes_nothing_beside_its_target`
  one put in place inside it. A move after the store's last look is the
  other process's own act. After that look SQLite's own steps by name act
  in whatever directory stands at the name, among them: its open of
  `store.db`; the writes of a `create` into the database that it finds
  there; and the removal of the log of a database with no pages at its
  first read. Two more are read in the pinned source and shown by no run:
  the log is opened by its name with the create flag at the first read,
  with no second walk of the name (sqlcipher.c lines 69570 to 69572), and
  a file named `store.db-journal` of a database with no pages is removed
  by its name (lines 65118 to 65121). At its close SQLite checkpoints and
  removes the log by name only while the name still leads to the file
  that it holds, read by the file number (lines 41951 to 41958); after a
  move it does neither. The store's own calls act through the handle. What
  was moved away is ciphertext whose key a shred destroys, and whose root
  a delete-all destroys with every other root file under the roots
  directory. The review's two runs W1 (pr-review-14), taken before
  store-3, are the
  facts of the first three steps: with another directory put at the name after the last look, a
  `create` answered `Ok` and that directory, which had held an empty
  `store.db`, held a `store.db` of 4,096 bytes and a log; an `open`
  answered `OlderStore`, and that directory lost its `store.db-wal` of 21
  bytes. On Windows a link that arrives at the log's name after the look
  and before SQLite makes the log is the same limit.
  Two counted races are ignored in the default run and run by hand when
  that code changes. Their counts are no measurement of a rate; they come
  from this Mac at a 1-minute load near 100.
  - 300 opens while a thread swaps `store.db` between a hard link and a
    symbolic link to the same file outside the store, and no open leaves a
    log beside the target:
    `cargo test -p halvo-capture --lib -- --ignored --exact store::hardening_tests::no_open_goes_through_a_link_that_a_thread_swaps_in_at_store_db --nocapture`.
    The last run: `opens=300 swaps=117 ok_with_a_log_beside_the_target=0
    ok_inside=6 refused_as_corrupt=294`. The same test at 09943666, before
    the handle rule: `ok_with_a_log_beside_the_target=11` of 300.
  - 3,600 sweeps while a thread swaps a shard for a symbolic link to a
    directory that holds a file with a segment's name, and no sweep removes
    that file:
    `cargo test -p halvo-capture --lib -- --ignored --exact store::hardening_tests::the_keyless_sweep_removes_nothing_outside_while_a_thread_swaps_a_shard_for_a_link --nocapture`,
    with its control, which swaps in a plain directory:
    `store::hardening_tests::the_keyless_sweep_race_control_with_a_plain_directory_swapped_in`.
    The last runs: `sweeps=3600 racer_swaps=355 removed_outside=0`, and the
    control `sweeps=3600 racer_swaps=591 removed_outside=0`. At 09943666 the
    race removed the file outside once in 3,600 sweeps, and its control
    removed it in none.
- The mode of an entry that exists. Check 1 makes its store in a new
  directory that the test made at 0700. So a store whose `create` makes
  its directory with the mode of the umask (`create_private_dir_all`), or
  whose `open` and `create` do not narrow a wider directory
  (`Dir::make_private`), passes it.
  `store::hardening_tests::every_store_file_is_made_0600_and_every_directory_0700_under_umask_022`
  tests the modes in a child process with that umask. It passes the first
  of those two stores as well, because `create` narrows the directory on
  its handle right after it makes it (one run of the fifth formal step),
  so nothing reads the mode that `create_private_dir_all` gives.
  The store sets no mode on a `store.key`, a `segments`,
  a shard, a segment file or a log that exists: each keeps a wider mode
  that it had, except an empty log, which SQLite sets to the mode of
  `store.db` when it opens it, at an `open` that it refuses among them.
  Of the entries that exist, check 1 reads `store.db` alone, which its
  `Reopen` widens.
- A FIFO at a store name. No move of check 1 puts one there. Since
  store-3 no call of the store opens one: the lock is on the directory,
  and delete-all removes an entry at `store.key` that is no regular file
  without opening it.
  `store::hardening_tests::a_socket_a_directory_and_a_fifo_at_store_key_answer_obstructed_without_a_wait`
  reads it.
- A second name of a file (a hard link). Another process of the same user
  can give `store.db`, its log or a segment file a second name outside the
  store after any check; the store does not refuse a file by its count of
  names, because a backup tool can restore a store with shared names, so a
  second name keeps the bytes that the store wrote, after delete-all too
  (contract C11.3, Links).
- The order of a durable write and of a durable removal: the sync of the
  file, the rename, the sync of the directory (`write_durably`,
  `remove_durably`). Both syncs are std's `File::sync_all`, the file's on
  its own descriptor and the directory's on a duplicate of the handle's
  descriptor, never by a name; on macOS that call is `fcntl(F_FULLFSYNC)`,
  tried again when a signal interrupts it. No test can see which system
  call a sync makes. Check 1 reads what a write and a shred leave in a
  process that keeps running, and makes no power loss. The tests of
  `store::durable` read the order over a recording file system, and
  `store::hardening_tests::a_shred_clears_the_key_durably_before_it_removes_the_file_and_syncs_the_shard_after`
  reads over the store that a shred removes a file only when its key is
  gone from `store.db` and its log, and syncs the shard after. No row
  turns the order; changed copies of the fifth formal step failed these
  tests for a removal before the checkpoint, for a missing directory sync
  and for a publish before the file's sync. The recording file system
  names an entry by the name of the handle and the name of the entry. A
  handle keeps its directory when that directory is moved, so the recorded
  name and the entry that the store acted on part then. No read of check 1
  uses a recorded name: check 1 gives the store no recording file system,
  and it moves no directory.
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
- One lock layer in the open race. On macOS the open race of check 2
  passes a store that takes no directory lock, and passes a store without
  the exclusive mode, because the other layer refuses every second opener
  (one run of each at the sixth formal step). The creators' race reads the
  first layer and the cases around the directory lock read the second.
- The directory lock on Windows by a row. The rows run on macOS, so no
  run shows the creators' race failing on a Windows store that takes no
  lock on `writer.lock`; the unmutated race runs in the Windows job.
- What a sharing violation at `writer.lock` should answer. On Windows a
  program that has the lock file open in a way that conflicts with the
  store's open, as the unlink of check 2 has for an instant, makes `open`
  and `create` answer `Io` with a kind that no caller can name. The checks
  take it as a refusal of the moment; whether the store should answer
  `Locked` for it, or try again, is not decided here.
- The `DatabaseLocked` half of the mapping in `verify_key`. With that half
  alone removed, check 2 passes: a second process gets `DatabaseBusy`.
- `create` beside a store that holds data. The creators of check 2 race
  on an empty directory, where the one that got the store holds one
  segment while the others are refused; check 1 tries `create` beside the
  live store of its own process. No case starts a creator beside a store
  of an earlier run that another process holds or has left.
  `db::tests::create_refuses_a_directory_that_already_holds_a_store` reads
  the second.
- A refusal before the custody where no hold covers the open. Check 2 reads
  the order only for an open that began and returned inside one hold of
  another contender. An open that overlaps the start or the end of a hold
  may ask its custody and be refused by `verify_key`, in the store as it
  is, so the check takes that. A store that asks its custody first only at
  those edges passes.
  `store::hardening_tests::a_held_store_answers_locked_before_its_custody_is_called`
  counts the custody calls of one open of a held store.
- Two writers in different user accounts, or over a network filesystem.
- The unlock call in `DirLock`'s `Drop`, as distinct from the release of
  the lock when its descriptor closes. The patches under Break proofs
  showed on the lock file that check 1 fails when the lock outlives the
  store, and that only check 1 notices a missing unlock, and only when a
  drop and the next `open` or `create` both
  fall between a child's spawn and its exec; no run of them was taken on
  the directory lock. Three tests named under the
  break proofs test the unlock itself, on Unix only: the two
  `hardening_tests`, and
  `db::tests::a_dropped_directory_lock_is_released_while_a_duplicate_of_its_descriptor_stays_open`,
  which has no timing in it.
- A forked child that runs destructors and drops its copy of the store:
  `DirLock`'s `Drop` then unlocks the lock that its parent still
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
  runs `sh docs/verify/check-covered.sh`, and `12-test` runs the eight tests.

On each push to main, `30-mutants.sh` also runs mutants in full, in an
order that the commit id decides. It starts no row once its budget of
seconds (`budget_s`) has passed since the first row started, lets the row
in progress end, and fails when a row that ran is not caught by its
verdict; another commit takes the rows in another order. The whole list,
which requires that the test of every `formal-` row fails, runs on a
person's machine as a line of the phase's gate and of the verifier gates of
the units that name `mutation-test.sh`.

Outcome of the last pass, the fourth formal step, over the 17 commits of
the unit store-2 (an `open` that makes no database, format versions, the
settings read back, the sweep that follows no link, the modes, and
`Locked` before the custody): updated, for both checks.

Check 1 stands on the changed `open`, `create` and write paths, and it
passes. One of its rows had come to fail for another reason than its line
gives: the formal row of the journal mode, which now turns the read-back with
the pragma (see Break proofs). Check 1 gained the read of the modes, with
two rows on `durable.rs`.

Check 2 passes, and its three older rows fail as before. A held store
whose lock file was unlinked is now refused by `Db::refuse_a_live_holder`,
before the custody, and no longer by `verify_key`. The check as it stood
did not read that order: it passed a store that drops the answer of
`Db::refuse_a_live_holder` and refuses after the unwrap. It gained that
read, with the row `formal-held-store-asks-its-custody`.

One finding is about the store and not about the checks: the open of the
lock file under an unlink. Check 2 as it stood before
this pass, alone, in 376 runs on a 16-core Mac at a 1-minute load of
18 to 37: 4 runs failed, each with `Io(NotFound)` from the open of one
contender, in a case with an unlink cadence. `WriterLock::take` alone, in
a changed copy with a probe test: 9 answers `Io(NotFound)` in 1,703,204
takes from two or five threads while another thread unlinked the file
every 1 or 7 ms, none in 142,013 takes from one thread, and no failure of
the narrowing of the directory in 1,845,217 calls.

The cause is the open of `writer.lock` with the create flag. When two
processes are in that open while a third unlinks the name, macOS can answer
it with "no such file". The narrowing of the directory before every take made
that about 36 times more frequent: in a changed copy, 8 in 1,450,310 takes
with it and 1 in 6,490,549 without it. Since the fix of ruling ST-23,
`WriterLock::take` tries that open again while it answers "no such file",
at most 4 tries in all, with no sleep and no clock. The narrowing before every take
stays. A directory that is gone answers so on every try, and `open` still
answers `Io(NotFound)` for it
(`db::tests::a_take_of_the_writer_lock_in_a_directory_that_is_gone_answers_not_found`).
What holds since then, counted on the same 16-core Mac:
`db::tests::a_take_of_the_writer_lock_never_answers_not_found_while_another_thread_unlinks_the_file`
takes the lock 1,000,000 times from five threads while another thread
unlinks the file every 1 ms. Without the retry 7 takes answered
`Io(NotFound)`, the first at take 182,838; with it, none. Check 2 alone:
4 failed runs in 376 before the fix, none in 300 after it, at a 1-minute
load of 41 to 95. That test asserts only what every run shows: no take
fails. Since ruling ST-24 (b) it is ignored in the default run, because it
costs 30 to 50 s and decides nothing that the seam tests below do not. Run
it by hand when the code of `WriterLock` changes:
`cargo test -p halvo-capture --lib -- --ignored --exact store::db::tests::a_take_of_the_writer_lock_never_answers_not_found_while_another_thread_unlinks_the_file --nocapture`.
From store-3 on, macOS takes the lock on the store directory itself and no
lock file exists, so this test, the retry, its seam tests and the row
`lock-file-open-not-retried` are gone. Check 2 does not fail through the
directory lock alone or through the database's `locking_mode = EXCLUSIVE`
alone, because either one refuses a second process: its rows remove both,
and the rows `directory-lock-never-refuses`,
`directory-lock-refusal-taken-as-a-lock` and
`live-holder-busy-not-mapped-to-locked` judge each part alone by a test
that sees it. What follows is the record of the
lock file as it stood until then.
Its last run, at the commit that ignored it, on this Mac at a 1-minute load
of 98 to 107: 1,000,000 takes, 14,469 unlinks, no take that answered
`Io(NotFound)` and no other error, in 43.5 s. Since ruling ST-24 the retry is decided by a seam and draws nothing
from the system: `WriterLock::take` opens the file through
`WriterLock::take_with`, which takes the opener as a parameter, and a test
passes an opener whose first k opens answer "no such file".
`db::tests::a_take_tries_the_open_of_the_lock_file_again_while_it_answers_not_found`
requires that k of 0 to 3 take the lock after exactly k + 1 opens and that
k of 4 answers `Io(NotFound)` after exactly 4, and
`db::tests::a_take_hands_on_an_open_error_of_another_kind_after_one_try`
requires one open for another kind of error. The row
`lock-file-open-not-retried` of `store/mutants.rows` takes the retry out,
and the first of those tests fails in every run. On
Windows a delete of the name while a holder keeps it open leaves it
delete-pending, and the open answers access denied, not "no such file", so
the retry changes no answer there.

The fourth formal step read the fix at its head. Both checks pass, and
check 2 alone passed 100 more runs of 100, at a 1-minute load of 48 to 59.
The retry admits no second holder. A try keeps nothing of the try before
it, so a take with the retry ends as a take without it that began later
could end: with the lock on the file that had the name at that moment, or
with an error. A process whose locked file was unlinked, beside one that
locked a new file of that name, is the case that check 2 makes in every
case with an unlink cadence, with or without the retry, and
`Db::refuse_a_live_holder` and the database lock refuse the second of
them. Check 2 does not decide the retry itself; the seam tests above do. How
often a second try fails was counted outside the store, with the same two
system calls from five processes while a sixth unlinked the name every
1 ms: the first try answered "no such file" in 23 of 2,867,815 takes, and
the second try in none of those 23.

The fourth formal step read the eight commits after the comment pass and
the round of ruling ST-25. They change no code that a property, a read or a
row of this file stands on, with one addition to the path of every open:
`Db::refuse_a_linked_file`, after the read of the key file and before the
holder probe and the custody, which answers `Corrupt` for a `store.db` that
is a link. Both checks run it in every open, on a `store.db` that is no
link, and no move makes one that is (see Links under Not covered). Since
that round `durable::no_follow` exists for macOS and Windows alone, with
the code that each of the two systems had before. Both checks pass, check 2
alone passed 20 more runs of 20, and the 28 rows whose lines those
commits moved are caught as before, each by its test.

The fifth formal step read the six commits of the round of ruling ST-26,
which rebuilt how the store reaches its files: every entry through a
handle of its directory (`durable::Dir`), the database opens with their
flags written out, the mode of `store.db` set on the store's own
descriptor, and a log that a refused open leaves as it found it. Outcome:
updated, for check 1; in sync, for check 2.

Both checks pass. Check 2 alone passed 20 more runs of 20 at a 1-minute
load of 128 to 150, with 62,168 refused opens whole inside a hold and none
after the custody. It reads the order that it read before: `open` takes
the handle of the directory first, before the lock, and the probe for a
holder still answers before the custody. The nine rows whose text the
round changed, or whose lines stand within three lines of a changed hunk,
are caught, each by its test.

Three things had drifted. Check 1 did not decide the new sentence on the
mode of `store.db`: it passed a copy of the store that sets no mode
there. Its `Reopen` now widens `store.db` first, and the row
`formal-store-db-keeps-a-wider-mode` fails through `open_database`. The
row `formal-store-directories-take-the-umask` held a statement on
`create_private_dir_all`, through which check 1 no longer fails; the
statement is cut, and the function stands under Not covered. And
sentences of this file said more than the code keeps, or named code that
the round renamed: the sentence on modes in property 1, the list of what
stays by name under Links, and the list of code that no row fails
through.

No sentence of property 1 rests on a name that the recording file system
records, and no move that fails or is refused in check 1 reaches the two
new steps of a segment write or a failing call of the sweep (see Not
covered for what a failure there leaves).

The sixth formal step read the 18 commits of the unit store-3: the root
in a file under a roots directory on both systems, delete-all with the
root first and every step tried, `create.pending` and a key file that is
published create-only, the lock on the directory, the volume name for
SQLite on macOS, and the look after an open. Outcome: updated, for both
checks.

Both checks passed at the head of the unit as they stood, and check 2
alone passed 20 runs of 20. Five things had drifted.

Check 1 read no error of a delete-all. Its `ShredAll` took `Ok` alone, and
its one move with a fault read two fields of the report. It passed all of
its tests on a copy of the store in the order that the store had before
store-3, the shred of every segment before the root step (one run, nine
tests). With the op `ArmFault` and the step as it is stated now, the
fixed order fails that copy at move 30, under a damaged page, and the
row `formal-delete-all-returns-before-its-root-step` keeps it.
`formal-delete-all-destroys-nothing` had come to fail at an `expect` of
the harness on the answer of the call, and fails at the read of the root
again.

Two rows of the root step of a create did not build: `abandon_create`
takes the place of the key file since store-3, and their substitutions
called it without. With the call repaired, they and
`formal-create-takes-an-existing-root` failed only at the stand-in custody
of move 25 and not at move 6 (see Break proofs); two of the three turn the
look for a root file as well and fail at move 6 again.

`formal-checkpoint-not-verified` passed the random test of check 1 (one
run): its first statement landed in `Db::log_drained`, which store-3 gave
the text of the checkpoint statement, and left the checkpoint of a shred
as it is. The statement is tied to `Db::checkpoint_truncate`.

Check 2 passed a store that drops the answer of
`Db::refuse_a_live_holder`. On macOS the directory lock refuses every
second opener before the probe, and nothing can take that lock away, so
the open race reads neither the probe, nor `verify_key` behind a held
store, nor the exclusive mode alone. The open race keeps one row,
`formal-single-writer-both-locks`. The cases around the directory lock
read the second layer and the place of the probe, with three rows.

No case started two creators. The creators' race does, and the row
`formal-directory-lock-never-taken`, the patch `flock-only` of the first
runs, fails it while the open race and the cases around the lock pass.

And sentences of this file named the lock file, `WriterLock`, answers of
a link as `Corrupt`, steps of delete-all by name and a move of the fixed
order that is gone; they are corrected.

On Windows the open race had drifted as well, and only the hosted job
showed it: since store-3 opens `writer.lock` without delete sharing, the
parent's unlink no longer leaves the name delete-pending (access denied,
which the contender took) but can refuse a contender's open with a
sharing violation, which it did not take. One case in the one hosted run
of this step failed so, "contender 0: open failed with Io(Uncategorized),
which is no refusal of a second writer". The contender takes that answer
now and the line of a pass counts it (`sharing_violation`).

The same step built what the fourth formal step had handed on. Check 1
has a move that opens a store without its database (taken away, or cut to
nothing) and reads the answer and the listing; a move that puts a link
below `segments` with a watched directory outside the store; and a read
of the modes inside a create, where `create.pending` stands. Each has a
row. The rows of the link and of the modes passed the fixed order as it
stood before their moves (one run each). The row of the open passed the
fixed order of the unit's head and was already refused before its move by
the reopen of a stopped store, which this step made exact: with the key
file left and the database gone, an `open` that makes a database goes on
to its custody and answers `RootMissing`. The new move refuses it on a
live store, with the listing. The settings keep their proof in
`db::tests::pragmas_are_set`.
