# The coordinator's models

This directory holds two TLA+ models of the coordinator that the crate `halvo-coord` will
implement, and the traces that the crate's tests replay. `Schedule` is the machine that
decides when a run starts, how it moves and how it ends. `Confirm4` is the life of a
confirmation and the guard on the runner's effect records. The models were written before
the code. A change to a rule changes the model first; then the traces are exported again
and committed. Nobody edits a trace file by hand.

| File | What it holds |
|---|---|
| `Schedule.tla`, `Schedule.cfg` | the schedule model, its full configuration and the invariants C1 to C11 |
| `Schedule-days.cfg`, `Schedule-queue.cfg` | the two small configurations of the schedule model that CI runs |
| `Confirm4.tla`, `Confirm4.cfg` | the confirmation model, its configuration and the invariants L1 to L10 |
| `ScheduleTrace.tla`, `Confirm4Trace.tla`, `TraceText.tla` | the modules that walk a model and print one JSON line per trace |
| `limits.json` | the limits of the models, with the field names of the struct `Limits` |
| `export.sh` | the command that writes the two trace files |
| `traces-schedule.jsonl`, `traces-confirm.jsonl` | 200 random walks of each model, then 11 and 15 directed traces |

## The checker

The models are checked by TLC, which runs from `tla2tools.jar` of the tlaplus project's
release v1.7.4. On 2026-11-11 the address
`https://github.com/tlaplus/tlaplus/releases/latest/download/tla2tools.jar` led to that
release. GitHub gives that address to the newest release that is neither a draft nor a
pre-release, so a pre-release is never the pinned one. The jar is not in this repository.
`export.sh` takes its path with `--jar`, and the TLC commands below take it with `-cp`.

```
jar url=https://github.com/tlaplus/tlaplus/releases/download/v1.7.4/tla2tools.jar bytes=2274532 sha256=936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88
```

The jar holds three licences in two files, read from the jar itself. `License.txt` is the
MIT License, with the line "Copyright (c) 2019 Microsoft Research". `META-INF/LICENSE.txt`
holds the Eclipse Public License 2.0 and, after it, the GNU General Public License 2.0
with the Classpath Exception; both are for the mail classes inside the jar (`javax/mail`,
`javax/activation`, `com/sun/mail`). The jar is a tool that is fetched and run. Nothing in
this repository links it, ships it or holds a copy of it.

The classes of TLC in the jar are Java 8 bytecode (class-file version 52). `export.sh`
asks for Java 17 or later, the floor that the check of these models uses, and for nothing
else. No other tool turns TLC's output into JSON lines: the trace modules build each line
as a TLA+ string and print it with TLC's `PrintT`, and `export.sh` takes the quotes off
with `sed`.

## Check the models

Run TLC on a copy, so that this directory gains no state file and no trace-spec file:

```sh
d=$(mktemp -d)
cp formal/r4-coordinator/*.tla formal/r4-coordinator/*.cfg "$d"
cd "$d"
java -Xmx2g -XX:+UseParallelGC -cp PATH tlc2.TLC -workers 1 -metadir "$d/meta" -config Schedule.cfg Schedule
java -Xmx2g -XX:+UseParallelGC -cp PATH tlc2.TLC -workers 1 -metadir "$d/meta" -deadlock -config Confirm4.cfg Confirm4
```

Each run must print `Model checking completed. No error has been found.` `Confirm4` runs
with `-deadlock`, because a state after the app ended has no next state on purpose.

A model has two kinds of configuration. The full one is the model's own bound. Whoever
changes a file of this directory runs it, through the machine's helper for timed runs, the
verifier runs it at the gate of the phase in which the change lands, and this table
records its states and the commit. CI runs the small ones alone: `30-r4-formal.sh` runs
`days` and `queue` of `Schedule` and the one configuration of `Confirm4`, which is small
enough, and prints their names, so that its PASS is never read as the full bound. To run
a small configuration, name its file after `-config` in the command above.

| Model | Configuration | File | What it holds | Distinct states | States generated | Depth |
|---|---|---|---|---|---|---|
| `Schedule` | `full` | `Schedule.cfg` | five agents, three days, a queue of two, one restart, one deleted workflow | 722,204 | 21,515,176 | 14 |
| `Schedule` | `days` | `Schedule-days.cfg` | the full one without `a5` and with a queue of one | 154,332 | 3,848,822 | 12 |
| `Schedule` | `queue` | `Schedule-queue.cfg` | five agents and a queue of two on one day: no restart, no deleted workflow, no switch-off, no stop, no failed frame | 817 | 18,247 | 8 |
| `Confirm4` | `full` | `Confirm4.cfg` | one run, two requests, two effects | 1,656 | 179,109 | 20 |

| Configuration | Rows of the table of wrong designs that TLC reports in it | `Reach` properties that TLC reports violated in it |
|---|---|---|
| `full` of `Schedule` | 1 to 8, 17 to 21, 28 to 34 and 38 | all of `Schedule` |
| `days` | 1 to 8, 18 to 21, 28, 29, 31 to 34 and 38 | all of `Schedule` but `ReachQueueTwo`, `ReachQueueOrder` and `ReachQueueIdle`, which need a queue of two |
| `queue` | 17 and 30: of the two, only `queue` holds `a5`, the agent with no schedule that needs no person. It holds no stop, so row 38 is no run of it | `ReachQueueTwo`, `ReachQueueOrder`, `ReachQueueIdle` |
| `full` of `Confirm4` | 9 to 16, 22 to 27, 35 to 37 and 39 | all of `Confirm4` |

The counts are TLC's own lines, with one worker, for `Schedule.tla` as it stands since
commit b877054 and `Confirm4.tla` as it stands since commit 785ca67. The heap bound `-Xmx2g` keeps a run far
below the machine's cap of 8,589,934,592 bytes of resident size. More workers find the
same states; the depth that TLC prints can then differ.

What each configuration costs on the development Mac (arm64, JDK 17.0.20.1), one run each
on 2026-11-11 through the machine's helper for timed runs, read with `/usr/bin/time -l`.
The time holds the start of the JVM and follows the load of the machine.

| Model | Configuration | Time, one worker | Largest resident size | 1-minute load before the run |
|---|---|---|---|---|
| `Schedule` | `full` | 1130.14 s | 1,487,798,272 bytes | 130.27 |
| `Schedule` | `days` | 112.88 s | 1,024,049,152 bytes | 94.55 |
| `Schedule` | `queue` | 1.41 s | 459,702,272 bytes | 34.16 |
| `Confirm4` | `full` | 8.70 s | 434,012,160 bytes | 367.31 |

## What a property is read from

Each model has two variables. `st` is what the coordinator keeps and what its guards read:
the live runs, the queue, the counts, the guard's table. `h` is the history: what
happened, written only by the move that made it happen. `Step(st, move)` never sees `h`,
so no guard of the start, of the tick or of the link reads it, and a restart leaves it as
it is. Every one of the twenty-one invariants reads `h` alone.

