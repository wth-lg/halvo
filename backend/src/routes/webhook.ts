import { providerPort } from "../billing/index.ts";
import { log } from "../log.ts";
import type { Env } from "../types.ts";

const REJECTED_BODY = '{"result":"rejected"}';

export async function webhook(rawBody: Uint8Array, headers: Headers, env: Env): Promise<Response> {
  const verified = await providerPort(env).verifyWebhook(rawBody, headers, Date.now());
  if (verified.kind === "rejected") {
    log({ event: "rejected", route: "webhook", status: 400 });
    return new Response(REJECTED_BODY, { status: 400, headers: { "content-type": "application/json" } });
  }
  const { event } = verified;
  if (event.kind !== "ignored") await env.ACCOUNT.get(env.ACCOUNT.idFromName(event.accountId)).webhookEvent(event);
  return new Response(null, { status: 200 });
}
