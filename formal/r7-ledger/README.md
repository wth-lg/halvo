# The model of the cap ledger

A person sets a spend cap for the hosted models that their agents may use. The account object's
ledger must hold it: every admission counts the holds that are still open, and every hold ends
once, by settlement, void or expiry. The stress check (check 50) reaches the interleavings that its
timing happens to make. This directory reaches all of them for a small configuration: a TLA+ model
of the ledger and the gateway, eight properties that TLC checks on every state that it keeps and
on every step of the model, and traces of the same model that a replay runs against the real
ledger.

The model is checked, not the code. The replay ties the two as far as it compares: it fails, and
names the action, where a step that a trace reaches answers or stores something other than the
model's value in a column that the replay compares. What it does not compare is in the list of
"What the model does not show".

## Files

| File | What it holds |
|---|---|
| `CapLedger.tla` | the model, the eight properties, the view, and the reachability predicates |
| `CapLedger.cfg` | the first configuration: three request ids, calls from any sender |
| `CapLedgerGateway.cfg` | the second configuration: two request ids and one gateway that the model follows |
| `CapLedgerAll.cfg` | three request ids with every feature at once, for one search outside the gate |
| `CapLedgerTrace.tla`, `CapLedgerTrace.cfg` | a seeded walk through the same model that prints each action and the state after it as one JSON line |
| `check.mjs`, `tlc.mjs`, `LoopbackTlc.java` | check 95: the checker on a copy of the spec, the walk, then the replay |
| `counts.json` | the distinct-state count of each configuration, to which check 95 holds each search |
| `controls.mjs` | 17 reachability controls, the breaks, at least one for each property, and the control of the view |
| `SKILL.md` | for the next change: what is covered, how to run, how to read a failure, the sync commit |
| `../test/ledger/model-replay.mjs` | the replay |

## Run

```sh
sh scripts/prerequisites.sh    # once: tla2tools.jar 1.7.4 into .tools/, against scripts/tools.sha256
sh gate/checks/95-model.sh     # one line: PASS model: ... or FAIL model: ...; first failure: ...
node formal/controls.mjs       # the controls; node formal/controls.mjs reach, break PropA, or view
```

