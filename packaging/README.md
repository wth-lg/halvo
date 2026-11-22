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
2. It runs `node scripts/notices/generate.js --triple <triple> --out
   target/notices/THIRD-PARTY-NOTICES.txt` and stops when that fails (see Notices).
   The triple is the `host:` line of `rustc -vV`.
3. For each helper it runs, at the repository root,
   `cargo build --release --locked --manifest-path <manifest> -p <package> --bin <bin>`,
   with no feature and no test target. These are the same builds as the second
   to the fifth call of `scripts/r1/ci.d/62-build.sh`, so cargo builds the
   dependencies once for both.
4. It copies the built binary to `target/helpers/<name>-<triple>`, with `.exe` at
   the end on Windows. It copies only when the bytes differ.
5. It starts the Tauri CLI with `--config` that sets `bundle.externalBin` to
   `../target/helpers/<name>` and `bundle.resources` to the notices file for this
   run alone. On a Mac the same `--config` sets `bundle.macOS.signingIdentity` to
   `-`, and the CLI starts with `CI=true`.

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

## Windows installer

The installer installs Halvo for the person who runs it alone, into
`%LOCALAPPDATA%\Halvo` unless that person chooses another folder, with its
uninstall entry under `HKCU`. `src-tauri/windows/hooks.nsh` holds Halvo's hooks
of the installer template of the Tauri bundler, which
`bundle.windows.nsis.installerHooks` of `src-tauri/tauri.windows.conf.json`
names. `src-tauri/windows/English.nsh` is the template's English text of
tauri-cli 2.12.4, with the label of the uninstaller's box in Halvo's words; the
bundler takes it in place of its own, so a later CLI that adds a string needs the
string here too. The hooks hold every sentence that a person reads from them.

Before an uninstall, and before an install over an installed copy, the hooks stop
Halvo in this order: `halvo-capture.exe ctl stop` from the install folder (exit
69, where no capture service runs, is no fault), then the end of `halvo.exe` by
the template's own step, then a wait of at most 20 s until `halvo.exe` and each
helper of `helpers.d` can be opened for writing. A file that is still held stops
the uninstall or the install, before any file or value goes. Check 70 fails when
`hooks.nsh` waits for no helper of `helpers.d`.

Where the box is ticked, the uninstall first asks `halvo-capture.exe
--default-data-dir` for the program's own default data directory, and runs
`halvo-capture.exe --delete-all --data-dir` with the printed path as one
argument; the hooks hold no rule of the paths. At exit 0 the uninstall goes on,
and the template's own step removes the folder `%LOCALAPPDATA%\com.halvo.desktop`,
which holds the web view's folder `EBWebView`; where that folder still stands,
the hooks run that step again for at most 20 s, then name the folder. At exit 75,
or at any other end, the uninstall stops before any file or value goes. A silent
uninstall ticks the box by the switch `/DELETEALL`, and never without it:

```sh
uninstall.exe /S /DELETEALL
```

A silent uninstall without `/DELETEALL`, and an uninstall with the box not
ticked, delete nothing that Halvo stored: Halvo's data folder and the web view's
folder, with the reports of a crash of Halvo's window, stay on the computer. An
uninstall for an update (`/UPDATE`) deletes nothing either. Every other uninstall
takes the value `Halvo` away under `Software\Microsoft\Windows\CurrentVersion\Run`
of the person's own registry, whatever its text; an uninstall by another account
leaves the value of the first person.

After an install, where none of the values that the template's own WebView2
section reads is there, the installer says that the runtime is missing; a silent
install writes the sentence into `WEBVIEW2-RUNTIME-MISSING.txt` in the install
folder, which the uninstall removes. The installer downloads nothing.

On a GitHub-hosted runner alone, and on no other machine, check 70 installs the
built installer with `/S` into a scratch folder while the values that the
template reads for the runtime are moved aside, and puts them back before the
app starts and in its trap. It starts the installed app on a provisioned scratch
data directory, reads its one child, a status with the store open and a stop
that holds, installs over the copy while `halvo-owner.exe` is held and reads that
nothing was installed, then starts the app on the runner account's default data
directory, plants a file in `EBWebView` and the Run value, and runs `uninstall.exe
/S /DELETEALL` while the app and its child run. It reads that no process from the
folder, no file there but `uninstall.exe`, no Run value, no uninstall entry, no
default data directory and no planted file is left. It ends only the processes
that it started, by their ids, and takes away what it planted, also where it
fails.

