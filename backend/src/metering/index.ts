import type { Metering } from "./api.ts";

export * from "./api.ts";

export function makeMetering(config: { intervalSeconds: number; marginSeconds: number }): Metering {
  const intervalMs = config.intervalSeconds * 1000;
  return {
    init() {},
    begin() {},
    billingPeriod() {},
    intervalAt: (_sql, now) => Math.floor(now / intervalMs) * intervalMs,
    cut() {},
    oldestUnsealed: () => null,
    seal() {},
    send: () => ({ call: null, notBillable: 0 }),
    record: () => ({ call: null, notBillable: 0 }),
    nextSend: () => null,
    settled: () => true,
  };
}
