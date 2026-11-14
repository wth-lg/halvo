import { spawnSync } from "node:child_process";
import {
  copyFileSync,
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  realpathSync,
  symlinkSync,
} from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { cachedLicenses, installedLicenses, packagesOfEveryLockfileDocument } from "./lockfile.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const readJson = (path) => JSON.parse(readFileSync(path, "utf8"));

const TRACKERS = [
  /^@sentry(-internal)?\//,
  /^sentry/,
  /^@?bugsnag/,
  /^@?rollbar/,
  /^raygun/,
  /^@?honeybadger/,
  /^@?airbrake/,
  /^trackjs$/,
  /^@?logrocket/,
  /^@datadog\//,
  /^dd-trace$/,
  /^@?newrelic/,
  /^elastic-apm/,
  /^@elastic\/apm/,
  /^@instana\//,
  /^@dynatrace\//,
  /^@microsoft\/applicationinsights/,
  /^applicationinsights$/,
  /^@?posthog/,
  /^@?mixpanel/,
  /^@?amplitude/,
  /^@segment\//,
  /^@rudderstack\//,
  /^@vercel\/(analytics|speed-insights)$/,
  /^@?plausible/,
  /^@fullstory\//,
  /^@hotjar\//,
  /^@?countly/,
  /^@firebase\/(analytics|crashlytics)/,
  /^@backtrace\//,
  /^backtrace-node$/,
  /crash-?report/,
  /crashpad/,
  /(^|[/-])telemetry($|-)/,
  /analytics/,
  /error-?reporting/,
  /^@appsignal\//,
  /^@?matomo/,
  /^toucan-js$/,
  /^raven(-js)?$/,
  /^@highlight-run\//,
  /^highlight\.run$/,
  /^@opentelemetry\/exporter-/,
  /^@microlabs\/otel-cf-workers$/,
  /^rudder-sdk-js$/,
  /^react-ga4$/,
  /^@snowplow\//,
  /^heap-api$/,
  /^statsig-node$/,
  /^@grafana\/faro-/,
  /^@exceptionless\//,
  /^stackdriver-errors-js$/,
  /^@umami\//,
  /^appsignal$/,
  /^hot-shots$/,
  /^datadog-metrics$/,
  /^@telemetrydeck\//,
  /^atatus-nodejs$/,
  /^@hyperdx\//,
  /^@axiomhq\//,
  /^@logtail\//,
  /^@baselime\//,
  /^keen-tracking$/,
  /^clarity-js$/,
  /^smartlook-client$/,
  /^@lumigo\//,
  /^lightstep-tracer$/,
  /^honeycomb-beeline$/,
];

export function permits(spdxExpression, permitted) {
  const tokens = spdxExpression.replace(/([()])/g, " $1 ").trim().split(/\s+/);
  let position = 0;
  const peek = () => tokens[position];
  const take = () => tokens[position++];
  const term = () => {
    if (peek() === "(") {
      take();
      const inner = either();
      if (take() !== ")") throw new Error("unbalanced");
      return inner;
    }
    let id = take();
    if (id === undefined || [")", "OR", "AND", "WITH"].includes(id)) throw new Error("no license");
    if (peek() === "WITH") {
      take();
      id = `${id} WITH ${take()}`;
    }
    return permitted.has(id);
  };
  const both = () => {
    let ok = term();
    while (peek() === "AND") {
      take();
      const next = term();
      ok = ok && next;
    }
    return ok;
  };
  const either = () => {
    let ok = both();
    while (peek() === "OR") {
      take();
      const next = both();
      ok = ok || next;
    }
    return ok;
  };
  try {
    const ok = either();
    return ok && position === tokens.length;
  } catch {
    return false;
  }
}

const names = (exception) =>
  typeof exception.package === "string" &&
  !exception.package.includes("*") &&
  Array.isArray(exception.versions) &&
  exception.versions.length > 0 &&
  exception.versions.every((version) => typeof version === "string" && version !== "");
const covers = (exception, entry) => names(exception) && exception.package === entry.name && exception.versions.includes(entry.version);

