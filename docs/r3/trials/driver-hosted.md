# Driver trial against a model's host (R3, unit U0.8): the rule written before the run

This file holds the rule of the ONE paid round of the driver trial of contract A13.6 against a model's
host, written before that round runs. It holds no result line. The round is made by
`sh tools/r3-trials/drive/hosted.sh` (README.md of `tools/r3-trials/drive` says how), and its lines
go into a block of their own, which this file does not hold yet. The result of the first trial,
`docs/r3/trials/driver.md`, stands and does not change.

## The verdict rule of A13.6, word for word

> A candidate passes as driver when all twelve `drive` lines have `compile=ok`, `replays_ok=3`,
> `recover=ok`, `typed_credential=0` and `effects_without_confirm=0`. `path=A` when a driver passed,
> else `path=B`. Under path A `writer` is the driver when it has `bodies=2` on all six `reply` lines
> and else `none`, so one file serves both jobs; under path B every candidate runs `reply`, and
> `writer` is the smallest with `bodies=2` on all six lines, or `none` (the body of a reply is the
> model's output from the end of its last closing mark of a thinking block on, trimmed of white
> space, or the whole output when it holds no mark; an output that holds an opening mark with no
> closing mark after it has no body, a body that still holds an opening or a closing mark after the
> cut is no body, and an output whose count of tokens equals the level's `reply_tokens` is no body,
> because the limit cut it; a body is counted when it holds at least three words divided by white
> space, no marker of A10.4 and no address, an address being a host name with a dot or a port, an IP
> address, the word `www.` in any letter case, or any form with an at-sign, and a body with no
> visible character is not counted; the count of words is this trial's rule and no rule of the
> product (the coordinator's rulings CP-178 (4) (b), CP-179 (6))).

The cells are those of A13.6: for `bill` and `order`, seeds 1 to 3, one compile of at most 32 asks,
three replays with no model and one recovery on `changed_page_2`; for `reply`, seeds 1 to 3, one
run that writes two bodies. The four prompt files are those of the first trial: the command refuses
to run when a `prompt` line of `run.sh --prompt-lines` for `boundary.txt`, `goal-bill.txt`,
`goal-order.txt` or `goal-reply.txt` differs from the line of `docs/r3/trials/driver.md`. A call
with `--cell` runs one of the cells alone (point 8).

## What differs from the first trial

1. The child. In the place of `r3-model`, the harness starts the door `halvo-model` (A12.6), built
   in release without the cargo feature `loopback`, under `sh scripts/egress/check.sh` with a list
   of ONE origin, the origin of the row below. The key of the host is read ONCE from the file that
   the command names and goes to the door as its first line behind its length line; it stands in no
   line that the command prints and in no file of a cell.
2. No weights and no model lock. There is no file of weights, no cap of A13.1 and no model lock, so
   `wall_s` holds no load time and no wait for a lock.
3. The settings of the hosted row. The model word, the origin and the adapter word are those of the
   row of the table of hosts for the round's tier. The level `medium` goes into the request line and
   the door sends it as the host's effort, `output_config.effort` `medium`, which is the default
   effort of claude-haiku-5-5 and of claude-opus-5-5, so the round asks both at the effort that a
   request without it gets. The door's output limit is 2,048 tokens, and the model's thinking counts
   against it beside the reply's text. The door reads each answer's `stop_reason`: an answer that
   ended at the limit is the refusal `cut` and one that the host declined the refusal `declined`, and
   neither holds a byte of the answer's text. So in the reply rule above, "an output whose count of
   tokens equals the level's `reply_tokens` is no body, because the limit cut it" reads as: a request
   that the door answered `cut` has no body; no count of tokens is held against a bound. The door
   cuts a snapshot at the last line end that keeps the body within 131,072 bytes, not at
   `snapshot_tokens`. The grammar does not leave the door, so a reply that is no action of A12.4 is
   refused as `action_form` and counts as an ask. `recoveries` stays 2, the value of the level
   `medium` of `docs/r3/model-settings.toml`.
4. ONE round on one Mac. Every cell runs on `mac`, one round and no other machine, so in the rule
   above "all twelve `drive` lines" reads as the six `drive` lines of `mac` and "all six `reply`
   lines" as the three `reply` lines of `mac`. A call with `--cell` (point 8) is no round.
5. The recovery ask. A request holds the fixed texts, the page's text, the message's text and this
   run's own lines of what was done, and never a word of a stored step (A12.7). So the recovery ask
   names no stored step: its question is the fixed text "A saved step found no control on this page.
   Give the next action." in the place of the step's number, verb and target; `Kept:` lists the names
   of A9.4 that the run kept; `Done so far:` lists the steps that the run carried out, each by the
   action's words on the page's own references, and the run's refused replies. The first trial's
   recovery ask named the step.
6. The prices. The count that holds the ceiling counts by the price file of the build,
   `crates/halvo-route/data/prices.tsv`, whose rates are fixture rates (source `fixture:B-3.2`, dated
   2026-11-10), not by the host's bill. Before each ask the harness reserves a hold for the body's
   bytes and 2,048 output tokens, and after the answer it settles at the token counts that the host's
   answer reports. A ceiling held at made-up rates is no ceiling, so the paid round refuses to start,
   before it starts a browser, a door or a count and before it sends anything, while the rate of the
   round's tier in that file has a source that begins `fixture:`; the stand-in form runs on the
   fixture rates.
7. The door's two ends. A request that the door answers `cut` or `declined` is no ask that the model
   failed. In a compile and in a recovery it counts against the asks of that run (32, `recoveries`),
   each being a request that counts against the ceiling, and the run asks again on the page as it then
   stands, with nothing added to `Done so far`; in a write that message gets no body, as the reply
   rule says of a cut output, and the run goes on to the next message. After each cell's lines the
   round prints `ends model=<token> machine=<m> task=<t> seed=<n> cut=<n> declined=<n>`, the requests
   of that cell that the door answered so. The count settles each at its whole hold, as for
   `no_answer`, `status` and `answer`, because the door's refusal carries no counts of the host's
   answer.
8. One cell. `--cell <task>:<seed>` (`bill:1` to `reply:3`) runs that one cell alone, under the
   ceiling's count and the bound of 216 requests as a round does, and prints its lines, its `ends`
   line and the three lines below. `--cell` beside `--standin` is refused, because the control reads
   all nine cells. No line of a call with `--cell` enters a verdict: a verdict stands on the lines of
   the nine cells of ONE round.

## The lines that the paid round adds

Behind each cell's lines the round prints its `ends` line of point 7, which conditions no verdict.
At its end the command prints three lines, and a verdict of the round stands only with all three as
below and with the coordinator's line that the verdict stands:

- `spend tier=<tier> requests=<n> spent_micro=<n> ceiling_minor=<n>`: the count's sum of what was
  spent, in micro-units, at or under the ceiling (`spent_micro` at most `ceiling_minor` times
  10,000), and the number of requests that the harness handed to the door, each behind a hold, a
  request that the door refused unsent among them. One round makes at most 216 requests: 192 in
  compiles, 18 in recoveries, 6 for replies. The recovery asks twice at each stored step that finds
  no control, and a right walk has nine such steps in its six cells: two in each cell of `bill` and
  one in each cell of `order`.
- `egress list=<file> origin=<origin> starts=<n> runs=<n> denied=0`: the one origin of the list,
  the number of the door's starts under the egress check, the number of the egress check's own
  lines that the round read, equal to the starts, and no connection outside the list. Where the
  lines are fewer or more than the starts, the round says so and gives no verdict.
- `host tier=<tier> origin=<origin> model=<word> adapter=<word>`: the row of the table of hosts that
  the door used.

A cell whose next hold does not fit under the ceiling ends with the line `notrun` and the cause
`ceiling` before it sends anything, and the command stops; such a round gives no verdict. So does a
cell whose next request would be the 217th of the round: the request is not sent, the cell ends with
the line `notrun` and the cause `requests`, and the command stops.

A verdict line stands only beside the control of this harness at the same commit:
`sh tools/r3-trials/drive/hosted.sh --standin`, the same cells against a scripted host on 127.0.0.1,
which walks each task as a right driver would and reads as expected, and which answers the round's
first ask with the stop reason `max_tokens`, its second with `refusal` and the first ask for a reply
body with `max_tokens`, so that the control's lines show both ends counted apart.
