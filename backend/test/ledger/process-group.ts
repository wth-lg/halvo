import { type Child, execFileSync, spawn } from "node:child_process";

declare module "node:child_process" {
  export function spawn(
    command: string,
    args: string[],
    options: { env?: Record<string, string | undefined>; stdio: ("ignore" | "pipe" | "inherit" | number)[]; detached: true },
  ): Child;
}

type Options = { env?: Record<string, string | undefined>; stdio: ("ignore" | "pipe" | "inherit" | number)[] };

export interface Group {
  pid: number;
  started: string;
  child: Child;
}

interface Row {
  pid: number;
  pgid: number;
  started: string;
}

interface Live {
  groups: Set<Group>;
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

function members(group: Group): number[] {
  const rows = table().filter((row) => row.pgid === group.pid);
  const leader = rows.find((row) => row.pid === group.pid);
  if (leader !== undefined && leader.started !== group.started) return [];
  return rows.map((row) => row.pid);
}

function signal(group: Group, name: "SIGTERM" | "SIGKILL"): boolean {
  if (members(group).length === 0) return false;
  try {
    process.kill(-group.pid, name);
    return true;
  } catch {
    return false;
  }
}

async function gone(group: Group, ms: number): Promise<boolean> {
  const until = Date.now() + ms;
  while (members(group).length > 0) {
    if (Date.now() >= until) return false;
    await pause(100);
  }
  return true;
}

function stopNow(group: Group): void {
  for (const name of ["SIGTERM", "SIGKILL"] as const) {
    if (!signal(group, name)) return;
    const until = Date.now() + GRACE_MS;
    while (members(group).length > 0 && Date.now() < until) sleep(100);
    if (members(group).length === 0) return;
  }
}

function live(): Live {
  const shared = globalThis as unknown as Record<symbol, Live | undefined>;
  const found = shared[LIVE];
  if (found !== undefined) return found;
  const made: Live = { groups: new Set(), installed: false };
  shared[LIVE] = made;
  return made;
}

function stopAll(state: Live): void {
  for (const group of state.groups) stopNow(group);
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
 * Starts command in a process group of its own and returns the group, known by its leader's pid and start time.
 * Until endGroup ends it, the process that started it ends the group, TERM first and KILL after 5 s, when it gets
 * TERM, INT or HUP (and then dies by that signal), when its IPC channel to vitest's main process closes (and then
 * exits with 1), and when it exits.
 */
export function startGroup(command: string, args: string[], options: Options): Group {
  const state = live();
  install(state);
  const child = spawn(command, args, { ...options, detached: true });
  if (child.pid === undefined) throw new Error(`${command} did not start`);
  const own = table().find((row) => row.pid === child.pid);
  if (own === undefined || own.pgid !== child.pid) throw new Error(`${command} (pid ${child.pid}) does not lead its own process group`);
  const group = { pid: child.pid, started: own.started, child };
  state.groups.add(group);
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
  live().groups.delete(group);
}
