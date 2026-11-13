import { describe, expect, test } from "vitest";
import type { Usage } from "../../src/ledger/index.ts";
import { FinalUsage } from "../../src/gateway/usage.ts";

const encoder = new TextEncoder();
const line = (data: string) => `data: ${data}\n\n`;
const typed = (type: string, fields: object = {}) => `event: ${type}\ndata: ${JSON.stringify({ type, ...fields })}\n\n`;
const usageChunk = (input: number, output: number) => line(JSON.stringify({ choices: [], usage: { prompt_tokens: input, completion_tokens: output } }));
const TEXT = line(JSON.stringify({ choices: [{ delta: { content: "a" } }] }));
const DONE = line("[DONE]");
const START = (input: number) => typed("message_start", { message: { usage: { input_tokens: input } } });
const DELTA = (output: number) => typed("message_delta", { usage: { output_tokens: output } });
const STOP = typed("message_stop");
const ERROR = typed("error", { error: { type: "overloaded_error" } });
const PING = typed("ping");
const used = (inputTokens: number, outputTokens: number): Usage => ({ inputTokens, outputTokens });

function finalOf(events: string[]): Usage | undefined {
  const usage = new FinalUsage();
  for (const event of events) usage.feed(encoder.encode(event));
  usage.end();
  return usage.result();
}

describe("what each kind of data line does to the usage (ruling B-2)", () => {
  test("a chunk without a type sets the usage to its own usage object", () => {
    expect(finalOf([usageChunk(30, 90), DONE])).toEqual(used(30, 90));
  });

  test("a chunk without a type and without a usage object sets the usage to none", () => {
    expect(finalOf([usageChunk(30, 90), TEXT, DONE])).toBeUndefined();
  });

  test("[DONE] makes the usage in force final, or the stream has none", () => {
    expect(finalOf([usageChunk(30, 90), DONE])).toEqual(used(30, 90));
    expect(finalOf([DONE])).toBeUndefined();
    expect(finalOf([usageChunk(30, 90)])).toBeUndefined();
  });

  test("message_start gives the input tokens, and a message_delta without input tokens takes them", () => {
    expect(finalOf([START(3), DELTA(4), STOP])).toEqual(used(3, 4));
  });

  test("each message_delta replaces the cumulative usage", () => {
    expect(finalOf([START(3), DELTA(4), DELTA(6), STOP])).toEqual(used(3, 6));
  });

  test("a message_delta replaces only the counts that it carries; a count that it does not carry stays as message_start or an earlier delta gave it", () => {
    const delta = (usage: object) => typed("message_delta", { usage });
    expect(finalOf([START(3), delta({ input_tokens: 5, output_tokens: 4 }), delta({ output_tokens: 6 }), STOP])).toEqual(used(5, 6));
    expect(finalOf([START(3), delta({ output_tokens: 4 }), delta({ input_tokens: 8 }), STOP])).toEqual(used(8, 4));
    expect(finalOf([START(3), delta({ output_tokens: 4 }), typed("message_delta", { delta: { stop_reason: "end_turn" } }), STOP])).toEqual(used(3, 4));
  });

  test("message_stop terminates: a message_delta after it changes nothing", () => {
    expect(finalOf([START(3), DELTA(4), STOP, DELTA(9)])).toEqual(used(3, 4));
  });

  test("a data line that does not parse as JSON ends the claim to a final usage", () => {
    expect(finalOf([usageChunk(30, 90), line("{not json"), DONE])).toBeUndefined();
  });

  test("an event whose type is error ends the claim to a final usage", () => {
    expect(finalOf([usageChunk(30, 90), ERROR, DONE])).toBeUndefined();
    expect(finalOf([START(3), DELTA(4), ERROR, STOP])).toBeUndefined();
  });

  test("a second message_start ends the claim to a final usage", () => {
    expect(finalOf([START(30), START(7), DELTA(90), STOP])).toBeUndefined();
  });

  test("a value that is no object changes nothing", () => {
    for (const value of ["5", "null", '"a string"', "[1]", "true"]) {
      expect({ value, final: finalOf([usageChunk(30, 90), line(value), DONE]) }).toEqual({ value, final: used(30, 90) });
    }
  });

  test("an object of another type changes nothing, in either shape", () => {
    expect(finalOf([usageChunk(30, 90), PING, DONE])).toEqual(used(30, 90));
    expect(finalOf([START(3), DELTA(4), PING, typed("content_block_delta", { delta: { text: "a" } }), STOP])).toEqual(used(3, 4));
  });

  test("a data line with nothing after the colon does not parse as JSON and ends the claim to a final usage", () => {
    expect(finalOf([usageChunk(30, 90), "data:\n\n", DONE])).toBeUndefined();
  });

  test("an event with no data line, a comment or an event name alone, changes nothing", () => {
    expect(finalOf([usageChunk(30, 90), ": keep-alive\n\n", "event: ping\n\n", DONE])).toEqual(used(30, 90));
  });
});

