# S1: cap ledger under concurrent streams

rule: MET (0 violations in 165 runs of 28 cells; the control missed the rule at 20 and at 200, so the result counts)

Measured 2026-11-10T07:13:52.288Z on Apple M3 Max, 16 cores, 64 GiB, macOS 27.2, Node v26.12.0; wrangler 4.159.0, miniflare 5.20261109.0-beta, workerd 1.20261109.1.
Code: commit 32be930b202b, with the files of spikes/s1 and stress as committed at the start and at the end of the run.
One execution of `node spikes/s1/run.mjs`: 165 runs in 28 cells, then 17 control runs and 1 row-size run.

## Rule

This section was committed before the first measured run. It is the unit's rule as the plan
states it (plan.md, P0, "S1, rule"). `node spikes/s1/run.mjs` rewrites the rest of this file and
copies this section unchanged.

> After each run the driver joins the ledger's rows with the mock's log by request id. The rule is
> met when, in every cell: the wave admits exactly K; a refused request has no charge; a request
> whose final usage was delivered and whose settle was acknowledged is charged exactly its price
> times that usage; an admitted request the gateway voided has no charge and no entry in the
> mock's log; every other admitted request is charged its full hold, marked unreported; no ledger
> row lacks a driver request; counted spend passes the cap by 0 outside the over-report cell and by
> at most the sum of overruns inside it; no hold is open after its expiry time; no receipt lacks a
> stored hold after the kill; no settle arrives after expiry in a run without the kill. A control
> run of a variant whose admission ignores open holds must miss the rule at 20 and 200, or the
> result does not count. It reports the 99th-percentile settle latency at 200, which sets the
> gateway's margin, and the bytes per stored row. It unblocks D10 and confirms D5. A miss is traced
> to ledger logic, to the local simulator (miniflare 5 is a pre-release, r2#3) or to the Durable
> Object's design; only the last moves the runtime. A mock upstream measures the ledger, not what a
> provider bills for an aborted stream (r3#52, r3#53).

The cells, as the plan states them: three levels (1, 20 and 200 concurrent requests on one
account) times the mock's eight switches, each cell run 5 times with the whole server process
tree killed at a seeded time and restarted on the same state directory; and four more cells, the
wave, the rollover, the quiet cell and the void cell.

## What ran

The server under test is `spikes/s1`: a Worker that plays the gateway, and one SQLite-backed Durable Object per account that is the ledger. Each run starts its own `wrangler dev` (one process tree: wrangler, esbuild and two workerd processes) and its own mock upstream (`stress/upstream.mjs`), on ports the system picks.
A run sends a seeded plan of streaming requests on one account: 1, 20, 200 concurrent clients, 8, 3, 2 requests per client. Client 0 aborts every stream before the usage chunk; of the other streams one in 4 is aborted mid-stream. With a switch on, about half of the requests take the fault and the rest stream normally. The cap fits 3, 12, 120 holds, so admission has to refuse.
In a kill run the whole server tree gets SIGKILL 20 to 200 ms (seeded) after the stream that arms it began (the delay is drawn from the seed), and the server restarts on the same state directory. The arming stream is one of the first three admitted streams, or the first one at level 1. In even-numbered runs, on their first attempt, it is the first stream admitted after a settle was acknowledged, so that settled spend and open holds are both in storage at the kill. In the rollover cell it is the first stream admitted after the period's boundary. A kill run counts only if a hold was open and a stream in flight at the kill. A run that does not count (also a rollover run with no hold open across the boundary, and a void run with a hold that was not voided) or that fails to run is drawn again with a new seed, up to 6 attempts. A failure after the kill stands as a violation, and every failure is listed. Each matrix cell and the rollover cell also get 1 run without the kill, which the rule's last clause and the settle latency need. The wave and the void cell run without the kill; the quiet cell runs only with it.
Stress config: hold life 3000 ms, upstream deadline 1500 ms, margin 400 ms (1700 ms hold life in the void cell). After the last request the driver waits until every hold is 1000 ms past its expiry and reads the ledger with a call that runs no ledger step.

Sums over the counted runs of each cell. Money is in micro-units. "Worst counted spend minus cap" is the highest value over every ledger step of every run: 0 or less means the cap held. It is rebuilt from the rows, whose amounts and charges are checked against the driver's and the mock's records; the order of the steps is the ledger's own sequence number on each row.

| cell | kill runs counted | runs without the kill | requests | admitted | settled | expired, full hold | voided | refused | worst counted spend minus cap | overruns | violations |
|---|---|---|---|---|---|---|---|---|---|---|---|
| usage-last@1 | 5/5 | 1/1 | 48 | 26 | 15 | 11 | 0 | 22 | -13 | 0 | 0 |
| usage-missing@1 | 5/5 | 1/1 | 48 | 20 | 3 | 17 | 0 | 28 | -194 | 0 | 0 |
| zero-placeholder@1 | 5/5 | 1/1 | 48 | 29 | 20 | 9 | 0 | 19 | -105 | 0 | 0 |
| slow@1 | 5/5 | 1/1 | 48 | 29 | 18 | 11 | 0 | 19 | -122 | 0 | 0 |
| over-report@1 | 5/5 | 1/1 | 48 | 17 | 9 | 8 | 0 | 31 | 195 | 13450 | 0 |
| error-status@1 | 5/5 | 1/1 | 48 | 22 | 6 | 16 | 0 | 26 | -356 | 0 | 0 |
| reset@1 | 5/5 | 1/1 | 48 | 22 | 10 | 12 | 0 | 26 | -223 | 0 | 0 |
| anthropic-cut@1 | 5/5 | 1/1 | 48 | 22 | 7 | 15 | 0 | 26 | -155 | 0 | 0 |
| usage-last@20 | 5/5 | 1/1 | 360 | 91 | 37 | 54 | 0 | 251 | -186 | 0 | 0 |
| usage-missing@20 | 5/5 | 1/1 | 360 | 83 | 16 | 67 | 0 | 266 | -36 | 0 | 0 |
| zero-placeholder@20 | 5/5 | 1/1 | 360 | 96 | 41 | 55 | 0 | 255 | -6 | 0 | 0 |
| slow@20 | 5/5 | 1/1 | 360 | 86 | 23 | 63 | 0 | 267 | -78 | 0 | 0 |
| over-report@20 | 5/5 | 1/1 | 360 | 79 | 31 | 48 | 0 | 259 | 12180 | 29639 | 0 |
| error-status@20 | 5/5 | 1/1 | 360 | 83 | 18 | 65 | 0 | 257 | -117 | 0 | 0 |
| reset@20 | 5/5 | 1/1 | 360 | 80 | 17 | 63 | 0 | 262 | -256 | 0 | 0 |
| anthropic-cut@20 | 5/5 | 1/1 | 360 | 79 | 14 | 65 | 0 | 274 | 0 | 0 | 0 |
| usage-last@200 | 5/5 | 1/1 | 2400 | 917 | 484 | 433 | 0 | 1116 | -29 | 0 | 0 |
| usage-missing@200 | 5/5 | 1/1 | 2400 | 810 | 162 | 648 | 0 | 1391 | -31 | 0 | 0 |
| zero-placeholder@200 | 5/5 | 1/1 | 2400 | 885 | 318 | 567 | 0 | 1400 | -8 | 0 | 0 |
| slow@200 | 5/5 | 1/1 | 2400 | 824 | 181 | 643 | 0 | 1460 | -141 | 0 | 0 |
| over-report@200 | 5/5 | 1/1 | 2400 | 737 | 97 | 640 | 0 | 1627 | 11102 | 105373 | 0 |
| error-status@200 | 5/5 | 1/1 | 2400 | 797 | 137 | 660 | 0 | 1512 | -35 | 0 | 0 |
| reset@200 | 5/5 | 1/1 | 2400 | 812 | 145 | 667 | 0 | 1431 | -34 | 0 | 0 |
| anthropic-cut@200 | 5/5 | 1/1 | 2400 | 805 | 149 | 656 | 0 | 1573 | -10 | 0 | 0 |
| wave@200 | 0/0 | 5/5 | 1000 | 600 | 600 | 0 | 0 | 400 | 0 | 0 | 0 |
| rollover@20 | 5/5 | 1/1 | 360 | 163 | 94 | 69 | 0 | 192 | -19 | 0 | 0 |
| quiet@20 | 5/5 | 0/0 | 100 | 59 | 0 | 59 | 0 | 41 | -440 | 0 | 0 |
| void@20 | 0/0 | 5/5 | 300 | 272 | 0 | 0 | 272 | 28 | -23 | 0 | 0 |

## Measured values

- Settle latency at 200, 99th percentile: 18 ms (median 6 ms, highest 22 ms, 1279 settles in the 13 runs at 200 without the kill, each of them the only run of the spike at that time). In the wave alone, where the mock lets every admitted stream end at once: 19 ms over 600 settles.
  How: the gateway reads its clock before the settle call and after the ledger's answer and writes the difference into the stream's last event; the driver collects it from every stream whose client stayed connected. The Workers runtime is documented to move a Worker's clock only when I/O completes, so the figure runs from the last I/O the gateway saw before the call, the end of the upstream stream, to the arrival of the ledger's answer, in whole milliseconds.
  Limits: one Mac, loopback only, with the driver, the mock and the server sharing its cores; the Worker and the object are in one local workerd process, so the figure holds no network hop between them. It is a floor for the gateway's margin, not the margin. In the kill runs at 200, which also ran one at a time, the 99th percentile was 20 ms over 961 settles. Other work on this Mac was not stopped: its one-minute load average at the start of those 13 runs was 9.5 to 113.9 on 16 cores.
- Bytes per stored row: 180 (180224 bytes of growth over 1000 rows: 535 settled, 465 expired). At that size 1 GB holds about 5.9 million rows.
  How: the object's `sql.databaseSize` after the row-size run minus the same figure before its first request. Limits: SQLite grows in 4096-byte pages, so the figure is read to about 5 bytes per row; it is this spike's schema (18 columns, a 36-character request id as primary key) and not the schema of P4.
- Expiry: 5621 holds ended as expired: 5620 by the alarm and 1 by a ledger step that found them due. The longest time a hold stayed open in storage past its expiry was 18635 ms; in the quiet cell, where the alarm alone ends the holds after a restart, 1604 ms.
  How: `ended_at - expires_at` on each expired row, both on the ledger's clock. Limits: a kill run includes the time the server was down, and in every second quiet run the driver keeps it down past the expiry on purpose.
- Served but never settled: 176 holds expired at the full amount although the mock had delivered a final usage (the causes were not separated; the design allows three: the kill, a client that left, and the gateway's cut). They were charged 594128 micro-units; the usage the mock reported was worth 244647.
  How: expired rows whose request has a final usage in the mock's log. Limits: a stream cut before its usage chunk served tokens too, and the mock's log does not count them.
- Streams against their holds: in the 30 runs without the kill the upstream stream that ended latest ended 392 ms before its hold's expiry. The gateway cut 41 slow streams at its deadline, the expiry minus the margin.
  How: the end of each stream in the mock's log against `expires_at` on the request's row; a cut stream is one whose last event to the client says so. Limits: the slow switch draws stream lengths from 0.75 to 1.25 times the time the gateway allows, so about a fifth of the slow streams would end after the expiry if the gateway did not cut them.
- Requests lost on the way in: in 0 runs 0 sends were reset before the server accepted the connection, and 0 were answered 500 by wrangler's dev proxy, which had lost its own connection to the Worker ("Error inside ProxyWorker ... Network connection lost" in the server log, 0 such lines). The driver sent each of them again under the same request id.
  How: outside a kill, a send that ends with a connection reset and no answer, or with a 500 whose body carries no stack through the gateway's source. macOS keeps at most 128 connections waiting on a listening socket (`kern.ipc.somaxconn`) and resets the rest; a level of 200 opens 200 at once to the proxy, and the proxy opens 200 to the Worker. wrangler's proxy sends only GET and HEAD again. Limits: had a lost send reached the ledger after all, the second send would be answered as a repeat, which the join reports as a failure; a 500 from the gateway itself, and any other error line in a server log, is also reported as one.
- Holds overdue at the restart: in 75 kill runs 459 holds were past their expiry when the server answered again. 459 of them were ended by the alarm and the rest by a ledger step; the last one ended 3 ms before the restarted server's first answer to the driver. 2 of these runs are quiet runs in which the driver keeps the server down until 300 ms after the holds' expiry and then sends nothing: 23 holds, 23 of them ended by the alarm, the last one 7 ms before that first answer.
  How: rows that were open at the kill, expired, with an expiry at or before the time the restarted server first answered the driver's health call; `ended_at` on the ledger's clock against that time. "Before" means the alarm ran while the server was starting, before it answered the driver.
- Aborted streams: of the streams of the client that aborts before the usage chunk, 90 admitted, 70 settled at the usage the mock reported, 20 charged the full hold, 0 charged nothing. Of the streams aborted mid-stream, 1121 admitted, 24 settled at the usage the mock reported, 1097 charged the full hold, 0 charged nothing.
  How: the state of each such request's row. The gateway cancels the upstream call when a write to the client fails, and the hold then expires. A stream settles after its client left when the gateway read the final usage before a write failed; when the runtime tells a Worker that its client left was not measured.
- The kill: 130 kill runs counted, 14 attempt(s) did not count and 0 failed to run; each was drawn again. At the kill a median of 11 holds were open (fewest 1) and a median of 6 streams were in flight as the driver saw them. A restarted server took a median of 4951 ms from its start to its first answer, at most 20232 ms. Settles that arrived after the expiry in kill runs: 0.
  What the kill found in storage: in 47 of the 130 kill runs at least one settle was stored before the kill (979 settles in all), and in 53 runs the restarted ledger admitted again (388 holds), against totals it read from storage.
- Running time of the matrix: 14 min 14 s for 165 runs, up to 6 runs at once at levels 1 and 20 and one at a time at 200. The counted attempts add up to 48 min 21 s. A server took a median of 4624 ms to answer after its start. The machine's load average over the matrix was 9.5 to 296.5; other work was running on this Mac, so the time is what this execution took.

## Control

The same server with `ADMISSION:ignores-open-holds`: admission counts settled and expired spend and leaves open holds out. The unit requires this variant to miss the rule at 20 and at 200.

| cell | runs | runs that broke the cap or the wave's count | clauses broken | worst counted spend minus cap |
|---|---|---|---|---|
| usage-last@20 | 6 | 6 | cap 6 | 129819 |
| usage-last@200 | 6 | 6 | cap 6 | 951275 |
| wave@200 | 5 | 5 | wave-admits-k 10, cap 5 | 250560 |

The control missed the rule at 20 and at 200, so the result counts.

## Reproduce

```sh
sh scripts/prerequisites.sh    # once; the only step that uses the network
node spikes/s1/run.mjs         # the matrix, the control and the row-size run; rewrites this file
```

Run one at a time per checkout: a run first stops the servers that an interrupted run left behind. It exits 0 when the rule is met and 1 when it is missed or the run is incomplete. Each run's records stay under `.tmp/s1/STAMP/runs/KEY/` (the driver's requests, the mock's log, the ledger's rows, the server log, the state directory); `results.json` beside them holds every run's verdict and seed. `--cells REGEX`, `--runs N`, `--jobs N` and `--no-control` make a partial run, which prints its summary and leaves this file alone.
