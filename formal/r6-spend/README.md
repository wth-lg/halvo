# The model of the spend rule

This directory holds the model of the routing run's spend rule (contract T7.6 of that run).
A hosted request costs money. Two things keep that spend inside what the person allowed: no
request leaves the device without a grant, and before a request is dispatched the gateway takes
a hold at the ledger. The model writes down the device's side and the machine of one hold, and
TLC, a model checker, tries every order of steps of a small configuration.

TLC is `tla2tools.jar` of the tlaplus project's release v1.7.4, the release that the billing run
pins, so that the runs check with one file. The export script and the check fetch that one file
and compare its size and its SHA-256 with this line:

tlc url=https://github.com/tlaplus/tlaplus/releases/download/v1.7.4/tla2tools.jar bytes=2274532 sha256=936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88

The jar is a check tool. Nothing links it or ships it, and it is never committed.

## The files

| file | what it holds |
|---|---|
| `Hold.tla` | The machine of one hold, the two tables of T7.2 as the operator `Step(account, hold, move)`. It has no variable. `halvo_route::spend::step` must answer as it does. |
| `Spend.tla` | The system of T7.6 (`Init`, `Next`), its eleven invariants `S1` to `S11`, and the machine of one hold alone for the traces (`XInit`, `XNext`) with its directed traces (`Directed`). |
| `Spend.cfg` | The system at the bounds of T7.6, with the eleven invariants. |
| `ControlRuling.cfg`, `ControlResend.cfg` | The two controls. Each differs from `Spend.cfg` in the one constant `Fault` and lists one invariant, which TLC must report as violated. |
| `TwoLosses.cfg` | `Spend.cfg` with two lost answers at a time (`MaxLostAnswers = 2`), the bound that the two designs with a third send need. |
| `Export.cfg` | The machine of one hold alone, which `export.sh` runs in TLC's simulation mode. |
| `export.sh` | Writes the traces in the line forms below. `sh formal/r6-spend/export.sh --help` states its call. |
| `traces.jsonl` | The committed traces: `sh formal/r6-spend/export.sh --seed 3 --count 206 --out FILE` writes these bytes. Never edit it by hand. |
| `Replay.tla`, `Replay.cfg` | Replays a traces file against `Step` in one TLC run. |
| `Reach.tla` | Properties that TLC must violate, one for each thing that the system has to reach, and the invariant `OnlyFourCauses`. |

## A trace line and `spend::step`

The first line of `traces.jsonl` is `{"seed":<n>,"count":<n>}`. Each further line is one trace:

`{"cap":<n>|null,"active":"yes"|"no","spent":<n>,"held":<n>,"moves":[{"move":"<move>","arg":<n>|null,"answer":"ok"|"<cause>","state":"<state>","spent":<n>,"held":<n>,"cost":<n>}...]}`

Every amount of the file is in minor units, and `spend::step` counts in micro-units: multiply
`arg`, `spent`, `held` and `cost` by 10,000 (`MICRO_PER_MINOR`). `cap` is `cap_minor` as it
stands, and `null` is no cap. To replay a trace:

1. Start with `Account { cap_minor: cap, active: active == "yes", spent, held }` from the four
   keys of the line, and with `Hold::new()`. The `held` of the line is the sum of other holds.
2. For each element of `moves`, in order, call `step(&account, &hold, mv)`. `mv` is
   `Reserve { amount: arg }` for `reserve`, `Settle { cost: arg }` for `settle`, and `Dispatch`,
   `Void` or `Expire` for the three moves whose `arg` is `null`.
3. With the answer `ok` the call returns the next account and hold: the hold's state is `state`,
   its `cost` is `cost`, and the account's `spent` and `held` are those of the element. The next
   move starts from them.
4. With any other answer the call returns that cause. The element then repeats the state, the
   `spent`, the `held` and the `cost` from before the move, and the next move starts from the
   same values.

`cost` is what the hold stores after the move: the argument of a `settle` that went through, the
hold's amount after an `expire`, and 0 before either and after a `void`. A trace does not state
the hold's `amount`. The answers `repeat` and `other_arguments` show that it is kept, and so
does the `cost` after an `expire`.

