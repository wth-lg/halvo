
import { FakeMeter, type FakeMeterConfig } from "./fake-meter.ts";

const HOUR = 3600;
const DAY = 24 * HOUR;
const T0 = 1_800_000_000;

const base: FakeMeterConfig = {
  uniquenessWindowSeconds: DAY,
  duplicateAnswer: "error",
  graceSeconds: HOUR,
  invoicePeriodSeconds: 30 * DAY,
  readLagSeconds: 0,
  readGranularitySeconds: 1,
};

function meter(overrides: Partial<FakeMeterConfig> = {}): FakeMeter {
  const fake = new FakeMeter({ ...base, ...overrides });
  fake.subscribe("a", T0);
  return fake;
}

function event(identifier: string, amountMinor: number, timestamp: number, account = "a") {
  return { account, identifier, amountMinor, timestamp };
}

function throws(body: () => unknown): boolean {
  try {
    body();
    return false;
  } catch {
    return true;
  }
}

const EVERYTHING = [Number.NEGATIVE_INFINITY, Number.POSITIVE_INFINITY] as const;

export function selfCheck(): { name: string; ok: boolean }[] {
  const checks: { name: string; ok: boolean }[] = [];
  const check = (name: string, body: () => boolean): void => {
    checks.push({ name, ok: body() });
  };

  check("a duplicate one second inside the 24-hour window is not added (r1#84)", () => {
    const fake = meter();
    fake.send(event("x", 5, T0), T0);
    const again = fake.send(event("x", 5, T0), T0 + DAY - 1);
    return again.status === "duplicate" && fake.exactTotal("a", ...EVERYTHING) === 5;
  });
  check("the same identifier at the end of the window is added again (r1#84)", () => {
    const fake = meter();
    fake.send(event("x", 5, T0), T0);
    const again = fake.send(event("x", 5, T0), T0 + DAY);
    return again.status === "recorded" && fake.exactTotal("a", ...EVERYTHING) === 10;
  });
  check("a meter that never forgets answers a duplicate after 40 days", () => {
    const fake = meter({ uniquenessWindowSeconds: Number.POSITIVE_INFINITY });
    fake.send(event("x", 5, T0), T0);
    const again = fake.send(event("x", 5, T0 + 40 * DAY), T0 + 40 * DAY);
    return again.status === "duplicate" && fake.exactTotal("a", ...EVERYTHING) === 5;
  });
  check("a duplicate does not restart the window (r1#84)", () => {
    const fake = meter();
    fake.send(event("x", 5, T0), T0);
    const inside = fake.send(event("x", 5, T0), T0 + 23 * HOUR);
    const after = fake.send(event("x", 5, T0), T0 + DAY);
    return inside.status === "duplicate" && after.status === "recorded" && fake.exactTotal("a", ...EVERYTHING) === 10;
  });
  check("the window runs from the second an event is received, not from its timestamp", () => {
    const fake = meter();
    const now = T0 + 20 * DAY;
    fake.send(event("x", 5, now - 10 * DAY), now);
    const again = fake.send(event("x", 5, now - 10 * DAY), now + DAY - 1);
    return again.status === "duplicate" && fake.exactTotal("a", ...EVERYTHING) === 5;
  });
  check("the read lag runs from the second an event is received, not from its timestamp", () => {
    const fake = meter({ readLagSeconds: HOUR });
    const now = T0 + 2 * DAY;
    fake.send(event("x", 5, now - DAY), now);
    const early = fake.read("a", now - DAY, now - DAY + 1, now + HOUR - 1).totalMinor;
    const late = fake.read("a", now - DAY, now - DAY + 1, now + HOUR).totalMinor;
    return early === 0 && late === 5;
  });
  check("the fake counts a duplicate that carries other content", () => {
    const fake = meter();
    fake.send(event("x", 5, T0), T0);
    fake.send(event("x", 5, T0), T0 + 1);
    const same = fake.stats().duplicatesWithOtherContent;
    fake.send(event("x", 6, T0), T0 + 2);
    return same === 0 && fake.stats().duplicatesWithOtherContent === 1 && fake.stats().duplicates === 2;
  });
  check("the record, the total and the invoice keep accounts apart and leave out the second a range ends", () => {
    const fake = meter({ graceSeconds: 72 * HOUR });
    fake.subscribe("b", T0);
    const [first] = fake.periods("a", T0 + 31 * DAY);
    if (first === undefined) return false;
    fake.send(event("x", 5, first.start), T0);
    fake.send(event("y", 7, first.end - 1), first.end + 72 * HOUR - 1);
    fake.send(event("z", 9, first.end), first.end + 1);
    fake.send(event("w", 11, first.start, "b"), T0);
    return (
      fake.recordedEvents("a").length === 3 &&
      fake.exactTotal("a", first.start, first.end) === 12 &&
      fake.invoicedTotal("a", first) === 12 &&
      fake.exactTotal("b", first.start, first.end) === 11
    );
  });
  check("a duplicate can get the same answer as a recorded event", () => {
    const fake = meter({ duplicateAnswer: "same-as-recorded" });
    fake.send(event("x", 5, T0), T0);
    return fake.send(event("x", 5, T0), T0 + 1).status === "recorded" && fake.exactTotal("a", ...EVERYTHING) === 5;
  });
  check("identifiers are one namespace across accounts", () => {
    const fake = meter();
    fake.subscribe("b", T0);
    fake.send(event("x", 5, T0), T0);
    return fake.send(event("x", 7, T0, "b"), T0).status === "duplicate" && fake.exactTotal("b", ...EVERYTHING) === 0;
  });
  check("a timestamp 35 days old is taken and one second older is refused (r1#83)", () => {
    const fake = meter();
    const now = T0 + 40 * DAY;
    const atLimit = fake.send(event("x", 1, now - 35 * DAY), now);
    const older = fake.send(event("y", 1, now - 35 * DAY - 1), now);
    return atLimit.status === "recorded" && older.status === "refused" && older.reason === "timestamp_too_old";
  });
  check("a timestamp 5 minutes ahead is taken and one second later is refused (r1#83)", () => {
    const fake = meter();
    const atLimit = fake.send(event("x", 1, T0 + 300), T0);
    const later = fake.send(event("y", 1, T0 + 301), T0);
    return atLimit.status === "recorded" && later.status === "refused" && later.reason === "timestamp_in_future";
  });
  check("an amount that is not a whole minor unit of 1 or more is refused", () => {
    const fake = meter();
    const refused = [0, -1, 1.5, Number.NaN].map((amount) => fake.send(event(`x${amount}`, amount, T0), T0));
    return refused.every((r) => r.status === "refused") && fake.exactTotal("a", ...EVERYTHING) === 0;
  });
  check("an event for an account that never subscribed is refused", () => {
    const refused = meter().send(event("x", 1, T0, "nobody"), T0);
    return refused.status === "refused" && refused.reason === "unknown_account";
  });
  check("a read sums one account over a range that ends before its last second", () => {
    const fake = meter();
    fake.subscribe("b", T0);
    fake.send(event("x", 5, T0 + 10), T0 + 10);
    fake.send(event("y", 7, T0 + 20), T0 + 20);
    fake.send(event("z", 9, T0 + 10, "b"), T0 + 20);
    const read = fake.read("a", T0 + 10, T0 + 20, T0 + 30);
    return read.totalMinor === 5 && read.widenedFrom === T0 + 10 && read.widenedTo === T0 + 20;
  });
  check("a read leaves out an event younger than the read lag, then shows it (r1#83)", () => {
    const fake = meter({ readLagSeconds: HOUR });
    fake.send(event("x", 5, T0), T0);
    const early = fake.read("a", T0, T0 + 1, T0 + HOUR - 1).totalMinor;
    const late = fake.read("a", T0, T0 + 1, T0 + HOUR).totalMinor;
    return early === 0 && late === 5;
  });
  check("a read widens its range to whole windows and says so", () => {
    const fake = meter({ readGranularitySeconds: 60 });
    const minute = Math.floor((T0 + 90) / 60) * 60;
    fake.send(event("x", 5, minute), minute);
    fake.send(event("y", 7, minute + 59), minute + 59);
    fake.send(event("z", 9, minute + 60), minute + 60);
    const read = fake.read("a", minute + 30, minute + 31, minute + 120);
    return read.totalMinor === 12 && read.widenedFrom === minute && read.widenedTo === minute + 60;
  });
  check("usage that arrives inside the grace period is on the period's invoice (r1#85)", () => {
    const fake = meter();
    const [first] = fake.periods("a", T0 + 31 * DAY);
    if (first === undefined) return false;
    fake.send(event("x", 5, first.end - 1), first.end + HOUR - 1);
    return fake.invoicedTotal("a", first) === 5;
  });
  check("usage that arrives after the grace period is in the total and on no invoice (r1#85)", () => {
    const fake = meter();
    const [first, second] = fake.periods("a", T0 + 31 * DAY);
    if (first === undefined || second === undefined) return false;
    fake.send(event("x", 5, first.end - 1), first.end + HOUR);
    return (
      fake.exactTotal("a", first.start, first.end) === 5 &&
      fake.invoicedTotal("a", first) === 0 &&
      fake.invoicedTotal("a", second) === 0
    );
  });
  check("the end of a subscription closes the last period after the second it was asked in", () => {
    const fake = meter();
    const end = fake.endSubscription("a", T0 + 5 * DAY);
    const repeat = fake.endSubscription("a", T0 + 9 * DAY);
    const periods = fake.periods("a", T0 + 90 * DAY);
    return end === T0 + 5 * DAY + 1 && repeat === end && periods.length === 1 && periods[0]?.end === end;
  });
  check("invoice periods run from the subscription's start, 30 days each", () => {
    const fake = meter();
    const periods = fake.periods("a", T0 + 61 * DAY);
    return periods.length === 3 && periods[1]?.start === T0 + 30 * DAY && periods[2]?.end === T0 + 90 * DAY;
  });
  check("a window under 24 hours or a grace period over 72 hours is not a meter (r1#84, r1#85)", () => {
    return (
      throws(() => new FakeMeter({ ...base, uniquenessWindowSeconds: DAY - 1 })) &&
      throws(() => new FakeMeter({ ...base, graceSeconds: 72 * HOUR + 1 }))
    );
  });
  return checks;
}