`Step` returns, beside the answer and the next state, what the step did to the world, in
order (`out`): the events that it stored and the calls of the ports that it made. `Hist`
writes `h` from two things, the move and `out`. What the environment knows it takes from
the move and never from `out`: who pressed, which run the person stopped, what the port
answered, what the runner's last frame said, whether the acknowledgement came, which
record the runner sent. From `out` it takes what only the coordinator's act can be: that
an event was stored, that a runner was started, cancelled or killed, that a decision went
to the runner, that a record was passed on. Where the history needs a rule of the contract
(which holds stand by the stored events, which slot I5.3 gives a start, whether the grace
of a stop has passed), `Hist` computes it again from `h` itself. So a coordinator whose
own value is wrong, for example a count that a restart forgets, or whose own label is
wrong, for example a start that it calls the person's, changes `st`, its answers and
`out`, while `h` stays right and the invariant reports the start that should not have
happened.

The histories that take a fact from the move since the third round, with the property
that reads each:

| Fact | Before | Now | Read by |
|---|---|---|---|
| who asked for a queued start | the trigger of the step's `queued` event | the person, when the move is a `start` of that agent; the scheduler otherwise | C3, C5 |
| who asked for a stored start | the trigger of the step's `start` event | the person, when the move is a `start` of that agent or a start of the person waits for it in the queue | C5, C6 |
| whether a `run_started` event was acknowledged, and whether the runner started | two fields of the step's `start` event | the `port` of the move | C1 to C4, C7 |
| that a run was stopped by the person, by a lapse or by the switch-off control | the step's `stop` event | the move `stop` or `off`; a stop for a gone workflow is still the coordinator's call of `Runs::cancel` | C8, C9 |
| that a run ended, and the cause of that end | the step's `end` event | the move that ended it: `end` with its frame, a `restart` with its `Runs::reap`, a `Runs::kill` of a stopped run, a `Runs::start` under the port `no_run`; and for a stopped run the cause of the move that stopped it | C10 |
| which moves an item accepted, and which answer came from which source | the step's `move` and `answer` events | the move, when its answer is `ok` | L1, L4, L6 |
| what the request of an item showed | the step's `shown` event | the request of the move `issue` and its `digest_ok` | L2 |
| that the acknowledgement of the rows came, or that the store wrote them without one | a field of the step's `rows` event, and then the step's own `item_answered` event in any move | the move `record`, which is the answer of the port `Owner`, and the key `stored` of a lapse. The rows themselves are those that the line of that move stores and shows in `e`; an `item_answered` event in a line of another move acknowledges nothing | L3, L5, L7, L8, L10 |
| that the run was stopped | the step's `stop` event | the move `stop`, or a lapse with `wait_limit` or `cancelled`, when its answer is `ok` | L6, L9 |
| which record passed | the record of the step's `pass` event | the record of the move `guard` | L7, L8, L9, L10 |

## Schedule

A move is a record, and `Step(st, move)` gives the answer and the next state, so one step
of the model is one call of the coordinator.

| Part of `st` | Meaning | Item |
|---|---|---|
| `day` | the coordinator's day: the day of its last tick, or of the start of its life | I5.2, I5.9 |
| `life`, `nrun`, `dels` | the life of the app, the run ids handed out, the workflows deleted | I5.8 |
| `runs` | the live runs: id, agent, phase, the cause that a stop gave, the day of the start | I5.6 |
| `dying` | the runs that were stopped and whose runner ended before the kill; the first tick at which the grace has passed calls `Runs::kill` for each | I5.7, I5.9 |
| `queue` | the starts that wait, a person's before a scheduler's | I5.5 |
| `ready` | the agents with an open `ready` item | I5.3 |
| `off`, `paused`, `check` | the holds `off`, `failures` and `check` as the coordinator holds them | I5.4 |
| `gone` | the workflows that are not in the owner's list, as `Owner::workflows` returns it | I5.5, I5.7 |
| `fin`, `done`, `starts`, `fails`, `slotrun` | what the coordinator reads from the stored events: an agent with a run that ended `completed`, the day of the newest run that ended `completed` or `nothing_to_do`, the starts of each day, the failure streak, the newest slot that a `run_started` event names | I5.2 to I5.5 |

| Part of `h` | What happened | Read by |
|---|---|---|
| `live` | the runs between their spawn and their end, each with the life that started it, whether it was stopped (`stop`), the cause of that stop by the move that made it (`why`: the cause of a `stop` move, `cancelled` for `off`, `deleted` from the tick that sees the workflow gone), and whether it outlived the first tick at which the grace of that stop had passed (`late`) | C1, C2, C3, C8, C9, C10 |
| `wrongend` | the ends that happened and for which the step stored no `run_ended` event or one with another cause, and the `run_ended` events of runs that had not ended. The end of a run whose workflow is not in the owner's list owes no event, and an event for it is one too many | C10 |
| `noslot` | the stored starts that no move of the person asked for and whose slot is not due when they are stored, a start that leaves the queue among them: the agent's workflow has no schedule, or its current slot is not open by the served runs of the history, or that slot is older than `catch_up_days` on the day of the history, or a stored start was already given that slot | C11 |
| `wait` | for each agent, the starts that joined the queue and have not left it, by who asked: the person, when a `start` move of that agent queued it, and the scheduler otherwise; a restart empties it, as I5.8 empties the queue | C3, C5 |
| `count` | the `run_started` events of each agent and day, a start whose step 7 or 8 failed among them | C7 |
| `holds` | the holds that the stored events ask for: `off` from the switch-off control on, `failures` from the `run_ended` event that completes the streak until a start by the person, `check` from a `run_ended` with `effect_unknown` | C4 |
| `heldstarts`, `unacked` | the starts that were stored while such a hold stood, and the runs that began without an acknowledged `run_started` event | C4 |
| `done`, `served`, `streak` | the agents of which a run ended `completed`, the day of the newest served run, the failure streak, each from the `run_ended` events | C4, C5, C6 |
| `alone`, `extra` | the stored starts that no move of the person asked for, of an agent that needs the person or of which no run had ended `completed`; and the stored starts of a `start` move beyond its one | C5 |
| `given`, `sched`, `after` | the slots that I5.3 gave the stored starts of an agent, the scheduler's starts for each slot, and those that came after a start that I5.3 gave the same slot | C6 |
| `day`, `life` | the day of the last tick or restart, and the life of the app | C2, and the day of every stored start |
| `gone`, `gonestarts` | for each workflow, whether it is in the owner's list (`listed`), left it (`gone`) or left it before a tick (`seen`), and the starts that were stored for a workflow outside the list | C8 |

The configuration has five agents, the slots of day 1 and day 3 (`FirstSlot`, `Period`),
three days, one restart and one deleted workflow.

| Agent | Needs the person | Needs the model | Slots | A run ended `completed` before | Other |
|---|---|---|---|---|---|
| 1 | yes | yes | yes | yes | |
| 2 | no | yes | yes | yes | its workflow can go; the scheduler starts it |
| 3 | no | no | yes | no | the switch-off control is used on it |
| 4 | yes | no | no | yes | its workflow can go |
| 5 | no | no | no | yes | |

Two agents need the one model and three do not, so both halves of step 4 of I5.5 decide.
With two live runs and two starts in the queue, a fifth agent is the one whose start is
answered `queue_full`.

