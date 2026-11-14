# scripts/ci

`run-ci-d.sh` runs the checks of CI. A check is one script under `scripts/<run>/ci.d/`. A unit that adds a check adds a file there and edits no line of `.github/workflows/ci.yml`.

## Commands

Start each command at the repo root.

| Command | What it does |
|---|---|
| `sh scripts/ci/run-ci-d.sh <job>` | Runs every script whose `# jobs:` line names `<job>`. A job is `macos` or `windows`. |
| `sh scripts/ci/run-ci-d.sh <job> --self-test` | Runs the driver's own cases and the self-test of each of those scripts, several at a time (see Self-tests at the same time). |
| `sh scripts/ci/run-ci-d.sh <job> [--self-test] NAME...` | Runs only the named scripts. A NAME is the file name without `.sh`. With `--self-test`, the name `run-ci-d` stands for the driver's own cases. |
| `sh scripts/ci/run-ci-d.sh --list [<job>]` | Prints the path of each script in the order of the run. |
| `sh scripts/ci/run-ci-d.sh --rules` | Prints one line for each script: `<NN-name> (<jobs>): <rule>`. |

Each job of `ci.yml` has the steps `ci.d self-test` and `ci.d`, which run the first two commands.

## A script

- Its path is `scripts/<run>/ci.d/NN-name.sh`: two digits, then a name of `a-z`, `0-9` and `-`. The directory of R1 is `scripts/r1/ci.d/`. No two scripts have the same `NN-name`.
- Its first ten lines hold one line `# jobs: <job>[ <job>...]` and one line `# rule: <one sentence>`. The rule is the sentence that `--rules` prints. They may also hold the line `# self-test: alone`, which makes its self-test run alone.
- The driver starts it in the repo root as `sh <file>` or `sh <file> --self-test`, with no standard input.
- The driver sets `CI_D_REPO` (the repo root), `CI_D_JOB` (the job), `CI_D_ARTIFACTS` (the directory `target/ci-artifacts/<NN-name>/`, which the driver makes), `LC_ALL=C`, and `PATH` with the cargo bin directory first. Every other variable of the caller passes through.
- The last line that it prints on standard output and its exit status give the result:

  | Last line | Exit status |
  |---|---|
  | `PASS <NN-name>` | 0 |
  | `FAIL <NN-name> (<cause>)` | 1 |
  | `SKIP <NN-name> (<reason>)` | 77 |

  For any other status, or a last line that does not match the status, the driver prints `FAIL <NN-name> (exit <n>)`. In CI, where `CI=true`, a SKIP is a failure: a script installs the tool that it needs or fails.
- With `--self-test` it runs its own check on fixtures that it carries. At least one fixture must fail. Its last line is `PASS self-test:<NN-name> (<k> failing fixtures rejected)` with exit status 0, or `FAIL self-test:<NN-name> (<cause>)` with exit status 1. The driver fails a script whose `k` is 0.
- It writes only under a directory from `mktemp -d`, which it leaves in place, and under `CI_D_ARTIFACTS`. It changes no tracked file and builds with `--locked`.
- It prints counts, sizes, times and booleans only, because the logs of CI are public.
- It does not depend on another script having run.

## Limits that a script meets

- The windows job checks out the repository with depth 1, and the macos job checks it out with its whole history. A script that names the job `windows` reads no history: `git log` shows one commit, and a merge base or a diff against another commit fails there.
- The case `standin-strict-run-alone` of `check-cost.sh` and `check-memory.sh` counts the processes named `halvo-capture` on the whole machine with `pgrep`. It fails when another self-test of the R1 gate runs beside it, because that self-test starts its own stand-in under the same name. Run the gate's self-tests one at a time on one machine.

## Self-tests at the same time

With `--self-test` the driver runs up to `CI_D_SELF_TEST_JOBS` self-tests at a time, its own cases among them. Without the variable it runs as many as the machine has processors (`getconf _NPROCESSORS_ONLN`, else `NUMBER_OF_PROCESSORS`, else 1). The step `ci.d` always runs one script at a time, because the checks of cost and memory measure there.

