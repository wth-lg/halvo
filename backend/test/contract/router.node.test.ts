import { createHash } from "node:crypto";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import app, { credentialHash, makeApp, readAccountHeaders } from "../../src/app.ts";
import type { AppRequest, Handlers } from "../../src/contract/routes.ts";
import { routeTable } from "../../src/contract/routes.ts";
import { testPaths } from "../../src/gateway/test-clock.ts";
import type { Env } from "../../src/types.ts";

const ACCOUNT = "0f8e2d4c-6b1a-4c3e-8d2f-1a2b3c4d5e6f";
const CREDENTIAL = "7a".repeat(32);
const HEADERS = { "halvo-account": ACCOUNT, authorization: `Bearer ${CREDENTIAL}` };
const BODY = '{"cap":{"amount":1,"period":"week"}}';
const ENV = {} as Env;
const REJECTED = '{"result":"rejected"}';
const WEBHOOK_LIMIT = routeTable.routes.find((route) => route.name === "webhook")?.limit ?? 0;

let lines: Record<string, unknown>[] = [];
beforeEach(() => {
  lines = [];
  vi.spyOn(console, "log").mockImplementation((text: unknown) => {
    lines.push(JSON.parse(String(text)));
  });
});
afterEach(() => {
  vi.restoreAllMocks();
});

function recording(answers: Partial<Record<keyof Handlers, unknown>> = {}) {
  const seen: { route: string; request?: AppRequest; raw?: Uint8Array }[] = [];
  const app = (route: string) => async (request: AppRequest) => {
    seen.push({ route, request });
    const answer = answers[route as keyof Handlers];
    if (answer instanceof Error) throw answer;
    return route in answers ? (answer as object | null) : { result: "credential-mismatch" };
  };
  const handlers: Handlers = {
    "checkout-start": app("checkout-start"),
    status: app("status"),
    "cap-get": app("cap-get"),
    "cap-set": app("cap-set"),
    manage: app("manage"),
    "account-delete": app("account-delete"),
    webhook: async (raw) => {
      seen.push({ route: "webhook", raw });
      if (answers.webhook instanceof Error) throw answers.webhook;
      return new Response(null, { status: 200 });
    },
    done: () => {
      seen.push({ route: "done" });
      if (answers.done instanceof Error) throw answers.done;
      return new Response("<p>done</p>", { status: 200, headers: { "content-type": "text/html; charset=utf-8" } });
    },
  };
  return { seen, serve: makeApp(handlers) };
}

const call = (serve: (r: Request, e: Env) => Promise<Response>, path: string, init: RequestInit = {}) =>
  serve(new Request(`http://backend.test${path}`, init), ENV);

async function expectRejected(response: Response, status: number) {
  expect(response.status).toBe(status);
  expect(await response.text()).toBe(REJECTED);
  expect([...response.headers]).toEqual([["content-type", "application/json"]]);
}

