# Halvo backend

Halvo's subscription and billing backend: accounts, a subscription, a spend cap and usage
metering. It never holds anything captured on a user's machine.

It lives under `backend/` of the app repository, with its model under `formal/r7-ledger/`: on
2026-11-12 its tree came over as files from da7d398, the main of the repository where it was
built. It holds the Worker (its routes, the account object, the cap ledger and the gateway under
`src/`), a local stack, and a gate of 13 checks, of which the four of phases P5 and P6 are still
stubs. Nothing here creates an account, deploys or spends: everything runs on the local machine
against local mocks.

## Prerequisites

You need git, curl, cargo, lsof, Node 26 or newer and pnpm 12.14.0 (the version in
`package.json`). cargo is needed from this phase on, because `spikes/s3/crate/Cargo.lock` is
committed and the step below fetches its crates. The stack asks lsof which process listens on a
port. The gate runs on macOS, because its outbound check uses `sandbox-exec`. Later phases also
need Java for the model check: the plan names Java 17, and no run of this phase starts Java.

```sh
sh scripts/prerequisites.sh
```

This is the only step that uses the network. A second run downloads nothing and changes nothing.

The script first looks for git, curl, cargo, lsof and node. When one of them is missing, when
node is older than `package.json` asks, or when `package.json` states no Node version under
`engines.node`, the script stops there with exit 1 and one line that names what is missing.

| It does | Pinned by |
|---|---|
| Installs the npm packages, with install scripts off | `pnpm-lock.yaml` (a sha512 per package) |
| Installs the packages of each spike that has its own `spikes/NAME/pnpm-lock.yaml` | that lockfile |
| Fetches stripe-mock v0.206.0 and tla2tools.jar 1.7.4 into `.tools/` | `scripts/tools.sha256` |
| Fetches the license of each lockfile package that this platform does not install, into `.tools/licenses.json` | the lockfile's sha512, compared with the registry's |
| Installs the Chromium build of Playwright 1.65.0 into Playwright's cache | the Playwright version; no checksum |
| Runs `cargo fetch --locked` for every committed `Cargo.lock` | each `Cargo.lock` |

It sets no git hook: it does so only at the top of a work tree, and the hooks of the app
repository, commit-msg and pre-push, are set by the app's `sh scripts/prerequisites.sh`, which
also installs pnpm and this directory's packages and tools. Outside the repository this script
writes only to the caches of pnpm, Playwright and cargo in your home directory. Three packages in `pnpm-workspace.yaml` are let through pnpm's release-age wait,
because the plan pins versions that were published the day the lockfile was made.

## Run

```sh
sh scripts/stack.sh up            # start what is not running; wait until each member answers
sh scripts/stack.sh status        # one line per member; exit 0 when all answer
sh scripts/stack.sh url worker    # print the URL of one member
sh scripts/stack.sh down          # stop every member and its child processes
```

Every command is safe to run twice.

| Member | Base port | What it is |
|---|---|---|
| `stripe-mock` | 12111 (HTTPS on 12112) | Stripe's mock server: checks request shapes, answers with fixtures, keeps no state |
| `worker` | 8787 | the deployable entry, `wrangler.jsonc` |
| `worker-test` | 8788 | the test entry, `wrangler.test.jsonc` |

Every member listens on 127.0.0.1. Each checkout adds its own offset to the base ports, a multiple
of 30 taken from the checkout's path, so two worktrees on one machine can run the stack and the
gate at the same time. Never write a port into a test or a config: ask
`sh scripts/stack.sh url NAME`. Two checkouts get the same offset once in 300 pairs. `up` calls a
member up only when the listener that answers on 127.0.0.1 at the member's port belongs to the
member's own process group. When another process already answers at that port, `up` does not
start the member: it names the port and exits 1. When the member runs and another process
answers at its port, `up` names the port and the pid of that listener and exits 1, and `status`
reads `running, but another process answers` for that member and exits 1. In both cases, stop
that process or write another multiple of 30 into `.stack/port-offset`.

A member is one file under `stack.d/`. It sets `port`, with `STACK_PORT_OFFSET` added, and defines
`run`, which execs a server in the foreground on `127.0.0.1:$port`. Pid files and logs are in
`.stack/`.

