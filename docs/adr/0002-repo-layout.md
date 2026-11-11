# repo layout

decision: 2026-11-09T05:27:56Z
status: accepted
decided by: planner-1, run 20261108-halvo-kickoff
evidence: docs/research/researcher-8.md

## Context

One repository must build both platforms. The product will later have a backend for
accounts, billing and model-usage metering, and the zero-data-retention constraint
says that backend holds nothing captured on the end user's machine.

## Decision

This repository is one Tauri app in a Cargo workspace: `src-tauri/` is the Rust
crate, `ui/` is the static web view page, and the root `Cargo.toml` is the workspace.
The backend lives in a separate repository.

## Consequences

- A single-app Cargo workspace keeps one codebase building both platforms.
- Capture code and account, billing and metering code can never share a module,
  because they never share a repository. The zero-data-retention boundary is
  structural, not a convention (lane 9: the backend holds no capture data).
- Precedent: Bitwarden keeps clients and server in separate repositories (lane 8
  rows 9, 10). Same-repo alternatives were considered and rejected (rows 3, 6).
- Shared types between app and backend, if any appear, go through a published
  schema rather than a shared crate.

## Amendments

- 2026-11-11: The Decision describes the workspace at 08de1f1. The workspace gained
  crates/halvo-capture in 3771ece, gains crates/halvo-sqlcipher with store-1 and
  crates/halvo-control with control-1, and lists its members by a glob from gate-1
  on. ADR 0006 records the layout. The decision that the backend lives in a separate
  repository stands.
