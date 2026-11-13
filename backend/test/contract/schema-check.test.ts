import fc from "fast-check";
import { expect, test, vi } from "vitest";
import type { Handlers, RouteName } from "../../src/contract/routes.ts";
import type { Env } from "../../src/types.ts";

const SENTINEL = "hv60sentinel";
const UNMEASURED = "unmeasured";
const ACCOUNT = "3b8f1c2a-7d4e-4f6a-9b0c-1d2e3f4a5b6c";
const CREDENTIAL = "5e".repeat(32);
const SENT = [SENTINEL, ACCOUNT, CREDENTIAL];
const NAMES_FLOOR = 35;
const ARBITRARY_NAMES = 100;
const NOT_ARBITRARY = ["cap", "amount", "period", "__proto__"];
const BASE = "http://backend.test";
const APP_ROUTES: RouteName[] = ["checkout-start", "status", "cap-get", "cap-set", "manage", "account-delete"];

type Json = string | number | boolean | null | Json[] | { [key: string]: Json };
type JsonObject = { [key: string]: Json };
interface Route {
  name: string;
  kind: string;
  method: string;
  path: string;
  request: string | null;
  answer: string | null;
  limit: number;
  results: Record<string, string[]>;
}
interface Samples {
  requests: Record<string, JsonObject>;
  answers: Record<string, Record<string, JsonObject>>;
}
interface Schema {
  safeParse(value: unknown): { success: boolean };
}
interface Loaded {
  makeApp: (handlers: Handlers) => (request: Request, env: Env) => Promise<Response>;
  requestSchemas: Record<string, Schema | null>;
  largestBody: (route: RouteName) => string | null;
  routes: Route[];
  samples: Samples;
  names: string[];
  namesFile: unknown;
}
interface Case {
  group: "unmodified" | "forbidden" | "arbitrary" | "query" | "raw";
  request: Request;
  want: number;
}

async function importFrom(path: string): Promise<Record<string, unknown>> {
  return import(path);
}

async function textOf(path: string): Promise<string> {
  const loaded = await importFrom(`${path}?raw`);
  return String(loaded.default);
}

function headers(extra: Record<string, string> = {}): Headers {
  return new Headers({ "halvo-account": ACCOUNT, authorization: `Bearer ${CREDENTIAL}`, ...extra });
}

function request(route: Route, init: { path?: string; method?: string; headers?: Headers; body?: string | null }): Request {
  const method = init.method ?? route.method;
  const body = init.body ?? null;
  return new Request(`${BASE}${init.path ?? route.path}`, {
    method,
    headers: init.headers ?? headers(route.request === null ? {} : { "content-type": "application/json" }),
    body: method === "GET" || method === "HEAD" ? null : body,
  });
}

function unmodified(route: Route, body: string | null): Request {
  const json: Record<string, string> = route.request === null ? {} : { "content-type": "application/json" };
  return request(route, { body, headers: headers({ ...json, "x-halvo-sentinel": SENTINEL }) });
}

function holdsSent(text: string): boolean {
  return SENT.some((each) => text.includes(each));
}

function withKey(body: JsonObject, at: "root" | "cap", name: string): JsonObject {
  const copy = structuredClone(body);
  const target = at === "root" ? copy : (copy.cap as JsonObject);
  Object.defineProperty(target, name, { value: SENTINEL, enumerable: true, writable: true, configurable: true });
  return copy;
}

function arbitraryNames(seed: number, forbidden: string[]): string[] {
  const out: string[] = [];
  for (let runs = 1000; out.length < ARBITRARY_NAMES && runs <= 64_000; runs *= 2) {
    out.length = 0;
    for (const name of fc.sample(fc.string({ minLength: 1, maxLength: 16 }), { seed, numRuns: runs })) {
      if (out.length === ARBITRARY_NAMES) break;
      if (out.includes(name) || NOT_ARBITRARY.includes(name) || forbidden.includes(name)) continue;
      out.push(name);
    }
  }
  return out;
}

