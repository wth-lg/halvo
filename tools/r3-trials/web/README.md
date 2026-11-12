# Web trial

This directory holds the web executor trial of unit U0.2 (contract A13.2) and the isolation rows of
unit U0.7 (contract A13.7) of the agent-setup run. The library `r3_web` (`src/lib.rs`, `src/cdp.rs`
and `src/cdp/`) is the trial's adapter on chromiumoxide 0.9.1, which the profile trial of U0.3 also
uses. `src/bin/web-trial.rs` is the trial program, and `run.sh` is its one entry script, whose
`--help` states every argument. The result files are `docs/r3/trials/web-executor.md` and
`docs/r3/trials/web-isolation.md`. Each holds at most 40 lines of text above its block, as A13.1
asks, and the longer notes are here.

## The executor trial (U0.2)

This section was the text above the block of `docs/r3/trials/web-executor.md` until U0.7 cut that
file to the 40 lines of A13.1. It describes U0.2's own runs; U0.7 ran the nine steps again, and
the result file names those runs.

### How the cells ran

`sh tools/r3-trials/web/run.sh --candidate cdp --browser PATH --port 18190 --runs 10` builds the
trial program with `cargo build --release --locked`, starts the fixture server of unit U0.1 on the
port, and runs the nine steps of A13.2 in their order, 10 runs each, with no model. Before each run
it resets the fixture to seed 1 with the variants of the step, and starts the browser headless on a
new profile directory, one per run, inside the one scratch directory of the call: a new mktemp
directory, or the directory that `--scratch` names. After the run it stops the browser. The steps
read the fixture's state through the program's own HTTP client, which sends no `Origin` and no
`Sec-Fetch-Site` header. `--help` states every argument.

- Each step checks the column "ok when" of A13.2 on the fixture's state. Where that state cannot see
  what the browser did, the step also reads the request that the browser sent, through the
  adapter's request rule, which pauses each request before it leaves. `form_fill` requires
  `paperless=yes` in the body of the `POST /prefs`. `file_upload` requires that the file input
  holds one file of 1,024 bytes before the click and that the browser sent one `POST /upload`.
  The paused request carries no bytes of a file part, so the trial cannot read the file in the
  request, and the fixture records no size. `payment_frame` chooses the second option of
  `Payment method`, which the page does not select by default, reads the choice back from a new
  snapshot, and requires that the payment holds the second instrument of the seed, so a choice that
  does nothing misses. `changed_page_1` requires the first instrument, the page's default.
  `block_request` requires that the browser accepted the command that fails the request and that
  the page then shows the browser's error page, besides the unchanged `requests.state_changing`.
  The fixture takes the amount of a payment from its token, not from the browser, so the amount
  check holds whenever a payment is recorded.
- `median_ms` is the median, over the runs of the cell that printed ok, of the time from the first
  page open of the run to the end of its check. It leaves out the browser start. `cold_ms` is the
  median of 10 starts, from the launch call to the load of `/signin`.
- Each step was first seen to miss. `--fault NAME` puts `tools/r3-trials/web/faults.mjs` between
  the browser and the fixture, and the proxy breaks one route or one page. Each step missed once
  under its fault: `signin_form` under `signin_500`, `table_read` under `history_changed`,
  `form_fill` and `session_expiry` under `prefs_500`, `payment_frame` under `frame_hangs` (the frame
  never loads), `file_upload` under `upload_input_gone`, `changed_page_1` under `pay_now_renamed`,
  `block_request` under `prefs_get_form` and `credential_mark` under `password_plain`. A control
  run of all nine steps behind the proxy with `history_changed` missed only `table_read`.
- The cells of `mac` ran through the machine's timed-run helper in two calls. The helper recorded
  the nine steps from 2026-11-11T03:30:49Z to 2026-11-11T03:31:43Z, with the 1-minute load average
  at 4.90 before and 9.06 after, and the extras from 2026-11-11T03:31:43Z to 2026-11-11T03:31:49Z,
  with 9.06 before and 10.66 after. The fault runs above ran on the same code outside the helper,
  one run each. Other work on this Mac set that load, so the times of `mac` are times under load.
