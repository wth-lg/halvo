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
process by its recorded creation time, and then ends what the run left with the routine of every
other way out (`checks::end_runs_own`, below; the killed driver runs no end of its own). The run
line prints `left_before` and `left_after`, and a run with `left_after` above 0 is a miss (in
`relay_port_free` `left_before` above 0 is one too, CP-145 (1)). No row walks the tree of such a browser, so no row stops a
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

## The second round (U0.7b, rulings CP-64 to CP-79)

`run.sh ... --round 2 --relay` runs a row of the second round. Every browser then starts with the
relay as its only route, with the start flags of `ROUND2_FLAGS` in `src/lib.rs`, and with the
agent's settings written into its new profile before the start (`AGENT_LOCAL_STATE` and
`AGENT_PREFERENCES`). After the start the executor makes the browser refuse every download.

The relay holds a list by state (CP-64). Before each run the list holds the fixture's utility and
pay hosts on its port, and a row adds what it needs. The relay refuses a host outside the list at
the SOCKS5 request, before any byte and before its own lookup, and records each refusal by host
and port. When a host leaves the list, the relay closes that host's open tunnels. A refused tunnel
ends no run (CP-65).

| start flag or setting | what it closes |
|---|---|
| `--disable-updater-scheduler` | the browser's waking of its updater, which looks names up itself, outside the relay: on hosted-mac at 9d89486 GoogleUpdater and EdgeUpdater did so in the rows that ran past about 30 s (`--disable-background-networking` of the first flags does not stop it) |
| `--dns-prefetch-disable`, `net.network_prediction_options=2`, `dns_prefetching.enabled=false` | lookups and preconnects for links |
| `--no-pings`, `--disable-features=FetchLaterAPI` | a ping and a request sent when the page is gone |
| `--disable-features=SignedHTTPExchange,SignedExchangeSubresourcePrefetch` and `web_package.signed_exchange.enabled=false` | signed exchanges; Chrome 153 still offers them in its Accept header without the setting |
| `--disable-features=Prerender2,PrefetchProxy` | prerendered documents and prefetch through a proxy |
| `--disable-features=NetworkErrorLogging,Reporting,DocumentReporting` | the reports that a site asks the browser to upload |
| `--disable-features=MediaRouter,DialMediaRouteProvider,CastMediaRouteProvider` | discovery on the local network |
| `--disable-features=FedCm,PaymentHandler,WebAuthenticationCable,AttributionReporting,PrivacySandboxAdsAPIs` | federated sign-in, payment handlers, a passkey on another device, measurement reports |
| `--disable-features=WebRtcHideLocalIpsWithMdns` and `webrtc.ip_handling_policy=disable_non_proxied_udp`, `webrtc.multiple_routes_enabled=false`, `webrtc.nonproxied_udp_enabled=false` | WebRTC's STUN and mDNS packets; the start flag `--force-webrtc-ip-handling-policy` alone left 39 STUN and 55 mDNS denials in one run |
| `dns_over_https.mode=off` in Local State | secure lookups of the browser's own |
| content settings 2 for notifications, devices, the clipboard, the file system and the rest; password manager, autofill and payment methods off | every permission that a page can ask for, and saved credentials |
| the executor fails a request with the header `Service-Worker: script` | a service worker, and with it push messages and background sync |

The executor adds the policy `connect-src http: https: data: blob:` as a header line of its own
(CP-67). It pauses the responses of documents, scripts and other resources at the response stage.
It answers a document by fulfilment with the document's own status, headers and body and the line,
and lets a script go on with the line added. Without the relay, on plain http on a loopback address,
the browser's Local Network Access checks fail a fulfilled page's fetch
(`net::ERR_BLOCKED_BY_LOCAL_NETWORK_ACCESS_CHECKS`). Through the relay they do not.

Rows and their listeners:

- `--rows relay`: `list_refuse`, `list_hops`, `list_close` (`chain-listener.mjs` serves the hops)
  and `own_traffic`.
