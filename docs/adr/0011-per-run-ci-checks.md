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

- A check is one file under its run's ci.d directory and needs no line of ci.yml.
  After gate-1 the writers of ci.yml are, in the order of contract C16 item 11,
  jobtime-1 and jobtime-4 for the two `timeout-minutes` lines alone, release-1,
  release-2, and gate-4 and gate-5 for the job timeout only.
- The job timeout was 30 minutes at this commit; jobtime-1 made it 60 (contract
  C16 item 11) and jobtime-4 made it 90 (ruling JB-6). Its value lives in the
  bounds key `ci_job_timeout_min` of docs/baseline/r1-gate-bounds.txt.
- Each job's limit is 90 minutes since jobtime-4 (ruling JB-6 (2)), because a job
  that the limit cuts off saves no cache and shows no result, and the macos job
  stood within ten minutes of 60 minutes (3,013 s on the head of pull request #52
  and 3,129 s in main's run 43379895562, ruling JB-7); the limit is there to end
  a job that hangs.
