import { type Accounts, type Applied, SESSION_LENGTH_MS } from "../accounts/index.ts";
import type { ProviderEvent, ProviderPort, SubscriptionState } from "../billing/port.ts";
import type {
  CapGetAnswer,
  CapSetAnswer,
  CheckoutAnswer,
  DeleteAnswer,
  ManageAnswer,
  StatusAnswer,
} from "../contract/schemas.ts";
import type { EndResult, Ledger, Price, ReserveResult, Usage } from "../ledger/index.ts";
import { errorName, log } from "../log.ts";
import type { Metering, Next, UsageAnswer, UsageCall } from "../metering/index.ts";
import type { Cap, Micro, Ms, Sql, Step, Tier } from "../types.ts";

export const ALARM_RETRY_MS = 30_000;
export const ALARM_RETRY_LIMIT = 5;
export const ALARM_CALL_LIMIT = 3;
export const START_RESTARTS = 2;
export const CLOCK_RUN_LIMIT = 100;
export interface Host {
  step: Step;
  clock(): Ms;
  setAlarm(at: Ms | null): void;
  getAlarm(): Promise<Ms | null>;
  deleteAll(): Promise<void>;
  port: ProviderPort;
  config: { holdLifeMs: number; intervalSeconds: number; marginSeconds: number; publicOrigin: string; testClock: boolean };
}
export type Gate = { outcome: "refused"; reason: "no-such-account" | "credential-mismatch" | "pending" | "closing" };
export interface Snapshot {
  result: "snapshot";
  now: Ms;
  test_time: Ms | null;
  due: Ms | null;
  alarm: Ms | null;
  config: Host["config"];
  tables: Record<string, Record<string, string | number | null>[]>;
}
export interface Entries {
  start(accountId: string, hash: string, cap: Cap): Promise<CheckoutAnswer>;
  webhookEvent(event: ProviderEvent): Promise<void>;
  status(hash: string): Promise<StatusAnswer>;
  capGet(hash: string): Promise<CapGetAnswer>;
  capSet(hash: string, cap: Cap): Promise<CapSetAnswer>;
  manage(hash: string): Promise<ManageAnswer>;
  destroy(hash: string): Promise<DeleteAnswer>;
  reserve(hash: string, req: { requestId: string; tier: Tier; amount: Micro; price: Price }): Promise<ReserveResult | Gate>;
  dispatch(requestId: string): Promise<"go" | "stop">;
  settle(requestId: string, usage: Usage): Promise<EndResult>;
  void(requestId: string): Promise<EndResult>;
  alarm(): Promise<void>;
  rearm(): Promise<void>;
  testSetClock(time: Ms): Promise<{ result: "clock-set" | "behind" | "no-such-account" | "refused" }>;
  testSeed(a: {
    accountId: string;
    hash: string;
    subscription: SubscriptionState;
    customerId?: string;
    cap?: Cap;
    anchor?: Ms;
    time?: Ms;
  }): Promise<{ result: "seeded" | "not-empty" | "refused" }>;
  testInspect(): Promise<Snapshot | { result: "no-such-account" | "refused" }>;
}

const SKELETON = "skeleton";
const TEST_CLOCK = "test_clock";
const ENGINE_PREFIX = "sqlite_";
const PLATFORM_PREFIX = "_cf_";
const LOCAL_RUNTIME_TABLE = "__miniflare_do_name";
type Removal = "expired-unpaid" | "dropped" | "unanswered";

interface Open {
  sql: Sql;
  begin(): Ms;
  beginWithoutAlarm(): Ms;
  arm(): void;
}

function contractBroken(): never {
  const error = new Error("a module answered what contract 3.1 rules out");
  error.name = "ContractBrokenError";
  throw error;
}

function earliest(times: (Ms | null)[]): Ms | null {
  let found: Ms | null = null;
  for (const each of times) if (each !== null && (found === null || each < found)) found = each;
  return found;
}

