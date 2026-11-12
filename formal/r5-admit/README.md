# The model of admission and rollback

This directory holds the model that item L12 of the learning run's contract asks for.
TLC checks it. The model says how one learned repair of a step, which the contract calls
an edit, is admitted, served, counted, taken back and withdrawn (items L3.2 to L3.8 and
L4.2), and it states the properties Q1 to Q14 of L3.8. Two files come from it:
`edit-table.tsv`, which the test `edit_table` compares with `edit_step`, and
`traces.jsonl`, which the test `formal_traces` replays.

## The checker

TLC is `tla2tools.jar` of the tlaplus project's release `v1.7.4`. The jar is not in this
repository. The line below gives its address, its size in bytes and its SHA-256. A
download of that address on 2026-11-11 had this size and this hash.

```
jar url=https://github.com/tlaplus/tlaplus/releases/download/v1.7.4/tla2tools.jar bytes=2274532 sha256=936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88
```

The release and the SHA-256 are the ones that the billing run pins in
`scripts/fetch-tools.sh` and `scripts/tools.sha256` of its repository. TLC runs on Java 17
or later.

The jar holds two licence texts: `License.txt`, the MIT licence of the tools, and
`META-INF/LICENSE.txt`, the Eclipse Public License 2.0 of parts that it carries. A check
runs the jar. Nothing of the product links it and no bundle ships it (L0.8).

## Files

| file | what it holds |
|---|---|
| `Admit.tla` | the model: the step relation of one edit, the system, the properties Q1 to Q14 |
| `Admit.cfg` | the configuration that TLC checks |
| `Reach.tla` | states and steps that the system must be able to reach, each with its negation for TLC to violate |
| `Export.tla`, `Export.cfg` | the module and the configuration under which TLC prints the table and the traces |
| `export.sh` | the only writer of the two files below |
| `edit-table.tsv` | the 57 rows of L3.7 |
| `traces.jsonl` | 200 traces of one edit |

## The step relation

`EditStep(state, move, clean)` is what `edit_step` implements: the 7 states and the 8
moves of L3.7 with their tokens, and K = 2. It answers the next state and the next
`clean`, or `edit` with both unchanged. `Guard(state, move, clean)` names the row that a
call meets: `clean_below` and `clean_reaches` for `provisional` with `use_ok`, and `-`
for every other pair. The module asserts that outside that pair the next state does not
depend on `clean` and the next `clean` is the one passed in, but for `candidate` with
`keep`, which answers 0; and that `provisional` with `use_ok` and a `clean` of K answers
`edit`.

## The system

The properties are checked on a system that makes every move through `EditStep`.

| part of the model | what it stands for |
|---|---|
| `newest` | the agent's newest compile number, 1 or 2 |
| `edits[b][n]` | the stored edits of compile number `b` and step `n`, oldest first. An edit keeps every row that was written for it (`revs`), its list, the edit it names as `on`, its further key, and two values that the properties read: `recovered` (whether the log of the proposing run held the `recovered` line) and `onWas` (the state of the `on` edit when this one was stored) |
| a row of `revs` | one stored revision: the state, `clean`, `runs` (the numbers of the runs that it counts), `by` (the run whose `finish` wrote it, 0 for a compile) and `verified` (whether that run was verified) |
| `runs[s]` | what the host holds in memory for the run in slot `s`: its number (`id`, in the order of the loads), the compile number that it loaded (`base`), the edit served for each step (`served`), the step as it was served (`step`: the further key and the learned locators), and its candidates |
| `last` | what the environment knows of the last step and no row holds. After the `finish` of a run that was not lost: the steps for which its log held a counted line (`counted`) and the steps of which it made a clean use (`clean`: the run was verified, its log held `step_ok` for the step and no counted line). After a compile: whether it made its second commit (`second`). Every other step empties it |
| `Load` | `load` and `load_steps` of a replay run (L4.2, L3.3), in one step |
| `Propose` | a replay `put_playbook`: every proposal of one or two locators for a step, with the same or another value of the further key |
| `Finish` | `finish` of a run that was not lost: it withdraws the edits of a lower compile number, makes the use moves of the served edits, then keeps or drops each candidate (L3.4, L4.2) |
| `Compile` | a compile `put_playbook` that stores the second number, with its second commit or stopped before it |
| `Restart`, `FinishLost` | a restart of the host, and the `finish` of a run whose facts were lost |

A step has 2 compiled locators, 3 further locators and one further key, which stands
for every key of a step but `target.desc` and `target.locators`. `ListMax` 2 and
`EditMax` 3 stand for `LOCATORS_MAX` and `EDITS_MAX`.

