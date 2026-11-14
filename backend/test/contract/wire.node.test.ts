import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, test, vi } from "vitest";
import { exportSchema, largestBody } from "../../src/contract/export.ts";
import {
  answerSchemas,
  CAP_AMOUNT_MAX,
  CAP_AMOUNT_MIN,
  definitions,
  Rejected,
  requestSchemas,
  URL_MAX_LENGTH,
} from "../../src/contract/schemas.ts";

vi.setConfig({ testTimeout: 18_000 });

const ROOT = process.cwd();
const DEFS = [
  "Cap",
  "Period",
  "Spend",
  "Tier",
  "Url",
  "CapBody",
  "CheckoutAnswer",
  "StatusAnswer",
  "CapGetAnswer",
  "CapSetAnswer",
  "ManageAnswer",
  "DeleteAnswer",
  "Rejected",
];
const APP_ROUTES = ["checkout-start", "status", "cap-get", "cap-set", "manage", "account-delete"];
const RESULTS: Record<string, Record<string, string[]>> = {
  "checkout-start": { session: ["url"], "account-exists": [], "credential-mismatch": [], unavailable: [] },
  status: { pending: [], active: [], "payment-failed": [], cancelled: [], "no-such-account": [], "credential-mismatch": [] },
  "cap-get": { cap: ["cap", "spend"], "no-cap": [], pending: [], "no-such-account": [], "credential-mismatch": [] },
  "cap-set": { cap: ["cap", "spend"], pending: [], "no-such-account": [], "credential-mismatch": [] },
  manage: { page: ["url"], pending: [], "no-such-account": [], "credential-mismatch": [], unavailable: [] },
  "account-delete": { deleted: [], pending: [], "not-finished": [], "no-such-account": [], "credential-mismatch": [] },
};
const TEST_NAMES = { version: 0, names: [{ name: "window_title", sources: ["test"] }] };

type Node = { [key: string]: unknown };

const scratch = () => {
  mkdirSync(join(ROOT, ".tmp"), { recursive: true });
  return mkdtempSync(join(ROOT, ".tmp", "wire-test-"));
};

function rules(...args: string[]) {
  return spawnSync("node", [join(ROOT, "scripts/contract-rules"), ...args], { cwd: ROOT, encoding: "utf8" });
}

function walkOf(schema: unknown, names: unknown = TEST_NAMES) {
  const dir = scratch();
  writeFileSync(join(dir, "schema.json"), JSON.stringify(schema));
  writeFileSync(join(dir, "names.json"), JSON.stringify(names));
  const result = rules("walk", join(dir, "schema.json"), "--names", join(dir, "names.json"));
  return { status: result.status, lines: result.stdout.split("\n").filter((line) => line !== "") };
}

function keywordsIn(node: unknown, found: Set<string>): Set<string> {
  if (Array.isArray(node)) for (const each of node) keywordsIn(each, found);
  else if (node !== null && typeof node === "object") {
    for (const [key, value] of Object.entries(node)) {
      if (key !== "properties") found.add(key);
      if (key === "properties") for (const each of Object.values(value as Node)) keywordsIn(each, found);
      else keywordsIn(value, found);
    }
  }
  return found;
}

const committed = () => JSON.parse(readFileSync(join(ROOT, "contract/schema/v1.json"), "utf8"));
const withDef = (name: string, def: unknown) => {
  const schema = committed();
  schema.$defs[name] = def;
  return schema;
};

