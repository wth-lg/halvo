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

Window expiry and the cap remove recordings alone. A summary of what Halvo noticed on a
day, and the rows that support it, stay in Halvo's encrypted store of what it noticed
for 93 days from that day; a workflow that Halvo noticed stays there until the person
deletes it or deletes everything; and an agent stays until the person deletes it or
deletes everything, with the records of its runs for 93 days and a sign-in that it
keeps for 35 days.

Limit of delete-all: Delete everything destroys every key file in Halvo's data folder, not only the one of the store in use. So if the store folder inside it was moved away or replaced, for example by a shortcut, what was recorded there cannot be read again, wherever those files are. Halvo removes nothing in a place that is not its own folder: files may be left there, and you can remove them. One exception: a copy or a move of Halvo's whole data folder, with the folder roots inside it, takes the key files with it. Such a copy can still be opened, and Halvo cannot reach it. A key file that another program copied, or renamed inside the folder, is such a copy: Halvo does not find it, and with an earlier copy of the recordings it can still open them. Source: the store tests `delete_all_acts_through_its_handle_and_leaves_a_directory_put_at_the_store_name_as_it_was`, `delete_all_opens_lists_and_changes_nothing_behind_a_link_put_at_the_store_name`, `the_delete_tail_removes_a_link_at_the_store_name_and_then_sweeps_every_root_file_under_roots`, `delete_all_destroys_every_root_file_under_its_roots_directory_and_names_one_that_stays`, `the_tail_cleans_a_directory_with_a_zeroed_key_file_and_a_fresh_create_takes_it` and `the_tail_answers_each_arm_as_its_row_says_and_leaves_the_roots_directory_as_its_row_says` (store-3, P1 to P4 of ruling ST-30, and ruling ST-45 H1).

If the folder roots, or a key file in it, was moved, renamed or replaced while Halvo ran, Delete everything says that it is not finished and names the key file it could not find, unless a program put a file with that key file's name into the folder that stands there now. If you choose Delete everything again after Halvo starts again, it destroys every key file in the folder that now stands at that place and can say that it is finished; the key files in the moved folder can still open the recordings that were moved with them.

Limit on a Mac: A key that you press before a program or page has moved to its password field, for example while it is frozen or still loading, can be noted: when it was pressed, in which app and what kind of key, never which letter, digit or sign. To be sure, wait until the cursor blinks in the password field, or pause capture before you sign in. Halvo keeps one record for each key that it notes, in the order in which you pressed them, each with its second and its kind. So where it notes keys before a program or page has moved to its password field, or where it cannot confirm that it leaves out password fields, what Halvo keeps there shows how many letters, digits and signs you pressed, so the length of a password, and where among them you pressed a key of another kind, such as a delete key, Tab or Return. It never shows which letter, digit or sign.

Limit on Windows: A key that you press before a program or page has moved to its password field, for example while it is frozen or busy, can in rare cases be noted: when you pressed it, in which app and what kind of key it was, never which letter, digit or sign. To be sure, wait until the cursor blinks in the password field, or pause capture before you sign in. Halvo's own tests show it in these ways:

1. Type the first character of a password into a classic Windows program while it is busy, before it has moved the cursor to its password box, where the program itself fills or clears the box that has the cursor and then moves the cursor to the password box on its own a moment later. One key typed at the password field is noted, as a time, an app and a kind, never which key.
2. In a classic Windows program that has frozen for about half a second, press Tab to go to the password box and type the first character of the password about 0.4 s later, before the program wakes; the program, when it wakes, fills or clears the box that still has the cursor before it follows the Tab. One key typed at the password field is noted, as a time, an app and a kind, never which key.
3. Type two characters a third of a second apart into a classic Windows program that has frozen for more than a quarter of a second, where the program, on waking, moves the cursor from the box you typed into to its password box by itself (no Tab, no click). One key typed at the password field is noted, as a time, an app and a kind, never which key.
4. In a classic Windows program that freezes for about half a second, type the last character of the user name, press Tab, and type the first character of the password about 0.4 s later, all before the program wakes. One key typed at the password field is noted, as a time, an app and a kind, never which key.
5. In a classic Windows program, click a button or a menu item that pastes into the box that has the cursor, type the first character of the password at once, and have the program move the cursor to its password box on its own a moment later. One key typed at the password field is noted, as a time, an app and a kind, never which key.
6. Type two characters within about a hundredth of a second of each other into a web page's text box, on a page whose script, after the first character, freezes for a moment and then moves the cursor to its password box by itself. One key typed at the password field is noted, as a time, an app and a kind, never which key.
7. Type a password into a box that a page only draws as dots on a web page in Chrome or Edge. Every key typed there is noted, as a time, an app and a kind, never which key, as in any field read as plain.

