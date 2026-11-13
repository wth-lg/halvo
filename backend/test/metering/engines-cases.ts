import { makeAccounts } from "../../src/accounts/index.ts";
import type { ProviderEvent } from "../../src/billing/port.ts";
import { makeMetering, type UsageCall } from "../../src/metering/index.ts";
import type { Step } from "../../src/types.ts";

const ACCOUNT = "00000000-0000-4000-8000-0000000000e1";
const H = 3_600_000;
const T0 = 1_800_000_017_400;

export interface EngineCase {
  name: string;
  run(step: Step): unknown;
}

function dump(step: Step): unknown {
  return step((sql) => ({
    account: sql.exec("SELECT * FROM metering_account").map((row) => ({ ...row })),
    intervals: sql.exec("SELECT * FROM metering_interval ORDER BY start").map((row) => ({ ...row })),
  }));
}

export const engineCases: EngineCase[] = [
  {
    name: "the sync's statements: intervals, a billing period, seals, every answer of a send and a read, the limit and settled",
    run(step) {
      const metering = makeMetering({ intervalSeconds: 3600, marginSeconds: 300 });
      const seen: [string, unknown][] = [];
      const note = (label: string, value: unknown) => seen.push([label, value]);
      step((sql) => {
        metering.init(sql);
        metering.begin(sql, ACCOUNT, T0);
        metering.begin(sql, "ignored", T0 + 5);
        note("intervalAt first", metering.intervalAt(sql, T0 + 10));
        note("intervalAt same", metering.intervalAt(sql, T0 + H - 500));
        metering.billingPeriod(sql, { start: T0 - 400 + H / 2, end: T0 - 400 + 30 * 24 * H }, T0 + 20);
        metering.billingPeriod(sql, { start: T0 - 400 + H / 2, end: T0 - 400 + 30 * 24 * H }, T0 + 20);
        note("oldestUnsealed", metering.oldestUnsealed(sql));
        note("intervalAt after the period", metering.intervalAt(sql, T0 + H));
        note("send before any seal", metering.send(sql, T0 + H));
        note("settled with unsealed", metering.settled(sql));
      });
      const first = step((sql) => metering.oldestUnsealed(sql));
      step((sql) => {
        if (first === null) throw new Error("no interval");
        metering.seal(sql, first.interval, 5_000, first.end - 1);
        metering.seal(sql, first.interval + 1, 9_999, first.end);
        metering.seal(sql, first.interval, 5_000, first.end);
        note("oldestUnsealed after the zero seal", metering.oldestUnsealed(sql));
        note("nextSend with nothing pending", metering.nextSend(sql));
      });
      const second = step((sql) => metering.oldestUnsealed(sql));
      let call: UsageCall | null = null;
      step((sql) => {
        if (second === null) throw new Error("no interval");
        metering.seal(sql, second.interval, 2_495_000, second.end);
        note("nextSend after the seal", metering.nextSend(sql));
        const next = metering.send(sql, second.end);
        call = next.call;
        note("send", next);
        note("send again before the retry", metering.send(sql, second.end + 1));
      });
      step((sql) => {
        if (call === null) throw new Error("no call");
        note("record failed", metering.record(sql, call, { kind: "failed" }, second?.end ?? 0));
        note("record refused after failed", metering.record(sql, call, { kind: "refused", reason: "unknown_account" }, (second?.end ?? 0) + 60_000));
      });
      step((sql) => {
        const read: UsageCall = { kind: "read", interval: second?.interval ?? 0, from: 0, to: 1 };
        note("record a read that explains nothing", metering.record(sql, read, { kind: "total", from: 0, to: 1, totalMinor: 7 }, (second?.end ?? 0) + 60_000));
        note("record a read that lacks the amount", metering.record(sql, read, { kind: "total", from: 0, to: 1, totalMinor: 0 }, (second?.end ?? 0) + 60_000));
        note("record a refusal", metering.record(sql, call ?? read, { kind: "refused", reason: "unknown_account" }, (second?.end ?? 0) + 60_000));
        note("send after the retry", metering.send(sql, (second?.end ?? 0) + 120_000));
        note("record duplicate", metering.record(sql, call ?? read, { kind: "duplicate" }, (second?.end ?? 0) + 120_000));
        note("record after the outcome", metering.record(sql, call ?? read, { kind: "recorded" }, (second?.end ?? 0) + 120_000));
        note("record a stale refusal after the outcome", metering.record(sql, call ?? read, { kind: "refused", reason: "timestamp_too_old" }, (second?.end ?? 0) + 130_000));
        note("settled", metering.settled(sql));
      });
      step((sql) => {
        const at = (second?.end ?? 0) + 200_000;
        note("intervalAt third", metering.intervalAt(sql, at));
        metering.cut(sql, "cancel", at + 50_500, at + 60_000);
        metering.cut(sql, "cancel", at + 40_700, at + 70_000);
        metering.cut(sql, "cancel", at + 90_000, at + 80_000);
        metering.cut(sql, "delete", at + 100_000, at + 100_000);
        const third = metering.oldestUnsealed(sql);
        if (third === null) throw new Error("no interval");
        metering.seal(sql, third.interval, 30_000, at + 100_000);
        const next = metering.send(sql, at + 100_000 + 35 * 24 * H);
        note("send past the limit", next);
        note("settled at the end", metering.settled(sql));
      });
      return { seen, tables: dump(step) };
    },
  },
  {
    name: "the accounts module's statements for the cancel's time: kept events, the account made from them, a later and an earlier cancel event, and the seed",
    run(step) {
      const accounts = makeAccounts();
      const cap = { amount: 10, period: "week" as const };
      const event = (eventId: string, providerTime: number, state: "active" | "cancelled", endedAt: number | null): ProviderEvent => ({
        kind: "subscription",
        eventId,
        providerTime,
        accountId: ACCOUNT,
        subscriptionId: "sub_1",
        customerId: "cus_1",
        state,
        period: null,
        endedAt,
      });
      const seen: [string, unknown][] = [];
      step((sql) => {
        accounts.init(sql);
        accounts.keepPending(sql, { accountId: ACCOUNT, credentialHash: "h", cap, sessionId: "cs_1", url: "http://127.0.0.1:1/x", expiresAt: T0 }, T0);
        seen.push(["kept, a cancel", accounts.applyEvent(sql, event("e1", T0 + 20, "cancelled", null), T0)]);
        seen.push(["kept, an earlier cancel that is not newer", accounts.applyEvent(sql, event("e2", T0 + 10, "cancelled", null), T0)]);
        seen.push(["kept, a newer cancel", accounts.applyEvent(sql, event("e3", T0 + 30, "cancelled", null), T0)]);
        seen.push(["kept rows", sql.exec("SELECT * FROM kept_event").map((row) => ({ ...row }))]);
        seen.push(["complete", accounts.complete(sql, { sessionId: "cs_1", customerId: "cus_1", subscriptionId: "sub_1", payment: "paid" })]);
        seen.push(["subscription", accounts.subscription(sql)]);
        seen.push(["an earlier cancel that is not newer", accounts.applyEvent(sql, event("e4", T0 + 5, "cancelled", null), T0)]);
        seen.push(["the same time again", accounts.applyEvent(sql, event("e5", T0 + 6, "cancelled", null), T0)]);
        seen.push(["a stated end", accounts.applyEvent(sql, event("e6", T0 + 40, "cancelled", T0 + 2), T0)]);
        seen.push(["subscription after the stated end", accounts.subscription(sql)]);
        seen.push(["account row", sql.exec("SELECT * FROM account").map((row) => ({ ...row }))]);
      });
      return seen;
    },
  },
  {
    name: "the accounts module's seed writes no cancel time",
    run(step) {
      const accounts = makeAccounts();
      return step((sql) => {
        accounts.init(sql);
        accounts.seed(sql, { accountId: ACCOUNT, credentialHash: "h", state: "cancelled", customerId: "cus_1" }, T0);
        return { subscription: accounts.subscription(sql), rows: sql.exec("SELECT * FROM account").map((row) => ({ ...row })) };
      });
    },
  },
];
