import { makeAccounts } from "../../../src/accounts/index.ts";
import type { ProviderEvent, ProviderPort } from "../../../src/billing/port.ts";
import { type Entries, makeEntries, ownTables } from "../../../src/do/entries.ts";
import { makeLedger } from "../../../src/ledger/index.ts";
import { makeMetering } from "../../../src/metering/index.ts";
import { makeTestHost } from "../../sql/node.ts";
import { FakeMeter, type FakeMeterConfig, type MeterEvent, type SendResult } from "../fake-meter.ts";

export const HOUR = 3600;
export const DAY = 24 * HOUR;
export const RULE_MICRO_PER_MINOR = 10_000;
export const BILLING_PERIOD = 30 * DAY;
export const HOLD_LIFE = 900;
export const RETRY_AFTER = 60;
export const WINDOW_MARGIN = 300;
export const LATE_DELIVERY = 90;
export const LATE_RESPONSE = 20;
export const READ_WIDTH_SECONDS = 60;
if (LATE_DELIVERY > WINDOW_MARGIN || LATE_RESPONSE > WINDOW_MARGIN) {
  throw new RangeError("a late delivery and a late response arrive inside the window margin");
}
const CLOCK_START = 1_800_000_000 + 17;
const PAST_EVERY_OPEN_BILLING_PERIOD = BILLING_PERIOD + 4 * DAY;
const SETTLING_TIME = 2 * DAY;
const EVENT_LIMIT = 200_000;
const UNIT_PRICE = { input: 1_000_000, output: 1_000_000 };
const CAP_MINOR = 100_000_000;

export type FaultKind = "none" | "request-lost" | "response-lost" | "refused" | "late-delivery" | "late-response" | "crash-before-call" | "crash-after-call";

export type Fault =
  | { readonly kind: Exclude<FaultKind, "crash-before-call" | "crash-after-call"> }
  | { readonly kind: "crash-before-call" | "crash-after-call"; readonly downSeconds: number };

const NO_FAULT: Fault = { kind: "none" };

export type CancelForm = "stated" | "unstated-then-stated" | "unstated-reversed";

export type Command =
  | { readonly kind: "advance"; readonly seconds: number }
  | { readonly kind: "advance-to-period-end"; readonly account: number; readonly secondsBeforePeriodEnd: number }
  | { readonly kind: "reserve"; readonly account: number; readonly amountMicro: number; readonly dispatch: boolean }
  | { readonly kind: "settle"; readonly account: number; readonly holdsBackFromNewest: number; readonly percentOfHold: number }
  | { readonly kind: "crash"; readonly account: number; readonly downSeconds: number }
  | { readonly kind: "cancel"; readonly account: number; readonly form: CancelForm; readonly lagSeconds: number }
  | { readonly kind: "late-cancel-event"; readonly account: number }
  | { readonly kind: "reactivate"; readonly account: number }
  | { readonly kind: "delete"; readonly account: number };

export type Scenario = {
  readonly subscribeSecondsAfterClockStart: readonly number[];
  readonly meter: Omit<FakeMeterConfig, "invoicePeriodSeconds">;
  readonly intervalSeconds: number;
  readonly capPeriod: "week" | "month";
  readonly capAnchorSecondsBeforeSubscription: number;
  readonly commands: readonly Command[];
  readonly faultsByProviderCall: readonly Fault[];
  readonly repeatAnswer?: "duplicate" | "failed";
};

export type Options = { readonly downtimeBudgetSeconds: number; readonly onFailedCheck: "stop" | "list" };
export type Finding = { readonly check: string; readonly detail: string };
export type PeriodRow = {
  readonly account: string;
  readonly period: number;
  readonly reported: number;
  readonly carryIn: number;
  readonly provider: number;
  readonly carryOut: number;
  readonly invoicedMinor: number;
};
export type Report = {
  readonly callsWhileFaultsOn: number;
  readonly findings: readonly Finding[];
  readonly coverage: Readonly<Record<string, number>>;
  readonly periods: readonly PeriodRow[];
  readonly facts: Readonly<Record<string, number>>;
};

class Stop extends Error {}

type Row = Record<string, string | number | null>;
type ModelHold = { id: string; amount: number; period: number; expiresAt: number; open: boolean; dispatched: boolean; cost: number | null };
type Blocker = { promise: Promise<"blocked">; release(): void };

type Account = {
  index: number;
  id: string;
  hash: string;
  customer: string;
  subscribedAt: number;
  test: ReturnType<typeof makeTestHost>;
  entries: Entries;
  downUntil: number;
  generation: number;
  blocker: Blocker | null;
  inFlight: Promise<void> | null;
  pendingAnswer: { arrivesAt: number; deliver(): void } | null;
  holds: ModelHold[];
  reported: Map<number, number>;
  reportedTotal: number;
  ended: "cancel" | "delete" | null;
  deleting: boolean;
  destroyStarted: boolean;
  cancelled: boolean;
  periodsAnnounced: number;
  events: number;
  eventsHeldWhenEndCompleted: number | null;
  unexplainedReads: number;
  refused: Map<string, { amountMinor: number; timestamp: number }>;
  queuedCancel: ProviderEvent | null;
  cutAt: number | null;
  lastProviderTime: number;
  reactivateEventsSent: number;
  cancelSinceReactivation: boolean;
  writeOff: { amount: number; sealed: Set<number>; laterSeals: Set<number> } | null;
  lastSeen: { carry: number; writtenOff: number | null } | null;
  cancelsAfterAReactivation: number;
  closedNotBillable: Set<string>;
};

function blocker(): Blocker {
  let release = () => {};
  const promise = new Promise<"blocked">((resolve) => {
    release = () => resolve("blocked");
  });
  return { promise, release };
}

function never<T>(): Promise<T> {
  return new Promise<T>(() => {});
}

function int(row: Row | undefined, key: string): number {
  const value = row?.[key];
  return typeof value === "number" ? value : Number.NaN;
}

