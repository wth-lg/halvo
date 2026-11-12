import { readFileSync } from "node:fs";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { costOf } from "../../src/ledger/cost.ts";
import { type ChatRig, chatRig, chunk, upstreamOf } from "./chat-rig.ts";
import { LOW } from "./harness.ts";

const T = 1_000_000;
const LIFE_MS = 1000;
const encoder = new TextEncoder();
const USAGE = { inputTokens: 7, outputTokens: 9 };
const USAGE_CHUNK = chunk(JSON.stringify({ choices: [], usage: { prompt_tokens: USAGE.inputTokens, completion_tokens: USAGE.outputTokens } }));
const TEXT_CHUNK = chunk(JSON.stringify({ choices: [{ delta: { content: "a" } }] }));
const BODY = { tier: "low_cost_hosted", max_tokens: 100, messages: [{ role: "user", content: "Synthetic gateway test." }] };
const anthropic = (type: string, fields: object) => encoder.encode(`event: ${type}\ndata: ${JSON.stringify({ type, ...fields })}\n\n`);

let open: ChatRig[] = [];
beforeEach(() => {
  open = [];
  vi.spyOn(console, "log").mockImplementation(() => undefined);
});
afterEach(() => {
  for (const each of open) each.close();
  vi.restoreAllMocks();
});

async function rigFor(o: Parameters<typeof chatRig>[0]): Promise<ChatRig> {
  const made = await chatRig(o);
  open.push(made);
  return made;
}

describe("a usage report is final at the first terminating event read whole (ruling B-1), through chat() on a real ledger", () => {
  test("the usage chunk and [DONE], then a reset of the body: settled at the reported usage", async () => {
    const gateway = await rigFor({ at: T, holdLifeMs: LIFE_MS });
    gateway.setUpstream(upstreamOf([TEXT_CHUNK, USAGE_CHUNK, chunk("[DONE]")], "reset"));
    const sent = await gateway.send("reset-after-done", BODY);
    expect(sent.status).toBe(200);
    expect(gateway.hold("reset-after-done")).toMatchObject({ state: "settled", cost: costOf(LOW, USAGE), ended_by: "settle" });
  });

  test("the usage chunk and [DONE], then the body left open until the cut: settled at the reported usage", async () => {
    const gateway = await rigFor({ at: T, holdLifeMs: LIFE_MS });
    gateway.setUpstream(upstreamOf([TEXT_CHUNK, USAGE_CHUNK, chunk("[DONE]")], "open"));
    const sent = await gateway.send("open-after-done", BODY);
    expect(sent.status).toBe(200);
    expect(gateway.hold("open-after-done")).toMatchObject({ state: "settled", cost: costOf(LOW, USAGE), ended_by: "settle" });
  });

  test("a reset before [DONE]: not settled, and the hold expires at its amount", async () => {
    const gateway = await rigFor({ at: T, holdLifeMs: LIFE_MS });
    gateway.setUpstream(upstreamOf([TEXT_CHUNK, USAGE_CHUNK], "reset"));
    const sent = await gateway.send("reset-before-done", BODY);
    expect(sent.status).toBe(200);
    expect(sent.text).toContain('"settled":false');
    expect(gateway.hold("reset-before-done")).toMatchObject({ state: "open", cost: null });
    await gateway.rig.run({ at: T + LIFE_MS, call: { kind: "alarm" } });
    const row = gateway.hold("reset-before-done");
    expect(row).toMatchObject({ state: "expired", cost: null });
    expect(gateway.rig.read().periods).toEqual([expect.objectContaining({ reported: 0, unreported: row?.amount, open: 0 })]);
  });

  test("the Anthropic shape cut after message_stop: settled", async () => {
    const gateway = await rigFor({ at: T, holdLifeMs: LIFE_MS });
    gateway.setUpstream(
      upstreamOf([anthropic("message_start", { message: { usage: { input_tokens: 3 } } }), anthropic("message_delta", { usage: { output_tokens: 4 } }), anthropic("message_stop", {})], "open"),
    );
    const sent = await gateway.send("anthropic-cut-after-stop", BODY);
    expect(sent.status).toBe(200);
    expect(gateway.hold("anthropic-cut-after-stop")).toMatchObject({ state: "settled", cost: costOf(LOW, { inputTokens: 3, outputTokens: 4 }) });
  });
});

