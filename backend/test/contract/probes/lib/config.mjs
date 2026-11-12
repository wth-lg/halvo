import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const [out, root, main, extra] = process.argv.slice(2);
const p0 = JSON.parse(readFileSync(join(root, "wrangler.jsonc"), "utf8"));
const config = {
  name: "halvo-probe",
  main,
  compatibility_date: p0.compatibility_date,
  send_metrics: false,
  ...JSON.parse(extra),
};
writeFileSync(out, `${JSON.stringify(config, null, 2)}\n`);
