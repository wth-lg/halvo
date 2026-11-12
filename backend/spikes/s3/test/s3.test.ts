import { env, listDurableObjectIds, reset, SELF } from "cloudflare:test";
import fc from "fast-check";
import { afterAll, beforeAll, expect, test, vi } from "vitest";
import * as z from "zod";
import committedProbeBodies from "../crate/bodies/probe_union.json?raw";
import committedValidBodies from "../crate/bodies/valid.json?raw";
import committedAllof from "../crate/schemas/allof.json?raw";
import committedControlMap from "../crate/schemas/control_map.json?raw";
import committedControlOpen from "../crate/schemas/control_open.json?raw";
import committedExtend from "../crate/schemas/extend.json?raw";
import committedIntersection from "../crate/schemas/intersection.json?raw";
import committedProbe from "../crate/schemas/probe_union.json?raw";
import { closedReport, type Finding } from "../src/closed.ts";
import {
  buildProbe,
  buildSchema,
  DECLARED,
  exportMapControl,
  exportProbe,
  exportSchema,
  FORBIDDEN_MODALITY_NAMES,
  FORMS,
  type Form,
  fileText,
  OPENINGS,
  type Opening,
  POSITIONS,
  type Position,
  PROBE_BODIES,
  pathOf,
  SENTINEL,
  unit,
  VALID_BODIES,
  type Variant,
} from "../src/schema.ts";
import { searchableText } from "../src/show.ts";
import { ACCEPTED_BODY, readBody, rejectionBody } from "../src/worker.ts";

const SEED = 20261109;
const ARBITRARY_NAMES = 1000;

type Json = string | number | boolean | null | Json[] | JsonObject;
type JsonObject = { [key: string]: Json };

const PATHS: Readonly<Record<Position, readonly string[]>> = {
  root: [],
  cap: ["cap"],
  "cap.period": ["cap", "period"],
  device: ["device"],
};

