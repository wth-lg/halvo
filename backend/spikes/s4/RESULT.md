rule: MET (third run: the device-credential path reaches an active subscription with a cap in 9 actions for a US buyer and 8 for a UK buyer, against 9 and 8 expected, and all six conditions hold. The second run read MET at the same counts, with a browser that the driver started itself inside the sandbox. The first run, whose app page had a second click, read MISSED at 10 and 9 actions against the 8 and 7 stated before it)

# S4: prompt count through checkout

Measured on Apple M3 Max, 64 GiB, Darwin 27.2.0, Node v26.12.0: headless Chromium 158.0.8127.18 through Playwright 1.65.0; Better Auth 1.7.18 with @better-auth/passkey 1.7.18 on node:sqlite, in memory, telemetry off (its option, and BETTER_AUTH_TELEMETRY=0); oauth2-mock-server 9.3.0. Every page and server is a local stand-in on loopback. The commit that holds this file dates the run. This is the third run: the pages, the driver's actions and the rule of the second, with a browser that the repository's outbound check starts. After the measured part come what was stated before this run, then the record of the second run and of the first, each followed by what was stated before it.

## Counts

| candidate | buyer | reached | actions | start | card | cap | outside | typed fields |
|---|---|---|---|---|---|---|---|---|
| device credential, no sign-in step | US | active with a cap | 9 | 1 | 7 | 0 | 1 | 6 |
| device credential, no sign-in step | UK | active with a cap | 8 | 1 | 6 | 0 | 1 | 5 |
| passkey first | US | active with a cap | 10 | 1 | 7 | 0 | 2 | 6 |
| Google-style sign-in | US | active with a cap | 9 | 1 | 7 | 0 | 1 | 5 |

An action is one entry in the driver's log: a click, a typed field or a selection. The classes are the plan's: start is the first click, card is the card form on the payment page, cap is the spend cap, outside is everything else. The email typed on the payment page is class outside for every candidate.

Ranked by the rule on the logged actions, US buyer (fewer outside actions first, fewer typed fields on a tie):

1. Google-style sign-in: 1 outside action, 5 typed fields. Not measured, and not read as zero: the identity provider's own page, which asks for one or two confirmations in a browser that is already signed in (researcher-2 summary, an inference with no primary source) and for more in any other.
2. device credential, no sign-in step: 1 outside action, 6 typed fields.
3. passkey first: 2 outside actions, 6 typed fields. Not measured, and not read as zero: the system's passkey prompt (Touch ID, Windows Hello or a PIN), at least one on real hardware (researcher-5, option 3).

The order above counts only what the driver did. The rows named in it as not measured could not appear on this machine. If each adds one action, the least it can add, the order by outside actions is: device credential, no sign-in step (1), Google-style sign-in (2), passkey first (3).

## What the result decides

- The budget. The device-credential path took 9 actions with a US buyer (8 with a UK buyer, who gets no postal code). The count stated before this run was 9 (8 for the UK buyer). The log holds 1 click in the app, 7 checkout fields and the click on the pay button. No action is of class cap, because the click in the app sets the cap. gate/prompt-budget.json records budget 9 and expected 9. The first run, whose app page had a second click, read MISSED at 10 and 9 actions against the 8 and 7 stated before it.
- D7, first-run identity. The device credential adds no action before the payment page. Passkey first logs 10 actions against the device credential's 9: 1 click on the sign-in page more. It also carries a row that is not measured. Google-style sign-in logs 9 actions against the device credential's 9: 1 click on the sign-in page more and 1 typed field fewer. It also carries a row that is not measured. A candidate with a sign-in step goes to the owner with its count and is not built on it.
- D8, the return path after a browser sign-in. The device-credential path opens no sign-in page (0 actions in the browser outside the payment page), so D8 is not on its first-run path. Both other candidates pass through the sign-in page, so D8 is on theirs. In this stand-in the sign-in page goes straight on to the payment page and never returns to the app in between.

## The device-credential path, action by action (US buyer)

