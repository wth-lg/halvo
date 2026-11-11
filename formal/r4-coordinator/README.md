# The coordinator's models

This directory holds two TLA+ models of the coordinator that the crate `halvo-coord` will
implement, and the traces that the crate's tests replay. `Schedule` is the machine that
decides when a run starts, how it moves and how it ends. `Confirm4` is the life of a
confirmation and the guard on the runner's effect records. The models were written before
the code. A change to a rule changes the model first; then the traces are exported again
and committed. Nobody edits a trace file by hand.

| File | What it holds |
|---|---|
| `Schedule.tla`, `Schedule.cfg` | the schedule model, its configuration and the invariants C1 to C8 |
| `Confirm4.tla`, `Confirm4.cfg` | the confirmation model, its configuration and the invariants L1 to L7 |
| `ScheduleTrace.tla`, `Confirm4Trace.tla`, `TraceText.tla` | the modules that walk a model and print one JSON line per trace |
| `limits.json` | the limits of the models, with the field names of the struct `Limits` |
| `export.sh` | the command that writes the two trace files |
| `traces-schedule.jsonl`, `traces-confirm.jsonl` | 200 traces of each model |

## The checker

The models are checked by TLC, which runs from `tla2tools.jar` of the tlaplus project's
release v1.7.4. On 2026-11-11 the address
`https://github.com/tlaplus/tlaplus/releases/latest/download/tla2tools.jar` led to that
release. GitHub gives that address to the newest release that is neither a draft nor a
pre-release, so a pre-release is never the pinned one. The jar is not in this repository.
Every command here takes its path with `--jar`.

```
jar url=https://github.com/tlaplus/tlaplus/releases/download/v1.7.4/tla2tools.jar bytes=2274532 sha256=936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88
```

The licence, read from the jar itself: `License.txt` is the MIT License, with the line
"Copyright (c) 2019 Microsoft Research". The jar also holds the classes `javax/mail`,
`javax/activation` and `com/sun/mail`, and `META-INF/LICENSE.txt` is the text of the
Eclipse Public License 2.0. Nothing in this repository links the jar or holds a copy of it.

TLC needs Java 17 or later and nothing else. No other tool turns TLC's output into JSON
lines: the trace modules build each line as a TLA+ string and print it with TLC's
`PrintT`, and `export.sh` takes the quotes off with `sed`.

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

| Model | Distinct states | States generated | Depth | Time, one worker | Largest resident size |
|---|---|---|---|---|---|
| `Schedule` | 43,316 | 1,356,211 | 11 | 16.4 s | 704,184,320 bytes |
| `Confirm4` | 943 | 47,828 | 14 | 0.9 s | 199,704,576 bytes |

The counts are TLC's own lines. The times and sizes are from one run each on the
development Mac (arm64, JDK 17.0.20.1) on 2026-11-11, through the machine's helper for
timed runs, read with `/usr/bin/time -l`. The heap bound `-Xmx2g` keeps a run far below
the machine's cap of 8,589,934,592 bytes. With `-workers auto` TLC finds the same states
in less time; the depth that it prints can then differ.

## Schedule

The state is one record `st`. A move is a record, and `Step(st, move)` gives the answer
and the next state, so one step of the model is one call of the coordinator. A refused
move leaves the state as it was.

| Part of `st` | Meaning | Item |
|---|---|---|
| `day` | the day of the coordinator's clock; only a tick changes it | I5.2, I5.9 |
| `life`, `nrun`, `dels` | the life of the app, the run ids handed out, the workflows deleted | I5.8 |
| `runs` | the live runs: id, agent, phase, the cause that a stop gave, trigger, the day and the life of the start | I5.6 |
| `queue` | the starts that wait, a person's before a scheduler's | I5.5 |
| `ready` | the agents with an open `ready` item | I5.3 |
| `off`, `fails`, `check` | the holds `off`, `failures` and `check` | I5.4 |
| `gone` | the agents whose workflow is gone | I5.7 |
| `fin`, `done`, `starts`, `slotrun` | what the stored events say: a finished run, the day of the newest run that ended `completed` or `nothing_to_do`, the starts of each day, the newest slot that a `run_started` event names | I5.2, I5.3, I5.5 |
| `scount`, `stale`, and `ack`, `held` of a run | history, read only by the invariants: the scheduler's starts for each slot, a run of a gone workflow that outlived a tick, and how a run started | I5.12 |

The configuration has four agents, the slots of day 1 and day 3 (`FirstSlot`, `Period`),
three days, one restart and one deleted workflow. Agent 1 needs the person and the model.
Agent 2 needs the model and no person. Agent 3 needs neither, has no finished run, and is
the agent whose hold `off` comes and goes. Agent 4 needs the person and no model, has no
slot, and is the agent whose workflow can go. Two agents need the one model and two do
not, so both halves of step 4 of I5.5 decide: the model's guard and the count of live runs.