| Invariant | I5.12 | In the model |
|---|---|---|
| C1 | at most one live run holds the on-device model, and at most `max_live_runs` runs are live | at most one run of `h.live` is of an agent that needs the model, and at most `MaxLive` runs are in it |
| C2 | no run is live in a life of the app that did not start it | every run of `h.live` has the life of `h` |
| C3 | an agent has at most one live or queued run | for each agent, its runs in `h.live` and its starts in `h.wait` are at most one together |
| C4 | a run starts only when no hold stands by the stored events and with an acknowledged `run_started` event | `h.heldstarts` and `h.unacked` are 0 |
| C5 | a run of an agent that needs the person, and the first run of every agent, starts only by a move of the person; a start that leaves the queue counts as one by the person only when a move of the person queued it, whatever trigger the step gives it; and one move of the person leads to at most one `run_started` event | `h.alone` and `h.extra` are 0 |
| C6 | one slot leads to at most one start by the scheduler, and to none after a start that I5.3 gives that slot | `h.sched` is at most 1 for every agent and slot, and `h.after` is 0 |
| C7 | an agent has at most `starts_per_day` `run_started` events on one day | `h.count` is at most `StartsPerDay` for every agent and day |
| C8 | no run starts for a workflow that is not in the owner's list; a run whose workflow left the list is `stopping` or ended after the first tick that follows, and ended after the first tick at which the grace has passed since that stop | `h.gonestarts` is 0, and a run of `h.live` whose workflow is `seen` has `stop` and not `late` |
| C9 | a run that was stopped is ended after the first tick at which `cancel_grace_s` have passed since its stop, whoever stopped it | no run of `h.live` is `late` |
| C10 | the cause that a `run_ended` event stores is the cause that I5.6 gives the end that happened, read from the move that ended the run: `failed` for a start whose step 7 or 8 failed and for a last frame `failed` of a run that was not stopped, the cause of its stop for a stopped run, `interrupted` for a run that a restart finds live. This is read for a run whose workflow is in the owner's list at its end; the end of a run whose workflow is gone stores no event | `h.wrongend` is 0 |
| C11 | a start that no move of the person asked for is a start for a slot of the workflow's own schedule that is open when the start is stored, with no exception for a start that waited in the queue: the workflow has a schedule, no served run closes its current slot, that slot is at most `catch_up_days` old, and no stored start names it | `h.noslot` is 0 |

How the model reads I5 where the contract ruled or left a reading to the model. The traces
follow these readings.

1. An agent has a finished run when a run of it ended `completed`. Until then every run of
   it is a first run: the scheduler starts none, and a tick raises `ready`.
2. A tick raises no `ready` item for an agent that has a live run, a queued start, a hold
   or an open `ready` item, or whose workflow is gone.
3. A start by the person names the current slot when that slot is open and the agent has
   a finished run, and no slot otherwise. A start by the scheduler names the slot that is
   current when the start is stored.
4. The day of a run, for the rule of the open slot, is the day of its start.
5. A start goes through step 0 first: an agent whose workflow is gone gets `state`. A start
   by the person then ends the hold `failures`, also when a later step refuses it, so only
   a start by the scheduler is refused under that hold. The failure streak is not reset by
   that: one more failed run sets the hold again.
6. A start that leaves the queue goes through the steps from step 0 again and is dropped
   when step 0, 1, 2, 3, 6, 7 or 8 fails, and a start of the scheduler also when its slot
   is no longer due (reading 14); it stays at the head when step 4 fails, and then
   no start behind it is tried. One end takes one start from the queue. A tick takes one
   start from the queue when it killed no run and no run is live.
7. A run that was stopped ends with the cause of its stop, whatever frame its runner sends.
   A run that was not stopped keeps the outcome of its last frame, and `interrupted` with
   no frame. The switch-off control stops with `cancelled`.
8. `delete` takes the workflow out of the owner's list and stops nothing. The first tick
   after it stops the run of that workflow with the cause `deleted`, also a run that was
   already `stopping`, and drops the agent's queued start and its `ready` item. The end
   of a run whose workflow is gone stores nothing, whether the kill, the runner or a
   restart ends it: the owner has removed every row that names the workflow and refuses a
   new one with `no_workflow`. `deleted` is the cause of the stop and of the `ended` entry
   of the line, and of no `run_ended` event. The open item of such a run stores no
   `item_lapsed` either; `Confirm4` has no deleted workflow and does not show that.
9. `off` sets the hold at once and stops the agent's run. A queued start of that agent is
   dropped with `hold` when it leaves the queue, and an open `ready` item of it stays open
   (I5.7). No tick sets or ends the hold `off`.
10. A restart ends every run with `interrupted`, also a run that was `stopping`: a stop is
    no stored event, so recovery finds a `run_started` event with no `run_ended` and stores
    `interrupted` (I5.6, I5.8). It empties the queue and closes every `ready` item, and it
    takes the day that the new life reads.
11. A `ready` item closes when a start of its agent is stored or queued. It stays open
    when the start is refused.
12. The day changes only at a tick or at a restart. The kill comes at a tick at which
    `cancel_grace_s` have passed since the stop; a run that the same tick stopped is not
    killed by it. The tick calls `Runs::kill` also for a stopped run whose runner ended in
    between (I5.9); that call ends no run and stores nothing.
13. The holds `off` and `check` are never cleared, because no move of a trace is the
    control `switch_on` or the person's answer to a `check`.
14. A start of the scheduler that leaves the queue is asked the slot rule of I5.3 again,
    after the steps 0 to 3 and before step 4: its current slot is open, at most
    `catch_up_days` old and named by no `run_started` event. When the rule fails, the
    start is dropped as one whose step 3 fails, and `next` holds it with `state`. So a
    start that a tick queued on day 1 does not begin on day 2 with `catch_up_days` 0. A
    start of the person is not asked: the person asked for it. `ReachQueuedStale` shows
    such a drop.
15. `wait` comes when an item of the run opens and `resume` when that item is closed,
    which for a confirmation is its release or a lapse and not the person's answer
    (I5.6). The schedule model holds no item: `wait` and `resume` are free moves that
    ask only for the phase, so the model resumes at neither the answer nor the release.
    No property of the schedule reads which of the three phases a live run has, so a
    tie to an item would change none of C1 to C11; the link between the close of an item
    and `resume` is held by the code's test of `wait` and `resume` and by no property.
    The search also stops a run with `lapsed` in every phase but `stopping`, which is
    more than a wait limit can do; the walks stop with `lapsed` only a run that is
    `waiting`.

What the search does not cover. It follows a behaviour until its second stored start
(`MaxRuns`) and checks the state after a third. It ends a run that was not stopped
with the frame `completed` or `failed`, and a stopped run with no frame. A run that goes
without a frame and without a stop ends `interrupted`, which the rules treat as they treat
the end of a stopped run; it is in the walks only, like the other seven outcomes. The hold
`check` comes only from a frame `effect_unknown`, so only the walks hold it. `VIEW` takes
`ready` and the phases `starting`, `working` and `waiting` out of the fingerprint. That is
sound because `ready` decides only its own next value and the `raised` of a tick, the three
phases decide only which of `hello`, `wait` and `resume` is accepted, each of which leads
to a state of the same class, every stop, end, kill, switch-off and deletion treats the
three alike, and the history holds neither. `VIEW` keeps the cause of a stop, `cancelled`,
`lapsed` or `deleted`, in the state and in the history: C10 reads it, and a view that took
`lapsed` for `cancelled` hid a coordinator that stores `cancelled` for a lapsed stop. The
`Reach` properties that name the cause of a stop run without the `VIEW` line, as before.
`VIEW` also takes `dying` out: it decides only its own next value and the `Runs::kill`
calls for runs that have ended, which change no history; `ReachKillEnded` reads it and runs
without the `VIEW` line too. The model has no `skip`, no `switch_on`, no
answer to a `check`, no items budget, no effect in `releasing`, and no time but the grace.