describe("the schema export", () => {
  test("a fresh export equals contract/schema/v1.json, and node scripts/contract-rules export schema prints it", () => {
    const text = readFileSync(join(ROOT, "contract/schema/v1.json"), "utf8");
    expect(exportSchema()).toBe(text);
    const printed = rules("export", "schema");
    expect(printed.status).toBe(0);
    expect(printed.stdout).toBe(text);
    expect(text.endsWith("}\n")).toBe(true);
    expect(text).toBe(`${JSON.stringify(JSON.parse(text), null, 2)}\n`);
  });

  test("the root holds $schema and $defs alone, with the thirteen keys in order and no union, allOf or const", () => {
    const schema = committed();
    expect(Object.keys(schema)).toEqual(["$schema", "$defs"]);
    expect(schema.$schema).toBe("https://json-schema.org/draft/2020-12/schema");
    expect(Object.keys(schema.$defs)).toEqual(DEFS);
    expect(Object.keys(definitions)).toEqual(DEFS);
    const keywords = keywordsIn(schema.$defs, new Set());
    for (const refused of ["oneOf", "anyOf", "allOf", "const", "pattern", "format"]) expect(keywords.has(refused)).toBe(false);
    expect(JSON.stringify(schema)).not.toContain('"$ref":"#/$defs/Tier"');
  });

  test("the three constants stand at their pointers, and Url holds type and maxLength alone", () => {
    const schema = committed();
    expect(schema.$defs.Cap.properties.amount.minimum).toBe(CAP_AMOUNT_MIN);
    expect(schema.$defs.Cap.properties.amount.maximum).toBe(CAP_AMOUNT_MAX);
    expect([CAP_AMOUNT_MIN, CAP_AMOUNT_MAX, URL_MAX_LENGTH]).toEqual([1, 100_000_000, 2048]);
    expect(schema.$defs.Url).toEqual({ type: "string", maxLength: URL_MAX_LENGTH });
    expect(schema.$defs.Tier.enum).toEqual(["low_cost_hosted", "frontier"]);
    expect(schema.$defs.Rejected.properties.result).toEqual({ type: "string", enum: ["rejected"] });
  });

  test("each answer's result enum is inline and holds the values of contract 1.3", () => {
    const schema = committed();
    const answerDef: Record<string, string> = {
      "checkout-start": "CheckoutAnswer",
      status: "StatusAnswer",
      "cap-get": "CapGetAnswer",
      "cap-set": "CapSetAnswer",
      manage: "ManageAnswer",
      "account-delete": "DeleteAnswer",
    };
    let values = 0;
    for (const route of APP_ROUTES) {
      const def = schema.$defs[answerDef[route] as string];
      expect(def.properties.result.enum).toEqual(Object.keys(RESULTS[route] as object));
      expect(def.required).toEqual(["result"]);
      expect(Object.keys(def.properties).filter((key) => key !== "result")).toEqual([...new Set(Object.values(RESULTS[route] ?? {}).flat())]);
      values += def.properties.result.enum.length;
    }
    expect(values).toBe(29);
  });
});