| Invariant | What it says in the model |
|---|---|
| C1 | at most one live run is of an agent that needs the model, and at most `MaxLive` runs are live |
| C2 | every live run was started in the current life |
| C3 | an agent's live runs and queued starts are at most one together |
| C4 | every live run started with an acknowledged `run_started` event and with no hold |
| C5 | every live run of an agent that needs the person has the trigger `person` |
| C6 | no slot has two starts by the scheduler |
| C7 | no agent has more than `StartsPerDay` starts on any day |
| C8 | a run whose workflow is gone is `stopping`, and none is live after a tick at which the grace has passed |

How the model reads the places that I5 leaves open. The traces follow these readings, so
the code has to follow them too until the contract rules otherwise.

1. A finished run is a run with a `run_ended` event of any cause. A start whose step 7 or
   8 fails counts as one, and as a run that ended `failed`.
2. A tick raises no `ready` item for an agent that has a live run, a queued start, a hold
   or an open `ready` item.
3. A start by the person names the current slot when that slot is open and the agent has
   a finished run, and no slot otherwise. The model keeps the named slot only for agents
   that the scheduler may start.
4. The day of a run, for the rule of the open slot, is the day of its start.
5. A start that leaves the queue is dropped when its step 6, 7 or 8 fails.
6. One end takes one start from the queue. If that start is dropped, the next waits for
   the next end.
7. A run that ends with a last frame keeps the outcome of that frame, also while it is
   `stopping`. A kill, and an end with no frame, give a `stopping` run the cause of its
   stop, and any other run `interrupted`.
8. A deleted workflow takes its queued start and its `ready` item with it, and its run
   ends with `deleted` also when the run was already `stopping`.
9. A restart empties the queue and closes every `ready` item.
10. A `ready` item closes when a start of its agent is stored or queued. It stays open
    when the start is refused.
11. The day, and the hold `off`, change only at a tick. The holds `failures` and `check`
    are never cleared, because no move of a trace is the person's answer to them.
12. The kill comes at a tick at which `cancel_grace_s` have passed for every stop.

What the search does not cover. It follows a behaviour until its second stored start
(`MaxRuns`) and checks the state after a third. It ends runs with one frame of each class
(`completed`, `failed`, and no frame) and stops them with the cause `cancelled` (`Frames`,
`StopCauses`); the traces use all nine outcomes and both causes. `VIEW` takes `ready` and
the phases `starting`, `working` and `waiting` out of the fingerprint, because no rule of
the schedule reads them. `QueueMax` is 1, so the order of two queued starts is not seen.
The hold `check` comes only from a frame `effect_unknown`, which only the traces use. The
model has no `skip`, no items budget, no effect in `releasing`, and no time but the grace.

## Confirm4

The state holds the items of one run, the rows in the store, the guard's table, the
decisions that went to the runner, and whether the run and the app still live. The run
sends two requests. The first names the effects 1 (class `send`) and 2 (class `payment`),
so its presenter is `native`. The second names effect 1 with another digest, so its
presenter is `page`.

| Invariant | What it says in the model |
|---|---|
| L1 | a decision or a row holds `allow_once` only for an item whose accepted answer was `allow`, on an `armed` item, from the item's presenter |
| L2 | each digest of a released decision is the digest that the item showed for that effect |
| L3 | no approval id stands in two decisions or in two rows, each row's id stands in the decision of its item, and an item sends one decision |
| L4 | an item that reached `released` or `lapsed` is still in that state |
| L5 | when a decision was released, the guard's table held its rows |
| L6 | a decision that a lapse sent holds only `reject_once` |
| L7 | every record `approved` that the guard passed had an unused row with its effect, digest and approval id and the answer `allow_once`, and no approval id passed twice |

How the model reads the places that I4 leaves open:

1. A lapse sends the runner `reject_once` for every effect, with the approval ids of the
   answer when there was one and with new ids otherwise. No row holds the new ids.
2. The guard's table holds the rows whose acknowledgement the coordinator saw (the move
   `record`). Rows that the store wrote without an acknowledgement are in the store and
   not in the table.
3. A request while an item is open is refused with `state` and ends the run; the open
   item lapses with `run_ended` in the next step.
4. After a lapse with `wait_limit` or `cancelled` the runner may still send a request or
   an effect record.
5. A record `releasing` of a class with the presenter `native` passes only with a used
   row of the same effect, digest and approval id.
6. `confirm_step` takes a lapse with any cause from `shown`, `armed` or `answered`. The
   traces hold only the causes that can arise: `no_record` after an answer, `digest` at
   `issue`.

