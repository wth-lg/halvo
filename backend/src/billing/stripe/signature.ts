export const SIGNATURE_HEADER = "stripe-signature";
export const SIGNATURE_TOLERANCE_SECONDS = 300;

const encoder = new TextEncoder();
const HEX_SIGNATURE = /^[0-9a-f]{64}$/;
const WHOLE_SECONDS = /^[0-9]{1,15}$/;

function bytesOfHex(hex: string): Uint8Array {
  const bytes = new Uint8Array(hex.length / 2);
  for (let at = 0; at < bytes.length; at += 1) bytes[at] = Number.parseInt(hex.slice(at * 2, at * 2 + 2), 16);
  return bytes;
}

function parsed(header: string): { timestamp: number; signatures: string[] } | null {
  let timestamp: number | null = null;
  const signatures: string[] = [];
  for (const item of header.split(",")) {
    const at = item.indexOf("=");
    if (at < 0) continue;
    const key = item.slice(0, at).trim();
    const value = item.slice(at + 1).trim();
    if (key === "t") {
      if (timestamp !== null || !WHOLE_SECONDS.test(value)) return null;
      timestamp = Number(value);
    } else if (key === "v1" && HEX_SIGNATURE.test(value)) {
      signatures.push(value);
    }
  }
  return timestamp === null || signatures.length === 0 ? null : { timestamp, signatures };
}

export async function signatureHolds(rawBody: Uint8Array, header: string | null, secret: string, now: number): Promise<boolean> {
  if (header === null || secret === "") return false;
  const signed = parsed(header);
  if (signed === null) return false;
  if (Math.abs(Math.floor(now / 1000) - signed.timestamp) > SIGNATURE_TOLERANCE_SECONDS) return false;
  const prefix = encoder.encode(`${signed.timestamp}.`);
  const message = new Uint8Array(prefix.byteLength + rawBody.byteLength);
  message.set(prefix, 0);
  message.set(rawBody, prefix.byteLength);
  const key = await crypto.subtle.importKey("raw", encoder.encode(secret), { name: "HMAC", hash: "SHA-256" }, false, ["verify"]);
  for (const signature of signed.signatures) {
    if (await crypto.subtle.verify("HMAC", key, bytesOfHex(signature), message)) return true;
  }
  return false;
}
