import type { Env } from "../types.ts";
import type { ProviderPort } from "./port.ts";
import { makeStripePort } from "./stripe/adapter.ts";

export { PROVIDER_CALL_TIMEOUT_MS } from "./stripe/timeout.ts";

export function providerPort(env: Env): ProviderPort {
  return makeStripePort({
    baseUrl: env.PROVIDER_BASE_URL ?? "",
    apiKey: env.PROVIDER_API_KEY ?? "",
    signingSecret: env.WEBHOOK_SIGNING_SECRET ?? "",
    priceId: env.PROVIDER_PRICE_ID ?? "",
  });
}