describe("the void branch (ruling B-4)", () => {
  test("with less time left than the deadline plus the margin, the gateway calls void, answers 503 voided and makes no upstream call", async () => {
    const gateway = await rigFor({ at: T, holdLifeMs: 1000, deadlineMs: 1500, marginMs: 400 });
    gateway.setUpstream(upstreamOf([USAGE_CHUNK, chunk("[DONE]")], "close"));
    const sent = await gateway.send("short-life", BODY);
    expect([sent.status, JSON.parse(sent.text)]).toEqual([503, { result: "voided" }]);
    expect(gateway.calls).toEqual(["reserve", "void"]);
    expect(gateway.upstreamCalls()).toBe(0);
    expect(gateway.hold("short-life")).toMatchObject({ state: "voided", dispatched: 0, cost: null });
    expect(gateway.rig.read().periods).toEqual([expect.objectContaining({ reported: 0, unreported: 0, open: 0 })]);
  });

  test("where the void is refused, the gateway answers 502 no-time-left and makes no upstream call", async () => {
    const gateway = await rigFor({ at: T, holdLifeMs: 1000, deadlineMs: 1500, marginMs: 400 });
    gateway.setUpstream(upstreamOf([USAGE_CHUNK, chunk("[DONE]")], "close"));
    gateway.before.void = () => gateway.rig.setNow(T + 1000);
    const sent = await gateway.send("void-refused", BODY);
    expect([sent.status, JSON.parse(sent.text)]).toEqual([502, { result: "no-time-left", hold: "refused" }]);
    expect(gateway.calls).toEqual(["reserve", "void"]);
    expect(gateway.upstreamCalls()).toBe(0);
    expect(gateway.hold("void-refused")).toMatchObject({ state: "expired", dispatched: 0 });
  });
});

describe("the hold's amount is what the gateway computes from the body it received (ruling B-5)", () => {
  const prices = JSON.parse(readFileSync("test-kit/prices/v1.json", "utf8")) as Record<"low_cost_hosted" | "frontier", { input: number; output: number }>;
  const expectedHold = (tier: "low_cost_hosted" | "frontier", body: string, outputTokens: number) => {
    const total = BigInt(encoder.encode(body).byteLength) * BigInt(prices[tier].input) + BigInt(outputTokens) * BigInt(prices[tier].output);
    return Number((total + 999_999n) / 1_000_000n);
  };
  const MULTI_BYTE = "é中😀".repeat(40);
  const cases: { name: string; tier: "low_cost_hosted" | "frontier"; fields: object; outputTokens: number }[] = [
    { name: "text of two, three and four bytes per character", tier: "low_cost_hosted", fields: { max_tokens: 100, messages: [{ role: "user", content: MULTI_BYTE }] }, outputTokens: 100 },
    { name: "n of 1", tier: "low_cost_hosted", fields: { max_tokens: 640, n: 1 }, outputTokens: 640 },
    { name: "n of 2", tier: "low_cost_hosted", fields: { max_tokens: 640, n: 2 }, outputTokens: 1280 },
    { name: "n of 8", tier: "frontier", fields: { max_tokens: 640, n: 8 }, outputTokens: 5120 },
    { name: "max_tokens absent", tier: "low_cost_hosted", fields: {}, outputTokens: 8192 },
    { name: "max_tokens at the limit, with n of 8 and text of several bytes per character", tier: "frontier", fields: { max_tokens: 8192, n: 8, messages: [{ role: "user", content: MULTI_BYTE }] }, outputTokens: 65_536 },
  ];
  for (const each of cases) {
    test(`${each.name}: the receipt and the hold carry the amount that the price file gives`, async () => {
      const gateway = await rigFor({ at: T });
      gateway.setUpstream(upstreamOf([USAGE_CHUNK, chunk("[DONE]")], "close"));
      const body = JSON.stringify({ tier: each.tier, messages: [{ role: "user", content: "Synthetic hold test." }], ...each.fields });
      const amount = expectedHold(each.tier, body, each.outputTokens);
      const sent = await gateway.send(`hold-${cases.indexOf(each)}`, body);
      expect(sent.status).toBe(200);
      expect(sent.receipt?.amount).toBe(amount);
      expect(gateway.hold(`hold-${cases.indexOf(each)}`)).toMatchObject({ amount, input_price: prices[each.tier].input, output_price: prices[each.tier].output });
    });
  }
});