- `--rows reach` (CP-78): a page on 127.0.0.1, a secure context, makes one kind of context read
  `evil.agent.test` on the listener's port. The relay's list holds that host, and the request rule
  does not, so only the rule stands between the read and the listener. `ways-listener.mjs` counts
  each request to that host with its bytes, and each context's report that it ran.
- `--rows ways`: `ws_allowed` with its control (`ws-listener.mjs`) and `frame_doc`.
- `--rows egress`: the rows of the ways out, served by `ways-listener.mjs`. In these rows the
  executor starts the trial program in the browser's place. That program runs
  `sh scripts/egress/check.sh LIST -- BROWSER ARGS` in the repository, with a list that holds the
  relay's port alone (CP-71). The check's standard error goes to a file, which the program
  follows. A pipe would end the check with SIGPIPE when the executor's stop ends the program first.
  After the stop, the trial ends any process that is still running with the run's profile path in
  its command line, except the check's own shell and runner. It then reads the check's
  `denied.log`.
  On macOS the browser gets `--no-sandbox`: inside the check's sandbox Chrome 153 cannot start its
  own ("sandbox initialization failed: Operation not permitted", then "GPU process isn't usable").
  Its network service makes the same connections either way. The browser also writes its network
  log. A UDP denial that matches a UDP connect that the check refused (net error -10) is read as no
  packet: the connect of a UDP socket sends none. Chrome makes such a connect to
  `[2001:4860:4860::8888]:443`, and Edge to `[2603:1020:201:10::10f]:443`, to learn whether IPv6
  reaches the internet. `handler_launch` does not run on this Mac, because a handler could open a
  window here. Its page adds its frame and its two navigations after the load; a frame of another
  program's scheme in the page itself keeps the load from ending. `updater_wake` leaves an idle
  page open for 75 s, past the moment when the browser would wake its updater.

## The third round (U0.7c, rulings CP-89, CP-93 and CP-95)

`run.sh ... --round 3 --relay` runs a row of the third round. It starts the browser as the product
does (CP-89 (b)): the trial program's own process starts it as a direct child on its debugging
pipe and is its one client; nothing connects to a browser at an address, and the library's
operation that did so is struck with the attach cell. `--channel port` runs the same row over a
debugging port on the loopback address, a control that holds no verdict open.

### The start (A11.1, CP-93 (1))

`src/start.rs` holds every start of a child of the trial's process. `StartLock` is the one lock
that each start takes, and `clippy.toml` refuses `Command::spawn`, `output` and `status`
elsewhere in the crate, so a start that skips the lock does not pass clippy where clippy runs on
this crate. No hosted job runs it there today (the crate has its own workspace, outside the root's
members); the test `a_start_waits_while_the_start_lock_is_held` pins that `Start::start` takes the
lock. tokio's process feature is on in the build, because chromiumoxide 0.9.1 turns it on, so
`clippy.toml` also refuses `tokio::process::Command` and chromiumoxide's own starts of a process
(`Browser::launch`, `BrowserConfig::launch`, its `async_process::Command`): no code of the crate
uses them, and `hard_kill` in start.rs rests on that no runtime holds a child of the trial.

- macOS: the two pipes are made, and each of their four ends is copied to a number of 10 or more
  with close-on-exec, under the lock. The browser is started by `posix_spawn` with
  `POSIX_SPAWN_CLOEXEC_DEFAULT`; the file actions open the null device at 0 and 1, the start's
  stderr file at 2, and place the browser's read end at 3 and its write end at 4. The browser then
  holds those five descriptors and nothing else of the trial's process, and the executor keeps its
  two ends with close-on-exec. A placed descriptor is copied above the placed numbers first,
  because `posix_spawn_file_actions_adddup2(3, 3)` keeps a close-on-exec flag.
- Windows: the two pipes are made not inheritable. Under the lock the browser's two ends and its
  standard handles are made inheritable, `CreateProcessW` starts the browser with
  `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` naming those handles alone and
  `--remote-debugging-io-pipes=<read>,<write>`, and the ends are made not inheritable again
  before the lock is released.
