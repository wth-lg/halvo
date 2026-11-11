# Changelog

All notable changes to Halvo are recorded in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
the project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

To record a change, add one line at the end of the list under the Unreleased
heading: `- <Type>: <text>`, where Type is Added, Changed, Deprecated, Removed,
Fixed or Security. A pull request adds one entry for each type of change it
makes. The list has no `###` headings, so that two branches can each add a line
and merge without an entry landing under the heading of another.
`sh scripts/check-changelog.sh` checks the file.

## [Unreleased]

- Added: Document the build command and record it failing on the empty repository
- Added: Ignore build output, node_modules and Claude worktrees
- Added: Add the Tauri 2 app that shows and prints its version
- Added: Add the GitHub Actions workflow that builds both platforms
- Added: Add the changelog and the release dry run
- Added: Add the ADRs for the three scaffold decisions
- Added: Copy the research reports and the plan into docs
- Added: Build the bare executable on Windows without installer tooling
- Added: Update the plan copy after the ledger plan revisions
- Added: Refresh the research report copies after the review fixes
- Added: Add the R1 gate harness and the pre-crate baseline
- Added: Scaffold the halvo-capture crate with shared types and module boundaries
- Added: Silence the shellcheck findings in the gate harness
- Added: Run cargo test, clippy and fmt on both CI jobs
- Added: Add the consent status types, stored-data inventory, pause and storage cap with mock-store tests
- Added: Add just-in-time macOS permission onboarding with System Settings deep links and lost-grant walk-back
- Added: Add the data view, its Tauri commands and the menu-bar capture indicator
- Added: Add failing tests for the adversarial review of the encrypted store
- Added: Measure the peak resident set of a 512 MiB segment write and read in an ignored test
- Added: Prove the store crypto-shred and single-writer invariants over random operation sequences and process races
- Added: Add the macOS secure-input guard and the capture backend dependencies
- Added: Add the idempotent macOS LaunchAgent install for the capture daemon
- Added: Add the macOS capture sources for screen, keystroke, pointer, foreground app and window title
- Added: Add the Windows capture dependencies for Graphics Capture, Raw Input and the registry
- Added: Add the Windows capture backend behind cfg with the logon-task install and unit tests
- Added: Add a CI grep lint banning low-level input hooks in the capture crate
- Added: Reject attribution lines in commit messages with a commit-msg hook and a CI step
- Added: Add AGENTS.md with the rule table and import it from CLAUDE.md
- Added: Add failing fixtures for a trailer below scissors text that git keeps
- Added: Add failing fixtures for scissors lines git does not write and for a merge conclusion
- Changed: Split the foreground state into one writer and readers and name the unpersisted placement
- Changed: Make the secure-input zero-keystroke invariant a type and a test
- Changed: Make the secure-input hold RAII and name the screen self-exclusion step
- Changed: Own window-title CoreFoundation values by Drop instead of manual CFRelease
- Changed: Subtract the unwired audio and mic on/off and name the XML, installer and start-failure types
- Changed: Make the inventory sorted by construction so eviction shreds the oldest day
- Changed: Request the Accessibility grant through the system prompt and name request outcomes by what the code knows
- Changed: Keep one public source for the pane title the running macOS shows
- Changed: Derive the grants to record from the permission list instead of a duplicate field
- Changed: Resolve the consent settings path through Tauri so the identifier lives only in tauri.conf.json
- Changed: Write the storage cap constants from the window, the daily estimate and the headroom
- Changed: Accept a platform refusal of a contending writer in the single-writer race check
- Changed: Name the calendar constants and the year-month-day conversions of UtcDay
- Changed: Add the Windows crate features for the new input, HID and registry-watch constants
- Changed: Merge CHANGELOG.md with git's union driver so parallel entries stop conflicting
- Changed: Print the help of the commit-message check from here-documents
- Changed: Name the newest exempt commit and record why it is exempt in AGENTS.md
- Changed: Name the parameters of expect and the one-mode rule of the option parser
- Changed: Say in the CI log why the commit-message step checks every commit
- Changed: Say newest exempt commit where the range check said baseline
- Changed: Remove comments that restate the code in the commit-message check
- Fixed: Read Windows raw input through an aligned buffer and close the pump start/stop race, carrying decoded payload sizes to an explicit metadata-only placement
- Fixed: Watch every registry root and path the installed-software scan reads and parse SystemComponent to a bool at the boundary
- Fixed: Quote every scheduled-task argument token and match the Task Scheduler Command element
- Fixed: Drop only the diff below git's own scissors block from the hook file
- Fixed: Report a failed awk as an error instead of an attribution line
- Fixed: Name the work-tree-root test of prerequisites.sh and make the name true
- Fixed: Match the scissors line by git's comment string and keep empty lines in its block
- Fixed: Say in the CI echo that exempt commits are not checked
- Fixed: Clamp a storage cap read from consent.json to the minimum so a zero cap cannot shred the store
- Added: Add SQLCipher and AEAD dependencies for the encrypted segment store
- Added: Add dev-fallback and release key custody for the store key
- Added: Add the encrypted segment store, crypto-shred and verify paths
- Changed: Strip comments from the encrypted store and custody files
- Changed: Build SQLCipher with its libtomcrypt provider instead of OpenSSL
- Changed: Stream segment files through bounded chunks instead of buffering them
- Fixed: Propagate keychain errors and bind each custody to a RootId
- Changed: Make the store a single writer with retention as a parameter and a sealed state
- Changed: Check deletion by decrypting with every remaining key
- Fixed: Destroy the keychain roots the store tests create when each test ends
- Fixed: Hold the store under an exclusive database lock and complete every shred before reporting it
- Fixed: Make the store and deletion gates measure the store instead of matching constants
- Fixed: Keep the Windows store root in a DPAPI-protected per-user file so delete-all destroys it
- Fixed: Make the DPAPI protector crate-visible and the RSS pattern reader unix-only so Windows compiles
- Added: Refuse attribution lines at push and at merge with a pre-push hook, scripts/merge-pr.sh, a tested CI script and tracked Claude Code settings, and lint line endings, the changelog and help text read by line number
- Changed: Decide every attribution check by one predicate, write the changelog as one flat list of typed entries, and pair each rule in AGENTS.md with its check and its limits
- Fixed: Check out shell scripts and git hooks with LF in every clone, and stop the commit-msg hook from refusing a clean git commit -v
- Fixed: Release the store's writer lock explicitly so a spawned child cannot keep it held
- Added: Add the r5-stat trial of the learning run's gate statistics, the tools/r5-trials workspace, docs/r5/targets.toml and the attribute files of tools/r5-trials, docs/r5 and fixtures/r5
- Added: Add the r5-drift drift harness and admission trial of the learning run: the case reader of contracts.md L9 with a refusal file for each rule except the size rule, which a test checks on a file that it builds, and the five counts of the L9.3 paragraph, which unit tests check on counts that they build, the page model, the arms none, plain and admit, the 16 development cases under fixtures/r5/cases/dev and docs/r5/cases.md
