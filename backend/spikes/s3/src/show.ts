
function bytesAsText(value: unknown): string | null {
  if (value instanceof ArrayBuffer) return new TextDecoder().decode(value);
  if (ArrayBuffer.isView(value)) return new TextDecoder().decode(value);
  return null;
}

function plain(value: unknown): unknown {
  if (typeof value === "bigint") return String(value);
  if (value instanceof Error) return `${value.name}: ${value.message} ${value.stack ?? ""}`;
  if (value instanceof Map || value instanceof Set) return [...value];
  return bytesAsText(value) ?? value;
}

export function searchableText(value: unknown): string {
  if (typeof value === "string") return value;
  try {
    const text = JSON.stringify(plain(value), (_key, item: unknown) => plain(item));
    return typeof text === "string" ? text : String(value);
  } catch {
    return String(value);
  }
}
