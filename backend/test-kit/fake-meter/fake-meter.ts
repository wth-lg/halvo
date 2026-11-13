
const HOUR = 3600;
const DAY = 24 * HOUR;

export const DOCUMENTED = {
  minUniquenessWindowSeconds: DAY,
  maxTimestampAgeSeconds: 35 * DAY,
  maxTimestampFutureSeconds: 300,
  defaultGraceSeconds: HOUR,
  maxGraceSeconds: 72 * HOUR,
} as const;

export type UnixSeconds = number;

export type MeterEvent = {
  readonly account: string;
  readonly identifier: string;
  readonly amountMinor: number;
  readonly timestamp: UnixSeconds;
};

export type RefusalReason =
  | "amount_not_whole_minor_units"
  | "timestamp_too_old"
  | "timestamp_in_future"
  | "unknown_account";

export type SendResult =
  | { readonly status: "recorded" }
  | { readonly status: "duplicate" }
  | { readonly status: "refused"; readonly reason: RefusalReason };

export type ReadResult = {
  readonly widenedFrom: UnixSeconds;
  readonly widenedTo: UnixSeconds;
  readonly totalMinor: number;
};

export type FakeMeterConfig = {
  readonly uniquenessWindowSeconds: number;
  readonly duplicateAnswer: "error" | "same-as-recorded";
  readonly graceSeconds: number;
  readonly invoicePeriodSeconds: number;
  readonly readLagSeconds: number;
  readonly readGranularitySeconds: number;
};

export type RecordedEvent = MeterEvent & { readonly receivedAt: UnixSeconds };

export type Period = { readonly index: number; readonly start: UnixSeconds; readonly end: UnixSeconds };

type Subscription = { readonly start: UnixSeconds; end: UnixSeconds | null };

export class FakeMeter {
  readonly config: FakeMeterConfig;
  private readonly subscriptions = new Map<string, Subscription>();
  private readonly events: RecordedEvent[] = [];
  private readonly lastRecordedAt = new Map<string, number>();
  private readonly counts = {
    recorded: 0,
    duplicates: 0,
    duplicatesWithOtherContent: 0,
    refused: 0,
    reads: 0,
  };

  constructor(config: FakeMeterConfig) {
    if (!(config.uniquenessWindowSeconds >= DOCUMENTED.minUniquenessWindowSeconds)) {
      throw new RangeError("the uniqueness window is at least 24 hours");
    }
    if (!(config.graceSeconds >= 0 && config.graceSeconds <= DOCUMENTED.maxGraceSeconds)) {
      throw new RangeError("the grace period is between 0 and 72 hours");
    }
    for (const seconds of [config.invoicePeriodSeconds, config.readGranularitySeconds]) {
      if (!(Number.isSafeInteger(seconds) && seconds >= 1)) {
        throw new RangeError("a period length is a whole number of seconds, 1 or more");
      }
    }
    if (!(Number.isSafeInteger(config.readLagSeconds) && config.readLagSeconds >= 0)) {
      throw new RangeError("the read lag is a whole number of seconds, 0 or more");
    }
    this.config = config;
  }

  subscribe(account: string, now: UnixSeconds): void {
    if (this.subscriptions.has(account)) throw new Error(`${account} is already subscribed`);
    this.subscriptions.set(account, { start: now, end: null });
  }

  endSubscription(account: string, now: UnixSeconds): number {
    const subscription = this.subscriptions.get(account);
    if (subscription === undefined) throw new Error(`${account} is not subscribed`);
    if (subscription.end === null) subscription.end = now + 1;
    return subscription.end;
  }

  resumeSubscription(account: string): void {
    const subscription = this.subscriptions.get(account);
    if (subscription === undefined) throw new Error(`${account} is not subscribed`);
    subscription.end = null;
  }