export async function runScenario(scenario: Scenario, options: Options): Promise<Report> {
  const world = new World(scenario, options);
  const original = console.log;
  console.log = (line: unknown) => world.logLine(String(line));
  try {
    await world.run();
  } catch (error) {
    if (!(error instanceof Stop)) world.threw(error);
  } finally {
    console.log = original;
    world.collectFacts();
    world.close();
  }
  return world.report();
}

class World {
  private now = 0;
  private readonly meter: FakeMeter;
  private readonly accounts: Account[] = [];
  private readonly lateDeliveries: { at: number; event: MeterEvent }[] = [];
  private readonly findings: Finding[] = [];
  private readonly coverage: Record<string, number> = {};
  private readonly rows: PeriodRow[] = [];
  private nextFault = 0;
  private callsWhileFaultsOn = 0;
  private faultsOn = true;
  private clockMoveStopsForAPendingAnswer = false;
  private movedTheCutBack = false;
  private closing = false;
  private downtimeLeft: number;
  private readonly scenario: Scenario;
  private readonly options: Options;

  constructor(scenario: Scenario, options: Options) {
    this.scenario = scenario;
    this.options = options;
    this.downtimeLeft = options.downtimeBudgetSeconds;
    if (!(scenario.meter.graceSeconds > HOLD_LIFE)) throw new RangeError("the grace period of the provider is longer than the life of a hold");
    if (scenario.meter.readGranularitySeconds !== READ_WIDTH_SECONDS) {
      throw new RangeError(`a scenario states a read width of its own (${scenario.meter.readGranularitySeconds} s), and the model reads READ_WIDTH_SECONDS (${READ_WIDTH_SECONDS} s)`);
    }
    const anchorBefore = scenario.capAnchorSecondsBeforeSubscription;
    if (!(anchorBefore >= 1 && anchorBefore < 7 * DAY)) throw new RangeError("the cap anchor lies 1 second or more, and less than a week, before the subscription");
    this.meter = new FakeMeter({ ...scenario.meter, invoicePeriodSeconds: BILLING_PERIOD });
  }

  close(): void {
    for (const account of this.accounts) account.test.close();
  }

  logLine(line: string): void {
    let event = "unreadable";
    try {
      event = String((JSON.parse(line) as { event?: unknown }).event);
    } catch {}
    this.count(`log.${event}`);
    if (event === "alarm-error" || event === "alarm-stopped") this.findings.push({ check: "the alarm caught an error", detail: line });
  }

  threw(error: unknown): void {
    const name = error instanceof Error ? error.name : "a value";
    const message = error instanceof Error ? error.message.split("\n")[0] : String(error);
    this.findings.push({ check: "an entry threw", detail: `${name}: ${message}` });
  }

  private readonly facts: Record<string, number> = {};

  collectFacts(): void {
    const add = (key: string, by: number): void => {
      this.facts[key] = (this.facts[key] ?? 0) + by;
    };
    for (const account of this.accounts) {
      const events = this.meter.recordedEvents(account.id);
      const atTheMeter = new Set(events.map((e) => e.identifier));
      const rows = account.test.host.step((sql) =>
        ownTables(sql).length === 0 ? [] : sql.exec("SELECT identifier, refusal FROM metering_interval WHERE outcome = ?", "not-billable"),
      );
      add("not-billable-held-by-the-meter", rows.filter((row) => atTheMeter.has(String(row.identifier))).length);
      add("not-billable-at-the-send-limit", rows.filter((row) => row.refusal === "send_limit").length);
      add("reactivate-events-sent", account.reactivateEventsSent);
      add("cancels-after-a-reactivate-event", account.cancelsAfterAReactivation);
      const periods = this.meter.periods(account.id, this.now);
      const end = periods.at(-1)?.end;
      const ended = periods.length > 0 && end !== undefined && account.ended !== null;
      add("minor-units-stamped-after-the-end", ended ? this.meter.exactTotal(account.id, end, Number.POSITIVE_INFINITY) : 0);
      add("accounts-deleted", account.ended === "delete" ? 1 : 0);
    }
  }

  report(): Report {
    return { callsWhileFaultsOn: this.callsWhileFaultsOn, findings: this.findings, coverage: this.coverage, periods: this.rows, facts: this.facts };
  }

  private count(key: string, by = 1): void {
    this.coverage[key] = (this.coverage[key] ?? 0) + by;
  }

  private fail(check: string, detail: string): void {
    this.findings.push({ check, detail });
    if (this.options.onFailedCheck === "stop") throw new Stop();
  }

  private modules() {
    return {
      accounts: makeAccounts(),
      ledger: makeLedger({ holdLifeMs: HOLD_LIFE * 1000 }),
      metering: makeMetering({ intervalSeconds: this.scenario.intervalSeconds, marginSeconds: WINDOW_MARGIN }),
    };
  }

  async run(): Promise<void> {
    const order = [...this.scenario.subscribeSecondsAfterClockStart.entries()].sort((a, b) => a[1] - b[1] || a[0] - b[0]);
    for (const [index, after] of order) await this.subscribe(index, CLOCK_START + after);
    this.accounts.sort((a, b) => a.index - b.index);
    this.now = CLOCK_START + Math.max(...this.scenario.subscribeSecondsAfterClockStart);
    for (const command of this.scenario.commands) {
      await this.apply(command);
      await this.advanceTo(this.now);
      this.checkStep();
    }
    await this.closingPhase();
    this.checkStep();
    this.checkEnd();
  }

