import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, openSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test, vi } from "vitest";
import { endGroup, type Group, groupMembers, startGroup } from "./process-group.ts";

const ROOT = process.cwd();
const SELF = "test/ledger/server-stops.node.test.ts";
const MODE: string | undefined = process.env.HALVO_SERVER_STOP_MODE;
const HERE: string | undefined = process.env.HALVO_SERVER_STOP_DIR;
const SETTLE_MS = 20_000;
const STOP_MS = 10_000;
const pause = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

interface Id {
  pid: number;
  started: string;
}

function startOf(pid: number): string | null {
  try {
    const text = execFileSync("ps", ["-o", "lstart=", "-p", String(pid)], { encoding: "utf8", env: { ...process.env, TZ: "UTC0", LC_ALL: "C" } });
    return text.trim() === "" ? null : text.trim().replace(/\s+/g, " ");
  } catch {
    return null;
  }
}

function signal(id: Id, name: string): void {
  if (startOf(id.pid) === id.started) process.kill(id.pid, name);
}

function made(): string {
  mkdirSync(join(ROOT, ".tmp"), { recursive: true });
  return mkdtempSync(join(ROOT, ".tmp", "server-stops."));
}

function startRun(dir: string, mode: string): Group {
  const log = openSync(join(dir, `${mode}-run.log`), "a");
  return startGroup(join(ROOT, "node_modules/.bin/vitest"), ["run", "--project", "node", SELF], {
    stdio: ["ignore", log, log],
    env: { ...process.env, HALVO_SERVER_STOP_MODE: mode, HALVO_SERVER_STOP_DIR: dir, HALVO_GROUP_MARKS: join(dir, "marks") },
  });
}

function logOf(dir: string, mode: string): string {
  return readFileSync(join(dir, `${mode}-run.log`), "utf8").slice(-3000);
}

function marked(dir: string): Id[] {
  const marks = join(dir, "marks");
  if (!existsSync(marks)) return [];
  return readdirSync(marks)
    .filter((name) => /^group-\d+\.json$/.test(name))
    .flatMap((name) => {
      try {
        const mark = JSON.parse(readFileSync(join(marks, name), "utf8"));
        return typeof mark.started === "string" ? [{ pid: mark.pid, started: mark.started }] : [];
      } catch {
        return [];
      }
    });
}

const membersOf = (ids: Id[]): Id[] => ids.flatMap((id) => groupMembers(id));

function described(ids: Id[]): string[] {
  if (ids.length === 0) return [];
  let text = "";
  try {
    text = execFileSync("ps", ["-o", "pid=,command=", "-p", ids.map((id) => id.pid).join(",")], { encoding: "utf8" });
  } catch {}
  return ids.map((id) => {
    const line = text.split("\n").find((each) => Number(each.trim().split(/\s+/)[0]) === id.pid);
    const words = (line ?? "").trim().split(/\s+/).slice(1);
    const base = (word: string | undefined) => (word ?? "").slice((word ?? "").lastIndexOf("/") + 1);
    const name = base(words[0]) === "node" ? `node ${base(words[1])}` : base(words[0]);
    return `pid ${id.pid}: ${name || "gone"}`;
  });
}

async function settle(ids: Id[], ms = SETTLE_MS): Promise<Id[]> {
  const until = Date.now() + ms;
  for (;;) {
    const left = membersOf(ids);
    if (left.length === 0 || Date.now() >= until) return left;
    await pause(100);
  }
}

async function endRecorded(id: Id): Promise<void> {
  for (const name of ["SIGTERM", "SIGKILL"]) {
    if (groupMembers(id).length === 0) return;
    try {
      process.kill(-id.pid, name);
    } catch {
      return;
    }
    if ((await settle([id])).length === 0) return;
  }
}

async function waitReady(dir: string, mode: string, run: Group): Promise<void> {
  for (let tries = 0; tries < 900; tries += 1) {
    if (existsSync(join(dir, "ready.json"))) return;
    if (groupMembers(run).length === 0) throw new Error(`the ${mode} run ended before it was ready:\n${logOf(dir, mode)}`);
    await pause(100);
  }
  throw new Error(`the ${mode} run was not ready after 90 s:\n${logOf(dir, mode)}`);
}

async function withRun(mode: string, body: (dir: string, run: Group, watched: Id[], runs: Group[]) => Promise<void>): Promise<void> {
  const dir = made();
  const run = startRun(dir, mode);
  const watched: Id[] = [];
  const runs = [run];
  let failure: Error | null = null;
  try {
    await body(dir, run, watched, runs);
  } catch (error) {
    failure = error as Error;
  }
  try {
    for (const id of watched) await endRecorded(id);
    for (const each of runs) {
      await endRecorded(each);
      await endGroup(each);
    }
  } catch (error) {
    if (failure === null) throw error;
    failure.message += `\nand then: ${String(error)}`;
  }
  if (failure !== null) throw failure;
}

function ready(): void {
  writeFileSync(join(HERE as string, "ready.json"), "{}");
}

const hold = () => new Promise<never>(() => {});

const SLEEPER = "setInterval(() => {}, 1 << 30)";
const STUBBORN = 'process.on("SIGTERM", () => {}); setInterval(() => {}, 1 << 30)';

function exitOf(run: Group, ms: number): Promise<number | null | "still running"> {
  return Promise.race([new Promise<number | null>((resolve) => run.child.on("exit", (code) => resolve(code))), pause(ms).then(() => "still running" as const)]);
}

async function oneGroup(dir: string, mode: string, run: Group): Promise<Id> {
  await waitReady(dir, mode, run);
  const groups = marked(dir);
  if (groups.length !== 1) throw new Error(`the ${mode} run marked ${groups.length} groups, not 1:\n${logOf(dir, mode)}`);
  return groups[0] as Id;
}

