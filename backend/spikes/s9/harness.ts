
import { FakeMeter, type FakeMeterConfig, type MeterEvent, type SendResult } from "../../test-kit/fake-meter/fake-meter.ts";
import {
  AccountObject,
  type CallOutcome,
  type ProviderCall,
  Store,
  type Stored,
  type SyncConfig,
  type Variant,
} from "./sync.ts";

export const HOUR = 3600;
export const DAY = 24 * HOUR;

export const RULE_MICRO_PER_MINOR = 10_000;

const SECONDS_OFF_THE_MINUTE = 17;
const CLOCK_START = 1_800_000_000 + SECONDS_OFF_THE_MINUTE;
export const BILLING_PERIOD = 30 * DAY;
export const HOLD_LIFE = 900;
export const RETRY_AFTER = 60;
export const WINDOW_MARGIN = 300;
export const LATE_DELIVERY = 90;
export const LATE_RESPONSE = 20;
if (LATE_DELIVERY > WINDOW_MARGIN || LATE_RESPONSE > WINDOW_MARGIN) {
  throw new RangeError("a late delivery and a late response arrive inside the window margin");
}
const PAST_EVERY_OPEN_BILLING_PERIOD = BILLING_PERIOD + 4 * DAY;
const SETTLING_TIME = 2 * DAY;

export type FaultKind =
  | "none"
  | "request-lost"
  | "response-lost"
  | "late-delivery"
  | "late-response"
  | "crash-before-call"
  | "crash-after-call";

export type Fault =
  | { readonly kind: Exclude<FaultKind, "crash-before-call" | "crash-after-call"> }
  | { readonly kind: "crash-before-call" | "crash-after-call"; readonly downSeconds: number };

const NO_FAULT: Fault = { kind: "none" };

export type Command =
  | { readonly kind: "advance"; readonly seconds: number }
  | { readonly kind: "advance-to-period-end"; readonly account: number; readonly secondsBeforePeriodEnd: number }
  | { readonly kind: "reserve"; readonly account: number; readonly amountMicro: number }
  | { readonly kind: "settle"; readonly account: number; readonly holdsBackFromNewest: number; readonly percentOfHold: number }
  | { readonly kind: "crash"; readonly account: number; readonly downSeconds: number }
  | { readonly kind: "cancel"; readonly account: number }
  | { readonly kind: "delete"; readonly account: number };

export type Scenario = {
  readonly subscribeSecondsAfterClockStart: readonly number[];
  readonly meter: Omit<FakeMeterConfig, "invoicePeriodSeconds">;
  readonly intervalSeconds: number | null;
  readonly capPeriodSeconds: number;
  readonly capAnchorSecondsBeforeSubscription: number;
  readonly commands: readonly Command[];
  readonly faultsByProviderCall: readonly Fault[];
};

export type Options = {
  readonly variant: Variant;
  readonly downtimeBudgetSeconds: number;
  readonly onFailedCheck: "stop" | "list";
};

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
};

class Stop extends Error {}

type ModelHold = { id: string; amount: number; period: number; capPeriod: number; expiresAt: number; open: boolean };

type Account = {
  id: string;
  store: Store;
  object: AccountObject;
  config: SyncConfig;
  subscribedAt: number;
  downUntil: number;
  pendingAnswer: { arrivesAt: number; providerCall: ProviderCall; outcome: CallOutcome } | null;
  holds: ModelHold[];
  reported: Map<number, number>;
  reportedTotal: number;
  ended: "cancel" | "delete" | null;
  eventsHeldWhenEndCompleted: number | null;
};

type CallResult =
  | { readonly kind: "answered"; readonly outcome: CallOutcome }
  | { readonly kind: "crashed" }
  | { readonly kind: "answer-pending" };

const CRASHED: CallResult = { kind: "crashed" };
const ANSWER_PENDING: CallResult = { kind: "answer-pending" };