## Confirm4

`st` holds the items of one run, the port's count of approval ids, the guard's table, and
whether the run and the app still live. The run sends two requests. The first names the
effects 1 (class `send`) and 2 (class `payment`), so its presenter is `native`. The second
names effect 1 with another digest, so its presenter is `page`.

| Part of `h` | What happened | Read by |
|---|---|---|
| `shown` | for each item, the effects and digests of the request that was shown | L2 |
| `moves` | for each item, the moves whose answer was `ok`, in order, a lapse with its cause | L4, L6 |
| `answers` | each `answer` move whose answer was `ok`: the item, the answer, the source, and whether the item's last accepted move was `arm` | L1 |
| `minted` | for each item, the approval ids that the port minted for the answer of an `answer` move | L3 |
| `stored`, `acked` | the `item_answered` rows that the Owner holds, as whole rows. They are acknowledged and stored at the move `record`, which is the answer of the port `Owner`, and they are the rows that the line of that move holds in `e`. They are stored without an acknowledgement at a lapse with `no_record` whose key `stored` is `yes`, and they are the rows that the line of that lapse holds in `e`. An `item_answered` event in a line of another move acknowledges nothing, so no property reads a row that no trace line shows | L3, L5, L7, L8, L10 |
| `got` | every decision that the runner received: the item, `release` or `lapse`, the entries, and whether the run had been stopped before | L1 to L3, L5 to L10 |
| `released` | the `item_released` rows, which the move `release` stores | L5, L6 |
| `passed` | every record `approved` or `releasing` that the guard passed, with whether an earlier record had used its approval id, whether an `approved` record of the same effect, digest and approval id had passed before, and whether the run was stopped | L7 to L10 |
| `stopped` | whether the run was stopped | L6, L9 |

| Invariant | I5.12 | In the model |
|---|---|---|
| L1 | a decision holds `allow_once` only after an accepted `answer` with `allow` on its own item, while that item was `armed`, from that item's presenter | for every `allow_once` entry of `h.got`, `h.answers` holds such an answer of the decision's item |
| L2 | the digests of a released decision are the digests of the request that was shown | the effects and digests of a released decision of `h.got` are `h.shown` of its item |
| L3 | an approval id stands in at most one decision and in at most one row; the id of an acknowledged row whose decision was released stands in that decision; a lapse's decision holds no id of the answer and no id that a row holds or will hold | the four clauses over `h.got`, `h.stored`, `h.acked` and `h.minted` |
| L4 | no move leaves `released` or `lapsed`, and a `recorded` item lapses only with `run_ended` or `app_ended`; an `answered` item lapses only with `run_ended`, `cancelled`, `no_record` or `app_ended`, as the table of I4.2 has it | in `h.moves`, a `release` or a `lapse` is the last move of its item, a `lapse` after an `answer` has one of the four causes, and a `lapse` after a `record` one of the two |
| L5 | no decision is released before its rows are acknowledged, as whole entries; an `item_released` row stands only for a decision that the runner received, with its entries | the entries of a released decision of `h.got` are the rows of `h.acked` of its item, and every row of `h.released` has its released decision in `h.got` |
| L6 | a lapse's decision holds `reject_once` only; once a run was stopped no later decision holds `allow_once`; no `item_released` row names an item that lapsed | over `h.got`, its mark of the stop, `h.released` and `h.moves` |
| L7 | the guard passes a record `approved` only with a matching unused acknowledged row of the same effect and digest whose decision was released with `allow_once` | every `approved` record of `h.passed` has such a row in `h.acked` with its release in `h.got`, and no earlier record used its approval id |
| L8 | the guard passes a record `releasing` of a native class only when such a row was used | every such record of `h.passed` has the row and an earlier `approved` record of the same effect, digest and approval id |
| L9 | no record `approved` or `releasing` passes for an approval id that the runner received with `reject_once`, or for a run that was stopped | no record of `h.passed` was passed after a stop, and none names an approval id of a `reject_once` entry of `h.got` |
| L10 | the guard passes a record `releasing`, of any class, only when its approval id is that of an acknowledged row of the same effect and the same digest whose decision was released with `allow_once` | every `releasing` record of `h.passed` has such a row in `h.acked` with its release in `h.got` |

How the model reads I4:

1. A lapse of a run that still lives sends `reject_once` for every effect under approval
   ids that the port mints anew. No row holds them. A lapse with `run_ended` or `app_ended`
   sends nothing.
2. The guard's table takes the rows at the move `record`, counts them from the move
   `release` on, and loses the rows of an item at any lapse of it. Rows that the store
   wrote without an acknowledgement are in the store and never in the table.
3. A request while an item is open is refused with `state` and ends the run; the open
   item lapses with `run_ended` in the next step.
4. A lapse with `wait_limit` or `cancelled` stops the run, and so does the move `stop`, the
   person's stop while no item is open. A stopped run gets no further item: its request is
   answered `state`. Its records `approved` and `releasing` are answered `approval`;
   every other record passes.
5. A record `releasing`, of any class, passes only when the table holds a released row
   with its approval id, its effect, its digest and the answer `allow_once`. Of a class
   with the presenter `native` it passes, beside that, only when an `approved` record used
   that row. So the id of no decision, the id of a row before its release, an id of a
   `reject_once` decision and the id of another effect's approval are each answered
   `approval` (I4.7). A record of the class `send` may pass on a row that no `approved`
   record used.
6. `confirm_step` takes a lapse with any of the six causes from `shown` or `armed`, from
   `answered` only with `run_ended`, `cancelled`, `no_record` or `app_ended`, and from
   `recorded` only with `run_ended` or `app_ended`. The wait limit does not lapse an item
   that the person has answered (I4.2): such a lapse, and one with `digest`, is answered
   `state`. The walks hold
   only the causes that can arise: `no_record` after an answer, `digest` at `issue`.
7. The move `release` stores the `item_released` row. A restart is the app's end: the open
   item lapses with `app_ended`, and no move follows.

What the properties do not show. L3 reads the stored rows as a set, so a row that is
written twice cannot be told from a row that is written once: a `release` that writes the
item's rows again breaks no property. A ledger reader would see two rows for one approval
id; no record passes for it.

## Reachability and controls

A property whose name starts with `Reach` says that some part of the configuration never
does its work. TLC must report each one violated. To run one, add the line
`INVARIANT ReachQueueFull` to a copy of the `.cfg`; for the properties of the second row
the line is `PROPERTY <name>`.

