# capture pipeline architecture

decision: 2026-11-09T07:51:36Z
status: accepted
decided by: planner-1, run 20261108-halvo-r1-capture
evidence: row 10 of decisions.tsv of run 20261108-halvo-r1-capture

## Context

R1 captures activity on macOS and Windows from one codebase. Each signal (the
screen, keys, the pointer, the foreground app and its window title, audio and the
camera) has its own native interface on each system, and a capture process must
never defeat secure input.

## Decision

An event-driven native stack with one source for each signal: ScreenCaptureKit and
windows-capture for frames, cpal for audio, a listen-only CGEventTap and Raw Input
for keys and the pointer, NSWorkspace and SetWinEventHook for the foreground app and
the title, and nokhwa for the camera, on the objc2 and windows crates. The capture
code is one workspace member, halvo-capture, with the code of each system behind
cfg, and one daemon is the sole writer of what capture records.

## Consequences

- What a default run stores is the content floor of ADR 0008, which narrows this
  stack: microphone, camera and system audio stay off in R1 (row 13), and the record
  has no constructor for those kinds.
- ADR 0006 records who opens the store that the daemon writes, and ADR 0007 how the
  daemon starts.
- At 685d846 an event holds an id, the time, a sequence number, the event type, the
  foreground app and a byte count (crates/halvo-capture/src/event.rs:89-101, with
  the byte count in the segment reference at :75-79). It holds no key code, pointer
  position, pixel or title text, and main at 685d846 persists none of it. Two
  sentences of the commit message of 685d846 are wrong: "now reach the store",
  because nothing drains a sink into a store at that commit, and "a CI lint bans
  low-level keyboard hooks", because that lint is the gate sub-check `no-ll-hook`
  and no workflow ran it at that commit. The message stays as it is.
