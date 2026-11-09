# Plan: 20261108-halvo-kickoff

status: executing
planner: planner-1
inputs: ~/.wstack/runs/20261108-halvo-kickoff/kickoff.md, ~/.wstack/runs/20261108-halvo-kickoff/run.md, ~/.wstack/runs/20261108-halvo-kickoff/reports/researcher-1.md, researcher-2.md, researcher-3.md, researcher-4.md, researcher-5.md, researcher-6.md, researcher-7.md, researcher-8.md, researcher-9.md
done-when: the checks under '## 4. Done when' in ~/.wstack/runs/20261108-halvo-kickoff/kickoff.md
rigor: high; copied from run.md
harness: the section 4 checks are the harness; run them at the halvo-scaffold head SHA: check-plan.sh on plan.md, actionlint on .github/workflows/, a fresh clone plus the README Prerequisites, Build and --version flow, the Release dry run, the git ls-files filter, the docs/ byte-diffs, and the ADR and decisions.tsv checks, all listed under '## 4. Done when' in ~/.wstack/runs/20261108-halvo-kickoff/kickoff.md
baseline: captured in P0; measured by running the README '## Build' command on the one-commit README-only repo and recording its non-zero exit; limited by being a single run on this Mac where the empty repo has no project to build, so it shows only that the build fails before the scaffold exists
checkpoint: none
trail: PR-body summary

## Phases

### P0: Build baseline
goal: record that the documented build command fails on the pre-scaffold repo, as the builder's first commit
hypothesis: n/a
units: the builder, in the halvo-scaffold worktree, adds the README '## Build' command, runs it on the README-only repo, confirms a non-zero exit (no project to build), and commits that as the failing-first commit with the captured output per kickoff 3.3; this is the build command failing, not a product test
owner: builder x1
writes: the halvo-scaffold worktree first commit (README '## Build' stub and the captured failing output)
fan-out: none
depends-on: none
gate: verifier | the first commit on halvo-scaffold records the README '## Build' command exiting non-zero on the README-only repo, with the output captured in the commit
review: none
on-fail: if the empty-repo build unexpectedly succeeds or the commit is missing, re-run and re-commit the baseline; do not start P1

