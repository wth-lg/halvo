
import type { MeterEvent, ReadResult, RefusalReason, SendResult } from "../../test-kit/fake-meter/fake-meter.ts";

const MICRO_PER_MINOR = 10_000;

type ControlVariant =
  | "new-identifier-on-retry"
  | "resend-without-read"
  | "drop-remainder"
  | "round-up-last"
  | "cut-at-cap-period"
  | "timestamp-at-interval-start"
  | "seal-with-open-holds"
  | "window-restarts-at-resend"
  | "read-ignores-neighbours"
  | "seal-out-of-time-order"
  | "identifier-without-account"
  | "settle-counts-in-the-newest-interval";

export type Variant = "as-designed" | ControlVariant;

export type SyncConfig = {
  readonly variant: Variant;
  readonly intervalSeconds: number | null;
  readonly billingPeriodSeconds: number;
  readonly capPeriodSeconds: number;
  readonly capAnchor: number;
  readonly holdLifeSeconds: number;
  readonly documentedMinUniquenessWindowSeconds: number;
  readonly requestTravelMarginSeconds: number;
  readonly retryAfterSeconds: number;
};

type Hold = {
  amount: number;
  expiresAt: number;
  intervalStart: number;
  capPeriod: number;
  state: "open" | "settled" | "expired";
  cost: number | null;
};

type Sealed = {
  readonly carryInMicro: number;
  readonly amountMinor: number;
  readonly carryOutMicro: number;
  readonly identifier: string;
  readonly timestamp: number;
};

type Interval = {
  start: number;
  end: number;
  openHolds: number;
  reportedMicro: number;
  sealed: Sealed | null;
  send: { firstAttemptPlannedAt: number; attempts: number } | null;
  outcome: "acknowledged" | "nothing-to-send" | "refused" | null;
};

export type Stored = {
  account: string;
  subscribedAt: number;
  ending: "cancel" | "delete" | null;
  endedAt: number | null;
  lastNow: number;
  holds: Record<string, Hold>;
  admittingIntervalsOldestFirst: Interval[];
  carryMicro: number;
  writtenOff: number | null;
  capTotals: Record<string, { reported: number; unreported: number }>;
  retryAt: number | null;
  unexplainedReads: number;
};

export class Store {
  data: Stored | null = null;
}

export type ProviderCall =
  | { readonly kind: "idle" }
  | {
      readonly kind: "send";
      readonly intervalStart: number;
      readonly event: MeterEvent;
      readonly attempt: number;
      readonly afterRead: boolean;
    }
  | {
      readonly kind: "read";
      readonly intervalStart: number;
      readonly account: string;
      readonly from: number;
      readonly to: number;
    };

export type CallOutcome =
  | { readonly kind: "sent"; readonly result: SendResult }
  | { readonly kind: "read"; readonly result: ReadResult }
  | { readonly kind: "no-answer" };

const IDLE: ProviderCall = { kind: "idle" };

function copy<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

function noLaterAttemptCanSucceed(reason: RefusalReason): boolean {
  return reason === "timestamp_too_old";
}

function identifierAcrossAccounts(account: string, intervalStart: number): string {
  return `${account}:${intervalStart}`;
}

function neverBackwards(now: number, lastNow: number): number {
  return Math.max(now, lastNow);
}

export class AccountObject {
  private readonly store: Store;
  private readonly config: SyncConfig;

  constructor(store: Store, config: SyncConfig) {
    this.store = store;
    this.config = config;
  }

  static open(store: Store, config: SyncConfig, account: string, subscribedAt: number): AccountObject {
    store.data = {
      account,
      subscribedAt,
      ending: null,
      endedAt: null,
      lastNow: subscribedAt,
      holds: {},
      admittingIntervalsOldestFirst: [],
      carryMicro: 0,
      writtenOff: null,
      capTotals: {},
      retryAt: null,
      unexplainedReads: 0,
    };
    return new AccountObject(store, config);
  }

  reserve(holdId: string, amount: number, now: number): boolean {
    return this.atomicStep(now, false, (s, at) => {
      if (s.ending !== null) return false;
      if (s.holds[holdId] !== undefined) return true;
      const [start, end] = this.intervalBounds(s, at);
      let interval = s.admittingIntervalsOldestFirst.at(-1);
      if (interval === undefined || interval.start !== start) {
        interval = { start, end, openHolds: 0, reportedMicro: 0, sealed: null, send: null, outcome: null };
        s.admittingIntervalsOldestFirst.push(interval);
      }
      interval.openHolds += 1;
      const capPeriod = Math.floor((at - this.config.capAnchor) / this.config.capPeriodSeconds);
      s.holds[holdId] = {
        amount,
        expiresAt: at + this.config.holdLifeSeconds,
        intervalStart: start,
        capPeriod,
        state: "open",
        cost: null,
      };
      return true;
    });
  }

