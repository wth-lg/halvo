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

Limit of delete-all: Delete everything destroys every key file in Halvo's data folder, not only the one of the store in use. So if the store folder inside it was moved away or replaced, for example by a shortcut, what was recorded there cannot be read again, wherever those files are. Halvo removes nothing in a place that is not its own folder: files may be left there, and you can remove them. One exception: a copy or a move of Halvo's whole data folder, with the folder roots inside it, takes the key files with it. Such a copy can still be opened, and Halvo cannot reach it. A key file that another program copied, or renamed inside the folder, is such a copy: Halvo does not find it, and with an earlier copy of the recordings it can still open them. Source: the store tests `delete_all_acts_through_its_handle_and_leaves_a_directory_put_at_the_store_name_as_it_was`, `delete_all_opens_lists_and_changes_nothing_behind_a_link_put_at_the_store_name`, `the_delete_tail_removes_a_link_at_the_store_name_and_then_sweeps_every_root_file_under_roots`, `delete_all_destroys_every_root_file_under_its_roots_directory_and_names_one_that_stays`, `the_tail_cleans_a_directory_with_a_zeroed_key_file_and_a_fresh_create_takes_it` and `the_tail_answers_each_arm_as_its_row_says_and_leaves_the_roots_directory_as_its_row_says` (store-3, P1 to P4 of ruling ST-30, and ruling ST-45 H1).

If the folder roots, or a key file in it, was moved, renamed or replaced while Halvo ran, Delete everything says that it is not finished and names the key file it could not find, unless a program put a file with that key file's name into the folder that stands there now. If you choose Delete everything again after Halvo starts again, it destroys every key file in the folder that now stands at that place and can say that it is finished; the key files in the moved folder can still open the recordings that were moved with them.

Limit on a Mac: A key that you press before a program or page has moved to its password field, for example while it is frozen or still loading, can be noted: when it was pressed, in which app and what kind of key, never which letter, digit or sign. To be sure, wait until the cursor blinks in the password field, or pause capture before you sign in. Halvo keeps one record for each key that it notes, in the order in which you pressed them, each with its second and its kind. So where it notes keys before a program or page has moved to its password field, or where it cannot confirm that it leaves out password fields, what Halvo keeps there shows how many letters, digits and signs you pressed, so the length of a password, and where among them you pressed a key of another kind, such as a delete key, Tab or Return, or a sign that your keyboard types with Option or AltGr, which Halvo notes as a shortcut. It never shows which letter, digit or sign.

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

Where a sentence names a unit, that unit makes it true.

- On a Mac, Halvo records no title of a browser's window, private or not: the app and
  the time are recorded as for every app, and the title is never read. This holds for
  every browser that Halvo knows by name: Chrome, Edge, Firefox, Safari and the other
  browsers in wide use, with their beta, developer, nightly and test builds. Halvo
  records no title of Thunderbird's windows either, because Thunderbird's id shares the
  beginning of Firefox's. A browser that Halvo does not know as one has its titles
  recorded like any app's, its private windows too.
- On Windows, Halvo records the titles of browser windows for Edge and Chrome alone.
  It records no title of a Firefox window, and none of a window of another browser
  whose program it knows by name, such as Brave, Opera or Vivaldi; the name of the
  app and the time are recorded as for every app. A browser whose program it does not
  know by name is recorded like any app, the title of its private windows too.
- On Windows, an Edge title is kept only where it ends with the brand name,
  `Microsoft Edge`, and no closing bracket stands before the brand with nothing but
  characters that are neither letters nor digits between them. An InPrivate window
  carries the marker `[InPrivate]` there; under the German and the Japanese interface
  it read the same, with an en dash or a hyphen before the brand. A page whose own
  title ends in a closing bracket is not kept. If Edge marked a private window
  otherwise under some interface language, the titles of that window would be kept.
- On Windows the title of a Chrome window does not tell a private window (it read
  `PAGE - Google Chrome` in 3 of 3 runs). Halvo asks the window instead, through UI
  Automation, and keeps a title only where the window's toolbar holds a button named
  Back, Forward or Reload and no button of the window carries the word Incognito; it
  keeps no title of a window in the first two seconds after it first sees the
  window. So a Chrome window under another interface language keeps no title. If an
  Incognito window showed one of those English names on its toolbar and no button
  with the word Incognito, its titles would be kept; no run has yet opened Chrome in
  Incognito to show the rule on a real window.
