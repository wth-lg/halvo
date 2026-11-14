# Browser profile trial (U0.3): notes

`docs/r3/trials/browser-profile.md` holds the result of this trial: a short text and its lines. This
file holds the rest of what the builders recorded: how each item runs, where the browsers wrote their
temporary files, the runs that show that each item can come out the other way, and what the lines
cannot show. `sh tools/r3-trials/profile/run.sh --help` states every argument.

## How each item runs

run.sh builds the programs of this directory with `cargo build --release --locked`, and ends by exec
of `profile-trial`, which starts the fixture server of unit U0.1 (`tools/fixture-server/variants`) on
the port. A SIGTERM or SIGINT sent to run.sh therefore reaches the trial.

Every browser of the first round started headless through `Executor::start` of the library in
`tools/r3-trials/web`, with the first flag set of A13.2, the resolver rule and, on macOS,
`--use-mock-keychain`. From the second round on it starts as the product does (the section The
second round below). `start` makes the
profile directory at the path that the caller names and refuses a path that exists or is a link. It
makes a temporary directory `<profile>-tmp` beside the profile, with the same refusals, and gives it to
the browser as `TMPDIR` on macOS and as `TMP` and `TEMP` on Windows. It keeps the canonical paths of
both. A start that fails, or whose future is dropped (the trial drops it on a signal), removes both.
`stop_and_remove` closes the browser and waits up to 15 s for every process of its tree and for the
crash handler that leaves the tree on macOS. From 10 s on it ends a process of the tree only when its
pid still has the creation time recorded for it, read again just before the kill. It never ends the
crash handler. Then it removes the two directories, following no link, and says `removed=no` when
anything of them is left.

- Item 1 resets the fixture to seed 1 with the variant `second_factor`. On profile A it signs in,
  passes `/verify` and reaches `/bill`. It exports the cookies and the localStorage of the utility
  and payment origins with `export_state`, and `stop_and_remove` stops browser A. Browser B starts on
  profile B, which `start` makes as it made A. The trial reads B's cookies for the exported origins
  (`b_empty_at_start` on the `profile` line), imports with `import_state` and opens `/bill`. A landing
  on `/bill` is `import=ok second_factor_asked_again=no`, on `/verify` `import=ok
  second_factor_asked_again=yes`, and on any other page, `/signin` among them, `import=miss`.
- The exported state of the fixture holds 3 cookies of the utility host (`halvo_fx_session`,
  `halvo_fx_ref`, `halvo_fx_device`), all host-only session cookies, two of them HttpOnly, and no
  localStorage item. Its JSON form is 887 bytes. The trial keeps it in memory and writes it nowhere.
- The watch reads the front application (`lsappinfo front` on macOS, the foreground window on
  Windows) and the list of on-screen windows (`CGWindowListCopyWindowInfo`, `EnumWindows`) every
  250 ms, from before the first start until 10 s after the last stop. `keychain ... prompt=yes` means
  that the front application changed to a process of the browser's tree or to a prompt process of
  the system (`SecurityAgent`, `coreautha`, `UserNotificationCenter`, `universalAccessAuthWarn`,
  `consent.exe`, `CredentialUIBroker.exe`), or that a new window of such a process appeared. Any other
  change of the front application is counted on standard error (`front_other_changes`). A read that
  fails, also of the front application's process, makes `notrun ... cause=window_list` in place of
  the `session` line, with no `keychain` line.