`scripts/env.sh` sets four switches. `WRANGLER_SEND_METRICS=false`,
`WRANGLER_SEND_ERROR_REPORTS=false` and `WRANGLER_HIDE_BANNER=true` are wrangler's switches for
its usage metrics, its error reports and its banner. Under the profile of the outbound check,
`wrangler deploy --dry-run` builds the Worker when all three are set and is ended at its first
attempt outside loopback when the first or the third is not; with only the second unset it made
no attempt. `CLOUDFLARE_CF_FETCH_ENABLED=false` turns off a fetch of the stack: the control run
of the outbound check starts the stack without it and has to be caught at an attempt outside
loopback. Start tools through the scripts, or through `sh scripts/with-env.sh COMMAND`, so they
get them. No switch shows that nothing left the machine: the outbound check does, which counts
the attempts of the stack and of the other checks and passes only at `denied=0`. pnpm itself asks
the registry for updates, so the stack and the gate call the tools in `node_modules/.bin` and
never pnpm.

Static analysis and the placeholder test:

```sh
node_modules/.bin/tsc --noEmit
node_modules/.bin/biome lint .
(cd .. && sh scripts/ci/run-ci-d.sh macos 16-shell-lint)
sh scripts/with-env.sh node_modules/.bin/vitest run
```

## Gate

```sh
sh scripts/stack.sh up
sh scripts/gate.sh                # one PASS or FAIL line per check; exit 0 only when all pass
sh scripts/gate.sh --self-test    # each check passes on a good fixture and fails on a bad one
```

A check is `gate/checks/NN-name.sh`. It prints exactly one result line, `PASS name: ...` with exit
0 or `FAIL name: ...` with exit 1; the rest of its output goes to a log under `.tmp/`. A check that
breaks this contract is reported as FAIL, and so is a check of the driver's list whose file is
absent. With `--self-test` a check prints
`self-test name: good FIXTURE: PASS as expected` and `self-test name: bad FIXTURE: FAIL as expected`;
a check that is no longer a stub and lacks either one, or prints these lines and nothing of its
runs, fails the self-test. A check finds a stack member with `sh scripts/stack.sh url NAME`.

`scripts/gate.sh` holds the list of the 13 checks and prints one line for each, in the order of
the list. `--except ID` leaves a check out and prints `SKIP name: left out by --except ID` in its
place; an ID that names no check is bad usage (exit 64). The gate exits 2 and prints the reason
on stderr when it cannot run: `scripts/check-machine-paths.sh` fails, `gate/checks` holds a file
that the list does not name, every check was left out, or the log directory under `.tmp/` cannot
be made. `GATE_CHECKS_DIR` and `GATE_NESTED` are for self-tests: the first points the driver at a
directory of fixture checks, and the second marks a run that the driver's self-test started.
After it has read its arguments, the driver names each on stderr when it is set. A run without
`--self-test` refuses `GATE_CHECKS_DIR` (exit 64) unless `GATE_NESTED` is set or
`scripts/outbound-hosts.sh` started the run, which the driver reads from `OUTBOUND_RUN`.

No tracked file may hold a home-directory path. `sh scripts/check-machine-paths.sh` reads the
index and prints `FILE:LINE:PATH` for each path under /Users/ or /home/ or under a Windows users
directory, when a name follows the directory, in files and in the targets of symbolic links. It
reads bytes, so it does not find such a path in UTF-16 text or in percent-encoding. A name is
taken for a placeholder only when its first character is `<`, `$`, `%`, `*` or `{`: `<name>`,
`$USER`, `%USERNAME%`, `*` and `{name}` pass. Every other name is refused, also one that starts
with `_`, `-` or a dot. The directory alone passes when a blank, the end of the line, a double
or single quote, a backtick, a comma, a round, square or curly bracket, or a period before a
blank or the end of the line follows it. What stands behind one of these characters is not read
as a name. In the name of a tracked file, which it prints as line
0, it looks only for a home path that a tool wrote with dashes for separators, such as
`-Users-<name>-Documents`, and not for the forms with slashes, because a directory named `home`
or `Users` is a common name in a source tree. The gate runs it before its checks, so the gate
needs a git checkout.
With `--staged` it reads only the files that the index adds or changes against the last
commit, for a pre-commit or a pre-merge-commit hook; no hook of the app repository runs it, and
in CI `scripts/r7/ci.d/23-r7-machine-paths.sh` runs it over the tracked files under `backend/`
and `formal/r7-ledger/`. Write such a path relative to the repository, take the location from an environment
variable, or write the name as one of those placeholders.