`AdmitCause` answers the free refusal first, which stands for the causes `mode`,
`class`, `value` and `length`, and then the five computed causes in the order of L3.2:
`base`, `scope`, `locator`, `rejected`, `full`. The cause `base` holds for a step
number that the playbook lacks and for a run whose compile number is no longer the
newest.

The log of a run is free. At `finish` the model takes every combination of verified or
not and, for each step, of a set of the four lines `step_ok` (`ok`), a counted
`step_failed` (`counted`), a `step_failed` that does not count (`other`) and `recovered`.
So a counted line and `step_ok` stand in one log, and the counted line decides (L3.5).
A set has no order: `recovered` stands for a `recovered` line with no `step_failed` line
of the step after it. For a step on which the run was served no edit the model takes
no line but `recovered`, and it takes `recovered` only for a step with a candidate: the
rule makes no move for the other sets.

Readings that a builder should know:

- A proposal may name one locator twice. `New` keeps the first, so that a list holds no
  locator twice. With `Fault = "repeat_kept"` the repeat stays and TLC reports Q7.
- An edit carries the further key of the step that was admitted, and `load_steps`
  serves it. The row of L1.3 has no place for such a key. So in the model Q3 holds
  through the cause `scope` alone.
- After a stop between a compile's two commits the runs in flight keep their facts or
  lose them; the model takes both.
- A run whose facts were lost changes nothing that is stored.
- A served edit that another run's `finish` made `active` before this run's `finish`
  takes the row of an `active` edit: a counted line of this run does not revert it.
- The model has no `learn_run` row, no mode but replay, and no agent without a stored
  compile number.

## The bounds of `Admit.cfg`

Two runs at a time, 5 runs in one behaviour (`RunsMax`), and the fourth and the fifth
run only while step 2 holds no edit and no candidate (`RunsOnBothSteps` 3, the
constraint `Bound`). One restart and one compile. TLC treats the further locators of a
step and the two slots as interchangeable (`Symmetry`).

A wider bound, four runs that may all use both steps (`RunsMax` 4 with
`RunsOnBothSteps` 4), was run once on 2026-11-11 for 90 minutes with eight workers. It
did not end. When it was stopped TLC had generated 114,263,154 states and found
5,941,859 distinct ones, 3,863,822 of them still on its queue, at a depth of 15, and
had reported no property. So that bound is not checked, and the bound of `Admit.cfg`
stands: the model does not show what needs a fourth run while step 2 is in use.

## The properties

`Admit.cfg` names 18 formulas for the 14 properties. Q2, Q4, Q6, Q7, Q11 and Q12 are
checked on every state. Q1, Q3, Q5, Q8, Q9, Q10, Q13 and Q14 are checked on every step,
and so are `Q7Step`, `Q11Step`, `Q12Step` and `Q12Move`, which hold the clauses of Q7,
Q11 and Q12 that speak of a step. A property of a step reads what the step did and what the environment
recorded: `runs[s]` for what `Load` handed a run, `last` for the log, the rows before
and after. None reads `InForce`, the operator with which `Load` chooses an edit.

