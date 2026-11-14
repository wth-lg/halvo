# Fake meter

A stand-in for a provider's usage meter that keeps state, which stripe-mock does not
(researcher-1 rows 77 and 84 of run 20261109-halvo-r7-billing). `fake-meter.ts` has no I/O and
imports nothing: the caller passes the time into every call, so a test owns the clock.

| Rule | Source | In the fake |
|---|---|---|
| An identifier is unique inside a rolling window of at least 24 hours | r1#84 | `uniquenessWindowSeconds`, 24 hours or more, or `Infinity`. Inside the window a repeat adds nothing. After it the same identifier is recorded again. Identifiers are one namespace across accounts. |
| A timestamp is at most 35 days old and at most 5 minutes ahead | r1#83 | `send` refuses with `timestamp_too_old` or `timestamp_in_future`. |
| An invoice is finalized one grace period after its period ends: 1 hour by default, 72 hours at most | r1#85 | `invoicedTotal` counts only what arrived before that. Later usage is in `exactTotal` and `read` and on no invoice. |
| Events are processed asynchronously | r1#83 | `readLagSeconds`: `read` leaves out younger events. |

What the fake decides for itself, because no research row settles it:

- **The answer to a duplicate.** `duplicateAnswer` is `"error"` (status `duplicate`) or `"same-as-recorded"`. A caller must treat both as "the meter holds this identifier".
- **Amounts.** Whole minor units, 1 or more. Zero is refused, so a caller that sends empty intervals shows up.
- **The width of a read.** `read` widens its range to whole windows of `readGranularitySeconds` and returns the range it summed, so a caller must be ready for a total that holds more than it asked for. That the provider's summary call takes whole minutes is the builder's memory, not a research row; 1 turns the widening off. Check 55's model reads one width, `READ_WIDTH_SECONDS` of `rerun/world.ts`, and refuses a scenario whose meter states another, so that no setting can turn the minute-wide read off for itself.
- **The end of a subscription.** The last invoice period ends one second after the end was asked, so the second of a cancel belongs to it.
- **Invoice periods.** Fixed length from the subscription's start, not calendar months.
- **The invoice of a period that an end cut short.** It is finalized one grace period after that end, like any other. r1#85 speaks of the previous period, and no row covers a cancel. This is the lenient choice: the last interval is always sent after the cancel.
- **What the window and the lag count from.** Both run from the second the meter received the event, not from its timestamp, and a duplicate does not restart the window.

`read` is what a sync may call. `exactTotal`, `recordedEvents`, `periods`, `invoicedTotal` and `stats` read the record itself, for a test's own measurements.

`self-check.ts` exports `selfCheck()`, one entry per rule above. `sh spikes/s9/run.sh` prints its count first.

`port.ts` exports `meterPort({ meter, now, faults?, always?, cancel?, checkout?, session? })`, a `ProviderPort` whose `sendUsage` and `readUsageTotal` go to the fake at the second that `now()` gives, keyed by the account id. Each call takes the next fault of `faults`, or `always` when the list is empty: `request-lost` answers `failed` and the fake sees nothing, `response-lost` lets the fake take the call and answers `failed`, `refuse` answers that refusal without the fake, and `during` runs a function while the call is in flight and then takes its `after` fault. `calls` lists every send, read and cancel with its answer. The other methods answer `failed`, `rejected` or `nothing-to-cancel`, unless `checkout`, `session` or `cancel` give an answer. The tests of unit 5.1 under `test/metering/` drive the account object through it (`test/metering/world.ts`).

`rerun/` is check 55's model over the real account object. One choice of it is the model's own and no provider's rule: a stamp at or after the subscription's end belongs to the subscription's last billing period only when it lies before the contract's cut, both for the meter's events and for an amount that the account closed as not billable. The contract's cut is the earliest time (the stated end, else the provider time) of the cancel events that reached the account object before the seal of the interval that this time would end; a cancel event that arrives after that seal, or after the account's storage is gone, moves nothing. The model reads the seal from the stored intervals when each cancel event arrives, and counts the orderings in which an earlier time moved the cut back across an interval that is not sealed, ends after that time and has a hold, open or settled (`ordering.moved-the-cut-back-across-an-unsealed-interval-with-a-hold`). The count shows that the orderings reach the state the path needs; it does not prove that a wrong cut would fail there. A stamp at or after the cut belongs to no period, and the line fails. What the real provider does with a meter event stamped after the subscription's end is one of the questions for the owner's sandbox run (ruling P7-80, part 7), and the line of check 55 rests on this choice until then.

The model's reading of the stored state fails a cancelled account whose row keeps no cancel time (no stated end and no first cancel) and a cancelled, metered account with no cut; a cancelled row with a stated end and no first cancel is counted, because contract 3.2 asks for the stated end, or else the earliest provider time. The reading is of accounts that events made: the model makes every account active and cancels it by events. A cancelled row made by the test entry's seed has no marks by design, and the model never seeds one.