A file holds two kinds of trace, in one line form, and `count` counts both. The first six are
the directed traces that `Spend.tla` lists as `Directed`. Each has five moves: a reserve, a
dispatch, a settle that goes through, then a reserve with the same amount, answered `repeat`,
and a second settle, answered `state`. Two of the six settle below the hold's amount, two at it
and two at 0. The others are drawn by TLC and have 12 moves each. The committed file holds 200
drawn traces; among them are a `reserve` of 0 and a `settle` whose cost is above the hold's
amount, which goes through.

## What each configuration checks

`Spend.cfg` is the system. The first state sets the ruling to `none` or `hosted` and the cap to
0, 1, 2 or 3. Two runs of one device each make up to two requests with an amount of 1 or 2. A
coordinator reads the room left and admits a run with an estimate of 1 or 2 by `fits` of T8.1
against the room that it read last, at most twice a run, and only a run with no hold that is
`held` or `sent`. A run sends a request only with a grant, which exists only with the ruling
`hosted`. The gateway takes the send, calls `reserve`, then `dispatch` or `void`, then `settle`,
or the stream ends with no usage and the hold stays `sent` until the ledger expires it. The
ledger answers by `Step`. The environment changes the cap at most twice, raises `spent` by 1 for
a second device at most twice and only while the room left is at least 1, loses an answer, and
stops a run once. After a lost answer the run sends the request once more with the same id.

The eleven invariants read the variable `saw`, which the model writes at the step where the
thing happens, and the variable `last`, the record of the last step when it was a reserve that
went through or an admission. Two things read `saw` beside the invariants. The coordinator's
guard on a run's holds finds that run's holds through the ids that its sends carried
(`OpenHolds`). The step that drops the record of an ended request finds the record there
(`Ended`, `Forget`). No move of a run reads `saw`.

| invariant | its sentence | what it reads |
|---|---|---|
| `S1` | With the ruling `none` no hold ever leaves `new`. | `saw.under`: the rulings in force when the ledger took a hold out of `new`. |
| `S2` | A hold is `sent` only after it was `held`. | `saw.path`: the states that the ledger gave each hold, in order. |
| `S3` | A request is dispatched at most once, whatever the number of its sends. | `saw.calls`: the calls that the model host got for each request. |
| `S4` | After every `Reserve` that goes through, `spent + held` is at or under the cap in force at that step. | `last`: the account's sum after the reserve and the cap that the ledger used. |
| `S5` | A request has at most one hold. | `saw.ids`, the ids that the sends of a request carried on the wire, and `saw.path`. |
| `S6` | At every admission the estimates of the admitted and running tasks sum to at most the room that the coordinator read. | `last`: the estimate plus `saw.live` of the other runs, against `saw.room`, the room that the ledger told the coordinator. A run is live from its admission to its end, and while a send of it is out. |
| `S7` | A run has at most one hold that is `held` or `sent`. | `saw.ids` and `saw.path`. |
| `S8` | After a run has received the refusal `over_cap` it sends nothing until it is admitted again. | `saw.trail`: an `over_cap` answer that reached the run, then a send on the wire with no admission between. |
| `S9` | A second send of a request carries the amount of its first. | `saw.amounts`: the amounts that the sends of a request carried on the wire. |
| `S10` | Every request that reaches the host was sent by a run that was admitted at the send. | `saw.calls`, and `saw.unadmitted`: a send that left while `saw.live` of its run was 0. |
| `S11` | A request is sent at most twice. | `saw.sends`: the count of the sends of a request on the wire. |

`ControlRuling.cfg` sets `Fault = "grant_without_ruling"`: a request may be granted under the
ruling `none`, and TLC must report `S1`. `ControlResend.cfg` sets `Fault = "resend_new_id"`: a
second send takes a new id, and TLC must report `S5`.

`TwoLosses.cfg` is the system with two lost answers at a time, so that a request can lose the
answer of its second send too. As the file stands, with `Fault = "none"`, TLC finds no
violated invariant over 4,181,062 distinct states, to a depth of 23.

