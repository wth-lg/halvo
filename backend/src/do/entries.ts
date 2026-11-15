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
import { PROVIDER_CALL_TIMEOUT_MS } from "../billing/stripe/timeout.ts";
import { errorName, log } from "../log.ts";
import { type Metering, type Next, RETRY_DELAY_CAP_MS, SYNC_RETRY_MS, type UsageAnswer, type UsageCall } from "../metering/index.ts";
import type { Cap, Micro, Ms, Sql, Step, Tier } from "../types.ts";

export const ALARM_RETRY_MS = 30_000;
/**
 * Outside the mark, five alarm runs in a row that catch an error of the due time end with alarm-stopped and no alarm when
 * the exit's read of the mark shows no mark, while the entries still answer; one throw of the count's own write with no
 * mark stops at once (contract 3.4 line 556). When the run's read of the mark at its end fails as well, the alarm does not
 * stop and runs on at the pause: such an exit counts as under the mark (P7-110 L3 (a)).
 */
export const ALARM_RETRY_LIMIT = 5;
export const ALARM_CALL_LIMIT = 3;
export const START_RESTARTS = 2;
export const CLOCK_RUN_LIMIT = 100;
export const DELETE_REMOVAL_BOUND_MS = 86_400_000;
/**
 * The cap of the pause after a failing run of the alarm under the mark. The pause starts at ALARM_RETRY_MS and doubles with
 * each failing run in a row counted under the current mark, so a begun delete costs at most one run per ALARM_RETRY_MS,
 * settling to one run per ALARM_PAUSE_CAP_MS while an error repeats or no answer of the cancel comes. The count restarts at
 * a delete: the statement of destroy that first writes the mark writes it to 0. The count of failing runs is written by a
 * statement of its own and also kept in the instance's memory as a floor, under the mark's time. While the count of
 * failing runs cannot be stored, because its one statement fails or every read of the skeleton fails, and the object is
 * new at each run, the pause stays at ALARM_RETRY_MS: at most 2,881 runs a day, fewer when a cancel try is owed later. No
 * retry of the write is built, because a statement that fails in every run fails in its retry. Such a run makes a cancel
 * call only at the stored next try of a failed answer; a cancel call that throws stores no next try, so then each run
 * makes one.
 */
export const ALARM_PAUSE_CAP_MS = 3_600_000;
/**
 * The life of a stored record of a cancel call in flight, from the call's begin in the step's now: at least the provider
 * call's own timeout plus ALARM_RETRY_MS. A record at or after its begin plus this time is void for every reader.
 */
export const CALL_IN_FLIGHT_MS = PROVIDER_CALL_TIMEOUT_MS + ALARM_RETRY_MS;
/**
 * The bound of the await of a cancel call, on the host's timer: at least the provider call's own timeout and strictly
 * shorter than CALL_IN_FLIGHT_MS. At the bound the run goes on as for a failed call, and the call's record stands to its
 * expiry.
 */