describe("the pipeline of contract 1.4", () => {
  test("step 1: an unknown path gets 404 and another method 405, both logged with the route none", async () => {
    const { serve, seen } = recording();
    await expectRejected(await call(serve, "/v1/nothing"), 404);
    await expectRejected(await call(serve, "/v1/cap", { method: "POST", headers: HEADERS }), 405);
    await expectRejected(await call(serve, "/V1/cap", { headers: HEADERS }), 404);
    expect(lines).toEqual([
      { event: "rejected", route: "none", status: 404 },
      { event: "rejected", route: "none", status: 405 },
      { event: "rejected", route: "none", status: 404 },
    ]);
    expect(seen).toEqual([]);
  });

  test("step 2: a query string gets 400, also a bare question mark", async () => {
    const { serve } = recording();
    await expectRejected(await call(serve, "/v1/subscription?", { headers: HEADERS }), 400);
    await expectRejected(await call(serve, "/v1/subscription?a=1", { headers: HEADERS }), 400);
    await expectRejected(await call(serve, "/v1/webhook?a=1", { method: "POST", body: "{}" }), 400);
    expect(lines.at(-1)).toEqual({ event: "rejected", route: "webhook", status: 400 });
  });

  test("step 3: a missing or malformed header gets 400", async () => {
    const { serve, seen } = recording();
    for (const headers of [
      { authorization: HEADERS.authorization },
      { "halvo-account": ACCOUNT },
      { ...HEADERS, "halvo-account": ACCOUNT.toUpperCase() },
      { ...HEADERS, "halvo-account": "0f8e2d4c-6b1a-1c3e-8d2f-1a2b3c4d5e6f" },
      { ...HEADERS, authorization: `bearer ${CREDENTIAL}` },
      { ...HEADERS, authorization: `Bearer ${CREDENTIAL}0` },
    ] as Record<string, string>[]) {
      await expectRejected(await call(serve, "/v1/subscription", { headers }), 400);
    }
    expect(seen).toEqual([]);
  });

  test("step 4: a body on a route that takes none gets 400, and a webhook body over its limit 413", async () => {
    const { serve, seen } = recording();
    await expectRejected(await call(serve, "/v1/manage", { method: "POST", headers: HEADERS, body: "x" }), 400);
    await expectRejected(await call(serve, "/v1/account", { method: "DELETE", headers: HEADERS, body: "{}" }), 400);
    await expectRejected(await call(serve, "/v1/webhook", { method: "POST", body: "x".repeat(WEBHOOK_LIMIT + 1) }), 413);
    const exact = await call(serve, "/v1/webhook", { method: "POST", body: "y".repeat(WEBHOOK_LIMIT) });
    expect(exact.status).toBe(200);
    expect(seen.map((each) => each.route)).toEqual(["webhook"]);
    expect(seen[0]?.raw?.byteLength).toBe(WEBHOOK_LIMIT);
  });

  test("ruling 11: the read stops at the first chunk that takes the count past the limit, cancels the stream and pulls no further chunk", async () => {
    const { serve, seen } = recording();
    let given = 0;
    let pulls = 0;
    let cancelled = false;
    const body = new ReadableStream<Uint8Array>(
      {
        pull(controller) {
          if (given >= 100) {
            controller.close();
            return;
          }
          pulls += 1;
          given += 10;
          controller.enqueue(new Uint8Array(10).fill(0x20));
        },
        cancel() {
          cancelled = true;
        },
      },
      { highWaterMark: 0 },
    );
    const response = await call(serve, "/v1/cap", { method: "PUT", headers: { ...HEADERS, "content-type": "application/json" }, body, duplex: "half" } as RequestInit);
    await expectRejected(response, 413);
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect({ given, pulls, cancelled }).toEqual({ given: 50, pulls: 5, cancelled: true });
    expect(seen).toEqual([]);
  });

  test("ruling 11 on the webhook: the read stops at the chunk of 4,096 bytes that takes the count past 16,384, cancels the stream and pulls no further chunk", async () => {
    const { serve, seen } = recording();
    let given = 0;
    let pulls = 0;
    let cancelled = false;
    const body = new ReadableStream<Uint8Array>(
      {
        pull(controller) {
          if (given >= 10 * 4096) {
            controller.close();
            return;
          }
          pulls += 1;
          given += 4096;
          controller.enqueue(new Uint8Array(4096).fill(0x20));
        },
        cancel() {
          cancelled = true;
        },
      },
      { highWaterMark: 0 },
    );
    const response = await call(serve, "/v1/webhook", { method: "POST", body, duplex: "half" } as RequestInit);
    await expectRejected(response, 413);
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(WEBHOOK_LIMIT).toBe(16_384);
    expect({ given, pulls, cancelled }).toEqual({ given: 20_480, pulls: 5, cancelled: true });
    expect(seen).toEqual([]);
  });

  test("step 5: content type, size, JSON, the byte-equal rule and the strict schema", async () => {
    const { serve, seen } = recording();
    const json = { ...HEADERS, "content-type": "application/json" };
    await expectRejected(await call(serve, "/v1/checkout", { method: "POST", headers: HEADERS, body: BODY }), 415);
    await expectRejected(await call(serve, "/v1/checkout", { method: "POST", headers: { ...json, "content-type": "application/json; charset=utf-8" }, body: BODY }), 415);
    await expectRejected(await call(serve, "/v1/checkout", { method: "POST", headers: json, body: `${BODY}${" ".repeat(10)}` }), 413);
    for (const body of ["{", '{"cap":{"period":"week","amount":1} }', '{"cap":{"amount":1.0,"period":"week"}}', '{"cap":1,"cap":{"amount":1,"period":"week"}}', '{"cap":{"amount":1,"period":"day"}}', '{"cap":{"amount":1}}', "﻿{}"]) {
      await expectRejected(await call(serve, "/v1/cap", { method: "PUT", headers: json, body }), 400);
    }
    expect(seen).toEqual([]);
    const reordered = await call(serve, "/v1/cap", { method: "PUT", headers: json, body: '{"cap":{"period":"week","amount":1}}' });
    expect(reordered.status).toBe(200);
  });

  test("step 6: the handler gets the account id, the credential hash and the validated body, and its answer is checked", async () => {
    const answers: Partial<Record<keyof Handlers, unknown>> = { "cap-set": { result: "cap", cap: { amount: 1, period: "week" }, spend: { spent: 0, held: 0, period_end: 9 } } };
    const { serve, seen } = recording(answers);
    const response = await call(serve, "/v1/cap", { method: "PUT", headers: { ...HEADERS, "content-type": "application/json" }, body: BODY });
    expect(response.status).toBe(200);
    expect(response.headers.get("content-type")).toBe("application/json");
    expect(await response.text()).toBe('{"result":"cap","cap":{"amount":1,"period":"week"},"spend":{"spent":0,"held":0,"period_end":9}}');
    expect(seen[0]?.request).toEqual({
      accountId: ACCOUNT,
      credentialHash: createHash("sha256").update(CREDENTIAL, "utf8").digest("hex"),
      body: { cap: { amount: 1, period: "week" } },
    });
    expect(lines).toEqual([]);
  });

  test("an answer that fails its schema or results, a handler that throws, and null", async () => {
    const thrown = new Error("never logged");
    thrown.name = "boom";
    const cases: [unknown, number, Record<string, unknown> | null][] = [
      [{ result: "active", extra: 1 }, 500, { event: "answer-defect", route: "status", status: 500, error: "schema" }],
      [{ result: "nope" }, 500, { event: "answer-defect", route: "status", status: 500, error: "schema" }],
      [thrown, 500, { event: "handler-error", route: "status", status: 500, error: "Error" }],
      [null, 501, null],
    ];
    for (const [answer, status, line] of cases) {
      lines = [];
      const { serve } = recording({ status: answer });
      const response = await call(serve, "/v1/subscription", { headers: HEADERS });
      expect(response.status).toBe(status);
      expect(await response.text()).toBe("");
      expect(response.headers.get("content-type")).toBeNull();
      expect(lines).toEqual(line === null ? [] : [line]);
    }
    lines = [];
    const { serve } = recording({ manage: { result: "page" } });
    expect((await call(serve, "/v1/manage", { method: "POST", headers: HEADERS })).status).toBe(500);
    expect(lines).toEqual([{ event: "answer-defect", route: "manage", status: 500, error: "results" }]);
    lines = [];
    const undefinedKey = recording({ "cap-get": { result: "no-cap", cap: undefined } });
    const dropped = await call(undefinedKey.serve, "/v1/cap", { headers: HEADERS });
    expect({ status: dropped.status, body: await dropped.text() }).toEqual({ status: 200, body: '{"result":"no-cap"}' });
    expect(lines).toEqual([]);
  });

  test("ruling 6: results applies to the value that the router sends, so A1 to A4 of the review get 500 with answer-defect results, and A5 stays the control", async () => {
    const onPrototype = Object.assign(Object.create({ url: "http://127.0.0.1:1/x" }), { result: "pending" });
    const notEnumerable = Object.defineProperty({ result: "pending" }, "url", { get: () => "http://127.0.0.1:1/x", enumerable: false });
    const manage = { path: "/v1/manage", init: { method: "POST", headers: HEADERS } };
    const cases: [string, keyof Handlers, { path: string; init: RequestInit }, unknown][] = [
      ["A1", "manage", manage, { result: "page", url: undefined }],
      ["A2", "cap-get", { path: "/v1/cap", init: { headers: HEADERS } }, { result: "cap", cap: undefined, spend: undefined }],
      ["A3", "manage", manage, onPrototype],
      ["A4", "manage", manage, notEnumerable],
      ["A5", "manage", manage, { result: "pending", url: "http://127.0.0.1:1/x" }],
    ];
    const got: unknown[] = [];
    for (const [label, route, request, answer] of cases) {
      lines = [];
      const { serve } = recording({ [route]: answer });
      const response = await call(serve, request.path, request.init);
      got.push({ label, status: response.status, body: await response.text(), lines });
    }
    expect(got).toEqual(
      cases.map(([label, route]) => ({ label, status: 500, body: "", lines: [{ event: "answer-defect", route, status: 500, error: "results" }] })),
    );
  });

  test("review 2: step 3 comes before 5, a missing content type is 415, white space after the body is 400, a trailing slash is 404", async () => {
    const { serve, seen } = recording();
    await expectRejected(await call(serve, "/v1/checkout", { method: "POST", body: new TextEncoder().encode(BODY) }), 400);
    await expectRejected(await call(serve, "/v1/checkout", { method: "POST", headers: { "content-type": "text/plain" }, body: "x".repeat(99) }), 400);
    await expectRejected(await call(serve, "/v1/checkout", { method: "POST", headers: HEADERS, body: new TextEncoder().encode(BODY) }), 415);
    await expectRejected(await call(serve, "/v1/cap", { method: "PUT", headers: { ...HEADERS, "content-type": "application/json" }, body: `${BODY}\n` }), 400);
    await expectRejected(await call(serve, "/v1/cap/", { headers: HEADERS }), 404);
    await expectRejected(await call(serve, "/v1/cap/", { method: "PUT", headers: { ...HEADERS, "content-type": "application/json" }, body: BODY }), 404);
    expect(seen).toEqual([]);
  });

  test("review 2: an extra key after result fails results, and the answer is written in the schema's order", async () => {
    const extra = recording({ manage: { result: "pending", url: "http://127.0.0.1:1/x" } });
    lines = [];
    expect((await call(extra.serve, "/v1/manage", { method: "POST", headers: HEADERS })).status).toBe(500);
    expect(lines).toEqual([{ event: "answer-defect", route: "manage", status: 500, error: "results" }]);
    const order = recording({ "cap-get": { spend: { spent: 0, held: 0, period_end: 9 }, cap: { amount: 1, period: "week" }, result: "cap" } });
    const response = await call(order.serve, "/v1/cap", { headers: HEADERS });
    expect(await response.text()).toBe('{"result":"cap","cap":{"amount":1,"period":"week"},"spend":{"spent":0,"held":0,"period_end":9}}');
  });

  test("review N2: a name getter that throws, a result getter that throws and a body stream that errors each get 500, an empty body and one handler-error line with no message", async () => {
    const nameThrows = Object.defineProperty(new Error("name-getter-message"), "name", {
      get() {
        throw new Error("name-getter-message");
      },
    });
    const resultThrows = {
      get result() {
        throw new Error("result-getter-message");
      },
    };
    const failing = () =>
      new ReadableStream<Uint8Array>({
        pull(controller) {
          controller.error(new Error("stream-message"));
        },
      });
    const json = { ...HEADERS, "content-type": "application/json" };
    const cases: [string, Partial<Record<keyof Handlers, unknown>>, string, string, RequestInit][] = [
      ["name getter", { status: nameThrows }, "status", "/v1/subscription", { headers: HEADERS }],
      ["result getter", { status: resultThrows }, "status", "/v1/subscription", { headers: HEADERS }],
      ["body stream", {}, "cap-set", "/v1/cap", { method: "PUT", headers: json, body: failing(), duplex: "half" } as RequestInit],
      ["webhook body stream", {}, "webhook", "/v1/webhook", { method: "POST", body: failing(), duplex: "half" } as RequestInit],
    ];
    const got: unknown[] = [];
    for (const [label, answers, , path, init] of cases) {
      lines = [];
      const { serve } = recording(answers);
      try {
        const response = await call(serve, path, init);
        got.push({ label, status: response.status, body: await response.text(), type: response.headers.get("content-type"), lines });
      } catch (error) {
        got.push({ label, threw: String(error) });
      }
    }
    expect(got).toEqual(
      cases.map(([label, , route]) => ({ label, status: 500, body: "", type: null, lines: [{ event: "handler-error", route, status: 500, error: "Error" }] })),
    );
    expect(JSON.stringify(got)).not.toContain("-message");
  });

  test("the webhook and the done page return the handler's Response, and a throw gives 500 with handler-error", async () => {
    const ok = recording();
    const page = await call(ok.serve, "/v1/done");
    expect(page.status).toBe(200);
    expect(await page.text()).toBe("<p>done</p>");
    await expectRejected(await call(ok.serve, "/v1/done?x"), 400);
    const thrown = new Error("never logged");
    thrown.name = "kaput";
    const failing = recording({ webhook: thrown, done: thrown });
    lines = [];
    expect((await call(failing.serve, "/v1/webhook", { method: "POST", body: "{}" })).status).toBe(500);
    expect((await call(failing.serve, "/v1/done")).status).toBe(500);
    expect(lines).toEqual([
      { event: "handler-error", route: "webhook", status: 500, error: "Error" },
      { event: "handler-error", route: "done", status: 500, error: "Error" },
    ]);
  });

  test("no app-facing route of the default app answers the stub's 501: with no bindings, each valid request reaches its handler and fails there",async () => {
    const appRoutes = routeTable.routes.filter((route) => route.kind === "app");
    expect(appRoutes.map((route) => route.name)).toEqual(["checkout-start", "status", "cap-get", "cap-set", "manage", "account-delete"]);
    for (const route of appRoutes) {
      const withBody = route.request !== null;
      lines = [];
      const response = await app(
        new Request(`http://backend.test${route.path}`, {
          method: route.method,
          headers: { ...HEADERS, ...(withBody ? { "content-type": "application/json" } : {}) },
          body: withBody ? BODY : null,
        }),
        ENV,
      );
      expect({ route: route.name, status: response.status, reached: lines.some((line) => line.event === "handler-error" && line.route === route.name) }).toEqual({
        route: route.name,
        status: 500,
        reached: true,
      });
    }
  });
});