The gate has 13 checks. Four are stubs that print FAIL until their unit replaces the file:
lifecycle, prompts, metering and storage. `docs/baseline/r7-gate-before.txt` is the gate's output
at the first phase, before any check was built.

**licenses** (`gate/lib/licenses.mjs`). It reads the license of every package in `pnpm-lock.yaml`.
A package bundled into the deployable Worker must have a license in `gate/license-allowlist.json`.
Every other package must have one too, or an entry in `gate/license-exceptions.json` that names
the package by its exact name, its versions, its license, the chain that brings it and the reason. The check also fails when it could not read every
package's license, when an exception covers no package, and when a package is an error-tracking,
analytics or crash-upload package. That last rule is a list of name patterns in
`gate/lib/licenses.mjs`: a package of that kind whose name the list does not hold passes it, and
only the outbound check then sees what the package sends. The PASS line ends with `cached=N`: the
number of packages that this platform does not install, whose license the check reads from
`.tools/licenses.json`. The check fails for every entry of that file that does not carry the
lockfile's integrity for its package. It cannot see an entry whose license was changed by hand
while its integrity was kept. The gate trusts the license field of each package's manifest, and of
the registry's answer for a package that this platform does not install; it reads no license text.
At 69342faa the license of 23 of the 29 packages that the exceptions name was read from the
registry's statement, and one installed package, @img/sharp-libvips-darwin-arm64, carries no
license file. The self-test reads the made checkouts under
`gate/fixtures/licenses/` with the readers of the check and builds their Workers with wrangler,
so it needs the switches of `scripts/env.sh`: start it with
`sh gate/checks/85-licenses.sh --self-test`.

**outbound** (`scripts/outbound-hosts.sh`). It starts a second stack and runs every other check
under a sandbox profile that denies every connection outside loopback, name lookups included, and
ends the process that tried. The check fails on the number of attempts. The number after
`denied=` is a sum over the sources that the script prints, so one attempt by a Node process
reads 2; only 0 means that no source saw an attempt. The check also reads the line that every
other check prints inside the sandbox: its PASS line says how many read PASS and FAIL there. A
run that counts no attempt still ends as `cannot count` when the gate inside the sandbox ends
with a code other than 0 or 1, when a check breaks its contract there, or when a check reads FAIL
inside the sandbox and does not read FAIL outside it. It then runs the stack
with one switch left off, and fails unless that control run is caught and the system log holds
the record of it. A lookup of any name is an attempt, `localhost` included, because the profile
cannot see which name a process asks for: inside this check a tool must use `127.0.0.1`. The
record that each Node process keeps does name what it looked up, so when that name is `localhost`
the FAIL line says to use `127.0.0.1`.

What it guarantees: one attempt by one process fails the check. The process dies at the denied
call, so nothing leaves the Mac, and macOS writes a record of the denial to the system log, where
the check counts it. The profile ends the process because macOS records a denial that ends a
process, and leaves plain denials of a binary that Apple did not sign out of the log: in the runs
made here, the first 8 of each minute. For those 8 the record comes from sandboxd, not from the
kernel, and it came up to 2.5 s after the kill on this Mac under load. The check therefore reads
the log for 5 s after a run that shows no record, so a passing check takes 5 s longer. The
check also counts the record that each Node process keeps of its own attempts
(`scripts/outbound-audit.cjs`, loaded through `NODE_OPTIONS`), and a command that itself dies of
the kill. A process that the command leaves behind is stopped before the count is taken: the
command runs in a process group of its own, and when it ends the script lists what is left of
that group and of its children and stops it. When something does not stop, a run that counts no
attempt ends as `cannot count`.

A process under the profile can make a hard link and rename a file that has a second name. At
such a call macOS asks every rule of a profile about both paths and carries out a rule's kill
even when it then allows the call, so the profile allows the network operations for every path.
A socket with an address has no path.