function answered(outcome: CallOutcome): CallResult {
  return { kind: "answered", outcome };
}

function deleteOutranksCancel(previous: Account["ended"], next: "cancel" | "delete"): "cancel" | "delete" {
  return previous === "delete" ? "delete" : next;
}

export function runScenario(scenario: Scenario, options: Options): Report {
  const world = new World(scenario, options);
  try {
    world.run();
  } catch (error) {
    if (!(error instanceof Stop)) throw error;
  }
  return world.report();
}

class World {
  private now: number;
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
  private closing = false;
  private downtimeLeft: number;
  private readonly scenario: Scenario;
  private readonly options: Options;

  constructor(scenario: Scenario, options: Options) {
    this.scenario = scenario;
    this.options = options;
    this.downtimeLeft = options.downtimeBudgetSeconds;
    if (!(scenario.meter.graceSeconds > HOLD_LIFE)) {
      throw new RangeError("the grace period of the provider is longer than the life of a hold");
    }
    const anchorBefore = scenario.capAnchorSecondsBeforeSubscription;
    if (!(anchorBefore >= 1 && anchorBefore < scenario.capPeriodSeconds)) {
      throw new RangeError("the cap anchor lies 1 second or more, and less than one cap period, before the subscription");
    }
    this.meter = new FakeMeter({ ...scenario.meter, invoicePeriodSeconds: BILLING_PERIOD });
    this.now = CLOCK_START + Math.max(...scenario.subscribeSecondsAfterClockStart);
    for (const [index, after] of scenario.subscribeSecondsAfterClockStart.entries()) {
      const id = `acct-${index}`;
      const subscribedAt = CLOCK_START + after;
      const config: SyncConfig = {
        variant: options.variant,
        intervalSeconds: scenario.intervalSeconds,
        billingPeriodSeconds: BILLING_PERIOD,
        capPeriodSeconds: scenario.capPeriodSeconds,
        capAnchor: subscribedAt - scenario.capAnchorSecondsBeforeSubscription,
        holdLifeSeconds: HOLD_LIFE,
        documentedMinUniquenessWindowSeconds: DAY,
        requestTravelMarginSeconds: WINDOW_MARGIN,
        retryAfterSeconds: RETRY_AFTER,
      };
      const store = new Store();
      this.meter.subscribe(id, subscribedAt);
      this.accounts.push({
        id,
        store,
        object: AccountObject.open(store, config, id, subscribedAt),
        config,
        subscribedAt,
        downUntil: 0,
        pendingAnswer: null,
        holds: [],
        reported: new Map(),
        reportedTotal: 0,
        ended: null,
        eventsHeldWhenEndCompleted: null,
      });
    }
  }

  run(): void {
    for (const command of this.scenario.commands) {
      this.apply(command);
      this.advanceTo(this.now);
      this.checkStep();
    }
    this.closingPhase();
    this.checkStep();
    this.checkEnd();
  }

  private closingPhase(): void {
    this.closing = true;
    this.passEveryOpenPeriodUnderTheFaultsThatAreLeft();
    this.switchFaultsOffAndBringEachObjectUp();
    this.giveTheSyncSettlingTime();
  }

  private passEveryOpenPeriodUnderTheFaultsThatAreLeft(): void {
    this.advanceTo(this.now + PAST_EVERY_OPEN_BILLING_PERIOD);
  }

  private switchFaultsOffAndBringEachObjectUp(): void {
    this.faultsOn = false;
    this.advanceTo(Math.max(this.now, ...this.accounts.map((account) => account.downUntil)));
  }

  private giveTheSyncSettlingTime(): void {
    this.advanceTo(this.now + SETTLING_TIME);
  }

  report(): Report {
    return {
      callsWhileFaultsOn: this.callsWhileFaultsOn,
      findings: this.findings,
      coverage: this.coverage,
      periods: this.rows,
    };
  }