Reading the code shows three more ways, which no test runs:

1. Type two characters into a Windows program that is busy while a password manager fills its password box, where the program, on waking, moves the cursor to the password box by itself. It needs a password box that reports its changes, which the classic Windows password box did not do in the runs. One key typed at the password field is noted, as a time, an app and a kind, never which key.
2. As in ways 5 and 6 above, but type the password character more than about a third of a second after the click, or after the Enter. One key typed at the password field is noted, as a time, an app and a kind, never which key.
3. In a classic Windows program that has frozen, press Tab into its password box (or click it), type two characters about half a second later, and have the program, when it wakes, take the first in the password box and then move the cursor back to the box before it by itself, while Windows sends no notice of the Tab's move. One key typed at the password field is noted, as a time, an app and a kind, never which key.

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

- On a Mac, Halvo reads the title of the window in front only while you have given it
  the Accessibility permission. For a browser it decides by the browser. Chrome and
  Edge: Halvo records a title only where the browser shows English and lies in an
  Applications folder, and only where the title ends with the browser's own name,
  ` - Google Chrome` or ` - Microsoft Edge`, or with its own name and then ` - ` and the
  name of a profile, as both end the titles of a person with several profiles; it
  records no title of their beta, developer and test builds. Halvo also leaves out an
  Edge title where a closing square bracket stands before ` - Microsoft Edge` with
  nothing but signs that are neither letters nor digits between them, because there Edge
  on Windows puts the mark of its InPrivate window, and where the end of the title that
  it reads holds no letter, digit or closing square bracket before that name. Chrome and Edge end the
  title of a private window with `(Incognito)` or `(InPrivate)` and of a Guest window
  with `(Guest)`, and while a dialog over the page has the focus the title is the
  dialog's own: Halvo records none of these titles. Chrome also opens windows that keep
  no history and that no person opens by a command of private browsing, such as the
  window of a developer tool's own session or the sign-in page of a network that asks
  for one: Chrome names such a window neither Incognito nor Guest, its title ends with
  Chrome's own name, and Halvo records it. Where Chrome or Edge gives no title of its
  own making for a window, the Mac gives the page's own title, and a page whose own
  title ends as an ordinary window's does would then be recorded, in a private window
  too; no line that has been read shows when that happens. Halvo tells the language from
  the browser itself, and it asks macOS for no name of a file that a browser holds open.
  When a Chrome or Edge that lies in an Applications folder is in front and Halvo has
  not yet told its language, Halvo reads, from the disk, the language files inside that
  browser's own app, each in a folder named after its language (`en.lproj` for English),
  and with each the two numbers by which macOS knows a file on a disk, its device and
  its inode. Then it asks macOS for the list of what this browser holds open. That list
  gives, for each entry, a number and a kind, such as a file or a network connection,
  and neither a name nor an address. About each open file Halvo then asks once more, and
  macOS answers with one block of numbers and no name: the file's device and inode, and
  beside them its kind, its owner and group, its permissions, its size, its times, a few
  numbers more of the same sort, and how and how far into it the browser has it open. Of
  that block Halvo takes the device and the inode alone. Where exactly one of the
  browser's language files is among the open files by these two numbers, Halvo takes its
  language for the browser's. Halvo keeps none of these numbers: they are held in memory
  for that one look, and none is stored, shown or sent anywhere. Each time Halvo is
  about to read a title of a Chrome or Edge in front, also where it then reads none, and
  again behind a look that gave an answer, it asks macOS about that browser's process.
  Where it needs room to remember one more copy of a browser, it asks the same about the
  copies of that browser that it remembers, to tell whether they still run. macOS
  answers with a block of numbers about the process, such as the processor time that it
  has used and the memory that it holds, and Halvo takes one of them alone: the time at
  which the process started. What it remembers of a look is in its memory alone: for
  each copy of a browser that it looked at, the number and the start of its process, by
  which Halvo finds it again, and whether it took that copy for English or how often it
  has looked without an answer. It forgets all of that when its capture service ends and
  when Halvo deletes everything. It looks once for a browser that it finds running,
  after you opened that browser, after Halvo's capture service started or after Halvo
  has deleted everything, and again, at most 25 times, while a look gives it no answer.
  Only where more than four copies of the same browser run at once, each started as a
  program of its own, can it look at one of them again, and it may then do so each time
  that copy comes to the front. Where the language file that the browser holds open is
  the English one, and the browser lies in an Applications folder, Halvo records that
  browser's titles by the rule above. Where it is the file of another language, where
  the browser holds none of them or more than one, or where macOS does not give Halvo
  the numbers, it records no title of that browser, so under any other language it
  records none. A browser carries more than one file for each language; Halvo takes it
  for English only where the one that it holds open lies in the folder `en.lproj`, and
  no run has read a browser that holds another one of them open. A Chrome or Edge that
  was open before Halvo started is told in the same way as one that you open later.
  Where the files of the version that runs were taken off the disk while that browser
  ran, as when its app is replaced by hand, the browser holds files that the disk no
  longer has: a look then finds none of them, and Halvo records no title of that browser
  until you quit it and open it again. Two runs on 26 November 2026 read a Mac's Chrome
  158.0.8127.37 and Edge 157.0.4341.60: opened in English, each held its English
  language file open and no other; opened in German, whether by the language that was
  set for that app or by the argument `-AppleLanguages` on its command line, each held
  its German one and no other; and so it was at the first window, 20 seconds later and
  behind a private window. The second of them told each by the two numbers alone; in it
  a second set of language files appeared inside each app while it ran, as a new version
  beside the old one would make it, and the look still found the one that the browser
  held. Those runs did not show Halvo recording a title by this reading, and no run has
  shown the reading on a person's own Mac. They read no browser that had been open for
  longer than three minutes and none whose files were taken off the disk while it ran,
  and they read English and German alone. Halvo records no title of any other browser
  that it knows by
  name, such as Safari, Firefox, Brave, Opera or Vivaldi, with their beta, developer,
  nightly and test builds, and none of an app that your Mac lists as one that can open
  web links. Where Halvo cannot read that list, or cannot tell where such an app lies on
  your Mac, it records the titles of that app, a browser and its private windows among
  them. Thunderbird and the other apps of Mozilla, of the Tor Project and of Chromium
  that Halvo does not know by name have their titles recorded only where Halvo could
  read that list and the app is not on it. Halvo records the titles of every other app,
  except its own and the password managers that it leaves out whole. For a browser, the
  app and the time are recorded as for every app. A browser that Halvo does not know and
  that your Mac does not list for web links has its titles recorded like any app's, its
  private windows too. How Chrome ends its titles is read from Chrome's public source as
  of 25 November 2026. Three runs on 25 November 2026 read the titles of real windows of
  Chrome 158.0.8127.37 and Edge 157.0.4341.60 on a Mac: an ordinary window ended with
  the browser's own name; a private and a Guest window ended as said above; a page's
  message box gave the title `127.0.0.1:<port> says` in an ordinary and in a private
  window alike; a second profile's window ended with ` - ` and the profile's name behind
  the browser's own, and that profile's private window ended as a private window does.
  Set against these titles in its tests, Halvo's rule keeps the ordinary window's and
  the second profile's and leaves the others out. The runs read the titles alone: they
  did not show Halvo recording a title of either browser through the whole of its rule,
  and on them Halvo could not read the language that Chrome was set to. They read one
  version of each browser and one language. If another version of Chrome or Edge ended
  the title of a private window as it ends an ordinary one, or put the mark of a private
  window behind a profile's name, Halvo would record the titles of that window.
