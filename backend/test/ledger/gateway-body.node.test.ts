import { expect, test } from "vitest";
import { CHOICES_MAX, OUTPUT_LIMIT_MAX, readChatBody } from "../../src/gateway/body.ts";

const MOCK = { switch: "usage-last", shape: "openai", chunks: 3, chunk_ms: 10, input_tokens: 4, output_tokens: 5 };
const base = { tier: "low_cost_hosted", max_tokens: 640, stream: true, messages: [{ role: "user", content: "hello" }], mock: MOCK };

test("the stress driver's body is read: the ceiling tier, the output limit, one choice, and a forwarded body without the tier", () => {
  expect(readChatBody(JSON.stringify(base))).toEqual({
    tier: "low_cost_hosted",
    outputLimit: 640,
    choices: 1,
    forward: JSON.stringify({ stream: true, messages: [{ role: "user", content: "hello" }], mock: MOCK, model: "low_cost_hosted", max_tokens: 640 }),
  });
});

test("the output limit is clamped, and the forwarded body asks for no more than the hold covers", () => {
  const read = readChatBody(JSON.stringify({ ...base, max_tokens: OUTPUT_LIMIT_MAX + 1, n: 2 }));
  expect(read).toMatchObject({ outputLimit: OUTPUT_LIMIT_MAX, choices: 2 });
  expect(JSON.parse(read?.forward ?? "{}")).toMatchObject({ max_tokens: OUTPUT_LIMIT_MAX, n: 2 });
  const without = readChatBody(JSON.stringify({ tier: "frontier", messages: [{ role: "user", content: [{ type: "text", text: "hi" }] }] }));
  expect(without).toMatchObject({ tier: "frontier", outputLimit: OUTPUT_LIMIT_MAX, choices: 1 });
});

test("text parts only: content named by URL, file id or stored conversation, any tool, and any other key are refused before reserve", () => {
  const refused = [
    "{",
    "[]",
    JSON.stringify({ ...base, tier: undefined }),
    JSON.stringify({ ...base, tier: "on_device" }),
    JSON.stringify({ ...base, model_tier: "frontier" }),
    JSON.stringify({ ...base, model: "a-frontier-model" }),
    JSON.stringify({ ...base, messages: [] }),
    JSON.stringify({ ...base, messages: [{ role: "user", content: [{ type: "image_url", image_url: { url: "http://127.0.0.1:1/x.png" } }] }] }),
    JSON.stringify({ ...base, messages: [{ role: "user", content: [{ type: "input_file", file_id: "file_1" }] }] }),
    JSON.stringify({ ...base, messages: [{ role: "user", content: [{ type: "text", text: "a", file_id: "file_1" }] }] }),
    JSON.stringify({ ...base, messages: [{ role: "tool", content: "a" }] }),
    JSON.stringify({ ...base, previous_response_id: "resp_1" }),
    JSON.stringify({ ...base, conversation: "conv_1" }),
    JSON.stringify({ ...base, tools: [{ type: "web_search" }] }),
    JSON.stringify({ ...base, tools: [{ type: "function", function: { name: "f" } }] }),
    JSON.stringify({ ...base, max_tokens: 0 }),
    JSON.stringify({ ...base, max_tokens: 1.5 }),
    JSON.stringify({ ...base, n: CHOICES_MAX + 1 }),
    JSON.stringify({ ...base, mock: { ...MOCK, url: "http://127.0.0.1:1/" } }),
  ];
  expect(refused.map((text) => readChatBody(text))).toEqual(refused.map(() => null));
});