- Item 2 runs item 1 and then searches every file under the scratch directory, which holds both
  profiles and both temporary directories, under each entry that appeared at the top of the system's
  temporary directory during the run (other programs' entries among them, which can only add hits),
  and, on macOS, the files changed during the run in `Crashpad` of Chrome and Edge and in their helper
  caches. It counts the page marker of seed 1 in UTF-8, UTF-16LE and UTF-16BE and the name of the
  cookie whose exported value is the marker (`halvo_fx_ref`), because the browser stores cookie values
  encrypted. `marker_hits` sums these counts and adds one for each of the four directories still on
  disk (`dirs_left` on the `searched` line). The `trace` line counts the utility host name over the
  scratch and the new temporary entries only. A search that could not finish prints its `notrun` line
  in place of the `disk` line. Standard error gives the count of new temporary entries, never a name.
- Item 4 runs two shapes under `tree-peak`, five runs each: the nine steps of `tools/r3-trials/web`
  (`--runs 1`, nine short browser starts; `shape=steps`), then the trial's own task mode
  (`shape=task`): one browser signs in with the second factor, opens `/bill` and `/pay` and holds the
  payment page for 20 s. `--shape` runs one of them. `tree-peak` reads every 50 ms on macOS the sum of
  `phys_footprint` of the command and its descendants, adding a process that the reparenting to
  launchd took out of the tree when it started during the run and its executable lies in an `.app`
  bundle of the tree or is the command's own binary (Chrome's `chrome_crashpad_handler`). On Windows it
  reads the peak of a Job object that holds the command from a suspended start, every 250 ms. The
  bound of a call is 8 MiB times ceil(5 times the larger median / 33554432). A `notrun` line of item 4
  says `cause=browser_start` when a line of a run ends so (the program of the nine steps prints
  `notrun candidate=... cause=browser_start` for a browser that does not start) and `cause=runs`
  otherwise. A task run whose stop leaves its profile is a miss. On every path the `task` line's
  `removed=yes` says that neither its profile nor `<profile>-tmp` exists when the line is printed,
  read without following a link, and `removed=no` that one of them exists or could not be read.
- A SIGTERM or SIGINT stops the browsers with their trees, removes their profiles and temporary
  directories, also of a browser that was starting, stops the fixture server and ends with
  `notrun ... cause=signal` and exit 1, also when it comes after the session. In item 4 the program of
  `tools/r3-trials/web` stops each of its browsers with `stop_and_remove` as well (since 85f255a), so
  after the steps shape, also after an INT or a TERM, the scratch holds no profile and no temporary
  directory of a browser; a file beside them stays.

On Windows the stop has a window that this trial does not close. `procs::table()` reads the process
table in two steps: the parent number from the Toolhelp snapshot, and the creation time from a later
open of the process by its number. A number that is reused while the snapshot is walked can
therefore enter the tree with a member's number as its parent, and the stop would end that process.
The rule of `descendants` that a row created before its parent is no child does not stop this,
because the process that takes the number starts later. No Windows cell of this trial is stopped by
a signal. The first unit that stops a browser with this library on Windows closes the window first,
by taking both values of a row from one handle of the process.

No hosted job of the repository builds or tests the two trial crates: they are not members of the
root workspace. On the macOS job the shell lint and the two repository checks of line endings and of
own-source reads read their scripts, and the hook scan `18-hook-scan` reads their Rust files, as it
reads every tracked `.rs` file. On the Windows job the step that clones the checkout with
`core.autocrlf=true` reads their scripts. The hosted runs that this file cites ran on scratch
branches: 42962050772 and 42976989121 built and ran the trial on both systems, and 42995011332 on
Windows alone, and none of the three ran `cargo test` of either trial crate. Run 43042611636, of the
scratch branch `scratch/r3-u03-last-4`, ran `cargo test` and clippy of both crates on both systems.

## Item 4 in numbers (03f4c53 and 85f255a)