  private async subscribe(index: number, at: number): Promise<void> {
    this.now = at;
    const id = `00000000-0000-4000-8000-${String(index + 1).padStart(12, "0")}`;
    const account = { index, id } as Account;
    const port = this.port(account);
    const test = makeTestHost({
      port,
      clock: () => this.now * 1000,
      config: { holdLifeMs: HOLD_LIFE * 1000, intervalSeconds: this.scenario.intervalSeconds, marginSeconds: WINDOW_MARGIN },
    });
    Object.assign(account, {
      hash: String(index + 1).repeat(64),
      customer: `cus_${index + 1}`,
      subscribedAt: at,
      test,
      entries: makeEntries(test.host, this.modules()),
      downUntil: 0,
      generation: 0,
      blocker: null,
      inFlight: null,
      pendingAnswer: null,
      holds: [],
      reported: new Map(),
      reportedTotal: 0,
      ended: null,
      deleting: false,
      destroyStarted: false,
      cancelled: false,
      periodsAnnounced: 0,
      events: 0,
      eventsHeldWhenEndCompleted: null,
      unexplainedReads: 0,
      refused: new Map(),
      queuedCancel: null,
      cutAt: null,
      lastProviderTime: 0,
      reactivateEventsSent: 0,
      cancelSinceReactivation: false,
      writeOff: null,
      lastSeen: null,
      cancelsAfterAReactivation: 0,
      closedNotBillable: new Set(),
    } satisfies Omit<Account, "index" | "id">);
    this.meter.subscribe(id, at);
    this.accounts.push(account);
    const deleteAll = test.host.deleteAll;
    test.host.deleteAll = async () => {
      this.readRemainder(account);
      this.readRecordAgainstTheProvider(account, null);
      this.readStoredState(account, "delete");
      this.count("remainder.read-before-the-delete");
      await deleteAll();
    };
    const seeded = await account.entries.testSeed({
      accountId: id,
      hash: account.hash,
      subscription: "active",
      customerId: account.customer,
      cap: { amount: CAP_MINOR, period: this.scenario.capPeriod },
      anchor: (at - this.scenario.capAnchorSecondsBeforeSubscription) * 1000,
    });
    if (seeded.result !== "seeded") this.fail("seed", `${id} answered ${seeded.result}`);
    await this.announcePeriod(account);
  }

  private subscriptionEvent(account: Account, over: Partial<Extract<ProviderEvent, { kind: "subscription" }>>): ProviderEvent {
    account.events += 1;
    account.lastProviderTime = Math.max(account.lastProviderTime, over.providerTime ?? this.now * 1000);
    return {
      kind: "subscription",
      eventId: `evt_${account.index + 1}_${account.events}`,
      providerTime: this.now * 1000,
      accountId: account.id,
      subscriptionId: `sub_${account.index + 1}`,
      customerId: account.customer,
      state: "active",
      period: null,
      endedAt: null,
      ...over,
    };
  }

  private async announcePeriod(account: Account): Promise<void> {
    const start = account.subscribedAt + account.periodsAnnounced * BILLING_PERIOD;
    account.periodsAnnounced += 1;
    if (account.ended !== null) return;
    this.count("period.announced");
    this.countAnOpenIntervalAcross(account, start, start + BILLING_PERIOD);
    await account.entries.webhookEvent(
      this.subscriptionEvent(account, { providerTime: start * 1000, period: { start: start * 1000, end: (start + BILLING_PERIOD) * 1000 } }),
    );
  }

  private countAnOpenIntervalAcross(account: Account, start: number, end: number): void {
    const across = account.test.host.step(
      (sql) =>
        ownTables(sql).length > 0 &&
        sql.exec("SELECT start FROM metering_interval WHERE sealed_at IS NULL AND ((start < ? AND end_at > ?) OR (start < ? AND end_at > ?))", start * 1000, start * 1000, end * 1000, end * 1000)
          .length > 0,
    );
    if (across) this.count("period.learned-while-an-open-interval-spans-its-edge");
  }

  private port(account: Account): ProviderPort {
    const failed = { kind: "failed" as const };
    const seconds = () => this.now;
    return {
      startCheckout: async () => failed,
      verifyWebhook: async () => ({ kind: "rejected" }),
      readSession: async () => failed,
      openManagePage: async () => failed,
      cancelSubscription: async () => {
        this.meter.endSubscription(account.id, seconds());
        this.count("call.cancel");
        return { kind: "cancelled" };
      },
      sendUsage: async (e) => {
        const fault = this.takeFault();
        const attempt = this.attemptOf(account, e.identifier);
        this.count(attempt);
        const event: MeterEvent = { account: e.accountId, identifier: e.identifier, amountMinor: e.amountMinor, timestamp: e.timestamp };
        if (e.accountId !== account.id || e.customerId !== account.customer) this.fail("a call for another account", `${account.id} sent for ${e.accountId}`);
        switch (fault.kind) {
          case "none":
            return this.answerOf(this.deliver(event));
          case "request-lost":
            return failed;
          case "response-lost":
            this.deliver(event);
            return failed;
          case "refused":
            account.refused.set(e.identifier, { amountMinor: e.amountMinor, timestamp: e.timestamp });
            return { kind: "refused", reason: "unknown_account" };
          case "late-delivery":
            this.lateDeliveries.push({ at: this.now + LATE_DELIVERY, event });
            return failed;
          case "late-response": {
            const answer = this.answerOf(this.deliver(event));
            return this.answerLater(account, answer);
          }
          case "crash-before-call":
            return this.crashDuringCall(account, fault.downSeconds);
          case "crash-after-call":
            this.deliver(event);
            return this.crashDuringCall(account, fault.downSeconds);
        }
      },
      readUsageTotal: async (a) => {
        const fault = this.takeFault();
        this.count("call.read");
        if (fault.kind === "request-lost" || fault.kind === "response-lost" || fault.kind === "refused") return failed;
        if (fault.kind === "crash-before-call" || fault.kind === "crash-after-call") return this.crashDuringCall(account, fault.downSeconds);
        const read = this.meter.read(a.accountId, a.from, a.to, seconds());
        this.explain(account, a.from, read.widenedFrom, read.widenedTo, read.totalMinor);
        return { kind: "total", from: read.widenedFrom, to: read.widenedTo, totalMinor: read.totalMinor };
      },
    };
  }

