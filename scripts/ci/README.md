# scripts/ci

`run-ci-d.sh` runs the checks of CI. A check is one script under `scripts/<run>/ci.d/`. A unit that adds a check adds a file there and edits no line of `.github/workflows/ci.yml`.

## Commands

Start each command at the repo root.

| Command | What it does |
|---|---|
| `sh scripts/ci/run-ci-d.sh <job>` | Runs every script whose `# jobs:` line names `<job>`. A job is `macos` or `windows`. |
| `sh scripts/ci/run-ci-d.sh <job> --self-test` | Runs the driver's own cases, then the self-test of each of those scripts. |
| `sh scripts/ci/run-ci-d.sh <job> [--self-test] NAME...` | Runs only the named scripts. A NAME is the file name without `.sh`. With `--self-test`, the name `run-ci-d` stands for the driver's own cases. |
| `sh scripts/ci/run-ci-d.sh --list [<job>]` | Prints the path of each script in the order of the run. |
| `sh scripts/ci/run-ci-d.sh --rules` | Prints one line for each script: `<NN-name> (<jobs>): <rule>`. |

Each job of `ci.yml` has the steps `ci.d self-test` and `ci.d`, which run the first two commands.

## A script

- Its path is `scripts/<run>/ci.d/NN-name.sh`: two digits, then a name of `a-z`, `0-9` and `-`. The directory of R1 is `scripts/r1/ci.d/`. No two scripts have the same `NN-name`.
- Its first ten lines hold one line `# jobs: <job>[ <job>...]` and one line `# rule: <one sentence>`. The rule is the sentence that `--rules` prints.
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

## What the driver prints

1. The output of each script while it runs, then `seconds <NN-name> <n>`.
2. One line for each script: the script's PASS, FAIL or SKIP line, or the driver's own FAIL line for it.
3. `ci.d <job>: <p> passed, <f> failed, <s> skipped`. With `--self-test` the line is `ci.d <job> self-test: <p> passed, <f> failed`.
4. The output of the lockfile guard, `git diff --exit-code HEAD -- Cargo.lock package-lock.json`, when a lockfile differs from HEAD, staged or not. The guard also compares the hash of each lockfile in the work tree with its blob in HEAD, so an index entry marked assume-unchanged hides no change.

## Exit status

| Status | Meaning |
|---|---|
| 0 | Every script that ran passed or skipped, at least one passed, and the lockfile guard passed. |
| 1 | A script failed, every script skipped, no script names the job, a NAME matches no script, or the lockfile guard failed. |
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
