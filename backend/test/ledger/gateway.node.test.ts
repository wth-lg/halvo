import { mkdirSync, mkdtempSync, openSync, readFileSync } from "node:fs";
import { createServer } from "node:net";
import { join } from "node:path";
import { afterAll, beforeAll, expect, test } from "vitest";
import { costOf } from "../../src/ledger/cost.ts";
import { endGroup, type Group, startGroup } from "./process-group.ts";

const ROOT = process.cwd();
const PRICE = { input: 3_100_000, output: 15_700_000 };
const SWITCHES: [string, string | null][] = [
  ["HALVO_WRANGLER_CONFIG_HOME", null],
  ["CLOUDFLARE_CF_FETCH_ENABLED", "false"],
  ["WRANGLER_SEND_METRICS", "false"],
  ["WRANGLER_SEND_ERROR_REPORTS", "false"],
  ["WRANGLER_HIDE_BANNER", "true"],
];
const pause = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

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

let worker: Group | null = null;
let mock: Group | null = null;
let base = "";
let mockBase = "";
let logPath = "";

beforeAll(async () => {
  const missing = SWITCHES.find(([name, value]) => {
    const set = process.env[name];
    return set === undefined || set === "" || (value !== null && set !== value);
  });
  if (missing !== undefined) throw new Error(`run this test with pnpm test: ${missing[0]} is not set as scripts/env.sh sets it`);
  mkdirSync(join(ROOT, ".tmp"), { recursive: true });
  const dir = mkdtempSync(join(ROOT, ".tmp", "gateway-test."));
  const upstream = startGroup(process.execPath, [join(ROOT, "stress/upstream.mjs"), join(dir, "mock.jsonl")], { stdio: ["ignore", "pipe", "inherit"] });
  mock = upstream;
  const mockPort = await new Promise<number>((resolve, reject) => {
    let text = "";
    upstream.child.stdout?.on("data", (part) => {
      text += String(part);
      if (text.includes("\n")) resolve(JSON.parse(text.slice(0, text.indexOf("\n"))).port);
    });
    upstream.child.on("exit", () => reject(new Error("the mock upstream exited")));
  });
  mockBase = `http://127.0.0.1:${mockPort}`;
  const port = await freePort();
  const inspector = await freePort();
  base = `http://127.0.0.1:${port}`;
  logPath = join(dir, "wrangler.log");

  const log = openSync(logPath, "a");
  worker = startGroup(
    join(ROOT, "node_modules/.bin/wrangler"),
    [
      "dev",
      "--config",
      join(ROOT, "wrangler.test.jsonc"),
      "--ip",
      "127.0.0.1",
      "--port",
      String(port),
      "--inspector-port",
      String(inspector),
      "--persist-to",
      join(dir, "state"),
      "--show-interactive-dev-session=false",
      "--var",
      `UPSTREAM_BASE_URL:${mockBase}`,
      "--var",
      `PUBLIC_ORIGIN:${base}`,
    ],
    { stdio: ["ignore", log, log], env: { ...process.env, XDG_CONFIG_HOME: process.env.HALVO_WRANGLER_CONFIG_HOME } },
  );
  for (let tries = 0; ; tries += 1) {
    const ready = await fetch(`${base}/test/inspect`, { method: "POST", body: JSON.stringify({ account: "00000000-0000-4000-8000-000000000000" }) }).then(
      (response) => response.status === 200,
      () => false,
    );
    if (ready) break;
    if (tries > 600) throw new Error(`the test entry did not answer; see ${logPath}`);
    await pause(100);
  }
}, 90_000);

afterAll(async () => {
  await endGroup(worker);
  await endGroup(mock);
}, 30_000);

let counter = 0;
function freshAccount(): { account: string; credential: string; headers: Record<string, string> } {
  counter += 1;
  const hex = (Date.now() * 1000 + counter).toString(16).padStart(12, "0").slice(-12);
  const account = `5e1d2c3b-4a59-4687-9a6b-${hex}`;
  const credential = hex.padStart(64, "a");
  return { account, credential, headers: { "halvo-account": account, authorization: `Bearer ${credential}`, "content-type": "application/json" } };
}

async function seed(who: ReturnType<typeof freshAccount>, subscription = "active", amount = 100): Promise<void> {
  const answer = await fetch(`${base}/test/seed`, {
    method: "POST",
    body: JSON.stringify({ account: who.account, credential: who.credential, subscription, cap: { amount, period: "week" } }),
  }).then((response) => response.json());
  expect(answer).toEqual({ result: "seeded" });
}

async function holds(account: string): Promise<Record<string, unknown>[]> {
  const response = await fetch(`${base}/test/inspect`, { method: "POST", body: JSON.stringify({ account }) });
  const snapshot = (await response.json()) as { tables?: { ledger_holds?: Record<string, unknown>[] } };
  return snapshot.tables?.ledger_holds ?? [];
}

const body = (mock: object, extra: object = {}) =>
  JSON.stringify({ tier: "low_cost_hosted", max_tokens: 640, stream: true, messages: [{ role: "user", content: "Synthetic gateway test." }], mock, ...extra });
const USAGE_LAST = { switch: "usage-last", shape: "openai", chunks: 6, chunk_ms: 100, input_tokens: 40, output_tokens: 300 };