- The cells of the hosted machines ran in one job per machine, Chrome first and then Edge, on the
  head of the trial code plus one commit that adds the call of `run.sh` to the workflow file. The
  `hosted` lines name the run and that commit.

### Start of the browser

The `flag` lines in the block are the first flag set. The trial passes them, the resolver rule and,
on macOS, `--use-mock-keychain`, and nothing else of its own. chromiumoxide adds these flags itself: `--remote-debugging-port=0` (the
browser picks the port; `--extras` names one for the attach check), `--user-data-dir`,
`--headless=new`, `--hide-scrollbars`, `--mute-audio` and `--disable-extensions`. The trial turns
off chromiumoxide's own list of 24 default flags (`disable_default_args`).

The resolver rule is one flag with two rules:
`--host-resolver-rules=MAP *.test 127.0.0.1,MAP * ~NOTFOUND`.

`sh tools/r3-trials/web/run.sh --candidate cdp --browser PATH --port 18190 --resolver-probe` showed
the rule at work on this Mac on 2026-11-11. The name `halvo-resolver-probe.localhost` got
`net::ERR_NAME_NOT_RESOLVED` and sent 0 requests to the fixture. Chrome resolves a `.localhost`
name to loopback by itself, with no lookup, so without the second rule the fixture would have seen
that request. `utility.agent.test` sent 2 requests to the fixture. The probe makes no lookup of a
name.

On macOS, both on this Mac and on the hosted macOS image, the caller adds `--use-mock-keychain`,
which no `flag` line holds. The first start of a browser in this unit, at 2026-11-11T00:25:58Z,
left the front app as it was (75 samples over 75 s, one app) and left no process behind.

### Browsers

Each run line holds the product and version that the browser reported over the protocol
(`Browser.getVersion`). All 90 runs of each machine and browser, 9 cells of 10 runs, reported one
version:

| machine | browser | version | `--browser` path |
|---|---|---|---|
| `mac` | Chrome | 153.0.8010.48 | `/Applications/Google Chrome.app/Contents/MacOS/Google Chrome` |
| `hosted-mac` | Chrome | 157.0.8103.39 | `/Applications/Google Chrome.app/Contents/MacOS/Google Chrome` |
| `hosted-mac` | Edge | 156.0.4314.58 | `/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge` |
| `hosted-win` | Chrome | 156.0.8078.80 | `C:/Program Files/Google/Chrome/Application/chrome.exe` |
| `hosted-win` | Edge | 156.0.4314.58 | `C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe` |

No image holds a second major version of Chrome. The macos-15 image also holds Google Chrome for
Testing 157.0.8103.38, the same major as its Chrome. The windows-2025 image holds one version
directory under its Chrome install. All cells ran headless. Hosted runners can open a headed
window, but this trial did not open one.

### The library

- `pipe=no`: chromiumoxide 0.9.1 starts the browser with `--remote-debugging-port` and reads the
  WebSocket address from the browser's standard error, and it connects only over a WebSocket. It
  has no way to use `--remote-debugging-pipe`. Read on 2026-11-11 in `src/browser/config.rs` and
  `src/browser/mod.rs` of the package that cargo fetched from
  `https://crates.io/crates/chromiumoxide/0.9.1`.
- A snapshot gives each text node of the accessibility tree its own `text` line, so a label in the
  frame and the amount beside it come out as two `text` lines. A credential field follows A8.2: an `input` of type
  `password` or with one of the three `autocomplete` tokens, or a control whose parent or an
  ancestor of it carries `data-halvo-captcha`.
- The licence lines cover every crate in the trial's lock file, 138 packages, read from each
  crate's manifest with `node tools/r3-trials/web/licences.mjs --read 2026-11-11`. `unicode-ident`
  joins two licences with AND and has one line for each. The crate chromiumoxide is
  `MIT OR Apache-2.0`, with both texts in its package. No driver binary and no runtime are fetched.