## Reachability and controls

A property whose name starts with `Reach` says that some part of the configuration never
does its work. TLC must report each one violated. To run one, add the line
`INVARIANT ReachQueueFull` to a copy of the `.cfg`; for `ReachHold`, `ReachSlots`,
`ReachQueue`, `ReachRestart`, `ReachDelete` and `ReachDeleted` of `Schedule` the line is
`PROPERTY <name>`.

| Model | Properties |
|---|---|
| `Schedule` | `ReachPerson`, `ReachAuto`, `ReachHold`, `ReachFailures`, `ReachSlots`, `ReachCatchUp`, `ReachQueue`, `ReachQueueFull`, `ReachBudget`, `ReachTwoLive`, `ReachRestart`, `ReachDelete`, `ReachDeleted` |
| `Confirm4` | `ReachEffects`, `ReachSecond`, `ReachPresenters`, `ReachLostAck`, `ReachRestart`, `ReachOldApproval`, `ReachMoney` |

Four more properties are stronger than the contract's sentences, and TLC reports each one
violated in the model as it stands: `StaysLive` (no run of a gone workflow is live at
all), `PassedAfterRelease` (the guard passes a record only after its decision was
released), `EveryIdHasRow` (every approval id of a sent decision stands in a row) and
`RowsAreSent` (every row with `allow_once` belongs to a released decision).

The two control copies. Each is the model with one line replaced, and TLC must report the
named invariant violated.

- `Schedule` without the model's guard in step 4 breaks C1. Replace
  `ModelFree(s, a) == a \notin NeedsModel \/ \A r \in s.runs : r.agent \notin NeedsModel`
  by `ModelFree(s, a) == TRUE`.
- `Confirm4` without the wait of the move `record` breaks L5. Replace
  `ELSE IF s.items[i].st # "recorded" THEN [r |-> "state", st |-> s]` by
  `ELSE IF s.items[i].st \notin {"answered", "recorded"} THEN [r |-> "state", st |-> s]`.

## Limits

`limits.json` holds every field of `Limits`. A field that a model reads is smaller than
the product's value, but for `max_live_runs`. A field that no model reads holds the
product's value. `export.sh` fails when a value here differs from `Schedule.cfg`.

| Field | Model | Product | Constant of `Schedule.cfg` |
|---|---|---|---|
| `max_live_runs` | 2 | 1 | `MaxLive` |
| `queue_max` | 1 | 4 | `QueueMax` |
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
above). It exits 1 and writes no trace file when TLC fails, when a file has not `count`
traces, when the schedule traces lack one of the answers `budget`, `queue_full` and
`queued`, or when `limits.json` differs from `Schedule.cfg`. It runs java with `-Xmx1g`.
It works in a directory from `mktemp -d` and leaves that directory in place.

A trace is a walk of at most 16 moves from the first state. The walk takes its steps from
the model's own `Step`. A generator in the trace module picks each move; TLC's random
numbers and TLC's order of sets are not used. About one move in eight is one that the
model refuses. Two exports with one seed and count gave the same bytes on the development
Mac. The export is compared with the committed files in the jobs `macos` and `windows`;
before the first run of the check only the Mac was tried. One export of both files took
8.4 s there, with a largest resident size of 477,167,616 bytes.

The first line of a file is `{"seed":<n>,"count":<n>}`. Each further line is one trace,
`{"n":<count>,"moves":[{"m":"<move>","a":{...},"r":"<answer>","s":{...}}...]}`. `a` holds
what the move is given, `r` the answer, and `s` what the step did and then the state after
it. Keys stand in the order of the tables below.

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

The replay starts on day 1 with no run, an empty queue, no item and no hold. Day 1 is any
day that the replay picks. Every run has `unknown` 0 but one that ended `effect_unknown`,
which has 1.

| Agent | Needs the person | Needs the model | Workflow | Before the trace |
|---|---|---|---|---|
| `a1` | yes | yes | trigger `schedule`, period 2, `last` on day -1 | one run, `completed` on day -1 |
| `a2` | no | yes | the same | the same |
| `a3` | no | no | the same | no run |
| `a4` | yes | no | trigger `manual` | one run, `completed` on day -1 |

After the keys of its move, every `s` holds the state: `day`; `runs`, the live runs with
their phases, by run number; `queue`, the agents whose starts wait, the head first;
`ready`, the agents with an open `ready` item; `held`, the agents for which `holds`
returns a hold.

