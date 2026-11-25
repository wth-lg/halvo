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

- On a Mac, a title of Chrome or Edge is stored only where the browser runs with an
  English interface and the title ends with the browser's own name, ` - Google Chrome`
  or ` - Microsoft Edge`, in both also with ` - ` and a profile's name behind the
  browser's own name; the titles of their private and Guest windows, of Chrome's
  isolated windows and of a dialog end otherwise; for Edge the last letter, digit or
  closing square bracket before the name must not be a closing square bracket. On
  Windows, a title of Edge is stored only where it ends with Edge's name and the last
  letter, digit or closing bracket before the name is not a closing bracket, and a title
  of Chrome only where the name that Chrome gives the inside of the window ends with
  Chrome's own name, alone or followed by a profile's name,
  where the browser's own toolbar outside the page holds a button named Back,
  Forward or Reload, and where no button of the window carries the word Incognito. No
  title of Firefox, Safari or another browser that Halvo knows by name is stored, and
  the title of a private window of a browser that Halvo does not know as one is stored
  (macos-4, win-4, mac-title-ends and win-chrome-guest).
- A terminal that does not assert secure input gets no protection, so key classes
  and the window title are recorded while a password is typed there (macos-4).
- Characters and key codes are never stored (core-2's types).

## Amendments

- 2026-11-22: The third item of the Decision changes, by the coordinator's order at
  line 16453 and its ruling at line 16515 of its pending starts in run
  20261108-halvo-r1-capture, which turn ruling D10: the key-down of a modifier key (Shift, Caps Lock, Control, Option or
  Alt, Command or the Windows key, Fn) is no key-class event, whether the modifier
  is released alone or starts a chord, on both systems. A key pressed while one is
  held keeps its class as before: a shortcut while Command, Control, Option or Alt,
  or the Windows key is held, its own class while Shift alone is held. The reason:
  a Shift stored in front of a letter shows which letters of a password are
  capitals (unit lone-modifier-out).
- 2026-11-25: a key of the class printable that is pressed on a Mac with Option, or with Option and Shift, and neither Command nor Control, is of the class printable and no longer a shortcut. The reason: a shortcut in its place showed where in a password such a sign stands. No line reads which sign a key types. On Windows nothing changes: a key pressed with AltGr is not noted there.