- `mac`, Chrome, through the timed-run helper. `shape=steps` ran at 85f255a, 2026-11-11T11:56:17Z to
  11:57:19Z, with the 1-minute load average 10.09 before and 17.07 after. Its five peaks lie between
  951,979,016 and 1,003,197,512 bytes, median 969,575,432. Each run took 152 to 182 samples and
  adopted 9 processes, one crash handler per browser, which the session's stop now waits for. At
  03f4c53, when the program of the nine steps used `stop()` and left its profiles, the same shape read
  931,302,864 to 955,682,088, median 934,693,560, with 17 or 18 adopted, 2026-11-11T09:10:31Z to
  09:11:20Z at a load of 29.55 before and 25.54 after. `shape=task` ran at 03f4c53, 09:11:20Z
  to 09:13:12Z, with the load at 25.54 before and 16.35 after. One task run took 21.4 to 21.6 s, 20 s
  of it the held page, and 352 to 359 samples. Its peaks lie between 965,299,592 and 977,227,432,
  median 971,787,872. That median is the larger, so the bound of `mac` is 1,216,348,160.
- At the peak of the median task run (run 4), the `treepart` lines give the GPU and utility helpers
  580.3 MB, the renderers 277.5 MB, the browser's main process 84.5 MB, the fixture server 22.4 MB,
  the trial program 3.9 MB and the crash handler 3.1 MB. At the peak of the median run of the nine
  steps at 85f255a (run 4) the renderers held 265.7 MB, the helpers 579.7 MB, the main process
  81.4 MB and the program of the nine steps 14.9 MB, against 237.3, 580.5, 85.0 and 4.4 MB at
  03f4c53; that program now reads the process table every 50 ms while it stops each browser.
- `hosted-win`: in run 42976989121 (03f4c53) the task medians are 321,466,368 (Chrome) and 367,390,720
  (Edge); in run 42995011332 (85f255a) the steps medians are 328,142,848 and 370,241,536, against
  312,881,152 and 356,491,264 at 03f4c53. The bound of `hosted-win` comes from Edge's steps shape:
  469,762,048. After the steps shape there, as on `mac`, the scratch held no profile and no
  temporary directory, and a file beside them stayed.
- Builder-8's cells at 21b88ee measured the nine steps alone, with a median of 974,098,160 on `mac`.
  The median of the nine steps at 85f255a is 0.5 % lower and the task median 0.2 % lower; the bound
  falls by one step of 8 MiB, to 1,216,348,160. Each shape is one call of five runs on a loaded machine, a peak
  between two samples is missed, and memory that a daemon of the system holds for the browser lies
  outside the tree.

## Where the browsers wrote their temporary files

A listing during item 1 (every 100 ms, by name pattern only; a temporary entry is the browser's when
the `SingletonSocket` link of one of its profiles points into it, or when it lies in the run's own
temporary directory; other new entries are counted, not named; builder-9's evidence `p03/` and
`hosted/`):

- Before the run's own temporary directory existed (this Mac, f6c246c), both profiles' singleton
  sockets lay in `<system temporary directory>/com.google.Chrome.*`, one directory per browser, which
  Chrome removes at its exit.
- With it, Chrome on this Mac (52f8625) and Chrome and Edge on `hosted-mac` (03f4c53, run 42976989121)
  still put their singleton directory in the system's temporary directory (`com.google.Chrome.*`,
  `com.microsoft.edgemac.*`), and their run's own temporary directories stayed empty, although the
  library passes `TMPDIR` to the browser process. On `hosted-mac` each browser also made one entry
  of its bundle id under the per-user `X` directory (the code-signing clone). So on macOS the search
  of the system's temporary directory stays as it was.
- On `hosted-win` (03f4c53, run 42976989121) Chrome wrote 18 and Edge 26 files `<guid>.tmp` into the
  run's own temporary directories, and the system's temporary directory got no new entry during
  either run. No listing of Windows exists from before the change: builder-8's run read only files
  that stayed after the run.

## Each item can come out the other way