export function evaluate({ packages, untied, bundled, allowlist, exceptions }) {
  const problems = [];
  const permitted = new Set(allowlist);
  if (packages.length === 0) problems.push("the lockfile lists no package");
  const unread = packages.filter((entry) => entry.license === undefined);
  if (unread.length > 0) {
    problems.push(
      `read the license of ${packages.length - unread.length} of the ${packages.length} packages the lockfile lists; ` +
        `not read: ${unread.map((entry) => entry.id).join(", ")}. Run sh scripts/prerequisites.sh.`,
    );
  }
  for (const id of untied) {
    problems.push(
      `the cached license of ${id} does not carry the lockfile's integrity for that package, or was written under another reading of a list of licenses. Run sh scripts/prerequisites.sh.`,
    );
  }
  for (const exception of exceptions) {
    if (!exception.package || !exception.license || !exception.reason) {
      problems.push(`an exception lacks its package, license or reason: ${JSON.stringify(exception)}`);
    }
    if (!names(exception)) {
      problems.push(`an exception does not name one package by its exact name, without *, and its versions: ${JSON.stringify(exception)}`);
    }
  }
  const used = new Set();
  for (const entry of packages) {
    if (TRACKERS.some((pattern) => pattern.test(entry.name))) {
      problems.push(`${entry.id} is an error-tracking, analytics or crash-upload package`);
    }
    if (entry.license === undefined || permits(entry.license, permitted)) continue;
    const exception = exceptions.find((each) => covers(each, entry) && each.license === entry.license);
    if (exception) used.add(`${exceptions.indexOf(exception)}@${entry.version}`);
    else problems.push(`${entry.id} has the license ${entry.license}, which is neither on the allowlist nor in the exceptions`);
  }
  for (const [index, exception] of exceptions.entries()) {
    for (const version of names(exception) ? exception.versions : []) {
      if (!used.has(`${index}@${version}`)) {
        problems.push(`the exception for ${exception.package}@${version} (${exception.license}) covers no lockfile package: remove it`);
      }
    }
  }
  const byId = new Map(packages.map((entry) => [entry.id, entry]));
  for (const id of bundled) {
    const entry = byId.get(id);
    if (!entry) problems.push(`the bundle holds ${id}, which the lockfile does not list`);
    else if (entry.license === undefined || !permits(entry.license, permitted)) {
      problems.push(
        `the bundled package ${id} has the license ${entry.license}, which is not on the allowlist; ` +
          "an exception covers packages that are not bundled",
      );
    }
  }
  const excepted = packages.filter(
    (entry) => entry.license !== undefined && !permits(entry.license, permitted),
  ).length;
  const cached = packages.filter((entry) => entry.cached).length;
  return { problems, packages: packages.length, excepted, bundled: bundled.length, cached };
}

const NODE_MODULES = "/node_modules/";

function packageRootOf(real) {
  const at = real.lastIndexOf(NODE_MODULES);
  const parts = real.slice(at + NODE_MODULES.length).split("/");
  return join(real.slice(0, at + NODE_MODULES.length), ...parts.slice(0, parts[0].startsWith("@") ? 2 : 1));
}

