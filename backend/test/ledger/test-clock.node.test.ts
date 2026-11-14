import { expect, test } from "vitest";
import { testPaths } from "../../src/gateway/test-clock.ts";
import type { Env } from "../../src/types.ts";

const ACCOUNT = "5c1e9a2b-7d3f-4a6e-9b8c-2f4e6a8c0d1e";
const CREDENTIAL = "a".repeat(64);
const DATE_MAX_MS = 8_640_000_000_000_000;

function recordingEnv() {
  const calls: unknown[] = [];
  const object = {
    testSetClock: async (time: number) => {
      calls.push({ clock: time });
      return { result: "clock-set" };
    },
    testSeed: async (seed: Record<string, unknown>) => {
      calls.push({ seed: { time: seed.time, anchor: seed.anchor } });
      return { result: "seeded" };
    },
  };
  const env = { ACCOUNT: { idFromName: (name: string) => name, get: () => object } } as unknown as Env;
  return { env, calls };
}

const post = (env: Env, path: string, body: object) => testPaths(new Request(`http://t${path}`, { method: "POST", body: JSON.stringify(body) }), env);
const seed = (extra: object) => ({ account: ACCOUNT, credential: CREDENTIAL, subscription: "active", ...extra });

test("unit 4.5 (contract 5, review F3): a clock call and a seed with a test time or an anchor above 8,640,000,000,000,000 ms are answered 400 rejected, and the object is not called", async () => {
  const { env, calls } = recordingEnv();
  for (const above of [DATE_MAX_MS + 1, Number.MAX_SAFE_INTEGER]) {
    for (const [path, body] of [
      ["/test/clock", { account: ACCOUNT, time: above }],
      ["/test/seed", seed({ time: above })],
      ["/test/seed", seed({ anchor: above })],
    ] as const) {
      const response = await post(env, path, body);
      expect({ path, body, status: response?.status, type: response?.headers.get("content-type"), text: await response?.text() }).toEqual({
        path,
        body,
        status: 400,
        type: "application/json",
        text: '{"result":"rejected"}',
      });
    }
  }
  expect(calls).toEqual([]);
});

test("unit 4.5: a clock call and a seed at 8,640,000,000,000,000 ms, the largest time that a Date holds, reach the object", async () => {
  const { env, calls } = recordingEnv();
  const clock = await post(env, "/test/clock", { account: ACCOUNT, time: DATE_MAX_MS });
  expect([clock?.status, await clock?.json()]).toEqual([200, { result: "clock-set" }]);
  const seeded = await post(env, "/test/seed", seed({ time: DATE_MAX_MS, anchor: DATE_MAX_MS }));
  expect([seeded?.status, await seeded?.json()]).toEqual([200, { result: "seeded" }]);
  expect(calls).toEqual([{ clock: DATE_MAX_MS }, { seed: { time: DATE_MAX_MS, anchor: DATE_MAX_MS } }]);
});