These runs on this Mac each gave the other result (one run each): those of the first three bullets
at commit 03f4c53, the commit of the counted cells (builder-9's evidence `other-way/`), and the
others where their bullet says:

- `--leave-out all` and `--leave-out halvo_fx_session` gave `import=miss`; `--leave-out
  halvo_fx_device` gave `import=ok second_factor_asked_again=yes`.
- `--plant utf8|utf16le|utf16be:parent|temp` gave `marker_hits=1` in each of the six runs, with the
  hit in the planted encoding.
- In a copy whose stop removes neither directory, both stops said `removed=no`, the `profile` line
  `a_gone=no b_gone=no`, and the `disk` line `marker_hits=8`: 4 from the name of the marker cookie in
  the two kept cookie stores and 4 for the directories left. The marker itself was in no file in
  UTF-8 or UTF-16: the fixture sends `Cache-Control: no-store`, and the cookie store keeps the value
  encrypted.
- A copy that gives browser B one cookie of the fixture before the import printed
  `b_empty_at_start=no` (at f50624c); a copy that keeps both temporary directories printed
  `dirs_left=2` and `marker_hits=2` (at 52f8625); a copy whose front reader reports a change of the
  front application to another process printed `prompt=no` and counted the change on standard error
  (at af34d80).
- A SIGTERM during browser B's start (0 and 75 files in profile B at the signal), during the sign-in,
  during the watch's tail and during the wait before the search each ended with
  `notrun ... cause=signal`, exit 1, an empty scratch and no process. A file of mode 000 in the
  scratch gave `notrun ... cause=unreadable` and no `disk` line. These runs ran in the work tree of
  point 4 before its commit e7c8f81 was made: 52f8625 with the three files that e7c8f81 changes
  (`profile-trial.rs`, `cdp.rs` and `cdp/session.rs`) changed.
- A browser path that exits at once gave `notrun item=4 ... cause=browser_start` (at fd908e2).

## tree-peak on trees of known allocations

`tree-peak --runs 3 -- alloc-tree`, at 50 ms on this Mac (builder-9's evidence `p07/`): children
that allocate and write 100, 200 and 300 MiB read 629,490,096 bytes over the empty tree's 6,424,328,
344,496 bytes (0.055 %) above the known 629,145,600; with an orphan of 150 MiB, 0.054 % above
786,432,000; the orphan of 400 MiB alone 0.051 % above; one child of 1,024 MiB 0.188 % below. The
three runs of each case agreed within 0.01 %. A child that holds 400 MiB for 120 ms read 422,528,008
bytes in 3 of 3 runs at 50 ms and 2,753,136 bytes (missed) in 3 of 3 runs at the former 250 ms. A
tree that lives about 2.5 s got 43 to 45 samples at 50 ms, against 11 at 250 ms. `treeinfo lifetime_max_sum` sums each process's
maximum up to its last sample, so it misses growth after that sample and every process that no sample
saw: with a 300 MiB child that lived 20 ms it read 8 to 105 MB in four of five runs. It is no upper
bound.

## Places outside the profile

Run 42962050772 of builder-8 repeated items 1 and 2 on the hosted machines and read every file that
had changed under the home directory and the per-user directories since the start (496 and 522 files
on `hosted-mac`, 1 and 3 on `hosted-win`); none held the marker. Besides their profiles the browsers
wrote: Chrome on macOS `~/Library/Application Support/Google/Chrome/Crashpad/settings.dat` and Metal
shader caches under `<per-user cache dir>/com.google.Chrome.helper/`; Edge on macOS its `Crashpad`
directory, the Metal caches under `<per-user cache dir>/com.microsoft.edgemac.helper/` and one file
in the group container `~/Library/Group Containers/UBF8T346G9.com.microsoft.edgemac.widgets/`; on
Windows only the system's `AppData/LocalLow/Microsoft/CryptnetUrlCache/MetaData/` (Chrome) and
`AppData/Local/Microsoft/TokenBroker/Cache/` (Edge). On this Mac Chrome also makes an app clone for
code signing under `<per-user dir>/X/com.google.Chrome.code_sign_clone/`, which it removes at a clean
exit. The search does not read the group container, the `X` directory, or any place outside the
scratch and the new temporary entries on Windows.

## Reviews before the counted cells

Builder-8's review (three finders, one skeptic) found the stored cookie that the first search could
not see, a missing check of profile B, a watch that sampled four times and an adoption rule of
`tree-peak` that took foreign processes; all were fixed before its cells. Builder-9's review of the
closed list at af34d80 (three finders, one skeptic, 422 s) found nine points; eight are fixed in
e8a6093, 89be48d and 03f4c53. The ninth, the profiles and temporary directories of the nine steps
that the program of `tools/r3-trials/web` left in item 4's scratch, is fixed in 85f255a: that program
now stops each browser with `stop_and_remove`. Before it, a run that ended left 9 profiles and 9
temporary directories, and an INT or a TERM 2 and 2; after it, 0 in each case (builder-9's evidence
`addition1/`).

## The second round (U0.3's second pull request)

### The product's start

From this round every browser of items 1, 2 and 4 starts as A11.1 starts the agent's browser after
U0.7c (`r3_profile::product`):

- on its debugging pipe, the trial program its one client;
- with `Executor::start_settled`, which writes the agent's `Default/Preferences` (every permission
  denied, prediction, passwords, payment methods, safe browsing and suggestions off) and a `Local
  State` before the start; the `Local State` is U0.7c's same-site setting, which isolates
  `http://pay.agent.test:<port>` and turns secure lookups off (`same_site_local_state`), because
  U0.7c's verdict says that the product's start writes that setting;
- with the first flag set of A13.2, U0.7b's flags for the ways out (`ROUND2_FLAGS`) and the relay's
  flags (`relay_flags`): `--proxy-server` and `--proxy-bypass-list` name the relay, and
  `--disable-quic` and `--force-webrtc-ip-handling-policy=disable_non_proxied_udp` close ways out;
- with the relay of the trial program as its only route. The relay's list holds the fixture's
  utility and payment hosts on the port, as the product's list holds the run's bound origins. The
  start names the browser's first process to the relay and asks for one origin outside the list
  before the trial acts.

The relay's guard runs as the program that started the browser with `--relay-guard`, so
`profile-trial` answers that flag first (`r3_web::relay::guard_main`). The steps shape of item 4
runs the web trial with `--round 3 --relay`, U0.7c's start of the nine steps, which writes the
agent's settings but not the same-site setting. On macOS the library adds `--use-mock-keychain`.

`profile-trial --print-set` prints the `flag` and `pref` lines of the result file from these
constants: a `flag` line for each flag of the set without what the executor sets itself (the
profile directory, headless, the debugging pipe, `--proxy-server` and `--proxy-bypass-list`) and
without the resolver rules; a `pref` line for each leaf of the two files, its keys joined by dots.

Flags of the trial that the product does not carry: `--use-mock-keychain` (the product's browser
uses the system's key store for its cookie store, and A8.3 holds: a run's profile lies under the data
directory and goes at the run's end); every flag of a debugging port (`--remote-debugging-port`),
which only the port-form controls of the web trial carry; and `--isolate-origins`, which the rows of
A13.7 add and which names a site on the command line, where the product writes the same-site setting
into the profile instead (CP-93 (5), CP-95 (2)).

### Item 3

`run.sh --item 3` runs `sh scripts/egress/check.sh scripts/r3/egress/fixtures.txt --` the trial
program with `--item egress`. That program starts the fixture on 127.0.0.1:18090, the list's one
entry, and one Chrome that signs in with the second factor, opens `/bill` and `/pay` and holds that
page for 20 s, as the task shape of item 4 does. The outer program prints the inner program's
`egress_run` line, then `egress machine=<m> mode=<mode> browser_started=<yes|no> denied=<n>` from the
check's last line, and on standard error the count of the denial log's lines by their first word.
Readings fixed before the runs:

- The browser runs without the relay. The relay is the executor's, and the check's list holds the
  fixture alone (A14.3): a browser behind the relay would send everything to the relay's loopback
  port, which the list does not hold, and the check would see nothing of the browser's own traffic.
  So the browser goes direct, and the check sees each of its own connections. The relay's flags that
  close a way out stay; the two that name the relay go.
- From 8b0f1a1e two functions of the library decide the lines, and the program prints what they
  return. `product::run_line` gives the inner line: `browser_started=yes` only when the executor's
  start of the browser returned Ok; a fault before any start (the runtime, the seed, the fixture's
  start or reset) and a signal give `task=notrun`; a failed start (`browser_start`, `no_channel`,
  `start`) gives `browser_started=no task=miss`. `product::outer` reads that line by its keys: a
  failed start gives the `egress` line with `browser_started=no`; a start that returned Ok gives the
  `egress` line only with `task=ok`, and otherwise `notrun item=3 ... cause=task`; `task=notrun`
  gives `notrun item=3 ... cause=<its word>`, or the signal's notrun line; `--sandbox-off` gives
  `egress_control` and never `egress`. Before 8b0f1a1e the value came from a list of fault names,
  and a failed fixture reset or `no_channel` read yes (review B2). Every recorded inner line with yes
  reads `task=ok cause=-`, so the rule narrows yes and no recorded line changes. No `--no-sandbox` is
  given, so a browser that starts under the check's mode only with its own sandbox off reads
  `browser_started=no` (A13.3).
- `--flags first` runs the first flag set of A13.2 with the resolver rule and no setting written,
  where the item begins; `--flags final` (the default) runs the set above. The block holds the final
  set's lines; the first set's lines stand in this file.
- `--sandbox-off` is a control and never a cell: it adds `--no-sandbox` and prints an
  `egress_control` line in place of the `egress` line.
- On Windows the checkout wrote `scripts/r3/egress/fixtures.txt` with CRLF line ends, since no
  `eol=lf` attribute covered that path, and the check's list reader refuses a CR (run 43543961191
  refused both calls). Run 43547292129 passed an LF copy of the list. From c2f235af the root
  `.gitattributes` gives the file `eol=lf`, and item 3 passes the tracked file itself.

### Item 3 in lines

The `egress` lines of the first flag set, which the block does not hold, by run. On `mac` at
c9eafc41 and on `hosted-mac` in runs 43547292129, 43552018301 and 43566221064:

```text
egress machine=mac mode=sandbox browser_started=no denied=0
egress machine=hosted-mac mode=sandbox browser_started=no denied=0
```

On `hosted-win`, runs 43547292129 and 43552018301:

```text
egress machine=hosted-win mode=wfp browser_started=yes denied=4
```

On `hosted-win`, run 43566221064:

```text
egress machine=hosted-win mode=wfp browser_started=yes denied=2
```

Run 43566221064 prints its two denials by protocol, port and loopback host alone: 2 times `proto=udp
port=5353 loopback=no`. Runs 43547292129 and 43552018301 printed the count alone, so the kinds of
their denials are unknown. The count of the first set is what the browser happened to try in a run,
and no text here gives it as fixed: in every run the first set showed at least one denied connection
and the final set showed none. The final set adds U0.7b's flags, the two relay flags that close a
way out (`--disable-quic`, the WebRTC policy) and the agent's settings. So item 3 adds no flag to
U0.7c's set, and the cells of A13.2 and A13.7, which U0.7c ran with that set, stand. The kinds of
run 43566221064's denials, and of the controls at c9eafc41 and in that run, come from the round's
step script, which is not in the tree (`.u03r4/item3-extra.sh` of the scratch head cf5bb96a); item
3's program prints the count alone today. From the next run of item 3 on, the program prints one
line for each denial with its protocol, port and loopback host alone; that change is the planner's
to place.

What `browser_started=no` covers on `mac` and `hosted-mac`: every call of either set ended with
`cause=start`, which the executor gives when the browser answered its first call on the pipe
(`Browser.getVersion`) and the start failed after it. On `mac` the browser's kept standard error
holds Chrome's line `sandbox initialization failed` (12 lines at the first set and 13 at the final
set, at c9eafc41); on `hosted-mac` it holds the same line (13 lines at each set in run
43566221064). The check's own output does not say why a browser did not start.

