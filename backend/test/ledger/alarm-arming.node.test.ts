import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { makeAccounts } from "../../src/accounts/index.ts";
import { ALARM_RETRY_LIMIT, type Host, makeEntries } from "../../src/do/entries.ts";
import { costOf } from "../../src/ledger/cost.ts";
import { makeLedger } from "../../src/ledger/index.ts";
import { makeMetering } from "../../src/metering/index.ts";
import { makeTestHost } from "../../test-kit/sql/node.ts";
import { type ChatRig, chatRig, chunk, upstreamOf } from "./chat-rig.ts";
import { type AlarmFault, failingPort, HOLD_LIFE_MS, injectedAlarmError, LOW, type Rig, seededRig } from "./harness.ts";

const T = Date.UTC(2031, 1, 3);
const WEEK_MS = 604_800_000;
const USAGE = { inputTokens: 7, outputTokens: 9 };
const USAGE_CHUNK = chunk(JSON.stringify({ choices: [], usage: { prompt_tokens: USAGE.inputTokens, completion_tokens: USAGE.outputTokens } }));
const BODY = { tier: "low_cost_hosted", max_tokens: 100, messages: [{ role: "user", content: "Synthetic alarm test." }] };
const ARMING_FAILED = '{"event":"alarm-error","error":"InjectedAlarmError"}';

let lines: string[] = [];
let open: { close(): void }[] = [];
beforeEach(() => {
  lines = [];
  open = [];
  vi.spyOn(console, "log").mockImplementation((line: unknown) => {
    lines.push(String(line));
  });
});
afterEach(() => {
  for (const each of open) each.close();
  vi.restoreAllMocks();
});

async function rigWith(fault: AlarmFault): Promise<Rig> {
  const rig = await seededRig({ cap: { amount: 10, period: "week" }, at: T, alarmFault: fault });
  open.push(rig);
  return rig;
}

const holdOf = (rig: Rig, id: string) => rig.read().holds.find((row) => row.request_id === id);

test("ruling B-48: when the host's alarm call throws after a step has committed, the step's answer stands, the failure is logged, and the alarm is armed again by the next step", async () => {
  const fault = { throws: false };
  const rig = await rigWith(fault);
  fault.throws = true;
  const reserve = await rig.run({ at: T, call: { kind: "reserve", requestId: "a", amount: 40_000 } });
  expect(reserve).toMatchObject({ outcome: "admitted" });
  expect(await rig.run({ at: T + 1, call: { kind: "dispatch", requestId: "a" } })).toBe("go");
  expect(holdOf(rig, "a")).toMatchObject({ state: "open", dispatched: 1 });
  expect(await rig.run({ at: T + 2, call: { kind: "settle", requestId: "a", usage: USAGE } })).toMatchObject({ outcome: "settled", repeat: false });
  expect(lines.filter((line) => line === ARMING_FAILED)).toHaveLength(3);
  expect(rig.alarmAt()).toBeNull();
  fault.throws = false;
  await rig.run({ at: T + 3, call: { kind: "reserve", requestId: "b", amount: 10_000 } });
  expect(rig.alarmAt()).toBe(T + 3 + 3000);
});

test("ruling B-48: a clock call whose clearing of the platform alarm throws still answers clock-set, with its test time stored", async () => {
  const rig = await rigWith({ throws: true });
  expect(await rig.entries().testSetClock(T + 10)).toEqual({ result: "clock-set" });
  const snapshot = await rig.entries().testInspect();
  expect(snapshot).toMatchObject({ result: "snapshot", test_time: T + 10 });
  expect(lines).toContain(ARMING_FAILED);
});

test("ruling B-48, a missed alarm: no due hold stays open past a step that takes the time; the next step ends the hold with the mark at its amount and the one without as voided", async () => {
  const fault = { throws: true };
  const rig = await rigWith(fault);
  await rig.run({ at: T, call: { kind: "reserve", requestId: "unmarked", amount: 40_000 } });
  await rig.run({ at: T, call: { kind: "reserve", requestId: "marked", amount: 50_000 } });
  expect(await rig.run({ at: T, call: { kind: "dispatch", requestId: "marked" } })).toBe("go");
  expect(rig.alarmAt()).toBeNull();
  fault.throws = false;
  expect(await rig.run({ at: T + 3000, call: { kind: "capGet" } })).toEqual({
    result: "cap",
    cap: { amount: 10, period: "week" },
    spend: { spent: 50_000, held: 0, period_end: T + WEEK_MS },
  });
  expect(holdOf(rig, "unmarked")).toMatchObject({ state: "voided", dispatched: 0, ended_at: T + 3000 });
  expect(holdOf(rig, "marked")).toMatchObject({ state: "expired", dispatched: 1, ended_at: T + 3000 });
});