  private attemptOf(account: Account, identifier: string): string {
    const row = account.test.host.step(
      (sql) => sql.exec("SELECT attempts, first_attempt, unanswered FROM metering_interval WHERE identifier = ?", identifier)[0],
    );
    const attempts = int(row, "attempts");
    if (attempts <= 1) return "send.first-attempt";
    const firstAttempt = int(row, "first_attempt");
    return this.now * 1000 >= firstAttempt + 86_400_000 - WINDOW_MARGIN * 1000 ? "send.resend-after-the-window" : "send.retry-inside-the-window";
  }

  private explain(account: Account, from: number, widenedFrom: number, widenedTo: number, total: number): void {
    const rows = account.test.host.step((sql) =>
      sql.exec("SELECT amount, timestamp, outcome FROM metering_interval WHERE sealed_at IS NOT NULL AND timestamp >= ? AND timestamp < ?", widenedFrom, widenedTo),
    );
    let others = 0;
    let own = Number.NaN;
    for (const row of rows) {
      if (int(row, "timestamp") === from) own = int(row, "amount");
      else if (row.outcome === "acknowledged") others += int(row, "amount");
    }
    if (others > 0) this.count("read.range-holds-another-interval");
    if (rows.some((row) => int(row, "timestamp") !== from)) this.count("read.range-holds-another-sealed-interval");
    if (total === others) this.count("read.lacks-the-amount");
    else if (total === others + own) this.count("read.holds-the-amount");
    else if (total < others + own && this.scenario.meter.readLagSeconds > 0) this.count("read.short-by-the-lag");
    else account.unexplainedReads += 1;
  }

  private takeFault(): Fault {
    const fault: Fault = (this.faultsOn ? this.scenario.faultsByProviderCall[this.nextFault] : undefined) ?? NO_FAULT;
    this.nextFault += 1;
    if (this.faultsOn) this.callsWhileFaultsOn += 1;
    this.count(`fault.${fault.kind}`);
    if (!this.closing) this.count("call.before-the-closing-phase");
    return fault;
  }

  private answerOf(
    result: SendResult,
  ): { kind: "recorded" } | { kind: "duplicate" } | { kind: "failed" } | { kind: "refused"; reason: Extract<SendResult, { status: "refused" }>["reason"] } {
    if (result.status === "refused") return { kind: "refused", reason: result.reason };
    if (result.status === "duplicate" && this.scenario.repeatAnswer === "failed") {
      this.count("send.repeat-answered-failed");
      return { kind: "failed" };
    }
    return { kind: result.status };
  }

  private answerLater<T>(account: Account, answer: T): Promise<T> {
    this.clockMoveStopsForAPendingAnswer = true;
    return new Promise<T>((resolve) => {
      account.pendingAnswer = { arrivesAt: this.now + LATE_RESPONSE, deliver: () => resolve(answer) };
      account.blocker?.release();
    });
  }

  private crashDuringCall<T>(account: Account, downSeconds: number): Promise<T> {
    this.crash(account, downSeconds);
    return never<T>();
  }

  private deliver(event: MeterEvent): SendResult {
    const result = this.meter.send(event, this.now);
    this.count(`meter.${result.status}`);
    if (result.status === "refused") this.fail("refused by the provider", `${event.identifier}: ${result.reason}`);
    return result;
  }

  private crash(account: Account, downSeconds: number): void {
    const down = Math.min(downSeconds, this.downtimeLeft);
    this.downtimeLeft -= down;
    account.generation += 1;
    account.entries = makeEntries(account.test.host, this.modules());
    account.pendingAnswer = null;
    account.inFlight = null;
    account.downUntil = this.now + down;
    account.blocker?.release();
    account.entries.rearm().catch((error: unknown) => this.threw(error));
    if (down >= DAY) this.count("crash.down-past-the-window");
  }

  private async drive(account: Account, start: () => Promise<unknown>): Promise<void> {
    const generation = account.generation;
    const gate = blocker();
    account.blocker = gate;
    const done = start().then(() => "done" as const);
    const first = await Promise.race([done, gate.promise]);
    if (account.blocker === gate) account.blocker = null;
    if (first === "done" || account.generation !== generation) return;
    account.inFlight = done.then(() => {});
  }

  private async resume(account: Account): Promise<void> {
    const pending = account.pendingAnswer;
    const inFlight = account.inFlight;
    if (pending === null || inFlight === null) return;
    account.pendingAnswer = null;
    account.inFlight = null;
    await this.drive(account, () => {
      pending.deliver();
      return inFlight;
    });
  }

  private async apply(command: Command): Promise<void> {
    if (command.kind === "advance") {
      await this.advanceTo(this.now + command.seconds);
      return;
    }
    const account = this.accounts[command.account % this.accounts.length];
    if (account === undefined) return;
    if (command.kind === "advance-to-period-end") {
      const periods = Math.floor((this.now - account.subscribedAt) / BILLING_PERIOD) + 1;
      await this.advanceTo(Math.max(this.now, account.subscribedAt + periods * BILLING_PERIOD - command.secondsBeforePeriodEnd));
      return;
    }
    if (this.now < account.downUntil) {
      this.count("command.dropped-while-down");
      return;
    }
    if (account.inFlight !== null) this.count("step.during-provider-call");
    switch (command.kind) {
      case "reserve":
        await this.reserve(account, command.amountMicro, command.dispatch);
        return;
      case "settle":
        await this.settle(account, command.holdsBackFromNewest, command.percentOfHold);
        return;
      case "crash":
        this.crash(account, command.downSeconds);
        this.count("crash.idle");
        return;
      case "cancel":
        await this.end(account, "cancel", command.form, command.lagSeconds);
        this.readStoredState(account, "event");
        return;
      case "late-cancel-event":
        await this.lateCancel(account);
        this.readStoredState(account, "event");
        return;
      case "reactivate":
        await this.reactivate(account);
        return;
      case "delete":
        await this.end(account, "delete", "stated", 0);
        return;
    }
  }

