import { createHash } from "node:crypto";

const DONE_PAGE = '<!doctype html>\n<html lang="en"><head><meta charset="utf-8"><title>Done</title></head><body><p>Done. You can close this page.</p></body></html>\n';
const JSON_TYPE = "application/json";

const hashOf = (credential) => createHash("sha256").update(credential, "ascii").digest("hex");

/**
 * Makes the stub's judge of one request from the vendored schema, route table and samples.
 * `handle({method, target, headers, body})` takes the method, the request target as received, the headers as
 * [name, value] pairs in their order and the body bytes, and returns `{status, type, body, result}`: the answer of
 * contracts.md 1.4 and 1.3, with `type` null for an empty body and `result` null for an answer that is no JSON.
 * The control functions move the scripted states and return how many accounts changed.
 */
export function makeApp({ schema, routes, samples }) {
  const patterns = new Map(Object.entries(routes.headers).map(([name, header]) => [name, new RegExp(header.pattern)]));
  const accounts = new Map();
  let notFinished = false;

  const sample = (route, result) => structuredClone(samples.answers[route][result]);
  const reply = (status, answer) => ({ status, type: JSON_TYPE, body: JSON.stringify(answer), result: answer.result });
  const reject = (status) => reply(status, structuredClone(samples.rejected));
  const capAnswer = (entry) => ({ result: "cap", cap: { ...entry.cap }, spend: { ...samples.answers["cap-get"].cap.spend } });

  function decide(name, id, hash, body) {
    const entry = accounts.get(id);
    if (entry === undefined) {
      if (name !== "checkout-start") return sample(name, "no-such-account");
      accounts.set(id, { state: "pending", hash, cap: { ...body.cap } });
      return sample(name, "session");
    }
    if (entry.hash !== hash) return sample(name, "credential-mismatch");
    if (entry.state === "pending") {
      if (name !== "checkout-start") return sample(name, "pending");
      entry.cap = { ...body.cap };
      return sample(name, "session");
    }
    switch (name) {
      case "checkout-start":
        return sample(name, "account-exists");
      case "status":
        return sample(name, entry.state);
      case "cap-get":
        return capAnswer(entry);
      case "cap-set":
        entry.cap = { ...body.cap };
        return capAnswer(entry);
      case "manage":
        return sample(name, "page");
      default:
        if (notFinished) {
          notFinished = false;
          return sample(name, "not-finished");
        }
        accounts.delete(id);
        return sample(name, "deleted");
    }
  }

  function handle({ method, target, headers, body }) {
    const header = (name) => {
      const values = headers.filter(([key]) => key.toLowerCase() === name).map(([, value]) => value);
      return values.length === 0 ? null : values.join(", ");
    };
    let url;
    try {
      url = target.startsWith("/") ? new URL(`http://127.0.0.1${target}`) : new URL(target);
    } catch {
      return reject(404);
    }
    const onPath = routes.routes.filter((route) => route.path === url.pathname);
    if (onPath.length === 0) return reject(404);
    const route = onPath.find((candidate) => candidate.method === method);
    if (route === undefined) return reject(405);
    if (target.includes("?")) return reject(400);
    if (route.kind === "app") {
      for (const name of ["halvo-account", "authorization"]) {
        const value = header(name);
        if (value === null || !patterns.get(name).test(value)) return reject(400);
      }
    }
    let parsed = null;
    if (route.request === null) {
      if (route.kind === "provider") return reject(body.length > route.limit ? 413 : 400);
      if (body.length > 0) return reject(400);
    } else {
      const type = header("content-type");
      if (type === null || !patterns.get("content-type").test(type)) return reject(415);
      if (body.length > route.limit) return reject(413);
      try {
        parsed = JSON.parse(body.toString("utf8"));
      } catch {
        return reject(400);
      }
      if (!Buffer.from(JSON.stringify(parsed), "utf8").equals(body)) return reject(400);
      if (!schema.validate(route.request, parsed)) return reject(400);
    }
    if (route.kind === "page") return { status: 200, type: "text/html; charset=utf-8", body: DONE_PAGE, result: null };
    const credential = header("authorization").slice("Bearer ".length);
    return reply(200, decide(route.name, header("halvo-account"), hashOf(credential), parsed));
  }

  const move = (from, to) => {
    let changed = 0;
    for (const [id, entry] of accounts) {
      if (!from.includes(entry.state)) continue;
      if (to === null) accounts.delete(id);
      else entry.state = to;
      changed += 1;
    }
    return changed;
  };

  return {
    handle,
    complete: () => move(["pending"], "active"),
    expire: () => move(["pending"], null),
    cancel: () => move(["active", "payment-failed"], "cancelled"),
    failPayment: () => move(["active"], "payment-failed"),
    armNotFinished: () => {
      const changed = notFinished ? 0 : 1;
      notFinished = true;
      return changed;
    },
  };
}
