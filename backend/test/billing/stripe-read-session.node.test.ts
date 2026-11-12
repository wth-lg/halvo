import { expect, test } from "vitest";
import { makeStripePort } from "../../src/billing/stripe/adapter.ts";
import { freePort, PROVIDER_API_KEY, startRecorder, WEBHOOK_SIGNING_SECRET } from "./servers.ts";

const ERROR = JSON.stringify({ error: { type: "invalid_request_error", code: "resource_missing", message: "No such checkout.session", param: "session" } });

function portAt(baseUrl: string) {
  return makeStripePort({ baseUrl, apiKey: PROVIDER_API_KEY, signingSecret: WEBHOOK_SIGNING_SECRET, priceId: "price_test" });
}

test("a read of a session tells unknown from failed", async () => {
  const cases: [string, { status: number; body: string }, string][] = [
    ["404 with the error object", { status: 404, body: ERROR }, "unknown"],
    ["404 without a body that parses", { status: 404, body: "not found" }, "failed"],
    ["404 with JSON that is no error object", { status: 404, body: JSON.stringify({ error: "missing" }) }, "failed"],
    ["404 with an error of an unknown type", { status: 404, body: JSON.stringify({ error: { type: "gone" } }) }, "failed"],
    ["404 with a known error type and no code", { status: 404, body: JSON.stringify({ error: { type: "invalid_request_error", message: "Unrecognized request URL" } }) }, "failed"],
    [
      "404 with a known error type and another code",
      { status: 404, body: JSON.stringify({ error: { type: "invalid_request_error", code: "parameter_unknown", message: "refused" } }) },
      "failed",
    ],
    ["500 with the error object", { status: 500, body: JSON.stringify({ error: { type: "api_error" } }) }, "failed"],
  ];
  for (const [name, reply, want] of cases) {
    const server = await startRecorder(() => reply);
    try {
      expect({ name, answer: await portAt(server.url).readSession("cs_test_1") }).toEqual({ name, answer: { kind: want } });
      expect(server.requests.map((each) => `${each.method} ${each.path}`)).toEqual(["GET /v1/checkout/sessions/cs_test_1"]);
    } finally {
      await server.close();
    }
  }
  const closed = await freePort();
  expect(await portAt(`http://127.0.0.1:${closed}`).readSession("cs_test_1")).toEqual({ kind: "failed" });
});
