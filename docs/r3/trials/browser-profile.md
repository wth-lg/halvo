# Browser profile trial

Unit U0.3 of the agent-setup run tests the privacy model of the agent browser (contract A13.3,
A8.3). A run's browser starts with an empty profile and a temporary directory of its own, and the
run removes both at its end. A sign-in that passed a second factor reaches the next run only as an
exported state. This first pull request holds items 1, 2 and 4. Item 3 (egress) and the `flag`
lines come with the second, so the verdict ends with `egress=open`.
`tools/r3-trials/profile/README.md` says how each item runs and holds the longer notes.

What ran. At commit 03f4c53 items 1 and 2 ran once per cell, on `mac` with Chrome and on `hosted-mac`
and `hosted-win` with Chrome and Edge in hosted run 42976989121. The task shape of item 4 ran at 03f4c53 on
`mac` with Chrome through the timed-run helper (2026-11-11T09:11:20Z to 2026-11-11T09:13:12Z) and on `hosted-win` with
Chrome and Edge in run 42976989121. The steps shape ran at 85f255a, whose one change of code makes the
program of the nine steps remove its browsers' directories: on `mac` through the helper
(2026-11-11T11:56:17Z to 2026-11-11T11:57:19Z) and on `hosted-win` in run 42995011332. Each hosted run is of a scratch
branch with its commit plus one commit on the workflow file. The `--browser` paths are `/Applications/Google Chrome.app/Contents/MacOS/Google Chrome`
and `/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge` on the Macs, and
`C:/Program Files/Google/Chrome/Application/chrome.exe` and
`C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe` on `hosted-win`.

Choices that the contract leaves. Item 2 runs item 1 first, so one run gives the three lines of a
cell. A landing on `/bill` is `import=ok second_factor_asked_again=no`, one on `/verify` is
`second_factor_asked_again=yes`, and any other is `import=miss`. Origin storage is localStorage.
`marker_hits` counts the marker in three encodings, the name of the cookie whose value is the marker
(the browser keeps that value encrypted), and one for each profile or temporary directory left on
disk. The task shape of item 4 signs in, passes `/verify`, opens `/bill` and `/pay`, and holds that
page for 20 s. Both shapes also measure the trial program and the fixture server.

What the lines cannot show. The state carries cookies and localStorage only, not sessionStorage,
IndexedDB or the HTTP cache, and the fixture uses none of them. Each cell of items 1 and 2 is one
run. The search matches raw bytes, so it cannot see the marker in compressed bytes. On macOS every
start adds the mock keychain, so `prompt=no` cannot show what the product's browser asks of the
keychain. Chrome and Edge on macOS keep their singleton directory in the system's temporary directory
although `TMPDIR` names the run's own (on Windows both write into the run's own), so the search of
the system's temporary directory stays. On `hosted-win` the search read no file after the removal,
so `marker_hits=0` rests there on the removal. On macOS `tree-peak` misses a peak shorter than its
50 ms period.

