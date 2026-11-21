import * as fs from "node:fs";
import { mkdirSync, mkdtempSync, readdirSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { afterAll, beforeEach, expect, test } from "vitest";
import { endGroup, groupMembers, type Processes, startGroup, useProcesses } from "./process-group.ts";

const { chmodSync, readFileSync, unlinkSync } = fs as unknown as {
  chmodSync(path: string, mode: number): void;
  readFileSync(path: string, encoding: "utf8"): string;
  unlinkSync(path: string): void;
};

interface FRow {
  pid: number;
  pgid: number;
  stat: string;
  started: string;
  termProof?: boolean;
  killProof?: boolean;
}

const ROOT = process.cwd();
const OWNER = "Thu Jan 1 00:00:10 1970";
const LATER = "Thu Jan 1 01:00:00 1970";
const OTHER = "Thu Jan 1 02:00:00 1970";
const HERE = realpathSync(ROOT);

let rows: FRow[] = [];
let events: string[] = [];
let failReads = 0;
let leadsNot = false;
let goneAtRead = false;
let nextPid = 200_000;
let marks = "";
let handlers = new Map<string, (() => void)[]>();
let onPs: (() => void) | null = null;
let spawnStarted = LATER;
const dirs: string[] = [];

const world: Processes = {
  ps: () => {
    const hook = onPs;
    onPs = null;
    if (hook !== null) hook();
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
    rows.push({ pid: nextPid, pgid: leadsNot ? 1 : nextPid, stat: "S", started: spawnStarted });
    events.push("spawn");
    return { pid: nextPid } as unknown as ReturnType<Processes["spawn"]>;
  },
  kill: (pid, signal) => {
    events.push(`${signal} ${pid}`);
    if (pid < 0 && !rows.some((row) => row.pgid === -pid)) throw new Error("ESRCH");
    rows = rows.filter((row) => row.pgid !== -pid || (signal === "SIGTERM" && row.termProof === true) || row.killProof === true);
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

function fresh(): void {
  rows = [{ pid: process.pid, pgid: process.pid, stat: "S", started: OWNER }];
  events = [];
  failReads = 0;
  leadsNot = false;
  goneAtRead = false;
  handlers = new Map();
  onPs = null;
  mkdirSync(join(ROOT, ".tmp"), { recursive: true });
  const made = mkdtempSync(join(ROOT, ".tmp", "process-group-start."));
  dirs.push(made);
  marks = join(made, "marks");
  process.env.HALVO_GROUP_MARKS = marks;
  spawnStarted = LATER;
  useProcesses(world, 300);
}

beforeEach(fresh);

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
  rows.push({ pid: 300_900, pgid: 300_900, stat: "S", started: LATER });
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
  world.spawn = () => ({ pid: undefined, on: () => {} }) as unknown as ReturnType<Processes["spawn"]>;
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

function laterMark(pid: number): void {
  rows = rows.filter((row) => row.pgid !== pid);
  rows.push({ pid, pgid: pid, stat: "S", started: OTHER });
  writeFileSync(join(marks, `group-${pid}.json`), JSON.stringify({ pid, started: OTHER, spawned: 7200, owner: { pid: 299_999, started: OTHER }, checkout: HERE }));
}

test("endGroup and stopAll remove a mark only while it reads as they wrote it", async () => {
  const opts = { stdio: ["ignore", "ignore", "ignore"] as "ignore"[] };
  const markOf = (pid: number) => join(marks, `group-${pid}.json`);
  const textOf = (path: string) => {
    try {
      return readFileSync(path, "utf8");
    } catch {
      return null;
    }
  };
  const kept = (path: string, wrote: string | null) => (textOf(path) === wrote ? "kept byte for byte" : textOf(path) === null ? "removed" : "changed");
  const cases: Record<string, unknown> = {};

  const a = startGroup("server", [], opts);
  laterMark(a.pid);
  await endGroup(a);
  const afterEndGroup = marksIn();
  const b = startGroup("server", [], opts);
  laterMark(b.pid);
  fire("exit");
  cases["a later mark of another owner"] = { afterEndGroup, afterExit: marksIn() };

  fresh();
  const clock = Date.now;
  Date.now = () => 3_599_999;
  const first = startGroup("server", [], opts);
  rows = rows.filter((row) => row.pgid !== first.pid);
  nextPid = first.pid - 1;
  Date.now = () => 3_600_050;
  const second = startGroup("server", [], opts);
  Date.now = clock;
  const secondText = textOf(markOf(second.pid));
  events = [];
  await endGroup(first);
  cases["the same owner, the same pid and start second, another spawn second"] = {
    samePidAndStart: second.pid === first.pid && second.started === first.started,
    signalsOfTheFirstEnding: groupSignals(first.pid),
    laterMark: kept(markOf(second.pid), secondText),
  };

  for (const by of ["endGroup", "exit"]) {
    fresh();
    const c = startGroup("server", [], opts);
    const other = JSON.parse(textOf(markOf(c.pid)) as string);
    other.checkout = `${other.checkout}/another`;
    const otherText = JSON.stringify(other);
    writeFileSync(markOf(c.pid), otherText);
    rows = rows.filter((row) => row.pgid !== c.pid);
    events = [];
    if (by === "endGroup") await endGroup(c);
    else fire("exit");
    cases[`a mark of another checkout at the path, then ${by}`] = { signals: groupSignals(c.pid), mark: kept(markOf(c.pid), otherText) };
  }

  fresh();
  const d = startGroup("server", [], opts);
  unlinkSync(markOf(d.pid));
  events = [];
  await endGroup(d);
  cases["no mark at the path, then endGroup"] = { term: groupSignals(d.pid).includes(`SIGTERM -${d.pid}`) };

  fresh();
  const e = startGroup("server", [], opts);
  rows = rows.filter((row) => row.pgid !== e.pid);
  nextPid = e.pid - 1;
  spawnStarted = OTHER;
  const f = startGroup("server", [], opts);
  const laterText = textOf(markOf(f.pid));
  await endGroup(e);
  const afterEndA = kept(markOf(f.pid), laterText);
  events = [];
  fire("exit");
  cases["the same owner's later group of the pid with another start time"] = {
    samePid: f.pid === e.pid,
    afterEndA,
    termToTheLaterGroupAtExit: groupSignals(f.pid).includes(`SIGTERM -${f.pid}`),
    afterExit: kept(markOf(f.pid), laterText),
  };

  expect(cases).toEqual({
    "a later mark of another owner": { afterEndGroup: [`group-${a.pid}.json`], afterExit: [`group-${a.pid}.json`, `group-${b.pid}.json`] },
    "the same owner, the same pid and start second, another spawn second": { samePidAndStart: true, signalsOfTheFirstEnding: [], laterMark: "kept byte for byte" },
    "a mark of another checkout at the path, then endGroup": { signals: [], mark: "kept byte for byte" },
    "a mark of another checkout at the path, then exit": { signals: [], mark: "kept byte for byte" },
    "no mark at the path, then endGroup": { term: true },
    "the same owner's later group of the pid with another start time": { samePid: true, afterEndA: "kept byte for byte", termToTheLaterGroupAtExit: true, afterExit: "removed" },
  });
});

test("a failed second write of startGroup leaves no group in the kit's list, so a later mark of the same pid stays at the owner's exit", () => {
  startGroup("server", [], { stdio: ["ignore", "ignore", "ignore"] });
  onPs = () => chmodSync(join(marks, `group-${nextPid}.json`), 0o444);
  expect(() => startGroup("server", [], { stdio: ["ignore", "ignore", "ignore"] })).toThrow("EACCES");
  const failed = nextPid;
  laterMark(failed);
  fire("exit");
  expect(marksIn()).toEqual([`group-${failed}.json`]);
});

test('startGroup of a missing command throws "<name> did not start" and leaves no uncaught error', async () => {
  useProcesses(null);
  const missing = join(marks, "..", "no-such-binary");
  const uncaught: string[] = [];
  const listener = (error: unknown) => {
    uncaught.push(String(error).replace(ROOT, "<backend>"));
  };
  process.on("uncaughtException", listener);
  try {
    expect(() => startGroup(missing, [], { stdio: ["ignore", "ignore", "ignore"] })).toThrow(/^no-such-binary did not start$/);
    await pause(200);
  } finally {
    process.removeListener("uncaughtException", listener);
  }
  expect(uncaught).toEqual([]);
});

test("endGroup: a stop inside an ending whose TERM took the leader sends KILL to the members that are left, and the mark goes", async () => {
  const forms: Record<string, unknown> = {};
  for (const event of ["SIGINT", "SIGTERM", "exit"]) {
    const a = startGroup("server", [], { stdio: ["ignore", "ignore", "ignore"] });
    nextPid += 1;
    const member = nextPid;
    rows.push({ pid: member, pgid: a.pid, stat: "S", started: LATER, termProof: true });
    events = [];
    const ending = endGroup(a);
    const leaderLeftOnTheEndingsTerm = !rows.some((row) => row.pid === a.pid);
    fire(event);
    forms[event] = {
      leaderLeftOnTheEndingsTerm,
      killToTheGroup: groupSignals(a.pid).includes(`SIGKILL -${a.pid}`),
      memberLeft: rows.some((row) => row.pid === member),
      marks: marksIn(),
    };
    await ending.catch(() => {});
  }
  const done = { leaderLeftOnTheEndingsTerm: true, killToTheGroup: true, memberLeft: false, marks: [] };
  expect(forms).toEqual({ SIGINT: done, SIGTERM: done, exit: done });
}, 30_000);

test("startGroup's errors of a failed start name no directory of the checkout and keep node's code", () => {
  const checkout = [ROOT, HERE];
  const caught = (run: () => unknown) => {
    try {
      run();
    } catch (error) {
      const message = String((error as Error).message);
      return { code: (error as { code?: string }).code, pathOfTheCheckout: checkout.some((path) => message.includes(path)) };
    }
    return { code: "no error", pathOfTheCheckout: false };
  };
  const cases: Record<string, unknown> = {};
  mkdirSync(join(marks, `group-${nextPid + 1}.json`), { recursive: true });
  cases["the mark's path is a directory"] = caught(() => startGroup("server", [], { stdio: ["ignore", "ignore", "ignore"] }));
  useProcesses(null);
  cases["the command holds a NUL byte"] = caught(() => startGroup(join(ROOT, "no\0command"), [], { stdio: ["ignore", "ignore", "ignore"] }));
  expect(cases).toEqual({
    "the mark's path is a directory": { code: "EISDIR", pathOfTheCheckout: false },
    "the command holds a NUL byte": { code: "ERR_INVALID_ARG_VALUE", pathOfTheCheckout: false },
  });
});

test("after an ending that rejects, the owner's handlers send no signal to the group's id unless its leader leads, and leave the group and its mark", async () => {
  const cases: Record<string, unknown> = {};
  const want: Record<string, unknown> = {};
  for (const by of ["a read of ps fails inside the wait", "a member is left after KILL"]) {
    fresh();
    const a = startGroup("server", [], { stdio: ["ignore", "ignore", "ignore"] });
    nextPid += 1;
    const member = nextPid;
    const afterKill = by === "a member is left after KILL";
    rows.push({ pid: member, pgid: a.pid, stat: afterKill ? "D" : "S", started: LATER, termProof: true, killProof: afterKill });
    const ending = endGroup(a);
    if (!afterKill) failReads = 1;
    const thrown = await ending.then(
      () => "no error",
      (error: Error) => error.message,
    );
    rows = rows.filter((row) => row.pgid !== a.pid);
    nextPid += 1;
    rows.push({ pid: nextPid, pgid: a.pid, stat: "S", started: OTHER });
    events = [];
    fire("exit");
    cases[by] = { thrown, signalsAtExit: groupSignals(a.pid), markLeft: marksIn().includes(`group-${a.pid}.json`) };
    want[by] = {
      thrown: afterKill ? `process group ${a.pid} (started ${LATER}) still has members ${member}` : "ps was stopped",
      signalsAtExit: [],
      markLeft: true,
    };
  }
  expect(cases).toEqual(want);
}, 20_000);
