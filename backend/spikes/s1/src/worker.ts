import { costOf, type EndResult, Ledger, type LedgerEnv, type Micro, micro, type Price, type Usage } from "./ledger";

export { Ledger };

interface Env extends LedgerEnv {
  readonly LEDGER: DurableObjectNamespace<Ledger>;
  readonly UPSTREAM_PORT: string;
  readonly DEADLINE_MS: string;
  readonly MARGIN_MS: string;
  readonly INPUT_PRICE: string;
  readonly OUTPUT_PRICE: string;
}

interface Config {
  readonly upstream: string;
  readonly deadlineMs: number;
  readonly marginMs: number;
  readonly price: Price;
}

function positiveInteger(name: string, value: string): number {
  const number = Number(value);
  if (!Number.isSafeInteger(number) || number <= 0) throw new Error(`${name} must be a positive integer, not ${value}`);
  return number;
}

function readConfig(env: Env): Config {
  return {
    upstream: `http://127.0.0.1:${positiveInteger("UPSTREAM_PORT", env.UPSTREAM_PORT)}/v1/chat`,
    deadlineMs: positiveInteger("DEADLINE_MS", env.DEADLINE_MS),
    marginMs: positiveInteger("MARGIN_MS", env.MARGIN_MS),
    price: {
      input: micro(positiveInteger("INPUT_PRICE", env.INPUT_PRICE)),
      output: micro(positiveInteger("OUTPUT_PRICE", env.OUTPUT_PRICE)),
    },
  };
}

const json = (body: unknown, status = 200, headers: Record<string, string> = {}): Response =>
  new Response(`${JSON.stringify(body)}\n`, { status, headers: { "content-type": "application/json", ...headers } });

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === "object" && value !== null;
const count = (value: unknown): number | undefined =>
  typeof value === "number" && Number.isSafeInteger(value) && value >= 0 ? value : undefined;

const holdFor = (price: Price, bodyBytes: number, maxTokens: number): Micro =>
  costOf(price, { inputTokens: bodyBytes, outputTokens: maxTokens });

class FinalUsage {
  private readonly decoder = new TextDecoder();
  private pending = "";
  private openAiLastChunkUsage: Usage | undefined;
  private anthropicInputTokens: number | undefined;
  private anthropicCumulativeUsage: Usage | undefined;
  private final: Usage | undefined;

  feed(bytes: Uint8Array): void {
    this.pending += this.decoder.decode(bytes, { stream: true });
    for (;;) {
      const end = this.pending.indexOf("\n\n");
      if (end < 0) return;
      this.event(this.pending.slice(0, end));
      this.pending = this.pending.slice(end + 2);
    }
  }

  result(): Usage | undefined {
    return this.final;
  }

  private event(block: string): void {
    const data = block
      .split("\n")
      .filter((line) => line.startsWith("data:"))
      .map((line) => line.slice(5).trim())
      .join("\n");
    if (data === "") return;
    if (data === "[DONE]") {
      this.final = this.openAiLastChunkUsage;
      return;
    }
    let parsed: unknown;
    try {
      parsed = JSON.parse(data);
    } catch {
      this.openAiLastChunkUsage = undefined;
      return;
    }
    if (!isRecord(parsed)) return;
    const usage = isRecord(parsed.usage) ? parsed.usage : undefined;
    switch (parsed.type) {
      case undefined: {
        const input = count(usage?.prompt_tokens);
        const output = count(usage?.completion_tokens);
        this.openAiLastChunkUsage =
          input === undefined || output === undefined ? undefined : { inputTokens: input, outputTokens: output };
        return;
      }
      case "message_start": {
        const message = isRecord(parsed.message) && isRecord(parsed.message.usage) ? parsed.message.usage : undefined;
        this.anthropicInputTokens = count(message?.input_tokens);
        return;
      }
      case "message_delta": {
        const input = count(usage?.input_tokens) ?? this.anthropicInputTokens;
        const output = count(usage?.output_tokens);
        this.anthropicCumulativeUsage =
          input === undefined || output === undefined ? undefined : { inputTokens: input, outputTokens: output };
        return;
      }
      case "message_stop":
        this.final = this.anthropicCumulativeUsage;
        return;
      default:
        return;
    }
  }
}

type Trailer =
  | { readonly settled: true; readonly outcome: EndResult["outcome"]; readonly cost: number; readonly settle_ms: number }
  | { readonly settled: false; readonly why: string };

async function writeTrailerIfClientRemains(
  writer: WritableStreamDefaultWriter<Uint8Array>,
  trailer: Trailer,
): Promise<void> {
  try {
    await writer.write(new TextEncoder().encode(`event: ledger\ndata: ${JSON.stringify(trailer)}\n\n`));
    await writer.close();
  } catch {}
}

