# Web executor trial

Unit U0.2 of the agent-setup run picks the library that drives the browser of Halvo's agents.
Contract A13.2 fixes the candidates, their order and the rule. The first candidate is `cdp`, the
Rust crate chromiumoxide 0.9.1, which speaks the browser's own DevTools protocol. It needs no
driver binary and no second runtime. The trial builds the adapter of a later candidate only after
the one before it missed.

## How the cells ran

`sh tools/r3-trials/web/run.sh --candidate cdp --browser PATH --port 18190 --runs 10` builds the
trial program with `cargo build --release --locked`, starts the fixture server of unit U0.1 on the
port, and runs the nine steps of A13.2 in their order, 10 runs each, with no model. Before each run
it resets the fixture to seed 1 with the variants of the step, and starts the browser headless on a
new profile directory under a new mktemp directory. After the run it stops the browser. The steps
read the fixture's state through the program's own HTTP client, which sends no `Origin` and no
`Sec-Fetch-Site` header. `--help` states every argument.

- Each step checks the column "ok when" of A13.2 on the fixture's state. Where that state cannot see
  what the browser did, the step also reads the request that the browser sent, through the
  adapter's request rule, which pauses each request before it leaves. `form_fill` requires
  `paperless=yes` in the body of the `POST /prefs`. `file_upload` requires that the file input
  holds one file of 1,024 bytes before the click and that the browser sent one `POST /upload`.
  The paused request carries no bytes of a file part, so the trial cannot read the file in the
  request, and the fixture records no size. `payment_frame` and
  `changed_page_1` also require that the payment holds the first instrument of the seed, and
  `payment_frame` reads the chosen option back from the page. `block_request` requires that the
  browser accepted the command that fails the request and that the page then shows the browser's
  error page, besides the unchanged `requests.state_changing`. The first option of `Payment method`
  is also the option that the page selects by default, so the choice in `payment_frame` changes
  nothing that the fixture can see. The fixture takes the amount of a payment from its token, not
  from the browser, so the amount check holds whenever a payment is recorded.
- `median_ms` is the median, over the runs of the cell that printed ok, of the time from the first
  page open of the run to the end of its check. It leaves out the browser start. `cold_ms` is the
  median of 10 starts, from the launch call to the load of `/signin`.
- Each step was first seen to miss. `--fault NAME` puts `tools/r3-trials/web/faults.mjs` between
  the browser and the fixture, and the proxy breaks one route or one page. Each step missed once
  under its fault: `signin_form` under `signin_500`, `table_read` under `history_changed`,
  `form_fill` and `session_expiry` under `prefs_500`, `payment_frame` under `frame_hangs` (the frame
  never loads), `file_upload` under `upload_input_gone`, `changed_page_1` under `pay_now_renamed`,
  `block_request` under `prefs_get_form` and `credential_mark` under `password_plain`. A control
  run of all nine steps behind the proxy with `history_changed` missed only `table_read`.
- The cells of `mac` ran through the machine's timed-run helper in one call, which the helper
  recorded from 2026-11-11T01:50:04Z to 2026-11-11T01:52:13Z, with the 1-minute load average at
  3.12 before and 9.11 after. Inside that call the cell ran from 01:51:12Z to 01:52:05Z and the
  extras from 01:52:05Z to 01:52:11Z. The call also held the development pass, the fault runs, the
  snapshot and the resolver probe. Other work on this Mac set that load, so the times of `mac` are
  times under load.
- The cells of the hosted machines ran in one job per machine, Chrome first and then Edge, on the
  head of the trial code plus one commit that adds the call of `run.sh` to the workflow file. The
  `hosted` lines name the run and that commit.

## Start of the browser

The `flag` lines in the block are the first flag set. The trial passes them, the resolver rule and
nothing else of its own. chromiumoxide adds these flags itself: `--remote-debugging-port=0` (the
browser picks the port; `--extras` names one for the attach check), `--user-data-dir`,
`--headless=new`, `--hide-scrollbars`, `--mute-audio` and `--disable-extensions`. The trial turns
off chromiumoxide's own list of 24 default flags (`disable_default_args`).

The resolver rule is one flag with two rules:

```text
--host-resolver-rules=MAP *.test 127.0.0.1,MAP * ~NOTFOUND
```