| n | where | class | action | target | source | status |
|---|---|---|---|---|---|---|
| 1 | app | start | click | subscribe | plan: plan.md, units S4 and 8.1: the one click, which sets the default cap and starts checkout | measured |
| 2 | checkout page | outside | typed | email | docs: r1#93, r5#27 | measured |
| 3 | checkout page | card | typed | card-number | docs: r5#107 | measured |
| 4 | checkout page | card | typed | card-expiry | docs: r5#107 | measured |
| 5 | checkout page | card | typed | card-cvc | docs: r5#107 | measured |
| 6 | checkout page | card | typed | cardholder-name | docs: r5#106, r5#20 | measured |
| 7 | checkout page | card | selection | country | docs: r5#106 | measured |
| 8 | checkout page | card | typed | postal-code | docs: r5#107, r5#76 | measured |
| 9 | checkout page | card | click | pay | unknown | measured |
| | checkout page | card | | 3-D Secure challenge | unknown | not measured |
| | browser | card | | wallet prompt (Apple Pay or Google Pay in place of the card) | docs: r5#104, r5#105 | not measured |

These are the rows of gate/prompt-budget.json. The two rows that are not measured carry no count and add nothing to the budget.

## The conditions of the rule line

1. Holds: both device-credential runs reach the state active with a cap set (US buyer: active with a cap; UK buyer: active with a cap).
2. Holds: in both runs the driver's log for the checkout page equals the page's own count (US buyer: 8 logged, 8 counted by the page; UK buyer: 7 logged, 7 counted by the page).
3. Holds: neither device-credential run logs more actions than its expected count (US buyer: 9 logged, 9 expected; UK buyer: 8 logged, 8 expected).
4. Holds: gate/prompt-budget.json holds one row for each action logged on the US run, the two rows that are not measured, and no other row (budget 9 beside expected 9, 9 measured rows for 9 logged actions, 2 rows not measured).
5. Holds: this file gives each of the three candidates its count, or the reason it was not measured (device credential, no sign-in step, US buyer: 9; device credential, no sign-in step, UK buyer: 8; passkey first, US buyer: 10; Google-style sign-in, US buyer: 9).
6. Holds: every control fails as stated (4 of 4).

## Outbound

This run counts. The measuring step ran under scripts/outbound-hosts.sh browser: the outbound check started the pinned Chromium under its second profile, and the driver connected to it through test-kit/browser. The check counted no denied attempt (denied=0): no record of a process that it ended, an empty Node record, and no attempt outside loopback in Chromium's net log of the whole step. That count leaves out Chromium's probes of IPv6 reachability, a UDP connect to [2001:4860:4860::8888]:443 that sends no packet and that the check denies in any case. The check's output says how many it left out; the number changes from run to run and is in .tmp/s4/sandbox.txt. The driver refused 0 page requests outside loopback over the 4 measured runs. The request of the remote-request control never reaches a socket, because the driver refuses it first, so the check has nothing to count for it. The spike's Node code reaches its servers by address, not by the name localhost: the check ends a Node process that looks up that name.

## Controls

| control | what the run showed | failed as it must |
|---|---|---|
| unlogged input | the page counted 9 inputs and the log holds 8 actions for the page | yes |
| no payment | the page answered "refused" and the backend reports: pending, cap set | yes |
| budget rows | one row added: 10 measured rows for 9 logged actions; one action row removed: 8 measured rows for 9 logged actions; the wallet prompt given a count: the row wallet-prompt is not measured and carries a count | yes |
| remote request | the driver counted 1 request outside loopback in the control and 0 in the 6 other runs | yes |

## How each value was measured, and what limits it

