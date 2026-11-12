import * as z from "zod";
import { type RouteName, routeTable } from "./routes.ts";
import { definitions, requestSchemas } from "./schemas.ts";

const SCHEMA_DIALECT = "https://json-schema.org/draft/2020-12/schema";

type JsonSchema = { [key: string]: unknown };

function fileText(value: unknown): string {
  return `${JSON.stringify(value, null, 2)}\n`;
}

function schemaExport(): { $schema: string; $defs: Record<string, JsonSchema> } {
  const registry = z.registry<{ id: string }>();
  for (const [id, schema] of Object.entries(definitions)) registry.add(schema, { id });
  const { schemas } = z.toJSONSchema(registry, { io: "input", uri: (id) => `#/$defs/${id}` });
  const defs: Record<string, JsonSchema> = {};
  for (const id of Object.keys(definitions)) {
    const { $schema: _dialect, $id: _id, ...rest } = schemas[id] as JsonSchema;
    defs[id] = rest;
  }
  return { $schema: SCHEMA_DIALECT, $defs: defs };
}

export function exportSchema(): string {
  return fileText(schemaExport());
}

export function exportRoutes(): string {
  return fileText(routeTable);
}

function idOf(schema: z.ZodType): string {
  const found = Object.entries(definitions).find(([, each]) => each === schema);
  if (!found) throw new Error("a request schema is not one of the definitions of schemas.ts");
  return found[0];
}

function largestValue(node: JsonSchema, defs: Record<string, JsonSchema>): unknown {
  if (typeof node.$ref === "string") {
    const target = defs[node.$ref.replace("#/$defs/", "")];
    if (!target) throw new Error(`the export names no definition at ${node.$ref}`);
    return largestValue(target, defs);
  }
  if (Array.isArray(node.enum)) {
    return node.enum.reduce((longest: unknown, each: unknown) =>
      JSON.stringify(each).length > JSON.stringify(longest).length ? each : longest,
    );
  }
  if (node.type === "object") {
    const properties = (node.properties ?? {}) as Record<string, JsonSchema>;
    return Object.fromEntries(Object.entries(properties).map(([key, each]) => [key, largestValue(each, defs)]));
  }
  if (node.type === "integer") {
    const bounds = [node.minimum, node.maximum].filter((each): each is number => typeof each === "number");
    return bounds.reduce((longest, each) => (String(each).length > String(longest).length ? each : longest));
  }
  if (node.type === "string" && typeof node.maxLength === "number") return "x".repeat(node.maxLength);
  throw new Error(`largestBody cannot size a schema of type ${String(node.type)}`);
}

export function largestBody(routeName: RouteName): string | null {
  const schema = requestSchemas[routeName];
  if (schema === null) return null;
  const { $defs } = schemaExport();
  const root = $defs[idOf(schema)];
  if (!root) throw new Error(`the export has no definition for the request of ${routeName}`);
  return JSON.stringify(largestValue(root, $defs));
}