`sh tools/r3-trials/web/run.sh --candidate cdp --browser PATH --port 18190 --resolver-probe` showed
the rule at work on this Mac on 2026-11-11. The name `halvo-resolver-probe.localhost` got
`net::ERR_NAME_NOT_RESOLVED` and sent 0 requests to the fixture. Chrome resolves a `.localhost`
name to loopback by itself, with no lookup, so without the second rule the fixture would have seen
that request. `utility.agent.test` sent 2 requests to the fixture. The probe makes no lookup of a
name.

On macOS, both on this Mac and on the hosted macOS image, the caller adds `--use-mock-keychain`,
which no `flag` line holds. The first start of a browser in this unit, at 2026-11-11T00:25:58Z,
left the front app as it was (75 samples over 75 s, one app) and left no process behind.

## Browsers

Each run line holds the product and version that the browser reported over the protocol
(`Browser.getVersion`). All 90 runs of each cell reported one version:

| machine | browser | version | `--browser` path |
|---|---|---|---|
| `mac` | Chrome | 153.0.8010.48 | `/Applications/Google Chrome.app/Contents/MacOS/Google Chrome` |
| `hosted-mac` | Chrome | 157.0.8103.39 | `/Applications/Google Chrome.app/Contents/MacOS/Google Chrome` |
| `hosted-mac` | Edge | 156.0.4314.58 | `/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge` |
| `hosted-win` | Chrome | 156.0.8078.80 | `C:/Program Files/Google/Chrome/Application/chrome.exe` |
| `hosted-win` | Edge | 156.0.4314.58 | `C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe` |

No image holds a second major version of Chrome. The macos-15 image also holds Google Chrome for
Testing 157.0.8103.38, the same major as its Chrome. The windows-2025 image holds one version
directory under its Chrome install. All cells ran headless. Hosted runners can open a headed
window, but this trial did not open one.

## The library

- `pipe=no`: chromiumoxide 0.9.1 starts the browser with `--remote-debugging-port` and reads the
  WebSocket address from the browser's standard error, and it connects only over a WebSocket. It
  has no way to use `--remote-debugging-pipe`. Read on 2026-11-11 in `src/browser/config.rs` and
  `src/browser/mod.rs` of the package that cargo fetched from
  `https://crates.io/crates/chromiumoxide/0.9.1`.
- A snapshot gives each text node of the accessibility tree its own `text` line, so a label in the
  frame and the amount beside it come out as two `text` lines. A credential field follows A8.2: an `input` of type
  `password` or with one of the three `autocomplete` tokens, or a control whose parent or an
  ancestor of it carries `data-halvo-captcha`.
- The licence lines cover every crate in the trial's lock file, 138 packages, read from each
  crate's manifest with `node tools/r3-trials/web/licences.mjs --read 2026-11-11`. `unicode-ident`
  joins two licences with AND and has one line for each. The crate chromiumoxide is
  `MIT OR Apache-2.0`, with both texts in its package. No driver binary and no runtime are fetched.
- `added_bytes` is the growth of the stripped release binary of `web-trial` built with the feature
  `driver` (chromiumoxide, tokio, futures) against the same program built with
  `--no-default-features`, on this Mac.
- `tools/r3-trials/web/snapshots/pay-seed-1.txt` is the snapshot of `/pay` of seed 1 with its frame
  in the line form of A11.1.