async function load(failures: string[]): Promise<{ seed: number | null; loaded: Loaded | null }> {
  let seed: number | null = null;
  try {
    const text = await textOf("../../gate/fixtures/schema/seed.txt");
    if (/^[0-9]+\n?$/.test(text)) seed = Number(text.trim());
    else failures.push("gate/fixtures/schema/seed.txt holds no decimal integer");
  } catch (error) {
    failures.push(`gate/fixtures/schema/seed.txt does not load (${(error as Error).name})`);
  }
  const parts: Record<string, unknown> = {};
  const wanted: [string, () => Promise<unknown>][] = [
    ["src/app.ts", () => importFrom("../../src/app.ts")],
    ["src/contract/schemas.ts", () => importFrom("../../src/contract/schemas.ts")],
    ["src/contract/export.ts", () => importFrom("../../src/contract/export.ts")],
    ["src/contract/routes.ts", () => importFrom("../../src/contract/routes.ts")],
    ["contract/samples/v1.json", async () => JSON.parse(await textOf("../../contract/samples/v1.json"))],
    ["contract/names/forbidden-names.json", async () => JSON.parse(await textOf("../../contract/names/forbidden-names.json"))],
  ];
  for (const [name, get] of wanted) {
    try {
      parts[name] = await get();
    } catch (error) {
      failures.push(`${name} does not load (${(error as Error).name})`);
    }
  }
  if (failures.length > 0 && Object.keys(parts).length < wanted.length) return { seed, loaded: null };
  const app = parts["src/app.ts"] as Record<string, unknown>;
  const schemas = parts["src/contract/schemas.ts"] as Record<string, unknown>;
  const exported = parts["src/contract/export.ts"] as Record<string, unknown>;
  const routes = parts["src/contract/routes.ts"] as { routeTable?: { routes?: Route[] } };
  const names = parts["contract/names/forbidden-names.json"] as { names?: { name: string }[] };
  return {
    seed,
    loaded: {
      makeApp: app.makeApp as Loaded["makeApp"],
      requestSchemas: schemas.requestSchemas as Loaded["requestSchemas"],
      largestBody: exported.largestBody as Loaded["largestBody"],
      routes: routes.routeTable?.routes ?? [],
      samples: parts["contract/samples/v1.json"] as Samples,
      names: (names.names ?? []).map((entry) => entry.name),
      namesFile: names,
    },
  };
}

async function namesFloorFailure(file: unknown): Promise<string | null> {
  const { version, names } = (file ?? {}) as { version?: unknown; names?: unknown };
  const count = Array.isArray(names) ? names.length : 0;
  if (typeof version !== "number" || !Number.isInteger(version) || version < 0) {
    return `the names file holds ${count} names and no version that is a whole number, so its floor is unknown`;
  }
  let floor = NAMES_FLOOR;
  if (version > 0) {
    let text: string | null = null;
    try {
      text = await textOf("../../contract/names/floor.txt");
    } catch {
      text = null;
    }
    const at = `the names file holds ${count} names at version ${version}, and contract/names/floor.txt`;
    if (text === null) return `${at}, which gives its floor, is missing`;
    if (!/^[0-9]+\n$/.test(text)) return `${at} holds other than one decimal integer and a newline`;
    floor = Number(text.slice(0, -1));
    if (floor < NAMES_FLOOR) return `${at} gives the floor ${floor}, below the ${NAMES_FLOOR} names of version 0`;
  }
  return count < floor ? `the names file holds ${count} names, below its floor of ${floor}` : null;
}

function rawCases(route: Route, sample: JsonObject | undefined): Case[] {
  const cases: Case[] = [];
  const json = { "content-type": "application/json" };
  const sentinelHeader = { "x-halvo-sentinel": SENTINEL };
  const all = (extra: Record<string, string> = {}) => headers({ ...(route.request === null ? {} : json), ...sentinelHeader, ...extra });
  const body = route.request === null || sample === undefined ? null : JSON.stringify(sample);
  if (route.request !== null) {
    const domain = `{"cap":{"amount":1,"period":"${SENTINEL}"}}`;
    cases.push({ group: "raw", want: 400, request: request(route, { headers: all(), body: `{"${SENTINEL}"` }) });
    cases.push({ group: "raw", want: 400, request: request(route, { headers: all(), body: `["${SENTINEL}"]` }) });
    cases.push({ group: "raw", want: 400, request: request(route, { headers: all(), body: `"${SENTINEL}"` }) });
    cases.push({ group: "raw", want: 415, request: request(route, { headers: all({ "content-type": "text/plain" }), body: `"${SENTINEL}"` }) });
    cases.push({ group: "raw", want: 413, request: request(route, { headers: all(), body: domain.padEnd(route.limit + 1, " ") }) });
    cases.push({ group: "raw", want: 400, request: request(route, { headers: all(), body: `{"cap":1,${body?.slice(1)}` }) });
    cases.push({ group: "raw", want: 400, request: request(route, { headers: all(), body: domain }) });
  }
  if (route.name === "manage" || route.name === "account-delete") {
    cases.push({ group: "raw", want: 400, request: request(route, { headers: all(json), body: `"${SENTINEL}"` }) });
  }
  cases.push({ group: "raw", want: 404, request: request(route, { path: `${route.path}/${SENTINEL}`, headers: all(), body }) });
  cases.push({ group: "raw", want: 404, request: request(route, { path: `${route.path}x`, headers: all(), body }) });
  cases.push({ group: "raw", want: 405, request: request(route, { method: "PATCH", headers: all(), body }) });
  const withoutAccount = all();
  withoutAccount.delete("halvo-account");
  cases.push({ group: "raw", want: 400, request: request(route, { headers: withoutAccount, body }) });
  cases.push({ group: "raw", want: 400, request: request(route, { headers: all({ authorization: `Bearer ${SENTINEL}` }), body }) });
  return cases;
}

