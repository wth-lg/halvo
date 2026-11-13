import { openSync, writeSync } from "node:fs";
import { createServer } from "node:http";
import { z } from "zod";

const logPath = process.argv[2];
const port = Number(process.argv[3] ?? "0");
if (!logPath || !Number.isSafeInteger(port) || port < 0 || port > 65535) {
  console.error("usage: node stress/upstream.mjs LOG [PORT]");
  process.exit(64);
}
const logFd = openSync(logPath, "ax");
const log = (id, ev, fields = {}) => writeSync(logFd, `${JSON.stringify({ t: Date.now(), id, ev, ...fields })}\n`);

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const waves = new Map();
function wave(id) {
  let entry = waves.get(id);
  if (!entry) {
    let release;
    const released = new Promise((resolve) => {
      release = resolve;
    });
    entry = { released, release };
    waves.set(id, entry);
  }
  return entry;
}

function readBody(req) {
  return new Promise((resolve, reject) => {
    const parts = [];
    req.on("data", (part) => parts.push(part));
    req.on("end", () => resolve(Buffer.concat(parts)));
    req.on("error", reject);
  });
}

function write(res, text) {
  return new Promise((resolve) => {
    if (res.destroyed || res.writableEnded) return resolve(false);
    res.write(text, (error) => resolve(!error));
  });
}

const openaiChunk = (delta, finish, usage) =>
  `data: ${JSON.stringify({
    id: "chatcmpl-mock",
    object: "chat.completion.chunk",
    model: "mock",
    choices: usage && !delta ? [] : [{ index: 0, delta: delta ?? {}, finish_reason: finish }],
    usage,
  })}\n\n`;
const anthropicEvent = (type, fields) => `event: ${type}\ndata: ${JSON.stringify({ type, ...fields })}\n\n`;

const MockInstructions = z.object({
  switch: z.enum(["usage-last", "usage-missing", "zero-placeholder", "slow", "over-report", "error-status", "reset", "anthropic-cut"]),
  shape: z.enum(["openai", "anthropic"]),
  chunks: z.number().int(),
  chunk_ms: z.number().int(),
  input_tokens: z.number().int(),
  output_tokens: z.number().int(),
  wave: z.string().optional(),
});

async function chat(req, res) {
  const id = String(req.headers["x-request-id"] ?? "");
  const mock = MockInstructions.parse(JSON.parse((await readBody(req)).toString("utf8")).mock);
  log(id, "request", { switch: mock.switch });

  if (mock.switch === "error-status") {
    res.writeHead(500, { "content-type": "application/json" });
    res.end(JSON.stringify({ error: { type: "server_error", message: "mock upstream error" } }));
    log(id, "end", { clean: false, why: "error-status" });
    return;
  }
  if (mock.switch === "reset") {
    req.socket.resetAndDestroy();
    log(id, "end", { clean: false, why: "reset" });
    return;
  }

  let ended = false;
  const end = (clean, why) => {
    if (ended) return;
    ended = true;
    log(id, "end", { clean, why });
  };
  res.on("close", () => end(false, "closed"));
  res.writeHead(200, { "content-type": "text/event-stream", "cache-control": "no-store" });

  const anthropic = mock.shape === "anthropic" || mock.switch === "anthropic-cut";
  const usageCost = mock.switch === "zero-placeholder" ? { cost: 0 } : {};
  const step = async (text, first) => {
    if (!(await write(res, text))) return false;
    if (first && mock.wave) await wave(mock.wave).released;
    await sleep(mock.chunk_ms);
    return !ended;
  };

  if (anthropic) {
    const start = { input_tokens: mock.input_tokens, output_tokens: 1 };
    const message = { id: "msg_mock", type: "message", role: "assistant", content: [], model: "mock", usage: start };
    if (!(await step(anthropicEvent("message_start", { message }), true))) return;
    log(id, "usage", { final: false, ...start });
    const block = { index: 0, content_block: { type: "text", text: "" } };
    if (!(await step(anthropicEvent("content_block_start", block)))) return;
    for (let i = 0; i < mock.chunks; i++) {
      const delta = { index: 0, delta: { type: "text_delta", text: "tok " } };
      if (!(await step(anthropicEvent("content_block_delta", delta)))) return;
    }
    if (!(await step(anthropicEvent("content_block_stop", { index: 0 })))) return;
    if (mock.switch === "anthropic-cut") {
      const cumulative = { output_tokens: Math.max(1, Math.floor(mock.output_tokens / 2)) };
      const delta = anthropicEvent("message_delta", { delta: { stop_reason: null }, usage: cumulative });
      if (!(await write(res, delta))) return;
      log(id, "usage", { final: false, input_tokens: mock.input_tokens, ...cumulative });
      await sleep(mock.chunk_ms);
      end(false, "cut");
      res.destroy();
      return;
    }
    const usage = { output_tokens: mock.output_tokens };
    const last =
      anthropicEvent("message_delta", { delta: { stop_reason: "end_turn" }, usage }) +
      anthropicEvent("message_stop", {});
    if (!(await write(res, last))) return;
    log(id, "usage", { final: true, input_tokens: mock.input_tokens, ...usage });
    res.end(() => end(true, "done"));
    return;
  }

  for (let i = 0; i < mock.chunks; i++) {
    const placeholder = i === 0 && mock.switch === "zero-placeholder";
    const usage = placeholder ? { prompt_tokens: 0, completion_tokens: 0, total_tokens: 0, cost: 0 } : null;
    if (!(await step(openaiChunk({ content: "tok " }, null, usage), i === 0))) return;
    if (placeholder) log(id, "usage", { final: false, input_tokens: 0, output_tokens: 0 });
  }
  if (!(await step(openaiChunk({}, "stop", null)))) return;
  if (mock.switch === "usage-missing") {
    if (!(await write(res, "data: [DONE]\n\n"))) return;
    res.end(() => end(true, "done-without-usage"));
    return;
  }
  const usage = {
    prompt_tokens: mock.input_tokens,
    completion_tokens: mock.output_tokens,
    total_tokens: mock.input_tokens + mock.output_tokens,
    ...usageCost,
  };
  if (!(await write(res, `${openaiChunk(null, null, usage)}data: [DONE]\n\n`))) return;
  log(id, "usage", { final: true, input_tokens: mock.input_tokens, output_tokens: mock.output_tokens });
  res.end(() => end(true, "done"));
}

const server = createServer((req, res) => {
  const url = new URL(req.url ?? "/", "http://127.0.0.1");
  if (req.method === "POST" && url.pathname === "/v1/chat") {
    chat(req, res).catch((error) => {
      log(String(req.headers["x-request-id"] ?? ""), "end", { clean: false, why: `mock failed: ${error.message}` });
      res.destroy();
    });
    return;
  }
  if (req.method === "POST" && url.pathname === "/release") {
    wave(url.searchParams.get("wave") ?? "").release();
    res.end("released\n");
    return;
  }
  res.writeHead(404).end();
});
server.listen(port, "127.0.0.1", () => {
  console.log(JSON.stringify({ port: server.address().port }));
});
