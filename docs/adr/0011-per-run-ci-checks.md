# per-run CI checks

decision: 2026-11-10T18:47:29Z
status: accepted
decided by: planner-2, run 20261108-halvo-r1-capture
evidence: row 111 of decisions.tsv of run 20261108-halvo-r1-capture; contract C16 of that run

## Context

Each unit of R1, and each later run, adds checks to CI. The coordinator ruled
structure over edits to shared files, and an earlier row keeps one workflow file.

## Decision

- One workflow file, .github/workflows/ci.yml, with exactly two jobs, `macos` and
  `windows`.
- Each job holds the driver call, the steps `ci.d self-test` and `ci.d`, beside the
  check steps that the two correction runs added. Format, Clippy and Test are ci.d
  scripts of gate-1.
- The driver `sh scripts/ci/run-ci-d.sh <job>` runs every `scripts/*/ci.d/*.sh`
  whose `# jobs:` line names the job, in byte order of the path. It refuses a script
  with no `# jobs:` line or no `# rule:` line, passes each script's lines through,
  prints one PASS, FAIL or SKIP line for each file and exits with the worst status.
- R1's directory is scripts/r1/ci.d/; another run uses its own. A script states its
  rule on its `# rule:` line, and `sh scripts/ci/run-ci-d.sh --rules` prints the
  table that AGENTS.md points to.
- The root manifest lists the workspace members by a glob. Whoever merges resolves
  the lock files again.
- gate-1 landed it.

## Consequences

- A unit that adds a check adds one file under its run's ci.d directory and edits no
  line of ci.yml.
- The job timeout is 30 minutes at this commit. Its value lives in the bounds key
  `ci_job_timeout_min` of docs/baseline/r1-gate-bounds.txt.
