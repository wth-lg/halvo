import { randomBytes } from "node:crypto";
import { readFileSync } from "node:fs";
import { createServer } from "node:http";
import { startCheckoutPage } from "../checkout-page/server.mjs";
import { sign } from "./sign.mjs";

export const API_VERSION = "2026-09-30.endive";
export const PERIOD_SECONDS = 2_592_000;
const BODY_LIMIT = 64 * 1024;
const DELIVERY_TIMEOUT_MS = 30_000;
const SESSION_MIN_SECONDS = 1_800;
const SESSION_MAX_SECONDS = 86_400;
const ACCOUNT_KEY = "halvo_account";
const buyer = JSON.parse(readFileSync(new URL("./buyer.json", import.meta.url), "utf8"));

function newId(prefix) {
  return `${prefix}_test_${randomBytes(12).toString("hex")}`;
}

function providerError(status, message, param) {
  return { status, body: { error: { type: "invalid_request_error", code: status === 404 ? "resource_missing" : undefined, message, param } } };
}

function address() {
  return { city: buyer.city, country: buyer.country, line1: buyer.line1, line2: buyer.line2, postal_code: buyer.postal_code, state: buyer.state };
}

function buyerOfSession() {
  return {
    customer_details: {
      address: address(),
      business_name: null,
      email: buyer.email,
      individual_name: null,
      name: buyer.name,
      phone: buyer.phone,
      tax_exempt: "none",
      tax_ids: [],
    },
    customer_email: buyer.email,
    collected_information: { business_name: null, individual_name: buyer.name, shipping_details: { address: address(), name: buyer.name } },
  };
}

function buyerOfInvoice() {
  return { customer_name: buyer.name, customer_email: buyer.email, customer_phone: buyer.phone, customer_address: address() };
}

function paramsOf(search, body, type) {
  const names = [];
  const values = new Map();
  const texts = [search.startsWith("?") ? search.slice(1) : search];
  if (body.length > 0 && (type ?? "").startsWith("application/x-www-form-urlencoded")) texts.push(body);
  for (const text of texts) {
    for (const [name, value] of new URLSearchParams(text)) {
      names.push(name);
      values.set(name, value);
    }
  }
  return { names, values };
}

async function readBody(request) {
  const chunks = [];
  let size = 0;
  for await (const chunk of request) {
    size += chunk.length;
    if (size > BODY_LIMIT) throw new Error("the request body is too large");
    chunks.push(chunk);
  }
  return Buffer.concat(chunks).toString("utf8");
}

