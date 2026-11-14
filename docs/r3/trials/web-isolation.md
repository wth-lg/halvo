# Web isolation trial

Unit U0.7 of the agent-setup run tests the request rule of the web executor where the one-site
fixture cannot (contract A13.7): a payment frame in a process of its own, a post from a new window,
a post from that frame, a WebSocket handshake to a host that is not allowed, and a page behind a
certificate that the system does not trust. Rulings CP-22 and CP-24 add the row `driver_killed`:
the driver dies with a request paused. `tools/r3-trials/web/README.md` says how each row runs, what
the library changed and why, and names the relay's flags and what each closes.

What ran, at the unit's head 4f494e9 with the fixture as it is. The six rows ran with
`run.sh ... --runs 10 --rows isolation`. Each of their browsers starts with the flags of
`docs/r3/trials/web-executor.md` and one more, which run.sh adds and no `flag` line holds:
`--isolate-origins=http://pay.agent.test:<port>`, where `<port>` is the fixture's port. The four
forms of `driver_killed` ran with `run.sh ... --runs 20 --rows driver`, each after one control run
that let the request through. On `mac` each cell was one call of the timed-run helper, from
2026-11-12T01:42:48Z to 2026-11-12T02:16:36Z, under a 1-minute load average between 88.34 and 503.72.
The hosted cells ran in run 43103175522, on a scratch branch that holds the head plus one commit on the
workflow file. The `--browser` paths are those of `docs/r3/trials/web-executor.md`. Edge on the
macOS image updated itself from 156 to 157 during the six rows (corrected in U0.7b, ruling CP-73).
In the cells of `hosted-mac` with Edge, the nine steps and the first four of the six rows ran on
Edg/156.0.4314.58. `block_websocket` ran on 156 in its first 2 runs and on Edg/157.0.4341.46 in the
other 8; its line says `major=156`, the major of its first run. `cert_error` and the driver row ran
on 157.

Choices that the contract leaves. A frame runs "as a target of its own, out of the page's process"
when `Target.getTargets` lists a target of type `iframe` on the pay origin, which Chromium makes
for an out-of-process frame only. `block_popup` requires that the paused POST came from a frame
other than the executor's main frame, and `block_frame_post` that it came from the pay frame's
target. `cert_error` is ok when the navigation's error starts `net::ERR_CERT_` and the listener
counted no HTTP request. A `driver_killed` run is ok when no byte reached the request's address
within 10 s of the kill; `median_ms` there is the time from the kill to the end of the browser's
process, over the runs in which it ended. On a miss, `bytes=` gives the bytes that arrived.

What the lines show and cannot show. `block_websocket` misses in every cell. Chrome's DevTools
protocol has no way to pause or fail a WebSocket handshake by its address, so the library has no
WebSocket part, and the handshake reaches the fixture's `evil` host. By the rule fixed before the
runs, that miss gives `product=no`. `driver_killed_port` misses in every cell: the browser outlives
its killed driver and lets the paused request go (9160 bytes in 20 of 20 kills on `mac` with Chrome, 9160 bytes in 20 of 20 kills on `hosted-mac` with Chrome, 9440 bytes in 20 of 20 kills on `hosted-mac` with Edge, 9040 bytes in 20 of 20 kills on `hosted-win` with Chrome, 9320 bytes in 20 of 20 kills on `hosted-win` with Edge). `driver_killed_pipe` and both
forms with the relay let no byte through in any cell, and every control arrived. Through the
relay the other rows gave their results unchanged, and every connection that reached the fixture
came from the driver's process (75 route lines with the relay, 320 connections, 0 from another process; 45 route lines of the control with no relay, 100 connections, 100 from another process). A request that arrives after a row's last read is not
seen. Ten runs cannot show a miss rarer than about one in four, and twenty about one in seven.

```text
hosted machine=hosted-mac run=43103175522 sha=f2e53eeb7893784891a5c9038a0f63dcaa4ec124
hosted machine=hosted-win run=43103175522 sha=f2e53eeb7893784891a5c9038a0f63dcaa4ec124
cell candidate=cdp machine=mac browser=chrome major=153 step=payment_frame_isolated ok=10 of=10 median_ms=304
cell candidate=cdp machine=mac browser=chrome major=153 step=changed_page_1_isolated ok=10 of=10 median_ms=277
cell candidate=cdp machine=mac browser=chrome major=153 step=block_popup ok=10 of=10 median_ms=95
cell candidate=cdp machine=mac browser=chrome major=153 step=block_frame_post ok=10 of=10 median_ms=106
cell candidate=cdp machine=mac browser=chrome major=153 step=block_websocket ok=0 of=10 median_ms=0
cell candidate=cdp machine=mac browser=chrome major=153 step=cert_error ok=10 of=10 median_ms=334
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=payment_frame_isolated ok=10 of=10 median_ms=1010
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=changed_page_1_isolated ok=10 of=10 median_ms=986
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=block_popup ok=10 of=10 median_ms=175
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=block_frame_post ok=10 of=10 median_ms=216
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=block_websocket ok=0 of=10 median_ms=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=cert_error ok=10 of=10 median_ms=483
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=payment_frame_isolated ok=10 of=10 median_ms=1089
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=changed_page_1_isolated ok=10 of=10 median_ms=1070
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=block_popup ok=10 of=10 median_ms=189
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=block_frame_post ok=10 of=10 median_ms=209
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=block_websocket ok=0 of=10 median_ms=0
cell candidate=cdp machine=hosted-mac browser=edge major=157 step=cert_error ok=10 of=10 median_ms=454
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=payment_frame_isolated ok=10 of=10 median_ms=1195
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=changed_page_1_isolated ok=10 of=10 median_ms=1439
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=block_popup ok=10 of=10 median_ms=90
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=block_frame_post ok=10 of=10 median_ms=95
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=block_websocket ok=0 of=10 median_ms=0
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=cert_error ok=10 of=10 median_ms=909
cell candidate=cdp machine=hosted-win browser=edge major=156 step=payment_frame_isolated ok=10 of=10 median_ms=1515
cell candidate=cdp machine=hosted-win browser=edge major=156 step=changed_page_1_isolated ok=10 of=10 median_ms=1523
cell candidate=cdp machine=hosted-win browser=edge major=156 step=block_popup ok=10 of=10 median_ms=101
cell candidate=cdp machine=hosted-win browser=edge major=156 step=block_frame_post ok=10 of=10 median_ms=100
cell candidate=cdp machine=hosted-win browser=edge major=156 step=block_websocket ok=0 of=10 median_ms=0
cell candidate=cdp machine=hosted-win browser=edge major=156 step=cert_error ok=10 of=10 median_ms=1203
cell candidate=cdp machine=mac browser=chrome major=153 step=driver_killed_pipe ok=20 of=20 median_ms=3481
cell candidate=cdp machine=mac browser=chrome major=153 step=driver_killed_port ok=0 of=20 median_ms=0 bytes=9160
cell candidate=cdp machine=mac browser=chrome major=153 step=driver_killed_pipe_relay ok=20 of=20 median_ms=688
cell candidate=cdp machine=mac browser=chrome major=153 step=driver_killed_port_relay ok=20 of=20 median_ms=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=driver_killed_pipe ok=20 of=20 median_ms=178
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=driver_killed_port ok=0 of=20 median_ms=0 bytes=9160
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=driver_killed_pipe_relay ok=20 of=20 median_ms=223
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=driver_killed_port_relay ok=20 of=20 median_ms=0
cell candidate=cdp machine=hosted-mac browser=edge major=157 step=driver_killed_pipe ok=20 of=20 median_ms=488
cell candidate=cdp machine=hosted-mac browser=edge major=157 step=driver_killed_port ok=0 of=20 median_ms=0 bytes=9440
cell candidate=cdp machine=hosted-mac browser=edge major=157 step=driver_killed_pipe_relay ok=20 of=20 median_ms=567
cell candidate=cdp machine=hosted-mac browser=edge major=157 step=driver_killed_port_relay ok=20 of=20 median_ms=0
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=driver_killed_pipe ok=20 of=20 median_ms=259
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=driver_killed_port ok=0 of=20 median_ms=0 bytes=9040
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=driver_killed_pipe_relay ok=20 of=20 median_ms=250
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=driver_killed_port_relay ok=20 of=20 median_ms=0
cell candidate=cdp machine=hosted-win browser=edge major=156 step=driver_killed_pipe ok=20 of=20 median_ms=370
cell candidate=cdp machine=hosted-win browser=edge major=156 step=driver_killed_port ok=0 of=20 median_ms=0 bytes=9320
cell candidate=cdp machine=hosted-win browser=edge major=156 step=driver_killed_pipe_relay ok=20 of=20 median_ms=367
cell candidate=cdp machine=hosted-win browser=edge major=156 step=driver_killed_port_relay ok=20 of=20 median_ms=0
verdict executor=cdp version=0.9.1 product=no
```

## Second round (U0.7b)

Unit U0.7b runs the rows of A13.7's second round, under rulings CP-64 to CP-79. The browser's only
route is the relay, which holds a list by state. Every browser starts with the agent's start flags
and profile settings, and the executor adds the policy against WebSockets.
`tools/r3-trials/web/README.md` names each flag and setting and what it closes, and says how each
row runs. The first round's lines and verdict above stay as they were.

What ran, at the unit's commit 3eea655. Every row ran with `run.sh ... --round 2`, and all but the
driver forms also with `--relay`. The deciding rows ran 10 times per cell, and each driver form 20
times after one control run. These rows are `--rows relay` (`list_refuse`, `list_hops` and
`list_close`), the nine rows, `--rows isolation`, `--rows reach` and `--rows driver`. The recorded
rows ran as A13.7 names them: `own_traffic` 3 times; `ws_allowed` 5 times with one control run;
`frame_doc` 10 times; and `--rows egress` 5 times on the hosted machines. On `mac` each row set was
one call of the run helper, from 2026-11-12T10:09:48Z to 2026-11-12T10:49:39Z, on a lane that records no time, so
`median_ms` there is not a timed measure. The hosted cells ran in run 43197875800, on a scratch branch
that holds the commit plus one commit on the workflow file. On `hosted-mac` Chrome 157.0.8103.39 ran, and Edge
156.0.4314.58 until the driver row and 157.0.4341.46 in it; on `hosted-win` Chrome 156.0.8078.80
and Edge 156.0.4314.58 ran. Each cell's runs reported one major version.

Readings that the contract leaves, fixed before the runs:

- `list_refuse` arms no request rule, so the relay alone decides. `block_websocket` is ok "by its
  row above": the relay refuses the handshake's host, the fixture counts nothing, and the run does
  not end.
- The reach rows (CP-78) read `evil.agent.test` on the listener's port. The relay's list holds that
  host and the request rule does not, so only the rule stands between the read and the host, and a
  row is ok when no byte reaches it. The pages are on `127.0.0.1`, a secure context, so that a
  service worker can register. The executor fails a request that carries `Service-Worker: script`,
  so in `reach_service` no worker registers and the row shows that no byte reaches the host.
- The product's form is the port with the relay. `driver_killed_port`, the port without the relay,
  is the form that the relay exists for; its miss is expected and decides nothing for the product.
  chromiumoxide 0.9.1 has no pipe, and the trial's pipe client has no operations of the executor.
  So `frame_doc` on the pipe and the nine rows through the relay of the pipe are written as not run
  (CP-79). A13.7 names 36 `relay_cell` lines (the nine steps, the two relay forms, `mac` and
  `hosted-win` with Chrome), and a line that is missing gives `relay=open`. The 18 lines of the pipe
  form are missing.
- The rows of the ways out read G, the `denied.log` of `scripts/egress/check.sh`, with the browser
  as its command and a list that holds the relay's port alone. On macOS the browser gets
  `--no-sandbox`, because Chrome cannot start its own sandbox inside the check's. A UDP denial that
  matches a UDP connect that the check refused in the browser's network log counts as no packet.
  The IPv6 test of Chrome connects to `[2001:4860:4860::8888]:443`, and Edge's to
  `[2603:1020:201:10::10f]:443`; a connect of a UDP socket sends nothing. Every other denial counts
  as a packet. On Windows the check sees UDP at its first packet. `handler_launch` does not run on
  `mac`, where a handler could open a window.
