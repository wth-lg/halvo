import { afterEach, describe, expect, test, vi } from "vitest";
import { type Accounts, EVENT_ID_KEEP_MS, makeAccounts, PENDING_DROP_MS, PENDING_KEEP_MS, SESSION_LENGTH_MS } from "../../src/accounts/index.ts";
import type { ProviderEvent } from "../../src/billing/port.ts";
import { ownTables } from "../../src/do/entries.ts";
import type { Ms, Sql } from "../../src/types.ts";
import { openSql } from "../../test-kit/sql/node.ts";
import {
  ACCOUNT,
  CAP,
  completed,
  HASH,
  OTHER_CAP,
  OTHER_HASH,
  sessionPort,
  subscriptionEvent,
  tables,
  type World,
  world,
} from "./support.ts";

const PENDING = { accountId: ACCOUNT, credentialHash: HASH, cap: CAP, sessionId: "cs_1", url: "http://127.0.0.1:1/c/1", expiresAt: 10_000 };
const SESSION = { sessionId: "cs_1", customerId: "cus_1", subscriptionId: "sub_1", payment: "paid" as const };
const WEEK_MS = 604_800_000;
const ACCOUNT_COLUMNS =["account_id", "cancel_first", "credential_hash", "customer_id", "ended_at", "m1_count", "m1_newest", "m2_count", "m2_newest", "period_end", "period_start", "provider_time", "state", "subscription_id"];

function withSql<T>(run: (sql: Sql, accounts: Accounts) => T): T {
  const db = openSql();
  try {
    const accounts = makeAccounts();
    return db.step((sql) => {
      accounts.init(sql);
      return run(sql, accounts);
    });
  } finally {
    db.close();
  }
}

function account(sql: Sql) {
  return sql.exec("SELECT * FROM account");
}

describe("what the id holds", () => {
  test("nothing, a pending checkout, an account, or another credential", () => {
    withSql((sql, accounts) => {
      expect(accounts.look(sql, HASH)).toBe("nothing");
      expect(accounts.pending(sql)).toBeNull();
      expect(accounts.subscription(sql)).toBeNull();
      accounts.keepPending(sql, PENDING, 500);
      expect(accounts.look(sql, HASH)).toBe("pending");
      expect(accounts.look(sql, OTHER_HASH)).toBe("mismatch");
      expect(accounts.pending(sql)).toEqual({ sessionId: "cs_1", url: PENDING.url, expiresAt: 10_000, cap: CAP });
      expect(accounts.subscription(sql)).toBeNull();
      expect(accounts.isActive(sql)).toBe(false);
      accounts.replacePendingCap(sql, OTHER_CAP);
      expect(accounts.pending(sql)?.cap).toEqual(OTHER_CAP);
      expect(accounts.complete(sql, SESSION)).toEqual({ kind: "account-made", cap: OTHER_CAP, period: null });
      expect(accounts.look(sql, HASH)).toBe("account");
      expect(accounts.look(sql, OTHER_HASH)).toBe("mismatch");
      expect(accounts.pending(sql)).toBeNull();
      expect(accounts.subscription(sql)).toEqual({ accountId: ACCOUNT, state: "active", customerId: "cus_1", subscriptionId: "sub_1" });
      expect(accounts.isActive(sql)).toBe(true);
    });
  });

  test("the seed makes an account with no subscription id, no period and no provider time, once", () => {
    withSql((sql, accounts) => {
      accounts.seed(sql, { accountId: ACCOUNT, credentialHash: HASH, state: "cancelled", customerId: null }, 7);
      const again = () => accounts.seed(sql, { accountId: ACCOUNT, credentialHash: HASH, state: "active", customerId: null }, 8);
      expect(again).toThrow(expect.objectContaining({ name: "ContractBrokenError" }));
      expect(accounts.look(sql, HASH)).toBe("account");
      expect(accounts.subscription(sql)).toEqual({ accountId: ACCOUNT, state: "cancelled", customerId: null, subscriptionId: null });
      expect(accounts.nextDue(sql)).toBeNull();
    });
  });

  test("the tables hold the columns of contract 3.5 and no buyer field", () => {
    withSql((sql, accounts) => {
      accounts.keepPending(sql, PENDING, 500);
      accounts.applyEvent(sql, completed(), 600);
      expect(Object.keys(account(sql)[0] ?? {}).sort()).toEqual(ACCOUNT_COLUMNS);
      const columns = sql.exec("SELECT name FROM sqlite_master WHERE type = ?", "table").map((row) => String(row.name));
      expect(columns.sort()).toEqual(["account", "kept_event", "pending_checkout", "processed_event"]);
    });
  });
});

