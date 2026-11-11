# The page-test harness

`tools/r4-page/` is the instrument that runs the six accessibility checks of the interface run (contracts.md I9.3) in a real browser engine. This file says how to call it, and records what was read before the first fetch and every choice that the contract leaves to the harness.

## Fetch

```sh
sh tools/r4-page/fetch.sh DIR [--browser webkit|chromium]
```

The script copies `tools/r4-page/package.json` and `tools/r4-page/package-lock.json` into DIR and runs `npm ci --ignore-scripts` there, with the npm cache under `DIR/npm-cache`. It downloads the archive of one browser build, compares the archive with the size and SHA-256 of its line in `tools/r4-page/files.tsv`, and only then writes it to `DIR/archives/` and unpacks it to `DIR/builds/<build>-<system>/`. The build is the one that `tools/r4-page/browser.txt` names for the machine, or the one of `--browser`. The machine is `hosted-mac` or `hosted-win` by the system when `CI` is set, and `mac` otherwise. A second call keeps an archive whose hash is right, a build that it unpacked from that archive, and a package that it installed from the same lock. The script writes nothing outside DIR. It is the only step of the harness that reaches a network: the npm registry and Playwright's download host.

On success it prints one line, for example `fetch browser=webkit build=webkit-2388 system=mac26-arm64 bytes=85789503 archive=kept extracted=no package=kept`, and exits 0. It exits 1 with a line `fetch failed step=<download|extract|package|internal> ...` when a step fails. It refuses with exit 2 and one line on standard error:

| cause | when |
|---|---|
| `usage` | no DIR, a second DIR, or a flag other than `--browser webkit` or `--browser chromium` |
| `browser` | `browser.txt` names no browser for the machine, holds a malformed line, or Playwright 1.65.0 has no build for the system |
| `files` | `files.tsv` has no line for the browser on the system, or holds a malformed line |
| `hash` | the downloaded archive differs from its line in size or SHA-256; nothing is written |

On this Mac the fetch goes to `tools/r4-page/.browsers/`, which `tools/r4-page/.gitignore` leaves untracked. The library and the self-test read the directory in `R4_PAGE_BROWSERS`, else `tools/r4-page/.browsers/`.

## The four functions

`tools/r4-page/index.mjs` exports four functions.

- `serve({ui, fixtures})` serves, from 127.0.0.1 on a free port, the directory `ui` at `/`, the directory `fixtures` at `/__fixtures/`, the harness frame at `/__r4/frame/`, the harness sample at `/__r4/sample/` and the engine's `ace.js` at `/__r4/engine/ace.js`. It returns the origin, for example `http://127.0.0.1:52011`. The server does not keep the process alive.
- `open({origin, state, views, frame, replies, out})` starts the browser of the machine headless, at a viewport of 1024 by 768 CSS pixels, and returns a page handle. `state` is the name of a state: the stub reads `state-<state>.json` from the fixtures directory. `views` is a list of script paths on the origin, for example `['/app/views/needs.js']` when `ui` is the repository's `ui/` directory. `frame` is a path on the origin and defaults to `/__r4/frame/index.html`. `replies` is a list of transcript lines (I13.3), as strings or as objects, and replaces `state`. `out` is a directory for `calls.jsonl` and `actions.txt`. The handle has `close()`, which closes the page and, after the last open page, the browser.
- `act(page, {act, param, n, text})` activates the one control that has that `data-act` (and that `data-param` and `data-n`): a click for a button, a choice for a radio input, and `text` typed key by key for the control `typed`. It throws when no control or more than one control matches, and when the page made a call that the transcript does not hold next.
- `check(page, n, {snapshot, write})` runs check `n` and returns `{ok, counts, names}`. `snapshot` is the path of the structure file, which checks 3 and 5 need. With `write`, check 3 writes the file and returns `ok`.

What each check returns:

| n | `counts` | `names` |
|---|---|---|
| 1 | `{violations, review}` | the rule ids with a violation |
| 2 | `{keyboard: 'ok'|'miss'}` | the misses: `unreached`, `focus_unseen`, `dialog_not_taken`, `dialog_focus_lost`, `dialog_not_closed`, `opaque` |
| 3 | `{structure: 'same'|'differ'}` | `opaque` on that miss, else none |
| 4 | `{size: 'ok'|'miss'}` | the `data-act` name, or the tag name, of each control that is too small |
| 5 | `{modes: 'ok'|'miss', modes_off}` | the modes that the browser could not turn on, or `opaque` |
| 6 | `{markup: 'ok'|'miss'}` | `elements` or `opaque` on a miss |