## Notices

The Mac bundle holds `Contents/Resources/THIRD-PARTY-NOTICES.txt`, and the
installer puts `THIRD-PARTY-NOTICES.txt` beside `halvo.exe`. The file lists the
software of others inside the build and the text of each licence. It says nothing
of Halvo's own licence, which the owner has not chosen yet (item I-146).

The generator `scripts/notices/generate.js` reads `cargo metadata --locked
--format-version 1 --filter-platform <triple>` once and walks from the app and
from each helper of `helpers.d` over normal edges. It skips proc-macro crates,
which run inside the compiler alone, and the members of the workspace. For each
crate it lists the name, the version and the licence field. Its texts are the
files at the crate's root whose names start with LICENSE, LICENCE, COPYING,
COPYRIGHT, NOTICE or UNLICENSE, in any case, and the file that its
`license-file` names. A crate that ships no such file gets the standard text of
each licence id of its field from `scripts/notices/licences/<id>.txt`, with the
line `Copyright (c) the authors of <crate>: <authors>` as the holder, and the
generator prints at every run how many crates take that way. Each standard text
holds the line `Copyright (c) <holder>`, which the generator fills in, and the
generator fails for a standard text without it. A crate that ships
no text file and names no licence fails the generator. An equal text stands
once. The file holds no path of a machine, no date and no version of Halvo, so
two runs give the same bytes. The wrapper writes the file only when its bytes
differ, as it copies a helper, and no tracked configuration names it.

`scripts/notices/config.json` names what no normal edge gives:

- `app`: the manifest of the app, the first root of the walk.
- `compiled`: a build-dependency whose sources a build script compiles in, as
  `sqlcipher-src` is for `halvo-sqlcipher` (SQLCipher, SQLite and LibTomCrypt).
  An entry names the crate, the package that build-depends on it (`by`) and its
  texts, each a `file` in the crate's directory, or the part of such a file from
  the first line that holds `from` through the first line after it that holds
  `through`. The crate's own text files come with them. The generator fails when
  a crate that the walk reaches build-depends on a crate whose name ends in
  `-src` and no entry names that edge, and when an entry names an edge that the
  metadata lacks.
- `tracked`: a text of the repository for data that a member of the workspace
  embeds, with `name`, `licence`, the member (`in`) and its `texts` by `path`
  from the repository root: the Public Suffix List in `halvo-schema-core` and the
  rules of gitleaks in `halvo-redact`. It is listed when the walk reaches the
  member.

The standard texts are licence files of crates on crates.io, with the holder
line in place of the holder's name, or before the text where it names no holder
(Apache-2.0 and MPL-2.0): Apache-2.0 from `LICENSE-APACHE` of
annotate-snippets 0.12.16, MPL-2.0 from `LICENSE` of cssparser 0.37.0,
BSD-3-Clause from `LICENSE` of alloc-no-stdlib 3.0.0, Zlib from `LICENSE-ZLIB`
of bytemuck 1.25.2 and MIT from `LICENSE` of sqlcipher-src 420.0.0.

`scripts/r1/ci.d/72-notices.sh` runs the generator twice for each of
`aarch64-apple-darwin` and `x86_64-pc-windows-msvc`. It fails when the generator
fails, when two runs differ, and when the file names no Zetetic, the holder of
SQLCipher. Check 70 reads the file in the copy out of the disk image and beside
`halvo.exe` in the unpacked installer: it is there, it holds the bytes that the
generator writes for the host triple in the same run, and Zetetic stands on one
line at least.

## Not yet here

- The owner's signing and notarization (item I-116).
- A check of the minimum macOS version that each binary names in its own build; the check reads the bundle's floor alone.
- The list of modes that only a debug build has.
- A run of the installer and the uninstaller under a standard account.
- The removal of the reports of a crash of Halvo's window at every start and every uninstall.
