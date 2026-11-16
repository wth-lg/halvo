import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, openSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { join } from "node:path";
import { expect, test } from "vitest";
import { endGroup, type Group, startGroup } from "./process-group.ts";

const ROOT = process.cwd();
const SELF = "test/ledger/stopped-run.node.test.ts";
const MODE: string | undefined = process.env.HALVO_STOPPED_RUN_MODE;
const HERE: string | undefined = process.env.HALVO_STOPPED_RUN_DIR;
const SERVER = ["esbuild", "workerd", "workerd", "wrangler", "wrangler's node child"];
const SETTLE_MS = 20_000;
const pause = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

interface Row {
  pid: number;
  ppid: number;
  pgid: number;
  started: string;
  command: string;
}

interface Id {
  pid: number;
  started: string;
}

interface Ready {
  fork: Id;
  group: Id;
}

function table(): Row[] {
  const text = execFileSync("ps", ["-A", "-o", "pid=,ppid=,pgid=,stat=,lstart=,command="], { encoding: "utf8" });
  const rows: Row[] = [];
  for (const line of text.split("\n")) {
    const match = /^\s*(\d+)\s+(\d+)\s+(\d+)\s+(\S+)\s+(\S+\s+\S+\s+\S+\s+\S+\s+\S+)\s+(.*)$/.exec(line);
    if (match === null || (match[4] as string).startsWith("Z")) continue;
    rows.push({
      pid: Number(match[1]),
      ppid: Number(match[2]),
      pgid: Number(match[3]),
      started: (match[5] as string).replace(/\s+/g, " "),
      command: match[6] as string,
    });
  }
  return rows;
}

function startedOf(pid: number): string {
  const row = table().find((each) => each.pid === pid);
  if (row === undefined) throw new Error(`process ${pid} is not in the process table`);
  return row.started;
}

function groupOf(id: Id, rows = table()): Row[] {
  const leader = rows.find((row) => row.pid === id.pid);
  if (leader !== undefined && leader.started !== id.started) return [];
  return rows.filter((row) => row.pgid === id.pid);
}

function kind(command: string): string {
  const first = command.split(" ")[0] as string;
  const base = first.slice(first.lastIndexOf("/") + 1);
  if (base === "esbuild" || base === "workerd") return base;
  if (base === "node" && command.includes("/wrangler-dist/cli.js")) return "wrangler's node child";
  if (base === "node" && command.includes("/wrangler/bin/wrangler.js")) return "wrangler";
  return `other (${base})`;
}

const described = (rows: Row[]) => rows.map((row) => `${kind(row.command)} pid ${row.pid} group ${row.pgid}`);

function descendants(pid: number, rows: Row[]): number[] {
  const found = new Set([pid]);
  for (let grew = true; grew; ) {
    grew = false;
    for (const row of rows) {
      if (found.has(row.ppid) && !found.has(row.pid)) {
        found.add(row.pid);
        grew = true;
      }
    }
  }
  return [...found];
}

function devDirs(dir: string): string[] {
  const tmp = join(dir, "project", ".wrangler", "tmp");
  return existsSync(tmp) ? readdirSync(tmp) : [];
}

async function settle(ids: Id[]): Promise<Row[]> {
  const until = Date.now() + SETTLE_MS;
  for (;;) {
    const rows = table();
    const left = ids.flatMap((id) => groupOf(id, rows));
    if (left.length === 0 || Date.now() >= until) return left;
    await pause(100);
  }
}

async function endRecorded(id: Id): Promise<void> {
  for (const name of ["SIGTERM", "SIGKILL"]) {
    if (groupOf(id).length === 0) return;
    try {
      process.kill(-id.pid, name);
    } catch {
      return;
    }
    if ((await settle([id])).length === 0) return;
  }
}

function startRun(dir: string, mode: string, file: string, env: Record<string, string> = {}): Group {
  const log = openSync(join(dir, `${mode}-run.log`), "a");
  return startGroup(join(ROOT, "node_modules/.bin/vitest"), ["run", "--project", "node", file], {
    stdio: ["ignore", log, log],
    env: { ...process.env, HALVO_STOPPED_RUN_MODE: mode, HALVO_STOPPED_RUN_DIR: dir, ...env },
  });
}

function logOf(dir: string, mode: string): string {
  const text = readFileSync(join(dir, `${mode}-run.log`), "utf8");
  return text.slice(-3000);
}

