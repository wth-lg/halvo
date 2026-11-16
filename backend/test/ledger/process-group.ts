import { type Child, execFileSync, spawn } from "node:child_process";
import { mkdirSync, readdirSync, readFileSync, unlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";

declare module "node:child_process" {
  export function spawn(
    command: string,
    args: string[],
    options: { env?: Record<string, string | undefined>; stdio: ("ignore" | "pipe" | "inherit" | number)[]; detached: true },
  ): Child;
  export function execFileSync(command: string, args: string[], options: { encoding: "utf8"; env: Record<string, string | undefined> }): string;
}

declare module "node:fs" {
  export function unlinkSync(path: string): void;
}

type Options = { env?: Record<string, string | undefined>; stdio: ("ignore" | "pipe" | "inherit" | number)[] };

export interface Group {
  pid: number;
  started: string;
  child: Child;
}

/** What the kit asks of the system; the kit's own tests replace it with a recorded world (useProcesses). */
export interface Processes {
  ps(): string;
  spawn(command: string, args: string[], options: Options): Child;
  kill(pid: number, signal: string): void;
  exit(code: number): void;
  on(event: string, listener: () => void): void;
  off(event: string, listener: () => void): void;
}

interface Id {
  pid: number;
  started: string;
}

interface Mark {
  pid: number;
  started: string | null;
  spawned: number;
  owner: Id;
}

interface Row {
  pid: number;
  pgid: number;
  started: string;
}

interface Live {
  processes: Processes;
  groups: Map<Group, string>;
  owner: Id | null;
  signals: boolean;
  installed: boolean;
}

const GRACE_MS = 5_000;
const STOPS = ["SIGTERM", "SIGINT", "SIGHUP"] as const;
const LIVE = Symbol.for("halvo.test.process-group");
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const pause = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
const sleep = (ms: number) => Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);

const SYSTEM: Processes = {
  ps: () => execFileSync("ps", ["-A", "-o", "pid=,pgid=,stat=,lstart="], { encoding: "utf8", env: { ...process.env, TZ: "UTC0", LC_ALL: "C" } }),
  spawn: (command, args, options) => spawn(command, args, { ...options, detached: true }),
  kill: (pid, signal) => {
    process.kill(pid, signal);
  },
  exit: (code) => process.exit(code),
  on: (event, listener) => {
    process.on(event, listener);
  },
  off: (event, listener) => {
    process.removeListener(event, listener);
  },
};

function live(): Live {
  const shared = globalThis as unknown as Record<symbol, Live | undefined>;
  const found = shared[LIVE];
  if (found !== undefined) return found;
  const made: Live = { processes: SYSTEM, groups: new Map(), owner: null, signals: false, installed: false };
  shared[LIVE] = made;
  return made;
}

/** For the kit's own tests: puts processes in place of the system and forgets every group, mark path and handler that the kit knew. */
export function useProcesses(processes: Processes | null): void {
  const shared = globalThis as unknown as Record<symbol, Live | undefined>;
  shared[LIVE] = { processes: processes ?? SYSTEM, groups: new Map(), owner: null, signals: false, installed: false };
}

function table(): Row[] {
  const rows: Row[] = [];
  for (const line of live().processes.ps().split("\n")) {
    const match = /^\s*(\d+)\s+(\d+)\s+(\S+)\s+(\S.*\S)\s*$/.exec(line);
    if (match === null || (match[3] as string).startsWith("Z")) continue;
    rows.push({ pid: Number(match[1]), pgid: Number(match[2]), started: (match[4] as string).replace(/\s+/g, " ") });
  }
  return rows;
}

function epoch(started: string): number | null {
  const match = /^\w{3} (\w{3}) (\d{1,2}) (\d{2}):(\d{2}):(\d{2}) (\d{4})$/.exec(started);
  if (match === null) return null;
  const month = MONTHS.indexOf(match[1] as string);
  if (month < 0) return null;
  return Date.UTC(Number(match[6]), month, Number(match[2]), Number(match[3]), Number(match[4]), Number(match[5])) / 1000;
}

function members(group: Id, rows = table()): number[] {
  const inGroup = rows.filter((row) => row.pgid === group.pid);
  const leader = inGroup.find((row) => row.pid === group.pid);
  if (leader !== undefined && leader.started !== group.started) return [];
  return inGroup.map((row) => row.pid);
}

