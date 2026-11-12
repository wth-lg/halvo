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

Unit U0.7c runs A13.7's third round under rulings CP-89, CP-93 and CP-95 (2): the trial program
starts the browser as a direct child on its debugging pipe and is its one client, and the relay is
its only route, in all five cells. `tools/r3-trials/web/README.md` ("The third round") says how the
start, the wire, the relay's peer check, the guard of the relay's port, the checks of a session and
each row work. The lines and verdicts of the first two rounds above stay as they were.

What ran, at the unit's code commit e72f083 (`mac`: c296a1c, which differs from it in Windows code
alone): every row with `run.sh ... --round 3 --relay` (the driver row with `--round 3`), 10 runs
per cell and 20 kills per driver form: `--rows round3`, the nine rows, `--rows isolation`,
`--rows relay` (`list_refuse`, `list_hops`, `list_close`), `--rows reach`, `--rows ways --step
frame_doc` (and with `--channel port`, a control), `--rows driver` (four forms, `relay_port_free`)
and `--rows org`. On `mac` (Chrome 153.0.8010.48) each row set was one call of the run helper on a
lane that records no time, from 2026-11-12T20:47:33Z to 2026-11-12T21:36:43Z, so `median_ms` there is no timed measure. The hosted
cells ran in run the sixth scratch run (pending): its lines join this block when it ends. A `check` line's `form=` comes from its call's `start` line.

Choices that the text leaves, each with its reason in the README: the probe's origin
`http://halvo-start-probe.invalid/`; `FIRST_REPLY_WAIT` 30 s, `PROBE_WAIT` 10 s, `MAX_OPEN` 512 and
`MAX_RECORD` 4096; a `check` line sums each rule of A11.1's check over a cell's runs and three reads
(after the start, after the row, after the stop). A hard kill closes the relay's listener with the
killed process at once while the browser's tree ends seconds later (with no guard,
`relay_port_free` read the port free at 1 ms and the browser gone at 2476 to 4036 ms, 3 of 3 runs
on `mac`), so a guard process holds a copy of the listener until the tree is gone. `org_policy` on
`mac` is a `notrun` line (`cause=no_policy_place`): Chrome on macOS takes a policy from the system's
managed preferences alone. The hosted jobs write it for their life and remove it after each cell.
The `org_` cells carry `major=0`: the start fails before the browser gives a version.

Flags of the trial that the product does not carry (A13.3, CP-89 (g)): `--use-mock-keychain` (the
product's browser keeps its cookies with the system's key store, A8.3); `--isolate-origins=` of the
six rows (it names the payment site, CP-93 (5)); `--remote-debugging-port=0` of the port control;
the resolver rule's map of `*.test` to loopback (the fixture's names). `same_site_pref` sets the
profile's flags entry `isolate-origins` with its origin list (Local State
`browser.enabled_labs_experiments`, `browser.enabled_labs_experiments_origin_lists`).

The four open facts of CP-93: written when the hosted cells are in.

What the lines show and cannot show: written when the hosted cells are in.