| name | the sentence of L3.8, shortened | in the model |
|---|---|---|
| Q1 | a step is served with an edit only when that edit is `provisional` or `active` and of the newest compile number, and it is the edit in force by L3.3 | on a `Load` step: each edit handed to the run is stored, `provisional` or `active` and of the number `newest`, and what the run is handed for a step is `Due`: the `provisional` edit of that number, else its newest `active` one, else nothing |
| Q2 | every stored edit has at least one run in `runs`, and its first run was verified and recovered the edit's step | every row counts at least one run; the first row was written by a run that it counts and that was verified; `recovered` holds |
| Q3 | the served step equals the compiled step in every key but `target.desc` and `target.locators` | on a `Load` step: the further key of each step handed to the run is the compiled step's |
| Q4 | the edit that an edit names as `on` was `active` when the edit was stored and has the same `base` and `n` | `onWas` is `active`, and `on` names the same `base` and `n` |
| Q5 | the list of the edit in force on a step holds no locator that a `reverted` edit of the same agent, `base` and `n` brought, and no compiled locator | on a `Load` step: the learned locators handed to the run hold none that an edit with a `reverted` row brought (`Bars`) and no compiled one |
| Q6 | an edit reaches `active` only from `provisional` after K clean uses, and `reverted` only from `provisional` | read on the rows of each edit: an `active` row follows a `provisional` one, holds a `clean` of K, and K rows before it added a run; a `reverted` row follows a `provisional` one |
| Q7 | an edit's list holds 1 to `LOCATORS_MAX` locators and none twice, an edit is first stored only under the newest compile number as it stands at the commit that stores it, and a step has at most one `provisional` edit of the newest number | the lists and the count on every state; `Q7Step`: an edit that a step adds is of the number `newest` |
| Q8 | the `finish` of a run that was not lost, whose log holds a counted line for a step, moves the edit of that step that was served to the run to `reverted` when that edit is `provisional` and of the newest number at that `finish` | on a `Finish` step: for each step in `last.counted` the served edit, when it was `provisional` and of the number `newest` before the step, is `reverted` after it |
| Q9 | no step gives a use move to an edit below the newest compile number, and no step withdraws an edit of the newest number; the compile's second commit, and the next `finish` of a run that was not lost, withdraw every edit of a lower number | on every step: an edit below the newest number gains no row but a `withdrawn` one, an edit of the newest number gains no `withdrawn` row, and after a compile's second commit and after a `Finish` every edit below the newest number is `withdrawn` |
| Q10 | a `reverted` edit stays `reverted` until a recompile withdraws it, its `reverted` row rests, and the barred set of a step and the newest number never shrinks | on every step: no edit and no row is lost, a `reverted` edit gains no row but a `withdrawn` one, and `Bars` of the newest number holds after the step what it held before |
| Q11 | a stored list starts with a locator that its edit brought; a step has at most `EDITS_MAX` edits of a number; a candidate as `admit` took it holds no barred locator and no locator of the served list | the first two on every state; `Q11Step`: on a step that gives a run a candidate, the run's compile number is the newest (the row `base` of L3.2), and the candidate holds no locator of `Bars`, of the learned locators that the run was handed or of the compiled ones |
| Q12 | an edit reaches `active` after K clean uses by K different verified runs, each of which was served the edit; a `finish` whose log holds a clean use of a served `provisional` edit of the newest number, with `clean` at K - 1, writes an `active` row | on every state: the K rows that added a run before an edit's first `active` row were written by K different verified runs, each new to `runs`; `Q12Step`: a row that adds a run is the only row that the step adds to its edit, its step is in `last.clean`, and the run that finishes was served the edit and has the row's number; `Q12Move`: on a `Finish` step, for each step in `last.clean` the served edit, when it was `provisional` and of the number `newest` before the step, has a new last row that counts the run and one more clean use, and that row is `active` when its `clean` is K and `provisional` otherwise |
| Q13 | a run's use moves go only to the edits that it was served, a run is served only edits of the compile number that it loaded, and the `finish` of a lost run changes nothing | on every step: a row that is not `withdrawn` is added to an edit only by the `Finish` of a run that was served the edit; a `Load` gives the run the number `newest` and edits of that number alone; a `FinishLost` leaves `edits` as it is; and after a restart no run in flight keeps its facts |
| Q14 | a revert follows only a counted line of its own step | on every step: a `reverted` row is added to an edit only when the edit's step is in `last.counted`, which only the `Finish` of a run that was not lost fills, from that run's log |

Two formulas ask more than the words of their sentence. Q1 also fails when a run is
handed no edit for a step on which one is in force. `Q12Move` asks for a row at every
clean use of a served `provisional` edit, not only at the one that reaches K, so that
a model which never counts a clean use is reported too. `Q11Step` holds the clause of
the row `base` of L3.2: a candidate is taken only for a run whose compile number is
the newest.

## The wrong designs

`Fault` is `"none"` in `Admit.cfg`. Each other value puts one wrong design in place of
the rule. The table gives what TLC reports when it checks such a copy with every
property of `Admit.cfg` and one worker: the first property that the search meets. A
design may break others too. In the column of the bound, `3 runs` is `RunsMax` 3 with
`RunsOnBothSteps` 2, `4 runs` is `RunsMax` 4 with `RunsOnBothSteps` 2, and `one step` is
`RunsOnBothSteps` 0; each is a part of the bound of `Admit.cfg`. The outputs are kept in
the learning run's ledger, under `evidence/formal-verification-3/`.