- On Windows, Halvo records the titles of browser windows for Edge and Chrome alone.
  It records no title of a Firefox window, and none of a window of another browser
  whose program it knows by name, such as Brave, Opera or Vivaldi; the name of the
  app and the time are recorded as for every app. A browser whose program it does not
  know by name is recorded like any app, the title of its private windows too.
- On Windows, an Edge title is kept only where it ends with the brand name,
  `Microsoft Edge`, and no closing square bracket, `]` or its full-width form `］`,
  stands before the brand with nothing but characters that are neither letters nor
  digits between them. An InPrivate window carries the marker `[InPrivate]` there;
  under the German and the Japanese interface it read the same, with an en dash or a
  hyphen before the brand. A page whose own title ends in a closing square bracket is
  not kept. If Edge marked a private window otherwise under some interface language,
  the titles of that window would be kept.
- On Windows the title of a Chrome window does not tell a private window: it reads
  `PAGE - Google Chrome` in an Incognito and in a Guest window too. Halvo asks the
  window instead, through UI Automation, and keeps a title of Chrome only where three
  things hold: the name that Chrome gives the inside of the window ends with
  ` - Google Chrome`, or with ` - Google Chrome - ` and the name of a profile where Chrome
  holds several; the browser's own toolbar, outside the page, holds a button named Back,
  Forward or Reload; and no button of the window carries the word Incognito. In an
  Incognito or a Guest window that inside name ends with the window's mark instead, so
  none of their titles is kept, in full screen and in a pop-up too. Halvo also keeps no
  title of a window in the first two seconds after it first sees the window, none while a
  page's dialog is in front, none of an app window, none under another interface
  language, and, by Chrome's public source alone, none of Chrome Beta, Dev or Canary or
  of Chromium. Four runs on 25 November 2026 opened Chrome on Windows in these states;
  before this change the titles of a Guest window were kept, and so were those of an
  Incognito or a Guest window in full screen whose page showed a toolbar of its own. The
  names are those of the Chrome on the machines of those runs: a version of Chrome that
  named the inside of a private window as it names an ordinary one's, or put the mark of a
  private window behind a profile's name, and showed no button with the word Incognito,
  would have that window's titles kept.
