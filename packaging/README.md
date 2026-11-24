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
directory, newer than the start of its build. Before it attaches the image, it
copies it into `CI_D_ARTIFACTS`, so that the run's artifact `ci-artifacts-macos`
holds the image also when a later step fails. It attaches the image read-only and
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
names. `src-tauri/windows/English.nsh` holds every string of the English file of
that template in tauri-cli 2.12.4, which pins tauri-bundler 2.10.4, since the
bundler takes it in place of its own, so a later CLI that adds a string needs the
string here too. Its sentences that a person can meet in this build are in
Halvo's words: the page "Halvo is already installed" with its choices, the box
when the removal that it starts fails, the question while Halvo is open with the
two lines after it, the box on the installer's last page and the label of the
uninstaller's box. The words `older` and `unknown`, which that page's sentence
reads at an update, and the strings that this build never shows stay as the
bundler wrote them; the installer's other pages speak NSIS's own English. The
hooks hold every sentence that a person reads from them.

Before an uninstall, and before an install over an installed copy, the hooks stop
Halvo in this order: `halvo-capture.exe ctl stop` from the install folder (exit
69, where no capture service runs, is no fault), then the end of `halvo.exe` by
the template's own step, which ends the processes of that file of the install
folder alone, then a wait of at most 20 s until `halvo.exe` and each helper of
`helpers.d` can be opened for writing. A file that is still held stops the
uninstall with the sentence "Halvo is still running, so nothing was removed.
...", and the install with its own, before any file or value goes. `ctl stop`
names no data folder, so it reaches the capture service of the account that runs
it: an uninstall under another account (below) makes no such stop. Check 70
fails when `hooks.nsh` waits for no helper of `helpers.d`, or when its first
hook of the uninstall calls `ctl stop` outside the test of the account's own
install.

Before the stop, the uninstall reads whose install it removes: the uninstall
entry of the account that runs it, under `HKCU`, names its install folder in the
value `InstallLocation`, and the hooks hold that folder against the folder of the
uninstaller, both without quotes and a trailing backslash and in their short
form. The install and its entry are one account's, so another folder, or no
entry, reads as another account's install. There, with the box ticked, the hooks
show one sentence with OK and Cancel before anything goes: this account has no
record of installing this copy, so the uninstaller deletes no Halvo data. Cancel
stops the uninstall; OK clears the box, so that neither the hooks' delete nor the
template's own step removes any Halvo data, the running account's own neither,
and the program goes. A silent uninstall with `/DELETEALL` there shows nothing and
stops before anything goes, with exit 2.

Under another account the template's own steps still take away two things of the
running person that are named by the product and not by the folder: their Run
value `Halvo`, the start of their own Halvo at sign-in, and their own uninstall
entry, its place in the list of installed apps. So the first hook of the
uninstall, behind every point at which the hooks can still stop it and before
those steps, holds both, and the last hook writes them back first, so that OK
removes the program only, as the box's sentence says:

- The Run value comes back with its text where the program that it names, cut
  from between its first two quotes, or up to its first space where the text
  starts with no quote, is not directly in the folder that goes; both folders are
  held in the one spelling of the compare above. A value in the form that Halvo
  writes, the program in quotes and with backslashes, that names the program in
  the folder that goes stays away. Where there is no such value, or one
  that the hooks cannot read as text, nothing is held, and the template's step
  takes a value that stands, as before. Under another account the hooks' own line
  no longer takes the value away, so an uninstall that the template's step still
  stops, before it removes anything, loses nothing.
- The uninstall entry goes by `reg.exe export` into one file of the uninstaller's
  own folder in the temp folder, and comes back by `reg.exe import` from it; the
  hooks remove the file behind the import, whatever it answered. Where the
  account has no such entry, nothing is exported and nothing comes back. Where
  the export fails, also where that file cannot be written, nothing is held, and
  the template's step takes the entry, as before. Where the import fails, the
  entry stays away and nothing says so.

A file of the install folder that the session of the person who installed it
still holds stops such an uninstall with the sentence "Halvo is still running, so
nothing was removed. ..."; where the template's step finds that person's
`halvo.exe` and cannot end it, the template's own text stops it first. Check 70
fails when the hooks hold the two things anywhere but behind the delete in the
first hook of the uninstall, write them back anywhere but first in the last hook,
or hold the Run value where its program is in the folder that goes.

What this way does not cover yet, each a named limit:

- A kill of the uninstaller under another account between the template's steps
  and its last hook loses what the hooks held: the running person's start at
  sign-in and their entry in the list of installed apps go, as before the
  write-back, and the file of the entry stays in the temp folder.
- The Run value stays away only in the form that Halvo writes. A text that
  another hand wrote under that name comes back although its program is gone,
  where it has no quotes and a space in its path, a forward slash before the
  program's name, or the program in a folder below the folder that goes. Halvo
  writes none of these, and at each start that a person agreed to it writes its
  own text over another one; what Windows does with such a value at sign-in is
  read in no run.
- Under another account the template's own step still clears the jump list and
  the recent destinations of the app's id for the running account, as before; no
  line of Halvo sets a jump list.
- The kinds of value: Halvo writes its Run value as a string, and the template
  writes strings and numbers into the uninstall entry, which come back as they
  were. A value of another kind that another program put into the entry comes
  back as `reg.exe` exports it, which no run reads; under the name of the Run
  value, an expandable string comes back as a plain string, and a value that is
  not text is not held and goes, as before.
- Two people who installed Halvo into one folder read as one account: whoever
  uninstalls reads as the one who installed it, so their ticked box deletes their
  own Halvo data, while the other person's data, start entry and uninstall entry
  stay, and no sentence says so.
