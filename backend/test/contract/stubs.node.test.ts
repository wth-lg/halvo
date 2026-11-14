import { expect, test } from "vitest";
import { makeAccounts } from "../../src/accounts/index.ts";
import { makeLedger } from "../../src/ledger/index.ts";
import { makeMetering } from "../../src/metering/index.ts";
import { openSql } from "../../test-kit/sql/node.ts";

const ACCOUNT = "0f8e2d4c-6b1a-4c3e-8d2f-1a2b3c4d5e6f";
const HASH = "c".repeat(64);
const EVENT = {
  kind: "checkout-completed" as const,
  eventId: "evt",
  providerTime: 1,
  accountId: ACCOUNT,
  sessionId: "cs",
  customerId: "cus",
  subscriptionId: null,
  payment: "paid" as const,
};

function nameOfThrow(run: () => unknown): string | null {
  try {
    run();
  } catch (error) {
    return (error as Error).name;
  }
  return null;
}

test("the accounts module keeps a seeded account's id, hash, state and customer id, and refuses a pending checkout beside it", () => {
  const db = openSql();
  const accounts = makeAccounts();
  db.step((sql) => {
    accounts.init(sql);
    expect(accounts.look(sql, HASH)).toBe("nothing");
    expect(accounts.subscription(sql)).toBeNull();
    expect(accounts.isActive(sql)).toBe(false);
    accounts.seed(sql, { accountId: ACCOUNT, credentialHash: HASH, state: "active", customerId: "cus_1" }, 5);
    expect(accounts.look(sql, HASH)).toBe("account");
    expect(accounts.look(sql, "d".repeat(64))).toBe("mismatch");
    expect(accounts.subscription(sql)).toEqual({ accountId: ACCOUNT, state: "active", customerId: "cus_1", subscriptionId: null });
    expect(accounts.isActive(sql)).toBe(true);
    expect(accounts.pending(sql)).toBeNull();
    expect(accounts.wake(sql, 5)).toBeNull();
    expect(accounts.nextDue(sql)).toBeNull();
    expect(accounts.forget(sql, 5)).toBe(0);
    expect(accounts.applyEvent(sql, EVENT, 5)).toEqual({ kind: "none", overrode: false });
    expect(accounts.complete(sql, { sessionId: "cs", customerId: "cus", subscriptionId: null, payment: "paid" })).toEqual({ kind: "none", overrode: false });
    expect(accounts.record(sql, "cs", "open", 5)).toBe("stale");
    expect(nameOfThrow(() => accounts.keepPending(sql, { accountId: ACCOUNT, credentialHash: HASH, cap: { amount: 1, period: "week" }, sessionId: "cs", url: "u", expiresAt: 1 }, 5))).toBe(
      "ContractBrokenError",
    );
    expect(nameOfThrow(() => accounts.replacePendingCap(sql, { amount: 1, period: "week" }))).toBe("ContractBrokenError");
    expect(sql.exec("SELECT event_id, received_at FROM processed_event")).toEqual([{ event_id: "evt", received_at: 5 }]);
  });
  db.step((sql) => {
    const seeded = makeAccounts();
    sql.exec("UPDATE account SET state = ?, customer_id = ?", "payment-failed", null);
    expect(seeded.isActive(sql)).toBe(false);
    expect(seeded.subscription(sql)).toEqual({ accountId: ACCOUNT, state: "payment-failed", customerId: null, subscriptionId: null });
  });
  db.close();
});

test("the ledger keeps the cap as last set and answers the same calls as contracts 3.2 and 4 say (P4 replaced the stub)", () => {
  const db = openSql();
  const ledger = makeLedger({ holdLifeMs: 3000 });
  db.step((sql) => {
    const week = 604_800_000;
    ledger.init(sql);
    expect(ledger.cap(sql, 10)).toBeNull();
    ledger.setCap(sql, { amount: 5, period: "week" }, 10);
    ledger.setCap(sql, { amount: 7, period: "month" }, 20, 99);
    expect(ledger.cap(sql, 30)).toEqual({ cap: { amount: 7, period: "month" }, spent: 0, held: 0, periodEnd: 10 + week });
    const req = { requestId: "r", tier: "frontier" as const, amount: 1, price: { input: 1, output: 1 } };
    expect(ledger.reserve(sql, req, 30, { active: true, interval: 0 })).toEqual({ outcome: "admitted", receipt: { requestId: "r", amount: 1, now: 30, expiresAt: 3030 } });
    expect(ledger.dispatch(sql, "r", 30)).toBe("go");
    expect(ledger.settle(sql, "r", { inputTokens: 1, outputTokens: 1 }, 30)).toEqual({ outcome: "settled", cost: 1, overrun: 0, repeat: false });
    expect(ledger.void(sql, "r", 30)).toEqual({ outcome: "refused", reason: "already-ended" });
    expect(ledger.expire(sql, 30)).toBe(0);
    expect(ledger.intervalTotals(sql, 0)).toEqual({ reported: 1, unreported: 0, open: 0 });
    expect(ledger.openHolds(sql)).toBe(0);
    expect(ledger.nextDue(sql)).toBeNull();
    expect(sql.exec("SELECT amount, period FROM ledger_cap")).toEqual([{ amount: 7, period: "week" }]);
  });
  db.close();
});

test("the metering module answers every call of contract 3.2 on test-kit/sql (P5 replaced the stub)", () => {
  const db = openSql();
  const metering = makeMetering({ intervalSeconds: 3600, marginSeconds: 300 });
  db.step((sql) => {
    metering.init(sql);
    metering.begin(sql, ACCOUNT, 1);
    metering.billingPeriod(sql, { start: 0, end: 7_200_000 }, 1);
    expect(metering.intervalAt(sql, 1_000)).toBe(0);
    expect(metering.oldestUnsealed(sql)).toEqual({ interval: 0, end: 3_600_000 });
    metering.seal(sql, 0, 25_000, 3_600_000);
    expect(metering.nextSend(sql)).toBe(3_600_000);
    const next = metering.send(sql, 3_600_000);
    expect(next).toEqual({ call: { kind: "send", interval: 0, identifier: `${ACCOUNT}:0`, amountMinor: 2, timestamp: 3_599 }, notBillable: 0 });
    if (next.call === null) throw new Error("the module named no call");
    expect(metering.record(sql, next.call, { kind: "recorded" }, 3_600_000)).toEqual({ call: null, notBillable: 0 });
    metering.cut(sql, "cancel", 3_600_000, 3_600_000);
    expect(metering.settled(sql)).toBe(true);
  });
  db.close();
});