function bundledPackages(checkout) {
  if (!process.env.HALVO_WRANGLER_CONFIG_HOME) {
    throw new Error(
      "scripts/env.sh did not set the switches of wrangler: start this check with sh gate/checks/85-licenses.sh, or with sh scripts/with-env.sh node gate/lib/licenses.mjs",
    );
  }
  const state = process.env.GATE_STATE ?? join(root, ".tmp");
  mkdirSync(state, { recursive: true });
  const out = mkdtempSync(join(state, "bundle-"));
  const meta = join(out, "meta.json");
  const run = spawnSync(
    join(root, "node_modules/.bin/wrangler"),
    ["deploy", "--dry-run", "--config", join(checkout, "wrangler.jsonc"), "--outdir", out, "--metafile", meta],
    {
      cwd: checkout,
      encoding: "utf8",
      env: { ...process.env, XDG_CONFIG_HOME: process.env.HALVO_WRANGLER_CONFIG_HOME },
    },
  );
  console.log(`${run.stdout ?? ""}${run.stderr ?? ""}`.trimEnd());
  if (run.status !== 0 || !existsSync(meta)) {
    throw new Error(`wrangler deploy --dry-run did not build the bundle (exit ${run.status})`);
  }
  const extra = readdirSync(out).filter((name) => !["index.js", "index.js.map", "meta.json", "README.md"].includes(name));
  if (extra.length > 0) {
    throw new Error(`the bundle carries modules that the metafile does not list (${extra.join(", ")}); extend this check before shipping them`);
  }
  const inputs = Object.keys(readJson(meta).inputs);
  const ids = new Set();
  for (const input of inputs) {
    const path = [input, input.replace(/^[\w-]+:/, "")].map((each) => resolve(checkout, each)).find(existsSync);
    if (!path) {
      console.log(`  bundle input without a file: ${input}`);
      continue;
    }
    const real = realpathSync(path);
    if (!real.includes(NODE_MODULES)) continue;
    const manifest = readJson(join(packageRootOf(real), "package.json"));
    ids.add(`${manifest.name}@${manifest.version}`);
  }
  console.log(`  bundle inputs: ${inputs.length}; packages among them: ${[...ids].join(", ") || "none"}`);
  return [...ids].sort();
}

function checkoutAt(checkout) {
  const packages = packagesOfEveryLockfileDocument(readFileSync(join(checkout, "pnpm-lock.yaml"), "utf8"));
  const installed = installedLicenses(checkout);
  const cache = cachedLicenses(checkout, packages);
  for (const entry of packages) {
    if (installed.has(entry.id)) entry.license = installed.get(entry.id) ?? "NONE";
    else if (cache.licenses.has(entry.id)) {
      entry.license = cache.licenses.get(entry.id) ?? "NONE";
      entry.cached = true;
    }
  }
  return {
    packages,
    untied: cache.untied,
    bundled: bundledPackages(checkout),
    exceptions: readJson(join(checkout, "gate/license-exceptions.json")),
  };
}

function fixture(path) {
  const data = readJson(path);
  const packages = data.packages.map((entry) => ({
    ...entry,
    name: entry.id.slice(0, entry.id.lastIndexOf("@")),
    version: entry.id.slice(entry.id.lastIndexOf("@") + 1),
  }));
  return {
    expect: data.expect,
    input: { packages, untied: data.untied ?? [], bundled: data.bundled ?? [], exceptions: data.exceptions ?? [] },
  };
}

function fixtures(directory) {
  return readdirSync(directory, { withFileTypes: true })
    .filter((entry) => entry.isDirectory() || entry.name.endsWith(".json"))
    .map((entry) => {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) {
        return { name: entry.name, expect: readJson(join(path, "expect.json")), read: () => checkoutAt(path) };
      }
      const { expect, input } = fixture(path);
      return { name: basename(entry.name, ".json"), expect, read: () => input };
    })
    .sort((left, right) => (left.name < right.name ? -1 : 1));
}

function realFlowOnACopyOf(checkout) {
  const state = process.env.GATE_STATE ?? join(root, ".tmp");
  mkdirSync(state, { recursive: true });
  const copy = mkdtempSync(join(state, "licenses-real-flow-"));
  cpSync(checkout, copy, { recursive: true, verbatimSymlinks: true });
  for (const file of ["gate/lib/licenses.mjs", "gate/lib/lockfile.mjs", "gate/license-allowlist.json"]) {
    mkdirSync(dirname(join(copy, file)), { recursive: true });
    copyFileSync(join(root, file), join(copy, file));
  }
  mkdirSync(join(copy, "node_modules/.bin"), { recursive: true });
  symlinkSync(join(root, "node_modules/.bin/wrangler"), join(copy, "node_modules/.bin/wrangler"));
  const run = spawnSync(process.execPath, [join(copy, "gate/lib/licenses.mjs")], { encoding: "utf8" });
  return { lines: (run.stdout ?? "").trimEnd().split("\n"), exit: run.status };
}