async function waitReady(dir: string, mode: string, run: Group): Promise<Ready> {
  const file = join(dir, "ready.json");
  for (let tries = 0; tries < 900; tries += 1) {
    if (existsSync(file)) {
      try {
        return JSON.parse(readFileSync(file, "utf8"));
      } catch {}
    }
    if (groupOf(run).length === 0) throw new Error(`the ${mode} run ended before it was ready:\n${logOf(dir, mode)}`);
    await pause(100);
  }
  throw new Error(`the ${mode} run was not ready after 90 s:\n${logOf(dir, mode)}`);
}

function freePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const server = createServer();
    server.on("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address();
      server.close(() => resolve(port));
    });
  });
}

async function holdWrangler(dir: string): Promise<never> {
  const project = join(dir, "project");
  mkdirSync(project, { recursive: true });
  const config = JSON.parse(readFileSync(join(ROOT, "wrangler.test.jsonc"), "utf8"));
  delete config.$schema;
  config.main = join(ROOT, config.main);
  writeFileSync(join(project, "wrangler.jsonc"), JSON.stringify(config));
  const port = await freePort();
  const inspector = await freePort();
  const log = openSync(join(dir, "wrangler.log"), "a");
  const group = startGroup(
    join(ROOT, "node_modules/.bin/wrangler"),
    [
      "dev",
      "--config",
      join(project, "wrangler.jsonc"),
      "--ip",
      "127.0.0.1",
      "--port",
      String(port),
      "--inspector-port",
      String(inspector),
      "--persist-to",
      join(dir, "state"),
      "--show-interactive-dev-session=false",
    ],
    { stdio: ["ignore", log, log], env: { ...process.env, XDG_CONFIG_HOME: process.env.HALVO_WRANGLER_CONFIG_HOME } },
  );
  for (let tries = 0; ; tries += 1) {
    const ready = await fetch(`http://127.0.0.1:${port}/test/inspect`, {
      method: "POST",
      body: JSON.stringify({ account: "00000000-0000-4000-8000-000000000000" }),
    }).then(
      (response) => response.status === 200,
      () => false,
    );
    if (ready) break;
    if (tries > 600) throw new Error("wrangler dev did not answer within 60 s");
    await pause(100);
  }
  const ready: Ready = { fork: { pid: process.pid, started: startedOf(process.pid) }, group: { pid: group.pid, started: group.started } };
  writeFileSync(join(dir, "ready.json"), JSON.stringify(ready));
  return new Promise<never>(() => {});
}

function holdSleeper(dir: string): Promise<never> {
  const group = startGroup(process.execPath, ["-e", "setInterval(() => {}, 1 << 30)"], { stdio: ["ignore", "ignore", "ignore"] });
  const ready: Ready = { fork: { pid: process.pid, started: startedOf(process.pid) }, group: { pid: group.pid, started: group.started } };
  writeFileSync(join(dir, "ready.json"), JSON.stringify(ready));
  return new Promise<never>(() => {});
}

function marked(dir: string): Id[] {
  if (!existsSync(dir)) return [];
  return readdirSync(dir)
    .filter((name) => /^group-\d+\.json$/.test(name))
    .flatMap((name) => {
      try {
        const mark = JSON.parse(readFileSync(join(dir, name), "utf8"));
        return [{ pid: mark.pid, started: mark.started }];
      } catch {
        return [];
      }
    });
}

function exitOf(run: Group): Promise<number | null> {
  return new Promise((resolve) => run.child.on("exit", (code) => resolve(code)));
}