describe("a dispatch that the ledger answers with stop (ruling B-10)", () => {
  test("a hold that expires between the reserve and the dispatch gives 409 stopped and no upstream call", async () => {
    const gateway = await rigFor({ at: T });
    gateway.setUpstream(upstreamOf([USAGE_CHUNK, chunk("[DONE]")], "close"));
    gateway.before.dispatch = () => gateway.rig.setNow(T + 3000);
    const sent = await gateway.send("expired-before-dispatch", BODY);
    expect([sent.status, JSON.parse(sent.text)]).toEqual([409, { result: "stopped" }]);
    expect(gateway.calls).toEqual(["reserve", "dispatch"]);
    expect(gateway.upstreamCalls()).toBe(0);
    expect(gateway.hold("expired-before-dispatch")).toMatchObject({ state: "expired", dispatched: 0 });
  });

  test("two dispatches of one request at once give one 200 and one 409 stopped", async () => {
    const gateway = await rigFor({ at: T });
    gateway.setUpstream(upstreamOf([USAGE_CHUNK, chunk("[DONE]")], "close"));
    let release: () => void = () => undefined;
    const bothReserved = new Promise<void>((resolve) => {
      release = resolve;
    });
    gateway.before.dispatch = async () => {
      if (gateway.calls.filter((call) => call === "reserve").length === 2) release();
      await bothReserved;
    };
    const answers = await Promise.all([gateway.send("sent-twice", BODY), gateway.send("sent-twice", BODY)]);
    expect(answers.map((answer) => answer.status).sort()).toEqual([200, 409]);
    expect(JSON.parse(answers.find((answer) => answer.status === 409)?.text ?? "null")).toEqual({ result: "stopped" });
    expect(gateway.upstreamCalls()).toBe(1);
    expect(gateway.hold("sent-twice")).toMatchObject({ state: "settled", dispatched: 1 });
  });
});

describe("an upstream call that throws (ruling B-16)", () => {
  test("is answered 502 upstream-failed with the error's name as why, and the planted marker is in no log line and not in the answer", async () => {
    const marker = "answer-marker-3c71e0";
    const lines: string[] = [];
    for (const method of ["log", "error", "warn", "info", "debug"] as const) {
      vi.spyOn(console, method).mockImplementation((...args: unknown[]) => {
        lines.push(args.map((arg) => (typeof arg === "string" ? arg : JSON.stringify(arg))).join(" "));
      });
    }
    const gateway = await rigFor({ at: T });
    gateway.setUpstream(async () => {
      throw new TypeError(`fetch failed ${marker}`);
    });
    const sent = await gateway.send("upstream-throws", { ...BODY, messages: [{ role: "user", content: `Synthetic ${marker}` }] });
    expect([sent.status, JSON.parse(sent.text)]).toEqual([502, { result: "upstream-failed", why: "TypeError" }]);
    expect(sent.text).not.toContain(marker);
    expect(lines.filter((text) => text.includes(marker))).toEqual([]);
    expect(lines).toEqual([]);
  });
});

describe("a terminating event that is not read whole, through chat() (ruling B-13)", () => {
  test("the usage chunk, then data: [DONE] with no line end or with one line end, then a reset: no settle, and the hold expires at its amount", async () => {
    for (const [index, tail] of ["data: [DONE]", "data: [DONE]\n"].entries()) {
      const gateway = await rigFor({ at: T, holdLifeMs: LIFE_MS });
      gateway.setUpstream(upstreamOf([TEXT_CHUNK, USAGE_CHUNK, encoder.encode(tail)], "reset"));
      const id = `done-not-whole-${index}`;
      const sent = await gateway.send(id, BODY);
      expect(sent.status).toBe(200);
      expect(gateway.calls).toEqual(["reserve", "dispatch"]);
      await gateway.rig.run({ at: T + LIFE_MS, call: { kind: "alarm" } });
      const row = gateway.hold(id);
      expect(row).toMatchObject({ state: "expired", cost: null });
      expect(gateway.rig.read().periods).toEqual([expect.objectContaining({ reported: 0, unreported: row?.amount, open: 0 })]);
    }
  });
});

