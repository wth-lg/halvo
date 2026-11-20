import { mkdirSync, mkdtempSync, readdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import { afterAll, beforeEach, expect, test } from "vitest";
import { type Processes, startGroup, useProcesses } from "./process-group.ts";

interface FRow {
  pid: number;
  pgid: number;
  stat: string;
  started: string;
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
    rows = rows.filter((row) => row.pgid !== -pid);
  },
  exit: () => {},
  on: () => {},
  off: () => {},
};

const marksIn = () => readdirSync(marks).sort();

beforeEach(() => {
  rows = [{ pid: process.pid, pgid: process.pid, stat: "S", started: OWNER }];
  events = [];
  failReads = 0;
  leadsNot = false;
  goneAtRead = false;
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