  settle(holdId: string, cost: number, now: number): void {
    this.atomicStep(now, undefined, (s) => {
      const hold = s.holds[holdId];
      if (hold === undefined) return;
      if (hold.state === "expired") {
        hold.cost ??= cost;
        return;
      }
      if (hold.state === "settled") return;
      hold.state = "settled";
      hold.cost = cost;
      const own = s.admittingIntervalsOldestFirst.find((i) => i.start === hold.intervalStart);
      const newest = s.admittingIntervalsOldestFirst.at(-1);
      const wrong =
        this.config.variant === "settle-counts-in-the-newest-interval" && newest !== undefined && newest.sealed === null;
      const credited = wrong ? newest : own;
      if (own !== undefined) own.openHolds -= 1;
      if (credited !== undefined) credited.reportedMicro += cost;
      capTotal(s, hold.capPeriod).reported += cost;
    });
  }

  end(kind: "cancel" | "delete", endedAtProvider: number, now: number): void {
    this.atomicStep(now, undefined, (s) => {
      if (s.ending !== null) {
        if (kind === "delete") s.ending = "delete";
        return;
      }
      s.ending = kind;
      s.endedAt = endedAtProvider;
      const last = s.admittingIntervalsOldestFirst.at(-1);
      if (last !== undefined && last.sealed === null && last.end > endedAtProvider) last.end = endedAtProvider;
    });
  }

  alarmAt(): number | null {
    const s = this.store.data;
    if (s === null) return null;
    let at = Number.POSITIVE_INFINITY;
    for (const hold of Object.values(s.holds)) {
      if (hold.state === "open") at = Math.min(at, hold.expiresAt);
    }
    const unsealed = s.admittingIntervalsOldestFirst.find((i) => i.sealed === null);
    if (unsealed !== undefined && this.holdsEnded(unsealed)) at = Math.min(at, unsealed.end);
    const pending = s.admittingIntervalsOldestFirst.find((i) => i.sealed !== null && i.outcome === null);
    if (pending !== undefined) at = Math.min(at, s.retryAt ?? s.lastNow);
    return at === Number.POSITIVE_INFINITY ? null : at;
  }

  wake(now: number): ProviderCall {
    return this.atomicStep(now, IDLE, (s, at) => {
      this.sealDueOldestFirst(s, at);
      const interval = s.admittingIntervalsOldestFirst.find((i) => i.sealed !== null && i.outcome === null);
      if (interval === undefined || interval.sealed === null) return IDLE;
      if (s.retryAt !== null && at < s.retryAt) return IDLE;
      s.retryAt = at + this.config.retryAfterSeconds;
      if (interval.send === null) {
        interval.send = { firstAttemptPlannedAt: at, attempts: 1 };
        return this.sendPlan(s, interval, interval.sealed, false);
      }
      const identifierSurelyHeldUntil =
        interval.send.firstAttemptPlannedAt + this.config.documentedMinUniquenessWindowSeconds - this.config.requestTravelMarginSeconds;
      if (at < identifierSurelyHeldUntil || this.config.variant === "resend-without-read") {
        interval.send.attempts += 1;
        return this.sendPlan(s, interval, interval.sealed, false);
      }
      return this.readBeforeResend(s, interval, interval.sealed);
    });
  }

  record(providerCall: ProviderCall, outcome: CallOutcome, now: number): ProviderCall {
    return this.atomicStep(now, IDLE, (s) => {
      if (providerCall.kind === "idle" || outcome.kind === "no-answer") return IDLE;
      const interval = s.admittingIntervalsOldestFirst.find((i) => i.start === providerCall.intervalStart);
      if (interval === undefined || interval.sealed === null || interval.outcome !== null) return IDLE;
      if (providerCall.kind === "send" && outcome.kind === "sent") {
        if (outcome.result.status !== "refused") {
          interval.outcome = "acknowledged";
          s.retryAt = null;
        } else if (noLaterAttemptCanSucceed(outcome.result.reason)) {
          interval.outcome = "refused";
          s.retryAt = null;
        }
        return IDLE;
      }
      if (providerCall.kind === "read" && outcome.kind === "read" && interval.send !== null) {
        const { widenedFrom, widenedTo, totalMinor } = outcome.result;
        const others = this.otherAcknowledgedMinorIn(s, interval, widenedFrom, widenedTo);
        if (totalMinor === others + interval.sealed.amountMinor) {
          interval.outcome = "acknowledged";
          s.retryAt = null;
          return IDLE;
        }
        if (totalMinor === others) {
          interval.send.attempts += 1;
          if (this.config.variant === "window-restarts-at-resend") interval.send.firstAttemptPlannedAt = s.lastNow;
          return this.sendPlan(s, interval, interval.sealed, true);
        }
        s.unexplainedReads += 1;
      }
      return IDLE;
    });
  }

  view(): Stored | null {
    return this.store.data === null ? null : copy(this.store.data);
  }

  unexplainedReads(): number {
    return this.store.data?.unexplainedReads ?? 0;
  }

  ended(): boolean {
    const s = this.store.data;
    const storageDroppedAfterDelete = s === null;
    const carryWrittenOffAfterCancel = s !== null && s.writtenOff !== null;
    return storageDroppedAfterDelete || carryWrittenOffAfterCancel;
  }