`Export.cfg` is the machine of one hold alone: one account whose cap (none, or 0 to 3) and
`active` do not change, whose `spent` and `held` start at 0 to 2, one hold that starts `new`,
and at each step one of the five moves with an argument of 0 to 3. TLC draws the argument for
`dispatch`, `void` and `expire` too and the machine does not read it, so each move is as likely
as another. Nothing else moves. TLC prints one line a drawn trace, at its twelfth move, and
one line a directed trace when it sets up the first states.

## What three parts of the system stand for

The admission. The coordinator admits against the room that it read. Another device can spend
between the read and the admission, and so can a run that was admitted before. `S6` therefore
bounds an admission by the room that was read and not by the room in force. The admission is a
courtesy to the person, so that a task does not start and then stop; it is no guard of the cap.
What holds the cap is the ledger alone, `S4`. A request's amount is not bound by its task's
estimate: the estimate is what the admission reads, the amount is the ledger's upper bound for
one request, and the admission lowers the chance that a task is refused in its middle and
promises nothing.

The guard on a run's holds. The coordinator of T8.2 cannot read a hold. The device knows a
hold's life: a run that ends with a request that got no answer `ok` and no refusal that stores
nothing reports the time until which a hold of that request may be open, its last send plus the
hold's life and the allowance of T11.4 on its own clock, and the coordinator admits the task
again only after that time. The coordinator drops the task's estimate from its list when the
run ends, as the model does, and keeps the reported time alone: the room that it reads next
already holds the open hold in its `held`, so an estimate kept beside it would count that hold
twice. The model has no clock. Its guard, which admits a run again only when the ledger shows
no hold of it that is `held` or `sent`, stands for that wait. A coordinator that admits again
on the run's own report that its request has ended is the wrong design `readmit_on_report`,
which `S7` reports.

The second send. It cannot meet the first at the gateway. A device that has no answer waits the
hold's life and an allowance before its second send (T11.4), and the gateway cuts its upstream
call before the hold expires (B-10), so the gateway's work on the first send has ended when the
second is sent. In the model the second send follows a lost answer, and an answer exists only
when that work has ended. The model has no invariant here: that the wait is longer than the cut
is a premise, the first item of "What the model does not show". Behind it stands the billing
run's own property that a hold is dispatched at most once: (g) of its model, and its test of two
dispatches at once.

## How the model keeps the search small

Four choices change which states TLC visits. The first three change no behaviour that an
invariant can see. The fourth is a bound.

- When a request has ended for the run and its hold is neither `held` nor `sent`, the next step
  drops the ledger's record of that hold and what `saw` holds of that request (`Forget`). No
  move can reach that hold again. The invariants were checked in every state before it. For a
  run that has made its last request and is off, the same step sets its count of admissions to
  the bound and clears its wait for an admission and `saw.trail` of it: such a run can neither
  be admitted nor send, so nothing reads the three again.
- When the ledger expires a hold while the gateway still works on it, the gateway's work ends
  in the same step: its next call would be refused, and an expiry leaves the room as it is.
- The environment's change of the cap is part of the next step that reads the cap: a send, a
  read of the room, or a spend of the second device. No other step reads the cap.
- `Hurry = TRUE`, the bound of `Spend.cfg`: after a lost answer of a request that has a hold,
  the second send, the ledger's answer to it (`repeat`, then `used` or `state` for the gateway's
  next call) and the hand back follow with no other step between. Those steps change nothing at
  the ledger. What `Hurry` leaves out is every state in which another run, the coordinator, the
  environment or the ledger, by an expiry of that hold, moves between them.

The run with every interleaving covers what `Hurry` leaves out. It is a run by hand: a copy of
`Spend.cfg` with `Hurry = FALSE`, every invariant of `Spend.cfg`, four workers. Whoever changes
the model, and whoever reads the change, runs it again and writes its count, its time and its
date here.

| date | model files of | invariants | distinct states | depth | violated | wall | 1-minute load |
|---|---|---|---|---|---|---|---|
| 2026-11-11 | `4668e9f` | ten | 2,737,006 | 23 | none | 154 s, four workers | 83 |
| 2026-11-11 | `8da2571` | eleven | 3,197,441 | 24 | none | 625 s, four workers | 687 |