function resultOf(read) {
  let result;
  try {
    result = evaluate({ ...read(), allowlist });
  } catch (error) {
    result = { problems: [error.message] };
  }
  return { ...result, ...outcomeOf(result) };
}

const same = (left, right) => JSON.stringify(left) === JSON.stringify(right);

function isAsExpected(expect, result) {
  if (expect === "pass") return result.problems.length === 0;
  if (typeof expect === "string") return result.problems.some((problem) => problem.includes(expect));
  return Object.keys(expect).length > 0 && Object.keys(expect).every((key) => same(result[key], expect[key]));
}

function outcomeOf(result) {
  const failed = result.problems.length > 0;
  const line = failed
    ? `FAIL licenses: ${result.problems.length} problems; first: ${result.problems[0]}`
    : `PASS licenses: packages=${result.packages} exceptions=${result.excepted} bundled=${result.bundled} cached=${result.cached}`;
  return { lines: [...result.problems.map((problem) => `  ${problem}`), line], exit: failed ? 1 : 0 };
}

function report(result) {
  const { lines, exit } = outcomeOf(result);
  for (const line of lines) console.log(line);
  return exit;
}

const allowlist = readJson(join(root, "gate/license-allowlist.json"));
const [mode, argument] = process.argv.slice(2);
if (mode === "--self-test") {
  let failed = 0;
  let ran = 0;
  const directory = join(root, "gate/fixtures/licenses");
  for (const { name, expect, read } of fixtures(directory)) {
    ran += 1;
    const result = resultOf(read);
    if (!isAsExpected(expect, result)) {
      console.log(`self-test licenses: ${name}: expected ${JSON.stringify(expect)}, got ${JSON.stringify(result)}`);
      failed = 1;
    } else if (result.problems.length === 0) {
      console.log(`self-test licenses: good ${name}: PASS as expected`);
    } else {
      console.log(`self-test licenses: bad ${name}: FAIL as expected`);
      const shown = typeof expect === "string" ? [result.problems.find((problem) => problem.includes(expect))] : result.problems;
      for (const problem of shown) console.log(`  ${problem}`);
    }
  }
  const madeCheckout = join(directory, "bad-readers");
  const expected = readJson(join(madeCheckout, "expect.json"));
  const real = realFlowOnACopyOf(madeCheckout);
  if (real.exit === expected.exit && same(real.lines.slice(-expected.lines.length), expected.lines)) {
    console.log("self-test licenses: bad real-flow-on-a-copy-of-bad-readers: FAIL as expected");
    console.log(`  ${real.lines.at(-1)}`);
  } else {
    const wanted = `exit ${expected.exit} and the lines ${JSON.stringify(expected.lines)}`;
    const got = `exit ${real.exit} and the lines ${JSON.stringify(real.lines)}`;
    console.log(`self-test licenses: real-flow-on-a-copy-of-bad-readers: expected ${wanted}, got ${got}`);
    failed = 1;
  }
  const fixturesOnDisk = readdirSync(directory, { withFileTypes: true }).filter(
    (entry) => !entry.name.startsWith(".") && (entry.isDirectory() || entry.name.endsWith(".json")),
  ).length;
  if (ran !== fixturesOnDisk) {
    console.log(`self-test licenses: ${ran} fixtures ran, and gate/fixtures/licenses holds ${fixturesOnDisk} fixtures`);
    failed = 1;
  }
  process.exit(failed);
} else if (mode === "--fixture") {
  process.exit(report(evaluate({ ...fixture(argument).input, allowlist })));
} else if (mode === undefined) {
  let result;
  try {
    result = evaluate({ ...checkoutAt(root), allowlist });
  } catch (error) {
    console.log(`FAIL licenses: ${error.message}`);
    process.exit(1);
  }
  process.exit(report(result));
} else {
  console.error("usage: node gate/lib/licenses.mjs [--self-test | --fixture FILE]");
  process.exit(64);
}