`open` prints `browser name=<webkit|chromium> version=<v>` on standard output the first time that it starts a browser in a process. Check 1 throws an error with `code` `engine` and a `reason` when the engine cannot run (see below). Every other misuse throws an error with `code` `usage` or `fixture`.

## The stub

The page gets `Halvo.ipc.call` from the harness before any script of the page runs. A page script cannot replace it, also not by assigning a new `window.Halvo`. The stub answers `agents_snapshot` with the `snapshot` of the state file, `agents_card` with its `card`, and every other call with `reply-ok.json`. It answers `agents_next` with `reply-next-same.json` after 1,000 ms, so that a view that calls again at once makes at most one call a second. With `replies`, the first `state` line answers `agents_snapshot`. Each `call` line must match the next call other than `agents_next` in command and arguments, and its `reply` answers it. A later `state` line answers the `agents_next` call that waits at that moment, or the next one to come.

Under `out`, the stub writes `calls.jsonl`, one line `{"n":<count>,"call":"<command>","args":{...}}` per call other than `agents_snapshot` and `agents_next`, and `act` writes `actions.txt`, one line `action i=<count> type=<click|choose|type> act=<name>` per action.

The harness frame `tools/r4-page/frame/index.html` loads the view scripts in order, places one section per view by `order`, with an `h2` that holds `heading`, calls each view's `mount(element)` and then its `show()`. `Halvo.views.window(state)` calls `show()` of every view for the state `shown` and `hide()` for any other state.

## The rule engine

| item | value as read |
|---|---|
| package | `accessibility-checker-engine` 4.0.34 |
| licence | Apache-2.0 |
| file loaded into the page | `ace.js` of the package, served by the harness from 127.0.0.1 |
| rule set | `WCAG_2_2`, the package's rule set for WCAG 2.2 levels A and AA |

Each result of the engine has a value of two words. A result with `VIOLATION` and `FAIL` is the level `violation`, which fails check 1. A result with `PASS` is the level `pass` and is not counted. Every other result is counted as `review` and fails nothing. These are the levels `potentialviolation` (`VIOLATION`, `POTENTIAL`), `recommendation` (`RECOMMENDATION`, `FAIL`), `potentialrecommendation` (`RECOMMENDATION`, `POTENTIAL`) and `manual` (`MANUAL`), and any result with `INFORMATION`.

A route of the browser aborts and counts every request to a host other than 127.0.0.1. The engine is `none` when its package has no `package.json` or no `ace.js`, when its licence is outside the list of contracts.md I0.7, when the page has no `ace.Checker` after the load, when the engine has no rule set `WCAG_2_2`, when it throws, when it runs no rule, or when the route counted a request during its run. The error names the reason: `package`, `file`, `licence`, `ruleset`, `throws`, `no_rule` or `request`.

## The six checks

Checks 2 to 6 read the top document alone. So each of them misses, with the name `opaque`, on a page that holds an `iframe`, `frame`, `object` or `embed`, or on which a script has attached a shadow root, open or closed: the harness cannot see the controls, names and elements there. Writing a structure file is exempt.

The role of an element is the first token of its `role` attribute that is an ARIA role, as a browser computes it. The controls of a state are the elements that are rendered (not `display: none`, not `visibility: hidden`, with a box), not disabled, and either a native control (`button`, `input` other than hidden, `select`, `textarea`, `a` with `href`, `summary`, an element that the person can edit), an element with `data-act`, an element with a `tabindex` of 0 or more, or an element whose role is a widget role. A confirmation is a rendered element whose role is `alertdialog`. When one is open, the controls of check 2 are those inside it. Without one, an element inside `inert` is no control of check 2.

