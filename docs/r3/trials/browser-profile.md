# Browser profile trial

Unit U0.3 of the agent-setup run tests the privacy model of the agent browser (contract A13.3,
A8.3). The browser of a run starts with an empty profile directory and the run removes that
directory at its end. A sign-in that passed a second factor carries over to the next run only as an
exported state, which the app keeps in its own encrypted store. This is the first pull request of
the unit, with items 1, 2 and 4. Item 3 (egress) and the list file of A14.3 belong to the second pull
request, so the verdict line ends with `egress=open`.

## How the cells ran

`sh tools/r3-trials/profile/run.sh --item 1|2|4 --browser PATH --port N` builds the trial programs
of `tools/r3-trials/profile` with `cargo build --locked` and starts the fixture server of unit U0.1.
Every browser starts headless with the first flag set of A13.2, the resolver rule and, on macOS, the
mock keychain, all through the library of `tools/r3-trials/web`. `--help` states every argument.
`--item 3` prints `notrun item=3 ... cause=second_pull_request` and exits 1.

- Item 1 resets the fixture to seed 1 with the variant `second_factor`. It signs in on profile A,
  passes `/verify` and reaches `/bill`. It then exports the cookies and the localStorage of the
  utility and payment origins, with the library's new operation `export_state`. It stops browser A
  with `stop_and_remove`, which removes profile A. Then it starts browser B on a new, empty directory
  of another name, imports with `import_state` and opens `/bill`. The page where B lands gives the
  `session` line: `/bill` is `import=ok second_factor_asked_again=no`, `/verify` is
  `import=ok second_factor_asked_again=yes`, and any other page, `/signin` among them, is
  `import=miss`. The `profile` line shows `a_gone=yes` and `b_gone=yes` after the stops.
- The exported state of the fixture holds 3 cookies of the utility host (`halvo_fx_session`,
  `halvo_fx_ref`, `halvo_fx_device`). All three are host-only session cookies, and two of them are
  HttpOnly. The state holds no localStorage item. Its JSON form is 887 bytes on every machine and
  browser. The trial keeps it in memory and writes it nowhere.
- The `keychain` line reads the front application (`lsappinfo front` on macOS, the foreground window
  on Windows) and the list of on-screen windows of the session (CoreGraphics
  `CGWindowListCopyWindowInfo` on macOS, `EnumWindows` on Windows; no script of the system). The
  watch reads both every 250 ms, from before the first start until 10 s after the last stop.
  `prompt=yes` would mean that the front application changed, or that a new window appeared whose
  process belongs to the browser's tree or is a prompt process of the system (`SecurityAgent`,
  `coreautha`, `UserNotificationCenter`, `universalAccessAuthWarn`, `consent.exe`,
  `CredentialUIBroker.exe`). A failed read makes a `notrun` line. On macOS the library adds
  `--use-mock-keychain` on every start, on this Mac and on the hosted Mac alike. The browser then
  never reads the system keychain. So on macOS the line shows only that no other prompt of the system
  appeared. It cannot show whether the browser without the mock keychain would ask for the keychain.
  On a hosted runner nobody could answer a prompt, so a prompt would stay on screen and the line would
  read `yes`.
- Item 2 runs item 1 and then searches for the page marker of seed 1, in UTF-8, UTF-16LE and
  UTF-16BE. It searches every file under the parent directory of the two profiles and every entry
  that appeared in the temporary directory of the process during the run. On macOS it also searches
  the files that changed during the run in the browser's per-user places outside the profile:
  `Crashpad` under `~/Library/Application Support/Google/Chrome` and `.../Microsoft Edge`, and the
  helper caches under the per-user cache directory. The cookie `halvo_fx_ref` carries the page
  marker, and the browser stores its value encrypted. So the search also counts each occurrence of
  the name of the cookie whose exported value is the marker, and `marker_hits` sums both counts. The
  `trace` line counts the utility host name over the same places.
- Item 4 runs five runs of the nine steps of `tools/r3-trials/web` (`--runs 1` each) under
  `tree-peak`. `tree-peak` starts the command and, every 250 ms, sums the memory of the command and
  all its descendants. On macOS that is `phys_footprint` of each process. A process that the
  reparenting to launchd took out of the tree also counts when it started during the run and its
  executable lies in an `.app` bundle of the tree, or is the command's own binary. Chrome's
  `chrome_crashpad_handler` is such a process: Chrome double-forks it, and it runs with parent
  pid 1. On Windows `tree-peak` reads the peak of a Job object, which holds the command from its
  first instruction (a suspended start). The measured tree holds the trial program and the fixture
  server besides the browser, so the bound counts them too (see below).