export const CANCEL_CALL_BOUND_MS = PROVIDER_CALL_TIMEOUT_MS + ALARM_RETRY_MS / 2;
export interface Host {
  step: Step;
  clock(): Ms;
  /** A pause on the host's timer, never on a stored test time; the platform's timer when the host gives none. */
  pause?(ms: Ms): Promise<void>;
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
const CANCEL_CALL = "cancel_call";
const ENGINE_PREFIX = "sqlite_";
const PLATFORM_PREFIX = "_cf_";
const LOCAL_RUNTIME_TABLE = "__miniflare_do_name";
type Removal = "expired-unpaid" | "dropped" | "unanswered";

interface Mark {
  since: Ms | null;
  answeredAt: Ms | null;
  answeredFor: string | null;
  nextAt: Ms | null;
  delay: Ms | null;
  decidedAt: Ms | null;
}

type Fourth =
  | { kind: "answer"; answer: DeleteAnswer }
  | { kind: "again" }
  | { kind: "cancel"; subscriptionId: string | null; mark: Ms; unacknowledged: number; began: Ms | null }
  | { kind: "answered"; mark: Ms };

const NOT_FINISHED: DeleteAnswer = { result: "not-finished" };

/**
 * One run of the alarm, from its begin in alarm() or testSetClock until its exit has armed. `kept` is the earliest time at
 * which a step of an entry other than the run armed the alarm since the run began; the run's own steps record nothing.
 * Named limit (ruling P7-115 A5): two instances over one storage for a moment. A delete answered by the other instance is
 * not in this instance's memory; the count restarts in storage at the mark, so this instance's failing exit pauses
 * ALARM_RETRY_MS and arms at most one ALARM_RETRY_MS after its exit, and the delete's own arm may be overwritten by that.
 * A stored kept time is refused: a write in the arming step could roll that step back (P7-110 L1 (c)), and a read of the
 * platform's alarm needs an await.
 */
interface AlarmRun {
  mark: Mark | null | "unknown";
  kept: Ms | null;
}

interface Open {
  sql: Sql;
  begin(): Ms;
  beginWithoutAlarm(): Ms;
  arm(): void;
  caught(error: unknown): void;
}

class LoggedAlarmError extends Error {
  readonly first: unknown;
  constructor(first: unknown) {
    super("an error of the alarm's run under the mark, already logged");
    this.name = "LoggedAlarmError";
    this.first = first;
  }
}

/**
 * Who runs a step: its caught errors are logged with the caller's line and counted for the caller. `run` names the alarm run
 * whose own step this is; a step without it belongs to another entry.
 */
interface Caller {
  line: "alarm-error" | "destroy-error";
  caught: number;
  readonly run?: AlarmRun;
}

const byTheAlarm = (): Caller => ({ line: "alarm-error", caught: 0 });

/**
 * The arm of a step of the run, or of its failing exit, at `now`: the earlier of its own time and the bound, which is the
 * kept time when it lies after now, else now plus ALARM_RETRY_MS. With no kept time the arm is its own time.
 */
function boundByTheKeptTime(run: AlarmRun, at: Ms, now: Ms): Ms {
  if (run.kept === null) return at;
  return Math.min(at, run.kept > now ? run.kept : now + ALARM_RETRY_MS);
}

/** A read of the skeleton that found no mark, or found no skeleton: the one ground, beside a test time, to stop the alarm. */
interface NoMark {
  readonly ground: "no mark";
}

/** A test time stored in the object: with one, no step arms the platform's alarm. */
interface TestTime {
  readonly ground: "test time";
  readonly at: Ms;
}

function noMarkIn(mark: Mark | null): NoMark | null {
  return mark === null || mark.since === null ? { ground: "no mark" } : null;
}

function testTimeOf(at: Ms | null): TestTime | null {
  return at === null ? null : { ground: "test time", at };
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

function timeOrNull(value: string | number | null | undefined): Ms | null {
  return value === null || value === undefined ? null : Number(value);
}

function answeredFor(mark: Mark, subscriptionId: string | null): boolean {
  return mark.answeredAt !== null && mark.answeredFor === subscriptionId;
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
  const removalRejections = new WeakSet<object>();
  let dueErrorsOfTheRun: unknown[] | null = null;
  let runInFlight: AlarmRun | null = null;
  let removalLogged: Ms | null = null;
  let failuresKept: { since: Ms | null; count: number } | null = null;
  const sendsEndedHere = new Map<string, Ms>();
  const sendKey = (interval: Ms, sentAt: Ms) => `${interval}:${sentAt}`;
  const sendEnds = (sql: Sql, now: Ms): Ms | null => {
    for (const [key, sentAt] of sendsEndedHere) if (sentAt + CALL_IN_FLIGHT_MS <= now) sendsEndedHere.delete(key);
    const open = metering.sendsInFlight(sql, now, CALL_IN_FLIGHT_MS).filter((each) => !sendsEndedHere.has(sendKey(each.interval, each.sentAt)));
    return earliest(open.map((each) => each.sentAt + CALL_IN_FLIGHT_MS));
  };
  const planned = (call: UsageCall | null, now: Ms) => {
    if (call !== null && call.kind === "send") sendsEndedHere.set(sendKey(call.interval, now), now);
  };
  const leavesDestroy = (error: unknown) =>
    errorName(error) === "ObjectStoppedError" || (typeof error === "object" && error !== null && removalRejections.has(error));

  const tableExists = (sql: Sql, name: string) =>
    sql.exec("SELECT name FROM sqlite_master WHERE type = ? AND name = ?", "table", name).length > 0;
  const holds = (sql: Sql) => tableExists(sql, SKELETON);
  const skeletonRow = (sql: Sql) =>
    sql.exec(
      "SELECT destroying_since, last_now, alarm_failures, cancel_answered_at, cancel_answered_for, cancel_next_at, cancel_delay, removal_decided_at FROM skeleton",
    )[0] ??
    contractBroken();
  const lastNow = (sql: Sql) => Number(skeletonRow(sql).last_now);
  const markOf = (sql: Sql): Mark => {
    const row = skeletonRow(sql);
    const answeredFor = row.cancel_answered_for;
    return {
      since: timeOrNull(row.destroying_since),
      answeredAt: timeOrNull(row.cancel_answered_at),
      answeredFor: answeredFor === null || answeredFor === undefined ? null : String(answeredFor),
      nextAt: timeOrNull(row.cancel_next_at),
      delay: timeOrNull(row.cancel_delay),
      decidedAt: timeOrNull(row.removal_decided_at),
    };
  };
  const destroyingSince = (sql: Sql) => markOf(sql).since;
  const testTime = (sql: Sql): Ms | null => {
    if (!host.config.testClock || !tableExists(sql, TEST_CLOCK)) return null;
    const row = sql.exec("SELECT time FROM test_clock")[0];
    return row === undefined ? null : Number(row.time);
  };
  const time = (sql: Sql) => testTime(sql) ?? host.clock();
  const callRecord = (sql: Sql): { subscriptionId: string; beganAt: Ms } | null => {
    if (!tableExists(sql, CANCEL_CALL)) return null;
    const row = sql.exec("SELECT subscription_id, began_at FROM cancel_call")[0];
    return row === undefined ? null : { subscriptionId: String(row.subscription_id), beganAt: Number(row.began_at) };
  };
  const callEnds = (sql: Sql, subscriptionId: string | null, now: Ms): Ms | null => {
    const record = callRecord(sql);
    if (record === null || subscriptionId === null || record.subscriptionId !== subscriptionId) return null;
    const ends = record.beganAt + CALL_IN_FLIGHT_MS;
    return now < ends ? ends : null;
  };
  const storeCallRecord = (sql: Sql, subscriptionId: string, began: Ms) => {
    sql.exec("CREATE TABLE IF NOT EXISTS cancel_call (subscription_id TEXT NOT NULL, began_at INTEGER NOT NULL)");
    sql.exec("DELETE FROM cancel_call");
    sql.exec("INSERT INTO cancel_call (subscription_id, began_at) VALUES (?, ?)", subscriptionId, began);
  };
  const clearCallRecord = (sql: Sql, subscriptionId: string | null, began: Ms | null) => {
    if (subscriptionId === null || began === null || !tableExists(sql, CANCEL_CALL)) return;
    sql.exec("DELETE FROM cancel_call WHERE subscription_id = ? AND began_at = ?", subscriptionId, began);
  };
  const isMetered = (sql: Sql) => {
    const subscription = accounts.subscription(sql);
    return subscription !== null && subscription.customerId !== null;
  };

  /**
   * The delete term of the due time under the mark, keyed by the subscription id that the account row holds now: with an
   * answer stored for that id, the stored decision's retry, or the removal's bound while no decision is stored; else the end
   * of a cancel call in flight for that id; else the end of a send in flight, the stored next try, and the mark itself while no hold and no send is open.
   */
  function deleteTerm(sql: Sql, mark: Mark & { since: Ms }, subscriptionId: string | null): Ms | null {
    if (answeredFor(mark, subscriptionId)) return mark.decidedAt === null ? (mark.answeredAt as Ms) + DELETE_REMOVAL_BOUND_MS : mark.decidedAt + SYNC_RETRY_MS;
    const now = Math.max(time(sql), lastNow(sql));
    const callEnd = callEnds(sql, subscriptionId, now);
    if (callEnd !== null) return callEnd;
    const sendEnd = sendEnds(sql, now);
    if (mark.nextAt !== null) return earliest([sendEnd, mark.nextAt]);
    if (sendEnd !== null) return sendEnd;
    return ledger.openHoldsAt(sql, Math.max(time(sql), lastNow(sql))) === 0 ? mark.since : null;
  }

  function dueOf(sql: Sql): Ms | null {
    const metered = isMetered(sql);
    const times = [ledger.nextDue(sql), accounts.nextDue(sql)];
    if (metered) {
      times.push(metering.nextSend(sql));
      const u = metering.oldestUnsealed(sql);
      if (u !== null && ledger.intervalTotals(sql, u.interval).open === 0) times.push(u.end);
    }
    const mark = markOf(sql);
    const subscription = accounts.subscription(sql);
    if (mark.since !== null && subscription !== null) times.push(deleteTerm(sql, { ...mark, since: mark.since }, subscription.subscriptionId));
    return earliest(times);
  }

  /**
   * Arms the platform's alarm at a time; it never takes null, so no arm can clear the alarm under the mark. The arms: a
   * step's (step), each bounded by the kept time when the step is the run's own, and the failing exit's two (countFailure),
   * each bounded the same way. Answers whether the arm was set.
   */
  function armAlarm(at: Ms): boolean {
    try {
      host.setAlarm(at);
      return true;
    } catch (caught) {
      log({ event: "alarm-error", error: errorName(caught) });
      return false;
    }
  }

  /**
   * The one function that stops the alarm. Its ground is a read that found no mark or a stored test time. The sites: the
   * stop at the limit of failing runs outside the mark (countFailure), the test-time branches of a step's arm (step) and of
   * the count step's two arms (countFailure), and testSetClock. host.deleteAll removes the alarm with the storage
   * (account-object.ts); after a rejected removal of a destroy from outside nothing arms again (unit 5.10's).
   */
  function stopAlarm(ground: NoMark | TestTime): void {
    void ground;
    try {
      host.setAlarm(null);
    } catch (caught) {
      log({ event: "alarm-error", error: errorName(caught) });
    }
  }

  function step<T>(run: (open: Open) => T, by: Caller = byTheAlarm()): T {
    let arm = false;
    let due: Ms | null = null;
    let stepNow: Ms = 0;
    let stop: NoMark | TestTime | null = null;
    let errors: unknown[] = [];
    const value = host.step((sql) => {
      errors = [];
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
        caught: (error) => {
          errors.push(error);
        },
      });
      if (arm) {
        stop = testTimeOf(testTime(sql));
        try {
          due = dueOf(sql);
        } catch (error) {
          errors.push(error);
          dueErrorsOfTheRun?.push(error);
          due = host.clock() + ALARM_RETRY_MS;
        }
        const now = Math.max(time(sql), lastNow(sql));
        stepNow = now;
        let mark: Mark | null | "unknown" = "unknown";
        try {
          mark = markOf(sql);
        } catch {}
        const noMark = mark === "unknown" ? null : noMarkIn(mark);
        if (due === null) {
          if (noMark !== null) stop = stop ?? noMark;
          else due = now + ALARM_RETRY_MS;
        } else if (dueErrorsOfTheRun !== null && noMark === null && due <= now) due = now + ALARM_RETRY_MS;
      }
      return result;
    });
    for (const error of errors) log({ event: by.line, error: errorName(error) });
    by.caught += errors.length;
    if (arm) {
      if (stop !== null) stopAlarm(stop);
      else if (due !== null) {
        const at = by.run === undefined ? due : boundByTheKeptTime(by.run, due, stepNow);
        if (armAlarm(at) && by.run === undefined && runInFlight !== null) runInFlight.kept = earliest([runInFlight.kept, at]);
      }
    }
    return value;
  }

