# S2: kit footprint, Mac leg

rule: MET (the highest summed RSS was 2443 MiB against a limit of 4096 MiB, and no run attempted a connection outside loopback)

Measured 2026-11-10T16:15:03.915Z on Apple M3 Max, 64 GiB, macOS 27.2, Node v26.12.0. 3 runs, each from a stopped stack.

| run | ready (s) | RSS after ready (MiB) | RSS at the peak of gate.sh (MiB) | RSS idle after gate.sh (MiB) | gate.sh (s) | outbound |
|---|---|---|---|---|---|---|
| 1 | 1.24 | 1025 (11 processes) | 2435 (36 processes, 42 samples) | 1027 (11 processes) | 12.8 | PASS outbound: denied=0 over the stack and the other checks, which read 1 PASS and 11 FAIL inside the sandbox; probes left out=0; control=caught |
| 2 | 1.23 | 1017 (11 processes) | 2432 (36 processes, 42 samples) | 1020 (11 processes) | 12.8 | PASS outbound: denied=0 over the stack and the other checks, which read 1 PASS and 11 FAIL inside the sandbox; probes left out=0; control=caught |
| 3 | 1.22 | 1017 (11 processes) | 2443 (36 processes, 42 samples) | 1020 (11 processes) | 12.8 | PASS outbound: denied=0 over the stack and the other checks, which read 1 PASS and 11 FAIL inside the sandbox; probes left out=0; control=caught |

- Ready time: median 1.23 s, from 1.22 to 1.24 s.
  It is the wall time of `sh scripts/stack.sh up` when no member runs, until stripe-mock and both Worker entries answer over HTTP.
  The script starts the three members together and polls each every 0.1 s, so the slowest member sets it and it is read to about 0.1 s.
  It includes starting node and wrangler; files were already in the page cache, so a first start after a reboot is slower and was not measured.
- RSS: the sum of the `rss` column of `ps` over the kit's process tree, which counts a page that processes share once per process.
  It is an upper bound on the memory the kit holds. The tree is every process in a process group that scripts/stack.sh recorded
  (the stack, and the stacks that the outbound check runs under the sandbox) plus gate.sh and its descendants.
- After ready: the highest of 3 samples, 1 s apart, starting 2 s after the stack answered.
- Peak of gate.sh: the highest of the samples taken while gate.sh ran, one every 305 ms at most
  (a 250 ms sleep plus one ps call); a spike shorter than that can be missed.
  The gate.sh column is the time from its start to the first sample after it ended.
- What gate.sh ran: in the last run it printed 13 result lines, of which 11 were stubs that do no work.
  The sums hold only what those checks start. A check that later starts a test runner, a browser or the model checker adds to the peak.
- Idle after gate.sh: the highest of 3 samples, 1 s apart, starting 5 s after gate.sh ended, with the stack still up.
- Outbound: the line that gate.sh printed for the outbound check in the same run. Its count can miss a single attempt by a
  binary that Apple did not sign and that is not Node (see the README, Gate).
- The limit is 4 GiB, one quarter of a 16 GiB laptop. Every number here was measured on this 64 GiB machine.
  No run on a 16 GiB machine exists: the sums are an upper bound on what the kit asks for, and say nothing about how a
  16 GiB machine behaves when memory is short.

Reproduce: `node spikes/s2/measure.mjs 3 > spikes/s2/RESULT.md` after `sh scripts/prerequisites.sh`. It stops the stack before and after each run.
