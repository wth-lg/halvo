import { spawn } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { afterAll, beforeAll, expect, test } from "vitest";
import { chat } from "../../src/gateway/chat.ts";
import type { Env } from "../../src/types.ts";

const ROOT = process.cwd();
const ACCOUNT = "6f1e2d3c-4b5a-4968-8776-5a4b3c2d1e0f";
const CREDENTIAL = "4e".repeat(32);
const REPLY_DELAY_MS = 600;
const LIFE_MS = 1500;
const MARGIN_MS = 200;
const pause = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

let mock: { kill(signal?: string): boolean } | null = null;
let mockBase = "";
let mockLog = "";

beforeAll(async () => {
  mkdirSync(join(ROOT, ".tmp"), { recursive: true });
  const dir = mkdtempSync(join(ROOT, ".tmp", "gateway-chat-test."));
  mockLog = join(dir, "mock.jsonl");
  const child = spawn(process.execPath, [join(ROOT, "stress/upstream.mjs"), mockLog], { stdio: ["ignore", "pipe", "inherit"] });
  mock = child;
  const port = await new Promise<number>((resolve, reject) => {
    let text = "";
    child.stdout.on("data", (part) => {
      text += String(part);
      if (text.includes("\n")) resolve(JSON.parse(text.slice(0, text.indexOf("\n"))).port);
    });
    child.on("exit", () => reject(new Error("the mock upstream exited")));
  });
  mockBase = `http://127.0.0.1:${port}`;
});

afterAll(() => {
  mock?.kill("SIGTERM");
});

test("a slow reserve reply does not move the cut past the hold: the upstream is cut DISPATCH_MARGIN_MS before expiresAt, counted from the reserve's send", async () => {
  let expiresAt = 0;
  const object = {
    async reserve(_hash: string, req: { requestId: string; amount: number }) {
      const now = Date.now();
      expiresAt = now + LIFE_MS;
      await pause(REPLY_DELAY_MS);
      return { outcome: "admitted", receipt: { requestId: req.requestId, amount: req.amount, now, expiresAt } };
    },
    dispatch: async () => "go",
    settle: async () => ({ outcome: "settled", cost: 1, overrun: 0, repeat: false }),
    void: async () => ({ outcome: "voided" }),
  };
  const env = {
    ACCOUNT: { idFromName: () => "id", get: () => object },
    UPSTREAM_BASE_URL: mockBase,
    UPSTREAM_DEADLINE_MS: "100",
    DISPATCH_MARGIN_MS: String(MARGIN_MS),
  } as unknown as Env;
  const waits: Promise<unknown>[] = [];
  const ctx = { waitUntil: (promise: Promise<unknown>) => waits.push(promise) } as unknown as ExecutionContext;
  const body = JSON.stringify({
    tier: "low_cost_hosted",
    max_tokens: 100,
    messages: [{ role: "user", content: "Synthetic slow reply test." }],
    mock: { switch: "usage-last", shape: "openai", chunks: 40, chunk_ms: 100, input_tokens: 4, output_tokens: 5 },
  });
  const response = await chat(
    new Request("http://backend.test/v1/chat", {
      method: "POST",
      headers: { "halvo-account": ACCOUNT, authorization: `Bearer ${CREDENTIAL}`, "content-type": "application/json", "x-request-id": "slow-reply" },
      body,
    }),
    env,
    ctx,
  );
  expect(response.status).toBe(200);
  const text = await response.text();
  await Promise.all(waits);
  expect(text).toContain('"settled":false');
  await pause(100);
  const end = readFileSync(mockLog, "utf8")
    .split("\n")
    .filter(Boolean)
    .map((line) => JSON.parse(line))
    .find((event) => event.id === "slow-reply" && event.ev === "end");
  const cutBeforeExpiry = expiresAt - end.t;
  expect(cutBeforeExpiry).toBeGreaterThanOrEqual(MARGIN_MS - 80);
  expect(cutBeforeExpiry).toBeLessThanOrEqual(MARGIN_MS + 120);
});
