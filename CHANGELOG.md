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
- Added: Add a gate lint banning low-level input hooks in the capture crate
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
- Changed: Decide every attribution check by one predicate, write the changelog as one flat list of typed entries, pair each rule in AGENTS.md with its check and its limits, and remove comments that restate the code in the commit-message check
- Fixed: Check out shell scripts and git hooks with LF in every clone, and stop the commit-msg hook from refusing a clean git commit -v
- Fixed: Release the store's writer lock explicitly so a spawned child cannot keep it held
- Added: Add the per-run ci.d driver with the nine R1 checks, the bounds file of the R1 gate, its memory check and its mutant rows beside the code
- Changed: Give each CI job a 30-minute timeout, cancel only superseded pull_request runs, save the Rust cache on main only and list the workspace members by a glob
- Fixed: Make the R1 gate checks fail closed when a source did not run, the input counts of a run contradict its input mode, a bound is unreadable, a verify step outlives its deadline, a ci.d entry is not a regular NN-name.sh file, a mutant had no build of its kind, or the tree changed during the run
- Security: Pin every workflow action to a commit, give the workflow read-only permissions and let Dependabot propose action updates
- Added: Add the page-test harness under tools/r4-page with its pinned packages, the browser builds pinned by address, size and SHA-256 and not committed, the six accessibility checks and a self-test on a sample view
- Added: Add docs/r2 with its README and the .gitattributes files that give the workflow run's directories LF line ends and keep its fixtures out of line-end conversion
- Added: Add the learning run's first phase: the r5-stat trial of the gate statistics with the tools/r5-trials workspace, docs/r5/targets.toml and the attribute files of tools/r5-trials, docs/r5 and fixtures/r5; the r5-routine trial, which fixes the three routine constants in docs/r5/targets.toml at 3-60-5, with its seeds file, its result file and the sample of 135 trains in fixtures/r5/trains/sample.jsonl; the r5-drift drift harness and admission trial: the case reader of contracts.md L9, the five counts of the L9.3 paragraph, the page model, the arms none, plain and admit, the 16 development cases under fixtures/r5/cases/dev and docs/r5/cases.md; docs/r5/heldout.txt with the SHA-256 of the archive of the 24 held-out cases, which stay outside the repository until freeze point 2; the result of the admission trial in docs/r5/trials/admission.md, whose verdict is rule=admit, and freeze point 1 in docs/r5/freeze.toml; and docs/r5/README.md and docs/r5/plan-revisions.md, which say what each file of the run's directory holds and which lines of docs/plan.md the run revises
- Added: Add ADRs 0004 to 0012 for what the capture run decided, docs/privacy-limits.md on what expiry and delete-all cover and which private windows capture tells, and a ci.d check of the ADR headers
- Changed: Name the capture daemon in docs/plan.md as the only opener of the capture store and the app as its starter, with a dated revision line for each edit
- Fixed: Cite the research lines that state the facts in ADRs 0001 and 0003 and record the crates of the workspace in ADR 0002, each by a dated amendment
- Added: Add three units of the agent-setup run: the local fixture sites (utility, payment frame, shop, mail and calendar faces, control) with three invented seeds, nine variants, a state object and a self-test, listed in docs/r3/fixtures.md; the web executor trial (tools/r3-trials/web), a chromiumoxide adapter, nine capability steps run with no model against the fixture sites, a fault proxy that makes each step miss, and the result file docs/r3/trials/web-executor.md; and the sheet trial of U0.4 with its corpus of eight workbooks and its result, which names umya-spreadsheet for U2.3
- Added: Add the router crate halvo-route with its tiers, call kinds, categories, line, price file and policy types, and the gate tool route-gate with its generator, scorer, self-test and the check 30-r6-table; the gateway request crate halvo-route-wire, which reads and writes the body of a hosted model call and reads its two answers, with its limits, schema and vectors under docs/r6/gateway/ and the check 20-r6-gateway; LF line ends under docs/r6/ and scripts/r6/ in every clone, with docs/r6/README.md saying what its directories hold; and the record of which of the four routing models may enter the model trials, with their licence lines and the size and SHA-256 of each 4-bit file that enters
- Changed: Give each CI job a 60-minute timeout in place of 30 minutes, so that the checks of the later runs fit, with the bound ci_job_timeout_min and the self-test case that reads it
- Added: Add docs/r4/alive.md, the interface run's table of which parts of Halvo may be alive at once, with the bound in bytes of each part for which another run has set one, and docs/r4/README.md, which says what each file of docs/r4 holds and which unit adds it
- Added: Add the crate halvo-schema with the id types, the parsed app, domain and site types, the workflow, summary, support and evidence records, the request envelopes over four roles with their compile-fail test, the two strict decoders and the reply writer; and `route-gate import`, which reads the trial files docs/r3/trials/driver.md and docs/r3/trials/model-cells.md and writes the four outcome files, with three golden cases and the check 35-r6-outcomes
- Added: Add docs/r2/category-parameters.md, the table that gives each of the ten seed categories its four parameters from the capture layer (category, site, trigger and recurrence), for later rows with the source open to join
- Added: Add the browser profile trial of the agent-setup run (tools/r3-trials/profile): a session exported from one headless profile and imported into a new empty one, a search of the removed profile's parent and of the run's temporary entries for the page marker, and the tool tree-peak for the memory of a process tree; the web trial library gains the export and import of cookies and localStorage, a start that makes the profile and a temporary directory of the browser's own and refuses an existing or linked path, and a stop that removes both, ends only processes that still have their recorded creation time and reports every process id of the tree, with the results in docs/r3/trials/browser-profile.md
- Added: Add the crate halvo-coord of the interface run: the coordinator's ids, names, limits, ports and memory ports, the words table with its export, the list of the page's commands and the 35 page fixtures written from the Rust types; and scripts/r6/layout.txt, the routing run's 66 path patterns with the unit that owns each, and the check 10-r6-layout.sh, which fails when a tracked file under one of the run's seven roots matches no pattern and, called with --all, when a pattern of a unit not named after --waits has no file
- Fixed: Make scripts/prerequisites.sh create `~/.cargo/bin` before it copies the prebuilt cargo-release there, so that a machine with no such directory, such as the hosted macOS runner, gets the prebuilt binary and does not build the tool from source; when the download, the unpacking or the copy fails, the script names that step before it falls back to cargo install; with `sh scripts/prerequisites.sh --self-test`
- Added: Add crates/halvo-cap, which runs one child under a memory cap and reads its peak (a Job object on Windows; on macOS a 250 ms poll of the lifetime peak physical footprint and a kill on a breach), with the binary measure-peak and its control child, the per-user model lock, the example hold that probes what the reading does not count (the 8 names of the macOS list and the 4 of the Windows list), scripts/r2/ci.d/11-cap.sh and docs/r2/trials/cap.md; the halvo-redact crate, whose mask function replaces card numbers by the Luhn check, IBANs by mod 97, US Social Security numbers by format and keys by 72 prefixes of 27 rules of the default configuration of gitleaks v8.30.1 with a token, with its list of documented misses and the ci.d check 22-redact; and three cases on two new fixture repositories in the self-test of 10-r6-layout.sh, for its glob matcher
- Added: Add tests for the rules of halvo-cap that stood without one: the Job drops its working-set maximum on the Windows error 1314 alone and returns each other error that the test tries (1200 to 1430, 2, 5, 6, 8, 87, 1450, 1453 and 1455); measure-peak exits with status 1 when any one of its three checks misses, also after a run that the cap stopped while the other checks are ok; a relative cache variable is refused with the words `error no model lock: the variable HOME is not set to an absolute path` (LOCALAPPDATA on Windows); a median equal to the limit passes the checks, and on macOS a read equal to the limit leaves a run uncapped while a read one byte above it kills the run; and the self-test of scripts/r2/ci.d/11-cap.sh reads the exit status beside the last line of enter_repository and of measure, and rejects 15 failing fixtures. The boundary test of halvo-redact plants every one of the 72 key prefixes of the table, 27,825 cases in all
- Added: tools/r2-probes, the probes of the P0 cost trials, and the trial files layer-cost.md, input-visibility.md and private-window.md under docs/r2/trials/.
- Added: Model the coordinator's schedule and its confirmations in TLA+, with the twenty-one invariants that TLC checks, each read from a history that no guard reads, and the traces that the code will replay
- Added: Add to the crate halvo-route the ruling (`ruling=none`, with a hosted value in the crate's own unit tests alone), the pick from a policy table, a call's cell and the agent's ceiling to a tier, a hand back with its cause or no model, the default cap line (2,000 minor units a month, provisional), and the module release that compiles in the ruling, the first policy table of 44 rows and the cap line and fails closed on each file that does not parse
- Added: Add the interface run's layout check: scripts/r4/layout.txt holds the 13 roots and the 78 path patterns of I14, and scripts/r4/ci.d/10-r4-layout.sh fails in both CI jobs when a tracked file under a root matches no pattern (`layout outside=<path>`), or with `--all` when a pattern has no file, with a self-test of 57 failing fixtures.
- Added: Add docs/r3/category-parameters.md, the 22 parameter rows of the agent-setup run with the source and the binding of each, docs/r3/README.md, scripts/r3/layout.txt, the 54 patterns of the run's file table, and the check 10-r3-layout, which fails when a tracked file under one of the run's roots matches no pattern
- Added: Complete the crate halvo-schema with the kind registry (field types, retention classes, origins, the four kinds of this run, the entry checks and the record check), Coarse with its compile-time assertion, the read trait and the step traits, REPLY_MAX, the four files of later runs, export-names with the NAMES lists, the golden files with their tests, the check 20-schema and tests of three rules of its first part; the record check refuses a key of a nested object that is no name, that is of digits alone or that stands twice, and a record that nests deeper than 128 levels; Coarse is sealed, so that only src/kinds.rs implements it; and tests/locked_files.rs refuses in the seven locked files a path attribute, the name include, macro_use and a call of a macro that no locked file defines, and in the four files of later runs macro_export
- Changed: An app id of halvo-schema refuses every control character of the Unicode category Cc, which is U+0000 to U+001F, U+007F and U+0080 to U+009F, where it refused U+0000 to U+001F and U+007F alone, so that an app id with a character of U+0080 to U+009F, which main took before, is refused now
- Added: Add the crate halvo-seam, the read door between the capture store and the analysis: the capture record with its reader and writer, the seam messages with their length-prefixed framing, the client, the fixture transport over batch files, the input-set reader with the reduction and docs/r2/input-sets/app-title.toml, the exported conformance suite with controls for some of its checks and a store replaced while a cursor is open, and the check 21-seam
- Fixed: The conformance suite of halvo-seam reports four more kinds of wrong server, each with a control: a filtered cursor opened at a position other than zero that serves from the start of the store, another store id at the zero position that opens a cursor, a list of kinds in another order than C4.4's that gets bad-request, and a position two past the last line of an inner segment that gets bad-position; the four checks of the suite that compare a server's replies and had no control each have one; tests and self-test fixtures now fail on ten one-rule changes that passed before, which touch the client's position after store-changed or store-error to an open, the fixture transport's replace mark after a close or the end of a connection, the suite's look-ahead check and its count of stops in set C, the error of an expiry segment above 2^63 - 1, and three ways out of 21-seam.sh; and the input-set reader refuses a symbolic link, also one that names a regular file