- On Windows, Halvo asks the program in front whether the field with the cursor is a
  password field. While the program says yes, Halvo records no window title. Where a
  program gives no answer, Halvo records the window title, also while you type a
  password into a box that such a program draws by itself. It records no key there.
  On Windows, when Halvo starts and when you switch to another window, Halvo records the
  title of the window in front only after it has read where the cursor is in that
  window. Until that read comes it records no title of that window, and where the read
  finds a password field, or fails, it records none. The same holds for screen activity,
  where that is switched on. It records no key. A program that puts the cursor on its
  window first, and moves it into a password field only after Halvo has read the window,
  can have that window's title recorded. In Chrome and Edge, a page's title that changes
  in the instant in which the page takes the cursor, or the title of a window that you
  switch to in that instant, can be recorded although the cursor lands in a password
  field.
- Some programs show what you type in the title of their window, for example the name of
  a document or the words of a search. Halvo records such a title like any other. It
  holds a title back while it can tell that the cursor is in a password field, and on
  Windows it cannot tell that in a program that gives no answer.
- A terminal that does not assert secure input gets no protection, so key classes
  and the window title are recorded while a password is typed there (macos-4).
- A box that a web page only draws as dots, without telling the system that it is
  a password box, is not recognised as a password field, and what is typed into it
  is recorded like any other typing (win-3, ruling W3-20). On Windows nothing that
  UI Automation reports tells such a box from any other text box in Chrome, Edge or
  Firefox; in Chrome and Edge, a text box that a page only draws as dots is an
  ordinary text field to Halvo: every key typed there is noted in the same way. On
  macOS the guard rests on the system's secure input, which a program
  turns on for its own password fields, and a text field that a page only styles as
  dots does not turn it on, so the two systems are alike here as far as the team
  has read; no test has shown it on a Mac yet.
