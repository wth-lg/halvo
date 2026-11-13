import { DurableObject } from "cloudflare:workers";
import type * as z from "zod";
import { buildSchema, pathOf, type Variant, variantOf } from "./schema.ts";
import { searchableText } from "./show.ts";

export interface Env {
  readonly STORE: DurableObjectNamespace<Store>;
}

export class Store extends DurableObject<Env> {
  constructor(ctx: DurableObjectState, env: Env) {
    super(ctx, env);
    ctx.storage.sql.exec("CREATE TABLE IF NOT EXISTS accepted (id INTEGER PRIMARY KEY AUTOINCREMENT, body TEXT NOT NULL)");
  }

  record(body: string): void {
    this.ctx.storage.sql.exec("INSERT INTO accepted (body) VALUES (?)", body);
  }

  async dump(): Promise<string[]> {
    const sql = this.ctx.storage.sql;
    const tables = sql
      .exec("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE '\\_cf\\_%' ESCAPE '\\' AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\'")
      .toArray()
      .map((row) => String(row.name));
    const rows: string[] = [];
    for (const table of tables) {
      for (const row of sql.exec(`SELECT * FROM "${table.replaceAll('"', '""')}"`).toArray()) {
        rows.push(`${table} ${searchableText(row)}`);
      }
    }
    for (const [key, value] of await this.ctx.storage.list()) rows.push(`kv ${key} ${searchableText(value)}`);
    return rows;
  }
}

export type Rule = "route" | "json" | "canonical" | "schema";

export function rejectionBody(rule: Rule): string {
  return `{"error":"${rule}"}`;
}
export const ACCEPTED_BODY = '{"ok":true}';

type Read = { readonly ok: true; readonly value: unknown } | { readonly ok: false; readonly rule: Rule };

function sameBytes(a: Uint8Array, b: Uint8Array): boolean {
  if (a.length !== b.length) return false;
  for (let index = 0; index < a.length; index += 1) if (a[index] !== b[index]) return false;
  return true;
}

export function readBody(bytes: Uint8Array, routerRule: boolean): Read {
  let value: unknown;
  try {
    value = JSON.parse(new TextDecoder().decode(bytes));
  } catch {
    return { ok: false, rule: "json" };
  }
  if (routerRule && !sameBytes(bytes, new TextEncoder().encode(JSON.stringify(value)))) {
    return { ok: false, rule: "canonical" };
  }
  return { ok: true, value };
}

const schemas = new Map<string, z.ZodType>();
function schemaOf(variant: Variant): z.ZodType {
  const key = pathOf({ ...variant, routerRule: true, leaky: false });
  let schema = schemas.get(key);
  if (schema === undefined) {
    schema = buildSchema(variant);
    schemas.set(key, schema);
  }
  return schema;
}

function answer(status: number, body: string): Response {
  return new Response(body, { status, headers: { "content-type": "application/json" } });
}

function reject(variant: Variant | null, rule: Rule, bytes: Uint8Array): Response {
  if (variant?.leaky) {
    const text = new TextDecoder().decode(bytes);
    console.log({ event: "rejected", rule, body: text });
    return answer(400, text);
  }
  const line = `{"event":"rejected","rule":"${rule}"}`;
  console.log(line);
  return answer(rule === "route" ? 404 : 400, rejectionBody(rule));
}

export default {
  async fetch(request, env): Promise<Response> {
    const path = new URL(request.url).pathname;
    const variant = request.method === "POST" ? variantOf(path) : null;
    const bytes = new Uint8Array(await request.arrayBuffer());
    if (variant === null) return reject(null, "route", bytes);
    const read = readBody(bytes, variant.routerRule);
    if (!read.ok) return reject(variant, read.rule, bytes);
    const parsed = schemaOf(variant).safeParse(read.value);
    if (!parsed.success) return reject(variant, "schema", bytes);
    await env.STORE.get(env.STORE.idFromName(path)).record(JSON.stringify(parsed.data));
    console.log('{"event":"accepted"}');
    return answer(200, ACCEPTED_BODY);
  },
} satisfies ExportedHandler<Env>;