  private count(key: string, by = 1): void {
    this.coverage[key] = (this.coverage[key] ?? 0) + by;
  }

  private fail(check: string, detail: string): void {
    this.findings.push({ check, detail });
    if (this.options.onFailedCheck === "stop") throw new Stop();
  }

  private apply(command: Command): void {
    if (command.kind === "advance") {
      this.advanceTo(this.now + command.seconds);
      return;
    }
    const account = this.accounts[command.account % this.accounts.length];
    if (account === undefined) return;
    if (command.kind === "advance-to-period-end") {
      const periods = Math.floor((this.now - account.subscribedAt) / BILLING_PERIOD) + 1;
      this.advanceTo(Math.max(this.now, account.subscribedAt + periods * BILLING_PERIOD - command.secondsBeforePeriodEnd));
      return;
    }
    if (this.now < account.downUntil) {
      this.count("command.dropped-while-down");
      return;
    }
    if (account.pendingAnswer !== null) this.count("step.during-provider-call");
    switch (command.kind) {
      case "reserve": {
        if (account.ended !== null) {
          if (account.object.reserve(`${account.id}-late`, command.amountMicro, this.now)) {
            this.fail("reserve after the end", `${account.id} admitted a hold after ${account.ended}`);
          }
          return;
        }
        const id = `${account.id}-h${account.holds.length}`;
        if (!account.object.reserve(id, command.amountMicro, this.now)) {
          this.fail("reserve", `${account.id} refused a hold while active`);
          return;
        }
        const period = Math.floor((this.now - account.subscribedAt) / BILLING_PERIOD);
        const capPeriod = Math.floor((this.now - account.config.capAnchor) / account.config.capPeriodSeconds);
        const untilPeriodEnd = account.subscribedAt + (period + 1) * BILLING_PERIOD - this.now;
        if (untilPeriodEnd <= HOLD_LIFE) this.count("reserve.in-the-last-900-s-of-a-billing-period");
        if (untilPeriodEnd === 1) this.count("reserve.in-the-last-second-of-a-billing-period");
        const before = account.holds.at(-1);
        if (before !== undefined) {
          const newBilling = before.period !== period;
          const newCap = before.capPeriod !== capPeriod;
          if (newBilling) this.count("hold.first-of-a-billing-period");
          if (newCap) this.count("hold.first-of-a-cap-period");
          if (newCap && !newBilling) this.count("hold.cap-period-rolled-inside-a-billing-period");
          if (newBilling && !newCap) this.count("hold.billing-period-rolled-inside-a-cap-period");
        }
        account.holds.push({
          id,
          amount: command.amountMicro,
          period,
          capPeriod,
          expiresAt: this.now + HOLD_LIFE,
          open: true,
        });
        this.count("reserve");
        return;
      }
      case "settle": {
        const count = account.holds.length;
        const hold = count === 0 ? undefined : account.holds[count - 1 - (command.holdsBackFromNewest % count)];
        if (hold === undefined) return;
        const cost = Math.floor((hold.amount * command.percentOfHold) / 100);
        account.object.settle(hold.id, cost, this.now);
        if (!hold.open) {
          this.count("settle.repeat-or-after-expiry");
        } else if (this.now >= hold.expiresAt) {
          hold.open = false;
          this.count("settle.after-expiry");
        } else {
          hold.open = false;
          account.reported.set(hold.period, (account.reported.get(hold.period) ?? 0) + cost);
          account.reportedTotal += cost;
          this.count("settle.counted");
          if (cost > hold.amount) this.count("settle.above-the-hold");
          if (Math.floor((this.now - account.subscribedAt) / BILLING_PERIOD) > hold.period) {
            this.count("settle.counted-after-its-billing-period-ended");
          }
          if (account.ended !== null) this.count("settle.after-cancel-or-delete");
        }
        return;
      }
      case "crash":
        this.crash(account, command.downSeconds);
        this.count("crash.idle");
        return;
      case "cancel":
      case "delete": {
        const endedAt = this.meter.endSubscription(account.id, this.now);
        account.object.end(command.kind, endedAt, this.now);
        const previous = account.ended;
        account.ended = deleteOutranksCancel(previous, command.kind);
        if (previous === "delete") {
          this.count(`${command.kind}.after-delete`);
          return;
        }
        const openHolds = account.holds.some((h) => h.open && this.now < h.expiresAt);
        this.count(previous === null ? command.kind : `${command.kind}.after-cancel`);
        if (previous === null && openHolds) this.count(`${command.kind}.with-open-hold`);
        return;
      }
    }
  }