function holdGroup(code: string): Promise<never> {
  startGroup(process.execPath, ["-e", code], { stdio: ["ignore", "ignore", "ignore"] });
  ready();
  return hold();
}

const STARTS: [string, string][] = [
  ["gateway-chat", "the mock upstream of test/ledger/gateway-chat.node.test.ts"],
  ["stripe-mock", "stripe-mock, which startStripeMock of test/billing/servers.ts starts"],
  ["stripe-meter-load", "the node child of test/metering/stripe-meter-load.node.test.ts"],
];

if (MODE !== undefined) {
  if (HERE === undefined) throw new Error("HALVO_SERVER_STOP_MODE is set and HALVO_SERVER_STOP_DIR is not");
  if (MODE === "gateway-chat") {
    await import("./gateway-chat.node.test.ts");
    test("holds the mock upstream of gateway-chat.node.test.ts up until the run is stopped", () => {
      ready();
      return hold();
    }, 300_000);
  } else if (MODE === "stripe-mock") {
    test("holds stripe-mock up until the run is stopped", async () => {
      const { startStripeMock } = await import("../billing/servers.ts");
      await startStripeMock();
      ready();
      return hold();
    }, 300_000);
  } else if (MODE === "stripe-meter-load") {
    vi.doMock("../billing/servers.ts", async (importOriginal) => {
      const real = await importOriginal<typeof import("../billing/servers.ts")>();
      const startRecorder: typeof real.startRecorder = (answer) =>
        real.startRecorder((request) => {
          if (answer(request) === "hang") return "hang";
          ready();
          return "hang";
        });
      return { ...real, startRecorder };
    });
    await import("../metering/stripe-meter-load.node.test.ts");
  } else if (MODE === "sleeper") {
    test("holds a group of a sleeping node up until the run is stopped", () => holdGroup(SLEEPER), 300_000);
  } else if (MODE === "stubborn") {
    test("holds a group of a node that outlives TERM up until the run is stopped", () => holdGroup(STUBBORN), 300_000);
  } else if (MODE === "stubborn-end") {
    test("starts a group of a node that outlives TERM and ends, so that vitest stops the fork", () => {
      startGroup(process.execPath, ["-e", STUBBORN], { stdio: ["ignore", "ignore", "ignore"] });
      ready();
    });
  } else if (MODE === "sweep") {
    test("starts nothing, after the run's set-up", () => {});
  } else throw new Error(`no mode ${MODE}`);
} else {
  for (const [mode, server] of STARTS) {
    test(`${server}: a run whose main process alone gets INT while it is up leaves no process of the run's group or of a group that the run marked, and it runs in a group that the run marked`, () =>
      withRun(mode, async (dir, run, watched) => {
        await waitReady(dir, mode, run);
        const groups = marked(dir);
        watched.push(...groups);
        const where = groups.some((id) => groupMembers(id).length > 0) ? "in a group that the run marked" : "in the run's own process group, which no mark names";
        signal(run, "SIGINT");
        expect({ where, left: described(await settle([run, ...groups], STOP_MS)) }).toEqual({ where: "in a group that the run marked", left: [] });
      }), 120_000);
  }

  test("stop kind INT to vitest's main process alone: a run whose main process alone gets INT leaves no process of the run's group or of the group that it started", () =>
    withRun("sleeper", async (dir, run, watched) => {
      const group = await oneGroup(dir, "sleeper", run);
      watched.push(group);
      signal(run, "SIGINT");
      expect(described(await settle([run, group], STOP_MS))).toEqual([]);
    }), 120_000);

  test("stop kind TERM by pid to every process of the run: when each process of the run's group and of the group that it started gets TERM by its pid, and that group's leader outlives TERM, none is left, since the kit sends KILL after its grace", () =>
    withRun("stubborn", async (dir, run, watched) => {
      const group = await oneGroup(dir, "stubborn", run);
      watched.push(group);
      const every = [...groupMembers(run), ...groupMembers(group)];
      expect(every.filter((id) => id.pid === group.pid)).toHaveLength(1);
      for (const id of every) signal(id, "SIGTERM");
      expect(described(await settle([run, group]))).toEqual([]);
    }), 120_000);

  test("stop kind the close of the channel by itself: a run whose main process alone gets KILL, so that its fork gets no signal and sees only its channel to vitest close, leaves no process of the run's group or of the group that it started", () =>
    withRun("sleeper", async (dir, run, watched) => {
      const group = await oneGroup(dir, "sleeper", run);
      watched.push(group);
      signal(run, "SIGKILL");
      expect(described(await settle([run, group], STOP_MS))).toEqual([]);
    }), 120_000);

  test("stop kind vitest's KILL of a fork: when vitest stops a fork whose group outlives TERM, its KILL 500 ms after TERM comes before the kit's, the group keeps its mark, and the next run of the same checkout ends that group by its mark, so that none is left", () =>
    withRun("stubborn-end", async (dir, run, watched, runs) => {
      const group = await oneGroup(dir, "stubborn-end", run);
      watched.push(group);
      const code = await exitOf(run, 60_000);
      expect({ code, members: groupMembers(group).length, kept: marked(dir).some((id) => id.pid === group.pid) }, logOf(dir, "stubborn-end")).toEqual({ code: 0, members: 1, kept: true });
      const next = startRun(dir, "sweep");
      runs.push(next);
      expect(await exitOf(next, 90_000), logOf(dir, "sweep")).toBe(0);
      expect(described(await settle([group]))).toEqual([]);
      expect(logOf(dir, "sweep")).toContain(`process-group: ended group ${group.pid} (started ${group.started}), which a run of this checkout left: 1 process`);
    }), 180_000);
}
