# The page-test harness

`tools/r4-page/` is the instrument that runs the six accessibility checks of the interface run (contracts.md I9.3) in a real browser engine. This file records what the contract leaves to the harness, and what was read before the first fetch.

## Package

`tools/r4-page/package.json` and its lock pin two packages. The lock resolves three:

| package | version | licence, read from the package |
|---|---|---|
| `playwright` | 1.65.0 | Apache-2.0 (`package.json` and `LICENSE`) |
| `playwright-core` | 1.65.0 | Apache-2.0 (`package.json` and `LICENSE`) |
| `accessibility-checker-engine` | 4.0.34 | Apache-2.0 (`package.json` and the header of `ace.js.LICENSE.txt`) |

`playwright` and `playwright-core` inline other npm packages into some of their files and list them with their licence texts in the `.LICENSE` files beside those files. The licence texts there are MIT, ISC and BSD-3-Clause.

## Browser builds

Each build that a machine can start has a line in `tools/r4-page/files.tsv`: the build, the system name that Playwright 1.65.0 uses for its downloads, the address on Playwright's download host, the size in bytes and the SHA-256. One download of each archive into a directory from `mktemp -d` gave the size and the hash. Nothing in that directory was run.

| build | system | machines | licence as read |
|---|---|---|---|
| `webkit-2388` | `mac26-arm64` | `mac` | The archive holds no licence file for WebKit itself. It holds the licence files of four parts of the web inspector (CodeMirror, three.js, Esprima, CSSDocumentation). |
| `webkit-2388` | `mac15-arm64` | `hosted-mac` | As for `mac26-arm64`. |
| `chromium-1254` | `win64` | `hosted-win` | Chrome for Testing 158.0.8127.18. The archive holds no licence file. Its `ABOUT` file says "Copyright 2026 Google LLC. All rights reserved", names the Chromium project and other open source software (`chrome://credits`), and points to the terms of service at `chrome://terms`. |
| `chromium-1254` | `mac26-arm64`, `mac15-arm64` | none in `browser.txt` | The same `ABOUT` text. The archive also holds the Widevine module with Google's licence, which says that the module is not open source. |

contracts.md I0.7 puts these builds outside the licence rule, because they run only as tools of a check. This harness does not read the publishers' own statements of these licences, because a step of this unit reaches only the npm registry and Playwright's download host.