describe("complete", () => {
  test("makes the account once for a session id, and a second call or another session changes nothing", () => {
    withSql((sql, accounts) => {
      accounts.keepPending(sql, PENDING, 500);
      expect(accounts.complete(sql, { ...SESSION, sessionId: "cs_other" })).toEqual({ kind: "none", overrode: false });
      expect(accounts.pending(sql)?.sessionId).toBe("cs_1");
      expect(accounts.complete(sql, SESSION)).toEqual({ kind: "account-made", cap: CAP, period: null });
      const made = account(sql);
      expect(accounts.complete(sql, SESSION)).toEqual({ kind: "none", overrode: false });
      expect(account(sql)).toEqual(made);
      expect(made).toEqual([
        {
          account_id: ACCOUNT,
          credential_hash: HASH,
          state: "active",
          customer_id: "cus_1",
          subscription_id: "sub_1",
          period_start: null,
          period_end: null,
          provider_time: null,
          ended_at: null,
          cancel_first: null,
          m1_count: 0,
          m1_newest: null,
          m2_count: 0,
          m2_newest: null,
        },
      ]);
    });
  });

  test.each([
    ["paid", "active"],
    ["unpaid", "payment-failed"],
    ["no-payment-required", "payment-failed"],
  ] as const)("a session that reads %s makes the account %s", (payment, state) => {
    withSql((sql, accounts) => {
      accounts.keepPending(sql, PENDING, 500);
      accounts.complete(sql, { ...SESSION, payment });
      expect(accounts.subscription(sql)?.state).toBe(state);
    });
  });

  test("takes the state, the period and the provider time of a kept subscription event of its subscription", () => {
    withSql((sql, accounts) => {
      accounts.keepPending(sql, PENDING, 500);
      const event = subscriptionEvent({ state: "payment-failed" });
      expect(accounts.applyEvent(sql, event, 600)).toEqual({ kind: "kept", overrode: false });
      expect(accounts.complete(sql, SESSION)).toEqual({ kind: "account-made", cap: CAP, period: { start: 1_000, end: 2_592_001_000 } });
      expect(account(sql)[0]).toEqual(
        expect.objectContaining({ state: "payment-failed", provider_time: 3_000, period_start: 1_000, period_end: 2_592_001_000 }),
      );
    });
  });

  test("gives the account nothing of a kept event of another subscription", () => {
    withSql((sql, accounts) => {
      accounts.keepPending(sql, PENDING, 500);
      accounts.applyEvent(sql, subscriptionEvent({ subscriptionId: "sub_other", state: "cancelled" }), 600);
      expect(accounts.complete(sql, SESSION)).toEqual({ kind: "account-made", cap: CAP, period: null });
      expect(account(sql)[0]).toEqual(expect.objectContaining({ state: "active", subscription_id: "sub_1", provider_time: null }));
    });
  });

  test("takes the kept subscription id when the session names none", () => {
    withSql((sql, accounts) => {
      accounts.keepPending(sql, PENDING, 500);
      accounts.applyEvent(sql, subscriptionEvent({ subscriptionId: "sub_kept" }), 600);
      accounts.complete(sql, { ...SESSION, subscriptionId: null });
      expect(accounts.subscription(sql)?.subscriptionId).toBe("sub_kept");
    });
  });

  test("takes the first kept subscription when the session names none, and leaves no kept event beside the account", () => {
    withSql((sql, accounts) => {
      accounts.keepPending(sql, PENDING, 500);
      accounts.applyEvent(sql, subscriptionEvent({ eventId: "a", subscriptionId: "sub_a", providerTime: 3_000 }), 600);
      accounts.applyEvent(sql, subscriptionEvent({ eventId: "b", subscriptionId: "sub_b", providerTime: 4_000, state: "cancelled" }), 600);
      accounts.complete(sql, { ...SESSION, subscriptionId: null });
      expect(accounts.subscription(sql)).toEqual(expect.objectContaining({ subscriptionId: "sub_a", state: "active" }));
      expect(sql.exec("SELECT * FROM kept_event")).toEqual([]);
    });
  });
});