One reading widens the system: the model lets the environment lose an answer of any request,
one at a time, where T7.6 says "loses an answer".

## Each invariant can fail

`Fault` names one guard that is off. With all eleven invariants listed, TLC reports the one of
the row first, in the configuration of the row with `Fault` set.

| `Fault` | the guard that is off | reported | configuration |
|---|---|---|---|
| `grant_without_ruling` | a grant needs the ruling `hosted` | `S1` | `Spend.cfg` |
| `reserve_to_sent` | the ledger's `reserve` leaves the hold `held` | `S2` | `Spend.cfg` |
| `dispatch_twice` | the ledger refuses the dispatch of a `sent` hold | `S3` | `Spend.cfg` |
| `reserve_over_cap` | the ledger's `reserve` tests the cap | `S4` | `Spend.cfg` |
| `resend_new_id` | a second send carries the id of the first | `S5` | `Spend.cfg` |
| `admit_alone` | an admission counts the estimates of the running tasks | `S6` | `Spend.cfg` |
| `send_in_flight` | a run sends only when no request of it is in flight | `S7` | `Spend.cfg` |
| `send_after_over_cap` | a run that got `over_cap` waits for an admission | `S8` | `Spend.cfg` |
| `resend_other_amount` | a second send carries the amount of the first | `S9` | `Spend.cfg` |
| `send_when_off` | a run sends only while it is admitted | `S10` | `Spend.cfg` |
| `third_send_same_id` | a run sends a request at most twice | `S11` | `TwoLosses.cfg` |

A third send needs the answer of the second send to be lost too, which `Spend.cfg` does not
allow: with `Fault = "third_send_same_id"` in `Spend.cfg` TLC reports nothing.

The other values of `Fault` are wrong designs of the device's side that the invariants were
tried against. Each is reported in the configuration of its row with `Fault` set, by the
invariant of its row. `expired_given_back` needs every interleaving: under `Hurry` the record of
an expired hold is dropped before the coordinator can move, and TLC reports nothing. The others
are reported with `Hurry = FALSE` too, by the same invariant.

| `Fault` | what the design does | reported | configuration |
|---|---|---|---|
| `grant_kept` | a grant is kept after the ruling was taken back | `S1` | `Spend.cfg` |
| `dispatch_on_repeat` | the gateway calls the host on a repeated reserve | `S3` | `Spend.cfg` |
| `list_until_first_send` | the coordinator's list drops a task at its first send | `S6` | `Spend.cfg` |
| `next_request_after_loss` | after a lost answer the run goes on to its next request | `S7` | `Spend.cfg` |
| `resend_after_over_cap` | the refused request is sent again at once | `S8` | `Spend.cfg` |
| `stop_drops_flight` | a stop drops the request in flight | `S6` | `Spend.cfg` |
| `expired_given_back` | the coordinator adds the amount of an expired hold back to the room | `S6` | `Spend.cfg` with `Hurry = FALSE` |
| `readmit_on_report` | the coordinator admits again on the run's own report | `S7` | `Spend.cfg` |
| `next_after_fail` | the run goes on after an answer that is neither ok nor `over_cap` | `S7` | `Spend.cfg` |
| `self_admission` | after `over_cap` the run resumes with no admission | `S8` | `Spend.cfg` |
| `room_without_held` | the coordinator computes the room without `held` | `S6` | `Spend.cfg` |
| `shared_id` | two runs mint the same id | `S7` | `Spend.cfg` |
| `third_send_new_id` | a third send takes a new id | `S11`, and `S5` when it is listed alone | `TwoLosses.cfg` |

## The commands

Run TLC on a copy of the files in a directory from `mktemp -d`, so that this directory gains no
file. `-Djava.io.tmpdir=.` gives each run its own place for TLC's standard modules: two TLC
runs that start at once on one machine otherwise read each other's half-written copies.

```sh
java -Xmx2g -XX:+UseParallelGC -Djava.io.tmpdir=. -cp tla2tools.jar tlc2.TLC \
    -workers 4 -metadir meta -config Spend.cfg Spend.tla
```