function isRemoval(answer: string): answer is Removal {
  return answer === "expired-unpaid" || answer === "dropped" || answer === "unanswered";
}

export function ownTables(sql: Sql): string[] {
  return sql
    .exec(
      "SELECT name FROM sqlite_master WHERE type = ? AND substr(name, 1, 7) <> ? AND substr(name, 1, 4) <> ? AND name <> ?",
      "table",
      ENGINE_PREFIX,
      PLATFORM_PREFIX,
      LOCAL_RUNTIME_TABLE,
    )
    .map((row) => String(row.name));
}

export function makeEntries(host: Host, modules: { accounts: Accounts; ledger: Ledger; metering: Metering }): Entries {
  const { accounts, ledger, metering } = modules;

  const tableExists = (sql: Sql, name: string) =>
    sql.exec("SELECT name FROM sqlite_master WHERE type = ? AND name = ?", "table", name).length > 0;
  const holds = (sql: Sql) => tableExists(sql, SKELETON);
  const skeletonRow = (sql: Sql) => sql.exec("SELECT destroying_since, last_now, alarm_failures FROM skeleton")[0] ?? contractBroken();
  const lastNow = (sql: Sql) => Number(skeletonRow(sql).last_now);
  const destroyingSince = (sql: Sql) => {
    const value = skeletonRow(sql).destroying_since;
    return value === null || value === undefined ? null : Number(value);
  };
  const testTime = (sql: Sql): Ms | null => {
    if (!host.config.testClock || !tableExists(sql, TEST_CLOCK)) return null;
    const row = sql.exec("SELECT time FROM test_clock")[0];
    return row === undefined ? null : Number(row.time);
  };
  const time = (sql: Sql) => testTime(sql) ?? host.clock();
  const isMetered = (sql: Sql) => {
    const subscription = accounts.subscription(sql);
    return subscription !== null && subscription.customerId !== null;
  };

  function dueOf(sql: Sql): Ms | null {
    const metered = isMetered(sql);
    const times = [ledger.nextDue(sql), accounts.nextDue(sql)];
    if (metered) {
      times.push(metering.nextSend(sql));
      const u = metering.oldestUnsealed(sql);
      if (u !== null && ledger.intervalTotals(sql, u.interval).open === 0) times.push(u.end);
    }
    return earliest(times);
  }

  function armAlarm(at: Ms | null): void {
    try {
      host.setAlarm(at);
    } catch (caught) {
      log({ event: "alarm-error", error: errorName(caught) });
    }
  }

  function step<T>(run: (open: Open) => T): T {
    let arm = false;
    let due: Ms | null = null;
    let tested = false;
    const value = host.step((sql) => {
      const begin = (withAlarm: boolean) => {
        const now = Math.max(time(sql), lastNow(sql));
        sql.exec("UPDATE skeleton SET last_now = ?", now);
        arm = withAlarm;
        return now;
      };
      const result = run({
        sql,
        begin: () => begin(true),
        beginWithoutAlarm: () => begin(false),
        arm: () => {
          arm = true;
        },
      });
      if (arm) {
        due = dueOf(sql);
        tested = testTime(sql) !== null;
      }
      return result;
    });
    if (arm) armAlarm(tested ? null : due);
    return value;
  }

  function createTables(sql: Sql, now: Ms): void {
    sql.exec("CREATE TABLE skeleton (destroying_since INTEGER, last_now INTEGER NOT NULL, alarm_failures INTEGER NOT NULL)");
    sql.exec("INSERT INTO skeleton (destroying_since, last_now, alarm_failures) VALUES (?, ?, ?)", null, now, 0);
    accounts.init(sql);
    ledger.init(sql);
    metering.init(sql);
  }

  function storeTestTime(sql: Sql, at: Ms): void {
    if (tableExists(sql, TEST_CLOCK)) sql.exec("UPDATE test_clock SET time = ?", at);
    else {
      sql.exec("CREATE TABLE test_clock (time INTEGER NOT NULL)");
      sql.exec("INSERT INTO test_clock (time) VALUES (?)", at);
    }
  }

  function accountMade(sql: Sql, made: Extract<Applied, { kind: "account-made" }>, accountId: string, now: Ms): void {
    ledger.setCap(sql, made.cap, now);
    metering.begin(sql, accountId, now);
    if (made.period !== null) metering.billingPeriod(sql, made.period, now);
    const cancelledAt = accounts.subscription(sql)?.cancelledAt;
    if (cancelledAt !== undefined) metering.cut(sql, "cancel", cancelledAt, now);
  }

  async function removeAll(removal: Removal): Promise<void> {
    await host.deleteAll();
    if (removal === "dropped") log({ event: "pending-dropped" });
    if (removal === "unanswered") log({ event: "pending-unanswered" });
  }

  function logNotBillable(next: Next): void {
    for (let each = 0; each < next.notBillable; each += 1) log({ event: "usage-not-billable" });
  }

  function sealLoop(sql: Sql, now: Ms): void {
    let sealed: Ms | null = null;
    for (;;) {
      const u = metering.oldestUnsealed(sql);
      if (u === null || u.end > now || u.interval === sealed) return;
      const totals = ledger.intervalTotals(sql, u.interval);
      if (totals.open > 0) return;
      metering.seal(sql, u.interval, totals.reported, now);
      sealed = u.interval;
    }
  }

  function idsOf(sql: Sql): { accountId: string; customerId: string } {
    const subscription = accounts.subscription(sql);
    if (subscription === null || subscription.customerId === null) return contractBroken();
    return { accountId: subscription.accountId, customerId: subscription.customerId };
  }

  async function callPort(call: UsageCall, ids: { accountId: string; customerId: string }): Promise<UsageAnswer> {
    if (call.kind === "send") {
      return host.port.sendUsage({
        ...ids,
        identifier: call.identifier,
        amountMinor: call.amountMinor,
        timestamp: call.timestamp,
      });
    }
    return host.port.readUsageTotal({ ...ids, from: call.from, to: call.to });
  }

  async function sendAndRecord(): Promise<"gone" | "done"> {
    const sent = step((open) => {
      if (!holds(open.sql)) return null;
      const now = open.begin();
      ledger.expire(open.sql, now);
      const next = metering.send(open.sql, now);
      return { next, ids: next.call === null ? null : idsOf(open.sql) };
    });
    if (sent === null) return "gone";
    logNotBillable(sent.next);
    let call = sent.next.call;
    let ids = sent.ids;
    for (let calls = 0; call !== null && ids !== null && calls < ALARM_CALL_LIMIT; calls += 1) {
      const made = call;
      const answer = await callPort(made, ids);
      const recorded = step((open) => {
        if (!holds(open.sql)) return null;
        const now = open.begin();
        ledger.expire(open.sql, now);
        if (!isMetered(open.sql)) return "not-metered" as const;
        const next = metering.record(open.sql, made, answer, now);
        return { next, ids: next.call === null ? null : idsOf(open.sql) };
      });
      if (recorded === null) return "gone";
      if (recorded === "not-metered") return "done";
      logNotBillable(recorded.next);
      call = recorded.next.call;
      ids = recorded.ids;
    }
    return "done";
  }

  async function alarmBody(): Promise<void> {
    const first = step((open) => {
      if (!holds(open.sql)) return null;
      const now = open.begin();
      const failures = Number(skeletonRow(open.sql).alarm_failures);
      ledger.expire(open.sql, now);
      if (isMetered(open.sql)) sealLoop(open.sql, now);
      accounts.forget(open.sql, now);
      return { failures, wake: accounts.wake(open.sql, now), metered: isMetered(open.sql) };
    });
    if (first === null) return;
    let metered = first.metered;
    if (first.wake !== null) {
      const sessionId = first.wake.sessionId;
      const answer = await host.port.readSession(sessionId);
      const after = step((open) => {
        if (!holds(open.sql)) return null;
        const now = open.begin();
        ledger.expire(open.sql, now);
        if (answer.kind === "complete") {
          const made = accounts.complete(open.sql, answer.session);
          if (made.kind === "account-made") {
            accountMade(open.sql, made, (accounts.subscription(open.sql) ?? contractBroken()).accountId, now);
          }
          return { removal: null, metered: isMetered(open.sql) };
        }
        const recorded = accounts.record(open.sql, sessionId, answer.kind, now);
        if (isRemoval(recorded)) {
          open.beginWithoutAlarm();
          return { removal: recorded, metered: false };
        }
        return { removal: null, metered: isMetered(open.sql) };
      });
      if (after === null) return;
      if (after.removal !== null) {
        await removeAll(after.removal);
        return;
      }
      metered = after.metered;
    }
    if (metered && (await sendAndRecord()) === "gone") return;
    if (first.failures > 0) {
      step((open) => {
        if (!holds(open.sql)) return;
        open.begin();
        open.sql.exec("UPDATE skeleton SET alarm_failures = ?", 0);
      });
    }
  }

  function countFailure(error: unknown): void {
    const name = errorName(error);
    log({ event: "alarm-error", error: name });
    let count = ALARM_RETRY_LIMIT;
    let tested = false;
    try {
      const counted = host.step((sql) => {
        const now = Math.max(time(sql), lastNow(sql));
        const failures = Math.min(Number(skeletonRow(sql).alarm_failures) + 1, ALARM_RETRY_LIMIT);
        sql.exec("UPDATE skeleton SET last_now = ?, alarm_failures = ?", now, failures);
        return { failures, tested: testTime(sql) !== null };
      });
      count = counted.failures;
      tested = counted.tested;
    } catch {
      count = ALARM_RETRY_LIMIT;
    }
    if (count < ALARM_RETRY_LIMIT) {
      armAlarm(tested ? null : host.clock() + ALARM_RETRY_MS);
      return;
    }
    log({ event: "alarm-stopped", error: name });
    armAlarm(null);
  }

  function guarded<T>(hash: string, answers: { nothing: T; mismatch: T; pending: T }, onAccount: (open: Open, now: Ms) => T): T {
    return step((open) => {
      if (!holds(open.sql)) return answers.nothing;
      const now = open.begin();
      const look = accounts.look(open.sql, hash);
      if (look === "nothing") return answers.nothing;
      if (look === "mismatch") return answers.mismatch;
      if (look === "pending") return answers.pending;
      return onAccount(open, now);
    });
  }

  function capAnswer(sql: Sql, now: Ms): { result: "cap"; cap: Cap; spend: { spent: Micro; held: Micro; period_end: Ms } } | null {
    const read = ledger.cap(sql, now);
    if (read === null) return null;
    return { result: "cap", cap: read.cap, spend: { spent: read.spent, held: read.held, period_end: read.periodEnd } };
  }

  async function destroyStepThree(metered: boolean): Promise<"gone" | "done"> {
    const sealed = step((open) => {
      if (!holds(open.sql)) return false;
      const now = open.begin();
      ledger.expire(open.sql, now);
      if (metered) sealLoop(open.sql, now);
      return true;
    });
    if (!sealed) return "gone";
    return metered ? sendAndRecord() : "done";
  }

  return {
    async start(accountId, hash, cap) {
      for (let restarts = 0; ; restarts += 1) {
        const first = step((open) => {
          if (!holds(open.sql)) return { kind: "nothing" as const, now: time(open.sql) };
          const now = open.begin();
          const look = accounts.look(open.sql, hash);
          if (look === "mismatch") return { kind: "answer" as const, answer: { result: "credential-mismatch" as const } };
          if (look === "account") return { kind: "answer" as const, answer: { result: "account-exists" as const } };
          if (look === "nothing") return { kind: "nothing" as const, now };
          const pending = accounts.pending(open.sql) ?? contractBroken();
          if (now < pending.expiresAt) {
            accounts.replacePendingCap(open.sql, cap);
            return { kind: "answer" as const, answer: { result: "session" as const, url: pending.url } };
          }
          return { kind: "read" as const, pending };
        });
        if (first.kind === "answer") return first.answer;
        let now = first.kind === "nothing" ? first.now : 0;
        if (first.kind === "read") {
          const pending = first.pending;
          const answer = await host.port.readSession(pending.sessionId);
          const after = step((open) => {
            if (!holds(open.sql)) return { kind: "stale" as const };
            const stepNow = open.begin();
            if (answer.kind === "complete") {
              const made = accounts.complete(open.sql, answer.session);
              if (made.kind === "none") return { kind: "stale" as const };
              accountMade(open.sql, made, accountId, stepNow);
              return { kind: "answer" as const, answer: { result: "account-exists" as const } };
            }
            const recorded = accounts.record(open.sql, pending.sessionId, answer.kind, stepNow);
            if (isRemoval(recorded)) {
              open.beginWithoutAlarm();
              return { kind: "removed" as const, removal: recorded, now: stepNow };
            }
            if (recorded === "kept") {
              accounts.replacePendingCap(open.sql, cap);
              return { kind: "answer" as const, answer: { result: "session" as const, url: pending.url } };
            }
            return { kind: "stale" as const };
          });
          if (after.kind === "answer") return after.answer;
          if (after.kind === "stale") {
            if (restarts === START_RESTARTS) return { result: "unavailable" };
            continue;
          }
          await removeAll(after.removal);
          now = after.now;
        }
        if (host.config.publicOrigin === "") return { result: "unavailable" };
        const session = await host.port.startCheckout({
          accountId,
          successUrl: `${host.config.publicOrigin}/v1/done`,
          expiresAt: now + SESSION_LENGTH_MS,
        });
        if (session.kind === "failed") return { result: "unavailable" };
        return step((open): CheckoutAnswer => {
          const kept = {
            accountId,
            credentialHash: hash,
            cap,
            sessionId: session.sessionId,
            url: session.url,
            expiresAt: session.expiresAt,
          };
          if (!holds(open.sql)) {
            const stepNow = time(open.sql);
            createTables(open.sql, stepNow);
            open.arm();
            accounts.keepPending(open.sql, kept, stepNow);
            return { result: "session", url: session.url };
          }
          const stepNow = open.begin();
          const look = accounts.look(open.sql, hash);
          if (look === "mismatch") return { result: "credential-mismatch" };
          if (look === "account") return { result: "account-exists" };
          if (look === "pending") {
            accounts.replacePendingCap(open.sql, cap);
            return { result: "session", url: (accounts.pending(open.sql) ?? contractBroken()).url };
          }
          accounts.keepPending(open.sql, kept, stepNow);
          return { result: "session", url: session.url };
        });
      }
    },

    async webhookEvent(event) {
      if (event.kind === "ignored") return;
      const after = step((open): { noRecord: boolean; overrode: boolean } => {
        if (!holds(open.sql)) return { noRecord: event.kind === "checkout-completed", overrode: false };
        const now = open.begin();
        const applied = accounts.applyEvent(open.sql, event, now);
        if (applied.kind === "account-made") {
          accountMade(open.sql, applied, event.accountId, now);
          return { noRecord: false, overrode: false };
        }
        if (applied.kind === "state") {
          if (!isMetered(open.sql)) return { noRecord: false, overrode: applied.overrode };
          if (applied.period !== null) metering.billingPeriod(open.sql, applied.period, now);
          if (applied.state === "cancelled" && applied.was !== "cancelled") {
            metering.cut(open.sql, "cancel", accounts.subscription(open.sql)?.cancelledAt ?? event.providerTime, now);
          }
          return { noRecord: false, overrode: applied.overrode };
        }
        if (applied.kind === "cancel-time") {
          if (isMetered(open.sql)) metering.cut(open.sql, "cancel", applied.at, now);
          return { noRecord: false, overrode: false };
        }
        if (event.kind === "checkout-completed" && applied.kind === "none") {
          const pending = accounts.pending(open.sql);
          return { noRecord: pending !== null && pending.sessionId !== event.sessionId, overrode: false };
        }
        return { noRecord: false, overrode: applied.overrode };
      });
      if (after.noRecord) log({ event: "completed-no-record" });
      if (after.overrode) log({ event: "cancel-overrides-order" });
    },

    async status(hash) {
      return guarded<StatusAnswer>(
        hash,
        { nothing: { result: "no-such-account" }, mismatch: { result: "credential-mismatch" }, pending: { result: "pending" } },
        (open) => ({ result: (accounts.subscription(open.sql) ?? contractBroken()).state }),
      );
    },

    async capGet(hash) {
      return guarded<CapGetAnswer>(
        hash,
        { nothing: { result: "no-such-account" }, mismatch: { result: "credential-mismatch" }, pending: { result: "pending" } },
        (open, now) => capAnswer(open.sql, now) ?? { result: "no-cap" },
      );
    },

    async capSet(hash, cap) {
      return guarded<CapSetAnswer>(
        hash,
        { nothing: { result: "no-such-account" }, mismatch: { result: "credential-mismatch" }, pending: { result: "pending" } },
        (open, now) => {
          ledger.setCap(open.sql, cap, now);
          return capAnswer(open.sql, now) ?? contractBroken();
        },
      );
    },

    async manage(hash) {
      const first = guarded<ManageAnswer | { customerId: string }>(
        hash,
        { nothing: { result: "no-such-account" }, mismatch: { result: "credential-mismatch" }, pending: { result: "pending" } },
        (open) => {
          const subscription = accounts.subscription(open.sql) ?? contractBroken();
          if (subscription.customerId === null || host.config.publicOrigin === "") return { result: "unavailable" };
          return { customerId: subscription.customerId };
        },
      );
      if ("result" in first) return first;
      const page = await host.port.openManagePage({
        customerId: first.customerId,
        returnUrl: `${host.config.publicOrigin}/v1/done`,
      });
      return page.kind === "page" ? { result: "page", url: page.url } : { result: "unavailable" };
    },

    async destroy(hash) {
      const answers = {
        nothing: { result: "no-such-account" as const },
        mismatch: { result: "credential-mismatch" as const },
        pending: { result: "pending" as const },
      };
      const first = guarded<DeleteAnswer | { metered: boolean }>(hash, answers, (open, now) => {
        const metered = isMetered(open.sql);
        if (destroyingSince(open.sql) === null) {
          open.sql.exec("UPDATE skeleton SET destroying_since = ?", now);
          if (metered) metering.cut(open.sql, "delete", now, now);
        }
        return { metered };
      });
      if ("result" in first) return first;
      if ((await destroyStepThree(first.metered)) === "gone") return { result: "deleted" };
      const fourth = guarded<DeleteAnswer | { subscriptionId: string | null; mark: Ms }>(
        hash,
        { ...answers, nothing: { result: "deleted" } },
        (open) => {
          const metered = isMetered(open.sql);
          const mark = destroyingSince(open.sql);
          const unfinished = mark === null || ledger.openHolds(open.sql) > 0 || (metered && !metering.settled(open.sql));
          if (unfinished) return { result: "not-finished" };
          return { subscriptionId: (accounts.subscription(open.sql) ?? contractBroken()).subscriptionId, mark };
        },
      );
      if ("result" in fourth) return fourth;
      const cancelled = await host.port.cancelSubscription(fourth.subscriptionId);
      if (cancelled.kind === "failed") return { result: "not-finished" };
      const last = step((open): DeleteAnswer | "delete" => {
        if (!holds(open.sql)) return { result: "deleted" };
        open.begin();
        if (accounts.look(open.sql, hash) !== "account" || destroyingSince(open.sql) !== fourth.mark) return { result: "not-finished" };
        open.beginWithoutAlarm();
        return "delete";
      });
      if (last !== "delete") return last;
      await host.deleteAll();
      return { result: "deleted" };
    },

    async reserve(hash, req) {
      const refused = (reason: Gate["reason"]): Gate => ({ outcome: "refused", reason });
      return guarded<ReserveResult | Gate>(
        hash,
        { nothing: refused("no-such-account"), mismatch: refused("credential-mismatch"), pending: refused("pending") },
        (open, now) => {
          if (destroyingSince(open.sql) !== null) return refused("closing");
          const interval = isMetered(open.sql) ? metering.intervalAt(open.sql, now) : 0;
          const active = accounts.isActive(open.sql);
          return ledger.reserve(open.sql, req, now, { active, interval });
        },
      );
    },

    async dispatch(requestId) {
      return step((open) => {
        if (!holds(open.sql)) return "stop";
        return ledger.dispatch(open.sql, requestId, open.begin());
      });
    },

    async settle(requestId, usage) {
      return step((open): EndResult => {
        if (!holds(open.sql)) return { outcome: "refused", reason: "unknown-request" };
        return ledger.settle(open.sql, requestId, usage, open.begin());
      });
    },

    async void(requestId) {
      return step((open): EndResult => {
        if (!holds(open.sql)) return { outcome: "refused", reason: "unknown-request" };
        return ledger.void(open.sql, requestId, open.begin());
      });
    },

    async alarm() {
      try {
        await alarmBody();
      } catch (error) {
        countFailure(error);
      }
    },

    async rearm() {
      step((open) => {
        if (holds(open.sql)) open.begin();
      });
    },

    async testSetClock(at) {
      if (!host.config.testClock) return { result: "refused" };
      const stored = step((open) => {
        if (!holds(open.sql)) return "no-such-account" as const;
        if (at < lastNow(open.sql)) return "behind" as const;
        storeTestTime(open.sql, at);
        open.beginWithoutAlarm();
        return "stored" as const;
      });
      if (stored !== "stored") return { result: stored };
      armAlarm(null);
      let before: Ms | null = null;
      for (let runs = 0; runs < CLOCK_RUN_LIMIT; runs += 1) {
        const due = host.step((sql) => (holds(sql) ? dueOf(sql) : null));
        if (due === null || due > at || (runs > 0 && due === before)) break;
        before = due;
        try {
          await alarmBody();
        } catch (error) {
          countFailure(error);
          break;
        }
      }
      return { result: "clock-set" };
    },

    async testSeed(a) {
      if (!host.config.testClock) return { result: "refused" };
      return step((open) => {
        if (holds(open.sql)) return { result: "not-empty" as const };
        const now = a.time ?? host.clock();
        createTables(open.sql, now);
        if (a.time !== undefined) storeTestTime(open.sql, a.time);
        open.arm();
        accounts.seed(open.sql, { accountId: a.accountId, credentialHash: a.hash, state: a.subscription, customerId: a.customerId ?? null }, now);
        if (a.customerId !== undefined) metering.begin(open.sql, a.accountId, now);
        if (a.cap !== undefined) ledger.setCap(open.sql, a.cap, now, a.anchor);
        return { result: "seeded" as const };
      });
    },

    async testInspect() {
      if (!host.config.testClock) return { result: "refused" };
      const read = host.step((sql) => {
        if (!holds(sql)) return null;
        const tables: Snapshot["tables"] = {};
        for (const name of ownTables(sql)) {
          tables[name] = sql.exec(`SELECT * FROM "${name.replaceAll('"', '""')}"`).map((each) => ({ ...each }));
        }
        return { test_time: testTime(sql), due: dueOf(sql), tables };
      });
      if (read === null) return { result: "no-such-account" };
      return {
        result: "snapshot",
        now: host.clock(),
        test_time: read.test_time,
        due: read.due,
        alarm: await host.getAlarm(),
        config: host.config,
        tables: read.tables,
      };
    },
  };
}