describe("the helpers that P4's gateway shares", () => {
  test("credentialHash is the SHA-256 of the 64 characters, in lower-case hex", async () => {
    expect(await credentialHash(CREDENTIAL)).toBe(createHash("sha256").update(CREDENTIAL, "utf8").digest("hex"));
  });

  test("readAccountHeaders applies the two patterns of the route table", () => {
    expect(readAccountHeaders(new Headers(HEADERS))).toEqual({ accountId: ACCOUNT, credential: CREDENTIAL });
    expect(readAccountHeaders(new Headers({ ...HEADERS, authorization: `Bearer ${CREDENTIAL.toUpperCase()}` }))).toBeNull();
    expect(readAccountHeaders(new Headers({ authorization: HEADERS.authorization }))).toBeNull();
  });

  test("testPaths answers null outside its three POST paths, ignores a query string and rejects a bad body without an object call", async () => {
    const env = {
      ACCOUNT: {
        idFromName: () => {
          throw new Error("no object call is expected");
        },
      },
    } as unknown as Env;
    expect(await testPaths(new Request("http://t/test/seed"), env)).toBeNull();
    expect(await testPaths(new Request("http://t/test/other", { method: "POST", body: "{}" }), env)).toBeNull();
    expect(await testPaths(new Request("http://t/v1/cap", { method: "POST", body: "{}" }), env)).toBeNull();
    for (const [path, body] of [
      ["/test/clock?x=1", "{"],
      ["/test/clock", '{"account":"x","time":1}'],
      ["/test/seed", `{"account":"${ACCOUNT}","credential":"${CREDENTIAL}","subscription":"pending"}`],
      ["/test/seed", `{"account":"${ACCOUNT}","credential":"${CREDENTIAL}","subscription":"active","state":"active"}`],
      ["/test/inspect", `{"account":"${ACCOUNT}","extra":1}`],
    ] as const) {
      const response = await testPaths(new Request(`http://t${path}`, { method: "POST", body }), env);
      expect(response?.status).toBe(400);
      expect(await response?.text()).toBe(REJECTED);
      expect(response?.headers.get("content-type")).toBe("application/json");
    }
    expect(lines).toHaveLength(5);
    expect(lines.every((line) => line.event === "rejected" && line.route === "none" && line.status === 400)).toBe(true);
  });

  test("review 2: testPaths serves POST alone and refuses an unknown key of the clock, a bad credential and a negative time", async () => {
    const env = {
      ACCOUNT: {
        idFromName: () => {
          throw new Error("no object call is expected");
        },
      },
    } as unknown as Env;
    for (const method of ["PUT", "DELETE", "PATCH"]) {
      expect(await testPaths(new Request("http://t/test/seed", { method, body: "{}" }), env)).toBeNull();
    }
    lines = [];
    for (const [path, body] of [
      ["/test/clock", `{"account":"${ACCOUNT}","time":1,"extra":1}`],
      ["/test/clock", `{"account":"${ACCOUNT}","time":-1}`],
      ["/test/seed", `{"account":"${ACCOUNT}","credential":"${CREDENTIAL.toUpperCase()}","subscription":"active"}`],
      ["/test/seed", `{"account":"${ACCOUNT}","credential":"${CREDENTIAL}0","subscription":"active"}`],
      ["/test/seed", `{"account":"${ACCOUNT}","credential":"${CREDENTIAL}","subscription":"active","anchor":-5}`],
      ["/test/seed", `{"account":"${ACCOUNT}","credential":"${CREDENTIAL}","subscription":"active","time":-5}`],
      ["/test/seed", `{"account":"${ACCOUNT}","credential":"${CREDENTIAL}","subscription":"active","customer":""}`],
    ] as const) {
      const response = await testPaths(new Request(`http://t${path}`, { method: "POST", body }), env);
      expect({ body, status: response?.status }).toEqual({ body, status: 400 });
    }
    expect(lines).toHaveLength(7);
  });
});