- The wire (`src/pipe.rs`, `src/cdp/wire.rs`) writes each command as its JSON and a NUL byte and
  splits the browser's output at each NUL byte, with chromiumoxide's protocol types for every
  message (`MethodCall`, `Message`, `CdpEventMessage` and the typed commands). The executor's
  operations run on the session of its own page target, which the browser-level auto-attach
  gives it, and on the session of each frame that runs apart. The executor enables the Network
  domain on each page and frame target before it runs, so that the two pauses of one request
  carry one network id, but not on a new window: Chrome 153 detaches a window that waits for the
  debugger in its first navigation when that domain is enabled on it.
- The browser's first reply on the pipe has a deadline, `FIRST_REPLY_WAIT` (30 s). When it passes,
  the executor ends the browser and the start fails with `NO_CHANNEL_MESSAGE`, or with
  `browser_start` when the first process has already ended. Every way out of a run ends what the run
  left (`checks::end_runs_own`, CP-126 E1): the stop after a cell's run (before the guard's wait,
  and again after the stop's own wait), a failed start, an executor dropped without its stop (a
  failed cell, and INT and TERM, which reach the program because run.sh ends by exec and drop the
  row that runs), a start that a signal cut off, and a kill row's run after its watch. A process is the run's own (`checks::Mark`)
  when it began at or after the run's start less 1 s, its program lies inside the browser's app
  bundle on macOS or its install directory on Windows (`StartOptions.program`, the browser itself
  under the egress wrap), and one of its arguments names the run's profile as a whole path
  (`names_profile`). Each pass reads the process table, records each such process by number and
  creation time and ends it by `procs::kill`, which reads the creation time again just before the
  kill and leaves a number that another process took alone; the routine stops on a pass that finds
  none (only a process that names the profile can make another one that names it), or after 100
  passes. Each run line prints `left_before` (the first pass) and `left_after` (the last), and a run
  with `left_after` above 0 is no ok run. When its first pass finds any, the routine prints
  `left_found n=<count> programs=<file names of their programs>` on standard error. The egress check's shell, its runner and its `log stream`
  run other programs and are left alone, as is a browser that names another profile. On `mac` one
  failed run of `start_probe` at 3268a2d3 left a browser process with the profile, the pipe flag and
  no parent for four hours, which the end of the first process did not reach (037b242c).
- The start names the browser's first process to the relay, starts the guard of the relay's port
  and asks for `http://halvo-start-probe.invalid/`, an origin that no run binds; it goes on only
  when the relay recorded that refusal within `PROBE_WAIT` (10 s), else it ends the browser with
  `NO_ROUTE_MESSAGE`. `.invalid` resolves nowhere, so a browser whose route bypasses the relay
  sends nothing for it.

### The relay's peer check (CP-89 (e), CP-93 (6))

The relay checks each connection after its accept and before it reads a byte (`check_peer` in
`src/relay.rs`), under the start lock, while it holds the socket: the owners of the connection's
other end, read from the system's table, must be exactly one process, and that process must be a
process of the browser's tree by pid and creation time. On macOS the relay reads the descriptors of
the tree's processes (`proc_pidinfo` and `proc_pidfdinfo`) and, when they name one owner, those of
every other process that the user may read; on Windows it reads `GetExtendedTcpTable` with owner
ids. It reads the owner again after the tree and refuses when the two reads differ. A refused peer
is closed unread and recorded apart (`foreign`, at most `MAX_RECORD` entries, with a count of what
the record did not keep); at most `MAX_OPEN` connections are open at once. On Windows the listener
is bound with `SO_EXCLUSIVEADDRUSE`; on macOS it is bound under the start lock, because std makes a
socket close-on-exec only after it exists.

The three races of A11.1, and what is left of each:

1. The peer closes before the table is read: the relay holds the accepted socket, a closed peer has
   no owner, and the connection is refused and recorded apart (`no_owner` or `owner_changed`). The
   runs show such records for the browser's own connections that closed early; nothing was read.