`sh gate/fixtures/outbound/controls.sh` is the proof, and takes about ten minutes. Node, workerd,
stripe-mock, the pinned Chromium and a C program built for the run each make one attempt, 20
times, five of them after 90 seconds of quiet. The browser is the one that the script starts (see
below), and its attempt is a page that asks for `http://192.0.2.1/`. The script exits 0 only when
the check saw every attempt and the system log held the record of each. Before this profile the
check saw 13, 14, 12 and 14 of the 20 attempts by the four that are not Node. With it the check
saw all 100, and the log held all 100; the browser's attempt was then its own probe, inside the
sandbox. With the browser under its second profile and a page's request as its attempt, the check
saw all 100 in two of three runs, and the log held all 100. In the other run, under a load average
above 100, it saw 99: in one round the log stream dropped messages and the check said
`cannot count`, which fails a gate run but is not a count. Run it again after a macOS update. The self-test
runs each control once and fails without the log's record; the C program needs `cc`.

`sh gate/checks/90-outbound.sh --self-test NAME...` runs only the named fixtures.
`scripts/outbound-hosts.sh` needs an `sh` with job control (`set -m`), which the `/bin/sh` of
macOS has.

What it costs: every denied attempt leaves a crash report, named after the process, in
`~/Library/Logs/DiagnosticReports`. A gate run leaves two, from the control run, and a self-test
about eight: 8 in one run, in which 11 fixtures end a process. macOS did not write one for every
kill here: about 70 kills of each control left 27 to 30 reports under its name. It sends crash
reports to Apple only from a Mac that shares its analytics. A page that keeps trying costs more,
because the browser starts its network process again each time the profile has ended it. A page
that opens a WebRTC connection makes that process ask for the default route with a UDP call to
port 53: in one run made here the profile ended it 221 times in 5 s, and the check failed with
as many records.

A browser for a check. A check gets its browser from `test-kit/browser`:

```js
import { openBrowser } from "../../test-kit/browser/index.mjs";
const browser = await openBrowser();
```

A check cannot start a browser of its own inside this check. Before Chromium loads a page, even
one on loopback, it asks whether IPv6 is reachable with a UDP connect to
`[2001:4860:4860::8888]:443`. The call sends nothing and no switch turns it off, but it is a call
outside loopback: the profile ends Chromium's network process and no page loads. The FAIL line
then names the browser and says to use the browser mode.

In that mode the script starts the pinned Chromium itself, before the command and outside the
command's sandbox: headless, under a second profile (`scripts/outbound-browser.sb`), with a
debugging endpoint on a loopback port. `gate` does this for the checks, whether or not one of
them asks for a browser, so the gate needs the pinned Chromium that the prerequisites step
installs. `sh scripts/outbound-hosts.sh browser COMMAND` does it for one command. The script puts the endpoint into
`HALVO_BROWSER_ENDPOINT`. `openBrowser` connects to that browser when the variable is set. Inside
an outbound run without the variable it starts nothing and says which mode to use. Anywhere else
it starts the pinned Chromium, so a check also runs by itself.

The second profile is the first with one change: a UDP call to port 443 outside loopback is
denied without ending the process. The profile language can match the protocol and the port of a
call, not its address, so no rule can name the probe alone. Everything else stays denied and
still ends the browser's process: a page that asks for a host outside loopback over TCP, a
lookup of a name, a UDP call to any other port. The one rule writes nothing to the system log:
macOS logs a later denial of the same process and port as a duplicate of the first record, and a
record of the probe hid the kill of a TCP call to port 443. So the script counts the browser's
UDP calls to port 443 from Chromium's own net log, after it has stopped the browser. Every
address outside loopback in that log is an attempt, except the probe: a UDP connect to the
probe's address, with no datagram, on a socket that Chromium's own check of IPv6 made. The
browser is started with `--no-system-proxy-config-service`, so a proxy that macOS is set to use
carries no request past the sandbox. The result line says how many probes the count left out
(`probes left out=N`). The check fails with `cannot count` when
the net log is missing, when Chromium did not finish it, when the browser's network process,
which writes it, did not live through the whole run, and when the net log says that the browser
took its proxy settings from macOS after all. A run with a browser leaves that browser's
profile directory and net log in the run's directory under `.tmp/`: 0.2 to 2.2 MB in the runs
made here.

