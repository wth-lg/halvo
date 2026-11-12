import * as z from "zod";

export const SENTINEL = "S3-SENTINEL-not-real-data";

export const FORMS = ["extend", "intersection", "allof"] as const;
export type Form = (typeof FORMS)[number];

export const POSITIONS = ["root", "cap", "cap.period", "device"] as const;
export type Position = (typeof POSITIONS)[number];

export const OPENINGS = ["strip", "loose"] as const;
export type Opening = (typeof OPENINGS)[number];

export interface Variant {
  readonly form: Form;
  readonly open: null | { readonly at: Position; readonly as: Opening };
  readonly routerRule: boolean;
  readonly leaky: boolean;
}

export function unit(form: Form): Variant {
  return { form, open: null, routerRule: true, leaky: false };
}

export function pathOf(variant: Variant): string {
  const open = variant.open === null ? "strict" : `${variant.open.as}@${variant.open.at}`;
  return `/${variant.form}/${open}/${variant.routerRule ? "rule" : "norule"}/${variant.leaky ? "leaky" : "quiet"}`;
}

function oneOf<T extends string>(allowed: readonly T[], text: string | undefined): T | null {
  return allowed.find((item) => item === text) ?? null;
}

export function variantOf(path: string): Variant | null {
  const parts = path.split("/");
  if (parts.length !== 5 || parts[0] !== "") return null;
  const form = oneOf(FORMS, parts[1]);
  const rule = oneOf(["rule", "norule"] as const, parts[3]);
  const handler = oneOf(["leaky", "quiet"] as const, parts[4]);
  if (form === null || rule === null || handler === null) return null;
  let open: Variant["open"] = null;
  if (parts[2] !== "strict") {
    const [as, at, ...rest] = (parts[2] ?? "").split("@");
    const opening = oneOf(OPENINGS, as);
    const position = oneOf(POSITIONS, at);
    if (opening === null || position === null || rest.length > 0) return null;
    open = { at: position, as: opening };
  }
  return { form, open, routerRule: rule === "rule", leaky: handler === "leaky" };
}

type Looseness = "strict" | Opening;
type Shape = Readonly<Record<string, z.ZodType>>;

function objectOf(looseness: Looseness, shape: Shape): z.ZodType {
  switch (looseness) {
    case "strict":
      return z.strictObject(shape);
    case "strip":
      return z.object(shape);
    case "loose":
      return z.looseObject(shape);
  }
}

const platform = { platform: z.enum(["macos", "windows"]) };
const install = { install_id: z.string().regex(/^[0-9a-f]{32}$/) };

function extended(looseness: Looseness): z.ZodType {
  switch (looseness) {
    case "strict":
      return z.strictObject(platform).extend(install);
    case "strip":
      return z.object(platform).extend(install);
    case "loose":
      return z.looseObject(platform).extend(install);
  }
}

function unfoldableInExport(half: z.ZodType): z.ZodType {
  return half.describe("the platform half of the device");
}

function composed(form: Form, looseness: Looseness): z.ZodType {
  switch (form) {
    case "extend":
      return extended(looseness);
    case "intersection":
      return z.intersection(objectOf(looseness, platform), objectOf(looseness, install));
    case "allof":
      return z.intersection(unfoldableInExport(objectOf(looseness, platform)), objectOf(looseness, install));
  }
}

export const DECLARED: Readonly<Record<Position, readonly string[]>> = {
  root: ["kind", "tier", "cap", "device", "locale"],
  cap: ["amount_minor", "currency", "period"],
  "cap.period": ["unit", "count"],
  device: ["platform", "install_id"],
};

export function buildSchema(variant: Pick<Variant, "form" | "open">): z.ZodType {
  const at = (position: Position): Looseness => (variant.open?.at === position ? variant.open.as : "strict");
  const period = objectOf(at("cap.period"), {
    unit: z.enum(["day", "week", "month"]),
    count: z.int().min(1).max(12),
  });
  const cap = objectOf(at("cap"), {
    amount_minor: z.int().min(0).max(1_000_000),
    currency: z.enum(["usd", "eur"]),
    period,
  });
  const device = composed(variant.form, at("device"));
  return objectOf(at("root"), {
    kind: z.literal("cap_set"),
    tier: z.enum(["standard", "plus"]),
    cap,
    device,
    locale: z.enum(["en", "de"]).optional(),
  }).meta({ title: "Request" });
}

const IO_THAT_SHOWS_AN_OPEN_OBJECT = "input";

export function exportSchema(variant: Pick<Variant, "form" | "open">, io: "input" | "output" = IO_THAT_SHOWS_AN_OPEN_OBJECT): unknown {
  return z.toJSONSchema(buildSchema(variant), { io });
}

export function exportMapControl(): unknown {
  return z.toJSONSchema(
    z.strictObject({ kind: z.literal("cap_set"), labels: z.record(z.string(), z.int().min(0).max(9)) }).meta({ title: "Request" }),
    { io: IO_THAT_SHOWS_AN_OPEN_OBJECT },
  );
}

export function buildProbe(): z.ZodType {
  const byCount = z.strictObject({ type: z.literal("count"), count: z.int().min(1).max(9) });
  const byUnit = z.strictObject({ type: z.literal("unit"), unit: z.enum(["day", "week"]) });
  const none = z.strictObject({ type: z.literal("none") });
  return z
    .strictObject({
      choice: z.discriminatedUnion("type", [byCount, byUnit, none]),
      note: z.enum(["a", "b"]).nullable(),
      items: z.array(z.strictObject({ sku: z.enum(["x", "y"]) })).max(3),
    })
    .meta({ title: "Request" });
}

export function exportProbe(): unknown {
  return z.toJSONSchema(buildProbe(), { io: IO_THAT_SHOWS_AN_OPEN_OBJECT });
}

export const fileText = (value: unknown): string => `${JSON.stringify(value, null, 2)}\n`;

export const PROBE_BODIES: readonly string[] = [
  '{"choice":{"type":"count","count":3},"note":null,"items":[{"sku":"x"},{"sku":"y"}]}',
  '{"choice":{"type":"unit","unit":"week"},"note":"a","items":[]}',
  '{"choice":{"type":"none"},"note":"b","items":[{"sku":"y"}]}',
];

export const VALID_BODIES: readonly string[] = [
  '{"kind":"cap_set","tier":"plus","cap":{"amount_minor":2500,"currency":"usd","period":{"unit":"month","count":1}},"device":{"platform":"macos","install_id":"00112233445566778899aabbccddeeff"}}',
  '{"kind":"cap_set","tier":"standard","cap":{"amount_minor":0,"currency":"eur","period":{"unit":"week","count":12}},"device":{"platform":"windows","install_id":"ffeeddccbbaa99887766554433221100"},"locale":"de"}',
];

export const FORBIDDEN_MODALITY_NAMES: readonly string[] = [
  "screen_frame",
  "keystroke",
  "pointer",
  "foreground_app",
  "window_title",
  "system_audio",
  "microphone",
  "camera",
];
