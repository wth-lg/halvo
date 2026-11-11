# content floor

decision: 2026-11-10T18:47:26Z
status: accepted
decided by: planner-13, run 20261108-halvo-r1-capture
evidence: row 108 of decisions.tsv of run 20261108-halvo-r1-capture; contract C13 of that run; docs/privacy-limits.md

## Context

ADR 0004 chose a source for each signal. This ADR records what a default run
stores from those sources, and what it never stores. RD-01, RD-02 and RD-04 are owner
asks; this ADR records the default that the units build to, and a different answer
is a dated line under Amendments.

## Decision

A default run stores:

- the title text of the foreground window, with a byte cap, and none while secure
  input is active;
- the foreground app;
- one key-class event for every key-down transition, a modifier included, alone or
  at the start of a chord, with no key-up and no autorepeat (rulings D10), and never
  a character or a key code;
- the pointer classes click, scroll, drag and move, with one coalesced move record
  for each interval and no coordinates.

A default run stores no screen record. One activity record for each changed screen
frame, with no pixel, is stored only while the switch `screen_activity` is on, and
its default is off. So the first-run text says `Screen activity is not recorded.`
The site read sits behind the switch `site_read`, default off, and its reader ships
only if spike S8 takes branch A.

The switches are `title_text`, `key_class`, `pointer_class`, `site_read` and
`screen_activity`. R1 has no switch for screen content and no empty hook for one;
the second layer brings its own switch with its unit (rulings D7 and B7).

Text lives only in the encrypted segment payload. Every stored text (the app id, the
title text, the site domain) is built by one constructor that takes the registered
filters, `StoredText::filtered`, which daemon-1 lands. R1 registers one filter and
nothing else, the byte cap: 1,024 bytes for the app id and the title, and 253 bytes
for the site domain, the longest host name in text form. The cap counts bytes of the
stored text and cuts at a character boundary. R1 builds no detector and depends on
no crate of another run. A later run registers its filter ahead of the byte cap
(ADR 0012).

Exclusions before capture: apps on the static list, and every process in the tree
of the exclusion root, which in the product is the Halvo app.

On Windows the keyboard source is out of the default source set in R1, so no
key-class event is stored there and the first-run text says
`Typing is not observed on Windows in this version.` win-3 lands the witness, the
three-state password-field detector and the zero-keystroke assertion. One Revision
of the plan switches the source in after a recorded run, typed by a person on a
Windows 11 PC, in which the detector gives 0 keystroke records while a password
field has the focus and records the keys typed outside it.

Installed software: both readers stay in the tree with no event kind (RD-02).

## Consequences

The limits, which docs/privacy-limits.md states with their sources:

- The title of a private window of Chrome, Edge, Firefox or Safari is not stored when
  the browser runs with an English interface, and the title of any other private
  window is stored (macos-4 and win-4).
- A terminal that does not assert secure input gets no protection, so key classes
  and the window title are recorded while a password is typed there (macos-4).
- Characters and key codes are never stored (core-2's types).
