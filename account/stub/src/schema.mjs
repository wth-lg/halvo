/** Thrown when a schema holds a keyword or a value outside the closed set, or when a name is no entry of `$defs`. */
export class SchemaError extends Error {
  constructor(message) {
    super(message);
    this.name = "SchemaError";
  }
}

const own = (object, key) => Object.hasOwn(object, key);
const isObject = (value) => typeof value === "object" && value !== null && !Array.isArray(value);
const isNames = (value) => Array.isArray(value) && value.every((member) => typeof member === "string");
const TYPES = ["object", "array", "string", "integer"];

const KEYWORDS = {
  type: (value) => TYPES.includes(value),
  properties: (value) => isObject(value) && Object.values(value).every(isObject),
  required: isNames,
  additionalProperties: (value) => value === false,
  enum: isNames,
  minimum: Number.isFinite,
  maximum: Number.isFinite,
  maxLength: (value) => Number.isSafeInteger(value) && value >= 0,
  items: isObject,
  $ref: (value) => typeof value === "string",
};

const ROOT = ["$schema", "$defs"];
const REF = /^#\/\$defs\/(.+)$/;

function refName(ref, defs, at) {
  const match = REF.exec(ref);
  if (match === null || !own(defs, match[1])) {
    throw new SchemaError(`$ref at ${at}`);
  }
  return match[1];
}

function walk(node, defs, at) {
  for (const [keyword, value] of Object.entries(node)) {
    if (!own(KEYWORDS, keyword) || !KEYWORDS[keyword](value)) {
      throw new SchemaError(`${keyword} at ${at}`);
    }
  }
  if (own(node, "$ref")) refName(node.$ref, defs, at);
  if (own(node, "properties")) {
    for (const [name, child] of Object.entries(node.properties)) walk(child, defs, `${at}/properties/${name}`);
  }
  if (own(node, "items")) walk(node.items, defs, `${at}/items`);
}

function holds(node, value, defs) {
  if (own(node, "$ref") && !holds(defs[REF.exec(node.$ref)[1]], value, defs)) return false;
  if (own(node, "type")) {
    const fits = {
      object: isObject(value),
      array: Array.isArray(value),
      string: typeof value === "string",
      integer: Number.isInteger(value),
    }[node.type];
    if (!fits) return false;
  }
  if (own(node, "enum") && !node.enum.includes(value)) return false;
  if (typeof value === "number") {
    if (own(node, "minimum") && value < node.minimum) return false;
    if (own(node, "maximum") && value > node.maximum) return false;
  }
  if (typeof value === "string" && own(node, "maxLength") && [...value].length > node.maxLength) return false;
  if (Array.isArray(value) && own(node, "items") && !value.every((item) => holds(node.items, item, defs))) return false;
  if (isObject(value)) {
    const properties = own(node, "properties") ? node.properties : {};
    if (own(node, "required") && !node.required.every((name) => own(value, name))) return false;
    for (const key of Object.keys(value)) {
      if (own(properties, key)) {
        if (!holds(properties[key], value[key], defs)) return false;
      } else if (own(node, "additionalProperties")) {
        return false;
      }
    }
  }
  return true;
}

/**
 * Reads a JSON Schema 2020-12 document whose root holds `$schema` and `$defs` only and whose nodes use only
 * the keywords of the walk of contracts.md 1.1. Throws SchemaError at the first keyword or value outside that set.
 * Returns `validate(name, value)`, true when the parsed JSON `value` holds against the entry `name` of `$defs`.
 */
export function loadSchema(doc) {
  if (!isObject(doc) || !isObject(doc.$defs) || Object.keys(doc).some((key) => !ROOT.includes(key))) {
    throw new SchemaError("root at /");
  }
  const defs = doc.$defs;
  for (const [name, node] of Object.entries(defs)) {
    if (!isObject(node)) throw new SchemaError(`entry at /$defs/${name}`);
    walk(node, defs, `/$defs/${name}`);
  }
  return {
    validate(name, value) {
      if (!own(defs, name)) throw new SchemaError(`no entry ${name}`);
      return holds(defs[name], value, defs);
    },
  };
}

/** True when the keys of `answer` are exactly `result` and the keys that `results` lists for its value (contracts.md 1.3). */
export function keysAgree(results, answer) {
  if (typeof answer.result !== "string" || !own(results, answer.result)) return false;
  const want = ["result", ...results[answer.result]].sort();
  const got = Object.keys(answer).sort();
  return want.length === got.length && want.every((key, index) => key === got[index]);
}
