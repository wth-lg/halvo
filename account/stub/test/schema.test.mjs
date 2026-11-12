import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { loadSchema, keysAgree, SchemaError } from "../src/schema.mjs";

const vendored = (name) => JSON.parse(readFileSync(new URL(`../../schema/${name}`, import.meta.url), "utf8"));
const schema = loadSchema(vendored("schema.v1.json"));
const samples = vendored("samples.v1.json");
const routes = vendored("routes.v1.json");

const root = (defs) => ({ $schema: "https://json-schema.org/draft/2020-12/schema", $defs: defs });

test("every sample of the vendored samples validates against its schema", () => {
  for (const route of routes.routes.filter((r) => r.kind === "app")) {
    for (const answer of Object.values(samples.answers[route.name])) {
      assert.equal(schema.validate(route.answer, answer), true, `${route.name} ${answer.result}`);
    }
    if (route.request !== null) {
      assert.equal(schema.validate(route.request, samples.requests[route.name]), true, route.name);
    }
  }
  assert.equal(schema.validate("Rejected", samples.rejected), true);
});

test("a cap body outside the schema is refused", () => {
  const good = { cap: { amount: 1, period: "week" } };
  assert.equal(schema.validate("CapBody", good), true);
  assert.equal(schema.validate("CapBody", { cap: { amount: 100000000, period: "month" } }), true);
  const bad = [
    null,
    [],
    "x",
    {},
    { cap: null },
    { cap: [] },
    { cap: { amount: 1 } },
    { cap: { period: "week" } },
    { cap: { amount: 0, period: "week" } },
    { cap: { amount: 100000001, period: "week" } },
    { cap: { amount: 1.5, period: "week" } },
    { cap: { amount: "1", period: "week" } },
    { cap: { amount: 1, period: "year" } },
    { cap: { amount: 1, period: "week", extra: 1 } },
    { cap: { amount: 1, period: "week" }, extra: 1 },
    JSON.parse('{"cap":{"amount":1,"period":"week"},"__proto__":{}}'),
    JSON.parse('{"cap":{"amount":1,"period":"week","constructor":1}}'),
  ];
  for (const value of bad) {
    assert.equal(schema.validate("CapBody", value), false, JSON.stringify(value));
  }
});

test("an answer outside the schema is refused", () => {
  assert.equal(schema.validate("StatusAnswer", { result: "gone" }), false);
  assert.equal(schema.validate("StatusAnswer", {}), false);
  assert.equal(schema.validate("CheckoutAnswer", { result: "session", url: 5 }), false);
  assert.equal(schema.validate("CheckoutAnswer", { result: "session", url: "a".repeat(2048) }), true);
  assert.equal(schema.validate("CheckoutAnswer", { result: "session", url: "a".repeat(2049) }), false);
  assert.equal(schema.validate("CheckoutAnswer", { result: "session", url: "\u{1F600}".repeat(2048) }), true);
  const spend = { spent: 0, held: 0, period_end: 9007199254740991 };
  const cap = { amount: 1, period: "week" };
  assert.equal(schema.validate("CapGetAnswer", { result: "cap", cap, spend }), true);
  assert.equal(schema.validate("CapGetAnswer", { result: "cap", cap, spend: { ...spend, held: -1 } }), false);
  assert.equal(schema.validate("CapGetAnswer", { result: "cap", cap, spend: { ...spend, spent: 9007199254740992 } }), false);
});

test("a name that is no entry of $defs is refused", () => {
  assert.throws(() => schema.validate("constructor", {}), SchemaError);
  assert.throws(() => schema.validate("Nope", {}), SchemaError);
});

test("the items keyword applies its schema to every element", () => {
  const s = loadSchema(root({ L: { type: "array", items: { type: "integer", minimum: 0, maximum: 9 } } }));
  assert.equal(s.validate("L", [0, 9]), true);
  assert.equal(s.validate("L", [0, 10]), false);
  assert.equal(s.validate("L", {}), false);
});

test("a schema with a keyword or a value outside the closed set is refused at load", () => {
  const cases = {
    "unknown keyword": root({ A: { type: "string", pattern: "x" } }),
    "open object": root({ A: { type: "object", properties: {}, additionalProperties: true } }),
    "unsupported type": root({ A: { type: "number", minimum: 0, maximum: 1 } }),
    "type not a string": root({ A: { type: ["string"] } }),
    "enum member not a string": root({ A: { type: "string", enum: ["a", 1] } }),
    "enum not an array": root({ A: { type: "string", enum: "a" } }),
    "required not a list of names": root({ A: { type: "object", required: [1], additionalProperties: false } }),
    "properties not an object": root({ A: { type: "object", properties: [], additionalProperties: false } }),
    "property schema not an object": root({ A: { type: "object", properties: { a: true }, additionalProperties: false } }),
    "minimum not a number": root({ A: { type: "integer", minimum: "0", maximum: 1 } }),
    "maximum not a number": root({ A: { type: "integer", minimum: 0, maximum: null } }),
    "maxLength not a count": root({ A: { type: "string", maxLength: -1 } }),
    "items not a schema": root({ A: { type: "array", items: [] } }),
    "ref to a missing entry": root({ A: { $ref: "#/$defs/B" } }),
    "ref to an inherited name": root({ A: { $ref: "#/$defs/constructor" } }),
    "ref outside $defs": root({ A: { $ref: "other.json#/$defs/A" } }),
    "$schema below the root": root({ A: { $schema: "x", type: "string" } }),
    "$defs below the root": root({ A: { $defs: {}, type: "string" } }),
    "root keyword outside the set": { ...root({}), title: "x" },
    "root without $defs": { $schema: "https://json-schema.org/draft/2020-12/schema" },
    "nested unknown keyword": root({ A: { type: "object", properties: { a: { type: "string", format: "uuid" } }, additionalProperties: false } }),
    "unknown keyword inside items": root({ A: { type: "array", items: { type: "string", const: "x" } } }),
  };
  for (const [name, doc] of Object.entries(cases)) {
    assert.throws(() => loadSchema(doc), SchemaError, name);
  }
});

test("an answer agrees with results only with exactly the keys listed for its value", () => {
  const results = routes.routes.find((r) => r.name === "cap-get").results;
  const cap = { amount: 1, period: "week" };
  const spend = { spent: 0, held: 0, period_end: 0 };
  assert.equal(keysAgree(results, { result: "cap", cap, spend }), true);
  assert.equal(keysAgree(results, { result: "no-cap" }), true);
  assert.equal(keysAgree(results, { result: "cap", cap }), false);
  assert.equal(keysAgree(results, { result: "pending", cap, spend }), false);
  assert.equal(keysAgree(results, { result: "no-cap", spend }), false);
  assert.equal(keysAgree(results, { result: "toString" }), false);
  assert.equal(keysAgree(results, { result: "gone" }), false);
});