| `m` | Keys of `a` | `r` | Keys of `s` before the state | Function |
|---|---|---|---|---|
| `start` | `agent`, `trigger` (always `person`), `port` | `ok`, `queued`, `hold`, `budget`, `state`, `queue_full`, `no_service` | `run`: the run whose `run_started` event was stored, or null | `start_step`, then steps 5 to 8 |
| `hello`, `wait`, `resume` | `run` | `ok`, `state` | none | `run_step` |
| `stop` | `run`, `cause`: `cancelled` for the person's stop, `lapsed` for a lapse with `wait_limit` | `ok`, `state` | none | `run_step` |
| `end` | `run`, `frame`: the outcome of the runner's last frame, or null when the runner went without one; `port` | `ok`, `state` | with `ok`: `cause`, the stored cause, and `next` | `run_step` |
| `tick` | `day`; `off`, the agents with the hold `off` from now on; `grace`; `port` | `ok` | `ended`, the runs that the kill ended; `raised`, the agents that get a `ready` item; `started`, the scheduler's starts in the order of the agents | `tick_step` |
| `restart` | none | `ok` | `ended` | `recover_step` |
| `delete` | `agent`, whose workflow is gone | `ok` | `stopped`, the runs that were stopped | `delete_step` |

A `start` with `ok` holds the move `spawn` of I5.6: the run is `starting` in `s`. `port`
says what the ports do during the line: `ok`; `no_ack`, where the `run_started` event gets
no acknowledgement (step 6), nothing is stored and the answer is `no_service`; `no_run`,
where step 7 or 8 fails, `run_started` and `run_ended` with `failed` are stored and the
answer is `no_service`. In a `tick` and an `end` the port holds for every start that the
line makes.

An entry of `next` and of `started` is `{"agent","r","run"}`: a start that the line made
for that agent, with the answer of `start_step` or `no_service`, and its run or null. In
`next`, `queued` means that the start stays at the head of the queue; `hold`, `budget`,
`state` and `no_service` mean that it left the queue. An entry of `ended` is
`{"run","cause","next"}`.

The clock of the replay. A `tick` sets the day to `a.day`. Before a `tick` with `grace`
`yes` the clock goes on by `cancel_grace_s`; at every other line it stands still. So a
tick with `grace` `yes` kills every `stopping` run, and one with `grace` `no` kills none.
A `tick` does its work in this order: the day and the holds `off`, the kills by run
number, then the agents from `a1` to `a4`. A `restart` ends the runs by run number.

### Confirm traces

A confirm trace has one run of one agent. It ends before 16 moves when the app ended, or
when nothing is left but refusals and records that every guard passes.

After the keys of its move, every `s` holds the state: `items`, each item with its state;
`table`, the approval ids of the rows that the guard holds; `used`, those that a record
used; `run`, `live` or `ended`.

| `m` | Keys of `a` | `r` | Keys of `s` before the state | Function |
|---|---|---|---|---|
| `issue` | `effects`, each `{"effect","class","digest"}`; `digest_ok` | `ok`, `state` | `item`, the new item or null; `sent` | `confirm_step` |
| `arm` | `item` | `ok`, `state` | none | `confirm_step` |
| `answer` | `item`, `answer` (`allow`, `reject`), `from` (`page`, `native`) | `ok`, `stale`, `presenter`, `early` | `approvals`, the ids that were minted, in the order of the effects | `confirm_step` |
| `record` | `item` | `ok`, `state` | none | `confirm_step` |
| `release` | `item` | `ok`, `state` | `sent` | `confirm_step` |
| `lapse` | `item`, `cause`, `stored` | `ok`, `state` | `sent` | `confirm_step` |
| `guard` | `effect`, `class`, `state`, `digest`, `approval` | `ok`, `approval` | none | `guard_step` |

`sent` is the decision that the line sent to the runner, one
`{"effect","digest","approval","answer"}` per effect, or an empty list. An `issue` with
`digest_ok` `no` makes an item that is `lapsed` with the cause `digest`, and sends
`reject_once`. An `issue` with the answer `state` came while an item was open: `run` is
`ended`, and the next line is the lapse of that item with `run_ended`. `stored` is `yes`
or `no` in a lapse with `no_record`, and says whether the store wrote the rows although no
acknowledgement came; it is null in every other lapse, and `confirm_step` does not see it.
A trace has no line for a restart: the app's end is the lapse of the open item with
`app_ended`, and no line follows it. The records of a `guard` line have the states
`approved`, `releasing` and `committed`.

## Keep the models true to the code

The models cover `holds`, `start_step` and `tick_step`, `run_step`, `recover_step` and
`delete_step`, `confirm_step` and `guard_step` of `halvo-coord`. None of them exists at
the commit that adds this file. The crate's test `formal_traces` replays every committed
line. When TLC reports an invariant violated, read its trace against I4 and I5: a step
that the contract does not allow is a fault of the model, and a trace of allowed steps is
a finding about the design. When the test fails, the code and the model differ: decide
which one the contract supports, and change the other.
