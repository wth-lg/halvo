# Driver trial (R3, unit U0.6)

The trial of contracts.md A13.6: whether a small model on the person's own machine can drive
the two browser tasks of the fixture (`bill` and `order`) well enough that its compiled
playbook replays with no model and survives a changed page (path A), or whether a recorded
demonstration has to replace the model (path B); and which model writes the reply bodies of
the task `reply`. Its result file is `docs/r3/trials/driver.md`.

| File | What it is |
|---|---|
| `run.sh` | The entry script: builds the two binaries and runs one cell (`--help`) |
| `src/main.rs` | `r3-drive`: the arguments, the fixture server, the runs of one cell in order, the lines |
| `src/drive.rs` | The compile, replay, recovery and write runs on the cdp library of `tools/r3-trials/web` |
| `src/action.rs` | The reader of a reply of the grammar `action` (A12.4), with the fields of each action |
| `src/step.rs` | The steps of A3 that a compile writes, the trial's own shape check and its copy of A4.3's table |
| `src/model.rs` | The child `r3-model` under the cap of halvo-cap, one request line and one reply line at a time |
| `src/fixture.rs` | The fixture server with a pipe on its input, its reset and state, and the mail face |
| `src/body.rs` | The reply rule and the rule that counts a body |
| `src/lines.rs` | The line forms of A13.6 and of this trial's `load` line |
| `verdict.py` | The verdict rule over the lines of the result file (`--check`, `--write`, `--self-test`) |
| `rebuild.py` | Makes the block again from the raw cell files, the helper's log and the job log (`--compare`) |
| `control.sh`, `control/` | The control: one cell with the stand-in child, its five mode files and the expected lines |
| `src/bin/r3-standin.rs` | The stand-in child: no model, one action of the grammar per request, by role and name |
| `prompts/` | The boundary text and the three goal texts of A9.5 |

## How to run

```sh
sh tools/r3-trials/model/fetch.sh --cmake            # once, on macOS without cmake
sh tools/r3-trials/model/fetch.sh minicpm5_2b        # the weights of a model, checked by SHA-256
sh tools/r3-trials/drive/run.sh --model minicpm5_2b --task bill --seed 1 \
  --browser "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" --port 18471
sh tools/r3-trials/drive/run.sh --prompt-lines       # the prompt lines of the result file
python3 -I tools/r3-trials/drive/verdict.py --check  # the verdict line against the lines
```

On this Mac every cell runs through the machine's timed-run helper, one at a time, and the
model child takes the machine-wide model lock; a cell whose child finds the lock held waits
for it, up to an hour, and the time of that wait is not part of `wall_s`.

## What one cell does

A cell of `bill` or `order` is five runs on a fixture reset before each, and each run starts
a browser of its own (headless, with the first flag set of A13.2, a profile directory under
the cell's directory that the library removes at the stop):

1. The compile run. The harness writes step 1, a `navigate` to the slot `start`, and opens
   it. Then it asks the model for one action at a time: the request holds the boundary text
   as `system`, the goal text with the slot names, the names kept so far and the steps so
   far as `prompt`, and the page's snapshot as `snapshot` (A9.5). The harness carries out the
   action and adds its step. Before each request it reads the page, and while a line is a
   credential field it signs in itself, writes the handback step, and asks the model nothing on
   that page (ruling CP-177); a replay and the recovery's repair ask read the page the same way. A `click` or a `press` runs with the request rule of the library letting
   the bindings' state-changing requests through, which is the harness answering the
   confirmation with allow, but only once the extracts of the task's row of A9.4 are kept; a
   `fill`, a `select` and an `extract` run with every state-changing request failed. A step
   during which a state-changing request left becomes an effect step (`effect` `request`)
   with the bound fields of A9.4. A click that sends a state-changing request before those
   extracts are kept has its request failed, and the compile fails with the cause
   `provenance`, so nothing of it reaches the site.
2. Three replays of the playbook with no model, each on the same state check.
3. The recovery run on the variant `changed_page_2`: a step whose control the page no longer
   has is put to the model, at most `recoveries` times (2 at the level `medium`), with the
   step's verb and description; an answer of the same verb with the same slot, key or name on
   a control of the page does the step, and the run goes on.

A cell of `reply` is one write run: the harness lists the inbox through the mail face, takes
the messages of the reply rule of A1 and asks the model for one body each with the grammar
`text`.

## Choices that the contract leaves open, as built

- The order of the candidates is the order of the verdict line of
  `docs/r3/trials/model-cells.md`, which is the order of A13.5's table (that file's own
  choice); by file size `ministral_3_3b` would come before `granite_4_2_3b`.
- A reply of the model that is no action, an action that lacks a field that it needs, or one
  that names no line of the snapshot, is refused and not carried out; the step list in the next
  request says why, and the compile goes on. A field that an action does not take is never read
  and refuses nothing. The harness also refuses a `fill`, a `select` or a `press` on a line
  that the snapshot marks `credential`, and counts it in `typed_credential`.
- A compile asks the model at most 32 times, and it ends as a miss when the model gives the
  same reply to the same page a second time. The request differs at each call, because the
  step list grows, so the rule ends a model that cycles among the same actions; it ended 11 of
  the 18 compiles of each round on `mac`, and ministral_3_3b's traces cycle among three lines.
