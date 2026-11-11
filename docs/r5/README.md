# docs/r5: the learning run

The learning run teaches Halvo's agents where a step finds its element when
a page changes. A repair is kept only after the run that made it is
verified, and it is reverted when it fails within two clean uses. This
directory holds the run's fixed numbers and the trials that chose them,
measured before any learning code exists.

- `README.md`: this file.
- `.gitattributes`: the line `* text eol=lf`, so that git checks out every
  file here with LF line ends and the hashes in `freeze.toml` hold in every
  clone.
- `cases.md`: the form of a case file of the drift trial, how the reader in
  `tools/r5-trials/drift` reads it, and a worked example, which
  `cargo test` in `tools/r5-trials` reads.
- `freeze.toml`: freeze point 1, the SHA-256 of `targets.toml`, of the
  development cases, of the `stat` and `drift` trial directories and of
  the held-out archive; a later change to a file it covers voids the trial
  lines measured before it.
- `heldout.txt`: the 24 held-out cases by count and archive hash, the tree
  of `tools/r5-trials/drift` that accepted them, and the six spread lines
  of `sh tools/r5-trials/drift/run.sh --count DIR`. The cases stay outside
  the repository until the learning code is frozen.
- `targets.toml`: the constants of the rule and the routine marks and the
  thresholds of the gate and of memory; the three trial programs read it.
- `trials/`: one result file per trial, a date line and a machine line,
  then the output of the trial's command in a fenced block.
  - `admission.md`: `sh tools/r5-trials/drift/run.sh` with no argument,
    four lines per development case; its verdict is `rule=admit`.
  - `routine.md`: `sh tools/r5-trials/routine/run.sh`, whose test in
    `tools/r5-trials/routine` reads it.
  - `stat.md`: `sh tools/r5-trials/stat/run.sh`, the critical win counts
    of the gate's sign test and its verdict.
- `plan-revisions.md`: where this run departs from `docs/plan.md`.
