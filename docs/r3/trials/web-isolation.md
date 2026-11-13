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

Unit U0.7c runs A13.7's third round under CP-89, CP-93, CP-95 (2), CP-126, CP-137 and CP-140: the
trial program starts the browser on its debugging pipe as its one client, the relay its only route,
in all five cells.

What ran: each row set (`round3`, the nine rows, `isolation`, `relay`, `reach`, `ways --step
frame_doc` with a port control, `driver`, `org`) with `run.sh ... --round 3 --relay` (`driver`
without `--relay`), 10 runs per cell, 20 kills per driver form. `mac` (Chrome 153.0.8010.48) ran on
an untimed helper lane from 2026-11-13T05:18:41Z to 06:40:08Z inside no load bound, so its
`median_ms` is no timed measure. hosted-mac (Chrome 157.0.8103.39, Edge 156.0.4314.58) ran in run
43369346664; hosted-win (Chrome 156.0.8078.80, Edge 156.0.4314.58) in run 43368925419, its kill rows
in run 43374121394 and three `org_` cells in run 43384012622. Every way out of a run ends the
processes that the run left (CP-126 E1, the README). Each run line prints `left_before` and
`left_after`, and no line of the block has `left_after` above 0.

Choices (reasons in the README): the probe's origin `http://halvo-start-probe.invalid/`;
`FIRST_REPLY_WAIT` 30 s, `PROBE_WAIT` 10 s, `MAX_OPEN` 512, `MAX_RECORD` 4096; a `check` line sums
A11.1's rules over a cell's runs and three reads; a guard holds the relay's listener until the tree
is gone.

`org_policy`: `mac` is `notrun cause=no_policy_place`. A hosted cell counts only with a start
without the setting that replied before the write and after the removal, the value read back (the
key; `defaults read` of the property list), and the browser's policy page, printed headless with no
debugging channel and read as text, naming the setting as a whole word (`r3_web::policy::named`); a
page that does not name it holds the cell open (`notrun cause=policy_not_seen`). Run 43347520562's
reader took the name as a part of the text; the fixed rule agrees on its nine saved pages. On
hosted-win pypdf (BSD-3-Clause) reads the page: Edge's lists `RemoteDebuggingAllowed` with the value
false, but its text breaks the name and joins the last letter to the value, so that cell is notrun.

The verdict line reads `product=no` by two cells, and neither shows the product failing: one is a
timed row on a loaded Mac (`relay_port_free`, after the block), the other one read at a start that
the next two reads of the same run no longer find. Two points with the coordinator hold it open.

CP-93's open facts. (1) On Windows the policy key grants `Authenticated Users` `ReadKey`. On macOS
the property list is root:wheel with mode -rw-r--r--, and the page named the policy when the job's
own account, which can use sudo, read it; no line reads the page as an account without administrator
rights. (2) Windows' exclusive bind holds: `relay_port_free` 20 of 20 on hosted-win. (3) UDP beyond
loopback: 0 in every pipe `check` line of the block. (4) Edge: every deciding pipe cell of Edge is
ok but hosted-win's `org_debugging`, which is notrun.