  private advanceTo(target: number): void {
    this.clockMoveStopsForAPendingAnswer = false;
    for (let guard = 0; ; guard += 1) {
      if (guard > 200_000) {
        this.fail("settling", "more than 200,000 events fired in one clock move");
        throw new Stop();
      }
      let at = Number.POSITIVE_INFINITY;
      let fire: (() => void) | null = null;
      for (const [index, delivery] of this.lateDeliveries.entries()) {
        if (delivery.at < at) {
          at = delivery.at;
          fire = () => {
            this.lateDeliveries.splice(index, 1);
            this.deliver(delivery.event);
          };
        }
      }
      for (const account of this.accounts) {
        const pendingAnswer = account.pendingAnswer;
        if (pendingAnswer !== null) {
          if (pendingAnswer.arrivesAt < at) {
            at = pendingAnswer.arrivesAt;
            fire = () => {
              account.pendingAnswer = null;
              this.finish(account, pendingAnswer.providerCall, pendingAnswer.outcome);
            };
          }
          continue;
        }
        const alarm = account.object.alarmAt();
        if (alarm === null) continue;
        const due = Math.max(alarm, account.downUntil);
        if (due < at) {
          at = due;
          fire = () => this.wake(account);
        }
      }
      if (fire === null || at > target) break;
      this.now = Math.max(this.now, at);
      fire();
      this.checkStep();
      if (this.clockMoveStopsForAPendingAnswer && !this.closing) return;
    }
    this.now = Math.max(this.now, target);
  }

  private wake(account: Account): void {
    this.count("wake");
    const providerCall = account.object.wake(this.now);
    if (providerCall.kind === "idle") return;
    const result = this.call(account, providerCall);
    if (result.kind === "answered") this.finish(account, providerCall, result.outcome);
  }

  private finish(account: Account, providerCall: ProviderCall, outcome: CallOutcome): void {
    const next = account.object.record(providerCall, outcome, this.now);
    const asksForNothingAfterASend = providerCall.kind !== "send" || next.kind === "idle";
    const asksOnlyForASendAfterARead = providerCall.kind !== "read" || next.kind !== "read";
    if (!asksForNothingAfterASend || !asksOnlyForASendAfterARead) {
      throw new Error(`one sync wake makes at most one read and one send: ${account.id} asked for a ${next.kind} after a ${providerCall.kind}`);
    }
    if (providerCall.kind === "read" && outcome.kind === "read") {
      this.count(next.kind === "send" ? "read.lacks-the-amount" : "read.holds-the-amount");
    }
    if (next.kind === "idle") return;
    const second = this.call(account, next);
    if (second.kind === "answered") this.finish(account, next, second.outcome);
  }