  private async reserve(account: Account, amountMicro: number, dispatch: boolean): Promise<void> {
    const id = `r${account.index + 1}-${account.holds.length + 1}`;
    const answer = await account.entries.reserve(account.hash, { requestId: id, tier: "low_cost_hosted", amount: amountMicro, price: UNIT_PRICE });
    const admitted = "outcome" in answer && answer.outcome === "admitted";
    if (account.cancelled || account.destroyStarted) {
      if (admitted) this.fail("reserve after the end", `${account.id} admitted a hold after ${account.ended}`);
      return;
    }
    if (!admitted) {
      if (account.deleting) return;
      this.fail("reserve", `${account.id} refused a hold while active: ${JSON.stringify(answer)}`);
      return;
    }
    const period = Math.floor((this.now - account.subscribedAt) / BILLING_PERIOD);
    const untilPeriodEnd = account.subscribedAt + (period + 1) * BILLING_PERIOD - this.now;
    if (untilPeriodEnd <= HOLD_LIFE) this.count("reserve.in-the-last-900-s-of-a-billing-period");
    if (untilPeriodEnd === 1) this.count("reserve.in-the-last-second-of-a-billing-period");
    const before = account.holds.at(-1);
    if (before !== undefined && before.period !== period) this.count("hold.first-of-a-billing-period");
    let dispatched = false;
    if (dispatch) {
      const go = await account.entries.dispatch(id);
      if (go !== "go") this.fail("dispatch", `${account.id} answered ${go} for an open hold`);
      dispatched = true;
      this.count("hold.dispatched");
    }
    account.holds.push({ id, amount: amountMicro, period, expiresAt: this.now + HOLD_LIFE, open: true, dispatched, cost: null });
    this.count("reserve");
  }

  private async settle(account: Account, back: number, percent: number): Promise<void> {
    const count = account.holds.length;
    const hold = count === 0 ? undefined : account.holds[count - 1 - (back % count)];
    if (hold === undefined) return;
    const cost = Math.floor((hold.amount * percent) / 100);
    const answer = await account.entries.settle(hold.id, { inputTokens: cost, outputTokens: 0 });
    if (!hold.open) {
      this.count("settle.repeat-or-after-expiry");
      return;
    }
    if (this.now >= hold.expiresAt) {
      hold.open = false;
      this.count("settle.after-expiry");
      const refusedAs = answer.outcome === "refused" ? answer.reason : null;
      const ended = refusedAs === "already-ended" || (refusedAs === "unknown-request" && !this.holdsStorage(account));
      if (!hold.dispatched && !ended) {
        this.fail("a settle after a voided hold's expiry", `${hold.id} answered ${JSON.stringify(answer)}`);
      }
      return;
    }
    hold.open = false;
    hold.cost = cost;
    if (answer.outcome !== "settled" || answer.cost !== cost) this.fail("settle", `${hold.id} answered ${JSON.stringify(answer)}, the driver's cost ${cost}`);
    account.reported.set(hold.period, (account.reported.get(hold.period) ?? 0) + cost);
    account.reportedTotal += cost;
    this.count("settle.counted");
    if (cost > hold.amount) this.count("settle.above-the-hold");
    if (Math.floor((this.now - account.subscribedAt) / BILLING_PERIOD) > hold.period) this.count("settle.counted-after-its-billing-period-ended");
    if (account.ended !== null || account.deleting) this.count("settle.after-cancel-or-delete");
  }

  private async end(account: Account, kind: "cancel" | "delete", form: CancelForm, lagSeconds: number): Promise<void> {
    const previous = account.ended;
    const openHolds = account.holds.some((h) => h.open && this.now < h.expiresAt);
    if (kind === "cancel") {
      account.cancelled = true;
      account.cancelSinceReactivation = true;
      if (account.reactivateEventsSent > 0) account.cancelsAfterAReactivation += 1;
      const endedAt = this.meter.endSubscription(account.id, this.now);
      const first = Math.max(endedAt * 1000, account.lastProviderTime + 1000);
      if (first <= account.lastProviderTime) {
        this.fail("the provider stamps a cancel before an event it sent earlier", `${account.id}: cancel at ${first}, an earlier event at ${account.lastProviderTime}`);
      }
      const stated = this.subscriptionEvent(account, { state: "cancelled", providerTime: first, endedAt: endedAt * 1000 });
      if (form === "stated" || lagSeconds <= 0) {
        this.cancelArrives(account, stated);
        await account.entries.webhookEvent(stated);
      } else {
        const later = first + lagSeconds * 1000;
        this.count(`cancel.${form}`);
        const unstated = this.subscriptionEvent(account, { state: "cancelled", providerTime: later });
        this.cancelArrives(account, unstated);
        await account.entries.webhookEvent(unstated);
        account.queuedCancel = form === "unstated-then-stated" ? stated : this.subscriptionEvent(account, { state: "cancelled", providerTime: first });
      }
      if (previous === null) account.ended = "cancel";
    } else {
      account.ended = "delete";
      account.deleting = true;
      await this.destroy(account);
    }
    if (previous === "delete") {
      this.count(`${kind}.after-delete`);
      return;
    }
    this.count(previous === null ? kind : `${kind}.after-cancel`);
    if (previous === null && openHolds) this.count(`${kind}.with-open-hold`);
  }

  private async lateCancel(account: Account): Promise<void> {
    const event = account.queuedCancel;
    if (event === null) return;
    account.queuedCancel = null;
    const sealed = account.test.host.step((sql) =>
      ownTables(sql).length === 0 ? 0 : sql.exec("SELECT start FROM metering_interval WHERE sealed_at IS NOT NULL AND end_at > ?", Number(event.kind === "subscription" ? event.providerTime : 0)).length,
    );
    this.count(sealed > 0 ? "cancel.late-event-after-the-seal" : "cancel.late-event-before-the-seal");
    this.cancelArrives(account, event);
    await account.entries.webhookEvent(event);
  }