function count(totals: Map<string, number>, key: string): number {
  return totals.get(key) ?? 0;
}

async function measure(): Promise<string> {
  const failures: string[] = [];
  const { seed, loaded } = await load(failures);
  const tokens: Record<string, string> = {
    seed: seed === null ? UNMEASURED : String(seed),
    unmodified: UNMEASURED,
    validator: UNMEASURED,
    forbidden: UNMEASURED,
    arbitrary: UNMEASURED,
    query: UNMEASURED,
    raw: UNMEASURED,
    "5xx": UNMEASURED,
    "handler-calls": UNMEASURED,
    sentinel: UNMEASURED,
  };
  const line = () => Object.entries(tokens).map(([key, value]) => `${key}=${value}`).join(" ");
  if (loaded === null || seed === null) return `FAIL schema: ${line()}; first failure: ${failures[0]}`;
  const belowFloor = await namesFloorFailure(loaded.namesFile);
  if (belowFloor !== null) failures.push(belowFloor);

  const byName = new Map(loaded.routes.map((route) => [route.name, route]));
  const app = APP_ROUTES.map((name) => byName.get(name)).filter((route): route is Route => route !== undefined);
  if (app.length !== APP_ROUTES.length) failures.push(`the route table holds ${app.length} of the six app routes`);
  const withBody = app.filter((route) => route.request !== null);
  const withoutBody = app.filter((route) => route.request === null);
  const forbidden = loaded.names;
  const arbitrary = arbitraryNames(seed, forbidden);
  if (arbitrary.length !== ARBITRARY_NAMES) failures.push(`fast-check gave ${arbitrary.length} distinct arbitrary names, not ${ARBITRARY_NAMES}`);

  let calls = 0;
  const firstAnswer = (route: string) => Object.values(loaded.samples.answers[route] ?? {})[0] ?? null;
  const record = (route: string) => async () => {
    calls += 1;
    return firstAnswer(route);
  };
  const handlers = {
    "checkout-start": record("checkout-start"),
    status: record("status"),
    "cap-get": record("cap-get"),
    "cap-set": record("cap-set"),
    manage: record("manage"),
    "account-delete": record("account-delete"),
    webhook: async () => {
      calls += 1;
      return new Response(null, { status: 200 });
    },
    done: () => {
      calls += 1;
      return new Response("done", { status: 200 });
    },
  } as unknown as Handlers;
  const serve = loaded.makeApp(handlers);

  let validatorRefused = 0;
  let validatorTotal = 0;
  const cases: Case[] = [];
  for (const route of withBody) {
    const sample = loaded.samples.requests[route.name];
    const largest = loaded.largestBody(route.name as RouteName);
    if (sample === undefined || largest === null) {
      failures.push(`${route.name} has no sample body or no largest valid body`);
      continue;
    }
    cases.push({ group: "unmodified", want: 200, request: unmodified(route, JSON.stringify(sample)) });
    cases.push({ group: "unmodified", want: 200, request: unmodified(route, largest) });
    const schema = loaded.requestSchemas[route.name];
    for (const at of ["root", "cap"] as const) {
      for (const [group, names] of [["forbidden", forbidden], ["arbitrary", arbitrary]] as const) {
        for (const name of names) {
          const body = withKey(sample, at, name);
          validatorTotal += 1;
          if (schema?.safeParse(body).success === false) validatorRefused += 1;
          cases.push({ group, want: 413, request: request(route, { body: JSON.stringify(body) }) });
        }
      }
    }
  }
  for (const route of withoutBody) cases.push({ group: "unmodified", want: 200, request: unmodified(route, null) });
  for (const route of app) {
    const body = route.request === null ? null : JSON.stringify(loaded.samples.requests[route.name]);
    const queries = [...forbidden.map((name) => `${encodeURIComponent(name)}=${SENTINEL}`), `${SENTINEL}=1`];
    for (const query of queries) cases.push({ group: "query", want: 400, request: request(route, { path: `${route.path}?${query}`, body }) });
    cases.push(...rawCases(route, loaded.samples.requests[route.name]));
  }

  const lines: string[] = [];
  let notOneString = 0;
  const spies = (["log", "info", "warn", "error", "debug"] as const).map((level) =>
    vi.spyOn(console, level).mockImplementation((...args: unknown[]) => {
      if (args.length === 1 && typeof args[0] === "string") lines.push(args[0]);
      else notOneString += 1;
    }),
  );
  const hits = new Map<string, number>();
  const totals = new Map<string, number>();
  let fiveHundreds = 0;
  let rejectionCalls = 0;
  let sentResponses = 0;
  try {
    for (const each of cases) {
      totals.set(each.group, count(totals, each.group) + 1);
      const before = calls;
      let response: Response;
      try {
        response = await serve(each.request, {} as Env);
      } catch (error) {
        failures.push(`the app threw ${(error as Error).name} for ${each.group} ${each.request.method} ${each.request.url}`);
        continue;
      }
      const text = await response.text();
      const shown = `${text} ${[...response.headers].map(([key, value]) => `${key}: ${value}`).join(" ")}`;
      if (holdsSent(shown)) sentResponses += 1;
      const rejection = each.group !== "unmodified";
      if (rejection && response.status >= 500) fiveHundreds += 1;
      if (rejection && calls !== before) rejectionCalls += calls - before;
      const rightBody = !rejection || (text === '{"result":"rejected"}' && response.headers.get("content-type") === "application/json");
      const reachedItsHandler = rejection || calls === before + 1;
      if (response.status === each.want && rightBody && reachedItsHandler) hits.set(each.group, count(hits, each.group) + 1);
      else if (failures.length < 5) {
        const what = reachedItsHandler ? (rightBody ? "" : " with another body") : " without one call of its recording handler";
        failures.push(`${each.group} ${each.request.method} ${each.request.url} answered ${response.status}${what}, not ${each.want}`);
      }
    }
  } finally {
    for (const spy of spies) spy.mockRestore();
  }
  const sentLines = lines.filter(holdsSent).length;
  const sentinels = sentResponses + sentLines + notOneString;

  const n = forbidden.length;
  const expected: Record<string, number> = {
    unmodified: 8,
    validator: 4 * n + 4 * ARBITRARY_NAMES,
    forbidden: 4 * n,
    arbitrary: 4 * ARBITRARY_NAMES,
    query: 6 * n + 6,
    raw: 46,
  };
  const measured: Record<string, [number, number]> = {
    unmodified: [count(hits, "unmodified"), count(totals, "unmodified")],
    validator: [validatorRefused, validatorTotal],
    forbidden: [count(hits, "forbidden"), count(totals, "forbidden")],
    arbitrary: [count(hits, "arbitrary"), count(totals, "arbitrary")],
    query: [count(hits, "query"), count(totals, "query")],
    raw: [count(hits, "raw"), count(totals, "raw")],
  };
  for (const [key, [got, total]] of Object.entries(measured)) {
    tokens[key] = `${got} of ${total}`;
    if (total !== expected[key]) failures.push(`the ${key} group has ${total} cases, and contract 9.1 gives ${expected[key]}`);
    else if (got !== total) failures.push(`the ${key} group passed ${got} of ${total} cases`);
  }
  tokens["5xx"] = String(fiveHundreds);
  tokens["handler-calls"] = String(rejectionCalls);
  tokens.sentinel = String(sentinels);
  if (fiveHundreds > 0) failures.push(`${fiveHundreds} rejection cases got a 5xx`);
  if (rejectionCalls > 0) failures.push(`the recording handlers were called ${rejectionCalls} times for rejection cases`);
  if (sentinels > 0) {
    failures.push(
      `the sentinel token is ${sentinels}: ${sentResponses} responses and ${sentLines} lines hold the sentinel, the account id or the credential that the check sent, and ${notOneString} console calls were not one string argument`,
    );
  }
  if (failures.length === 0) return `PASS schema: ${line()}`;
  return `FAIL schema: ${line()}; first failure: ${failures[0]}`;
}

test("check 60: the injector finds no request that passes validation and should not", async () => {
  const line = await measure();
  console.log(`schema-check-result: ${line}`);
  expect(line.startsWith("PASS schema: "), line).toBe(true);
}, 300_000);
