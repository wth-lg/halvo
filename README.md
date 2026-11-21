# Halvo

Halvo is a desktop app for macOS and Windows that lets a person with no technical
background get real work done through AI agents, without opening a terminal, writing
a prompt or choosing a model. The plan is in `docs/plan.md` and the decisions are in
`docs/adr/`.

This repository holds:

| Path | What it holds |
|---|---|
| `src-tauri/` | The app's Rust crate (Tauri 2), which builds the binary `halvo`. |
| `ui/` | The page that the app's window shows, and its tests. |
| `crates/` | The other Rust crates of the Cargo workspace. `crates/halvo-capture/` builds the second binary, `halvo-capture`, the process that records activity into an encrypted store on the computer. |
| `account/` | A second Cargo workspace, which the root `Cargo.toml` excludes. |
| `backend/` | The subscription and billing backend, a pnpm project with its own `README.md`. |
| `formal/` | Formal models, one directory for each. |
| `scripts/` | The prerequisites, release and repository check scripts, and the CI checks under `scripts/<run>/ci.d/`. |
| `docs/`, `tools/`, `fixtures/` | Plans, decision records, test tools and fixtures. |

## Prerequisites

```sh
sh scripts/prerequisites.sh
```

One command, run from the repository root: on macOS in Terminal, and on Windows in
Git Bash, the shell that comes with Git for Windows. Use Git Bash, not WSL: under WSL
the commands run as on Linux, and this repository builds no Linux app. The command
installs only as the
current user, with no sudo, and is safe to run twice.

It first checks that `curl`, `node` and `npm` are on PATH and, on macOS, that
`xcode-select -p` finds the Xcode Command Line Tools (`xcode-select --install`). It
names each one that is missing and stops before any download. On Windows it does not
check for the Visual Studio C++ build tools, which Rust needs there, and prints a
warning that says so. The GitHub Actions runners have all of these.

Then it:

- sets `core.hooksPath` to `.githooks` in this clone, so git runs two hooks: the
  commit-msg hook rejects an attribution line in a commit message before the commit
  is made, and the pre-push hook refuses a push that would add a commit with such a
  line. If another git setting overrides the path, the command names that setting
  and stops;
- installs rustup and the Rust toolchain that `rust-toolchain.toml` pins, with the
  components that file lists (`clippy` and `rustfmt`), into `~/.rustup` and
  `~/.cargo`;
- installs the Tauri CLI into `node_modules/` with `npm ci`;
- installs cargo-release into `~/.cargo/bin` from the prebuilt archive of its release,
  after it has checked the archive's sha256 against the digest that the script pins.
  A digest that differs, an archive that does not unpack, a copy that fails or an
  installed cargo-release that does not report the pinned version stops the command.
  Only a failed download, or a platform with no prebuilt archive, makes it build
  cargo-release with `cargo install` instead. A version check that fails after
  the install leaves the new binary in place, and the next run's presence check
  installs again;
- installs cargo-deny from its prebuilt archive after the same kind of sha256 check,
  fetches the crates of `account/Cargo.lock`, and
  sets up the backend: the pinned pnpm, the packages of its lock, its pinned tools
  and the licences of its packages.

rustup is installed with `--no-modify-path`, so after a first install `cargo` is not
on the PATH of the shell. Every command that this file documents and that runs cargo
puts `~/.cargo/bin` on its own PATH, so none of them needs that. To run `cargo` yourself, add
`~/.cargo/bin` to PATH.

Supported systems: macOS 14 or later on Apple silicon (arm64), with no build for Intel
Macs, and Windows on x86_64. The macOS floor is a plan that the app bundle does not
yet declare: `src-tauri/tauri.conf.json` sets no minimum system version, so the
bundle carries Tauri's default until the packaging work sets 14.

## Build

```sh
npm run tauri build
```

Run from the repository root. `npm run tauri` runs the Tauri CLI through
`scripts/tauri.js`, which puts `~/.cargo/bin` on PATH. The command builds the Rust
crate in `src-tauri/` in release mode with the page in `ui/` and, on macOS, bundles
it as an app. The output is unsigned: no signing identity is configured, so macOS
applies only the ad-hoc signature that the linker adds to every arm64 binary.

| Platform | Build command | Unsigned app |
|---|---|---|
| macOS | `npm run tauri build` | `target/release/bundle/macos/Halvo.app` |
| Windows | `npm run tauri build -- --no-bundle` | `target/release/halvo.exe` |

On Windows the app is the bare executable and no installer is produced; the bundle
targets in `src-tauri/tauri.conf.json` are `["app"]`. The `target/` directory is at
the repository root because the root `Cargo.toml` is a Cargo workspace. On macOS,
running the binary with `--version` prints the version and exits before any window
opens:

```sh
target/release/bundle/macos/Halvo.app/Contents/MacOS/halvo --version
```

### Tests, lint and the gate

CI runs its checks through one driver, the same on a developer's machine. Run it
from the repository root, with `macos` on macOS and `windows` on Windows in Git
Bash. The driver puts `~/.cargo/bin` first on PATH.

```sh
sh scripts/ci/run-ci-d.sh macos 10-format 11-clippy 12-test
```

- `10-format` runs `cargo fmt --check`.
- `11-clippy` runs `cargo clippy --workspace --all-targets --locked -- -D warnings`.
- `12-test` runs `cargo test` over every package of the workspace with the
  committed lockfile.
- `sh scripts/ci/run-ci-d.sh macos` runs every check of the macOS job,
  `sh scripts/ci/run-ci-d.sh --rules` prints the rule of each check, and
  `--self-test` after the job runs the checks' own cases. `scripts/ci/README.md`
  describes the driver.
- On a push and on a pull request CI runs the merge gate, the checks that
  `scripts/ci/merge-gate.txt` names for each job
  (`sh scripts/ci/run-ci-d.sh <job> --merge-gate`), and what
  `scripts/ci/slices.txt` names runs in six slices, one a day from Monday to
  Saturday and in a run by hand (`sh scripts/ci/run-ci-d.sh <job> --slice <slice>`).
- `sh scripts/r1-gate/run.sh --self-test` proves the logic of the R1 gate on
  fixtures, and `sh scripts/r1-gate/run.sh --help` lists its sub-checks and its
  measured form; `scripts/r1-gate/README.md` describes it.
- The checks of the repository's rules in `AGENTS.md`:
  `sh scripts/check-changelog.sh`, `sh scripts/check-line-endings.sh`,
  `sh scripts/check-own-source-reads.sh` and `sh scripts/check-commit-messages.sh`.

## Version

One version covers every package of the workspace and both binaries, `halvo` and
`halvo-capture`. `cargo metadata` reports it for both binaries from the `version`
of `[workspace.package]` in the root `Cargo.toml`, which is `0.0.0` now. The other
packages of the workspace inherit that value or write the same value in their own
manifests, and the
check `61-version` fails when any package reports another.

- Both binaries read it at compile time through `env!("CARGO_PKG_VERSION")`, in
  `src-tauri/src/app.rs` and in `crates/halvo-capture/src/bin/halvo-capture.rs`,
  and `halvo --version` prints it.
- The Tauri bundle takes its version from the same value, because
  `src-tauri/tauri.conf.json` sets no `version` of its own.
- A release sets every package of the workspace to the next version (see Release).

## Release

```sh
sh scripts/release.sh --dry-run
```

The release tool is cargo-release, configured in `release.toml` and
`src-tauri/release.toml`, and `scripts/release.sh` runs it with `~/.cargo/bin` on
PATH. The dry run prints the next version (`0.0.1` from `0.0.0`), the commit
message, the tag (`v0.0.1`) and the `CHANGELOG.md` section that the release would
write. It changes no tracked file: it writes `target/release-commit-message.toml`,
and cargo writes its own files under the ignored `target/`. Every dry run and every
release asks index.crates.io about each package of the workspace by its name, sends
nothing else from the repository, and needs the network.

```sh
sh scripts/release.sh --execute
```

The release runs on the branch `main` only and, in a clone with a remote named
`origin`, only when HEAD is the newest commit of `main` there. It sets every package of the workspace to the next version,
moves the entries of `## [Unreleased]` under a heading of that version, grouped by
type under the Keep a Changelog headings (`### Added`, `### Changed` and so on),
and makes one local commit and one tag `v<version>`. It pushes nothing. The date in
the new heading is the UTC date of the release.

No release is made before the store unit store-4 is on `main`. Until then
`--execute` stops before cargo-release while a tracked file under
`crates/halvo-capture/src` holds the statement that creates the search index
`events_fts`, which store-4 takes out, and it makes no commit and no tag; the dry run
prints the line where `--execute` would stop. Named limit: a tag that a person
pushes by hand before store-4 is on `main` still runs the build check of CI in a
red job, which uploads that build as an artifact.

Pushing the commit and the tag, a GitHub release, signing and publishing are the
owner's own steps.

Both forms take an optional bump level after the flag: `patch` (default), `minor` or
`major`. `CHANGELOG.md` follows Keep a Changelog. A pull request records its changes
in one new file under `changelog.d/`, named after its branch, with one line for each
type of change that it makes, `- <Type>: <text>`, where Type is Added, Changed,
Deprecated, Removed, Fixed or Security, and leaves `CHANGELOG.md` from its
`## [Unreleased]` heading down as it is. At a release `sh scripts/changelog-join.sh`
moves those lines to the end of the list under that heading, and
`sh scripts/check-changelog.sh` checks both.