TLC ends with the status 0 and the line `Model checking completed. No error has been found.`
The line `<n> states generated, <n> distinct states found, 0 states left on queue.` gives the
count: 1,701,266 distinct states, to a depth of 23. With `MaxStops = 0` in a copy of
`Spend.cfg` the count is 460,824. A control ends with the status 12
and the line `Error: Invariant S1 is violated.` or `Error: Invariant S5 is violated.`

To replay a traces file, write it as one constant beside `Hold.tla`, `Replay.tla` and
`Replay.cfg`, and run TLC once:

```sh
{
    printf -- '---- MODULE Committed ----\nEXTENDS Hold\nTraces == <<\n'
    sed -e 1d -e 's/\[/<</g' -e 's/\]/>>/g' -e 's/{/[/g' -e 's/}/]/g' \
        -e 's/"\([a-z]*\)":/\1 |-> /g' -e 's/null/Null/g' -e '$!s/$/,/' traces.jsonl
    printf '>>\n====\n'
} >Committed.tla
java -Xmx2g -XX:+UseParallelGC -Djava.io.tmpdir=. -cp tla2tools.jar tlc2.TLC \
    -workers 1 -metadir meta -config Replay.cfg Replay.tla
```

TLC prints one line in double quotes, `"replay count=206 accepted=206 pairs=30 causes=8"`, and
ends with the status 0. `accepted` counts the traces in which `Step` gives every move the stated
answer, state, `spent`, `held` and `cost`. A move word outside the list of T7.2 ends the run with the
status 10 and no such line, because TLC cannot evaluate `Step`. An answer word or a state word
outside the lists does not end the run: the trace is not accepted, so `accepted` is under
`count`, an unknown answer counts as one more cause, and TLC ends with the status 0.

To check a property of `Reach.tla`, name it in a copy of `Spend.cfg` in place of the eleven
invariants, as `INVARIANTS` or, for a name that ends a `[][...]_vars` formula, as `PROPERTIES`,
and run TLC with `Reach.tla` as the module. TLC must report it as violated; the trace is the
proof that the system reaches the thing.

## What stands as it is

- `S5` speaks of a request's holds. Two runs that send under one id are outside it, because an
  id is minted from 16 random bytes (T7.3). The wrong design `shared_id` is reported by `S7`.
- `S8` speaks of `over_cap`. T11.5 names six other answers after which nothing is sent again.
  Of the six, `Step` and the traces hold `no_cap` and `not_active` alone; `pending`, `closing`,
  `no_such_account` and `credential_mismatch` are in no model and in no trace. The branch's
  sender owes a test for each of the six.
- The cover of an export depends on the seed. Of the seeds 1 to 8, two give every pair and
  every cause with 206 traces, 3 and 8, and `export.sh` refuses the others with `cover`.
- These readings of what T7.6 leaves open:
  - In the system a settle costs from 1 up to the hold's amount. The traces go below and above.
  - The first state takes any cap of 0 to 3.
  - When a stream ends with no usage, the gateway answers the device at once and the hold
    stays `sent` until the ledger expires it.
  - The coordinator admits by `fits` and not by `admit` of T8.1, so under the ruling `none` a
    run is admitted and the grant is the one guard.
  - After `over_cap` and a new admission a run goes on with its next request under a new id.
  - After an answer that is neither ok nor `over_cap` the task is handed back, and it may be
    admitted once more.
  - A stop with no request in flight takes effect at once. A run with a stop asked and a lost
    answer may send again or give up.
  - `export.sh` takes a seed of 1 to 999,999,999 and a count of 200 to 99,999. It refuses a TLC
    run that fails as `java`, a README with no one pin line as `hash`, and a `mktemp -d` that
    fails as `fetch`. It calls `mktemp`, `curl`, `shasum` or `sha256sum`, `java`, `sh`, `awk`
    and `sed`, and no command that links or removes a file: it makes `--out` with the shell's
    no-clobber and writes it last, and where that last write fails, the part that was written
    stays and the script says so and exits 1.

## What the model does not show