- Actions. Each number is the length of the driver's action log for one run of the path. The one wrapper that performs a click, a typed field or a selection writes the entry (handle in spikes/s4/measure.mjs). Chromium reported no dialog in any run; one would have been logged as an action.
- Runs. One run of each path for each command. The counts are whole actions on fixed pages and carry no timing, so a second run writes the same file.
- The page's own count. The stand-in page counts a field each time its value changes and the field is left, and a button each time it is clicked. It sends the count with the form, and the stand-in's server keeps it. The page does not tell a person's input from a script's, so the equality proves that the log and the page agree, not that a person could do the same. The page also sends the row of each input, and a run whose list differs from the log's does not count.
- Reached. The state is what the stand-in backend holds when the run ends. The subscription becomes active when the payment page has sent a complete form with a valid test card. The cap is set when the one click in the app sends the default that the page shows. The backend, the app view, the sign-in page and the payment page were written for this spike. No Tauri app, no provider API and no signed webhook is in the path.
- Payment page. It shows the rows of test-kit/checkout-fields.json. A US buyer fills 7 fields (7 modelled on report rows, 0 unknown) and makes 1 click (0 from a report row, 1 unknown: no row names the button that sends the form). A UK buyer fills 6. Nobody has seen these rows on the hosted page. What that page calls the name field and which countries get a postal code are open (researcher-5, Unknowns). Whether it fills in the country by itself is this spike's own open question.
- The app. The page shows the default cap above one Subscribe button, as the view of the plan's unit 8.1 does. The click sends that cap with the request that starts checkout, and the backend keeps it. The default, 20 USD a month, is a fixture in the page until R6 sets the product's number. The page of the first run had a second click, on a cap control that it showed after the payment.
- Passkey first. 1 passkey was made in Playwright's virtual authenticator and Better Auth stored 1 passkey row. The virtual authenticator answers with no prompt, so the system prompt of a real authenticator is not in the count.
- Google-style sign-in. oauth2-mock-server answers the request at once and shows no page, so the provider's account choice or sign-in is not in the count. Better Auth passed the email from the mock to the payment page, which showed it locked, so this candidate types one field fewer there.
- Sign-in page. The one click that each sign-in candidate logs there is on a button of the stand-in page. Better Auth and the mock ask for no click, and a real page could start the step by itself.
- Better Auth rows. With IP tracking off and the user agent blanked by a hook, 0 of the 2 session rows of the two sign-in candidates held an IP address or a user agent.
- Not counted. The path ends on the payment page. The driver then waits until the app page shows the state that the backend holds, which is no action. What the real app and browser show after the payment belongs to S5 and to phase 1. The plan also counts an answered OS dialog as a prompt (plan.md, done-when). No check on this Mac observes one, so none is in a count.

## The payment page's rows by seller pair

