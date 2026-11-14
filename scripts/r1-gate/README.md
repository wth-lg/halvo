# The R1 gate

`run.sh` is the driver: `sh scripts/r1-gate/run.sh --help` lists every sub-check
by its letter of the done-when. `lib.sh` holds the predicates, and each
`check-<name>.sh` holds the cases that prove them (`--self-test -v`) and the
measured run (`--measure`). The bounds stand in `docs/baseline/r1-gate-bounds.txt`.

## What a line reads

- The timed-run, secure-input, cost and memory lines each read the report of
  their own run. Each fails when a source did not start: a line
  `halvo-capture: source <name> did not start` on standard error, a
  `failed_source=` line, or no screen frame or foreground app.
- Under `--input operator` the timed-run, cost and memory lines fail when that
  report holds no keystroke or no pointer event, and the secure-input line fails
  when it holds no pointer event; the sentence says that the operator gave no
  input. Under `--input none` each of the four lines fails when the report holds
  a keystroke or a pointer count above 0, and gives both counts; with both counts
  0 it ends NO-INPUT. Under either input mode a report without one line
  `modality=keystroke count=<digits>` and one line `modality=pointer count=<digits>`
  fails the line, because it shows neither input nor its absence.
- A secure-input count above 0 is the number of key events that reached the tap
  while secure input was held and that the product suppressed. The line prints
  the keystroke count of the same report beside it.
- `run.sh` requires exactly one line `<WORD> <name>` from each check, with the
  word that its exit status gives.
- Every child that a check starts gets its duration plus `child_deadline_extra_s`
  seconds: each timed run, and the `--verify-store` and `--verify-deletion` runs,
  whose duration is 0. A child past its deadline is ended, and the line is FAIL.
  A TERM or a HUP to the check ends the child too.

## Mutant rows

`mutation-test.sh` reads every tracked file named `mutants.rows` of HEAD, from
git's own list of the tracked paths, and runs its rows. It refuses, with a
`FAIL mutation:rows (...)` line and exit 1 in every mode but `--self-test`, a tracked path named
`mutants.rows` that is a symbolic link, that the copy of HEAD lacks (as a file
that an export-ignore attribute keeps out of the archive), or whose path holds
a tab, a newline, a backslash or a quote. A file that git does not track is
outside HEAD, and a file named `Mutants.rows` is no rows file. A row file
holds rows for files under its own directory. The runner is run on macOS, on
the measuring Mac and in the macos job; its stop is not shown under Git Bash.
It works in a throwaway copy of HEAD under TMPDIR and removes that copy when
it ends, on every path but a KILL of the runner itself, after which the copy
and its build directory stay; `--keep-scratch` keeps it and prints
`scratch mutation: <path>`. The grammar
below is frozen: a change to it is a change to this file, to the runner and to
the contract that names it.

One row is one line of six fields, separated by one tab each. No field is
empty, and a line that is empty is skipped.

    name <tab> file <tab> substitution <tab> verdict <tab> state <tab> item

- **name**: one name in the whole repository, compared as text, byte for byte,
  in the rows files and in `mutants.expected`, a backslash too. The only
  repeat allowed is a transitional row of scripts/r1-gate/mutants.rows and the
  row that takes it over in the rows file beside its file. Before it applies any row,
  and under `--list` too, the runner refuses every other repeat, in one rows
  file or in several, and exits 1 with one line for each such name:
  `FAIL mutation:<name> (the name stands in <rows file> line <n> and in <rows file> line <n>)`.
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
  - `transitional`: a row of the gate's file on code that has no row file yet;
    a transitional row in any other rows file is refused in the run modes and
    listed by `--list`.
    It runs while no `mutants.rows` stands in the directory of its file. Once
    that file exists it must hold the name: the gate's copy then prints
    `MOVED mutation:<name> (<file>)`, and otherwise
    `FAIL mutation:<name> (transitional row not taken over by <file>)`.
  - `control`: the runner's own control. Its mutant must fail to build, and the
    runner must report it as not built.
  - `pending ITEM`: prints `OPEN mutation:<name> (ITEM)` and is not run.
  - `retired REASON`: the code is gone; REASON names the test that replaced the row.
- **item**: the id of the plan item that owns the row.

The runner builds every mutant before it judges it. Every path under crates/ or
src-tauri/, and Cargo.toml and Cargo.lock, gets the cargo build, whatever kind of
file it is: a data file or a `.pl` file there also gets the cargo build, so a row
on it can apply and build when no build read the file. Outside those paths a `.sh`
file gets `sh -n`, a `.c` file `cc -fsyntax-only`, a `.pl` file `perl -c` and a
`.swift` file `swiftc -parse`, and a row on a file of any other kind is refused.
`FAIL mutation:<name> (the mutant does not build)` is never a caught mutant.
`mutants.expected` lists one name for each mutant that a review applied and
the name of every row whose state is `required` or `required on <place>`; a
unit that adds such a row adds its name in the same change. Under `--strict` a
FAIL line comes for each name that no row file holds or retires, for each such
required row whose name the list lacks, with the row file named, and for a
missing `mutants.expected`. The count line of every run gives both numbers.
Under `--strict` a row that `--budget` left unrun is a FAIL line too,
`strict: the budget left the row unrun`; with `--only`, `--strict` reads no
state of a row that `--only` leaves out, and the name rules still read every
row.

### A stopped run

