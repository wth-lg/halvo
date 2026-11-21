import { type Child, execFileSync, spawn } from "node:child_process";
import { mkdirSync, readdirSync, readFileSync, realpathSync, unlinkSync, writeFileSync } from "node:fs";
import { basename, dirname, join } from "node:path";

declare module "node:path" {
  export function basename(path: string): string;
}

declare module "node:child_process" {
  export function spawn(
    command: string,
    args: string[],
    options: { env?: Record<string, string | undefined>; stdio: ("ignore" | "pipe" | "inherit" | number)[]; detached: true },
  ): Child;
  export function execFileSync(command: string, args: string[], options: { encoding: "utf8"; env: Record<string, string | undefined> }): string;
  interface Child {
    on(event: "error", listener: (error: Error) => void): void;
  }
}

declare module "node:fs" {
  export function unlinkSync(path: string): void;
  export function realpathSync(path: string): string;
  export function rmSync(path: string, options: { recursive: true; force: true }): void;
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
  checkout: string;
}

interface Row {
  pid: number;
  pgid: number;
  started: string;
}

interface Entry {
  path: string;
  text: string;
}

interface Live {
  processes: Processes;
  groups: Map<Group, Entry>;
  ending: Map<Group, Promise<void>>;
  confirmed: Set<Group>;
  owner: Id | null;
  signals: boolean;
  installed: boolean;
  grace: number;
}

const GRACE_MS = 5_000;
const STOPS = ["SIGTERM", "SIGINT", "SIGHUP"] as const;
const LIVE = Symbol.for("halvo.test.process-group");
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const pause = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
const sleep = (ms: number) => Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
const processes = (n: number) => (n === 1 ? "1 process has" : `${n} processes have`);
const many = (n: number) => (n === 1 ? "1 process" : `${n} processes`);

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
  const made: Live = { processes: SYSTEM, groups: new Map(), ending: new Map(), confirmed: new Set(), owner: null, signals: false, installed: false, grace: GRACE_MS };
  shared[LIVE] = made;
  return made;
}

/**
 * For the kit's own tests: puts processes in place of the system, with a grace of graceMs between TERM and KILL
 * (5 s unless given), and forgets every group, mark path and handler that the kit knew.
 */
export function useProcesses(processes: Processes | null, graceMs = GRACE_MS): void {
  const shared = globalThis as unknown as Record<symbol, Live | undefined>;
  shared[LIVE] = { processes: processes ?? SYSTEM, groups: new Map(), ending: new Map(), confirmed: new Set(), owner: null, signals: false, installed: false, grace: graceMs };
}

