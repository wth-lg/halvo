import type { Micro } from "../types.ts";
import type { Price, Usage } from "./api.ts";

const TOKENS_PER_PRICE_UNIT = 1_000_000n;
const LARGEST_SAFE = BigInt(Number.MAX_SAFE_INTEGER);

function wholeCount(name: string, value: number): bigint {
  if (!Number.isSafeInteger(value) || value < 0) throw new RangeError(`${name} is not a whole number of 0 or more`);
  return BigInt(value);
}

export function costOf(price: Price, usage: Usage): Micro {
  const total =
    wholeCount("inputTokens", usage.inputTokens) * wholeCount("price.input", price.input) +
    wholeCount("outputTokens", usage.outputTokens) * wholeCount("price.output", price.output);
  const cost = (total + TOKENS_PER_PRICE_UNIT - 1n) / TOKENS_PER_PRICE_UNIT;
  if (cost > LARGEST_SAFE) throw new RangeError("the cost is above the largest integer that a number holds exactly");
  return Number(cost);
}
