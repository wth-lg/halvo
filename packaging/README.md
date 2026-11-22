# Packaging

This directory holds what the packaging of the app reads. One command builds the
Mac app bundle, sealed ad hoc, with four helpers inside it, and one disk image
that holds the bundle. The helpers are the capture daemon `halvo-capture`, the
owner of the derived store `halvo-owner`, the analysis worker `halvo-analysis`
and the runner of an agent `halvo-runner`:

```sh
npm run tauri build -- -- --locked
```

It writes `target/release/bundle/macos/Halvo.app` and the disk image
`target/release/bundle/dmg/Halvo_<version>_aarch64.dmg`. Each helper's binary lies in
`Halvo.app/Contents/MacOS/`, beside the app's binary `halvo`, as an external
binary of the Tauri bundler. The daemon starts `halvo-owner` and `halvo-analysis`
from the directory of its own binary, so they ship beside it. On a Mac the app
looks for `halvo-runner` beside its own executable when a person starts an agent,
so the runner ships there too. The check `scripts/r1/ci.d/70-package-macos.sh`
builds the bundle by that command and reads it, and it fails when
`Contents/MacOS` holds an entry other than `halvo` and the helpers. It also
copies the app out of the disk image, starts the copy hidden on a scratch data
directory and reads that its one daemon child answers and stops. On Windows
the same command makes a per-user installer, and
`scripts/r1/ci.d/70-package-windows.sh` unpacks it with no install and fails when
an `.exe` other than `halvo.exe`, the helpers and the template's `uninstall.exe`
stands beside `halvo.exe`. No person installs the disk image or the installer yet.

## Helpers

Each file of `helpers.d/` names one helper binary that ships beside the app. The
file's name is the installed name. It holds three lines:

```text
package=<the cargo package>
bin=<the binary of that package>
manifest=<the package's Cargo.toml, from the repository root>
```

The file of a helper that has no `--version` argument holds a fourth line:

```text
version=none
```

`sh scripts/r1/ci.d/62-build.sh --files`, by which check 70 reads what it built,
starts each file with `--version`. It reads a helper with that fourth line for
the markers and the imports as every other file, and does not start it. The
files of `halvo-owner` and `halvo-analysis` hold the line.

For `build` and `bundle`, and for no other command of the Tauri CLI,
`scripts/tauri.js` does this in order:

1. It runs `sh scripts/r1/ci.d/54-tag-needs-store4.sh` and stops when that fails.
2. For each helper it runs, at the repository root,
   `cargo build --release --locked --manifest-path <manifest> -p <package> --bin <bin>`,
   with no feature and no test target. These are the same builds as the second
   to the fifth call of `scripts/r1/ci.d/62-build.sh`, so cargo builds the
   dependencies once for both.
3. It copies the built binary to `target/helpers/<name>-<triple>`, with `.exe` at
   the end on Windows. The triple is the `host:` line of `rustc -vV`. It copies
   only when the bytes differ.
4. It starts the Tauri CLI with `--config` that sets `bundle.externalBin` to
   `../target/helpers/<name>` for this run alone. On a Mac the same `--config`
   sets `bundle.macOS.signingIdentity` to `-`, and the CLI starts with `CI=true`.

No tracked configuration names an external binary. Tauri's build script requires
each external binary to exist, so `cargo test` on a clean checkout would fail if
`src-tauri/tauri.conf.json` named one. Check 70 fails when a tracked file under
`src-tauri` names `externalBin`.

The wrapper refuses `--target` and `CARGO_BUILD_TARGET`: it stages the helpers for
the host triple alone. With `--debug` it still stages the release build of each
helper.

## Signing

On a Mac the bundle is sealed ad hoc. `scripts/tauri.js` gives the Tauri CLI the
signing identity `-` for that build alone, so the bundler signs `halvo`, the
helpers and the bundle with no certificate and no identity. This seal is not the
owner's signing. Check 70 reads that `codesign --verify --deep --strict` passes
on the copy out of the disk image, and that `codesign -dv` names an ad hoc
signature and a bound `Info.plist`.
Each build gets a new ad hoc seal, and macOS ties a permission that a person grants to that seal, so until the owner signs the app, a person who installs a rebuilt Halvo grants its permissions in Privacy & Security again.

On Windows the installer is unsigned. On both systems `scripts/tauri.js` takes
every variable whose name starts with `APPLE_` or `TAURI_SIGNING_`, and
`HALVO_WINDOWS_SIGN_COMMAND`, out of the environment of each command that it
starts. Check 70 fails when such a variable is set, and when a tracked file under
`src-tauri` names `signingIdentity`, so no tracked file holds the identity `-`.

The function `sign_step` in `scripts/tauri.js` is the place of signing and
notarization. The owner decides how the app is signed (item I-116). The wrapper
calls it after a `build` or `bundle` that exits 0 and made a bundle, so not with
`--no-bundle`. It does nothing yet.

## Minimum macOS version

The bundle's Info.plist names 14.0 as its LSMinimumSystemVersion, from bundle.macOS.minimumSystemVersion in src-tauri/tauri.macos.conf.json, so macOS opens the app on macOS 14.0 or later alone.
Check 70 reads that number in the copy out of the disk image, and that its major
is `MACOS_FLOOR_MAJOR` of `crates/halvo-control/src/onboarding.rs`.

## Disk image

The same file `src-tauri/tauri.macos.conf.json` names the targets `app` and
`dmg`, so the build makes the disk image with Tauri's own script. The image holds
`Halvo.app` and a link to `/Applications`. The wrapper starts the CLI with
`CI=true`, so the script skips its Finder step and drives no Finder window; a
hosted runner builds the image. The image has no layout of its own: a person sees
the app and the link to Applications where Finder puts them.

A hosted runner's hdiutil now and then answers `hdiutil: create failed - Resource
busy` when the script makes the image. When the CLI's call fails and its output
holds that line, the wrapper says so on standard error and runs the CLI once more;
it tries no second time, and for no other failure.

Check 70 requires exactly one `*.dmg` in `release/bundle/dmg` of the target
directory, newer than the start of its build. It attaches the image read-only and
unbrowsed under its scratch directory, copies `Halvo.app` out of it with `ditto`
and detaches it, also when it fails. It writes a state file into a scratch data
directory, so that the app reads the directory as provisioned and starts its
daemon with no consent, and starts the copy's `halvo` with `--background` and
`--data-dir` on it. It requires exactly one `halvo-capture` of the copy with that
app as its parent, a status that reads the store open, a `ctl stop` that exits 0
with no second child after it, and no process of the copy after the app's end.
The app counts the daemon as its own child only after one of its polls, which
come every `POLL_MS` of `crates/halvo-control/src/lib.rs`, has read the daemon's
pid, and it starts a child again after the end of a daemon that it does not count
as its own; so the check sends `ctl stop` two poll periods and one second after
the daemon's first answer.
It ends only the app that it started, by its pid.

## Not yet here

- The notices file, a unit of its own.
- The owner's signing and notarization (item I-116).
- A check of the minimum macOS version that each binary names in its own build; the check reads the bundle's floor alone.
- The list of modes that only a debug build has.