- A `way` line with `runs=0` names a way that no row runs: `closed=yes` where a start flag, a
  profile setting or the executor closes it, and `closed=no` where nothing does (`get_with_data`,
  a GET to an allowed host that carries a page's data in its address, A4.2).

What the lines show and cannot show. Every cell of the rows that decide reads `ok=10`, or `ok=20`
for a driver form, in all five cells. These are the relay's list rows; the six rows, where
`block_websocket` is now ok through the relay's refusal; the nine rows; the six reach rows; and
the driver forms that the product can take. `driver_killed_port` misses in every cell, as in the
first round, with 9040 to 9440 bytes in 20 of 20 kills. By the rule fixed before the runs that gives
`product=yes`. `relay=open`, because the 18 relay cells of the pipe form could not run. Every
`relay_cell` line that ran reads `ok=10`, and every kill of a relay form reads `arrived=0`. On
`hosted-mac` with Edge the pipe form's control once did not see its request, so that cell is not run
(the race that the first round's verifier named). A13.7 runs the driver row on `mac` and
`hosted-win` with Chrome, and by CP-79 a control that could not run holds no verdict open.

The verifier's changed copies, each run on `mac` with Chrome in one clone and restored by
`git checkout`, show that the reach rows can fail:

- M05, which lets every read through, fails five of the six reach rows. `reach_service` holds,
  because no worker runs.
- M02, which loses the arming of new targets, fails none, because the rule at the browser's level
  pauses every read.
- With the browser's level gone too (M01 ported, with M02), four rows fail.
- With the executor's refusal of a worker's script removed, the worker registers and runs, and
  the rule pauses both its reads, even with the browser's level gone.

The ways out read closed in every cell of `hosted-win`, where the check sees UDP at its first
packet. On `hosted-mac` four ways read otherwise:

- `peer_connection` has two UDP denials to port 53 in each run, which the browser's network log
  does not explain.
- `handler_launch` counted new processes outside the browser's tree that are the machine's own:
  the runner's `ps`, `bash` and `sleep`, and EdgeUpdater's server. None is a mail client or a
  handler of the page's scheme.
- In `reports` (6 of 10 runs) and `updater_wake` (5 of 10) the check ended with no line of its own
  and wrote no `done`, so those runs read `closed=open`. Where the check was read, it held no
  packet.

At 9d89486 GoogleUpdater and EdgeUpdater looked names up outside the relay in the rows that ran
past about 30 s. With `--disable-updater-scheduler`, at c22fb55 and at 3eea655, neither did. Ten
runs cannot show a miss rarer than about one in four, and twenty about one in seven.

Line forms of this block, besides the `cell` and `hosted` lines of A13.1 and A13.2 (CP-79):

- `cell ... major=<v>[ majors=<v>:<n>,...] step=<s> ok=<n> of=<n> median_ms=<n>[ bytes=<n>][ relay=<pipe_relay|port_relay>]`:
  `majors=` when the cell's runs reported more than one major (CP-73), `relay=` when the cell ran
  through the relay;
- `relay_cell machine=<m> browser=<b> form=port_relay step=<s> ok=<n> of=10`;
- `kill machine=<m> form=<pipe|port|pipe_relay|port_relay> kills=20 arrived=<n>`;
- `control candidate=cdp machine=<m> browser=<b> step=driver_killed_<form> let_through=yes browser_after=<gone|alive> connections=<n> bytes=<n> seen=<yes|no>`:
  the control run before a driver form's kills;
- `way row=<r> machine=<m> browser=<b> closed=<yes|no|open> runs=<n>`; for `frame_doc` with
  `form=port_relay` before `machine=` and `told=<n>` at the end; with `machine=all browser=all
  runs=0` for a way that no row runs;
- `control row=ws_allowed machine=<m> browser=<b> arrived_runs=<n> runs=<n>`: the runs in which
  every context's handshake arrived with no policy;
- `notrun way row=frame_doc form=pipe machine=<m> browser=<b> cause=<c>` and
  `notrun relay_cell machine=<m> browser=<b> form=pipe_relay cause=<c>`;
- `notrun candidate=cdp machine=<m> browser=<b> step=<s> cause=<c>`.

```text
hosted machine=hosted-mac run=43197875800 sha=26feea1f2db86dbd608dad118cf8d89123ada2d2
hosted machine=hosted-win run=43197875800 sha=26feea1f2db86dbd608dad118cf8d89123ada2d2
cell candidate=cdp machine=mac browser=chrome major=153 step=reach_page ok=10 of=10 median_ms=3225 relay=port_relay
cell candidate=cdp machine=mac browser=chrome major=153 step=reach_opened ok=10 of=10 median_ms=3254 relay=port_relay
cell candidate=cdp machine=mac browser=chrome major=153 step=reach_frame ok=10 of=10 median_ms=3250 relay=port_relay
cell candidate=cdp machine=mac browser=chrome major=153 step=reach_worker ok=10 of=10 median_ms=3231 relay=port_relay
cell candidate=cdp machine=mac browser=chrome major=153 step=reach_shared ok=10 of=10 median_ms=3250 relay=port_relay
cell candidate=cdp machine=mac browser=chrome major=153 step=reach_service ok=10 of=10 median_ms=3295 relay=port_relay
cell candidate=cdp machine=mac browser=chrome major=153 step=list_refuse ok=10 of=10 median_ms=2367 relay=port_relay
cell candidate=cdp machine=mac browser=chrome major=153 step=list_hops ok=10 of=10 median_ms=1330 relay=port_relay
cell candidate=cdp machine=mac browser=chrome major=153 step=list_close ok=10 of=10 median_ms=1471 relay=port_relay
cell candidate=cdp machine=mac browser=chrome major=153 step=signin_form ok=10 of=10 median_ms=119 relay=port_relay
relay_cell machine=mac browser=chrome form=port_relay step=signin_form ok=10 of=10
cell candidate=cdp machine=mac browser=chrome major=153 step=table_read ok=10 of=10 median_ms=121 relay=port_relay
relay_cell machine=mac browser=chrome form=port_relay step=table_read ok=10 of=10
cell candidate=cdp machine=mac browser=chrome major=153 step=form_fill ok=10 of=10 median_ms=227 relay=port_relay
relay_cell machine=mac browser=chrome form=port_relay step=form_fill ok=10 of=10
cell candidate=cdp machine=mac browser=chrome major=153 step=payment_frame ok=10 of=10 median_ms=255 relay=port_relay
relay_cell machine=mac browser=chrome form=port_relay step=payment_frame ok=10 of=10
cell candidate=cdp machine=mac browser=chrome major=153 step=file_upload ok=10 of=10 median_ms=225 relay=port_relay
relay_cell machine=mac browser=chrome form=port_relay step=file_upload ok=10 of=10
cell candidate=cdp machine=mac browser=chrome major=153 step=session_expiry ok=10 of=10 median_ms=230 relay=port_relay
relay_cell machine=mac browser=chrome form=port_relay step=session_expiry ok=10 of=10
cell candidate=cdp machine=mac browser=chrome major=153 step=changed_page_1 ok=10 of=10 median_ms=222 relay=port_relay
relay_cell machine=mac browser=chrome form=port_relay step=changed_page_1 ok=10 of=10
cell candidate=cdp machine=mac browser=chrome major=153 step=block_request ok=10 of=10 median_ms=215 relay=port_relay
relay_cell machine=mac browser=chrome form=port_relay step=block_request ok=10 of=10
cell candidate=cdp machine=mac browser=chrome major=153 step=credential_mark ok=10 of=10 median_ms=213 relay=port_relay
relay_cell machine=mac browser=chrome form=port_relay step=credential_mark ok=10 of=10
cell candidate=cdp machine=mac browser=chrome major=153 step=payment_frame_isolated ok=10 of=10 median_ms=304 relay=port_relay
cell candidate=cdp machine=mac browser=chrome major=153 step=changed_page_1_isolated ok=10 of=10 median_ms=272 relay=port_relay
cell candidate=cdp machine=mac browser=chrome major=153 step=block_popup ok=10 of=10 median_ms=81 relay=port_relay
cell candidate=cdp machine=mac browser=chrome major=153 step=block_frame_post ok=10 of=10 median_ms=102 relay=port_relay
cell candidate=cdp machine=mac browser=chrome major=153 step=block_websocket ok=10 of=10 median_ms=54 relay=port_relay
cell candidate=cdp machine=mac browser=chrome major=153 step=cert_error ok=10 of=10 median_ms=321 relay=port_relay
way row=ws_allowed machine=mac browser=chrome closed=yes runs=5
control row=ws_allowed machine=mac browser=chrome arrived_runs=1 runs=1
way row=frame_doc form=port_relay machine=mac browser=chrome closed=yes runs=10 told=10
way row=own_traffic machine=mac browser=chrome closed=yes runs=3
control candidate=cdp machine=mac browser=chrome step=driver_killed_pipe let_through=yes browser_after=gone connections=1 bytes=458 seen=yes
kill machine=mac form=pipe kills=20 arrived=0
cell candidate=cdp machine=mac browser=chrome major=153 step=driver_killed_pipe ok=20 of=20 median_ms=216
control candidate=cdp machine=mac browser=chrome step=driver_killed_port let_through=yes browser_after=alive connections=1 bytes=458 seen=yes
kill machine=mac form=port kills=20 arrived=20
cell candidate=cdp machine=mac browser=chrome major=153 step=driver_killed_port ok=0 of=20 median_ms=0 bytes=9160
control candidate=cdp machine=mac browser=chrome step=driver_killed_pipe_relay let_through=yes browser_after=gone connections=1 bytes=458 seen=yes
kill machine=mac form=pipe_relay kills=20 arrived=0
cell candidate=cdp machine=mac browser=chrome major=153 step=driver_killed_pipe_relay ok=20 of=20 median_ms=226 relay=pipe_relay
control candidate=cdp machine=mac browser=chrome step=driver_killed_port_relay let_through=yes browser_after=alive connections=1 bytes=458 seen=yes
kill machine=mac form=port_relay kills=20 arrived=0
cell candidate=cdp machine=mac browser=chrome major=153 step=driver_killed_port_relay ok=20 of=20 median_ms=0 relay=port_relay
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=reach_page ok=10 of=10 median_ms=3263 relay=port_relay
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=reach_opened ok=10 of=10 median_ms=3619 relay=port_relay
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=reach_frame ok=10 of=10 median_ms=3589 relay=port_relay
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=reach_worker ok=10 of=10 median_ms=3233 relay=port_relay
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=reach_shared ok=10 of=10 median_ms=3271 relay=port_relay
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=reach_service ok=10 of=10 median_ms=3232 relay=port_relay
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=list_refuse ok=10 of=10 median_ms=2390 relay=port_relay
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=list_hops ok=10 of=10 median_ms=1310 relay=port_relay
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=list_close ok=10 of=10 median_ms=1480 relay=port_relay
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=signin_form ok=10 of=10 median_ms=460 relay=port_relay
relay_cell machine=hosted-mac browser=chrome form=port_relay step=signin_form ok=10 of=10
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=table_read ok=10 of=10 median_ms=516 relay=port_relay
relay_cell machine=hosted-mac browser=chrome form=port_relay step=table_read ok=10 of=10
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=form_fill ok=10 of=10 median_ms=721 relay=port_relay
relay_cell machine=hosted-mac browser=chrome form=port_relay step=form_fill ok=10 of=10
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=payment_frame ok=10 of=10 median_ms=641 relay=port_relay
relay_cell machine=hosted-mac browser=chrome form=port_relay step=payment_frame ok=10 of=10
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=file_upload ok=10 of=10 median_ms=774 relay=port_relay
relay_cell machine=hosted-mac browser=chrome form=port_relay step=file_upload ok=10 of=10
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=session_expiry ok=10 of=10 median_ms=740 relay=port_relay
relay_cell machine=hosted-mac browser=chrome form=port_relay step=session_expiry ok=10 of=10
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=changed_page_1 ok=10 of=10 median_ms=695 relay=port_relay
relay_cell machine=hosted-mac browser=chrome form=port_relay step=changed_page_1 ok=10 of=10
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=block_request ok=10 of=10 median_ms=713 relay=port_relay
relay_cell machine=hosted-mac browser=chrome form=port_relay step=block_request ok=10 of=10
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=credential_mark ok=10 of=10 median_ms=688 relay=port_relay
relay_cell machine=hosted-mac browser=chrome form=port_relay step=credential_mark ok=10 of=10
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=payment_frame_isolated ok=10 of=10 median_ms=941 relay=port_relay
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=changed_page_1_isolated ok=10 of=10 median_ms=891 relay=port_relay
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=block_popup ok=10 of=10 median_ms=119 relay=port_relay
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=block_frame_post ok=10 of=10 median_ms=260 relay=port_relay
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=block_websocket ok=10 of=10 median_ms=53 relay=port_relay
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=cert_error ok=10 of=10 median_ms=614 relay=port_relay
way row=no_worker_paths machine=hosted-mac browser=chrome closed=yes runs=5
way row=udp_quic machine=hosted-mac browser=chrome closed=yes runs=5
way row=webtransport machine=hosted-mac browser=chrome closed=yes runs=5
way row=peer_connection machine=hosted-mac browser=chrome closed=no runs=5
way row=secure_dns machine=hosted-mac browser=chrome closed=yes runs=5
way row=lan_discovery machine=hosted-mac browser=chrome closed=yes runs=5
way row=link_lookup machine=hosted-mac browser=chrome closed=yes runs=5
way row=after_unload machine=hosted-mac browser=chrome closed=yes runs=5
way row=reports machine=hosted-mac browser=chrome closed=open runs=5
way row=handler_launch machine=hosted-mac browser=chrome closed=no runs=5
way row=updater_wake machine=hosted-mac browser=chrome closed=open runs=5
way row=ws_allowed machine=hosted-mac browser=chrome closed=yes runs=5
control row=ws_allowed machine=hosted-mac browser=chrome arrived_runs=1 runs=1
way row=frame_doc form=port_relay machine=hosted-mac browser=chrome closed=yes runs=10 told=10
way row=own_traffic machine=hosted-mac browser=chrome closed=yes runs=3
control candidate=cdp machine=hosted-mac browser=chrome step=driver_killed_pipe let_through=yes browser_after=gone connections=1 bytes=458 seen=yes
kill machine=hosted-mac form=pipe kills=20 arrived=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=driver_killed_pipe ok=20 of=20 median_ms=225
control candidate=cdp machine=hosted-mac browser=chrome step=driver_killed_port let_through=yes browser_after=alive connections=1 bytes=458 seen=yes
kill machine=hosted-mac form=port kills=20 arrived=20
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=driver_killed_port ok=0 of=20 median_ms=0 bytes=9160
control candidate=cdp machine=hosted-mac browser=chrome step=driver_killed_pipe_relay let_through=yes browser_after=gone connections=1 bytes=458 seen=yes
kill machine=hosted-mac form=pipe_relay kills=20 arrived=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=driver_killed_pipe_relay ok=20 of=20 median_ms=225 relay=pipe_relay
control candidate=cdp machine=hosted-mac browser=chrome step=driver_killed_port_relay let_through=yes browser_after=alive connections=1 bytes=458 seen=yes
kill machine=hosted-mac form=port_relay kills=20 arrived=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=driver_killed_port_relay ok=20 of=20 median_ms=0 relay=port_relay
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=reach_page ok=10 of=10 median_ms=3222 relay=port_relay
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=reach_opened ok=10 of=10 median_ms=3648 relay=port_relay
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=reach_frame ok=10 of=10 median_ms=3340 relay=port_relay
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=reach_worker ok=10 of=10 median_ms=3265 relay=port_relay
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=reach_shared ok=10 of=10 median_ms=3259 relay=port_relay
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=reach_service ok=10 of=10 median_ms=3244 relay=port_relay
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=list_refuse ok=10 of=10 median_ms=2425 relay=port_relay
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=list_hops ok=10 of=10 median_ms=1372 relay=port_relay
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=list_close ok=10 of=10 median_ms=1565 relay=port_relay
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=signin_form ok=10 of=10 median_ms=558 relay=port_relay
relay_cell machine=hosted-mac browser=edge form=port_relay step=signin_form ok=10 of=10
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=table_read ok=10 of=10 median_ms=548 relay=port_relay
relay_cell machine=hosted-mac browser=edge form=port_relay step=table_read ok=10 of=10
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=form_fill ok=10 of=10 median_ms=783 relay=port_relay
relay_cell machine=hosted-mac browser=edge form=port_relay step=form_fill ok=10 of=10
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=payment_frame ok=10 of=10 median_ms=839 relay=port_relay
relay_cell machine=hosted-mac browser=edge form=port_relay step=payment_frame ok=10 of=10
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=file_upload ok=10 of=10 median_ms=792 relay=port_relay
relay_cell machine=hosted-mac browser=edge form=port_relay step=file_upload ok=10 of=10
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=session_expiry ok=10 of=10 median_ms=924 relay=port_relay
relay_cell machine=hosted-mac browser=edge form=port_relay step=session_expiry ok=10 of=10
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=changed_page_1 ok=10 of=10 median_ms=788 relay=port_relay
relay_cell machine=hosted-mac browser=edge form=port_relay step=changed_page_1 ok=10 of=10
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=block_request ok=10 of=10 median_ms=800 relay=port_relay
relay_cell machine=hosted-mac browser=edge form=port_relay step=block_request ok=10 of=10
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=credential_mark ok=10 of=10 median_ms=837 relay=port_relay
relay_cell machine=hosted-mac browser=edge form=port_relay step=credential_mark ok=10 of=10
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=payment_frame_isolated ok=10 of=10 median_ms=987 relay=port_relay
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=changed_page_1_isolated ok=10 of=10 median_ms=962 relay=port_relay
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=block_popup ok=10 of=10 median_ms=108 relay=port_relay
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=block_frame_post ok=10 of=10 median_ms=125 relay=port_relay
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=block_websocket ok=10 of=10 median_ms=55 relay=port_relay
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=cert_error ok=10 of=10 median_ms=591 relay=port_relay
way row=no_worker_paths machine=hosted-mac browser=edge closed=yes runs=5
way row=udp_quic machine=hosted-mac browser=edge closed=yes runs=5
way row=webtransport machine=hosted-mac browser=edge closed=yes runs=5
way row=peer_connection machine=hosted-mac browser=edge closed=no runs=5
way row=secure_dns machine=hosted-mac browser=edge closed=yes runs=5
way row=lan_discovery machine=hosted-mac browser=edge closed=yes runs=5
way row=link_lookup machine=hosted-mac browser=edge closed=yes runs=5
way row=after_unload machine=hosted-mac browser=edge closed=yes runs=5
way row=reports machine=hosted-mac browser=edge closed=open runs=5
way row=handler_launch machine=hosted-mac browser=edge closed=no runs=5
way row=updater_wake machine=hosted-mac browser=edge closed=open runs=5
way row=ws_allowed machine=hosted-mac browser=edge closed=yes runs=5
control row=ws_allowed machine=hosted-mac browser=edge arrived_runs=1 runs=1
way row=frame_doc form=port_relay machine=hosted-mac browser=edge closed=yes runs=10 told=10
way row=own_traffic machine=hosted-mac browser=edge closed=yes runs=3
control candidate=cdp machine=hosted-mac browser=edge step=driver_killed_pipe let_through=yes browser_after=gone connections=0 bytes=0 seen=no
notrun candidate=cdp machine=hosted-mac browser=edge step=driver_killed_pipe cause=control
control candidate=cdp machine=hosted-mac browser=edge step=driver_killed_port let_through=yes browser_after=alive connections=1 bytes=472 seen=yes
kill machine=hosted-mac form=port kills=20 arrived=20
cell candidate=cdp machine=hosted-mac browser=edge major=157 step=driver_killed_port ok=0 of=20 median_ms=0 bytes=9440
control candidate=cdp machine=hosted-mac browser=edge step=driver_killed_pipe_relay let_through=yes browser_after=gone connections=1 bytes=472 seen=yes
kill machine=hosted-mac form=pipe_relay kills=20 arrived=0
cell candidate=cdp machine=hosted-mac browser=edge major=157 step=driver_killed_pipe_relay ok=20 of=20 median_ms=555 relay=pipe_relay
control candidate=cdp machine=hosted-mac browser=edge step=driver_killed_port_relay let_through=yes browser_after=alive connections=1 bytes=472 seen=yes
kill machine=hosted-mac form=port_relay kills=20 arrived=0
cell candidate=cdp machine=hosted-mac browser=edge major=157 step=driver_killed_port_relay ok=20 of=20 median_ms=0 relay=port_relay
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=reach_page ok=10 of=10 median_ms=3793 relay=port_relay
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=reach_opened ok=10 of=10 median_ms=3851 relay=port_relay
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=reach_frame ok=10 of=10 median_ms=3894 relay=port_relay
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=reach_worker ok=10 of=10 median_ms=3808 relay=port_relay
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=reach_shared ok=10 of=10 median_ms=3860 relay=port_relay
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=reach_service ok=10 of=10 median_ms=3932 relay=port_relay
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=list_refuse ok=10 of=10 median_ms=3245 relay=port_relay
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=list_hops ok=10 of=10 median_ms=1927 relay=port_relay
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=list_close ok=10 of=10 median_ms=2076 relay=port_relay
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=signin_form ok=10 of=10 median_ms=870 relay=port_relay
relay_cell machine=hosted-win browser=chrome form=port_relay step=signin_form ok=10 of=10
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=table_read ok=10 of=10 median_ms=922 relay=port_relay
relay_cell machine=hosted-win browser=chrome form=port_relay step=table_read ok=10 of=10
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=form_fill ok=10 of=10 median_ms=934 relay=port_relay
relay_cell machine=hosted-win browser=chrome form=port_relay step=form_fill ok=10 of=10
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=payment_frame ok=10 of=10 median_ms=1018 relay=port_relay
relay_cell machine=hosted-win browser=chrome form=port_relay step=payment_frame ok=10 of=10
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=file_upload ok=10 of=10 median_ms=932 relay=port_relay
relay_cell machine=hosted-win browser=chrome form=port_relay step=file_upload ok=10 of=10
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=session_expiry ok=10 of=10 median_ms=974 relay=port_relay
relay_cell machine=hosted-win browser=chrome form=port_relay step=session_expiry ok=10 of=10
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=changed_page_1 ok=10 of=10 median_ms=990 relay=port_relay
relay_cell machine=hosted-win browser=chrome form=port_relay step=changed_page_1 ok=10 of=10
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=block_request ok=10 of=10 median_ms=1002 relay=port_relay
relay_cell machine=hosted-win browser=chrome form=port_relay step=block_request ok=10 of=10
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=credential_mark ok=10 of=10 median_ms=982 relay=port_relay
relay_cell machine=hosted-win browser=chrome form=port_relay step=credential_mark ok=10 of=10
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=payment_frame_isolated ok=10 of=10 median_ms=1093 relay=port_relay
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=changed_page_1_isolated ok=10 of=10 median_ms=1127 relay=port_relay
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=block_popup ok=10 of=10 median_ms=83 relay=port_relay
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=block_frame_post ok=10 of=10 median_ms=98 relay=port_relay
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=block_websocket ok=10 of=10 median_ms=52 relay=port_relay
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=cert_error ok=10 of=10 median_ms=843 relay=port_relay
way row=no_worker_paths machine=hosted-win browser=chrome closed=yes runs=5
way row=udp_quic machine=hosted-win browser=chrome closed=yes runs=5
way row=webtransport machine=hosted-win browser=chrome closed=yes runs=5
way row=peer_connection machine=hosted-win browser=chrome closed=yes runs=5
way row=secure_dns machine=hosted-win browser=chrome closed=yes runs=5
way row=lan_discovery machine=hosted-win browser=chrome closed=yes runs=5
way row=link_lookup machine=hosted-win browser=chrome closed=yes runs=5
way row=after_unload machine=hosted-win browser=chrome closed=yes runs=5
way row=reports machine=hosted-win browser=chrome closed=yes runs=5
way row=handler_launch machine=hosted-win browser=chrome closed=yes runs=5
way row=updater_wake machine=hosted-win browser=chrome closed=yes runs=5
way row=ws_allowed machine=hosted-win browser=chrome closed=yes runs=5
control row=ws_allowed machine=hosted-win browser=chrome arrived_runs=1 runs=1
way row=frame_doc form=port_relay machine=hosted-win browser=chrome closed=yes runs=10 told=10
way row=own_traffic machine=hosted-win browser=chrome closed=yes runs=3
control candidate=cdp machine=hosted-win browser=chrome step=driver_killed_pipe let_through=yes browser_after=gone connections=1 bytes=452 seen=yes
kill machine=hosted-win form=pipe kills=20 arrived=0
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=driver_killed_pipe ok=20 of=20 median_ms=251
control candidate=cdp machine=hosted-win browser=chrome step=driver_killed_port let_through=yes browser_after=alive connections=1 bytes=452 seen=yes
kill machine=hosted-win form=port kills=20 arrived=20
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=driver_killed_port ok=0 of=20 median_ms=0 bytes=9040
control candidate=cdp machine=hosted-win browser=chrome step=driver_killed_pipe_relay let_through=yes browser_after=gone connections=1 bytes=452 seen=yes
kill machine=hosted-win form=pipe_relay kills=20 arrived=0
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=driver_killed_pipe_relay ok=20 of=20 median_ms=265 relay=pipe_relay
control candidate=cdp machine=hosted-win browser=chrome step=driver_killed_port_relay let_through=yes browser_after=alive connections=1 bytes=452 seen=yes
kill machine=hosted-win form=port_relay kills=20 arrived=0
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=driver_killed_port_relay ok=20 of=20 median_ms=0 relay=port_relay
cell candidate=cdp machine=hosted-win browser=edge major=156 step=reach_page ok=10 of=10 median_ms=4009 relay=port_relay
cell candidate=cdp machine=hosted-win browser=edge major=156 step=reach_opened ok=10 of=10 median_ms=4122 relay=port_relay
cell candidate=cdp machine=hosted-win browser=edge major=156 step=reach_frame ok=10 of=10 median_ms=4093 relay=port_relay
cell candidate=cdp machine=hosted-win browser=edge major=156 step=reach_worker ok=10 of=10 median_ms=4039 relay=port_relay
cell candidate=cdp machine=hosted-win browser=edge major=156 step=reach_shared ok=10 of=10 median_ms=4030 relay=port_relay
cell candidate=cdp machine=hosted-win browser=edge major=156 step=reach_service ok=10 of=10 median_ms=4010 relay=port_relay
cell candidate=cdp machine=hosted-win browser=edge major=156 step=list_refuse ok=10 of=10 median_ms=3313 relay=port_relay
cell candidate=cdp machine=hosted-win browser=edge major=156 step=list_hops ok=10 of=10 median_ms=2148 relay=port_relay
cell candidate=cdp machine=hosted-win browser=edge major=156 step=list_close ok=10 of=10 median_ms=2359 relay=port_relay
cell candidate=cdp machine=hosted-win browser=edge major=156 step=signin_form ok=10 of=10 median_ms=1098 relay=port_relay
relay_cell machine=hosted-win browser=edge form=port_relay step=signin_form ok=10 of=10
cell candidate=cdp machine=hosted-win browser=edge major=156 step=table_read ok=10 of=10 median_ms=1102 relay=port_relay
relay_cell machine=hosted-win browser=edge form=port_relay step=table_read ok=10 of=10
cell candidate=cdp machine=hosted-win browser=edge major=156 step=form_fill ok=10 of=10 median_ms=1309 relay=port_relay
relay_cell machine=hosted-win browser=edge form=port_relay step=form_fill ok=10 of=10
cell candidate=cdp machine=hosted-win browser=edge major=156 step=payment_frame ok=10 of=10 median_ms=1248 relay=port_relay
relay_cell machine=hosted-win browser=edge form=port_relay step=payment_frame ok=10 of=10
cell candidate=cdp machine=hosted-win browser=edge major=156 step=file_upload ok=10 of=10 median_ms=1255 relay=port_relay
relay_cell machine=hosted-win browser=edge form=port_relay step=file_upload ok=10 of=10
cell candidate=cdp machine=hosted-win browser=edge major=156 step=session_expiry ok=10 of=10 median_ms=1303 relay=port_relay
relay_cell machine=hosted-win browser=edge form=port_relay step=session_expiry ok=10 of=10
cell candidate=cdp machine=hosted-win browser=edge major=156 step=changed_page_1 ok=10 of=10 median_ms=1283 relay=port_relay
relay_cell machine=hosted-win browser=edge form=port_relay step=changed_page_1 ok=10 of=10
cell candidate=cdp machine=hosted-win browser=edge major=156 step=block_request ok=10 of=10 median_ms=1282 relay=port_relay
relay_cell machine=hosted-win browser=edge form=port_relay step=block_request ok=10 of=10
cell candidate=cdp machine=hosted-win browser=edge major=156 step=credential_mark ok=10 of=10 median_ms=1217 relay=port_relay
relay_cell machine=hosted-win browser=edge form=port_relay step=credential_mark ok=10 of=10
cell candidate=cdp machine=hosted-win browser=edge major=156 step=payment_frame_isolated ok=10 of=10 median_ms=1384 relay=port_relay
cell candidate=cdp machine=hosted-win browser=edge major=156 step=changed_page_1_isolated ok=10 of=10 median_ms=1339 relay=port_relay
cell candidate=cdp machine=hosted-win browser=edge major=156 step=block_popup ok=10 of=10 median_ms=93 relay=port_relay
cell candidate=cdp machine=hosted-win browser=edge major=156 step=block_frame_post ok=10 of=10 median_ms=98 relay=port_relay
cell candidate=cdp machine=hosted-win browser=edge major=156 step=block_websocket ok=10 of=10 median_ms=52 relay=port_relay
cell candidate=cdp machine=hosted-win browser=edge major=156 step=cert_error ok=10 of=10 median_ms=1084 relay=port_relay
way row=no_worker_paths machine=hosted-win browser=edge closed=yes runs=5
way row=udp_quic machine=hosted-win browser=edge closed=yes runs=5
way row=webtransport machine=hosted-win browser=edge closed=yes runs=5
way row=peer_connection machine=hosted-win browser=edge closed=yes runs=5
way row=secure_dns machine=hosted-win browser=edge closed=yes runs=5
way row=lan_discovery machine=hosted-win browser=edge closed=yes runs=5
way row=link_lookup machine=hosted-win browser=edge closed=yes runs=5
way row=after_unload machine=hosted-win browser=edge closed=yes runs=5
way row=reports machine=hosted-win browser=edge closed=yes runs=5
way row=handler_launch machine=hosted-win browser=edge closed=yes runs=5
way row=updater_wake machine=hosted-win browser=edge closed=yes runs=5
way row=ws_allowed machine=hosted-win browser=edge closed=yes runs=5
control row=ws_allowed machine=hosted-win browser=edge arrived_runs=1 runs=1
way row=frame_doc form=port_relay machine=hosted-win browser=edge closed=yes runs=10 told=10
way row=own_traffic machine=hosted-win browser=edge closed=yes runs=3
control candidate=cdp machine=hosted-win browser=edge step=driver_killed_pipe let_through=yes browser_after=gone connections=1 bytes=466 seen=yes
kill machine=hosted-win form=pipe kills=20 arrived=0
cell candidate=cdp machine=hosted-win browser=edge major=156 step=driver_killed_pipe ok=20 of=20 median_ms=328
control candidate=cdp machine=hosted-win browser=edge step=driver_killed_port let_through=yes browser_after=alive connections=1 bytes=466 seen=yes
kill machine=hosted-win form=port kills=20 arrived=20
cell candidate=cdp machine=hosted-win browser=edge major=156 step=driver_killed_port ok=0 of=20 median_ms=0 bytes=9320
control candidate=cdp machine=hosted-win browser=edge step=driver_killed_pipe_relay let_through=yes browser_after=gone connections=1 bytes=466 seen=yes
kill machine=hosted-win form=pipe_relay kills=20 arrived=0
cell candidate=cdp machine=hosted-win browser=edge major=156 step=driver_killed_pipe_relay ok=20 of=20 median_ms=323 relay=pipe_relay
control candidate=cdp machine=hosted-win browser=edge step=driver_killed_port_relay let_through=yes browser_after=alive connections=1 bytes=466 seen=yes
kill machine=hosted-win form=port_relay kills=20 arrived=0
cell candidate=cdp machine=hosted-win browser=edge major=156 step=driver_killed_port_relay ok=20 of=20 median_ms=0 relay=port_relay
notrun way row=frame_doc form=pipe machine=mac browser=chrome cause=no_executor_on_pipe
notrun relay_cell machine=mac browser=chrome form=pipe_relay cause=no_executor_on_pipe
notrun way row=frame_doc form=pipe machine=hosted-mac browser=chrome cause=no_executor_on_pipe
notrun relay_cell machine=hosted-mac browser=chrome form=pipe_relay cause=no_executor_on_pipe
notrun way row=frame_doc form=pipe machine=hosted-mac browser=edge cause=no_executor_on_pipe
notrun relay_cell machine=hosted-mac browser=edge form=pipe_relay cause=no_executor_on_pipe
notrun way row=frame_doc form=pipe machine=hosted-win browser=chrome cause=no_executor_on_pipe
notrun relay_cell machine=hosted-win browser=chrome form=pipe_relay cause=no_executor_on_pipe
notrun way row=frame_doc form=pipe machine=hosted-win browser=edge cause=no_executor_on_pipe
notrun relay_cell machine=hosted-win browser=edge form=pipe_relay cause=no_executor_on_pipe
way row=profile_new machine=all browser=all closed=yes runs=0
way row=permissions machine=all browser=all closed=yes runs=0
way row=downloads machine=all browser=all closed=yes runs=0
way row=push_and_sync machine=all browser=all closed=yes runs=0
way row=measurement_reports machine=all browser=all closed=yes runs=0
way row=federated_signin machine=all browser=all closed=yes runs=0
way row=passkey_other_device machine=all browser=all closed=yes runs=0
way row=payment_handler machine=all browser=all closed=yes runs=0
way row=proxy_prefetch machine=all browser=all closed=yes runs=0
way row=get_with_data machine=all browser=all closed=no runs=0
verdict executor=cdp version=0.9.1 product=yes relay=open open_ways=peer_connection,reports,handler_launch,updater_wake,get_with_data
```

## Third round (U0.7c)

Unit U0.7c runs A13.7's third round under the rulings up to CP-158: the trial program starts the
browser on its debugging pipe as its one client, the relay its only route, in all five cells.

What ran: each row set (`round3`, the nine rows, `isolation`, `relay`, `reach`, `ways --step
frame_doc` with a port control, `driver`, `org`) with `run.sh ... --round 3 --relay` (`driver`
without `--relay`), 10 runs per cell, 20 kills per driver form. `mac` (Chrome 153.0.8010.48) ran on
an untimed helper lane inside no load bound, so its `median_ms` is no timed measure. hosted-mac
(Chrome 157.0.8103.39, Edge 156.0.4314.58) ran in run 43369346664 and its pipe-form cells and
`relay_port_free` again in run 43407107710, Edge's `start_probe` in run 43416830488; hosted-win
(Chrome 156.0.8078.80, Edge 156.0.4314.58) in run 43368925419 and later runs for single cells. Every
way out of a run ends the processes that the run left (CP-126 E1); each run line prints
`left_before` and `left_after`, and no line of the block has `left_after` above 0.

Choices (reasons in the README): `FIRST_REPLY_WAIT` 30 s, `PROBE_WAIT` 10 s, `MAX_OPEN` 512,
`MAX_RECORD` 4096; a `check` line sums A11.1's rules over a cell's runs and three reads, five of
them decide and `udp_wide` is a note (CP-145 (3)); a counted process is classed counts, passing or
open by its own state (CP-145 (2)).

The guard of the relay's port holds it until the browser's tree is gone; when its executor is gone
it waits 3 s, asks the run's processes to end, forces them 2 s later, and frees the port 1 s after
the tree (CP-148): under load a browser whose driver died hard did not end by itself.

`org_policy`: `mac` is `notrun cause=no_policy_place`. A hosted cell counts only with a start
without the setting that replied before the write and after the removal, the value read back (the
key; `defaults read` of the property list), and the browser's policy page, printed headless with no
debugging channel and read as text, naming the setting (`r3_web::policy`): a whole word, whole-word
pieces, or letters from a line's start ended by the value that the job wrote (CP-145 (6)).

The verdict line reads `product=yes relay=yes`. Run 43407107710's call of hosted-mac Edge's
`start_probe` was cut short at its third run, whose start gave no reply within 30 s; the failed
start's own end found one Edge process left and ended it (`left_found n=1 programs=Microsoft_Edge`).
The cell was run once more (run 43416830488, `ok=10 of=10`), whose lines replace it (A13.7,
CP-158).

CP-93's open facts. (1) On Windows the policy key grants `Authenticated Users` `ReadKey`. On macOS
the property list is root:wheel with mode -rw-r--r--, and the page named the policy when the job's
own account, which can use sudo, read it; no line reads the page as an account without administrator
rights. (2) Windows' exclusive bind holds: `relay_port_free` 20 of 20 on hosted-win. (3) UDP beyond
loopback: 0 in every pipe `check` line of the block. (4) Edge: every deciding pipe cell of Edge is
ok.

```text
hosted machine=hosted-mac run=43369346664 sha=583e198338bbeab60fc3a2ed526e3c4405bd16fd
hosted machine=hosted-win run=43368925419 sha=f09e0d99ee779bc2d0b01a59f487eb4381a49921
hosted machine=hosted-mac run=43407107710 sha=8e16631c61cb67970f658c84bc075dd4c10cafc3 steps=signin_form,table_read,form_fill,payment_frame,file_upload,session_expiry,changed_page_1,block_request,credential_mark,pipe_inherit,relay_foreign,start_probe,same_site_pref,same_site_rule,payment_frame_isolated,changed_page_1_isolated,block_popup,block_frame_post,block_websocket,cert_error,list_refuse,list_hops,list_close,reach_page,reach_opened,reach_frame,reach_worker,reach_shared,reach_service,frame_doc,relay_port_free
hosted machine=hosted-mac run=43416830488 sha=f9ee2878123c4c80440a0d30d7fc8ffc78f4488e steps=edge:start_probe
hosted machine=hosted-win run=43374121394 sha=873749da0de0b0d1880702d13968a7f5ce900114 steps=driver_killed_pipe,driver_killed_pipe_relay,driver_killed_port,driver_killed_port_relay
hosted machine=hosted-win run=43384012622 sha=d684140639bb245bb34c4b6558dcf88fafcf1727 steps=chrome:org_debugging
hosted machine=hosted-win run=43389541875 sha=7b8bc2d9ab71c84752a7a1228a856dad11a8ba45 steps=edge:org_debugging,edge:org_proxy
hosted machine=hosted-win run=43398919479 sha=73a865b4f5aa22882763a47211c1f8f65c9d5753 steps=relay_port_free
check machine=mac browser=chrome form=pipe_relay step=pipe_inherit ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=pipe_inherit ok=10 of=10 median_ms=10 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=start_probe ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=start_probe ok=10 of=10 median_ms=213 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=same_site_pref ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=same_site_pref ok=10 of=10 median_ms=396 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=same_site_rule ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=same_site_rule ok=10 of=10 median_ms=54 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=signin_form ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=signin_form ok=10 of=10 median_ms=217 relay=pipe_relay
relay_cell machine=mac browser=chrome form=pipe_relay step=signin_form ok=10 of=10
check machine=mac browser=chrome form=pipe_relay step=table_read ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=table_read ok=10 of=10 median_ms=235 relay=pipe_relay
relay_cell machine=mac browser=chrome form=pipe_relay step=table_read ok=10 of=10
check machine=mac browser=chrome form=pipe_relay step=form_fill ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=form_fill ok=10 of=10 median_ms=336 relay=pipe_relay
relay_cell machine=mac browser=chrome form=pipe_relay step=form_fill ok=10 of=10
check machine=mac browser=chrome form=pipe_relay step=payment_frame ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=payment_frame ok=10 of=10 median_ms=382 relay=pipe_relay
relay_cell machine=mac browser=chrome form=pipe_relay step=payment_frame ok=10 of=10
check machine=mac browser=chrome form=pipe_relay step=file_upload ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=file_upload ok=10 of=10 median_ms=371 relay=pipe_relay
relay_cell machine=mac browser=chrome form=pipe_relay step=file_upload ok=10 of=10
check machine=mac browser=chrome form=pipe_relay step=session_expiry ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=session_expiry ok=10 of=10 median_ms=362 relay=pipe_relay
relay_cell machine=mac browser=chrome form=pipe_relay step=session_expiry ok=10 of=10
check machine=mac browser=chrome form=pipe_relay step=changed_page_1 ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=changed_page_1 ok=10 of=10 median_ms=406 relay=pipe_relay
relay_cell machine=mac browser=chrome form=pipe_relay step=changed_page_1 ok=10 of=10
check machine=mac browser=chrome form=pipe_relay step=block_request ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=block_request ok=10 of=10 median_ms=320 relay=pipe_relay
relay_cell machine=mac browser=chrome form=pipe_relay step=block_request ok=10 of=10
check machine=mac browser=chrome form=pipe_relay step=credential_mark ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=credential_mark ok=10 of=10 median_ms=376 relay=pipe_relay
relay_cell machine=mac browser=chrome form=pipe_relay step=credential_mark ok=10 of=10
check machine=mac browser=chrome form=pipe_relay step=payment_frame_isolated ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=payment_frame_isolated ok=10 of=10 median_ms=480 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=changed_page_1_isolated ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=changed_page_1_isolated ok=10 of=10 median_ms=429 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=block_popup ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=block_popup ok=10 of=10 median_ms=49 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=block_frame_post ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=block_frame_post ok=10 of=10 median_ms=100 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=block_websocket ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=block_websocket ok=10 of=10 median_ms=53 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=cert_error ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=cert_error ok=10 of=10 median_ms=1168 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=list_refuse ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=list_refuse ok=10 of=10 median_ms=2556 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=list_hops ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=list_hops ok=10 of=10 median_ms=2062 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=list_close ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=list_close ok=10 of=10 median_ms=1967 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=reach_page ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=reach_page ok=10 of=10 median_ms=3503 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=reach_opened ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=reach_opened ok=10 of=10 median_ms=3713 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=reach_frame ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=reach_frame ok=10 of=10 median_ms=3831 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=reach_worker ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=reach_worker ok=10 of=10 median_ms=3810 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=reach_shared ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=reach_shared ok=10 of=10 median_ms=3853 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=reach_service ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=reach_service ok=10 of=10 median_ms=3896 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=frame_doc ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
way row=frame_doc form=pipe_relay machine=mac browser=chrome closed=yes runs=10 told=10
check machine=mac browser=chrome form=port_relay step=frame_doc ok=0 of=10 port_flag=129 tcp_listen=20 udp_wide=0 active_port_file=20 executor_listener=0 pipe_holder=0
way row=frame_doc form=port_relay machine=mac browser=chrome closed=yes runs=10 told=10
control candidate=cdp machine=mac browser=chrome step=driver_killed_pipe let_through=yes browser_after=gone connections=1 bytes=458 seen=yes
kill machine=mac form=pipe kills=20 arrived=0
cell candidate=cdp machine=mac browser=chrome major=153 step=driver_killed_pipe ok=20 of=20 median_ms=2182
control candidate=cdp machine=mac browser=chrome step=driver_killed_port let_through=yes browser_after=alive connections=1 bytes=458 seen=yes
kill machine=mac form=port kills=20 arrived=20
cell candidate=cdp machine=mac browser=chrome major=153 step=driver_killed_port ok=0 of=20 median_ms=0 bytes=9160
control candidate=cdp machine=mac browser=chrome step=driver_killed_pipe_relay let_through=yes browser_after=gone connections=1 bytes=458 seen=yes
kill machine=mac form=pipe_relay kills=20 arrived=0
cell candidate=cdp machine=mac browser=chrome major=153 step=driver_killed_pipe_relay ok=20 of=20 median_ms=1235 relay=pipe_relay
control candidate=cdp machine=mac browser=chrome step=driver_killed_port_relay let_through=yes browser_after=alive connections=1 bytes=458 seen=yes
kill machine=mac form=port_relay kills=20 arrived=0
cell candidate=cdp machine=mac browser=chrome major=153 step=driver_killed_port_relay ok=20 of=20 median_ms=0 relay=port_relay
check machine=mac browser=chrome form=pipe_relay step=relay_foreign ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=relay_foreign ok=10 of=10 median_ms=16 relay=pipe_relay
run candidate=cdp machine=mac browser=chrome step=relay_port_free i=1 result=ok cause=- watched=yes gone_ms=5114 free_ms=6155 max_gap_ms=200 version=Chrome/153.0.8010.48 left=no left_before=0 left_after=0 ended_by=guard ended_ms=5073
run candidate=cdp machine=mac browser=chrome step=relay_port_free i=2 result=ok cause=- watched=yes gone_ms=4536 free_ms=5538 max_gap_ms=215 version=Chrome/153.0.8010.48 left=no left_before=0 left_after=0 ended_by=guard ended_ms=4490
run candidate=cdp machine=mac browser=chrome step=relay_port_free i=3 result=ok cause=- watched=yes gone_ms=5113 free_ms=6094 max_gap_ms=235 version=Chrome/153.0.8010.48 left=no left_before=0 left_after=0 ended_by=guard ended_ms=5071
run candidate=cdp machine=mac browser=chrome step=relay_port_free i=4 result=ok cause=- watched=yes gone_ms=5181 free_ms=6177 max_gap_ms=205 version=Chrome/153.0.8010.48 left=no left_before=0 left_after=0 ended_by=guard ended_ms=5120
run candidate=cdp machine=mac browser=chrome step=relay_port_free i=5 result=ok cause=- watched=yes gone_ms=5046 free_ms=6076 max_gap_ms=174 version=Chrome/153.0.8010.48 left=no left_before=0 left_after=0 ended_by=guard ended_ms=5044
run candidate=cdp machine=mac browser=chrome step=relay_port_free i=6 result=ok cause=- watched=yes gone_ms=5096 free_ms=6120 max_gap_ms=258 version=Chrome/153.0.8010.48 left=no left_before=0 left_after=0 ended_by=guard ended_ms=5039
run candidate=cdp machine=mac browser=chrome step=relay_port_free i=7 result=ok cause=- watched=yes gone_ms=5124 free_ms=6165 max_gap_ms=264 version=Chrome/153.0.8010.48 left=no left_before=0 left_after=0 ended_by=guard ended_ms=5141
run candidate=cdp machine=mac browser=chrome step=relay_port_free i=8 result=ok cause=- watched=yes gone_ms=5131 free_ms=6187 max_gap_ms=177 version=Chrome/153.0.8010.48 left=no left_before=0 left_after=0 ended_by=guard ended_ms=5036
run candidate=cdp machine=mac browser=chrome step=relay_port_free i=9 result=ok cause=- watched=yes gone_ms=5104 free_ms=6110 max_gap_ms=171 version=Chrome/153.0.8010.48 left=no left_before=0 left_after=0 ended_by=guard ended_ms=5081
run candidate=cdp machine=mac browser=chrome step=relay_port_free i=10 result=ok cause=- watched=yes gone_ms=5103 free_ms=6147 max_gap_ms=241 version=Chrome/153.0.8010.48 left=no left_before=0 left_after=0 ended_by=guard ended_ms=5085
cell candidate=cdp machine=mac browser=chrome major=153 step=relay_port_free ok=10 of=10 median_ms=5108 relay=pipe_relay
way row=frame_doc form=pipe_relay machine=hosted-mac browser=chrome closed=yes runs=10 told=10
check machine=hosted-mac browser=chrome form=port_relay step=frame_doc ok=0 of=10 port_flag=90 tcp_listen=20 udp_wide=0 active_port_file=20 executor_listener=0 pipe_holder=0
way row=frame_doc form=port_relay machine=hosted-mac browser=chrome closed=yes runs=10 told=10
control candidate=cdp machine=hosted-mac browser=chrome step=driver_killed_pipe let_through=yes browser_after=gone connections=1 bytes=458 seen=yes
kill machine=hosted-mac form=pipe kills=20 arrived=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=driver_killed_pipe ok=20 of=20 median_ms=281
control candidate=cdp machine=hosted-mac browser=chrome step=driver_killed_port let_through=yes browser_after=alive connections=1 bytes=458 seen=yes
kill machine=hosted-mac form=port kills=20 arrived=20
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=driver_killed_port ok=0 of=20 median_ms=0 bytes=9160
control candidate=cdp machine=hosted-mac browser=chrome step=driver_killed_pipe_relay let_through=yes browser_after=gone connections=1 bytes=458 seen=yes
kill machine=hosted-mac form=pipe_relay kills=20 arrived=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=driver_killed_pipe_relay ok=20 of=20 median_ms=128 relay=pipe_relay
control candidate=cdp machine=hosted-mac browser=chrome step=driver_killed_port_relay let_through=yes browser_after=alive connections=1 bytes=458 seen=yes
kill machine=hosted-mac form=port_relay kills=20 arrived=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=driver_killed_port_relay ok=20 of=20 median_ms=0 relay=port_relay
way row=frame_doc form=pipe_relay machine=hosted-mac browser=edge closed=yes runs=10 told=10
check machine=hosted-mac browser=edge form=port_relay step=frame_doc ok=0 of=10 port_flag=73 tcp_listen=20 udp_wide=0 active_port_file=20 executor_listener=0 pipe_holder=0
way row=frame_doc form=port_relay machine=hosted-mac browser=edge closed=yes runs=10 told=10
control candidate=cdp machine=hosted-mac browser=edge step=driver_killed_pipe let_through=yes browser_after=gone connections=1 bytes=472 seen=yes
kill machine=hosted-mac form=pipe kills=20 arrived=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=driver_killed_pipe ok=20 of=20 median_ms=550
control candidate=cdp machine=hosted-mac browser=edge step=driver_killed_port let_through=yes browser_after=alive connections=1 bytes=472 seen=yes
kill machine=hosted-mac form=port kills=20 arrived=20
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=driver_killed_port ok=0 of=20 median_ms=0 bytes=9440
control candidate=cdp machine=hosted-mac browser=edge step=driver_killed_pipe_relay let_through=yes browser_after=gone connections=1 bytes=472 seen=yes
kill machine=hosted-mac form=pipe_relay kills=20 arrived=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=driver_killed_pipe_relay ok=20 of=20 median_ms=269 relay=pipe_relay
control candidate=cdp machine=hosted-mac browser=edge step=driver_killed_port_relay let_through=yes browser_after=alive connections=1 bytes=472 seen=yes
kill machine=hosted-mac form=port_relay kills=20 arrived=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=driver_killed_port_relay ok=20 of=20 median_ms=0 relay=port_relay
cell candidate=cdp machine=hosted-mac browser=chrome major=0 step=org_debugging ok=10 of=10 median_ms=0 relay=pipe_relay control_before=reply value_read=0 page_named=yes plist_owner=root:wheel plist_mode=-rw-r--r-- plist_value=0 page_named=yes page_platform=3 control_platform=2 control_after=reply
cell candidate=cdp machine=hosted-mac browser=chrome major=0 step=org_proxy ok=10 of=10 median_ms=0 relay=pipe_relay control_before=reply value_read=direct page_named=yes plist_owner=root:wheel plist_mode=-rw-r--r-- plist_value=direct page_named=yes page_platform=3 control_platform=2 control_after=reply
cell candidate=cdp machine=hosted-mac browser=edge major=0 step=org_debugging ok=10 of=10 median_ms=0 relay=pipe_relay control_before=reply value_read=0 page_named=yes plist_owner=root:wheel plist_mode=-rw-r--r-- plist_value=0 page_named=yes page_platform=1 control_platform=0 control_after=reply
cell candidate=cdp machine=hosted-mac browser=edge major=0 step=org_proxy ok=10 of=10 median_ms=0 relay=pipe_relay control_before=reply value_read=direct page_named=yes plist_owner=root:wheel plist_mode=-rw-r--r-- plist_value=direct page_named=yes page_platform=1 control_platform=0 control_after=reply
check machine=hosted-win browser=chrome form=pipe_relay step=pipe_inherit ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=pipe_inherit ok=10 of=10 median_ms=184 relay=pipe_relay
check machine=hosted-win browser=chrome form=pipe_relay step=relay_foreign ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=relay_foreign ok=10 of=10 median_ms=10 relay=pipe_relay
check machine=hosted-win browser=chrome form=pipe_relay step=start_probe ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=start_probe ok=10 of=10 median_ms=76 relay=pipe_relay
check machine=hosted-win browser=chrome form=pipe_relay step=same_site_pref ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=same_site_pref ok=10 of=10 median_ms=224 relay=pipe_relay
check machine=hosted-win browser=chrome form=pipe_relay step=same_site_rule ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=same_site_rule ok=10 of=10 median_ms=54 relay=pipe_relay
check machine=hosted-win browser=chrome form=pipe_relay step=signin_form ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=signin_form ok=10 of=10 median_ms=147 relay=pipe_relay
relay_cell machine=hosted-win browser=chrome form=pipe_relay step=signin_form ok=10 of=10
check machine=hosted-win browser=chrome form=pipe_relay step=table_read ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=table_read ok=10 of=10 median_ms=153 relay=pipe_relay
relay_cell machine=hosted-win browser=chrome form=pipe_relay step=table_read ok=10 of=10
check machine=hosted-win browser=chrome form=pipe_relay step=form_fill ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=form_fill ok=10 of=10 median_ms=252 relay=pipe_relay
relay_cell machine=hosted-win browser=chrome form=pipe_relay step=form_fill ok=10 of=10
check machine=hosted-win browser=chrome form=pipe_relay step=payment_frame ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=payment_frame ok=10 of=10 median_ms=303 relay=pipe_relay
relay_cell machine=hosted-win browser=chrome form=pipe_relay step=payment_frame ok=10 of=10
check machine=hosted-win browser=chrome form=pipe_relay step=file_upload ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=file_upload ok=10 of=10 median_ms=249 relay=pipe_relay
relay_cell machine=hosted-win browser=chrome form=pipe_relay step=file_upload ok=10 of=10
check machine=hosted-win browser=chrome form=pipe_relay step=session_expiry ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=session_expiry ok=10 of=10 median_ms=256 relay=pipe_relay
relay_cell machine=hosted-win browser=chrome form=pipe_relay step=session_expiry ok=10 of=10
check machine=hosted-win browser=chrome form=pipe_relay step=changed_page_1 ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=changed_page_1 ok=10 of=10 median_ms=295 relay=pipe_relay
relay_cell machine=hosted-win browser=chrome form=pipe_relay step=changed_page_1 ok=10 of=10
check machine=hosted-win browser=chrome form=pipe_relay step=block_request ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=block_request ok=10 of=10 median_ms=237 relay=pipe_relay
relay_cell machine=hosted-win browser=chrome form=pipe_relay step=block_request ok=10 of=10
check machine=hosted-win browser=chrome form=pipe_relay step=credential_mark ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=credential_mark ok=10 of=10 median_ms=222 relay=pipe_relay
relay_cell machine=hosted-win browser=chrome form=pipe_relay step=credential_mark ok=10 of=10
check machine=hosted-win browser=chrome form=pipe_relay step=payment_frame_isolated ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=payment_frame_isolated ok=10 of=10 median_ms=356 relay=pipe_relay
check machine=hosted-win browser=chrome form=pipe_relay step=changed_page_1_isolated ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=changed_page_1_isolated ok=10 of=10 median_ms=332 relay=pipe_relay
check machine=hosted-win browser=chrome form=pipe_relay step=block_popup ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=block_popup ok=10 of=10 median_ms=80 relay=pipe_relay
check machine=hosted-win browser=chrome form=pipe_relay step=block_frame_post ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=block_frame_post ok=10 of=10 median_ms=96 relay=pipe_relay
check machine=hosted-win browser=chrome form=pipe_relay step=block_websocket ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=block_websocket ok=10 of=10 median_ms=52 relay=pipe_relay
check machine=hosted-win browser=chrome form=pipe_relay step=cert_error ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=cert_error ok=10 of=10 median_ms=240 relay=pipe_relay
check machine=hosted-win browser=chrome form=pipe_relay step=list_refuse ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=list_refuse ok=10 of=10 median_ms=2414 relay=pipe_relay
check machine=hosted-win browser=chrome form=pipe_relay step=list_hops ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=list_hops ok=10 of=10 median_ms=1256 relay=pipe_relay
check machine=hosted-win browser=chrome form=pipe_relay step=list_close ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=list_close ok=10 of=10 median_ms=1476 relay=pipe_relay
check machine=hosted-win browser=chrome form=pipe_relay step=reach_page ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=reach_page ok=10 of=10 median_ms=3179 relay=pipe_relay
check machine=hosted-win browser=chrome form=pipe_relay step=reach_opened ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=reach_opened ok=10 of=10 median_ms=3206 relay=pipe_relay
check machine=hosted-win browser=chrome form=pipe_relay step=reach_frame ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=reach_frame ok=10 of=10 median_ms=3236 relay=pipe_relay
check machine=hosted-win browser=chrome form=pipe_relay step=reach_worker ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=reach_worker ok=10 of=10 median_ms=3185 relay=pipe_relay
check machine=hosted-win browser=chrome form=pipe_relay step=reach_shared ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=reach_shared ok=10 of=10 median_ms=3182 relay=pipe_relay
check machine=hosted-win browser=chrome form=pipe_relay step=reach_service ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=reach_service ok=10 of=10 median_ms=3181 relay=pipe_relay
check machine=hosted-win browser=chrome form=pipe_relay step=frame_doc ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
way row=frame_doc form=pipe_relay machine=hosted-win browser=chrome closed=yes runs=10 told=10
check machine=hosted-win browser=chrome form=port_relay step=frame_doc ok=0 of=10 port_flag=90 tcp_listen=20 udp_wide=0 active_port_file=20 executor_listener=0 pipe_holder=0
way row=frame_doc form=port_relay machine=hosted-win browser=chrome closed=yes runs=10 told=10
kill machine=hosted-win form=pipe kills=20 arrived=0
kill machine=hosted-win form=port kills=20 arrived=20
kill machine=hosted-win form=pipe_relay kills=20 arrived=0
kill machine=hosted-win form=port_relay kills=20 arrived=0
cell candidate=cdp machine=hosted-win browser=chrome major=0 step=org_proxy ok=10 of=10 median_ms=0 relay=pipe_relay control_before=reply value_read=direct page_named=yes control_after=reply
check machine=hosted-win browser=edge form=pipe_relay step=pipe_inherit ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=edge major=156 step=pipe_inherit ok=10 of=10 median_ms=231 relay=pipe_relay
check machine=hosted-win browser=edge form=pipe_relay step=relay_foreign ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=edge major=156 step=relay_foreign ok=10 of=10 median_ms=11 relay=pipe_relay
check machine=hosted-win browser=edge form=pipe_relay step=start_probe ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=edge major=156 step=start_probe ok=10 of=10 median_ms=83 relay=pipe_relay
check machine=hosted-win browser=edge form=pipe_relay step=same_site_pref ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=edge major=156 step=same_site_pref ok=10 of=10 median_ms=271 relay=pipe_relay
check machine=hosted-win browser=edge form=pipe_relay step=same_site_rule ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=edge major=156 step=same_site_rule ok=10 of=10 median_ms=54 relay=pipe_relay
check machine=hosted-win browser=edge form=pipe_relay step=signin_form ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=edge major=156 step=signin_form ok=10 of=10 median_ms=174 relay=pipe_relay
relay_cell machine=hosted-win browser=edge form=pipe_relay step=signin_form ok=10 of=10
check machine=hosted-win browser=edge form=pipe_relay step=table_read ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=edge major=156 step=table_read ok=10 of=10 median_ms=171 relay=pipe_relay
relay_cell machine=hosted-win browser=edge form=pipe_relay step=table_read ok=10 of=10
check machine=hosted-win browser=edge form=pipe_relay step=form_fill ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=edge major=156 step=form_fill ok=10 of=10 median_ms=288 relay=pipe_relay
relay_cell machine=hosted-win browser=edge form=pipe_relay step=form_fill ok=10 of=10
check machine=hosted-win browser=edge form=pipe_relay step=payment_frame ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=edge major=156 step=payment_frame ok=10 of=10 median_ms=356 relay=pipe_relay
relay_cell machine=hosted-win browser=edge form=pipe_relay step=payment_frame ok=10 of=10
check machine=hosted-win browser=edge form=pipe_relay step=file_upload ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=edge major=156 step=file_upload ok=10 of=10 median_ms=277 relay=pipe_relay
relay_cell machine=hosted-win browser=edge form=pipe_relay step=file_upload ok=10 of=10
check machine=hosted-win browser=edge form=pipe_relay step=session_expiry ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=edge major=156 step=session_expiry ok=10 of=10 median_ms=314 relay=pipe_relay
relay_cell machine=hosted-win browser=edge form=pipe_relay step=session_expiry ok=10 of=10
check machine=hosted-win browser=edge form=pipe_relay step=changed_page_1 ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=edge major=156 step=changed_page_1 ok=10 of=10 median_ms=339 relay=pipe_relay
relay_cell machine=hosted-win browser=edge form=pipe_relay step=changed_page_1 ok=10 of=10
check machine=hosted-win browser=edge form=pipe_relay step=block_request ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=edge major=156 step=block_request ok=10 of=10 median_ms=278 relay=pipe_relay
relay_cell machine=hosted-win browser=edge form=pipe_relay step=block_request ok=10 of=10
check machine=hosted-win browser=edge form=pipe_relay step=credential_mark ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=edge major=156 step=credential_mark ok=10 of=10 median_ms=268 relay=pipe_relay
relay_cell machine=hosted-win browser=edge form=pipe_relay step=credential_mark ok=10 of=10
check machine=hosted-win browser=edge form=pipe_relay step=payment_frame_isolated ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=edge major=156 step=payment_frame_isolated ok=10 of=10 median_ms=392 relay=pipe_relay
check machine=hosted-win browser=edge form=pipe_relay step=changed_page_1_isolated ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=edge major=156 step=changed_page_1_isolated ok=10 of=10 median_ms=376 relay=pipe_relay
check machine=hosted-win browser=edge form=pipe_relay step=block_popup ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=edge major=156 step=block_popup ok=10 of=10 median_ms=86 relay=pipe_relay
check machine=hosted-win browser=edge form=pipe_relay step=block_frame_post ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=edge major=156 step=block_frame_post ok=10 of=10 median_ms=94 relay=pipe_relay
check machine=hosted-win browser=edge form=pipe_relay step=block_websocket ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=edge major=156 step=block_websocket ok=10 of=10 median_ms=52 relay=pipe_relay
check machine=hosted-win browser=edge form=pipe_relay step=cert_error ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=edge major=156 step=cert_error ok=10 of=10 median_ms=220 relay=pipe_relay
check machine=hosted-win browser=edge form=pipe_relay step=list_refuse ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=edge major=156 step=list_refuse ok=10 of=10 median_ms=2441 relay=pipe_relay
check machine=hosted-win browser=edge form=pipe_relay step=list_hops ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=edge major=156 step=list_hops ok=10 of=10 median_ms=1294 relay=pipe_relay
check machine=hosted-win browser=edge form=pipe_relay step=list_close ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=edge major=156 step=list_close ok=10 of=10 median_ms=1480 relay=pipe_relay
check machine=hosted-win browser=edge form=pipe_relay step=reach_page ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=edge major=156 step=reach_page ok=10 of=10 median_ms=3193 relay=pipe_relay
check machine=hosted-win browser=edge form=pipe_relay step=reach_opened ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=edge major=156 step=reach_opened ok=10 of=10 median_ms=3248 relay=pipe_relay
check machine=hosted-win browser=edge form=pipe_relay step=reach_frame ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=edge major=156 step=reach_frame ok=10 of=10 median_ms=3260 relay=pipe_relay
check machine=hosted-win browser=edge form=pipe_relay step=reach_worker ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=edge major=156 step=reach_worker ok=10 of=10 median_ms=3200 relay=pipe_relay
check machine=hosted-win browser=edge form=pipe_relay step=reach_shared ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=edge major=156 step=reach_shared ok=10 of=10 median_ms=3187 relay=pipe_relay
check machine=hosted-win browser=edge form=pipe_relay step=reach_service ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-win browser=edge major=156 step=reach_service ok=10 of=10 median_ms=3193 relay=pipe_relay
check machine=hosted-win browser=edge form=pipe_relay step=frame_doc ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
way row=frame_doc form=pipe_relay machine=hosted-win browser=edge closed=yes runs=10 told=10
check machine=hosted-win browser=edge form=port_relay step=frame_doc ok=0 of=10 port_flag=70 tcp_listen=20 udp_wide=0 active_port_file=20 executor_listener=0 pipe_holder=0
way row=frame_doc form=port_relay machine=hosted-win browser=edge closed=yes runs=10 told=10
kill machine=hosted-win form=pipe kills=20 arrived=0
kill machine=hosted-win form=port kills=20 arrived=20
kill machine=hosted-win form=pipe_relay kills=20 arrived=0
kill machine=hosted-win form=port_relay kills=20 arrived=0
check machine=hosted-mac browser=chrome form=pipe_relay step=pipe_inherit ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=pipe_inherit ok=10 of=10 median_ms=5 relay=pipe_relay
check machine=hosted-mac browser=chrome form=pipe_relay step=relay_foreign ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=relay_foreign ok=10 of=10 median_ms=12 relay=pipe_relay
check machine=hosted-mac browser=chrome form=pipe_relay step=start_probe ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=start_probe ok=10 of=10 median_ms=503 relay=pipe_relay
check machine=hosted-mac browser=chrome form=pipe_relay step=same_site_pref ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=same_site_pref ok=10 of=10 median_ms=949 relay=pipe_relay
check machine=hosted-mac browser=chrome form=pipe_relay step=same_site_rule ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=same_site_rule ok=10 of=10 median_ms=56 relay=pipe_relay
check machine=hosted-mac browser=chrome form=pipe_relay step=signin_form ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=signin_form ok=10 of=10 median_ms=608 relay=pipe_relay
relay_cell machine=hosted-mac browser=chrome form=pipe_relay step=signin_form ok=10 of=10
check machine=hosted-mac browser=chrome form=pipe_relay step=table_read ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=table_read ok=10 of=10 median_ms=624 relay=pipe_relay
relay_cell machine=hosted-mac browser=chrome form=pipe_relay step=table_read ok=10 of=10
check machine=hosted-mac browser=chrome form=pipe_relay step=form_fill ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=form_fill ok=10 of=10 median_ms=945 relay=pipe_relay
relay_cell machine=hosted-mac browser=chrome form=pipe_relay step=form_fill ok=10 of=10
check machine=hosted-mac browser=chrome form=pipe_relay step=payment_frame ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=payment_frame ok=10 of=10 median_ms=749 relay=pipe_relay
relay_cell machine=hosted-mac browser=chrome form=pipe_relay step=payment_frame ok=10 of=10
check machine=hosted-mac browser=chrome form=pipe_relay step=file_upload ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=file_upload ok=10 of=10 median_ms=811 relay=pipe_relay
relay_cell machine=hosted-mac browser=chrome form=pipe_relay step=file_upload ok=10 of=10
check machine=hosted-mac browser=chrome form=pipe_relay step=session_expiry ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=session_expiry ok=10 of=10 median_ms=775 relay=pipe_relay
relay_cell machine=hosted-mac browser=chrome form=pipe_relay step=session_expiry ok=10 of=10
check machine=hosted-mac browser=chrome form=pipe_relay step=changed_page_1 ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=changed_page_1 ok=10 of=10 median_ms=736 relay=pipe_relay
relay_cell machine=hosted-mac browser=chrome form=pipe_relay step=changed_page_1 ok=10 of=10
check machine=hosted-mac browser=chrome form=pipe_relay step=block_request ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=block_request ok=10 of=10 median_ms=667 relay=pipe_relay
relay_cell machine=hosted-mac browser=chrome form=pipe_relay step=block_request ok=10 of=10
check machine=hosted-mac browser=chrome form=pipe_relay step=credential_mark ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=credential_mark ok=10 of=10 median_ms=720 relay=pipe_relay
relay_cell machine=hosted-mac browser=chrome form=pipe_relay step=credential_mark ok=10 of=10
check machine=hosted-mac browser=chrome form=pipe_relay step=payment_frame_isolated ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=payment_frame_isolated ok=10 of=10 median_ms=978 relay=pipe_relay
check machine=hosted-mac browser=chrome form=pipe_relay step=changed_page_1_isolated ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=changed_page_1_isolated ok=10 of=10 median_ms=992 relay=pipe_relay
check machine=hosted-mac browser=chrome form=pipe_relay step=block_popup ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=block_popup ok=10 of=10 median_ms=128 relay=pipe_relay
check machine=hosted-mac browser=chrome form=pipe_relay step=block_frame_post ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=block_frame_post ok=10 of=10 median_ms=314 relay=pipe_relay
check machine=hosted-mac browser=chrome form=pipe_relay step=block_websocket ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=block_websocket ok=10 of=10 median_ms=55 relay=pipe_relay
check machine=hosted-mac browser=chrome form=pipe_relay step=cert_error ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=cert_error ok=10 of=10 median_ms=342 relay=pipe_relay
check machine=hosted-mac browser=chrome form=pipe_relay step=list_refuse ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=list_refuse ok=10 of=10 median_ms=2704 relay=pipe_relay
check machine=hosted-mac browser=chrome form=pipe_relay step=list_hops ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=list_hops ok=10 of=10 median_ms=1612 relay=pipe_relay
check machine=hosted-mac browser=chrome form=pipe_relay step=list_close ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=list_close ok=10 of=10 median_ms=1797 relay=pipe_relay
check machine=hosted-mac browser=chrome form=pipe_relay step=reach_page ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=reach_page ok=10 of=10 median_ms=3489 relay=pipe_relay
check machine=hosted-mac browser=chrome form=pipe_relay step=reach_opened ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=reach_opened ok=10 of=10 median_ms=3667 relay=pipe_relay
check machine=hosted-mac browser=chrome form=pipe_relay step=reach_frame ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=reach_frame ok=10 of=10 median_ms=3552 relay=pipe_relay
check machine=hosted-mac browser=chrome form=pipe_relay step=reach_worker ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=reach_worker ok=10 of=10 median_ms=3561 relay=pipe_relay
check machine=hosted-mac browser=chrome form=pipe_relay step=reach_shared ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=reach_shared ok=10 of=10 median_ms=3533 relay=pipe_relay
check machine=hosted-mac browser=chrome form=pipe_relay step=reach_service ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=reach_service ok=10 of=10 median_ms=3548 relay=pipe_relay
check machine=hosted-mac browser=chrome form=pipe_relay step=frame_doc ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
run candidate=cdp machine=hosted-mac browser=chrome step=relay_port_free i=1 result=ok cause=- watched=yes gone_ms=90 free_ms=1208 max_gap_ms=22 version=Chrome/157.0.8103.39 left=no left_before=0 left_after=0 ended_by=self
run candidate=cdp machine=hosted-mac browser=chrome step=relay_port_free i=2 result=ok cause=- watched=yes gone_ms=219 free_ms=1332 max_gap_ms=26 version=Chrome/157.0.8103.39 left=no left_before=0 left_after=0 ended_by=self
run candidate=cdp machine=hosted-mac browser=chrome step=relay_port_free i=3 result=ok cause=- watched=yes gone_ms=183 free_ms=1351 max_gap_ms=30 version=Chrome/157.0.8103.39 left=no left_before=0 left_after=0 ended_by=self
run candidate=cdp machine=hosted-mac browser=chrome step=relay_port_free i=4 result=ok cause=- watched=yes gone_ms=164 free_ms=1408 max_gap_ms=27 version=Chrome/157.0.8103.39 left=no left_before=0 left_after=0 ended_by=self
run candidate=cdp machine=hosted-mac browser=chrome step=relay_port_free i=5 result=ok cause=- watched=yes gone_ms=107 free_ms=1217 max_gap_ms=22 version=Chrome/157.0.8103.39 left=no left_before=0 left_after=0 ended_by=self
run candidate=cdp machine=hosted-mac browser=chrome step=relay_port_free i=6 result=ok cause=- watched=yes gone_ms=239 free_ms=1298 max_gap_ms=21 version=Chrome/157.0.8103.39 left=no left_before=0 left_after=0 ended_by=self
run candidate=cdp machine=hosted-mac browser=chrome step=relay_port_free i=7 result=ok cause=- watched=yes gone_ms=135 free_ms=1247 max_gap_ms=23 version=Chrome/157.0.8103.39 left=no left_before=0 left_after=0 ended_by=self
run candidate=cdp machine=hosted-mac browser=chrome step=relay_port_free i=8 result=ok cause=- watched=yes gone_ms=168 free_ms=1191 max_gap_ms=22 version=Chrome/157.0.8103.39 left=no left_before=0 left_after=0 ended_by=self
run candidate=cdp machine=hosted-mac browser=chrome step=relay_port_free i=9 result=ok cause=- watched=yes gone_ms=150 free_ms=1238 max_gap_ms=26 version=Chrome/157.0.8103.39 left=no left_before=0 left_after=0 ended_by=self
run candidate=cdp machine=hosted-mac browser=chrome step=relay_port_free i=10 result=ok cause=- watched=yes gone_ms=130 free_ms=1332 max_gap_ms=22 version=Chrome/157.0.8103.39 left=no left_before=0 left_after=0 ended_by=self
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=relay_port_free ok=10 of=10 median_ms=157 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=pipe_inherit ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=pipe_inherit ok=10 of=10 median_ms=6 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=relay_foreign ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=relay_foreign ok=10 of=10 median_ms=6 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=same_site_pref ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=same_site_pref ok=10 of=10 median_ms=851 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=same_site_rule ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=same_site_rule ok=10 of=10 median_ms=57 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=signin_form ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=signin_form ok=10 of=10 median_ms=693 relay=pipe_relay
relay_cell machine=hosted-mac browser=edge form=pipe_relay step=signin_form ok=10 of=10
check machine=hosted-mac browser=edge form=pipe_relay step=table_read ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=table_read ok=10 of=10 median_ms=727 relay=pipe_relay
relay_cell machine=hosted-mac browser=edge form=pipe_relay step=table_read ok=10 of=10
check machine=hosted-mac browser=edge form=pipe_relay step=form_fill ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=form_fill ok=10 of=10 median_ms=969 relay=pipe_relay
relay_cell machine=hosted-mac browser=edge form=pipe_relay step=form_fill ok=10 of=10
check machine=hosted-mac browser=edge form=pipe_relay step=payment_frame ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=payment_frame ok=10 of=10 median_ms=978 relay=pipe_relay
relay_cell machine=hosted-mac browser=edge form=pipe_relay step=payment_frame ok=10 of=10
check machine=hosted-mac browser=edge form=pipe_relay step=file_upload ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=file_upload ok=10 of=10 median_ms=1012 relay=pipe_relay
relay_cell machine=hosted-mac browser=edge form=pipe_relay step=file_upload ok=10 of=10
check machine=hosted-mac browser=edge form=pipe_relay step=session_expiry ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=session_expiry ok=10 of=10 median_ms=893 relay=pipe_relay
relay_cell machine=hosted-mac browser=edge form=pipe_relay step=session_expiry ok=10 of=10
check machine=hosted-mac browser=edge form=pipe_relay step=changed_page_1 ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=changed_page_1 ok=10 of=10 median_ms=952 relay=pipe_relay
relay_cell machine=hosted-mac browser=edge form=pipe_relay step=changed_page_1 ok=10 of=10
check machine=hosted-mac browser=edge form=pipe_relay step=block_request ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=block_request ok=10 of=10 median_ms=1016 relay=pipe_relay
relay_cell machine=hosted-mac browser=edge form=pipe_relay step=block_request ok=10 of=10
check machine=hosted-mac browser=edge form=pipe_relay step=credential_mark ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=credential_mark ok=10 of=10 median_ms=920 relay=pipe_relay
relay_cell machine=hosted-mac browser=edge form=pipe_relay step=credential_mark ok=10 of=10
check machine=hosted-mac browser=edge form=pipe_relay step=payment_frame_isolated ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=payment_frame_isolated ok=10 of=10 median_ms=1204 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=changed_page_1_isolated ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=changed_page_1_isolated ok=10 of=10 median_ms=1109 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=block_popup ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=block_popup ok=10 of=10 median_ms=122 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=block_frame_post ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=block_frame_post ok=10 of=10 median_ms=295 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=block_websocket ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=block_websocket ok=10 of=10 median_ms=55 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=cert_error ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=cert_error ok=10 of=10 median_ms=378 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=list_refuse ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=list_refuse ok=10 of=10 median_ms=2921 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=list_hops ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=list_hops ok=10 of=10 median_ms=1795 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=list_close ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=list_close ok=10 of=10 median_ms=1884 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=reach_page ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=reach_page ok=10 of=10 median_ms=3590 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=reach_opened ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=reach_opened ok=10 of=10 median_ms=3771 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=reach_frame ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=reach_frame ok=10 of=10 median_ms=3648 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=reach_worker ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=reach_worker ok=10 of=10 median_ms=3651 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=reach_shared ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=reach_shared ok=10 of=10 median_ms=3634 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=reach_service ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=reach_service ok=10 of=10 median_ms=3628 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=frame_doc ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
run candidate=cdp machine=hosted-mac browser=edge step=relay_port_free i=1 result=ok cause=- watched=yes gone_ms=247 free_ms=1308 max_gap_ms=30 version=Edg/156.0.4314.58 left=no left_before=0 left_after=0 ended_by=self
run candidate=cdp machine=hosted-mac browser=edge step=relay_port_free i=2 result=ok cause=- watched=yes gone_ms=323 free_ms=1546 max_gap_ms=19 version=Edg/156.0.4314.58 left=no left_before=0 left_after=0 ended_by=self
run candidate=cdp machine=hosted-mac browser=edge step=relay_port_free i=3 result=ok cause=- watched=yes gone_ms=339 free_ms=1447 max_gap_ms=33 version=Edg/156.0.4314.58 left=no left_before=0 left_after=0 ended_by=self
run candidate=cdp machine=hosted-mac browser=edge step=relay_port_free i=4 result=ok cause=- watched=yes gone_ms=311 free_ms=1489 max_gap_ms=29 version=Edg/156.0.4314.58 left=no left_before=0 left_after=0 ended_by=self
run candidate=cdp machine=hosted-mac browser=edge step=relay_port_free i=5 result=ok cause=- watched=yes gone_ms=226 free_ms=1359 max_gap_ms=21 version=Edg/156.0.4314.58 left=no left_before=0 left_after=0 ended_by=self
run candidate=cdp machine=hosted-mac browser=edge step=relay_port_free i=6 result=ok cause=- watched=yes gone_ms=169 free_ms=1262 max_gap_ms=29 version=Edg/156.0.4314.58 left=no left_before=0 left_after=0 ended_by=self
run candidate=cdp machine=hosted-mac browser=edge step=relay_port_free i=7 result=ok cause=- watched=yes gone_ms=228 free_ms=1269 max_gap_ms=28 version=Edg/156.0.4314.58 left=no left_before=0 left_after=0 ended_by=self
run candidate=cdp machine=hosted-mac browser=edge step=relay_port_free i=8 result=ok cause=- watched=yes gone_ms=128 free_ms=1250 max_gap_ms=20 version=Edg/156.0.4314.58 left=no left_before=0 left_after=0 ended_by=self
run candidate=cdp machine=hosted-mac browser=edge step=relay_port_free i=9 result=ok cause=- watched=yes gone_ms=275 free_ms=1400 max_gap_ms=28 version=Edg/156.0.4314.58 left=no left_before=0 left_after=0 ended_by=self
run candidate=cdp machine=hosted-mac browser=edge step=relay_port_free i=10 result=ok cause=- watched=yes gone_ms=239 free_ms=1317 max_gap_ms=30 version=Edg/156.0.4314.58 left=no left_before=0 left_after=0 ended_by=self
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=relay_port_free ok=10 of=10 median_ms=243 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=start_probe ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0 port_flag_passing=0 port_flag_open=0 pipe_holder_passing=0 pipe_holder_open=0 table_unread=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=start_probe ok=10 of=10 median_ms=584 relay=pipe_relay
control candidate=cdp machine=hosted-win browser=chrome step=driver_killed_pipe let_through=yes browser_after=gone connections=1 bytes=452 seen=yes
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=driver_killed_pipe ok=20 of=20 median_ms=278
control candidate=cdp machine=hosted-win browser=chrome step=driver_killed_port let_through=yes browser_after=alive connections=1 bytes=452 seen=yes
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=driver_killed_port ok=0 of=20 median_ms=0 bytes=9040
control candidate=cdp machine=hosted-win browser=chrome step=driver_killed_pipe_relay let_through=yes browser_after=gone connections=1 bytes=452 seen=yes
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=driver_killed_pipe_relay ok=20 of=20 median_ms=264 relay=pipe_relay
control candidate=cdp machine=hosted-win browser=chrome step=driver_killed_port_relay let_through=yes browser_after=alive connections=1 bytes=452 seen=yes
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=driver_killed_port_relay ok=20 of=20 median_ms=0 relay=port_relay
control candidate=cdp machine=hosted-win browser=edge step=driver_killed_pipe let_through=yes browser_after=gone connections=1 bytes=466 seen=yes
cell candidate=cdp machine=hosted-win browser=edge major=156 step=driver_killed_pipe ok=20 of=20 median_ms=323
control candidate=cdp machine=hosted-win browser=edge step=driver_killed_port let_through=yes browser_after=alive connections=1 bytes=466 seen=yes
cell candidate=cdp machine=hosted-win browser=edge major=156 step=driver_killed_port ok=0 of=20 median_ms=0 bytes=9320
control candidate=cdp machine=hosted-win browser=edge step=driver_killed_pipe_relay let_through=yes browser_after=gone connections=1 bytes=466 seen=yes
cell candidate=cdp machine=hosted-win browser=edge major=156 step=driver_killed_pipe_relay ok=20 of=20 median_ms=329 relay=pipe_relay
control candidate=cdp machine=hosted-win browser=edge step=driver_killed_port_relay let_through=yes browser_after=alive connections=1 bytes=466 seen=yes
cell candidate=cdp machine=hosted-win browser=edge major=156 step=driver_killed_port_relay ok=20 of=20 median_ms=0 relay=port_relay
cell candidate=cdp machine=hosted-win browser=chrome major=0 step=org_debugging ok=10 of=10 median_ms=0 relay=pipe_relay control_before=reply value_read=0 page_named=yes control_after=reply
cell candidate=cdp machine=hosted-win browser=edge major=0 step=org_debugging ok=10 of=10 median_ms=0 relay=pipe_relay control_before=reply value_read=0 page_named=yes control_after=reply
cell candidate=cdp machine=hosted-win browser=edge major=0 step=org_proxy ok=10 of=10 median_ms=0 relay=pipe_relay control_before=reply value_read=direct page_named=yes control_after=reply
run candidate=cdp machine=hosted-win browser=chrome step=relay_port_free i=1 result=ok cause=- watched=yes gone_ms=251 free_ms=1259 max_gap_ms=5 version=Chrome/156.0.8078.80 left=no left_before=0 left_after=0 ended_by=self
run candidate=cdp machine=hosted-win browser=chrome step=relay_port_free i=2 result=ok cause=- watched=yes gone_ms=268 free_ms=1304 max_gap_ms=5 version=Chrome/156.0.8078.80 left=no left_before=0 left_after=0 ended_by=self
run candidate=cdp machine=hosted-win browser=chrome step=relay_port_free i=3 result=ok cause=- watched=yes gone_ms=265 free_ms=1282 max_gap_ms=5 version=Chrome/156.0.8078.80 left=no left_before=0 left_after=0 ended_by=self
run candidate=cdp machine=hosted-win browser=chrome step=relay_port_free i=4 result=ok cause=- watched=yes gone_ms=262 free_ms=1303 max_gap_ms=5 version=Chrome/156.0.8078.80 left=no left_before=0 left_after=0 ended_by=self
run candidate=cdp machine=hosted-win browser=chrome step=relay_port_free i=5 result=ok cause=- watched=yes gone_ms=209 free_ms=1223 max_gap_ms=5 version=Chrome/156.0.8078.80 left=no left_before=0 left_after=0 ended_by=self
run candidate=cdp machine=hosted-win browser=chrome step=relay_port_free i=6 result=ok cause=- watched=yes gone_ms=263 free_ms=1301 max_gap_ms=5 version=Chrome/156.0.8078.80 left=no left_before=0 left_after=0 ended_by=self
run candidate=cdp machine=hosted-win browser=chrome step=relay_port_free i=7 result=ok cause=- watched=yes gone_ms=303 free_ms=1317 max_gap_ms=6 version=Chrome/156.0.8078.80 left=no left_before=0 left_after=0 ended_by=self
run candidate=cdp machine=hosted-win browser=chrome step=relay_port_free i=8 result=ok cause=- watched=yes gone_ms=229 free_ms=1251 max_gap_ms=5 version=Chrome/156.0.8078.80 left=no left_before=0 left_after=0 ended_by=self
run candidate=cdp machine=hosted-win browser=chrome step=relay_port_free i=9 result=ok cause=- watched=yes gone_ms=247 free_ms=1273 max_gap_ms=5 version=Chrome/156.0.8078.80 left=no left_before=0 left_after=0 ended_by=self
run candidate=cdp machine=hosted-win browser=chrome step=relay_port_free i=10 result=ok cause=- watched=yes gone_ms=305 free_ms=1334 max_gap_ms=6 version=Chrome/156.0.8078.80 left=no left_before=0 left_after=0 ended_by=self
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=relay_port_free ok=10 of=10 median_ms=262 relay=pipe_relay
run candidate=cdp machine=hosted-win browser=edge step=relay_port_free i=1 result=ok cause=- watched=yes gone_ms=329 free_ms=1372 max_gap_ms=6 version=Edg/156.0.4314.58 left=no left_before=0 left_after=0 ended_by=self
run candidate=cdp machine=hosted-win browser=edge step=relay_port_free i=2 result=ok cause=- watched=yes gone_ms=314 free_ms=1358 max_gap_ms=6 version=Edg/156.0.4314.58 left=no left_before=0 left_after=0 ended_by=self
run candidate=cdp machine=hosted-win browser=edge step=relay_port_free i=3 result=ok cause=- watched=yes gone_ms=5037 free_ms=7963 max_gap_ms=15 version=Edg/156.0.4314.58 left=no left_before=0 left_after=0 ended_by=guard ended_ms=6950
run candidate=cdp machine=hosted-win browser=edge step=relay_port_free i=4 result=ok cause=- watched=yes gone_ms=548 free_ms=1557 max_gap_ms=7 version=Edg/156.0.4314.58 left=no left_before=0 left_after=0 ended_by=self
run candidate=cdp machine=hosted-win browser=edge step=relay_port_free i=5 result=ok cause=- watched=yes gone_ms=318 free_ms=1336 max_gap_ms=7 version=Edg/156.0.4314.58 left=no left_before=0 left_after=0 ended_by=self
run candidate=cdp machine=hosted-win browser=edge step=relay_port_free i=6 result=ok cause=- watched=yes gone_ms=307 free_ms=1320 max_gap_ms=6 version=Edg/156.0.4314.58 left=no left_before=0 left_after=0 ended_by=self
run candidate=cdp machine=hosted-win browser=edge step=relay_port_free i=7 result=ok cause=- watched=yes gone_ms=353 free_ms=1366 max_gap_ms=8 version=Edg/156.0.4314.58 left=no left_before=0 left_after=0 ended_by=self
run candidate=cdp machine=hosted-win browser=edge step=relay_port_free i=8 result=ok cause=- watched=yes gone_ms=305 free_ms=1318 max_gap_ms=7 version=Edg/156.0.4314.58 left=no left_before=0 left_after=0 ended_by=self
run candidate=cdp machine=hosted-win browser=edge step=relay_port_free i=9 result=ok cause=- watched=yes gone_ms=329 free_ms=1356 max_gap_ms=6 version=Edg/156.0.4314.58 left=no left_before=0 left_after=0 ended_by=self
run candidate=cdp machine=hosted-win browser=edge step=relay_port_free i=10 result=ok cause=- watched=yes gone_ms=306 free_ms=1335 max_gap_ms=6 version=Edg/156.0.4314.58 left=no left_before=0 left_after=0 ended_by=self
cell candidate=cdp machine=hosted-win browser=edge major=156 step=relay_port_free ok=10 of=10 median_ms=323 relay=pipe_relay
same_site pref=yes rule=yes start_writes_setting=yes
verdict executor=cdp version=0.9.1 form=pipe product=yes relay=yes library=protocol_types
```

Which code each set of lines ran at (CP-126 E8 (b)). The final code that the runs measured is that
of 39165925: the commits after it change the trial's tests and doc comments, its README,
CHANGELOG.md and this file alone; each line of the trial's src and tests that they change is inside
a test module or a doc comment (CP-161). Each difference below is
between the code of the set and that code. CP-145 changed the row `relay_port_free` (its watch and
its decision), the check (classes of the counted processes, four keys, `table_unread` and the naming
line), the page reader (its third form) and the guard (an unread table); CP-148 lets the guard end
the run when its executor is gone.

| set | code | differences | can a result rest on a difference |
|---|---|---|---|
| `mac`: `round3` but its `relay_foreign` step, the nine rows | 5ad984ae (95d33beb and 269bac38 change tests alone) | the kill rows' end after the watch; the `org_` page rule and its third form; the row; the check's classes and keys; an unread table; the guard's rule (it acts only when its executor is gone); tests | no: none of these is a kill row or an `org_` cell, and each of their check lines counts 0 in `port_flag` and `pipe_holder`, so no class could change it |
| `mac`: `isolation`, `relay`, `reach`, `ways`, the four `driver_killed_*` forms, `relay_foreign` (its third call; the two earlier calls at 5ad984ae stand in the text below the table) | bed6b1aa | the page rule and its third form; the row; the check's classes and keys; an unread table; the guard's rule (it acts only when its executor is gone); tests | no, for the same reasons; the four kill forms are not the row |
| `mac`: `relay_port_free` | 39165925 (the call at the guard's fix) | none | none |
| hosted-win: every line but the kill rows and three `org_` cells | c49bd8ef (run 43368925419) | `procs=`; the kill rows' end; the page rule; the row; the check's classes and keys; an unread table; the guard's rule (it acts only when its executor is gone); tests | no: each check line counts 0 in the two rules, and Chrome's `org_proxy` page named its setting by the first form |
| hosted-win: the four `driver_killed_*` forms | bed6b1aa (run 43374121394) | the page rule; the row; the check's classes; an unread table; the guard's rule (it acts only when its executor is gone); tests | no: the four forms are not the row |
| hosted-win: `relay_port_free` | 699bd411 (run 43398919479) | the guard's record and the line's `ended_by` | no: what the guard does is the same; only its record and the label changed |
| hosted-win: Chrome's `org_debugging` | 2f85bed0 (run 43384012622) | the page rule's third form; the row; the check's classes; an unread table; the guard's rule; a test | no: its page named the setting by the first form |
| hosted-win: Edge's `org_debugging` and `org_proxy` | 2dc61025 (run 43389541875) | the row; the check's classes; an unread table; the guard's rule (it acts only when its executor is gone); tests | no: no check reads in an `org_` cell |
| hosted-mac: every pipe-form cell and `relay_port_free` but Edge's `start_probe` | 39165925 (run 43407107710) | none | none |
| hosted-mac: Edge's `start_probe` | 42f1526a (run 43416830488, the cell alone) | none: 42f1526a changes this file alone | none |
| hosted-mac: the four `driver_killed_*` forms, frame_doc on the port, the two `way` lines of frame_doc on the pipe (run 43407107710 printed the same bytes), the four `org_` cells | 5ad984ae (run 43369346664) | the kill rows' end; the page rule's third form; the row; the check's classes and keys; an unread table; the guard; tests | no for the `org_` cells (each page named its setting) and the port-form control; the kill rows rest on the job's scan at its end, ruled before |
| the hosted rows of runs 43313951706 and 43381911741, the `org_` cells of run 43347520562 | e72f0835, 590a306d, 037b242c | earlier code | no line of the block comes from them |

The guard's rule of CP-148 acts in the relay forms of `driver_killed` too (`driver_killed_pipe_relay`,
`driver_killed_port_relay`): after the driver's kill it ends what still runs after 3 s. In those
forms the relay runs in the executor's process and dies with it, so no tunnel can carry a byte after
the kill, and the guard's rule then ends what still runs. The kept lines of those forms (0 bytes in
every kill) stand either way; no run of this round measured them again.

Which browser a `kill` line is (CP-133 (i)). No `kill` line of this file names its browser; the
trial prints the key `browser=` from U0.3's second pull request on. Until a run prints it, a `kill`
line of the second block, and of the third block on `mac` and `hosted-mac`, is the browser's of the
`cell` line right after it, and in the third block the four `hosted-win` lines after Chrome's
`frame_doc` lines are Chrome's and the four after Edge's are Edge's.

The 45 `cell` lines of web-executor.md are of run 43103175522: the first round's executor on the
library's port connection. No run of this round measured them again.

`relay_port_free` (CP-145 (1)). The row asserts A13.7's sentence and promises no seconds: a kill is
ok, a miss (`free_first`, `tree_left`, `left_behind`) or open (`one_pass`), at any load; the watch
ends nothing before its cap of 300 s. The first call of the changed row on `mac` (abe40e13, load
219.76 to 119.30) read ok 6, a miss `tree_left` 2 (the browser's first process still ran at 300 s)
and open `one_pass` 2. Its cause, read live with tools that only read (CP-148): when the driver
dies hard the browser reads its pipe closed in the same second (`Connection terminated while
reading from pipe`), begins its shutdown, and its helpers end; its own process then waits idle (a
shutdown watchdog armed, the main thread in its run loop, 0% CPU) and does not end. Nothing else
held an end of its pipe, it held no socket, and it had no child; 70cb7488's code shows the same
(its control call ended 2 of 10 within its 30 s, the other eight in that state), and at the head one
browser ended by itself at 234 s. So the guard now ends the run when its executor is gone (README,
The guard of the relay's port). The lines of the calls at the fix replace the cell's lines in the
block (E9, I4): `mac`, Chrome, 39165925, load 145.25 to 197.71: ok 10 of 10, each kill
`ended_by=guard` (the first process still ran after the 3 s grace and did not end when asked; it was
ended at about 5 s), the tree gone at 4.5 to 5.2 s and the port free about 1 s later, nothing left,
the longest pass 264 ms; hosted-win, Chrome and Edge, run 43398919479: ok 10 of 10 each, 19 kills
ended by themselves in 0.2 to 0.55 s and Edge's third by the guard at 5.0 s. hosted-mac, Chrome and Edge, run 43407107710: ok 10 of 10 each, every kill ended by itself in 0.09 to 0.34 s, the port free about 1 s later.

The kept calls of the earlier row on `mac` stand here, read as CP-145 (4) reads them: the earlier
loop read the port before the tree in a pass, so a line with `gone_ms` below `free_ms` shows the
order up to the gap between the two reads of one pass. Ten kills each, with the 1-minute load:

| call | code | started | load before to after | earlier reading | ok | open, the watch ended with neither event | open, the row's own end not counted | miss | slowest end |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 5ad984ae | 05:23:11Z | 104.56 to 91.43 | 5 of 10 | 0 | 5 | 5 | 0 | 18.0 s |
| 2 | bed6b1aa | 06:00:06Z | 73.65 to 72.90 | 10 of 10 | 10 | 0 | 0 | 0 | 4.9 s |
| 3 | bed6b1aa | 06:03:25Z | 70.83 to 83.63 | 10 of 10 | 10 | 0 | 0 | 0 | 1.7 s |
| 4 | bed6b1aa | 06:15:48Z | 106.30 to 97.68 | 10 of 10 | 10 | 0 | 0 | 0 | 2.9 s |
| 5 | bed6b1aa (in the `driver` row) | 06:19:28Z | 115.30 to 101.67 | 10 of 10 | 10 | 0 | 0 | 0 | 18.7 s |
| 6 | bed6b1aa | 06:38:59Z | 101.67 to 98.78 | 10 of 10 | 10 | 0 | 0 | 0 | 9.2 s |

No kept line of `mac` is a miss under the rule. The first call's five lines with neither event are
open (the earlier watch ended at 30 s and the row then ended the browser itself), and its other five
show the order with no count of the row's own end. Four controls of 70cb7488's code passed at the
same loads, with the tree gone after up to 22.6 s. Could the five lines of call 1 be the fault of an
unread process table? No: in that code a failed read of the table before the kill left the tree
empty, which reads gone at the first pass, a failed read of one process's state reads it not
running, which is gone too, and the guard, given a failed read, ended and freed the port; each gives
a time where the five lines give `gone_ms=- free_ms=-`. No kept line records a failed read: that code
printed none. On the hosted Mac (run 43369346664, the earlier row) 18 of Chrome's 20 lines show the
order and 2 give both events in one millisecond, so that cell is open; its lines carry no left counts,
and what was left rests on the job's scan, which found none.

The check read. The kept check line of run 43369346664 (hosted-mac, Chrome, `signin_form`, run 7, at
the read after the start: `port_flag=1`, `pipe_holder=1`; the two later reads of the run 0) was made
under the earlier check: it names no process and carries no `*_passing` or `*_open` key, so by CP-145
(2) (h) it was no read of the two counts and its cell was open. Run 43407107710 read every pipe-form
cell of Chrome and Edge on the hosted Mac under the new check (each counted process classed and named
by a `check_named` line): 1,776 reads, 592 of them after a start, and none counted a process in
`port_flag` or `pipe_holder`, so no `check_named` line was printed and the cell's line is replaced
(E9, I4). The single read of run 43369346664 was not seen again.

Lines that a later call replaced in the block (A13.7, CP-145 (5) and (9), CP-158, CP-161 F4). The
block holds one call's lines for a cell, the newest call's; these six stand here. `mac`, Chrome,
`relay_foreign`: the cell's lines in the block are those of its third call, at bed6b1aa
(cp126/mac/relay_foreign.2.txt, 05:56:18Z, load 90.19 to 72.20). Its two earlier calls, ten runs
each, at 5ad984ae: the showing for CP-126 E1 (cp126/mac/relay_foreign.txt, 05:18:41Z, load 85.43
to 108.14) and the step inside the set `round3` (cp126/mac/round3.txt, 05:27:25Z, load 85.03 to
74.26), in that order:

- `check machine=mac browser=chrome form=pipe_relay step=relay_foreign ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0`
- `cell candidate=cdp machine=mac browser=chrome major=153 step=relay_foreign ok=10 of=10 median_ms=16 relay=pipe_relay`
- `check machine=mac browser=chrome form=pipe_relay step=relay_foreign ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0`
- `cell candidate=cdp machine=mac browser=chrome major=153 step=relay_foreign ok=10 of=10 median_ms=26 relay=pipe_relay`

hosted-mac, `frame_doc` on the pipe: the cell's check lines in the block are those of run
43407107710. The two check lines of run 43369346664, Chrome's and then Edge's, were made under the
earlier check, without the keys of CP-145 (2), and count 0 in every rule:

- `check machine=hosted-mac browser=chrome form=pipe_relay step=frame_doc ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0`
- `check machine=hosted-mac browser=edge form=pipe_relay step=frame_doc ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0`

The two `way` lines of `frame_doc` on the pipe stand in the block at the place of run 43369346664;
run 43407107710 printed the same bytes for both browsers.

hosted-win Edge's `org_debugging` (CP-145 (6)). The page reader has a third form: the name's letters
from the start of a line with only white space or line breaks between them, ended by the value that
the job wrote and read back as the page prints it. On the kept page of run 43384012622 it names the
setting (`how=joined`); on the nine pages of run 43347520562 it gives the answers of the two earlier
forms. Run 43389541875, Edge's two `org_` cells alone: both controls replied, the key read back 0 and
`direct`, the page named each setting, and each cell is `ok=10 of=10`.

The verdict (CP-158). The block reads `product=yes relay=yes`. Hosted-mac Edge's `start_probe` was
run twice. Run 43407107710 printed `notrun candidate=cdp machine=hosted-mac browser=edge
step=start_probe cause=browser_start`: its runs 1 and 2 were ok, their starts answered in 0.9 and
1.0 s; at run 3 the start gave no reply on the pipe within the 30 s of `FIRST_REPLY_WAIT`, and one
Edge process was found left at that failed start's end and ended. Run 43416830488, the cell alone at
42f1526a, printed `ok=10 of=10`, its starts answered in 0.9 to 5.7 s (5.7 s for the call's first,
its setup included). The first call was cut short by a fault of its start step, so by A13.7's
sentence the cell was run once more, and the later call's lines replace the cell's lines in the
block while the earlier ones stand here (E9, I4; CP-145 (1) (e)). Two points go to the planner and
are not built in this unit: what the product's executor does with a start that gives no reply
within its bound (the tree of that start ended by the rule of CP-148, the person told, whether one
more start is tried), and whether 30 s is the right bound for a first start on a slow machine, with
these two runs' start times as the first numbers. Every other deciding cell of the five holds, the
check lines count 0 in every rule that decides, and no run line has `left_after` above 0.

What the third round did not measure (CP-126 E8 (c)). No cell ran on an organisation's managed
machine: the job wrote each `org_` policy itself on a hosted runner, and no management profile or
domain delivered one. Every browser started headless, so no cell measured a browser with a window.
Every run started with a new profile in a scratch directory, never a person's own profile. Every
process ran under the one account of the machine or of the job, so no cell measured a peer of
another account; the cell of `relay_foreign` under another account is with the planner.
