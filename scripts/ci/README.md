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
- The driver starts it in the repo root as `sh <file>` or `sh <file> --self-test`, with no standard input, through `perl`, which gives it a process group of its own and sets INT and QUIT to their defaults. A background job of a shell starts with INT and QUIT ignored, and no shell can undo that, so without this start a script could not show what it does on an interrupt. A script therefore meets INT, QUIT, HUP and TERM as it does in the foreground on that system, in both steps, several at a time and alone; on the windows runner the sh of Git Bash runs on after a QUIT that it sends itself, also in the foreground, so a script there does too. The driver fails with `FAIL run-ci-d (perl does not run; ...)` when `perl` does not run.
- A script that runs longer than `CI_D_SCRIPT_LIMIT_S` seconds (default 1200) fails with `FAIL <NN-name> (time limit <n> s)` in the step `ci.d` and with `FAIL self-test:<NN-name> (time limit <n> s)` in the step `ci.d self-test`. The driver sends TERM to the script's process group, and KILL to the group 5 seconds later when a process of the group is still there.
- The driver sets `CI_D_REPO` (the repo root), `CI_D_JOB` (the job), `CI_D_ARTIFACTS` (the directory `target/ci-artifacts/<NN-name>/`, which the driver makes), `LC_ALL=C`, and `PATH` with the cargo bin directory first. Every other variable of the caller passes through.
- The last line that it prints on standard output and its exit status give the result:

  | Last line | Exit status |
  |---|---|
  | `PASS <NN-name>` | 0 |
  | `FAIL <NN-name> (<cause>)` | 1 |
  | `SKIP <NN-name> (<reason>)` | 77 |

  For any other status, or a last line that does not match the status, the driver prints `FAIL <NN-name> (exit <n>)`. In CI, where `CI=true`, a SKIP is a failure: a script installs the tool that it needs or fails.
- With `--self-test` it runs its own check on fixtures that it carries. At least one fixture must fail. Its last line is `PASS self-test:<NN-name> (<k> failing fixtures rejected)` with exit status 0, or `FAIL self-test:<NN-name> (<cause>)` with exit status 1. The driver fails a script whose `k` is 0.
- The driver gives it a `TMPDIR` of its own in both steps, a new empty directory, and removes that directory after the script, whatever its status. When the directory cannot be removed, the script fails with `FAIL <NN-name> (its TMPDIR cannot be removed)`, or `FAIL self-test:<NN-name> (...)` in the step `ci.d self-test`. When the driver exits, it removes its own temporary directory.
- It writes only under a directory from `mktemp -d` with a template under `$TMPDIR` and under `CI_D_ARTIFACTS`. A script that makes a temporary directory anywhere else removes it by its own trap on EXIT, INT, TERM and HUP. It changes no tracked file and builds with `--locked`.
- The driver holds the rule on tracked files. It reads `git status --porcelain --untracked-files=no`, the entries of `git ls-files -v` that are marked assume-unchanged or skip-worktree, and the commit of HEAD before the first script, after each script of the step `ci.d`, and after the step `ci.d self-test` as a whole, whose self-tests run at the same time. Each entry that a script or the step changed fails the run with `FAIL run-ci-d (<NN-name> changed a tracked file: <path>)`, `FAIL run-ci-d (the step ci.d self-test changed a tracked file: <path>)` or `FAIL run-ci-d (<NN-name> moved HEAD)`. A tracked file that already differs from HEAD before the first script fails the run with `FAIL run-ci-d (a tracked file differs from HEAD before the first script: <path>)`, and no script is named for it.
- It prints counts, sizes, times and booleans only, because the logs of CI are public.
- It does not depend on another script having run.

## Limits that a script meets

- The windows job checks out the repository with depth 1, and the macos job checks it out with its whole history. A script that names the job `windows` reads no history: `git log` shows one commit, and a merge base or a diff against another commit fails there.
- The case `standin-strict-run-alone` of `check-cost.sh` and `check-memory.sh` counts the processes named `halvo-capture` on the whole machine with `pgrep`. It fails when another self-test of the R1 gate runs beside it, because that self-test starts its own stand-in under the same name. Run the gate's self-tests one at a time on one machine.
- A native Windows program reads `TEMP` and `TMP`, not `TMPDIR`, so what it writes there stays outside the `TMPDIR` that the driver removes. A signal of the driver does not reach a native Windows program, such as `cargo.exe`, that a script started; such a program can run on after the time limit and after an interrupt, and can hold the lock of the build directory.
- `target/ci-artifacts/12-test/tests.txt` holds the Running and Doc-tests lines and the test result lines of 12-test's `cargo test` run. It is no proof by itself that 12-test ran in the same driver run or passed: an earlier run can have left it, and a test's child can print lines of the same form. A script runs no crate's tests that 12-test runs, unless it reads that file first and runs them only when the file is missing, or its jobs line names windows alone (contract C16, point 18).

## Self-tests at the same time

With `--self-test` the driver runs up to `CI_D_SELF_TEST_JOBS` self-tests at a time, its own cases among them. Without the variable it runs as many as the machine has processors (`getconf _NPROCESSORS_ONLN`, else `NUMBER_OF_PROCESSORS`, else 1). The step `ci.d` always runs one script at a time, because the checks of cost and memory measure there.