describe("rules that no test held (ruling B-13)", () => {
  test("B-1, read whole: a [DONE] after the usage chunk with no line end, or with one line end, is not read whole: no final usage", () => {
    expect(finalOf([usageChunk(30, 90), "data: [DONE]"])).toBeUndefined();
    expect(finalOf([usageChunk(30, 90), "data: [DONE]\n"])).toBeUndefined();
  });

  test("one shape's usage is not in force at the other shape's terminating event: message_start, message_delta, then [DONE] and no message_stop: no final usage", () => {
    expect(finalOf([START(3), DELTA(4), DONE])).toBeUndefined();
  });
});

describe("a message_delta whose count is no count clears the count in force (ruling B-14)", () => {
  const delta = (usage: object) => typed("message_delta", { usage });
  for (const [name, value] of [
    ["-1", -1],
    ["null", null],
  ] as const) {
    test(`a count of ${name} with no later delta: no final usage`, () => {
      expect(finalOf([START(3), delta({ output_tokens: 4 }), delta({ output_tokens: value }), STOP])).toBeUndefined();
    });

    test(`a count of ${name}, then a delta that carries a count before message_stop: the final usage is at that count`, () => {
      expect(finalOf([START(3), delta({ output_tokens: 4 }), delta({ output_tokens: value }), delta({ output_tokens: 6 }), STOP])).toEqual(used(3, 6));
    });
  }
});

describe("sequences (ruling B-2; the verifier's four and the review's lines)", () => {
  test("usage, [DONE], a text chunk, a second [DONE]: final at the first usage", () => {
    expect(finalOf([usageChunk(30, 90), DONE, TEXT, DONE])).toEqual(used(30, 90));
  });

  test("usage 30/90, [DONE], usage 5/5, [DONE]: final at 30/90", () => {
    expect(finalOf([usageChunk(30, 90), DONE, usageChunk(5, 5), DONE])).toEqual(used(30, 90));
  });

  test("a second message_start before the delta: no final usage", () => {
    expect(finalOf([START(30), START(7), DELTA(90), STOP])).toBeUndefined();
  });

  test("message_start, message_delta, message_stop, then [DONE]: final at the Anthropic usage", () => {
    expect(finalOf([START(30), DELTA(90), STOP, DONE])).toEqual(used(30, 90));
  });

  test("a usage chunk without a type, then message_stop: no final usage", () => {
    expect(finalOf([usageChunk(30, 90), STOP])).toBeUndefined();
  });

  test("after a usage chunk, each of data: 5, data: null, a string and a ping, then [DONE]: final at the usage", () => {
    for (const between of [line("5"), line("null"), line('"a string"'), PING]) {
      expect({ between, final: finalOf([usageChunk(30, 90), between, DONE]) }).toEqual({ between, final: used(30, 90) });
    }
  });

  test("an error event before the terminating event: no final usage", () => {
    expect(finalOf([usageChunk(30, 90), ERROR, DONE])).toBeUndefined();
  });

  test("a line that does not parse: no final usage, also when a usage chunk follows it", () => {
    expect(finalOf([line("{not json"), DONE])).toBeUndefined();
    expect(finalOf([usageChunk(30, 90), line("{not json"), usageChunk(30, 90), DONE])).toBeUndefined();
  });
});
