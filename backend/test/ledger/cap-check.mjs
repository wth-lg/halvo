import { execFileSync } from "node:child_process";
import { randomBytes, randomUUID } from "node:crypto";
import { readFileSync } from "node:fs";
import http from "node:http";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = process.env.HALVO_ROOT ?? join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const WEEK_MS = 604_800_000;
const START = 1_893_456_000_000;
const MICRO_PER_MINOR = 10_000;
const TIER = "low_cost_hosted";
const FIRST_CAP = { amount: 100, period: "week" };
const LOWER_CAP = { amount: 1, period: "week" };

const tokens = {
  "read-back": "unmeasured",
  "hold-open": "unmeasured",
  "set-below": "unmeasured",
  settled: "unmeasured",
  "refused-before-roll": "unmeasured",
  roll: "unmeasured",
  "admitted-after-roll": "unmeasured",
};
const line = () =>
  Object.entries(tokens)
    .map(([name, value]) => `${name}=${value}`)
    .join(" ");

class Stop extends Error {}
const stop = (sentence) => {
  throw new Stop(sentence);
};

const prices = JSON.parse(readFileSync(join(ROOT, "test-kit/prices/v1.json"), "utf8"));
const price = prices[TIER];
const costOf = (input, output) =>
  Number((BigInt(input) * BigInt(price.input) + BigInt(output) * BigInt(price.output) + 999_999n) / 1_000_000n);