  send(event: MeterEvent, now: UnixSeconds): SendResult {
    if (!this.subscriptions.has(event.account)) return this.refuse("unknown_account");
    if (!(Number.isSafeInteger(event.amountMinor) && event.amountMinor >= 1)) {
      return this.refuse("amount_not_whole_minor_units");
    }
    if (!Number.isSafeInteger(event.timestamp)) return this.refuse("timestamp_too_old");
    if (event.timestamp < now - DOCUMENTED.maxTimestampAgeSeconds) {
      return this.refuse("timestamp_too_old");
    }
    if (event.timestamp > now + DOCUMENTED.maxTimestampFutureSeconds) {
      return this.refuse("timestamp_in_future");
    }
    const seenAt = this.lastRecordedAt.get(event.identifier);
    if (seenAt !== undefined && now - seenAt < this.config.uniquenessWindowSeconds) {
      this.counts.duplicates += 1;
      let held: RecordedEvent | undefined;
      for (const e of this.events) {
        if (e.identifier === event.identifier) held = e;
      }
      if (
        held !== undefined &&
        (held.account !== event.account ||
          held.amountMinor !== event.amountMinor ||
          held.timestamp !== event.timestamp)
      ) {
        this.counts.duplicatesWithOtherContent += 1;
      }
      return { status: this.config.duplicateAnswer === "error" ? "duplicate" : "recorded" };
    }
    this.events.push({ ...event, receivedAt: now });
    this.lastRecordedAt.set(event.identifier, now);
    this.counts.recorded += 1;
    return { status: "recorded" };
  }

  read(account: string, from: UnixSeconds, to: UnixSeconds, now: UnixSeconds): ReadResult {
    if (!(Number.isSafeInteger(from) && Number.isSafeInteger(to) && from < to)) {
      throw new RangeError("a read takes a range of whole seconds with from before to");
    }
    this.counts.reads += 1;
    const width = this.config.readGranularitySeconds;
    const widenedFrom = Math.floor(from / width) * width;
    const widenedTo = Math.ceil(to / width) * width;
    let totalMinor = 0;
    for (const e of this.events) {
      const visible = e.receivedAt + this.config.readLagSeconds <= now;
      if (e.account === account && visible && e.timestamp >= widenedFrom && e.timestamp < widenedTo) {
        totalMinor += e.amountMinor;
      }
    }
    return { widenedFrom, widenedTo, totalMinor };
  }


  exactTotal(account: string, from: UnixSeconds, to: UnixSeconds): number {
    let totalMinor = 0;
    for (const e of this.events) {
      if (e.account === account && e.timestamp >= from && e.timestamp < to) {
        totalMinor += e.amountMinor;
      }
    }
    return totalMinor;
  }

  recordedEvents(account: string): readonly RecordedEvent[] {
    return this.events.filter((e) => e.account === account);
  }

  periods(account: string, now: UnixSeconds): Period[] {
    const subscription = this.subscriptions.get(account);
    if (subscription === undefined) return [];
    const length = this.config.invoicePeriodSeconds;
    const stop = subscription.end ?? Number.POSITIVE_INFINITY;
    const periods: Period[] = [];
    for (let index = 0; ; index += 1) {
      const start = subscription.start + index * length;
      if (start > now || start >= stop) break;
      periods.push({ index, start, end: Math.min(start + length, stop) });
    }
    return periods;
  }

  invoicedTotal(account: string, period: Period): number {
    const finalizedAt = period.end + this.config.graceSeconds;
    let totalMinor = 0;
    for (const e of this.events) {
      const inPeriod = e.timestamp >= period.start && e.timestamp < period.end;
      if (e.account === account && inPeriod && e.receivedAt < finalizedAt) totalMinor += e.amountMinor;
    }
    return totalMinor;
  }

  stats(): Readonly<typeof this.counts> {
    return { ...this.counts };
  }

  private refuse(reason: RefusalReason): SendResult {
    this.counts.refused += 1;
    return { status: "refused", reason };
  }
}
