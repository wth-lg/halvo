---
name: verify-cap-ledger
description: Check a change to the cap ledger, to the gateway's calls or to the account object's time and alarm against the TLA+ model in formal/ and its replay. Use when a diff touches src/ledger/, src/gateway/chat.ts, the entries reserve, dispatch, settle, void, alarm, rearm, capSet or capGet of src/do/entries.ts, formal/ or test/ledger/model-replay.mjs.
---

# Verify the cap ledger

`formal/CapLedger.tla` is a model of the account object's cap ledger and of the gateway that calls
it. TLC checks eight properties on every state that it keeps and on every step that the model can
take, at the bounds of `formal/CapLedger.cfg` and of `formal/CapLedgerGateway.cfg`. A replay then runs traces
of the same model against the real ledger in Node and compares the stored rows after every
action. `formal/README.md` states the properties, the bounds, what the model leaves out and how a
trace maps to the object.

## Does a change touch what is verified

| File | What the model describes of it |
|---|---|
| `src/ledger/ledger.ts` | every function that takes `now`: the switch of the period type, the expiry of due holds, then the function's own work |
| `src/ledger/periods.ts` | the week; a month only as a fixed number of ticks |
| `src/do/entries.ts` | `reserve`, `dispatch`, `settle`, `void`, `alarm`, `rearm`, `capSet`, `capGet`: the step's time and the alarm after it |
| `src/gateway/chat.ts` | the order reserve, then void or dispatch, then the upstream call only on `go`, then settle |

The sync commit is 677c053acf001ef05d3cac105979f0ef07a0bc4f: the model was last confirmed to
describe the code at that commit. Read `git diff 677c053 -- src/ledger src/gateway/chat.ts
src/do/entries.ts` first. Move the sync commit forward, here and nowhere else, only after the
check passes at the new commit and you have read that diff against the model's steps.

## Run

```sh
sh scripts/prerequisites.sh          # once: fetches the pinned tla2tools.jar into .tools/
sh gate/checks/95-model.sh           # the checker on the eight properties, then the replay
node formal/controls.mjs             # after a change of the model: 17 reachability controls, 11 breaks, the control of the view
```

`MODEL_WORKERS=6` in front of either command gives TLC six worker threads in place of 12.

The check prints one line, `PASS model: ...` or `FAIL model: ...; first failure: ...`. It keeps
TLC's output and the generated traces in a new directory `model.*` under `.tmp/`.

## Read a failure

- `violated=StepA` or `violated=StateA`, with any letter from A to H: the property of that letter
  in `formal/README.md`. Open the file that the log names (`check.out`, or `check-gateway.out`
  for the second configuration). TLC prints the states that lead to the step; `last` in each
  state is the call that was delivered and its answer. Write the
  calls as a list and make each one against the real ledger with `seededRig` of
  `test/ledger/harness.ts`. If a step cannot happen in the code, or the code answers otherwise,
  the model is wrong: correct the model from the code. If every step happens as the model says,
  the code breaks the property: report it with that list, and do not weaken the property.
- `differences=N`. The line names the trace, the action and the first field that differs. The
  model's value comes from `Step` in `CapLedger.tla`, the object's from the ledger. Decide from
  contract 3.2 and the plan's design which one is right. A model that is wrong is drift: correct
  it and run the controls. A ledger that is wrong is a bug: report it with the trace.
- `elements=K of N` with K below N. The traces no longer reach a listed case. Either a guard of
  the model now forbids it, which is a finding about the model, or the walk's weights in
  `CapLedgerTrace.tla` need another draw; the replay's `missing:` line names the case.
- `TLC counted N distinct states ..., not the M that formal/counts.json records`. The search was
  complete and has another size than the recorded one. When the model or a configuration changed
  on purpose, record the new count in `formal/counts.json` in the same commit, and the new count
  of one request id in the two bad fixtures of `gate/fixtures/model/` that name it. When nothing
  changed on purpose, a bound was cut or the view changed: find out which before you record a
  count.
- `TLC did not complete the check ... (exit N)`. TLC ended with an error status or without its
  count line, and printed no violation. Open the output that the log names: an out-of-memory
  error, a parse error of the spec and a Java error all end here.

## Change the model

Keep each guard on a line of its own, so that a break stays one exact replacement. After a change
run `node formal/controls.mjs`: every reachability control must still be violated, and every break
must still report its own property. A break whose text is gone needs a new text in `BREAKS` of
`formal/controls.mjs`. A property that changes is a change of the plan's promise: say so in the
commit and in `formal/README.md`.

A new field of the object, or a new thing that a step reads, needs a look at `View` in the spec.
The view says which states the search takes for the same one. A field that a later step reads
and the view leaves out makes the search skip states, and the check still prints PASS.
`node formal/controls.mjs view`, which the self-test of the check runs, compares the search with
the view with a search without it for the first six steps; what it shows ends there. The
section "How the search ends" of the README lists what the view holds and why the rest is left
out.
