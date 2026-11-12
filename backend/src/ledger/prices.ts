import type { Micro, Tier } from "../types.ts";

export type PriceTable = Record<Tier, { input: Micro; output: Micro; notBilled: number[] }>;