```text
licence item=libc@0.2.191 spdx=MIT read=2026-11-11 ok=yes
licence item=windows-sys@0.61.2 spdx=MIT read=2026-11-11 ok=yes
hosted machine=hosted-mac run=42976989121 sha=6e1ade82f1c5db019bbb687e955adfb28f964b81
hosted machine=hosted-win run=42976989121 sha=6e1ade82f1c5db019bbb687e955adfb28f964b81
hosted machine=hosted-win run=42995011332 sha=6a13d1ca27dd30d403b112e06a474a0e84d109bc
session machine=mac browser=chrome import=ok second_factor_asked_again=no
session machine=hosted-mac browser=chrome import=ok second_factor_asked_again=no
session machine=hosted-mac browser=edge import=ok second_factor_asked_again=no
session machine=hosted-win browser=chrome import=ok second_factor_asked_again=no
session machine=hosted-win browser=edge import=ok second_factor_asked_again=no
keychain machine=mac browser=chrome prompt=no
keychain machine=hosted-mac browser=chrome prompt=no
keychain machine=hosted-mac browser=edge prompt=no
keychain machine=hosted-win browser=chrome prompt=no
keychain machine=hosted-win browser=edge prompt=no
disk machine=mac browser=chrome marker_hits=0
disk machine=hosted-mac browser=chrome marker_hits=0
disk machine=hosted-mac browser=edge marker_hits=0
disk machine=hosted-win browser=chrome marker_hits=0
disk machine=hosted-win browser=edge marker_hits=0
browser machine=mac browser=chrome shape=steps run=1 peak_bytes=1003197512
browser machine=mac browser=chrome shape=steps run=2 peak_bytes=987025064
browser machine=mac browser=chrome shape=steps run=3 peak_bytes=967904288
browser machine=mac browser=chrome shape=steps run=4 peak_bytes=969575432
browser machine=mac browser=chrome shape=steps run=5 peak_bytes=951979016
browser machine=mac browser=chrome shape=task run=1 peak_bytes=969936600
browser machine=mac browser=chrome shape=task run=2 peak_bytes=965299592
browser machine=mac browser=chrome shape=task run=3 peak_bytes=973540768
browser machine=mac browser=chrome shape=task run=4 peak_bytes=971787872
browser machine=mac browser=chrome shape=task run=5 peak_bytes=977227432
browser machine=hosted-win browser=chrome shape=steps run=1 peak_bytes=333254656
browser machine=hosted-win browser=chrome shape=steps run=2 peak_bytes=323710976
browser machine=hosted-win browser=chrome shape=steps run=3 peak_bytes=324091904
browser machine=hosted-win browser=chrome shape=steps run=4 peak_bytes=328142848
browser machine=hosted-win browser=chrome shape=steps run=5 peak_bytes=328278016
browser machine=hosted-win browser=chrome shape=task run=1 peak_bytes=328699904
browser machine=hosted-win browser=chrome shape=task run=2 peak_bytes=327753728
browser machine=hosted-win browser=chrome shape=task run=3 peak_bytes=320552960
browser machine=hosted-win browser=chrome shape=task run=4 peak_bytes=319766528
browser machine=hosted-win browser=chrome shape=task run=5 peak_bytes=321466368
browser machine=hosted-win browser=edge shape=steps run=1 peak_bytes=381464576
browser machine=hosted-win browser=edge shape=steps run=2 peak_bytes=376107008
browser machine=hosted-win browser=edge shape=steps run=3 peak_bytes=359645184
browser machine=hosted-win browser=edge shape=steps run=4 peak_bytes=370241536
browser machine=hosted-win browser=edge shape=steps run=5 peak_bytes=362643456
browser machine=hosted-win browser=edge shape=task run=1 peak_bytes=367390720
browser machine=hosted-win browser=edge shape=task run=2 peak_bytes=369782784
browser machine=hosted-win browser=edge shape=task run=3 peak_bytes=360443904
browser machine=hosted-win browser=edge shape=task run=4 peak_bytes=365957120
browser machine=hosted-win browser=edge shape=task run=5 peak_bytes=370835456
browser machine=hosted-win bound_bytes=469762048
browser machine=mac bound_bytes=1216348160
verdict profile=ephemeral egress=open browser_bound_bytes=1216348160
```

## Second round (U0.3's second pull request)

Items 1, 2 and 4 ran again under the start that the product carries after U0.7c, and item 3 ran.
Each browser of items 1 and 2 and of item 4's task shape starts on its debugging pipe, with the
agent's profile settings and the same-site setting written before the start, the first flag set of
A13.2, U0.7b's flags for the ways out and the relay's flags, the relay its only route; the steps
shape runs the web trial's start of U0.7c, which writes no same-site setting (README). The `flag`
and `pref` lines are that set, the same-site origin given for the gate's port 18090 (each cell wrote
its own); item 3 added no flag. Flags of the trial that the product does not carry: the mock
keychain on every macOS start (the product's browser uses the system's key store, A8.3), the port
flags of the web trial's port-form controls, and `--isolate-origins` of A13.7's rows (CP-93 (5)).