```text
hosted machine=hosted-mac run=43369346664 sha=583e198338bbeab60fc3a2ed526e3c4405bd16fd
hosted machine=hosted-win run=43368925419 sha=f09e0d99ee779bc2d0b01a59f487eb4381a49921
hosted machine=hosted-win run=43374121394 sha=873749da0de0b0d1880702d13968a7f5ce900114 steps=driver_killed_pipe,driver_killed_pipe_relay,driver_killed_port,driver_killed_port_relay,relay_port_free
hosted machine=hosted-win run=43384012622 sha=d684140639bb245bb34c4b6558dcf88fafcf1727 steps=chrome:org_debugging,edge:org_debugging,edge:org_proxy
check machine=mac browser=chrome form=pipe_relay step=relay_foreign ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=relay_foreign ok=10 of=10 median_ms=16 relay=pipe_relay
cell candidate=cdp machine=mac browser=chrome major=153 step=relay_port_free ok=5 of=10 median_ms=12273 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=pipe_inherit ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=pipe_inherit ok=10 of=10 median_ms=10 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=relay_foreign ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=relay_foreign ok=10 of=10 median_ms=26 relay=pipe_relay
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
cell candidate=cdp machine=mac browser=chrome major=153 step=relay_port_free ok=10 of=10 median_ms=3517 relay=pipe_relay
check machine=mac browser=chrome form=pipe_relay step=relay_foreign ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=mac browser=chrome major=153 step=relay_foreign ok=10 of=10 median_ms=16 relay=pipe_relay
cell candidate=cdp machine=mac browser=chrome major=153 step=relay_port_free ok=10 of=10 median_ms=824 relay=pipe_relay
cell candidate=cdp machine=mac browser=chrome major=153 step=relay_port_free ok=10 of=10 median_ms=625 relay=pipe_relay
cell candidate=cdp machine=mac browser=chrome major=153 step=relay_port_free ok=10 of=10 median_ms=2204 relay=pipe_relay
cell candidate=cdp machine=mac browser=chrome major=153 step=relay_port_free ok=10 of=10 median_ms=3330 relay=pipe_relay
check machine=hosted-mac browser=chrome form=pipe_relay step=pipe_inherit ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=pipe_inherit ok=10 of=10 median_ms=6 relay=pipe_relay
check machine=hosted-mac browser=chrome form=pipe_relay step=relay_foreign ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=relay_foreign ok=10 of=10 median_ms=9 relay=pipe_relay
check machine=hosted-mac browser=chrome form=pipe_relay step=start_probe ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=start_probe ok=10 of=10 median_ms=438 relay=pipe_relay
check machine=hosted-mac browser=chrome form=pipe_relay step=same_site_pref ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=same_site_pref ok=10 of=10 median_ms=725 relay=pipe_relay
check machine=hosted-mac browser=chrome form=pipe_relay step=same_site_rule ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=same_site_rule ok=10 of=10 median_ms=55 relay=pipe_relay
check machine=hosted-mac browser=chrome form=pipe_relay step=signin_form ok=9 of=10 port_flag=1 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=1
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=signin_form ok=10 of=10 median_ms=739 relay=pipe_relay
relay_cell machine=hosted-mac browser=chrome form=pipe_relay step=signin_form ok=10 of=10
check machine=hosted-mac browser=chrome form=pipe_relay step=table_read ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=table_read ok=10 of=10 median_ms=650 relay=pipe_relay
relay_cell machine=hosted-mac browser=chrome form=pipe_relay step=table_read ok=10 of=10
check machine=hosted-mac browser=chrome form=pipe_relay step=form_fill ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=form_fill ok=10 of=10 median_ms=923 relay=pipe_relay
relay_cell machine=hosted-mac browser=chrome form=pipe_relay step=form_fill ok=10 of=10
check machine=hosted-mac browser=chrome form=pipe_relay step=payment_frame ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=payment_frame ok=10 of=10 median_ms=849 relay=pipe_relay
relay_cell machine=hosted-mac browser=chrome form=pipe_relay step=payment_frame ok=10 of=10
check machine=hosted-mac browser=chrome form=pipe_relay step=file_upload ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=file_upload ok=10 of=10 median_ms=757 relay=pipe_relay
relay_cell machine=hosted-mac browser=chrome form=pipe_relay step=file_upload ok=10 of=10
check machine=hosted-mac browser=chrome form=pipe_relay step=session_expiry ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=session_expiry ok=10 of=10 median_ms=808 relay=pipe_relay
relay_cell machine=hosted-mac browser=chrome form=pipe_relay step=session_expiry ok=10 of=10
check machine=hosted-mac browser=chrome form=pipe_relay step=changed_page_1 ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=changed_page_1 ok=10 of=10 median_ms=835 relay=pipe_relay
relay_cell machine=hosted-mac browser=chrome form=pipe_relay step=changed_page_1 ok=10 of=10
check machine=hosted-mac browser=chrome form=pipe_relay step=block_request ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=block_request ok=10 of=10 median_ms=708 relay=pipe_relay
relay_cell machine=hosted-mac browser=chrome form=pipe_relay step=block_request ok=10 of=10
check machine=hosted-mac browser=chrome form=pipe_relay step=credential_mark ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=credential_mark ok=10 of=10 median_ms=752 relay=pipe_relay
relay_cell machine=hosted-mac browser=chrome form=pipe_relay step=credential_mark ok=10 of=10
check machine=hosted-mac browser=chrome form=pipe_relay step=payment_frame_isolated ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=payment_frame_isolated ok=10 of=10 median_ms=910 relay=pipe_relay
check machine=hosted-mac browser=chrome form=pipe_relay step=changed_page_1_isolated ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=changed_page_1_isolated ok=10 of=10 median_ms=977 relay=pipe_relay
check machine=hosted-mac browser=chrome form=pipe_relay step=block_popup ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=block_popup ok=10 of=10 median_ms=99 relay=pipe_relay
check machine=hosted-mac browser=chrome form=pipe_relay step=block_frame_post ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=block_frame_post ok=10 of=10 median_ms=257 relay=pipe_relay
check machine=hosted-mac browser=chrome form=pipe_relay step=block_websocket ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=block_websocket ok=10 of=10 median_ms=55 relay=pipe_relay
check machine=hosted-mac browser=chrome form=pipe_relay step=cert_error ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=cert_error ok=10 of=10 median_ms=390 relay=pipe_relay
check machine=hosted-mac browser=chrome form=pipe_relay step=list_refuse ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=list_refuse ok=10 of=10 median_ms=2689 relay=pipe_relay
check machine=hosted-mac browser=chrome form=pipe_relay step=list_hops ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=list_hops ok=10 of=10 median_ms=1616 relay=pipe_relay
check machine=hosted-mac browser=chrome form=pipe_relay step=list_close ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=list_close ok=10 of=10 median_ms=1809 relay=pipe_relay
check machine=hosted-mac browser=chrome form=pipe_relay step=reach_page ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=reach_page ok=10 of=10 median_ms=3522 relay=pipe_relay
check machine=hosted-mac browser=chrome form=pipe_relay step=reach_opened ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=reach_opened ok=10 of=10 median_ms=3607 relay=pipe_relay
check machine=hosted-mac browser=chrome form=pipe_relay step=reach_frame ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=reach_frame ok=10 of=10 median_ms=3492 relay=pipe_relay
check machine=hosted-mac browser=chrome form=pipe_relay step=reach_worker ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=reach_worker ok=10 of=10 median_ms=3469 relay=pipe_relay
check machine=hosted-mac browser=chrome form=pipe_relay step=reach_shared ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=reach_shared ok=10 of=10 median_ms=3393 relay=pipe_relay
check machine=hosted-mac browser=chrome form=pipe_relay step=reach_service ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=reach_service ok=10 of=10 median_ms=3419 relay=pipe_relay
check machine=hosted-mac browser=chrome form=pipe_relay step=frame_doc ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
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
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=relay_port_free ok=10 of=10 median_ms=167 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=pipe_inherit ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=pipe_inherit ok=10 of=10 median_ms=4 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=relay_foreign ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=relay_foreign ok=10 of=10 median_ms=6 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=start_probe ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=start_probe ok=10 of=10 median_ms=490 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=same_site_pref ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=same_site_pref ok=10 of=10 median_ms=753 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=same_site_rule ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=same_site_rule ok=10 of=10 median_ms=57 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=signin_form ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=signin_form ok=10 of=10 median_ms=729 relay=pipe_relay
relay_cell machine=hosted-mac browser=edge form=pipe_relay step=signin_form ok=10 of=10
check machine=hosted-mac browser=edge form=pipe_relay step=table_read ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=table_read ok=10 of=10 median_ms=729 relay=pipe_relay
relay_cell machine=hosted-mac browser=edge form=pipe_relay step=table_read ok=10 of=10
check machine=hosted-mac browser=edge form=pipe_relay step=form_fill ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=form_fill ok=10 of=10 median_ms=924 relay=pipe_relay
relay_cell machine=hosted-mac browser=edge form=pipe_relay step=form_fill ok=10 of=10
check machine=hosted-mac browser=edge form=pipe_relay step=payment_frame ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=payment_frame ok=10 of=10 median_ms=956 relay=pipe_relay
relay_cell machine=hosted-mac browser=edge form=pipe_relay step=payment_frame ok=10 of=10
check machine=hosted-mac browser=edge form=pipe_relay step=file_upload ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=file_upload ok=10 of=10 median_ms=933 relay=pipe_relay
relay_cell machine=hosted-mac browser=edge form=pipe_relay step=file_upload ok=10 of=10
check machine=hosted-mac browser=edge form=pipe_relay step=session_expiry ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=session_expiry ok=10 of=10 median_ms=951 relay=pipe_relay
relay_cell machine=hosted-mac browser=edge form=pipe_relay step=session_expiry ok=10 of=10
check machine=hosted-mac browser=edge form=pipe_relay step=changed_page_1 ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=changed_page_1 ok=10 of=10 median_ms=1018 relay=pipe_relay
relay_cell machine=hosted-mac browser=edge form=pipe_relay step=changed_page_1 ok=10 of=10
check machine=hosted-mac browser=edge form=pipe_relay step=block_request ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=block_request ok=10 of=10 median_ms=997 relay=pipe_relay
relay_cell machine=hosted-mac browser=edge form=pipe_relay step=block_request ok=10 of=10
check machine=hosted-mac browser=edge form=pipe_relay step=credential_mark ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=credential_mark ok=10 of=10 median_ms=870 relay=pipe_relay
relay_cell machine=hosted-mac browser=edge form=pipe_relay step=credential_mark ok=10 of=10
check machine=hosted-mac browser=edge form=pipe_relay step=payment_frame_isolated ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=payment_frame_isolated ok=10 of=10 median_ms=1014 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=changed_page_1_isolated ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=changed_page_1_isolated ok=10 of=10 median_ms=996 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=block_popup ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=block_popup ok=10 of=10 median_ms=111 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=block_frame_post ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=block_frame_post ok=10 of=10 median_ms=231 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=block_websocket ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=block_websocket ok=10 of=10 median_ms=55 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=cert_error ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=cert_error ok=10 of=10 median_ms=428 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=list_refuse ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=list_refuse ok=10 of=10 median_ms=2856 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=list_hops ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=list_hops ok=10 of=10 median_ms=1738 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=list_close ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=list_close ok=10 of=10 median_ms=1999 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=reach_page ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=reach_page ok=10 of=10 median_ms=3718 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=reach_opened ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=reach_opened ok=10 of=10 median_ms=3932 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=reach_frame ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=reach_frame ok=10 of=10 median_ms=3783 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=reach_worker ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=reach_worker ok=10 of=10 median_ms=3712 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=reach_shared ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=reach_shared ok=10 of=10 median_ms=3625 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=reach_service ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=reach_service ok=10 of=10 median_ms=3615 relay=pipe_relay
check machine=hosted-mac browser=edge form=pipe_relay step=frame_doc ok=10 of=10 port_flag=0 tcp_listen=0 udp_wide=0 active_port_file=0 executor_listener=0 pipe_holder=0
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
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=relay_port_free ok=10 of=10 median_ms=266 relay=pipe_relay
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
control candidate=cdp machine=hosted-win browser=chrome step=driver_killed_pipe let_through=yes browser_after=gone connections=1 bytes=452 seen=yes
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=driver_killed_pipe ok=20 of=20 median_ms=278
control candidate=cdp machine=hosted-win browser=chrome step=driver_killed_port let_through=yes browser_after=alive connections=1 bytes=452 seen=yes
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=driver_killed_port ok=0 of=20 median_ms=0 bytes=9040
control candidate=cdp machine=hosted-win browser=chrome step=driver_killed_pipe_relay let_through=yes browser_after=gone connections=1 bytes=452 seen=yes
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=driver_killed_pipe_relay ok=20 of=20 median_ms=264 relay=pipe_relay
control candidate=cdp machine=hosted-win browser=chrome step=driver_killed_port_relay let_through=yes browser_after=alive connections=1 bytes=452 seen=yes
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=driver_killed_port_relay ok=20 of=20 median_ms=0 relay=port_relay
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=relay_port_free ok=10 of=10 median_ms=285 relay=pipe_relay
control candidate=cdp machine=hosted-win browser=edge step=driver_killed_pipe let_through=yes browser_after=gone connections=1 bytes=466 seen=yes
cell candidate=cdp machine=hosted-win browser=edge major=156 step=driver_killed_pipe ok=20 of=20 median_ms=323
control candidate=cdp machine=hosted-win browser=edge step=driver_killed_port let_through=yes browser_after=alive connections=1 bytes=466 seen=yes
cell candidate=cdp machine=hosted-win browser=edge major=156 step=driver_killed_port ok=0 of=20 median_ms=0 bytes=9320
control candidate=cdp machine=hosted-win browser=edge step=driver_killed_pipe_relay let_through=yes browser_after=gone connections=1 bytes=466 seen=yes
cell candidate=cdp machine=hosted-win browser=edge major=156 step=driver_killed_pipe_relay ok=20 of=20 median_ms=329 relay=pipe_relay
control candidate=cdp machine=hosted-win browser=edge step=driver_killed_port_relay let_through=yes browser_after=alive connections=1 bytes=466 seen=yes
cell candidate=cdp machine=hosted-win browser=edge major=156 step=driver_killed_port_relay ok=20 of=20 median_ms=0 relay=port_relay
cell candidate=cdp machine=hosted-win browser=edge major=156 step=relay_port_free ok=10 of=10 median_ms=346 relay=pipe_relay
cell candidate=cdp machine=hosted-win browser=chrome major=0 step=org_debugging ok=10 of=10 median_ms=0 relay=pipe_relay control_before=reply value_read=0 page_named=yes control_after=reply
notrun candidate=cdp machine=hosted-win browser=edge step=org_debugging cause=policy_not_seen reason=page_not_named
cell candidate=cdp machine=hosted-win browser=edge major=0 step=org_proxy ok=10 of=10 median_ms=0 relay=pipe_relay control_before=reply value_read=direct page_named=yes control_after=reply
same_site pref=yes rule=yes start_writes_setting=yes
verdict executor=cdp version=0.9.1 form=pipe product=no relay=yes library=protocol_types
```

