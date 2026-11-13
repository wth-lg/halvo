import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { EndResult, Usage } from "../../src/ledger/index.ts";
import { AFTER_CLIENT_GONE_MS, CLIENT_BACKLOG_MAX, type Cut, cutAt, pump } from "../../src/gateway/stream.ts";

const encoder = new TextEncoder();
const chunk = (data: string) => encoder.encode(`data: ${data}\n\n`);
const USAGE_CHUNK = chunk(JSON.stringify({ choices: [], usage: { prompt_tokens: 7, completion_tokens: 9 } }));
const SETTLED: EndResult = { outcome: "settled", cost: 42, overrun: 0, repeat: false };

let lines: unknown[] = [];
beforeEach(() => {
  lines = [];
  vi.spyOn(console, "log").mockImplementation((text: unknown) => {
    lines.push(JSON.parse(String(text)));
  });
});
afterEach(() => {
  vi.restoreAllMocks();
});

function upstreamOf(parts: Uint8Array[], gapMs = 0): { stream: ReadableStream<Uint8Array>; pulled: () => number } {
  let at = 0;
  return {
    stream: new ReadableStream<Uint8Array>({
      async pull(controller) {
        if (gapMs > 0) await new Promise((resolve) => setTimeout(resolve, gapMs));
        const next = parts[at];
        at += 1;
        if (next === undefined) controller.close();
        else controller.enqueue(next);
      },
    }),
    pulled: () => at,
  };
}

function quietCut(): Cut & { tightened: number[] } {
  const tightened: number[] = [];
  return { signal: new AbortController().signal, tighten: (at) => tightened.push(at), tightened };
}

async function readAll(stream: ReadableStream<Uint8Array>): Promise<string> {
  return new Response(stream).text();
}

test("with the client listening, the stream is passed on, settled from the final usage and closed with the ledger event", async () => {
  const upstream = upstreamOf([chunk('{"choices":[{"delta":{"content":"a"}}]}'), USAGE_CHUNK, chunk("[DONE]")]);
  const { readable, writable } = new TransformStream<Uint8Array, Uint8Array>();
  const settled: Usage[] = [];
  const [trailer, text] = await Promise.all([
    pump({ upstream: upstream.stream, client: writable, cut: quietCut(), marginMs: 400, settle: async (usage) => {
      settled.push(usage);
      return SETTLED;
    }, clock: () => 0 }),
    readAll(readable),
  ]);
  expect(settled).toEqual([{ inputTokens: 7, outputTokens: 9 }]);
  expect(trailer).toEqual({ settled: true, outcome: "settled", cost: 42, settle_ms: 0 });
  expect(text.endsWith(`event: ledger\ndata: ${JSON.stringify(trailer)}\n\n`)).toBe(true);
});

test("when the client leaves mid-stream, the gateway reads the upstream answer to its end and settles the reported amount (contract 10)", async () => {
  const upstream = upstreamOf([chunk('{"choices":[{"delta":{"content":"a"}}]}'), chunk('{"choices":[{"delta":{"content":"b"}}]}'), USAGE_CHUNK, chunk("[DONE]")], 5);
  const { readable, writable } = new TransformStream<Uint8Array, Uint8Array>();
  const reader = readable.getReader();
  const cut = quietCut();
  const settled: Usage[] = [];
  const leaving = reader.read().then(() => reader.cancel());
  const trailer = await pump({ upstream: upstream.stream, client: writable, cut, marginMs: 400, settle: async (usage) => {
      settled.push(usage);
      return SETTLED;
    }, clock: () => 1000 });
  await leaving;
  expect(upstream.pulled()).toBe(5);
  expect(settled).toEqual([{ inputTokens: 7, outputTokens: 9 }]);
  expect(trailer).toMatchObject({ settled: true, outcome: "settled" });
  expect(cut.tightened).toEqual([1000 + AFTER_CLIENT_GONE_MS - 400]);
});

test("a stream cut before its final usage is not settled, so its hold expires in full", async () => {
  const controller = new AbortController();
  const upstream = new ReadableStream<Uint8Array>({
    start(stream) {
      stream.enqueue(chunk('{"choices":[{"delta":{"content":"a"}}]}'));
      controller.signal.addEventListener("abort", () => stream.error(new DOMException("cut", "AbortError")));
    },
  });
  const { readable, writable } = new TransformStream<Uint8Array, Uint8Array>();
  const settle = vi.fn(async () => SETTLED);
  const done = pump({ upstream, client: writable, cut: { signal: controller.signal, tighten: () => undefined }, marginMs: 400, settle });
  const text = readAll(readable);
  setTimeout(() => controller.abort(), 20);
  expect(await done).toEqual({ settled: false, why: "the stream was cut: AbortError" });
  expect(settle).not.toHaveBeenCalled();
  expect(await text).toContain('"settled":false');
});

