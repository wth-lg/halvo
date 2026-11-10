# r2-probes

Probes for three cost trials of the workflow run (phase P0, units U0.3, U0.4 and U0.5). They run on
the hosted runners macos-15 and windows-2025 and write the files under `docs/r2/trials/`:
`layer-cost.md`, `input-visibility.md` and `private-window.md`. No product build reads this crate.
It has a workspace and a `Cargo.lock` of its own.

## What a run does

`sh tools/r2-probes/run-hosted.sh OUT` does the whole run on one runner. It fetches the ONNX
Runtime archive, the PP-OCRv6 tiny models and LibreOffice by the list in `files.tsv`, and checks
the size and the SHA-256 of each file. It builds both binaries from the lock, and runs
`r2-probes suite`. The suite writes into the new directory `OUT`:

- `records.jsonl`: one line per run of a cell, or one line that says why a cell is not measurable.
- `meta.json`: the runner, the system version, the display size before and after the probe set
  it, the browser versions, and the branch, commit, run and attempt of the hosted run.
- `frames/`: the frame of each run of an extraction cell at downscale 1.0, and one frame of the
  first run of each other cell.

Arguments after `OUT` go to the suite: `--only CELL-PREFIX` runs a part of the cells, and
`--runs N` changes the run count.

`r2-probes trial KIND --artifact DIR... --records FILE... --lines FILE --out FILE` writes a trial
file from the records. The verdict lines are computed from the figures as the file prints them.

## Cells

Every run of a cell is one process. A crash is run again, and the cell counts the reruns in
`crash_reruns_count`. A run that gives no answer in 60 s ends, and its cell is not measurable.

- `address.RUNNER.BROWSER`: each run starts a new browser process on the page `text-1`, served
  from 127.0.0.1, and makes one first read of the address field. On macOS the read first takes
  the attributes `AXEnhancedUserInterface` and `AXManualAccessibility` of the application
  element, then walks the focused window breadth first and reads `AXRole` and `AXValue` of each
  node. On Windows it walks the control view of the window with UI Automation and reads the
  value pattern. It does not enter the page. The read tries again every 250 ms for 5 s, because
  a browser fills its tree after the first query. `readable_runs` counts the runs in which the
  value held `127.0.0.1` and no activation attribute was set.
- `accessibility.RUNNER.TARGET[.plain|.activated]`: the focused element and the subtree of the
  focused window in the form of contracts.md C11.7 (16 levels, 1,600 lines). The name of a node
  is `AXTitle`, or `AXDescription` where the title is empty, and its value is `AXValue`; on
  Windows they are the name and the value pattern of the control view. The macOS walk also reads
  `AXRole` of each node. A run reads the subtree twice, 1.5 s apart, and keeps the larger text;
  `first_bytes` is the first read. `.activated` sets `AXEnhancedUserInterface` on macOS and
  starts Chrome with `--force-renderer-accessibility` on Windows.
- `extract.RUNNER.ENGINE.TARGET.xSCALE`: a frame of the display through ScreenCaptureKit or
  Windows.Graphics.Capture, then two calls of the engine in the same process. `first_call_ms`
  is the first call. `call_ms` is the second call: the larger of its elapsed time and the
  processor time of the process over it. The pages show in Chrome in kiosk mode, so the frame
  holds the page alone. Downscale 0.5 averages blocks of 2 by 2 pixels.
- `engine.RUNNER.ENGINE.xSCALE`: the runs of the three text pages together. The verdict lines
  read these cells at downscale 1.0.
- `private.RUNNER.BROWSER`: one normal and one private window. The signal `attribute` answers
  when an accessibility attribute of the private window or of an element of its frame (8
  levels, 600 nodes, outside the page) holds `incognito`, `inprivate` or `private` and the
  normal window has no such value. `class` answers when the window classes differ. `title`
  answers when the title of the private window holds a marker and the title of the normal
  window holds none. The title is `AXTitle` on macOS and the window text on Windows.
- `input.windows-2025.DRIVER`: a message-only window registers Raw Input for keyboards and
  mice and counts key-down and button-down records while the driver sends 20 key presses and
  5 clicks to the page `input`. `page_keys_count` and `page_clicks_count` are what the page
  counted.
- `listen-event-access.macos-15`: the return value of `CGPreflightListenEventAccess`.

## The second timing on this Mac

`sh tools/r2-probes/retime.sh FRAMES FILES OUT` times the PP-OCRv6 probe on the saved frames of
a hosted macos-15 run, one process per frame, each under `/usr/bin/time -l`. Run it alone
through the helper for timed runs. `r2-probes stamp` then adds the start and end times of the
helper's record line and the largest resident set size to each record.

## Checks

```sh
cargo fmt --check
cargo clippy --all-targets --features pp-ocrv6 -- -D warnings
cargo test
shellcheck -s sh fetch-files.sh run-hosted.sh retime.sh
```
