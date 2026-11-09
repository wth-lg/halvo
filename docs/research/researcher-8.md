# researcher #8: Desktop frameworks
STATUS: ISSUES
now: 2026-11-09    stale cutoff: 2026-05-09    lane: 8

## Summary
- Frameworks (sub-question 1). Ten frameworks meet the hard requirements on paper: Tauri 2, Electron, GPUI, Compose Multiplatform, Flutter, .NET MAUI, Avalonia, Wails 3, React Native for Windows and macOS, and Qt 6 under LGPLv3. All have MIT, Apache-2.0, BSD-3 or LGPL licenses, so none blocks a later MIT release of the app code. Native access is Rust crates for Tauri and GPUI, Node-API addons for Electron (napi-rs for Rust), dart:ffi for Flutter, C# platform code for MAUI and Avalonia, the JVM Foreign Function and Memory API for Compose, cgo for Wails, and native modules for React Native. Two shipping capture apps matter most. screenpipe records screen and audio 24/7 on macOS and Windows with a Tauri shell over Rust capture crates, but its license is a source-available commercial license (findings #4, #11). Cap, a screen recorder, shipped on Tauri until 0.6.0 and in October 2026 moved its desktop app to GPUI, citing lost clicks in web-view overlays and display-picker crashes, then deleted the Tauri app from main (#1, #2, #21). Bundle size: Tauri's site says "as small as 600 KB" (self-reported, setup not stated, #43). No official Electron, Flutter or Compose figure was found; third-party comparisons disagree widely and are not cited.
- Build commands, paths, install, runners (sub-question 2). Tauri: `tauri build` writes installers under `src-tauri/target/release/bundle/` (the docs show `release/bundle/nsis/` for Windows; the `.app` directory on macOS is documented by structure, not by path, #13, #28); rustup installs with one user-level curl command, but macOS also needs Xcode Command Line Tools (#28, #44). Electron Forge: `package` writes the `.app` or `.exe` folder to `/out/`, `make` writes installers to `/out/make/`, and Node installs user-level with fnm (#38, #37). Flutter: `flutter build windows` writes to `build\windows\runner\Release`; the macOS `.app` path is not stated in the pages fetched (#39, #46); the SDK is an archive you extract, but desktop builds need Xcode and Visual Studio (#33, #45). .NET MAUI: `dotnet publish -f net8.0-maccatalyst -c Release -p:CreatePackage=false` produces an unsigned `.app`, and `dotnet publish -f net8.0-windows10.0.19041.0 -c Release -p:RuntimeIdentifierOverride=win10-x64 -p:WindowsPackageType=None` produces an unpackaged exe under `bin\Release\...\win10-x64\publish` (#29, #35); the install page shows `sudo dotnet workload install maui` on macOS, although dotnet-install.sh itself needs no admin rights (#32, #36). Compose: `packageDmg` and `packageMsi` run jpackage into `build/compose/binaries`, each format only on its own OS (#41). Wails: `wails build -platform darwin|windows` writes to `build/bin` (#20). GitHub's macos-15 and windows-2025 images ship Rust, Node.js, .NET SDKs, Xcode and Visual Studio; Flutter is not preinstalled (#7, #24). Whether each framework's default CI build is unsigned is documented only for MAUI (publish-unsigned page) and implied for Forge and Tauri, whose signing is an opt-in configuration (#29, #40, #13). See Unknowns.
- Release tooling (sub-question 3). Tools with a documented dry run that prints the next version: semantic-release `--dry-run` prints the next version and release notes (#42), knope `--dry-run` prints what each workflow step would do and `--override-version` forces a version (#49), git-cliff `--bumped-version` prints the next version and `--bump --unreleased` renders the section (#47, example config for Keep a Changelog at #25), cargo-release runs in dry-run mode by default and needs `--execute` to act (#26), release-please `--dry-run` "Prepare but do not take action" (#27), changesets `changeset status` prints the release plan without writing (#30). Fit to Keep a Changelog with an Unreleased section: git-cliff ships a keepachangelog.toml example; knope writes `## <version> (<date>)` headings per version; semantic-release and release-please generate Conventional Commits sections, not Keep a Changelog; cargo-release edits CHANGELOG.md through `pre-release-replacements` and is Rust-only. tauri-action and electron-builder publish artifacts to GitHub Releases and are not version or changelog tools (#8, #16).
- CI patterns (sub-question 4). Tauri's official GitHub pipeline page uses a matrix over macos-latest and windows-latest with tauri-action and a setup-node cache, so a two-literal-job workflow is a straightforward rewrite of that example, not a documented pattern (#51, #8). Caching: actions/setup-node `cache` input (official, #23), Swatinem/rust-cache for Cargo (community, LGPL-3.0 action, #22), subosito/flutter-action `cache` input (community, last push before the cutoff, #34). Artifacts: actions/upload-artifact v7 (#19). actionlint checks the YAML (#31). No framework publishes a no-matrix two-job example; the planner should take the matrix example and expand it by hand.
- Backend placement (sub-question 5). Both layouts exist in each ecosystem. Same repo: Cap (Turborepo; `apps/desktop-gpui` next to a Next.js web app and shared Rust crates, #3) and Zed (Rust editor plus `Dockerfile-collab` for its collaboration server in one repo, #6). Separate repos: Bitwarden (`clients` monorepo for web, browser, desktop Electron and CLI, `server` for the API, #9, #10) and AppFlowy (Flutter plus Rust app repo, AppFlowy-Cloud backend repo, #14, #15). A Cargo workspace or a Turborepo with `apps/` and `crates/` directories leaves both doors open.

## Findings
| # | claim | source | type | date | primary? | confidence |
|---|---|---|---|---|---|---|
| 1 | Cap: pull request #3529 "remove the Tauri app, GPUI is the only desktop app" was merged into main on 2026-10-29 | https://github.com/CapSoftware/Cap/pull/3529 | code | 2026-10-29 | yes | high |
| 2 | Cap: the 2026-10-21 engineering post says Cap 0.6.1 moved the desktop app off Tauri and web views to GPUI; it reports CPU 76.6% on Tauri versus 17.5% on GPUI for one editor-playback workload on one M4 Max, self-reported by the author, single machine, run count not stated | https://cap.so/blog/rebuilding-cap-in-gpui | blog | 2026-10-21 | yes | medium |
| 3 | Cap: the README describes a Turborepo monorepo whose desktop app is native Rust with a GPUI interface for macOS, Windows and Linux, a Next.js web app with Drizzle and MySQL, and recording, rendering and export in shared Rust crates | https://github.com/CapSoftware/Cap | repo | 2026-11-09 | yes | high |
| 4 | screenpipe: the README says the UI layer is a desktop app built with Tauri over Rust capture, storage, engine and service crates, with Windows and macOS downloads, under the Screenpipe Commercial License (source-available, commercial use needs a license) | https://github.com/mediar-ai/screenpipe | repo | 2026-11-09 | yes | high |
| 5 | tauri: GitHub lists Apache-2.0 for the repository, 112k stars, last push 2026-11-09 | https://github.com/tauri-apps/tauri | repo | 2026-11-09 | yes | high |
| 6 | Zed: the repository holds the editor crates and a Dockerfile-collab for its collaboration server, so client and backend share one repo | https://github.com/zed-industries/zed | repo | 2026-11-09 | yes | high |
| 7 | gpui: the crates/gpui directory in the Zed repository carries a LICENSE-APACHE file and a Windows resources directory | https://github.com/zed-industries/zed/tree/main/crates/gpui | code | 2026-11-09 | yes | high |
| 8 | tauri-action: builds a Tauri app and optionally uploads the bundles to a GitHub release; the README example uses tagName app-v__VERSION__; latest release action-v1.0.0 on 2026-06-29 | https://github.com/tauri-apps/tauri-action | repo | 2026-11-07 | yes | high |
| 9 | Bitwarden: the clients repository is one monorepo for the web vault, browser extension, Electron desktop app and CLI | https://github.com/bitwarden/clients | repo | 2026-11-09 | yes | high |
| 10 | Bitwarden: the server repository holds the API, database and Docker infrastructure separately from the clients | https://github.com/bitwarden/server | repo | 2026-11-09 | yes | high |
| 11 | Wails: v3.0.0 was published on GitHub on 2026-10-26; the repository is MIT and lists Windows and macOS targets | https://github.com/wailsapp/wails | repo | 2026-11-09 | yes | high |
| 12 | MAUI: dotnet/maui is MIT with release 10.0.120 on 2026-10-21 and 29 releases in the last 12 months | https://github.com/dotnet/maui | repo | 2026-11-09 | yes | high |
| 13 | Tauri: the Windows installer page says the NSIS installer lands in src-tauri/target/release/bundle/nsis/ and that signing cross-compiled Windows installers needs an external signing tool | https://v2.tauri.app/distribute/windows-installer/ | docs | 2026-11-05 | yes | high |
| 14 | AppFlowy: the app repository (Flutter frontend, Rust backend crates) is AGPL-3.0 with release 0.15.0 on 2026-11-02 | https://github.com/AppFlowy-IO/AppFlowy | repo | 2026-11-02 | yes | high |
| 15 | AppFlowy: AppFlowy-Cloud is a separate repository for the hosted backend | https://github.com/AppFlowy-IO/AppFlowy-Cloud | repo | 2026-11-09 | yes | high |
| 16 | electron-builder: the README offers `electron-builder --dir` for an unpacked build and publishing to GitHub Releases, S3 and Spaces; latest tag electron-updater@7.0.0-alpha.13 on 2026-10-29 | https://github.com/electron-userland/electron-builder | repo | 2026-11-08 | yes | high |
| 17 | electron: the repository is MIT, 123k stars, latest tag v46.0.0-alpha.7 on 2026-11-06 | https://github.com/electron/electron | repo | 2026-11-08 | yes | high |
| 18 | Avalonia: the repository is MIT with release 12.1.5 on 2026-11-02 and 30 releases in 12 months | https://github.com/AvaloniaUI/Avalonia | repo | 2026-11-08 | yes | high |
| 19 | upload-artifact: the current major is v7 (v7.0.2 on 2026-10-07); the README says v4+ is not supported on GitHub Enterprise Server | https://github.com/actions/upload-artifact | repo | 2026-10-07 | yes | high |
| 20 | wails: the CLI reference says `wails build` outputs to build/bin, `-platform` selects darwin or windows targets, and `wails build -clean` cleans build/bin | https://github.com/wailsapp/wails/blob/master/website/docs/reference/cli.mdx | docs | 2026-10-18 | yes | high |
| 21 | Tauri: the releases page lists tauri v2.12.5 (2026-10-21) as the newest 2.x and tauri v3.0.0-alpha.6 (2026-11-06) as the newest tag | https://github.com/tauri-apps/tauri/releases | release | 2026-11-06 | yes | high |
| 22 | rust-cache: Swatinem/rust-cache is a GitHub Action for Cargo caching, release v2.9.3 on 2026-10-20, LGPL-3.0 (an action, not linked into the app) | https://github.com/Swatinem/rust-cache | repo | 2026-10-26 | yes | high |
| 23 | setup-node: the README says caching is enabled with the `cache` input and that pnpm and Yarn need it configured explicitly; latest release v7.1.0 on 2026-10-08 | https://github.com/actions/setup-node | repo | 2026-11-04 | yes | high |
| 24 | runner-images: the Windows2025 image README lists Rust with Cargo 1.99.1, .NET Core SDKs, Node.js and Visual Studio | https://github.com/actions/runner-images/blob/main/images/windows/Windows2025-Readme.md | docs | 2026-11-05 | yes | high |
| 25 | git-cliff: the repository ships examples/keepachangelog.toml, a configuration that renders a Keep a Changelog layout | https://github.com/orhun/git-cliff/blob/main/examples/keepachangelog.toml | code | 2026-08-29 | yes | high |
| 26 | cargo-release: the README states that dry-run mode is the default and `-x, --execute` is needed to act; it changes the version in Cargo.toml and can edit CHANGELOG.md through pre-release-replacements | https://github.com/crate-ci/cargo-release | repo | 2026-11-01 | yes | high |
| 27 | release-please: the CLI doc lists `--dry-run` as "Prepare but do not take action" and `--release-as` to force a version; the manifest-pr command is deprecated in favor of release-pr | https://github.com/googleapis/release-please/blob/main/docs/cli.md | docs | 2026-08-18 | yes | high |
| 28 | Tauri: the prerequisites page installs Rust with `curl --proto '=https' --tlsv1.2 https://sh.rustup.rs -sSf` piped to sh and says desktop-only macOS development needs Xcode Command Line Tools via `xcode-select --install` | https://v2.tauri.app/start/prerequisites/ | docs | 2026-08-20 | yes | high |
| 29 | MAUI: the publish-unsigned page builds a Mac Catalyst .app with `dotnet publish -f net8.0-maccatalyst -c Release -p:CreatePackage=false` (CreatePackage false yields a .app instead of a .pkg) | https://learn.microsoft.com/en-us/dotnet/maui/mac-catalyst/deployment/publish-unsigned | docs | 2026-10-08 | yes | high |
| 30 | changeset: `changeset status [--verbose] [--output={filePath}] [--since={gitTag}]` prints the planned releases and can write them as JSON without changing files | https://github.com/changesets/changesets/blob/main/docs/command-line-options.md | docs | 2026-08-03 | yes | high |
| 31 | actionlint: a static checker for GitHub Actions workflow files; release v1.7.12 on 2026-03-30, last push 2026-07-16 | https://github.com/rhysd/actionlint | repo | 2026-07-16 | yes | high |
| 32 | MAUI: the installation page shows `sudo dotnet workload install maui` for the macOS command line | https://learn.microsoft.com/en-us/dotnet/maui/get-started/installation | docs | 2026-06-15 | yes | high |
| 33 | Flutter: the manual install page has you download the SDK archive, extract it and add it to PATH | https://docs.flutter.dev/install/manual | docs | 2026-05-05 | yes | high |
| 34 | flutter-action: subosito/flutter-action supports a `cache` input backed by actions/cache; release v2.23.0 on 2026-03-25, last push 2026-04-30 | https://github.com/subosito/flutter-action | repo | 2026-04-30 | yes | high |
| 35 | MAUI: the unpackaged Windows page runs `dotnet publish -f net8.0-windows10.0.19041.0 -c Release -p:RuntimeIdentifierOverride=win10-x64 -p:WindowsPackageType=None` and the executable lands under bin\Release\net8.0-windows10.0.19041.0\win10-x64\publish | https://learn.microsoft.com/en-us/dotnet/maui/windows/deployment/publish-unpackaged-cli | docs | 2026-01-15 | yes | high |
| 36 | dotnet-install: the script page says it installs the SDK without user interaction and without admin rights | https://learn.microsoft.com/en-us/dotnet/core/tools/dotnet-install-script | docs | 2024-01-15 | yes | high |
| 37 | fnm: Node version manager installed with a `curl -fsSL` script into the user's home | https://github.com/Schniz/fnm | repo | 2026-10-26 | yes | high |
| 38 | Forge: the build lifecycle page says the Package step writes the packaged app to /out/ for the host platform and architecture only, and Make writes installers to /out/make/ | https://www.electronforge.io/core-concepts/build-lifecycle | docs | unknown | yes | high |
| 39 | Flutter: the Windows building page says the release executable is in the build\windows\runner\Release directory | https://docs.flutter.dev/platform-integration/windows/building | docs | unknown | yes | high |
| 40 | Forge: the macOS code-signing guide says that from macOS 10.15 an app must be signed and notarized to run without the user disabling OS checks, and configures signing as an opt-in osxSign step | https://www.electronforge.io/guides/code-signing/code-signing-macos | docs | unknown | yes | high |
| 41 | Compose Multiplatform: the native distribution page says `packageDmg` and `packageMsi` run jpackage, each format builds only on its own OS, output goes to build/compose/binaries, and signing and notarization are macOS-specific options | https://kotlinlang.org/docs/multiplatform/compose-native-distribution.html | docs | unknown | yes | high |
| 42 | semantic-release: the configuration page says `-d, --dry-run` skips prepare, publish, addChannel, success and fail, prints the next version and release notes, and still verifies push permission | https://semantic-release.gitbook.io/semantic-release/usage/configuration | docs | unknown | yes | high |
| 43 | Tauri: the home page says the size of a Tauri app can be as small as 600 KB (vendor self-report, setup not stated) | https://tauri.app/ | docs | unknown | yes | low |
| 44 | rustup: the install command is `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs` piped to sh, a user-level install | https://rustup.rs | docs | unknown | yes | high |
| 45 | Flutter: the macOS install page requires Xcode for macOS desktop development | https://docs.flutter.dev/platform-integration/macos/install-macos | docs | unknown | yes | high |
| 46 | Flutter: the macOS deployment page builds with `flutter build macos --release` and its distribution steps outside the App Store sign and notarize with Developer ID (`xcrun productsign`) | https://docs.flutter.dev/deployment/macos | docs | 2026-05-05 | yes | medium |
| 47 | git-cliff: `git cliff --bumped-version` prints the next version and `--bump --unreleased` renders the unreleased section under that version | https://git-cliff.org/docs/usage/bump-version/ | docs | unknown | yes | high |
| 48 | Flutter: dart:ffi with the FFI plugin template is the documented path to call macOS C APIs | https://docs.flutter.dev/platform-integration/macos/c-interop | docs | 2026-05-05 | yes | high |
| 49 | knope: `--dry-run` prints what would happen without writing, `--override-version` forces the version, and the repo released knope/v0.23.2 on 2026-10-22 | https://knope.tech/reference/command-line-arguments/ | docs | 2023-11-08 | yes | high |
| 50 | knope: the changelog page says each version starts with a level-2 heading with the version number and release date | https://knope.tech/reference/concepts/changelog/ | docs | unknown | yes | high |
| 51 | Tauri: the GitHub pipeline page's example uses a matrix over macos-latest (two targets), ubuntu and windows-latest, actions/checkout, actions/setup-node with a cache, and tauri-action | https://v2.tauri.app/distribute/pipelines/github/ | docs | unknown | yes | high |
| 52 | Tauri: the plugins page says plugins handle commands in Rust, Kotlin or Swift, with Kotlin and Swift for the mobile targets | https://v2.tauri.app/develop/plugins/ | docs | unknown | yes | high |
| 53 | Tauri: the macOS bundle page documents the <productName>.app layout (Contents/MacOS, Resources, _CodeSignature) and points to the signing docs | https://v2.tauri.app/distribute/macos-application-bundle/ | docs | 2025-07-21 | yes | high |
| 54 | Electron: native Node modules (node-gyp or Node-API addons) are the documented way to call OS APIs | https://www.electronjs.org/docs/latest/tutorial/using-native-node-modules | docs | 2026-02-17 | yes | high |
| 55 | napi-rs: a framework for Node.js addons in Rust via Node-API, release napi-v3.16.1 on 2026-10-31 | https://github.com/napi-rs/napi-rs | repo | 2026-11-07 | yes | high |
| 56 | Avalonia: the macOS deployment page builds the .app by hand from `dotnet publish` output (Contents/MacOS, Info.plist, icns) or with the Parcel tool, then covers signing and notarization | https://docs.avaloniaui.net/docs/deployment/macos | docs | unknown | yes | high |
| 57 | Qt: the licensing page offers Qt under LGPLv3 (or GPLv3) and a commercial license | https://doc.qt.io/qt-6/licensing.html | docs | unknown | yes | high |
| 58 | aqtinstall: a pip-installed Qt installer; latest release v3.3.0 on 2025-06-02, last push 2026-11-01 | https://github.com/miurahr/aqtinstall | repo | 2026-11-01 | yes | high |
| 59 | JEP 454: the Foreign Function and Memory API is final in Java 22, the JVM path to native calls for a Compose desktop app | https://openjdk.org/jeps/454 | docs | unknown | yes | high |
| 60 | Compose Multiplatform: the repository is Apache-2.0 with v1.13.0-beta02 on 2026-11-04 | https://github.com/JetBrains/compose-multiplatform | repo | 2026-11-07 | yes | high |
| 61 | Flutter: the repository is BSD-3-Clause, 181k stars, last push 2026-11-08 | https://github.com/flutter/flutter | repo | 2026-11-08 | yes | high |
| 62 | react-native-windows: MIT per the LICENSE file; release 0.85.0 on 2026-10-20 | https://github.com/microsoft/react-native-windows | repo | 2026-11-07 | yes | high |
| 63 | react-native-macos: MIT; release 0.83.0 on 2026-09-30 | https://github.com/microsoft/react-native-macos | repo | 2026-11-06 | yes | high |
| 64 | OBS: the macOS build instructions list Qt among the prebuilt dependencies; the project is GPL-2.0 | https://github.com/obsproject/obs-studio/wiki/Build-Instructions-For-Mac | docs | unknown | yes | high |
| 65 | Kap: an Electron screen recorder for macOS only; latest release v3.6.0 on 2022-10-27, last push 2024-11-12 | https://github.com/wulkano/Kap | repo | 2024-11-12 | yes | high |
| 66 | runner-images: the macos-15 image README lists Cargo 1.98.0, Node.js 22.23.2, .NET SDKs 8, 9 and 10, and Xcode; Flutter is not listed | https://github.com/actions/runner-images/blob/main/images/macos/macos-15-Readme.md | docs | 2026-08-28 | yes | high |
| 67 | Keep a Changelog: version 1.1.0 defines the `## [Unreleased]` section at the top of CHANGELOG.md | https://keepachangelog.com/en/1.1.0/ | docs | unknown | yes | high |
| 68 | cargo-release: the reference lists `-x, --execute` and `pre-release-replacements` for editing files such as CHANGELOG.md at release time | https://github.com/crate-ci/cargo-release/blob/main/docs/reference.md | docs | 2026-09-18 | yes | high |

## Options for the planner
All ten options below have a current form dated 2026, so recency does not separate them; the order uses fit to the hard requirements, then evidence of a shipping capture app, then toolchain friction. Health lines come from evidence/researcher-8/repo-health.txt (gh api, 2026-11-09).

1. **Tauri 2.12.5** (2026-10-21)
   - What: Rust core plus OS web view for the UI; plugins and native access in Rust.
   - Fit: one repo builds both (meets); native access through Rust crates (meets); Apache-2.0 and MIT (meets); unsigned bundles via `tauri build`, tauri-action without signing secrets (meets, signing is opt-in); rustup is user-level but macOS also needs Xcode Command Line Tools (meets with a caveat).
   - Tradeoffs: the only framework here with a shipping 24/7 screen and audio recorder on both platforms (screenpipe); smallest vendor-claimed size; official CI example and action. Against: Cap left Tauri in October 2026 for GPUI over web-view overlay and picker problems; Tauri 3 is in alpha, so the 2.x line will be superseded; the UI runs in two different web engines (WebView2, WKWebView).
   - Health: last commit 2026-11-09, latest release tauri-v3.0.0-alpha.6 2026-11-06 (newest stable tauri v2.12.5 2026-10-21), releases in last 12 months 100+, open issues 1509, archived no, license Apache-2.0 (dual MIT).
   - Evidence: #4, #5, #8, #13, #21, #28, #43, #51, #52, #53; counter-evidence #1, #2.
2. **Electron** (newest tag v46.0.0-alpha.7, 2026-11-06; stable line not checked here)
   - What: bundled Chromium and Node; native access through Node-API addons (napi-rs for Rust).
   - Fit: one repo (meets); native access via addons (meets); MIT (meets); unsigned `out/` packages by default with Forge (meets); Node via fnm, no sudo, no Xcode needed to package (meets, lowest install friction).
   - Tradeoffs: largest runtime (no official figure found); mature packaging (Forge, electron-builder), many desktop examples (Bitwarden). Against: every capture call is a native addon; the only Electron capture app found (Kap) is macOS-only and unmaintained.
   - Health: last commit 2026-11-08, latest tag v46.0.0-alpha.7 2026-11-06, releases in last 12 months 100+, open issues 783, archived no, license MIT.
   - Evidence: #9, #16, #17, #37, #38, #40, #54, #55, #65.
3. **GPUI** (Zed crate, 2026-11-09)
   - What: Rust GPU-rendered UI framework from Zed, used by Cap's recorder on macOS, Windows and Linux.
   - Fit: one repo (meets); native access is plain Rust (meets); Apache-2.0 (meets); unsigned builds are plain Cargo binaries (meets); rustup (meets, same Xcode CLT caveat as Tauri).
   - Tradeoffs: a shipping recorder (Cap) chose it over Tauri and Electron in 2026, with self-reported CPU and memory wins. Against: no bundler, installer, updater or CI action of its own (Cap wrote `gpui:build`); API changes with Zed; docs are a README and examples; no stable release tags of its own.
   - Health: last commit 2026-11-09 (Zed repo), latest release v1.28.0-pre 2026-11-04 (Zed, not GPUI), releases in last 12 months 100+ (Zed), open issues 3076 (Zed), archived no, license Apache-2.0 for crates/gpui.
   - Evidence: #1, #2, #3, #6, #7.
4. **Compose Multiplatform 1.13.0-beta02** (2026-11-04)
   - What: Kotlin UI on the JVM; jpackage builds dmg and msi.
   - Fit: one repo (meets); native via JEP 454 FFM or JNI (meets); Apache-2.0 (meets); `packageDmg` and `packageMsi` run without signing config (meets); JDK is an archive, no sudo (meets).
   - Tradeoffs: one language, mature packaging. Against: JVM runtime size (no official figure); no capture app found on it; each format builds only on its OS (fine for two jobs).
   - Health: last commit 2026-11-07, latest release v1.13.0-beta02 2026-11-04, releases in last 12 months 30, open issues 27, archived no, license Apache-2.0.
   - Evidence: #41, #59, #60.
5. **Flutter 3.x** (2026-11-08)
   - What: Dart UI with its own renderer; dart:ffi for native.
   - Fit: one repo (meets); native via dart:ffi plugins (meets); BSD-3-Clause (meets); unsigned Windows build documented, macOS unsigned state not stated (partly unknown); SDK is an archive, but macOS desktop needs Xcode and Windows needs Visual Studio, and GitHub runners do not preinstall Flutter (meets with friction).
   - Tradeoffs: AppFlowy shows Flutter plus Rust at scale. Against: the macOS output path and unsigned default are not in the docs fetched; no capture app found.
   - Health: last commit 2026-11-08, latest GitHub release tag 3.19.0-0.1.pre 2024-01-11 (Flutter publishes releases outside GitHub Releases), releases in last 12 months 0 on GitHub, open issues 13524, archived no, license BSD-3-Clause.
   - Evidence: #14, #33, #34, #39, #45, #46, #48, #61, #66.
6. **.NET MAUI 10.0.120** (2026-10-21)
   - What: C# UI; Windows via WinUI, macOS via Mac Catalyst.
   - Fit: one repo (meets); native via C# platform code (meets, but Mac Catalyst, not AppKit, see Unknowns); MIT (meets); unsigned builds documented for both platforms (meets, best documented); dotnet-install needs no admin, but the MAUI page shows `sudo dotnet workload install maui` on macOS (caveat).
   - Tradeoffs: the clearest unsigned-publish docs of any option. Against: Mac Catalyst's access to CGEventTap, ScreenCaptureKit and Accessibility is not confirmed here; Windows build needs Windows.
   - Health: last commit 2026-11-09, latest release 10.0.120 2026-10-21, releases in last 12 months 29, open issues 1374, archived no, license MIT.
   - Evidence: #12, #29, #32, #35, #36.
7. **Avalonia 12.1.5** (2026-11-02)
   - What: C# cross-platform UI with its own renderer.
   - Fit: one repo (meets); native via P/Invoke (meets); MIT (meets); .app built by hand from `dotnet publish` output or with Parcel (meets, manual); dotnet-install (meets).
   - Tradeoffs: real AppKit host, unlike Catalyst. Against: no first-party bundler for macOS; no capture app found.
   - Health: last commit 2026-11-08, latest release 12.1.5 2026-11-02, releases in last 12 months 30, open issues 2016, archived no, license MIT.
   - Evidence: #18, #56.
8. **Wails 3.0.0** (2026-10-26)
   - What: Go core plus OS web view.
   - Fit: one repo (meets); native via cgo (meets); MIT (meets); `wails build` to build/bin (meets); Go tarball, no sudo (meets).
   - Tradeoffs: stable for two weeks as of now; docs site returns 403 to plain fetches; no capture app found.
   - Health: last commit 2026-11-09, latest release v3.2.0-nightly.20261103 2026-11-03 (stable v3.0.0 2026-10-26), releases in last 12 months 100+, open issues 393, archived no, license MIT.
   - Evidence: #11, #20.
9. **React Native for Windows 0.85 and macOS 0.83** (2026-10-20, 2026-09-30)
   - What: two platform packages from Microsoft on one React Native codebase.
   - Fit: one repo (meets); native modules in C++ and Objective-C (meets); MIT (meets); unsigned builds not checked (unknown); Node plus Xcode and Visual Studio (friction).
   - Tradeoffs: two separately versioned platform packages; no capture app found.
   - Health: react-native-windows last commit 2026-11-07, release 0.85.0 2026-10-20, 38 releases in 12 months, 777 open issues, MIT; react-native-macos last commit 2026-11-06, release 0.83.0 2026-09-30, 12 releases, 148 open issues, MIT.
   - Evidence: #62, #63.
10. **Qt 6 under LGPLv3** (licensing page, undated)
   - What: C++ framework; OBS Studio is the capture app built on it.
   - Fit: one repo (meets); native in C++ (meets); LGPLv3 allows MIT app code with dynamic linking (meets, with LGPL obligations); unsigned builds (not checked); install without sudo via aqtinstall, whose last release is 2025-06-02 (stale).
   - Tradeoffs: proven in OBS. Against: C++ toolchain, LGPL relinking obligations, stale installer tool.
   - Health: aqtinstall last commit 2026-11-01, latest release v3.3.0 2025-06-02, 0 releases in 12 months, 79 open issues, MIT; Qt itself is not on GitHub as a canonical repo.
   - Evidence: #57, #58, #64.

Release tooling, ranked for a Keep-a-Changelog CHANGELOG.md with an Unreleased section and a dry run that prints version, tag and section:
1. **git-cliff 2.14.2** (2026-09-18): `--bumped-version` and `--bump --unreleased` print the next version and section; keepachangelog.toml example; needs Conventional Commits; Apache-2.0. Health: last commit 2026-11-02, 6 releases in 12 months, 114 open issues. Evidence #25, #47.
2. **knope 0.23.2** (2026-10-22): `--dry-run` prints each step, `--override-version`, version headings with dates; changeset files instead of commit parsing; MIT; small community (207 stars). Health: last commit 2026-11-07, 25 releases in 12 months, 67 open issues. Evidence #49, #50.
3. **cargo-release 1.1.7** (2026-10-21): dry run by default, `pre-release-replacements` for CHANGELOG.md; Rust-only, so it fits Tauri or GPUI only. Health: last commit 2026-11-01, 11 releases, 70 open issues, Apache-2.0. Evidence #26, #68.
4. **semantic-release** (newest tag v26.0.0-beta.6, 2026-11-06): `--dry-run` prints next version and notes but writes Conventional Commits sections, not Keep a Changelog, and checks push permission. Health: last commit 2026-11-06, 14 releases, 412 open issues, MIT. Evidence #42.
5. **release-please 17.11.2** (2026-08-24): `--dry-run`, PR-based, Conventional Commits sections. Health: last commit 2026-11-03, 21 releases, 388 open issues, Apache-2.0. Evidence #27.
6. **changesets** (2026-09-14): `changeset status` without writes; JavaScript workspace centric. Health: last commit 2026-11-08, 100+ package releases, 286 open issues, MIT. Evidence #30.
Not version tools: tauri-action (#8) and electron-builder publish (#16) upload artifacts to GitHub Releases.

CI building blocks: actionlint v1.7.12 (#31), actions/setup-node v7.1.0 cache (#23), Swatinem/rust-cache v2.9.3 (#22), actions/upload-artifact v7.0.2 (#19), subosito/flutter-action v2.23.0 (#34, stale), runner images macos-15 and windows-2025 (#24, #66).

Rejected:
- screenpipe as a code base: Screenpipe Commercial License, source-available, commercial use needs a license (fails the open-source-later requirement). It stays as evidence that Tauri can host 24/7 capture (#4).
- Kap: macOS only, no release since 2022, last push 2024 (fails one-repo-both-platforms and health) (#65).
- Rust or Swift and Kotlin core with two native shells: one repository is possible, but it is two UI codebases and not a framework; no primary source evaluated. Not ranked.
- Tauri 3.0.0-alpha: alpha, not a shipping base (#21).

## Unknowns
- Flutter macOS `.app` output path · checked: docs.flutter.dev deployment/macos and platform-integration/macos/building · would settle it: `flutter build macos` on a runner, or the flutter/website source.
- Whether Tauri, Compose and Flutter default CI builds are unsigned without extra config · checked: Tauri sign and bundle pages, Compose native-distribution page, Flutter deployment page · would settle it: one run of each build on macos-15 and `codesign -dv` on the output.
- Whether `xcode-select --install` needs an admin account on a Mac · checked: Tauri prerequisites page · would settle it: Apple's documentation or a test on a standard account; GitHub runners have Xcode preinstalled, so CI is unaffected (#66).
- Official bundle sizes for Electron, Flutter, Compose and MAUI hello-world apps · checked: electronjs.org performance page, project READMEs · would settle it: building each and measuring; third-party figures disagree (3 MB to 10 MB for Tauri, 85 MB to 200 MB for Electron) and are not cited.
- Mac Catalyst access to CGEventTap, Accessibility and ScreenCaptureKit for .NET MAUI · checked: MAUI docs fetched · would settle it: Apple's Catalyst API availability tables (lane 4).
- Single version source of truth for Tauri (tauri.conf.json, Cargo.toml, package.json) · checked: not fetched · would settle it: v2.tauri.app configuration reference.
- GPUI packaging and updater tooling · checked: Cap README · would settle it: reading Cap's `gpui:build` script.
- Tauri 3 release timeline · checked: releases page, blog index · would settle it: Tauri 3.0 milestone on GitHub.

## Stale flags
- https://v2.tauri.app/distribute/macos-application-bundle/ (source file 2025-07-21): older than the cutoff on a fast-moving topic; no newer treatment found, bundle layout is stable.
- https://learn.microsoft.com/en-us/dotnet/core/tools/dotnet-install-script (2024-01-15): older than the cutoff; no newer page found.
- https://knope.tech/reference/command-line-arguments/ (source file 2023-11-08): older than the cutoff; knope/v0.23.2 (2026-10-22) shows the project is active and the page is still live.
- subosito/flutter-action (last push 2026-04-30): older than the cutoff; no newer official Flutter action found.
- wulkano/Kap (2024-11-12): abandoned; kept only as the Electron capture example.
- miurahr/aqtinstall (release 2025-06-02): older than the cutoff; repo still receives commits.
- Overruled: Cap repository topic tags "tauri" and "tauri-app" by PR #3529 (2026-10-29) and the 2026-10-21 post; Cap is no longer a Tauri app.
- Overruled: Wails v3 beta status by the v3.0.0 release of 2026-10-26.

## Changes
none (report and evidence directory only).

## Evidence
- evidence/researcher-8/fetch-all.sh and fetch-all.txt: every URL fetched, HTTP code and byte count.
- evidence/researcher-8/pages/*.html and *.txt: raw pages and text extractions (html2txt.py).
- evidence/researcher-8/repo-health.sh and repo-health.txt: gh api health for 31 repositories.
- evidence/researcher-8/grep-facts.sh and grep-facts.txt: the quoted passages behind the findings.
- evidence/researcher-8/url-check.sh and url-check.txt: per-row curl code and subject grep.
- evidence/researcher-8/verify.txt: outputs of the Verify-by commands.

## Assumptions
- "Unsigned builds possible" is read as "the build succeeds with no signing identity configured"; only MAUI documents this in so many words.
- Repository pages are dated by their last push (gh api pushed_at); GitHub file pages by the file's last commit; Microsoft Learn pages by ms.date; pages with no visible date are `unknown`.
- Tauri's license is written as Apache-2.0 because that is what GitHub reports; the repository also carries an MIT license file.
- The web searches used were for public concepts only; no private names were sent.

## Issues and risks
- wails.io and v3.wails.io return 403 to curl and to the harness fetch tool, so the Wails rows cite GitHub files instead.
- electron.build renders its docs with JavaScript (292 bytes to curl), so electron-builder is cited from its README.
- docs.avaloniaui.net returns 404 for the capitalized `/macOS` path; the lowercase path works and is the one cited.
- Tauri's docs do not state the macOS output directory in text; the Windows page gives `release/bundle/nsis/` and the macOS page gives the bundle layout only.
- The system `grep` on this machine is ugrep (alias) and rejects long context patterns; the scripts call /usr/bin/grep.
- Rows 2 (Cap post) and 43 (Tauri home) carry self-reported numbers.
- STATUS is ISSUES because sub-question 2 rests on partial documentation for unsigned defaults and the Flutter macOS path, and because no official bundle sizes exist for most options.

## Learnings
- A shipping recorder's exit from Tauri (Cap, 2026-10) is the newest primary evidence on web-view shells for capture UIs; check it before treating Tauri's screenpipe precedent as settled.
- Start with GitHub file pages (blob URLs) for docs that live in repos: they give a commit date and survive bot blocks on the docs sites.