test("a cumulative usage seen before a cut is not final: an Anthropic-shaped stream cut after message_delta is not settled", async () => {
  const anthropic = (type: string, fields: object) => encoder.encode(`event: ${type}\ndata: ${JSON.stringify({ type, ...fields })}\n\n`);
  const upstream = upstreamOf([anthropic("message_start", { message: { usage: { input_tokens: 3 } } }), anthropic("message_delta", { usage: { output_tokens: 4 } })]);
  const { readable, writable } = new TransformStream<Uint8Array, Uint8Array>();
  const settle = vi.fn(async () => SETTLED);
  const [trailer] = await Promise.all([pump({ upstream: upstream.stream, client: writable, cut: quietCut(), marginMs: 400, settle }), readAll(readable)]);
  expect(settle).not.toHaveBeenCalled();
  expect(trailer).toEqual({ settled: false, why: "the stream ended with no final usage" });
});

test("the final usage is read from events whose lines end in CRLF or CR, also when a CRLF is split between two chunks", async () => {
  const usage = JSON.stringify({ choices: [], usage: { prompt_tokens: 7, completion_tokens: 9 } });
  for (const parts of [[`data: ${usage}\r\n\r\ndata: [DONE]\r\n\r\n`], [`data: ${usage}\r\r`, "data: [DONE]\r\r"], [`data: ${usage}\r`, "\n\r", "\ndata: [DONE]\r\n\r\n"]]) {
    const upstream = upstreamOf(parts.map((part) => encoder.encode(part)));
    const { readable, writable } = new TransformStream<Uint8Array, Uint8Array>();
    const settled: Usage[] = [];
    await Promise.all([
      pump({
        upstream: upstream.stream,
        client: writable,
        cut: quietCut(),
        marginMs: 400,
        settle: async (used) => {
          settled.push(used);
          return SETTLED;
        },
      }),
      readAll(readable),
    ]);
    expect({ parts, settled }).toEqual({ parts, settled: [{ inputTokens: 7, outputTokens: 9 }] });
  }
});

test("a client that stops reading cannot hold the pump past the cut", async () => {
  const cut = cutAt(Date.now() + 50);
  const parts = Array.from({ length: 50 }, () => chunk(`{"choices":[{"delta":{"content":"${"x".repeat(1000)}"}}]}`));
  const upstream = new ReadableStream<Uint8Array>({
    start(stream) {
      for (const part of parts) stream.enqueue(part);
      cut.signal.addEventListener("abort", () => stream.error(new DOMException("cut", "AbortError")));
    },
  });
  const { writable } = new TransformStream<Uint8Array, Uint8Array>(undefined, undefined, { highWaterMark: 0 });
  const trailer = await pump({ upstream, client: writable, cut, marginMs: 400, settle: async () => SETTLED });
  cut.clear();
  expect(trailer).toEqual({ settled: false, why: "the stream was cut: AbortError" });
});

test("a settle that throws is logged as handler-error and reported in the ledger event", async () => {
  const upstream = upstreamOf([USAGE_CHUNK, chunk("[DONE]")]);
  const { readable, writable } = new TransformStream<Uint8Array, Uint8Array>();
  const [trailer] = await Promise.all([
    pump({
      upstream: upstream.stream,
      client: writable,
      cut: quietCut(),
      marginMs: 400,
      settle: async () => {
        throw new TypeError("object gone");
      },
    }),
    readAll(readable),
  ]);
  expect(trailer).toEqual({ settled: false, why: "the settle call failed" });
  expect(lines).toEqual([{ event: "handler-error", route: "none", status: 500, error: "TypeError" }]);
});

test("tighten never moves the cut later", async () => {
  const cut = cutAt(Date.now() + 30);
  cut.tighten(Date.now() + 60_000);
  await new Promise((resolve) => setTimeout(resolve, 120));
  expect(cut.signal.aborted).toBe(true);
  cut.clear();
});