describe("an answer writes no line (contract 10)", () => {
  const REQUEST_MARKER = "request-marker-5f1c9e";
  const ANSWER_MARKER = "answer-marker-8d24b7";
  const marked = { tier: "low_cost_hosted", max_tokens: 100, messages: [{ role: "user", content: `Synthetic log test ${REQUEST_MARKER}.` }] };
  const answerParts = [chunk(JSON.stringify({ choices: [{ delta: { content: ANSWER_MARKER } }] })), USAGE_CHUNK, chunk("[DONE]")];

  function captureLines(): string[] {
    const lines: string[] = [];
    for (const method of ["log", "error", "warn", "info", "debug"] as const) {
      vi.spyOn(console, method).mockImplementation((...args: unknown[]) => {
        lines.push(args.map((arg) => (typeof arg === "string" ? arg : JSON.stringify(arg))).join(" "));
      });
    }
    return lines;
  }

  const cases: { name: string; status: number; run(): Promise<{ status: number; text: string }> }[] = [
    {
      name: "a 200 with a stream that is settled",
      status: 200,
      run: async () => {
        const gateway = await rigFor({ at: T });
        gateway.setUpstream(upstreamOf(answerParts, "close"));
        return gateway.send("log-200", marked);
      },
    },
    {
      name: "a 503 voided",
      status: 503,
      run: async () => {
        const gateway = await rigFor({ at: T, holdLifeMs: 1000, deadlineMs: 1500, marginMs: 400 });
        gateway.setUpstream(upstreamOf(answerParts, "close"));
        return gateway.send("log-503", marked);
      },
    },
    {
      name: "a 502 no-time-left",
      status: 502,
      run: async () => {
        const gateway = await rigFor({ at: T, holdLifeMs: 1000, deadlineMs: 1500, marginMs: 400 });
        gateway.setUpstream(upstreamOf(answerParts, "close"));
        gateway.before.void = () => gateway.rig.setNow(T + 1000);
        return gateway.send("log-502-no-time", marked);
      },
    },
    {
      name: "a 502 upstream-failed",
      status: 502,
      run: async () => {
        const gateway = await rigFor({ at: T });
        gateway.setUpstream(async () => {
          throw new TypeError(`fetch failed ${ANSWER_MARKER}`);
        });
        return gateway.send("log-502-upstream", marked);
      },
    },
    {
      name: "a 409 repeat of a settled request",
      status: 409,
      run: async () => {
        const gateway = await rigFor({ at: T });
        gateway.setUpstream(upstreamOf(answerParts, "close"));
        await gateway.send("log-409", marked);
        return gateway.send("log-409", marked);
      },
    },
    {
      name: "a 409 stopped",
      status: 409,
      run: async () => {
        const gateway = await rigFor({ at: T });
        gateway.setUpstream(upstreamOf(answerParts, "close"));
        gateway.before.dispatch = () => gateway.rig.setNow(T + 3000);
        return gateway.send("log-409-stopped", marked);
      },
    },
    {
      name: "a 403 refused over-cap",
      status: 403,
      run: async () => {
        const gateway = await rigFor({ at: T, cap: { amount: 1, period: "week" } });
        gateway.setUpstream(upstreamOf(answerParts, "close"));
        return gateway.send("log-403", { ...marked, max_tokens: 8192 });
      },
    },
  ];

  for (const each of cases) {
    test(`${each.name} writes no log line and no line holds a byte of the messages or of the answer`, async () => {
      const lines = captureLines();
      const sent = await each.run();
      expect(sent.status).toBe(each.status);
      expect(lines.filter((text) => text.includes(REQUEST_MARKER) || text.includes(ANSWER_MARKER))).toEqual([]);
      expect(lines).toEqual([]);
    });
  }
});