TLC needs a JDK of version 17 or later on the path. It runs with 12 worker threads and a Java
heap of at most 4 GB; `MODEL_WORKERS=6 sh gate/checks/95-model.sh` gives it six threads, for a
machine that other work shares, and the counts on the check's line stay the same. Every run
copies the spec into a new
directory under `.tmp/` (the gate's own directory when the gate runs the check) and gives TLC
that directory for its state files, so the tracked tree gains no file. Nothing is removed: the
directories keep TLC's output, with the trace of a violation, and the generated traces.

TLC is started so that the outbound check (check 90) counts nothing and no port is open to
another machine:

- `LoopbackTlc.java` is TLC's main class behind a socket factory for loopback. TLC's set of seen
  states is a remote object of Java's RMI, and making it opens a listening socket. Without the
  factory that socket listens on every interface; with it, on 127.0.0.1. Java runs the launcher
  from its source, so no class file is written.
- `-Djava.rmi.server.hostname=127.0.0.1` and `-Djdk.net.hosts.file=/dev/null` keep the JVM from
  asking the system for the address of this machine's name. The outbound profile ends a process
  at any name lookup, and TLC without the two switches ends there.
- `-Dutil.ExecutionStatisticsCollector.id=NO_STATISTICS` turns off TLC's optional report of run
  statistics, whatever a file in the home directory says.

## What the check prints

```
PASS model: states=N gateway-states=N properties=8 violated=none seed=N traces=N actions=N differences=0 elements=26 of 26
```

`states` and `gateway-states` are the distinct states that TLC found for the two configurations,
counted by the view (below). `properties` is the number of the plan's properties that both
configurations name. `seed`, `traces` and `actions` are those of the walk that was replayed,
`differences` is the number of traces in which the object left the model, and `elements` the
kinds of action that the traces held. A FAIL line carries the same tokens with what was
measured, `unmeasured` for the rest, and the first failure: the property that TLC reports
(`violated=StepA` is (a)), the first action at which the object differs, or the element that the
traces never reach.

The check fails when a search does not end as TLC ends a complete one: with status 0, with the
count line that leaves no state on the queue, and with the sentence that no error was found. It
also fails when a count is not the one that `counts.json` records for that configuration, so a
search that was cut, to one request id for example, cannot pass. A change of the model that
changes a count changes `counts.json` in the same commit.

`sh gate/checks/95-model.sh --self-test` runs the check on seven bad fixtures of
`gate/fixtures/model/`, each of which must make it fail in its own words, then on the
checkout as it is, which must pass, and last it runs the control of the view. The fixtures are a spec whose admission leaves out the open
holds, a ledger with the same change, a ledger whose interval totals sum amounts, a trace that
the ledger does not follow, a launcher that makes TLC end with status 75 after a complete
search, a launcher that takes the count line out of TLC's output, and a first configuration of
one request id. The three fixtures of the replay run `node formal/check.mjs --replay-only`,
which leaves the two searches out; such a run can fail and never passes. The self-test stops at
the first fixture that does not end as expected.

## What the model describes

The object is one record `o`: the stored time, the cap row, one row for each hold, one row of
totals for each cap period that admitted a hold, and the platform alarm. One model step is one
step of the object: `Step` in the spec is `within` of `src/ledger/ledger.ts` followed by the one
ledger function, and the skeleton's alarm after it.

| In the model | In the code |
|---|---|
| `Now(o, given)` | `begin` of `src/do/entries.ts`: the later of the time that the skeleton gives and `last_now` |
| `SwitchPeriodType`, `ExpireDue`, together `Pre` | `switchPeriodType` and `expireDue` in `within` |
| `OwnReserve`, `OwnDispatch`, `OwnSettle`, `OwnVoid`, `OwnSetCap`, `CapAnswer` | `reserve`, `dispatch`, `settle`, `void`, `setCap`, `cap` of `makeLedger` |
| the move `expire` | the entry `alarm`: `ledger.expire` |
| the move `setcap`, the move `capread` | the entries `capSet` (`setCap`, then `cap`) and `capGet` |
| `Rearm`, the move `restart` | the entry `rearm` after a new `makeEntries` on the same storage |
| the move `back` | a restart on a host whose clock is one tick behind the stored time |
| `o.alarm` after every step | `host.setAlarm(due)`; the snapshot's `due` |
| `gw`, `GatewayAfter` | `serve` of `src/gateway/chat.ts`: reserve; on an admitted hold, or a repeat that is open and not marked, void or dispatch; the upstream call only on `go`; settle with the final usage |
| the move `stop` | a gateway that ends with no further call: a crash, or a stream with no final usage |
| the move `tick` | the runtime clock moves on by one tick |

Delivery. Any call may reach the object at any time, for any request id, any number of times:
the calls of `CallsOfAnySender`. This holds a duplicate of every call, every order of arrival, a
call for an id that has no hold yet, and the calls of every gateway that the model does not
follow. In the second configuration the model also follows one gateway invocation at a time
(`gw`), whose next call depends on the answer to its last one. Its calls are a subset of the
calls of any sender, so the followed gateway adds no state of the ledger; it adds the rule that
the upstream call is made only on `go`, and it lets the walk print traces that look like requests.

Money is in units of one minor unit of the cap. Time is in ticks.

## The eight properties

Each property has a condition on every step, `Step_`, and four of them also a condition on every
state, `State_`. A state condition reads the object alone. A step condition compares the object
before the step with the object after it and reads the call and its answer from `last`. TLC
checks a state condition on every state that it keeps and a step condition on every step that it
generates, also on a step into a state that it has seen before. The configurations name each
part, and TLC reports a violation under that name.

| The sentence of the plan's unit 4.4, as ruled for (a), (b), (d), (e) and (f) | On every state | On every step |
|---|---|---|
| (a) for each cap period, counted spend rises only at an admission within the cap in force, which is the amount of the last setCap, or by an overrun recorded at the settle of an open hold; a cost reported after the expiry is stored on the hold and enters no total (contract 3.2); and overruns stay 0 while every reported cost is within its hold | `StateA`: `OverrunsStayZeroWhileEveryReportedCostIsWithinItsHold` | `StepA`: `CountedSpendRisesOnlyAtAnAdmissionOrByARecordedOverrun` |
| (b) after a step that calls the ledger at time t no open hold has an expiry at or before t, no hold ends by expiry before its expiry, and after a restart, as after every step, the due time of contract 3.1 is at or before every open expiry | `StateB`: `TheDueTimeIsAtOrBeforeTheEarliestExpiry` | `StepB`: `NoOpenHoldIsDueAfterAStepOfTheLedger`, `NoHoldEndsByExpiryBeforeItsExpiry` |
| (c) a hold ends once | | `StepC`: `AnEndedHoldKeepsItsEnd`, `AHoldIsNeverRemovedOrMadeAgain` |
| (d) a repeat is a call for an id that has a hold; it changes no total and no hold state beyond the expiry of due holds with which every step begins; a reserve that was refused stores nothing, so the same request sent again is a new reserve; a settle, void or expire that arrives before its reserve is refused, and one that arrives after the hold has ended changes nothing but the overrun that Settle records | | `StepD`: `ARepeatChangesNoTotalAndNoHoldState`, `TheSameCallAgainAtOnceChangesNothing`, `ACallBeforeItsReserveIsRefused`, `ACallAfterTheEndChangesNothingButTheOverrunThatSettleRecords` |
| (e) the ledger refuses a reserve when the subscription is not active, no cap is set or counted spend plus the hold would pass the cap, and for no other reason than these and the Request id rule; a call that throws rolls back and is no step of this property | | `StepE`: `TheRequestIdRule`, `ARefusalForOneOfTheThreeReasonsAndNoOther` |
| (f) a hold adds exactly the cost of its first settle when settled, its full amount when expired and nothing when voided, and no total changes otherwise; a call that throws rolls back and is no step of this property | `StateF`: `EveryTotalIsWhatItsHoldsAdd` | `StepF`: `ATotalChangesOnlyByWhatAHoldAdds`, `ASettledHoldCarriesTheCostOfItsFirstSettle` |
| (g) a dispatch happens only under an open hold, at most once per hold, and a dispatched hold never ends as void | `StateG`: `AtMostOncePerHold`, `ADispatchedHoldNeverEndsAsVoid`, `TheGatewayIsUpstreamOnlyUnderAMarkedHold` | `StepG`: `ADispatchHappensOnlyUnderAnOpenHold`, `TheGatewayCallsUpstreamOnlyOnGo` |
| (h) no total of a sealed interval changes | | `StepH`: `NoTotalOfASealedIntervalChanges`, `AHoldJoinsOnlyTheIntervalOfItsStep`, `AnEndedHoldAddsTheSameToItsIntervalForGood` |

`PropA` to `PropH` in the spec join the two parts of each property.

The steps of (b). `StateB`, the due time at or before the earliest expiry, holds after every step
of the object, a restart among them. `StepB`, no open hold at or past its expiry, is stated for
the seven steps that call the ledger with a time: reserve, dispatch, settle, void, expire (the
alarm), setcap and capread, a refused one too. It is not stated for `restart` and `back`: `rearm`
writes the stored time, sets the alarm and ends no hold, so a hold can stay open past its expiry
until the next of the seven steps, with the alarm set to that expiry.

`StepB` has a second half: no hold ends by expiry before its expiry. A step that turns an open
hold into an expired one runs at or after the expiry of that hold, so a ledger that ends a hold
one tick early is reported.

What (d) compares. Every one of the seven steps first ends the holds that are due (`Pre`), also
when its call is refused. (d) compares the object after the call with the object after `Pre`
alone, so a repeat that comes later can find its own hold expired by time, and that expiry is no
change that (d) counts.

Counted spend in (a) and (e) is read from the stored totals of the period. (f) says that a total
changes in a step by exactly what the holds that changed in that step add, so the stored totals
are the sums over the holds.

The cap in force in (a) and (e) is the amount of the last setCap. The model keeps that amount in
`hist.lastSetCap`, written from the call itself and apart from the ledger's cap row, and `StepA`
and `StepE` hold an admission to it: a reserve is admitted only within that amount, and it is
refused over the cap only when it would pass that amount. A setCap that leaves the old amount in
force is therefore reported, by (a) when a cut does not take and by (e) when a raise does not
take. Contract 3.2 gives the amount of a setCap force from its step on, also when its period type
waits for the end of the period, so the history holds one amount.

`TypeOK` is also checked. It is no property of the plan: it says that every field of the model
holds a value of its kind.

## How the search ends

The model's clock has no upper bound, so the model has no last state. TLC compares two states by
`View` and explores one of each kind:

- whether the clock is behind the stored time (0 or 1 tick);
- the cap as the next step will see it: its amount, the period type, how far the time of the next
  step is into the cap period, and a change of type that waits, with the ticks until it applies;
- the totals row of that cap period;
- how many holds there are of each kind. The kind of a hold is its state, amount, mark, cost and
  overrun and whether a late cost is stored; for an open hold also the ticks until it is due (0
  when it is due), whether it belongs to that cap period and whether it belongs to the sync
  interval of the next step. The kind also says whether the
  followed gateway serves the hold, and at which step;
- the due time, counted from the time of the next step;
- how far the time of the next step is into its sync interval;
- the history: the amount of the last setCap and, in the reachability controls, the facts that
  they ask for.

The view counts holds and does not name them, because the request ids are interchangeable: two
states that differ only in which id has which hold are one.

What the view leaves out is what no later step reads: the absolute time, the period and the
interval of an ended hold, the totals rows of earlier periods, and `last`, the call that led to a
state. Two states that agree in the view take every call in the same way. The step conditions hold
for both or for neither, because they compare a state with its successor (a difference of totals,
a hold that changed) and read times only against the time of the step. One conjunct does read
what the view leaves out, `NoTotalOfASealedIntervalChanges`, which names intervals; the two
conjuncts beside it say the same without them. `StateF` and `TypeOK` also read it; they are
checked on the one state of each kind that TLC keeps, and `StepF` carries (f) on every step.

The sync interval of an open hold and the place of the clock within a sync interval feed no step
of the model, only (h). The view holds both, so that TLC expands every place of the clock within
an interval and (h) is checked for each. This doubles the states of each configuration, from
93,497 to 186,993 and from 396,278 to 792,554.

`node formal/controls.mjs view` holds the view to its claim. It searches a copy of the spec that
counts its steps, at the bounds of `CapLedgerGateway.cfg` and up to 6 steps, three times:
without a view, with the view, and with a weaker view that leaves out when a hold is due. The
search with the view must keep exactly the views that the search without one reaches, 14,737
views in 276,941 states, and the weaker view must miss some, which shows that the comparison
can fail. 6 steps is the largest bound at which the three searches end in about a minute
on a quiet machine. The self-test of check 95 runs this control.

## Bounds

The check runs TLC on two configurations of the one spec.

| Bound | `CapLedger.cfg` | `CapLedgerGateway.cfg` | The unit line asks for at least |
|---|---|---|---|
| request ids, each for one hold | 3 | 2 | 3 |
| senders | any | any, and one gateway that the model follows | |
| cap periods | not bounded | not bounded | 2 |
| period types | week (3 ticks) | week and month (4 ticks), set and changed at any time | |
| cap values | 1 and 3 | 1 and 3 | 2 |
| hold amounts | 1 and 2 | 1 and 2 | values that allow an overrun |
| costs | 1 and 3: within both holds, above both | 0, 1 and 3 | values that allow an overrun |
| life of a hold | 2 ticks | 2 ticks | |
| sync interval | 2 ticks | 2 ticks | |
| deliveries of one call | any number | any number | one duplicate of each call |
| restarts | any number | any number | one |
| setCap | any number | any number | one |
| the clock | never behind the stored time | one tick behind after a restart, at any restart | |

`MaxTime` is 1,000,000 in both, far above any time that the search reaches. The reachability
controls and the breaks run at the bounds of `CapLedgerGateway.cfg`.

The two configurations meet the bounds together. `CapLedger.cfg` holds the three request ids;
`CapLedgerGateway.cfg` holds both period types, the followed gateway, a cost of 0 and a clock
behind; both hold restarts, setCap, duplicates and the overrun. Each feature of the model is in a
configuration with at least two request ids.

`CapLedgerAll.cfg` has three request ids with every feature at once. It is no part of check 95.
One search of it is made once, outside the gate, when the machine's 1-minute load is under 30,
with `MODEL_WORKERS=4 node formal/controls.mjs all`, and its count, TLC's time and its date
stand here as landing evidence:

- not made yet. On 2026-11-11 the machine's 1-minute load stayed between 60 and 430 from the
  hour at which the model was final, never under 30. In its first minute the search, with 4
  workers at a load of about 290, generated 2,948,506 states, found 189,764 distinct ones and had
  119,398 on its queue at depth 10.

`CapLedgerTrace.cfg`, the bounds of the walk, has three request ids, both period types, the
followed gateway, the three costs and the clock that falls behind. It differs in two more
values, because the replay runs on calendar time: a month is 12 ticks (28 days when a week is 3
ticks: February 2031), and the clock stops at tick 22.

## Controls

`node formal/controls.mjs` runs the three kinds at the bounds of `CapLedgerGateway.cfg`: the
reachability controls, the breaks, and the control of the view, which "How the search ends"
describes.

Reachability. Each element is a predicate `Reach...` of the spec. The control asks TLC to check
the invariant `No...`, its negation, with the history facts on (`History = TRUE`) and no view,
and TLC must report the invariant violated; the trace that it prints is one way to the element,
and with several workers not always the shortest.

| Element of the unit line | Predicate |
|---|---|
| a duplicate of each call | `ReachDuplicateReserve`, `ReachDuplicateDispatch`, `ReachDuplicateSettle`, `ReachDuplicateVoid`, `ReachDuplicateExpire` |
| a reordered delivery | `ReachSettleBeforeItsReserve`, `ReachSettleAfterExpiry` |
| an overrun | `ReachOverrun` |
| a change of the cap below counted spend with a hold open | `ReachCapBelowCountedSpendWithAHoldOpen` |
| a change of the period type | `ReachChangeOfPeriodType` |
| a period boundary with a hold open | `ReachPeriodBoundaryWithAHoldOpen` |
| a restart between a dispatch and its settle | `ReachRestartBetweenDispatchAndSettle` |
| the gateway's crash after dispatch | `ReachGatewayCrashAfterDispatch` |
| a void refused for a dispatched hold | `ReachVoidRefusedForADispatchedHold` |

Three more predicates show a reading or a limit that the sections below name:
`ReachOpenHoldPastItsExpiryAfterARestart`, `ReachRefusedReserveAdmittedWhenSentAgain` and
`ReachLateOverrunInNoTotal`.

`ReachDuplicateExpire` asks for an expire of an id whose earlier expire was answered `expired`;
the history keeps those ids.

Breaks. Each break is one change of a copy of the spec, one replaced text, after which TLC must
report the property that the break aims at and no other, checked alone. Every property has at
least one.

| Property | The change |
|---|---|
| `PropA` | the admission leaves out the open holds (the change of the gate line) |
| `PropB` | a hold whose expiry is exactly now stays open |
| `PropC` | a settle after expiry makes the hold settled |
| `PropD` | a repeated reserve renews the expiry of its hold |
| `PropE` | a reserve that would reach the cap exactly is refused |
| `PropF` | a void leaves the open total as it was |
| `PropG` | a void does not look at the dispatch mark |
| `PropH` | a step takes the time it is given, also when the stored time is later |
| `PropACut` | a cut of the cap by a setCap of the same period type does not take; (a) reports it |
| `PropERaise` | a raise of the cap by a setCap of the same period type does not take; (e) reports it |
| `PropBEarly` | a hold ends one tick before its expiry; (b) reports it |

## The replay

Check 95 runs TLC on `CapLedgerTrace.tla`, which walks through the model with a generator seeded
by `Seed` and prints `Count` traces of `Depth` actions each. The seed and the counts are in
`CapLedgerTrace.cfg`, in the first line of the traces and on the check's line. `model-replay.mjs`
then makes every action against the real object in Node, through `seededRig` of
`test/ledger/harness.ts`: the entries of `src/do/entries.ts` over the real ledger and a SQLite
database in memory.

After every action it compares:

- the answer of the call with the model's answer;
- every row of `ledger_cap`, `ledger_periods` and `ledger_holds` with the model's cap, totals and
  holds: amounts, the period, the expiry, the state, the dispatch mark, the cost, the overrun and
  whether a late cost is stored;
- the ledger's `intervalTotals` for the sync interval 0, which holds every hold of the replay's
  account, with the model's sums over all holds: the costs of the settled ones, the amounts of the
  expired ones and the number of the open ones;
- the due time of the snapshot (`testInspect`) with the model's alarm, and the platform alarm.

Time. One tick is a third of a week. Tick 0 is 2031-02-01 00:00 UTC, the life of a hold is two
ticks, one model unit of money is one minor unit, and a cost of n units is settled with 3,084 n
input tokens and 28 n output tokens, which cost 10,000 n micro-units at the low tier's price.

Two clock paths, by turns. In a trace with an odd number the replay sets the host's clock, as the
ledger's own tests do, so time passes with no step of the object and a due hold ends inside the
next call. In a trace with an even number it stores a test time first and moves it only through
`testSetClock`, the method behind `POST /test/clock`: that method runs the alarm when a hold is
due, so the walk gives such a move the model's tick and its expire step as one action. Every
seventh trace has a subscription that is not active.

Why Node and not the test entry over HTTP. One trace runs on one object, and the test entry has
no path for most actions of a trace:

| Action | What the test entry offers |
|---|---|
| reserve | only `POST /v1/chat`, which goes on to void or dispatch, the upstream call and settle in the same request |
| dispatch, settle, void | no path; `/v1/chat` makes them in its own order, so a duplicate, a settle before its reserve or a void of a dispatched hold cannot be sent |
| the alarm | only inside `POST /test/clock`, and only when a due time has come |
| restart | no path; it takes a stop of the server |
| setCap, the cap read | `PUT /v1/cap` and `GET /v1/cap`; the replay calls the same entries, `capSet` and `capGet` |
| the clock | `POST /test/clock`; the replay calls its method, `testSetClock` |

The elements. The replay counts 26 kinds of action in the traces that it ran (`ELEMENTS` in
`model-replay.mjs`), an overrun, a settle after expiry and a period type that switched among
them, and check 95 fails when one of them is missing, so a walk that no longer reaches a case
does not pass unseen.

`gate/fixtures/model/bad-trace-admitted-over-the-cap.jsonl` is a generated file that is tracked:
it is the bad fixture of the check's self-test, the start of one trace of the walk through a copy
of the spec whose admission leaves out the open holds. The ledger refuses the reserve that this
trace admits.

## Readings

Where the plan or the contract leaves a move open, the model takes the reading that admits more
behaviour. Readings 1 and 2 are ruled since: the sentences of (b) and (d) above now say them.

1. A step, in (b). The model reads it as a step that calls the ledger with a time (contract 3.4).
   The restart's `rearm` step calls no ledger function, so after it an open hold can be past its
   expiry until the alarm, which is then already due, runs
   (`ReachOpenHoldPastItsExpiryAfterARestart`). Read over every step of the object, (b) would not
   hold of the design.
2. A repeat, in (d). The model reads it from what is stored: a reserve whose id has a hold, a
   dispatch of a marked hold, a settle or a void or an expire of a hold that the same call
   ended. A call that was refused and is sent again is no repeat: a reserve that was refused over
   the cap is admitted when it comes again with the same arguments and the cap then has room
   (`ReachRefusedReserveAdmittedWhenSentAgain`). The model also checks that any call delivered a
   second time at once changes nothing.
3. The model's expire names one hold for its answer, and its step ends every hold that is due, as
   the ledger's expire does. An expire that ended only the named hold would leave a due hold open
   after a step, against (b).
4. Sealed, in (h). The model has no seal step. An interval counts as sealed from the moment at
   which the rule allows the seal: it has ended and none of its holds is open. The rule's order,
   oldest first, is left out, which makes the set larger. Intervals are slots of a fixed length
   from tick 0; a billing period, a cancel and an account delete, which cut an interval, are
   outside the model.
5. A settle before its reserve. The gateway as built waits for every answer, so it cannot send
   one. The model delivers it, because (d) names it and because `settle` carries no credential.
6. The gateway's test of the time left and its cut of the upstream call are not in the model. The
   model's gateway may void or dispatch whatever time is left, and a settle may arrive at any
   time, which holds every timing that the test allows and more.
7. The order of the three refusals. `StepE` asks that each stated reason is true. The order of
   contract 3.2 (not active, no cap, over the cap) is in the model's step, and the replay compares
   the reasons.
8. Time starts at the first setCap, at tick 0, which becomes the first anchor. Before it the
   ledger refuses every reserve and stores nothing.
9. A clock behind the stored time. The model lets it fall behind at a restart only, by one tick.
   A clock that goes back while it is still ahead of the stored time gives the object nothing to
   see: the next step takes the earlier time, as if the later tick had not come.

## What the model does not show

- A fourth request, and a request id that is used again after its hold has ended.
- Three requests together with a second period type, with the followed gateway, with a cost of 0
  or with a clock that is behind: the first configuration has the three requests, the second the
  rest. `CapLedgerAll.cfg` has all of it at once, for the one search outside the gate.
- The tier and the price in the request-id rule. The ledger compares the tier, the amount and the
  price of a reserve with those of the stored hold; the model has one tier and one price, so the
  model and the replay compare the amount only.
- A month is a fixed number of ticks. The calendar rule of contract 4 (the anchor's day n months
  later, the last day of a shorter month) is the ledger's own test's. In the replay every month
  period starts in February 2031, and a second month period is entered but never left.
- The anchor that a seed gives.
- The answer of the alarm's expire. The model's expire names one hold and answers for it; the
  ledger's expire takes no id and answers a count, which the replay does not compare. The expire
  clauses of (d) are checked in the model only.
- Two throws. A reserve with an amount or a price that is no whole number throws, and so does a
  settle that would take counted spend past the exact range of whole numbers. A call that throws
  rolls back and is no step of the model: its amounts are whole numbers, and its totals stay far
  below that range.
- Columns that the model does not hold. The replay does not compare `last_now`, the `created_`
  and `ended_` columns, `late_seq`, the tokens of a settle, or more of `late_at` than whether it
  is set.
- A late overrun. A settle after expiry stores its cost and overrun on the hold row and changes
  no total (contract 3.2), so counted spend holds the hold's amount and not the higher cost
  (`ReachLateOverrunInNoTotal`).
- The pause and the stop of the alarm after a caught error, and the refusals that the skeleton
  makes before it calls the ledger: the plan puts both outside the model.
- Atomic steps. A step of the model happens whole or not at all; that a cut transaction leaves no
  partial step is the platform's, and the ledger's rollback tests run it.
- A hold that lives longer than a cap period, a clock more than one tick behind, and two open
  holds whose expiries are more than one tick apart.
- The replay's account is not metered, so every hold stores the interval 0, and the replay
  compares the totals of that one interval. That no total of a sealed interval changes, (h), is
  checked in the model only. The replay's subscription state is fixed for a trace, because the
  accounts module at this commit is a stub that applies no event.
