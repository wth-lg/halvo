# R7 spikes S5 (keychain half) and S6 (helper half)

Scratch branch, never merged. Every value in the jobs is synthetic.

The rules were written here before the first measured run (commit 99a00a1). The plan revised
its S5 unit on 2026-11-09 22:00, after run 1. The rules below follow the revised plan.
"Changed after the first measured run" lists each difference from the first version.

## Rules, as the plan states them

### S5: credential custody across builds, keychain half

One build stores an item through security-framework (macOS) or DPAPI (Windows). A rebuilt
build, ad hoc re-signed on macOS, reads it with user interaction disabled. The runners are the
hosted macos-15 and windows-2025.

- The rule is met when the read succeeds on both runners.
- P1's report predicts MISSED on macos-15 and MET on windows-2025. An ad hoc signature names
  the code's own hashes, so a rebuilt build is other code to the keychain. No push is spent on
  making the macos-15 leg pass.
- A read that succeeds counts as MET only when the log shows two different code hashes for the
  two builds, because the same code read twice shows nothing. With equal hashes the leg prints
  MISSED and gives that reason.
- A miss prints MISSED and does not fail the job.
- A miss means the device credential does not survive an update of an unsigned build. The
  custody crate then keeps an unsigned build's credential in a file only the user can read.
  The keychain path waits for the owner's signing (the same Developer ID identity on every Mac
  release; inferred, not run). A failed read is reported as a locked credential and is never
  answered by making a second credential. D7 returns to the planner only on a MISSED on
  windows-2025, which the report does not predict.

Two controls decide whether a leg's line counts. If either fails, the job prints DOES NOT
COUNT and fails:

- Before: the build that stored the item reads it back with user interaction disabled. If it
  cannot, the key store is unusable on the runner and the leg says nothing about a rebuild.
- After: build 1, installed again after build 2's read, reads the item once more. If it
  cannot, the item or the key store changed during the leg, and build 2's result cannot be
  tied to the rebuild.

### S6: helper start, helper half

A second binary of the scratch crate reads an item that it stored itself in an untimed first
run, makes one HTTPS request with reqwest and rustls (OS trust store) to a loopback stub whose
test certificate authority only the runner trusts, and exits.

- Each runner prints the wall time and peak RSS of 30 runs and `ok=30/30`.
- A run is ok only when it exits 0 after reading its item and the stub's own record holds its
  request.
- The line reads MET when all 30 runs are ok and the worst wall time is at or under 1 second.
- A control run made before the runner trusts the certificate authority must fail, or the line
  does not count.

### Changed after the first measured run

From the plan's revision of 2026-11-09 22:00:

- The predicted results are named, and no push is spent on the macos-15 leg.
- Equal code hashes make a leg MISSED. The first version here voided the line when the two
  binaries were identical.
- "Creating a second account" became "making a second credential".
- D7 returns to the planner only on a MISSED on windows-2025. The first version sent D7 back
  on any miss.

From run 1 and a review of the harness. Each of these can only turn a line into DOES NOT
COUNT:

- S5: the control after build 2's read decides whether the line counts. Run 1 printed that
  read and it passed on both runners. From run 2 on, a failure voids the line.
- S6: the control must fail on a certificate error. A helper that fails at the stub for
  another reason shows no trust decision.
- S6: when every timed run fails on a certificate error, the runner never trusted the CA. The
  line then reads DOES NOT COUNT. Before, it would have read MISSED and blamed the helper.

## Rule lines

S5 RULE macos-15: MISSED
S5 RULE windows-2025: MET
S5 RULE both runners: MISSED
S6 RULE macos-15: MET
S6 RULE windows-2025: MET

The S5 rule needs the read to succeed on both runners, so the MISSED leg makes it MISSED. The
MISSED leg is the predicted one. Both runs below printed the same five verdicts.

## Measured values

| Run | Run id | Commit | Branch | Started (UTC) | Conclusion |
|---|---|---|---|---|---|
| 1 | 42764934594 | 8d55687 | scratch/r7-s5-1 | 2026-11-10 05:53 | failure (see below) |
| 2 | 42770843157 | dc776c5 | scratch/r7-s5-2 | 2026-11-10 06:47 | success |