- On Windows, Halvo records a key only when, just after you press it, the text changes in a field that Windows does not report as a password field. It records when you pressed the key, in which app, and what kind of key it was: a letter, digit or sign, or a delete key. It never records which letter, digit or sign. Halvo can tell this only in Chrome and Edge and in the plain text boxes of classic Windows programs: in every other program it records no key. It records no Return or Enter, no Tab, no arrow key, no Escape and no shortcut. On Windows, Halvo records no key in a program whose text box is not the plain one of a classic Windows program: a word processor or another program with a rich text box, a program built with a newer Windows framework, a terminal, a program built on Chromium under its own name, and Firefox (win-6).
- On Windows, in the plain text box of a classic program, after Tab, an arrow key, Escape, a shortcut, a key that puts an accent on the next letter, or an Enter that makes no new line, Halvo leaves out letters that you type next, until you pause for a quarter of a second: at an ordinary typing speed all of them, at a very fast one only the last before the pause, at a slow one only the first. This does not happen in pages in Chrome and Edge. Where a password manager fills a field, or a page clears its password box, while you type in another field, Halvo leaves out what you type for about a third of a second.
- On Windows, when you start Chrome or Edge and type at once, Halvo can leave out what you type until it has read which field has the cursor. It reads that when you click or press Enter, Tab or Escape, and, where it records window titles, also a moment after the window comes in front.
- A password that you make visible, for example with a show-password button, is typed into an ordinary text field: Halvo notes its keys like any others, when, in which app and what kind of key, never which letter, digit or sign, and so its length. Pause capture before you type a password that you can see. On Windows what Halvo keeps shows at most how many letters, digits and signs you typed in such a field: a sign typed with AltGr and the repeats of a held key are not noted.
- Halvo records no keystroke typed while secure input is held: a key event that
  reaches it in that time is not recorded and is only counted by type (macos-4).
  Line (c) of the gate is the check that shows it, and gate-3 builds that line
  (contract C4 item 13).
- On a Mac, while a password field or another app holds secure input, Halvo records no
  window title and no screen activity either. From 30 seconds of such a hold on, the
  Status page says which of typing, window titles and screen activity Halvo is not
  recording, until the hold ends; it does not say so while that line shows another
  reason, while the page says that the app in front has stopped, or while capture
  or the Mac's input source does not run; the first 30 seconds of a hold show
  nothing, nor do the first 30 seconds after the Mac's input source starts during
  a hold, and behind
  its end nothing tells of it.
