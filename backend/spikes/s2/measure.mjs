import { execFileSync, spawn, spawnSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const runs = Number(process.argv[2] ?? 3);
const LIMIT_MIB = 4096;
const SLEEP_MS = 250;
const began = Date.now();
const squeeze = (text) => text.trim().replace(/\s+/g, " ");
const PS_ENV_OF_STACK_SH = { LC_ALL: "C", TZ: "UTC0" };

function processTable() {
  const output = execFileSync("ps", ["-axo", "pid=,ppid=,pgid=,rss=,lstart=,comm="], {
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
    env: { ...process.env, ...PS_ENV_OF_STACK_SH },
  });
  const rows = [];
  for (const line of output.split("\n")) {
    const match = /^\s*(\d+)\s+(\d+)\s+(\d+)\s+(\d+)\s+(\S+\s+\S+\s+\d+\s+[\d:]+\s+\d+)\s/.exec(line);
    if (match) {
      rows.push({ pid: +match[1], parent: +match[2], group: +match[3], rssKib: +match[4], started: squeeze(match[5]) });
    }
  }
  if (rows.length === 0) throw new Error("ps printed no row that this script can read");
  return rows;
}

function recordedLeaders() {
  const directories = [join(root, ".stack")];
  const temporary = join(root, ".tmp");
  if (existsSync(temporary)) {
    for (const name of readdirSync(temporary)) {
      const directory = join(temporary, name);
      if (name.startsWith("outbound.") && statSync(directory).mtimeMs >= began - 1000) {
        directories.push(join(directory, "stack"));
      }
    }
  }
  const leaders = [];
  for (const directory of directories) {
    if (!existsSync(directory)) continue;
    for (const name of readdirSync(directory).filter((each) => each.endsWith(".pid"))) {
      const [pid, started] = readFileSync(join(directory, name), "utf8").split("\n");
      if (/^\d+$/.test(pid ?? "")) leaders.push({ pid: +pid, started: squeeze(started ?? "") });
    }
  }
  return leaders;
}

function kit(gatePid) {
  const rows = processTable();
  const byPid = new Map(rows.map((row) => [row.pid, row]));
  const groups = new Set();
  for (const leader of recordedLeaders()) {
    const row = byPid.get(leader.pid);
    if (!row || row.started === leader.started) groups.add(leader.pid);
  }
  const members = new Set(rows.filter((row) => groups.has(row.group)).map((row) => row.pid));
  if (gatePid !== undefined && byPid.has(gatePid)) {
    members.add(gatePid);
    for (let grew = true; grew; ) {
      grew = false;
      for (const row of rows) {
        if (!members.has(row.pid) && members.has(row.parent)) {
          members.add(row.pid);
          grew = true;
        }
      }
    }
  }
  let rssKib = 0;
  for (const pid of members) rssKib += byPid.get(pid).rssKib;
  return { mib: rssKib / 1024, processes: members.size };
}

async function highest(samples, gatePid) {
  let top = { mib: 0, processes: 0 };
  for (let index = 0; index < samples; index++) {
    const now = kit(gatePid);
    if (now.mib > top.mib) top = now;
    if (index < samples - 1) await sleep(1000);
  }
  return top;
}

const stack = (command) => spawnSync("sh", [join(root, "scripts/stack.sh"), command], { cwd: root, encoding: "utf8" });

async function measure() {
  stack("down");
  try {
    const start = performance.now();
    const up = stack("up");
    const readySeconds = (performance.now() - start) / 1000;
    if (up.status !== 0) throw new Error(`stack.sh up failed:\n${up.stdout}${up.stderr}`);
    await sleep(2000);
    const afterReady = await highest(3);
    if (afterReady.processes === 0) throw new Error("the stack is up, but no process of it was found: the sums would be 0");

    const gate = spawn("sh", [join(root, "scripts/gate.sh")], { cwd: root, stdio: ["ignore", "pipe", "ignore"] });
    let output = "";
    gate.stdout.on("data", (chunk) => {
      output += chunk;
    });
    let running = true;
    const finished = new Promise((resolve) => gate.on("close", (code) => resolve(code))).finally(() => {
      running = false;
    });
    const gateStart = performance.now();
    let peak = { mib: 0, processes: 0 };
    let samples = 0;
    while (running) {
      const now = kit(gate.pid);
      samples++;
      if (now.mib > peak.mib) peak = now;
      await sleep(SLEEP_MS);
    }
    await finished;
    const gateSeconds = (performance.now() - gateStart) / 1000;
    await sleep(5000);
    const idle = await highest(3);
    const results = output.split("\n").filter((line) => /^(PASS|FAIL) /.test(line));
    const outbound = results.find((line) => / outbound: /.test(line)) ?? "no outbound line";
    const stubs = results.filter((line) => / not built yet /.test(line)).length;
    return { readySeconds, afterReady, peak, samples, gateSeconds, idle, outbound, checks: results.length, stubs };
  } finally {
    stack("down");
  }
}

const results = [];
for (let run = 0; run < runs; run++) results.push(await measure());

const sysctl = (name) => execFileSync("sysctl", ["-n", name], { encoding: "utf8" }).trim();
const fixed = (value, digits = 1) => value.toFixed(digits);
const median = (values) => {
  const ordered = [...values].sort((a, b) => a - b);
  const middle = ordered.length / 2;
  return ordered.length % 2 === 1 ? ordered[Math.floor(middle)] : (ordered[middle - 1] + ordered[middle]) / 2;
};
const ready = results.map((result) => result.readySeconds);
const top = Math.max(...results.flatMap((result) => [result.afterReady.mib, result.peak.mib, result.idle.mib]));
const outboundClean = results.every((result) => result.outbound.startsWith("PASS outbound: denied=0"));
const met = top <= LIMIT_MIB && outboundClean;
const gib = fixed(Number(sysctl("hw.memsize")) / 2 ** 30, 0);
const interval = Math.max(...results.map((result) => (result.gateSeconds * 1000) / result.samples));
const last = results[results.length - 1];

const lines = [
  "# S2: kit footprint, Mac leg",
  "",
  `rule: ${met ? "MET" : "MISSED"} (the highest summed RSS was ${fixed(top, 0)} MiB against a limit of ${LIMIT_MIB} MiB, and ` +
    `${outboundClean ? "no run attempted a connection outside loopback" : "a run attempted a connection outside loopback, or the outbound check failed"})`,
  "",
  `Measured ${new Date().toISOString()} on ${sysctl("machdep.cpu.brand_string")}, ${gib} GiB, ` +
    `macOS ${execFileSync("sw_vers", ["-productVersion"], { encoding: "utf8" }).trim()}, Node ${process.version}. ` +
    `${runs} runs, each from a stopped stack.`,
  "",
  "| run | ready (s) | RSS after ready (MiB) | RSS at the peak of gate.sh (MiB) | RSS idle after gate.sh (MiB) | gate.sh (s) | outbound |",
  "|---|---|---|---|---|---|---|",
  ...results.map(
    (result, index) =>
      `| ${index + 1} | ${fixed(result.readySeconds, 2)} | ${fixed(result.afterReady.mib, 0)} (${result.afterReady.processes} processes) | ` +
      `${fixed(result.peak.mib, 0)} (${result.peak.processes} processes, ${result.samples} samples) | ` +
      `${fixed(result.idle.mib, 0)} (${result.idle.processes} processes) | ${fixed(result.gateSeconds, 1)} | ${result.outbound} |`,
  ),
  "",
  `- Ready time: median ${fixed(median(ready), 2)} s, from ${fixed(Math.min(...ready), 2)} to ${fixed(Math.max(...ready), 2)} s.`,
  "  It is the wall time of `sh scripts/stack.sh up` when no member runs, until stripe-mock and both Worker entries answer over HTTP.",
  "  The script starts the three members together and polls each every 0.1 s, so the slowest member sets it and it is read to about 0.1 s.",
  "  It includes starting node and wrangler; files were already in the page cache, so a first start after a reboot is slower and was not measured.",
  "- RSS: the sum of the `rss` column of `ps` over the kit's process tree, which counts a page that processes share once per process.",
  "  It is an upper bound on the memory the kit holds. The tree is every process in a process group that scripts/stack.sh recorded",
  "  (the stack, and the stacks that the outbound check runs under the sandbox) plus gate.sh and its descendants.",
  "- After ready: the highest of 3 samples, 1 s apart, starting 2 s after the stack answered.",
  `- Peak of gate.sh: the highest of the samples taken while gate.sh ran, one every ${fixed(interval, 0)} ms at most`,
  `  (a ${SLEEP_MS} ms sleep plus one ps call); a spike shorter than that can be missed.`,
  "  The gate.sh column is the time from its start to the first sample after it ended.",
  `- What gate.sh ran: in the last run it printed ${last.checks} result lines, of which ${last.stubs} were stubs that do no work.`,
  "  The sums hold only what those checks start. A check that later starts a test runner, a browser or the model checker adds to the peak.",
  "- Idle after gate.sh: the highest of 3 samples, 1 s apart, starting 5 s after gate.sh ended, with the stack still up.",
  "- Outbound: the line that gate.sh printed for the outbound check in the same run. Its count can miss a single attempt by a",
  "  binary that Apple did not sign and that is not Node (see the README, Gate).",
  `- The limit is 4 GiB, one quarter of a 16 GiB laptop. Every number here was measured on this ${gib} GiB machine.`,
  "  No run on a 16 GiB machine exists: the sums are an upper bound on what the kit asks for, and say nothing about how a",
  "  16 GiB machine behaves when memory is short.",
  "",
  "Reproduce: `node spikes/s2/measure.mjs 3 > spikes/s2/RESULT.md` after `sh scripts/prerequisites.sh`. It stops the stack before and after each run.",
  "",
];
process.stdout.write(lines.join("\n"));
process.exit(met ? 0 : 1);
