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
- Prove the store crypto-shred and single-writer invariants over random operation sequences and process races
- Add the macOS secure-input guard and the capture backend dependencies
- Add the idempotent macOS LaunchAgent install for the capture daemon
- Add the macOS capture sources for screen, keystroke, pointer, foreground app and window title
- Add the Windows capture dependencies for Graphics Capture, Raw Input and the registry
- Add the Windows capture backend behind cfg with the logon-task install and unit tests
- Add a CI grep lint banning low-level input hooks in the capture crate
- Reject attribution lines in commit messages with a commit-msg hook and a CI step
- Add AGENTS.md with the rule table and import it from CLAUDE.md
- Add failing fixtures for a trailer below scissors text that git keeps

### Changed

- Split the foreground state into one writer and readers and name the unpersisted placement
- Make the secure-input zero-keystroke invariant a type and a test
- Make the secure-input hold RAII and name the screen self-exclusion step
- Own window-title CoreFoundation values by Drop instead of manual CFRelease
- Subtract the unwired audio and mic on/off and name the XML, installer and start-failure types
- Make the inventory sorted by construction so eviction shreds the oldest day
- Request the Accessibility grant through the system prompt and name request outcomes by what the code knows
- Keep one public source for the pane title the running macOS shows
- Derive the grants to record from the permission list instead of a duplicate field
- Resolve the consent settings path through Tauri so the identifier lives only in tauri.conf.json
- Write the storage cap constants from the window, the daily estimate and the headroom
- Accept a platform refusal of a contending writer in the single-writer race check
- Name the calendar constants and the year-month-day conversions of UtcDay
- Add the Windows crate features for the new input, HID and registry-watch constants
- Merge CHANGELOG.md with git's union driver so parallel entries stop conflicting
- Print the help of the commit-message check from here-documents
- Name the newest exempt commit and record why it is exempt in AGENTS.md
- Name the parameters of expect and the one-mode rule of the option parser
- Say in the CI log why the commit-message step checks every commit

### Fixed

- Read Windows raw input through an aligned buffer and close the pump start/stop race, carrying decoded payload sizes to an explicit metadata-only placement
- Watch every registry root and path the installed-software scan reads and parse SystemComponent to a bool at the boundary
- Quote every scheduled-task argument token and match the Task Scheduler Command element
- Drop only the diff below git's own scissors block from the hook file
- Report a failed awk as an error instead of an attribution line
- Name the work-tree-root test of prerequisites.sh and make the name true

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