- On a Mac, while Halvo records typing, it asks macOS how long ago a key was last pressed on the whole computer: at most once a second, and only in a second in which no key reached Halvo. The answer is a length of time alone: it names no key, no text and no app, and it is no number of keys. Halvo stores it nowhere. Halvo does not ask while a password field or another app keeps typing private, nor at its first reading behind that; behind such a hold the answer can be the time since the last key of a password, and Halvo only compares it with one second. It uses the answer for one thing: where through 30 seconds a key was pressed in ten or more of them and no key reached Halvo, the Status page says that Halvo is not recording typing.
- On a Mac and on Windows, Halvo's search for repeated tasks asks the system how long ago the last input was: on a Mac the last input of any kind on the whole computer, on Windows the last input of your session. It asks at most once a second while Halvo's capture service runs and the search is switched on, also while a password is typed. The answer is a length of time alone: it names no key, no text and no app, and it is no number of inputs. Halvo stores it nowhere and only compares it with four minutes: the search runs only after four minutes with no input. In the same turn it reads whether the computer is on mains power and whether its low-power mode is on.
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
  Status meanwhile shows capture as running and counts nothing as left out. From the
  third event in a row that it leaves out in this way, the Status page says that
  Halvo has recorded nothing since it could not tell whether to leave out the app in
  use, and it says so until Halvo records again, its capture service starts anew or
  someone asks Halvo to delete everything; the first two such events show nothing,
  behind a new start and behind such a delete too. Where it left out events in this way
  for a minute or more, the Status page afterwards says from when to when Halvo recorded
  nothing, for 24 hours or until a later such stretch has ended, the capture service
  starts anew or Halvo has deleted everything; for this the capture service keeps the two
  times in its memory and in no file, and nothing else of the stretch. The limits:
  Halvo reads which app is in front every 0.4 seconds, so a click or a key in an
  agent's browser within 0.4 seconds after it comes to the front can be recorded for
  the app that was in front before; a capture service that the app did not start
  knows a run's browser only from its first reading of the marks after the run's
  runner started, up to a second later, and one that starts after a mark was removed
  does not know that run's browser; a service on another data directory reads only
  that directory's marks; on Windows Halvo does not read the marks, so there only the
  process tree leaves an agent's browser out.
- On Windows, Halvo records no window title and no screen activity of a program that
  Windows shows inside a frame of its own, such as Settings or the Calculator, while
  that frame is in front, whatever has the cursor. It records the app in front and
  clicks of such a program under the program's own name, and no key that you type in
  it. While such a frame is in front and the program's own window does not hold the
  cursor, as while the program opens or wakes, Halvo records nothing of that window, and
  in that time the Status page can say that Halvo could not tell whether to leave out
  the app in use.

The first-run text and the data view quote these three sentences word for word.
The owner approves them once, in the table of user-visible sentences for each
system; until then they are the recommended text.

- `Halvo does not record what you type.` It shows where Halvo records no
  typing: on a Mac while Halvo's tap asks for no key press, which is without the grant of Input Monitoring and while the switch `key_class` is off; on Windows while the keyboard source line reads off, as while the
  switch `key_class` is off. A Windows service run starts the keyboard source
  while `key_class` is on (typing-on-win, the constant `TYPING_ON_WINDOWS`), and
  the Windows bullets on typing above say what it records.
- `Screen activity is not recorded.` core-2 builds the switch `screen_activity`
  with the default off; the sentence holds while the switch is off.
- For an unsigned macOS build: `This is an unsigned build: the key that protects your data is kept in a file with owner-only permissions in your account.`
  store-3 and daemon-3 make it true.

docs-2 confirms each sentence of this section against the merged code.

## What an agent's request can hold

A person's agent runs in a program of its own, halvo-runner. That program links
no crate that opens a store (check 21, `scripts/r1/ci.d/21-tree-bans.sh`), and
no client of the capture service, which can pause the capture and delete
everything. The stored records that it gets are the answers to the fifteen
runner bodies that its supervisor answers, and a runner body answers records of
these kinds alone:

- `agent_config`, `agent_finished`, `agent_playbook`, `agent_bindings`,
  `agent_session`, `agent_start`, `agent_reply` and `agent_reply_body`;
- the target of a `learn_edit` row, laid on a step of `load_steps`.

The twelfth body, `drop_session`, answers no record: it makes Halvo stop using
the kept sign-in of one site of the agent. Two bodies more answer no record:
they hold and settle the cost of one model request against the person's cap.

For an agent that the person allowed to use a hosted model, the supervisor also
hands the run the key of that model's host when the run asks for it. No runner of
this build asks for it, so no run gets the key.

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

