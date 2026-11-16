import { type Child, execFileSync, spawn } from "node:child_process";
import { mkdirSync, readdirSync, readFileSync, unlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";

declare module "node:child_process" {
  export function spawn(
    command: string,
    args: string[],
    options: { env?: Record<string, string | undefined>; stdio: ("ignore" | "pipe" | "inherit" | number)[]; detached: true },
  ): Child;
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

interface Id {
  pid: number;
  started: string;
}

interface Mark extends Id {
  owner: Id;
}

interface Row {
  pid: number;
  pgid: number;
  started: string;
}

interface Live {
  groups: Map<Group, string>;
  installed: boolean;
}

const GRACE_MS = 5_000;
const STOPS = ["SIGTERM", "SIGINT", "SIGHUP"] as const;
const LIVE = Symbol.for("halvo.test.process-group");
const pause = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
const sleep = (ms: number) => Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);

function table(): Row[] {
  const text = execFileSync("ps", ["-A", "-o", "pid=,pgid=,stat=,lstart="], { encoding: "utf8" });
  const rows: Row[] = [];
  for (const line of text.split("\n")) {
    const match = /^\s*(\d+)\s+(\d+)\s+(\S+)\s+(\S.*\S)\s*$/.exec(line);
    if (match === null || (match[3] as string).startsWith("Z")) continue;
    rows.push({ pid: Number(match[1]), pgid: Number(match[2]), started: (match[4] as string).replace(/\s+/g, " ") });
  }
  return rows;
}

function members(group: Id, rows = table()): number[] {
  const inGroup = rows.filter((row) => row.pgid === group.pid);
  const leader = inGroup.find((row) => row.pid === group.pid);
  if (leader !== undefined && leader.started !== group.started) return [];
  return inGroup.map((row) => row.pid);
}

function signal(group: Id, name: "SIGTERM" | "SIGKILL"): boolean {
  if (members(group).length === 0) return false;
  try {
    process.kill(-group.pid, name);
    return true;
  } catch {
    return false;
  }
}

async function gone(group: Id, ms: number): Promise<boolean> {
  const until = Date.now() + ms;
  while (members(group).length > 0) {
    if (Date.now() >= until) return false;
    await pause(100);
  }
  return true;
}

function stopNow(group: Id): boolean {
  for (const name of ["SIGTERM", "SIGKILL"] as const) {
    if (!signal(group, name)) return members(group).length === 0;
    const until = Date.now() + GRACE_MS;
    while (members(group).length > 0 && Date.now() < until) sleep(100);
    if (members(group).length === 0) return true;
  }
  return false;
}

function unmark(path: string): void {
  try {
    unlinkSync(path);
  } catch {}
}

function live(): Live {
  const shared = globalThis as unknown as Record<symbol, Live | undefined>;
  const found = shared[LIVE];
  if (found !== undefined) return found;
  const made: Live = { groups: new Map(), installed: false };
  shared[LIVE] = made;
  return made;
}

function stopAll(state: Live): void {
  for (const [group, mark] of state.groups) {
    if (stopNow(group)) unmark(mark);
  }
  state.groups.clear();
}

function install(state: Live): void {
  if (state.installed) return;
  state.installed = true;
  const handlers = new Map<string, () => void>();
  for (const name of STOPS) {
    handlers.set(name, () => {
      stopAll(state);
      for (const [each, handler] of handlers) process.removeListener(each, handler);
      process.kill(process.pid, name);
    });
  }
  for (const [name, handler] of handlers) process.on(name, handler);
  process.on("disconnect", () => {
    stopAll(state);
    process.exit(1);
  });
  process.on("exit", () => stopAll(state));
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
 * Starts command in a process group of its own and returns the group, known by its leader's pid and start time.
 * It writes the mark group-<pid>.json into marksDir(), with the group's pid and start time and the pid and start time
 * of the process that started it (the owner), and removes the mark once the group has no member left.
 * Until endGroup ends it, the owner ends the group, TERM first and KILL after 5 s, when it gets TERM, INT or HUP
 * (and then dies by that signal), when its IPC channel to vitest's main process closes (and then exits with 1),
 * and when it exits. A KILL of the owner runs none of these: the group and its mark stay until sweepMarks.
 */
export function startGroup(command: string, args: string[], options: Options): Group {
  const state = live();
  install(state);
  const child = spawn(command, args, { ...options, detached: true });
  if (child.pid === undefined) throw new Error(`${command} did not start`);
  const rows = table();
  const own = rows.find((row) => row.pid === child.pid);
  if (own === undefined || own.pgid !== child.pid) throw new Error(`${command} (pid ${child.pid}) does not lead its own process group`);
  const owner = rows.find((row) => row.pid === process.pid);
  if (owner === undefined) throw new Error(`the process table has no row for this process (pid ${process.pid})`);
  const group = { pid: child.pid, started: own.started, child };
  const dir = marksDir();
  const mark = join(dir, `group-${group.pid}.json`);
  const record: Mark = { pid: group.pid, started: group.started, owner: { pid: owner.pid, started: owner.started } };
  state.groups.set(group, mark);
  mkdirSync(dir, { recursive: true });
  writeFileSync(mark, JSON.stringify(record));
  return group;
}

/** Ends the group with TERM and, if a member is left after 5 s, with KILL; throws if a member is left 5 s after KILL. */
export async function endGroup(group: Group | null): Promise<void> {
  if (group === null) return;
  signal(group, "SIGTERM");
  if (!(await gone(group, GRACE_MS))) {
    signal(group, "SIGKILL");
    if (!(await gone(group, GRACE_MS))) throw new Error(`process group ${group.pid} (started ${group.started}) still has members ${members(group).join(" ")}`);
  }
  const state = live();
  const mark = state.groups.get(group);
  if (mark !== undefined) unmark(mark);
  state.groups.delete(group);
}

function readMark(path: string): Mark | null {
  try {
    const value: unknown = JSON.parse(readFileSync(path, "utf8"));
    const id = (each: unknown): each is Id =>
      typeof each === "object" &&
      each !== null &&
      Number.isSafeInteger((each as Id).pid) &&
      (each as Id).pid > 1 &&
      typeof (each as Id).started === "string";
    if (!id(value)) return null;
    const owner = (value as { owner?: unknown }).owner;
    return id(owner) ? { pid: value.pid, started: value.started, owner: { pid: owner.pid, started: owner.started } } : null;
  } catch {
    return null;
  }
}

/**
 * Ends the groups that the marks in marksDir() name and whose owner is gone: what a run of this checkout left when
 * its process got KILL. A mark whose owner (pid and start time) is alive is kept, since a run that is still going owns
 * it. A group whose leader has the marked pid and start time gets TERM, and KILL after 5 s, and its mark goes once it
 * has no member. A group whose leader is gone while processes still have its pid as their group is left with its mark,
 * since nothing tells those processes from a later group of the same id. Returns one line for each group that it ended
 * or left, and for each mark that it could not read.
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
    const leader = rows.find((row) => row.pid === mark.pid);
    const count = members(mark, rows).length;
    if (count === 0) {
      unmark(path);
    } else if (leader === undefined) {
      lines.push(`process-group: left group ${mark.pid} (started ${mark.started}): its leader is gone and ${count} processes have its id as their group`);
    } else if (stopNow(mark)) {
      unmark(path);
      lines.push(`process-group: ended group ${mark.pid} (started ${mark.started}), which a run of this checkout left: ${count} processes`);
    } else {
      lines.push(`process-group: group ${mark.pid} (started ${mark.started}) still has members after KILL`);
    }
  }
  return lines;
}
