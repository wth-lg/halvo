# CI and release tooling

decision: 2026-11-09T05:27:56Z
status: accepted
decided by: planner-1, run 20261108-halvo-kickoff
evidence: docs/research/researcher-8.md

## Context

The kickoff fixes the CI shape: one workflow file, two jobs written out with no
matrix, one on a macOS runner and one on a Windows runner, triggered on every push
and on pull requests. It also requires release notes from the first commit in Keep a
Changelog form, and a release command whose dry run prints the next version, the tag
and the changelog section it would write, and writes nothing.

## Decision

GitHub Actions with one workflow and two literal jobs, `runs-on: macos-15` and
`runs-on: windows-2025`. cargo-release for versioning, with Keep a Changelog for the
release notes.

## Consequences

- Two literal jobs match the run-card CI rule. The macos-15 and windows-2025 images
  ship Rust and Node (lane 8 rows 24, 66), and the jobs pin their own versions:
  Rust 1.99.1 in rust-toolchain.toml and Node 26 in .github/workflows/ci.yml.
- cargo-release is dry-run by default and acts only with `--execute`. It reads and
  bumps the version in `src-tauri/Cargo.toml` and promotes the `## [Unreleased]`
  section of `CHANGELOG.md` through `pre-release-replacements` (rows 26, 67, 68).
- The documented release command is a repo script, `sh scripts/release.sh --dry-run`,
  because cargo-release's own dry run prints the version bump and the diff of the
  changelog replacement but neither the tag name nor the body of the section it
  would write. The script runs cargo-release's dry run, takes the tag name from the
  settings cargo-release resolved, and prints the section from `CHANGELOG.md`.
- The prerequisites script installs cargo-release from a prebuilt binary so CI jobs
  stay short; it falls back to `cargo install` when no prebuilt binary matches.

## Amendments

- 2026-11-11: The first line under Consequences says that the images ship Rust
  and Node and that the jobs pin their own versions. Row 66 lists Cargo 1.98.0 and
  Node 22.23.2 for macos-15, which are not the versions that the jobs use.
