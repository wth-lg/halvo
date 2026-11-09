# researcher #4: Capture and permissions
STATUS: PASS
researcher | now: 2026-11-09 | stale cutoff: 2026-05-09 | lane: 4

## Summary

Scope note on OS versions: "current" macOS is read as macOS 26 (Tahoe) and "previous" as macOS 15 (Sequoia); Windows target is Windows 11. Whether a fall-2026 macOS release now supersedes Tahoe is an open item (see Unknowns). Every API below is a documented, supported path; undocumented and kernel-level paths are listed as rejected in Options.

**1. Capture APIs (macOS current+previous; Windows 11).**
- Screen: macOS uses ScreenCaptureKit (the supported current path; the older CGDisplayStream/CGWindowListCreateImage route is deprecated). Windows 11 uses Windows.Graphics.Capture (Windows.Graphics.Capture, modern, per-window or per-display) or the lower-level Desktop Duplication API (DXGI) for full-screen duplication. (findings #1, #8, #9)
- Keystroke + mouse: macOS uses Quartz Event Services (CGEventTap) for both keyboard and mouse events; it needs Accessibility or Input Monitoring. Windows uses SetWindowsHookEx (WH_KEYBOARD_LL / WH_MOUSE_LL) or Raw Input (RIDEV_INPUTSINK) for keyboard and mouse. (findings #4, #10, #11)
- Foreground window / what the user is doing: macOS reads NSWorkspace.frontmostApplication for the app, and the Accessibility API (AXUIElement) for window titles and in-app UI. Windows uses GetForegroundWindow plus UI Automation for element-level detail. (findings #5, #6, #12, #13)
- Installed-app enumeration: macOS enumerates /Applications and uses LaunchServices/system_profiler; Windows reads the Uninstall registry key (the documented inventory source; Win32_Product is discouraged for its side effects). (findings #16)
- System audio output: macOS uses Core Audio taps (the documented system-audio capture API; ScreenCaptureKit also carries audio). Windows uses WASAPI loopback recording. (findings #2, #14)
- Microphone + camera: macOS uses AVFoundation (AVCaptureDevice); Windows uses MediaCapture (and GraphicsCapturePicker triggers the screen-capture consent UI). (findings #3, #15, #17)

**2. Permission names and grant flow.**
- macOS panes (System Settings > Privacy & Security): "Screen & System Audio Recording" (renamed from "Screen Recording" in macOS Sonoma; audio and screen are one grant), "Accessibility", "Input Monitoring", "Microphone", "Camera". The app can open the exact pane with the `x-apple.systempreferences:` URL scheme and can trigger the OS consent prompt programmatically (e.g. CGRequestScreenCaptureAccess, AVCaptureDevice.requestAccess). macOS Sequoia re-prompts for screen recording monthly (weekly in early betas) and re-prompts after the app's code signature changes; an OS update can silently reset grants. (findings #7, #18, #19, #20, #24)
- Windows 11 (Settings > Privacy & security): Camera and Microphone are user-toggled per-app capability pages; keyboard/mouse hooks and screen capture are NOT gated by a Privacy page toggle (no OS consent dialog for SetWindowsHookEx or Desktop Duplication). The modern Windows.Graphics.Capture picker shows a consent/selection UI; packaged apps declare capabilities (webcam, microphone) in the manifest. (findings #21, #22, #23)

**3. What secure-input and protected modes block.**
- macOS EnableSecureEventInput (used automatically by NSSecureTextField/password fields) suppresses CGEventTap keystroke capture while a protected field has focus; Halvo therefore cannot see passwords typed into such fields, and must not defeat it. ScreenCaptureKit excludes windows marked protected and yields no content for DRM-protected media. (findings #1, #25)
- Windows: a window that calls SetWindowDisplayAffinity with WDA_EXCLUDEFROMCAPTURE is omitted from capture (older WDA_MONITOR shows a black rectangle); the UAC secure desktop and UIPI block a lower-privilege app from hooking a higher-privilege one. Hardware-DRM premium video decodes in a protected media path, so capture shows black frames / muted audio. All of this is excluded from Halvo's observation. (findings #26, #27, #28)

**4. Distribution / store / AV rules.**
- macOS: Apple notarization scans for malware and the Mac App Store review guidelines restrict keystroke logging and data collection; Gatekeeper blocks unsigned/unnotarized apps by default. A screen/keystroke recorder realistically ships Developer ID + notarized outside the Mac App Store, because App Store review is hostile to global keylogging. (findings #29, #30, #31)
- Windows: Microsoft Store policies restrict apps that capture user data without consent; SmartScreen warns on low-reputation unsigned downloads (mitigated by code-signing reputation). Keyboard hooking + screen recording is the exact behaviour EDR/AV flags as "keylogging" (MITRE ATT&CK T1056.001), so a shipping recorder needs signing, clear consent UX and often AV allow-listing. (findings #32, #33, #34)

**5. Least-friction onboarding, and how shipping recorders sequence prompts.**
- The supported low-friction pattern: install a signed/notarized app, then request each OS permission just-in-time at first use, deep-linking the user to the exact Settings pane, because macOS cannot grant these silently and Windows camera/mic need the per-app toggle. There is no fully silent grant path for a non-technical user; MDM can pre-approve some macOS permissions but NOT Screen Recording (per Hubstaff's silent-deploy docs). (findings #36)
- Shipping examples: RescueTime asks for Accessibility (window titles) + Automation (browser URLs); Hubstaff asks for Screen Recording (screenshots) + Automation + Accessibility and documents that macOS updates silently revoke Screen Recording; Timing uses Accessibility first and Screen Recording only as a fallback for window titles it cannot read; Loom documents Camera/Microphone/Screen-Recording toggles; CleanShot requests Accessibility + Screen Recording at setup (system audio is bundled into the screen grant). Screenpipe and ActivityWatch (open source) and Microsoft Recall (OS-level, Copilot+ only) are the always-on-capture references; Rewind, the closest prior art, had its capture disabled on 2025-12-19 after Meta acquired Limitless. (findings #35, #36, #37, #38, #39, #40, #41, #42, #43, #44, #45, #46, #47)

## Findings

| # | claim | source | type | date | primary? | confidence |
|---|-------|--------|------|------|----------|------------|
| 1 | ScreenCaptureKit: Apple's supported framework to capture screen, window and app audio on macOS; needs screen-recording authorization. | https://developer.apple.com/documentation/screencapturekit | docs | unknown | yes | high |
| 2 | Core Audio taps: documented macOS API to capture system audio output from other processes. | https://developer.apple.com/documentation/coreaudio/capturing-system-audio-with-core-audio-taps | docs | unknown | yes | high |
| 3 | AVCaptureDevice: AVFoundation type for microphone and camera capture and for requesting capture authorization on macOS. | https://developer.apple.com/documentation/avfoundation/avcapturedevice | docs | unknown | yes | high |
| 4 | Quartz Event Services: macOS CGEventTap API that intercepts keyboard and mouse events; requires Accessibility/Input Monitoring. | https://developer.apple.com/documentation/coregraphics/quartz-event-services | docs | unknown | yes | high |
| 5 | frontmostApplication: NSWorkspace property giving the current foreground app on macOS. | https://developer.apple.com/documentation/appkit/nsworkspace/frontmostapplication | docs | unknown | yes | high |
| 6 | AXUIElement: macOS Accessibility API header for reading window titles and in-app UI elements. | https://developer.apple.com/documentation/applicationservices/axuielement_h | docs | unknown | yes | high |
| 7 | CGRequestScreenCaptureAccess: macOS call that triggers the screen-recording permission prompt programmatically. | https://developer.apple.com/documentation/coregraphics/cgrequestscreencaptureaccess() | docs | unknown | yes | high |
| 8 | Windows.Graphics.Capture: modern Windows 11 API for per-window and per-display screen capture with a consent picker. | https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture | docs | 2017-02-08 | yes | high |
| 9 | Desktop Duplication API: DXGI API for full-screen frame duplication on Windows. | https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/desktop-dup-api | docs | 2018-05-31 | yes | high |
| 10 | SetWindowsHookEx: Windows API to install WH_KEYBOARD_LL / WH_MOUSE_LL hooks for keystroke and mouse capture. | https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowshookexw | docs | 2022-06-01 | yes | high |
| 11 | Raw Input: Windows input API (RIDEV_INPUTSINK) for background keyboard and mouse data. | https://learn.microsoft.com/en-us/windows/win32/inputdev/about-raw-input | docs | 2025-07-14 | yes | high |
| 12 | GetForegroundWindow: Windows API returning the handle of the current foreground window. | https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getforegroundwindow | docs | 2018-12-05 | yes | high |
| 13 | UI Automation: Windows accessibility framework for reading element-level detail of the active app. | https://learn.microsoft.com/en-us/windows/win32/winauto/entry-uiauto-win32 | docs | 2025-07-14 | yes | high |
| 14 | loopback recording: WASAPI feature to capture the system audio output stream on Windows. | https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording | docs | 2025-04-16 | yes | high |
| 15 | MediaCapture: Windows API for microphone and camera capture. | https://learn.microsoft.com/en-us/uwp/api/windows.media.capture.mediacapture | docs | 2017-02-08 | yes | high |
| 16 | Uninstall Registry Key: documented Windows location that lists installed software for inventory. | https://learn.microsoft.com/en-us/windows/win32/msi/uninstall-registry-key | docs | 2018-05-31 | yes | high |
| 17 | GraphicsCapturePicker: Windows UI that prompts the user to consent to and choose a capture source. | https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.graphicscapturepicker | docs | 2017-02-08 | yes | high |
| 18 | System Audio Recording: macOS Privacy & Security pane ("Screen & System Audio Recording") that controls screen and system-audio capture per app. | https://support.apple.com/guide/mac-help/mchld6aa7d23/mac | docs | unknown | yes | high |
| 19 | Accessibility: macOS Privacy & Security pane granting an app control and read access used for window titles and automation. | https://support.apple.com/guide/mac-help/mh43185/mac | docs | unknown | yes | high |
| 20 | Input Monitoring: macOS Privacy & Security pane that must be granted before an app can monitor keyboard/pointer input. | https://support.apple.com/guide/mac-help/control-access-to-input-monitoring-on-mac-mchl4cedafb6/mac | docs | unknown | yes | high |
| 21 | camera in Windows: Windows 11 Settings > Privacy & security page where the user toggles per-app camera access. | https://support.microsoft.com/en-us/windows/manage-app-permissions-for-your-camera-in-windows-87ebc757-1f87-7bbf-84b5-0686afb6ca6b | docs | 2026-03-30 | yes | high |
| 22 | microphone in Windows: Windows 11 Settings > Privacy & security page where the user toggles per-app microphone access. | https://support.microsoft.com/en-us/windows/turn-on-app-permissions-for-your-microphone-in-windows-94991183-f69d-b4cf-4679-c98ca45f577a | docs | 2026-07-29 | yes | high |
| 23 | app capability declarations: Windows manifest capabilities (webcam, microphone) a packaged app must declare for those devices. | https://learn.microsoft.com/en-us/windows/uwp/packaging/app-capability-declarations | docs | 2026-09-08 | yes | high |
| 24 | Screen Recording: macOS Sequoia re-prompts users to reauthorize screen-recording apps monthly (weekly in early betas). | https://www.macrumors.com/2024/08/15/macos-sequoia-screen-recording-app-permissions/ | news | 2024-08-15 | no | med |
| 25 | Secure Event Input: Apple technote describing EnableSecureEventInput, which blocks event taps from reading keystrokes in protected fields. | https://leopard-adc.pepas.com/technotes/tn2007/tn2150.html | technote | unknown | no | med |
| 26 | WDA_EXCLUDEFROMCAPTURE: SetWindowDisplayAffinity flag that excludes a window from screen capture on Windows. | https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowdisplayaffinity | docs | 2018-12-05 | yes | high |
| 27 | WDA_EXCLUDEFROMCAPTURE: available from Windows 10 build 2004 and Windows 11; older WDA_MONITOR shows a black rectangle instead. | https://blogs.windows.com/windowsdeveloper/2019/09/16/new-ways-to-do-screen-capture/ | vendor blog | 2019-09-16 | yes | high |
| 28 | WDA_EXCLUDEFROMCAPTURE: analysis noting hardware DRM decodes in a protected media path that user/kernel capture cannot read, so capture shows black. | https://www.ioactive.com/the-drm-flag-that-isnt-drm/ | analysis | unknown | no | med |
| 29 | Notarizing: Apple notarization scans macOS software for malicious content before distribution outside the App Store. | https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution | docs | unknown | yes | high |
| 30 | Gatekeeper: macOS blocks apps that are not signed and notarized by default. | https://support.apple.com/guide/security/gatekeeper-and-runtime-protection-sec5599b66df/web | docs | unknown | yes | high |
| 31 | App Review Guidelines: Apple's App Store rules restrict apps that record keystrokes or collect data without consent. | https://developer.apple.com/app-store/review/guidelines/ | docs | unknown | yes | high |
| 32 | Store Policies: Microsoft Store policy set restricting apps that capture or transmit user data without clear consent. | https://learn.microsoft.com/en-us/windows/apps/publish/store-policies | docs | 2026-09-14 | yes | high |
| 33 | SmartScreen: Microsoft Defender SmartScreen warns users before running low-reputation or unsigned downloaded apps. | https://learn.microsoft.com/en-us/windows/security/operating-system-security/virus-and-threat-protection/microsoft-defender-smartscreen/ | docs | 2026-04-23 | yes | high |
| 34 | Keylogging: MITRE ATT&CK catalogs keyboard-input capture (T1056.001) as a detected adversary technique, which EDR/AV flag. | https://attack.mitre.org/techniques/T1056/001/ | reference | unknown | no | med |
| 35 | RescueTime: requires macOS Accessibility (window titles) and Automation (browser URLs) permissions, re-granted after macOS updates. | https://help.rescuetime.com/article/59-how-do-i-enable-accessibility-permissions-on-mac-osx | docs | unknown | yes | high |
| 36 | Hubstaff: documents that macOS Screen Recording must be approved manually per device and cannot be pre-approved via MDM, and updates can silently revoke it. | https://support.hubstaff.com/hubstaff-silent-app-setup-macos/ | docs | unknown | yes | high |
| 37 | Hubstaff: screenshot capture on macOS 10.15+ requires enabling the app under Screen and System Audio Recording in Privacy & Security. | https://support.hubstaff.com/how-to-give-hubstaff-screen-capture-permissions-on-macos/ | docs | unknown | yes | high |
| 38 | Timing: uses the Accessibility API for window titles and requests Screen Recording only as a fallback (CGWindow API) for titles it cannot read. | https://timingapp.com/help/faq | docs | unknown | yes | med |
| 39 | Screen Recording: Loom documents enabling Camera, Microphone and Screen Recording for its app in macOS Privacy settings. | https://support.loom.com/hc/en-us/articles/360018605337-Camera-Issues-on-Mac-How-to-enable-your-privacy-settings-for-Loom | docs | unknown | yes | med |
| 40 | CleanShot: setup requests Accessibility and Screen Recording, with system audio bundled into the single screen-recording grant. | https://www.heise.de/en/news/Screen-recording-warning-Apple-probably-wants-to-disturb-less-often-9835885.html | news | unknown | no | med |
| 41 | screenpipe: open-source always-on local recorder of screen and audio for macOS, Windows and Linux. | https://github.com/mediar-ai/screenpipe | repo | unknown | yes | high |
| 42 | ActivityWatch: open-source automatic time tracker logging active window and app usage cross-platform. | https://github.com/ActivityWatch/activitywatch | repo | unknown | yes | high |
| 43 | Recall: Microsoft Windows feature that periodically snapshots the screen, restricted to Copilot+ PCs and gated by Windows Hello. | https://support.microsoft.com/en-us/windows/retrace-your-steps-with-recall-aa03f8a0-a78b-4b3e-b0a1-2eb8ac48701c | docs | 2025-10-10 | yes | high |
| 44 | Recall: Microsoft documents developer APIs to integrate app content with Recall snapshots on Windows. | https://learn.microsoft.com/en-us/windows/ai/apis/recall | docs | 2025-12-01 | yes | high |
| 45 | Rewind: Meta's acquisition of Limitless led to Rewind's screen/audio capture being disabled on 2025-12-19. | https://winbuzzer.com/2025/12/05/meta-acquires-ai-wearables-startup-limitless-kills-pendant-sales-and-sunsets-rewind-app-xcxwbn/ | news | 2025-12-05 | no | med |
| 46 | Limitless: markets an always-on personal AI that records and transcribes what you hear and see. | https://www.limitless.ai/ | vendor | unknown | yes | med |
| 47 | CleanShot: commercial macOS screen capture and recording app (prior art for the capture-permission flow). | https://cleanshot.com/ | vendor | unknown | yes | med |

## Options for the planner

The lane ranks capture-API options per modality (supported, documented, macOS+Windows only; the app never defeats secure input). Health lines use repo/package signals where an option is a library; the OS APIs are vendor platform APIs, so their "health" is the owning platform.

**Screen capture**
1. macOS ScreenCaptureKit + Windows.Graphics.Capture (RANK 1). Both vendor-current, hardware-accelerated, respect protected-content exclusion and the OS consent UI. Health: Apple platform API (supported on macOS 12.3+); Microsoft platform API (Windows 10 1803+/Windows 11), ms.date 2017-02-08 on the reference, actively documented.
2. Windows Desktop Duplication API (RANK 2, Windows fallback). Full-screen duplication, lower-level; use when per-window capture is not needed. Health: Microsoft platform API, ms.date 2018-05-31.

**Keystroke + mouse**
1. macOS Quartz Event Services (CGEventTap) + Windows Raw Input (RANK 1). Documented, respects EnableSecureEventInput on macOS. Health: Apple platform API; Microsoft platform API, ms.date 2025-07-14 (recently maintained).
2. Windows SetWindowsHookEx low-level hooks (RANK 2). Documented but the classic keylogger pattern that AV/EDR flag hardest; prefer Raw Input where possible. Health: Microsoft platform API, ms.date 2022-06-01.

**Foreground app + installed software**
1. macOS NSWorkspace + Accessibility (AXUIElement); Windows GetForegroundWindow + UI Automation; installed apps via LaunchServices (macOS) and the Uninstall registry key (Windows) (RANK 1). All documented. Health: vendor platform APIs, references dated 2017-2025.

**System audio output**
1. macOS Core Audio taps + Windows WASAPI loopback (RANK 1). Both documented. Health: Apple platform API (macOS 14.2+ for Core Audio taps); Microsoft platform API, ms.date 2025-04-16.

**Microphone + camera**
1. macOS AVFoundation (AVCaptureDevice) + Windows MediaCapture (RANK 1). Documented, trigger the OS consent prompt. Health: vendor platform APIs.

**Rejected (fail a hard requirement)**
- Kernel extensions / IOKit keyboard drivers (macOS) and kernel-mode keyboard filter drivers (Windows): rejected, not a supported user-space path for a notarized consumer app, flagged by AV/EDR, and raises the notarization/signing bar sharply.
- Defeating EnableSecureEventInput or capturing password fields: rejected, violates the lane's hard requirement that the app never defeats secure input.
- Deprecated macOS CGDisplayStream / CGWindowListCreateImage for ongoing capture: rejected in favor of ScreenCaptureKit, which Apple now requires for new screen capture.
- Win32_Product for installed-app inventory: rejected; Microsoft documents it as having side effects (triggers MSI reconfiguration); use the Uninstall registry key.

## Unknowns

- Current macOS major version: this report treats macOS 26 (Tahoe) as current and macOS 15 (Sequoia) as previous. Whether a fall-2026 macOS release has shipped and changed any pane name or re-prompt cadence is unconfirmed from a dated primary source within the window. Settle by reading Apple's current "What's new" and the live Privacy & Security help page.
- Exact macOS Sequoia screen-recording re-prompt interval in the shipping release (vs. betas): reported as monthly by secondary news (row 24); Apple has no single dated primary page stating the final cadence. Settle with Apple Platform Security or release notes.
- Whether Windows 11 adds any OS consent gate for keyboard hooking or Desktop Duplication in a 2025-2026 update: no evidence found that it does; settle by re-checking the input and capture docs' change history.
- Exact publish/modified dates for Apple developer-doc pages and the TN2150 technote: Apple pages render dates client-side, so dates are marked unknown.

## Stale flags

- Rewind (row 45): the closest prior-art always-on recorder had capture disabled 2025-12-19 after Meta acquired Limitless; it is no longer a live product to benchmark grant flows against. Screenpipe, ActivityWatch and Microsoft Recall are the current references instead.
- TN2150 / EnableSecureEventInput (row 25): the live Apple developer reference page for the Carbon function returns 404; the surviving citable copy is an archived technote mirror. The behavior is still current (NSSecureTextField uses it), but the primary URL is gone.
- macOS screen-recording source (row 24) is 2024-08-15 news, older than the 2026-05-09 cutoff; kept because no dated primary states the final cadence and it is the clearest public account. Flagged as stale.
- AppleInsider and rewind.ai and neowin.net returned 403 to curl and were replaced with 2xx sources (MacRumors, winbuzzer); no stale content retained from them.

## Changes

New report. No prior researcher-4.md existed. No rows superseded except that 403-blocked candidate URLs were swapped for 2xx-returning equivalents before writing (noted in Stale flags and Issues).

## Evidence

All under ~/.wstack/runs/20261108-halvo-kickoff/evidence/researcher-4/:
- pages/*.html: raw fetched source pages (t1-t19, u1-u8, v1-v8, w1-w8, x1-x8, y1-y3, z1-z5), one per candidate URL, saved during pre-verification.
- url-check.sh: re-runnable checker that curls every findings-row URL and greps the row subject; writes url-check.txt.
- url-check.txt: per-row HTTP code and grep result (the PASS evidence for the Done-when url check).
- verify.txt: outputs of the Done-when head/grep/awk commands.

## Assumptions

- "Current/previous macOS" = macOS 26 Tahoe / macOS 15 Sequoia; "Windows" = Windows 11 (brief names Windows 11). Recorded because the brief says "current and previous major version" without pinning numbers.
- Doc pages without a machine-readable date are dated `unknown` rather than guessed (working rule 11).
- Subject strings in the claim column are chosen to appear verbatim (case-insensitive) in each source page; `&` in a pane name was replaced by a substring the page actually contains (e.g. "System Audio Recording").
- Primary? = yes for vendor/official docs and vendors' own support pages; no for third-party news, blogs and MITRE.

## Issues and risks

- No row relies on a login-gated or non-2xx URL. During research, rewind.ai, appleinsider.com and neowin.net returned HTTP 403 to curl (bot protection); none are cited. The Rewind-shutdown fact is instead cited to winbuzzer (row 45, 200), and the Sequoia re-prompt to MacRumors (row 24, 200).
- Several key capture behaviors (secure input, DRM black frames) are best documented by vendor-adjacent analysis (rows 25, 28) rather than a single clean primary page; confidence marked med and the underlying platform behavior cross-checked against the Microsoft SetWindowDisplayAffinity primary (row 26).
- EDR/AV detection names: this lane cites the technique taxonomy (MITRE T1056.001, row 34) rather than a specific vendor signature name, because signature names are vendor-internal and not reliably published; a deeper dive would need per-vendor documentation.

## Learnings

- Apple developer documentation pages serve the API name in the raw HTML (title/meta) even though availability strings render client-side, so they curl+grep cleanly for the subject but not for version numbers.
- Microsoft Learn pages expose a reliable `ms.date` meta tag, giving real YYYY-MM-DD dates; Apple pages do not, so those stay `unknown`.
- The single hardest constraint for Halvo is not any missing API but macOS re-prompting (monthly screen recording, reset on signature change or OS update) plus the no-silent-grant rule: a non-technical user must be walked to System Settings panes repeatedly, and MDM cannot pre-grant Screen Recording.

## Follow-up

Added `(findings #n)` citations to every `## Summary` bullet so each statement traces to its backing rows; all five sub-questions now carry at least one row citation. No other section changed: the title line, the `now:` / `stale cutoff:` line, the Findings table and all later sections are byte-identical to before, and STATUS stays PASS (every sub-question has a backing row). The before/after diff is saved at evidence/researcher-4/summary-citations.diff and touches only lines between `## Summary` and `## Findings`.
