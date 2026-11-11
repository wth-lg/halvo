# app framework and language

decision: 2026-11-09T05:27:56Z
status: accepted
decided by: planner-1, run 20261108-halvo-kickoff
evidence: docs/research/researcher-8.md

## Context

Halvo ships one codebase to macOS and Windows. The app needs native access to every
screen, input and audio capture API that the lane 4 research lists, must install
with no sudo, and must build unsigned until a signing identity exists. The license
must leave the repository free to become MIT later.

## Decision

Tauri 2 with a Rust core and a minimal system web view UI.

## Consequences

- Tauri 2 is the only framework in the lane 8 research with a shipping 24/7 screen
  and audio recorder on both platforms (screenpipe, lane 8 rows 4, 5, 51, 53).
- Rust reaches every capture API in lane 4 without a binding layer.
- rustup installs the toolchain as the current user with no sudo (rows 28, 44).
- Tauri is Apache-2.0 and MIT, and `tauri build` produces unsigned bundles by default.
- Risk: Cap left Tauri for GPUI in October 2026 over lost clicks in web-view overlays
  and display-picker crashes (lane 8 summary, docs/research/researcher-8.md:6, and
  option 1, :90). GPUI has no bundler or CI tooling (option 3, :102),
  so it is not an option today; the risk is recorded and revisited if the capture
  work in R1 hits the same problems.

## Amendments

- 2026-11-11: The risk line under Consequences cites the lane 8 summary
  (docs/research/researcher-8.md:6) and option 1 (:90) for Cap's reasons and
  option 3 (:102) for the GPUI tooling, in place of the three lane 8 rows that it
  first cited, which do not state those facts.