if (MODE !== undefined) {
  if (HERE === undefined) throw new Error("HALVO_STOPPED_RUN_MODE is set and HALVO_STOPPED_RUN_DIR is not");
  if (MODE === "wrangler") test("holds wrangler dev up until the run is stopped", () => holdWrangler(HERE), 300_000);
  else if (MODE === "sleeper") test("holds a sleeping node up until the run is stopped", () => holdSleeper(HERE), 300_000);
  else if (MODE === "sweep") test("starts nothing, after the run's set-up", () => {});
  else throw new Error(`no mode ${MODE}`);
} else {
  test("after KILL to a whole run, the next run of the same checkout ends the server that the run left, by the mark that it wrote, and wrangler removes its build; a server that a run of another checkout marked stays", async () => {
    mkdirSync(join(ROOT, ".tmp"), { recursive: true });
    const dir = mkdtempSync(join(ROOT, ".tmp", "stopped-run."));
    const other = mkdtempSync(join(ROOT, ".tmp", "stopped-run."));
    const killed = startRun(dir, "wrangler", SELF);
    const elsewhere = startRun(other, "sleeper", SELF, { HALVO_GROUP_MARKS: join(other, "marks") });
    let server: Id | null = null;
    let sleeper: Id | null = null;
    let next: Group | null = null;
    try {
      server = (await waitReady(dir, "wrangler", killed)).group;
      sleeper = (await waitReady(other, "sleeper", elsewhere)).group;
      process.kill(-killed.pid, "SIGKILL");
      process.kill(-elsewhere.pid, "SIGKILL");
      expect(described(await settle([killed, elsewhere]))).toEqual([]);
      expect(groupOf(server).map((row) => kind(row.command)).sort()).toEqual(SERVER);
      expect(groupOf(sleeper)).toHaveLength(1);
      expect(devDirs(dir).length).toBeGreaterThan(0);
      next = startRun(dir, "sweep", SELF);
      const code = await Promise.race([exitOf(next), pause(90_000).then(() => "still running")]);
      expect(code, logOf(dir, "sweep")).toBe(0);
      expect(described(await settle([server]))).toEqual([]);
      expect(devDirs(dir)).toEqual([]);
      expect(logOf(dir, "sweep")).toContain(`process-group: ended group ${server.pid} (started ${server.started})`);
      expect(groupOf(sleeper)).toHaveLength(1);
    } finally {
      for (const id of [server, sleeper]) if (id !== null) await endRecorded(id);
      for (const run of [killed, elsewhere, next]) await endGroup(run);
    }
  }, 180_000);

  test("test/contract/wrangler.node.test.ts starts wrangler dev in a group that its run marks, and a run of it whose main process alone gets SIGHUP while that server starts leaves no process of the run's group or of that group", async () => {
    mkdirSync(join(ROOT, ".tmp"), { recursive: true });
    const dir = mkdtempSync(join(ROOT, ".tmp", "stopped-run."));
    const marks = join(dir, "marks");
    const run = startRun(dir, "contract", "test/contract/wrangler.node.test.ts", { HALVO_GROUP_MARKS: marks });
    const watched: Id[] = [run];
    try {
      for (let tries = 0; ; tries += 1) {
        const groups = [run, ...marked(marks)];
        const rows = table();
        const holder = groups.find((id) => groupOf(id, rows).some((row) => kind(row.command) === "wrangler"));
        if (holder !== undefined) {
          watched.push(...groups.slice(1));
          const where = holder === run ? "in the run's own process group, which no mark names" : "in a group that the run marked";
          expect(`wrangler dev runs ${where}`).toBe("wrangler dev runs in a group that the run marked");
          break;
        }
        if (groupOf(run, rows).length === 0) throw new Error(`the run ended before its wrangler dev started:\n${logOf(dir, "contract")}`);
        if (tries > 1200) throw new Error(`no wrangler dev in the run after 60 s:\n${logOf(dir, "contract")}`);
        await pause(50);
      }
      process.kill(run.pid, "SIGHUP");
      expect(described(await settle(watched))).toEqual([]);
    } finally {
      for (const id of watched.slice(1)) await endRecorded(id);
      await endGroup(run);
    }
  }, 120_000);

  const stops: [string, "group" | "main"][] = [
    ["SIGTERM", "group"],
    ["SIGINT", "group"],
    ["SIGHUP", "group"],
    ["SIGTERM", "main"],
    ["SIGHUP", "main"],
  ];
  for (const [signal, to] of stops) {
    const whom = to === "group" ? "to the run's process group" : "to the run's main process alone";
    test(`a run stopped half-way by ${signal} ${whom} leaves none of wrangler, its node child, esbuild and workerd, and wrangler removes its build under .wrangler/tmp`, async () => {
      mkdirSync(join(ROOT, ".tmp"), { recursive: true });
      const dir = mkdtempSync(join(ROOT, ".tmp", "stopped-run."));
      const run = startRun(dir, "wrangler", SELF);
      let server: Id | null = null;
      try {
        const ready = await waitReady(dir, "wrangler", run);
        server = ready.group;
        const rows = table();
        const members = groupOf(server, rows);
        expect(members.map((row) => kind(row.command)).sort()).toEqual(SERVER);
        expect(descendants(server.pid, rows).filter((pid) => !members.some((row) => row.pid === pid))).toEqual([]);
        expect(devDirs(dir).length).toBeGreaterThan(0);
        expect(groupOf(run, rows).some((row) => row.pid === ready.fork.pid)).toBe(true);
        process.kill(to === "group" ? -run.pid : run.pid, signal);
        const left = await settle([server, run]);
        expect(described(left)).toEqual([]);
        expect(devDirs(dir)).toEqual([]);
      } finally {
        if (server !== null) await endRecorded(server);
        await endGroup(run);
      }
    }, 120_000);
  }
}
