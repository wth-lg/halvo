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

const GRACE_MS = 5_000;
const pause = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

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

const atExit = new Map<Group, () => void>();

export function startGroup(command: string, args: string[], options: Options): Group {
  const child = spawn(command, args, { ...options, detached: true });
  if (child.pid === undefined) throw new Error(`${command} did not start`);
  const own = table().find((row) => row.pid === child.pid);
  if (own === undefined || own.pgid !== child.pid) throw new Error(`${command} (pid ${child.pid}) does not lead its own process group`);
  const group = { pid: child.pid, started: own.started, child };
  const backstop = () => {
    signal(group, "SIGKILL");
  };
  atExit.set(group, backstop);
  process.once("exit", backstop);
  return group;
}

export async function endGroup(group: Group | null): Promise<void> {
  if (group === null) return;
  signal(group, "SIGTERM");
  if (!(await gone(group, GRACE_MS))) {
    signal(group, "SIGKILL");
    if (!(await gone(group, GRACE_MS))) throw new Error(`process group ${group.pid} (started ${group.started}) still has members ${members(group).join(" ")}`);
  }
  const backstop = atExit.get(group);
  if (backstop !== undefined) process.removeListener("exit", backstop);
  atExit.delete(group);
}