  private call(account: Account, providerCall: ProviderCall): CallResult {
    if (providerCall.kind === "idle") return answered({ kind: "no-answer" });
    const fault: Fault = (this.faultsOn ? this.scenario.faultsByProviderCall[this.nextFault] : undefined) ?? NO_FAULT;
    this.nextFault += 1;
    if (this.faultsOn) this.callsWhileFaultsOn += 1;
    this.count(`fault.${fault.kind}`);
    if (!this.closing) this.count("call.before-the-closing-phase");
    if (providerCall.kind === "read") {
      this.count("call.read");
      if (fault.kind === "request-lost" || fault.kind === "response-lost") return answered({ kind: "no-answer" });
      if (fault.kind === "crash-before-call" || fault.kind === "crash-after-call") {
        this.crash(account, fault.downSeconds);
        return CRASHED;
      }
      const result = this.meter.read(providerCall.account, providerCall.from, providerCall.to, this.now);
      const neighbours = this.meter
        .recordedEvents(providerCall.account)
        .filter((e) => e.timestamp >= result.widenedFrom && e.timestamp < result.widenedTo && e.timestamp !== providerCall.from);
      if (neighbours.length > 0) this.count("read.range-holds-another-interval");
      return answered({ kind: "read", result });
    }
    if (providerCall.attempt === 1) this.count("send.first-attempt");
    else if (providerCall.afterRead) this.count("send.resend-after-the-window");
    else this.count("send.retry-inside-the-window");
    switch (fault.kind) {
      case "none":
        return answered({ kind: "sent", result: this.deliver(providerCall.event) });
      case "request-lost":
        return answered({ kind: "no-answer" });
      case "response-lost":
        this.deliver(providerCall.event);
        return answered({ kind: "no-answer" });
      case "late-delivery":
        this.lateDeliveries.push({ at: this.now + LATE_DELIVERY, event: providerCall.event });
        return answered({ kind: "no-answer" });
      case "late-response":
        this.clockMoveStopsForAPendingAnswer = true;
        account.pendingAnswer = {
          arrivesAt: this.now + LATE_RESPONSE,
          providerCall,
          outcome: { kind: "sent", result: this.deliver(providerCall.event) },
        };
        return ANSWER_PENDING;
      case "crash-before-call":
        this.crash(account, fault.downSeconds);
        return CRASHED;
      case "crash-after-call":
        this.deliver(providerCall.event);
        this.crash(account, fault.downSeconds);
        return CRASHED;
    }
  }

  private deliver(event: MeterEvent): SendResult {
    const result = this.meter.send(event, this.now);
    this.count(`meter.${result.status}`);
    if (result.status === "refused") {
      this.fail("refused by the provider", `${event.identifier}: ${result.reason}`);
    }
    return result;
  }