- That the device's wait before a second send is longer than the gateway's cut of the first.
  The model has no time, so this is a premise of the model and no invariant of it. The wait is
  the hold's life of T11.4 and the branch's allowance, counted from the device's last send:
  `hold_life_ms`, 120,000 ms in both rows of `docs/r6/gateway/tier-limits.tsv`, provisional.
  The cut is `HOLD_LIFE_MS` less `DISPATCH_MARGIN_MS` after the gateway's reserve call (B-10):
  3,000 less 400, 2,600 ms, in `wrangler.jsonc` and `wrangler.test.jsonc` of the billing run's
  repository. Its keeper is a test of the unit that builds the device's sender, the branch's,
  which fails when the wait is not longer than the cut.
- The ledger's inside. `S2`, `S3` and `S4` hold because `Step` is written so; the billing run's
  model proves them for the real ledger.
- Time. The model has no clock: the wait before a second send and the life of a hold are steps
  that may come in any order, but for the two places above where a guard stands for a wait.
- The four answers `pending`, `closing`, `no_such_account` and `credential_mismatch`, and what
  a device does after any of the six answers of T11.5 after which nothing is sent again.
- An account that is not active or has no cap, in the system. `Export.cfg` and the traces
  cover both.
- In `Step`: a result that does not fit a `u64`, and a move that would lower the account's
  `held` below 0. The rule for `held` is written in `Step`; no state of either configuration
  reaches it.
- Progress. The model has no property that a task is admitted or ends: a coordinator that keeps
  an ended task in its list admits less and breaks no invariant here.
- More than two runs, more than two requests a task, amounts above 2, a cap above 3, and more
  than one stop.

## The check 55-r6-formal.sh

`scripts/r6/ci.d/55-r6-formal.sh` runs in CI's macos job, and by hand with
`sh scripts/ci/run-ci-d.sh macos 55-r6-formal`. It checks `Spend.cfg`, replays `traces.jsonl`
through `Replay.tla`, replays traces of its own as a control of the replay, and runs the two
controls. `--self-test` runs the two controls, the replay control against two weakened copies
of `Replay.tla`, and a failing fixture for each of its guards.

The script holds these beside the model, and a change of any of them changes the script in the
same pull request:

- The configuration of `Spend.cfg` and of the two controls: the constants `Runs = {r1, r2}`,
  `MaxRequests = 2`, `MaxAdmissions = 2`, `MaxCapChanges = 2`, `MaxOtherSpends = 2`,
  `MaxLostAnswers = 1`, `MaxStops = 1`, `Hurry = TRUE`, `TraceMoves = 12` and `Null = Null`, and the
  lines `SYMMETRY RunSymmetry`, `INIT Init`, `NEXT Next` and `CHECK_DEADLOCK FALSE`. `Spend.cfg`
  sets `Fault = "none"` and lists S1 to S11. `ControlRuling.cfg` sets
  `Fault = "grant_without_ruling"` and lists S1 alone. `ControlResend.cfg` sets
  `Fault = "resend_new_id"` and lists S5 alone.
- `Replay.cfg`: the two lines `CONSTANTS` and `Null = Null`.
- The jar: the address, the size and the SHA-256 of the line `tlc url=...` at the top of this file.
- The form of a trace: the line forms above, the ranges of `Export.cfg`, five moves for each of
  the six directed traces and twelve for each drawn one, at least 206 traces, and a header whose
  count equals the number of traces.
- The banner of TLC 2.19, which every TLC run must print once.
- The replay control: two traces that the machine gives and 26 copies of them that it does not.

The check reads the committed files, as blobs of `HEAD`, and not the work tree. Commit a change
of the model before you run the check on it.

What the check does not see:

- An invariant weakened in `Spend.tla`. Run the rows of "Each invariant can fail" by hand.
- `Hold.tla`'s `Step` changed with the traces exported again. The test `formal_traces` of the
  routing run's U2.2 holds the traces against `spend::step` once it lands; until then nothing
  holds them.
- A trace replaced by another one that the machine gives, a directed one among them. The six
  directed traces are not compared with `Directed`.
- The seed of the header line.
- A change of `Replay.tla` that the replay control does not catch. The control catches a replay
  that skips a position of a trace or a field of a move, not every way to weaken `Walk`.
- The `java` and the `curl` that the job's environment names. The check trusts both.
