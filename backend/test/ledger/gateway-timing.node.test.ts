import { expect, test } from "vitest";
import { timingOf } from "../../src/gateway/timing.ts";

const RECEIPT = { now: 10_000, expiresAt: 13_000 };

test("the upstream call is cut DISPATCH_MARGIN_MS before the receipt's expiry, counted on the gateway's clock from the moment it sent the reserve call", () => {
  expect(timingOf(RECEIPT, 500, 500, 1500, 400).cutAt).toBe(500 + 3000 - 400);
  expect(timingOf(RECEIPT, 500, 900, 1500, 400).cutAt).toBe(500 + 3000 - 400);
});

test("the gateway voids when the time left is under the deadline plus the margin, and dispatches at equality", () => {
  expect(timingOf(RECEIPT, 500, 500, 1500, 400).voidIt).toBe(false);
  expect(timingOf(RECEIPT, 500, 500 + 1100, 1500, 400).voidIt).toBe(false);
  expect(timingOf(RECEIPT, 500, 500 + 1101, 1500, 400).voidIt).toBe(true);
  expect(timingOf({ now: 0, expiresAt: 1700 }, 0, 0, 1500, 400).voidIt).toBe(true);
  expect(timingOf({ now: 0, expiresAt: 1900 }, 0, 0, 1500, 400).voidIt).toBe(false);
});