Which code each set of lines ran at (CP-126 E8 (b)). The final code is 2f85bed0: the commits after
it change the README, CHANGELOG.md and this file alone. Each difference below is between the code
of the set and 2f85bed0.

| set | code | differences | can a result rest on a difference |
|---|---|---|---|
| `mac`: `relay_foreign` and `relay_port_free` (first runs), `round3`, the nine rows | 5ad984ae (95d33beb and 269bac38 change tests alone) | the kill rows end the run's own processes after their watch; an `org_` page that does not name the setting holds the cell open; two tests; the check names what it counts | no for every row but `relay_port_free`: none of them is a kill row or an `org_` cell, and the check counts as before. `relay_port_free`: its line before the watch's end is the same code; the end after the watch is new |
| `mac`: `isolation`, `relay`, `reach`, `ways`, `driver`, `relay_foreign` and `relay_port_free` again | bed6b1aa | the `org_` page rule; two tests; the check names what it counts | no: no `org_` cell runs on `mac`, and the check counts as before |
| hosted-win: every line but the kill rows and three `org_` cells | c49bd8ef (run 43368925419) | `procs=` on each run line; the kill rows' end; the `org_` page rule; the check names what it counts; tests | no: `procs=` is a printed count, the kill rows' end runs in the kill rows alone, and Chrome's `org_proxy` page named its setting, so the page rule gives the same answer |
| hosted-win: the kill rows of both browsers (`relay_port_free`, `driver_killed_*`) | bed6b1aa (run 43374121394) | the `org_` page rule; the check names what it counts; tests | no |
| hosted-win: Edge's two `org_` cells and Chrome's `org_debugging` | 2f85bed0 (run 43384012622) | none | none |
| hosted-win: the same three cells in run 43381911741 | 590a306d | none that a result rests on | no line of the block comes from it: my step printed Edge's new-tab page (as in run 43368925419) and read Chrome's page before the binary was built |
| hosted-mac: every line, with the four `org_` cells | 5ad984ae (run 43369346664) | the kill rows' end; the `org_` page rule; the check names what it counts; tests | yes for the kill rows: their run lines carry no left counts. By the coordinator's ruling they rest on the run's lines and on the scan at the job's end, which found 0 processes of the trial's runs. No for the `org_` cells: each page named its setting |
| the hosted rows of run 43313951706 (e72f0835) and the `org_` cells of run 43347520562 (037b242c) | e72f0835, 037b242c | the end of a run's own processes on every way out (CP-126 E1) | yes for every row: no line of the block comes from them now |