- the local gate, automatic tax off: 7 fields for a US buyer.
- Stripe as the seller, Stripe Tax on: at least 7 fields for a US buyer; no number is stated for the minimum address fields that tax calculation needs (r1#93).
- Stripe Managed Payments: at least 7 fields for a US buyer; no number is stated for the buyer's name (r1#99) and a valid billing address (r1#99).
- 3-D Secure challenge: not measured (source unknown).
- wallet prompt (Apple Pay or Google Pay in place of the card): not measured (source docs: r5#104, r5#105).

Reproduce: `sh spikes/s4/run.sh` after `sh scripts/prerequisites.sh`. It runs the paths and the controls under the outbound sandbox, then rewrites this file above the next heading and gate/prompt-budget.json. A run with the same result on the same machine, with the same versions, leaves `git status` clean. Exit 0 for MET, 1 for MISSED, 2 when the run could not be made.

## Stated before the third run

I wrote this section before the third run, knowing the result of the second. The second run, the first run and what was stated before each stay below as they were.

### What changes: how the driver gets its browser

In the first two runs the driver started Chromium itself, inside the sandbox of `sh scripts/outbound-hosts.sh run`, and read Chromium's net log itself. Since the second run the repository's outbound check ends a process at its first call outside loopback, so that it sees every attempt. A browser that is started inside that sandbox ends at Chromium's own check for IPv6 reachability. At commit 5317fc8, with the driver of the second run, no page loads: each of the seven paths and controls stops at its first `page.goto` with net::ERR_ABORTED, the check counts 7 denied attempts, and the rule line reads MISSED.

In the third run the outbound check starts the browser. `sh scripts/outbound-hosts.sh browser` starts the pinned Chromium before the measuring step, under its second profile, and the driver connects to that browser through test-kit/browser. The second profile denies everything outside loopback, as the first does. The check reads the browser's net log and counts every attempt outside loopback in it, except Chromium's check for IPv6.

Nothing else changes. The pages, the stand-ins, the driver's actions and its log, the rule (rule.mjs) and the four controls are those of the second run.

### The expected count of the third run

The same as for the second run, in the table under "Stated before the second run" below: 9 actions for a card with a postal code (US buyer) and 8 for a card without one (UK buyer).

### When the rule line of the third run reads MET

When the six conditions stated before the second run hold. They are unchanged.

A run counts only when it went under the outbound check in its browser mode, the driver got its browser from that check, and the check counted nothing (denied=0). This is the sentence stated before the second run, with the check now doing what the spike did then. The check's count leaves out only Chromium's check for IPv6, and its output says how many times it left one out. It fails on any other attempt in Chromium's net log, on a record of a process that it ended, on a Node record that is not empty, and on a net log that is missing or not complete. The spike no longer reads the net log. It also no longer asks that the log hold the request of the remote-request control: the driver refuses that request before it reaches a socket, and the check counts what reaches a socket.

## The second run (commits c329f21 and 073d185)

Its rule line, as commit 073d185 holds it:

> rule: MET (second run: the device-credential path reaches an active subscription with a cap in 9 actions for a US buyer and 8 for a UK buyer, against 9 and 8 expected, and all six conditions hold. The first run, whose app page had a second click, read MISSED at 10 and 9 actions against the 8 and 7 stated before it)

| candidate | buyer | actions | start | card | cap | outside | typed fields |
|---|---|---|---|---|---|---|---|
| device credential, no sign-in step | US | 9 | 1 | 7 | 0 | 1 | 6 |
| device credential, no sign-in step | UK | 8 | 1 | 6 | 0 | 1 | 5 |
| passkey first | US | 10 | 1 | 7 | 0 | 2 | 6 |
| Google-style sign-in | US | 9 | 1 | 7 | 0 | 1 | 5 |

In that run the driver started the browser itself inside the sandbox, which then denied Chromium's check for IPv6 without ending the process. `git show 073d185:spikes/s4/RESULT.md` prints the whole result of that run, and `git show 073d185:gate/prompt-budget.json` prints its budget file.

## Stated before the second run

The plan's unit S4 was revised after the first statement in this file, and the coordinator ruled on it in a message. I wrote this section after the first run and before the second, knowing the numbers of the first run. The first run, and what was stated before it, stay below as they were.

### The ruling

> Ruling on your spike after the plan was revised (plan.md line 26, written before your result existed). Three things differ from what you were told. (1) The expected count is 9 actions for a card with a postal code and 8 without, not 8 and 7: the click on the pay button counts, because a person makes it. (2) The app page has one click, class start: it shows the default cap above one Subscribe button, and that click sets the cap and starts checkout, so the path logs no action of class cap and no second click in the app. (3) The stand-in page counts the pay click together with the inputs it receives. Your first run (10 and 9, rule MISSED, commit 5eb8d27) stays in the record as it is, with the reason: it holds one app click that this design does not have. Now: build the one-click app page, write the new expected numbers into RESULT.md before the rerun, rerun both forms, and report both runs. Read line 26 of plan.md for the full rule text; where it differs from your brief, the plan wins. Never run rm.

These are the times that the files show, in local time on 2026-11-09. The first trial of the measuring step ran at 21:48. Commit 5eb8d27 with the first result is from 21:58:44. plan.md was last modified at 22:43:32, and its Revisions list dates the S4 edit 22:00. The planner's report (reports/planner-3.md) says that it wrote 9 and 8 at 21:54, before that result existed. I cannot check that time from the files.

### The rule, as the revised plan states it (plan.md line 26)

> The rule ranks the candidates by outside actions, fewer typed fields first on a tie; a candidate with a sign-in step is not built on its count but goes to the owner with it. The expected count of the device-credential path comes from documents, not from the run, and is written into RESULT.md before the run it is compared with: the app's one click, class start (the spike's app page shows the default cap above one Subscribe button, as the view of unit 8.1 does, and that click sets the cap and starts checkout, so the path logs no action of class cap and no second click in the app); one action for each documented input of the hosted page for a typed card with defaults, 6 (email, card number, expiry, CVC, name, country) or 7 with the postal code of a US card (P1's count from r5#27, r5#107, r5#106, r5#20, r5#26 and r5#76, derived from those pages and not seen live); and the click on the page's pay button, which the counting rule counts and no report row lists, because it is not a field. That is 9 actions for a card with a postal code, as in the US, and 8 for one without. The driver runs the path once for each form and clicks the pay button as a person does, and the page counts that click with the inputs it receives. The gate's budget is the count of the US run, the longer form, and gate/prompt-budget.json records it beside the expected count. Its rule line reads MET when the device-credential path reaches an active subscription with a cap in both runs; in each run the driver's action log for the stand-in page equals the page's own count of inputs received; neither run logs more actions than its expected count, and one that does turns the line to MISSED; gate/prompt-budget.json holds one row for each action logged on the US run, the two rows printed as not measured and no other row; and RESULT.md gives each of the three candidates its count or the reason it was not measured.

### The expected count of the second run

| form | the app's one click | documented inputs of the hosted page | the click on the pay button | expected count |
|---|---|---|---|---|
| a card with a postal code (US buyer) | 1 | 7 | 1 | 9 |
| a card without one (UK buyer) | 1 | 6 | 1 | 8 |

The app page of the second run shows the default cap above one Subscribe button, as the view of the plan's unit 8.1 does. The click on that button sends the cap with the request that starts checkout. No path has a second click in the app, and the three candidates share the page.

### When the rule line of the second run reads MET

It reads MET only when all six hold, and MISSED when any one does not.

1. Both device-credential runs reach the state active with a cap set, as the stand-in backend reports it.
2. In each run the number of actions that the driver logged for the checkout stand-in page equals the number of inputs that the page counted itself and sent with the form. The page counts the click on its pay button with its fields.
3. Neither run logs more actions than its expected count, 9 and 8.
4. gate/prompt-budget.json holds one row for each action logged on the US run, the two rows 3-D Secure challenge and wallet prompt with the status not measured and no count, and no other row. It records the count of the US run as the budget and the expected count beside it.
5. This file gives each of the three candidates its count, or the reason it was not measured.
6. Every control fails as stated before the first run. If one does not, the result does not count.

The ranking and the four controls are the ones stated before the first run.

A run counts only when it went under the outbound sandbox, the sandbox denied nothing but Chromium's check for IPv6 reachability, its Node record is empty and Chromium's net log holds no other attempt outside loopback. The first statement did not say this. The first run showed the need for it: the sandbox denies that check and counts it, so its own count is above 0 in most runs.

## The first run (commit 5eb8d27)

Its rule line, as that commit holds it:

> rule: MISSED (condition 5 does not hold: the device-credential path took 10 actions with a US buyer against 8 expected, and 9 against 7 with a UK buyer; the 2 beyond the expected count are the click on pay and the click on confirm-cap; conditions 1, 2, 3, 4 and 6 hold)

| candidate | buyer | actions | start | card | cap | outside | typed fields |
|---|---|---|---|---|---|---|---|
| device credential, no sign-in step | US | 10 | 1 | 7 | 1 | 1 | 6 |
| device credential, no sign-in step | UK | 9 | 1 | 6 | 1 | 1 | 5 |
| passkey first | US | 11 | 1 | 7 | 1 | 2 | 6 |
| Google-style sign-in | US | 10 | 1 | 7 | 1 | 1 | 5 |

The app page of that run had two clicks. One was on Subscribe. The other was on a cap control that the page showed after the payment. The ruling gives the reason this run is kept apart from the second: it holds one click in the app that the plan's design does not have. Its expected count, 8 and 7, also left out the click on the pay button. Three runs and one from a fresh clone wrote the same files. `git show 5eb8d27:spikes/s4/RESULT.md` prints the whole result of that run, and `git show 5eb8d27:gate/prompt-budget.json` prints its budget file.

## Stated before the first run

### The rule, as the plan states it (plan.md, phase P0, unit S4)

> The rule ranks the candidates by outside actions, fewer typed fields first on a tie; the gate's budget is the device-credential path's count; a candidate with a sign-in step is not built on its count but goes to the owner with it. Its rule line reads MET when the device-credential path reaches an active subscription with a cap; on that path the driver's action log for the stand-in page equals the page's own count of inputs received; gate/prompt-budget.json holds one row for each action logged on that path, the two rows printed as not measured and no other row; and RESULT.md gives each of the three candidates its count or the reason it was not measured.

One action is one click, one typed field, one selection or one answered browser dialog (plan.md, done-when). The count of a path is the number of entries in the driver's action log, from the first click in the app's subscription view to the state active with a cap set. Every action of the driver goes through one wrapper, which logs it.

### The expected count (the coordinator's message, received before any run)

> Before the first measured run, write the expected count into the spike's RESULT.md beside the rule: the documented checkout fields plus the app's one click. The research on the first run (reports/researcher-5.md) derives 6 inputs on the checkout page, 7 in the US. The rule line then reads MISSED when the measured count of the device-credential path is above that expected count, and the budget file records both numbers.

| buyer | documented checkout inputs (researcher-5, Summary 6) | the app's one click | expected count |
|---|---|---|---|
| in the United States | 7: email, card number, expiry, CVC, name, country, postal code | 1 | 8 |
| in the United Kingdom, where no postal code is collected (researcher-5 row 76) | 6: the same without the postal code | 1 | 7 |

The device-credential path is driven once for each buyer. The budget is the count of the US run: it has the longer form, and a budget must admit it. gate/prompt-budget.json holds the rows of the US run, the budget and the expected count.

### When the rule line reads MET

It reads MET only when all six hold, and MISSED when any one does not.

1. Both device-credential runs reach the state active with a cap set, as the stand-in backend reports it.
2. In both runs the number of actions that the driver logged for the checkout stand-in page equals the number of inputs that the page counted itself and sent with the form.
3. gate/prompt-budget.json holds one row for each action logged on the US run, the two rows 3-D Secure challenge and wallet prompt with the status not measured and no count, and no other row.
4. This file gives each of the three candidates its count, or the reason it was not measured.
5. Neither device-credential run logs more actions than its expected count.
6. Every control below fails as stated. If one does not, the result does not count.

### Ranking

The candidates are ranked by the actions of class outside in their logs, then by the number of typed fields. A row that is not measured is never read as zero: where a candidate carries one of its own, the ranking says so beside its place.

Two such rows are known before the run. The passkey candidate's system prompt (Touch ID, Windows Hello or a PIN) cannot appear, because a virtual authenticator answers the request. The Google-style candidate's page at the identity provider cannot appear, because the OAuth mock answers at once and shows no page.

### Controls (each must fail)

| control | what it does | what it must show |
|---|---|---|
| unlogged input | the US run, with one more field typed outside the driver's wrapper | the page counts one input more than the log holds, so condition 2 fails |
| no payment | the US run with the card number left empty | the stand-in page refuses the form and the state never becomes active, so condition 1 fails |
| budget rows | three altered copies of the budget rows: one row added, one action row removed, the wallet prompt given a count | condition 3 fails for each copy |
| remote request | a page of the run asks for http://192.0.2.1/, an address outside loopback | the driver's own count of such requests is 1; in every other run it is 0 |

Every run goes under the repository's outbound sandbox (scripts/outbound-hosts.sh run). That sandbox denies a browser's request outside loopback but does not always count a single one (README, Gate), so the driver also counts such requests itself and refuses them before they leave the browser. The last control proves that count.
