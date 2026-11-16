import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { afterAll, beforeEach, expect, test } from "vitest";
import { endGroup, type Processes, startGroup, sweepMarks, useProcesses } from "./process-group.ts";

interface FRow {
  pid: number;
  pgid: number;
  stat: string;
  started: string;
  termProof?: boolean;
  killProof?: boolean;
}

const ROOT = process.cwd();
const OWNER = "Mon Nov 16 06:00:00 2026";
const LATER = "Mon Nov 16 07:00:00 2026";
const OTHER = "Mon Nov 16 08:00:00 2026";
const LATER_EPOCH = Date.UTC(2026, 10, 16, 7, 0, 0) / 1000;

let rows: FRow[] = [];
let record: string[] = [];
let exits: number[] = [];
let handlers = new Map<string, (() => void)[]>();
let nextPid = 50_000;
let marks = "";
let onPs: (() => void) | null = null;

const world: Processes = {
  ps: () => {
    const hook = onPs;
    onPs = null;
    if (hook !== null) hook();
    return rows.map((row) => `${row.pid} ${row.pgid} ${row.stat} ${row.started}`).join("\n");
  },
  spawn: () => {
    nextPid += 1;
    rows.push({ pid: nextPid, pgid: nextPid, stat: "S", started: LATER });
    return { pid: nextPid } as unknown as ReturnType<Processes["spawn"]>;
  },
  kill: (pid, signal) => {
    const target = Math.abs(pid);
    record.push(`${signal} ${pid} ${rows.find((row) => row.pid === target)?.started ?? "gone"}`);
    if (pid > 0) return;
    if (!rows.some((row) => row.pgid === target)) throw new Error("ESRCH");
    rows = rows.filter(
      (row) => !(row.pgid === target && !row.stat.startsWith("Z") && (signal === "SIGKILL" ? !row.killProof : !row.termProof && !row.killProof)),
    );
  },
  exit: (code) => {
    exits.push(code);
  },
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

const count = (event: string) => (handlers.get(event) ?? []).length;
const marksIn = () => (existsSync(marks) ? readdirSync(marks).sort() : []);
const start = () => startGroup("x", [], { stdio: ["ignore", "ignore", "ignore"] });
const row = (pid: number) => rows.find((each) => each.pid === pid) as FRow;

function group(options: { members?: number; killProof?: boolean; started?: string } = {}): number {
  nextPid += 1;
  const leader = nextPid;
  rows.push({ pid: leader, pgid: leader, stat: "S", started: options.started ?? LATER, killProof: options.killProof });
  for (let i = 1; i < (options.members ?? 1); i += 1) {
    nextPid += 1;
    rows.push({ pid: nextPid, pgid: leader, stat: "S", started: LATER, killProof: options.killProof });
  }
  return leader;
}

function mark(pid: number, fields: Record<string, unknown>, name = `group-${pid}.json`): void {
  mkdirSync(marks, { recursive: true });
  writeFileSync(join(marks, name), JSON.stringify({ pid, ...fields }));
}

const gone = { pid: 777_777, started: OWNER };

beforeEach(() => {
  rows = [
    { pid: 1, pgid: 1, stat: "Ss", started: "Sun Nov 1 00:00:00 2026" },
    { pid: process.pid, pgid: process.pid, stat: "S", started: OWNER },
  ];
  record = [];
  exits = [];
  handlers = new Map();
  onPs = null;
  mkdirSync(join(ROOT, ".tmp"), { recursive: true });
  marks = join(mkdtempSync(join(ROOT, ".tmp", "process-group-rules.")), "marks");
  process.env.HALVO_GROUP_MARKS = marks;
  useProcesses(world);
});

afterAll(() => {
  useProcesses(null);
  delete process.env.HALVO_GROUP_MARKS;
});

test("startGroup marks the group with its owner before it reads ps, adds the start time after, and installs one handler per event", () => {
  const seen: unknown[] = [];
  const spawnOf = world.spawn;
  world.spawn = (command, args, options) => {
    const child = spawnOf(command, args, options);
    onPs = () => seen.push(JSON.parse(readFileSync(join(marks, `group-${child.pid}.json`), "utf8")));
    return child;
  };
  try {
    const a = start();
    start();
    expect(seen).toHaveLength(2);
    expect(seen[0]).toMatchObject({ pid: a.pid, started: null, owner: { pid: process.pid, started: OWNER } });
    expect(JSON.parse(readFileSync(join(marks, `group-${a.pid}.json`), "utf8"))).toMatchObject({
      pid: a.pid,
      started: LATER,
      owner: { pid: process.pid, started: OWNER },
    });
    expect(["SIGTERM", "SIGINT", "SIGHUP", "disconnect", "exit"].map(count)).toEqual([1, 1, 1, 1, 1]);
  } finally {
    world.spawn = spawnOf;
  }
});

test("startGroup sends KILL to the group, removes the mark and throws when its read of ps fails after the spawn", () => {
  const spawnOf = world.spawn;
  let pid = 0;
  let armed = false;
  world.spawn = (command, args, options) => {
    const child = spawnOf(command, args, options);
    pid = child.pid as number;
    if (armed) {
      onPs = () => {
        throw new Error("ps was stopped");
      };
    }
    return child;
  };
  try {
    start();
    armed = true;
    expect(start).toThrow("ps was stopped");
    expect(record).toEqual([`SIGKILL -${pid} ${LATER}`]);
    expect(marksIn()).toEqual([`group-${pid - 1}.json`]);
  } finally {
    world.spawn = spawnOf;
  }
});

test("startGroup sends KILL to the group and throws when the child's leader is a zombie whose children run on", () => {
  const spawnOf = world.spawn;
  world.spawn = (command, args, options) => {
    const child = spawnOf(command, args, options);
    row(child.pid as number).stat = "Z";
    rows.push({ pid: (child.pid as number) + 1000, pgid: child.pid as number, stat: "S", started: LATER });
    return child;
  };
  try {
    expect(start).toThrow("does not lead its own process group");
    expect(rows.filter((each) => each.pgid === nextPid && !each.stat.startsWith("Z"))).toEqual([]);
    expect(marksIn()).toEqual([]);
  } finally {
    world.spawn = spawnOf;
  }
});

for (const name of ["SIGTERM", "SIGINT", "SIGHUP"]) {
  test(`${name}: TERM to every group, the marks go, the listeners go, and the process dies by ${name}`, () => {
    const a = start();
    const b = start();
    fire(name);
    expect(record).toEqual([`SIGTERM -${a.pid} ${LATER}`, `SIGTERM -${b.pid} ${LATER}`, `${name} ${process.pid} ${OWNER}`]);
    expect(marksIn()).toEqual([]);
    expect(["SIGTERM", "SIGINT", "SIGHUP"].map(count)).toEqual([0, 0, 0]);
  });
}

test("after a signal that the process outlives, a group started later still ends at the next signal", () => {
  start();
  fire("SIGINT");
  const b = start();
  record = [];
  fire("SIGTERM");
  expect(record).toEqual([`SIGTERM -${b.pid} ${LATER}`, `SIGTERM ${process.pid} ${OWNER}`]);
});

test("groups that outlive TERM all get TERM at once and KILL after one grace of 5 s, and their marks go", () => {
  const a = start();
  const b = start();
  row(a.pid).termProof = true;
  row(b.pid).termProof = true;
  const before = Date.now();
  fire("SIGTERM");
  const took = Date.now() - before;
  expect(record).toEqual([`SIGTERM -${a.pid} ${LATER}`, `SIGTERM -${b.pid} ${LATER}`, `SIGKILL -${a.pid} ${LATER}`, `SIGKILL -${b.pid} ${LATER}`, `SIGTERM ${process.pid} ${OWNER}`]);
  expect(took).toBeGreaterThanOrEqual(4_500);
  expect(took).toBeLessThan(8_000);
  expect(marksIn()).toEqual([]);
}, 20_000);

test("a group that outlives KILL keeps its mark", () => {
  const a = start();
  row(a.pid).killProof = true;
  fire("SIGHUP");
  expect(record.slice(0, 2)).toEqual([`SIGTERM -${a.pid} ${LATER}`, `SIGKILL -${a.pid} ${LATER}`]);
  expect(marksIn()).toEqual([`group-${a.pid}.json`]);
}, 30_000);

test("the members of a group are the processes with its id as their group: a child that outlives TERM gets KILL", () => {
  const a = start();
  nextPid += 1;
  rows.push({ pid: nextPid, pgid: a.pid, stat: "S", started: LATER, termProof: true });
  fire("SIGTERM");
  expect(record).toEqual([`SIGTERM -${a.pid} ${LATER}`, `SIGKILL -${a.pid} gone`, `SIGTERM ${process.pid} ${OWNER}`]);
}, 20_000);

test("a zombie member does not hold the stop", () => {
  const a = start();
  nextPid += 1;
  rows.push({ pid: nextPid, pgid: a.pid, stat: "Z", started: LATER });
  fire("SIGTERM");
  expect(record).toEqual([`SIGTERM -${a.pid} ${LATER}`, `SIGTERM ${process.pid} ${OWNER}`]);
});

test("a group whose leader pid now has another start time gets no signal", () => {
  const a = start();
  row(a.pid).started = OTHER;
  fire("SIGTERM");
  expect(record).toEqual([`SIGTERM ${process.pid} ${OWNER}`]);
});

test("a group that ended by itself, whose id another program's leaderless group now holds, gets no signal from a handler or from endGroup", async () => {
  const a = start();
  const b = start();
  rows = rows.filter((each) => each.pgid !== a.pid && each.pgid !== b.pid);
  rows.push({ pid: 90_001, pgid: a.pid, stat: "S", started: OTHER });
  rows.push({ pid: 90_002, pgid: b.pid, stat: "S", started: OTHER });
  await expect(endGroup(b)).rejects.toThrow("still has members");
  fire("exit");
  expect(record).toEqual([]);
  expect(rows.filter((each) => each.pid >= 90_001)).toHaveLength(2);
});

test("the close of the IPC channel ends every group and exits with 1", () => {
  const a = start();
  fire("disconnect");
  expect(record).toEqual([`SIGTERM -${a.pid} ${LATER}`]);
  expect(exits).toEqual([1]);
  expect(marksIn()).toEqual([]);
});

test("exit ends every group", () => {
  const a = start();
  fire("exit");
  expect(record).toEqual([`SIGTERM -${a.pid} ${LATER}`]);
});

test("endGroup ends the group, removes its mark and forgets it", async () => {
  const a = start();
  await endGroup(a);
  expect(record).toEqual([`SIGTERM -${a.pid} ${LATER}`]);
  expect(marksIn()).toEqual([]);
  fire("exit");
  expect(record).toEqual([`SIGTERM -${a.pid} ${LATER}`]);
});

test("sweep: a group whose owner is gone and whose leader matches gets TERM, its mark goes and a line says so", () => {
  const g = group({ members: 5 });
  mark(g, { started: LATER, owner: gone });
  expect(sweepMarks()).toEqual([`process-group: ended group ${g} (started ${LATER}), which a run of this checkout left: 5 processes`]);
  expect(record).toEqual([`SIGTERM -${g} ${LATER}`]);
  expect(marksIn()).toEqual([]);
});

test("sweep: a mark written before its start time was read names the leader of its pid that started within the second before or two seconds after the spawn", () => {
  const g = group({ members: 2 });
  const h = group({ started: OTHER });
  mark(g, { started: null, spawned: LATER_EPOCH - 1, owner: gone });
  mark(h, { started: null, spawned: LATER_EPOCH, owner: gone });
  expect(sweepMarks()).toEqual([`process-group: ended group ${g} (started ${LATER}), which a run of this checkout left: 2 processes`]);
  expect(record).toEqual([`SIGTERM -${g} ${LATER}`]);
  expect(marksIn()).toEqual([]);
});

test("sweep: a mark whose owner is alive stays and its group gets no signal", () => {
  const g = group();
  mark(g, { started: LATER, owner: { pid: process.pid, started: OWNER } });
  expect(sweepMarks()).toEqual([]);
  expect(record).toEqual([]);
  expect(marksIn()).toEqual([`group-${g}.json`]);
});

test("sweep: an owner pid that another process now holds counts as gone", () => {
  const g = group();
  mark(g, { started: LATER, owner: { pid: process.pid, started: OTHER } });
  sweepMarks();
  expect(record).toEqual([`SIGTERM -${g} ${LATER}`]);
});

test("sweep: a group whose leader is gone while members keep its id is left with its mark", () => {
  const g = group({ members: 3 });
  rows = rows.filter((each) => each.pid !== g);
  mark(g, { started: LATER, owner: gone });
  expect(sweepMarks()).toEqual([`process-group: left group ${g} (started ${LATER}): its leader is gone and 2 processes have its id as their group`]);
  expect(record).toEqual([]);
  expect(marksIn()).toEqual([`group-${g}.json`]);
});

test("sweep: a group that is gone, or whose leader pid has another start time, loses its mark with no line and no signal", () => {
  mark(54_321, { started: LATER, owner: gone });
  const g = group({ started: OTHER });
  mark(g, { started: LATER, owner: gone });
  expect(sweepMarks()).toEqual([]);
  expect(record).toEqual([]);
  expect(marksIn()).toEqual([]);
});

test("sweep: a group that outlives KILL keeps its mark and a line says so", () => {
  const g = group({ killProof: true });
  mark(g, { started: LATER, owner: gone });
  expect(sweepMarks()).toEqual([`process-group: group ${g} (started ${LATER}) still has members after KILL`]);
  expect(record).toEqual([`SIGTERM -${g} ${LATER}`, `SIGKILL -${g} ${LATER}`]);
  expect(marksIn()).toEqual([`group-${g}.json`]);
}, 30_000);

test("sweep: a file that is not named group-<pid>.json is not read", () => {
  const g = group();
  mark(g, { started: LATER, owner: gone }, `group-${g}.json.old`);
  mark(g, { started: LATER, owner: gone }, `x-group-${g}.json`);
  expect(sweepMarks()).toEqual([]);
  expect(record).toEqual([]);
});

test("sweep: a mark of pid 1, of pid 0, with no owner or with no start time and no spawn second is skipped and nothing is signalled", () => {
  mark(1, { started: "Sun Nov 1 00:00:00 2026", owner: gone });
  mark(0, { started: "Sun Nov 1 00:00:00 2026", owner: gone }, "group-0.json");
  const g = group();
  const h = group();
  mark(g, { started: LATER });
  mark(h, { started: null, owner: gone });
  expect(sweepMarks()).toEqual([
    "process-group: skipped the mark group-0.json, which is no group with an owner",
    "process-group: skipped the mark group-1.json, which is no group with an owner",
    `process-group: skipped the mark group-${g}.json, which is no group with an owner`,
    `process-group: skipped the mark group-${h}.json, which is no group with an owner`,
  ]);
  expect(record).toEqual([]);
});

test("sweep: a directory that does not exist gives no line", () => {
  process.env.HALVO_GROUP_MARKS = join(marks, "none");
  expect(sweepMarks()).toEqual([]);
});

test("the node tests' set-up prints each line of the sweep to stderr", async () => {
  const g = group();
  mark(g, { started: LATER, owner: gone });
  const setup = (await import("./node-setup.ts")).default;
  const written: string[] = [];
  const original = process.stderr.write;
  process.stderr.write = (text: string) => {
    written.push(text);
    return true;
  };
  try {
    setup();
  } finally {
    process.stderr.write = original;
  }
  expect(written).toEqual([`process-group: ended group ${g} (started ${LATER}), which a run of this checkout left: 1 processes\n`]);
});

test("the kit reads start times in UTC whatever TZ its process has, so a sweep under another TZ keeps the mark of a live owner", async () => {
  useProcesses(null);
  const zone = process.env.TZ;
  process.env.TZ = "Pacific/Kiritimati";
  const sleeper = startGroup(process.execPath, ["-e", "setInterval(() => {}, 1 << 30)"], { stdio: ["ignore", "ignore", "ignore"] });
  try {
    const written = JSON.parse(readFileSync(join(marks, `group-${sleeper.pid}.json`), "utf8"));
    const utc = execFileSync("ps", ["-o", "lstart=", "-p", String(sleeper.pid)], { encoding: "utf8", env: { ...process.env, TZ: "UTC0", LC_ALL: "C" } });
    expect(written.started).toBe(utc.trim().replace(/\s+/g, " "));
    process.env.TZ = "America/Adak";
    expect(sweepMarks()).toEqual([]);
    expect(marksIn()).toEqual([`group-${sleeper.pid}.json`]);
  } finally {
    if (zone === undefined) delete process.env.TZ;
    else process.env.TZ = zone;
    await endGroup(sleeper);
  }
  expect(marksIn()).toEqual([]);
}, 30_000);
