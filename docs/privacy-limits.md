# What expiry and delete-all cover, and what capture stores

This file states the limits of Halvo's privacy promises in plain words. It is
decided that the README (docs-2), the data view and the consent text (app-3 and
app-4) quote it and do not restate it; at this version none of them quotes it. ADR 0005 states
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
| Delete-all | Nothing decrypts: delete-all destroys the root first (RD-20; store-3 lands the root step, and daemon-3d lands Delete everything on it) and then removes the key file, the database and the segments, and after it even the older files of the next column open Sealed (evidence/interrogate-rr-store-1/09-rollback.log, control line). | Nothing decrypts: the older files open Sealed, because the root they need is gone (evidence/interrogate-rr-store-1/09-rollback.log, control line). | Everything decrypts, by reading the code on both systems, because both use the same root-file code. Not run: store-6 ports the rollback test and win-5 runs the restore test on Windows. |

Limit of delete-all: Delete everything destroys every key file in Halvo's data folder, not only the one of the store in use. So if the store folder inside it was moved away or replaced, for example by a shortcut, what was recorded there cannot be read again, wherever those files are. Halvo removes nothing in a place that is not its own folder: files may be left there, and you can remove them. One exception: a copy or a move of Halvo's whole data folder, with the folder roots inside it, takes the key files with it. Such a copy can still be opened, and Halvo cannot reach it. Source: the store tests `delete_all_acts_through_its_handle_and_leaves_a_directory_put_at_the_store_name_as_it_was`, `delete_all_opens_lists_and_changes_nothing_behind_a_link_put_at_the_store_name`, `the_delete_tail_removes_a_link_at_the_store_name_and_then_sweeps_every_root_file_under_roots`, `delete_all_destroys_every_root_file_under_its_roots_directory_and_names_one_that_stays`, `the_tail_cleans_a_directory_with_a_zeroed_key_file_and_a_fresh_create_takes_it` and `the_tail_answers_each_arm_as_its_row_says_and_leaves_the_roots_directory_as_its_row_says` (store-3, P1 to P4 of ruling ST-30, and ruling ST-45 H1).

If the folder roots was moved or replaced while Halvo ran, Delete everything says that it is not finished and names the key file it could not find. If you choose Delete everything again after Halvo starts again, it destroys every key file in the folder that now stands at that place and can say that it is finished; the key files in the moved folder can still open the recordings that were moved with them.

Limit on a Mac: A key that you press before a program or page has reacted, for example while it is frozen or still loading, can be noted even if it ends up in a password field: when it was pressed, in which app and what kind of key, never which letter or digit. To be sure, pause capture before you type a password.