The control with `--sandbox-off` (final set) is no cell. The count of denials that it logs differs
from run to run:

- `mac`: 3 at d66953a4, 4 at 6be76414, 4 at c9eafc41.
- `hosted-mac`: 3 in run 43547292129, 4 in run 43552018301, 3 in run 43566221064.

On this Mac every logged denial read is a refused UDP call to port 443 with no address: the logs at
d66953a4 and 6be76414 name one process of `Google Chrome Helper` for all of a run's lines, and at
c9eafc41 the step printed the protocol, the port and the loopback host alone (4 times `proto=udp
port=443 loopback=unknown`). Runs 43547292129 and 43552018301 printed the `hosted-mac` count alone;
run 43566221064 prints 3 times `proto=udp port=443 loopback=unknown`. No run shows what these calls
were for. A reading that no run shows: they are the UDP connects of Chrome's IPv6 test to port 443,
which the web trial's README names and which send nothing. On `hosted-win`, `denied=0` cannot count
such a call: the Filtering Platform does not observe IPv6 on that runner (docs/r2/trials/egress.md,
its line `not-observable windows-2025 wfp ipv6`).

### Item 4 in numbers (run 43552018301)

- `hosted-win`, Chrome: steps median 220,573,696, task median 235,069,440; Edge: steps median
  189,034,496, task median 214,368,256. The machine's bound comes from Chrome's task shape:
  301,989,888 (8 MiB times ceil(5 times 235,069,440 / 33,554,432)). In run 43547292129 Chrome's task
  median was 234,323,968 and the bound 293,601,280: 1.25 times the median lies at 35.0 steps of 8 MiB,
  so a difference of 0.3 % in the median moves the bound by one step. The first round's bound there
  was 469,762,048, from Edge's steps shape on the first flag set.
