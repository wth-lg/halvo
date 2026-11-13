import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { appendFileSync, closeSync, existsSync, mkdirSync, openSync, readFileSync, writeFileSync } from "node:fs";
import http from "node:http";
import { createServer } from "node:net";
import { loadavg } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { stripVTControlCharacters } from "node:util";
import { z } from "zod";
import { holdFor } from "./config.mjs";
import { call } from "./http.mjs";
import { gatewayOf, judge } from "./rule.mjs";
import { MICRO_PER_MINOR, spikeApi } from "./targets.mjs";

const int = z.number().int();
const MockEvent = z.object({
  t: int,
  id: z.string(),
  ev: z.enum(["request", "usage", "end"]),
  final: z.boolean().optional(),
  input_tokens: int.optional(),
  output_tokens: int.optional(),
  clean: z.boolean().optional(),
});
/** The header x-ledger-receipt as the driver keeps it: the hold's amount, its expiry and the ledger's time of the reserve, which the S1 spike's receipt lacks. */
export const Receipt = z.object({ requestId: z.string(), amount: int, now: int.optional(), expiresAt: int });
const Trailer = z.union([
  z.object({ settled: z.literal(true), outcome: z.string(), cost: int, settle_ms: int }),
  z.object({ settled: z.literal(false), why: z.string() }),
]);

const isFunction = z.custom((value) => typeof value === "function");
const CtxFromTheCaller = z.object({
  cfg: z.object({}),
  target: z.object({ command: isFunction }),
  outDir: z.string(),
  jobs: int.positive(),
  progress: isFunction,
});

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