2. The peer's process ends and its number is taken before the tree is checked: the tree is matched
   by pid and creation time, and the owner is read twice, before and after the tree. Left: a new
   process that takes the number between the two reads and holds the same connection, which only
   inheritance from the old one gives.
3. A socket inherited by a child or passed to another process: on macOS the walk finds every holder
   of the connection among the processes that the user may read, and two holders are refused
   (`several_owners`); the walk holds the start lock, so no child of the trial's process is between
   its fork and its exec. Left: on Windows the table names one owner whatever other process holds a
   copy, and on macOS a process of another user is not read.

### The guard of the relay's port

A11.1 says that the browser's processes are ended before the relay's listener closes because the
pipe ends the browser with the executor's process. Under the system's hard kill that does not hold:
the system closes the listener with the killed process at once, and the browser's tree ends
seconds later. The executor therefore starts a guard for each browser: the trial program with
`--relay-guard <pid> <creation time>`, started with `posix_spawn` and close-on-exec by default
(a handle list on Windows) with the relay's listener as its one descriptor beside its standard
streams. It never accepts; it reads the process table every 20 ms and exits when no process of the
browser's tree runs. The tree is what its reads found: the first process, the processes that
`checks::tree_of` adds while the first process runs (the adopted ones that name the run's profile
among them), and each process whose parent was a member that still ran at the read that found it,
also after the first process has ended. Two limits are U2.1's to close (CP-161). A process that a
member starts after the first process has ended and that loses its parent before the next read,
20 ms later, is not followed, so the guard can free the port while it runs. When the guard cannot
be started, the browser's start goes on without one and no line says so (`Relay::start_guard`
gives nothing, and the start in `cdp.rs` goes on). A table that could not be read is not an empty one: the guard
keeps the port and reads again every 2 ms, and when the table stays unread for 300 s it ends with
exit 3 and the line `guard_end cause=table_unread` (`relay::guard_loop`). The executor's stop waits
for it.

The browser does not always end by itself when its pipe closes. Under load on the development Mac
it reads the pipe closed in the second of a hard kill of its driver (`Connection terminated while
reading from pipe`), begins its shutdown, its helpers end, and its own process then waits idle and
does not end: in one call 2 of 10 still ran at 300 s and one ended at 234 s (CP-145 (1), the cause
read in CP-148). So the executor gives the guard its own number and start time beside the
browser's (CP-148). While the executor runs the guard ends nothing. When the executor no longer runs
and a process of the browser's tree still does, the guard waits `GRACE` (3 s) so that a browser that
ends by itself may do so, then asks each process of the run that still runs to end (SIGTERM on
macOS, by number and start time in the table of that moment, the creation time read again; on
Windows there is no asking), ends by force each one that still runs `TERM_WAIT` (2 s) later, reads the
table until none runs, keeps the port `PORT_MARGIN` (1 s, longer than one pass of the row's watch on
the loaded Mac, whose longest pass was 476 ms) and only then exits. It ends no process whose number
another process took. When it ended processes it writes `guard_ended` in the run's temporary
directory (`ended_by=guard ms=... asked=... forced=...`). A hard kill of the guard as well leaves the
browser to itself; that is a named limit. On Windows the trial starts the browser with a handle list
and no job object, so there too the browser ends by itself when its pipe handles close, and the
guard's rule ends only what still runs after its grace. A failed accept does not end
the relay's accept loop (`ACCEPT_PAUSE`), so the listener stays open while the relay lives.

One test (`the_guard_ends_a_real_stand_in_after_its_executor_is_killed`, macOS) drives the guard
with the real process table and the real ask, and its end is fenced to the test's own stand-in
browser by number and start time: any other call sends nothing and fails the test, and the test ends
both of its stand-ins on every way out. A changed copy of the guard's loop, its member rule, the ask,
the forced end or the fence runs by the exact names of the tests on stand-in tables alone, never
with that test or with the whole suite (CP-161).

### The checks of a session (CP-89 (c))