  private cancelArrives(account: Account, event: ProviderEvent): void {
    if (event.kind !== "subscription") return;
    const at = (event.endedAt ?? event.providerTime) / 1000;
    if (account.cutAt === null) {
      account.cutAt = at;
      return;
    }
    if (at >= account.cutAt) return;
    const stays = account.test.host.step(
      (sql) => ownTables(sql).length === 0 || sql.exec("SELECT start FROM metering_interval WHERE sealed_at IS NOT NULL AND end_at > ?", at * 1000).length > 0,
    );
    if (stays) return;
    account.cutAt = at;
    const acrossAHold = account.test.host.step(
      (sql) =>
        sql.exec(
          "SELECT i.start FROM metering_interval i WHERE i.sealed_at IS NULL AND i.end_at > ? AND EXISTS (SELECT 1 FROM ledger_holds h WHERE h.sync_interval = i.start AND h.state IN ('open', 'settled'))",
          at * 1000,
        ).length > 0,
    );
    if (acrossAHold && !this.movedTheCutBack) this.count("ordering.moved-the-cut-back-across-an-unsealed-interval-with-a-hold");
    if (acrossAHold) this.movedTheCutBack = true;
  }

  private async reactivate(account: Account): Promise<void> {
    if (account.ended !== "cancel" || account.destroyStarted || account.deleting) return;
    const period = Math.floor((this.now - account.subscribedAt) / BILLING_PERIOD);
    const start = account.subscribedAt + period * BILLING_PERIOD;
    account.reactivateEventsSent += 1;
    this.count("reactivate");
    const providerTime = Math.max(this.now * 1000, account.lastProviderTime + 1000);
    this.countAnOpenIntervalAcross(account, start, start + BILLING_PERIOD);
    await account.entries.webhookEvent(
      this.subscriptionEvent(account, { state: "active", providerTime, period: { start: start * 1000, end: (start + BILLING_PERIOD) * 1000 } }),
    );
    this.readStoredState(account, "reactivate");
  }

  private readStoredState(account: Account, after: "reactivate" | "event" | "delete"): void {
    const read = account.test.host.step((sql) => {
      if (ownTables(sql).length === 0) return null;
      const row = sql.exec("SELECT state, ended_at, cancel_first FROM account")[0];
      const metering = sql.exec("SELECT cut_kind FROM metering_account")[0];
      return row === undefined
        ? null
        : { state: row.state, endedAt: row.ended_at, cancelFirst: row.cancel_first, metered: metering !== undefined, cutKind: metering?.cut_kind ?? null };
    });
    if (read === null) return;
    this.count("stored-state.read");
    if (read.state === "cancelled") {
      this.count("stored-state.cancelled");
      if (read.endedAt === null && read.cancelFirst === null) {
        this.fail("a cancelled account keeps no cancel time", `${account.id}: state cancelled with ended_at null and cancel_first null`);
      } else if (read.cancelFirst === null) {
        this.count("stored-state.cancelled-with-a-stated-end-and-no-first-cancel");
      }
      if (read.metered && read.cutKind === null) this.fail("a cancelled account has no cut", `${account.id}: state cancelled, metered, cut_kind null`);
    }
    if (after === "reactivate" && read.state !== "cancelled") {
      this.fail("a cancelled account changed its state", `${account.id}: the state is ${read.state} after an active event`);
    }
    const cancelKept = read.endedAt !== null || read.cancelFirst !== null;
    if (read.state !== "cancelled" && (cancelKept || read.cutKind === "cancel")) {
      this.fail(
        "the stored state disagrees with the cancel",
        `${account.id}: state ${read.state} with ended_at ${read.endedAt}, cancel_first ${read.cancelFirst}, cut_kind ${read.cutKind}`,
      );
    }
  }

  private readRecordAgainstTheProvider(account: Account, rows: Row[] | null): void {
    const intervals =
      rows ??
      account.test.host.step((sql) => (ownTables(sql).length === 0 ? [] : sql.exec("SELECT start, identifier, outcome FROM metering_interval WHERE outcome = ?", "not-billable")));
    const held = new Set(this.meter.recordedEvents(account.id).map((e) => e.identifier));
    for (const interval of intervals) {
      if (interval.outcome === "not-billable") account.closedNotBillable.add(String(interval.identifier));
      if (interval.outcome !== "not-billable" || !held.has(String(interval.identifier))) continue;
      this.fail("the record disagrees with the provider", `${account.id} interval ${interval.start} is not billable while the meter holds ${interval.identifier}`);
    }
  }

  private readRemainder(account: Account): void {
    const read = account.test.host.step((sql) => {
      if (ownTables(sql).length === 0) return null;
      const row = sql.exec("SELECT carry, written_off FROM metering_account")[0];
      if (row === undefined) return null;
      return { carry: Number(row.carry), writtenOff: row.written_off === null ? null : Number(row.written_off), sealed: sql.exec("SELECT start, reported FROM metering_interval WHERE sealed_at IS NOT NULL") };
    });
    if (read === null) return;
    account.lastSeen = { carry: read.carry, writtenOff: read.writtenOff };
    if (!account.cancelSinceReactivation && !account.destroyStarted) return;
    this.count("remainder.read");
    if (read.writtenOff === null) {
      if (account.writeOff !== null) this.fail("the amount written off changed", `${account.id}: from ${account.writeOff.amount} to null`);
      return;
    }
    if (account.writeOff === null) {
      account.writeOff = { amount: read.writtenOff, sealed: new Set(read.sealed.map((r) => Number(r.start))), laterSeals: new Set() };
      this.count("remainder.write-off-seen");
    } else if (read.writtenOff !== account.writeOff.amount) {
      this.fail("the amount written off changed", `${account.id}: from ${account.writeOff.amount} to ${read.writtenOff}`);
    }
    if (read.carry !== 0) this.fail("a carry after the write-off", `${account.id}: carry ${read.carry}, written off ${read.writtenOff}`);
    for (const row of read.sealed) {
      const start = Number(row.start);
      if (account.writeOff.sealed.has(start) || account.writeOff.laterSeals.has(start)) continue;
      account.writeOff.laterSeals.add(start);
      this.count("interval.sealed-after-the-write-off");
      if (Number(row.reported) > 0) this.fail("an interval with usage sealed after the write-off", `${account.id} interval ${start}: reported ${row.reported}`);
    }
  }