Run 1 ended as a failure. Its macos job passed. Its windows job passed the S5 step and the
S6 step and then failed the last step, the harness self-test, which runs after both
measurements. Cause: the S6 step had put its test CA in the machine Root store, the self-test
made a second CA with the same subject, and Windows paired the new leaf with the old CA by
name and reported a bad signature (0x80096004). The measured S6 run had the only CA of that
name, so its numbers stand. Commit ee183d0 gives every test CA its own subject, and the same
step passed in run 2.

| Runner | Image | Machine |
|---|---|---|
| macos-15 | macos-15-arm64 20261104.0514.1, macOS 15.8.2, System Integrity Protection disabled | Apple M1 (Virtual), 3 cores, 7 GiB |
| windows-2025 | windows-2025-vs2026 20261103.298, Windows 10.0.26100.33674 | 4 cores, 16 GiB; AMD EPYC 9V74 in run 1, AMD EPYC 7763 in run 2 |

### S5

| Run | Runner | Code hash of build 1 | Code hash of build 2 | Build 1 reads its own item | Rebuilt build 2 reads it | Build 1, installed again, reads it | Line |
|---|---|---|---|---|---|---|---|
| 1 | macos-15 | cdhash 6830223c… | cdhash 36c85386… | ok | failed, OSStatus -25293 | ok | MISSED |
| 2 | macos-15 | cdhash 8f47b05b… | cdhash 4d2420a9… | ok | failed, OSStatus -25293 | ok | MISSED |
| 1 | windows-2025 | sha256 cee4d545… | sha256 46819794… | ok | ok | ok | MET |
| 2 | windows-2025 | sha256 82e16d8d… | sha256 767f57cc… | ok | ok | ok | MET |

- Both builds come from the same source. They differ in a build stamp, as two versions of an
  app differ. Build 2 replaces build 1 at the same path, as an update does.
- The code hash is what the key store can tell two builds apart by: the code directory hash
  of the signature on macOS, the SHA-256 of the program file on Windows.
- macOS. Both builds are ad hoc signed with the same identifier, and the designated
  requirement of each is its own code directory hash. Every call runs under
  `SecKeychainSetUserInteractionAllowed(false)`. Build 2's read ends with -25293
  (errSecAuthFailed). The item is still in the keychain afterwards, and build 1 reads it again
  once it is back in place. So the item is locked to the build that stored it.
- Windows. The item is a DPAPI blob in user scope, written and read with
  `CRYPTPROTECT_UI_FORBIDDEN`. DPAPI ties it to the user, not to the program.

### S6

| Run | Runner | ok | Wall time per run, ms: min / median / p90 / max | Peak RSS, MiB: median / max | Control before trust |
|---|---|---|---|---|---|
| 1 | macos-15 | 30/30 | 42.6 / 62.4 / 78.3 / 113.7 | 8.8 / 9.0 | failed: UnknownIssuer |
| 2 | macos-15 | 30/30 | 37.4 / 40.1 / 44.5 / 59.8 | 8.8 / 8.9 | failed: UnknownIssuer |
| 1 | windows-2025 | 30/30 | 76.6 / 81.7 / 89.7 / 96.7 | 13.5 / 13.6 | failed: UnknownIssuer |
| 2 | windows-2025 | 30/30 | 90.3 / 93.0 / 95.0 / 104.5 | 13.4 / 13.7 | failed: UnknownIssuer |