describe("the walk", () => {
  test("it passes the committed export with a names list of its own", () => {
    expect(walkOf(committed())).toEqual({ status: 0, lines: [] });
  });

  test("it refuses each kind of node, one line per node, in the order of a depth-first walk", () => {
    const cases: [string, unknown, string[]][] = [
      ["an allOf", withDef("CapBody", { allOf: [committed().$defs.CapBody, committed().$defs.Rejected] }), ["allOf at /$defs/CapBody"]],
      ["an open object", withDef("Spend", { ...committed().$defs.Spend, additionalProperties: undefined }), ["open-object at /$defs/Spend"]],
      ["additionalProperties true", withDef("Spend", { ...committed().$defs.Spend, additionalProperties: true }), ["additionalProperties at /$defs/Spend"]],
      ["a const", withDef("Rejected", { ...committed().$defs.Rejected, properties: { result: { const: "rejected" } } }), ["const at /$defs/Rejected/properties/result"]],
      ["a number", withDef("Spend", { ...committed().$defs.Spend, properties: { spent: { type: "number", minimum: 0, maximum: 1 } } }), ["unbounded-number at /$defs/Spend/properties/spent"]],
      ["an integer without maximum", withDef("Spend", { ...committed().$defs.Spend, properties: { spent: { type: "integer", minimum: 0 } } }), ["unbounded-number at /$defs/Spend/properties/spent"]],
      ["a free string", withDef("Spend", { ...committed().$defs.Spend, properties: { note: { type: "string", maxLength: 9 } } }), ["unbounded-string at /$defs/Spend/properties/note"]],
      ["a pattern", withDef("Url", { type: "string", maxLength: 9, pattern: "^x$" }), ["pattern at /$defs/Url"]],
      ["an array without items", withDef("Spend", { ...committed().$defs.Spend, properties: { list: { type: "array" } } }), ["items at /$defs/Spend/properties/list"]],
      ["a ref outside $defs", withDef("Spend", { ...committed().$defs.Spend, properties: { other: { $ref: "#/definitions/Cap" } } }), ["$ref at /$defs/Spend/properties/other"]],
      ["a third tier", withDef("Tier", { type: "string", enum: ["on_device", "low_cost_hosted", "frontier"] }), ["tier at /$defs/Tier"]],
      ["a ref to Tier", withDef("Spend", { ...committed().$defs.Spend, properties: { tier: { $ref: "#/$defs/Tier" } } }), ["tier at /$defs/Spend/properties/tier"]],
      ["a Url in a request", withDef("CapBody", { ...committed().$defs.CapBody, properties: { cap: { $ref: "#/$defs/Cap" }, url: { $ref: "#/$defs/Url" } } }), ["unbounded-string at /$defs/CapBody/properties/url"]],
      ["a forbidden request key", withDef("CapBody", { ...committed().$defs.CapBody, properties: { cap: { $ref: "#/$defs/Cap" }, window_title: { type: "string", enum: ["x"] } } }), ["forbidden-name at /$defs/CapBody/properties/window_title"]],
    ];
    const spend = (properties: object) => withDef("Spend", { ...committed().$defs.Spend, properties });
    const inner = committed();
    inner.$defs.Cap.properties.inner = { $ref: "#/$defs/Inner" };
    inner.$defs.Inner = { type: "object", properties: { window_title: { type: "string", enum: ["x"] } }, additionalProperties: false };
    cases.push(
      ["an empty schema", spend({ note: {} }), ["open-object at /$defs/Spend/properties/note"]],
      ["a type given as a list", spend({ spent: { type: ["number"] } }), ["type at /$defs/Spend/properties/spent"]],
      ["a nested $defs", withDef("Spend", { ...committed().$defs.Spend, $defs: { X: { type: "number" } } }), ["$defs at /$defs/Spend"]],
      ["a $ref to no definition", spend({ other: { $ref: "#/$defs/Nope" } }), ["$ref at /$defs/Spend/properties/other"]],
      ["items as a list", spend({ list: { type: "array", items: [{ type: "number" }] } }), ["items at /$defs/Spend/properties/list"]],
      ["properties with no type", spend({ o: { properties: { a: { type: "string", enum: ["x"] } } } }), ["open-object at /$defs/Spend/properties/o"]],
      ["properties beside an enum", spend({ o: { type: "string", enum: ["x"], properties: {} } }), ["open-object at /$defs/Spend/properties/o"]],
      ["an integer without minimum", spend({ spent: { type: "integer", maximum: 1 } }), ["unbounded-number at /$defs/Spend/properties/spent"]],
      ["a Url without maxLength", withDef("Url", { type: "string" }), ["unbounded-string at /$defs/Url"]],
      ["items that are a number", spend({ list: { type: "array", items: { type: "number" } } }), ["unbounded-number at /$defs/Spend/properties/list/items"]],
      ["a second key at the root", { ...committed(), extra: 1 }, ["extra at /"]],
      ["a forbidden name reached through two refs", inner, ["forbidden-name at /$defs/Inner/properties/window_title"]],
      [
        "items before properties in the file",
        withDef("Spend", { type: "object", items: { type: "number" }, properties: { a: { type: "number" } }, additionalProperties: false }),
        ["unbounded-number at /$defs/Spend/items", "unbounded-number at /$defs/Spend/properties/a"],
      ],
    );
    for (const [name, schema, lines] of cases) {
      expect({ name, ...walkOf(JSON.parse(JSON.stringify(schema))) }).toEqual({ name, status: 1, lines });
    }
  }, 60_000);

  test("a forbidden name in an answer schema passes, and a key and closedness on one node print the keyword", () => {
    const answer = withDef("StatusAnswer", { ...committed().$defs.StatusAnswer, properties: { result: committed().$defs.StatusAnswer.properties.result, window_title: { type: "string", enum: ["x"] } } });
    expect(walkOf(answer)).toEqual({ status: 0, lines: [] });
    const both = withDef("Spend", { type: "object", properties: {}, description: "x" });
    expect(walkOf(both)).toEqual({ status: 1, lines: ["description at /$defs/Spend"] });
  });

  test("review C6: an inline enum that holds a tier name is refused with tier at its pointer, in a request and in an answer", () => {
    const capBody = committed().$defs.CapBody;
    const inRequest = withDef("CapBody", { ...capBody, properties: { ...capBody.properties, t: { type: "string", enum: ["low_cost_hosted", "frontier"] } } });
    expect(walkOf(inRequest)).toEqual({ status: 1, lines: ["tier at /$defs/CapBody/properties/t"] });
    const status = committed().$defs.StatusAnswer;
    const inAnswer = withDef("StatusAnswer", { ...status, properties: { ...status.properties, t: { type: "string", enum: ["frontier"] } } });
    expect(walkOf(inAnswer)).toEqual({ status: 1, lines: ["tier at /$defs/StatusAnswer/properties/t"] });
  });

  test("a $ref to #/$defs/constructor is refused with $ref", () => {
    const spend = committed().$defs.Spend;
    const answer = withDef("Spend", { ...spend, properties: { ...spend.properties, other: { $ref: "#/$defs/constructor" } } });
    expect(walkOf(answer)).toEqual({ status: 1, lines: ["$ref at /$defs/Spend/properties/other"] });
    const request = withDef("CapBody", { ...committed().$defs.CapBody, properties: { cap: { $ref: "#/$defs/toString" } } });
    expect(walkOf(request)).toEqual({ status: 1, lines: ["$ref at /$defs/CapBody/properties/cap"] });
  });

  test("an enum with no type, or with a member that is no string, is refused with enum", () => {
    const spend = (spent: unknown) => withDef("Spend", { ...committed().$defs.Spend, properties: { ...committed().$defs.Spend.properties, spent } });
    const cases: [unknown, string][] = [
      [{ enum: ["x"] }, "enum at /$defs/Spend/properties/spent"],
      [{ enum: [1.5] }, "enum at /$defs/Spend/properties/spent"],
      [{ type: "string", enum: [1.5] }, "enum at /$defs/Spend/properties/spent"],
      [{ type: "string", enum: ["x", null] }, "enum at /$defs/Spend/properties/spent"],
      [{ type: "integer", minimum: 1, maximum: 2, enum: [1, 2] }, "enum at /$defs/Spend/properties/spent"],
    ];
    for (const [node, line] of cases) expect(walkOf(spend(node))).toEqual({ status: 1, lines: [line] });
    const request = withDef("CapBody", { ...committed().$defs.CapBody, properties: { cap: { enum: [{ any: "thing" }, "free", 1.5, null] } } });
    expect(walkOf(request)).toEqual({ status: 1, lines: ["enum at /$defs/CapBody/properties/cap"] });
  });

  test("without the names file and without --names the walk prints forbidden-name at / and exits 1", () => {
    const dir = scratch();
    mkdirSync(join(dir, "scripts"));
    copyFileSync(join(ROOT, "scripts/contract-rules"), join(dir, "scripts/contract-rules"));
    writeFileSync(join(dir, "schema.json"), JSON.stringify(committed()));
    const result = spawnSync("node", [join(dir, "scripts/contract-rules"), "walk", join(dir, "schema.json")], { cwd: dir, encoding: "utf8" });
    expect(result.status).toBe(1);
    expect(result.stdout).toBe("forbidden-name at /\n");
  });

  test("bad usage exits 64", () => {
    expect(rules().status).toBe(64);
    expect(rules("export", "other").status).toBe(64);
    expect(rules("walk").status).toBe(64);
  });
});

