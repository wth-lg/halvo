# The R1 gate

`run.sh` is the driver: `sh scripts/r1-gate/run.sh --help` lists every sub-check
by its letter of the done-when. `lib.sh` holds the predicates, and each
`check-<name>.sh` holds the cases that prove them (`--self-test -v`) and the
measured run (`--measure`). The bounds stand in `docs/baseline/r1-gate-bounds.txt`.

## Mutant rows

`mutation-test.sh` discovers every tracked file named `mutants.rows` and runs
its rows. A row file holds rows for files under its own directory. The grammar
below is frozen: a change to it is a change to this file, to the runner and to
the contract that names it.

One row is one line of six fields, separated by one tab each. No field is
empty, and a line that is empty is skipped.

    name <tab> file <tab> substitution <tab> verdict <tab> state <tab> item

- **name**: one name in the whole repository. The only repeat allowed is a
  transitional row and the row that takes it over.
- **file**: the file to mutate, relative to the directory of the row file, with
  no `..` in it. For a `transitional` or a `control` row it is relative to the
  repository root.
- **substitution**: one or more perl `s///` statements, separated by `;`. The
  runner applies each to the whole file, so `\n` matches a line end. A
  statement that matches nothing fails the row, and so does a substitution
  that changes nothing.
- **verdict**: the one source that must show the mutant caught, from a closed
  list:
  - `measured KEY...`: a gate line reads `FAIL <check> (... measured: ...)` and
    names every KEY. The source scan names its rule, for example `active-tap`.
  - `refused TEXT`: the mutant built, and a gate line reads
    `FAIL <check> (verify-... unavailable: ...)` with TEXT in it.
  - `leak`: a gate line reads `FAIL <check> (leak: ...)`.
  - `test NAME`: exactly one test runs and fails inside a time limit; a hang
    fails the row. NAME is a cargo test of the crate that holds the row file,
    run by `--exact` on its library, or
    `selftest:<check>/<case>`, a case of a gate self-test. Unmutated, the same
    test must run as exactly one passing test.
- **state**:
  - `required`: the mutant must be caught: `PASS mutation:<name> (<verdict>: ...)`.
  - `required on measuring-mac`: as required; under `CI=true` it prints
    `NOT-RUN mutation:<name> (runs on the measuring Mac)`, which `--strict` fails.
  - `required on hosted-runner`, `required on test-bed`: a row whose verdict
    needs generated input. Elsewhere it prints
    `NOT-RUN mutation:<name> (needs input; runs on <place>)`, which is no FAIL.
  - `transitional`: a row of the gate's file on code that has no row file yet.
    It runs while no `mutants.rows` stands in the directory of its file. Once
    that file exists it must hold the name: the gate's copy then prints
    `MOVED mutation:<name> (<file>)`, and otherwise
    `FAIL mutation:<name> (transitional row not taken over by <file>)`.
  - `control`: the runner's own control. Its mutant must fail to build, and the
    runner must report it as not built.
  - `pending ITEM`: prints `OPEN mutation:<name> (ITEM)` and is not run.
  - `retired REASON`: the code is gone; REASON names the test that replaced the row.
- **item**: the id of the plan item that owns the row.

The runner builds every mutant before it judges it:
`FAIL mutation:<name> (the mutant does not build)` is never a caught mutant.
`mutants.expected` lists one name for each mutant that a review applied; under
`--strict` a name that no row file holds or retires is a FAIL.