  private async destroy(account: Account): Promise<void> {
    if (!account.deleting || account.inFlight !== null || this.now < account.downUntil) return;
    let answer: unknown = null;
    account.destroyStarted = true;
    await this.drive(account, async () => {
      answer = await account.entries.destroy(account.hash);
    });
    const result = (answer as { result?: string } | null)?.result;
    if (result === "deleted" || result === "no-such-account") account.deleting = false;
    else if (answer !== null && result !== "not-finished") this.fail("account delete", `${account.id} answered ${JSON.stringify(answer)}`);
  }

  private async wake(account: Account): Promise<void> {
    this.count("wake");
    account.test.host.setAlarm(null);
    await this.drive(account, () => account.entries.alarm());
    await this.destroy(account);
  }

  private async advanceTo(target: number): Promise<void> {
    this.clockMoveStopsForAPendingAnswer = false;
    for (let fired = 0; ; fired += 1) {
      if (fired > EVENT_LIMIT) {
        this.fail("settling", `more than ${EVENT_LIMIT} events fired in one clock move`);
        throw new Stop();
      }
      let at = Number.POSITIVE_INFINITY;
      let fire: (() => Promise<void>) | null = null;
      for (const [index, delivery] of this.lateDeliveries.entries()) {
        if (delivery.at < at) {
          at = delivery.at;
          fire = async () => {
            this.lateDeliveries.splice(index, 1);
            this.deliver(delivery.event);
          };
        }
      }
      for (const account of this.accounts) {
        const pending = account.pendingAnswer;
        if (pending !== null) {
          if (pending.arrivesAt < at) {
            at = pending.arrivesAt;
            fire = () => this.resume(account);
          }
          continue;
        }
        if (account.inFlight !== null) continue;
        const renewal = account.subscribedAt + account.periodsAnnounced * BILLING_PERIOD;
        if (account.ended === null && Math.max(renewal, account.downUntil) < at) {
          at = Math.max(renewal, account.downUntil);
          fire = () => this.announcePeriod(account);
        }
        const alarm = account.test.alarmAt();
        if (alarm === null) continue;
        const due = Math.max(Math.ceil(alarm / 1000), account.downUntil);
        if (due < at) {
          at = due;
          fire = () => this.wake(account);
        }
      }
      if (fire === null || at > target) break;
      this.now = Math.max(this.now, at);
      await fire();
      this.checkStep();
      if (this.clockMoveStopsForAPendingAnswer && !this.closing) return;
    }
    this.now = Math.max(this.now, target);
  }

  private async closingPhase(): Promise<void> {
    this.closing = true;
    await this.advanceTo(this.now + PAST_EVERY_OPEN_BILLING_PERIOD);
    this.faultsOn = false;
    await this.advanceTo(Math.max(this.now, ...this.accounts.map((account) => account.downUntil)));
    for (const account of this.accounts) await this.destroy(account);
    await this.advanceTo(this.now + SETTLING_TIME);
    for (const account of this.accounts) await this.destroy(account);
  }

  private holdsStorage(account: Account): boolean {
    return account.test.host.step((sql) => ownTables(sql).length > 0);
  }

  private objectEnded(account: Account): boolean {
    if (!this.holdsStorage(account)) return true;
    return account.test.host.step((sql) => {
      const row = sql.exec("SELECT written_off FROM metering_account")[0];
      const open = sql.exec("SELECT start FROM metering_interval WHERE outcome IS NULL").length;
      return row !== undefined && row.written_off !== null && open === 0;
    });
  }

  private checkStep(): void {
    for (const account of this.accounts) {
      const everything = this.meter.exactTotal(account.id, Number.NEGATIVE_INFINITY, Number.POSITIVE_INFINITY);
      if (everything * RULE_MICRO_PER_MINOR > account.reportedTotal) {
        this.fail(
          "the provider-side total passes the reported total",
          `${account.id}: provider ${everything * RULE_MICRO_PER_MINOR}, reported ${account.reportedTotal}`,
        );
      }
      if (account.unexplainedReads > 0) this.fail("a read the sync cannot explain", account.id);
      if (account.cancelSinceReactivation || account.destroyStarted) this.readRemainder(account);
      if (account.ended !== null && account.eventsHeldWhenEndCompleted === null && account.inFlight === null && this.objectEnded(account)) {
        account.eventsHeldWhenEndCompleted = this.meter.recordedEvents(account.id).length;
      }
    }
  }

  private tables(account: Account): { intervals: Row[]; meteringAccount: Row | null; holds: Row[] } | null {
    return account.test.host.step((sql) => {
      if (ownTables(sql).length === 0) return null;
      return {
        intervals: sql.exec("SELECT * FROM metering_interval ORDER BY start"),
        meteringAccount: sql.exec("SELECT * FROM metering_account")[0] ?? null,
        holds: sql.exec("SELECT request_id, state, cost, amount, dispatched FROM ledger_holds"),
      };
    });
  }