function table(zombies = false): Row[] {
  const rows: Row[] = [];
  for (const line of live().processes.ps().split("\n")) {
    const match = /^\s*(\d+)\s+(\d+)\s+(\S+)\s+(\S.*\S)\s*$/.exec(line);
    if (match === null || (!zombies && (match[3] as string).startsWith("Z"))) continue;
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

/**
 * The processes that are not zombies and have the group's pid as their group id, each with its pid and start time, as
 * the kit reads ps; none when a process with that pid has another start time. It sends nothing.
 */
export function groupMembers(group: Id): Id[] {
  const rows = table();
  const ids = new Set(members(group, rows));
  return rows.filter((row) => ids.has(row.pid)).map((row) => ({ pid: row.pid, started: row.started }));
}

function leads(group: Id, rows: Row[]): boolean {
  return rows.some((row) => row.pid === group.pid && row.pgid === group.pid && row.started === group.started);
}

function send(group: Id, name: "SIGTERM" | "SIGKILL"): void {
  try {
    live().processes.kill(-group.pid, name);
  } catch {}
}

function stopNow(groups: Id[], ended: Set<Id> = new Set()): Id[] {
  const rows = table();
  const confirmed = groups.filter((group) => leads(group, rows) || (ended.has(group) && members(group, rows).length > 0));
  const left = groups.filter((group) => !confirmed.includes(group) && members(group, rows).length > 0);
  let going = confirmed;
  for (const name of ["SIGTERM", "SIGKILL"] as const) {
    for (const group of going) send(group, name);
    const until = Date.now() + live().grace;
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

function release(path: string, text: string): boolean {
  let now: string;
  try {
    now = readFileSync(path, "utf8");
  } catch {
    return true;
  }
  if (now !== text) return false;
  unmark(path);
  return true;
}

function foreign(entry: Entry): boolean {
  try {
    return readFileSync(entry.path, "utf8") !== entry.text;
  } catch {
    return false;
  }
}

function leftForTheMark(group: Id, entry: Entry): void {
  process.stderr.write(`process-group: left group ${group.pid} (started ${group.started}): the mark ${basename(entry.path)} is not the text that this run wrote for it\n`);
}

function stopAll(state: Live): void {
  const groups = [...state.groups.keys()];
  const left = stopNow(groups, state.confirmed);
  for (const group of groups) {
    const entry = state.groups.get(group) as Entry;
    if (!left.includes(group)) release(entry.path, entry.text);
  }
  state.groups.clear();
  state.confirmed.clear();
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
 * (ps lstart read with TZ=UTC0 and LC_ALL=C). It throws before the spawn when the process table has no row for its
 * own process. Right after the spawn it writes the mark group-<pid>.json into marksDir() with the pid, the second
 * before the spawn, the pid and start time of the process that started it (the owner) and the real path of the
 * working directory (the checkout); after its read of ps it adds the leader's start time; it removes the mark once
 * the group has no member left. If that read fails or the child does not lead a group of its own, it reads ps again,
 * zombies included, and sends KILL to the group id when that read shows the child's pid with a start time, or when ps
 * cannot be read: the one signal of the kit with no start time, sent while startGroup still runs, before node can reap
 * the child, so that no other process can have its pid. Then it removes the mark and throws; its errors name the
 * command by its file name, and a command that does not start throws that error alone, with node's own error event
 * of the child caught. No error that it throws, its own or node's, names a directory of the checkout: the mark and
 * the command are named by their file names, and node's code of the error is kept. Until endGroup ends it, the owner ends the group when it gets TERM, INT or
 * HUP (and then dies by that signal), when its IPC channel to vitest's main process closes (and then exits with 1),
 * and when it exits: TERM to a group whose leader has the recorded pid and start time, and KILL after the grace to
 * the members that are left, also when the leader itself left on TERM, that TERM or the TERM of an endGroup that had
 * read the leader and was still waiting. When vitest stops a fork it sends KILL 500 ms
 * after its TERM, so a group that outlives TERM by more than that keeps its mark, as does a group that ends on TERM
 * when the kit's own read of ps before its TERM takes longer than those 500 ms. A KILL of the owner runs none of
 * these: the group and its mark stay until sweepMarks.
 */
export function startGroup(command: string, args: string[], options: Options): Group {
  try {
    return startOwn(command, args, options);
  } catch (error) {
    throw plain(error, command);
  }
}

function plain(error: unknown, command: string): Error {
  const found = error as { message?: unknown; code?: unknown } | null;
  let message = typeof found?.message === "string" ? found.message : String(error);
  const dirs = [marksDir(), dirname(command), process.cwd()];
  try {
    dirs.push(realpathSync(process.cwd()));
  } catch {}
  for (const dir of dirs.filter((each) => each.startsWith("/")).sort((a, b) => b.length - a.length)) {
    message = message.split(`${dir}/`).join("").split(dir).join(basename(dir));
  }
  const made = new Error(message) as Error & { code?: unknown };
  if (found?.code !== undefined) made.code = found.code;
  return made;
}

function startOwn(command: string, args: string[], options: Options): Group {
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
  const checkout = realpathSync(process.cwd());
  const spawned = Math.floor(Date.now() / 1000);
  const child = state.processes.spawn(command, args, options);
  const name = basename(command);
  if (child.pid === undefined) {
    child.on("error", () => {});
    throw new Error(`${name} did not start`);
  }
  const mark = join(dir, `group-${child.pid}.json`);
  const record: Mark = { pid: child.pid, started: null, spawned, owner, checkout };
  try {
    writeFileSync(mark, JSON.stringify(record));
    const own = table().find((row) => row.pid === child.pid);
    if (own === undefined || own.pgid !== child.pid) throw new Error(`${name} (pid ${child.pid}) does not lead its own process group`);
    const group = { pid: child.pid, started: own.started, child };
    const text = JSON.stringify({ ...record, started: own.started });
    writeFileSync(mark, text);
    state.groups.set(group, { path: mark, text });
    return group;
  } catch (error) {
    let rows: Row[] | null = null;
    try {
      rows = table(true);
    } catch {}
    const leader = rows?.find((row) => row.pid === child.pid);
    if (rows === null || leader !== undefined) send({ pid: child.pid, started: leader?.started ?? "" }, "SIGKILL");
    unmark(mark);
    throw error;
  }
}

/**
 * Ends the group while its leader has the recorded pid and start time: TERM, and KILL to the members that are left
 * after 5 s (also when the leader itself left on TERM); throws if a member is left 5 s after KILL. The group stays
 * known to the owner's handlers until endGroup has ended it, so that a stop or a failed read of ps while endGroup
 * runs still leaves the group to the handlers, which send KILL after the grace to the members that are left also when
 * the leader has left on endGroup's own TERM; a second endGroup of the same group waits for the first. endGroup and
 * the handlers remove a mark only while it reads as the text that startGroup wrote for the group; where a mark of
 * another text stands at the group's path, endGroup sends no signal, forgets the group and prints one line, and
 * where no mark stands there it ends the group as above. A group whose
 * leader is gone when it is called gets no signal: endGroup prints one line, keeps its mark for the sweep and does not
 * throw, since nothing tells those processes from another program's group that took the id.
 */
export async function endGroup(group: Group | null): Promise<void> {
  if (group === null) return;
  const state = live();
  const running = state.ending.get(group);
  if (running !== undefined) return running;
  const ending = endKnown(state, group).finally(() => state.ending.delete(group));
  state.ending.set(group, ending);
  return ending;
}

async function endKnown(state: Live, group: Group): Promise<void> {
  const entry = state.groups.get(group);
  const rows = table();
  if (!leads(group, rows)) {
    state.groups.delete(group);
    const count = members(group, rows).length;
    if (count === 0) {
      if (entry !== undefined && !release(entry.path, entry.text)) leftForTheMark(group, entry);
    } else {
      process.stderr.write(`process-group: left group ${group.pid} (started ${group.started}): its leader was gone when endGroup was called and ${processes(count)} its id as their group\n`);
    }
    return;
  }
  if (entry !== undefined && foreign(entry)) {
    state.groups.delete(group);
    leftForTheMark(group, entry);
    return;
  }
  state.confirmed.add(group);
  try {
    send(group, "SIGTERM");
    if (!(await gone(group, state.grace))) {
      send(group, "SIGKILL");
      await gone(group, state.grace);
    }
    if (members(group).length > 0) {
      throw new Error(`process group ${group.pid} (started ${group.started}) still has members ${members(group).join(" ")}`);
    }
  } finally {
    state.confirmed.delete(group);
  }
  state.groups.delete(group);
  if (entry !== undefined) release(entry.path, entry.text);
}

async function gone(group: Id, ms: number): Promise<boolean> {
  const until = Date.now() + ms;
  while (members(group).length > 0) {
    if (Date.now() >= until) return false;
    await pause(100);
  }
  return true;
}

function readMark(text: string): Mark | null {
  try {
    const value: unknown = JSON.parse(text);
    const pid = (each: unknown): boolean => Number.isSafeInteger(each) && (each as number) > 1;
    const id = (each: unknown): each is Id => typeof each === "object" && each !== null && pid((each as Id).pid) && typeof (each as Id).started === "string";
    if (typeof value !== "object" || value === null) return null;
    const { pid: marked, started, spawned, owner, checkout } = value as Record<string, unknown>;
    if (!pid(marked) || !id(owner) || typeof checkout !== "string" || !(typeof started === "string" || (started === null && Number.isSafeInteger(spawned)))) return null;
    return { pid: marked as number, started: started as string | null, spawned: Number(spawned), owner: { pid: owner.pid, started: owner.started }, checkout };
  } catch {
    return null;
  }
}

/**
 * Ends the groups that the marks in marksDir() name and whose owner is gone: what a run of this checkout left when
 * its process got KILL. It leaves alone, with a line: a mark of another checkout; a mark whose owner (pid and start
 * time) is alive, with no line, since a run that is still going owns it; the group of the sweeping process; a group
 * whose leader is gone while processes still have its pid as their group, since nothing tells those processes from a
 * later group of the same id; a group whose leader started before the mark's owner; and, for a mark written before
 * its start time was read, a group whose leader started outside the second before to two seconds after the spawn.
 * Any other group whose leader has the marked pid and start time gets TERM, and KILL after the grace, and its mark
 * goes once the group has no member, as does the mark of a group that is gone; it removes a mark only when the file
 * still reads as it did. Returns one line for each group that it ended or left, and for each mark that it could not
 * read.
 */
export function sweepMarks(): string[] {
  const dir = marksDir();
  let names: string[];
  try {
    names = readdirSync(dir);
  } catch {
    return [];
  }
  const here = realpathSync(process.cwd());
  const lines: string[] = [];
  for (const name of names.filter((each) => /^group-\d+\.json$/.test(each)).sort()) {
    const path = join(dir, name);
    let text: string;
    try {
      text = readFileSync(path, "utf8");
    } catch {
      continue;
    }
    const mark = readMark(text);
    if (mark === null) {
      lines.push(`process-group: skipped the mark ${name}, which is no group with an owner`);
      continue;
    }
    if (mark.checkout !== here) {
      lines.push(`process-group: left the mark ${name}, which a run of another checkout wrote`);
      continue;
    }
    const rows = table();
    if (rows.some((row) => row.pid === mark.owner.pid && row.started === mark.owner.started)) continue;
    const self = rows.find((row) => row.pid === process.pid);
    const leader = rows.find((row) => row.pid === mark.pid && row.pgid === mark.pid);
    const at = leader === undefined ? null : epoch(leader.started);
    const inside = at !== null && at >= mark.spawned - 1 && at <= mark.spawned + 2;
    if (mark.started === null && leader !== undefined && !inside) {
      lines.push(`process-group: left group ${mark.pid} (started ${leader.started}): its leader started outside the seconds of the spawn that its mark records`);
      continue;
    }
    const started = mark.started ?? (leader !== undefined ? leader.started : "no start time");
    const shown = mark.started === null && leader === undefined ? "" : ` (started ${started})`;
    const group = { pid: mark.pid, started };
    const count = members(group, rows).length;
    const born = epoch(started);
    const ownerBorn = epoch(mark.owner.started);
    if (count === 0) {
      release(path, text);
    } else if (self !== undefined && mark.pid === self.pgid) {
      lines.push(`process-group: left group ${mark.pid}: it is the group of the sweeping process`);
    } else if (!leads(group, rows)) {
      lines.push(`process-group: left group ${mark.pid}${shown}: its leader is gone and ${processes(count)} its id as their group`);
    } else if (born === null || ownerBorn === null || born < ownerBorn) {
      lines.push(`process-group: left group ${mark.pid} (started ${started}): its leader started before the run that marked it`);
    } else if (stopNow([group]).length === 0) {
      release(path, text);
      lines.push(`process-group: ended group ${mark.pid} (started ${started}), which a run of this checkout left: ${many(count)}`);
    } else {
      lines.push(`process-group: group ${mark.pid} (started ${started}) still has members after KILL`);
    }
  }
  return lines;
}
