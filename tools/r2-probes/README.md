# r2-probes

Probes for three cost trials of the workflow run (phase P0, units U0.3, U0.4 and U0.5). They run on
the hosted runners macos-15 and windows-2025 and write the files under `docs/r2/trials/`:
`layer-cost.md`, `input-visibility.md` and `private-window.md`. No product build reads this crate.
It has a workspace and a `Cargo.lock` of its own.

## What a run does

`sh tools/r2-probes/run-hosted.sh OUT` does the whole run on one runner. It fetches the ONNX
Runtime archive, the PP-OCRv6 tiny models and LibreOffice by the list in `files.tsv`, and checks
the size and the SHA-256 of each file. It builds both binaries from the lock, and runs
`r2-probes suite`. The suite sets the display to 1920x1080 where it differs, and writes into the
new directory `OUT`:

- `records.jsonl`: one line per run of a cell, or one line that says why a cell is not measurable.
- `meta.json`: the runner, the system version, the display size before and after the probe set
  it, the browser versions, and the branch, commit, run and attempt of the hosted run.
- `frames/`: the frame of each run of an extraction cell on a text page at downscale 1.0.

Browser profiles, the LibreOffice profile and the frames of the two LibreOffice windows go to
`OUT-scratch`, beside `OUT`. Arguments after `OUT` go to the suite: `--only CELL-PREFIX` runs a
part of the cells, `--runs N` changes the run count, and `--debug-frames` also saves one frame of
the first run of every other cell, with the pointer.

`r2-probes trial KIND --artifact DIR... --records FILE... --lines FILE --out FILE` writes a trial
file from the records, and `r2-probes lines KIND --crate-dir tools/r2-probes` prints the lines
that stand between the cells and the verdicts (crates, fetched files, commands).

## Cells

Every run of a cell is one process. A crash is run again, and the cell counts the reruns in
`crash_reruns_count`. A browser that does not request its page in 30 s is started once more,
and `start_reruns_count` counts those starts. A run that gives no answer in 60 s ends, and its cell is not measurable.
The extraction cells run first, before any other window is on the display: on the Windows
runner a prompt that Firefox raises at its first start stays on the display and would stand in
every later frame. Every run of an address cell starts a browser process of its own with a new
profile, so no other cell has touched the process that it reads.

- `extract.RUNNER.ENGINE.TARGET.xSCALE`: a frame of the display through ScreenCaptureKit or
  Windows.Graphics.Capture, then two calls of the engine in the same process. `first_call_ms`
  is the first call. `call_ms` is the second call: the larger of its elapsed time and the
  processor time of the process over the same span. The pages show in Chrome in kiosk mode, so
  the frame holds the page alone; the page `list-1` is longer than the display, so its frame
  holds its first rows and its cells have no error rate. Downscale 0.5 averages blocks of 2 by 2 pixels. The error
  rate is the Levenshtein distance between the engine's pieces, joined by one space, and the
  text of the page body, both with white space folded, over the length of the page text, in
  Unicode scalar values. `stored_bytes` is the UTF-8 length of the joined pieces.
- `engine.RUNNER.ENGINE.xSCALE`: the runs of the three text pages together. The verdict lines
  read these cells at downscale 1.0. A cell whose name ends in `.this-mac` holds the second
  timing of the same engine on the saved frames, on a machine that is no hosted VM.
- `address.RUNNER.BROWSER`: each run starts a new browser process with a new profile on the
  page `text-1`, served from 127.0.0.1, and makes one first read of the address field. On macOS
  the read first takes the attributes `AXEnhancedUserInterface` and `AXManualAccessibility` of
  the application element, then walks the focused window breadth first and reads `AXRole` and
  `AXValue` of each node. On Windows it walks the control view of the window with UI Automation
  and reads the value pattern. It does not enter the page. The read tries again every 250 ms,
  because a browser fills its tree after the first query: for 7 s on Windows, and for 2 s on
  macOS. On macOS, after 2 s without a value, the read asks for `AXRole` of the application
  element once and tries for 5 s more; `role_query_runs` counts the runs that needed this
  query. `cpu_before_ms` is the processor time of the browser's processes over the 2 s before
  the read, and `cpu_after_ms` over the 2 s that start with the read; in a run that needed the
  role query the second window starts at the query, so that what the query started is inside
  the figure. `mem_before_bytes` and `mem_after_bytes` are taken at the ends of the two windows. `readable_runs` counts the runs in which the value held
  `127.0.0.1` and no activation attribute was set.