export async function startProviderFake({ port = 0, mockUrl, apiKey, signingSecret, entries, clock = () => Date.now() }) {
  const sessions = new Map();
  const subscriptions = new Map();
  const events = new Map();
  const requests = [];
  const templates = new Map();
  let origin = "";

  const seconds = () => Math.floor(clock() / 1000);

  async function template(path) {
    if (!templates.has(path)) {
      const response = await fetch(`${mockUrl}${path}`, { headers: { authorization: `Bearer ${apiKey}` } });
      if (!response.ok) throw new Error(`stripe-mock answered ${response.status} to ${path}`);
      templates.set(path, await response.json());
    }
    return structuredClone(templates.get(path));
  }

  async function deliver(eventId) {
    const event = events.get(eventId);
    const session = sessions.get(event.sessionId);
    const t = seconds();
    let status = 0;
    try {
      const response = await fetch(`${session.entry}/v1/webhook`, {
        method: "POST",
        headers: { "content-type": "application/json; charset=utf-8", "stripe-signature": sign(signingSecret, event.body, t) },
        body: event.body,
        signal: AbortSignal.timeout(DELIVERY_TIMEOUT_MS),
      });
      await response.arrayBuffer();
      status = response.status;
    } catch {
      status = 0;
    }
    const delivery = { event: eventId, status };
    event.deliveries.push(delivery);
    return delivery;
  }

  function makeEvent(session, type, object, created) {
    const id = newId("evt");
    const envelope = {
      id,
      object: "event",
      api_version: API_VERSION,
      created,
      data: { object },
      livemode: false,
      pending_webhooks: 1,
      request: { id: null, idempotency_key: null },
      type,
    };
    events.set(id, { id, type, created, body: JSON.stringify(envelope, null, 2), deliveries: [], sessionId: session.id });
    return id;
  }

  async function emit(session, made) {
    for (const id of made) {
      if (session.held) session.queue.push(id);
      else await deliver(id);
    }
    return made;
  }

  function sessionView(session) {
    return structuredClone(session.object);
  }

  async function subscriptionObject(session, created, customer) {
    const object = await template("/v1/subscriptions/sub_halvo_template");
    const id = newId("sub");
    const item = object.items?.data?.[0] ?? {};
    Object.assign(object, {
      id,
      object: "subscription",
      customer,
      status: "active",
      metadata: { ...session.subscriptionMetadata },
      created,
      start_date: created,
      canceled_at: null,
      ended_at: null,
      cancel_at: null,
      cancel_at_period_end: false,
      livemode: false,
      latest_invoice: null,
      items: {
        ...(object.items ?? {}),
        object: "list",
        data: [{ ...item, id: newId("si"), subscription: id, current_period_start: created, current_period_end: created + PERIOD_SECONDS }],
      },
    });
    return object;
  }

  async function invoiceObject(subscription, status, created) {
    const object = await template("/v1/invoices/in_halvo_template");
    Object.assign(object, {
      id: newId("in"),
      object: "invoice",
      customer: subscription.object.customer,
      status,
      created,
      livemode: false,
      parent: {
        type: "subscription_details",
        quote_details: null,
        subscription_details: { metadata: { ...subscription.object.metadata }, subscription: subscription.object.id },
      },
      ...buyerOfInvoice(),
    });
    return object;
  }

  function completeSession(session, payment, subscriptionId, customer = newId("cus")) {
    Object.assign(session.object, {
      status: "complete",
      payment_status: payment,
      customer,
      subscription: subscriptionId,
      ...buyerOfSession(),
    });
  }

  async function pay(sessionId, created = seconds()) {
    const session = sessions.get(sessionId);
    if (session.object.status !== "open") return { result: "not-open" };
    const customer = newId("cus");
    const object = await subscriptionObject(session, created, customer);
    if (session.object.status !== "open") return { result: "not-open" };
    completeSession(session, "paid", object.id, customer);
    const subscription = { object, sessionId };
    subscriptions.set(object.id, subscription);
    const made = [
      makeEvent(session, "checkout.session.completed", sessionView(session), created),
      makeEvent(session, "customer.subscription.created", structuredClone(object), created),
      makeEvent(session, "invoice.paid", await invoiceObject(subscription, "paid", created), created),
    ];
    return { result: "paid", subscription: object.id, events: await emit(session, made) };
  }

  async function subscriptionChange(subscriptionId, from, to, invoice, type, created) {
    const subscription = subscriptions.get(subscriptionId);
    if (!from.includes(subscription.object.status)) return null;
    const session = sessions.get(subscription.sessionId);
    subscription.object.status = to;
    if (to === "canceled") Object.assign(subscription.object, { canceled_at: created, ended_at: created });
    const made = [];
    if (invoice !== null) made.push(makeEvent(session, `invoice.${invoice === "paid" ? "paid" : "payment_failed"}`, await invoiceObject(subscription, invoice, created), created));
    made.push(makeEvent(session, type, structuredClone(subscription.object), created));
    return emit(session, made);
  }

  const testCalls = {
    async lookup({ url }) {
      const found = [...sessions.values()].find((each) => each.object.url === url);
      return found ? { result: "session", session: found.id } : { result: "unknown" };
    },
    async hold({ session }) {
      const found = sessions.get(session);
      if (!found) return { result: "unknown" };
      found.held = true;
      return { result: "held" };
    },
    async release({ session }) {
      const found = sessions.get(session);
      if (!found) return { result: "unknown" };
      found.held = false;
      const queue = found.queue.splice(0);
      const deliveries = [];
      for (const id of queue) deliveries.push(await deliver(id));
      return { result: "released", deliveries };
    },
    async pay({ session, created }) {
      if (!sessions.has(session)) return { result: "unknown" };
      return pay(session, created);
    },
    async "complete-unpaid"({ session, created = seconds() }) {
      const found = sessions.get(session);
      if (!found) return { result: "unknown" };
      if (found.object.status !== "open") return { result: "not-open" };
      completeSession(found, "unpaid", null);
      const made = [makeEvent(found, "checkout.session.completed", sessionView(found), created)];
      return { result: "completed", events: await emit(found, made) };
    },
    async expire({ session, created = seconds() }) {
      const found = sessions.get(session);
      if (!found) return { result: "unknown" };
      if (found.object.status !== "open") return { result: "not-open" };
      found.object.status = "expired";
      const made = [makeEvent(found, "checkout.session.expired", sessionView(found), created)];
      return { result: "expired", events: await emit(found, made) };
    },
    async "fail-payment"({ subscription, created = seconds() }) {
      if (!subscriptions.has(subscription)) return { result: "unknown" };
      const made = await subscriptionChange(subscription, ["active"], "past_due", "open", "customer.subscription.updated", created);
      return made ? { result: "payment-failed", events: made } : { result: "not-active" };
    },
    async recover({ subscription, created = seconds() }) {
      if (!subscriptions.has(subscription)) return { result: "unknown" };
      const made = await subscriptionChange(subscription, ["past_due"], "active", "paid", "customer.subscription.updated", created);
      return made ? { result: "recovered", events: made } : { result: "not-failed" };
    },
    async cancel({ subscription, created = seconds() }) {
      if (!subscriptions.has(subscription)) return { result: "unknown" };
      const made = await subscriptionChange(subscription, ["active", "past_due"], "canceled", null, "customer.subscription.deleted", created);
      return made ? { result: "cancelled", events: made } : { result: "already-ended" };
    },
    async deliver({ events: ids }) {
      if (!Array.isArray(ids) || ids.some((id) => !events.has(id))) return { result: "unknown" };
      const deliveries = [];
      for (const id of ids) deliveries.push(await deliver(id));
      return { result: "delivered", deliveries };
    },
  };

  function pageInputs(session) {
    return page.sessionCounts().find((each) => each.id === session.pageId)?.inputs ?? null;
  }

  function stateOf(kind, id) {
    if (kind === "sessions") {
      const session = sessions.get(id);
      if (!session) return { result: "unknown" };
      const { object } = session;
      return {
        result: "session",
        id,
        account: session.accountId,
        status: object.status,
        payment_status: object.payment_status,
        customer: object.customer,
        subscription: object.subscription,
        expires_at: object.expires_at,
        url: object.url,
        entry: session.entry,
        held: session.held,
        queued: [...session.queue],
        inputs: pageInputs(session),
      };
    }
    if (kind === "subscriptions") {
      const subscription = subscriptions.get(id);
      if (!subscription) return { result: "unknown" };
      return { result: "subscription", id, status: subscription.object.status, customer: subscription.object.customer, session: subscription.sessionId };
    }
    const event = events.get(id);
    if (!event) return { result: "unknown" };
    return { result: "event", id, type: event.type, created: event.created, body: event.body, deliveries: event.deliveries };
  }

  function createRefusal(values) {
    const names = [...values.keys()];
    if (names.some((name) => /^subscription_data\[trial_(period_days|end|settings)\]/.test(name))) return "the provider fake takes no trial";
    if (names.some((name) => name.startsWith("subscription_data[billing_cycle_anchor]"))) return "the provider fake takes no billing cycle anchor";
    if (values.get("automatic_tax[enabled]") === "true") return "the provider fake takes no automatic tax";
    if (values.has("expires_at")) {
      const expiresAt = Number(values.get("expires_at"));
      if (expiresAt < seconds() + SESSION_MIN_SECONDS || expiresAt > seconds() + SESSION_MAX_SECONDS) {
        return "the provider fake takes an expiry from 30 minutes to 24 hours after the call only";
      }
    }
    return null;
  }

  async function ownAnswer(method, path, values, mocked) {
    if (method === "POST" && path === "/v1/checkout/sessions") {
      const successUrl = values.get("success_url") ?? "";
      let entry = null;
      try {
        entry = new URL(successUrl).origin;
      } catch {}
      if (entry === null || !entries.includes(entry)) return providerError(400, "the provider fake takes a success URL of a stack entry only", "success_url");
      const refusal = createRefusal(values);
      if (refusal !== null) return providerError(400, refusal, undefined);
      const id = newId("cs");
      const shown = page.createSession({ clientReferenceId: id });
      const subscriptionMetadata = {};
      const accountId = values.get(`subscription_data[metadata][${ACCOUNT_KEY}]`);
      if (accountId !== undefined) subscriptionMetadata[ACCOUNT_KEY] = accountId;
      const object = {
        ...mocked,
        id,
        object: "checkout.session",
        mode: values.get("mode") ?? null,
        status: "open",
        payment_status: "unpaid",
        url: shown.url,
        success_url: successUrl,
        client_reference_id: values.get("client_reference_id") ?? null,
        expires_at: Number(values.get("expires_at")),
        created: seconds(),
        customer: null,
        subscription: null,
        customer_details: null,
        customer_email: null,
        collected_information: null,
        metadata: {},
        livemode: false,
      };
      sessions.set(id, { id, object, entry, accountId: object.client_reference_id, subscriptionMetadata, pageId: shown.id, held: false, queue: [] });
      return { status: 200, body: object };
    }
    const sessionRead = /^\/v1\/checkout\/sessions\/([^/]+)$/.exec(path);
    if (method === "GET" && sessionRead) {
      const session = sessions.get(decodeURIComponent(sessionRead[1]));
      return session ? { status: 200, body: sessionView(session) } : providerError(404, `No such checkout.session: '${sessionRead[1]}'`, "session");
    }
    if (method === "POST" && path === "/v1/billing_portal/sessions") {
      const id = newId("bps");
      return {
        status: 200,
        body: { ...mocked, id, object: "billing_portal.session", customer: values.get("customer") ?? null, return_url: values.get("return_url") ?? null, url: `${origin}/manage/${id}`, livemode: false },
      };
    }
    const subscriptionPath = /^\/v1\/subscriptions\/([^/]+)$/.exec(path);
    if (subscriptionPath && (method === "GET" || method === "DELETE")) {
      const id = decodeURIComponent(subscriptionPath[1]);
      const subscription = subscriptions.get(id);
      if (!subscription) return providerError(404, `No such subscription: '${id}'`, "id");
      if (method === "GET") return { status: 200, body: structuredClone(subscription.object) };
      if (subscription.object.status === "canceled") return providerError(400, "This subscription has already been canceled.", undefined);
      const now = seconds();
      Object.assign(subscription.object, { status: "canceled", canceled_at: now, ended_at: now });
      return { status: 200, body: structuredClone(subscription.object) };
    }
    return null;
  }

  function keyOf(authorization) {
    if (typeof authorization !== "string") return null;
    if (authorization.startsWith("Bearer ")) return authorization.slice("Bearer ".length);
    if (authorization.startsWith("Basic ")) return Buffer.from(authorization.slice("Basic ".length), "base64").toString("utf8").split(":")[0];
    return null;
  }

  async function v1(request, url, raw) {
    const method = request.method;
    const path = url.pathname;
    if (raw === null) {
      const status = keyOf(request.headers.authorization) === apiKey ? 400 : 401;
      requests.push({ method, path, params: paramsOf(url.search, "", "").names, mock_status: 0, fake_status: status });
      return { status, text: JSON.stringify(providerError(status, "the request body is too large", undefined).body), type: "application/json" };
    }
    const { names, values } = paramsOf(url.search, raw, request.headers["content-type"]);
    if (keyOf(request.headers.authorization) !== apiKey) {
      const refused = providerError(401, "Invalid API Key provided", undefined);
      refused.body.error.code = undefined;
      requests.push({ method, path, params: names, mock_status: 0, fake_status: 401 });
      return { status: 401, text: JSON.stringify(refused.body), type: "application/json" };
    }
    const headers = { authorization: request.headers.authorization };
    for (const name of ["content-type", "stripe-version"]) if (request.headers[name]) headers[name] = request.headers[name];
    const forwarded = await fetch(`${mockUrl}${path}${url.search}`, { method, headers, body: method === "GET" ? undefined : raw });
    const mockText = await forwarded.text();
    const mockType = forwarded.headers.get("content-type") ?? "application/json";
    if (forwarded.status < 200 || forwarded.status >= 300) {
      requests.push({ method, path, params: names, mock_status: forwarded.status, fake_status: forwarded.status });
      return { status: forwarded.status, text: mockText, type: mockType };
    }
    let mocked = {};
    try {
      mocked = JSON.parse(mockText);
    } catch {}
    const own = await ownAnswer(method, path, values, mocked);
    const status = own ? own.status : forwarded.status;
    requests.push({ method, path, params: names, mock_status: forwarded.status, fake_status: status });
    return own ? { status, text: JSON.stringify(own.body), type: "application/json" } : { status, text: mockText, type: mockType };
  }

  async function handle(request, response) {
    const url = new URL(request.url, "http://127.0.0.1");
    const send = (status, type, text) => response.writeHead(status, { "content-type": type, "cache-control": "no-store" }).end(text);
    const sendJson = (status, value) => send(status, "application/json", JSON.stringify(value));
    if (url.pathname.startsWith("/v1/")) {
      const raw = await readBody(request).catch(() => null);
      const answer = await v1(request, url, raw);
      return send(answer.status, answer.type, answer.text);
    }
    if (request.method === "GET" && url.pathname === "/") return sendJson(200, { result: "provider-fake" });
    if (request.method === "GET" && url.pathname === "/test/requests") return sendJson(200, { result: "requests", requests });
    const state = /^\/test\/(sessions|subscriptions|events)\/([^/]+)$/.exec(url.pathname);
    if (request.method === "GET" && state) return sendJson(200, stateOf(state[1], decodeURIComponent(state[2])));
    const manage = /^\/manage\/([^/]+)$/.exec(url.pathname);
    if (request.method === "GET" && manage) {
      return send(200, "text/html; charset=utf-8", "<!doctype html><title>Manage subscription (local stand-in)</title><h1>Manage subscription</h1>");
    }
    const call = /^\/test\/([a-z-]+)$/.exec(url.pathname);
    if (request.method === "POST" && call && Object.hasOwn(testCalls, call[1])) {
      let body;
      try {
        body = JSON.parse(await readBody(request));
      } catch {
        return sendJson(400, { result: "bad-request" });
      }
      if (typeof body !== "object" || body === null || Array.isArray(body)) return sendJson(400, { result: "bad-request" });
      return sendJson(200, await testCalls[call[1]](body));
    }
    return sendJson(404, { result: "not-found" });
  }

  const page = await startCheckoutPage({
    async onPaid({ clientReferenceId }) {
      if (sessions.has(clientReferenceId)) await pay(clientReferenceId);
    },
  });
  const server = createServer((request, response) => {
    handle(request, response).catch((error) => {
      response.writeHead(500, { "content-type": "text/plain" }).end(String(error?.message ?? error));
    });
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(port, "127.0.0.1", resolve);
  });
  origin = `http://127.0.0.1:${server.address().port}`;
  return {
    origin,
    pageOrigin: page.origin,
    async close() {
      server.closeAllConnections();
      await new Promise((resolve) => server.close(resolve));
      await page.close();
    },
  };
}