function isObject(value: Json | undefined): value is JsonObject {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function defineOwnProperty(target: JsonObject, key: string, value: Json): void {
  Object.defineProperty(target, key, { value, enumerable: true, writable: true, configurable: true });
}

function editAt(node: JsonObject, path: readonly string[], edit: (object: JsonObject) => JsonObject): JsonObject {
  const [head, ...rest] = path;
  if (head === undefined) return edit(node);
  const copy: JsonObject = {};
  for (const [key, value] of Object.entries(node)) {
    defineOwnProperty(copy, key, key === head && isObject(value) ? editAt(value, rest, edit) : value);
  }
  return copy;
}

function withKey(body: JsonObject, at: Position, name: string, value: Json, first: boolean): string {
  const edited = editAt(body, PATHS[at], (object) => {
    const copy: JsonObject = {};
    if (first) defineOwnProperty(copy, name, value);
    for (const [key, member] of Object.entries(object)) defineOwnProperty(copy, key, member);
    if (!first) defineOwnProperty(copy, name, value);
    return copy;
  });
  return JSON.stringify(edited);
}

interface Member {
  readonly key: string;
  readonly value: Json;
  readonly text: string;
}

function jsonTextByHand(node: Json, at: string, edit: (at: string, members: readonly Member[]) => readonly string[]): string {
  if (!isObject(node)) return JSON.stringify(node);
  const members = Object.entries(node).map(([key, value]) => ({
    key,
    value,
    text: `${JSON.stringify(key)}:${jsonTextByHand(value, at === "root" ? key : `${at}.${key}`, edit)}`,
  }));
  return `{${edit(at, members).join(",")}}`;
}

const plain = (members: readonly Member[]): readonly string[] => members.map((member) => member.text);
const valueText = (member: Member): string => member.text.slice(JSON.stringify(member.key).length + 1);
const escaped = (text: string): string =>
  [...text].map((char) => `\\u${char.charCodeAt(0).toString(16).padStart(4, "0")}`).join("");

const FULLWIDTH_OFFSET = 0xfee0;
const LOOKALIKES: Readonly<Record<string, string>> = {
  a: "а",
  c: "с",
  e: "е",
  i: "і",
  o: "о",
  p: "р",
  s: "ѕ",
  x: "х",
  y: "у",
};
const FOLDS: readonly (readonly [string, string])[] = [
  ["k", "K"],
  ["s", "ſ"],
  ["i", "ı"],
  ["i", "İ"],
];

function variantsOf(name: string): string[] {
  const chars = [...name];
  const first = chars[0] ?? "";
  const lookalike = chars.findIndex((char) => char in LOOKALIKES);
  const variants = [
    name.toUpperCase(),
    first.toUpperCase() + name.slice(first.length),
    chars.map((char, index) => (index % 2 === 0 ? char.toUpperCase() : char)).join(""),
    chars.map((char) => (char > " " && char <= "~" ? String.fromCharCode(char.charCodeAt(0) + FULLWIDTH_OFFSET) : char)).join(""),
    lookalike < 0 ? name : chars.map((char, index) => (index === lookalike ? (LOOKALIKES[char] ?? char) : char)).join(""),
    ...FOLDS.map(([from, to]) => name.replace(from, to)),
    `${name}​`,
    `​${name}`,
    `﻿${name}`,
    `${first}‍${name.slice(first.length)}`,
    `${first}­${name.slice(first.length)}`,
    `${name}́`,
    ` ${name}`,
    `${name} `,
    `${name}\t`,
    `${name}\n`,
    `${name}\u0000`,
    name.replaceAll("_", "-"),
    name.replaceAll("_", ""),
    name.replace(/_(.)/g, (_match, char: string) => char.toUpperCase()),
  ];
  return [...new Set(variants)].filter((variant) => variant !== name);
}

interface Tally {
  cases: number;
  accepted: number;
  schema: number;
  canonical: number;
  json: number;
  other: number;
  sentinelInResponse: number;
  parsedValid: number;
}
const emptyTally = (): Tally => ({ cases: 0, accepted: 0, schema: 0, canonical: 0, json: 0, other: 0, sentinelInResponse: 0, parsedValid: 0 });

const consoleLines: string[] = [];

async function send(variant: Variant, tally: Tally, body: string | Uint8Array, headers: Record<string, string> = {}): Promise<void> {
  const response = await SELF.fetch(`http://s3.test${pathOf(variant)}`, {
    method: "POST",
    headers: { "content-type": "application/json", ...headers },
    body,
  });
  const text = await response.text();
  const seen = [String(response.status), response.statusText, ...[...response.headers].flat(), text].join("\n");
  tally.cases += 1;
  if (seen.includes(SENTINEL)) tally.sentinelInResponse += 1;
  if (response.status >= 200 && response.status < 300) tally.accepted += 1;
  else if (response.status === 400 && text === rejectionBody("schema")) tally.schema += 1;
  else if (response.status === 400 && text === rejectionBody("canonical")) tally.canonical += 1;
  else if (response.status === 400 && text === rejectionBody("json")) tally.json += 1;
  else tally.other += 1;
  if (response.status === 200 && text !== ACCEPTED_BODY) tally.other += 1;
}

const store = (variant: Variant) => env.STORE.get(env.STORE.idFromName(pathOf(variant)));
const withSentinel = (lines: readonly string[]): number => lines.filter((line) => line.includes(SENTINEL)).length;

const bodies: readonly JsonObject[] = VALID_BODIES.map((text) => JSON.parse(text) as JsonObject);
const utf8 = (text: string): Uint8Array => new TextEncoder().encode(text);

type PerPosition = Record<Position, Tally>;
const perPosition = (): PerPosition => ({ root: emptyTally(), cap: emptyTally(), "cap.period": emptyTally(), device: emptyTally() });

async function sendUnknownKeys(variant: Variant, names: (at: Position) => readonly (readonly [string, Json])[]): Promise<PerPosition> {
  const tallies = perPosition();
  for (const at of POSITIONS) {
    for (const [name, value] of names(at)) {
      if (DECLARED[at].includes(name)) continue;
      for (const body of bodies) {
        for (const first of [true, false]) {
          const text = withKey(body, at, name, value, first);
          expect(JSON.stringify(JSON.parse(text)), "an unknown-key body must be canonical").toBe(text);
          await send(variant, tallies[at], text);
        }
      }
    }
  }
  return tallies;
}

const sentinelValue = (names: readonly string[]) => (): readonly (readonly [string, Json])[] =>
  names.map((name) => [name, SENTINEL] as const);

function smuggled(at: Position): JsonObject {
  const value: JsonObject = { polluted: SENTINEL };
  for (const key of DECLARED[at]) value[key] = SENTINEL;
  return value;
}

function reencodings(member: Member): string[] {
  const key = JSON.stringify(member.key);
  if (typeof member.value === "string") return [`${key}:"${escaped(member.value.slice(0, 1))}${member.value.slice(1)}"`];
  if (typeof member.value !== "number") return [];
  const forms = [`${member.value}.0`, `${member.value}e0`, `${member.value}E0`, `${member.value}.000000000000000000001`];
  if (member.value === 0) forms.push("-0");
  return forms.map((form) => `${key}:${form}`);
}

type RouterClass = "repeat, sentinel first" | "repeat, sentinel last" | "padding" | "trailing data" | "encoding" | "not an object";

async function gzip(text: string): Promise<Uint8Array> {
  const stream = new Blob([text]).stream().pipeThrough(new CompressionStream("gzip"));
  return new Uint8Array(await new Response(stream).arrayBuffer());
}

function utf16(text: string, littleEndian: boolean, mark: boolean): Uint8Array {
  const units = mark ? `﻿${text}` : text;
  const bytes = new Uint8Array(units.length * 2);
  const view = new DataView(bytes.buffer);
  for (let index = 0; index < units.length; index += 1) view.setUint16(index * 2, units.charCodeAt(index), littleEndian);
  return bytes;
}

async function routerCases(body: JsonObject): Promise<readonly (readonly [RouterClass, string | Uint8Array, Record<string, string>?])[]> {
  const text = JSON.stringify(body);
  expect(jsonTextByHand(body, "root", (_at, members) => plain(members)), "the hand renderer must write canonical text").toBe(text);
  const cases: (readonly [RouterClass, string | Uint8Array, Record<string, string>?])[] = [];
  const sentinel = JSON.stringify(SENTINEL);

  for (const position of POSITIONS) {
    const editOne = (edit: (members: readonly Member[]) => readonly string[]): string =>
      jsonTextByHand(body, "root", (at, members) => (at === position ? edit(members) : plain(members)));
    let here: readonly Member[] = [];
    editOne((members) => {
      here = members;
      return plain(members);
    });
    for (const member of here) {
      const repeated = `${JSON.stringify(member.key)}:${sentinel}`;
      cases.push(["repeat, sentinel first", editOne((members) => [repeated, ...plain(members)])]);
      cases.push(["repeat, sentinel last", editOne((members) => [...plain(members), repeated])]);
      const swap = (text: string): string =>
        editOne((members) => members.map((other) => (other.key === member.key ? text : other.text)));
      cases.push(["encoding", swap(`"${escaped(member.key)}":${valueText(member)}`)]);
      for (const text of reencodings(member)) cases.push(["encoding", swap(text)]);
    }
  }

  const parsed: Json = JSON.parse(text);
  for (const padded of [
    ` ${text}`,
    `${text} `,
    `${text}\n`,
    `${text}\r\n`,
    `\t${text}`,
    `\n${text}`,
    JSON.stringify(parsed, null, 2),
    JSON.stringify(parsed, null, "\t"),
    text.replace(":", ": "),
    text.replace(",", ", "),
    text.replace("{", "{ "),
    `${text.slice(0, -1)} }`,
  ]) {
    cases.push(["padding", padded]);
  }
  cases.push(["padding", new Uint8Array([0xef, 0xbb, 0xbf, ...utf8(text)])]);

  for (const trailing of [`${text}\u0000`, `${text}}`, `${text}${text}`, `${text},`, `${text}\n${text}`, `/* */${text}`, `${text.slice(0, -1)},}`]) {
    cases.push(["trailing data", trailing]);
  }

  for (const littleEndian of [true, false]) {
    for (const mark of [true, false]) cases.push(["encoding", utf16(text, littleEndian, mark)]);
  }
  const bytes = utf8(text);
  cases.push(["encoding", new Uint8Array([bytes[0] ?? 0, 0xc0, 0xa2, ...bytes.slice(2)])]);
  cases.push(["encoding", new Uint8Array([...bytes.slice(0, 2), 0x80, ...bytes.slice(2)])]);
  cases.push(["encoding", await gzip(text), { "content-encoding": "gzip" }]);

  for (const other of [`[${text}]`, "[]", JSON.stringify(text), "0", "null", "true", "{}", ""]) {
    cases.push(["not an object", other]);
  }
  return cases;
}

type RouterTallies = Record<RouterClass, Tally>;

async function sendRouterCases(variant: Variant): Promise<RouterTallies> {
  const tallies: RouterTallies = {
    "repeat, sentinel first": emptyTally(),
    "repeat, sentinel last": emptyTally(),
    padding: emptyTally(),
    "trailing data": emptyTally(),
    encoding: emptyTally(),
    "not an object": emptyTally(),
  };
  const strict = buildSchema(unit(variant.form));
  for (const body of bodies) {
    for (const [klass, payload, headers] of await routerCases(body)) {
      const read = readBody(typeof payload === "string" ? utf8(payload) : payload, false);
      if (read.ok && strict.safeParse(read.value).success) tallies[klass].parsedValid += 1;
      await send(variant, tallies[klass], payload, headers);
    }
  }
  return tallies;
}

const COMMITTED_EXPORT: Readonly<Record<Form, string>> = { extend: committedExtend, intersection: committedIntersection, allof: committedAllof };

async function dumpAll(): Promise<{ instances: number; rows: string[] }> {
  const ids = await listDurableObjectIds(env.STORE);
  const rows: string[] = [];
  for (const id of ids) rows.push(...(await env.STORE.get(id).dump()));
  return { instances: ids.length, rows };
}

const measuredForJudge: Record<string, unknown> = {};
const forms: Record<string, Record<string, unknown>> = {};
const protoNamesBefore = Object.getOwnPropertyNames(Object.prototype).sort();

const realLog = console.log.bind(console);

beforeAll(() => {
  for (const method of ["log", "info", "warn", "error", "debug"] as const) {
    vi.spyOn(console, method).mockImplementation((...parts: unknown[]) => {
      consoleLines.push(parts.map(searchableText).join(" "));
    });
  }
});

afterAll(() => {
  vi.restoreAllMocks();
});

test("the runtime is workerd and Zod runs without eval", () => {
  let newFunction = "allowed";
  try {
    new Function("");
  } catch {
    newFunction = "throws";
  }
  const util: unknown = (z.core as { util?: { allowsEval?: { value?: unknown } } }).util?.allowsEval?.value;
  measuredForJudge.runtime = {
    userAgent: navigator.userAgent,
    zodAllowsEval: util ?? "not exposed",
    zodJitless: z.config().jitless ?? false,
    newFunction,
    objectPrototypeNames: protoNamesBefore,
  };
  measuredForJudge.seed = SEED;
  expect(navigator.userAgent).toBe("Cloudflare-Workers");
});

for (const form of FORMS) {
  const variant = unit(form);
  const record: Record<string, unknown> = {};
  forms[form] = record;

  test(`${form}: the unmodified bodies, the unknown keys and the router rule`, async () => {
    await reset();
    const linesBefore = consoleLines.length;

    const positive = emptyTally();
    for (const text of VALID_BODIES) await send(variant, positive, text);
    const rowsAfterPositive = (await dumpAll()).rows.length;
    record.positive = { bodies: VALID_BODIES.length, ...positive, stateRows: rowsAfterPositive };

    const prototypeNames = [...protoNamesBefore, "prototype"];
    const sampled = fc.sample(
      fc.oneof(fc.string(), fc.string({ unit: "grapheme" }), fc.string({ unit: "binary" })),
      { seed: SEED, numRuns: ARBITRARY_NAMES },
    );
    const arbitrary = [...new Set(sampled)];
    const unknown: Record<string, PerPosition> = {};
    unknown["forbidden names"] = await sendUnknownKeys(variant, sentinelValue(FORBIDDEN_MODALITY_NAMES));
    unknown["Object.prototype names"] = await sendUnknownKeys(variant, sentinelValue(prototypeNames));
    unknown["__proto__, constructor, prototype with an object value"] = await sendUnknownKeys(variant, (at) =>
      ["__proto__", "constructor", "prototype"].map((name) => [name, smuggled(at)] as const),
    );
    unknown["variants of declared keys"] = await sendUnknownKeys(variant, (at) =>
      DECLARED[at].flatMap(variantsOf).map((name) => [name, SENTINEL] as const),
    );
    unknown["variants of forbidden names"] = await sendUnknownKeys(variant, sentinelValue(FORBIDDEN_MODALITY_NAMES.flatMap(variantsOf)));
    unknown["arbitrary names"] = await sendUnknownKeys(variant, sentinelValue(arbitrary));
    record.unknown = unknown;
    record.arbitrary = { sampled: sampled.length, distinct: arbitrary.length, longest: Math.max(...arbitrary.map((name) => name.length)) };

    record.router = await sendRouterCases(variant);

    const dump = await dumpAll();
    record.state = {
      instances: dump.instances,
      rows: dump.rows.length,
      rowsAddedByCases: dump.rows.length - rowsAfterPositive,
      rowsWithSentinel: withSentinel(dump.rows),
    };
    const lines = consoleLines.slice(linesBefore);
    record.console = { lines: lines.length, linesWithSentinel: withSentinel(lines), distinctLines: [...new Set(lines)].sort() };
  });

  test(`${form}: the export`, () => {
    const fresh = exportSchema(variant);
    const report = closedReport(fresh);
    const control: Record<string, Record<string, readonly Finding[]>> = {};
    for (const at of POSITIONS) {
      const perOpening: Record<string, readonly Finding[]> = {};
      for (const as of OPENINGS) perOpening[as] = closedReport(exportSchema({ form, open: { at, as } })).findings;
      control[at] = perOpening;
    }
    record.export = {
      equalsCommitted: fileText(fresh) === COMMITTED_EXPORT[form],
      composition: ["allOf", "anyOf", "oneOf", "$ref", "unevaluatedProperties"].filter((keyword) => JSON.stringify(fresh).includes(`"${keyword}"`)),
      objects: report.objects,
      withoutAdditionalPropertiesFalse: report.withoutAdditionalPropertiesFalse,
      findings: report.findings,
      K5: control,
      outputModeFindingsForStripAtCap: closedReport(exportSchema({ form, open: { at: "cap", as: "strip" } }, "output")).findings,
    };
  });

  test(`${form}: controls K1 and K2, one open object at a time`, async () => {
    const controls: Record<string, Record<string, unknown>> = {};
    for (const as of OPENINGS) {
      const perPositionOpened: Record<string, unknown> = {};
      for (const at of POSITIONS) {
        const opened: Variant = { ...variant, open: { at, as } };
        const tallies = await sendUnknownKeys(opened, sentinelValue(FORBIDDEN_MODALITY_NAMES));
        const dump = await store(opened).dump();
        perPositionOpened[at] = {
          cases: Object.fromEntries(POSITIONS.map((position) => [position, tallies[position].cases])),
          accepted: Object.fromEntries(POSITIONS.map((position) => [position, tallies[position].accepted])),
          stateRows: dump.length,
          stateRowsWithSentinel: withSentinel(dump),
        };
      }
      controls[as satisfies Opening] = perPositionOpened;
    }
    record.K1 = controls.strip;
    record.K2 = controls.loose;
  });

  test(`${form}: control K3, the router rule switched off`, async () => {
    const tallies = await sendRouterCases({ ...variant, routerRule: false });
    record.K3 = Object.fromEntries(Object.entries(tallies).map(([klass, tally]) => [klass, { cases: tally.cases, accepted: tally.accepted, parsedValid: tally.parsedValid }]));
  });

  test(`${form}: control K4, a rejection handler that logs and returns the body`, async () => {
    const linesBefore = consoleLines.length;
    const tallies = await sendUnknownKeys({ ...variant, leaky: true }, sentinelValue(FORBIDDEN_MODALITY_NAMES));
    const total = POSITIONS.reduce((sum, position) => sum + tallies[position].cases, 0);
    record.K4 = {
      cases: total,
      accepted: POSITIONS.reduce((sum, position) => sum + tallies[position].accepted, 0),
      responsesWithSentinel: POSITIONS.reduce((sum, position) => sum + tallies[position].sentinelInResponse, 0),
      consoleLinesWithSentinel: withSentinel(consoleLines.slice(linesBefore)),
    };
  });
}

test("record what the closed check reports on its own fixtures", () => {
  const closed = { type: "object", properties: { a: { type: "string", const: "a" } }, additionalProperties: false };
  const other = { type: "object", properties: { b: { type: "string", const: "b" } }, additionalProperties: false };
  const why = (schema: unknown): string[] => closedReport(schema).findings.map((finding) => finding.why);
  measuredForJudge.walker = {
    closedObject: why(closed),
    allOfOfObjects: why({ allOf: [closed, other] }),
    allOfWithUnevaluatedPropertiesFalse: why({ allOf: [closed, other], unevaluatedProperties: false }),
    allOfOfStrings: why({ allOf: [{ type: "string", minLength: 1 }, { type: "string", maxLength: 3 }] }),
    closedByUnevaluatedPropertiesOnly: why({ type: "object", properties: { a: { type: "string", const: "a" } }, unevaluatedProperties: false }),
    nestedOpenObject: why({ ...closed, properties: { a: { type: "object", properties: {} } } }),
    patternProperties: why({ ...closed, patternProperties: { "^x": { type: "string" } } }),
    trueSchema: why({ ...closed, properties: { a: true } }),
    emptySchema: why({ ...closed, properties: { a: {} } }),
    openBranchOfAnyOf: why({ anyOf: [closed, { type: "object", properties: {} }] }),
    openDefinition: why({ ...closed, $defs: { d: { type: "object" } } }),
  };
});

test("record whether the exports of the typify controls and the bodies equal the committed files", () => {
  measuredForJudge.typifyControls = {
    openEqualsCommitted: fileText(exportSchema({ form: "extend", open: { at: "cap", as: "strip" } })) === committedControlOpen,
    mapEqualsCommitted: fileText(exportMapControl()) === committedControlMap,
  };
  measuredForJudge.bodiesEqualCommitted = fileText(VALID_BODIES.map((text) => JSON.parse(text))) === committedValidBodies;
});

type Step = string | number;

function objectPaths(node: Json, path: readonly Step[] = []): (readonly Step[])[] {
  if (Array.isArray(node)) return node.flatMap((item, index) => objectPaths(item, [...path, index]));
  if (!isObject(node)) return [];
  return [path, ...Object.entries(node).flatMap(([key, value]) => objectPaths(value, [...path, key]))];
}

function withKeyAt(node: Json, path: readonly Step[], name: string): Json {
  const [head, ...rest] = path;
  if (Array.isArray(node)) return node.map((item, index) => (index === head ? withKeyAt(item, rest, name) : item));
  if (!isObject(node)) return node;
  const copy: JsonObject = {};
  for (const [key, value] of Object.entries(node)) defineOwnProperty(copy, key, key === head ? withKeyAt(value, rest, name) : value);
  if (head === undefined) defineOwnProperty(copy, name, SENTINEL);
  return copy;
}

test("probe outside the rule: a union, a nullable field and an array of objects", () => {
  const schema = buildProbe();
  const fresh = exportProbe();
  const positions = new Set<string>();
  let accepted = 0;
  let cases = 0;
  let refused = 0;
  for (const text of PROBE_BODIES) {
    const body: Json = JSON.parse(text);
    if (schema.safeParse(body).success) accepted += 1;
    for (const path of objectPaths(body)) {
      positions.add(path.map((step) => (typeof step === "number" ? "[]" : step)).join(".") || "root");
      for (const name of ["window_title", "__proto__", "x"]) {
        cases += 1;
        const injected: unknown = JSON.parse(JSON.stringify(withKeyAt(body, path, name)));
        if (!schema.safeParse(injected).success) refused += 1;
      }
    }
  }
  measuredForJudge.probe = {
    bodies: PROBE_BODIES.length,
    accepted,
    positions: [...positions],
    unknownKeyCases: cases,
    refused,
    exportEqualsCommitted: fileText(fresh) === committedProbe && fileText(PROBE_BODIES.map((text) => JSON.parse(text))) === committedProbeBodies,
    findings: closedReport(fresh).findings,
    composition: ["allOf", "anyOf", "oneOf", "$ref", "$defs"].filter((keyword) => JSON.stringify(fresh).includes(`"${keyword}"`)),
  };
});

test("record the own names of Object.prototype after the cases", () => {
  measuredForJudge.pollution = {
    namesBefore: protoNamesBefore.length,
    namesAfter: Object.getOwnPropertyNames(Object.prototype).length,
    pollutedProbe: "polluted" in {},
  };
});

test("print the measured values", () => {
  measuredForJudge.forms = forms;
  realLog(`S3-RESULT ${JSON.stringify(measuredForJudge)}`);
});
