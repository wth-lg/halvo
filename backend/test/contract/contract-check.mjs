import { spawnSync } from "node:child_process";
import { createHash, randomBytes, randomUUID } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { pathToFileURL } from "node:url";

const root = process.env.HALVO_ROOT;
const state = process.env.GATE_STATE ?? join(root, ".tmp");
mkdirSync(state, { recursive: true });
const work = mkdtempSync(join(state, "contract-"));
const UNMEASURED = "unmeasured";
const CONFIG_KEYS = [
  "$schema",
  "name",
  "main",
  "compatibility_date",
  "send_metrics",
  "durable_objects",
  "migrations",
  "vars",
  "observability",
];
const DEPLOYABLE_PATHS = ["/v1/chat", "/test/clock", "/test/seed", "/test/inspect"];
const BUNDLE_MUST_NOT_HOLD = ["src/gateway/", "stress/", "test-kit/"];
const SAMPLES = 32;
const NAMES_FLOOR = 35;

const tokens = {
  rules: UNMEASURED,
  export: UNMEASURED,
  routes: UNMEASURED,
  samples: UNMEASURED,
  types: UNMEASURED,
  open: UNMEASURED,
  bundle: UNMEASURED,
  deployable: UNMEASURED,
  "test-paths": UNMEASURED,
  config: UNMEASURED,
};
const failures = [];
const note = (text) => console.log(`  ${text}`);
const fail = (sentence) => {
  failures.push(sentence);
  note(`failed: ${sentence}`);
};

function readText(path) {
  const full = join(root, path);
  return existsSync(full) ? readFileSync(full, "utf8") : null;
}

function readJson(path) {
  const text = readText(path);
  return text === null ? null : JSON.parse(text);
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { cwd: root, encoding: "utf8", ...options });
  note(`ran ${command} ${args.join(" ")}: exit ${result.status}`);
  if (result.stderr) note(result.stderr.trimEnd().split("\n").slice(-15).join("\n  "));
  return result;
}

async function load(path) {
  return import(pathToFileURL(join(root, path)).href);
}

const rulesScript = join(root, "scripts/contract-rules");
let schemaText = null;

function namesFloorFailure(file, floorText) {
  const count = Array.isArray(file?.names) ? file.names.length : 0;
  const version = file?.version;
  if (!Number.isInteger(version) || version < 0) {
    return `the names file holds ${count} names and no version that is a whole number, so its floor is unknown`;
  }
  let floor = NAMES_FLOOR;
  if (version > 0) {
    const at = `the names file holds ${count} names at version ${version}, and contract/names/floor.txt`;
    if (floorText === null) return `${at}, which gives its floor, is missing`;
    if (!/^[0-9]+\n$/.test(floorText)) return `${at} holds other than one decimal integer and a newline`;
    floor = Number(floorText.slice(0, -1));
    if (floor < NAMES_FLOOR) return `${at} gives the floor ${floor}, below the ${NAMES_FLOOR} names of version 0`;
  }
  return count < floor ? `the names file holds ${count} names, below its floor of ${floor}` : null;
}

function measureNamesFloor() {
  const failure = namesFloorFailure(readJson("contract/names/forbidden-names.json"), readText("contract/names/floor.txt"));
  if (failure !== null) fail(failure);
}

function measureRulesAndExport() {
  if (!existsSync(rulesScript)) {
    fail("scripts/contract-rules does not exist, so the export and the walk cannot run");
    return;
  }
  const fresh = run("node", [rulesScript, "export", "schema"]);
  if (fresh.status !== 0) {
    fail(`node scripts/contract-rules export schema exited ${fresh.status}`);
    return;
  }
  const freshPath = join(work, "fresh-schema.json");
  writeFileSync(freshPath, fresh.stdout);
  const walk = run("node", [rulesScript, "walk", freshPath]);
  const lines = walk.stdout.split("\n").filter((line) => line !== "");
  note(`walk lines: ${lines.length}`);
  for (const line of lines.slice(0, 20)) note(line);
  if (walk.status === 0 && lines.length === 0) tokens.rules = "ok";
  else if (walk.status === 1 && lines.length > 0) {
    tokens.rules = lines[0];
    fail(`the rules walk refused the fresh export with ${lines.length} lines, the first ${lines[0]}`);
  } else fail(`the rules walk exited ${walk.status} with ${lines.length} lines`);
  schemaText = readText("contract/schema/v1.json");
  if (schemaText === null) {
    tokens.export = "missing";
    fail("contract/schema/v1.json is missing");
  } else if (schemaText === fresh.stdout) tokens.export = "equal";
  else {
    tokens.export = "differs";
    fail("a fresh export of the schemas differs from contract/schema/v1.json");
  }
}