- A person whose uninstall entry is gone reads as another account, and so does a
  person who installed again into another folder and uninstalls the older copy:
  their ticked box deletes none of their Halvo data and they read the sentence;
  without the box the crash reports of their window stay too, with no sentence.
  The hooks do not try to tell such a person from another account. The one who
  installed again keeps the entry of their newer Halvo in the list of installed
  apps, and its start at sign-in where the Run value names the newer copy, as
  Halvo writes it at each start of that copy that the person agreed to; where
  they never started the newer copy so, the value names the older copy and goes
  with it.
- Neither of those two people gets the stop of their capture service before the
  uninstall of their own copy: where its app and its capture service run, the
  template's step ends the app, with windows after it asks, and the capture
  service ends by itself behind its app, which check 34 reads within 12 s of the
  app's end, inside the wait of 20 s. Where a file of the folder is still held at
  the end of that wait, they read "Halvo is still running, so nothing was
  removed. ...", nothing goes, and a second try goes through once the service has
  ended.

Where the box is ticked at the account's own install, the uninstall first asks
`halvo-capture.exe --default-data-dir` for the program's own default data
directory, and runs
`halvo-capture.exe --delete-all --data-dir` with the printed path as one
argument. No line of the hooks names the data directory or builds a path from
the account's name. At exit 0 the uninstall goes on, and the template's own step
removes the folder of the app's id, `%LOCALAPPDATA%\com.halvo.desktop`, which
holds the web view's folder `EBWebView`. The hooks name that folder by the
template's own expression, `$LOCALAPPDATA\${BUNDLEID}`, for two things alone:
where the folder still stands, they run the template's step again for at most
20 s, and then they name the folder in the sentence that asks the person to
remove it. At exit 66, where the delete could not use the data directory and
removed nothing, the uninstall stops before any file or value goes with a
sentence that names that directory: the person can remove it and run the
uninstaller again, or run it without the box to remove the program only. At exit
75, or at any other end, the uninstall stops before any file or value goes. A
silent uninstall ticks the box by the switch `/DELETEALL`, and never without it,
as check 70 calls it:

```sh
uninstall.exe /S /DELETEALL _?=<the install folder>
```

Only this form, with `_?=` and the install folder last, gives the caller a
readable exit: 0 where the uninstall went to its end, and 2 where it stopped.
Under another account a silent run without `/DELETEALL` goes to its end with 0,
also where the write-back of the uninstall entry failed. A
silent run that left a report ends with exit 3, read with `_?=`: a silent run
ends with 3 where a report or the folder of Halvo's window stays, with the box or
without. A run with windows never ends with 3, since the template's own
installer, at its choice "Remove the installed Halvo first, then install this
version", starts the uninstaller with windows and `_?=` and reads every exit but
0 and 1 as a failed uninstall.
Without `_?=` the uninstaller starts a copy of itself and exits at once, so the
caller reads 0 whatever the copy did.

The box deletes what Halvo's own Delete everything deletes, and its label says
what stays: "Also delete Halvo's data. Copies next to your files stay."

At every uninstall of the account's own install that is not for an update, with
the box ticked or not, the hooks call `halvo.exe --remove-crash-reports` while
`halvo.exe` stands, behind the stop and the delete. The form removes the crash
reports of Halvo's window, which can hold what the window showed, and prints one
line, `crash_reports folder=<path> removed=<n> left=<n>`. Every end of the form
but 0 reads as a report that stays. Without the box the uninstall then ends with
a sentence that names the folder from that line, and a silent one ends with exit
3. Where the form does not start or its line names no folder, the sentence names
the folder of Halvo's window, `%LOCALAPPDATA%\com.halvo.desktop\EBWebView`,
and never the folder of the app's id, which holds the data that a person who did
not tick the box kept. The form refuses a link at the folder of the reports and
at each folder above it up to the folder of the app's id: it removes nothing
behind the link and says that a report stays, so a person who moved one of those
folders by a link reads at every start, and at every uninstall without the box,
that a report stays, also where none does. It reads those folders once and then
removes each file by its path, so a program that runs as the same person and
puts a link in place during that walk can lead it to other files, with the
rights of the window that runs it.

A silent uninstall without `/DELETEALL`, and an uninstall with the box not
ticked, delete nothing else that Halvo stored: Halvo's data folder and the web
view's folder stay on the computer. An uninstall for an update (`/UPDATE`)
deletes nothing, and calls no form. Every other uninstall
takes the value `Halvo` away under `Software\Microsoft\Windows\CurrentVersion\Run`
of the person's own registry, whatever its text; under another account it comes
back, unless it names the program in the folder that goes.

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
nothing was installed. With a file at the name of the default data directory it
reads exit 66 of the delete, and exit 2 of `uninstall.exe /S /DELETEALL` with
nothing taken away. It plants two reports in the folder that `halvo.exe
--remove-crash-reports` names, holds one open, and reads that `uninstall.exe /S`
exits 3, removes the program and the other report; then it installs again, and
installs a second copy into a second folder, so that the uninstall entry names
that folder. It starts the second copy's app on the runner account's default
data directory, plants a file in `EBWebView` and the Run value, turns the folder
that its own uninstall entry names, and reads exit 2 of the first copy's
`uninstall.exe /S /DELETEALL` with nothing taken away and the app still running;
then it puts the entry back. It runs the first copy's `uninstall.exe /S`, which
reads as another account's install, and reads exit 0, no file in the first
folder but `uninstall.exe`, the Run value and every value of the uninstall entry
as they stood, and the second copy's app with the same capture child. Then it
runs the second copy's `uninstall.exe /S /DELETEALL` while the app and its child
run. It reads that no process from the second
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