## Machines, browsers and times

| machine | browser | version | `--browser` path |
|---|---|---|---|
| `mac` | Chrome | 153.0.8010.48 | `/Applications/Google Chrome.app/Contents/MacOS/Google Chrome` |
| `hosted-mac` | Chrome | 157.0.8103.39 | `/Applications/Google Chrome.app/Contents/MacOS/Google Chrome` |
| `hosted-mac` | Edge | 156.0.4314.58 | `/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge` |
| `hosted-win` | Chrome | 156.0.8078.80 | `C:/Program Files/Google/Chrome/Application/chrome.exe` |
| `hosted-win` | Edge | 156.0.4314.58 | `C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe` |

Items 1 and 2 ran once per cell. On `mac` they ran at commit 93592c3. On the hosted machines they ran
in run 42960070038, on a scratch branch that holds the same commit plus one commit on the workflow
file. That commit calls `run.sh` for both browsers. Item 4 on `mac` ran through the machine's
timed-run helper, which recorded the five runs from 2026-11-11T04:53:44Z to 2026-11-11T04:54:15Z, with
the 1-minute load average at 3.64 before and 5.56 after. The five peaks on `mac` lie between
957,172,600 and 1,007,752,288 bytes. At the peak, the browser's GPU and utility helpers held about
586 MB, its renderers about 262 MB and its main process about 81 MB. The fixture server held about
24 MB and the trial program about 4 MB, which the bound includes. On `hosted-win`, Edge is the
worse browser. The bound of `mac` is the larger one, so it is `browser_bound_bytes`.

## Each item can come out the other way

These runs on this Mac each gave the other result. The cuts and the plants ran at d869b1d, and the
other runs at d4a8426, whose item code is that of the cells:

- Cutting the exported state lands on `/signin`: `--leave-out all` and
  `--leave-out halvo_fx_session` gave `import=miss`.
- Leaving out the cookie that carries the second factor, with `--leave-out halvo_fx_device`, gave
  `import=ok second_factor_asked_again=yes`.
- A planted marker is found in each encoding and place: `--plant utf8|utf16le|utf16be:parent|temp`
  gave `marker_hits=1` in each of the six runs, with the hit in the right encoding.
- With the removal of the profile turned off in a copy of the code, both profiles stayed and
  `marker_hits=4`, all four from the stored cookie. The marker itself appears in no file in plain
  UTF-8 or UTF-16: the fixture sends `Cache-Control: no-store`, and the cookie store keeps the value
  encrypted. The `trace` line gave `host_hits=122`. A finder of the review below ran the same
  mutant before the search counted the stored cookie, and reported `marker_hits=0`.
- A tree of known allocations: `tree-peak --runs 3 -- alloc-tree` with children that allocate and
  write 100, 200 and 300 MiB, and an orphan of 150 MiB that a child double-forks. Take its peak minus
  the peak of the same tree with no allocation (6,424,328 bytes on `mac`, 4,014,080 bytes on
  `hosted-win`). That difference exceeded the known sum of 786,432,000 bytes by 459,280 bytes on
  `mac` and by 1,576,960 bytes on `hosted-win`. Every case of the proof (600 MiB without the orphan,
  400 MiB in the orphan alone) stayed within 0.3 %, and the three runs of each case agreed within
  0.1 %. So the tolerance of `tree-peak` on a tree of known allocations is 0.3 % plus the baseline.
  A peak shorter than the 250 ms period escapes the macOS reading: a 400 MiB burst of 120 ms read as 2,753,136 bytes. The Windows Job
  object keeps its own peak.

## Places outside the profile

A run left nothing of the session outside the profile directory. Run 42962050772 repeated items 1
and 2 on the hosted machines with the same lines. After each browser it read every file that had
changed under the home directory and the per-user directories since the start (496 and 522 files on
`hosted-mac`, 1 and 3 on `hosted-win`), and found the marker in none. The browsers wrote these
places besides their profiles:

- Chrome on macOS: `~/Library/Application Support/Google/Chrome/Crashpad/` (`settings.dat`), and Metal
  shader caches under `<per-user cache dir>/com.google.Chrome.helper/com.apple.metal*/`. On this
  Mac it also made an app clone for code signing under
  `<per-user dir>/X/com.google.Chrome.code_sign_clone/`, which it removes at a clean exit.