| `Fault` | from | what it changes | TLC reports | distinct states | bound | output |
|---|---|---|---|---|---|---|
| `serve_reverted` | L12, the first control copy; review `load_serves_reverted` | `load_steps` may serve a `reverted` edit | Q1 | 1,980 | 3 runs | `runs/w-serve_reverted.txt` |
| `serve_lower` | review `load_serves_lower` | `load_steps` also reads edits of a lower compile number | Q1 | 603 | 3 runs | `runs/w-serve_lower.txt` |
| `stale_top` | verifier b1 | `load_steps` chooses the edit under compile number 1, whatever the newest is | Q1 | 603 | 3 runs | `runs/w-stale_top.txt` |
| `revert_compiled` | review revert-2 | after a revert `load_steps` serves the compiled step, not the `active` edit that the reverted one was built on | Q1 | 88,829 | 5 runs, one step | `runs/w-revert_compiled.txt` |
| `keep_unverified` | first round | a candidate of a run that is not verified is kept | Q2 | 32 | 3 runs | `runs/w-keep_unverified.txt` |
| `keep_unrecovered` | first round | a candidate is kept without a `recovered` line | Q2 | 32 | 3 runs | `runs/w-keep_unrecovered.txt` |
| `no_scope` | first round | `admit` does not answer `scope` | Q3 | 362 | 3 runs | `runs/w-no_scope.txt` |
| `base_any_state` | first round | the base of a new edit is the newest edit of the step in any state | Q4 | 1,964 | 3 runs | `runs/w-base_any_state.txt` |
| `no_bar_at_finish` | first round | `admit` bars a locator, the check at `finish` does not | Q5 | 6,889 | 5 runs, one step | `runs/w-no_bar_at_finish.txt` |
| `revert_active` | first round | `active` with `use_fail_counted` answers `reverted` | Q6 | 13,327 | 5 runs, one step | `runs/w-revert_active.txt` |
| `double_use` | review uses-1 | one run makes two `use_ok` moves on the edit that it was served | Q6 | 599 | 3 runs | `runs/w-double_use.txt` |
| `no_pending` | first round | a candidate is kept while the step has a `provisional` edit | Q7 | 821 | 3 runs | `runs/w-no_pending.txt` |
| `repeat_kept` | first round | a locator that a proposal names twice is new twice | Q7 | 45 | 3 runs | `runs/w-repeat_kept.txt` |
| `no_cut` | first round | a list is not cut to `ListMax` | Q7 | 9,907 | 3 runs | `runs/w-no_cut.txt` |
| `stale_base` | first round | a candidate is kept after the compile number changed | Q7Step | 155 | 3 runs | `runs/w-stale_base.txt` |
| `counted_ignored` | first round, gap 1 | a counted failure makes no move | Q8 | 597 | 3 runs | `runs/w-counted_ignored.txt` |
| `counted_verified` | review revert-3 | a counted failure reverts only in a verified run | Q8 | 597 | 3 runs | `runs/w-counted_verified.txt` |
| `ok_first` | review uses-4 | `step_ok` in a verified run wins over a counted line of the same log | Q8 | 600 | 3 runs | `runs/w-ok_first.txt` |
| `revert_gone` | review revert-1 | a revert writes a row that is not a `reverted` row, so nothing is barred | Q8 | 597 | 3 runs | `runs/w-revert_gone.txt` |
| `withdraw_last` | first round, gap 2 | `finish` withdraws the edits of a lower number after its use moves | Q9 | 1,993 | 3 runs | `runs/w-withdraw_last.txt` |
| `no_finish_withdraw` | review restart-2 | `finish` withdraws nothing | Q9 | 1,070 | 3 runs | `runs/w-no_finish_withdraw.txt` |
| `withdraw_below_loaded` | review restart-2, verifier b9 | `finish` withdraws only below the number that its run loaded | Q9 | 1,070 | 3 runs | `runs/w-withdraw_below_loaded.txt` |
| `withdraw_newest_too` | review restart-1 | `finish` also withdraws the edits of the newest number | Q9 | 281 | 3 runs | `runs/w-withdraw_newest_too.txt` |
| `second_commit_provisional_only` | review restart-4 | the compile's second commit withdraws only `provisional` edits | Q9 | 1,981 | 3 runs | `runs/w-second_commit_provisional_only.txt` |
| `reverted_returns` | second round | a `reverted` edit with `use_ok` answers `provisional` | Q10 | 5,124 | 3 runs | `runs/w-reverted_returns.txt` |
| `base_first` | review admit-2 | `build_list` puts the base edit's list first | Q11 | 9,858 | 3 runs | `runs/w-base_first.txt` |
| `cut_front` | review admit-3 | `build_list` cuts from the front | Q11 | 10,850 | 3 runs | `runs/w-cut_front.txt` |
| `left_ignores_base` | review B4 | `finish` does not take away the locators that the base edit lists | Q11 | 17,280 | 5 runs, one step | `runs/w-left_ignores_base.txt` |
| `no_full` | review B4, the edit limit | neither `admit` nor the check at `finish` reads the edit limit | Q11 | 13,784 | 3 runs, `EditMax` 2 | `runs/w-no_full.txt` |
| `compiled_new` | first round | a compiled locator counts as new | Q11Step | 7 | 3 runs | `runs/w-compiled_new.txt` |
| `no_bar_admit` | review admit-1 | `admit` never answers `rejected`; the check at `finish` stays | Q11Step | 3,193 | 3 runs | `runs/w-no_bar_admit.txt` |
| `held_compiled_only` | review admit-4 | `new_locators` is taken against the compiled locators alone | Q11Step | 593 | 3 runs | `runs/w-held_compiled_only.txt` |
| `admit_stale` | LM-9, the model as it stood | `admit` takes a candidate of a run whose compile number is no longer the newest | Q11Step | 52 | 3 runs | `runs/w-admit_stale.txt` |
| `no_bar` | L12, the second control copy | neither `admit` nor the check at `finish` bars a locator that a `reverted` edit brought | Q11Step | 3,193 | 3 runs | `runs/w-no_bar.txt` |
| `bar_newest_revert` | verifier b5b | the rule's barred set holds what the newest `reverted` edit brought and no more | Q11Step | 26,343 | 4 runs | `runs/w-bar_newest_revert.txt` |
| `ok_unverified` | review uses-3, verifier b6 | `use_ok` for a run that is not verified | Q12Step | 598 | 3 runs | `runs/w-ok_unverified.txt` |
| `other_as_ok` | verifier b6b | `use_ok` for a verified run whose log holds a failure that does not count and no `step_ok` | Q12Step | 598 | 3 runs | `runs/w-other_as_ok.txt` |
| `never_active` | reader `d12b_never_active` | the clean use that reaches K writes no row, so no edit becomes `active` | Q12Move | 5,143 | 3 runs | `runs/w-never_active.txt` |
| `use_in_force` | review uses-2, verifier b10 | the use move goes to the edit in force at `finish`, not to the edit that was served | Q13 | 739 | 3 runs | `runs/w-use_in_force.txt` |
| `lost_finish_moves` | review restart-3 | the `finish` of a lost run makes the use moves | Q13 | 1,099 | 3 runs | `runs/w-lost_finish_moves.txt` |
| `lost_finish_withdraws` | review restart-3 | the `finish` of a lost run withdraws the edits of a lower number | Q13 | 1,073 | 3 runs | `runs/w-lost_finish_withdraws.txt` |
| `keep_across_restart` | review restart-3 | a run keeps its facts and its candidates across a restart | Q13 | 11 | 3 runs | `runs/w-keep_across_restart.txt` |
| `counted_of_either_step` | reader `d13b` | a counted line of one step also counts against the edit that was served on the other step | Q14 | 1,892 | 3 runs | `runs/w-counted_of_either_step.txt` |
| `never_keep` | reader `dk_never_keep` | `finish` never keeps a candidate, so nothing is learned | nothing | 2,395 | 3 runs | `runs/w-never_keep.txt` |

