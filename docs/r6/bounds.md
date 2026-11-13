# Bounds of the router

The router is a library with no process, no store and no model. It runs inside the runner for each model call and inside the app for each admission, so its bound is a bound on what it adds to a process that links it.

`scripts/r6/ci.d/70-r6-memory.sh` builds `route-gate` in the dev profile and runs the built binary itself, at the path that cargo's artifact line of a fresh build gives and not through `cargo run`, as `route-gate probe --rounds 100000`, five times under `measure-peak --limit 16777216 --runs 5`. Each run asks for 100,000 picks, 100,000 admissions and 100,000 steps of the spend machine on the release data, and the count of rounds in a run's line is the probe's own word. The bound is 8 MiB times the ceiling of 5 times the median peak over 33,554,432 bytes, and the check requires it at or under 16,777,216 bytes. That line of 16 MiB is the planner's: two steps of that rounding, and no measurement.

These are the lines that the check printed on each machine:

```text
bound part=router machine=mac run=1 peak_bytes=1343848
bound part=router machine=mac run=2 peak_bytes=1360232
bound part=router machine=mac run=3 peak_bytes=1343848
bound part=router machine=mac run=4 peak_bytes=1343848
bound part=router machine=mac run=5 peak_bytes=1360232
bound part=router machine=mac bound_bytes=8388608
bound part=router machine=hosted-mac run=1 peak_bytes=1065728
bound part=router machine=hosted-mac run=2 peak_bytes=1049280
bound part=router machine=hosted-mac run=3 peak_bytes=1049280
bound part=router machine=hosted-mac run=4 peak_bytes=1049280
bound part=router machine=hosted-mac run=5 peak_bytes=1049280
bound part=router machine=hosted-mac bound_bytes=8388608
bound part=router machine=hosted-win run=1 peak_bytes=815104
bound part=router machine=hosted-win run=2 peak_bytes=823296
bound part=router machine=hosted-win run=3 peak_bytes=819200
bound part=router machine=hosted-win run=4 peak_bytes=806912
bound part=router machine=hosted-win run=5 peak_bytes=802816
bound part=router machine=hosted-win bound_bytes=8388608
```

The `mac` lines come from this repository's development Mac at commit b71ec4f7, run on 2026-11-12. The `hosted-mac` lines come from the job `macos` (runner `macos-15`) and the `hosted-win` lines from the job `windows` (runner `windows-2025`) of the hosted run 43320954252 at commit dc10ca0a. The probe's code outside its tests is the same at that commit and at the commit of this text. measure-peak read in `mode=poll` on both macOS machines and in `mode=job` on Windows. No machine here is a 16 GB laptop.

What a pass claims. Every run read `capped=no` and the median is at or under the limit, so no read was above 16,777,216 bytes. That never says that the probe held no more memory than that. On Windows `capped=no` says that no report of a refused allocation arrived.

What the tests hold and the runs do not. "Allocates nothing per round" is what the test `probe::tests::probe_allocates_nothing_per_round` holds: in a process of its own, the calls of the allocator and the bytes that they ask for, made by every thread of that process but the test harness's first thread, which runs no round, are the same for 1 round and for the 100,000 rounds that the check makes. It is no fact of the measured runs. Memory that a round touches outside the allocator is not read by that test, and the check fails on it only when it lifts the median above 13,421,772 bytes. On the release data every admission returns at the first rule of `admit`, because the estimate is 0, and every pick is `hand_back` or `none`, so the figure covers that path alone. The test `probe::tests::the_probe_names_no_process` reads the text of `probe.rs` outside its tests and of the arm of `probe` in `main.rs`, and fails when either names a process or the type `Command`; it holds against a slip, not against code that hides a process. The wider check, that no command of `route-gate` starts a process or opens a socket, is another unit's.

What the reading counts. On macOS it is the lifetime peak physical footprint of the started process, and it does not count `child-process`, `exec`, `mapped-read`, `mapped-written`, `purgeable`, `reusable`, `kernel-message` or `socket-buffer` memory. On Windows it is the Job's peak committed memory of one process, and it does not count `child-process`, `mapped-file`, `section` or `broker-process` memory.

What the bound covers. It bounds the whole probe process, and the Rust runtime is most of it. The router's own data are the four files that it compiles in, `policy.tsv`, `ruling.txt`, `cap.txt` and `prices.tsv` under `crates/halvo-route/data/`, 2,345 bytes together. The app's own line for the code that it links is the interface run's, and this figure is its upper limit.
