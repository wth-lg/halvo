import type { Ms } from "../types.ts";

export interface Timing {
  voidIt: boolean;
  cutAt: Ms;
}

export function timingOf(receipt: { now: Ms; expiresAt: Ms }, sentAt: Ms, now: Ms, deadlineMs: number, marginMs: number): Timing {
  const life = receipt.expiresAt - receipt.now;
  return { voidIt: life - (now - sentAt) < deadlineMs + marginMs, cutAt: sentAt + life - marginMs };
}