function resultsAgree(answer, results) {
  if (answer === null || typeof answer !== "object" || Array.isArray(answer)) return false;
  const listed = results[answer.result];
  if (!Array.isArray(listed)) return false;
  const want = ["result", ...listed].sort();
  const got = Object.keys(answer).sort();
  return want.length === got.length && want.every((key, index) => key === got[index]);
}

async function measureRoutes() {
  if (!existsSync(rulesScript)) return null;
  const fresh = run("node", [rulesScript, "export", "routes"]);
  const committed = readText("contract/routes/v1.json");
  if (fresh.status !== 0) {
    fail(`node scripts/contract-rules export routes exited ${fresh.status}`);
    return null;
  }
  if (committed === null) {
    tokens.routes = "missing";
    fail("contract/routes/v1.json is missing");
    return null;
  }
  if (committed !== fresh.stdout) {
    tokens.routes = "differs";
    fail("a fresh export of the route table differs from contract/routes/v1.json");
    return JSON.parse(committed);
  }
  const table = JSON.parse(committed);
  const schema = schemaText === null ? null : JSON.parse(schemaText);
  if (schema === null) {
    fail("the route table cannot be compared with a schema export that is missing");
    return table;
  }
  const { largestBody } = await load("src/contract/export.ts");
  for (const route of table.routes.filter((each) => each.kind === "app")) {
    const answer = schema.$defs[route.answer];
    const values = answer?.properties?.result?.enum ?? [];
    const keys = Object.keys(route.results);
    const sameValues = keys.length === values.length && keys.every((key) => values.includes(key));
    const listed = [...new Set(Object.values(route.results).flat())].sort();
    const properties = Object.keys(answer?.properties ?? {})
      .filter((key) => key !== "result")
      .sort();
    if (!sameValues || listed.join(",") !== properties.join(",")) {
      tokens.routes = `results-of-${route.name}`;
      fail(`the results of ${route.name} do not match the answer schema ${route.answer}`);
      return table;
    }
    const body = largestBody(route.name);
    const limit = body === null ? 0 : Buffer.byteLength(body, "utf8");
    if (route.limit !== limit) {
      tokens.routes = `limit-of-${route.name}`;
      fail(`the limit of ${route.name} is ${route.limit}, and its largest valid body has ${limit} bytes`);
      return table;
    }
  }
  tokens.routes = "equal";
  return table;
}

function samplesOf(file) {
  const out = [];
  for (const [route, body] of Object.entries(file?.requests ?? {})) out.push({ label: `requests/${route}`, kind: "request", route, value: body });
  for (const [route, byResult] of Object.entries(file?.answers ?? {})) {
    for (const [result, answer] of Object.entries(byResult)) out.push({ label: `answers/${route}/${result}`, kind: "answer", route, result, value: answer });
  }
  if (file && "rejected" in file) out.push({ label: "rejected", kind: "rejected", value: file.rejected });
  return out;
}

function expectedSampleLabels(table) {
  const labels = [];
  for (const route of table.routes.filter((each) => each.kind === "app")) {
    if (route.request !== null) labels.push(`requests/${route.name}`);
  }
  for (const route of table.routes.filter((each) => each.kind === "app")) {
    for (const result of Object.keys(route.results)) labels.push(`answers/${route.name}/${result}`);
  }
  labels.push("rejected");
  return labels;
}

async function measureSamples(table, rust) {
  const file = readJson("contract/samples/v1.json");
  if (file === null) {
    fail("contract/samples/v1.json is missing");
    return;
  }
  if (table === null) {
    fail("the samples cannot be counted without the route table");
    return;
  }
  const schemas = await load("src/contract/schemas.ts");
  const byRoute = Object.fromEntries(table.routes.map((route) => [route.name, route]));
  const expected = expectedSampleLabels(table);
  const present = new Map(samplesOf(file).map((sample) => [sample.label, sample]));
  const readByRust = new Set(rust?.read ?? []);
  let good = 0;
  for (const label of expected) {
    const sample = present.get(label);
    if (!sample) {
      fail(`contract/samples/v1.json has no sample ${label}`);
      continue;
    }
    let zodRead = false;
    let agrees = true;
    if (sample.kind === "request") zodRead = schemas.requestSchemas[sample.route]?.safeParse(sample.value).success === true;
    if (sample.kind === "answer") {
      zodRead = schemas.answerSchemas[sample.route]?.safeParse(sample.value).success === true;
      agrees = sample.value?.result === sample.result && resultsAgree(sample.value, byRoute[sample.route].results);
    }
    if (sample.kind === "rejected") zodRead = schemas.Rejected.safeParse(sample.value).success === true;
    if (zodRead && agrees && readByRust.has(label)) good += 1;
    else fail(`the sample ${label} was ${zodRead ? "" : "not "}read by its Zod schema, ${readByRust.has(label) ? "" : "not "}read by its Rust type, and ${agrees ? "agrees" : "does not agree"} with results`);
  }
  for (const label of present.keys()) {
    if (!expected.includes(label)) fail(`contract/samples/v1.json holds ${label}, which the route table does not list`);
  }
  if (expected.length !== SAMPLES) fail(`the route table asks for ${expected.length} samples, and contract 9.2 gives ${SAMPLES}`);
  tokens.samples = `${good} of ${expected.length}`;
}