A request of an agent to the host of a hosted AI model, which the person
allowed the agent to use, is of another kind. Before each such request the
agent's run asks the router's table, which has a row for each kind of task and
each kind of request, and it sends none where that row hands back. While every
compile, recover and write row of that table hands back, the rows for working
out an agent's steps, for mending a step and for writing a reply, no run sends
such a request. The tests `route_pick_before_model_call`
(`crates/halvo-runner/src/compile/tests.rs`) and
`with_no_writer_no_agreement_or_the_cap_reached_the_body_is_the_const_text_and_nothing_reaches_the_host`
(`crates/halvo-runner/src/model/write/tests.rs`) hold it for working out steps
and for writing a reply.

Such a request holds text alone: the test
`the_request_that_leaves_holds_the_closed_fields_alone`
(`crates/halvo-model/tests/door.rs`) holds that what leaves is a closed set of
text fields. For an agent that works out the steps of its task on a web page,
as one that pays a bill or orders from a shop, it holds three things:

- the text of the page as the agent's own browser shows it: what the page says,
  the names of its boxes, buttons and links, what its boxes hold, and the web
  address of a frame, a page inside the page, in which a text stands; the
  person's answers stand there as marks, as said below;
- fixed texts that come with Halvo: rules that tell the model to take the text
  of a page or a message as data and to follow no order in it, the task, and
  the form of the model's answer; and
- the run's own list of what it has done so far, such as which part of the page
  it filled and the name of the answer that it put there, and the names of the
  person's answers and of what it noted from a page, such as `payee` and
  `amount_due`, never the answers themselves.

For an agent that writes a reply to a mail, a request holds the fixed texts of a
reply and the subject and the body of the message that it answers, and whatever
else the message says goes with it, such as a name or a phone number in its
body; a very long message goes as far as its first lines fit in one request.
Beside these texts a request names the AI model, how much effort it should give,
low, medium or high, as the router's table names it, and the most that it may
write back, and it carries the key that the person saved for that host.

Before a request is made, Halvo puts a mark in the place of each answer that the
person gave the agent, such as the payee of a bill or the senders whose mail it
answers: the mark reads `[slot NAME]` with the name of that answer, for example
`[slot payee]`. Halvo looks for the answers in each part of a page's text that
leaves, the name of a box, a button or a text, what a box holds and the web
address of a frame, and in the subject and the body of a message. It finds an
answer in capitals or small letters alike, for the letters A to Z, with its
spaces squeezed as a page shows them, and inside a longer word, where the mark
then stands for that part of the word alone. In a message, the line that names
the sender always reads `From: [slot reply_senders]` and names no address,
however many senders the person listed. The test
`u72b_a_model_request_holds_the_page_the_fixed_texts_and_the_runs_lines_and_nothing_of_the_store`
(`crates/halvo-runner/src/compile/tests.rs`) holds the marks on a page, and the
test `u72b_a_messages_snapshot_names_no_sender_and_masks_its_subject_and_body`
(`crates/halvo-runner/src/model/snapshot.rs`) holds them in a message.

The mark stands only where a text holds an answer as the person gave it, in one
of the forms above; an answer in any other form can still leave with a request,
as in these cases:

- another spelling of a number or a date: an answer `12345678` that the page
  shows as `1234 5678`, or `01/12/2026` that it shows as `1 December 2026`;
- a part of an answer: of the payee `City Water Board`, a page that shows
  `City Water` alone; and of two senders or more whose mail the agent answers,
  one address that the subject or the body of a message names alone;
- an answer that a message writes with two spaces where the answer has one, or
  breaks at a line end: the two senders whose mail the agent answers, which
  Halvo holds as the one text `ada@example.com, bob@example.com`, in a mail
  that writes two spaces after the comma or starts a new line there;
- an answer that a box holds with two spaces where the answer has one, or with
  a no-break space, which looks like any other space: the payee
  `City Water Board` in a box that holds two spaces after `City`;
- a letter outside A to Z in the other case: an answer `Müller` that the page
  shows as `MÜLLER`;