| run | TLC reports | distinct states | bound | output |
|---|---|---|---|---|
| `serve_reverted` with Q1 alone, the first control copy | Q1 | 2,231 | `Admit.cfg` | `runs/ctl-serve_reverted.txt` |
| `no_bar` with Q5 alone, the second control copy | Q5 | 45,615 | `Admit.cfg` | `runs/ctl-no_bar.txt` |
| `double_use` with Q12Step alone | Q12Step | 599 | 3 runs | `runs/w-only-q12step-double_use.txt` |
| `never_keep` with `NotStopLeavesEdits` of `Reach.tla`: not reached | nothing | 2,395 | 3 runs | `runs/r-never_keep-StopLeavesEdits.txt` |

`serve_reverted` and `no_bar` are the control copies of L12. `serve_reverted`,
`serve_lower`, `stale_top` and `revert_compiled` change what `Load` hands a run, and no
property reads the operator that they change. `no_bar` breaks Q11 where the candidate is
taken and Q5 at the next load; the search meets Q11Step first, so the control copy is
checked with Q5 alone. `double_use` breaks Q6, whose rows now count runs, and Q12.
`no_bar_at_finish`, `revert_active`, `revert_compiled` and `left_ignores_base` need
four or five runs on one step; their bound is `RunsMax` 5 with `RunsOnBothSteps` 0,
which is a part of the bound of `Admit.cfg`. Within the 5 runs of `Admit.cfg` no step
gets a fourth edit, so `no_full` is run with `EditMax` 2. `bar_newest_revert` needs
four runs: two reverts on one step and a proposal after them.

