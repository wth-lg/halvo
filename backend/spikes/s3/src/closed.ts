
type JsonPointer = string;

export interface Finding {
  readonly at: JsonPointer;
  readonly why: string;
}

export interface ClosedReport {
  readonly objects: number;
  readonly withoutAdditionalPropertiesFalse: number;
  readonly findings: readonly Finding[];
}

type Json = { readonly [key: string]: unknown };

function isJson(value: unknown): value is Json {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

const OBJECT_KEYWORDS = ["properties", "required", "additionalProperties", "patternProperties", "propertyNames", "unevaluatedProperties"];
const VALUE_KEYWORDS = ["type", "$ref", "const", "enum", "allOf", "anyOf", "oneOf", "not", "if"];
const ONE_SCHEMA = ["items", "contains", "not", "if", "then", "else", "additionalProperties", "unevaluatedProperties", "unevaluatedItems", "propertyNames"];
const LIST_OF_SCHEMAS = ["allOf", "anyOf", "oneOf", "prefixItems"];
const MAP_OF_SCHEMAS = ["properties", "patternProperties", "$defs", "definitions", "dependentSchemas"];

function describesObject(schema: Json): boolean {
  const type = schema.type;
  if (type === "object" || (Array.isArray(type) && type.includes("object"))) return true;
  return OBJECT_KEYWORDS.some((keyword) => keyword in schema);
}

const ADDITIONAL_PROPERTIES_DOES_NOT_SEE_ACROSS_ALLOF_BRANCHES = "allOf, which the contract forbids";

export function closedReport(root: unknown): ClosedReport {
  const findings: Finding[] = [];
  let objects = 0;
  let withoutAdditionalPropertiesFalse = 0;

  const walk = (schema: unknown, at: string): void => {
    if (schema === true) {
      findings.push({ at, why: "accepts any value" });
      return;
    }
    if (!isJson(schema)) return;
    if (!describesObject(schema) && !VALUE_KEYWORDS.some((keyword) => keyword in schema)) {
      findings.push({ at, why: "accepts any value" });
    }
    if (describesObject(schema)) {
      objects += 1;
      if (schema.additionalProperties !== false) withoutAdditionalPropertiesFalse += 1;
      if (schema.additionalProperties !== false) findings.push({ at, why: "object without additionalProperties false" });
      if (isJson(schema.additionalProperties)) findings.push({ at, why: "schema-valued additionalProperties" });
      if ("patternProperties" in schema) findings.push({ at, why: "patternProperties" });
      if ("propertyNames" in schema) findings.push({ at, why: "propertyNames" });
    }
    if ("allOf" in schema) findings.push({ at, why: ADDITIONAL_PROPERTIES_DOES_NOT_SEE_ACROSS_ALLOF_BRANCHES });
    for (const keyword of ONE_SCHEMA) {
      if (isJson(schema[keyword])) walk(schema[keyword], `${at}/${keyword}`);
    }
    for (const keyword of LIST_OF_SCHEMAS) {
      const list = schema[keyword];
      if (Array.isArray(list)) for (const [index, item] of list.entries()) walk(item, `${at}/${keyword}/${index}`);
    }
    for (const keyword of MAP_OF_SCHEMAS) {
      const map = schema[keyword];
      if (isJson(map)) for (const [name, item] of Object.entries(map)) walk(item, `${at}/${keyword}/${name}`);
    }
  };

  walk(root, "");
  return { objects, withoutAdditionalPropertiesFalse, findings };
}
