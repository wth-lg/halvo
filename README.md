# Halvo

Halvo is a desktop app for macOS and Windows that lets a person with no technical
background get real work done through AI agents, without opening a terminal, writing
a prompt or choosing a model. This repository holds the app. At this stage it builds
an unsigned app on both platforms that shows its version in a window and prints it
with `--version`; the product features come in later runs, see `docs/plan.md`.

## Prerequisites

```sh
sh scripts/prerequisites.sh
```

One command, run from the repository root. It installs only as the current user, with
no sudo, and is safe to run twice. It installs rustup and the Rust toolchain pinned in
`rust-toolchain.toml` into `~/.rustup` and `~/.cargo`, the Tauri CLI into
`node_modules/` with `npm ci`, and cargo-release into `~/.cargo/bin` from a prebuilt
binary (or with `cargo install` when no prebuilt binary matches the platform). It
also sets `core.hooksPath` to `.githooks` in this clone, so git runs the commit-msg
hook that rejects attribution lines in commit messages.

It expects `curl`, `node` and `npm` on PATH, plus the Xcode Command Line Tools on
macOS (`xcode-select --install`) or the Visual Studio C++ build tools on Windows.
The GitHub Actions runners already have all of these.

## Build

```sh
npm run tauri build
```

Run from the repository root. The command builds the Rust crate in `src-tauri/` in
release mode with the static page in `ui/` and, on macOS, bundles it as an app. The
output is unsigned: no signing identity is configured, so macOS applies only the
ad-hoc signature the linker adds to every arm64 binary.

| Platform | Build command | Unsigned app | Binary to run with `--version` |
|---|---|---|---|
| macOS | `npm run tauri build` | `target/release/bundle/macos/Halvo.app` | `target/release/bundle/macos/Halvo.app/Contents/MacOS/halvo` |
| Windows | `npm run tauri build -- --no-bundle` | `target/release/halvo.exe` | `target/release/halvo.exe` |

On Windows the app is the bare executable; no installer is produced yet. The
`--no-bundle` flag skips Tauri's bundling step, which would otherwise download
installer tooling at build time, and the config lists no Windows bundle target.

The `target/` directory is at the repository root because the root `Cargo.toml` is a
Cargo workspace. Double-clicking the app opens the version window; running the binary
with `--version` prints the version and exits before any window opens:

```sh
target/release/bundle/macos/Halvo.app/Contents/MacOS/halvo --version
```

## Version

The version lives in one place: the `version` field of `[package]` in
`src-tauri/Cargo.toml`, which is `0.0.0` now.

- The window and the `--version` flag both read it at compile time through
  `env!("CARGO_PKG_VERSION")` in `src-tauri/src/main.rs`. The window title is
  `Halvo <version>` and the page shows `Version <version>`; `--version` prints the
  bare version string, for example `0.0.0`, to stdout and exits with code 0.
- The Tauri bundle takes its version from the same field, because
  `src-tauri/tauri.conf.json` sets no `version` of its own.
- The release tool (see Release) reads that field to compute the next version and
  rewrites it when a release is executed.

## Release

```sh
sh scripts/release.sh --dry-run
```

The release tool is cargo-release, configured in `release.toml`. The dry run exits
0, writes nothing, and prints the next version (`0.0.1` from `0.0.0`), the tag
(`v0.0.1`) and the `## [0.0.1]` section it would write to `CHANGELOG.md` from the
`## [Unreleased]` section. The script runs cargo-release's own dry run, which prints
the version bump and the changelog diff, then adds the tag name and the full section
body, which cargo-release does not print.

To perform the release, which bumps `src-tauri/Cargo.toml`, promotes the changelog
section, commits and tags `v<version>` without pushing:

```sh
sh scripts/release.sh --execute
```

Both forms take an optional bump level after the flag: `patch` (default), `minor` or
`major`. `CHANGELOG.md` follows Keep a Changelog, and the release promotes its
`## [Unreleased]` section. Every change goes into that section as one line at the end
of the list, `- <Type>: <text>`, where Type is Added, Changed, Deprecated, Removed,
Fixed or Security. A pull request adds one entry for each type of change it makes,
and `sh scripts/check-changelog.sh` checks the file.
