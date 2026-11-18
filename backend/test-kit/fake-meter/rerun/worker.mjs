import { Worker } from "node:worker_threads";

const PARENT_POLL_MS = 500;
const WATCHDOG = `
const { workerData } = require("node:worker_threads");
const { writeSync } = require("node:fs");
function gone() {
  if (process.ppid !== workerData.parent) return true;
  try {
    process.kill(workerData.parent, 0);
    return false;
  } catch (error) {
    return error.code === "ESRCH";
  }
}
function endSelf() {
  process.kill(process.pid, "SIGKILL");
}
setInterval(() => {
  if (!gone()) return;
  writeSync(2, "check 55 worker " + process.pid + ": its parent " + workerData.parent + " is gone; the worker ends itself\\n");
  endSelf();
}, workerData.pollMs);
`;
new Worker(WATCHDOG, { eval: true, workerData: { parent: process.ppid, pollMs: PARENT_POLL_MS } }).unref();

const sqlite = process.getBuiltinModule("node:sqlite");
const prepare = sqlite.DatabaseSync.prototype.prepare;
const prepared = new WeakMap();
sqlite.DatabaseSync.prototype.prepare = function prepareOnce(query) {
  let statements = prepared.get(this);
  if (statements === undefined) {
    statements = new Map();
    prepared.set(this, statements);
  }
  let statement = statements.get(query);
  if (statement === undefined) {
    statement = prepare.call(this, query);
    statements.set(query, statement);
  }
  return statement;
};
const close = sqlite.DatabaseSync.prototype.close;
sqlite.DatabaseSync.prototype.close = function closeAndForget() {
  prepared.delete(this);
  return close.call(this);
};

const { propertyRun, runOnce, searchCases, searchOne } = await import("./property.ts");
const cases = searchCases();

async function releasedWorlds(job) {
  if (typeof globalThis.gc !== "function") throw new Error("the worker runs without --expose-gc, so it cannot collect before it counts");
  const proto = sqlite.DatabaseSync.prototype;
  const refs = [];
  const seen = new WeakSet();
  const note = (db) => {
    if (seen.has(db)) return;
    seen.add(db);
    refs.push(new WeakRef(db));
  };
  const { prepare: cachedPrepare, exec } = proto;
  proto.prepare = function noted(query) {
    note(this);
    return cachedPrepare.call(this, query);
  };
  proto.exec = function noted(query) {
    note(this);
    return exec.call(this, query);
  };
  let sequences = 0;
  try {
    for (let round = 0; round < job.rounds; round += 1) sequences += (await searchOne(cases[job.index])).sequences;
  } finally {
    proto.prepare = cachedPrepare;
    proto.exec = exec;
  }
  for (let pass = 0; pass < 3; pass += 1) {
    await new Promise((resolve) => setTimeout(resolve, 0));
    globalThis.gc();
  }
  return { sequences, databases: refs.length, live: refs.filter((ref) => ref.deref() !== undefined).length, heapUsed: process.memoryUsage().heapUsed };
}

async function run(job) {
  const started = performance.now();
  if (job.kind === "memory") return { ...(await releasedWorlds(job)), seconds: (performance.now() - started) / 1000 };
  if (job.kind === "seed") {
    const result = await propertyRun({ seed: job.seed, orderings: job.orderings });
    return { ...result, seconds: (performance.now() - started) / 1000 };
  }
  const searchCase = cases[job.index];
  if (searchCase === undefined) throw new RangeError(`no search setting ${job.index}`);
  const result = await (job.kind === "once" ? runOnce(searchCase) : searchOne(searchCase));
  return { ...result, seconds: (performance.now() - started) / 1000 };
}

function memory() {
  return { resident: process.memoryUsage().rss, peak: process.resourceUsage().maxRSS * 1024 };
}

process.on("message", (job) => {
  if (job.kind === "exit") {
    process.disconnect();
    return;
  }
  run(job).then(
    (result) => process.send({ id: job.id, result, memory: memory() }),
    (error) => process.send({ id: job.id, error: `${error?.name ?? "Error"}: ${String(error?.message ?? error).split("\n")[0]}`, memory: memory() }),
  );
});
process.on("disconnect", () => process.exit(0));
process.send({ ready: cases.length });