`src/checks.rs` reads six rules, each with a count that is 0 when the rule holds: `port_flag` (a
process of the browser's tree with a debugging-port flag on its command line), `tcp_listen` (a TCP
socket of the tree that accepts connections), `udp_wide` (a UDP socket of the tree bound to an
address other than loopback), `active_port_file` (`DevToolsActivePort` in the profile),
`executor_listener` (a listener of the trial's process other than the relay's) and `pipe_holder`
(a process other than the browser's first one and the trial's that holds an end of the pipe: on
macOS every process whose descriptors the user may read, so that an end passed over a socket is
found too; on Windows the processes started at or after the browser, found by comparing each of
their handles with the executor's copies of the four ends by `CompareObjectHandles`, which makes
no request of the object: a request such as a name query waits on a pipe opened for synchronous
I/O while a read of it is pending). Every run of `--round 3` reads them after the start, after
the row and after the stop, and each cell prints one `check` line with the sum of each rule over its
runs and the number of runs in which all were 0. For the read after the stop the executor keeps
copies of its two ends open: on macOS a freed pipe's handle can name a new pipe of another process.
The check reads the process table under the start lock, so that no child of the trial's own
process is between its fork and its exec during the read. A read whose table could not be read is
open (`table_unread=1`), never a read that found nothing. Each process that a read counts in
`port_flag` or `pipe_holder` gets a class by its own state (CP-145 (2)). On macOS it counts at once
when its arguments were read (with the flag, or while it held an end), when it held one of the
executor's two ends, ran a program outside the browser's app bundle, or had a parent, by number and
start time, other than the browser's first process or a process of its tree inside the bundle;
otherwise the check follows it every 5 ms for at most 30 s until its arguments can be read. It is
`passing` when it then runs inside the bundle with no flag and holds no end (a failed read of its
descriptors counts as holding), `open` with `holder_gone` when it ended first, and counts otherwise,
also when its arguments are still unread at 30 s. On Windows every counted process counts. Each read
and each cell's `check` line carry `port_flag_passing`, `port_flag_open`, `pipe_holder_passing`,
`pipe_holder_open` and `table_unread` beside the six rules; a run's read holds when each of the two
rules counts nothing less its passing and open processes, none is open, the table was read and each
other rule is 0. Each counted process gets a `check_named` line on standard output: the step, the
run, the read, the rule, its number and start time, its parent's with `parent=first`, `tree`,
`executor`, `other` or `unread`, the program's file name with `bundle=in`, `out` or `unread`,
`args=read` or `unread`, the flag, the ends held, the milliseconds of the follow, what decided and
the class. The counts themselves do not change.

### Rows of the third round

- `pipe_inherit`: a thread starts copies of the trial program in a holding mode, each under the
  start lock, from just before the browser's start until it returns (up to 40); ok when none of them
  holds an end of the pipe.
- `relay_foreign`: after the sign-in, a copy of the trial program, a process outside the browser's
  tree, connects to the relay and sends a SOCKS5 greeting, a CONNECT to an allowed host and a
  request; ok when the relay sent it no byte, recorded it apart, recorded no refusal of that host,
  `record-listener.mjs` counted no connection, and the agent's own route still works.
- `start_probe`: ok when the relay recorded exactly one refusal of the probe's origin during the
  start, the fixture had counted no request at that moment, and the agent's first open then reaches
  the fixture.
