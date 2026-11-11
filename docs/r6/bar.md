# The published bar

Sign-in, a second factor and the confirmation of a payment go to the human at every tier, so no row measures them. Five seed categories have no runtime, and of the others only the compile and recover cells of bill and order and the write cell of reply have rows, because the other tasks call no model. The fixtures are the team's own and easier than live sites, which also put bot checks before an agent that no fixture has. A cell rests on 3 seeds and 2 machines, neither of them a 16 GB laptop. One on-device model decides every cell, the agent-setup run's, and rows of other models decide nothing. An attempt passes by the fixture's own state, while in the product a learned playbook checks itself by conditions that the model compiled, which are weaker. No hosted tier is recorded, so the owner's hypothesis about open weights against closed models is untested on every landlord task.

This page states the bar that a model tier must pass before the routing table may send a call to it, and what the count behind the bar can show. The table is `crates/halvo-route/data/policy.tsv`. `route-gate derive` writes it from the recorded rows; after the first table, which was written once by hand, no person edits it.

## Where the limits above come from

- The five seed categories with no runtime are `tenant_rent`, `lease_renewal_or_change`, `tenant_application_review`, `ride_order` and `daily_chart_check`. `CategoryId::has_runtime` in `crates/halvo-route/src/tier.rs` names the five that have one, and the policy hands every call of the other five back to the person.
- The three seeds and the two machines are the values that the reader of the outcome table accepts in a recorded row: the seeds 1, 2 and 3 and the machines `mac` and `hosted-win` (`Seed` and `Machine` in `crates/halvo-route-gate/src/table.rs`).
- The model that decides a cell is the `driver` of the freeze file for the kinds `compile` and `recover`, and its `writer` for the kind `write` (`Setup::fill` in `crates/halvo-route-gate/src/lib.rs`). Rows of every other model stay in the table and are counted in no line.
- No hosted row exists because the ruling of `crates/halvo-route/data/ruling.txt` is `ruling=none`, which allows the tier `on_device` alone (`Ruling::allowed` in `crates/halvo-route/src/ruling.rs`).

## The line

A tier meets the line for a cell on a set of rows when the set holds at least a least number of attempts and every one of them passed. The least number is LINE_DEV_MIN=4 for development rows and LINE_HELD_MIN=2 for held-out rows. Both are constants in `crates/halvo-route/src/line.rs`; `meets` there takes the least number as its argument, and the gate tool passes it one of the two. An attempt passes by the fixture's own state and never by a model's report: a compile is ok when the fixture's list equals the seed's end.

## The split

Rows of seed 1 and seed 2 are development rows, and rows of seed 3 are held-out rows (`Seed::split` in `crates/halvo-route-gate/src/table.rs`). The table is derived from development rows alone. The gate then requires that the held-out rows of every cell that the table routes to a tier meet the line with LINE_HELD_MIN. With 2 machines, a model cell has 4 development attempts (2 seeds on 2 machines) and 2 held-out attempts (1 seed on 2 machines), 6 in all.

## What the count can show

Each figure below is computed from the rate that it names; none is measured.

- A tier whose true pass rate is 0.60 clears a line of 6 attempts with no miss with a chance of 4.7 percent (0.6 to the power 6 is 0.0467). At 0.70 the chance is 11.8 percent (0.1176), and at 0.80 it is 26.2 percent (0.2621).
- A tier whose true pass rate is 0.95 misses that line with a chance of 26.5 percent (1 less 0.95 to the power 6 is 0.2649).
- Six passes of six give a one-sided 95 percent lower bound of 0.60 for the pass rate (0.05 to the power 1/6 is 0.607). Eighteen passes of eighteen give 0.84 (0.05 to the power 1/18 is 0.847).
- The attempts are not independent draws. The model samples greedily at temperature 0, so a second run of one seed on one machine repeats the first, and what varies is the seed and the machine. So the line is a floor for entering the table and no promise of a rate.
- `lower_pct(n)` in `crates/halvo-route/src/line.rs` gives the floor of 100 times 0.05 to the power 1/n, the bound for `n` attempts with no miss. For `n` from 1 to 36 it is 5 22 36 47 54 60 65 68 71 74 76 77 79 80 81 82 83 84 85 86 86 87 87 88 88 89 89 89 90 90 90 91 91 91 91 92. Above 36 it is 92, and for 0 it is 0. The unit test `lower_table` computes each value again in floating point. The gate prints it on the line of a cell that the router sends to a tier whose attempts all passed there, and 0 on every other cell line.

## The sources of the table

`docs/r6/outcomes/sources.tsv` names the two files that the outcome files are read from, each with the id of its bytes and its date. The row `s1` is the driver file of the agent-setup trial, `docs/r3/trials/driver.md`, which holds the attempts and is the source of every recorded row; its limits are written `3_seeds_2_machines_team_fixtures`. The row `s2` is the model cells file, `docs/r3/trials/model-cells.md`, which holds each model's memory bound on each machine and is the source of every row of `docs/r6/outcomes/models.tsv`; its limits are written `5_runs_a_cell_no_16gb_laptop`. When either file changes, its id no longer matches and the check `35-r6-outcomes.sh` fails until the rows are imported again. Before any row is imported, the table hands every model cell back to the person.

`docs/r6/hypothesis.md` records what published tables say about the owner's hypothesis, cell by cell.
