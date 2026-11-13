import { afterEach, beforeEach, expect, test, vi } from "vitest";
import app from "../../src/app.ts";
import type { Env } from "../../src/types.ts";

const ACCOUNT = "7c1e2d3f-4a5b-4c6d-8e7f-0a1b2c3d4e5f";
const CREDENTIAL = "5d".repeat(32);
const HEADERS = { "halvo-account": ACCOUNT, authorization: `Bearer ${CREDENTIAL}` };
const CAP_ANSWER = { result: "cap", cap: { amount: 3, period: "month" }, spend: { spent: 10, held: 20, period_end: 30 } };

let lines: unknown[] = [];
beforeEach(() => {
  lines = [];
  vi.spyOn(console, "log").mockImplementation((text: unknown) => {
    lines.push(JSON.parse(String(text)));
  });
});
afterEach(() => {
  vi.restoreAllMocks();
});

function recordingEnv(answer: object) {
  const calls: unknown[][] = [];
  const object = {
    capGet: async (...args: unknown[]) => {
      calls.push(["capGet", ...args]);
      return answer;
    },
    capSet: async (...args: unknown[]) => {
      calls.push(["capSet", ...args]);
      return answer;
    },
  };
  const env = {
    ACCOUNT: {
      idFromName: (name: string) => {
        calls.push(["idFromName", name]);
        return `id-of-${name}`;
      },
      get: (id: string) => {
        calls.push(["get", id]);
        return object;
      },
    },
  } as unknown as Env;
  return { env, calls };
}

async function sha256(text: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
  return [...new Uint8Array(digest)].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

test("the default app sends GET /v1/cap to capGet of the account's object and returns its answer unchanged", async () => {
  const { env, calls } = recordingEnv(CAP_ANSWER);
  const response = await app(new Request("http://backend.test/v1/cap", { headers: HEADERS }), env);
  expect([response.status, await response.json()]).toEqual([200, CAP_ANSWER]);
  expect(calls).toEqual([["idFromName", ACCOUNT], ["get", `id-of-${ACCOUNT}`], ["capGet", await sha256(CREDENTIAL)]]);
  expect(lines).toEqual([]);
});

test("the default app sends PUT /v1/cap with its cap to capSet of the account's object and returns its answer unchanged", async () => {
  const { env, calls } = recordingEnv(CAP_ANSWER);
  const body = '{"cap":{"amount":3,"period":"month"}}';
  const response = await app(new Request("http://backend.test/v1/cap", { method: "PUT", headers: { ...HEADERS, "content-type": "application/json" }, body }), env);
  expect([response.status, await response.json()]).toEqual([200, CAP_ANSWER]);
  expect(calls).toEqual([["idFromName", ACCOUNT], ["get", `id-of-${ACCOUNT}`], ["capSet", await sha256(CREDENTIAL), { amount: 3, period: "month" }]]);
});

test("each answer of the cap entries passes the router, and an answer outside the route's set is a defect", async () => {
  for (const answer of [{ result: "no-cap" }, { result: "pending" }, { result: "no-such-account" }, { result: "credential-mismatch" }]) {
    const response = await app(new Request("http://backend.test/v1/cap", { headers: HEADERS }), recordingEnv(answer).env);
    expect([response.status, await response.json()]).toEqual([200, answer]);
  }
  const wrong = await app(new Request("http://backend.test/v1/cap", { headers: HEADERS }), recordingEnv({ result: "session", url: "http://127.0.0.1:1/x" }).env);
  expect(wrong.status).toBe(500);
});
