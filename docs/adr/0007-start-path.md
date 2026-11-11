# start path

decision: 2026-11-10T03:14:36Z
status: accepted
decided by: eng-manager, run 20261108-halvo-r1-capture
evidence: row 81 of decisions.tsv of run 20261108-halvo-r1-capture; contract C14 of that run

## Context

The capture daemon needs a process that starts it. The person grants the capture
permissions to one program, and capture must stop when the app that shows the
capture indicator is gone.

## Decision

The app starts the capture daemon as its child and is the program the user grants.
A daemon that launchd starts is not the default path (row 81).

- On macOS the app is the login item com.halvo.desktop, which starts it with
  `--background` and has KeepAlive with SuccessfulExit false (RD-27). Scheme E10
  bans only a plist for the daemon (rulings D5).
- The daemon starts the permission request child by default (RD-29; rulings D2).
  The app's child is the other form. Both request types exist in halvo-control, and
  spike S1 decides between them.
- The daemon captures only while the app claims its indicator on the Status request.
  That claim is the lease. Its bounds key is `lease_bound_s`, with the first value
  10 s (three missed 3 s polls plus 1 s; RD-26, which row 84 adopted on
  2026-11-10).
- The daemon runs its one shutdown entry when the descriptor that it inherited from
  the app closes. The flag is `--lifeline` (RD-28; rulings D8).
- The Stop request stays in protocol v1. Its bounds key is `stop_deadline_s`, with
  the first value 20 s (RD-28 and RD-45, which row 84 adopted on 2026-11-10). The
  daemon's shutdown budget of 18,000 ms sits under that 20,000 ms stop wait
  (contract C14; rulings D3).
- On Windows the per-user Run value starts the app. There is no task and no
  elevation (RD-43).
- A daemon that launchd starts stays only as a branch for a signed release.

## Consequences

- gate-2 writes the two keys into docs/baseline/r1-gate-bounds.txt, and from then on
  that file holds the value in force.
- control-1 lands the login-item builder and, in the service module of
  halvo-control, the Run-value command line with its registry write, read and
  remove (RD-52; rulings D9). control-2, daemon-3, app-3 and win-5 land their
  parts, and app-4 calls them.
