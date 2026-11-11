# docs/r3: the agent-setup run

The agent-setup run configures one agent for each workflow that Halvo
identified and builds the runtime that carries out the agent's tasks. This
directory holds the run's documents. No command reads this file.

- `README.md`: this file.
- `.gitattributes`: the line `* text eol=lf`, so that git checks out every
  file here with LF line ends.
- `category-parameters.md`: every parameter that an agent needs, for each of
  the five seed categories that have a task in this run, with where its value
  comes from and where it is kept.
- `fixtures.md`: the routes, controls, cookies, data keys and variants of the
  local test sites in `tools/fixture-server/variants/`.
- `trials/`: the results of the trials of phase P0.
  - `web-executor.md`: which library drives the browser of the agents.
  - `browser-profile.md`: the privacy model of the agent browser: an empty
    profile and a temporary directory for each run, which the run removes at
    its end.
  - `sheet.md`: which library changes cells in a workbook that the user
    already has, and which kinds of workbook part the executor must refuse.
