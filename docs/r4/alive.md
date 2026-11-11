# What may be alive at once

Halvo runs on a person's own machine. The reference laptop has 16 GB of memory, and that is not enough for two loaded models beside everything else. In the interface run, the coordinator decides which parts of Halvo may run at the same time. This file says which parts may be alive together and how many bytes each part may hold. It is item I12.6 of the interface run's contract.

Each bound in the table is a bound that another run set, copied from that run's plan or contract. The column `source` names the run and the item: CR is the capture run, W the workflow run, A the agent-setup run and B the billing run. The bounds were read from those texts on 2026-11-11. A cell with no value holds `-`. The column `measured` holds `-` in every row until unit U7.1 fills it. No command reads this file.

| part | bound in bytes | source | alive with | measured |
|---|---|---|---|---|
| the capture daemon | 50,331,648 | W plan M1; CR plan Bounds | always | - |
| the app | - | CR plan Bounds | always | - |
| the derived owner | 33,554,432 | W plan M2 | on demand | - |
| the workflow run's worker | 268,435,456 | W plan done-when | on demand | - |
| the workflow run's batch with a model | 8,589,934,592 | W-C23.3 | never while a run's model child holds a model | - |
| one run tree: a runner, its browser and its model child | 8,589,934,592 in sum, the runner at most 268,435,456 | A-A14.2 | its model child never while the batch holds a model | - |
| the billing run's helper | - | B plan unit 7.3 | for the time of one call | - |

## The rows

- The capture daemon: the capture run's key `rss_max_bytes` is 50331648, and the workflow run's line M1 asks for a total peak of at most 48 MiB (50,331,648 bytes).
- The app: its bound is the capture run's key `app_rss_max_bytes` on macOS and `app_working_set_max_bytes` on Windows. On 2026-11-11 that run's plan gives each key a rule and no value. Its unit gate-3 writes the first values from its spike S7.
- The derived owner: the workflow run's line M2 asks that the owner's peak under the load of its arm 2 is at most 32 MiB (33,554,432 bytes), with an 8 MiB page-cache cap.
- The workflow run's worker: that run freezes its bound W in its phase P6, and W is at most 256 MiB (268,435,456 bytes).
- The workflow run's batch with a model: the model ceiling of that run is 8 GiB, which W-C23.3 states once in bytes as 8,589,934,592.
- One run tree: the runner with every process that it starts. The runner's bound is at most 268,435,456 bytes, and the bounds of the runner, its browser and its model child sum to at most 8,589,934,592 bytes, half the 16 GB of the reference laptops.
- The billing run's helper: that run reports the helper's peak memory and sets no bound for it, because the helper exits after each call.

## The rules that make the table true

The coordinator is not built yet. Once it is, these rules of the interface run's contract must hold:

- `max_live_runs` is 1, so at most one run tree is alive.
- The model lock of W-C23.12 lets one process hold a loaded model. So the batch's model and a run's model child are never alive together.
- Before a run starts, and while it is live, the coordinator asks the workflow run to defer its batch (I5.5). That request can fail. The batch is the only part for which the interface run makes it.
- A run that waits ends after `wait_limit_s`.

So one loaded model is alive at a time beside the parts that are always alive. A runner with its browser can be alive beside the worker, and beside the batch when a defer request has failed.