- Each self-test writes its standard output and its standard error to files of their own. A self-test has ended when its script has exited and every process that holds its standard output has closed it, as in a run one at a time; its standard error does not hold the end. When a self-test and every self-test before it in the order of the run have ended, the driver prints its standard output, then its standard error as far as it is written, then its `seconds` line. So the log holds each script's lines together and in the order of the names, whatever the number at a time. What a process of a self-test writes on standard error after that print, the driver prints before the recap, under the line `standard error of <NN-name> after it was printed:`.
- Each self-test gets a `TMPDIR` of its own, a new directory under the driver's temporary directory. The driver leaves it in place.
- A self-test runs alone when its first ten lines hold `# self-test: alone`, or when its name is the first word of a line of `scripts/ci/self-test-alone.txt`. The driver then waits until every other self-test has ended, runs it, and starts no other self-test until it has ended. A line of the list is the name, a blank and the reason; blanks before the name and a carriage return at the end do not count. The list holds the scripts that fail beside another and that the run which owns the list may not change; the owner of such a script moves it to its own header. The driver fails the step when the list names no script.
- A self-test that runs longer than `CI_D_SCRIPT_LIMIT_S` seconds (default 1200) fails with `FAIL self-test:<NN-name> (time limit <n> s)`. The driver sends TERM to the script's process, and KILL 5 seconds later; a process that the script started and that outlives it is not ended.
- A self-test whose job in the driver ends without an exit status fails with `FAIL self-test:<NN-name> (it ended without an exit status)`.
- The driver starts each self-test through `perl`, which gives it a process group of its own and sets INT and QUIT to their defaults before it runs `sh <file> --self-test`. A background job of a shell starts with INT and QUIT ignored, and no shell can undo that, so without this start a self-test could not show what its script does on an interrupt. A self-test therefore meets INT, QUIT, HUP and TERM as the same script does in the foreground on that system, several at a time and alone; on the windows runner the sh of Git Bash runs on after a QUIT that it sends itself, also in the foreground, so a self-test there does too. The step fails when `perl` does not run.
- When INT or TERM reaches the driver in this step, it sends TERM to the process group of every self-test that runs, KILL 5 seconds later to a group that is still there, prints `FAIL run-ci-d (the step was interrupted by <signal>; every self-test that it started is ended)` and exits with 130 or 143. A process that a self-test started into a process group of its own is not ended.

What a self-test keeps to, so that it can run beside the others: it writes only under its `TMPDIR` and its `CI_D_ARTIFACTS`, depends on no other script's self-test, binds no fixed port or fixed path, and counts no process of the whole machine. A self-test that cannot keep to this holds `# self-test: alone`.

## What the driver prints

1. The output of each script while it runs, then `seconds <NN-name> <n>`. With `--self-test`, the output of each script when it has ended, in the order of the names, then its `seconds` line.
2. One line for each script: the script's PASS, FAIL or SKIP line, or the driver's own FAIL line for it.
3. `ci.d <job>: <p> passed, <f> failed, <s> skipped`. With `--self-test` the line is `ci.d <job> self-test: <p> passed, <f> failed`.
4. The output of the lockfile guard, `git diff --exit-code HEAD -- Cargo.lock package-lock.json`, when a lockfile differs from HEAD, staged or not. The guard also compares the hash of each lockfile in the work tree with its blob in HEAD, so an index entry marked assume-unchanged hides no change.

## Exit status

| Status | Meaning |
|---|---|
| 0 | Every script that ran passed or skipped, at least one passed, and the lockfile guard passed. |
| 1 | A script failed, every script skipped, no script names the job, a NAME matches no script, or the lockfile guard failed. With `--self-test` also: `CI_D_SELF_TEST_JOBS` or `CI_D_SCRIPT_LIMIT_S` is set to anything but a whole number from 1 to 999999999, or `scripts/ci/self-test-alone.txt` cannot be read or names no script, or `perl` does not run. |
| 130, 143 | With `--self-test`: INT or TERM reached the driver, which ended every self-test that it had started. |
| 64 | Bad usage. |

The driver refuses an entry of a `ci.d` directory, prints `FAIL <name> (<cause>)` for it and counts it as failed in every job, when the entry is not a regular file (a directory or a symbolic link, also a dangling one), when its file name is not `NN-name.sh` (no `.sh` suffix, an upper-case suffix or a hidden name included), when its first ten lines do not hold exactly one `# jobs:` line and one `# rule:` line, when the `# jobs:` line names anything but `macos` and `windows`, or when another script has the same name.

With no script for the job the driver prints `FAIL run-ci-d (no scripts for <job>)` and exits 1, also with `--self-test`. The one exception is `--self-test run-ci-d`, which runs the driver's own cases and needs no script.

## The scripts root

The driver lists every entry of `scripts/*/ci.d/`, hidden entries included, in byte order of the path, so `scripts/r1/ci.d/` runs before `scripts/r2/ci.d/`, and it runs each entry that is a regular file named `NN-name.sh`. When `CI_D_SCRIPTS` is set, it lists `$CI_D_SCRIPTS/*/ci.d/` instead. This command shows that an empty set fails:

```sh
CI_D_SCRIPTS="$(mktemp -d)" sh scripts/ci/run-ci-d.sh macos
```

## Artifacts

A script saves what a reader of a failed run needs under `CI_D_ARTIFACTS`. The last step of each job uploads `target/ci-artifacts/` as the artifact `ci-artifacts-<job>`, also when the job failed.