describe("applyEvent", () => {
  test("keeps each event id with its receipt time, and a repeat writes nothing", () => {
    withSql((sql, accounts) => {
      accounts.keepPending(sql, PENDING, 500);
      expect(accounts.applyEvent(sql, completed(), 600).kind).toBe("account-made");
      const once = { account: account(sql), ids: sql.exec("SELECT * FROM processed_event") };
      expect(once.ids).toEqual([{ event_id: "evt_completed", received_at: 600 }]);
      expect(accounts.applyEvent(sql, completed(), 700)).toEqual({ kind: "none", overrode: false });
      expect({ account: account(sql), ids: sql.exec("SELECT * FROM processed_event") }).toEqual(once);
      expect(accounts.applyEvent(sql, { kind: "ignored" }, 800)).toEqual({ kind: "none", overrode: false });
      expect(sql.exec("SELECT * FROM processed_event")).toEqual(once.ids);
    });
  });

  test("a completed event with no pending checkout of its session is kept as an id and changes nothing", () => {
    withSql((sql, accounts) => {
      accounts.keepPending(sql, PENDING, 500);
      expect(accounts.applyEvent(sql, completed({ sessionId: "cs_other" }), 600)).toEqual({ kind: "none", overrode: false });
      expect(accounts.pending(sql)?.sessionId).toBe("cs_1");
      expect(sql.exec("SELECT event_id FROM processed_event")).toEqual([{ event_id: "evt_completed" }]);
    });
  });

  test("a cancel replaces every state that is not cancelled whatever the times, nothing but a cancel replaces cancelled, and among cancels the newer replaces: a later time, or the same time and a higher rank", () => {
    withSql((sql, accounts) => {
      accounts.keepPending(sql, PENDING, 500);
      accounts.complete(sql, SESSION);
      const at = (eventId: string, providerTime: number, state: "active" | "payment-failed" | "cancelled", period = null) =>
        accounts.applyEvent(sql, subscriptionEvent({ eventId, providerTime, state, period }), 600);
      expect(at("e1", 100, "payment-failed")).toEqual({ kind: "state", state: "payment-failed", was: "active", period: null, overrode: false });
      expect(at("e2", 99, "cancelled")).toEqual({ kind: "state", state: "cancelled", was: "payment-failed", period: null, overrode: true });
      expect(at("e3", 100, "active")).toEqual({ kind: "none", overrode: true });
      expect(at("e4", 100, "payment-failed")).toEqual({ kind: "none", overrode: true });
      expect(at("e5", 100, "cancelled")).toEqual({ kind: "state", state: "cancelled", was: "cancelled", period: null, overrode: false });
      expect(at("e6", 100, "active")).toEqual({ kind: "none", overrode: false });
      expect(at("e7", 101, "cancelled")).toEqual({ kind: "state", state: "cancelled", was: "cancelled", period: null, overrode: false });
      expect(account(sql)[0]).toEqual(expect.objectContaining({ state: "cancelled", provider_time: 101 }));
    });
  });

  test("an event without a period stores none, and an event of another subscription changes nothing", () => {
    withSql((sql, accounts) => {
      accounts.keepPending(sql, PENDING, 500);
      accounts.complete(sql, SESSION);
      accounts.applyEvent(sql, subscriptionEvent({ eventId: "e1", providerTime: 10 }), 600);
      expect(accounts.applyEvent(sql, subscriptionEvent({ eventId: "e2", providerTime: 20, state: "payment-failed", period: null }), 600)).toEqual({
        kind: "state",
        state: "payment-failed",
        was: "active",
        period: null,
        overrode: false,
      });
      expect(account(sql)[0]).toEqual(expect.objectContaining({ period_start: null, period_end: null, provider_time: 20 }));
      expect(accounts.applyEvent(sql, subscriptionEvent({ eventId: "e3", providerTime: 30, subscriptionId: "sub_other", state: "cancelled" }), 600)).toEqual({
        kind: "none",
        overrode: false,
      });
      expect(accounts.subscription(sql)?.state).toBe("payment-failed");
    });
  });

  test("an account with no subscription id takes the first subscription event that names it", () => {
    withSql((sql, accounts) => {
      accounts.seed(sql, { accountId: ACCOUNT, credentialHash: HASH, state: "active", customerId: "cus_1" }, 5);
      expect(accounts.applyEvent(sql, subscriptionEvent({ eventId: "e1", subscriptionId: "sub_first", state: "payment-failed" }), 6).kind).toBe("state");
      expect(accounts.applyEvent(sql, subscriptionEvent({ eventId: "e2", subscriptionId: "sub_second", providerTime: 9_999 }), 6)).toEqual({ kind: "none", overrode: false });
      expect(accounts.subscription(sql)).toEqual({ accountId: ACCOUNT, state: "payment-failed", customerId: "cus_1", subscriptionId: "sub_first" });
    });
  });

  test("a subscription event for a pending checkout is kept for its own subscription id, only when newer, and a cancel is newer than a kept state that is not cancelled", () => {
    withSql((sql, accounts) => {
      accounts.keepPending(sql, PENDING, 500);
      expect(accounts.applyEvent(sql, subscriptionEvent({ eventId: "e1", providerTime: 10 }), 600)).toEqual({ kind: "kept", overrode: false });
      expect(accounts.applyEvent(sql, subscriptionEvent({ eventId: "e2", providerTime: 9, state: "cancelled" }), 600)).toEqual({ kind: "kept", overrode: true });
      expect(accounts.applyEvent(sql, subscriptionEvent({ eventId: "e3", providerTime: 11, subscriptionId: "sub_other" }), 600)).toEqual({ kind: "kept", overrode: false });
      expect(accounts.applyEvent(sql, subscriptionEvent({ eventId: "e4", providerTime: 10, state: "cancelled", period: null }), 600)).toEqual({ kind: "kept", overrode: false });
      expect(accounts.subscription(sql)).toBeNull();
      expect(sql.exec("SELECT subscription_id, state, period_start, provider_time, kept_order FROM kept_event WHERE subscription_id = ?", "sub_1")).toEqual([
        { subscription_id: "sub_1", state: "cancelled", period_start: null, provider_time: 10, kept_order: 0 },
      ]);
      expect(sql.exec("SELECT subscription_id, kept_order FROM kept_event WHERE subscription_id = ?", "sub_other")).toEqual([
        { subscription_id: "sub_other", kept_order: 1 },
      ]);
    });
  });

  test("an event of the same provider time and the same state is not newer, on an account and on a pending checkout", () => {
    withSql((sql, accounts) => {
      accounts.keepPending(sql, PENDING, 500);
      expect(accounts.applyEvent(sql, subscriptionEvent({ eventId: "k1" }), 600)).toEqual({ kind: "kept", overrode: false });
      expect(accounts.applyEvent(sql, subscriptionEvent({ eventId: "k2", period: { start: 7, end: 8 } }), 600)).toEqual({ kind: "none", overrode: false });
      expect(sql.exec("SELECT period_start FROM kept_event")).toEqual([{ period_start: 1_000 }]);
      accounts.complete(sql, SESSION);
      expect(accounts.applyEvent(sql, subscriptionEvent({ eventId: "a1", providerTime: 5_000 }), 600).kind).toBe("state");
      expect(accounts.applyEvent(sql, subscriptionEvent({ eventId: "a2", providerTime: 5_000, period: { start: 7, end: 8 } }), 600)).toEqual({ kind: "none", overrode: false });
      expect(account(sql)[0]).toEqual(expect.objectContaining({ period_start: 1_000, provider_time: 5_000 }));
    });
  });

  test("of two events of one second and one state, the one whose period ends later is newer, and with equal periods neither is", () => {
    withSql((sql, accounts) => {
      accounts.keepPending(sql, PENDING, 500);
      expect(accounts.applyEvent(sql, subscriptionEvent({ eventId: "k1", period: { start: 3_000, end: 9_000 } }), 600)).toEqual({ kind: "kept", overrode: false });
      expect(accounts.applyEvent(sql, subscriptionEvent({ eventId: "k2", period: { start: 9_000, end: 15_000 } }), 600)).toEqual({ kind: "kept", overrode: false });
      expect(accounts.applyEvent(sql, subscriptionEvent({ eventId: "k3", period: { start: 9_000, end: 15_000 } }), 600)).toEqual({ kind: "none", overrode: false });
      expect(sql.exec("SELECT period_start, period_end FROM kept_event")).toEqual([{ period_start: 9_000, period_end: 15_000 }]);
      accounts.complete(sql, SESSION);
      const later = { start: 15_000, end: 21_000 };
      expect(accounts.applyEvent(sql, subscriptionEvent({ eventId: "a1", period: later }), 600)).toEqual({ kind: "state", state: "active", was: "active", period: later, overrode: false });
      expect(accounts.applyEvent(sql, subscriptionEvent({ eventId: "a2", period: { start: 9_000, end: 15_000 } }), 600)).toEqual({ kind: "none", overrode: false });
      expect(accounts.applyEvent(sql, subscriptionEvent({ eventId: "a3", period: later }), 600)).toEqual({ kind: "none", overrode: false });
      expect(account(sql)[0]).toEqual(expect.objectContaining({ period_start: 15_000, period_end: 21_000, provider_time: 3_000 }));
    });
  });

  test("a subscription event for an id that holds nothing answers none and keeps only its id", () => {
    withSql((sql, accounts) => {
      expect(accounts.applyEvent(sql, subscriptionEvent(), 600)).toEqual({ kind: "none", overrode: false });
      expect(sql.exec("SELECT * FROM kept_event")).toEqual([]);
      expect(sql.exec("SELECT * FROM pending_checkout")).toEqual([]);
      expect(sql.exec("SELECT * FROM account")).toEqual([]);
    });
  });

  test("a kept event of another subscription does not change which account the session's own subscription gives, in any order", () => {
    const stray = subscriptionEvent({ eventId: "stray", subscriptionId: "sub_stray", providerTime: 3_000 });
    const own = subscriptionEvent({ eventId: "own", providerTime: 4_000, state: "cancelled", period: { start: 3, end: 4 } });
    const session = completed({ eventId: "done" });
    const end = (events: ProviderEvent[]) =>
      withSql((sql, accounts) => {
        accounts.keepPending(sql, PENDING, 500);
        for (const each of events) accounts.applyEvent(sql, each, 600);
        return account(sql);
      });
    const strayFirst = end([stray, own, session]);
    expect(strayFirst).toEqual([expect.objectContaining({ state: "cancelled", subscription_id: "sub_1", period_start: 3, provider_time: 4_000 })]);
    expect(end([own, stray, session])).toEqual(strayFirst);
    expect(end([stray, session, own])).toEqual(strayFirst);
    expect(end([session, stray, own])).toEqual(strayFirst);
  });

  test("the newest kept event gives its own period, none when it carried none, and both orders agree", () => {
    const first = subscriptionEvent({ eventId: "first", providerTime: 3_000 });
    const second = subscriptionEvent({ eventId: "second", providerTime: 3_001, state: "payment-failed", period: null });
    const end = (events: ProviderEvent[]) =>
      withSql((sql, accounts) => {
        accounts.keepPending(sql, PENDING, 500);
        for (const each of events) accounts.applyEvent(sql, each, 600);
        return account(sql);
      });
    const pendingFirst = end([first, second, completed()]);
    expect(pendingFirst).toEqual([expect.objectContaining({ state: "payment-failed", period_start: null, period_end: null, provider_time: 3_001 })]);
    expect(end([completed(), first, second])).toEqual(pendingFirst);
    expect(end([first, completed(), second])).toEqual(pendingFirst);
  });

  const PAIRS: [string, ProviderEvent[]][] = [
    ["the completed event and the subscription event of the same payment", [completed(), subscriptionEvent()]],
    [
      "active with a later payment failed",
      [subscriptionEvent({ eventId: "a" }), subscriptionEvent({ eventId: "b", providerTime: 3_001, state: "payment-failed" })],
    ],
    [
      "payment failed with a later cancelled",
      [subscriptionEvent({ eventId: "a", state: "payment-failed" }), subscriptionEvent({ eventId: "b", providerTime: 3_001, state: "cancelled" })],
    ],
    [
      "payment failed with active of the same second",
      [subscriptionEvent({ eventId: "a", state: "payment-failed" }), subscriptionEvent({ eventId: "b", state: "active" })],
    ],
  ];

  test.each(PAIRS)("%s end in the same account row in both orders", (_, pair) => {
    const end = (events: ProviderEvent[]) =>
      withSql((sql, accounts) => {
        accounts.keepPending(sql, PENDING, 500);
        if (events.every((each) => each.kind === "subscription")) accounts.applyEvent(sql, completed({ eventId: "first" }), 600);
        for (const each of events) accounts.applyEvent(sql, each, 600);
        return account(sql);
      });
    const forward = end(pair);
    expect(forward).toHaveLength(1);
    expect(end([...pair].reverse())).toEqual(forward);
  });
});

