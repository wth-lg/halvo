rule: MET (100000 generated orderings over 5 seeds, 54860 of which put usage at the provider, and 1138851 fault sequences passed every check, and all 5 controls missed the rule)

# S9: billing sync against a stateful fake meter

Measured 2026-11-10 (UTC) on Apple M3 Max, 16 cores, 64 GiB, macOS 27.2, Node v26.12.0, fast-check 4.10.2, at commit 13b7b40.

Reproduce: `sh spikes/s9/run.sh` after `sh scripts/prerequisites.sh`. It exits 0 for MET, 1 for MISSED and 2 when the result does not count. It starts no server, uses no network and writes no file, and two runs print the same text. `--show` adds the smallest failing ordering of each control as JSON.

## What this decides

D11 keeps its default, one send for each sealed interval. The scheme below met the rule at interval lengths of 1 hour, 24 hours and the whole billing period. The spike did not run the other option, one event for each request.

| Part | The scheme the rule was met with |
|---|---|
| Interval | A slot of fixed length inside a billing period, cut at the period's end and at cancel or account delete. The sync seals it when it has ended and none of its holds is open, oldest first. |
| Amount | Fixed at sealing as the interval's reported total plus the remainder carried in, in whole minor units. The sync stores the rest, under one minor unit, with the interval and carries it on. It acknowledges an amount of zero with no call. |
| Identifier | `<account id>:<interval start in Unix seconds>`, stored at sealing and the same for every attempt. The account is part of it because the provider keeps identifiers in one namespace. |
| Timestamp | The interval's last second, which lies inside the interval's own billing period. |
| Order | One unacknowledged interval for each account at a time, oldest first, with one call in flight. |
| Retry | The sync stores the time of the first attempt before the call. Until 24 hours minus a margin after it, a retry sends the same event with no read. From then on, every resend follows a read of the provider-side total for the event's second, and happens only when that total lacks the amount. The sync never counts the 24 hours again from a later attempt. |
| End | At cancel the sync records the remainder as written off when the provider has acknowledged the last interval. At account delete it drops the storage only then. |

The provider port's metering method is two calls, the shapes unit 2.2 waits for:

1. **Send** one amount for one account and interval: account, identifier, amount in whole minor units of 1 or more, timestamp. The answer is `recorded`, `duplicate`, or `refused` with a reason. `recorded` and `duplicate` both acknowledge. No answer means the provider may or may not hold the event.
2. **Read** the total for one account and time range. The answer is the total in minor units and the range that was summed, which may be wider than the range asked for.

The rule does not choose the interval length, because all three lengths met it. The one measured difference is under "Usage on an invoice", and it is a reason not to use the whole period.

## Rule

Copied from the plan (P0, unit S9), unchanged:

> It meets its rule when, for every account and billing period and all in micro-units, the provider-side total plus the remainder carried out of the period equals the ledger's reported total of the period plus the remainder carried into it (the carry of P4's units, as unit 5.1 states it), where the remainder carried into a period is the one carried out of the period before it, zero for an account's first period, and the one carried out of its last period is the remainder left at cancel or at account delete; every remainder is under one minor unit; that last remainder is sent nowhere, and the one left at cancel is recorded as written off; and no account's provider-side total passes its reported total.

## How each term is measured

Fixed before the first run.

- **Units.** One minor unit is 10,000 micro-units: a micro-unit is a millionth of the currency's major unit and a minor unit is its hundredth.
- **Billing period.** The fake meter's invoice period for the account. The first starts when the account subscribes, each has a fixed length, and the last ends when the fake ends the subscription at cancel or at account delete. Its anchor differs from the cap period's in every run.
- **Provider-side total of a period.** The sum of every event the fake accepted for the account with a timestamp inside the period, times 10,000. The test reads it from the fake's own record, not through the read that the sync uses, which can lag.
- **The ledger's reported total of a period.** The test driver's own sum, not a figure the ledger under test supplies: the cost of every settle the driver delivered to an open hold before the hold's expiry time, counted in the billing period that admitted the hold.
- **Remainders.** The remainder carried into an account's first period is 0. The remainder carried out of a period is the reported total plus the remainder carried in, minus the provider-side total: the one value for which the rule's equation holds. The test then checks that it is at least 0 and under 10,000, that it equals the remainder the ledger stored for the end of that period while the ledger's storage exists, and it carries that value into the next period.
- **The last remainder is sent nowhere.** The fake's total for the account over all time equals the sum of its periods' totals, and the fake receives no event for the account after the last interval is acknowledged.
- **Written off at cancel.** After a cancel, the ledger's storage holds the last remainder as written off. After an account delete, the ledger's storage is empty.
- **No provider-side total passes its reported total.** Checked after every step of every run, for every account: the fake's total over all time, times 10,000, is at or under the driver's reported total.
- **When.** The last check runs after every step. The others run at the end of each run, after a closing phase in which the provider answers every call and the clock moves forward by one billing period and four days with every alarm fired on time.