`never_keep` breaks no property of `Admit.cfg`. A model that never learns stores
nothing, and every property speaks of what is stored or served. The reach runs see it.

## What the model does not show

- `Load` is one step. The model cannot show a compile that lands between a run's `load`
  and its `load_steps`, or between two pages of `load_steps` (L3.3). A case of
  `reverted_not_served` in L3.9 holds it in the tests of the learning code.
- A design that refuses more than the rule. `Propose` may refuse any proposal with the
  free refusal, and `Finish` may leave out any `recovered` line. So a rule that bars
  too much, or that drops a candidate that the rule keeps, reaches no state that the
  model does not reach, and no property can report it.
- A third compile number. The model cannot show the compile `put_playbook` that
  withdraws edits which a stop left, which Q9's sentence names beside the `finish`.
- A new edit whose first row takes over the `clean` of its base (the review's
  uses-5). Such an edit stays `provisional` for good. The traces report the form of it
  in `edit_step`; the bounds of `Admit.cfg` do not reach the form of it in the handler.
- The order of the lines of a log, and a `step_failed` line after a `recovered` line.
- A second agent, the mode of a run, the causes `mode`, `class`, `value` and `length`,
  and a request that arrives twice.

## Running TLC

Run TLC on a copy, so that it writes its state files outside the repository.

```sh
d=$(mktemp -d)
cp formal/r5-admit/*.tla formal/r5-admit/Admit.cfg "$d"
(cd "$d" && java -Xmx4g -XX:+UseParallelGC -cp "$TLA2TOOLS_JAR" tlc2.TLC -workers 8 -metadir meta -config Admit.cfg Admit)
```

TLC prints `Model checking completed. No error has been found.` and the number of
distinct states. A property that fails is reported by its name: `Invariant Q7 is
violated` for one that is checked on states, `Action property Q9 is violated` for one
that is checked on steps.

For a wrong design, change the line `Fault = "none"` in the copy of `Admit.cfg` and run
with `-workers 1`. For a control copy, also put the one line `PROPERTY Q1` (with
`serve_reverted`) or `PROPERTY Q5` (with `no_bar`) in place of the `PROPERTIES` block.

The check has two parts: the run above, in which TLC must report none of Q1 to Q14,
and the reach runs of the next section, in each of which TLC must report its line.

When TLC reports a property of `Admit.cfg`, replay its trace against the code and the
contract, step by step. A step that the code cannot make is an error of the model: fix
the model. When every step can happen, the code or the contract breaks the property:
report it with the trace, and do not weaken the property. After a change of the model
run `export.sh` again; the two files are never edited by hand.

## The reach runs

The properties of `Admit.cfg` speak of what is stored and served. A model that stores
nothing holds them all: with `Fault = "never_keep"` TLC reports none of them. So the
check also runs `Reach.tla`. It holds 25 states and steps that the system must reach,
each with its negation, and one run per name must report that negation: `Invariant
Not<Name> is violated` or `Action property Never<Name> is violated`. Its trace is a
behaviour that reaches the state or makes the step. A name that its run does not
report is a move that the model no longer makes, and the check fails.

```sh
reach() {
    sed -e "s/RunsMax = 5/RunsMax = $3/" -e "s/RunsOnBothSteps = 3/RunsOnBothSteps = $4/" \
        -e '/^PROPERTIES$/,$d' Admit.cfg >"$2.cfg"
    printf '%s %s\n' "$1" "$2" >>"$2.cfg"
    java -Xmx2g -XX:+UseParallelGC -cp "$TLA2TOOLS_JAR" tlc2.TLC -workers 1 \
        -metadir "meta-$2" -config "$2.cfg" Reach >"$2.out" 2>&1
    grep -q " $2 is violated" "$2.out" || echo "not reached: $2"
}
cd "$d"
for name in TwoRunsHoldCandidates CompileUnderCandidate StopLeavesEdits LoadBeforeWithdrawal \
    ProposalBeforeWithdrawal ProposalHoldsCompiled ProposalHoldsBarred EditsOnBothSteps \
    StaleProposalRefused CauseBase CauseScope CauseLocator CauseRejected; do
    reach INVARIANT "Not$name" 3 2
done
for name in RestartDropsCandidate FinishWithdrawsLeftEdits LostFinishBeforeWithdrawal \
    UnverifiedRunReverts LowerServedNoMove; do
    reach PROPERTY "Never$name" 3 2
done
for name in ProposalHoldsListed CandidateListedByBase ListCut EditLimit ActiveInForceAgain \
    CauseFull; do
    reach INVARIANT "Not$name" 5 0
done
reach PROPERTY NeverCountedOnActive 5 0
```