function measureTypes() {
  if (schemaText === null && readText("contract/schema/v1.json") === null) {
    fail("the crate of test/contract/rust cannot build without contract/schema/v1.json");
    return null;
  }
  const samples = join(root, "contract/samples/v1.json");
  const samplesPath = existsSync(samples) ? samples : join(work, "no-samples.json");
  if (!existsSync(samples)) writeFileSync(samplesPath, "{}\n");
  const cargo = run(
    "cargo",
    ["run", "--locked", "--offline", "--quiet", "--manifest-path", join(root, "test/contract/rust/Cargo.toml"), "--", samplesPath],
    { env: { ...process.env, CARGO_TARGET_DIR: join(root, ".tmp/contract-rust/target") } },
  );
  const line = cargo.stdout.split("\n").find((each) => each.startsWith("CONTRACT-RUST "));
  if (cargo.status !== 0 || !line) {
    fail(`cargo run --locked --offline of test/contract/rust exited ${cargo.status}`);
    return null;
  }
  const rust = JSON.parse(line.slice("CONTRACT-RUST ".length));
  note(`counted: ${rust.counted.join(" ")}`);
  for (const each of [...rust.open, ...rust.unknown_read, ...rust.refused]) note(each);
  tokens.types = String(rust.types);
  tokens.open = String(rust.open.length + rust.unknown_read.length);
  if (rust.missing.length > 0) fail(`the generated source has no type for the $defs key ${rust.missing[0]}`);
  const schema = JSON.parse(readText("contract/schema/v1.json"));
  const objects = Object.values(schema.$defs ?? {}).filter((def) => def.type === "object").length;
  if (rust.types !== objects) fail(`typify made ${rust.types} types that serde reads from an object, and $defs holds ${objects} object schemas`);
  if (rust.open.length > 0) fail(`a generated type is open: ${rust.open[0]}`);
  if (rust.unknown_read.length > 0) fail(`a generated type reads an unknown key: ${rust.unknown_read[0]}`);
  return rust;
}

function measureBundle() {
  const out = join(work, "bundle");
  const meta = join(out, "meta.json");
  const dry = run(
    join(root, "node_modules/.bin/wrangler"),
    ["deploy", "--dry-run", "--config", join(root, "wrangler.jsonc"), "--outdir", out, "--metafile", meta],
    { env: { ...process.env, XDG_CONFIG_HOME: process.env.HALVO_WRANGLER_CONFIG_HOME } },
  );
  if (dry.status !== 0 || !existsSync(meta)) {
    fail(`wrangler deploy --dry-run of the deployable entry exited ${dry.status}`);
    return;
  }
  const inputs = Object.keys(JSON.parse(readFileSync(meta, "utf8")).inputs).map((input) =>
    relative(root, resolve(root, input.replace(/^[\w-]+:/, ""))),
  );
  note(`bundle inputs: ${inputs.join(" ")}`);
  const held = inputs.filter((input) => BUNDLE_MUST_NOT_HOLD.some((prefix) => input.startsWith(prefix)));
  if (held.length === 0) tokens.bundle = "clean";
  else {
    tokens.bundle = held[0];
    fail(`the deployable bundle holds ${held.join(", ")}`);
  }
}

function stackUrl(name) {
  const result = run("sh", [join(root, "scripts/stack.sh"), "url", name]);
  return result.status === 0 ? result.stdout.trim() : null;
}

async function post(url, body) {
  return fetch(url, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
    signal: AbortSignal.timeout(10_000),
  });
}

async function measureDeployable() {
  const base = stackUrl("worker");
  if (base === null) {
    fail("sh scripts/stack.sh url worker names no member");
    return;
  }
  let notFound = 0;
  try {
    for (const path of DEPLOYABLE_PATHS) {
      const response = await post(`${base}${path}`, {});
      await response.arrayBuffer();
      note(`deployable POST ${path}: ${response.status}`);
      if (response.status === 404) notFound += 1;
      else fail(`POST ${path} on the deployable entry answered ${response.status}, not 404`);
    }
  } catch (error) {
    fail(`the deployable entry at ${base} did not answer (${error.name}); bring the stack up with sh scripts/stack.sh up`);
    return;
  }
  tokens.deployable = `${notFound} of ${DEPLOYABLE_PATHS.length}`;
}