What ran. The trial's code is 33393f00's; 3fc451d8 changes a lint line of the hosted harness, and
e54e2bf2 and later this file, the README, CHANGELOG.md and a test. On `mac` items 1 and 2
(d66953a4, the same code for them) and item 3 (33393f00 with CHANGELOG.md and the README changed;
d66953a4's clean call printed the same line) ran with Chrome on the helper's lane on 2026-11-14,
01:18Z to 02:37Z. Every hosted cell ran in run 43547292129 (scratch head 33393f00 plus the
workflow). The `mac` cell of item 4 is owed: memory pressure read 2 at each try and the brief takes a
peak only at 1, so the bound stays `open` and no `browser machine=mac` line stands.

Item 3 ran the trial program under `sh scripts/egress/check.sh` with the list of A14.3: the fixture
and one Chrome that signs in, opens `/bill` and `/pay` and holds that page for 20 s, direct and
without the relay, which is the executor's and outside the check's tree; the relay's flags that
close a way out stay. No `--no-sandbox` is given: under the `sandbox` mode Chrome does not start on
`mac` or `hosted-mac`. With `--sandbox-off`, a control and no cell, it starts and the check logs 3
denials on each Mac (on `mac` UDP to port 443 with no address). On `hosted-win` the first flag set
gave `denied=4` and the final set `denied=0`; the check there gets an LF copy of the list (README).

Beside `denied=0` (W-C26.8 item 5): on hosted-win, denied=0 proves that no process of the wrapped
tree opened a connection, sent a datagram of any IP protocol or resolved a name outside the list
while the command ran on windows-2025; not verified: ipv6, wmi, task, bits, and what a process did
after the command ended. A connection that the mode stops is stopped whether or not its report
arrives; denied=0 says that no report of a denial arrived, and 1 report in 494 runs of one denied
probe was lost (reports/verifier-13.md, claim 3), the workflow run's report, measured on this Mac.
On macos-15 and this-mac the browser did not start, so `denied=0` there proves nothing about it.

The token is `windows-only`; the miss on `hosted-mac` is request 7 of plan.md. Each cell of items 1
to 3 is one run, and the first round's limits of the search stand.

```text
licence item=libc@0.2.191 spdx=MIT read=2026-11-11 ok=yes
licence item=windows-sys@0.61.2 spdx=MIT read=2026-11-11 ok=yes
hosted machine=hosted-mac run=43547292129 sha=c8f9321b0d37e184f73fd6bd865f1f2876711c63
hosted machine=hosted-win run=43547292129 sha=c8f9321b0d37e184f73fd6bd865f1f2876711c63
session machine=mac browser=chrome import=ok second_factor_asked_again=no
session machine=hosted-mac browser=chrome import=ok second_factor_asked_again=no
session machine=hosted-mac browser=edge import=ok second_factor_asked_again=no
session machine=hosted-win browser=chrome import=ok second_factor_asked_again=no
session machine=hosted-win browser=edge import=ok second_factor_asked_again=no
keychain machine=mac browser=chrome prompt=no
keychain machine=hosted-mac browser=chrome prompt=no
keychain machine=hosted-mac browser=edge prompt=no
keychain machine=hosted-win browser=chrome prompt=no
keychain machine=hosted-win browser=edge prompt=no
disk machine=mac browser=chrome marker_hits=0
disk machine=hosted-mac browser=chrome marker_hits=0
disk machine=hosted-mac browser=edge marker_hits=0
disk machine=hosted-win browser=chrome marker_hits=0
disk machine=hosted-win browser=edge marker_hits=0
egress machine=mac mode=sandbox browser_started=no denied=0
egress machine=hosted-mac mode=sandbox browser_started=no denied=0
egress machine=hosted-win mode=wfp browser_started=yes denied=0
browser machine=hosted-win browser=chrome shape=steps run=1 peak_bytes=216453120
browser machine=hosted-win browser=chrome shape=steps run=2 peak_bytes=218685440
browser machine=hosted-win browser=chrome shape=steps run=3 peak_bytes=219860992
browser machine=hosted-win browser=chrome shape=steps run=4 peak_bytes=222932992
browser machine=hosted-win browser=chrome shape=steps run=5 peak_bytes=219295744
browser machine=hosted-win browser=chrome shape=task run=1 peak_bytes=233033728
browser machine=hosted-win browser=chrome shape=task run=2 peak_bytes=234663936
browser machine=hosted-win browser=chrome shape=task run=3 peak_bytes=230985728
browser machine=hosted-win browser=chrome shape=task run=4 peak_bytes=234323968
browser machine=hosted-win browser=chrome shape=task run=5 peak_bytes=235151360
browser machine=hosted-win browser=edge shape=steps run=1 peak_bytes=181817344
browser machine=hosted-win browser=edge shape=steps run=2 peak_bytes=182829056
browser machine=hosted-win browser=edge shape=steps run=3 peak_bytes=183750656
browser machine=hosted-win browser=edge shape=steps run=4 peak_bytes=181202944
browser machine=hosted-win browser=edge shape=steps run=5 peak_bytes=183144448
browser machine=hosted-win browser=edge shape=task run=1 peak_bytes=218705920
browser machine=hosted-win browser=edge shape=task run=2 peak_bytes=214028288
browser machine=hosted-win browser=edge shape=task run=3 peak_bytes=217288704
browser machine=hosted-win browser=edge shape=task run=4 peak_bytes=209104896
browser machine=hosted-win browser=edge shape=task run=5 peak_bytes=212201472
browser machine=hosted-win bound_bytes=293601280
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
flag --disable-updater-scheduler
flag --dns-prefetch-disable
flag --no-pings
flag --disable-extensions
flag --disable-component-extensions-with-background-pages
flag --disable-features=FetchLaterAPI,MediaRouter,DialMediaRouteProvider,CastMediaRouteProvider,SignedHTTPExchange,SignedExchangeSubresourcePrefetch,Prerender2,PrefetchProxy,NetworkErrorLogging,Reporting,DocumentReporting,FedCm,PaymentHandler,WebAuthenticationCable,AttributionReporting,PrivacySandboxAdsAPIs,InterestFeedContentSuggestions,OptimizationHints,Translate,WebRtcHideLocalIpsWithMdns
flag --disable-quic
flag --force-webrtc-ip-handling-policy=disable_non_proxied_udp
pref browser.enabled_labs_experiments=["isolate-origins"]
pref browser.enabled_labs_experiments_origin_lists.isolate-origins=http://pay.agent.test:18090
pref dns_over_https.mode=off
pref dns_over_https.templates=
pref autofill.credit_card_enabled=false
pref autofill.profile_enabled=false
pref browser.enable_spellchecking=false
pref credentials_enable_service=false
pref dns_prefetching.enabled=false
pref net.network_prediction_options=2
pref payments.can_make_payment_enabled=false
pref profile.default_content_setting_values.ar=2
pref profile.default_content_setting_values.automatic_downloads=2
pref profile.default_content_setting_values.bluetooth_guard=2
pref profile.default_content_setting_values.bluetooth_scanning=2
pref profile.default_content_setting_values.clipboard=2
pref profile.default_content_setting_values.file_system_read_guard=2
pref profile.default_content_setting_values.file_system_write_guard=2
pref profile.default_content_setting_values.geolocation=2
pref profile.default_content_setting_values.hid_guard=2
pref profile.default_content_setting_values.idle_detection=2
pref profile.default_content_setting_values.local_fonts=2
pref profile.default_content_setting_values.media_stream_camera=2
pref profile.default_content_setting_values.media_stream_mic=2
pref profile.default_content_setting_values.midi_sysex=2
pref profile.default_content_setting_values.nfc=2
pref profile.default_content_setting_values.notifications=2
pref profile.default_content_setting_values.payment_handler=2
pref profile.default_content_setting_values.protocol_handlers=2
pref profile.default_content_setting_values.sensors=2
pref profile.default_content_setting_values.serial_guard=2
pref profile.default_content_setting_values.storage_access=2
pref profile.default_content_setting_values.usb_guard=2
pref profile.default_content_setting_values.vr=2
pref profile.default_content_setting_values.window_placement=2
pref profile.password_manager_enabled=false
pref safebrowsing.enabled=false
pref search.suggest_enabled=false
pref web_package.signed_exchange.enabled=false
pref webrtc.ip_handling_policy=disable_non_proxied_udp
pref webrtc.multiple_routes_enabled=false
pref webrtc.nonproxied_udp_enabled=false
verdict profile=ephemeral egress=windows-only browser_bound_bytes=open
```
