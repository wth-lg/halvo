# key custody by feature

decision: 2026-11-10T18:47:28Z
status: accepted
decided by: planner-4, run 20261108-halvo-r1-capture
evidence: row 110 of decisions.tsv of run 20261108-halvo-r1-capture; contract C20 of that run

## Context

Row 12 wrapped the store key with the Secure Enclave on macOS. Under a rule by build
profile, the installable macOS build, which is unsigned, would never record.

## Decision

The custody is chosen by the cargo feature `secure-enclave` of
crates/halvo-capture/Cargo.toml, never by the build profile.

- Without the feature (every unsigned build, so also the installable bundle): macOS
  keeps the root in the per-user root file of RD-24, which has owner-only
  permissions, and Windows uses DPAPI. The view and the README label the macOS form
  as the key custody of an unsigned build. On an unsigned macOS build the first-run
  text and the data view carry this sentence (recommended text, RD-13):
  `This is an unsigned build: the key that protects your data is kept in a file with owner-only permissions in your account.`
- With the feature (the signed release, the owner's, which uses the enclave-backed
  custody): the feature line stays declared as the selector, and R1 ships no Secure
  Enclave custody, because store-6 deletes the skeleton (RD-42; rulings D1). Such a
  build creates no store and reports `store=no_release_custody`. A store created
  under the root-file custody is opened for maintenance only: expiry, listing and
  delete-all run, and capture does not (contract C20 item 3).
- A typed RootDenied state has a working delete in every case.

## Consequences

- store-3 lands the root-file custody, store-6 removes the skeleton, and daemon-3
  lands the policy function with its first caller (rulings B7).
- This records a decision that is closed, not a default: the team decided it, and
  the owner's ask on signing is unchanged.