export function mulberry32(seed) {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
export function fnv1a32(name) {
  let hash = 0x811c9dc5;
  for (let i = 0; i < name.length; i++) hash = Math.imul(hash ^ name.charCodeAt(i), 0x01000193);
  return hash >>> 0;
}


function processTable() {
  return execFileSync("ps", ["-axo", "pid=,ppid=,pgid="], { encoding: "utf8" })
    .trim()
    .split("\n")
    .map((line) => {
      const [pid, ppid, pgid] = line.trim().split(/\s+/).map(Number);
      return { pid, ppid, pgid };
    });
}

function treeOf(root) {
  const table = processTable();
  const found = new Set([root]);
  for (let grew = true; grew; ) {
    grew = false;
    for (const entry of table) {
      if (!found.has(entry.pid) && (found.has(entry.ppid) || entry.pgid === root)) {
        found.add(entry.pid);
        grew = true;
      }
    }
  }
  return table.filter((entry) => found.has(entry.pid)).map((entry) => entry.pid);
}

function alive(pids) {
  const live = new Set(processTable().map((entry) => entry.pid));
  return pids.filter((pid) => live.has(pid));
}

export function startedAt(pid) {
  try {
    const env = { ...process.env, LC_ALL: "C", TZ: "UTC0" };
    return execFileSync("ps", ["-o", "lstart=", "-p", String(pid)], { encoding: "utf8", env }).trim();
  } catch {
    return "";
  }
}

const NO_SUCH_PROCESS = "ESRCH";
const ONLY_AN_UNREAPED_ZOMBIE_IN_THE_GROUP = "EPERM";

function signal(pid, name) {
  try {
    process.kill(pid, name);
  } catch (error) {
    if (error.code !== NO_SUCH_PROCESS && error.code !== ONLY_AN_UNREAPED_ZOMBIE_IN_THE_GROUP) throw error;
  }
}

function signalGroup(pgid, name) {
  signal(-pgid, name);
}

async function killTree(server) {
  const killedAt = Date.now();
  signalGroup(server.pid, "SIGKILL");
  const pids = [...new Set([...server.pidsAtReady, ...treeOf(server.pid)])];
  for (const pid of pids) signal(pid, "SIGKILL");
  for (const until = Date.now() + 5000; ; await sleep(20)) {
    const left = alive(pids);
    if (left.length === 0) {
      server.stopped?.();
      return killedAt;
    }
    if (Date.now() > until) throw new Error(`processes survived SIGKILL: ${left.join(" ")}`);
  }
}

function register(ctx, pid) {
  ctx.started.add(pid);
  appendFileSync(ctx.registry, `${JSON.stringify({ pid, started: startedAt(pid) })}\n`);
}

export const isStillThatProcess = ({ pid, started }) => Boolean(started) && startedAt(pid) === started;

export function sweep(registry) {
  if (!existsSync(registry)) return [];
  const stopped = [];
  for (const line of readFileSync(registry, "utf8").split("\n").filter(Boolean)) {
    const { pid, started } = JSON.parse(line);
    if (!isStillThatProcess({ pid, started })) continue;
    for (const member of treeOf(pid)) signal(member, "SIGKILL");
    stopped.push(pid);
  }
  writeFileSync(registry, "");
  return stopped;
}


async function portsToSpare(limit) {
  for (;;) {
    const waiting = execFileSync("netstat", ["-anp", "tcp"], { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 }).split("TIME_WAIT").length - 1;
    if (waiting < limit) return;
    await sleep(1000);
  }
}

const apiOf = (ctx) => ctx.target.api ?? spikeApi;

const OWN_ERROR_EVENTS = new Set(["handler-error", "rejected", "answer-defect", "bad-variable", "alarm-error", "alarm-stopped"]);
function isOwnErrorLine(line) {
  const text = line.trim();
  if (!text.startsWith("{")) return false;
  try {
    return OWN_ERROR_EVENTS.has(JSON.parse(text).event);
  } catch {
    return false;
  }
}

const handedOut = new Set();
async function freePort() {
  for (;;) {
    const port = await new Promise((resolve, reject) => {
      const probe = createServer();
      probe.once("error", reject);
      probe.listen(0, "127.0.0.1", () => {
        const { port: picked } = probe.address();
        probe.close(() => resolve(picked));
      });
    });
    if (!handedOut.has(port)) {
      handedOut.add(port);
      return port;
    }
  }
}

async function startMock(ctx, logPath) {
  const child = spawn(process.execPath, [ctx.mockScript, logPath], { stdio: ["ignore", "pipe", "inherit"] });
  register(ctx, child.pid);
  const port = await new Promise((resolve, reject) => {
    let text = "";
    child.stdout.on("data", (part) => {
      text += part;
      const line = text.indexOf("\n");
      if (line >= 0) resolve(JSON.parse(text.slice(0, line)).port);
    });
    child.once("exit", (code) => reject(new Error(`the mock upstream exited with ${code}`)));
  });
  return { pid: child.pid, port, log: logPath };
}

async function startServer(ctx, options) {
  const { file, args, env } = ctx.target.command(options);
  const log = openSync(options.logFile, "a");
  const child = spawn(file, args, { detached: true, stdio: ["ignore", log, log], env });
  closeSync(log);
  let exited = false;
  child.once("exit", () => {
    exited = true;
  });
  register(ctx, child.pid);
  const server = { pid: child.pid, pidsAtReady: [child.pid], port: options.port, readyMs: 0, readyAt: 0, stopped: () => ctx.started.delete(child.pid) };
  const began = Date.now();
  const timeOfFirstAnswer = async () => {
    for (; ; await sleep(40)) {
      if (exited) throw new Error(`the server exited before it answered; see ${options.logFile}`);
      const answer = await apiOf(ctx).health(options.port).catch(() => null);
      if (answer?.status === 200) return Date.now();
      if (Date.now() - began > ctx.cfg.readyTimeoutMs) {
        await killTree(server);
        throw new Error(`the server did not answer within ${ctx.cfg.readyTimeoutMs} ms; see ${options.logFile}`);
      }
    }
  };
  server.readyAt = await timeOfFirstAnswer();
  server.readyMs = server.readyAt - began;
  server.pidsAtReady = treeOf(child.pid);
  return server;
}


const PROMPT = "Synthetic stress prompt with no captured content. ";
const SLOW_STREAM_LASTS = { from: 0.75, to: 1.25 };
const PERIOD_BEGAN_BEFORE_THE_CAP_MS = 60_000;

function planRun(cell, cfg, random, key) {
  const perClient = cell.kind === "wave" || cell.kind === "quiet" ? 1 : (cell.requestsPerClient ?? cfg.requestsPerClient[cell.level]);
  const clients = Math.max(cell.level, 4);
  const calm = cell.kind === "wave" || cell.kind === "void" || cell.kind === "rowsize";
  const hex = (length) => Array.from({ length }, () => Math.floor(random() * 16).toString(16)).join("");
  const prefix = `${hex(8)}-${hex(4)}-4${hex(3)}-8${hex(3)}`;
  const abortPhase = Math.floor(random() * cfg.abortMidEvery);
  const plan = [];
  let others = 0;
  for (let index = 0; index < cell.level * perClient; index++) {
    const client = index % clients;
    let behavior = "normal";
    if (!calm && client === 0) behavior = "abort-before-usage";
    else if (!calm && (others++ + abortPhase) % cfg.abortMidEvery === 0) behavior = "abort-mid";
    const wave = cell.kind === "wave";
    const maxTokens = wave ? cfg.maxTokens[1] : cfg.maxTokens[Math.floor(random() * cfg.maxTokens.length)];
    const fault = cell.switch !== "usage-last" && random() < cfg.faultShare ? cell.switch : "usage-last";
    const content = PROMPT.repeat(4).slice(0, wave ? 120 : 60 + Math.floor(random() * 140));
    const tier = "low_cost_hosted";
    const mock = {
      switch: fault,
      shape: cell.switch === "anthropic-cut" ? "anthropic" : "openai",
      chunks: cfg.chunks,
      chunk_ms: cfg.chunkMs,
      input_tokens: Math.ceil(content.length / 4),
      output_tokens: wave ? 100 + Math.floor(random() * 28) : 1 + Math.floor(random() * maxTokens),
    };
    if (fault === "slow") {
      const gatewayAllowsMs = cfg.holdLifeMs - cfg.marginMs;
      const waitsBeforeUsage = cfg.chunks + 1;
      const timesWhatTheGatewayAllows = SLOW_STREAM_LASTS.from + (SLOW_STREAM_LASTS.to - SLOW_STREAM_LASTS.from) * random();
      mock.chunk_ms = Math.round((timesWhatTheGatewayAllows * gatewayAllowsMs) / waitsBeforeUsage);
    }
    const heldInARollover = cell.kind === "rollover" && index < cell.level - 1;
    if (wave || heldInARollover) mock.wave = key;
    if (cell.kind === "rowsize") Object.assign(mock, { chunks: 1, chunk_ms: 1 });
    const build = () => JSON.stringify({ tier, max_tokens: maxTokens, stream: true, messages: [{ role: "user", content }], mock });
    if (fault === "over-report") {
      const timesTheInputTheHoldAssumes = 3;
      const bytesTheBodyCanGainByThisRewrite = 20;
      mock.output_tokens = maxTokens;
      mock.input_tokens = timesTheInputTheHoldAssumes * (Buffer.byteLength(build()) + bytesTheBodyCanGainByThisRewrite);
    }
    const body = build();
    plan.push({
      index,
      id: `${prefix}-${index.toString(16).padStart(12, "0")}`,
      client,
      behavior,
      fault,
      body,
      hold: holdFor(cfg.price, Buffer.byteLength(body), maxTokens),
      abortAfter: 1 + Math.floor(random() * (cfg.chunks - 1)),
      thinkMs: Math.floor(random() * cfg.thinkMs),
    });
  }
  return plan;
}

const wholeMinorUnitsAtOrAbove = (micro) => Math.ceil(micro / MICRO_PER_MINOR) * MICRO_PER_MINOR;

function capOf(cell, cfg, plan) {
  const total = plan.reduce((sum, spec) => sum + spec.hold, 0);
  if (cell.kind === "rowsize") return wholeMinorUnitsAtOrAbove(total);
  if (cell.kind === "wave") {
    assert(plan.every((spec) => spec.hold === plan[0].hold), "the requests of a wave must all have the same hold, or a cap of K holds does not admit exactly K");
    assert(plan[0].hold >= MICRO_PER_MINOR, "a hold of the wave must be at least one minor unit, or a cap of whole minor units at or above K holds admits more than K");
    return wholeMinorUnitsAtOrAbove(cell.k * plan[0].hold);
  }
  const meanHold = Math.round(total / plan.length);
  return wholeMinorUnitsAtOrAbove(cfg.capHolds[cell.level] * meanHold);
}

function accountOf(random) {
  const hex = (length) => Array.from({ length }, () => Math.floor(random() * 16).toString(16)).join("");
  const variant = "89ab"[Math.floor(random() * 4)];
  return { account: `${hex(8)}-${hex(4)}-4${hex(3)}-${variant}${hex(3)}-${hex(12)}`, credential: hex(64) };
}


const MAX_RESENDS = 5;
const LOST_BEFORE_AN_ANSWER = new Set(["ECONNRESET", "EPIPE"]);
const resetBeforeTheServerAccepted = (record) => record.status === null && LOST_BEFORE_AN_ANSWER.has(record.error);
/** True when an attempt was lost on its way to the gateway and the driver sends it again under its id. */
export const lostOnTheWayIn = (record) => resetBeforeTheServerAccepted(record) || record.status === 500;
/** True when the driver sends an attempt again: it was lost on its way in, the traffic is not paused, and fewer than MAX_RESENDS earlier attempts were sent again. */
export const sendsAgain = (record, paused) => !paused && record.lost.length < MAX_RESENDS && lostOnTheWayIn(record);
/** The entry that the driver keeps in a request's record for an attempt that it sends again. */
export const lostAttemptOf = (record) => (resetBeforeTheServerAccepted(record) ? { why: "reset", body: "" } : { why: "500", body: record.body });

class Traffic {
  constructor(plan, level, port, chat) {
    this.plan = plan;
    this.level = level;
    this.port = port;
    this.chat = chat;
    this.records = plan.map((spec) => ({
      id: spec.id,
      index: spec.index,
      client: spec.client,
      behavior: spec.behavior,
      fault: spec.fault,
      hold: spec.hold,
      sentAt: null,
      headersAt: null,
      endedAt: null,
      status: null,
      receipt: null,
      trailer: null,
      chunks: 0,
      abortedAt: null,
      endedCleanly: false,
      error: null,
      body: "",
      lost: [],
      counted: false,
    }));
    this.active = 0;
    this.answered = 0;
    this.admittedCount = 0;
    this.settlesSeen = 0;
    this.admittedAfterSettle = 0;
    this.paused = false;
    this.stopped = false;
    this.finished = false;
    this.waiters = [];
    this.done = null;
  }

  run() {
    this.done = Promise.all(Array.from({ length: this.level }, (_, slot) => this.client(slot))).then(() => {
      this.finished = true;
      this.wake();
    });
    return this.done;
  }

  wake() {
    for (const waiter of this.waiters.splice(0)) waiter();
  }

  async until(test) {
    while (!test()) await new Promise((resolve) => this.waiters.push(resolve));
  }

  async client(slot) {
    for (let index = slot; index < this.plan.length; index += this.level) {
      const record = this.records[index];
      if (index !== slot) await sleep(this.plan[index].thinkMs);
      for (;;) {
        await this.until(() => !this.paused || this.stopped);
        if (this.stopped) return;
        this.active++;
        await this.issue(this.plan[index], record);
        this.active--;
        this.wake();
        if (!sendsAgain(record, this.paused)) break;
        record.lost.push(lostAttemptOf(record));
        Object.assign(record, { sentAt: null, status: null, headersAt: null, endedAt: null, error: null, body: "", endedCleanly: false });
        await sleep(25 * record.lost.length);
      }
      if (!record.counted) {
        record.counted = true;
        this.answered++;
        this.wake();
      }
    }
  }

  async armedOnAdmitted(count) {
    const ready = () => this.admittedCount >= count;
    await this.until(() => ready() || this.finished);
    return ready();
  }

  async armedWithSettledSpendAndOpenHold() {
    const ready = () => this.admittedAfterSettle > 0;
    await this.until(() => ready() || this.finished);
    return ready();
  }

  admitted(test) {
    return this.records.some((record) => record.status === 200 && record.receipt !== null && test(record));
  }

  async armedAfter(time) {
    const ready = () => this.records.some((record) => record.status === 200 && record.sentAt >= time);
    await this.until(() => ready() || this.finished);
    return ready();
  }

  pause() {
    this.paused = true;
  }
  resume() {
    this.paused = false;
    this.wake();
  }
  stop() {
    this.stopped = true;
    this.wake();
  }
  idle() {
    return this.until(() => this.active === 0);
  }
  inFlight() {
    return this.records.filter((record) => record.status === 200 && record.endedAt === null).length;
  }
  lastEndedAt() {
    return Math.max(0, ...this.records.map((record) => record.endedAt ?? 0));
  }

  issue(spec, record) {
    return new Promise((resolve) => {
      const finish = (error, clean = false) => {
        if (record.endedAt !== null) return;
        record.endedAt = Date.now();
        record.endedCleanly = clean;
        if (error) record.error = error.code ?? error.message;
        this.wake();
        resolve();
      };
      const headers = { ...this.chat.headers, "content-type": "application/json", "content-length": Buffer.byteLength(spec.body), "x-request-id": spec.id };
      const options = { host: "127.0.0.1", port: this.port, path: this.chat.path, method: "POST", agent: false, headers };
      const request = http.request(options, (response) => {
        record.status = response.statusCode;
        record.headersAt = Date.now();
        const receipt = response.headers["x-ledger-receipt"];
        if (receipt) record.receipt = Receipt.parse(JSON.parse(receipt));
        if (record.status !== 500) {
          record.counted = true;
          this.answered++;
        }
        if (record.status === 200) {
          this.admittedCount++;
          if (this.settlesSeen > 0) this.admittedAfterSettle++;
        }
        this.wake();
        const abort = () => {
          record.abortedAt = Date.now();
          request.destroy();
          finish();
        };
        let pending = "";
        response.setEncoding("utf8");
        response.on("data", (text) => {
          if (record.status !== 200) {
            if (record.body.length < 400) record.body += text.slice(0, 400);
            return;
          }
          pending += text;
          for (let end = pending.indexOf("\n\n"); end >= 0 && record.abortedAt === null; end = pending.indexOf("\n\n")) {
            const block = pending.slice(0, end);
            pending = pending.slice(end + 2);
            const data = block.slice(block.indexOf("data: ") + 6);
            if (block.startsWith("event: ledger")) {
              record.trailer = Trailer.parse(JSON.parse(data));
              if (record.trailer.settled) this.settlesSeen++;
            } else if (data !== "[DONE]") {
              const chunk = JSON.parse(data);
              const choice = chunk.choices?.[0];
              if (chunk.type === "content_block_delta" || choice?.delta?.content !== undefined) {
                record.chunks++;
                if (spec.behavior === "abort-mid" && record.chunks >= spec.abortAfter) abort();
              } else if (chunk.type === "content_block_stop" || choice?.finish_reason === "stop") {
                if (spec.behavior === "abort-before-usage") abort();
              }
            }
          }
        });
        response.on("end", () => finish(null, true));
        response.on("error", finish);
        response.on("close", () => finish());
      });
      request.on("error", finish);
      request.setTimeout(20_000, () => request.destroy(new Error("the driver's request timed out")));
      record.sentAt = Date.now();
      request.end(spec.body);
    });
  }
}


async function crossTheBoundary(traffic, mockPort, wave, boundaryAt, holdLifeMs) {
  const admittedAt = (record) => record.receipt.expiresAt - holdLifeMs;
  const beforeTheBoundary = (record) => admittedAt(record) < boundaryAt;
  const fromTheBoundary = (record) => admittedAt(record) >= boundaryAt;
  await traffic.until(() => traffic.admitted((record) => beforeTheBoundary(record) && record.endedAt === null) || traffic.admitted(fromTheBoundary) || traffic.finished);
  traffic.pause();
  await sleep(Math.max(0, boundaryAt + 1 - Date.now()));
  traffic.resume();
  await traffic.until(() => traffic.admitted(fromTheBoundary) || traffic.finished);
  await call(mockPort, "POST", `/release?wave=${wave}`).catch(() => undefined);
}

/** The options with which the driver starts a cell's server: its ports, its state and log under the run's directory, and the hold life of the cell's gateway. */
export const serverOptionsOf = (cfg, cell, variant, dir, ports) => ({
  ...ports,
  stateDir: join(dir, "state"),
  logFile: join(dir, "server.log"),
  holdLifeMs: gatewayOf(cfg, cell).holdLifeMs,
  variant,
});

export async function runOnce(ctx, cell, variant, runNo, withKill, attempt) {
  const cfg = ctx.cfg;
  const who = variant === "counts-open-holds" ? "ledger" : "control";
  const key = `${who}-${cell.name.replace("@", "-at-")}-${withKill ? "kill" : "plain"}-${runNo}-try${attempt + 1}`;
  const seed = fnv1a32(key);
  const random = mulberry32(seed);
  const plan = planRun(cell, cfg, random, key);
  const cap = capOf(cell, cfg, plan);
  const run = accountOf(random);
  const api = apiOf(ctx);
  const dir = join(ctx.outDir, "runs", key);
  mkdirSync(dir, { recursive: true });
  await portsToSpare(cfg.timeWaitLimit);
  const began = Date.now();
  const oneMinuteLoadAtStart = Math.round(loadavg()[0] * 10) / 10;
  const mock = await startMock(ctx, join(dir, "mock.jsonl"));
  let server = null;
  let record;
  let kill = null;
  const gateway = gatewayOf(cfg, cell);
  try {
    const options = serverOptionsOf(cfg, cell, variant, dir, { port: await freePort(), inspectorPort: await freePort(), mockPort: mock.port });
    server = await startServer(ctx, options);
    const startMs = server.readyMs;
    const setAt = Date.now();
    const nextBoundaryAt = cell.kind === "rollover" ? setAt + cfg.rolloverAfterMs : setAt - PERIOD_BEGAN_BEFORE_THE_CAP_MS + cfg.periodMs;
    const anchor = nextBoundaryAt - cfg.periodMs;
    await api.setCap(server.port, run, cap, anchor, cfg.periodMs);
    const empty = await api.snapshot(server.port, run);

    const traffic = new Traffic(plan, cell.level, server.port, api.chat(run));
    traffic.run();
    if (cell.kind === "wave") {
      traffic
        .until(() => traffic.answered === plan.length)
        .then(() => call(mock.port, "POST", `/release?wave=${key}`))
        .catch(() => undefined);
    }
    if (cell.kind === "rollover") await crossTheBoundary(traffic, mock.port, key, nextBoundaryAt, options.holdLifeMs);
    let afterRestart = null;
    let restartMs = null;
    let restartReadyAt = null;
    let firstLedgerReadAt = null;
    if (withKill) {
      const sendsOneBurstOnly = cell.kind === "quiet";
      const late = runNo % 2 === 0 && attempt === 0 && !sendsOneBurstOnly;
      const draw = 1 + Math.floor(random() * 3);
      const oneAtATime = cell.level === 1;
      const armOn = oneAtATime ? 1 : draw;
      const delay = cfg.killDelayMs.min + random() * (cfg.killDelayMs.max - cfg.killDelayMs.min);
      const armed =
        cell.kind === "rollover"
          ? traffic.armedAfter(nextBoundaryAt)
          : late
            ? traffic.armedWithSettledSpendAndOpenHold()
            : traffic.armedOnAdmitted(armOn);
      if (await armed) await sleep(delay);
      traffic.pause();
      const inFlight = traffic.inFlight();
      kill = { at: await killTree(server), inFlight };
      await traffic.idle();
      const restartsWithAlarmOverdue = cell.kind === "quiet" && runNo % 2 === 0;
      if (restartsWithAlarmOverdue) {
        const everyHoldExpiredBy = kill.at + options.holdLifeMs;
        await sleep(Math.max(0, everyHoldExpiredBy + cfg.quietOverdueMs - Date.now()));
      }
      server = await startServer(ctx, options);
      restartMs = server.readyMs;
      restartReadyAt = server.readyAt;
      if (cell.kind === "quiet") {
        traffic.stop();
      } else {
        firstLedgerReadAt = Date.now();
        afterRestart = await api.snapshot(server.port, run);
        traffic.resume();
      }
    }
    await traffic.done;
    const due = Math.max(traffic.lastEndedAt() + options.holdLifeMs, restartReadyAt ?? 0) + cfg.alarmGraceMs;
    await sleep(Math.max(0, due - Date.now()));
    firstLedgerReadAt ??= Date.now();
    const final = await api.snapshot(server.port, run);
    record = { requests: traffic.records, empty, afterRestart, final, kill, firstLedgerReadAt, restartReadyAt, startMs, restartMs, anchor, periodMs: cfg.periodMs, gateway };
  } catch (error) {
    if (error instanceof Error) error.afterKill = kill !== null;
    throw error;
  } finally {
    if (server) await killTree(server);
    signal(mock.pid, "SIGTERM");
    ctx.started.delete(mock.pid);
  }
  const mockEvents = readFileSync(mock.log, "utf8")
    .split("\n")
    .filter(Boolean)
    .map((line) => MockEvent.parse(JSON.parse(line)));
  const logged = stripVTControlCharacters(readFileSync(join(dir, "server.log"), "utf8"))
    .split("\n")
    .filter((line) => line.includes("[wrangler:error]") || line.includes("[ERROR]") || isOwnErrorLine(line));
  const proxyErrors = logged.filter((line) => line.includes("Error inside ProxyWorker") && line.includes("Network connection lost"));
  const serverErrors = [...new Set(logged.filter((line) => !proxyErrors.includes(line)).map((line) => line.trim().slice(0, 200)))];
  const verdict = judge({ cell, cap, price: cfg.price, mockEvents, serverErrors, chatStatus: api.chatStatus, ledger: api.ledger, ...record });
  const result = {
    key,
    cell: cell.name,
    kind: cell.kind,
    level: cell.level,
    switch: cell.switch,
    variant,
    withKill,
    seed,
    cap,
    counted: verdict.counted,
    violations: verdict.violations,
    facts: { ...verdict.facts, proxyErrorsLogged: proxyErrors.length, load: oneMinuteLoadAtStart, emptyDatabaseSize: record.empty.databaseSize, startMs: record.startMs, restartMs: record.restartMs, inFlightAtKill: record.kill?.inFlight ?? null },
    account: run.account,
    ms: Date.now() - began,
    error: null,
  };
  writeFileSync(join(dir, "run.json"), `${JSON.stringify({ result, cap, ...record, mockEvents })}\n`);
  return result;
}


const runsAlone = (cell) => cell.level >= 200 || cell.kind === "rowsize";

async function schedule(tasks, capacity, halted) {
  let used = 0;
  let waiters = [];
  const running = [];
  for (const task of tasks) {
    const weight = Math.min(task.weight, capacity);
    while (used + weight > capacity) await new Promise((resolve) => waiters.push(resolve));
    if (halted()) break;
    used += weight;
    running.push(
      task.run().finally(() => {
        used -= weight;
        const woken = waiters;
        waiters = [];
        for (const waiter of woken) waiter();
      }),
    );
  }
  return Promise.all(running);
}

const FACTS_OF_A_DISCARDED_ATTEMPT = ["slow", "burstWaits", "burstMedianMs", "slowBoundMs", "lateSettles", "lostInTransport", "crossingBuilt", "firstAdmissionMs"];
const CROSSING_DRAWS_MAX = 5;

async function runCounted(ctx, cell, variant, runNo, withKill) {
  const who = variant === "counts-open-holds" ? "ledger" : "control";
  const discarded = [];
  let unbuiltDraws = 0;
  for (let attempt = 0; ; attempt++) {
    const began = Date.now();
    let result;
    try {
      result = await (ctx.runOnce ?? runOnce)(ctx, cell, variant, runNo, withKill, attempt);
    } catch (error) {
      const text = String(error?.stack ?? error);
      const violations = error?.afterKill ? [{ clause: "integrity", id: null, detail: `the run failed after the kill: ${text.split("\n")[0]}` }] : [];
      const key = `${who}-${cell.name.replace("@", "-at-")}-${withKill ? "kill" : "plain"}-${runNo}-try${attempt + 1}`;
      result = { key, cell: cell.name, kind: cell.kind, level: cell.level, switch: cell.switch, variant, withKill, counted: false, violations, facts: null, ms: Date.now() - began, error: text };
    }
    const unbuilt = result.facts?.crossed === false && result.facts?.crossingBuilt === false;
    const drawsOfTheCell = ctx.crossingDraws.get(cell.name) ?? 0;
    const drawAgain = unbuilt && drawsOfTheCell < CROSSING_DRAWS_MAX;
    if (drawAgain) {
      ctx.crossingDraws.set(cell.name, drawsOfTheCell + 1);
      unbuiltDraws++;
    }
    const rigUnbuilt = unbuilt && !drawAgain;
    const last = rigUnbuilt || attempt + 1 - unbuiltDraws >= ctx.cfg.attempts;
    const neverCounted = last && !(result.counted && !result.error);
    const lostTheCrossing = result.facts?.crossed === false && !unbuilt;
    if (ctx.stopAtFirstViolation && (result.violations.length > 0 || neverCounted || lostTheCrossing)) ctx.halted = true;
    if ((result.counted && !result.error) || last || ctx.halted) {
      ctx.progress(result, discarded.length);
      return rigUnbuilt ? { ...result, discarded, rigUnbuilt } : { ...result, discarded };
    }
    if (result.error || result.facts?.slow === true || unbuilt) ctx.progress(result, discarded.length);
    discarded.push({
      key: result.key,
      error: result.error,
      ms: result.ms,
      openAtKill: result.facts?.openAtKill ?? null,
      streamAtKill: result.facts?.streamAtKill ?? null,
      crossed: result.facts?.crossed ?? null,
      ...Object.fromEntries(FACTS_OF_A_DISCARDED_ATTEMPT.filter((name) => name in (result.facts ?? {})).map((name) => [name, result.facts[name]])),
      violations: result.violations,
    });
  }
}

export async function runCells(ctx, cellList, variant) {
  CtxFromTheCaller.parse(ctx);
  mkdirSync(ctx.outDir, { recursive: true });
  ctx.registry ??= join(ctx.outDir, "..", "started.jsonl");
  ctx.started ??= new Set();
  ctx.crossingDraws ??= new Map();
  ctx.mockScript ??= fileURLToPath(new URL("./upstream.mjs", import.meta.url));
  const tasks = [];
  for (const cell of cellList) {
    for (let runNo = 1; runNo <= cell.killRuns; runNo++) tasks.push({ cell, runNo, withKill: true });
    for (let runNo = 1; runNo <= cell.plainRuns; runNo++) tasks.push({ cell, runNo, withKill: false });
  }
  const weight = (task) => (runsAlone(task.cell) ? ctx.jobs : 1);
  tasks.sort((a, b) => weight(a) - weight(b));
  return schedule(
    tasks.map((task) => ({ weight: weight(task), run: () => runCounted(ctx, task.cell, variant, task.runNo, task.withKill) })),
    ctx.jobs,
    () => ctx.halted === true,
  );
}

export function stopAll(ctx) {
  for (const pid of ctx.started ?? []) {
    signalGroup(pid, "SIGKILL");
    signal(pid, "SIGKILL");
  }
}
