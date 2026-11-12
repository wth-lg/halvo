import type { Ms, Period } from "../types.ts";

export const WEEK_MS = 604_800_000;

export interface CapPeriod {
  start: Ms;
  end: Ms;
}

function remainder(value: number, by: number): number {
  return ((value % by) + by) % by;
}

function monthBoundary(anchor: Ms, months: number): Ms {
  const at = new Date(anchor);
  const first = new Date(Date.UTC(at.getUTCFullYear(), at.getUTCMonth() + months, 1));
  const year = first.getUTCFullYear();
  const month = first.getUTCMonth();
  const lastDay = new Date(Date.UTC(year, month + 1, 0)).getUTCDate();
  const day = Math.min(at.getUTCDate(), lastDay);
  return Date.UTC(year, month, day, at.getUTCHours(), at.getUTCMinutes(), at.getUTCSeconds(), at.getUTCMilliseconds());
}

export function periodAt(period: Period, anchor: Ms, now: Ms): CapPeriod {
  if (period === "week") {
    const start = now - remainder(now - anchor, WEEK_MS);
    return { start, end: start + WEEK_MS };
  }
  const at = new Date(anchor);
  const then = new Date(now);
  let months = (then.getUTCFullYear() - at.getUTCFullYear()) * 12 + (then.getUTCMonth() - at.getUTCMonth());
  if (monthBoundary(anchor, months) > now) months -= 1;
  return { start: monthBoundary(anchor, months), end: monthBoundary(anchor, months + 1) };
}