async function settledFrom(parts: Uint8Array[], answer: EndResult = SETTLED): Promise<{ settled: Usage[]; trailer: unknown }> {
  const upstream = upstreamOf(parts);
  const { readable, writable } = new TransformStream<Uint8Array, Uint8Array>();
  const settled: Usage[] = [];
  const [trailer] = await Promise.all([
    pump({
      upstream: upstream.stream,
      client: writable,
      cut: quietCut(),
      marginMs: 400,
      settle: async (used) => {
        settled.push(used);
        return answer;
      },
      clock: () => 0,
    }),
    readAll(readable),
  ]);
  return { settled, trailer };
}

test("an Anthropic-shaped stream whose message_delta carries no input tokens settles with those of message_start", async () => {
  const anthropic = (type: string, fields: object) => encoder.encode(`event: ${type}\ndata: ${JSON.stringify({ type, ...fields })}\n\n`);
  const seen = await settledFrom([anthropic("message_start", { message: { usage: { input_tokens: 3 } } }), anthropic("message_delta", { usage: { output_tokens: 4 } }), anthropic("message_stop", {})]);
  expect(seen.settled).toEqual([{ inputTokens: 3, outputTokens: 4 }]);
});

test("a data chunk that does not parse after the usage chunk leaves no final usage", async () => {
  const seen = await settledFrom([USAGE_CHUNK, chunk("{not json"), chunk("[DONE]")]);
  expect(seen.settled).toEqual([]);
});

test("the ledger event reports the outcome and the cost of a settle that the ledger answered as expired", async () => {
  const seen = await settledFrom([USAGE_CHUNK, chunk("[DONE]")], { outcome: "expired", cost: 55, overrun: 0 });
  expect(seen.trailer).toEqual({ settled: true, outcome: "expired", cost: 55, settle_ms: 0 });
});

function endingWith(parts: Uint8Array[], end: "reset" | "open", signal?: AbortSignal): ReadableStream<Uint8Array> {
  let at = 0;
  return new ReadableStream<Uint8Array>({
    start(stream) {
      if (end === "open") signal?.addEventListener("abort", () => stream.error(new DOMException("cut", "AbortError")), { once: true });
    },
    pull(stream) {
      const next = parts[at];
      at += 1;
      if (next !== undefined) stream.enqueue(next);
      else if (end === "reset") stream.error(new TypeError("Network connection lost."));
    },
  });
}

async function settledAfter(upstream: ReadableStream<Uint8Array>, cut: Cut): Promise<{ settled: Usage[]; trailer: unknown }> {
  const { readable, writable } = new TransformStream<Uint8Array, Uint8Array>();
  const settled: Usage[] = [];
  const [trailer] = await Promise.all([
    pump({
      upstream,
      client: writable,
      cut,
      marginMs: 400,
      settle: async (used) => {
        settled.push(used);
        return SETTLED;
      },
      clock: () => 0,
    }),
    readAll(readable),
  ]);
  return { settled, trailer };
}

test("pump: the usage chunk and [DONE], then a reset of the body: settled at the reported usage (ruling B-1)", async () => {
  const seen = await settledAfter(endingWith([USAGE_CHUNK, chunk("[DONE]")], "reset"), quietCut());
  expect(seen.settled).toEqual([{ inputTokens: 7, outputTokens: 9 }]);
  expect(seen.trailer).toMatchObject({ settled: true, outcome: "settled" });
});

test("pump: the usage chunk and [DONE], then the body left open until the cut: settled at the reported usage (ruling B-1)", async () => {
  const cut = cutAt(Date.now() + 50);
  const seen = await settledAfter(endingWith([USAGE_CHUNK, chunk("[DONE]")], "open", cut.signal), cut);
  cut.clear();
  expect(seen.settled).toEqual([{ inputTokens: 7, outputTokens: 9 }]);
});

test("pump: a reset before [DONE] is not settled (ruling B-1)", async () => {
  const seen = await settledAfter(endingWith([USAGE_CHUNK], "reset"), quietCut());
  expect(seen.settled).toEqual([]);
  expect(seen.trailer).toEqual({ settled: false, why: "the stream was cut: TypeError" });
});

