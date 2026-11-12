import { createHmac } from "node:crypto";

export function sign(secret, body, t) {
  const hex = createHmac("sha256", secret).update(`${t}.${body}`, "utf8").digest("hex");
  return `t=${t},v1=${hex}`;
}