- `relay_port_free` (with `--rows driver`): the driver, with the relay in its process and the
  browser on its pipe, is killed with the system's hard kill while a POST is paused. The row asserts
  A13.7's sentence: the browser is gone before the relay's port is free, and nothing of the run is
  left; it promises no seconds (CP-145 (1)). From the kill the watch (`r3_web::killwatch`) ends
  nothing: every 2 ms it reads first whether a process of the run still runs (one of the tree read
  before the kill, or any process that the run's mark owns, so a member born later counts) and then
  whether a new socket can bind the relay's port (with `SO_REUSEADDR`); when the port is free and a
  process ran, it reads the processes once more. At 30 s it prints a `watch_at` line with what still
  runs (the count, the programs' file names, whether the first process is among them) and watches
  on; at its cap of 300 s it prints the same and only then does the row end the run's processes.
  Once the order is shown it reads the port on for `free_ms` until it is free or 30 s have passed.
  A kill is `ok` when a pass read no process running and then the port still taken, and the row's
  own end after the watch found and left nothing (`left_before=0`, `left_after=0`); a `miss` with
  `free_first` (the port free while a process ran), `tree_left` (a process still runs at the cap)
  or `left_behind` (the tree read gone, and the row's end found or left a process); `open` with
  `one_pass` when both events were first seen in one pass, or `table_unread` when the table could
  not be read at the cap. A browser that the relay's guard ended after its executor was gone (see
  The guard of the relay's port) is no process that the row had to end: such a kill reads ok when
  the order held and nothing is left. The run line gives `result`, `cause`, `gone_ms`, `free_ms`,
  `max_gap_ms` and `ended_by` (`guard ended_ms=<ms>`, `self`, or `row` for the row's end at the
  cap). A call with a miss prints its cell line; a call with an open kill and no miss prints
  `notrun ... cause=watch_open` in place of it.
- `org_debugging` and `org_proxy` (`--rows org`): the caller sets the organisation's setting before
  the call and takes it away after it: the scratch workflow writes `RemoteDebuggingAllowed` off or
  `ProxyMode` `direct` under `HKLM\SOFTWARE\Policies` on Windows, and on the hosted Mac installs the
  bundle id's property list in `/Library/Managed Preferences` (owned by root, mode 644) and ends
  cfprefsd so that it reads the file again (`sudo defaults write` to that path exits 0, prints
  nothing and leaves no file: run 43333071145). Before each hosted Mac cell the workflow reads the
  policy back: a headless start with no debugging channel (the policy that closes the pipe would
  stop any other read) prints `chrome://policy/`, else `edge://policy/`, to a PDF, and PDFKit reads
  its text. Whether the text names the policy is decided by `r3_web::policy` (CP-126 E3), which
  `web-trial --policy-named NAME FILE [VALUE_READ]` gives the jobs: on a policy page (a line that
  begins with "policy name"), the name as a whole word that begins a row, or the pieces of a broken
  name, each a whole word, in the page's order and spelling the name and nothing more, or (CP-145
  (6)) the name's letters in order from the start of a line with only white space or line breaks
  between them, ended by the value that the job wrote and read back as the page prints it (`false`
  for the 0 of `RemoteDebuggingAllowed`, `direct` for `ProxyMode`), standing as the end of its word:
  pypdf gives Edge's Windows table as `Remot eDebuggingAllow e` and `dfalse Platform ...`. A longer
  name, the name inside another policy's value, another value and the text squashed to letters
  alone name nothing. One limit stays: a wrapped line of another policy's value that begins with the
  name and a value reads as named by the first form. No cell rests on it, because each job writes
  its one key. Run
  43347520562's reader tested for the name as a part of the text; the fixed rule gives the same
  answers on that run's saved pages. A hosted Mac cell runs only when the page names the policy
  and shows one more `Platform` source than a read of the page before the write, else the workflow
  prints `notrun cause=policy_not_seen`. The words of the name's line and the five lines after it
  are printed as words of a closed list (false, direct, Platform, Machine, Mandatory, OK, other).
  Chrome's printed page has the columns Policy name and Source alone, so the value comes from the
  file (`defaults read`, printed with the file's owner and mode). Ok when the start fails with the
  plain message of its rule (`NO_CHANNEL_MESSAGE` after the first reply's deadline, or
  `NO_ROUTE_MESSAGE` when the probe found no refusal) and the fixture counted no request. On this
  Mac the cell is a `notrun` line with `cause=no_policy_place`: Chrome on macOS takes a policy from
  the system's managed preferences, which this unit does not write here. On hosted-mac the four
  cells come from scratch run 43369346664 (CP-126, the fixed reader), all `ok=10 of=10` with each
  page naming its setting; run 43347520562 gave the same four. On hosted-win each of the four cells
  is `ok=10 of=10` with the control, the value and the page: Chrome's `org_proxy` from run
  43368925419, Chrome's `org_debugging` from run 43384012622, and Edge's `org_debugging` and
  `org_proxy` from run 43389541875. Edge's `org_debugging` did not hold before that run. Run
  43368925419 gave Edge `chrome://policy/` first, which printed Edge's new-tab page, so the page did
  not name the setting (`page_named=no`, and the cell `ok=0 of=10`). Run 43384012622 printed
  `edge://policy/`, which lists the setting, but pypdf gives its name as `Remot eDebuggingAllow e`
  with the last letter joined to the value on the next line (`dfalse ...`), which neither of the
  first two forms reads, so the cell was `notrun cause=policy_not_seen`. The reader's third form
  (CP-145 (6)) names that page, and run 43389541875 ran Edge's two `org_` cells alone: both
  controls replied, the key read back 0 and `direct`, and both pages named their settings. The runs
  before 43347520562 lost cells to the
  harness: in 43339621990 the workflow's own 300 s limit stopped Chrome's `org_debugging` at 9 of 9
  ok runs (ten runs of the 30 s first-reply deadline take about 303 s), the stopped call's fixture
  kept port 18190 for both `org_proxy` calls, and the runner's PDFKit put the pieces of Edge's
  policy name on lines that do not follow each other. The ninth run gives each call 420 s in a
  process group of its own, ended as a whole by its recorded id at the limit with a `call_stopped`
  line of the runs ok before the stop; gives each cell a port of its own and ends a holder of it
  only when the harness recorded that process, else moves the cell and says so; and reads the name
  also in the text squashed to letters and digits and in word pieces. The one process that the
  harness ends without a recorded id is cfprefsd (`sudo killall cfprefsd`): ending it is how the
  preferences daemon reads the new file.
- On hosted-win an `org_` cell (CP-126 E7) runs after a start without the setting that replied
  (`web-trial --org-control BROWSER DIR`, `org_control at=before`), the value that the job reads
  back from the key after its write, and the browser's own policy page printed headless with no
  debugging channel and read as text with pypdf, which the job installs for the read (BSD-3-Clause);
  the call gets the three as `R3_ORG_CONTROL_BEFORE`, `R3_ORG_VALUE_READ` with
  `R3_ORG_VALUE_WANT`, and `R3_ORG_PAGE_NAMED`, and prints them on the cell line. A run is ok only
  with a reply and the value as the cell needs it; a control that does not reply or a value that
  does not read back makes it a miss. Where the page was read and does not name the setting, the
  cell is `notrun cause=policy_not_seen reason=page_not_named` (CP-137): no policy is seen in force,
  so its runs count neither way. Where the page could not be read, the run rests on the reply and
  the value, and the cell line says `page_named=unread page_cause=<cause>` from
  `R3_ORG_PAGE_CAUSE` (`empty_text` when the PDF gives no text). After the removal a second control
  start runs (`org_control at=after`), which the collector puts beside the cell.
- Flags of the third round that the product does not carry (A13.3, CP-89 (g)): `--use-mock-keychain`
  (the product's browser keeps its cookies with the system's key store, A8.3), the six rows'
  `--isolate-origins=` (it names the payment site, CP-93 (5)), the port control's
  `--remote-debugging-port=0`, and the resolver rule's map of `*.test` to loopback (the fixture's
  names).
- `same_site_pref`: the profile's Local State holds the flags entry `isolate-origins` with the pay
  origin (`browser.enabled_labs_experiments` and `browser.enabled_labs_experiments_origin_lists`),
  and no start flag names the site; ok when the pay frame runs as a target of its own and no
  command line of the browser's tree holds `agent.test`.
- `same_site_rule`: no setting and no flag isolates the frame; the rule is armed on the page's
  target alone (`arm_rule_on_page_only`, a control of the trial), and the trial's script makes the
  frame send a `fetch` POST to its own origin; ok when that request is paused and failed and the
  fixture's `requests.state_changing` and `utility.payments` did not change.