| Model | Properties |
|---|---|
| `Schedule`, as `INVARIANT` | `ReachPerson`, `ReachAuto`, `ReachDeletableAuto`, `ReachFailures`, `ReachSlots`, `ReachCatchUp`, `ReachQueueTwo`, `ReachQueueOrder`, `ReachQueueFull`, `ReachBudget`, `ReachTwoLive`, `ReachGoneStart`, `ReachGoneSeen` |
| `Schedule`, as `PROPERTY` | `ReachFailuresCleared`, `ReachQueue`, `ReachQueueIdle`, `ReachRestart`, `ReachRestartDay`, `ReachRestartStopping`, `ReachGoneQueued`, `ReachOffLive`, `ReachOffQueued`, `ReachKillCancelled`, `ReachKillOff`, `ReachKillLapsed`, `ReachKillDeleted`, `ReachKillEnded`, `ReachRecause`, `ReachFailedFrame`, `ReachFailedStart`, `ReachQueuedStale` |
| `Confirm4`, as `INVARIANT` | `ReachEffects`, `ReachSecond`, `ReachPresenters`, `ReachLostAck`, `ReachRestart`, `ReachReleasedRestart`, `ReachLapseRecorded`, `ReachOldApproval`, `ReachMoney`, `ReachPageRecord`, `ReachOtherEffect`, `ReachOtherApproval`, `ReachStopLapse`, `ReachStopPerson`, `ReachStoppedRequest`, `ReachStoppedRecord`, `ReachRejected` |

Three more properties are stronger than the contract's sentences, and TLC reports each one
violated in the model as it stands: `StaysLive` (no run of a workflow outside the list is
live at all), `EveryIdHasRow` (every approval id of a decision stands in a row) and
`RowsAreSent` (every stored row with `allow_once` belongs to an open item or to a released
decision).

The two control copies. Each is the model with one line replaced, and TLC must report the
named invariant violated.

- `Schedule` without the model's guard in step 4 breaks C1. Replace
  `ModelFree(s, a) == a \notin NeedsModel \/ \A r \in s.runs : r.agent \notin NeedsModel`
  by `ModelFree(s, a) == TRUE`.
- `Confirm4` without the wait of the move `record` breaks L5. Replace
  `ELSE IF s.items[i].st # "recorded" THEN No(s, "state")` by
  `ELSE IF s.items[i].st \notin {"answered", "recorded"} THEN No(s, "state")`.

## Wrong designs and the property that reports each

Each row is a coordinator or a guard that is wrong by I4 or I5, made from the model by an
exact replacement in the functions that `Step` calls; `Hist` and the invariants stay as they
are. TLC was run on each with every invariant of its configuration and reports the
property of the third column first. The last column counts the committed traces in which a
replay of that design gives another answer, event, call or state at some move.

| # | Wrong design | TLC reports | Traces that differ |
|---|---|---|---|
| 1 | a restart rebuilds the day's count of starts from this life only | C7 | 38 of 211 |
| 2 | a start that failed at step 7 or 8 is not counted | C7 | 74 of 211 |
| 3 | the budget counts only the person's starts | C7 | 91 of 211 |
| 4 | the budget counts only the scheduler's starts | C7 | 208 of 211 |
| 5 | the holds `failures` and `check` are lost at a restart | C4 | 4 of 211 |
| 6 | the scheduler starts an agent that never had a completed run | C5 | 149 of 211 |
| 7 | a restart forgets which workflows are gone | C8 | 13 of 211 |
| 8 | the kill comes only for runs stopped for `deleted` | C9 | 60 of 211 |
| 9 | an item in `recorded` lapses with any cause and its rows stay in the table | L4; L3 when the lapse's decision also holds the approval ids of the answer; L9, checked alone, when the guard then also counts a row before its release | 30 of 215 |
| 10 | the table takes the stored rows of a `no_record` lapse | L7, and not L9: the lapse's decision holds new ids | 5 of 215 |
| 11 | the guard passes every `releasing` record | L8 | 22 of 215 |
| 12 | a `releasing` record passes on an unused row | L8 | 1 of 215 |
| 13 | the table pairs each approval id with the other effect | L7 | 56 of 215 |
| 14 | the `approved` match does not ask for `allow_once` | L7 | 1 of 215 |
| 15 | a `releasing` record with an unused row passes | L8 | 1 of 215 |
| 16 | the `approved` match drops the effect id | L7 | 2 of 215 |
| 17 | step 3 asks only the live run, not the queue | C3 | 25 of 211 |
| 18 | the tick asks "needs the person" only | C5 | 149 of 211 |
| 19 | a person's start names no slot | C6 | 181 of 211 |
| 20 | a failed start is not counted | C7 | 74 of 211 |
| 21 | the tick does not skip an agent whose workflow is gone, and its start skips step 0; with step 0 in place that start is answered `state` and no property is broken | C8 | 11 of 211 |
| 22 | the rows say `reject_once`, the release sends `allow_once` | L5 | 88 of 215 |
| 23 | a lapse is taken from `recorded` with any cause | L4 | 30 of 215 |
| 24 | the guard's rule for `releasing` records is dropped | L8 | 22 of 215 |
| 25 | a `releasing` record of the class `send` passes on an `allow_once` row before its release | L10 | 1 of 215 |
| 26 | a `releasing` record of the class `send` passes on the approval of another effect | L10 | 3 of 215 |
| 27 | a `releasing` record of the class `send` passes with another digest | L10 | 2 of 215 |
| 28 | a tick queues a start of an agent that needs the person, under the trigger `person` | C5 | 105 of 211 |
| 29 | a run that was not stopped and whose last frame says `failed` is stored `cancelled` | C10 | 35 of 211 |
| 30 | a tick starts an agent that needs no person and whose workflow has no schedule | C11 | 138 of 211 |
| 31 | a tick starts for a slot that a served run has closed | C11 | 3 of 211 |
| 32 | a tick starts for a slot that is older than `catch_up_days` | C11 | 51 of 211 |
| 33 | a start of the scheduler leaves the queue without being asked for its slot again | C11 | 1 of 211 |
| 34 | the end of a run whose workflow is gone stores `run_ended` | C10 | 25 of 211 |
| 35 | an item that the person has answered lapses with `wait_limit` | L4 | 2 of 215 |
| 36 | a release from `answered`, before the acknowledgement, that writes the rows itself and then sends the decision | L5; L7, L8 and L10, each checked alone, once a record passes on those rows | 1 of 215 |
| 37 | the same release that sends the decision and then writes the rows | L5; L10 checked alone | 1 of 215 |
| 38 | a run that a lapse stopped is stored `cancelled` | C10 | 1 of 211 |
| 39 | the record stores one row of two, and the release sends both | L5 | 107 of 215 |

The count of row 21 is that of its first half alone: a replay in which the tick does not
skip the agent gives another line whether or not its start skips step 0. A design that
sends the event of row 34 again after the owner's `no_workflow` is row 34 at a later
tick: the models hold no answer `no_workflow`, and C10 reports the first sending.

## Limits

`limits.json` holds every field of `Limits`. A field that a model reads is smaller than
the product's value, but for `max_live_runs`. A field that no model reads holds the
product's value. `export.sh` fails when a value here differs from `Schedule.cfg`.

| Field | Model | Product | Constant of `Schedule.cfg` |
|---|---|---|---|
| `max_live_runs` | 2 | 1 | `MaxLive` |
| `queue_max` | 2 | 4 | `QueueMax` |
| `starts_per_day` | 2 | 6 | `StartsPerDay` |
| `fails_to_pause` | 2 | 3 | `FailsToPause` |
| `catch_up_days` | 0 | 7 | `CatchUpDays` |

The models hold no clock, so they read no limit that counts time. `cancel_grace_s` enters
the replay through the key `grace` of a tick.

## Traces

```sh
sh formal/r4-coordinator/export.sh --jar PATH [--out DIR] [--seed N --count N]
```

