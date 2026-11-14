import type { EndResult, Price, ReserveResult, Usage } from "../../src/ledger/index.ts";
import type { Ms } from "../../src/types.ts";
import type { Answer, Call, Step } from "./harness.ts";

const WEEK = 604_800_000;
const MICRO_PER_MINOR = 10_000;

interface ModelHold {
  amount: number;
  price: Price;
  tier: string;
  expiresAt: Ms;
  periodStart: Ms;
  state: "open" | "settled" | "voided" | "expired";
  dispatched: boolean;
  voidedAtExpiry: boolean;
  cost: number | null;
  overrun: number;
  late: boolean;
}

export interface ModelTotals {
  reported: number;
  unreported: number;
  open: number;
}

const costOf = (price: Price, usage: Usage) =>
  Number((BigInt(usage.inputTokens) * BigInt(price.input) + BigInt(usage.outputTokens) * BigInt(price.output) + 999_999n) / 1_000_000n);

export class Model {
  readonly holds = new Map<string, ModelHold>();
  readonly periods = new Map<Ms, ModelTotals>();
  private readonly anchor: Ms;
  private readonly holdLifeMs: number;
  private lastNow: Ms;
  private capAmount: number;

  constructor(anchor: Ms, capAmount: number, holdLifeMs: number) {
    this.anchor = anchor;
    this.holdLifeMs = holdLifeMs;
    this.lastNow = anchor;
    this.capAmount = capAmount;
  }

  private periodStart(now: Ms): Ms {
    return now - ((((now - this.anchor) % WEEK) + WEEK) % WEEK);
  }

  private totals(start: Ms): ModelTotals {
    let found = this.periods.get(start);
    if (found === undefined) {
      found = { reported: 0, unreported: 0, open: 0 };
      this.periods.set(start, found);
    }
    return found;
  }

  private expire(now: Ms): void {
    for (const hold of this.holds.values()) {
      if (hold.state === "open" && hold.expiresAt <= now) {
        const totals = this.totals(hold.periodStart);
        totals.open -= hold.amount;
        if (hold.dispatched) {
          hold.state = "expired";
          totals.unreported += hold.amount;
        } else {
          hold.state = "voided";
          hold.voidedAtExpiry = true;
        }
      }
    }
  }

  private capAnswer(now: Ms) {
    const start = this.periodStart(now);
    const totals = this.periods.get(start) ?? { reported: 0, unreported: 0, open: 0 };
    return {
      result: "cap",
      cap: { amount: this.capAmount, period: "week" },
      spent: totals.reported + totals.unreported,
      held: totals.open,
      period_end: start + WEEK,
    };
  }

  run(step: Step): Answer {
    const now = Math.max(step.at, this.lastNow);
    this.lastNow = now;
    const call: Call = step.call;
    if (call.kind === "restart") return null;
    this.expire(now);
    switch (call.kind) {
      case "alarm":
        return null;
      case "capGet": {
        const read = this.capAnswer(now);
        return { result: "cap", cap: read.cap, spend: { spent: read.spent, held: read.held, period_end: read.period_end } };
      }
      case "capSet": {
        this.capAmount = call.cap.amount;
        const read = this.capAnswer(now);
        return { result: "cap", cap: read.cap, spend: { spent: read.spent, held: read.held, period_end: read.period_end } };
      }
      case "reserve":
        return this.reserve(call, now);
      case "dispatch": {
        const hold = this.holds.get(call.requestId);
        if (hold === undefined || hold.state !== "open" || hold.dispatched) return "stop";
        hold.dispatched = true;
        return "go";
      }
      case "settle":
        return this.settle(call.requestId, call.usage);
      case "void":
        return this.void(call.requestId);
    }
  }

  private reserve(call: Extract<Call, { kind: "reserve" }>, now: Ms): ReserveResult {
    const price = call.price ?? { input: 3_100_000, output: 15_700_000 };
    const tier = call.tier ?? "low_cost_hosted";
    const found = this.holds.get(call.requestId);
    if (found !== undefined) {
      if (found.amount !== call.amount || found.tier !== tier || found.price.input !== price.input || found.price.output !== price.output) {
        return { outcome: "refused", reason: "other-arguments" };
      }
      return { outcome: "repeat", state: found.state, dispatched: found.dispatched, receipt: { requestId: call.requestId, amount: found.amount, now, expiresAt: found.expiresAt } };
    }
    const start = this.periodStart(now);
    const totals = this.periods.get(start) ?? { reported: 0, unreported: 0, open: 0 };
    if (totals.reported + totals.unreported + totals.open + call.amount > this.capAmount * MICRO_PER_MINOR) return { outcome: "refused", reason: "over-cap" };
    this.totals(start).open += call.amount;
    const expiresAt = now + this.holdLifeMs;
    this.holds.set(call.requestId, { amount: call.amount, price, tier, expiresAt, periodStart: start, state: "open", dispatched: false, voidedAtExpiry: false, cost: null, overrun: 0, late: false });
    return { outcome: "admitted", receipt: { requestId: call.requestId, amount: call.amount, now, expiresAt } };
  }

  private settle(requestId: string, usage: Usage): EndResult {
    const hold = this.holds.get(requestId);
    if (hold === undefined) return { outcome: "refused", reason: "unknown-request" };
    if (hold.state === "voided") return { outcome: "refused", reason: "already-ended" };
    if (hold.state === "settled") return { outcome: "settled", cost: hold.cost ?? 0, overrun: hold.overrun, repeat: true };
    if (hold.state === "expired") {
      if (!hold.late) {
        hold.late = true;
        hold.cost = costOf(hold.price, usage);
        hold.overrun = Math.max(0, hold.cost - hold.amount);
      }
      return { outcome: "expired", cost: hold.cost ?? 0, overrun: hold.overrun };
    }
    const cost = costOf(hold.price, usage);
    hold.state = "settled";
    hold.cost = cost;
    hold.overrun = Math.max(0, cost - hold.amount);
    const totals = this.totals(hold.periodStart);
    totals.open -= hold.amount;
    totals.reported += cost;
    return { outcome: "settled", cost, overrun: hold.overrun, repeat: false };
  }

  private void(requestId: string): EndResult {
    const hold = this.holds.get(requestId);
    if (hold === undefined) return { outcome: "refused", reason: "unknown-request" };
    if (hold.state === "voided" && !hold.voidedAtExpiry) return { outcome: "voided" };
    if (hold.state !== "open") return { outcome: "refused", reason: "already-ended" };
    if (hold.dispatched) return { outcome: "refused", reason: "dispatched" };
    hold.state = "voided";
    this.totals(hold.periodStart).open -= hold.amount;
    return { outcome: "voided" };
  }
}
