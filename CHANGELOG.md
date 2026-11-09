# Changelog

All notable changes to Halvo are recorded in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
the project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Each entry under Unreleased is the subject line of one commit.

## [Unreleased]

### Added

- Document the build command and record it failing on the empty repository
- Ignore build output, node_modules and Claude worktrees
- Add the Tauri 2 app that shows and prints its version
- Add the GitHub Actions workflow that builds both platforms
- Add the changelog and the release dry run
- Add the ADRs for the three scaffold decisions
- Copy the research reports and the plan into docs
- Build the bare executable on Windows without installer tooling
- Update the plan copy after the ledger plan revisions
- Refresh the research report copies after the review fixes
- Add the R1 gate harness and the pre-crate baseline
- Scaffold the halvo-capture crate with shared types and module boundaries
- Silence the shellcheck findings in the gate harness
- Run cargo test, clippy and fmt on both CI jobs
- Add the consent status types, stored-data inventory, pause and storage cap with mock-store tests
- Add just-in-time macOS permission onboarding with System Settings deep links and lost-grant walk-back
- Add the data view, its Tauri commands and the menu-bar capture indicator
- Add failing tests for the adversarial review of the encrypted store
- Measure the peak resident set of a 512 MiB segment write and read in an ignored test

### Changed

- Make the inventory sorted by construction so eviction shreds the oldest day
- Request the Accessibility grant through the system prompt and name request outcomes by what the code knows
- Keep one public source for the pane title the running macOS shows
- Derive the grants to record from the permission list instead of a duplicate field
- Resolve the consent settings path through Tauri so the identifier lives only in tauri.conf.json
- Write the storage cap constants from the window, the daily estimate and the headroom
- Name the calendar constants and the year-month-day conversions of UtcDay

### Fixed

- Clamp a storage cap read from consent.json to the minimum so a zero cap cannot shred the store
- Add SQLCipher and AEAD dependencies for the encrypted segment store
- Add dev-fallback and release key custody for the store key
- Add the encrypted segment store, crypto-shred and verify paths
- Strip comments from the encrypted store and custody files
- Build SQLCipher with its libtomcrypt provider instead of OpenSSL
- Stream segment files through bounded chunks instead of buffering them
- Propagate keychain errors and bind each custody to a RootId
- Make the store a single writer with retention as a parameter and a sealed state
- Check deletion by decrypting with every remaining key
- Destroy the keychain roots the store tests create when each test ends
- Hold the store under an exclusive database lock and complete every shred before reporting it
- Make the store and deletion gates measure the store instead of matching constants
- Keep the Windows store root in a DPAPI-protected per-user file so delete-all destroys it
- Make the DPAPI protector crate-visible and the RSS pattern reader unix-only so Windows compiles