- the same letter stored in another way, which looks the same on the screen: an
  answer `Müller` that a page or a message holds as a plain `u` with two dots
  added to it;
- in the subject or the body of a message, a double quote where the answer holds
  two single quotes: an answer `5'' pipe` that the message writes as `5" pipe`;
  and
- an answer that holds a double quote and the word `value=` or `frame=`, which
  the agent's reading writes between the parts of a line, such as
  `Size" value="10` beside a box named `Size` that holds `10`: it can leave whole
  where its parts fall in two parts of one line.

These never leave with such a request:

- a page on which the agent's own reading finds a field for a password or
  another sign-in secret, with what that field holds. Where the agent's browser
  has its own window, the agent hands the browser to the person there, and its
  next request holds the page after the sign-in; where it has no window, the run
  ends at that field, and no further request is made. The tests
  `u72b_no_request_holds_a_snapshot_of_a_page_with_a_credential_field` and
  `u72b_a_line_with_the_credential_mark_hands_back_before_any_request`
  (`crates/halvo-runner/src/compile/tests.rs`) hold it where the browser has a
  window. Where it has none, the test
  `a_run_with_no_window_that_imported_nothing_ends_at_a_credential_field`
  (`crates/halvo-runner/src/handback/tests.rs`) holds that end for a run that
  repeats the steps that Halvo stored for the agent; for a run that works out
  its steps, that end is read from the code, and no test runs it;
- a word of a step that Halvo stored for the agent. The tests
  `u72b_a_model_request_holds_the_page_the_fixed_texts_and_the_runs_lines_and_nothing_of_the_store`
  (`crates/halvo-runner/src/compile/tests.rs`) and
  `a_reply_body_is_the_writers_text_asked_through_ask_alone_with_the_trials_texts`
  (`crates/halvo-runner/src/model/write/tests.rs`) hold it; and
- the key that the person saved for the model's host, in any text of a request:
  it goes to that host alone, in the header that the host reads. The tests
  `the_key_stands_in_the_doors_standard_input_and_in_nothing_else_that_the_run_wrote`
  (`crates/halvo-runner/src/model/tests.rs`),
  `the_request_that_leaves_holds_the_closed_fields_alone` and
  `with_a_grant_one_request_reaches_the_granted_origin_alone`
  (`crates/halvo-model/tests/door.rs`) hold it.

Halvo keeps the text of a reply that an AI model wrote for an agent, so that,
while the draft of that reply waits in the person's mailbox for an answer on its
card, the agent's next card shows the same text and the draft keeps it. It keeps
that text as the agent prepared it for the draft, also where the task ended
before a card showed it, and nothing else that the model wrote, in its store on
this computer, for 35 days from the day of the draft. A draft that still waits
when those 35 days have passed gets, at the agent's next task, the text that
Halvo sets itself in the place of the model's, on its card and in the mailbox.
Deleting the agent, or Delete everything, takes the kept text away with the
agent's other records. After the reply was sent, after Halvo found that its
draft was removed from the mailbox or no longer answers the sender alone, and
where Halvo could not tell whether it was sent, no task of the agent reads that
text again, and it stays in the store until its 35 days have passed. The test
`u79_a_waiting_draft_keeps_the_writers_text_at_a_later_start_and_its_send_holds_what_the_card_showed`
(`crates/halvo-runner/src/model/write/tests.rs`) holds that a later task shows
and sends the kept text and starts no request to a model, and the test
`u79_a_kept_reply_text_reads_while_its_draft_waits_and_no_read_hands_it_out_after_its_reply_moves_on`
(`crates/halvo-agent-store/src/reply_tests.rs`) holds that the read answers
while the draft waits and no more once the reply has moved on; that the kept
text goes with the agent's records when the agent is deleted and at Delete
everything, and that it goes after its 35 days, is read from the code, and no
test runs it. While every write row of the router's table hands back, no model
writes such a text, and Halvo keeps none.