The export writes `traces-schedule.jsonl` and `traces-confirm.jsonl`, into this directory
unless `--out` names another. Without `--seed` and `--count` it takes both from the first
line of the committed `traces-schedule.jsonl`. It refuses with the causes `usage`, `java`
(no Java, or one older than 17) and `jar` (no file, or a SHA-256 other than the line
above). It exits 1 and writes neither file when TLC fails, when a file has not `count`
walks and every directed trace, when the walks of the schedule lack one of the answers
`budget`, `queue_full` and `queued`, when the last move of a directed trace is not the
move with the answer of the tables below, or when `limits.json` differs from
`Schedule.cfg`. It runs java with `-Xmx1g`. It works in a directory from `mktemp -d` and
leaves that directory in place.

The export writes both files or neither. It copies the two new files into the directory as
`.traces-schedule.jsonl.new` and `.traces-confirm.jsonl.new`, keeps a copy of the old
schedule file as `.traces-schedule.jsonl.kept`, renames the two new files and takes the
kept copy away. When a copy or a rename fails, it prints `export write=miss` with the step
that failed and exits 1; before that it takes away every file that it made and, after a
failed second rename, puts the old schedule file back. So after a failed export the two
trace files are as they were, and the directory holds no file that the export made. It
starts no write when a file already stands at one of its three names. If the old schedule
file cannot be put back, the line also says `restore=miss` and names the kept copy, which
then holds the old file.

A walk is at most 16 moves from the first state. It takes its steps from the model's own
`Step`. A generator in the trace module picks each move; TLC's random numbers and TLC's
order of sets are not used. In the committed files the model refuses 385 of the
3,200 moves of the schedule walks (12.0%) and 321 of the 1,946
moves of the confirm walks (16.5%); a confirm walk also takes a refusal whenever no
move is left that the model accepts. Two exports with one seed and count gave the same
bytes on the development Mac. The export is compared with the committed files in the jobs
`macos` and `windows`; before the first run of the check only the Mac was tried. One
export of both files took 8.69 s there under a load of 5.77, with a largest
resident size of 547,782,656 bytes.

The first line of a file is `{"seed":<n>,"count":<n>,"stamp":"<digest>"}`. The stamp is
the SHA-256, as 64 hex digits, of the bytes of `Schedule.tla`, `Schedule.cfg`,
`Confirm4.tla`, `Confirm4.cfg`, `ScheduleTrace.tla`, `Confirm4Trace.tla`, `TraceText.tla`
and `limits.json`, in this order, with every carriage return taken out, so that a checkout
with CRLF line ends gives the same stamp. Both files of one export hold the same stamp, and
the export prints it. Whoever reads the two files compares the two stamps first and refuses
a pair in which they differ: such a pair holds the traces of two versions of the models.
The check `30-r4-formal.sh` refuses it with the cause `traces`, and the test
`formal_traces` fails. A reader that has the model files can compute the stamp again and
so tell traces that were exported from other model files. Each further line is one trace,
`{"n":<number>,"moves":[{"m":"<move>","a":{...},"r":"<answer>","e":[...],"p":[...],"s":{...}}...]}`. The walks
have the numbers 1 to `count`. The directed traces follow them, with one more key after
`n`: `{"n":<number>,"case":"<name>","moves":[...]}`; `count` does not count them. `a`
holds what the move is given, `r` the answer, `e` the events that the move stored, `p` the
calls of the ports that it made, and `s` what the step returned and then the state after
it. Keys stand in the order of the tables below.

### Events and calls

`e` and `p` each stand in the order in which the move made their entries. An entry of `e`
is one event of I7.1 with its keys in I7.1's order; an event of a schedule trace names its
agent, and a confirm trace has one agent and names none.

| `event` | Keys after `event` | The line that stores it |
|---|---|---|
| `run_started` | `run`, `slot` (a day or null), `trigger`; `agent` stands before `event` | every start that passes step 6: a `start`, a start that a `tick` or an `end` takes from the queue, a start of the scheduler in a `tick` |
| `run_ended` | `run`, `cause`; `agent` before `event` | `end`; a `tick` for each live run that it kills; `restart` for each live run; a start under the port `no_run`, with `failed`. The end of a run whose workflow is gone stores none |
| `hold_set` | `cause`; `agent` before `event` | `off`, with `person`; the line of a `run_ended` event that completes the failure streak while the hold `failures` does not stand, with `failures` |
| `hold_cleared` | `cause` (`failures`); `agent` before `event` | a `start` of an agent under the hold `failures` whose workflow is in the list, also when the start is then refused |
| `item_raised` | `item` | `issue` with `digest_ok` `yes` |
| `item_answered` | `item`, `effect`, `approval`, `digest`, `answer`: one entry per effect | `record`: the coordinator sent the rows at the answer, and they count as stored when the acknowledgement comes; a `lapse` with `no_record` whose `stored` is `yes`, before its `item_lapsed` event: the rows that the store wrote without an acknowledgement |
| `item_released` | as `item_answered` | `release` |
| `item_lapsed` | `item`, `cause` | every `lapse` with `ok`; `issue` with `digest_ok` `no`, with `digest` |

An entry of `p` is `{"call":"<name>"}` with the keys of the table after it.

| `call` | Keys | Port method | The line that makes it |
|---|---|---|---|
| `defer` | none | `Analysis::defer` | every start that passes step 4, also one that then fails at step 6; a `tick` before which a run is live |
| `start_run` | `run` | `Owner::send` with the body `start_run` | every start that passes step 6 |
| `start` | `run` | `Runs::start` | the same; under the port `no_run` it is this call that fails |
| `cancel` | `run`; none in a confirm trace | `Runs::cancel` | `stop` with `ok`; `off` and a `tick`, for each run that they stop; a `lapse` with `wait_limit` or `cancelled` |
| `kill` | `run` | `Runs::kill` | a `tick` with `grace` `yes`, for each run that was `stopping` before it and for each stopped run whose runner ended before it |
| `reap` | none | `Runs::reap` | `restart` |
| `set_state` | `agent`, `state` | `Owner::send` with the body `set_state` | `paused` with every `hold_set` event; `active` with a `hold_cleared` event when the hold `off` does not stand |
| `answer_confirm` | `item`, `answer` (`allow`, `reject`) | `Runs::answer_confirm` | an `answer` with `ok`; with `reject`, a `lapse` from `shown` or `armed` of a run that still lives and an `issue` with `digest_ok` `no` |

The stored rows of an item stand in `e` of its `record` line, or of its lapse with
`no_record` and `stored` `yes`, and in no other line: every property that reads a stored
row reads those entries, and L5 sets the decision of the `release` line beside them, entry
for entry.

The lists hold no call of `Owner::log`: `e` holds what was stored, and under the port
`no_ack` the `run_started` event is not stored and `e` is empty. They hold no call that
only reads (`Owner::workflows`, `history` and `agents`, `Clock`, `Rng`) and no notice
(`Tray`, `Page`). A record that the guard passes on is the answer `ok` of its line.

The two lists do not say which entry of `e` came before which entry of `p`. The model
makes them in this order, and `formal_traces` holds the code to it on the memory ports:

- one start: `defer`; `run_started`; `start_run`; `start`; under `no_run` then `run_ended`
  with `failed`, and `hold_set` with `set_state` when the streak is complete;
- a `start` line: `hold_cleared` and `set_state` `active` first, then the start;
- one end: `run_ended`; `hold_set` with `set_state`; then the start that the end takes from
  the queue;
