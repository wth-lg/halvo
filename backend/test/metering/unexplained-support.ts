import type { Sql, Step } from "../../src/types.ts";
import { ACCOUNT, HOUR, START, subscriptionEvent, type World } from "./world.ts";

export const END = START + HOUR;
export const TIMESTAMP = END / 1000 - 1;
export const CUT = END + 30_000;
export const SPAN = 86_100_000;
export const NOT_BILLABLE = '{"event":"usage-not-billable"}';
export const UNEXPLAINED = '{"event":"usage-total-unexplained"}';

type Row = Record<string, string | number | null>;

export function kinds(calls: { kind: string; answer: string }[]): string[] {
  return calls.map((each) => `${each.kind} ${each.answer}`);
}

export function count(lines: string[], line: string): number {
  return lines.filter((each) => each === line).length;
}

/** Puts an event that no interval owns at the meter, in the minute of the first interval's timestamp, received at `at`. */
export function foreign(w: World, amountMinor: number, at: number = END): void {
  w.meter.send({ account: ACCOUNT, identifier: `${ACCOUNT}:foreign`, amountMinor, timestamp: TIMESTAMP - 10 }, Math.floor(at / 1000));
}

/**
 * Two intervals stamped in one minute: the first, [START, END), with `first` micro-units, sealed and sent by the clock
 * call at END + 10 s, and the second, [END, CUT), with `second`, which a cancel that states its end at CUT cuts.
 */
export async function cutPair(w: World, first = 30_000, second = 20_000): Promise<void> {
  await w.use("r1", first);
  await w.clock(END + 10_000);
  await w.use("r2", second);
  await w.entries().webhookEvent(subscriptionEvent({ eventId: "evt_cancel", state: "cancelled", providerTime: CUT, endedAt: CUT }));
}

/** Moves the clock to the next try of the oldest interval that is sealed and open, while `go` holds, at most `max` times. */
export async function tries(w: World, go: (rows: Row[]) => boolean, max = 200): Promise<void> {
  for (let each = 0; each < max; each += 1) {
    const rows = await w.intervals();
    if (!go(rows)) return;
    const open = rows.find((row) => row.sealed_at !== null && row.outcome === null);
    if (open === undefined) return;
    const answer = await w.clock(Number(open.next_attempt));
    if (answer !== "clock-set") throw new Error(`the clock answered ${answer}`);
  }
  throw new Error(`more than ${max} tries`);
}

export const open = (index: number) => (rows: Row[]) => rows[index]?.outcome === null;

/**
 * Wraps the host's step: once armed, the next step commits and the object then stops, as a platform stop between
 * that step's commit and whatever the entry does after it.
 */
export function stopAfterTheNextCommit(w: World): () => void {
  const inner = w.host.step;
  let armed = false;
  const step: Step = <T>(run: (sql: Sql) => T): T => {
    const value = inner(run);
    if (armed) {
      armed = false;
      const error = new Error("the object stopped");
      error.name = "ObjectStoppedError";
      throw error;
    }
    return value;
  };
  w.host.step = step;
  return () => {
    armed = true;
  };
}