- Each self-test writes its standard output and its standard error to files of their own. A self-test has ended when its script has exited and every process that holds its standard output has closed it, as in a run one at a time; its standard error does not hold the end. When a self-test and every self-test before it in the order of the run have ended, the driver prints its standard output, then its standard error as far as it is written, then its `seconds` line. So the log holds each script's lines together and in the order of the run, the byte order of the path (see The scripts root), whatever the number at a time, when the job reads both streams through one pipe; the workflow runs the driver without `2>&1`, and a runner that reads the two streams through two pipes can put a script's standard error after the next script's lines. What a process of a self-test writes on standard error after that print, the driver prints before the recap, under the line `standard error of <NN-name> after it was printed:`.
- A self-test runs alone when its first ten lines hold `# self-test: alone`, or when its name is the first word of a line of `scripts/ci/self-test-alone.txt`. The driver then waits until every other self-test has ended, runs it, and starts no other self-test until it has ended. A line of the list is the name, a blank and the reason; blanks before the name and a carriage return at the end do not count. A blank line, and a line whose first word starts with `#`, is a comment. The list holds the scripts that fail beside another and that the run which owns the list may not change; the owner of such a script moves it to its own header. The driver fails the step with `FAIL run-ci-d (scripts/ci/self-test-alone.txt names <name> without a reason)` for a line that holds a name and no reason, with `FAIL run-ci-d (scripts/ci/self-test-alone.txt names <name>, which is no script)` for a line whose name is no script of any job, and with `FAIL run-ci-d (scripts/ci/self-test-alone.txt cannot be read)` when the list is there and is no file that it can read.
- A self-test whose job in the driver ends without an exit status fails with `FAIL self-test:<NN-name> (it ended without an exit status)`.

## A signal to the driver

When INT, TERM or HUP reaches the driver, in either step, it sends TERM to the process group of every script that runs, KILL 5 seconds later to a group that is still there, prints `FAIL run-ci-d (the step was interrupted by <signal>; every self-test that it started is ended)`, or `every script` in the step `ci.d`, and exits with 130, 143 or 129. A process that a script started into a process group of its own is not ended.

What a self-test keeps to, so that it can run beside the others: it writes only under its `TMPDIR` and its `CI_D_ARTIFACTS`, depends on no other script's self-test, binds no fixed port or fixed path, counts no process of the whole machine, installs no tool with a package manager that another self-test also installs (two `brew install` runs at the same time clashed on one bottle directory in scratch run 43269049946), and leaves no process that holds its standard output or standard error. A self-test that cannot keep to this holds `# self-test: alone`.

## What the driver prints

1. The output of each script while it runs, then `seconds <NN-name> <n>`, the script's time in whole seconds. With `--self-test`, the output of each script when it has ended, in the order of the run, then its `seconds` line. The `seconds` lines of self-tests that ran at the same time overlap, so their sum is more than the time of the step.
2. One line for each script: the script's PASS, FAIL or SKIP line, or the driver's own FAIL line for it; and the driver's FAIL line for each tracked file that changed (see A script).
3. `ci.d <job>: <p> passed, <f> failed, <s> skipped`. With `--self-test` the line is `ci.d <job> self-test: <p> passed, <f> failed`.
4. The output of the lockfile guard, `git diff --exit-code HEAD -- Cargo.lock package-lock.json`, when a lockfile differs from HEAD, staged or not. The guard also compares the hash of each lockfile in the work tree with its blob in HEAD, so an index entry marked assume-unchanged hides no change.

## Exit status

| Status | Meaning |
|---|---|
| 0 | Every script that ran passed or skipped, at least one passed, and the lockfile guard passed. |
| 1 | A script failed, every script skipped, no script names the job, a NAME matches no script, a tracked file differs from HEAD (see A script), the lockfile guard failed, `CI_D_SCRIPT_LIMIT_S` is set to anything but a whole number from 1 to 999999999, or `perl` does not run. With `--self-test` also: `CI_D_SELF_TEST_JOBS` is set to anything but a whole number from 1 to 999999999, or `scripts/ci/self-test-alone.txt` cannot be read, or a line of it names no script or gives no reason. |
| 130, 143, 129 | INT, TERM or HUP reached the driver, which ended every script that it had started. |
| 64 | Bad usage. |

The driver refuses an entry of a `ci.d` directory, prints `FAIL <name> (<cause>)` for it and counts it as failed in every job, when the entry is not a regular file (a directory or a symbolic link, also a dangling one), when its file name is not `NN-name.sh` (no `.sh` suffix, an upper-case suffix or a hidden name included), when its first ten lines do not hold exactly one `# jobs:` line and one `# rule:` line, when the `# jobs:` line names anything but `macos` and `windows`, or when another script has the same name.

With no script for the job the driver prints `FAIL run-ci-d (no scripts for <job>)` and exits 1, also with `--self-test`. The one exception is `--self-test run-ci-d`, which runs the driver's own cases and needs no script.

## The scripts root

The driver lists every entry of `scripts/*/ci.d/` in byte order of the path, and the hidden entries after all others, so `scripts/r1/ci.d/` runs before `scripts/r2/ci.d/`, and it runs each entry that is a regular file named `NN-name.sh`. When `CI_D_SCRIPTS` is set, it lists `$CI_D_SCRIPTS/*/ci.d/` instead. This command shows that an empty set fails:

```sh
CI_D_SCRIPTS="$(mktemp -d)" sh scripts/ci/run-ci-d.sh macos
```

## Artifacts

A script saves what a reader of a failed run needs under `CI_D_ARTIFACTS`. The last step of each job uploads `target/ci-artifacts/` as the artifact `ci-artifacts-<job>`, also when the job failed.