- a `tick`: `defer` when a run is live; `cancel` for the runs of gone workflows, by run
  number; by run number, `kill` and, for a run that was live, its end; the start from the
  queue; then the agents from `a1` to `a5`, each with its start;
- a `restart`: `reap`, then the ends by run number;
- `off`: `hold_set`, `cancel`, `set_state`;
- a `lapse`: `answer_confirm`, the decision of `sent`, `item_lapsed`, `cancel`;
- an `issue` with `digest_ok` `no`: `answer_confirm`, the decision of `sent`,
  `item_lapsed`; a `release`: the decision of `sent`, then `item_released`.

### What a replay rebuilds from earlier lines

A line shows the state after its move, but not everything that decides the next line.

- The cause of a run that is `stopping`: the `cause` of the `stop` line that stopped it,
  `cancelled` after `off`, and `deleted` from the first tick after the `delete` of its
  workflow on, also when the run was `stopping` before. It is the `cause` of the run's
  `run_ended` event unless a `restart` comes first; a run whose workflow is gone has no
  such event.
- The holds of an agent in `held`: `off` from its `off` line on; `failures` from a
  `hold_set` event with `failures` to the next `hold_cleared` event; `check` from a
  `run_ended` event with `effect_unknown` on. A `start` of the person ends `failures`
  alone.
- The trigger of each queued start: `person` for one that a `start` line with the answer
  `queued` put there, `schedule` for one of a tick's `started`. A start of the person
  stands before every start of the scheduler, each kind in the order of arrival. The
  trigger shows again in the `run_started` event.
- Run ids: one counter over the stored `run_started` events, from `r1`, across the lives
  of the app. A start that fails at step 6 takes no number.
- For the open slot: the day of each run's `run_started` line, the agents of which a run
  ended `completed` (those of the table and each one with such a `run_ended` event), and
  for each agent the day of its newest run that ended `completed` or `nothing_to_do`.
- The stopped runs whose `end` line came before a tick with `grace` `yes`: that tick
  calls `kill` for each, and a `restart` forgets them.
- For each approval id of `table`: its item, its effect, its digest and its answer, from
  the `item_answered` entries of the `record` line; it counts for a record from the
  `release` line of its item on.
- Approval ids: one counter, as the section on confirm traces says.

The order of every list. `runs`, `stopped` and `ended` stand by run number; `queue` with
its head first; `ready`, `held`, `gone`, `raised` and `started` by agent number; `starts`,
`slot` and `fails` for `a1` to `a5`; `items` by item number; `effects`, `sent`,
`approvals` and the entries of one `item_answered` or `item_released` event by effect
number; `table` and `used` by approval number; `e` and `p` as made.

A short id is a letter and a number. Its 32-digit id is the hex of its ASCII bytes with
zeros in front; a digest has 64 digits by the same rule.

| Short id | Kind | Id |
|---|---|---|
| `a1` | agent | `00000000000000000000000000006131` |
| `w1` | the workflow of `a1` | `00000000000000000000000000007731` |
| `r12` | run | `00000000000000000000000000723132` |
| `i1` | item | `00000000000000000000000000006931` |
| `e1` | effect | `00000000000000000000000000006531` |
| `p3` | approval; `p0` is an approval id that no decision holds | `00000000000000000000000000007033` |
| `d1` | digest | `0000000000000000000000000000000000000000000000000000000000006431` |

### Schedule traces

The replay starts on day 1 with no run, an empty queue, no item, no hold and every
workflow in the owner's list. Day 1 is any day that the replay picks. Every run has
`unknown` 0 but one that ended `effect_unknown`, which has 1.

| Agent | Needs the person | Needs the model | Workflow | Before the trace |
|---|---|---|---|---|
| `a1` | yes | yes | trigger `schedule`, period 2, `last` on day -1 | one run, `completed` on day -1 |
| `a2` | no | yes | the same | the same |
| `a3` | no | no | the same | no run |
| `a4` | yes | no | trigger `manual` | one run, `completed` on day -1 |
| `a5` | no | no | trigger `manual` | the same |

After the keys of its move, every `s` holds the state, in this order: `day`; `runs`, the
live runs with their phases, by run number; `queue`, the agents whose starts wait, the
head first; `ready`, the agents with an open `ready` item; `held`, the agents for which
`holds` returns a hold; `starts`, for the agents `a1` to `a5` in that order the `run_started`
events of the day; `slot`, in the same order the newest slot that a `run_started` event
names, or null; `fails`, in the same order the failure streak, which stops at
`fails_to_pause`; `gone`, the agents
whose workflow is not in the owner's list; `life`, the life of the app, from 1.

| `m` | Keys of `a` | `r` | Keys of `s` before the state | Function |
|---|---|---|---|---|
| `start` | `agent`, `trigger` (always `person`), `port` | `ok`, `queued`, `hold`, `budget`, `state`, `queue_full`, `no_service` | `run`: the run whose `run_started` event was stored, or null | `start_step`, then steps 5 to 8 |
| `hello`, `wait`, `resume` | `run` | `ok`, `state` | none | `run_step` |
| `stop` | `run`, `cause`: `cancelled` for the person's stop, `lapsed` for a lapse with `wait_limit` | `ok`, `state` | none | `run_step` |
| `end` | `run`, `frame`: the outcome of the runner's last frame, or null when the runner went without one; `port` | `ok`, `state` | with `ok`: `cause`, the cause of the end, which is stored unless the workflow is gone, and `next` | `run_step` |
| `tick` | `day`, `grace`, `port` | `ok` | `stopped`, the runs that the tick stopped because their workflow is gone; `ended`, the runs that the kill ended; `next`, the start that the tick took from the queue; `raised`, the agents that get a `ready` item; `started`, the scheduler's starts in the order of the agents | `tick_step` with `delete_step` |
| `restart` | `day`, the day that the new life reads | `ok` | `ended` | `recover_step` |
| `delete` | `agent`, whose workflow leaves the owner's list | `ok` | none | none: it changes the list that `Owner::workflows` gives |
| `off` | `agent` | `ok` | `stopped`, the run that the control stopped | `holds` with `run_step` |

A `start` with `ok` holds the move `spawn` of I5.6: the run is `starting` in `s`. A start
of an agent whose workflow is gone is answered `state`. `port` says what the ports do
during the line: `ok`; `no_ack`, where the `run_started` event gets no acknowledgement
(step 6), nothing is stored and the answer is `no_service`; `no_run`, where `Runs::start`
fails (step 8), `run_started` and `run_ended` with `failed` are stored and the answer is
`no_service`. A failure of step 7 stores the same two events and is in no trace. In a `tick` and an `end` the port holds for every start that the line makes.

An entry of `next` and of `started` is `{"agent","r","run"}`: a start that the line made
for that agent, with the answer of `start_step` or `no_service`, and its run or null. In
`next`, `queued` means that the start stays at the head of the queue; `hold`, `budget`,
`state` and `no_service` mean that it left the queue. `state` is also the answer for a
start of the scheduler whose slot is no longer due. An entry of `ended` is
`{"run","cause","next"}`. `next` of a tick is empty or one entry.

