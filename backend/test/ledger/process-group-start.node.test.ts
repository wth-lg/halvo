import { mkdirSync, mkdtempSync, readdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import { afterAll, beforeEach, expect, test } from "vitest";
import { endGroup, groupMembers, type Processes, startGroup, useProcesses } from "./process-group.ts";

interface FRow {
  pid: number;
  pgid: number;
  stat: string;
  started: string;
  termProof?: boolean;
}

const ROOT = process.cwd();
const OWNER = "Thu Jan 1 00:00:10 1970";
const LATER = "Thu Jan 1 01:00:00 1970";

let rows: FRow[] = [];
let events: string[] = [];
let failReads = 0;
let leadsNot = false;
let goneAtRead = false;
let nextPid = 200_000;
let marks = "";
let handlers = new Map<string, (() => void)[]>();
const dirs: string[] = [];

const world: Processes = {
  ps: () => {
    if (events.includes("spawn") && failReads > 0) {
      failReads -= 1;
      events.push("ps failed");
      throw new Error("ps was stopped");
    }
    if (events.includes("spawn")) events.push("ps");
    if (goneAtRead) return rows.filter((row) => row.pid !== nextPid).map((row) => `${row.pid} ${row.pgid} ${row.stat} ${row.started}`).join("\n");
    return rows.map((row) => `${row.pid} ${row.pgid} ${row.stat} ${row.started}`).join("\n");
  },
  spawn: () => {
    nextPid += 1;
    rows.push({ pid: nextPid, pgid: leadsNot ? 1 : nextPid, stat: "S", started: LATER });
    events.push("spawn");
    return { pid: nextPid } as unknown as ReturnType<Processes["spawn"]>;
  },
  kill: (pid, signal) => {
    events.push(`${signal} ${pid}`);
    if (pid < 0 && !rows.some((row) => row.pgid === -pid)) throw new Error("ESRCH");
    rows = rows.filter((row) => row.pgid !== -pid || (signal === "SIGTERM" && row.termProof === true));
  },
  exit: () => {},
  on: (event, listener) => {
    handlers.set(event, [...(handlers.get(event) ?? []), listener]);
  },
  off: (event, listener) => {
    handlers.set(
      event,
      (handlers.get(event) ?? []).filter((each) => each !== listener),
    );
  },
};

function fire(event: string): void {
  for (const listener of [...(handlers.get(event) ?? [])]) listener();
}

const pause = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
const groupSignals = (pid: number) => events.filter((event) => event.startsWith("SIG") && event.endsWith(` -${pid}`));
const rowOf = (pid: number) => rows.find((row) => row.pid === pid) as FRow;

const marksIn = () => readdirSync(marks).sort();

beforeEach(() => {
  rows = [{ pid: process.pid, pgid: process.pid, stat: "S", started: OWNER }];
  events = [];
  failReads = 0;
  leadsNot = false;
  goneAtRead = false;
  handlers = new Map();
  mkdirSync(join(ROOT, ".tmp"), { recursive: true });
  const made = mkdtempSync(join(ROOT, ".tmp", "process-group-start."));
  dirs.push(made);
  marks = join(made, "marks");
  process.env.HALVO_GROUP_MARKS = marks;
  useProcesses(world, 300);
});

afterAll(() => {
  useProcesses(null);
  delete process.env.HALVO_GROUP_MARKS;
  for (const dir of dirs) rmSync(dir, { recursive: true, force: true });
});

test("when the first read of ps after the spawn fails, startGroup reads ps again and sends KILL to the child's group only after that read showed the child's pid with its start time, removes the mark and throws", () => {
  failReads = 1;
  expect(() => startGroup("x", [], { stdio: ["ignore", "ignore", "ignore"] })).toThrow("ps was stopped");
  expect(events.slice(events.indexOf("spawn"))).toEqual(["spawn", "ps failed", "ps", `SIGKILL -${nextPid}`]);
  expect(marksIn()).toEqual([]);
});

test("when that second read shows no process with the child's pid, startGroup sends no signal, removes the mark and throws", () => {
  failReads = 1;
  goneAtRead = true;
  expect(() => startGroup("x", [], { stdio: ["ignore", "ignore", "ignore"] })).toThrow("ps was stopped");
  expect(events.slice(events.indexOf("spawn"))).toEqual(["spawn", "ps failed", "ps"]);
  expect(marksIn()).toEqual([]);
});

test("when ps cannot be read again either, startGroup sends KILL to the group of the child that it still holds: the one signal of the kit with no start time, which backend/README.md names", () => {
  failReads = 2;
  expect(() => startGroup("x", [], { stdio: ["ignore", "ignore", "ignore"] })).toThrow("ps was stopped");
  expect(events.slice(events.indexOf("spawn"))).toEqual(["spawn", "ps failed", "ps failed", `SIGKILL -${nextPid}`]);
  expect(marksIn()).toEqual([]);
});

test("groupMembers names each process that has the group's pid as its group id, with its start time, leaves out zombies, names none when the pid has another start time, and sends nothing", () => {
  rows.push(
    { pid: 300_001, pgid: 300_001, stat: "S", started: LATER },
    { pid: 300_002, pgid: 300_001, stat: "S", started: OWNER },
    { pid: 300_003, pgid: 300_001, stat: "Z", started: LATER },
    { pid: 300_004, pgid: 300_004, stat: "S", started: LATER },
    { pid: 300_005, pgid: 300_006, stat: "S", started: LATER },
  );
  expect(groupMembers({ pid: 300_001, started: LATER })).toEqual([
    { pid: 300_001, started: LATER },
    { pid: 300_002, started: OWNER },
  ]);
  expect(groupMembers({ pid: 300_001, started: OWNER })).toEqual([]);
  expect(groupMembers({ pid: 300_006, started: LATER })).toEqual([{ pid: 300_005, started: LATER }]);
  expect(events.filter((event) => event.startsWith("SIG"))).toEqual([]);
});

test("startGroup's errors name the command by its file name and not by the path that it was given", () => {
  leadsNot = true;
  const thrown = (() => {
    try {
      startGroup("/opt/kit-tools/bin/x-server", [], { stdio: ["ignore", "ignore", "ignore"] });
    } catch (error) {
      return String(error);
    }
    return "no error";
  })();
  expect(thrown).toBe(`Error: x-server (pid ${nextPid}) does not lead its own process group`);
  const spawnOf = world.spawn;
  world.spawn = () => ({ pid: undefined }) as unknown as ReturnType<Processes["spawn"]>;
  try {
    expect(() => startGroup("/opt/kit-tools/bin/x-server", [], { stdio: ["ignore", "ignore", "ignore"] })).toThrow(/^x-server did not start$/);
  } finally {
    world.spawn = spawnOf;
  }
});

test("endGroup: a stop that lands while endGroup reads ps or waits still ends the group, and its mark goes", async () => {
  const a = startGroup("server", [], { stdio: ["ignore", "ignore", "ignore"] });
  failReads = 1;
  await expect(endGroup(a)).rejects.toThrow("ps was stopped");
  fire("SIGINT");
  fire("exit");
  expect({ case: "the first read of endGroup fails", signals: groupSignals(a.pid), marks: marksIn() }).toEqual({
    case: "the first read of endGroup fails",
    signals: [`SIGTERM -${a.pid}`],
    marks: [],
  });
  const b = startGroup("server", [], { stdio: ["ignore", "ignore", "ignore"] });
  rowOf(b.pid).termProof = true;
  const ending = endGroup(b);
  await pause(150);
  fire("SIGINT");
  fire("exit");
  const signals = groupSignals(b.pid);
  expect({ case: "INT during the wait", term: signals.includes(`SIGTERM -${b.pid}`), last: signals.at(-1), marks: marksIn() }).toEqual({
    case: "INT during the wait",
    term: true,
    last: `SIGKILL -${b.pid}`,
    marks: [],
  });
  await ending;
}, 20_000);

test("endGroup: a second endGroup of the same group resolves only after the first has sent its KILL to a member that the leader left", async () => {
  const a = startGroup("server", [], { stdio: ["ignore", "ignore", "ignore"] });
  nextPid += 1;
  rows.push({ pid: nextPid, pgid: a.pid, stat: "S", started: LATER, termProof: true });
  const first = endGroup(a);
  await pause(150);
  await endGroup(a);
  expect(groupSignals(a.pid)).toEqual([`SIGTERM -${a.pid}`, `SIGKILL -${a.pid}`]);
  await first;
  expect(marksIn()).toEqual([]);
}, 20_000);