What it cannot see:

- A request that a process hands to something outside the sandbox. The profile denies two such
  ways, a name lookup through the system's DNS service and `open`, and the run drops the caller's
  proxy variables. It does not deny the others: a job given to launchd, an Apple event, a proxy
  that a tool finds some other way, a daemon that listens on loopback or on a unix socket.
- An attempt to reach `fe80::1` through a real interface. The profile language names a host only
  as `*` or `localhost`, and macOS takes `localhost` to include `fe80::1` on any interface. Only
  the Node record counts such an attempt.
- A record that the system log loses, or that sandboxd writes more than 5 s after the run. For a
  process that is neither Node nor the command itself, the log is the only record. The check
  fails when the log reports dropped messages, when its own canary does not show, when the
  control run's record is missing, and when a member of the stack no longer answers at the end of
  the run. One record lost between those, of a process that the stack can lose and still answer,
  would go unseen.
- A lookup through the DNS service by a process that skips the resolver's socket. It is denied,
  but the kill does not reach it, so it is counted only when macOS logs it. The self-test holds
  that record with a C program that asks launchd for the service.
- A UDP call to port 443 by the browser that Chromium's net log does not hold. The call is
  denied, but nothing ends the process, so the net log is its only record. The self-test
  proves that record for a QUIC session that a page opens to another address. A part of Chromium
  that opened a socket without writing to the net log would go unseen.
- A second outbound run in the same checkout at the same time. Each run first stops a stack that
  an earlier run left up, and it cannot tell that from the stack of a run that is still going.
  Run one gate per checkout.
- A process that left the command's process group and whose parent has ended, such as a daemon
  that calls `setsid`. The script does not find it when the command ends, so an attempt that it
  makes later is not counted.
- A listener on the wildcard address. A listener on `0.0.0.0` or `::` runs under the profile; one
  on an address outside loopback, such as 192.0.2.1, ends its process. A connection to such a
  listener from another host was not tried.

## Checks in CI and the gate's records

The one workflow of the app repository runs the backend's checks through its driver,
`scripts/ci/run-ci-d.sh`, as five scripts under `scripts/r7/ci.d/`: 10-r7-layout (every tracked
file under `backend/` and `formal/r7-ledger/` matches a line of `scripts/r7/layout.txt`, which
names its owner), 20-r7-backend-static (the typecheck and the lint), 21-r7-backend-test (the test
script, in the macos job alone: on windows-2025 the stack needs lsof and `ps -o`, which Git Bash
lacks, and the test script does not end, so the windows job runs no test of the backend),
22-r7-backend-records and 23-r7-machine-paths (`scripts/check-machine-paths.sh` over
`backend/` and `formal/r7-ledger/`).

The gate runs in no CI job: it needs the stack and the sandbox of macOS. Its lines are kept as
records, `docs/r7/gate/NN-name.txt`, one for each check: the line that the gate printed, the UTC
time and the 1-minute load at which it came, and the trees of `backend/` and of
`formal/r7-ledger/` that it ran on. 22-r7-backend-records fails in both jobs when a record names
other trees than HEAD's, so any commit that changes `backend/` or `formal/r7-ledger/` makes all 13
stale. Stamp them again at the top of the repository, on macOS, with nothing uncommitted under
either directory, and commit them with that change:

```sh
sh scripts/r7/stamp-gate-records.sh
```

It brings the stack up, runs `sh backend/scripts/gate.sh` once, brings the stack down and writes
the 13 records, and nothing else in the repository; the gate's logs stay under `.tmp/`. It takes
about half an hour: 1,684 to 1,911 s in the three runs of the fold, under a 1-minute load of 16 to
46. It writes no record when the trees are not clean, the stack does not come up, the trees change
during the run, or a check prints no line or more than one.

## Commits

The rules of the app repository, its `AGENTS.md`, bind every commit here. A commit is made by
the configured git user, and its message carries no attribution line: the app's commit-msg and
pre-push hooks and its CI read every message. A pull request adds one line for each type of
change that it makes at the end of the list under Unreleased in the app's `CHANGELOG.md`.