1. The engine runs on the whole document with the rule set `WCAG_2_2`.
2. The walk presses the walk key of the machine, starting from where the focus is after the page settled, until the focus comes back to where it started, at most twice the count of controls plus 8 times. A press that leaves the focus on the body counts as no stop. Every control must be a stop that a key press reached: the focus that the page set at load does not count. Radio inputs with one `name` count as one control, reached when one of them is a stop, because the key moves the focus into the group once. For each stop, the computed outline and border with the focus are compared with those after the walk, with no element focused. The focus is visible when the outline is on (its style is not `none`, its width is above 0 or its style is `auto`, its colour is not transparent) and differs from the outline without the focus, or when the border of a side differs and can be seen in one of the two states (its style is not `none` or `hidden`, its width is above 0, its colour is not transparent). A box shadow does not count, and nor does the contrast of an outline with what lies behind it. With a confirmation open, the focus must be inside it when the page has settled, and no stop may lie outside it. The safe control is the first control of the confirmation in document order, as I3.4 and I4.3 place it. On a second page of the same state the harness focuses the safe control and presses Enter, and the confirmation must be gone when the page has settled.
3. The structure is Playwright's ARIA snapshot of `body` in its JSON form (`ariaSnapshotJSON`): one line per node, indented by two spaces per level, `- ` and the node's keys other than `children` as compact JSON with sorted keys, with an LF at the end. The file is compared byte for byte. The text form of the snapshot leaves out a name of 900 characters or more (evidence of this unit, probe-longname); the JSON form keeps it. Both builds give the same text for the two sample states.
4. Every control has a box of at least 24 by 24 CSS pixels, and every control inside a confirmation at least 44 by 44. The box is the element's own box, also for a radio input with a label.
5. Each mode opens a new page of the same state with the mode on: `reduced_motion` (`prefers-reduced-motion: reduce`), `forced_colors` (`forced-colors: active`) and `width_320` (a viewport 320 CSS pixels wide). The harness reads whether the mode is on through `matchMedia` or the width of the window. A mode that is off is named in `modes_off`. In each mode that is on, checks 1 to 4 run (check 3 when `snapshot` is given, against the same file), and the page must have no horizontal scroll: the scrolling element's `scrollWidth` must not exceed its `clientWidth`, and neither may that of `html` or `body` when its `overflow-x` is `auto` or `scroll`. A scroll bar of an inner region is not the page's. Check 6 does not change with a mode and does not run in one.
6. Two new pages of the state are opened: one with the fixture as it is, and one where every text value is the string `<b data-x="1">x</b>&amp;`. The check passes when the two pages have the same count of elements and the second has no element with `data-x`. A text value is a string, or a string in a list, under a key that the reply shapes of I2.5, I2.6 and I3.4 write as `"<text>"`: `name`, `word`, `line`, `say`, `level`, `text`, `title`, `label`, `value`, `from_text` and `note`. The ids, the names of closed lists, the days (`next`, `day`) and the counts stay as they are, as the plan's verifier reads "text value" (reports/verifier-plan-1.md of the run). An empty string stays empty, because a view may leave out the element of an empty text.

## What the checks do not see

A page that changes later than the harness waits is checked as it was: the harness waits for the load, for the stub's answers, for 80 ms with no change of the document and for two animation frames. contracts.md I8.4 bars a view from setting a timer or an interval, and the page check of U3.3 fails a view file that names `setTimeout` or `setInterval`. A page that sees the engine's script tag and hides a fault while the engine runs passes check 1. A view's `innerHTML` on a day (`next`, `day`) passes check 6, because a day is not replaced. The harness review of this unit found each of these; the report of builder-1 lists them.

## Browser builds

Each build that a machine can start has a line in `tools/r4-page/files.tsv`: the build, the system name that Playwright 1.65.0 uses for its downloads, the address on Playwright's download host, the size in bytes and the SHA-256. One download of each archive into a directory from `mktemp -d` gave the size and the hash. Nothing in that directory was run. The library starts the build of its machine by its path, and only when the build's unpacked directory holds the hash of its line.

| build | system | machines | address | licence as read |
|---|---|---|---|---|
| `webkit-2388` | `mac26-arm64` | `mac` | `https://cdn.playwright.dev/dbazure/download/playwright/builds/webkit/2388/webkit-mac-26-arm64.zip` | The archive holds no licence file for WebKit itself. It holds the licence files of four parts of the web inspector (CodeMirror, three.js, Esprima, CSSDocumentation). |
| `webkit-2388` | `mac15-arm64` | `hosted-mac` | `https://cdn.playwright.dev/dbazure/download/playwright/builds/webkit/2388/webkit-mac-15-arm64.zip` | As for `mac26-arm64`. |
| `chromium-1254` | `win64` | `hosted-win` | `https://cdn.playwright.dev/builds/cft/158.0.8127.18/win64/chrome-win64.zip` | Chrome for Testing 158.0.8127.18. The archive holds no licence file. Its `ABOUT` file says "Copyright 2026 Google LLC. All rights reserved", names the Chromium project and other open source software (`chrome://credits`), and points to the terms of service at `chrome://terms`. |
| `chromium-1254` | `mac26-arm64`, `mac15-arm64` | none in `browser.txt` | `https://cdn.playwright.dev/builds/cft/158.0.8127.18/mac-arm64/chrome-mac-arm64.zip` | The same `ABOUT` text. The archive also holds the Widevine module with Google's licence, which says that the module is not open source. |