- A terminal that does not assert secure input gets no protection, so key classes
  and the window title are recorded while a password is typed there (macos-4).
- A box that a web page only draws as dots, without telling the system that it is
  a password box, is not recognised as a password field, and what is typed into it
  is recorded like any other typing (win-3, ruling W3-20). On Windows nothing that
  UI Automation reports tells such a box from any other text box in Chrome, Edge or
  Firefox. On macOS the guard rests on the system's secure input, which a program
  turns on for its own password fields, and a text field that a page only styles as
  dots does not turn it on, so the two systems are alike here as far as the team
  has read; no test has shown it on a Mac yet.
- Halvo records no keystroke typed while secure input is held: a key event that
  reaches it in that time is not recorded and is only counted by type (macos-4).
  Line (c) of the gate is the check that shows it, and gate-3 builds that line
  (contract C4 item 13).
- Characters and key codes are never stored: the types that core-2 builds have no
  field for them.
- On a Mac, Halvo records nothing of an agent's own browser: no app in front, no
  title, no key or click, and no frame of its windows, also while a person signs in
  for the agent there (agent-browser-out). It tells that browser by its process. The
  app starts each run's runner and the runner starts the browser, so every process
  whose parents lead back to the app is left out. Each run also has a mark under
  `agents/marks` in the data directory that names its runner and its browser by
  process number and start time; the capture service reads the marks once a second
  and leaves out each process that a mark names, and each process under one, for as
  long as that process runs, also after its runner has ended or its mark was
  removed. A process number that a mark names is recorded again once another program
  holds it, because that program started at another time. Where the service cannot
  list that folder or read a mark, it records nothing at all until it can again;
  Status meanwhile shows capture as running and counts nothing as left out, so a
  fault of that folder that lasts stops all recording without a sign. The limits:
  Halvo reads which app is in front every 0.4 seconds, so a click or a key in an
  agent's browser within 0.4 seconds after it comes to the front can be recorded for
  the app that was in front before; a capture service that the app did not start
  knows a run's browser only from its first reading of the marks after the run's
  runner started, up to a second later, and one that starts after a mark was removed
  does not know that run's browser; a service on another data directory reads only
  that directory's marks; on Windows Halvo does not read the marks, so there only the
  process tree leaves an agent's browser out.

The first-run text and the data view quote these three sentences word for word.
The owner approves them once, in the table of user-visible sentences for each
system; until then they are the recommended text.

- `Halvo does not record what you type.` win-3 keeps the Windows
  keyboard source out of the default source set.
- `Screen activity is not recorded.` core-2 builds the switch `screen_activity`
  with the default off; the sentence holds while the switch is off.
- For an unsigned macOS build: `This is an unsigned build: the key that protects your data is kept in a file with owner-only permissions in your account.`
  store-3 and daemon-3 make it true.

docs-2 confirms each sentence of this section against the merged code.

## What an agent's request can hold

A person's agent runs in a program of its own, halvo-runner. That program links
no crate that opens a store (check 21, `scripts/r1/ci.d/21-tree-bans.sh`), and
no client of the capture service, which can pause the capture and delete
everything. The stored records that it gets are the answers to the twelve
runner bodies that its supervisor answers, and a runner body answers records of
these kinds alone:

- `agent_config`, `agent_finished`, `agent_playbook`, `agent_bindings`,
  `agent_session`, `agent_start` and `agent_reply`;
- the target of a `learn_edit` row, laid on a step of `load_steps`.

The twelfth body, `drop_session`, answers no record: it makes Halvo stop using
the kept sign-in of one site of the agent.

The test `a_runner_body_answers_no_kind_outside_the_closed_list`
(`crates/halvo-learn-store/src/tests/kinds.rs`) holds that list.

So a request that an agent sends to a service that the person bound to it can
hold three things, and nothing else of what was captured:

- the agent's own config;
- the records of that agent's own runs; and, for a message that it asks about,
  the state of another agent's reply to that message (under way, done, withheld
  or not known), never what that reply says;
- what Halvo learned for that agent from its own runs: where on a page it found
  a control again after the page had changed.

An agent's browser also contacts some servers of its own maker by itself. Halvo
starts it so that its only way out is Halvo's own relay, which refuses every
request to a host that the person did not bind to the agent, before any
connection leaves the computer. A request of the browser's own to a host that
the person did bind is let through. The test
`a_host_outside_the_list_is_refused_before_any_connection_and_recorded`
(`crates/halvo-web/src/relay.rs`) shows the refusal.
