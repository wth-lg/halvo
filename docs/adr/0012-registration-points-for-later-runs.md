# registration points for later runs

decision: 2026-11-10T18:47:30Z
status: accepted
decided by: planner-2, run 20261108-halvo-r1-capture
evidence: row 112 of decisions.tsv of run 20261108-halvo-r1-capture; contract C18 of that run

## Context

Later runs need code inside R1's crates. The coordinator ruled that such code is
written by that run's builders in new files after the daemon units merge.

## Decision

Code that a later run needs inside R1's crates is written by that run's builders in
new files, after daemon-3 merges. R1 lands the points and no body, apart from the
record seam and the entry of the run mark.

- Reserved directories: crates/halvo-capture/src/daemon/r2/ and
  crates/halvo-control/src/r2/. No R1 unit writes a file in either.
- The empty cargo feature `r2`, off in every R1 build and check. R1 adds no CI line
  for it. The seam of the run mark is built with it at unit gates, in a scratch copy
  with a stand-in module. The first unit of the workflow run that writes the module
  file under crates/halvo-capture/src/daemon/r2/ adds the CI line under its own
  scripts/r2/ci.d/. Under the feature a batch holds one per-kind entry more than the
  kinds that R1 stores, the store refuses more than 8 entries, and a run that stores
  more kinds raises that bound itself.
- The call sites, one line each under `#[cfg(feature = "r2")]` that compiles to
  nothing in R1, each landed by its unit. Among them is the text filter point: one
  line in `TextFilters::r1()` where a later run registers its filter ahead of the
  byte cap (ADR 0008).
- The record seam of the run mark: under the feature, a constructor and a decoder
  path for the reserved kind `run_mark`, and the one method of the receiver in
  sink.rs that gives a mark its number.
- The entry of such a record into the owner's batch: one method of the owner under
  the feature, which answers true or false. The mark's kind is among the per-kind
  entries of the port, and one adapter line names it toward the store.
- The shared APIs that R1 uses itself: the key-file helper by path, the fill function
  of the prefill helper, the scripted source of the daemon's tests and the registry
  of the data directory.

## Consequences

- control-1, control-2, store-3, daemon-1, daemon-2 and daemon-3 land the points
  that the next run asks for, and this ADR lands none of them.
- R1 depends on no crate of a later run.