  function createTables(sql: Sql, now: Ms): void {
    sql.exec(
      "CREATE TABLE skeleton (destroying_since INTEGER, last_now INTEGER NOT NULL, alarm_failures INTEGER NOT NULL, cancel_answered_at INTEGER, cancel_answered_for TEXT, cancel_next_at INTEGER, cancel_delay INTEGER, removal_decided_at INTEGER)",
    );
    sql.exec(
      "INSERT INTO skeleton (destroying_since, last_now, alarm_failures, cancel_answered_at, cancel_answered_for, cancel_next_at, cancel_delay, removal_decided_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
      null,
      now,
      0,
      null,
      null,
      null,
      null,
      null,
    );
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

  async function sendAndRecord(by: Caller = byTheAlarm()): Promise<"gone" | "done"> {
    const sent = step((open) => {
      if (!holds(open.sql)) return null;
      const now = open.begin();
      ledger.expire(open.sql, now);
      const next = metering.send(open.sql, now);
      return { next, ids: next.call === null ? null : idsOf(open.sql), now };
    }, by);
    if (sent === null) return "gone";
    planned(sent.next.call, sent.now);
    logNotBillable(sent.next);
    let call = sent.next.call;
    let ids = sent.ids;
    let plannedAt = sent.now;
    for (let calls = 0; call !== null && ids !== null && calls < ALARM_CALL_LIMIT; calls += 1) {
      const made = call;
      const key = sendKey(made.interval, plannedAt);
      if (made.kind === "send") sendsEndedHere.delete(key);
      let answer: UsageAnswer;
      try {
        answer = await callPort(made, ids);
      } finally {
        if (made.kind === "send") sendsEndedHere.set(key, plannedAt);
      }
      const recorded = step((open) => {
        if (!holds(open.sql)) return null;
        const now = open.begin();
        ledger.expire(open.sql, now);
        if (!isMetered(open.sql)) return "not-metered" as const;
        const next = metering.record(open.sql, made, answer, now);
        return { next, ids: next.call === null ? null : idsOf(open.sql), now };
      }, by);
      if (recorded === null) return "gone";
      if (recorded === "not-metered") return "done";
      planned(recorded.next.call, recorded.now);
      logNotBillable(recorded.next);
      call = recorded.next.call;
      ids = recorded.ids;
      plannedAt = recorded.now;
    }
    return "done";
  }

  async function alarmBody(run: AlarmRun): Promise<void> {
    const outer = dueErrorsOfTheRun;
    const dueErrors: unknown[] = [];
    dueErrorsOfTheRun = dueErrors;
    try {
      await alarmRun(run, dueErrors);
    } finally {
      dueErrorsOfTheRun = outer;
    }
  }

  async function alarmRun(run: AlarmRun, dueErrors: unknown[]): Promise<void> {
    const caught: unknown[] = [];
    const byTheRun = (): Caller => ({ line: "alarm-error", caught: 0, run });
    let head: { mark: Mark; failures: number } | null | "unknown";
    try {
      head = host.step((sql) => (holds(sql) ? { mark: markOf(sql), failures: Number(skeletonRow(sql).alarm_failures) } : null));
    } catch (error) {
      caught.push(error);
      log({ event: "alarm-error", error: errorName(error) });
      head = "unknown";
    }
    if (head === null) return;
    run.mark = head === "unknown" ? "unknown" : head.mark;
    const marked = head === "unknown" || head.mark.since !== null;
    async function part<T>(work: () => Promise<T> | T): Promise<T | undefined> {
      if (!marked) return work();
      try {
        return await work();
      } catch (error) {
        caught.push(error);
        log({ event: "alarm-error", error: errorName(error) });
        return undefined;
      }
    }
    const since = head === "unknown" ? null : head.mark.since;
    if (since !== null) await part(() => cutAtTheMark(since, byTheRun()));
    const first = await part(() =>
      step((open) => {
        if (!holds(open.sql)) return null;
        const now = open.begin();
        ledger.expire(open.sql, now);
        if (isMetered(open.sql)) sealLoop(open.sql, now);
        accounts.forget(open.sql, now);
        return { wake: accounts.wake(open.sql, now), metered: isMetered(open.sql) };
      }, byTheRun()),
    );
    if (first === null) return;
    let metered = first?.metered ?? true;
    const wake = first?.wake ?? null;
    if (wake !== null) {
      const sessionId = wake.sessionId;
      const after = await part(async () => {
        const answer = await host.port.readSession(sessionId);
        return step((open) => {
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
        }, byTheRun());
      });
      if (after === null) return;
      if (after !== undefined) {
        if (after.removal !== null) {
          await removeAll(after.removal);
          return;
        }
        metered = after.metered;
      }
    }
    let thrown: { error: unknown } | null = null;
    try {
      if (metered && (await sendAndRecord(byTheRun())) === "gone") return;
    } catch (error) {
      thrown = { error };
    }
    if (marked) {
      let finished: Awaited<ReturnType<typeof finishDelete>> = { answer: NOT_FINISHED, thrown: null };
      try {
        finished = await finishDelete(null, byTheRun());
      } catch (error) {
        if (thrown === null) throw error;
        log({ event: "alarm-error", error: errorName(error) });
      }
      thrown = thrown ?? finished.thrown;
      if (finished.answer.result === "deleted") {
        if (thrown !== null) log({ event: "alarm-error", error: errorName(thrown.error) });
        return;
      }
    }
    if (thrown !== null) throw thrown.error;
    if (caught.length > 0) throw new LoggedAlarmError(caught[0]);
    if (dueErrors.length > 0) throw new LoggedAlarmError(dueErrors[0]);
    failuresKept = null;
    if (head === "unknown" || head.failures > 0) {
      step((open) => {
        if (!holds(open.sql)) return;
        open.begin();
        open.sql.exec("UPDATE skeleton SET alarm_failures = ?", 0);
      }, byTheRun());
    }
  }

  function countFailure(error: unknown, run: AlarmRun): void {
    const logged = error instanceof LoggedAlarmError;
    const name = errorName(logged ? error.first : error);
    if (!logged) log({ event: "alarm-error", error: name });
    let exit: { mark: Mark } | null | "unknown";
    try {
      exit = host.step((sql) => (holds(sql) ? { mark: markOf(sql) } : null));
    } catch (reading) {
      log({ event: "alarm-error", error: errorName(reading) });
      exit = "unknown";
    }
    if (exit === null) return;
    let marked = run.mark === "unknown" || (run.mark !== null && run.mark.since !== null) || exit === "unknown" || (exit !== null && exit.mark.since !== null);
    const since = exit === "unknown" ? null : exit.mark.since;
    const kept = failuresKept !== null && (exit === "unknown" || failuresKept.since === since) ? failuresKept.count : 0;
    let count = ALARM_RETRY_LIMIT;
    let tested: TestTime | null = null;
    let waits: Ms | null = null;
    let nextAt: Ms | null = null;
    let removal: { overdue: boolean } | null = null;
    let dueNow: { due: Ms; now: Ms } | null = null;
    let removalOfAnyId: Ms | null = null;
    const deleteDuringTheRun = run.mark !== null && run.mark !== "unknown" && run.mark.since === null && exit !== "unknown" && exit.mark.since !== null;
    let keptSince: Ms | null | "unknown" = exit === "unknown" ? "unknown" : since;
    try {
      const counted = host.step((sql) => {
        const mark = markOf(sql);
        const markedNow = marked || mark.since !== null;
        const prior = Number(skeletonRow(sql).alarm_failures);
        if (failuresKept !== null && failuresKept.since !== mark.since) failuresKept = null;
        const failures = markedNow ? Math.max(prior, failuresKept?.count ?? 0) + 1 : Math.min(prior + 1, ALARM_RETRY_LIMIT);
        sql.exec("UPDATE skeleton SET alarm_failures = ?", failures);
        return { failures, tested: testTimeOf(testTime(sql)), marked: markedNow, nextAt: mark.nextAt, since: mark.since };
      });
      count = counted.failures;
      tested = counted.tested;
      marked = counted.marked;
      nextAt = counted.nextAt;
      keptSince = counted.since;
    } catch (counting) {
      log({ event: "alarm-error", error: errorName(counting) });
      count = marked ? kept + 1 : ALARM_RETRY_LIMIT;
    }
    if (marked) failuresKept = { since: keptSince === "unknown" ? (failuresKept?.since ?? null) : keptSince, count };
    try {
      host.step((sql) => {
        sql.exec("UPDATE skeleton SET last_now = ?", Math.max(time(sql), lastNow(sql)));
      });
    } catch (writing) {
      log({ event: "alarm-error", error: errorName(writing) });
    }
    if (marked) {
      try {
        const owed = host.step((sql) => {
          const mark = markOf(sql);
          if (mark.since === null) return null;
          const subscriptionId = (accounts.subscription(sql) ?? contractBroken()).subscriptionId;
          const answered = answeredFor(mark, subscriptionId);
          const now = Math.max(time(sql), lastNow(sql));
          let due: Ms | null = null;
          if (deleteDuringTheRun) {
            try {
              due = dueOf(sql);
            } catch {}
          }
          return {
            term: deleteTerm(sql, { ...mark, since: mark.since }, subscriptionId),
            removal: answered ? { overdue: now >= (mark.answeredAt as Ms) + DELETE_REMOVAL_BOUND_MS } : null,
            dueNow: due === null ? null : { due, now },
          };
        });
        waits = owed?.term ?? null;
        removal = owed?.removal ?? null;
        dueNow = owed?.dueNow ?? null;
      } catch {
        waits = nextAt;
        if (exit !== "unknown" && exit.mark.answeredAt !== null) {
          removalOfAnyId = exit.mark.decidedAt === null ? exit.mark.answeredAt + DELETE_REMOVAL_BOUND_MS : exit.mark.decidedAt + SYNC_RETRY_MS;
        }
      }
    }
    if (marked) {
      const pause = Math.min(ALARM_RETRY_MS * 2 ** Math.min(count - 1, 30), ALARM_PAUSE_CAP_MS);
      let at = Math.max(host.clock() + pause, waits ?? 0);
      if (removal !== null) at = Math.min(at, Math.max(waits ?? 0, host.clock() + SYNC_RETRY_MS));
      if (removalOfAnyId !== null) at = Math.min(at, Math.max(removalOfAnyId, host.clock() + SYNC_RETRY_MS));
      if (dueNow !== null) at = Math.min(at, dueNow.due > dueNow.now ? dueNow.due : dueNow.now + ALARM_RETRY_MS);
      at = boundByTheKeptTime(run, at, host.clock());
      if (removal?.overdue === true) log({ event: "removal-overdue" });
      if (tested !== null) stopAlarm(tested);
      else armAlarm(at);
      return;
    }
    if (count < ALARM_RETRY_LIMIT) {
      if (tested !== null) stopAlarm(tested);
      else armAlarm(boundByTheKeptTime(run, host.clock() + ALARM_RETRY_MS, host.clock()));
      return;
    }
    const noMark = exit === "unknown" ? null : noMarkIn(exit.mark);
    if (noMark === null) return;
    log({ event: "alarm-stopped", error: name });
    stopAlarm(noMark);
  }

  function guarded<T>(hash: string, answers: { nothing: T; mismatch: T; pending: T }, onAccount: (open: Open, now: Ms) => T, by: Caller = byTheAlarm()): T {
    return step((open) => {
      if (!holds(open.sql)) return answers.nothing;
      const now = open.begin();
      const look = accounts.look(open.sql, hash);
      if (look === "nothing") return answers.nothing;
      if (look === "mismatch") return answers.mismatch;
      if (look === "pending") return answers.pending;
      return onAccount(open, now);
    }, by);
  }

  function capAnswer(sql: Sql, now: Ms): { result: "cap"; cap: Cap; spend: { spent: Micro; held: Micro; period_end: Ms } } | null {
    const read = ledger.cap(sql, now);
    if (read === null) return null;
    return { result: "cap", cap: read.cap, spend: { spent: read.spent, held: read.held, period_end: read.periodEnd } };
  }

  async function destroyStepThree(by: Caller = byTheAlarm()): Promise<"gone" | "done"> {
    const sealed = step((open) => {
      if (!holds(open.sql)) return null;
      const now = open.begin();
      ledger.expire(open.sql, now);
      const metered = isMetered(open.sql);
      if (metered) sealLoop(open.sql, now);
      return { metered };
    }, by);
    if (sealed === null) return "gone";
    return sealed.metered ? sendAndRecord(by) : "done";
  }

  function cutAtTheMark(mark: Ms, by: Caller = byTheAlarm()): void {
    step((open) => {
      if (!holds(open.sql)) return;
      const now = open.begin();
      if (isMetered(open.sql)) metering.cut(open.sql, "delete", mark, now);
    }, by);
  }

  function holder(sql: Sql, hash: string | null): DeleteAnswer | null {
    if (hash === null) return accounts.subscription(sql) === null ? NOT_FINISHED : null;
    const look = accounts.look(sql, hash);
    if (look === "nothing") return { result: "deleted" };
    if (look === "mismatch") return { result: "credential-mismatch" };
    if (look === "pending") return { result: "pending" };
    return null;
  }

  function fourth(hash: string | null, again: boolean, by: Caller): Fourth {
    return step((open): Fourth => {
      if (!holds(open.sql)) return { kind: "answer", answer: { result: "deleted" } };
      const now = open.begin();
      const refused = holder(open.sql, hash);
      if (refused !== null) return { kind: "answer", answer: refused };
      const mark = markOf(open.sql);
      if (mark.since === null || ledger.openHoldsAt(open.sql, now) > 0) return { kind: "answer", answer: NOT_FINISHED };
      if (!again) {
        try {
          if (isMetered(open.sql) && metering.oldestUnsealed(open.sql) !== null) return { kind: "again" };
        } catch (error) {
          open.caught(error);
        }
      }
      const subscriptionId = (accounts.subscription(open.sql) ?? contractBroken()).subscriptionId;
      if (answeredFor(mark, subscriptionId)) return { kind: "answered", mark: mark.since };
      if (callEnds(open.sql, subscriptionId, now) !== null) return { kind: "answer", answer: NOT_FINISHED };
      if (sendEnds(open.sql, now) !== null) return { kind: "answer", answer: NOT_FINISHED };
      if (hash === null && mark.nextAt !== null && now < mark.nextAt) return { kind: "answer", answer: NOT_FINISHED };
      let unacknowledged = 0;
      try {
        unacknowledged = isMetered(open.sql) ? metering.unacknowledged(open.sql) : 0;
      } catch (error) {
        open.caught(error);
      }
      if (subscriptionId !== null) storeCallRecord(open.sql, subscriptionId, now);
      return { kind: "cancel", subscriptionId, mark: mark.since, unacknowledged, began: subscriptionId === null ? null : now };
    }, by);
  }

  async function finishDelete(hash: string | null, by: Caller = byTheAlarm()): Promise<{ answer: DeleteAnswer; thrown: { error: unknown } | null }> {
    let thrown: { error: unknown } | null = null;
    let next = fourth(hash, false, by);
    if (next.kind === "again") {
      try {
        if ((await destroyStepThree(by)) === "gone") return { answer: { result: "deleted" }, thrown };
      } catch (error) {
        thrown = { error };
      }
      next = fourth(hash, true, by);
    }
    return { answer: await finishFrom(hash, next, by), thrown };
  }

  async function finishFrom(hash: string | null, next: Fourth, by: Caller): Promise<DeleteAnswer> {
    if (next.kind === "answer") return next.answer;
    if (next.kind === "again") return contractBroken();
    const mark = next.mark;
    const made = next.kind === "cancel" ? { subscriptionId: next.subscriptionId } : null;
    if (made !== null) {
      for (let each = 0; next.kind === "cancel" && each < next.unacknowledged; each += 1) log({ event: "cancel-before-send" });
      const id = made.subscriptionId;
      const began = next.kind === "cancel" ? next.began : null;
      const clear = () => {
        try {
          step((open) => {
            if (!holds(open.sql)) return;
            open.begin();
            clearCallRecord(open.sql, id, began);
          }, by);
        } catch (clearing) {
          log({ event: by.line, error: errorName(clearing) });
        }
      };
      const settled = (async () => host.port.cancelSubscription(id))().then(
        (answer) => ({ answer }),
        (error: unknown) => ({ error }),
      );
      const pause = host.pause ?? ((ms: Ms) => new Promise<void>((resolve) => setTimeout(resolve, ms)));
      const first = await Promise.race([settled, pause(CANCEL_CALL_BOUND_MS).then(() => null)]);
      let cancelled: Awaited<ReturnType<ProviderPort["cancelSubscription"]>>;
      if (first === null) {
        log({ event: "cancel-call-unsettled" });
        void settled.then(clear);
        cancelled = { kind: "failed" };
      } else if ("error" in first) {
        clear();
        throw first.error;
      } else cancelled = first.answer;
      if (cancelled.kind === "failed") {
        const unanswered = step((open): { answer: DeleteAnswer; capped: boolean } => {
          if (!holds(open.sql)) return { answer: { result: "deleted" }, capped: false };
          const now = open.begin();
          if (first !== null) clearCallRecord(open.sql, id, began);
          const current = markOf(open.sql);
          if (holder(open.sql, hash) !== null || current.since !== mark) return { answer: NOT_FINISHED, capped: false };
          const delay = current.delay ?? SYNC_RETRY_MS;
          open.sql.exec("UPDATE skeleton SET cancel_next_at = ?, cancel_delay = ?", now + delay, Math.min(2 * delay, RETRY_DELAY_CAP_MS));
          return { answer: NOT_FINISHED, capped: delay >= RETRY_DELAY_CAP_MS };
        }, by);
        if (unanswered.capped) log({ event: "cancel-unanswered" });
        return unanswered.answer;
      }
    }
    if (made !== null) {
      const began = next.kind === "cancel" ? next.began : null;
      const recorded = step((open): DeleteAnswer | null => {
        if (!holds(open.sql)) return { result: "deleted" };
        const now = open.begin();
        clearCallRecord(open.sql, made.subscriptionId, began);
        if (holder(open.sql, hash) !== null || destroyingSince(open.sql) !== mark) return NOT_FINISHED;
        const current = markOf(open.sql);
        const subscriptionId = (accounts.subscription(open.sql) ?? contractBroken()).subscriptionId;
        if (!answeredFor(current, made.subscriptionId) && !answeredFor(current, subscriptionId)) {
          open.sql.exec(
            "UPDATE skeleton SET cancel_answered_at = ?, cancel_answered_for = ?, cancel_next_at = ?, cancel_delay = ?, removal_decided_at = ?",
            now,
            made.subscriptionId,
            null,
            null,
            null,
          );
        }
        return null;
      }, by);
      if (recorded !== null) return recorded;
    }
    const last = step((open): DeleteAnswer | { decidedAt: Ms; closed: number | { unknown: unknown } } => {
      if (!holds(open.sql)) return { result: "deleted" };
      const now = open.begin();
      if (holder(open.sql, hash) !== null || destroyingSince(open.sql) !== mark) return NOT_FINISHED;
      const stored = markOf(open.sql);
      const subscriptionId = (accounts.subscription(open.sql) ?? contractBroken()).subscriptionId;
      if (stored.answeredAt === null || !answeredFor(stored, subscriptionId)) return NOT_FINISHED;
      const closedAtBound = (): number | { unknown: unknown } => {
        try {
          return isMetered(open.sql) ? metering.closedAtBound(open.sql) : 0;
        } catch (error) {
          open.caught(error);
          return { unknown: error };
        }
      };
      if (stored.decidedAt !== null) return { decidedAt: stored.decidedAt, closed: closedAtBound() };
      if (now >= stored.answeredAt + DELETE_REMOVAL_BOUND_MS) {
        let unknown: { unknown: unknown } | null = null;
        try {
          if (isMetered(open.sql)) metering.closeOpen(open.sql, now);
        } catch (error) {
          open.caught(error);
          unknown = { unknown: error };
        }
        open.sql.exec("UPDATE skeleton SET removal_decided_at = ?", now);
        return { decidedAt: now, closed: unknown ?? closedAtBound() };
      }
      const metered = isMetered(open.sql);
      if (ledger.openHoldsAt(open.sql, now) > 0 || (metered && !metering.settled(open.sql))) return NOT_FINISHED;
      open.sql.exec("UPDATE skeleton SET removal_decided_at = ?", now);
      return { decidedAt: now, closed: closedAtBound() };
    }, by);
    if ("result" in last) return last;
    if (removalLogged !== last.decidedAt) {
      removalLogged = last.decidedAt;
      if (typeof last.closed === "number") logNotBillable({ call: null, notBillable: last.closed });
      else log({ event: "usage-not-billable", error: errorName(last.closed.unknown) });
    }
    try {
      await host.deleteAll();
    } catch (error) {
      if (typeof error === "object" && error !== null) removalRejections.add(error);
      throw error;
    }
    return { result: "deleted" };
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
        (open) => {
          const subscription = accounts.subscription(open.sql) ?? contractBroken();
          const mark = markOf(open.sql);
          if (mark.since === null) return { result: subscription.state };
          return { result: answeredFor(mark, subscription.subscriptionId) ? "cancelled" : "closing" };
        },
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
      const by: Caller = { line: "destroy-error", caught: 0 };
      const first = guarded<DeleteAnswer | { cut: Ms | null }>(
        hash,
        answers,
        (open, now) => {
          if (destroyingSince(open.sql) !== null) return { cut: null };
          open.sql.exec("UPDATE skeleton SET destroying_since = ?, alarm_failures = 0", now);
          return { cut: now };
        },
        by,
      );
      if ("result" in first) return first;
      if (first.cut !== null) failuresKept = null;
      let failed = false;
      let leave: { error: unknown } | null = null;
      const caught = (error: unknown) => {
        if (leavesDestroy(error)) {
          leave = leave ?? { error };
          return;
        }
        failed = true;
        log({ event: "destroy-error", error: errorName(error) });
      };
      const mark = first.cut;
      if (mark !== null) {
        try {
          cutAtTheMark(mark, by);
        } catch (error) {
          caught(error);
        }
      }
      try {
        if ((await destroyStepThree(by)) === "gone") return { result: "deleted" };
      } catch (error) {
        caught(error);
      }
      let finished: Awaited<ReturnType<typeof finishDelete>> = { answer: NOT_FINISHED, thrown: null };
      try {
        finished = await finishDelete(hash, by);
      } catch (error) {
        caught(error);
      }
      if (finished.thrown !== null) caught(finished.thrown.error);
      if (leave !== null) throw (leave as { error: unknown }).error;
      if (!failed && by.caught === 0) return finished.answer;
      return host.step((sql) => holds(sql)) ? finished.answer : { result: "deleted" };
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
      const run: AlarmRun = { mark: "unknown", kept: null };
      const outer = runInFlight;
      runInFlight = run;
      try {
        await alarmBody(run);
      } catch (error) {
        countFailure(error, run);
      } finally {
        runInFlight = outer;
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
      stopAlarm({ ground: "test time", at });
      let before: Ms | null = null;
      for (let runs = 0; runs < CLOCK_RUN_LIMIT; runs += 1) {
        const due = host.step((sql) => (holds(sql) ? dueOf(sql) : null));
        if (due === null || due > at || (runs > 0 && due === before)) break;
        before = due;
        const run: AlarmRun = { mark: "unknown", kept: null };
        const outer = runInFlight;
        runInFlight = run;
        try {
          await alarmBody(run);
        } catch (error) {
          countFailure(error, run);
          break;
        } finally {
          runInFlight = outer;
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