The 45 `cell` lines of web-executor.md are of run 43103175522: the first round's executor on the
library's port connection. No run of this round measured them again.

`relay_port_free` on `mac` (CP-140 (1)): the block holds every run of the head, six with 55 of 60
ok, and the collector reads the cell as it reads every cell. No run on `mac` was made inside a load
bound. The runs, each 10 kills, with the 1-minute load before and after:

| run | code | ok | load | tree gone after the kill |
|---|---|---|---|---|
| head, 05:23:11Z | 5ad984ae | 5 of 10 | 104.56 to 91.43 | 5.6 to 18.0 s in the five ok runs; in the five misses not within the 30 s watch |
| control, 05:48:12Z | 70cb7488 | 10 of 10 | 110.29 to 90.19 | 0.8 to 19.0 s |
| head, 06:00:06Z | bed6b1aa | 10 of 10 | 73.65 to 72.90 | 0.26 to 4.9 s |
| control, 06:01:50Z | 70cb7488 | 10 of 10 | 74.60 to 70.59 | 0.2 to 1.2 s |
| head, 06:03:25Z | bed6b1aa | 10 of 10 | 70.83 to 83.63 | 0.26 to 1.7 s |
| control, 06:13:53Z | 70cb7488 | 10 of 10 | 108.92 to 110.98 | 1.0 to 5.4 s |
| head, 06:15:48Z | bed6b1aa | 10 of 10 | 106.30 to 97.68 | 0.8 to 2.9 s |
| control, 06:17:44Z | 70cb7488 | 10 of 10 | 103.92 to 115.30 | 0.7 to 22.6 s |
| head, in the `driver` row, 06:19:28Z | bed6b1aa | 10 of 10 | 115.30 to 101.67 (the row's) | 1.1 to 18.7 s |
| head, 06:38:59Z | bed6b1aa | 10 of 10 | 101.67 to 98.78 | 0.8 to 9.2 s |

The five misses came in one run. The control passed at the same load, with the tree gone after up
to 22.6 s against a watch of 30 s. The code of the row's watch is the same in all five head runs.
The kept lines of the five misses say `gone_ms=- free_ms=-`: at the end of the watch a process of
the tree read before the kill still ran, and the relay's port was not free; the code of that run
printed no name for it. They say `left=no`: after the watch the browser's first process either no
longer ran or was closed. That code printed nothing on the other processes of the tree at the end
of the run; at 06:00:20Z no process named that run's scratch directory. Every head run from bed6b1aa
on prints `left_before=0 left_after=0` in each run.

hosted-win Edge's `org_debugging` (CP-137 (5)): both controls replied, the key read back 0, and in
run 43368925419 each of the ten starts showed the effect; the page, printed with `edge://policy/`
in run 43384012622, lists the setting, but pypdf's text gives it as `Remot eDebuggingAllow e` on
one line and `dfalse Platform Device Mandator` on the next, so the rule's pieces spell more than
the name and the cell is `notrun cause=policy_not_seen`. Whether the controls and the value carry
the cell is with the coordinator.

The check read (CP-140 (2)): in run 43369346664, hosted-mac Chrome's `signin_form`, run 7 of 10,
the read after the start counted `port_flag=1` and `pipe_holder=1`; the reads after the row (0.74 s
later) and after the stop counted 0, and the run was ok with `left_after=0`. Its `check` line stays
as it reads. Since 350698a0 a check names each process that it counts in `port_flag` or
`pipe_holder` (`check_counted`: number, parent, program, and a second read 0.5 s later); run
43369346664 ran before it.

Two points hold the verdict open, with the coordinator for a check by three readers before they are
ruled: (a) whether a run of a row with a watch on `mac` counts for its cell only inside a load
bound that is stated before the run, with the lines outside it kept and counted for nothing; (b)
whether a count at the read after the start that the reads after the row and after the stop of the
same run no longer find is a holder at all or a child of the browser between its fork and its exec,
and what the check must print to tell the two apart. The planner has both with these numbers; a
later round applies what is ruled to the kept lines.

What the third round did not measure (CP-126 E8 (c)). No cell ran on an organisation's managed
machine: the job wrote each `org_` policy itself on a hosted runner, and no management profile or
domain delivered one. Every browser started headless, so no cell measured a browser with a window.
Every run started with a new profile in a scratch directory, never a person's own profile. Every
process ran under the one account of the machine or of the job, so no cell measured a peer of
another account; the cell of `relay_foreign` under another account is with the planner.
