# R7 spikes S5 (keychain half) and S6 (helper half)

Scratch branch, never merged. The rules below were written before the first measured run.
Every value in the jobs is synthetic.

## Rules, as the plan states them

### S5: credential custody across builds, keychain half

One build stores an item through security-framework (macOS) or DPAPI (Windows). A rebuilt
build, ad hoc re-signed on macOS, reads it with user interaction disabled. The runners are the
hosted macos-15 and windows-2025.

- The rule is met when the read succeeds on both runners.
- A miss prints MISSED and does not fail the job.
- A miss means the device credential does not survive an update of an unsigned build. The
  custody crate then keeps an unsigned build's credential in a file only the user can read, and
  the keychain path waits for the owner's signing. A failed read is reported as a locked
  credential and is never answered by creating a second account. D7 returns to the planner with
  P1's recovery answer.

Two checks decide whether a leg's line counts:

- The build that stored the item reads it back with user interaction disabled. If this fails,
  the key store itself is unusable on the runner and the leg says nothing about a rebuild.
- The two builds are different binaries (different SHA-256, and on macOS a different code
  directory hash). An update always changes the binary, so a byte-identical rebuild would not
  test an update.

### S6: helper start, helper half

A second binary of the scratch crate reads an item that it stored itself in an untimed first
run, makes one HTTPS request with reqwest and rustls (OS trust store) to a loopback stub whose
test certificate authority only the runner trusts, and exits.

- Each runner prints the wall time and peak RSS of 30 runs and `ok=30/30`.
- A run is ok only when it exits 0 after reading its item and the stub's own record holds its
  request.
- The line reads MET when all 30 runs are ok and the worst wall time is at or under 1 second.
- A control run made before the runner trusts the certificate authority must fail, or the line
  does not count.

## Rule lines

S5 RULE macos-15: PENDING
S5 RULE windows-2025: PENDING
S5 RULE both runners: PENDING
S6 RULE macos-15: PENDING
S6 RULE windows-2025: PENDING

## Measured values

Not measured yet.

## What each result decides

Not measured yet.

## How to reproduce

Not measured yet.
