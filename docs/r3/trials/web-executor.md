# Web executor trial

Unit U0.2 of the agent-setup run picks the library that drives the browser of Halvo's agents
(contract A13.2). The first candidate is `cdp`, the Rust crate chromiumoxide 0.9.1, which speaks
the browser's DevTools protocol with no driver binary and no second runtime. The trial builds the
adapter of a later candidate only after the one before it missed. `tools/r3-trials/web/README.md`
says how each step runs and what each check reads, and holds the longer notes of U0.2.

What ran. Unit U0.7 changed the library, so the nine steps ran again at its head 4f494e9 with
`sh tools/r3-trials/web/run.sh --candidate cdp --browser PATH --port 18190 --runs 10`. On `mac`
each step ran with Chrome as one call of the machine's timed-run helper, from 2026-11-12T01:28:12Z to
2026-11-12T01:42:48Z, under a 1-minute load average between 223.21 and 504.32, so the times of
`mac` are times under load. The extras line ran on `mac` in one more call, from 2026-11-12T01:54:14Z to
2026-11-12T01:55:13Z. The hosted cells ran in run 43103175522, Chrome first and then Edge, on a scratch branch
that holds the head plus one commit on the workflow file. U0.2's own cells, at 20d8421 in run
42950082356, all had `ok=10`. The library grew from U0.7's changes, so `added_bytes` grew too.

Choices that the contract leaves. The flags of the start are the `flag` lines below. The resolver
rule is one flag with two rules, `--host-resolver-rules=MAP *.test 127.0.0.1,MAP * ~NOTFOUND`; the
second keeps a `.localhost` name from reaching the fixture. On macOS the caller adds
`--use-mock-keychain`, which no `flag` line holds. The `--browser` paths are
`/Applications/Google Chrome.app/Contents/MacOS/Google Chrome` and
`/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge` on the Macs, and
`C:/Program Files/Google/Chrome/Application/chrome.exe` and
`C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe` on `hosted-win`. Browser versions,
as each run line reports them: `mac` Chrome 153.0.8010.48; `hosted-mac` Chrome 157.0.8103.39; `hosted-mac` Edge 156.0.4314.58 and 157.0.4341.46; `hosted-win` Chrome 156.0.8078.80; `hosted-win` Edge 156.0.4314.58. Edge on the macOS image updated itself during the
job; the nine steps ran on 156. No image holds a second major version of Chrome.
`pipe=no`: chromiumoxide 0.9.1 connects only over a WebSocket at `--remote-debugging-port`. The
licence lines cover the 138 packages of the trial's lock file, which U0.7 did not change.

The third round (U0.7c) measured again, at commit e72f083, the cells that the library's connection
decided: `--extras --round 3` on `mac` with Chrome 153.0.8010.48, one call of the timed-run helper
from 2026-11-12T21:37:14Z to 2026-11-12T21:47:14Z at a 1-minute load of 93.42, so `cold_ms` is a
time under load. `added_bytes` is the binary with the protocol types and the pipe alone
(`--no-default-features --features driver`) against one with no driver crate; `cold_ms` starts as
the product does (pipe, relay, probe); `attach` and `pipe` are struck with the connect operation.