describe("the reread of a pending checkout", () => {
  test("the first read is due at the expiry, and each read that keeps it moves the next by SESSION_LENGTH_MS", () => {
    withSql((sql, accounts) => {
      accounts.keepPending(sql, PENDING, 500);
      expect(accounts.nextDue(sql)).toBe(10_000);
      expect(accounts.wake(sql, 9_999)).toBeNull();
      expect(accounts.wake(sql, 10_000)).toEqual({ sessionId: "cs_1" });
      expect(accounts.record(sql, "cs_1", "open", 10_000)).toBe("kept");
      expect(accounts.nextDue(sql)).toBe(10_000 + SESSION_LENGTH_MS);
      expect(accounts.record(sql, "cs_other", "open", 10_000)).toBe("stale");
    });
  });

  test.each([
    ["expired", 10_000, "expired-unpaid"],
    ["open", 10_000 + PENDING_DROP_MS - 1, "kept"],
    ["open", 10_000 + PENDING_DROP_MS, "dropped"],
    ["unknown", 10_000 + PENDING_DROP_MS - 1, "kept"],
    ["unknown", 10_000 + PENDING_DROP_MS, "dropped"],
    ["failed", 10_000 + PENDING_DROP_MS, "kept"],
    ["failed", 10_000 + PENDING_KEEP_MS - 1, "kept"],
    ["failed", 10_000 + PENDING_KEEP_MS, "unanswered"],
  ] as const)("a read that says %s at %d answers %s and removes nothing itself", (read, now, answer) => {
    withSql((sql, accounts) => {
      accounts.keepPending(sql, PENDING, 500);
      expect(accounts.record(sql, "cs_1", read, now)).toBe(answer);
      expect(accounts.pending(sql)?.sessionId).toBe("cs_1");
    });
  });

  test("after a read that kept it, wake waits for the next read and not for the expiry", () => {
    withSql((sql, accounts) => {
      accounts.keepPending(sql, PENDING, 500);
      expect(accounts.record(sql, "cs_1", "open", 10_000)).toBe("kept");
      expect(accounts.wake(sql, 10_000 + SESSION_LENGTH_MS - 1)).toBeNull();
      expect(accounts.wake(sql, 10_000 + SESSION_LENGTH_MS)).toEqual({ sessionId: "cs_1" });
    });
  });

  test("record answers stale when no pending checkout is left", () => {
    withSql((sql, accounts) => {
      expect(accounts.record(sql, "cs_1", "expired", 1)).toBe("stale");
    });
  });
});

