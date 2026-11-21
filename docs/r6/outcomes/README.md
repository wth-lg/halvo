# Recorded outcome table

The four files beside this text are the recorded table of the routing work: `recorded.tsv`, `models.tsv`, `sources.tsv` and `freeze.txt`. `route-gate import` wrote them, and they were copied unchanged from its output directory. Nobody edits them by hand. When `docs/r3/trials/driver.md` or `docs/r3/trials/model-cells.md` changes, `35-r6-outcomes.sh` fails. To fix it, the same pull request runs `route-gate import` again into a new, empty directory and `route-gate derive` again into a file path that does not exist yet, and copies their five files over these four files and `crates/halvo-route/data/policy.tsv`.

## How the table was made

The import command, run from the repository root into a new, empty directory:

```text
cargo run -q --locked -p halvo-route-gate --bin route-gate -- import --driver docs/r3/trials/driver.md --cells docs/r3/trials/model-cells.md --date s1=2026-11-13 --date s2=2026-11-12 --out <new directory>
```

It printed `import rows=198 models=12 sources=2`. The input was the driver file with the blob 1aa10649bcccf8398e7341332fe398c6ff92afaa and the cells file with the blob c778001149588ef72427d20f68010f7c6338a611. This text describes the import of those two blobs alone: when `sources.tsv` names other blobs, the counts and dates below are out of date. The two dates were read from the text of the trial files. The next import reads its two dates anew, in the same way, from the text of the trial files that it imports. In the driver file, the last `end=` of its 28 `timed` lines is 2026-11-13T15:49:06Z. In the cells file, the last `end=` of its 12 `timed` lines is 2026-11-12T00:31:57Z. The second line of `freeze.txt` is the verdict line of the driver file, `verdict driver=none path=B writer=none`, as `on_device driver=none writer=none`.

`recorded.tsv` has 198 rows: three models (granite_4_2_3b, minicpm5_2b and ministral_3_3b) on two machines (`mac` and `hosted-win`) for three seeds, with 11 attempts each (5 for `bill`, 5 for `order` and 1 for `reply`). Five rows read `pass=yes`. All five are `write` attempts of ministral_3_3b. Every `compile`, `replay` and `recover` row reads `pass=no`. `models.tsv` has one memory bound for each of six models on each of the two machines.

The derive command, run from the repository root, wrote the policy into a file in a new, empty directory, because it writes no file that exists already. It prints no line:

```text
cargo run -q --locked -p halvo-route-gate --bin route-gate -- derive --table docs/r6/outcomes/recorded.tsv --freeze docs/r6/outcomes/freeze.txt --out <new directory>/policy.tsv
```

## The result: no model fills the on-device tier

No model drives and no model writes. So the derivation reads no row for the `compile`, `recover` and `write` cells, and each such cell of a category with a runtime reads `untested`; the cells of the five categories with no runtime read `no_runtime` and the three of `other` read `not_seed`, as in the first table. The rows stay in the table as evidence. They decide nothing and are counted in no line. `route-gate derive` on this table, with `--freeze docs/r6/outcomes/freeze.txt`, writes the 44 rows of the first table byte for byte. The router therefore hands every model cell back to the person. The gate's verdict on this table is `gate table=recorded router=product cells=44 routed=0 handed_back=30 derivation=same held=ok result=pass`. A pass with `routed=0` says nothing about a router, only that no cell went to a tier.

That verdict is the last line that this command prints, run from the repository root. It writes no file:

```text
cargo run -q --locked -p halvo-route-gate --bin route-gate -- score --table docs/r6/outcomes/recorded.tsv --router product --policy crates/halvo-route/data/policy.tsv --freeze docs/r6/outcomes/freeze.txt
```

## What the agent-setup run ruled, and what it left open

The ruling CP-179, Addition 3, of the agent-setup run is carried into this run (the coordinator's ruling SG-72, R2, way A). Its point (a): "By (4) every model has a miss of its own at the mended commit, so the block's line driver=none path=B stands and is no longer provisional as far as the three rules of this ruling go". So this table is a result, and its freeze line is no longer provisional as far as CP-179's three rules go.

Its point (b) did not decide whether a harness may end a compile at the right state by its own reading. ministral_3_3b's cell at the mended commit is a miss and stays one, and that question goes to a later trial. The readings at the mended commit entered no block, so they are not in this table.

The ruling CP-175 of the same run found that "the replay results of the order cells measured nothing; their compile results stand as measured". The 54 `replay` rows of `order` are in the table as printed. The derivation reads no `replay` row.

## Lines that a later rule would print otherwise

Under the agent-setup run's ruling CP-179, a body is the text after the model's thinking and has three words or more. Counted that way, three `mac` lines of the block would read otherwise: the `reply` lines of minicpm5_2b seed 1 and ministral_3_3b seed 1, and the `write` attempt line of the second, which the block prints with `pass=yes`. The block's lines stay as printed, and the importer takes them as printed. So `recorded.tsv` holds one row that the ruled count would not print: ministral_3_3b, `mac`, seed 1, `write`, `pass=yes`. With `writer=none` it decides nothing.

## Limits

- The importer does not check that a `write` attempt passes exactly when its `reply` line reads `bodies=2`. The agent-setup run's `tools/r3-trials/drive/verdict.py` checks it, and verdict.py may refuse more than the importer (the coordinator's ruling SG-72, R3, way B).
- The importer reads no raw file of the trial, so a value typed into the block by hand passes it.
- The rows come from three seeds and two machines, and the team wrote the fixtures. The memory bounds come from five runs a cell, and no machine is a 16 GB laptop. `sources.tsv` writes these limits as `3_seeds_2_machines_team_fixtures` and `5_runs_a_cell_no_16gb_laptop`.
