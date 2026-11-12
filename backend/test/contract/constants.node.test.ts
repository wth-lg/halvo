import { expect, test } from "vitest";
import { EVENT_ID_KEEP_MS, PENDING_DROP_MS, PENDING_KEEP_MS, SESSION_LENGTH_MS } from "../../src/accounts/index.ts";
import { ALARM_CALL_LIMIT, ALARM_RETRY_LIMIT, ALARM_RETRY_MS, CLOCK_RUN_LIMIT, START_RESTARTS } from "../../src/do/entries.ts";
import { REFUSAL_LIMIT, SEND_LIMIT_MS } from "../../src/metering/index.ts";
import { MICRO_PER_MINOR } from "../../src/types.ts";

test("the constants of contracts 3.1, 3.2 and the conventions have their contract values, and the entry files export them again", () => {
  expect(MICRO_PER_MINOR).toBe(10_000);
  expect(SESSION_LENGTH_MS).toBe(31 * 60 * 1000);
  expect(PENDING_DROP_MS).toBe(25 * 60 * 60 * 1000);
  expect(EVENT_ID_KEEP_MS).toBe(30 * 24 * 60 * 60 * 1000);
  expect(PENDING_KEEP_MS).toBe(EVENT_ID_KEEP_MS);
  expect(SEND_LIMIT_MS).toBe(35 * 24 * 60 * 60 * 1000);
  expect(REFUSAL_LIMIT).toBe(3);
  expect([ALARM_RETRY_MS, ALARM_RETRY_LIMIT, ALARM_CALL_LIMIT, START_RESTARTS, CLOCK_RUN_LIMIT]).toEqual([30_000, 5, 3, 2, 100]);
});