A test that a row runs can make a root that its own guard removes when its
case ends; a test ended in the middle of a case leaves that root behind. So
TERM, INT or HUP to the runner, to its process group or to every process
below it does not end the running test, gate check or cargo build of the copy:

- A cargo test runs with TERM, INT and HUP ignored, because a Rust test has
  no handler for them and would end before its guard. A gate self-test case
  runs with them at their defaults and ends its case by its own trap.
- The stop traps are the runner's first commands. In every mode but
  `--self-test`, which keeps the traps of lib.sh and exits with the signal's
  status and no STOPPED line, from the first signal on the runner starts no
  new step of the run (a row, a test, a build, a gate check, a copy, a read by
  awk, perl, cp, sort, tar or git; it still waits for the running test, gate
  check or cargo build of the copy as below, and it removes its scratch),
  prints exactly one STOPPED line, prints no FAIL, PASS, OPEN, NOT-RUN or count
  line for a step that the signal ended, and exits with 143 for TERM, 130 for
  INT and 129 for HUP. Exit 0, 1 and 2 never follow a signal that the runner
  got, and a later signal changes nothing. From the first signal on the runner
  ignores TERM, INT and HUP, and so does every process that it starts after it,
  which is only its own reads of the running step; a step that runs from before
  the signal has them ignored as below, but for a program that sets its own
  handler, as rustc does for INT.
- The STOPPED line has one of five forms. Before any row, during the copy of
  HEAD, the read of the rows files and the name check, the `--only`
  selection, and under `--list`:
  `STOPPED mutation (signal <name>; no row had started)`. In a row when no
  test runs and the running step ends within 60 s (the unmutated build, a gate
  check, a short step) and in the tail
  (the read of mutants.expected and the count line):
  `STOPPED mutation:<row> (signal <name>; no test was running)`. During a test:
  `STOPPED mutation:<row> (signal <name>; the running case was let end)`, after
  the runner sends the test's process group TERM and waits up to 60 s, a
  constant of the runner, for the test to end; or
  `... the running case did not end within 60 s and was killed`, and what that
  case made may be left. During a gate check or the cargo build of the copy
  that does not end within 60 s of the signal:
  `STOPPED mutation:<row> (signal <name>; the running check-<name> did not end within 60 s and was killed)`,
  or `... the running build of the copy did not end ...`, and what that step
  made may be left. A KILL of the runner itself leaves its scratch, the
  throwaway copy and its build directory, as well. A shielded step or test
  that was running then goes on with TERM, INT and HUP ignored; a step that
  the kill at the bound had already stopped stays stopped while its process
  group has another member and goes on when the group is orphaned; KILL to
  each of its processes ends them.
- A gate check and the cargo build of the copy, unmutated or of a mutant, run
  with TERM, INT and HUP ignored and with their standard input from /dev/null.
  On the signal the runner starts no next step and waits up to 60 s for the
  running step, then stops and kills it with every process below it and names
  it in the STOPPED line. The build of a mutant on any other file is not
  shielded. rustc sets its own handler for INT, so an INT ends a running
  rustc: when the runner got the same INT, as it does from its group, the run
  ends as a stop (`STOPPED mutation:<row> (signal INT; no test was running)`,
  exit 130); when it got none, the build reads as failed. A step whose exit
  status says that a signal ended it (129, 130, 137 or 143) is never read as an
  answer: with a signal of the
  runner's own within 2 s the run ends as a stop, and without one it ends with
  exit 2 and `FAIL step:<step> (ended by signal <name>; the runner got no signal)`.
  A cargo build that a signal ends while the runner got none reads as a mutant
  that does not build, because lib.sh's gate_build returns 1 whatever ended
  cargo: a FAIL, never a caught row.
- A signal that comes after a test or a shielded step has started and before
  the runner waits for it is acted on when that step ends by itself, with its
  own limit as its bound; a build has none. A runner that was started with a
  signal ignored, as a shell does for INT in a background job, cannot trap that
  signal.
- A test that the runner starts has its standard input from /dev/null and QUIT
  ignored. A row's test must not stop a child of its own by TERM, INT or HUP:
  the child of a shielded cargo test ignores them too. A gate check and the
  cargo build of the copy have their standard input from /dev/null too. A
  check that ends a child of its own by TERM, INT or HUP gets no such end: a
  verify child past its deadline ends by the check's KILL 5 s later.
- KILL cannot be caught. A caller that sends KILL some seconds after its TERM
  lets a case end only when the case ends within those seconds: give a run of
  rows a time limit above the time of its rows.

## What a run changes on a Mac

A measured run of a service check starts target/r1-gate/debug/halvo-capture, the one gate-built path, and macOS will list that path in Privacy & Security, once that binary asks for each grant, under Screen & System Audio Recording, Accessibility and Input Monitoring; the helper window reads nothing and adds no row, and each such check prints this as its line `note privacy rows: ...` before it starts the service.

## Limits

- The source scan of `check-no-ll-hook.sh` reads the text of the tracked files
  whose names end in `.rs`, in lower case, one line at a time. It does not see
  an alias of a banned name (`use ... as`), a path that is split over lines, code
  that `include!` or `#[path]` brings in from another file, or a file whose
  extension is not `.rs` in lower case.
- During the secure-input run the product withholds every key event by design,
  so a pointer count above 0 shows that a person was at the machine during the
  hold, not that a key was typed.