function leads(group: Id, rows: Row[]): boolean {
  return rows.some((row) => row.pid === group.pid && row.pgid === group.pid && row.started === group.started);
}

function send(group: Id, name: "SIGTERM" | "SIGKILL"): void {
  try {
    live().processes.kill(-group.pid, name);
  } catch {}
}

function stopNow(groups: Id[]): Id[] {
  const rows = table();
  const confirmed = groups.filter((group) => leads(group, rows));
  const left = groups.filter((group) => !confirmed.includes(group) && members(group, rows).length > 0);
  let going = confirmed;
  for (const name of ["SIGTERM", "SIGKILL"] as const) {
    for (const group of going) send(group, name);
    const until = Date.now() + GRACE_MS;
    while (going.some((group) => members(group).length > 0) && Date.now() < until) sleep(100);
    going = going.filter((group) => members(group).length > 0);
    if (going.length === 0) break;
  }
  return [...left, ...going];
}

function unmark(path: string): void {
  try {
    unlinkSync(path);
  } catch {}
}

function stopAll(state: Live): void {
  const groups = [...state.groups.keys()];
  const left = stopNow(groups);
  for (const group of groups) {
    if (!left.includes(group)) unmark(state.groups.get(group) as string);
  }
  state.groups.clear();
}

function install(state: Live): void {
  const processes = state.processes;
  if (!state.signals) {
    state.signals = true;
    const handlers = new Map<string, () => void>();
    for (const name of STOPS) {
      handlers.set(name, () => {
        stopAll(state);
        for (const [each, handler] of handlers) processes.off(each, handler);
        state.signals = false;
        processes.kill(process.pid, name);
      });
    }
    for (const [name, handler] of handlers) processes.on(name, handler);
  }
  if (!state.installed) {
    state.installed = true;
    processes.on("disconnect", () => {
      stopAll(state);
      processes.exit(1);
    });
    processes.on("exit", () => stopAll(state));
  }
}

/**
 * The directory of the marks of a checkout: HALVO_GROUP_MARKS when it is set and not empty, otherwise .tmp/groups
 * under the working directory, which is backend/ of the checkout when the node tests run.
 */
export function marksDir(): string {
  const set: string | undefined = process.env.HALVO_GROUP_MARKS;
  return set === undefined || set === "" ? join(process.cwd(), ".tmp", "groups") : set;
}

/**
 * Starts command in a process group of its own and returns the group, known by its leader's pid and start time
 * (ps lstart read with TZ=UTC0 and LC_ALL=C). Right after the spawn it writes the mark group-<pid>.json into
 * marksDir() with the pid, the second before the spawn and the pid and start time of the process that started it
 * (the owner); after its read of ps it adds the leader's start time; it removes the mark once the group has no member
 * left. If that read fails or the child does not lead a group of its own, it sends KILL to the group, removes the
 * mark and throws. Until endGroup ends it, the owner ends the group, TERM first and KILL after 5 s, when it gets
 * TERM, INT or HUP (and then dies by that signal), when its IPC channel to vitest's main process closes (and then
 * exits with 1), and when it exits; it signals a group only while its leader has the recorded pid and start time.
 * A KILL of the owner runs none of these: the group and its mark stay until sweepMarks.
 */
export function startGroup(command: string, args: string[], options: Options): Group {
  const state = live();
  if (state.owner === null) {
    const self = table().find((row) => row.pid === process.pid);
    if (self === undefined) throw new Error(`the process table has no row for this process (pid ${process.pid})`);
    state.owner = { pid: self.pid, started: self.started };
  }
  install(state);
  const owner = state.owner;
  const dir = marksDir();
  mkdirSync(dir, { recursive: true });
  const spawned = Math.floor(Date.now() / 1000);
  const child = state.processes.spawn(command, args, options);
  if (child.pid === undefined) throw new Error(`${command} did not start`);
  const mark = join(dir, `group-${child.pid}.json`);
  const record: Mark = { pid: child.pid, started: null, spawned, owner };
  try {
    writeFileSync(mark, JSON.stringify(record));
    const own = table().find((row) => row.pid === child.pid);
    if (own === undefined || own.pgid !== child.pid) throw new Error(`${command} (pid ${child.pid}) does not lead its own process group`);
    const group = { pid: child.pid, started: own.started, child };
    state.groups.set(group, mark);
    writeFileSync(mark, JSON.stringify({ ...record, started: own.started }));
    return group;
  } catch (error) {
    send({ pid: child.pid, started: "" }, "SIGKILL");
    unmark(mark);
    throw error;
  }
}

