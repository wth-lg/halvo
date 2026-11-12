import { expect, test } from "vitest";
import { makeStripePort } from "../../src/billing/stripe/adapter.ts";
import { startProviderFake } from "../../test-kit/provider-fake/fake.mjs";
import { PROVIDER_API_KEY, startStripeMock, WEBHOOK_SIGNING_SECRET } from "./servers.ts";

const KEY = PROVIDER_API_KEY;
const ENTRY = "http://127.0.0.1:1";

test("the fake answers a session it does not hold so that the adapter reads unknown", async () => {
  const mock = await startStripeMock();
  const fake = await startProviderFake({ mockUrl: mock.url, apiKey: KEY, signingSecret: WEBHOOK_SIGNING_SECRET, entries: [ENTRY], clock: () => 1_794_000_000_000 });
  try {
    const port = makeStripePort({ baseUrl: fake.origin, apiKey: KEY, signingSecret: WEBHOOK_SIGNING_SECRET, priceId: "price_test_halvo_local" });
    expect(await port.readSession("cs_test_not_held")).toEqual({ kind: "unknown" });
    const started = await port.startCheckout({ accountId: "3b7c1d2e-4f5a-4b6c-8d7e-9f0a1b2c3d4e", successUrl: `${ENTRY}/v1/done`, expiresAt: 1_794_001_860_000 });
    if (started.kind !== "session") throw new Error("the fake made no session");
    expect(started.expiresAt).toBe(1_794_001_860_000);
    expect(await port.readSession(started.sessionId)).toEqual({ kind: "open" });
    const requests = ((await (await fetch(`${fake.origin}/test/requests`)).json()) as { requests: unknown[] }).requests;
    expect(requests[0]).toEqual({ method: "GET", path: "/v1/checkout/sessions/cs_test_not_held", params: [], mock_status: 200, fake_status: 404 });
  } finally {
    await fake.close();
    await mock.stop();
  }
});