- `added_bytes` is the growth of the stripped release binary of `web-trial` built with the feature
  `driver` (chromiumoxide, tokio, futures) against the same program built with
  `--no-default-features`, on this Mac.
- `tools/r3-trials/web/snapshots/pay-seed-1.txt` is the snapshot of `/pay` of seed 1 with its frame
  in the line form of A11.1.

## The isolation rows (U0.7)

Unit U0.7 adds the six rows of contract A13.7. They show what the one-site fixture cannot: a
payment frame in a process of its own, a post from a new window, a post from that frame, a
WebSocket handshake to a host that is not allowed, and a page behind a certificate that the system
does not trust. `sh tools/r3-trials/web/run.sh --candidate cdp --browser PATH --port N --runs 10
--rows isolation [--step NAME]` runs them in place of the nine steps. Each browser of these rows
gets one start flag more, `--isolate-origins=http://pay.agent.test:N`, which run.sh adds. Every
row resets the fixture to seed 1 with its variants and starts its own browser on an empty profile,
as the nine steps do.

### How each row runs

- `payment_frame_isolated` and `changed_page_1_isolated` run `payment_frame` and `changed_page_1`
  of A13.2 unchanged. Then they require a target of type `iframe` whose address is on the pay
  origin, as `Target.getTargets` reports it. Chromium gives a frame a target of its own only when
  the frame runs out of the process of its page.
- `block_popup`, `block_frame_post` and `block_websocket` sign in, arm the rule with the utility and
  pay origins allowed and state-changing requests failed, and then open their page. The trial's
  script runs in the page's main world through `Runtime.evaluate`, which is no operation of A11.1.
  The step reads the counters of the row from `GET /state` after the script's change and before
  the action. It reads them again 2,000 ms after the executor returned the paused request, and
  once more after that read returned. A run is ok only when all three reads agree and the paused
  request is the row's, failed by the browser. A counter missing from the state object is a miss.
  When no paused request comes within 15 s, the step still waits 2,000 ms and reads twice, so a miss
  names the counters that moved. `median_ms` of these rows runs from the action to the moment the
  paused request appears in the executor's list.
- In `block_popup`, the action is the click of `Save preferences`, after the click of `Paperless
  billing`. In `block_frame_post` it is the click of `Pay now` in the frame. In `block_websocket`
  it is the script's `new WebSocket("ws://evil.agent.test:N/")` itself.
- `cert_error` makes a self-signed certificate for `utility.agent.test` and its key with the
  system's `openssl req -x509 -newkey rsa:2048` and a config file. All three files go in a new
  directory of the run's scratch directory. Nothing of them is printed or committed.
  `tls-listener.mjs` serves them with `node:https` on a loopback port that the system picks.
  The step opens `https://utility.agent.test:<that port>/` and then asks the listener for its count
  over standard input. It is ok when `Page.navigate` returned an error that starts
  `net::ERR_CERT_` and the listener counted no request. Where `openssl version` does not run, the
  cell is `notrun ... step=cert_error cause=openssl`.
- A request that reaches the fixture after the step's last read is not seen.

### What the library changed

- A second DevTools connection of the library. chromiumoxide answers a child target's wait itself
  and gives no way to send a command on a child's session, so the library opens one more connection
  to the browser's debugging address with chromiumoxide's own `Connection`, which adds no package.
  On that connection it sends `Target.setAutoAttach` with `waitForDebuggerOnStart` and `flatten` to
  the browser, and again to every page and frame that attaches. It resumes a target with
  `Runtime.runIfWaitingForDebugger` only after it has prepared the target. A target that it cannot
  prepare stays waiting, and `attach_faults()` names it.
- The request rule lives on that connection, at two levels. Arming sends `Fetch.enable` with the
  pattern `*` at the request stage to the browser itself, and to every attached page and frame; a
  new page or frame gets it before it resumes. Neither level sees every request alone. The browser
  level misses `fetch` with `keepalive` and `navigator.sendBeacon`. A page's own level missed the
  first requests of a sandboxed `srcdoc` frame, which Chrome 153 runs in a process of its own,
  and of a worker. A request that both levels pause is recorded once, by its network id. A worker
  whose session refuses `Fetch.enable` is covered by the browser level and is not kept waiting.
  Arming bumps a rule version. A target whose preparation saw an older version is armed again
  before it resumes, so no target waits for another. Each paused request carries the id of the
  frame that made it: `block_popup` requires a frame other than the executor's main frame, and
  `block_frame_post` the pay frame's own target.