```text
check machine=mac browser=chrome form=pipe_relay step=pipe_inherit ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=pipe_inherit ok=10 of=10 median_ms=11 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=relay_foreign ok=9 of=10 port_flag=0 tcp_listen=0 udp_wide=1 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=relay_foreign ok=10 of=10 median_ms=32 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=start_probe ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=start_probe ok=10 of=10 median_ms=198 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=same_site_pref ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=same_site_pref ok=10 of=10 median_ms=348 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=same_site_rule ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=same_site_rule ok=10 of=10 median_ms=54 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=signin_form ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=signin_form ok=10 of=10 median_ms=284 relay=pipe_relay
relay_cell machine=mac browser=chrome form=pipe_relay step=signin_form ok=10 of=10
check machine=mac browser=chrome form=pipe_relay step=table_read ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=table_read ok=10 of=10 median_ms=237 relay=pipe_relay
relay_cell machine=mac browser=chrome form=pipe_relay step=table_read ok=10 of=10
check machine=mac browser=chrome form=pipe_relay step=form_fill ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=form_fill ok=10 of=10 median_ms=422 relay=pipe_relay
relay_cell machine=mac browser=chrome form=pipe_relay step=form_fill ok=10 of=10
check machine=mac browser=chrome form=pipe_relay step=payment_frame ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=payment_frame ok=10 of=10 median_ms=412 relay=pipe_relay
relay_cell machine=mac browser=chrome form=pipe_relay step=payment_frame ok=10 of=10
check machine=mac browser=chrome form=pipe_relay step=file_upload ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=file_upload ok=10 of=10 median_ms=376 relay=pipe_relay
relay_cell machine=mac browser=chrome form=pipe_relay step=file_upload ok=10 of=10
check machine=mac browser=chrome form=pipe_relay step=session_expiry ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=session_expiry ok=10 of=10 median_ms=365 relay=pipe_relay
relay_cell machine=mac browser=chrome form=pipe_relay step=session_expiry ok=10 of=10
check machine=mac browser=chrome form=pipe_relay step=changed_page_1 ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=changed_page_1 ok=10 of=10 median_ms=456 relay=pipe_relay
relay_cell machine=mac browser=chrome form=pipe_relay step=changed_page_1 ok=10 of=10
check machine=mac browser=chrome form=pipe_relay step=block_request ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=block_request ok=10 of=10 median_ms=361 relay=pipe_relay
relay_cell machine=mac browser=chrome form=pipe_relay step=block_request ok=10 of=10
check machine=mac browser=chrome form=pipe_relay step=credential_mark ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=credential_mark ok=10 of=10 median_ms=347 relay=pipe_relay
relay_cell machine=mac browser=chrome form=pipe_relay step=credential_mark ok=10 of=10
check machine=mac browser=chrome form=pipe_relay step=payment_frame_isolated ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=payment_frame_isolated ok=10 of=10 median_ms=438 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=changed_page_1_isolated ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=changed_page_1_isolated ok=10 of=10 median_ms=415 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=block_popup ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=block_popup ok=10 of=10 median_ms=59 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=block_frame_post ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=block_frame_post ok=10 of=10 median_ms=101 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=block_websocket ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=block_websocket ok=10 of=10 median_ms=52 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=cert_error ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=cert_error ok=10 of=10 median_ms=350 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=list_refuse ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=list_refuse ok=10 of=10 median_ms=2424 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=list_hops ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=list_hops ok=10 of=10 median_ms=1433 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=list_close ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=list_close ok=10 of=10 median_ms=1490 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=reach_page ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=reach_page ok=10 of=10 median_ms=3241 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=reach_opened ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=reach_opened ok=10 of=10 median_ms=3256 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=reach_frame ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=reach_frame ok=10 of=10 median_ms=3272 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=reach_worker ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=reach_worker ok=10 of=10 median_ms=3221 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=reach_shared ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=reach_shared ok=10 of=10 median_ms=3219 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=reach_service ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=reach_service ok=10 of=10 median_ms=3219 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=frame_doc ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
way row=frame_doc form=pipe_relay machine=mac browser=chrome closed=yes runs=10 told=10
check machine=mac browser=chrome form=port_relay step=frame_doc ok=0 of=10 port_flag=91 tcp_listen=20 udp_wide=0 active_port_file=20 executor_listener=0 pipe_holder=0
way row=frame_doc form=port_relay machine=mac browser=chrome closed=yes runs=10 told=10
control candidate=cdp machine=mac browser=chrome step=driver_killed_pipe let_through=yes browser_after=gone connections=1 bytes=458 seen=yes
kill machine=mac form=pipe kills=20 arrived=0
cell candidate=cdp machine=mac browser=chrome major=153 step=driver_killed_pipe ok=20 of=20 median_ms=115
control candidate=cdp machine=mac browser=chrome step=driver_killed_port let_through=yes browser_after=alive connections=1 bytes=458 seen=yes
kill machine=mac form=port kills=20 arrived=20
cell candidate=cdp machine=mac browser=chrome major=153 step=driver_killed_port ok=0 of=20 median_ms=0 bytes=9160
control candidate=cdp machine=mac browser=chrome step=driver_killed_pipe_relay let_through=yes browser_after=gone connections=1 bytes=458 seen=yes
kill machine=mac form=pipe_relay kills=20 arrived=0
cell candidate=cdp machine=mac browser=chrome major=153 step=driver_killed_pipe_relay ok=20 of=20 median_ms=146 relay=pipe_relay
control candidate=cdp machine=mac browser=chrome step=driver_killed_port_relay let_through=yes browser_after=alive connections=1 bytes=458 seen=yes
kill machine=mac form=port_relay kills=20 arrived=0
cell candidate=cdp machine=mac browser=chrome major=153 step=driver_killed_port_relay ok=20 of=20 median_ms=0 relay=port_relay
cell candidate=cdp machine=mac browser=chrome major=153 step=relay_port_free ok=10 of=10 median_ms=4217 relay=pipe_relay
notrun candidate=cdp machine=mac browser=chrome step=org_debugging cause=no_policy_place
notrun candidate=cdp machine=mac browser=chrome step=org_proxy cause=no_policy_place
same_site pref=open rule=open start_writes_setting=open
verdict executor=cdp version=0.9.1 form=pipe product=open relay=open library=protocol_types
```