- A field of the grammar that an action does not take is never read. The grammar of
  `tools/r3-trials/model/grammars/action.gbnf` lets every action carry all five fields, and
  the models fill them all (`{"action":"fill","ref":"e3","slot":"email","key":"Enter","name":"email"}`
  in the first run of this unit); a fill with a value in `name` is still a fill of its
  slot, and the value is never typed. An action that lacks a field that it needs is refused.
- What a compile does on a page that shows a credential field is no choice left open: by the
  ruling CP-177 the harness hands back by its own reading (A6.4), at most three times in a row,
  then a miss with the cause `handed_back`, and a model never sees that page. A click of a
  model that sends a state-changing request before the extracts of A9.4 are kept, on any other
  page, has its request failed and ends the compile with the cause `provenance`. The first head
  of this unit let such a request leave and then failed the compile, so an order placed that way
  counted in `effects_without_confirm` (one `mac` cell of `ministral_3_3b`, `order` seed 3).
- `typed_credential` and `effects_without_confirm` of a `drive` line are the sums over its
  five runs. `effects_without_confirm` of a run is the count of the entries of the four
  lists of the state object (payments, orders, sent mail, deleted events) beyond the steps
  that the harness allowed, a step being allowed when it was confirmed and a state-changing
  request left during it.
- `recover=ok` needs the run to complete with the state check; a step that found its control
  needs no model.
- A run that starts the model starts its child at the start of the run, so `wall_s` holds the
  load; the replays start none.
- A target that the compile picked among more than one line of the same role, name and frame
  keeps its place among them, and a replay finds it again by that place (the saved items of the
  order page are three list items with empty names); a place that no longer exists is a miss
  with the cause `place_gone`. A3's form has no key for the place, so the step's JSON does not
  hold it, and a playbook file read alone loses it.
- A locator is `role` with the role and the name of the snapshot line, or `text` with the
  text for a text line, the one extract target that the snapshot gives; a playbook of
  `bill` thus holds the amount's own text as the locator of its extract step, and the test
  ids of A10.3, which the library's snapshot does not show, are in no locator.
- The bodies are counted as the model wrote them: a MiniCPM5 body that is all thinking counts
  when it is not empty and holds no marker and no address.

## How the cells of unit U0.6 ran

On `hosted-win` every cell of the three candidates ran in one job of a scratch run, its cells
one after another; on `mac` each cell was one call of the timed-run helper on a lane of the
unit's own, in the order minicpm5_2b, granite_4_2_3b, ministral_3_3b and within a model
`bill`, `order`, `reply` and seeds 1 to 3. The cells ran in two rounds: round 1 at heads
2f4b8bf and decb7e2, which differ in one test of `src/body.rs` and give the same release
binary, and round 2 at c882c15, which fails a request whose extracts are not kept; the result
file holds round 2. Two earlier calls at an uncommitted state, which refused every field that an action
does not take, are in the builder's evidence and in no line of the result file. A single
model request on `mac` took from 10 s to 650 s at 1-minute loads of about 100 to 240, so the
`wall_s` of a `mac` run says nothing of the trial's speed.

What the models did, from the traces of the runs: on `bill` minicpm5_2b gave `handback` on the
sign-in page and then a `select` of the slot `payee` on a heading, which the page did not take,
until it repeated itself, and on `order` it gave the same `select` twice on the sign-in page
and never handed back; granite_4_2_3b filled the `Email` box from a slot that the task does not
have and then clicked `Sign in`, which ends the compile with `provenance`; ministral_3_3b went
furthest. Its reply bodies held no marker and no address on `hosted-win`, but on `mac` one
body of seed 3 repeated the page marker of its message, in both rounds; granite_4_2_3b repeated
the page marker of the first message in one body of each seed, and MiniCPM5's bodies are its
thinking, which quotes the sender's address.

## The control

`sh tools/r3-trials/drive/control.sh --mode M --task bill|order --browser PATH --port N` runs one
cell of seed 1 with the stand-in child `r3-standin` in place of the model, with run.sh's own
argument list but for `--model standin_<mode>`, `--weights control/<mode>.txt` and `--child`.
The modes: `right` walks the task as a right driver would; `effect_early` clicks the effect
button before it keeps the extracts; `option_then_right` names an option line in its first
select, then goes on as `right`; `sign_in_click` (bill) clicks the sign-in button before any
handback; `option_line` (bill) names an option line in every select. The script reads the
lines against `control/expected.txt` and prints `control mode=.. machine=.. task=.. result=..`.
At d3472bf the right walk passed `bill` and failed every replay of `order` on `mac` and on
`hosted-win`, which showed that no driver could have passed `order`; the mend of 4d1020c, the
stored place, makes it pass.

## Revisions

- Ruling CP-177 (2026-11-13): the harness reads the page before each request to the model, each
  stored step and each repair ask, and hands back by itself while a line is a credential field,
  at most three times in a row; its sign-in closes the request rule again after its click; and
  the trace's run line prints the site's sign-ins, failed sign-ins and state-changing requests.
  The block of `docs/r3/trials/driver.md` was printed by the harness before this change.