test("a client that leaves mid-stream: the gateway reads the upstream answer to its end and settles the reported amount", async () => {
  const who = freshAccount();
  await seed(who);
  const sent = body(USAGE_LAST);
  const controller = new AbortController();
  const response = await fetch(`${base}/v1/chat`, { method: "POST", headers: { ...who.headers, "x-request-id": `leave-${counter}` }, body: sent, signal: controller.signal });
  expect(response.status).toBe(200);
  const reader = response.body?.getReader();
  await reader?.read();
  controller.abort();
  await pause(1500);
  const [row] = await holds(who.account);
  expect(row).toMatchObject({ state: "settled", cost: costOf(PRICE, { inputTokens: 40, outputTokens: 300 }), amount: costOf(PRICE, { inputTokens: new TextEncoder().encode(sent).byteLength, outputTokens: 640 }), dispatched: 1 });
}, 30_000);

test("the object's refusals are 403 with the closed body; a repeated request id is 409 and dispatches nothing more", async () => {
  const failed = freshAccount();
  await seed(failed, "payment-failed");
  const notActive = await fetch(`${base}/v1/chat`, { method: "POST", headers: failed.headers, body: body(USAGE_LAST) });
  expect([notActive.status, await notActive.json()]).toEqual([403, { result: "refused", reason: "not-active" }]);
  const nobody = freshAccount();
  const none = await fetch(`${base}/v1/chat`, { method: "POST", headers: nobody.headers, body: body(USAGE_LAST) });
  expect([none.status, await none.json()]).toEqual([403, { result: "refused", reason: "no-such-account" }]);
  const who = freshAccount();
  await seed(who, "active", 1);
  const mismatch = await fetch(`${base}/v1/chat`, { method: "POST", headers: { ...who.headers, authorization: `Bearer ${"f".repeat(64)}` }, body: body(USAGE_LAST) });
  expect([mismatch.status, await mismatch.json()]).toEqual([403, { result: "refused", reason: "credential-mismatch" }]);
  const over = await fetch(`${base}/v1/chat`, { method: "POST", headers: who.headers, body: body(USAGE_LAST, { max_tokens: 8192 }) });
  expect([over.status, await over.json()]).toEqual([403, { result: "refused", reason: "over-cap" }]);
  const once = await fetch(`${base}/v1/chat`, { method: "POST", headers: { ...who.headers, "x-request-id": "repeat-me" }, body: body({ ...USAGE_LAST, chunk_ms: 1 }, { max_tokens: 100 }) });
  expect(once.status).toBe(200);
  await once.text();
  const again = await fetch(`${base}/v1/chat`, { method: "POST", headers: { ...who.headers, "x-request-id": "repeat-me" }, body: body({ ...USAGE_LAST, chunk_ms: 1 }, { max_tokens: 100 }) });
  expect([again.status, await again.json()]).toEqual([409, { result: "repeat", state: "settled", dispatched: true }]);
}, 30_000);

test("a body that is not text parts only, too large, a query string or a bad request id is rejected before reserve and logged as rejected", async () => {
  const who = freshAccount();
  await seed(who);
  const cases: [string, RequestInit, number][] = [
    ["/v1/chat", { body: body(USAGE_LAST, { messages: [{ role: "user", content: [{ type: "image_url", image_url: { url: "http://127.0.0.1:1/x" } }] }] }) }, 400],
    ["/v1/chat", { body: body(USAGE_LAST, { tools: [{ type: "web_search" }] }) }, 400],
    ["/v1/chat", { body: body(USAGE_LAST, { messages: [{ role: "user", content: "x".repeat(1_048_577) }] }) }, 413],
    ["/v1/chat?x=1", { body: body(USAGE_LAST) }, 400],
    ["/v1/chat", { body: body(USAGE_LAST), headers: { ...who.headers, "x-request-id": "not ok!" } }, 400],
    ["/v1/chat", { body: body(USAGE_LAST), headers: { ...who.headers, "content-type": "text/plain" } }, 400],
  ];
  for (const [path, init, status] of cases) {
    const response = await fetch(`${base}${path}`, { method: "POST", headers: who.headers, ...init });
    expect({ path, status: response.status, body: await response.text() }).toEqual({ path, status, body: '{"result":"rejected"}' });
  }
  expect(await holds(who.account)).toEqual([]);
  await pause(300);
  const logged = readFileSync(logPath, "utf8")
    .split("\n")
    .filter((line) => line.trim().startsWith('{"event":"rejected","route":"none"'));
  expect(logged.length).toBeGreaterThanOrEqual(cases.length);
}, 30_000);

test("an upstream error with no usage is answered 502 and its hold expires in full", async () => {
  const who = freshAccount();
  await seed(who);
  const response = await fetch(`${base}/v1/chat`, { method: "POST", headers: who.headers, body: body({ ...USAGE_LAST, switch: "error-status" }) });
  expect([response.status, ((await response.json()) as { result: string }).result]).toEqual([502, "upstream-failed"]);
  expect((await holds(who.account))[0]).toMatchObject({ state: "open", dispatched: 1 });
  await pause(4000);
  expect((await holds(who.account))[0]).toMatchObject({ state: "expired", ended_by: "expire", cost: null });
  expect(mockBase.startsWith("http://127.0.0.1:")).toBe(true);
}, 30_000);