/**
 * Ends the group with TERM and, if a member is left after 5 s, with KILL; throws if a member is left 5 s after KILL.
 * Unlike the handlers and the sweep, it also signals the members of a group whose leader has exited, as the caller
 * holds the group that it started: a test that stops a run's leader itself ends what the run left in its group.
 */
export async function endGroup(group: Group | null): Promise<void> {
  if (group === null) return;
  if (members(group).length > 0) {
    send(group, "SIGTERM");
    if (!(await gone(group, GRACE_MS))) {
      send(group, "SIGKILL");
      await gone(group, GRACE_MS);
    }
  }
  if (members(group).length > 0) throw new Error(`process group ${group.pid} (started ${group.started}) still has members ${members(group).join(" ")}`);
  const state = live();
  const mark = state.groups.get(group);
  if (mark !== undefined) unmark(mark);
  state.groups.delete(group);
}

async function gone(group: Id, ms: number): Promise<boolean> {
  const until = Date.now() + ms;
  while (members(group).length > 0) {
    if (Date.now() >= until) return false;
    await pause(100);
  }
  return true;
}

function readMark(path: string): Mark | null {
  try {
    const value: unknown = JSON.parse(readFileSync(path, "utf8"));
    const pid = (each: unknown): boolean => Number.isSafeInteger(each) && (each as number) > 1;
    const id = (each: unknown): each is Id => typeof each === "object" && each !== null && pid((each as Id).pid) && typeof (each as Id).started === "string";
    if (typeof value !== "object" || value === null) return null;
    const { pid: marked, started, spawned, owner } = value as Record<string, unknown>;
    if (!pid(marked) || !id(owner) || !(typeof started === "string" || (started === null && Number.isSafeInteger(spawned)))) return null;
    return { pid: marked as number, started: started as string | null, spawned: Number(spawned), owner: { pid: owner.pid, started: owner.started } };
  } catch {
    return null;
  }
}

/**
 * Ends the groups that the marks in marksDir() name and whose owner is gone: what a run of this checkout left when
 * its process got KILL. A mark whose owner (pid and start time) is alive is kept, since a run that is still going owns
 * it. A group whose leader has the marked pid and start time (for a mark written before the start time was read, a
 * leader of the marked pid that started within a second before or two seconds after the spawn) gets TERM, and KILL
 * after 5 s, and its mark goes once it has no member. A group whose leader is gone while processes still have its pid
 * as their group is left with its mark, since nothing tells those processes from a later group of the same id.
 * Returns one line for each group that it ended or left, and for each mark that it could not read.
 */
export function sweepMarks(): string[] {
  const dir = marksDir();
  let names: string[];
  try {
    names = readdirSync(dir);
  } catch {
    return [];
  }
  const lines: string[] = [];
  for (const name of names.filter((each) => /^group-\d+\.json$/.test(each)).sort()) {
    const path = join(dir, name);
    const mark = readMark(path);
    if (mark === null) {
      lines.push(`process-group: skipped the mark ${name}, which is no group with an owner`);
      continue;
    }
    const rows = table();
    if (rows.some((row) => row.pid === mark.owner.pid && row.started === mark.owner.started)) continue;
    const leader = rows.find((row) => row.pid === mark.pid && row.pgid === mark.pid);
    const at = leader === undefined ? null : epoch(leader.started);
    const started =
      mark.started ?? (leader !== undefined && at !== null && at >= mark.spawned - 1 && at <= mark.spawned + 2 ? leader.started : "no start time");
    const group = { pid: mark.pid, started };
    const count = members(group, rows).length;
    if (count === 0) {
      unmark(path);
    } else if (!leads(group, rows)) {
      lines.push(`process-group: left group ${mark.pid} (started ${started}): its leader is gone and ${count} processes have its id as their group`);
    } else if (stopNow([group]).length === 0) {
      unmark(path);
      lines.push(`process-group: ended group ${mark.pid} (started ${started}), which a run of this checkout left: ${count} processes`);
    } else {
      lines.push(`process-group: group ${mark.pid} (started ${started}) still has members after KILL`);
    }
  }
  return lines;
}