function memberPort(name) {
  let url;
  try {
    url = execFileSync("sh", [join(ROOT, "scripts/stack.sh"), "url", name], { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
  } catch {
    stop(`the stack has no member ${name}`);
  }
  const match = /^http:\/\/127\.0\.0\.1:([0-9]+)$/.exec(url);
  if (!match) stop(`sh scripts/stack.sh url ${name} printed ${url}`);
  return Number(match[1]);
}

function request(port, method, path, { body, headers = {} } = {}) {
  return new Promise((resolve, reject) => {
    const sent = body === undefined ? headers : { ...headers, "content-type": "application/json", "content-length": Buffer.byteLength(body) };
    const outgoing = http.request({ host: "127.0.0.1", port, method, path, agent: false, headers: sent }, (response) => {
      const stream = { status: response.statusCode, text: "", first: null, ended: null };
      let gotFirst;
      stream.first = new Promise((resolveFirst) => {
        gotFirst = resolveFirst;
      });
      stream.ended = new Promise((resolveEnd, rejectEnd) => {
        response.setEncoding("utf8");
        response.on("data", (part) => {
          stream.text += part;
          gotFirst();
        });
        response.on("end", () => {
          gotFirst();
          resolveEnd(stream.text);
        });
        response.on("error", rejectEnd);
      });
      resolve(stream);
    });
    outgoing.on("error", reject);
    outgoing.setTimeout(10_000, () => outgoing.destroy(new Error(`${method} ${path} timed out`)));
    outgoing.end(body);
  });
}

async function whole(port, method, path, options) {
  const stream = await request(port, method, path, options);
  const text = await stream.ended;
  return { status: stream.status, text };
}

function jsonOf(answer, what) {
  try {
    return JSON.parse(answer.text);
  } catch {
    return stop(`${what} answered ${answer.status} with ${JSON.stringify(answer.text.slice(0, 120))}`);
  }
}

function chatBody({ maxTokens, outputTokens, wave }) {
  const mock = { switch: "usage-last", shape: "openai", chunks: 3, chunk_ms: 10, input_tokens: 40, output_tokens: outputTokens };
  if (wave !== undefined) mock.wave = wave;
  return JSON.stringify({
    tier: TIER,
    max_tokens: maxTokens,
    stream: true,
    messages: [{ role: "user", content: "Synthetic cap check prompt with no captured content." }],
    mock,
  });
}

function trailerOf(text) {
  const at = text.lastIndexOf("event: ledger\ndata: ");
  if (at < 0) return null;
  const rest = text.slice(at + "event: ledger\ndata: ".length);
  try {
    return JSON.parse(rest.slice(0, rest.indexOf("\n")));
  } catch {
    return null;
  }
}

async function main() {
  const worker = memberPort("worker-test");
  const account = randomUUID();
  const credential = randomBytes(32).toString("hex");
  const headers = { "halvo-account": account, authorization: `Bearer ${credential}` };
  const capGet = async () => jsonOf(await whole(worker, "GET", "/v1/cap", { headers }), "GET /v1/cap");
  const sameSpend = (answer, cap, spend) => JSON.stringify(answer) === JSON.stringify({ result: "cap", cap, spend });

  let seeded;
  try {
    seeded = await whole(worker, "POST", "/test/seed", {
      body: JSON.stringify({ account, credential, subscription: "active", cap: FIRST_CAP, time: START }),
    });
  } catch (error) {
    stop(`the stack's test entry on port ${worker} does not answer (${error.message}): run sh scripts/stack.sh up`);
  }
  if (jsonOf(seeded, "POST /test/seed").result !== "seeded") stop(`POST /test/seed answered ${seeded.text}`);

  const first = await whole(worker, "GET", "/v1/cap", { headers });
  if (first.status !== 200) {
    tokens["read-back"] = `status-${first.status}`;
    stop(`GET /v1/cap after the seed answered ${first.status} with ${JSON.stringify(first.text.slice(0, 120))}`);
  }
  if (!sameSpend(jsonOf(first, "GET /v1/cap"), FIRST_CAP, { spent: 0, held: 0, period_end: START + WEEK_MS })) {
    tokens["read-back"] = "differs";
    stop(`GET /v1/cap after the seed answered ${first.text}, not the seeded cap with nothing spent and the period ending a week after the seed`);
  }
  tokens["read-back"] = "ok";

  const upstream = memberPort("upstream");
  const wave = `cap-check-${account}`;
  const heldBody = chatBody({ maxTokens: 1000, outputTokens: 900, wave });
  const hold = costOf(Buffer.byteLength(heldBody), 1000);
  const held = await request(worker, "POST", "/v1/chat", { body: heldBody, headers: { ...headers, "x-request-id": randomUUID() } });
  if (held.status !== 200) {
    tokens["hold-open"] = `status-${held.status}`;
    stop(`the first reservation answered ${held.status} with ${JSON.stringify((await held.ended).slice(0, 120))}`);
  }
  await held.first;
  const during = await capGet();
  if (!sameSpend(during, FIRST_CAP, { spent: 0, held: hold, period_end: START + WEEK_MS })) {
    tokens["hold-open"] = "differs";
    stop(`while the first stream was open GET /v1/cap answered ${JSON.stringify(during)}, not ${hold} held`);
  }
  tokens["hold-open"] = "1";

  if (hold <= LOWER_CAP.amount * MICRO_PER_MINOR) stop(`the hold ${hold} is not above the lower cap, so the check proves nothing`);
  const set = await whole(worker, "PUT", "/v1/cap", { body: JSON.stringify({ cap: LOWER_CAP }), headers });
  const setAnswer = jsonOf(set, "PUT /v1/cap");
  if (set.status !== 200 || !sameSpend(setAnswer, LOWER_CAP, { spent: 0, held: hold, period_end: START + WEEK_MS })) {
    tokens["set-below"] = set.status === 200 ? "differs" : `status-${set.status}`;
    stop(`PUT /v1/cap below counted spend answered ${set.status} with ${set.text}`);
  }
  tokens["set-below"] = "accepted";

  await whole(upstream, "POST", `/release?wave=${encodeURIComponent(wave)}`);
  const trailer = trailerOf(await held.ended);
  const cost = costOf(40, 900);
  if (trailer?.settled !== true || trailer.outcome !== "settled" || trailer.cost !== cost) {
    tokens.settled = "0";
    stop(`the first stream ended with the ledger event ${JSON.stringify(trailer)}, not a settle of ${cost}`);
  }
  const after = await capGet();
  if (!sameSpend(after, LOWER_CAP, { spent: cost, held: 0, period_end: START + WEEK_MS })) {
    tokens.settled = "differs";
    stop(`after the settle GET /v1/cap answered ${JSON.stringify(after)}, not the lower cap with ${cost} spent`);
  }
  tokens.settled = "1";

  const smallBody = chatBody({ maxTokens: 100, outputTokens: 50 });
  const refused = await whole(worker, "POST", "/v1/chat", { body: smallBody, headers: { ...headers, "x-request-id": randomUUID() } });
  const refusal = refused.status === 403 ? jsonOf(refused, "the second reservation") : null;
  tokens["refused-before-roll"] = refusal?.result === "refused" ? refusal.reason : `status-${refused.status}`;
  if (tokens["refused-before-roll"] !== "over-cap") stop(`the reservation after the settle answered ${refused.status} with ${refused.text.slice(0, 120)}`);

  const clock = jsonOf(await whole(worker, "POST", "/test/clock", { body: JSON.stringify({ account, time: START + WEEK_MS }) }), "POST /test/clock");
  if (clock.result !== "clock-set") stop(`POST /test/clock answered ${JSON.stringify(clock)}`);
  const rolled = await capGet();
  if (!sameSpend(rolled, LOWER_CAP, { spent: 0, held: 0, period_end: START + 2 * WEEK_MS })) {
    tokens.roll = "differs";
    stop(`at the period's end GET /v1/cap answered ${JSON.stringify(rolled)}, not a new period with nothing spent`);
  }
  tokens.roll = "ok";

  const admitted = await request(worker, "POST", "/v1/chat", { body: smallBody, headers: { ...headers, "x-request-id": randomUUID() } });
  const admittedText = await admitted.ended;
  const lastTrailer = trailerOf(admittedText);
  if (admitted.status !== 200 || lastTrailer?.outcome !== "settled") {
    tokens["admitted-after-roll"] = admitted.status === 200 ? "unsettled" : `status-${admitted.status}`;
    stop(`the reservation after the roll answered ${admitted.status} with ${admittedText.slice(0, 160)}`);
  }
  tokens["admitted-after-roll"] = "1";
}

try {
  await main();
  console.log(`PASS cap: ${line()}`);
  process.exit(0);
} catch (error) {
  const why = error instanceof Stop ? error.message : `the check could not run: ${String(error?.message ?? error).split("\n")[0]}`;
  console.log(`FAIL cap: ${line()}; first failure: ${why}`);
  process.exit(1);
}