- Edge on macOS: `~/Library/Application Support/Microsoft Edge/Crashpad/`, the Metal caches under
  `<per-user cache dir>/com.microsoft.edgemac.helper/`, and one file in the group container
  `~/Library/Group Containers/UBF8T346G9.com.microsoft.edgemac.widgets/`.
- Windows: no file under the user's profile directory changed during a Chrome run except the
  system's `AppData/LocalLow/Microsoft/CryptnetUrlCache/MetaData/`. During an Edge run, 3 files
  changed in `AppData/Local/Microsoft/TokenBroker/Cache/`. The temporary directory got no new entry
  that stayed.

## Review before the counted cells

A review with three finders and one skeptic ran before the counted cells. It found nine points; the
ones that could make an item read ok while its condition did not hold were fixed before the cells:
the stored cookie (above), a check that profile B is gone, a watch that sampled only four times, and
a `tree-peak` that adopted any orphan from the directory of the command and that orphan's children.
`tree-peak` still misses a peak between two samples on macOS and an orphan whose executable lies
outside the tree's bundles and is not the command's binary; `treeinfo lifetime_max_sum` gives an
upper bound for the processes that it saw.

```text
licence item=libc@0.2.191 spdx=MIT read=2026-11-11 ok=yes
licence item=windows-sys@0.61.2 spdx=MIT read=2026-11-11 ok=yes
flag --disable-background-networking
flag --disable-component-update
flag --disable-sync
flag --disable-field-trial-config
flag --disable-client-side-phishing-detection
flag --disable-default-apps
flag --disable-breakpad
flag --disable-domain-reliability
flag --metrics-recording-only
flag --no-first-run
flag --no-default-browser-check
hosted machine=hosted-mac run=42960070038 sha=dc69cc0b8fb7f603eae167a7db0ecf17fae33681
hosted machine=hosted-win run=42960070038 sha=dc69cc0b8fb7f603eae167a7db0ecf17fae33681
session machine=mac browser=chrome import=ok second_factor_asked_again=no
session machine=hosted-mac browser=chrome import=ok second_factor_asked_again=no
session machine=hosted-mac browser=edge import=ok second_factor_asked_again=no
session machine=hosted-win browser=chrome import=ok second_factor_asked_again=no
session machine=hosted-win browser=edge import=ok second_factor_asked_again=no
keychain machine=mac browser=chrome prompt=no
keychain machine=hosted-mac browser=chrome prompt=no
keychain machine=hosted-mac browser=edge prompt=no
keychain machine=hosted-win browser=chrome prompt=no
keychain machine=hosted-win browser=edge prompt=no
disk machine=mac browser=chrome marker_hits=0
disk machine=hosted-mac browser=chrome marker_hits=0
disk machine=hosted-mac browser=edge marker_hits=0
disk machine=hosted-win browser=chrome marker_hits=0
disk machine=hosted-win browser=edge marker_hits=0
browser machine=mac browser=chrome run=1 peak_bytes=974343656
browser machine=mac browser=chrome run=2 peak_bytes=974098160
browser machine=mac browser=chrome run=3 peak_bytes=1007752288
browser machine=mac browser=chrome run=4 peak_bytes=972918248
browser machine=mac browser=chrome run=5 peak_bytes=957172600
browser machine=hosted-win browser=chrome run=1 peak_bytes=312467456
browser machine=hosted-win browser=chrome run=2 peak_bytes=311422976
browser machine=hosted-win browser=chrome run=3 peak_bytes=317095936
browser machine=hosted-win browser=chrome run=4 peak_bytes=314404864
browser machine=hosted-win browser=chrome run=5 peak_bytes=310804480
browser machine=hosted-win browser=edge run=1 peak_bytes=353570816
browser machine=hosted-win browser=edge run=2 peak_bytes=353222656
browser machine=hosted-win browser=edge run=3 peak_bytes=353067008
browser machine=hosted-win browser=edge run=4 peak_bytes=356106240
browser machine=hosted-win browser=edge run=5 peak_bytes=358105088
browser machine=hosted-win bound_bytes=444596224
browser machine=mac bound_bytes=1224736768
verdict profile=ephemeral egress=open browser_bound_bytes=1224736768
```
