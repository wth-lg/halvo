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
macOS image updated itself from 156 to 157 during that job, between the six rows and the driver row.

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
