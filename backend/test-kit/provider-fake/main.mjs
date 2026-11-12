import { readFileSync } from "node:fs";
import { startProviderFake } from "./fake.mjs";

function usage(message) {
  console.error(`provider-fake: ${message}`);
  console.error("usage: node test-kit/provider-fake/main.mjs --port N --mock URL --entry URL [--entry URL] --vars stack.d/vars.env");
  process.exit(64);
}

export function localValues(path) {
  let text;
  try {
    text = readFileSync(path, "utf8");
  } catch {
    throw new Error(`${path} is missing`);
  }
  const values = {};
  for (const [index, line] of text.split("\n").entries()) {
    if (line === "" || line.startsWith("#")) continue;
    const match = /^([A-Z][A-Z0-9_]*)=([A-Za-z0-9_]+)$/.exec(line);
    if (!match) throw new Error(`stack.d/vars.env line ${index + 1} is not NAME=VALUE`);
    values[match[1]] = match[2];
  }
  return values;
}

if (import.meta.main) {
  const options = { entries: [] };
  const argv = process.argv.slice(2);
  for (let at = 0; at < argv.length; at += 2) {
    const [name, value] = [argv[at], argv[at + 1]];
    if (value === undefined) usage(`${name} needs a value`);
    if (name === "--port") options.port = Number(value);
    else if (name === "--mock") options.mockUrl = value;
    else if (name === "--entry") options.entries.push(value);
    else if (name === "--vars") options.vars = value;
    else usage(`unknown argument ${name}`);
  }
  if (!Number.isInteger(options.port) || !options.mockUrl || !options.vars || options.entries.length === 0) usage("an argument is missing");
  let values;
  try {
    values = localValues(options.vars);
  } catch (error) {
    console.error(`provider-fake: ${error.message}`);
    process.exit(1);
  }
  if (!values.PROVIDER_API_KEY || !values.WEBHOOK_SIGNING_SECRET) {
    console.error("provider-fake: stack.d/vars.env lacks PROVIDER_API_KEY or WEBHOOK_SIGNING_SECRET");
    process.exit(1);
  }
  const fake = await startProviderFake({
    port: options.port,
    mockUrl: options.mockUrl,
    apiKey: values.PROVIDER_API_KEY,
    signingSecret: values.WEBHOOK_SIGNING_SECRET,
    entries: options.entries,
  });
  console.log(`provider-fake: listening on ${fake.origin}, checkout page on ${fake.pageOrigin}`);
  const stop = () => fake.close().then(() => process.exit(0));
  process.on("SIGTERM", stop);
  process.on("SIGINT", stop);
}