- `the_armed_rule_fails_every_post_to_a_host_that_is_not_allowed`, an ignored test of the library
  (`R3_TEST_BROWSER=PATH cargo test --lib -- --ignored`), arms the rule on a fixture page and sends
  POSTs to `evil.agent.test` in seven ways: from a sandboxed `srcdoc` frame, a plain `srcdoc`
  frame, a sandboxed form, a `data:` frame, a blob worker, `sendBeacon` and a `keepalive` fetch.
  It requires that each is failed and that `requests.evil` stays 0.
- Operations inside an out-of-process frame. Snapshot, resolve, read, select, click, fill, press and
  upload send their DOM, accessibility and runtime commands to the session of the target that runs
  the frame. A click adds the frame's content offset inside its page, read from the frame element,
  and dispatches the mouse events on the page. The rows use snapshot, resolve, read, select and
  click there; fill, press and upload in such a frame ran in no row.
- A click in such a frame can be lost. In hosted run 43087912803 on macos-15 with Chrome 157,
  `changed_page_1_isolated` missed 2 of 10 runs and `block_frame_post` 1 of 10 because the click
  had not reached the frame. So, before a click there, the library scrolls each frame that holds
  the ref into view from the page down and waits for two animation frames of the frame. A click
  listener on the ref sets a mark. When the mark reads that the click did not arrive, the library
  clicks again, at most three times. A mark that cannot be read because the document went away
  counts as arrived, so a click that arrived is never repeated. A probe copy that sent the first
  click 2,000 px off passed both rows with one payment each.
- New windows. When the executor's page opens a window, headless Chrome 153 stops serving the
  page that the new tab hides. Both the input acknowledgement and `Runtime.evaluate` waited 20 s
  and more. The same happened over a raw DevTools connection with no chromiumoxide, and
  `Page.bringToFront` or `Target.activateTarget` on the opener ended it at once. So when a page
  target whose opener is the executor's page attaches, the library activates its own page again.
- At start the library closes every page target but its own. Chrome's startup new tab holds a
  `chrome-untrusted://` frame that never answers `Target.setAutoAttach`.
- Certificate errors. chromiumoxide 0.9.1 ignores them unless told otherwise. The start now calls
  `respect_https_errors()`, and `connect` sets `ignore_https_errors` to false.

### What the rule cannot do: the WebSocket handshake

On Chrome 153 the DevTools protocol has no way to pause or fail a WebSocket handshake by its
address. `Fetch.enable`, at the page or at the browser level, does not see the handshake.
`Network.setBlockedURLs` does not stop it, with `urlPatterns` for all `ws` addresses, for one
host, or with the older `urls`. `Network.setRequestInterception` is gone. Only
`Network.emulateNetworkConditions` with `offline` stopped it, and that stops every request of the
target. Each probe ran over a raw connection with no chromiumoxide, and each time the fixture
counted the handshake in `requests.evil`. So `block_websocket` misses in every run, and the
library has no WebSocket part. The ways that remain change what U2.1 builds, so they are for the
planner. A script that every document and worker gets before it runs could wrap `WebSocket`, but a
page can get around it. A `Content-Security-Policy` with `connect-src` could be added to every
document response at the response stage. A proxy for each browser context could be set with
`Target.createBrowserContext` and `proxyServer`, with the rule in the proxy.

### What the lines cannot show

Ten runs cannot show a miss rarer than about one in four. The rows show the rule on the requests
that they make, and the ignored test on seven more ways out; neither shows it on a service worker,
a shared worker, a prefetch or a prerender. The rows read the fixture's counters and the
executor's list, so a request to a host that the fixture does not serve would show only in that
list.

