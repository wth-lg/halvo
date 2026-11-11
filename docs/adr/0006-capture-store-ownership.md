# capture-store ownership

decision: 2026-11-09T23:46:56Z
status: accepted
decided by: eng-manager, run 20261108-halvo-r1-capture
evidence: row 69 of decisions.tsv of run 20261108-halvo-r1-capture; row 69 gives the decision no label, so this heading is a proposal

## Context

Row 69 completed P6 of R1 with a redesign around one owner of the capture store:
the daemon owns the store, and the app is a client over a local control socket.

## Decision

The capture daemon owns the capture store and is the only process that opens it.
The app is a client over a local control channel and links no store code.

- The control crate, crates/halvo-control, holds what the app and the daemon share:
  the protocol, the data-directory layout, the service constants and the
  login-item builders. control-1 creates it.
- The workspace has four packages: halvo (src-tauri/, the app), halvo-capture
  (crates/halvo-capture/, the capture code and the daemon), halvo-control
  (crates/halvo-control/) and halvo-sqlcipher (crates/halvo-sqlcipher/, the shared
  SQLCipher build, which store-1 creates). It builds two executables: halvo, the
  app, and halvo-capture, the daemon.
- The store port is a trait in the capture crate. daemon-1 and daemon-2 build
  against it with a model store in their tests. daemon-3 builds the adapter over the
  encrypted store, which is the only code that opens a persistent store. It opens
  it from `halvo-capture --service-run`, from the offline delete and from the
  debug-only modes, each under the instance lock of the data directory.
- From app-2 on, the app links none of halvo-capture, halvo-sqlcipher, rusqlite or
  libsqlite3-sys, and a CI check fails when it does.

## Consequences

- One process holds the store key and writes the store, so no store has a second
  writer and the app needs no key.
- ADR 0002 keeps its backend decision; this ADR records the layout of the workspace.
- ADR 0007 records how the app starts the daemon.

## Amendments

- 2026-11-10: row 76 (2026-11-10T01:06:26Z) keeps the design and folds thirteen
  review findings and the additions into the units.
- 2026-11-10: row 79 (2026-11-10T01:52:01Z) lands the design as unit pull requests
  to main (store, control crate, daemon, app, macOS sources, Windows, gate and CI,
  packaging) in place of waves A and B on pull request #9, which closes as
  superseded when the daemon unit merges.
