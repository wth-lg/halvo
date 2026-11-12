import { readFileSync, writeFileSync } from "node:fs";
import { exportMapControl, exportProbe, exportSchema, fileText, PROBE_BODIES, VALID_BODIES } from "./src/schema.ts";

const files = {
  "schemas/extend.json": exportSchema({ form: "extend", open: null }),
  "schemas/intersection.json": exportSchema({ form: "intersection", open: null }),
  "schemas/allof.json": exportSchema({ form: "allof", open: null }),
  "schemas/control_open.json": exportSchema({ form: "extend", open: { at: "cap", as: "strip" } }),
  "schemas/control_map.json": exportMapControl(),
  "schemas/probe_union.json": exportProbe(),
  "bodies/valid.json": VALID_BODIES.map((text) => JSON.parse(text)),
  "bodies/probe_union.json": PROBE_BODIES.map((text) => JSON.parse(text)),
};

const check = process.argv.includes("--check");
let stale = 0;
for (const [name, value] of Object.entries(files)) {
  const url = new URL(`./crate/${name}`, import.meta.url);
  const text = fileText(value);
  if (!check) {
    writeFileSync(url, text);
    continue;
  }
  let committed = null;
  try {
    committed = readFileSync(url, "utf8");
  } catch {}
  if (committed !== text) {
    stale += 1;
    console.error(`export: crate/${name} differs from a fresh export`);
  }
}
process.exit(stale === 0 ? 0 : 1);