Where the time goes (median per run, from the helper's own clock):

| Run | Runner | Key-store read | Client build | Request (TLS handshake, OS verification, HTTP) | Outside main (spawn, load, exit) |
|---|---|---|---|---|---|
| 1 | macos-15 | 8.9 ms | 0.3 ms | 45.2 ms | 7.4 ms |
| 2 | macos-15 | 5.3 ms | 0.1 ms | 30.1 ms | 3.8 ms |
| 1 | windows-2025 | 1.2 ms | 0.3 ms | 72.4 ms | 7.4 ms |
| 2 | windows-2025 | 1.4 ms | 0.4 ms | 82.3 ms | 8.7 ms |

### How each number is measured

- Wall time. The driver takes a monotonic clock reading just before it spawns the helper and
  one just after it has reaped it. The time covers process creation, loading, the key-store
  read, the HTTPS request and the exit. 30 runs, one after another, on one runner VM.
- Peak RSS. `ru_maxrss` from `wait4` on macOS, `PeakWorkingSetSize` from
  `GetProcessMemoryInfo` on Windows, read for each run's own process.
- ok. The helper exited 0, and the stub's record holds a request with that run's id whose
  bearer is the item the helper stored in its first run.
- Control. Before the trust step the same helper, against the same stub and certificate, ends
  with exit 21 and `invalid peer certificate: UnknownIssuer`. The stub records
  `received fatal alert: UnknownCA` and no request.
- Trust. Windows: `certutil -addstore -f Root` puts the test CA in the machine Root store.
  macOS: `sudo security add-trusted-cert -d` adds it in the admin trust domain, and
  `security verify-cert` then accepts the leaf. Run 1 first tried to open the right
  `com.apple.trust-settings.admin`. macOS 15 refused that write (`NO (-60005)`) and the add
  worked without it, so from run 2 on the script leaves the authorization database alone.
  The CA's private key is never written to disk.

### What limits these numbers

- The stub is on loopback. A real backend adds DNS and network round trips, which this number
  does not hold.
- The stub runs inside the driver's process, on the same VM as the helper.
- The helper's client is reqwest with default features off except rustls: it offers no HTTP/2
  and does no system proxy lookup.
- The runs are warm: the helper has already run twice (first run, control) when timing starts.
  The first start after an install or a reboot is not measured.
- A hosted VM is a shared machine, and each run gets another one. The medians of the two runs
  differ by 22 ms on macOS and 11 ms on Windows. The cause was not traced. Most of the time
  and most of the spread are in the request phase.
- 30 runs per runner in each run. Unit 7.4 repeats the measurement on the real helper.

## What each result decides

S5, macOS, MISSED, as predicted. A device credential in the keychain does not survive an
update of an unsigned (ad hoc signed) build. The custody crate keeps an unsigned build's
credential in a file only the user can read. The keychain path waits for the owner's signing.
A failed read is reported as a locked credential and is never answered by making a second
credential: the measurement shows the item is still there and still belongs to the earlier
build. D7 does not return to the planner on this miss.

S5, Windows, MET, as predicted. An unsigned build reads the DPAPI item of the build before
it. On Windows the credential can stay under DPAPI across updates.

S6, both runners, MET. A helper that reads its credential and makes its own HTTPS request
through reqwest, rustls and the OS trust store starts and finishes in about a tenth of a
second, against a limit of 1 second. The worst of 120 runs took 114 ms. Start time does not
force the HTTPS client out of the helper.

Not measured here: a Developer ID signed build (signing is reserved for the owner), Windows
Credential Manager, the data protection keychain, and a real network path.

## How to reproduce

A push of this branch starts the run on both hosted runners:

```sh
git push origin <commit>:refs/heads/scratch/r7-s5-<k>
gh run list -R wth-lg/halvo --branch scratch/r7-s5-<k>
gh run view <run id> -R wth-lg/halvo --log | grep -E "S5 |S6 |trust-ca:|FACT "
```

Each job runs `bash scratch/r7-s5/ci.sh <step> <runner>` for the steps `facts`, `s5`,
`s6-build`, `s6` and `selftest`. The script and the binaries refuse to run outside a hosted
runner job, because they write to the key store and the trust store.

Checks that touch neither store and run on a developer's machine, from `scratch/r7-s5`:

```sh
cargo test
sh tests/s5-verdict-test.sh
cargo clippy --all-targets -- -D warnings
cargo clippy --target x86_64-pc-windows-msvc --no-default-features --lib --bins -- -D warnings
sh check-result.sh
```