test("pump: the Anthropic shape cut after message_stop is settled (ruling B-1)", async () => {
  const anthropic = (type: string, fields: object) => encoder.encode(`event: ${type}\ndata: ${JSON.stringify({ type, ...fields })}\n\n`);
  const cut = cutAt(Date.now() + 50);
  const parts = [anthropic("message_start", { message: { usage: { input_tokens: 3 } } }), anthropic("message_delta", { usage: { output_tokens: 4 } }), anthropic("message_stop", {})];
  const seen = await settledAfter(endingWith(parts, "open", cut.signal), cut);
  cut.clear();
  expect(seen.settled).toEqual([{ inputTokens: 3, outputTokens: 4 }]);
});

function closesBeforeTheCut(parts: Uint8Array[], signal: AbortSignal): ReadableStream<Uint8Array> {
  let at = 0;
  return new ReadableStream<Uint8Array>({
    start(stream) {
      signal.addEventListener("abort", () => stream.error(new DOMException("cut", "AbortError")), { once: true });
    },
    pull(stream) {
      const next = parts[at];
      at += 1;
      if (next === undefined) stream.close();
      else stream.enqueue(next);
    },
  });
}

const numbered = (count: number, size = 16) => Array.from({ length: count }, (_, index) => chunk(JSON.stringify({ choices: [{ delta: { content: `${index}:${"x".repeat(size)}` } }] })));
const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

async function readSlowly(readable: ReadableStream<Uint8Array>, everyMs: number): Promise<string> {
  const reader = readable.getReader();
  const decoder = new TextDecoder();
  let text = "";
  for (;;) {
    await sleep(everyMs);
    const { done, value } = await reader.read();
    if (done) return text;
    text += decoder.decode(value, { stream: true });
  }
}

test("a client that takes one chunk every 300 ms does not hold the upstream read: a stream that ended with its final usage before the cut is settled at the reported usage (ruling B-3)", async () => {
  const cut = cutAt(Date.now() + 1000);
  const { readable, writable } = new TransformStream<Uint8Array, Uint8Array>();
  const settled: Usage[] = [];
  const reading = readSlowly(readable, 300);
  const trailer = await pump({ upstream: closesBeforeTheCut([...numbered(6), USAGE_CHUNK, chunk("[DONE]")], cut.signal), client: writable, cut, marginMs: 400, settle: async (used) => {
      settled.push(used);
      return SETTLED;
    } });
  cut.clear();
  expect(settled).toEqual([{ inputTokens: 7, outputTokens: 9 }]);
  expect(trailer).toMatchObject({ settled: true, outcome: "settled" });
  expect(await reading).toContain(`event: ledger\ndata: ${JSON.stringify(trailer)}\n\n`);
}, 10_000);

test("a client that never reads does not hold the upstream read: the stream is settled at the reported usage (ruling B-3)", async () => {
  const cut = cutAt(Date.now() + 300);
  const { writable } = new TransformStream<Uint8Array, Uint8Array>();
  const settled: Usage[] = [];
  const trailer = await pump({ upstream: closesBeforeTheCut([...numbered(6), USAGE_CHUNK, chunk("[DONE]")], cut.signal), client: writable, cut, marginMs: 400, settle: async (used) => {
      settled.push(used);
      return SETTLED;
    } });
  cut.clear();
  expect(settled).toEqual([{ inputTokens: 7, outputTokens: 9 }]);
  expect(trailer).toMatchObject({ settled: true, outcome: "settled" });
});

test("an answer larger than the bound to a client that never reads: the gateway holds no more than CLIENT_BACKLOG_MAX bytes for it and settles (ruling B-3)", async () => {
  const cut = cutAt(Date.now() + 300);
  let offered = 0;
  const client = new WritableStream<Uint8Array>(
    { write: () => new Promise<void>(() => undefined) },
    {
      highWaterMark: 1,
      size(part) {
        offered += part.byteLength;
        return 1;
      },
    },
  );
  const parts = numbered(1200, 1000);
  const answerBytes = parts.reduce((sum, part) => sum + part.byteLength, 0);
  const settled: Usage[] = [];
  const trailer = await pump({ upstream: closesBeforeTheCut([...parts, USAGE_CHUNK, chunk("[DONE]")], cut.signal), client, cut, marginMs: 400, settle: async (used) => {
      settled.push(used);
      return SETTLED;
    } });
  cut.clear();
  expect(settled).toEqual([{ inputTokens: 7, outputTokens: 9 }]);
  expect(trailer).toMatchObject({ settled: true, outcome: "settled" });
  expect(answerBytes).toBeGreaterThan(CLIENT_BACKLOG_MAX);
  expect(offered).toBeGreaterThan(0);
  expect(offered).toBeLessThanOrEqual(CLIENT_BACKLOG_MAX);
});