async function pump(
  upstream: ReadableStream<Uint8Array>,
  client: WritableStream<Uint8Array>,
  settle: (usage: Usage) => Promise<EndResult>,
): Promise<void> {
  const reader = upstream.getReader();
  const writer = client.getWriter();
  const usage = new FinalUsage();
  let clean = false;
  let why = "the stream ended with no final usage";
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      usage.feed(value);
      await writer.write(value);
    }
    clean = true;
  } catch (error) {
    why = `the stream was cut: ${error instanceof Error ? error.message : String(error)}`;
    await reader.cancel().catch(() => undefined);
  }
  const final = clean ? usage.result() : undefined;
  let trailer: Trailer = { settled: false, why };
  if (final) {
    const before = Date.now();
    const ended = await settle(final);
    const cost = ended.outcome === "settled" || ended.outcome === "expired" ? ended.cost : 0;
    trailer = { settled: true, outcome: ended.outcome, cost, settle_ms: Date.now() - before };
  }
  await writeTrailerIfClientRemains(writer, trailer);
}

async function chat(request: Request, config: Config, ledger: DurableObjectStub<Ledger>, ctx: ExecutionContext) {
  const requestId = request.headers.get("x-request-id");
  if (!requestId) return json({ error: "x-request-id is required" }, 400);
  const body = await request.arrayBuffer();
  let parsed: unknown;
  try {
    parsed = JSON.parse(new TextDecoder().decode(body));
  } catch {
    return json({ error: "the body is not JSON" }, 400);
  }
  const maxTokens = isRecord(parsed) ? count(parsed.max_tokens) : undefined;
  if (maxTokens === undefined || maxTokens === 0) return json({ error: "max_tokens must be a positive integer" }, 400);

  const reserved = await ledger.reserve(requestId, holdFor(config.price, body.byteLength, maxTokens), config.price);
  if (reserved.outcome === "refused") return json({ refused: reserved.reason }, 402);
  if (reserved.outcome === "repeat") return json({ repeat: reserved.state }, 409);
  const receipt = reserved.receipt;
  const receiptHeader = { "x-ledger-receipt": JSON.stringify(receipt) };

  const left = receipt.expiresAt - Date.now();
  if (left < config.deadlineMs + config.marginMs) {
    const ended = await ledger.void(requestId);
    if (ended.outcome !== "voided") return json({ upstream: "not called", hold: ended.outcome }, 502, receiptHeader);
    return json({ voided: true }, 503, receiptHeader);
  }

  const cutAt = receipt.expiresAt - config.marginMs;
  const cut = new AbortController();
  const timer = setTimeout(() => cut.abort(), Math.max(0, cutAt - Date.now()));
  let upstream: Response;
  try {
    upstream = await fetch(config.upstream, {
      method: "POST",
      headers: { "content-type": "application/json", "x-request-id": requestId },
      body,
      signal: cut.signal,
    });
  } catch (error) {
    clearTimeout(timer);
    return json({ upstream: "failed", why: error instanceof Error ? error.message : String(error) }, 502, receiptHeader);
  }
  if (!upstream.ok || !upstream.body) {
    clearTimeout(timer);
    await upstream.body?.cancel();
    return json({ upstream: upstream.status }, 502, receiptHeader);
  }
  const { readable, writable } = new TransformStream<Uint8Array, Uint8Array>();
  ctx.waitUntil(
    pump(upstream.body, writable, (usage) => ledger.settle(requestId, usage)).finally(() => clearTimeout(timer)),
  );
  return new Response(readable, { headers: { "content-type": "text/event-stream", ...receiptHeader } });
}

export default {
  async fetch(request, env, ctx): Promise<Response> {
    const url = new URL(request.url);
    if (url.pathname === "/health") return new Response("ok\n");
    const account = url.searchParams.get("account");
    if (!account) return json({ error: "account is required" }, 400);
    const ledger = env.LEDGER.get(env.LEDGER.idFromName(account));
    if (request.method === "POST" && url.pathname === "/cap") {
      const body: unknown = await request.json();
      const cap = isRecord(body) ? count(body.cap) : undefined;
      const periodMs = isRecord(body) ? count(body.period_ms) : undefined;
      const anchorMs = isRecord(body) && typeof body.anchor_ms === "number" ? body.anchor_ms : undefined;
      if (cap === undefined) return json({ error: "cap must be a non-negative integer of micro-units" }, 400);
      const period = periodMs === undefined || anchorMs === undefined ? undefined : { periodMs, anchorMs };
      await ledger.setCap(micro(cap), period);
      return json({ cap });
    }
    if (request.method === "GET" && url.pathname === "/ledger") return json(await ledger.inspect());
    if (request.method === "POST" && url.pathname === "/v1/chat") return chat(request, readConfig(env), ledger, ctx);
    return json({ error: "not found" }, 404);
  },
} satisfies ExportedHandler<Env>;
