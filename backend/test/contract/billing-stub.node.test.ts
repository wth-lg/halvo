import { expect, test } from "vitest";
import { providerPort } from "../../src/billing/index.ts";
import type { Env } from "../../src/types.ts";

test("the provider port without a provider key answers failed and rejected, nothing-to-cancel only for a null id, and opens no connection", async () => {
  const fetched: unknown[] = [];
  const real = globalThis.fetch;
  globalThis.fetch = async (...args: Parameters<typeof fetch>) => {
    fetched.push(args);
    return real(...args);
  };
  try {
    const port = providerPort({ PROVIDER_BASE_URL: "http://127.0.0.1:1" } as Env);
    expect(await port.startCheckout({ accountId: "a", successUrl: "http://127.0.0.1:1/v1/done", expiresAt: 1 })).toEqual({ kind: "failed" });
    expect(await port.verifyWebhook(new Uint8Array([123, 125]), new Headers(), 1)).toEqual({ kind: "rejected" });
    expect(await port.readSession("cs_1")).toEqual({ kind: "failed" });
    expect(await port.openManagePage({ customerId: "cus_1", returnUrl: "http://127.0.0.1:1/v1/done" })).toEqual({ kind: "failed" });
    expect(await port.cancelSubscription("sub_1")).toEqual({ kind: "failed" });
    expect(await port.cancelSubscription(null)).toEqual({ kind: "nothing-to-cancel" });
    expect(await port.sendUsage({ accountId: "a", customerId: "c", identifier: "a:0", amountMinor: 1, timestamp: 0 })).toEqual({ kind: "failed" });
    expect(await port.readUsageTotal({ accountId: "a", customerId: "c", from: 0, to: 1 })).toEqual({ kind: "failed" });
  } finally {
    globalThis.fetch = real;
  }
  expect(fetched).toEqual([]);
});

test("the provider port module exports providerPort and, from P3 on, PROVIDER_CALL_TIMEOUT_MS", async () => {
  const billing = await import("../../src/billing/index.ts");
  expect(Object.keys(billing).sort()).toEqual(["PROVIDER_CALL_TIMEOUT_MS", "providerPort"]);
});
