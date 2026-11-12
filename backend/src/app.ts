import { type Handlers, type Route, type RouteName, routeTable } from "./contract/routes.ts";
import { answerSchemas, requestSchemas } from "./contract/schemas.ts";
import { errorName, log } from "./log.ts";
import { accountDelete } from "./routes/accounts.ts";
import { capGet, capSet } from "./routes/cap.ts";
import { checkoutStart } from "./routes/checkout.ts";
import { done } from "./routes/done.ts";
import { manage } from "./routes/manage.ts";
import { status } from "./routes/subscription.ts";
import { webhook } from "./routes/webhook.ts";
import type { Env } from "./types.ts";

const REJECTED_BODY = '{"result":"rejected"}';
const JSON_TYPE = "application/json";
const ACCOUNT_PATTERN = new RegExp(routeTable.headers["halvo-account"].pattern);
const CREDENTIAL_PATTERN = new RegExp(routeTable.headers.authorization.pattern);
const CONTENT_TYPE_PATTERN = new RegExp(routeTable.headers["content-type"].pattern);
const BEARER = "Bearer ";
const encoder = new TextEncoder();

type LoggedRoute = RouteName | "webhook" | "done" | "none";
type AppRoute = Extract<Route, { kind: "app" }>;

function isAppRoute(route: Route): route is AppRoute {
  return route.kind === "app";
}

function rejected(status: number, route: LoggedRoute): Response {
  log({ event: "rejected", route, status });
  return new Response(REJECTED_BODY, { status, headers: { "content-type": JSON_TYPE } });
}

function empty(status: number): Response {
  return new Response(null, { status });
}

export async function credentialHash(credential: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", encoder.encode(credential));
  return [...new Uint8Array(digest)].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

export function readAccountHeaders(headers: Headers): { accountId: string; credential: string } | null {
  const accountId = headers.get("halvo-account");
  const authorization = headers.get("authorization");
  if (accountId === null || authorization === null) return null;
  if (!ACCOUNT_PATTERN.test(accountId) || !CREDENTIAL_PATTERN.test(authorization)) return null;
  return { accountId, credential: authorization.slice(BEARER.length) };
}

async function readAtMost(request: Request, limit: number): Promise<Uint8Array | "too-long"> {
  if (request.body === null) return new Uint8Array(0);
  const reader = request.body.getReader();
  const chunks: Uint8Array[] = [];
  let size = 0;
  for (;;) {
    const { done: ended, value } = await reader.read();
    if (ended) break;
    size += value.byteLength;
    if (size > limit) {
      await reader.cancel();
      return "too-long";
    }
    chunks.push(value);
  }
  const bytes = new Uint8Array(size);
  let at = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, at);
    at += chunk.byteLength;
  }
  return bytes;
}

function sameBytes(a: Uint8Array, b: Uint8Array): boolean {
  return a.byteLength === b.byteLength && a.every((byte, index) => byte === b[index]);
}

function parsedBody(bytes: Uint8Array): { ok: true; value: unknown } | { ok: false } {
  let value: unknown;
  try {
    value = JSON.parse(new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(bytes));
  } catch {
    return { ok: false };
  }
  return sameBytes(encoder.encode(JSON.stringify(value)), bytes) ? { ok: true, value } : { ok: false };
}

function agreesWithResults(sent: Record<string, unknown>, route: Route): boolean {
  const results = route.results as Record<string, readonly string[]>;
  const result = sent.result;
  if (typeof result !== "string" || !Object.hasOwn(results, result)) return false;
  const want = ["result", ...(results[result] ?? [])].sort();
  const got = Object.keys(sent)
    .filter((key) => sent[key] !== undefined)
    .sort();
  return want.length === got.length && want.every((key, index) => key === got[index]);
}

async function answerOf(route: AppRoute, handlers: Handlers, request: Request, env: Env, body: unknown) {
  const name = route.name;
  const headers = readAccountHeaders(request.headers);
  if (headers === null) return rejected(400, name);
  const answer = await handlers[name]({ accountId: headers.accountId, credentialHash: await credentialHash(headers.credential), body }, env);
  if (answer === null) return empty(501);
  const checked = answerSchemas[name].safeParse(answer);
  if (!checked.success) {
    log({ event: "answer-defect", route: name, status: 500, error: "schema" });
    return empty(500);
  }
  if (!agreesWithResults(checked.data, route)) {
    log({ event: "answer-defect", route: name, status: 500, error: "results" });
    return empty(500);
  }
  return new Response(JSON.stringify(checked.data), { status: 200, headers: { "content-type": JSON_TYPE } });
}

async function matched(route: Route, handlers: Handlers, request: Request, env: Env): Promise<Response> {
  if (request.url.includes("?")) return rejected(400, route.name);
  if (route.kind === "app" && readAccountHeaders(request.headers) === null) return rejected(400, route.name);
  if (route.name === "webhook") {
    const raw = await readAtMost(request, route.limit);
    if (raw === "too-long") return rejected(413, route.name);
    return handlers.webhook(raw, request.headers, env);
  }
  if (route.name === "done") {
    if ((await readAtMost(request, 0)) === "too-long") return rejected(400, route.name);
    return handlers.done();
  }
  if (!isAppRoute(route)) return rejected(404, "none");
  const schema = requestSchemas[route.name];
  if (schema === null) {
    if ((await readAtMost(request, 0)) === "too-long") return rejected(400, route.name);
    return answerOf(route, handlers, request, env, null);
  }
  if (!CONTENT_TYPE_PATTERN.test(request.headers.get("content-type") ?? "")) return rejected(415, route.name);
  const raw = await readAtMost(request, route.limit);
  if (raw === "too-long") return rejected(413, route.name);
  const parsed = parsedBody(raw);
  if (!parsed.ok) return rejected(400, route.name);
  const valid = schema.safeParse(parsed.value);
  if (!valid.success) return rejected(400, route.name);
  return answerOf(route, handlers, request, env, valid.data);
}

export function makeApp(handlers: Handlers): (request: Request, env: Env) => Promise<Response> {
  return async (request, env) => {
    const url = new URL(request.url);
    const onPath = routeTable.routes.filter((each) => each.path === url.pathname);
    if (onPath.length === 0) return rejected(404, "none");
    const route = onPath.find((each) => each.method === request.method);
    if (route === undefined) return rejected(405, "none");
    try {
      return await matched(route, handlers, request, env);
    } catch (error) {
      log({ event: "handler-error", route: route.name, status: 500, error: errorName(error) });
      return empty(500);
    }
  };
}

export default makeApp({
  "checkout-start": checkoutStart,
  status,
  "cap-get": capGet,
  "cap-set": capSet,
  manage,
  "account-delete": accountDelete,
  webhook,
  done,
});
