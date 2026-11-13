import { readFileSync } from "node:fs";
import { expect, test } from "vitest";
import { PROVIDER_CALL_TIMEOUT_MS } from "../../src/billing/index.ts";
import { makeMetering, SYNC_RETRY_MS, UNIQUENESS_WINDOW_MS } from "../../src/metering/index.ts";

const vars = (path: string): Record<string, string> => JSON.parse(readFileSync(path, "utf8")).vars;

test("S9's second limit: the margin in both wrangler files covers the provider call's timeout, and the retry delay outlasts one call", () => {
  for (const path of ["wrangler.jsonc", "wrangler.test.jsonc"]) {
    const margin = Number(vars(path).SYNC_MARGIN_SECONDS) * 1000;
    expect({ path, margin, covers: margin > PROVIDER_CALL_TIMEOUT_MS }).toEqual({ path, margin: 300_000, covers: true });
  }
  expect(SYNC_RETRY_MS).toBeGreaterThan(PROVIDER_CALL_TIMEOUT_MS);
});

test("the factory refuses an interval under one second and a margin that is not under the uniqueness window", () => {
  const refused = (config: { intervalSeconds: number; marginSeconds: number }) => {
    try {
      makeMetering(config);
      return "made";
    } catch (error) {
      return (error as Error).name;
    }
  };
  expect([
    refused({ intervalSeconds: 0, marginSeconds: 300 }),
    refused({ intervalSeconds: 3600, marginSeconds: UNIQUENESS_WINDOW_MS / 1000 }),
    refused({ intervalSeconds: 3600, marginSeconds: -1 }),
    refused({ intervalSeconds: 3600, marginSeconds: 300 }),
  ]).toEqual(["RangeError", "RangeError", "RangeError", "made"]);
});