The SHA-256 of each archive is in its line of `files.tsv`. contracts.md I0.7 puts these builds outside the licence rule, because they run only as tools of a check. This harness does not read the publishers' own statements of these licences, because a step of this unit reaches only the npm registry and Playwright's download host. The Chromium build is the full Chrome for Testing build in Chromium's headless mode, not Playwright's headless shell, because the system web view of Windows is the full browser. The two `mac` lines of `chromium-1254` let a machine change its line of `browser.txt` to `chromium` without a new download line, and let this Mac run the Windows engine family.

WebKit 27.2 of `webkit-2388` starts headless on this Mac (macOS 27.2) and on `hosted-mac`. Chrome for Testing 158.0.8127.18 starts headless on this Mac and on `hosted-win`.

## Walk key

| machine | browser | walk key | measured |
|---|---|---|---|
| `mac` | webkit | Alt+Tab | Tab moved the focus only to the text input and to the body. Alt+Tab reached each button, the link, the radio group and the text input. |
| `hosted-mac` | webkit | Alt+Tab | The sample's walk reaches every control with this key in the hosted run. |
| `hosted-win` | chromium | Tab | The sample's walk reaches every control with this key in the hosted run. |

The library takes Alt+Tab for WebKit on macOS and Tab for every other pair.

## Modes

On this Mac both builds turned all three modes on. WebKit's forced colours turn on the media feature `forced-colors: active` and force no colour: a text colour of the page stays as it is. Chromium's forced colours replace the colours of the page, and give a focused button a border in the system's highlight colour, also when the page sets `outline: none`. So a page that removes its focus outline in forced colours passes check 2 there on Chromium, and the walk in normal colours still finds it.

## How the driver acts

Playwright drives the browser through the browser's own protocol. Its documentation in the type file of `playwright-core` 1.65.0 says that "Keyboard provides an api for managing a virtual keyboard" and that a key press "Dispatches a `keydown` event". It says nothing about events of the operating system on macOS. The workflow run measured that Playwright posts no input event on Windows (W builder-1). For macOS this harness has no measurement. The front app of this Mac was the same before and after the first headless start.

## The self-test

```sh
node tools/r4-page/selftest.mjs
```

The self-test serves `tools/r4-page/sample/`, opens the sample view `sample/view.js` on the states `a` and `b`, and runs each check on both states and on each planted fault of that check. `state-a.json` holds two agents and one setup item with a radio group and a typed input. `state-b.json` holds one agent and one confirmation, which the sample view shows in a modal dialog. The faults are chosen by a query string of the view's address (`/view.js?fault=<name>`):

| check | fault | state | what the fault does | what must miss |
|---|---|---|---|---|
| 1 | `contrast` | a | grey text of too little contrast | a violation |
| 2 | `unreachable` | a | a button with `tabindex="-1"` | `unreached` |
| 2 | `focus_unseen` | a | a button whose outline is `none` with the focus | `focus_unseen` |
| 2 | `focus_escapes` | b | the confirmation opens as a dialog that is not modal | `dialog_focus_lost` |
| 3 | `renamed` | a | a button with another name | `differ` |
| 4 | `small` | a | a button of 20 pixels in height | the control `delete` |
| 5 | `motion_hidden` | a | a control hidden with reduced motion | the mode `reduced_motion` alone |
| 5 | `forced_hidden` | a | a control hidden in forced colours | the mode `forced_colors` alone |
| 5 | `narrow_scroll` | a | a line at least 400 pixels wide | the mode `width_320` alone |
| 6 | `inner_html` | a | the first line set as markup | `miss` |

It prints `selftest check=<n> sample=<ok|miss> fault=<caught|missed>` for each check, then `page self-test: <k> of <k> ok engine=<offline|none>`. With `engine=none` it runs checks 2 to 6 and prints `engine none reason=<reason>` on standard error. A fault of check 5 whose mode is off on the machine is not counted. The self-test exits 0 when every check passed, 1 when one did not, and 2 on the refusals `usage` (an argument), `browser` (no fetched build of the machine) and `fixture` (a sample file is missing or is no JSON). It records no time.