test("a client that reads at once gets every byte in order, with the trailer last", async () => {
  const parts = [...numbered(200, 100), USAGE_CHUNK, chunk("[DONE]")];
  const upstream = upstreamOf(parts);
  const { readable, writable } = new TransformStream<Uint8Array, Uint8Array>();
  const [trailer, text] = await Promise.all([pump({ upstream: upstream.stream, client: writable, cut: quietCut(), marginMs: 400, settle: async () => SETTLED, clock: () => 0 }), readAll(readable)]);
  const decoder = new TextDecoder();
  expect(text).toBe(`${parts.map((part) => decoder.decode(part)).join("")}event: ledger\ndata: ${JSON.stringify(trailer)}\n\n`);
});

const sizedChunk = (size: number) => {
  const overhead = chunk(JSON.stringify({ choices: [{ delta: { content: "" } }] })).byteLength;
  return chunk(JSON.stringify({ choices: [{ delta: { content: "y".repeat(size - overhead) } }] }));
};
const OVER_THE_BOUND = sizedChunk(CLIENT_BACKLOG_MAX + 1);

async function readUntilItFails(readable: ReadableStream<Uint8Array>): Promise<{ text: string; failed: boolean }> {
  const reader = readable.getReader();
  const decoder = new TextDecoder();
  let text = "";
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) return { text, failed: false };
      text += decoder.decode(value, { stream: true });
    }
  } catch {
    return { text, failed: true };
  }
}

test("B-12, the limit of the test entry: when every chunk of an answer above the bound stands in the upstream before the first read, a client that reads at once becomes one that left; its read fails with no ledger event, and the hold settles once at the reported usage", async () => {
  const parts = [...Array.from({ length: 48 }, () => sizedChunk(65_536)), USAGE_CHUNK, chunk("[DONE]")];
  const upstream = new ReadableStream<Uint8Array>({
    start(controller) {
      for (const part of parts) controller.enqueue(part);
      controller.close();
    },
  });
  const { readable, writable } = new TransformStream<Uint8Array, Uint8Array>();
  const cut = quietCut();
  const settled: Usage[] = [];
  const reading = readUntilItFails(readable);
  const trailer = await pump({ upstream, client: writable, cut, marginMs: 400, settle: async (used) => {
      settled.push(used);
      return SETTLED;
    }, clock: () => 1000 });
  const got = await reading;
  expect(got.failed).toBe(true);
  expect(got.text).not.toContain("event: ledger");
  expect(got.text.length).toBeLessThan(48 * 65_536);
  expect(cut.tightened).toEqual([1000 + AFTER_CLIENT_GONE_MS - 400]);
  expect(settled).toEqual([{ inputTokens: 7, outputTokens: 9 }]);
  expect(trailer).toMatchObject({ settled: true, outcome: "settled" });
});

test("B-3, the edge of the bound: two chunks of 524,288 bytes with no pause to a client that takes no byte until the upstream has ended are both queued; the client is not one that left and gets both chunks and the trailer", async () => {
  const tail = new Uint8Array([...USAGE_CHUNK, ...chunk("[DONE]")]);
  const first = sizedChunk(524_288);
  const pad = sizedChunk(524_288 - tail.byteLength);
  const second = new Uint8Array(524_288);
  second.set(pad, 0);
  second.set(tail, pad.byteLength);
  const upstream = new ReadableStream<Uint8Array>({
    start(controller) {
      controller.enqueue(first);
      controller.enqueue(second);
      controller.close();
    },
  });
  const { readable, writable } = new TransformStream<Uint8Array, Uint8Array>();
  const cut = quietCut();
  const settled: Usage[] = [];
  const trailer = await pump({ upstream, client: writable, cut, marginMs: 400, settle: async (used) => {
      settled.push(used);
      return SETTLED;
    }, clock: () => 0 });
  expect(first.byteLength + second.byteLength).toBe(CLIENT_BACKLOG_MAX);
  expect(cut.tightened).toEqual([]);
  expect(settled).toEqual([{ inputTokens: 7, outputTokens: 9 }]);
  const text = await readAll(readable);
  const decoder = new TextDecoder();
  expect(text === `${decoder.decode(first)}${decoder.decode(second)}event: ledger\ndata: ${JSON.stringify(trailer)}\n\n`).toBe(true);
});