What the lines cannot show. Ten runs cannot show a miss rarer than about one in four. The verdict
stands for these nine steps. It decides U2.1 only by the rule of A13.7, whose rows are in
`docs/r3/trials/web-isolation.md`.

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
hosted machine=hosted-mac run=43103175522 sha=f2e53eeb7893784891a5c9038a0f63dcaa4ec124
hosted machine=hosted-win run=43103175522 sha=f2e53eeb7893784891a5c9038a0f63dcaa4ec124
cell candidate=cdp machine=mac browser=chrome major=153 step=signin_form ok=10 of=10 median_ms=121
cell candidate=cdp machine=mac browser=chrome major=153 step=table_read ok=10 of=10 median_ms=123
cell candidate=cdp machine=mac browser=chrome major=153 step=form_fill ok=10 of=10 median_ms=235
cell candidate=cdp machine=mac browser=chrome major=153 step=payment_frame ok=10 of=10 median_ms=245
cell candidate=cdp machine=mac browser=chrome major=153 step=file_upload ok=10 of=10 median_ms=231
cell candidate=cdp machine=mac browser=chrome major=153 step=session_expiry ok=10 of=10 median_ms=237
cell candidate=cdp machine=mac browser=chrome major=153 step=changed_page_1 ok=10 of=10 median_ms=226
cell candidate=cdp machine=mac browser=chrome major=153 step=block_request ok=10 of=10 median_ms=216
cell candidate=cdp machine=mac browser=chrome major=153 step=credential_mark ok=10 of=10 median_ms=232
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=signin_form ok=10 of=10 median_ms=540
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=table_read ok=10 of=10 median_ms=514
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=form_fill ok=10 of=10 median_ms=821
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=payment_frame ok=10 of=10 median_ms=993
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=file_upload ok=10 of=10 median_ms=963
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=session_expiry ok=10 of=10 median_ms=819
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=changed_page_1 ok=10 of=10 median_ms=812
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=block_request ok=10 of=10 median_ms=768
cell candidate=cdp machine=hosted-mac browser=chrome major=157 step=credential_mark ok=10 of=10 median_ms=658
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=signin_form ok=10 of=10 median_ms=637
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=table_read ok=10 of=10 median_ms=582
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=form_fill ok=10 of=10 median_ms=912
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=payment_frame ok=10 of=10 median_ms=895
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=file_upload ok=10 of=10 median_ms=863
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=session_expiry ok=10 of=10 median_ms=903
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=changed_page_1 ok=10 of=10 median_ms=868
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=block_request ok=10 of=10 median_ms=776
cell candidate=cdp machine=hosted-mac browser=edge major=156 step=credential_mark ok=10 of=10 median_ms=697
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=signin_form ok=10 of=10 median_ms=864
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=table_read ok=10 of=10 median_ms=901
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=form_fill ok=10 of=10 median_ms=988
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=payment_frame ok=10 of=10 median_ms=990
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=file_upload ok=10 of=10 median_ms=986
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=session_expiry ok=10 of=10 median_ms=1072
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=changed_page_1 ok=10 of=10 median_ms=1052
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=block_request ok=10 of=10 median_ms=1037
cell candidate=cdp machine=hosted-win browser=chrome major=156 step=credential_mark ok=10 of=10 median_ms=1005
cell candidate=cdp machine=hosted-win browser=edge major=156 step=signin_form ok=10 of=10 median_ms=1196
cell candidate=cdp machine=hosted-win browser=edge major=156 step=table_read ok=10 of=10 median_ms=1321
cell candidate=cdp machine=hosted-win browser=edge major=156 step=form_fill ok=10 of=10 median_ms=1310
cell candidate=cdp machine=hosted-win browser=edge major=156 step=payment_frame ok=10 of=10 median_ms=1569
cell candidate=cdp machine=hosted-win browser=edge major=156 step=file_upload ok=10 of=10 median_ms=1394
cell candidate=cdp machine=hosted-win browser=edge major=156 step=session_expiry ok=10 of=10 median_ms=1488
cell candidate=cdp machine=hosted-win browser=edge major=156 step=changed_page_1 ok=10 of=10 median_ms=1451
cell candidate=cdp machine=hosted-win browser=edge major=156 step=block_request ok=10 of=10 median_ms=1384
cell candidate=cdp machine=hosted-win browser=edge major=156 step=credential_mark ok=10 of=10 median_ms=1264
extras candidate=cdp added_bytes=10867056 cold_ms=628 record=ok attach=ok pipe=no
extras candidate=cdp form=pipe added_bytes=5651648 cold_ms=2131 record=ok
verdict executor=cdp version=0.9.1
```