  private crash(account: Account, downSeconds: number): void {
    const down = Math.min(downSeconds, this.downtimeLeft);
    this.downtimeLeft -= down;
    account.object = new AccountObject(account.store, account.config);
    account.pendingAnswer = null;
    account.downUntil = this.now + down;
    if (down >= DAY) this.count("crash.down-past-the-window");
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
      if (account.object.unexplainedReads() > 0) {
        this.fail("a read the sync cannot explain", account.id);
      }
      if (account.ended !== null && account.eventsHeldWhenEndCompleted === null && account.object.ended()) {
        account.eventsHeldWhenEndCompleted = this.meter.recordedEvents(account.id).length;
      }
    }
  }

  private checkStricterThanTheRule(account: Account, view: Stored | null): void {
    const events = this.meter.recordedEvents(account.id);
    if (new Set(events.map((e) => e.identifier)).size !== events.length) {
      this.fail("one identifier recorded twice", account.id);
    }
    for (const interval of view?.admittingIntervalsOldestFirst ?? []) {
      if (interval.sealed === null) continue;
      const held = events.filter((e) => e.timestamp === interval.sealed?.timestamp);
      const expected = interval.outcome === "acknowledged" ? 1 : 0;
      if (held.length !== expected || held.some((e) => e.amountMinor !== interval.sealed?.amountMinor)) {
        this.fail(
          "one event for each acknowledged interval",
          `${account.id} interval ${interval.start}: ${held.length} events, outcome ${interval.outcome}`,
        );
      }
    }
  }

  private checkEnd(): void {
    for (const account of this.accounts) {
      const view = account.object.view();
      if (account.ended === "delete") {
        if (view !== null) this.fail("storage after account delete", `${account.id} still holds storage`);
      } else if (view === null) {
        this.fail("storage", `${account.id} lost its storage without an account delete`);
      }
      if (view?.admittingIntervalsOldestFirst.some((i) => i.outcome === null)) {
        this.fail("an ended interval is not acknowledged", account.id);
      }
      if (view?.admittingIntervalsOldestFirst.some((i) => i.outcome === "refused")) {
        this.fail("an interval the provider refused", account.id);
      }
      this.checkStricterThanTheRule(account, view);
      this.count("hold.expired-unsettled", account.holds.filter((h) => h.open).length);
      let carryIn = 0;
      let insidePeriods = 0;
      for (const period of this.meter.periods(account.id, this.now)) {
        if (period.end > this.now) continue;
        const reported = account.reported.get(period.index) ?? 0;
        const providerMinor = this.meter.exactTotal(account.id, period.start, period.end);
        const provider = providerMinor * RULE_MICRO_PER_MINOR;
        const carryOut = reported + carryIn - provider;
        this.rows.push({
          account: account.id,
          period: period.index,
          reported,
          carryIn,
          provider,
          carryOut,
          invoicedMinor: this.meter.invoicedTotal(account.id, period),
        });
        if (carryIn > 0) this.count("period.starts-with-a-remainder");
        if (period.index > 0 && reported > 0) this.count("period.later-with-usage");
        if (carryOut < 0 || carryOut >= RULE_MICRO_PER_MINOR) {
          this.fail(
            "the equation with a remainder under one minor unit",
            `${account.id} period ${period.index}: reported ${reported} + carried in ${carryIn} - provider ${provider} = ${carryOut}`,
          );
        }
        if (view !== null) {
          let ledgerReported = 0;
          let storedCarry = 0;
          for (const interval of view.admittingIntervalsOldestFirst) {
            if (interval.start >= period.start && interval.start < period.end) ledgerReported += interval.reportedMicro;
            if (interval.sealed !== null && interval.end <= period.end) storedCarry = interval.sealed.carryOutMicro;
          }
          if (ledgerReported !== reported) {
            this.fail(
              "the ledger's reported total",
              `${account.id} period ${period.index}: ledger ${ledgerReported}, driver ${reported}`,
            );
          }
          if (storedCarry !== carryOut) {
            this.fail(
              "the stored remainder",
              `${account.id} period ${period.index}: stored ${storedCarry}, measured ${carryOut}`,
            );
          }
        }
        carryIn = carryOut;
        insidePeriods += providerMinor;
      }
      const everything = this.meter.exactTotal(account.id, Number.NEGATIVE_INFINITY, Number.POSITIVE_INFINITY);
      if (everything !== insidePeriods) {
        this.fail(
          "an event outside every billing period",
          `${account.id}: ${everything - insidePeriods} minor units`,
        );
      }
      if (account.ended === "cancel" && view !== null && view.writtenOff !== carryIn) {
        this.fail(
          "the remainder written off at cancel",
          `${account.id}: recorded ${view.writtenOff}, measured ${carryIn}`,
        );
      }
      if (account.ended !== null) {
        if (account.eventsHeldWhenEndCompleted === null) {
          this.fail("the end did not complete", `${account.id} after ${account.ended}`);
        } else if (this.meter.recordedEvents(account.id).length !== account.eventsHeldWhenEndCompleted) {
          this.fail("an event recorded after the last acknowledgement", account.id);
        } else {
          this.count(`${account.ended}.completed`);
          if (carryIn > 0) this.count(`${account.ended}.left-a-remainder`);
        }
      }
    }
    const stats = this.meter.stats();
    if (stats.duplicatesWithOtherContent > 0) {
      this.fail("one identifier with two contents", `${stats.duplicatesWithOtherContent} times`);
    }
  }
}