```text
licence item=aho-corasick@1.1.5 spdx=MIT read=2026-11-11 ok=yes
licence item=async-tungstenite@0.32.1 spdx=MIT read=2026-11-11 ok=yes
licence item=atomic-waker@1.1.2 spdx=MIT read=2026-11-11 ok=yes
licence item=base64@0.22.1 spdx=MIT read=2026-11-11 ok=yes
licence item=base64@0.23.1 spdx=MIT read=2026-11-11 ok=yes
licence item=bitflags@2.13.2 spdx=MIT read=2026-11-11 ok=yes
licence item=block-buffer@0.10.4 spdx=MIT read=2026-11-11 ok=yes
licence item=bumpalo@3.20.3 spdx=MIT read=2026-11-11 ok=yes
licence item=bytes@1.12.1 spdx=MIT read=2026-11-11 ok=yes
licence item=cfg-if@1.0.5 spdx=MIT read=2026-11-11 ok=yes
licence item=chromiumoxide_cdp@0.9.1 spdx=MIT read=2026-11-11 ok=yes
licence item=chromiumoxide_pdl@0.9.1 spdx=MIT read=2026-11-11 ok=yes
licence item=chromiumoxide_types@0.9.1 spdx=MIT read=2026-11-11 ok=yes
licence item=chromiumoxide@0.9.1 spdx=MIT read=2026-11-11 ok=yes
licence item=cpufeatures@0.2.17 spdx=MIT read=2026-11-11 ok=yes
licence item=crypto-common@0.1.7 spdx=MIT read=2026-11-11 ok=yes
licence item=data-encoding@2.11.1 spdx=MIT read=2026-11-11 ok=yes
licence item=digest@0.10.7 spdx=MIT read=2026-11-11 ok=yes
licence item=displaydoc@0.2.7 spdx=MIT read=2026-11-11 ok=yes
licence item=dunce@1.0.5 spdx=Apache-2.0 read=2026-11-11 ok=yes
licence item=either@1.19.0 spdx=MIT read=2026-11-11 ok=yes
licence item=errno@0.3.14 spdx=MIT read=2026-11-11 ok=yes
licence item=fnv@1.0.7 spdx=MIT read=2026-11-11 ok=yes
licence item=form_urlencoded@1.2.2 spdx=MIT read=2026-11-11 ok=yes
licence item=futures-channel@0.3.34 spdx=MIT read=2026-11-11 ok=yes
licence item=futures-core@0.3.34 spdx=MIT read=2026-11-11 ok=yes
licence item=futures-executor@0.3.34 spdx=MIT read=2026-11-11 ok=yes
licence item=futures-io@0.3.34 spdx=MIT read=2026-11-11 ok=yes
licence item=futures-macro@0.3.34 spdx=MIT read=2026-11-11 ok=yes
licence item=futures-sink@0.3.34 spdx=MIT read=2026-11-11 ok=yes
licence item=futures-task@0.3.34 spdx=MIT read=2026-11-11 ok=yes
licence item=futures-timer@3.0.4 spdx=MIT read=2026-11-11 ok=yes
licence item=futures-util@0.3.34 spdx=MIT read=2026-11-11 ok=yes
licence item=futures@0.3.34 spdx=MIT read=2026-11-11 ok=yes
licence item=generic-array@0.14.7 spdx=MIT read=2026-11-11 ok=yes
licence item=getrandom@0.3.4 spdx=MIT read=2026-11-11 ok=yes
licence item=heck@0.5.0 spdx=MIT read=2026-11-11 ok=yes
licence item=http-body-util@0.1.5 spdx=MIT read=2026-11-11 ok=yes
licence item=http-body@1.1.0 spdx=MIT read=2026-11-11 ok=yes
licence item=http@1.5.0 spdx=MIT read=2026-11-11 ok=yes
licence item=httparse@1.10.1 spdx=MIT read=2026-11-11 ok=yes
licence item=hyper-util@0.1.21 spdx=MIT read=2026-11-11 ok=yes
licence item=hyper@1.12.0 spdx=MIT read=2026-11-11 ok=yes
licence item=icu_collections@2.4.0 spdx=Unicode-3.0 read=2026-11-11 ok=yes
licence item=icu_locale_core@2.4.0 spdx=Unicode-3.0 read=2026-11-11 ok=yes
licence item=icu_normalizer_data@2.4.0 spdx=Unicode-3.0 read=2026-11-11 ok=yes
licence item=icu_normalizer@2.4.0 spdx=Unicode-3.0 read=2026-11-11 ok=yes
licence item=icu_properties_data@2.4.0 spdx=Unicode-3.0 read=2026-11-11 ok=yes
licence item=icu_properties@2.4.0 spdx=Unicode-3.0 read=2026-11-11 ok=yes
licence item=icu_provider@2.4.0 spdx=Unicode-3.0 read=2026-11-11 ok=yes
licence item=idna_adapter@1.2.2 spdx=MIT read=2026-11-11 ok=yes
licence item=idna@1.1.0 spdx=MIT read=2026-11-11 ok=yes
licence item=ipnet@2.12.2 spdx=MIT read=2026-11-11 ok=yes
licence item=itoa@1.0.19 spdx=MIT read=2026-11-11 ok=yes
licence item=js-sys@0.3.107 spdx=MIT read=2026-11-11 ok=yes
licence item=libc@0.2.191 spdx=MIT read=2026-11-11 ok=yes
licence item=litemap@0.8.3 spdx=Unicode-3.0 read=2026-11-11 ok=yes
licence item=log@0.4.35 spdx=MIT read=2026-11-11 ok=yes
licence item=memchr@2.8.3 spdx=MIT read=2026-11-11 ok=yes
licence item=mio@1.2.4 spdx=MIT read=2026-11-11 ok=yes
licence item=once_cell@1.21.4 spdx=MIT read=2026-11-11 ok=yes
licence item=percent-encoding@2.3.2 spdx=MIT read=2026-11-11 ok=yes
licence item=pin-project-lite@0.2.17 spdx=MIT read=2026-11-11 ok=yes
licence item=potential_utf@0.1.6 spdx=Unicode-3.0 read=2026-11-11 ok=yes
licence item=ppv-lite86@0.2.21 spdx=MIT read=2026-11-11 ok=yes
licence item=proc-macro2@1.0.107 spdx=MIT read=2026-11-11 ok=yes
licence item=quote@1.0.47 spdx=MIT read=2026-11-11 ok=yes
licence item=r-efi@5.3.0 spdx=MIT read=2026-11-11 ok=yes
licence item=rand_chacha@0.9.0 spdx=MIT read=2026-11-11 ok=yes
licence item=rand_core@0.9.5 spdx=MIT read=2026-11-11 ok=yes
licence item=rand@0.9.5 spdx=MIT read=2026-11-11 ok=yes
licence item=regex-automata@0.4.18 spdx=MIT read=2026-11-11 ok=yes
licence item=regex-syntax@0.8.11 spdx=MIT read=2026-11-11 ok=yes
licence item=regex@1.13.1 spdx=MIT read=2026-11-11 ok=yes
licence item=reqwest@0.13.5 spdx=MIT read=2026-11-11 ok=yes
licence item=rustversion@1.0.23 spdx=MIT read=2026-11-11 ok=yes
licence item=serde_core@1.0.229 spdx=MIT read=2026-11-11 ok=yes
licence item=serde_derive@1.0.229 spdx=MIT read=2026-11-11 ok=yes
licence item=serde_json@1.0.154 spdx=MIT read=2026-11-11 ok=yes
licence item=serde@1.0.229 spdx=MIT read=2026-11-11 ok=yes
licence item=sha1@0.10.7 spdx=MIT read=2026-11-11 ok=yes
licence item=signal-hook-registry@1.4.8 spdx=MIT read=2026-11-11 ok=yes
licence item=slab@0.4.12 spdx=MIT read=2026-11-11 ok=yes
licence item=smallvec@1.16.5 spdx=MIT read=2026-11-11 ok=yes
licence item=socket2@0.6.5 spdx=MIT read=2026-11-11 ok=yes
licence item=stable_deref_trait@1.2.1 spdx=MIT read=2026-11-11 ok=yes
licence item=syn@2.0.119 spdx=MIT read=2026-11-11 ok=yes
licence item=syn@3.0.10 spdx=MIT read=2026-11-11 ok=yes
licence item=sync_wrapper@1.0.2 spdx=Apache-2.0 read=2026-11-11 ok=yes
licence item=synstructure@0.14.0 spdx=MIT read=2026-11-11 ok=yes
licence item=thiserror-impl@2.0.21 spdx=MIT read=2026-11-11 ok=yes
licence item=thiserror@2.0.21 spdx=MIT read=2026-11-11 ok=yes
licence item=tinystr@0.8.5 spdx=Unicode-3.0 read=2026-11-11 ok=yes
licence item=tokio-macros@2.7.2 spdx=MIT read=2026-11-11 ok=yes
licence item=tokio@1.54.1 spdx=MIT read=2026-11-11 ok=yes
licence item=tower-http@0.6.11 spdx=MIT read=2026-11-11 ok=yes
licence item=tower-layer@0.3.3 spdx=MIT read=2026-11-11 ok=yes
licence item=tower-service@0.3.3 spdx=MIT read=2026-11-11 ok=yes
licence item=tower@0.5.3 spdx=MIT read=2026-11-11 ok=yes
licence item=tracing-attributes@0.1.31 spdx=MIT read=2026-11-11 ok=yes
licence item=tracing-core@0.1.36 spdx=MIT read=2026-11-11 ok=yes
licence item=tracing@0.1.44 spdx=MIT read=2026-11-11 ok=yes
licence item=try-lock@0.2.5 spdx=MIT read=2026-11-11 ok=yes
licence item=tungstenite@0.28.0 spdx=MIT read=2026-11-11 ok=yes
licence item=typenum@1.20.1 spdx=MIT read=2026-11-11 ok=yes
licence item=unicode-ident@1.0.27 spdx=MIT read=2026-11-11 ok=yes
licence item=unicode-ident@1.0.27 spdx=Unicode-3.0 read=2026-11-11 ok=yes
licence item=url@2.5.8 spdx=MIT read=2026-11-11 ok=yes
licence item=utf_types@1.0.0 spdx=MIT read=2026-11-11 ok=yes
licence item=utf-8@0.7.6 spdx=MIT read=2026-11-11 ok=yes
licence item=utf8_iter@1.1.0 spdx=MIT read=2026-11-11 ok=yes
licence item=version_check@0.9.5 spdx=MIT read=2026-11-11 ok=yes
licence item=want@0.3.2 spdx=MIT read=2026-11-11 ok=yes
licence item=wasi@0.11.1+wasi-snapshot-preview1 spdx=MIT read=2026-11-11 ok=yes
licence item=wasip2@1.0.4+wasi-0.2.12 spdx=MIT read=2026-11-11 ok=yes
licence item=wasm-bindgen-futures@0.4.80 spdx=MIT read=2026-11-11 ok=yes
licence item=wasm-bindgen-macro-support@0.2.130 spdx=MIT read=2026-11-11 ok=yes
licence item=wasm-bindgen-macro@0.2.130 spdx=MIT read=2026-11-11 ok=yes
licence item=wasm-bindgen-shared@0.2.130 spdx=MIT read=2026-11-11 ok=yes
licence item=wasm-bindgen@0.2.130 spdx=MIT read=2026-11-11 ok=yes
licence item=web-sys@0.3.107 spdx=MIT read=2026-11-11 ok=yes
licence item=which@8.0.6 spdx=MIT read=2026-11-11 ok=yes
licence item=windows-link@0.2.1 spdx=MIT read=2026-11-11 ok=yes
licence item=windows-registry@0.6.1 spdx=MIT read=2026-11-11 ok=yes
licence item=windows-result@0.4.1 spdx=MIT read=2026-11-11 ok=yes
licence item=windows-strings@0.5.1 spdx=MIT read=2026-11-11 ok=yes
licence item=windows-sys@0.61.2 spdx=MIT read=2026-11-11 ok=yes
licence item=wit-bindgen@0.57.1 spdx=MIT read=2026-11-11 ok=yes
licence item=writeable@0.6.4 spdx=Unicode-3.0 read=2026-11-11 ok=yes
licence item=yoke-derive@0.8.4 spdx=Unicode-3.0 read=2026-11-11 ok=yes
licence item=yoke@0.8.4 spdx=Unicode-3.0 read=2026-11-11 ok=yes
licence item=zerocopy-derive@0.8.63 spdx=MIT read=2026-11-11 ok=yes
licence item=zerocopy@0.8.63 spdx=MIT read=2026-11-11 ok=yes
licence item=zerofrom-derive@0.1.8 spdx=Unicode-3.0 read=2026-11-11 ok=yes
licence item=zerofrom@0.1.8 spdx=Unicode-3.0 read=2026-11-11 ok=yes
licence item=zerotrie@0.2.5 spdx=Unicode-3.0 read=2026-11-11 ok=yes
licence item=zerovec-derive@0.11.7 spdx=Unicode-3.0 read=2026-11-11 ok=yes
licence item=zerovec@0.11.9 spdx=Unicode-3.0 read=2026-11-11 ok=yes
licence item=zmij@1.0.23 spdx=MIT read=2026-11-11 ok=yes
flag --disable-background-networking
flag --disable-component-update
flag --disable-sync
flag --disable-field-trial-config
flag --disable-client-side-phishing-detection
flag --disable-default-apps
flag --disable-breakpad
flag --disable-domain-reliability
flag --metrics-recording-only
flag --no-first-run
flag --no-default-browser-check
hosted machine=hosted-mac run=42937993035 sha=5a3cbec722ffb115f02c17a199abe6dcf75eea0f
hosted machine=hosted-win run=42937993035 sha=5a3cbec722ffb115f02c17a199abe6dcf75eea0f
cell candidate=cdp machine=mac browser=chrome major=153 step=signin_form ok=10 of=10 median_ms=103
cell candidate=cdp machine=mac browser=chrome major=153 step=table_read ok=10 of=10 median_ms=106
cell candidate=cdp machine=mac browser=chrome major=153 step=form_fill ok=10 of=10 median_ms=195
cell candidate=cdp machine=mac browser=chrome major=153 step=payment_frame ok=10 of=10 median_ms=183
cell candidate=cdp machine=mac browser=chrome major=153 step=file_upload ok=10 of=10 median_ms=183
cell candidate=cdp machine=mac browser=chrome major=153 step=session_expiry ok=10 of=10 median_ms=198
cell candidate=cdp machine=mac browser=chrome major=153 step=changed_page_1 ok=10 of=10 median_ms=178
cell candidate=cdp machine=mac browser=chrome major=153 step=block_request ok=10 of=10 median_ms=179
cell candidate=cdp machine=mac browser=chrome major=153 step=credential_mark ok=10 of=10 median_ms=171
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=signin_form ok=10 of=10 median_ms=673
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=table_read ok=10 of=10 median_ms=673
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=form_fill ok=10 of=10 median_ms=1145
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=payment_frame ok=10 of=10 median_ms=939
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=file_upload ok=10 of=10 median_ms=868
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=session_expiry ok=10 of=10 median_ms=1002
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=changed_page_1 ok=10 of=10 median_ms=902
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=block_request ok=10 of=10 median_ms=951
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=credential_mark ok=10 of=10 median_ms=894
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=signin_form ok=10 of=10 median_ms=705
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=table_read ok=10 of=10 median_ms=714
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=form_fill ok=10 of=10 median_ms=1055
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=payment_frame ok=10 of=10 median_ms=1014
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=file_upload ok=10 of=10 median_ms=905
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=session_expiry ok=10 of=10 median_ms=1003
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=changed_page_1 ok=10 of=10 median_ms=910
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=block_request ok=10 of=10 median_ms=939
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=credential_mark ok=10 of=10 median_ms=694
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=signin_form ok=10 of=10 median_ms=864
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=table_read ok=10 of=10 median_ms=867
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=form_fill ok=10 of=10 median_ms=1016
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=payment_frame ok=10 of=10 median_ms=1008
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=file_upload ok=10 of=10 median_ms=963
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=session_expiry ok=10 of=10 median_ms=995
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=changed_page_1 ok=10 of=10 median_ms=1000
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=block_request ok=10 of=10 median_ms=982
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=credential_mark ok=10 of=10 median_ms=942
cell candidate=cdp machine=hosted-win browser=edge major=156 step=signin_form ok=10 of=10 median_ms=1266
cell candidate=cdp machine=hosted-win browser=edge major=156 step=table_read ok=10 of=10 median_ms=1383
cell candidate=cdp machine=hosted-win browser=edge major=156 step=form_fill ok=10 of=10 median_ms=1517
cell candidate=cdp machine=hosted-win browser=edge major=156 step=payment_frame ok=10 of=10 median_ms=1420
cell candidate=cdp machine=hosted-win browser=edge major=156 step=file_upload ok=10 of=10 median_ms=1500
cell candidate=cdp machine=hosted-win browser=edge major=156 step=session_expiry ok=10 of=10 median_ms=1476
cell candidate=cdp machine=hosted-win browser=edge major=156 step=changed_page_1 ok=10 of=10 median_ms=1468
cell candidate=cdp machine=hosted-win browser=edge major=156 step=block_request ok=10 of=10 median_ms=1533
cell candidate=cdp machine=hosted-win browser=edge major=156 step=credential_mark ok=10 of=10 median_ms=1385
extras candidate=cdp added_bytes=7779328 cold_ms=377 record=ok attach=ok pipe=no
verdict executor=cdp version=0.9.1
```