  private checkHolds(account: Account, holds: Row[]): void {
    for (const hold of account.holds) {
      const row = holds.find((each) => each.request_id === hold.id);
      const want = hold.cost !== null ? "settled" : hold.dispatched ? "expired" : "voided";
      if (row === undefined || row.state !== want || (want === "settled" && row.cost !== hold.cost)) {
        this.fail("a hold ends as the ledger's rule says", `${hold.id}: stored ${row?.state ?? "nothing"}, expected ${want}`);
      }
      if (want === "voided") this.count("hold.voided-at-expiry");
      if (want === "expired") this.count("hold.expired-at-its-amount");
    }
  }

  private checkEnd(): void {
    for (const account of this.accounts) {
      const view = this.tables(account);
      if (account.ended === "delete") {
        if (view !== null) this.fail("storage after account delete", `${account.id} still holds storage`);
      } else if (view === null) {
        this.fail("storage", `${account.id} lost its storage without an account delete`);
      }
      const intervals = view?.intervals ?? [];
      if (intervals.some((i) => i.outcome === null)) this.fail("an ended interval is not acknowledged", account.id);
      this.readRecordAgainstTheProvider(account, intervals);
      const refusedByAFault = (i: Row) => i.outcome === "not-billable" && i.refusal === "unknown_account" && account.refused.has(String(i.identifier));
      if (intervals.some((i) => i.outcome === "not-billable" && !refusedByAFault(i))) this.fail("an interval the provider refused", account.id);
      this.count("interval.not-billable-after-refusals", intervals.filter(refusedByAFault).length);
      if (view !== null) this.checkHolds(account, view.holds);
      const events = this.meter.recordedEvents(account.id);
      if (new Set(events.map((e) => e.identifier)).size !== events.length) this.fail("one identifier recorded twice", account.id);
      for (const interval of intervals) {
        if (interval.sealed_at === null) continue;
        const held = events.filter((e) => e.timestamp === interval.timestamp);
        const expected = interval.outcome === "acknowledged" ? [1] : [0];
        if (!expected.includes(held.length) || held.some((e) => e.amountMinor !== interval.amount)) {
          this.fail("one event for each acknowledged interval", `${account.id} interval ${interval.start}: ${held.length} events, outcome ${interval.outcome}`);
        }
      }
      let carryIn = 0;
      let insidePeriods = 0;
      const writtenOff = view?.meteringAccount?.written_off ?? null;
      const closedPeriods = this.meter.periods(account.id, this.now).filter((period) => period.end <= this.now);
      const lastPeriod = closedPeriods.at(-1)?.index;
      for (const period of closedPeriods) {
        const reported = account.reported.get(period.index) ?? 0;
        const stampedAfterTheEnd =
          period.index === lastPeriod && account.cutAt !== null && account.cutAt > period.end ? this.meter.exactTotal(account.id, period.end, account.cutAt) : 0;
        if (stampedAfterTheEnd > 0) this.count("event.stamped-after-the-subscription-end");
        const providerMinor = this.meter.exactTotal(account.id, period.start, period.end) + stampedAfterTheEnd;
        const provider = providerMinor * RULE_MICRO_PER_MINOR;
        let unsent = 0;
        const countedUntil = period.index === lastPeriod ? Math.max(period.end, account.cutAt ?? period.end) : period.end;
        for (const [identifier, sent] of account.refused) {
          const atTheMeter = events.some((e) => e.identifier === identifier);
          const closed = account.closedNotBillable.has(identifier);
          if (!atTheMeter && closed && sent.timestamp >= period.start && sent.timestamp < countedUntil) unsent += sent.amountMinor * RULE_MICRO_PER_MINOR;
        }
        const carryOut = reported + carryIn - provider - unsent;
        this.rows.push({ account: account.id, period: period.index, reported, carryIn, provider, carryOut, invoicedMinor: this.meter.invoicedTotal(account.id, period) });
        if (carryIn > 0) this.count("period.starts-with-a-remainder");
        if (period.index > 0 && reported > 0) this.count("period.later-with-usage");
        if (carryOut < 0 || carryOut >= RULE_MICRO_PER_MINOR) {
          this.fail(
            "the equation with a remainder under one minor unit",
            `${account.id} period ${period.index}: reported ${reported} + carried in ${carryIn} - provider ${provider} - closed as not billable ${unsent} = ${carryOut}`,
          );
        }
        if (view !== null) {
          let ledgerReported = 0;
          let storedCarry = 0;
          for (const interval of intervals) {
            const start = int(interval, "start") / 1000;
            const end = int(interval, "end_at") / 1000;
            if (start >= period.start && start < period.end) ledgerReported += int(interval, "reported") || 0;
            if (interval.sealed_at !== null && end <= period.end) storedCarry = int(interval, "carry_out");
          }
          if (ledgerReported !== reported) this.fail("the ledger's reported total", `${account.id} period ${period.index}: ledger ${ledgerReported}, driver ${reported}`);
          const stored = account.ended === "cancel" && period.index === lastPeriod ? writtenOff : storedCarry;
          if (stored !== carryOut) this.fail("the stored remainder", `${account.id} period ${period.index}: stored ${stored}, measured ${carryOut}`);
        }
        carryIn = carryOut;
        insidePeriods += providerMinor;
      }
      const everything = this.meter.exactTotal(account.id, Number.NEGATIVE_INFINITY, Number.POSITIVE_INFINITY);
      if (everything !== insidePeriods) this.fail("an event outside every billing period", `${account.id}: ${everything - insidePeriods} minor units`);
      if (account.ended !== null) {
        if (account.eventsHeldWhenEndCompleted === null) this.fail("the end did not complete", `${account.id} after ${account.ended}`);
        else if (events.length !== account.eventsHeldWhenEndCompleted) this.fail("an event recorded after the last acknowledgement", account.id);
        else {
          this.count(`${account.ended}.completed`);
          if (carryIn > 0) this.count(`${account.ended}.left-a-remainder`);
        }
      }
    }
    if (this.meter.stats().duplicatesWithOtherContent > 0) this.fail("one identifier with two contents", `${this.meter.stats().duplicatesWithOtherContent} times`);
  }
}