## What the test generates

Fixed before the first run.

- One or two accounts. Steps in any order: reserve; settle (also twice, after expiry and above the hold); clock moves from 0 seconds to days, which bring expiry and the rollover of the cap period and of the billing period; the sync's wake from its alarm (seal, send, retry); a request that is lost; a response that is lost; a request delivered late; a response delivered late; a crash before the provider call; a crash after it; a crash that keeps the account's object down for up to days; cancel; account delete.
- The fake's settings change from run to run: the uniqueness window (exactly 24 hours, longer, or never forgotten), its answer to a duplicate (an error, or the same answer as an accepted event), the grace period (1 hour or 72 hours), the lag of its read, and the width its read rounds a time range to. The sync's interval length changes too: 1 hour, 1 day, or the whole billing period.
- **Not generated in the rule run: an interval that stays unsent for 35 days.** The fake refuses a timestamp older than 35 days, so no sync can report an interval whose every attempt fails for that long. The rule run keeps the downtime of one run at or under 20 days, and any refusal by the fake fails the run. A run with a longer outage is measured apart and reported as a limit, outside the rule line.
- **Not generated in the rule run: a read that lags longer than the uniqueness window.** The rule run draws a lag of at most 23 hours. A run with a longer lag is measured apart and reported as a limit, outside the rule line.
- The seeds and the number of orderings are constants in `spikes/s9/run.mjs`, fixed in the commit that adds that file. A later change is listed in this file with its reason.

## What counts

The rule line reads MET only when every ordering of the rule run passes every check above and each control below fails at least one check. A control that does not fail means the result does not count, and the line says so.

Controls, each a variant of the sync that must miss the rule:

