import { credentialHash, readAccountHeaders } from "../app.ts";
import { routeTable } from "../contract/routes.ts";
import type { ReserveResult } from "../ledger/index.ts";
import { costOf } from "../ledger/cost.ts";
import type { PriceTable } from "../ledger/prices.ts";
import { errorName, log, numberVariable } from "../log.ts";
import type { Env } from "../types.ts";
import prices from "../../test-kit/prices/v1.json";
import { CHAT_BODY_LIMIT, readChatBody } from "./body.ts";
import { cutAt, pump } from "./stream.ts";
import { timingOf } from "./timing.ts";

export const CHAT_PATH = "/v1/chat";
const REQUEST_ID = /^[0-9A-Za-z-]{1,64}$/;
const CONTENT_TYPE = new RegExp(routeTable.headers["content-type"].pattern);
const PRICES: PriceTable = prices;
const JSON_TYPE = { "content-type": "application/json" };

function answer(status: number, body: object, headers: Record<string, string> = {}): Response {
  return new Response(JSON.stringify(body), { status, headers: { ...JSON_TYPE, ...headers } });
}

function rejected(status: number): Response {
  log({ event: "rejected", route: "none", status });
  return answer(status, { result: "rejected" });
}

async function readAtMost(request: Request, limit: number): Promise<{ text: string; size: number } | null> {
  if (request.body === null) return { text: "", size: 0 };
  const reader = request.body.getReader();
  const chunks: Uint8Array[] = [];
  let size = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    size += value.byteLength;
    if (size > limit) {
      await reader.cancel();
      return null;
    }
    chunks.push(value);
  }
  const bytes = new Uint8Array(size);
  let at = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, at);
    at += chunk.byteLength;
  }
  return { text: new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(bytes), size };
}

type Admitted = Extract<ReserveResult, { outcome: "admitted" }>["receipt"];

async function serve(request: Request, env: Env, ctx: ExecutionContext): Promise<Response> {
  if (new URL(request.url).search !== "" || request.url.includes("?")) return rejected(400);
  const ids = readAccountHeaders(request.headers);
  const sentId = request.headers.get("x-request-id");
  if (ids === null || (sentId !== null && !REQUEST_ID.test(sentId))) return rejected(400);
  if (!CONTENT_TYPE.test(request.headers.get("content-type") ?? "")) return rejected(400);
  let body: { text: string; size: number } | null;
  try {
    body = await readAtMost(request, CHAT_BODY_LIMIT);
  } catch {
    return rejected(400);
  }
  if (body === null) return rejected(413);
  const asked = readChatBody(body.text);
  if (asked === null) return rejected(400);
  const deadlineMs = numberVariable("UPSTREAM_DEADLINE_MS", env.UPSTREAM_DEADLINE_MS);
  const marginMs = numberVariable("DISPATCH_MARGIN_MS", env.DISPATCH_MARGIN_MS);
  const upstreamBase = env.UPSTREAM_BASE_URL ?? "";
  if (upstreamBase === "") return answer(502, { result: "upstream-failed", why: "no upstream" });

  const requestId = sentId ?? crypto.randomUUID();
  const rates = PRICES[asked.tier];
  const price = { input: rates.input, output: rates.output };
  const amount = costOf(price, { inputTokens: body.size, outputTokens: asked.outputLimit * asked.choices });
  const object = env.ACCOUNT.get(env.ACCOUNT.idFromName(ids.accountId));
  const hash = await credentialHash(ids.credential);
  const sentAt = Date.now();
  const reserved = await object.reserve(hash, { requestId, tier: asked.tier, amount, price });
  if (reserved.outcome === "refused") return answer(403, { result: "refused", reason: reserved.reason });
  if (reserved.outcome === "repeat" && (reserved.state !== "open" || reserved.dispatched)) {
    return answer(409, { result: "repeat", state: reserved.state, dispatched: reserved.dispatched });
  }
  const receipt: Admitted = reserved.receipt;
  const receiptHeader = { "x-ledger-receipt": JSON.stringify(receipt) };
  const timing = timingOf(receipt, sentAt, Date.now(), deadlineMs, marginMs);
  if (timing.voidIt) {
    const ended = await object.void(requestId);
    if (ended.outcome === "voided") return answer(503, { result: "voided" }, receiptHeader);
    return answer(502, { result: "no-time-left", hold: ended.outcome }, receiptHeader);
  }
  if ((await object.dispatch(requestId)) !== "go") return answer(409, { result: "stopped" }, receiptHeader);

  const cut = cutAt(timing.cutAt);
  let upstream: Response;
  try {
    upstream = await fetch(`${upstreamBase}/v1/chat`, {
      method: "POST",
      headers: { ...JSON_TYPE, "x-request-id": requestId },
      body: asked.forward,
      signal: cut.signal,
    });
  } catch (error) {
    cut.clear();
    return answer(502, { result: "upstream-failed", why: errorName(error) }, receiptHeader);
  }
  if (!upstream.ok || upstream.body === null) {
    await upstream.body?.cancel().catch(() => undefined);
    if (rates.notBilled.includes(upstream.status)) await object.settle(requestId, { inputTokens: 0, outputTokens: 0 });
    cut.clear();
    return answer(502, { result: "upstream-failed", status: upstream.status }, receiptHeader);
  }
  const { readable, writable } = new TransformStream<Uint8Array, Uint8Array>();
  ctx.waitUntil(
    pump({ upstream: upstream.body, client: writable, cut, marginMs, settle: (usage) => object.settle(requestId, usage) })
      .catch((error) => log({ event: "handler-error", route: "none", status: 500, error: errorName(error) }))
      .finally(() => cut.clear()),
  );
  return new Response(readable, { headers: { "content-type": "text/event-stream", ...receiptHeader } });
}

export async function chat(request: Request, env: Env, ctx: ExecutionContext): Promise<Response> {
  try {
    return await serve(request, env, ctx);
  } catch (error) {
    log({ event: "handler-error", route: "none", status: 500, error: errorName(error) });
    return new Response(null, { status: 500 });
  }
}
