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
  After gate-1 the writers of ci.yml are jobtime-1 and jobtime-4, in the order
  of contract C16 item 11, jobtime-9 by ruling PR-31 item 1 and jobtime-11 by
  ruling JB-52 and citime-w1 by ruling WS-9, for the two `timeout-minutes` lines
  alone, release-1, release-2, and gate-4 and gate-5 for the job timeout only.
- The job timeout was 30 minutes at this commit; jobtime-1 made it 60 (contract
  C16 item 11), jobtime-4 made it 90 (ruling JB-6), jobtime-9 made it 120
  (ruling PR-31 item 1), jobtime-11 made it 150 (ruling JB-52) and citime-w1
  made it 180 (ruling WS-9). Its value lives in the bounds key
  `ci_job_timeout_min` of docs/baseline/r1-gate-bounds.txt.
- Each job's limit is 180 minutes since citime-w1 (ruling WS-9), because main's
  push run 44373816491 at 68c80599 read 8,342 s in its windows job of 9,000 s.
  It was 150 minutes from jobtime-11 (ruling JB-52), because the
  macos job stood within minutes of 7,200 s for five landings and one slow
  runner or one dropped and rerun job cost two hours each time. It was 120
  minutes from jobtime-9 (ruling PR-31 item 1), because a job that the limit
  cuts off saves no cache and shows no result, and the macos job was reckoned
  at about 5,340 s on a normal runner after landing 43 against the 5,400 s of
  90 minutes, with landing 41's slow attempt at about 5,700 to 5,780 s by
  projection, and the cuts in hand were not reckoned to bring it under 5,400 s.
  It was 120 and not 100 minutes because that slow attempt's 5,780 s with
  store-3's 430 s more read 6,210 s, over the 6,000 s of 100 minutes, and no
  more than 120 because the limit is there to end a job that hangs; those
  figures were reckoned, not measured.