The clock of the replay. A `tick` sets the day to `a.day`. Before a `tick` with `grace`
`yes` the clock goes on by `cancel_grace_s`; at every other line it stands still. So a
tick with `grace` `yes` kills every run that was `stopping` before it, and one with
`grace` `no` kills none. A `tick` does its work in this order: the day; the workflows
that are gone (the stop of their runs, their queued starts, their `ready` items); the
kills by run number, each with the start that its end takes from the queue; one start
from the queue when the tick killed nothing and no run is live; then the agents from `a1`
to `a5`. A `restart` ends the runs by run number.

The directed traces, each with its last move:

| `case` | What it holds | Last move | `r` |
|---|---|---|---|
| `two_models` | `a1` holds the model; a start of `a2` waits although a second run may be live | `start` | `queued` |
| `two_runs_agent` | a start of an agent with a queued start, then of one with a live run | `start` | `state` |
| `hold_start` | a tick that starts nothing for `a2` under `failures`, the person's start that ends that hold, a start under `off`, and a start under `check` | `start` | `hold` |
| `unrecorded_start` | a start whose `run_started` event is not acknowledged; no run is stored | `start` | `no_service` |
| `second_slot_run` | a tick starts `a2` for its slot, a restart, and a second tick that starts nothing: `started` is empty | `tick` | `ok` |
| `auto_needs_person` | a tick with the open slot of `a1`: `raised` holds `a1`, and no run of it starts | `tick` | `ok` |
| `over_budget` | a third start of `a4` on one day, after a run that ended `completed` and a run that waited, was stopped with `lapsed` and ended with that cause | `start` | `budget` |
| `queue_full` | two live runs, two queued starts, and a start of `a5` | `start` | `queue_full` |
| `outlives_app` | a restart with a live run and a run that was stopped with `cancelled`: `ended` holds both with `interrupted` | `restart` | `ok` |
| `stop_tree` | a stop, a tick with `grace` `no` that ends nothing and queues the scheduler's start of `a2`, and the tick of the next day that kills: `ended` holds the run with `cancelled`, and its `next` holds `a2` with `state`, because the slot of day 1 is a day old | `tick` | `ok` |
| `deleted_workflow` | a delete, the tick that stops the run, and the tick that kills it: `ended` holds the run with `deleted`, and `e` holds no `run_ended` event | `tick` | `ok` |

### Confirm traces

A confirm trace has one run of one agent. A walk ends before 16 moves when the app ended,
or when nothing is left but refusals and records that every guard passes.

After the keys of its move, every `s` holds the state: `items`, each item with its state;
`table`, the approval ids of the rows that the guard's table holds, also before their
release; `used`, those that a record used; `run`, which is `live`, `stopped` or `ended`.

| `m` | Keys of `a` | `r` | Keys of `s` before the state | Function |
|---|---|---|---|---|
| `issue` | `effects`, each `{"effect","class","digest"}`; `digest_ok` | `ok`, `state` | `item`, the new item or null; `sent` | `confirm_step` |
| `arm` | `item` | `ok`, `state` | none | `confirm_step` |
| `answer` | `item`, `answer` (`allow`, `reject`), `from` (`page`, `native`) | `ok`, `stale`, `presenter`, `early` | `approvals`, the ids that were minted, in the order of the effects | `confirm_step` |
| `record` | `item` | `ok`, `state` | none | `confirm_step` |
| `release` | `item` | `ok`, `state` | `sent` | `confirm_step` |
| `lapse` | `item`, `cause`, `stored` | `ok`, `state` | `sent` | `confirm_step` |
| `stop` | none | `ok`, `state` | none | `run_step` |
| `guard` | `effect`, `class`, `state`, `digest`, `approval` | `ok`, `approval` | none | `guard_step` |

`sent` is the decision that the line sent to the runner, one
`{"effect","digest","approval","answer"}` per effect, or an empty list. The port mints
approval ids from a counter, `p1` first, in the order of the effects: at an accepted
`answer`, at a lapse that sends a decision, and at an `issue` with `digest_ok` `no`. Such
an `issue` makes an item that is `lapsed` with the cause `digest`, and sends
`reject_once`. An `issue` with the answer `state` came while an item was open, and then
`run` is `ended` and the next line is the lapse of that item with `run_ended`; or it came
after the run was stopped, and then nothing changes. `stored` is `yes` or `no` in a lapse
with `no_record`, and says whether the store wrote the rows although no acknowledgement
came, and with `yes` the line's `e` holds those rows; it is null in every other lapse, and
`confirm_step` does not see it. A `stop` is the
person's stop while no item is open; with an open item the trace holds the lapse of that
item with `cancelled`. A trace has no line for a restart: the app's end is the lapse of
the open item with `app_ended`, and no line follows it. The records of a `guard` line have
the states `approved`, `releasing` and `committed`, and name any effect with any digest
and any approval id that was minted.

The directed traces. Each of the eight `reason_` traces and `guard_releasing_other` ends
with a `guard` line that is answered `approval`; before it, the first request was allowed
and released with the approvals `p1` and `p2`, but in `reason_reject_row`, where it was
rejected and released. `guard_releasing_other` differs from `reason_effect` in the state
of its record and in what the record takes from the other effect: the approval id alone,
with its own effect and its own digest.

| `case` | What it holds | Last move | `r` |
|---|---|---|---|
| `answer_twice` | a second `answer` on an item that is `answered`, then `recorded`, then `released` | `answer` | `stale` |
| `answer_early` | an `answer` before `arm` | `answer` | `early` |
| `answer_page_native` | an `answer` from the page for an item of the presenter `native` | `answer` | `presenter` |
| `timer_allow` | `arm` and a lapse with `wait_limit`: `sent` holds `reject_once` only | `lapse` | `ok` |
| `no_record` | an answer with `allow`, a release before the record and a lapse with `wait_limit`, each answered `state`, then the lapse with `no_record`: `sent` holds `reject_once` under new ids | `lapse` | `ok` |
| `digest_shown` | an `issue` with `digest_ok` `no` | `issue` | `ok` |
| `reason_unknown_id` | `approved` with `p0` | `guard` | `approval` |
| `reason_used_row` | `approved` with `p1` a second time | `guard` | `approval` |
| `reason_digest` | `approved` for `e1` with `p1` and the digest `d3` | `guard` | `approval` |
| `reason_effect` | `approved` for `e2` with the digest and the approval of `e1` | `guard` | `approval` |
| `reason_reject_row` | `approved` with the id of a released `reject_once` row | `guard` | `approval` |
| `reason_releasing_no_row` | `releasing` of the payment with `p0` | `guard` | `approval` |
| `reason_releasing_unused_row` | `releasing` of the payment with `p2`, which no `approved` record used | `guard` | `approval` |
| `reason_stopped_run` | `stop`, then `approved` with the released, unused `p1` | `guard` | `approval` |
| `guard_releasing_other` | `releasing` of `e1`, of the class `send`: with `p1` before the release (`approval`) and after it (`ok`), with `p1` and the digest `d3` (`approval`), and last with its digest `d1` and `p2`, the approval of `e2` | `guard` | `approval` |

## Keep the models true to the code

The models cover `holds`, `start_step` and `tick_step`, `run_step`, `recover_step` and
`delete_step`, `confirm_step` and `guard_step` of `halvo-coord`. None of them exists at
the commit that adds this file. The crate's test `formal_traces` replays every committed
line. When TLC reports an invariant violated, read its trace against I4 and I5: a step
that the contract does not allow is a fault of the model, and a trace of allowed steps is
a finding about the design. When the test fails, the code and the model differ: decide
which one the contract supports, and change the other.