`$d` is the copy of the run above. The table gives each name with its line, its bound
and the distinct states that TLC had found when it reported the line, with one worker.

| name | line of the configuration | bound | what is reached | distinct states |
|---|---|---|---|---|
| `TwoRunsHoldCandidates` | `INVARIANT NotTwoRunsHoldCandidates` | 3 runs | two runs at a time, each with a candidate | 59 |
| `CompileUnderCandidate` | `INVARIANT NotCompileUnderCandidate` | 3 runs | the second compile number while a run of the first holds a candidate | 33 |
| `RestartDropsCandidate` | `PROPERTY NeverRestartDropsCandidate` | 3 runs | a restart while a run holds a candidate | 32 |
| `StopLeavesEdits` | `INVARIANT NotStopLeavesEdits` | 3 runs | a stop between a compile's two commits leaves an edit of the lower number that is not `withdrawn` | 154 |
| `FinishWithdrawsLeftEdits` | `PROPERTY NeverFinishWithdrawsLeftEdits` | 3 runs | a `finish` withdraws the edits that a stop left | 1,071 |
| `LoadBeforeWithdrawal` | `INVARIANT NotLoadBeforeWithdrawal` | 3 runs | a load under the newest number before that withdrawal | 603 |
| `ProposalBeforeWithdrawal` | `INVARIANT NotProposalBeforeWithdrawal` | 3 runs | a candidate of such a run before that withdrawal | 2,005 |
| `LostFinishBeforeWithdrawal` | `PROPERTY NeverLostFinishBeforeWithdrawal` | 3 runs | the `finish` of a lost run before that withdrawal, which leaves the edits as they are | 1,073 |
| `ProposalHoldsCompiled` | `INVARIANT NotProposalHoldsCompiled` | 3 runs | a proposal with a compiled locator, admitted without it | 2 |
| `ProposalHoldsBarred` | `INVARIANT NotProposalHoldsBarred` | 3 runs | a proposal with a barred locator, refused with `rejected` | 1,053 |
| `ProposalHoldsListed` | `INVARIANT NotProposalHoldsListed` | 5 runs, one step | a proposal with a locator that the base edit lists, admitted without it | 5,077 |
| `CandidateListedByBase` | `INVARIANT NotCandidateListedByBase` | 5 runs, one step | a candidate that the base edit lists whole by `finish` | 6,858 |
| `ListCut` | `INVARIANT NotListCut` | 5 runs, one step | a list cut at the limit, which loses a locator of its base | 4,945 |
| `EditLimit` | `INVARIANT NotEditLimit` | 5 runs, one step | a step with `EditMax` edits of one compile number | 13,707 |
| `ActiveInForceAgain` | `INVARIANT NotActiveInForceAgain` | 5 runs, one step | a `reverted` edit whose `active` base is in force again | 35,799 |
| `EditsOnBothSteps` | `INVARIANT NotEditsOnBothSteps` | 3 runs | an edit on each of the two steps | 146 |
| `UnverifiedRunReverts` | `PROPERTY NeverUnverifiedRunReverts` | 3 runs | a run that is not verified reverts a `provisional` edit | 597 |
| `CountedOnActive` | `PROPERTY NeverCountedOnActive` | 5 runs, one step | a counted line of a run whose served edit is `active` at its `finish`; the edit gains no row | 13,327 |
| `StaleProposalRefused` | `INVARIANT NotStaleProposalRefused` | 3 runs | a proposal of a run whose compile number is no longer the newest, refused with `base` | 12 |
| `LowerServedNoMove` | `PROPERTY NeverLowerServedNoMove` | 3 runs | a counted line for a served edit of a lower number, which is withdrawn and not reverted | 1,993 |
| `CauseBase` | `INVARIANT NotCauseBase` | 3 runs | `admit` answers `base` | 2 |
| `CauseScope` | `INVARIANT NotCauseScope` | 3 runs | `admit` answers `scope` | 2 |
| `CauseLocator` | `INVARIANT NotCauseLocator` | 3 runs | `admit` answers `locator` | 2 |
| `CauseRejected` | `INVARIANT NotCauseRejected` | 3 runs | `admit` answers `rejected` | 1,053 |
| `CauseFull` | `INVARIANT NotCauseFull` | 5 runs, one step | `admit` answers `full` | 19,430 |

