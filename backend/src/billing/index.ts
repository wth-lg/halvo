import { makeStripeMeter } from "../metering/stripe-meter.ts";
import type { Env } from "../types.ts";
import type { ProviderPort } from "./port.ts";
import { makeStripePort } from "./stripe/adapter.ts";

export { PROVIDER_CALL_TIMEOUT_MS } from "./stripe/timeout.ts";

export function providerPort(env: Env): ProviderPort {
  const port = makeStripePort({
    baseUrl: env.PROVIDER_BASE_URL ?? "",
    apiKey: env.PROVIDER_API_KEY ?? "",
    signingSecret: env.WEBHOOK_SIGNING_SECRET ?? "",
    priceId: env.PROVIDER_PRICE_ID ?? "",
  });
  const meter = makeStripeMeter({
    baseUrl: env.PROVIDER_BASE_URL ?? "",
    apiKey: env.PROVIDER_API_KEY ?? "",
    meterId: env.PROVIDER_METER_ID ?? "",
    eventName: env.PROVIDER_METER_EVENT ?? "",
  });
  return { ...port, sendUsage: meter.sendUsage, readUsageTotal: meter.readUsageTotal };
}