  private atomicStep<T>(now: number, whenEmpty: T, body: (s: Stored, at: number) => T): T {
    if (this.store.data === null) return whenEmpty;
    const s = copy(this.store.data);
    const at = neverBackwards(now, s.lastNow);
    s.lastNow = at;
    expireDueHolds(s, at);
    const out = body(s, at);
    this.store.data = finishEnding(s);
    return out;
  }

  private holdsEnded(interval: Interval): boolean {
    return interval.openHolds === 0 || this.config.variant === "seal-with-open-holds";
  }

  private sealDueOldestFirst(s: Stored, at: number): void {
    for (const [index, interval] of s.admittingIntervalsOldestFirst.entries()) {
      if (interval.sealed !== null) continue;
      if (interval.end > at || !this.holdsEnded(interval)) {
        if (this.config.variant === "seal-out-of-time-order") continue;
        break;
      }
      const total = interval.reportedMicro + s.carryMicro;
      let amountMinor = Math.floor(total / MICRO_PER_MINOR);
      let carryOutMicro = total - amountMinor * MICRO_PER_MINOR;
      if (this.config.variant === "drop-remainder") carryOutMicro = 0;
      const isLast = s.ending !== null && index === s.admittingIntervalsOldestFirst.length - 1;
      if (this.config.variant === "round-up-last" && isLast && carryOutMicro > 0) {
        amountMinor += 1;
        carryOutMicro = 0;
      }
      interval.sealed = {
        carryInMicro: s.carryMicro,
        amountMinor,
        carryOutMicro,
        identifier:
          this.config.variant === "identifier-without-account" ? `${interval.start}` : identifierAcrossAccounts(s.account, interval.start),
        timestamp: this.config.variant === "timestamp-at-interval-start" ? interval.start : interval.end - 1,
      };
      s.carryMicro = carryOutMicro;
      if (amountMinor === 0) interval.outcome = "nothing-to-send";
    }
  }

  private sendPlan(s: Stored, interval: Interval, sealed: Sealed, afterRead: boolean): ProviderCall {
    const attempt = interval.send?.attempts ?? 1;
    const identifier =
      this.config.variant === "new-identifier-on-retry" && attempt > 1
        ? `${sealed.identifier}:${attempt}`
        : sealed.identifier;
    return {
      kind: "send",
      intervalStart: interval.start,
      event: { account: s.account, identifier, amountMinor: sealed.amountMinor, timestamp: sealed.timestamp },
      attempt,
      afterRead,
    };
  }

  private otherAcknowledgedMinorIn(s: Stored, interval: Interval, widenedFrom: number, widenedTo: number): number {
    let minor = 0;
    for (const other of s.admittingIntervalsOldestFirst) {
      if (other === interval || other.sealed === null || other.outcome !== "acknowledged") continue;
      if (this.config.variant === "read-ignores-neighbours") continue;
      if (other.sealed.timestamp >= widenedFrom && other.sealed.timestamp < widenedTo) minor += other.sealed.amountMinor;
    }
    return minor;
  }

  private readBeforeResend(s: Stored, interval: Interval, sealed: Sealed): ProviderCall {
    return {
      kind: "read",
      intervalStart: interval.start,
      account: s.account,
      from: sealed.timestamp,
      to: sealed.timestamp + 1,
    };
  }

  private intervalBounds(s: Stored, at: number): [number, number] {
    if (this.config.variant === "cut-at-cap-period") {
      const length = this.config.capPeriodSeconds;
      const start = this.config.capAnchor + Math.floor((at - this.config.capAnchor) / length) * length;
      return [start, start + length];
    }
    const period = this.config.billingPeriodSeconds;
    const periodStart = s.subscribedAt + Math.floor((at - s.subscribedAt) / period) * period;
    const periodEnd = periodStart + period;
    const length = this.config.intervalSeconds;
    if (length === null) return [periodStart, periodEnd];
    const start = periodStart + Math.floor((at - periodStart) / length) * length;
    return [start, Math.min(start + length, periodEnd)];
  }
}

function expireDueHolds(s: Stored, at: number): void {
  for (const hold of Object.values(s.holds)) {
    if (hold.state !== "open" || hold.expiresAt > at) continue;
    hold.state = "expired";
    const interval = s.admittingIntervalsOldestFirst.find((i) => i.start === hold.intervalStart);
    if (interval !== undefined) interval.openHolds -= 1;
    capTotal(s, hold.capPeriod).unreported += hold.amount;
  }
}

function finishEnding(s: Stored): Stored | null {
  const everyIntervalHasAnOutcome = s.admittingIntervalsOldestFirst.every((i) => i.outcome !== null);
  if (s.ending === "delete" && everyIntervalHasAnOutcome) return null;
  if (s.ending === "cancel" && everyIntervalHasAnOutcome && s.writtenOff === null) s.writtenOff = s.carryMicro;
  return s;
}

function capTotal(s: Stored, capPeriod: number): { reported: number; unreported: number } {
  const key = String(capPeriod);
  let total = s.capTotals[key];
  if (total === undefined) {
    total = { reported: 0, unreported: 0 };
    s.capTotals[key] = total;
  }
  return total;
}