With `Fault = "never_keep"` the run of `NotStopLeavesEdits` ends with no error, at
2,395 distinct states: the model that never learns is seen by this run and by no
property of `Admit.cfg`.

## `edit-table.tsv`

One line per row of L3.7, `<state>\t<move>\t<guard>\t<answer>\t<clean>`, sorted in byte
order.

| column | holds |
|---|---|
| `state` | the state that `edit_step` is called with: `none`, `candidate`, `provisional`, `active`, `reverted`, `withdrawn` or `refused` |
| `move` | the move: `propose_ok`, `propose_refused`, `keep`, `drop`, `use_ok`, `use_fail_counted`, `use_fail_other` or `recompile` |
| `guard` | `clean_below`: the row for a `clean` of 0. `clean_reaches`: the row for a `clean` of 1. `-`: the row for every `clean` from 0 to K |
| `answer` | the next state, or `edit` when `edit_step` refuses the pair and leaves state and `clean` as they are |
| `clean` | the next `clean`. A count where the row fixes it: 3 rows. `same` where the answer keeps the `clean` that was passed in: 11 rows. `-` where the answer is `edit`: 43 rows |

There are 57 rows: one for each of the 7 states and 8 moves, and a second one for
`provisional` with `use_ok`. To compare `edit_step` with a row, call it at each `clean`
that the guard allows. A row with a count must answer that count, a row with `same` the
`clean` that was passed, a row with `-` the answer `edit`.

The file has no row for `provisional` with `use_ok` and a `clean` of K, which answers
`edit`. No trace reaches that call either. So a form of `edit_step` that answers a
state there (the review's `clean_k_active`) is shown by no file of this directory; a
test of `edit_step` in the learning code holds it (L12).

## `traces.jsonl`

The first line is `{"seed":<n>,"count":<n>}`. Every later line is one trace,
`{"moves":[{"move":"<move>","guard":"<guard>","answer":"<state or edit>","clean":<count or null>},...]}`,
of 6 to 16 moves. `clean` is the next `clean`, and null where the answer is `edit`.

To replay a trace, start with the state `none` and a `clean` of 0, and for each element
of `moves` in order:

1. Call `edit_step(state, move, clean)`.
2. When `answer` is `edit`, the call must answer `edit`. Keep the state and `clean`.
3. Otherwise the call must answer the next state `answer` and the next `clean` of the
   element. Take both for the next move.
4. `guard` is `clean_below` when the state is `provisional`, the move is `use_ok` and
   `clean` is 0; `clean_reaches` when `clean` is 1; and `-` on every other move.

A wrong `clean` in the code fails the row or the move that answers it. Five of the six
wrong forms of `edit_step` that the review made fail in this way: a `clean` that rises
on a clean use of an `active` edit, one that falls to 0 on a counted failure of an
`active` edit, one that falls to 0 on a failure that does not count, one that rises on
a revert, and one that falls on a withdrawal. The sixth is the call that no file holds
(see above).

The first 57 traces each aim at one row of the table, in the order of the lists
`States` and `Moves`: such a trace opens with the moves that lead to the row's state
(`Paths`) and then makes the row's move; the 57th aims at `provisional` with `use_ok`
and a `clean` of 1. Each later trace opens with one entry of the list `Openings`, which
the generator chooses. After its opening a trace goes on with free moves to its length:
half of them are drawn from all 8 moves and half from the moves that the state does not
refuse. So the traces meet each of the 57 rows whatever the seed, and `export.sh` fails
when one is not met. In the committed file 199 of the 200 traces hold an `edit` answer.
The numbers come from a generator that the
module computes with whole numbers (`Lcg`), and not from TLC's or Java's random numbers.
The export has run on macOS alone; nobody has compared its bytes with a run on Windows.

## `export.sh`

```sh
TLA2TOOLS_JAR=/path/to/tla2tools.jar sh formal/r5-admit/export.sh --out DIR
```

It writes both files into DIR, or into this directory with no argument, with the seed
and the count of the first line of `traces.jsonl` in this directory, or 1 and 200 when
that file is not there. A file that is there and whose first line has another form, an
empty file among them, is refused with `usage`. To draw other traces, change the first
line and run it again. A second run with the same first line writes the same bytes.
`sh formal/r5-admit/export.sh --help` states the refusals `usage`, `java` and `jar`. A
refusal writes no file and leaves none: where the script fetched the jar, it removes
its temporary directory before it refuses.