- `mac`: open. By the coordinator's rule a cell that records the browser's memory is taken on this
  Mac only at memory pressure level 1, and the level read 2 at every try, so the cell is not taken.
  A smoke call of one run per shape at 6be76414, not a cell, read 948,161,568 (steps) and
  959,450,264 (task) at level 2; no run shows what the cell would read at level 1. The cell is
  filled by `sh tools/r3-trials/profile/run.sh --item 4 --browser <Chrome> --port 18191` through the
  timed-run helper while the level reads 1 before and after.

### The commits of the cells

Items 1 and 2 ran on `mac` at d66953a4, the hosted cells of items 1, 2 and 4 in run 43552018301 at
eb189792, and item 3 at c9eafc41, on `mac` and in run 43566221064. The commits after each, with
the kinds of files they change; none changes a value that a cell computes:

- 33393f00, eb189792 (after d66953a4): item 3's call of the check alone, the list that it passes
  and the cause that its refusal prints.
- 3fc451d8, a264060b, 9877e3f1: the hosted harness's shellcheck names, its self-test's stand-ins and
  its help.
- c2f235af: the line ends of the egress list in a checkout, a check, the workflow and AGENTS.md.
- e54e2bf2, ba0a1afe, d8918435: this README, the result file, CHANGELOG.md and a test of the block.
- ac5cd783, 34c4a52a: comments taken out of both trials, one doc comment put back in place.
- 87ae0ba4, 020ac877, 1aa38a46: SAFETY notes, clippy.toml entries of both trials and the denied lint.
- 79995a4e, 4e3db934, 10c652f3: usages and texts of the reader, the collector and a refusal of the
  web trial, with self-test cases and tests.
- 3bb6aa73, 58784fbd, 9f1f95e5, c9eafc41: tests, and collector cases.
- 8b0f1a1e (before item 3 ran): the rule of item 3's `browser_started`; every recorded line keeps
  its value.
- cadf0314: the web trial's kill line is printed by `kill_line_of`, which takes the context, in place
  of the printer at its call; the line of each browser is the same, and two collector cases.
