# Browser profile trial

Unit U0.3 of the agent-setup run tests the privacy model of the agent browser (contract A13.3,
A8.3). A run's browser starts with an empty profile and a temporary directory of its own, and the
run removes both at its end. A sign-in that passed a second factor reaches the next run only as an
exported state. This first pull request holds items 1, 2 and 4. Item 3 (egress) and the `flag`
lines come with the second, so the verdict ends with `egress=open`.
`tools/r3-trials/profile/README.md` says how each item runs and holds the longer notes.

What ran. Every cell ran at commit 03f4c53. Items 1 and 2 ran once per cell: on `mac` with Chrome,
and on `hosted-mac` and `hosted-win` with Chrome and Edge in hosted run 42976989121. That run is of a
scratch branch with that commit plus one commit on the workflow file. Item 4 ran in its two shapes:
on `mac` with Chrome through the timed-run helper (`shape=steps` 2026-11-11T09:10:31Z to 2026-11-11T09:11:20Z,
`shape=task` 2026-11-11T09:11:20Z to 2026-11-11T09:13:12Z), and on `hosted-win` with Chrome and Edge in the same
hosted run. The `--browser` paths are `/Applications/Google Chrome.app/Contents/MacOS/Google Chrome`
and `/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge` on the Macs, and
`C:/Program Files/Google/Chrome/Application/chrome.exe` and
`C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe` on `hosted-win`.

Choices that the contract leaves. Item 2 runs item 1 first, so one run gives the three lines of a
cell. A landing on `/bill` is `import=ok second_factor_asked_again=no`, one on `/verify` is
`second_factor_asked_again=yes`, and any other is `import=miss`. Origin storage is localStorage.
`marker_hits` counts the marker in three encodings, the name of the cookie whose value is the marker
(the browser keeps that value encrypted), and one for each profile or temporary directory left on
disk. The task shape of item 4 signs in, passes `/verify`, opens `/bill` and `/pay`, and holds that
page for 20 s. Both shapes also measure the trial program and the fixture server.

What the lines cannot show. The state carries cookies and localStorage only, not sessionStorage,
IndexedDB or the HTTP cache, and the fixture uses none of them. Each cell of items 1 and 2 is one
run. The search matches raw bytes, so it cannot see the marker in compressed bytes. On macOS every
start adds the mock keychain, so `prompt=no` cannot show what the product's browser asks of the
keychain. Chrome and Edge on macOS keep their singleton directory in the system's temporary directory
although `TMPDIR` names the run's own (on Windows both write into the run's own), so the search of
the system's temporary directory stays. On `hosted-win` the search read no file after the removal,
so `marker_hits=0` rests there on the removal. On macOS `tree-peak` misses a peak shorter than its
50 ms period.

```text
licence item=libc@0.2.191 spdx=MIT read=2026-11-11 ok=yes
licence item=windows-sys@0.61.2 spdx=MIT read=2026-11-11 ok=yes
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