## The driver row and the relay (U0.7, rulings CP-22 and CP-24)

`sh tools/r3-trials/web/run.sh --candidate cdp --browser PATH --port N --runs 20 --rows driver
[--step NAME]` asks what becomes of a request that is paused when its driver dies. The trial
program starts a driver: itself, in a child mode that no other row uses. The driver starts the
browser, opens `/signin` of the fixture, arms a rule that leaves every state-changing request
paused with no answer (`hold_requests`, an operation for this row only), and sends one
`fetch` POST to `/held` on `record-listener.mjs`. When the driver reports the request paused, the
trial kills the driver with the system's hard kill: `Child::kill`, which is SIGKILL on macOS and
TerminateProcess on Windows. For 10 s after the kill, the listener records every connection and
every byte that reach it. The trial watches the browser's process by its number and creation time.
A run is ok when no byte arrived. Before the 20 kills, one control run lets the POST through first;
the listener must see it, or the cell is `notrun ... cause=control`. Then the trial closes a
browser that is still alive over its debugging address, or, where it has none, ends its root
process by its recorded creation time. No row walks the tree of such a browser, so no row stops a
browser on Windows through the snapshot of parents that can take a reused process number. Each
run removes the profile and the temporary directory that its driver made.

The four forms, in the order of CP-24:

- `driver_killed_pipe`: the browser speaks DevTools over `--remote-debugging-pipe`. chromiumoxide
  0.9.1 has no pipe, so the driver is the trial's own small client (`src/pipe.rs`). On macOS the
  browser reads fd 3 and writes fd 4. On Windows it gets two inheritable pipe handles with
  `--remote-debugging-io-pipes=<read>,<write>`.
- `driver_killed_port`: the library of the verdict over its loopback port.
- `driver_killed_pipe_relay` and `driver_killed_port_relay`: the same, with the relay in the
  driver's own process.

The relay (`src/relay.rs`) is a SOCKS5 listener on a loopback port of the driver's process. A
SOCKS5 tunnel carries one connection to one host, for http, https and WebSocket alike. So the
relay reads only the SOCKS handshake, never a byte of a request, and an http request needs no
proxy that reads it. The relay resolves names itself. A name under `.test`, `localhost` and a
loopback address go to 127.0.0.1, and any other name is refused. The browser gets these flags,
after the flags of the trial:

| flag | what it closes |
|---|---|
| `--proxy-server=socks5://127.0.0.1:<port>` | every connection of the browser goes to the relay; with SOCKS5 the browser sends the name and resolves nothing itself |
| `--proxy-bypass-list=<-loopback>` | the implicit rule that lets loopback addresses skip the proxy; without it the browser would reach 127.0.0.1 straight |
| `--disable-quic` | QUIC, which runs over UDP outside a SOCKS5 tunnel |
| `--force-webrtc-ip-handling-policy=disable_non_proxied_udp` | WebRTC's own UDP for calls, which does not go through the proxy |
| `--host-resolver-rules=MAP *.test 127.0.0.1,EXCLUDE 127.0.0.1,MAP * ~NOTFOUND` | the browser's own resolver still answers no name but `.test`; `EXCLUDE 127.0.0.1` lets the browser reach its proxy, which the trial's rule `MAP * ~NOTFOUND` sent nowhere (the first relay runs failed with `ERR_PROXY_CONNECTION_FAILED`). The browser takes the last copy of a switch, and this copy comes after the trial's |

When the driver's process ends, the system closes the relay's listening socket and its
tunnels. A request that the browser lets go after that finds no proxy.

Two checks run beside the row. `run.sh --relay` starts the relay in the trial program's own process
and runs the nine steps and the six rows through it. `run.sh --routes` reads, after each run's step,
the system's table of established TCP connections to the fixture's port (`lsof` on macOS,
`netstat -ano` on Windows). It prints one `route` line per cell: how many of those connections
the trial program holds, which is the relay, and how many any other process holds. A run with
`--routes` and no relay is the control of that check: there every connection belongs to a browser
process.
