import type { EndResult, Usage } from "../ledger/index.ts";
import { errorName, log } from "../log.ts";
import { FinalUsage } from "./usage.ts";

export const AFTER_CLIENT_GONE_MS = 30_000;
export const CLIENT_BACKLOG_MAX = 1_048_576;

export interface Cut {
  signal: AbortSignal;
  tighten(at: number): void;
}

export function cutAt(at: number, clock: () => number = Date.now): Cut & { clear(): void } {
  const controller = new AbortController();
  let deadline = at;
  let timer: ReturnType<typeof setTimeout> | null = null;
  const arm = () => {
    if (timer !== null) clearTimeout(timer);
    timer = setTimeout(() => controller.abort(), Math.max(0, deadline - clock()));
  };
  arm();
  return {
    signal: controller.signal,
    tighten(next) {
      if (next >= deadline || controller.signal.aborted) return;
      deadline = next;
      arm();
    },
    clear() {
      if (timer !== null) clearTimeout(timer);
    },
  };
}

type Trailer = { settled: true; outcome: EndResult["outcome"]; cost: number; settle_ms: number } | { settled: false; why: string };

export async function pump(o: {
  upstream: ReadableStream<Uint8Array>;
  client: WritableStream<Uint8Array>;
  cut: Cut;
  marginMs: number;
  settle: (usage: Usage) => Promise<EndResult>;
  clock?: () => number;
}): Promise<Trailer> {
  const clock = o.clock ?? Date.now;
  const reader = o.upstream.getReader();
  const writer = o.client.getWriter();
  const usage = new FinalUsage();
  let clientGone = false;
  let backlog = 0;
  const leave = () => {
    if (clientGone) return;
    clientGone = true;
    o.cut.tighten(clock() + AFTER_CLIENT_GONE_MS - o.marginMs);
  };
  const send = (bytes: Uint8Array) => {
    if (clientGone) return;
    if (backlog > 0 && backlog + bytes.byteLength > CLIENT_BACKLOG_MAX) {
      leave();
      return;
    }
    backlog += bytes.byteLength;
    writer.write(bytes).then(() => {
      backlog -= bytes.byteLength;
    }, leave);
  };
  let why = "the stream ended with no final usage";
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      usage.feed(value);
      send(value);
    }
  } catch (error) {
    why = `the stream was cut: ${errorName(error)}`;
    await reader.cancel().catch(() => undefined);
  }
  usage.end();
  const final = usage.result();
  let trailer: Trailer = { settled: false, why };
  if (final !== undefined) {
    const before = clock();
    try {
      const done = await o.settle(final);
      trailer = { settled: true, outcome: done.outcome, cost: done.outcome === "settled" || done.outcome === "expired" ? done.cost : 0, settle_ms: clock() - before };
    } catch (error) {
      log({ event: "handler-error", route: "none", status: 500, error: errorName(error) });
      trailer = { settled: false, why: "the settle call failed" };
    }
  }
  if (clientGone) {
    writer.abort().catch(() => undefined);
    return trailer;
  }
  writer.write(new TextEncoder().encode(`event: ledger\ndata: ${JSON.stringify(trailer)}\n\n`)).catch(() => undefined);
  writer.close().catch(() => undefined);
  return trailer;
}