test("B-3, writes no more: after the client was treated as one that left, no later chunk is written to it, also when its queue has room again", async () => {
  const written: number[] = [];
  const releases: (() => void)[] = [];
  const client = new WritableStream<Uint8Array>({
    write(part) {
      written.push(part.byteLength);
      return new Promise<void>((resolve) => {
        releases.push(resolve);
      });
    },
  });
  let openGate: () => void = () => undefined;
  const gate = new Promise<void>((resolve) => {
    openGate = resolve;
  });
  const first = sizedChunk(600_000);
  const parts = [first, sizedChunk(600_000), sizedChunk(1_000), USAGE_CHUNK, chunk("[DONE]")];
  let at = 0;
  const upstream = new ReadableStream<Uint8Array>({
    async pull(controller) {
      if (at === 2) await gate;
      const next = parts[at];
      at += 1;
      if (next === undefined) controller.close();
      else controller.enqueue(next);
    },
  });
  const cut = quietCut();
  const settled: Usage[] = [];
  const done = pump({ upstream, client, cut, marginMs: 400, settle: async (used) => {
      settled.push(used);
      return SETTLED;
    }, clock: () => 1000 });
  await sleep(30);
  expect(cut.tightened).toEqual([1000 + AFTER_CLIENT_GONE_MS - 400]);
  for (const release of releases.splice(0)) release();
  await sleep(30);
  openGate();
  const trailer = await done;
  expect(written).toEqual([first.byteLength]);
  expect(settled).toEqual([{ inputTokens: 7, outputTokens: 9 }]);
  expect(trailer).toMatchObject({ settled: true, outcome: "settled" });
});

test("one chunk of the bound plus one byte, with a pause of 5 ms between the upstream's chunks, to a client that reads at once is delivered whole, in order, with the trailer, and the hold settles", async () => {
  const parts = [OVER_THE_BOUND, USAGE_CHUNK, chunk("[DONE]")];
  const upstream = upstreamOf(parts, 5);
  const { readable, writable } = new TransformStream<Uint8Array, Uint8Array>();
  const settled: Usage[] = [];
  const [trailer, text] = await Promise.all([
    pump({ upstream: upstream.stream, client: writable, cut: quietCut(), marginMs: 400, settle: async (used) => {
        settled.push(used);
        return SETTLED;
      }, clock: () => 0 }),
    readAll(readable),
  ]);
  const decoder = new TextDecoder();
  expect(OVER_THE_BOUND.byteLength).toBe(CLIENT_BACKLOG_MAX + 1);
  expect(settled).toEqual([{ inputTokens: 7, outputTokens: 9 }]);
  expect(text === `${parts.map((part) => decoder.decode(part)).join("")}event: ledger\ndata: ${JSON.stringify(trailer)}\n\n`).toBe(true);
});

test("the same chunk to a client that never reads is queued, the next chunk makes the client one that left, and the hold settles at the reported usage", async () => {
  let offered = 0;
  const client = new WritableStream<Uint8Array>(
    { write: () => new Promise<void>(() => undefined) },
    {
      highWaterMark: 1,
      size(part) {
        offered += part.byteLength;
        return 1;
      },
    },
  );
  const cut = quietCut();
  const settled: Usage[] = [];
  const trailer = await pump({ upstream: upstreamOf([OVER_THE_BOUND, USAGE_CHUNK, chunk("[DONE]")]).stream, client, cut, marginMs: 400, settle: async (used) => {
      settled.push(used);
      return SETTLED;
    }, clock: () => 1000 });
  expect(offered).toBe(CLIENT_BACKLOG_MAX + 1);
  expect(cut.tightened).toEqual([1000 + AFTER_CLIENT_GONE_MS - 400]);
  expect(settled).toEqual([{ inputTokens: 7, outputTokens: 9 }]);
  expect(trailer).toMatchObject({ settled: true, outcome: "settled" });
});

test("cutAt aborts at its deadline and tighten only moves it earlier", async () => {
  let now = 0;
  const cut = cutAt(1_000_000, () => now);
  cut.tighten(2_000_000);
  now = 10;
  cut.tighten(30);
  await new Promise((resolve) => setTimeout(resolve, 60));
  expect(cut.signal.aborted).toBe(true);
  cut.clear();
});