async function alarmThatFails() {
  const fault = { throws: false };
  const made = makeTestHost({ port: failingPort, clock: () => T, config: { holdLifeMs: HOLD_LIFE_MS, testClock: true } });
  const host: Host = {
    ...made.host,
    setAlarm(at) {
      if (fault.throws) throw injectedAlarmError();
      made.host.setAlarm(at);
    },
  };
  const ledger = makeLedger({ holdLifeMs: HOLD_LIFE_MS });
  const failing = {
    ...ledger,
    expire(): number {
      throw new RangeError("a ledger that fails in the alarm");
    },
  };
  open.push(made);
  const entries = makeEntries(host, { accounts: makeAccounts(), ledger: failing, metering: makeMetering({ intervalSeconds: 3600, marginSeconds: 300 }) });
  expect(await entries.testSeed({ accountId: "6d1f0a2b-3c4e-4f5a-8b6c-7d8e9f0a1b2c", hash: "f".repeat(64), subscription: "active" })).toEqual({ result: "seeded" });
  fault.throws = true;
  const failures = () => made.host.step((sql) => sql.exec("SELECT alarm_failures FROM skeleton")[0]);
  return { entries, failures };
}

test("ruling B-48 in the alarm's own catch: a run whose ledger throws, on a host whose alarm call throws too, counts its failure, logs both and does not throw to the platform", async () => {
  const { entries, failures } = await alarmThatFails();
  await expect(entries.alarm()).resolves.toBeUndefined();
  expect(lines).toEqual(['{"event":"alarm-error","error":"RangeError"}', ARMING_FAILED]);
  expect(failures()).toEqual({ alarm_failures: 1 });
});

test("ruling B-51 (b), the alarm's stop path: at the retry limit a run whose ledger throws, on a host whose alarm call throws too, resolves, logs alarm-stopped and alarm-error, and keeps the count at the limit", async () => {
  const { entries, failures } = await alarmThatFails();
  for (let run = 1; run < ALARM_RETRY_LIMIT; run += 1) await expect(entries.alarm()).resolves.toBeUndefined();
  expect(failures()).toEqual({ alarm_failures: ALARM_RETRY_LIMIT - 1 });
  lines = [];
  await expect(entries.alarm()).resolves.toBeUndefined();
  expect(lines).toEqual(['{"event":"alarm-error","error":"RangeError"}', '{"event":"alarm-stopped","error":"RangeError"}', ARMING_FAILED]);
  expect(failures()).toEqual({ alarm_failures: ALARM_RETRY_LIMIT });
  lines = [];
  await expect(entries.alarm()).resolves.toBeUndefined();
  expect(lines).toEqual(['{"event":"alarm-error","error":"RangeError"}', '{"event":"alarm-stopped","error":"RangeError"}', ARMING_FAILED]);
  expect(failures()).toEqual({ alarm_failures: ALARM_RETRY_LIMIT });
});

test("ruling B-48, through chat(): a host whose alarm call throws in the dispatch step gives go with the mark and the upstream call, and the hold settles at the reported usage", async () => {
  const fault = { throws: false };
  const gateway: ChatRig = await chatRig({ at: T, alarmFault: fault });
  open.push(gateway);
  gateway.setUpstream(upstreamOf([USAGE_CHUNK, chunk("[DONE]")], "close"));
  gateway.before.dispatch = () => {
    fault.throws = true;
  };
  gateway.before.settle = () => {
    fault.throws = false;
  };
  const sent = await gateway.send("arming-fails-at-dispatch", BODY);
  expect(sent.status).toBe(200);
  expect(gateway.calls).toEqual(["reserve", "dispatch", "settle"]);
  expect(gateway.upstreamCalls()).toBe(1);
  expect(gateway.hold("arming-fails-at-dispatch")).toMatchObject({ state: "settled", dispatched: 1, cost: costOf(LOW, USAGE) });
  expect(lines).toContain(ARMING_FAILED);
});