Where the custody root lives decides the last column. An unsigned build keeps the
root in a per-user root file in the `roots` directory of the data directory, on both
systems: on macOS under `~/Library/Application Support/Halvo/roots/` (store-3 and
daemon-3 land it), on Windows under `%LOCALAPPDATA%\com.halvo.desktop\data\roots\`
(control-1 lands the data directory). So a copy of the whole data directory holds
the root, and a copy of the store directory alone does not. Builds before store-3
kept the macOS root in the login keychain, which the Time Machine exclusion does not
cover (`tmutil isexcluded` prints `[Included]` for the login keychain).

R1 records this limit and builds no per-day key. Halvo does not set the Time
Machine exclusion mark yet, so Time Machine copies the data directory like any
other. From daemon-3a on, Halvo sets the mark on the data directory when the daemon
starts. The mark stops Time Machine from copying the data directory and stops no
other copy.

A Secure Enclave custody would keep the root outside every copy of the directory.
R1 ships none, and no R1 installer is a build with the `secure-enclave` feature. It
is decided that such a build creates no store and opens an existing one for
maintenance only: store-6 removes the Secure Enclave skeleton and daemon-3 lands
the policy (ADR 0010).

## What else stays after a shred

- Free blocks of the volume. Right after a shred the wrapped key of the segment came
  back from the raw volume image, in a WAL frame and in a database page, in 6 of 6
  runs on APFS, and it was gone from both after 5 MiB of further capture
  (evidence/interrogate-rr-store-3/it14_raw_volume_apfs_run1.log to
  it14_raw_volume_apfs_run3.log, and the three logs of the same names under
  rerun/). A reader of those blocks also needs the owner's root.
- The Time Machine mark. Halvo does not set it yet, so Time Machine copies the data
  directory like any other. From daemon-3a on, Time Machine skips the data
  directory. A sync client, another backup tool, Migration Assistant and a copy made
  by hand are not covered by the mark.
  Whether a local APFS snapshot holds the directory is not verified.
- Segment files. From store-4 on, a file name carries no time, because file ids are
  random. The size of each file, its file times and the segment's number in the
  clear header remain, and they show when the person was active. R1 accepts this
  and pads nothing (RR-store-25).
- Key rows. The ids and times of shredded segments stay in store.db until
  delete-all, about 124 bytes a row (RD-21). The ids that stay are in
  evidence/interrogate-rr-store-3/it03_what_a_shred_leaves_inside_store_db.log.
- Rows of a segment shredded out of order (RR-store-08). store-4 closes this when it
  removes the per-event rows; until then the note in the table applies.

## What capture stores and does not exclude

Each sentence names the unit that makes it true.

- The title of a private window of Chrome, Edge, Firefox or Safari is not stored when
  the browser runs with an English interface, and the title of any other private
  window is stored (macos-4 and win-4). A source tells a private window by the ending
  that the browser gives its title, and only for the browser that the ending belongs
  to: on macOS `(Incognito)` for Chrome, `(InPrivate)` for Edge, an em dash and
  `Private Browsing` for Firefox and `, Private Browsing` for Safari; on Windows
  `[InPrivate]` for Edge and `Mozilla Firefox Private Browsing` for Firefox. A page
  whose own title holds the word private is kept, because a source matches the
  ending and never the word alone.
- On Windows the title of a Chrome window does not tell a private window (it read
  `PAGE - Google Chrome` in 3 of 3 runs). win-4 asks the window instead, through UI
  Automation, and drops the title of every Chrome window that the query does not
  tell as not private.
- Limits of that rule. The endings are those of an English interface, measured on
  the browser versions of the two hosted images: Chrome 157 and 156, Edge 156,
  Firefox 159 and Safari 27.2. With another interface language, another browser or a
  title that ends otherwise, the window is not told and its title is stored. On
  macOS the trial read the title through the accessibility interface, which is how
  macos-4 reads it; a read from the window list was not tried.
- A terminal that does not assert secure input gets no protection, so key classes
  and the window title are recorded while a password is typed there (macos-4).
- Halvo records no keystroke typed while secure input is held: a key event that
  reaches it in that time is not recorded and is only counted by type (macos-4).
  Line (c) of the gate is the check that shows it, and gate-3 builds that line
  (contract C4 item 13).
- Characters and key codes are never stored: the types that core-2 builds have no
  field for them.

The first-run text and the data view quote these three sentences word for word.
The owner approves them once, in the table of user-visible sentences for each
system; until then they are the recommended text.

- `Typing is not observed on Windows in this version.` win-3 keeps the Windows
  keyboard source out of the default source set.
- `Screen activity is not recorded.` core-2 builds the switch `screen_activity`
  with the default off; the sentence holds while the switch is off.
- For an unsigned macOS build: `This is an unsigned build: the key that protects your data is kept in a file with owner-only permissions in your account.`
  store-3 and daemon-3 make it true.

docs-2 confirms each sentence of this section against the merged code.

## What the derived store keeps after a removal

Note of 2026-11-13: when Halvo removes something from what it has learned about your
work (because it expired, because the recording behind it was removed, or because you
dismissed or deleted a workflow), a copy of the removed text can remain inside Halvo's
encrypted data file, with no time limit, until Halvo happens to reuse that space. The
same holds for an earlier value that Halvo replaced in place, as when you approve a
workflow or a later batch changes one. Any program that runs under your account can
find it there, and on a Mac so can anyone who has a copy of Halvo's whole data folder;
someone who has only the file cannot read it. A delete of everything that removes the
folder derived/ with its key leaves no such copy; no code of this version does that.
The mend is Halvo's own build of the database, which zeroes that space: the capture
run's unit sqlcipher-1 builds it (ruling S-82). U5.2 of the workflow run deletes this
note, when the mended build of the database and its test are on main.
