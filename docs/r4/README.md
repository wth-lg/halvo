# docs/r4: the interface run

The interface run builds the coordinator of Halvo's agents, which decides
when each agent runs and what waits for the person, and the views that show
the agents and those items. This directory holds the run's documents. Today
it holds four files. The other files below come with the unit named beside
each. No command reads these files.

- `README.md`: this file (U1.4).
- `.gitattributes`: the line `* text eol=lf`, so that git checks out every
  file here with LF line ends (U0.1).
- `harness.md`: how to call the page-test harness in `tools/r4-page/`, and
  the choices that the contract leaves to it (U0.1).
- `alive.md`: which parts of Halvo may be alive at once and the bound in
  bytes of each (U1.4). Unit U7.1 adds the measured values.
- `trials/window.md` and `trials/sync.md` will hold the results of the window
  and state-sync trial (U0.2).
- `trials/floor.md` will hold the result of the floor trial, and
  `page-features.txt` the list of page features that a view file must keep
  to (U0.3).
- `trials/driver.md` and `trials/dialog.md` will hold the results of the
  driver and native dialog trial (U0.4).
- `words.md` will hold the glossary: each term, its meaning, its run and
  whether it is reserved (U1.3).
- `bounds.md` will hold the memory figures of the app after its window was
  shown once, with and without agents, on each hosted machine (U7.1).
- `gate.md` will hold the checks of the run that a person makes by hand
  (U8.1).