### P1: Scaffold
goal: build the minimal scaffold of kickoff 3.3 so later runs start from it, with no product feature beyond the version window and --version flag
hypothesis: if the scaffold is built as specified then static analysis passes on its head SHA and both CI jobs build the unsigned app
units: twelve scaffold items, listed below, one builder, no builder branches
  - 1 framework and language: Tauri 2 (Rust core, minimal system web view UI); crate tauri = "2" (resolves to 2.12.5), @tauri-apps/cli "^2" (2.x), Rust stable pinned in rust-toolchain.toml (builder pins the exact patch; Cargo.lock and package-lock.json record all resolved versions); macOS Xcode command line tools and Node 26.12.0 are already present on this Mac
  - 2 repo layout: one Tauri app in a Cargo workspace at the repo root (src-tauri/ Rust crate plus a minimal ui/ directory; root Cargo.toml workspace, CHANGELOG.md, README.md, .gitignore, docs/, .github/workflows/, scripts/); the item 13 backend lives in a separate repository, so capture code and account, billing and metering code never share a module; consistent with the item 2 decisions.tsv row
  - 3 build command and app path: npm run tauri build on both platforms; macOS unsigned app at src-tauri/target/release/bundle/macos/Halvo.app, Windows unsigned exe at src-tauri/target/release/halvo.exe (plus the NSIS installer under src-tauri/target/release/bundle/nsis/); the builder confirms the exact macOS .app path by running the build, since lane 8 left it unstated
  - 4 prerequisites: one user-level command with no sudo, sh scripts/prerequisites.sh, which installs rustup and the pinned toolchain (curl https://sh.rustup.rs piped to sh -s -- -y), runs npm ci for @tauri-apps/cli, and cargo install cargo-release 1.1.7; idempotent, safe to run twice; Xcode command line tools and Node are already installed, so the command does not reinstall them
  - 5 version source of truth: Cargo.toml [package] version = "0.0.0" is the single source; the window and the --version flag both read env!("CARGO_PKG_VERSION"); tauri.conf.json inherits the crate version; cargo-release reads and bumps Cargo.toml; the builder confirms changing Cargo.toml changes the --version output
  - 6 release tool: cargo-release, dry run by default (writes nothing without --execute); README '## Release' command is cargo release patch --dry-run; configured with tag-name v{{version}} and pre-release-replacements that promote CHANGELOG.md '## [Unreleased]' to '## [0.0.1]'; the builder confirms the dry run prints next version 0.0.1, tag v0.0.1 and the '## 0.0.1' section promoted from '## Unreleased', and that git status --porcelain, git tag and gh release list stay unchanged or empty
  - 7 CI workflow: one file .github/workflows/ci.yml, two literal jobs written out (no matrix), one runs-on: macos-15 and one runs-on: windows-2025, triggered on push for every branch and on pull_request; each job runs the Prerequisites then the Build command, caches Cargo with Swatinem/rust-cache v2.9.3 and npm with actions/setup-node v7 cache, and uploads the unsigned app with actions/upload-artifact v7.0.2; actionlint v1.7.12 must pass; the builder confirms the build is unsigned by default (no signing secrets, codesign -dv shows no identity on macOS), since lane 8 left the default unstated
  - 8 CHANGELOG.md: Keep a Changelog layout with an '## [Unreleased]' section listing every scaffold commit
  - 9 README.md: sections '## Build', '## Prerequisites', '## Version', '## Release' and one paragraph stating the product
  - 10 .gitignore: includes .claude/worktrees/, plus target/ and node_modules/
  - 11 docs/adr/: one ADR per decision 1 to 3, each file's first heading the section 5 label verbatim (app framework and language; repo layout; CI and release tooling) and a decision: line quoting the ts of its decisions.tsv row; the builder brief lists ~/.wstack/runs/20261108-halvo-kickoff/decisions.tsv under Inputs and pastes the rows for items 1 to 3
  - 12 doc copies and filter: docs/research/researcher-1.md to researcher-9.md and docs/plan.md as byte copies of the ledger files at the scaffold commit; and no tracked path outside docs/ and .github/workflows/ matches capture|observ|workflow|agent|model|infer|storage|encrypt|permission|billing|subscri|account case-insensitively, so any Tauri template path that matches (for example a generated permissions/ directory) is renamed or dropped; the builder confirms with git ls-files piped through that filter returning nothing
owner: builder x1
writes: the halvo-scaffold worktree (src-tauri/, ui/, root Cargo.toml, .github/workflows/ci.yml, CHANGELOG.md, README.md, .gitignore, scripts/prerequisites.sh, docs/adr/, docs/research/, docs/plan.md)
fan-out: none
depends-on: P0
gate: static-analyzer | STATUS: PASS on the scaffold head SHA, then the eng-manager pushes halvo-scaffold and the Actions run on that SHA finishes with both jobs successful
review: extra
on-fail: revert the halvo-scaffold worktree to the P0 baseline commit, re-brief the builder on the failing item, and do not push until static-analyzer STATUS: PASS

### P2: Whole-work verification
goal: the done-when predicate holds on the real scaffold at the halvo-scaffold head SHA
hypothesis: n/a
units: the verifier runs every check under '## 4. Done when' in the kickoff on a fresh clone at the head SHA and records the full 40-character SHA
owner: verifier x1
writes: none
fan-out: none
depends-on: P0, P1
gate: verifier | the done-when predicate holds at the halvo-scaffold head SHA (the checks under '## 4. Done when' in ~/.wstack/runs/20261108-halvo-kickoff/kickoff.md)
review: none
on-fail: record NOT VERIFIED with evidence, park the run and replan the failing item; for an Actions billing or quota failure the eng-manager parks and asks the owner per kickoff section 0

## Standing rules
- Each phase gets one decisions.tsv row through the decision-log helper (~/.wstack/runs/20261108-halvo-kickoff/bin/decision-log/log.sh) when it lands or fails its gate, not a batch at the end.
- No builder branches exist in this run; one builder owns the halvo-scaffold worktree.
- The eng-manager alone pushes halvo-scaffold and opens the draft PR; nobody posts PR comments.

## Open decisions
none

## Roadmap
These are the product phases for later runs. They are never executed by this run. Each deferred decision carries an open: line on the entry it blocks; the next run fans out one option-researcher per option, then an eng-manager review, before a second planning pass.

### R1: Capture
goal: build the always-on capture pipeline for macOS and Windows using the lane-4 ranked APIs, persisting an encrypted capture stream within the observation window, never defeating secure input
owner: builder and researcher (counts set by the next run)
writes: the product capture module and its on-device encrypted store
fan-out: per option for the deferred decisions below
depends-on: the P1 scaffold
gate: verifier | a measured multi-day capture run stays within the privacy model and the lane-7 cost bounds, and secure-input fields are never captured
open: capture pipeline architecture: options ScreenCaptureKit plus Windows.Graphics.Capture event-driven frames, Desktop Duplication full-screen fallback, Core Audio taps plus WASAPI loopback, Quartz Event Services plus Raw Input
open: observation window and whether it is configurable: options fixed 7 days, fixed 14 days, user-configurable 7 to 14 days, continuous with a rolling window
open: local storage, encryption and deletion design: options SQLCipher page-encrypted SQLite, libsodium per-segment crypto-shred, LanceDB or sqlite-vec for vectors, one at-rest key in the Secure Enclave or DPAPI user scope
open: audio and camera capture, with options: options off by default, system-audio only, microphone opt-in, camera opt-in, all opt-in per modality
open: distribution channel per platform, since stores may forbid global capture: options Developer ID notarized outside the Mac App Store with a signed installer outside the Microsoft Store, store builds with reduced capture, direct download only

### R2: Workflow identification
goal: identify recurring tasks and automation opportunities from the capture stream on-device and surface them to the end user
owner: builder and researcher (counts set by the next run)
writes: the product analysis module and the workflow store
fan-out: none
depends-on: R1
gate: verifier | on a labelled capture sample the identifier proposes the seed workflow categories with precision and recall recorded against a held-out set

### R3: Agent setup
goal: configure one agent per identified workflow with a preset model tier, reasoning level, budget and autonomy, and decide whether each agent is confirmed before it runs
owner: builder and researcher (counts set by the next run)
writes: the product agent-configuration module
fan-out: per option
depends-on: R2
gate: verifier | each seed workflow yields a valid agent configuration and the confirm-or-unattended policy matches lane 6, where every shipping agent confirms before a purchase or payment and hands back login and 2FA, and unattended money movement is not a default
open: whether the end user confirms each agent before it runs: options confirm every run, draft-and-confirm for sensitive actions only, unattended for non-sensitive actions with a per-agent allowlist, fully unattended after a one-time approval

### R4: Orchestration interface
goal: one interface that shows every agent, its settings and its reasoning level and coordinates them (the owner's Grok-bot reference), and governs which guiding questions the app may ask
owner: builder (count set by the next run)
writes: the product interface module
fan-out: per option
depends-on: R3
gate: verifier | the interface lists all agents with settings and reasoning level and the end user completes the seed tasks without opening a terminal or writing a prompt
open: how agents are represented and orchestrated: options a single-process orchestrator with in-app agent records, a Prime-Agent-style continual harness, independent per-workflow runners under a coordinator, an RLM-style recursive subagent kernel
open: which guiding questions the app may ever ask the end user, and how they are batched: options never ask, ask only on a hard block batched once per session, ask per workflow at setup, ask on spend-cap and credential events only

### R5: Learning loop
goal: keep refining the user model and the agents on-device without moving data off the machine, within consumer-hardware limits
owner: builder and researcher (counts set by the next run)
writes: the product learning module and the playbook store
fan-out: per option
depends-on: R3
gate: verifier | a multi-day run shows measured improvement on a repeated seed task with all learning state on-device and deletable
learning options (lanes 1 and 2, bounded by consumer hardware and the no-upload retention reading): inference-only RLM over a local 8B model (fits a 32 GB laptop; its training does not), frozen-weights context and memory editing (Agentic Context Engineering playbook, Agent Skills folders, ReasoningBank), local retrieval (LanceDB or sqlite-vec with a small Apache-2.0 embedding model), optional small on-device LoRA (mlx-lm on a 32 GB Mac, Unsloth on an 8 GB NVIDIA laptop); cluster-scale weight-update self-improvement and RLM training near 48 H100-hours are out of laptop scope

### R6: Model routing by budget and task
goal: route each task to a model by the end user's budget tier and the task's complexity, surface the nuance only where a non-frontier model falls short, and set the default spend cap
owner: builder and researcher (counts set by the next run)
writes: the product routing module
fan-out: per option
depends-on: R3, R7
gate: verifier | on the seed task categories the router picks the cheapest tier that meets a published quality bar and never exceeds the spend cap
hypothesis status (lane 5, published comparison): supported for the largest open-weight models on general desktop and browser tasks; contradicted for the small laptop-fitting models on long-horizon computer use and terminal work; untested for the landlord tasks, which no published comparison covers
open: model providers and the retention term for each: options on-device open weights only, open weights via a zero-retention host, closed frontier via a zero-retention agreement, a mix by task tier
open: model routing by budget tier and task complexity, and the default spend cap: options fixed at about 20 dollars a month to low-cost models, tiered 20 versus higher to frontier, a per-task complexity classifier, a default spend cap at 50 a month that is configurable

### R7: Subscription and billing with its backend
goal: let the end user subscribe, enter a card and a spend cap with the fewest prompts, and meter model usage through a backend that holds no captured data
owner: builder and researcher (counts set by the next run)
writes: the separate backend repository (accounts, billing, metering) and the app's subscription view
fan-out: per option
depends-on: the P1 scaffold
gate: verifier | a test subscription, card entry and spend cap are set with the fewest prompts and the backend stores no field derived from capture
open: subscription, billing and backend architecture, holding no captured data: options hosted Stripe Checkout plus Billing with a self-hosted gateway enforcing the cap, a merchant-of-record such as Polar or Paddle with token metering, a thin API on Cloudflare Workers or Fly.io with Better Auth, an LLM gateway such as Vercel AI Gateway, LiteLLM or OpenRouter for per-user caps

## Foundations

### Core data structures
These are the product's records for later runs; they stay in plan.md and are not committed as code in this run.
- Capture stream event: event_id (UUID v7), captured_at (UTC instant plus a monotonic sequence), modality (enum: screen_frame, keystroke, pointer, foreground_app, window_title, system_audio, microphone, camera), foreground_app (string, bundle id or exe path), segment_ref (handle into the encrypted segment store), segment_key_id (integer, selects the per-segment key for crypto-shred), is_derived (bool, false for raw), retention_class (enum: raw, derived).
- Workflow model: workflow_id (UUID), label (string), trigger (sum type: app_context, schedule, manual), steps (ordered list of action descriptors), recurrence (observed count plus period), confidence (float 0 to 1), state (enum: proposed, approved, active, paused, deleted), agent_id (optional foreign key).
- Agent configuration: agent_id (UUID), workflow_id (foreign key), model_tier (enum: on_device, low_cost_hosted, frontier; no vendor name), reasoning_level (enum: low, medium, high), budget_cap (minor-currency integer plus a period enum), autonomy_mode (enum: observe, draft_and_confirm, unattended), required_permissions (set of OS permission ids), playbook_ref (handle to the context playbook or Agent Skills folder), state_version (integer, versioned per edit).

### Concurrency and shared state
- capture: always-on, the sole writer of the capture stream and the encrypted segments (macOS LaunchAgent after login; Windows per-user logon task). Owns the capture-stream store.
- analysis and workflow identification: a periodic background worker; reads the capture stream read-only, writes the workflow store and the playbooks. Owns the workflow store.
- agents: per-workflow runners; read the agent configuration and the playbook, write run logs and append playbook deltas. Own the run-log store; playbook writes are serialized through the analysis writer so no store has two writers.
- interface: the Tauri window; reads workflow and agent state read-only, writes user decisions (approve, spend cap, answers). Owns the user-decision store.
- shared state: the capture stream (capture writes, analysis reads), the version string (read-only, from Cargo.toml), and one at-rest root key per platform (Secure Enclave key on macOS, DPAPI user-scope key on Windows) that wraps the per-segment keys. Each store has exactly one writer.

### Frontend and backend boundary
- The desktop app sends the backend only: account identity for auth, billing (the payment provider's customer id and subscription state), and model-usage metering (per request: account id, timestamp, model tier, prompt and completion token counts, computed cost, request tags), per lane 9.
- Never crosses the boundary: any captured data (screen, keystrokes, pointer, audio, video), any derived data (workflow models, playbooks, screenshots, transcripts, embeddings), any file content, and any third-party-site credentials.
- The backend (item 13) is a separate repository holding a gateway with the vendor keys and the per-user spend cap, and no capture data.

### Zero-data-retention reading
- This plan builds on the no-upload reading: nothing captured or derived from capture ever leaves the machine; all such data stays on-device, encrypted, deletable and bounded by the observation window; only account, billing and metering data (no content) crosses to the backend.
- Why: the owner requires continuous cross-day learning (kickoff section 1.5), which the no-local-storage reading would make impossible; no-upload is the strongest guarantee compatible with the product and makes "the backend holds nothing captured" true by construction. The final reading is reserved for the owner and is the first parked question in handback.md; the privacy model below is written against the no-upload reading and holds a fortiori under the both reading.

## Privacy model
Each property is one checkable sentence; the check runs against the product when built.
- No captured or derived data leaves the machine.
  check: record the desktop app's outbound traffic to the backend during a capture-plus-analysis run with a local inspecting proxy -> every request body is auth, billing or metering JSON and contains no screen, keystroke, pointer, audio, image, transcript or embedding field
- Raw capture is kept no longer than the active observation window.
  check: inspect the segment store and key store for segments older than the configured window -> each such segment's per-segment key is destroyed and its ciphertext no longer decrypts
- The end user can see what is stored and delete it.
  check: open the app's data view, read the inventory, then trigger delete-all -> the capture stream, workflow store, playbooks, embeddings and segments are all removed
- Deletion removes derived state, not only raw capture.
  check: delete one workflow, then inspect the workflow store, its playbook files and the vector index after compaction -> no row, file or vector derived from that workflow remains
- Capture and derived data are encrypted at rest under a key usable without a prompt on an unattended machine.
  check: inspect key custody (macOS: a Secure Enclave key in the data-protection keychain owned by the signed LaunchAgent; Windows: a DPAPI user-scope key with no LOCAL_MACHINE flag) -> the root key unwraps after login with no prompt and no other user or admin can read the segments
- Derived state holds personal data only inside the encrypted on-device store.
  check: scan the filesystem for derived artefacts such as workflow models, transcripts, playbooks and embeddings -> none exists in plaintext outside the encrypted store
- The desktop app sends the backend only account, billing and metering data.
  check: test the backend request allowlist and schema against a sample that includes a capture-derived field -> the schema accepts account, billing and metering fields and rejects any field derived from capture

## Revisions
- none yet