describe("the samples and the largest bodies", () => {
  test("contract/samples/v1.json holds the two requests, the 29 answers and the rejection, each read by its schema", () => {
    const text = readFileSync(join(ROOT, "contract/samples/v1.json"), "utf8");
    const samples = JSON.parse(text);
    expect(text).toBe(`${JSON.stringify(samples, null, 2)}\n`);
    expect(Object.keys(samples)).toEqual(["version", "requests", "answers", "rejected"]);
    expect(samples.version).toBe(1);
    expect(Object.keys(samples.requests)).toEqual(["checkout-start", "cap-set"]);
    for (const [route, body] of Object.entries(samples.requests)) {
      expect(body).toEqual({ cap: { amount: 1, period: "week" } });
      expect(requestSchemas[route as "checkout-start"]?.safeParse(body).success).toBe(true);
    }
    let count = 0;
    for (const route of APP_ROUTES) {
      expect(Object.keys(samples.answers[route])).toEqual(Object.keys(RESULTS[route] as object));
      for (const [result, answer] of Object.entries(samples.answers[route] as Record<string, Node>)) {
        count += 1;
        expect(answer.result).toBe(result);
        expect(Object.keys(answer)).toEqual(["result", ...((RESULTS[route] as Record<string, string[]>)[result] ?? [])]);
        if ("url" in answer) expect(answer.url).toBe("http://127.0.0.1:1/x");
        if ("cap" in answer) expect(answer.cap).toEqual({ amount: 1, period: "week" });
        if ("spend" in answer) expect(answer.spend).toEqual({ spent: 0, held: 0, period_end: 0 });
        expect(answerSchemas[route as "status"].safeParse(answer).success).toBe(true);
      }
    }
    expect(count).toBe(29);
    expect(samples.rejected).toEqual({ result: "rejected" });
    expect(Rejected.safeParse(samples.rejected).success).toBe(true);
  });

  test("largestBody gives 45 bytes for the two routes with a request, and null for the four without", () => {
    for (const route of ["checkout-start", "cap-set"] as const) {
      const body = largestBody(route);
      expect(body).toBe('{"cap":{"amount":100000000,"period":"month"}}');
      expect(new TextEncoder().encode(body ?? "").byteLength).toBe(45);
      expect(requestSchemas[route]?.safeParse(JSON.parse(body ?? "null")).success).toBe(true);
    }
    for (const route of ["status", "cap-get", "manage", "account-delete"] as const) expect(largestBody(route)).toBeNull();
  });

  test("the strict schemas refuse an unknown key at every depth and a value out of its domain", () => {
    const body = { cap: { amount: 1, period: "week" } };
    const capBody = requestSchemas["checkout-start"];
    expect(capBody?.safeParse({ ...body, x: 1 }).success).toBe(false);
    expect(capBody?.safeParse({ cap: { ...body.cap, x: 1 } }).success).toBe(false);
    for (const amount of [0, CAP_AMOUNT_MAX + 1, 1.5, "1"]) expect(capBody?.safeParse({ cap: { amount, period: "week" } }).success).toBe(false);
    expect(capBody?.safeParse({ cap: { amount: 1, period: "day" } }).success).toBe(false);
    expect(answerSchemas.manage.safeParse({ result: "page", url: "x".repeat(URL_MAX_LENGTH + 1) }).success).toBe(false);
  });
});
