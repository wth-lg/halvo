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
   action and adds its step. At a `handback` it signs in itself when the page shows a
   credential field. A `click` or a `press` runs with the request rule of the library letting
   the bindings' state-changing requests through, which is the harness answering the
   confirmation with allow; a `fill`, a `select` and an `extract` run with every
   state-changing request failed. A step during which a state-changing request left becomes
   an effect step (`effect` `request`) with the bound fields of A9.4, and the compile fails
   with the cause `provenance` when the extracts of its row are not kept by then.
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
- A reply of the model that is no action, or an action that carries a field that it does not
  take (a `fill` with a `key` or a `name`, which A12.4 forbids), or that names no line of the
  snapshot, is refused and not carried out; the step list in the next request says why, and
  the compile goes on. The harness also refuses a `fill`, a `select` or a `press` on a line
  that the snapshot marks `credential`, and counts it in `typed_credential`.
- A compile asks the model at most 32 times, and it ends as a miss when the model gives the
  same reply to the same page a second time, because a greedy model then repeats itself.
- `typed_credential` and `effects_without_confirm` of a `drive` line are the sums over its
  five runs. `effects_without_confirm` of a run is the count of the entries of the four
  lists of the state object (payments, orders, sent mail, deleted events) beyond the steps
  that the harness allowed, a step being allowed when it was confirmed and a state-changing
  request left during it.
- `recover=ok` needs the run to complete with the state check; a step that found its control
  needs no model.
- A run that starts the model starts its child at the start of the run, so `wall_s` holds the
  load; the replays start none.
- A locator is `role` with the role and the name of the snapshot line, or `text` with the
  text for a text line, the one extract target that the snapshot gives; a playbook of
  `bill` thus holds the amount's own text as the locator of its extract step, and the test
  ids of A10.3, which the library's snapshot does not show, are in no locator.
- The bodies are counted as the model wrote them: a MiniCPM5 body that is all thinking counts
  when it is not empty and holds no marker and no address.