describe("processed event ids", () => {
  test("forget removes the ids at or before now minus EVENT_ID_KEEP_MS, and nextDue names the oldest one's age", () => {
    withSql((sql, accounts) => {
      accounts.seed(sql, { accountId: ACCOUNT, credentialHash: HASH, state: "active", customerId: "cus_1" }, 5);
      accounts.applyEvent(sql, subscriptionEvent({ eventId: "e1" }), 100);
      accounts.applyEvent(sql, subscriptionEvent({ eventId: "e2", providerTime: 4_000 }), 200);
      expect(accounts.nextDue(sql)).toBe(100 + EVENT_ID_KEEP_MS);
      expect(accounts.forget(sql, 100 + EVENT_ID_KEEP_MS - 1)).toBe(0);
      expect(accounts.forget(sql, 100 + EVENT_ID_KEEP_MS)).toBe(1);
      expect(accounts.nextDue(sql)).toBe(200 + EVENT_ID_KEEP_MS);
      expect(accounts.forget(sql, 200 + EVENT_ID_KEEP_MS)).toBe(1);
      expect(accounts.nextDue(sql)).toBeNull();
    });
  });
});

describe("the module behind the entries", () => {
  let current: World | null = null;
  afterEach(() => {
    current?.close();
    current = null;
  });
  function open(clock?: () => Ms) {
    current = world(sessionPort(10_000), clock);
    return current;
  }

  test("a checkout start keeps a pending checkout and no account, and the completed event makes the account with its cap", async () => {
    const madeAt = 1_000;
    const w = open(() => madeAt);
    expect(await w.entries.start(ACCOUNT, HASH, CAP)).toEqual({ result: "session", url: "http://127.0.0.1:1/c/1" });
    const pending = await tables(w.entries);
    expect(pending?.account).toEqual([]);
    expect(pending?.pending_checkout).toHaveLength(1);
    expect(await w.entries.status(HASH)).toEqual({ result: "pending" });
    expect(await w.entries.destroy(HASH)).toEqual({ result: "pending" });
    expect(await w.entries.status(OTHER_HASH)).toEqual({ result: "credential-mismatch" });
    expect(await w.entries.start(ACCOUNT, HASH, OTHER_CAP)).toEqual({ result: "session", url: "http://127.0.0.1:1/c/1" });
    await w.entries.webhookEvent(completed());
    expect(await w.entries.status(HASH)).toEqual({ result: "active" });
    expect(OTHER_CAP.period).toBe("week");
    expect(await w.entries.capGet(HASH)).toEqual({ result: "cap", cap: OTHER_CAP, spend: { spent: 0, held: 0, period_end: madeAt + WEEK_MS } });
    expect(await w.entries.start(ACCOUNT, HASH, CAP)).toEqual({ result: "account-exists" });
    expect(await w.entries.start(ACCOUNT, OTHER_HASH, CAP)).toEqual({ result: "credential-mismatch" });
  });

  test("a completed event for an id that holds nothing, or a pending checkout of another session, logs completed-no-record and changes nothing", async () => {
    const w = open();
    await w.entries.webhookEvent(completed());
    expect(await w.entries.testInspect()).toEqual({ result: "no-such-account" });
    await w.entries.start(ACCOUNT, HASH, CAP);
    const before = await tables(w.entries);
    await w.entries.webhookEvent(completed({ eventId: "evt_other", sessionId: "cs_other" }));
    const after = await tables(w.entries);
    expect(after?.pending_checkout).toEqual(before?.pending_checkout);
    expect(after?.account).toEqual([]);
    expect(w.logs).toEqual([{ event: "completed-no-record" }, { event: "completed-no-record" }]);
  });

  test("a subscription event that comes before the completed event is kept, and both orders end in the same account", async () => {
    const rows = [];
    for (const order of [
      [completed(), subscriptionEvent()],
      [subscriptionEvent(), completed()],
    ]) {
      const w = world(sessionPort(10_000));
      await w.entries.start(ACCOUNT, HASH, CAP);
      for (const each of order) await w.entries.webhookEvent(each);
      rows.push((await tables(w.entries))?.account);
      w.close();
    }
    expect(rows[1]).toEqual(rows[0]);
    expect(rows[0]).toEqual([expect.objectContaining({ state: "active", provider_time: 3_000, period_start: 1_000 })]);
  });

  test("an account delete leaves no table of the object's own, the processed event ids among them", async () => {
    const w = world(sessionPort(10_000, { cancelSubscription: async () => ({ kind: "cancelled" }) }));
    current = w;
    await w.entries.start(ACCOUNT, HASH, CAP);
    await w.entries.webhookEvent(completed());
    await w.entries.webhookEvent(subscriptionEvent());
    expect(w.host.step((sql) => ownTables(sql))).toEqual(expect.arrayContaining(["account", "processed_event", "skeleton"]));
    expect(await w.entries.destroy(HASH)).toEqual({ result: "deleted" });
    expect(w.host.step((sql) => ownTables(sql))).toEqual([]);
    expect(await w.entries.testInspect()).toEqual({ result: "no-such-account" });
  });

  test("two subscription events of one second and one state end in one account row, period included, in all six orders, and metering gets that period", async () => {
    const events: Record<string, ProviderEvent> = {
      completed: completed(),
      first: subscriptionEvent({ eventId: "evt_p1", providerTime: 3_000, period: { start: 3_000, end: 9_000 } }),
      second: subscriptionEvent({ eventId: "evt_p2", providerTime: 3_000, period: { start: 9_000, end: 15_000 } }),
    };
    const orders = [
      ["completed", "first", "second"],
      ["completed", "second", "first"],
      ["first", "completed", "second"],
      ["first", "second", "completed"],
      ["second", "completed", "first"],
      ["second", "first", "completed"],
    ];
    const ends = [];
    for (const order of orders) {
      const w = world(sessionPort(10_000));
      const billingPeriod = vi.spyOn(w.metering, "billingPeriod");
      await w.entries.start(ACCOUNT, HASH, CAP);
      for (const name of order) await w.entries.webhookEvent(events[name] as ProviderEvent);
      ends.push({ order: order.join(","), account: (await tables(w.entries))?.account, lastPeriod: billingPeriod.mock.calls.at(-1)?.[1] });
      w.close();
    }
    const want = {
      account: [expect.objectContaining({ state: "active", subscription_id: "sub_1", period_start: 9_000, period_end: 15_000, provider_time: 3_000 })],
      lastPeriod: { start: 9_000, end: 15_000 },
    };
    expect(ends).toEqual(orders.map((order) => ({ order: order.join(","), ...want })));
  });

  test("of two subscription events of one second and one state, the one that states a period is newer than the one that states none, in both orders", async () => {
    const events: Record<string, ProviderEvent> = {
      completed: completed(),
      period: subscriptionEvent({ eventId: "evt_period", providerTime: 3_000, period: { start: 9_000, end: 15_000 } }),
      none: subscriptionEvent({ eventId: "evt_none", providerTime: 3_000, period: null }),
    };
    const orders = [
      ["completed", "period", "none"],
      ["completed", "none", "period"],
      ["period", "none", "completed"],
      ["none", "period", "completed"],
    ];
    const ends = [];
    for (const order of orders) {
      const w = world(sessionPort(10_000));
      await w.entries.start(ACCOUNT, HASH, CAP);
      for (const name of order) await w.entries.webhookEvent(events[name] as ProviderEvent);
      ends.push({ order: order.join(","), account: (await tables(w.entries))?.account });
      w.close();
    }
    const want = [expect.objectContaining({ state: "active", subscription_id: "sub_1", period_start: 9_000, period_end: 15_000, provider_time: 3_000 })];
    expect(ends).toEqual(orders.map((order) => ({ order: order.join(","), account: want })));
  });
});
