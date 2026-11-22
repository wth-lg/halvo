# Packaging

This directory holds what the packaging of the app reads. One command builds the
Mac app bundle, unsigned, with three helpers inside it: the capture daemon
`halvo-capture`, the owner of the derived store `halvo-owner` and the analysis
worker `halvo-analysis`:

```sh
npm run tauri build -- -- --locked
```

It writes `target/release/bundle/macos/Halvo.app`. Each helper's binary lies in
`Halvo.app/Contents/MacOS/`, beside the app's binary `halvo`, as an external
binary of the Tauri bundler. The daemon starts `halvo-owner` and `halvo-analysis`
from the directory of its own binary, so they ship beside it. The check
`scripts/r1/ci.d/70-package-macos.sh` builds the bundle by that command and reads
it. Nobody installs this bundle yet.

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
   with no feature and no test target. These are the same builds as the second,
   third and fourth call of `scripts/r1/ci.d/62-build.sh`, so cargo builds the
   dependencies once for both.
3. It copies the built binary to `target/helpers/<name>-<triple>`, with `.exe` at
   the end on Windows. The triple is the `host:` line of `rustc -vV`. It copies
   only when the bytes differ.
4. It starts the Tauri CLI with `--config` that sets `bundle.externalBin` to
   `../target/helpers/<name>` for this run alone.

No tracked configuration names an external binary. Tauri's build script requires
each external binary to exist, so `cargo test` on a clean checkout would fail if
`src-tauri/tauri.conf.json` named one. Check 70 fails when a tracked file under
`src-tauri` names `externalBin`.

The wrapper refuses `--target` and `CARGO_BUILD_TARGET`: it stages the helpers for
the host triple alone. With `--debug` it still stages the release build of each
helper.

## Signing

The bundle is unsigned. `scripts/tauri.js` takes every variable whose name starts
with `APPLE_` or `TAURI_SIGNING_`, and `HALVO_WINDOWS_SIGN_COMMAND`, out of the
environment of each command that it starts. Check 70 fails when such a variable
is set, and when a tracked file under `src-tauri` names `signingIdentity`.

The function `sign_step` in `scripts/tauri.js` is the place of signing and
notarization. The owner decides how the app is signed (item I-116). The wrapper
calls it after a `build` or `bundle` that exits 0 and made a bundle, so not with
`--no-bundle`. It does nothing yet.

## Not yet here

- The seal of the bundle, the disk image and the notices file.
- The minimum macOS version of the bundle (14.0) and the list of modes that only
  a debug build has.
- Any start of the daemon or of the app from the bundle.