- `accessibility.RUNNER.TARGET[.plain|.activated]`: the focused element and the subtree of the
  focused window in the form of contracts.md C11.7 (16 levels, 1,600 lines). The name of a node
  is `AXTitle`, or `AXDescription` where the title is empty, and its value is `AXValue`; on
  Windows they are the name and the value pattern of the control view. The macOS walk also reads
  `AXRole` of each node. A run reads the subtree twice, 1.5 s apart, and keeps the larger text;
  `first_bytes` is the first read. `.activated` sets `AXEnhancedUserInterface` and
  `AXManualAccessibility` on macOS, waits 3 s and records the two error codes of the calls; on
  Windows it starts Chrome with `--force-renderer-accessibility`.
- `private.RUNNER.BROWSER`: one normal and one private window. The signal `attribute` answers
  when an accessibility attribute of the private window or of an element of its frame (8
  levels, 600 nodes, outside the page) holds `incognito`, `inprivate` or `private` and the
  normal window has no such value. `class` answers when the window classes differ. `title`
  answers when the title of the private window holds a marker and the title of the normal
  window holds none. The title is `AXTitle` on macOS and the window text on Windows. Safari has
  no switch for a private window: the probe presses the menu item File, New Private Window
  through the accessibility interface and then opens the page with `open -a Safari`.
- `input.windows-2025.DRIVER`: a message-only window registers Raw Input for keyboards and
  mice and counts key-down and button-down records while the driver sends 20 key presses and
  5 clicks to the page `input`. `page_keys_count` and `page_clicks_count` are what the page
  counted.
- `listen-event-access.macos-15`: the return value of `CGPreflightListenEventAccess`.

## Verdict lines

`r2-probes trial` computes them from the figures as the cell lines print them.

- An engine holds the line when `call_ms` of its cell `engine.RUNNER.ENGINE.x1.0` is at most
  172.0. Where that figure is over the line and a cell `…x1.0.this-mac` exists, the second
  figure decides and the value ends with `re-timed`; with no such cell it ends with
  `not re-timed`. Apple Vision has two cells, one per recognition level. Its verdict takes the
  accurate level when that level holds the line, else the fast level when that holds, else the
  accurate level.
- D3 is `os-pair` when both OS engines hold the line and their `cer_pct` at downscale 1.0
  differ by at most 2.0, else `pp-ocrv6-tiny`. The Apple Vision figure is that of the level
  its verdict took.
- The site layer stays when `readable_runs` equals `runs` in every `address` cell.
- The accessibility layer stays when no `accessibility` cell stores more than 78,000 bytes.
- The extracted-text layer stays when every engine that D3 names holds the line on its runner
  and no `extract` cell of those engines stores more than 78,000 bytes.
- A layer with a cell that is not measurable, and no cell over its line, is `not measurable`.

## The second timing on this Mac

`sh tools/r2-probes/retime.sh ENGINE FRAMES OUT [FILES]` times an engine again on the saved
frames of a hosted macos-15 run, one process per frame, each under `/usr/bin/time -l`. It
reads image files and calls no capture or accessibility interface. Run it alone through the
helper for timed runs. `r2-probes stamp` then adds the start and end times of the helper's
record line and the largest resident set size to each record.

## Checks

```sh
cargo fmt --check
cargo clippy --all-targets --features pp-ocrv6 -- -D warnings
cargo test
shellcheck -s sh fetch-files.sh run-hosted.sh retime.sh
```