1. A retry draws a new identifier (the scratch change that P5's gate names).
2. A resend after the uniqueness window skips the read of the provider-side total.
3. The remainder is dropped at each interval instead of carried.
4. The last remainder is rounded up and sent at cancel or at account delete.
5. Intervals are cut at the cap period's ends instead of the billing period's.

## Changes after the rule commit

The rule, its terms and the five controls are as committed in 6141fbd. What changed since, with the reason:

1. **Before the first measured run.** The closing phase keeps the faults that are left in force while the clock passes the end of every open billing period. Then every call succeeds, each object that is down comes back up, and the sync gets two more days. The rule commit said the provider answers every call during the whole closing phase. A send at a period's end would then never meet a fault.
2. **Before the first measured run.** One more check, stricter than the rule. The fake must hold exactly one event for each acknowledged interval, with that interval's amount, and no identifier twice.
3. **After the first measured run** (898fb3c, MET on 100,000 generated orderings). A variant that counts the 24 hours again from each resend passed all 100,000, although it double counts in an ordering with three crashes on one interval. The command now also runs every sequence of up to 4 faults on one interval (fd8058e, 59cf8b2), and the rule line requires that search to pass too.
4. **After the first measured run.** The generator also draws reads one hour and one day wide (4bc2bc4). Seven further controls were added; they are reported and do not decide the rule line.
5. **After a review** by four read-only agents (f842350, 13b7b40). It confirmed 15 findings. None breaks the sync as designed inside the bounds this file states. Three are limits outside those bounds, and twelve are gaps in what the checks, the generator and the fake's self-check could see. Fixed: the checks hold their own 10,000 and no longer take it from the code under test; a new command moves the clock to the last seconds of a billing period; one ordering in four puts two accounts on one interval grid; a cancel or delete that follows an account delete reaches the object; the fake's self-check states 35 days itself and has five more entries. The three limits below are text in this file and not code.

The seeds and the count of generated orderings never changed. They are 5 seeds of 20,000. Every measured run is in the run's evidence directory with its command.

## Measured values

All of it comes from one run of the command; the full output is at the end of this file.

### The rule run

- **Generated orderings that passed every check: 100,000 of 100,000.** fast-check 4.10.2, seeds 1 to 5, 20,000 orderings each. An ordering is 1 to 80 commands on one or two accounts, with a fault for each of its first 30 provider calls at most. It is a sample, not a proof. 54,860 orderings put usage at the provider. 30,197 reported no usage at all, so their equation is all zeros. 48,296 made a provider call before the closing phase.
- **Fault sequences that passed every check: 1,138,851 of 1,138,851, over 21 settings.** Every sequence of up to 4 faults, out of 16 (none, lost request, lost response, late request, and a crash before or after the call with one of 6 downtimes), on the provider calls of one interval. 16 settings: a window of 24 hours or one that never forgets, a read lag of 0 or 23 hours, both answers to a duplicate, an account that stays or is deleted. 4 settings: two intervals inside one day-wide read. 1 setting: a hold open across a billing period's end, with a later interval cut by a cancel. Its limits are one account, at most three intervals, a depth of 4 faults and no late response.
- **How often each step the unit names was reached**, from counters in the driver, over the 100,000 generated orderings. A counter counts events, not orderings, and says nothing about which pairs of steps met.

| Step | Count |
|---|---|
| Settle counted in the reported total | 158,050 (26,590 above the hold, 8,518 after a cancel or delete, 2,444 after the hold's billing period had ended) |
| Expire | 294,207 holds ended unsettled; 218,672 settles arrived after the expiry |
| Seal and send | 82,730 first attempts; 100,194 events recorded by the fake |
| Retry inside the window | 63,299 retries; 17,905 duplicates answered by the fake |
| Retry after the window | 40,465 reads: 13,209 found the amount, 8,102 lacked it and were followed by a resend, the rest got no answer; 70 summed a range that held another interval |
| Crash | 22,938 before the call, 22,561 after it, 151,304 between wakes; 65,460 kept the object down for 24 hours or more |
| Lost and late calls | 22,826 requests lost, 22,569 responses lost, 15,215 requests late, 15,032 responses late; 12,078 steps ran while a call was in flight |
| Period rollover | 132,176 holds were the first of a new billing period and 165,494 of a new cap period; the cap period rolled inside a billing period 53,608 times and the billing period inside a cap period 20,290 times; 78,276 reserves fell in the last 900 s of a billing period and 11,389 in its last second; 58,541 periods after an account's first held usage |
| Cancel | 33,600 on an active account (12,072 with a hold open). A delete followed 6,880 of them, and the other 26,720 completed with a write-off recorded. 9,866 more came after a delete. |
| Account delete | 33,938 on an active account (11,944 with a hold open) and 6,880 after a cancel. All 40,818 completed with the storage gone. |

- **Remainders.** Every remainder was at least 0 and under 10,000 micro-units, in every billing period of every ordering. That is the check, and it never failed. 138,527 periods started with a remainder carried in. 14,280 cancels and 21,536 deletes left a remainder above zero, which was sent nowhere.

### Controls

All five required controls missed the rule, each within the first 16 orderings of seed 1 (the lines are in the output below). Control 1 is the scratch change that P5's gate names. All seven further controls missed too. Two of them missed only in the fault-sequence search and in none of the 100,000 generated orderings: the 24 hours counted again from each resend, and a later interval sealed before an older one. Generated orderings alone do not reach a defect that needs three or four failures in a row on one interval, or a hold open across a period's end next to an interval cut by a cancel.

### Limits, outside the rule line

1. **The read must not lag too far behind.** With a 24-hour window and a read that lags 30 hours, the sync as designed double counts (the command shows it). A reviewer measured the edge in a scratch copy, with requests that travel up to 90 s. A lag of 86,310 s passed and 86,311 s double counted. The sync is exact only while the read's lag plus the time a resend may travel stays under the uniqueness window. r1#83 says processing is asynchronous and gives no bound.
2. **A request must not travel longer than the margin.** The sync stops reusing the identifier without a read 300 s before the 24 hours end. A reviewer's scratch run passed with requests 299 and 300 s late and double counted at 301 s. The command generates 90 s.
3. **35 days.** An object that is down for 36 days after a billing period ends gets a refusal. Its period then reads 250,000 micro-units reported and 0 at the provider. The provider-side total is short, never over. A reviewer found a worse order past 35 days. The provider records a resend just before the 35-day line and its answer is lost, the next read lags, and the provider refuses the next resend for age. The sync then stores "refused" for an interval whose event the provider holds. The rule run keeps downtime at or under 20 days, so neither case is in it.

### Usage on an invoice

The rule compares totals, and the fake's total takes an event whenever it arrives. An invoice does not. Usage that arrives after the grace period is in the total and on no invoice (r1#85). The fake says for every event whether it arrived in time.

| Interval | Grace | Billing periods with usage | Minor units at the provider | Arrived after the invoice was finalized |
|---|---|---|---|---|
| 1 h | 1 h | 13,053 | 131,513 | 5,106 (3.9%) |
| 1 h | 72 h | 12,869 | 129,084 | 2,067 (1.6%) |
| 24 h | 1 h | 13,011 | 130,000 | 5,688 (4.4%) |
| 24 h | 72 h | 12,851 | 128,812 | 2,008 (1.6%) |
| whole period | 1 h | 12,915 | 130,310 | 15,431 (11.8%) |
| whole period | 72 h | 13,274 | 133,259 | 6,059 (4.5%) |

The table groups the same 100,000 orderings by the interval length and grace period each one drew. The shares follow the generator's faults, where a crash can keep an object down for days, so they are not a forecast. A cancel's last period is finalized one grace period after the cancel, which is the fake's own choice; a reviewer measured, on the generator as it was before the last change, that in the 1-hour and 24-hour rows about nine tenths of the late usage is in such last periods. One thing holds whatever the faults are. With one interval for each billing period, all of a period's usage waits for one send made after the period has ended, inside the grace period.

### Time

The command took 197.3 s and 196.6 s of wall time in its two final runs in the worktree. Other builders' runs loaded the machine at the same time (load averages between 10 and 122), so this is not a clean timing. About one third of it is the generated orderings. The rest is the search and the controls.

## What this does not show

- **The provider.** The fake is the team's reading of three documentation rows. Only a sandbox shows what the provider answers to a duplicate, how far its read lags, whether its summary call takes whole minutes, what it does with a value of zero, how long an identifier may be, and whether usage stamped in the second of a cancel is on the last invoice. These belong on D15's list.
- **Every ordering.** 100,000 samples and a search 4 faults deep on one account. The control that only the search caught shows that sampling alone is not enough here. A model check of the sync, as P4 plans for the ledger, would cover what a sample cannot.
- **The real code.** The subject is a 400-line object in plain Node with a store that is copied for each step. Durable Object transactions, alarms and storage are S1's and P4's to show.
- **Not modelled.** Calendar months (periods are 30 days), a clock that differs between ledger and provider, a cancel that reaches the ledger later than the provider, void, the cap itself, more than two accounts, and the provider's rate limits (r1#83). The design meets the rate limit by keeping one call in flight for each account.
- **The invoice.** The rule is met on the provider's total. Usage acknowledged after the grace period is in that total and on no invoice, and the rule has no term for it.

## What a later phase must know

- **P2, the port.** The two calls above. A `duplicate` answer must reach the sync as an acknowledgement, whatever the provider's status code for it. A refusal for an old timestamp is final; other refusals are retried. The read returns the range it summed.
- **P5, the sync.** Keep one unacknowledged interval for each account. Store the event and the time of the first attempt before the first call. Never count the 24 hours again from a later attempt. Subtract the acknowledged intervals inside the read's range before comparing. Before storing a refusal that follows an unanswered attempt, read once more (limit 3). Give the margin a number that covers the HTTP timeout.
- **P3 and P6, account delete.** The rule needs every remainder under one minor unit, so an account delete cannot drop the storage at once. It cuts the interval, waits for open holds, sends, and drops the storage with the last acknowledgement. A delete while the provider is unreachable stays pending.
- **Grace period.** A retry after the uniqueness window comes at least 24 hours after the period's end, which is past the default 1-hour grace period. With the longest grace period, 72 hours, it can still reach the invoice. An interval that is unacknowledged near the end of the grace period needs an alert.
- **Check 55-metering (unit 5.3).** `spikes/s9/harness.ts` drives any object with the methods of `AccountObject` in `sync.ts`: reserve, settle, end, alarmAt, wake, record, view, unexplainedReads, ended. The controls are variants of the subject; P5's scratch change is control 1. The checks hold their own factor of 10,000.

## The command's output

```
fake meter: 22 of 22 of its own checks hold

rule run: the sync as designed, 5 seeds of 20000 orderings
  seed 1: 20000 orderings, every check passed
  seed 2: 20000 orderings, every check passed
  seed 3: 20000 orderings, every check passed
  seed 4: 20000 orderings, every check passed
  seed 5: 20000 orderings, every check passed
  fault-sequence search: 1138851 sequences of up to 4 faults on one interval, over 21 settings, every check passed

what the 100000 generated orderings held (counts over all of them):
  call.before-the-closing-phase: 164487
  call.read: 40465
  cancel: 33600
  cancel.after-cancel: 6877
  cancel.after-delete: 9866
  cancel.completed: 26720
  cancel.left-a-remainder: 14280
  cancel.with-open-hold: 12072
  command.dropped-while-down: 293920
  crash.down-past-the-window: 65460
  crash.idle: 151304
  delete: 33938
  delete.after-cancel: 6880
  delete.after-delete: 9981
  delete.completed: 40818
  delete.left-a-remainder: 21536
  delete.with-open-hold: 11944
  fault.crash-after-call: 22561
  fault.crash-before-call: 22938
  fault.late-delivery: 15215
  fault.late-response: 15032
  fault.none: 73455
  fault.request-lost: 22826
  fault.response-lost: 22569
  hold.billing-period-rolled-inside-a-cap-period: 20290
  hold.cap-period-rolled-inside-a-billing-period: 53608
  hold.expired-unsettled: 294207
  hold.first-of-a-billing-period: 132176
  hold.first-of-a-cap-period: 165494
  meter.duplicate: 17905
  meter.recorded: 100194
  orderings: 100000
  orderings.with-a-provider-call-before-the-closing-phase: 48296
  orderings.with-no-reported-usage: 30197
  orderings.with-usage-at-the-provider: 54860
  period.later-with-usage: 58541
  period.starts-with-a-remainder: 138527
  read.holds-the-amount: 13209
  read.lacks-the-amount: 8102
  read.range-holds-another-interval: 70
  reserve: 670929
  reserve.in-the-last-900-s-of-a-billing-period: 78276
  reserve.in-the-last-second-of-a-billing-period: 11389
  send.first-attempt: 82730
  send.resend-after-the-window: 8102
  send.retry-inside-the-window: 63299
  settle.above-the-hold: 26590
  settle.after-cancel-or-delete: 8518
  settle.after-expiry: 218672
  settle.counted: 158050
  settle.counted-after-its-billing-period-ended: 2444
  settle.repeat-or-after-expiry: 416961
  step.during-provider-call: 12078
  wake: 725770

usage on an invoice, by interval length and grace period (billing periods with usage):
  interval 1 h, grace 1 h: 13053 periods, 131513 minor units at the provider, 5106 of them (3.9%) arrived after the invoice was finalized, 33781 provider calls
  interval 1 h, grace 72 h: 12869 periods, 129084 minor units at the provider, 2067 of them (1.6%) arrived after the invoice was finalized, 33361 provider calls
  interval 24 h, grace 1 h: 13011 periods, 130000 minor units at the provider, 5688 of them (4.4%) arrived after the invoice was finalized, 32980 provider calls
  interval 24 h, grace 72 h: 12851 periods, 128812 minor units at the provider, 2008 of them (1.6%) arrived after the invoice was finalized, 32968 provider calls
  interval whole period, grace 1 h: 12915 periods, 130310 minor units at the provider, 15431 of them (11.8%) arrived after the invoice was finalized, 30286 provider calls
  interval whole period, grace 72 h: 13274 periods, 133259 minor units at the provider, 6059 of them (4.5%) arrived after the invoice was finalized, 31220 provider calls

controls that must miss the rule:
  a retry draws a new identifier: missed the rule at ordering 16 of seed 1
    check: the provider-side total passes the reported total (acct-0: provider 20000, reported 10000)
    smallest failing ordering: 3 commands, 1 faults
  a resend after the window skips the read: missed the rule at ordering 7 of seed 1
    check: the provider-side total passes the reported total (acct-0: provider 20000, reported 10000)
    smallest failing ordering: 5 commands, 1 faults
  the remainder is dropped at each interval: missed the rule at ordering 3 of seed 1
    check: the stored remainder (acct-0 period 0: stored 0, measured 100)
    smallest failing ordering: 2 commands, 0 faults
  the last remainder is rounded up and sent: missed the rule at ordering 12 of seed 1
    check: the provider-side total passes the reported total (acct-0: provider 10000, reported 1)
    smallest failing ordering: 4 commands, 0 faults
  intervals are cut at the cap period's ends: missed the rule at ordering 3 of seed 1
    check: the ledger's reported total (acct-0 period 0: ledger 0, driver 100)
    smallest failing ordering: 2 commands, 0 faults

further controls (reported, not part of the rule line):
  the event carries the interval's start as its timestamp: missed the rule at ordering 47 of seed 1
    check: refused by the provider (acct-0:1800000017: timestamp_too_old)
    smallest failing ordering: 5 commands, 0 faults
  an interval is sealed while one of its holds is open: missed the rule at ordering 15 of seed 1
    check: the stored remainder (acct-1 period 0: stored 0, measured 100)
    smallest failing ordering: 5 commands, 0 faults
  the 24 hours are counted again from each resend: missed the rule in the fault-sequence search, at sequence 284892, and in none of 100000 generated orderings
    check: the provider-side total passes the reported total (acct-0: provider 20000, reported 12345)
    smallest failing ordering: 3 commands, 4 faults
  a read takes its whole range for the one interval: missed the rule at ordering 510 of seed 1
    check: one event for each acknowledged interval (acct-0 interval 1802592017: 0 events, outcome acknowledged)
    smallest failing ordering: 9 commands, 2 faults
  a later interval is sealed before an older one with an open hold: missed the rule in the fault-sequence search, at sequence 1074021, and in none of 100000 generated orderings
    check: the equation with a remainder under one minor unit (acct-0 period 0: reported 15000 + carried in 0 - provider 20000 = -5000)
    smallest failing ordering: 10 commands, 0 faults
  the identifier leaves out the account: missed the rule at ordering 75 of seed 1
    check: one event for each acknowledged interval (acct-1 interval 1800000017: 0 events, outcome acknowledged)
    smallest failing ordering: 5 commands, 0 faults
  a settle counts in the newest interval, not the hold's own: missed the rule at ordering 273 of seed 1
    check: the ledger's reported total (acct-0 period 0: ledger 0, driver 1)
    smallest failing ordering: 6 commands, 0 faults

limits (outside the rule line):
  a read that lags 30 hours behind a 24-hour window, the sync as designed: missed the rule at ordering 35 of seed 1
    check: the provider-side total passes the reported total (acct-0: provider 20000, reported 10000)
    smallest failing ordering: 3 commands, 1 faults
  the object is down for 36 days after a billing period ends, the sync as designed:
    the fake's answer to the send: refused (acct-0:1800000017: timestamp_too_old)
    acct-0 period 0: reported 250000, provider-side 0, short by 250000 micro-units

rule: MET (100000 generated orderings over 5 seeds, 54860 of which put usage at the provider, and 1138851 fault sequences passed every check, and all 5 controls missed the rule)
```