async function measureTestPaths() {
  const base = stackUrl("worker-test");
  if (base === null) {
    fail("sh scripts/stack.sh url worker-test names no member");
    return;
  }
  const account = randomUUID();
  const credential = randomBytes(32).toString("hex");
  const hash = createHash("sha256").update(credential, "utf8").digest("hex");
  const time = Date.now();
  let answered = 0;
  try {
    const seed = await post(`${base}/test/seed`, { account, credential, subscription: "active", time });
    const seeded = seed.status === 200 ? await seed.json() : null;
    note(`test path seed: ${seed.status} ${JSON.stringify(seeded)}`);
    if (seeded?.result === "seeded") answered += 1;
    else fail(`POST /test/seed answered ${seed.status} ${JSON.stringify(seeded)}, not seeded`);
    const clock = await post(`${base}/test/clock`, { account, time: time + 1 });
    const set = clock.status === 200 ? await clock.json() : null;
    note(`test path clock: ${clock.status} ${JSON.stringify(set)}`);
    if (set?.result === "clock-set") answered += 1;
    else fail(`POST /test/clock answered ${clock.status} ${JSON.stringify(set)}, not clock-set`);
    const inspect = await post(`${base}/test/inspect`, { account });
    const snapshot = inspect.status === 200 ? await inspect.json() : null;
    const rows = Object.values(snapshot?.tables ?? {}).flat();
    const holds = rows.some((row) => Object.values(row).includes(account) && Object.values(row).includes(hash));
    note(`test path inspect: ${inspect.status}, result ${snapshot?.result}, tables ${Object.keys(snapshot?.tables ?? {}).join(" ")}`);
    if (snapshot?.result === "snapshot" && holds) answered += 1;
    else fail(`POST /test/inspect answered ${inspect.status} with no row that holds the seeded account id and credential hash`);
  } catch (error) {
    fail(`the test entry at ${base} did not answer (${error.name}); bring the stack up with sh scripts/stack.sh up`);
    return;
  }
  tokens["test-paths"] = `${answered} of 3`;
}

function measureConfig() {
  let config;
  try {
    config = JSON.parse(readFileSync(join(root, "wrangler.jsonc"), "utf8"));
  } catch (error) {
    tokens.config = "unparsed";
    fail(`JSON.parse cannot read wrangler.jsonc (${error.name}): it holds a comment or is no JSON`);
    return;
  }
  const keys = Object.keys(config).sort();
  const issues = [];
  if (keys.join(",") !== [...CONFIG_KEYS].sort().join(",")) issues.push(["keys", `wrangler.jsonc holds the top-level keys ${keys.join(", ")}, not the nine of contract 6`]);
  const vars = config.vars ?? {};
  if ("TEST_CLOCK" in vars || "UPSTREAM_BASE_URL" in vars) issues.push(["vars", "wrangler.jsonc declares TEST_CLOCK or UPSTREAM_BASE_URL"]);
  if ((config.durable_objects?.bindings ?? []).length !== 1) issues.push(["bindings", "wrangler.jsonc does not declare exactly one Durable Object binding"]);
  if (config.observability?.logs?.invocation_logs !== false) issues.push(["invocation-logs", "wrangler.jsonc does not set observability.logs.invocation_logs to false"]);
  if (issues.length === 0) tokens.config = "ok";
  else {
    tokens.config = issues[0][0];
    fail(issues[0][1]);
  }
}

async function step(name, measure) {
  try {
    return await measure();
  } catch (error) {
    fail(`the step ${name} threw ${error.name}: ${String(error.message).split("\n")[0]}`);
    return null;
  }
}

await step("names", measureNamesFloor);
await step("rules", measureRulesAndExport);
const table = await step("routes", measureRoutes);
const rust = await step("types", measureTypes);
await step("samples", () => measureSamples(table ?? readJson("contract/routes/v1.json"), rust));
await step("bundle", measureBundle);
await step("deployable", measureDeployable);
await step("test-paths", measureTestPaths);
await step("config", measureConfig);

const want = {
  rules: "ok",
  export: "equal",
  routes: "equal",
  open: "0",
  bundle: "clean",
  deployable: "4 of 4",
  "test-paths": "3 of 3",
  config: "ok",
};
const passed = failures.length === 0 && tokens.samples === `${SAMPLES} of ${SAMPLES}` && Object.entries(want).every(([key, value]) => tokens[key] === value);
const body = Object.entries(tokens)
  .map(([key, value]) => `${key}=${value}`)
  .join(" ");
if (passed) {
  console.log(`PASS contract: ${body}`);
  process.exit(0);
}
console.log(`FAIL contract: ${body}; first failure: ${failures[0] ?? "a token has another value than the PASS line"}`);
process.exit(1);
