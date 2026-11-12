import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, symlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../..", import.meta.url)).replace(/\/$/, "");
const ALLOWED = ["extend", "intersection"];
const FLAGGED = "allof: forbidden by the contract, flagged by the walk";
const NOT_FLAGGED = "allof: forbidden by the contract, not flagged";

const BASE = { extend: "MET", intersection: "MET" };
const every = (verdict) => Object.fromEntries(ALLOWED.map((form) => [form, verdict]));
const WORKER = "spikes/s3/src/worker.ts";
const CLOSED = "spikes/s3/src/closed.ts";
const SCHEMA = "spikes/s3/src/schema.ts";
const JUDGE = "spikes/s3/judge.mjs";
const NOT_COUNTED = "rule: not decided. The run does not count: ";
const NO_ARBITRARY_NAME = `${NOT_COUNTED}${[...ALLOWED, "allof"].flatMap((form) => ["root", "cap", "cap.period", "device"].map((position) => `${form}: no body of "arbitrary names" at ${position}`)).join("; ")}.`;
const RUN_MUST_NOT_COUNT = null;

const MUTATIONS = [
  {
    name: "one z.strictObject changed to z.object, then exported again",
    file: SCHEMA,
    from: "      return z.strictObject(platform).extend(install);",
    to: "      return z.object(platform).extend(install);",
    exportAgain: true,
    want: { validator: { ...BASE, extend: "MISSED" }, typify: { ...BASE, extend: "MISSED" }, flagged: true },
  },
  {
    name: "the router rule removed",
    file: WORKER,
    from: "  if (routerRule && !sameBytes(",
    to: "  if (false && !sameBytes(",
    want: { validator: every("MISSED"), typify: BASE, flagged: true },
  },
  {
    name: "every rejection logs and returns the body",
    file: WORKER,
    from: "  if (variant?.leaky) {",
    to: "  if (variant !== null) {",
    want: { validator: every("MISSED"), typify: BASE, flagged: true },
  },
  {
    name: "a rejection also logs the body inside an object, the answer stays fixed",
    file: WORKER,
    from: "  console.log(line);",
    to: "  console.log(line, { body: new TextDecoder().decode(bytes) });",
    want: { validator: every("MISSED"), typify: BASE, flagged: true },
  },
  {
    name: "a rejection also returns the body in a header, the body of the answer stays fixed",
    file: WORKER,
    from: '  return answer(rule === "route" ? 404 : 400, rejectionBody(rule));',
    to: '  return new Response(rejectionBody(rule), { status: 400, headers: { "x-echo": encodeURIComponent(new TextDecoder().decode(bytes)).slice(0, 4000) } });',
    want: { validator: every("MISSED"), typify: BASE, flagged: true },
  },
  {
    name: "a rejected body is written to another instance of the Durable Object",
    file: WORKER,
    from: '    if (!parsed.success) return reject(variant, "schema", bytes);',
    to: '    if (!parsed.success) {\n      await env.STORE.get(env.STORE.idFromName("elsewhere")).record(new TextDecoder().decode(bytes));\n      return reject(variant, "schema", bytes);\n    }',
    want: { validator: every("MISSED"), typify: BASE, flagged: true },
  },
  {
    name: "the committed export opened at cap, the schema unchanged",
    file: "spikes/s3/crate/schemas/extend.json",
    from: '        "period"\n      ],\n      "additionalProperties": false\n',
    to: '        "period"\n      ]\n',
    want: { validator: { ...BASE, extend: "MISSED" }, typify: { ...BASE, extend: "MISSED" }, flagged: true },
  },
  {
    name: "the closed check no longer reports allOf",
    file: CLOSED,
    from: '    if ("allOf" in schema) findings.push(',
    to: "    if (false) findings.push(",
    want: { validator: BASE, typify: BASE, flagged: false },
  },
  {
    name: "the closed check lets an allOf with unevaluatedProperties false pass",
    file: CLOSED,
    from: '    if ("allOf" in schema) findings.push(',
    to: '    if ("allOf" in schema && schema.unevaluatedProperties !== false) findings.push(',
    want: { validator: BASE, typify: BASE, flagged: false },
  },
  {
    name: "the allowed intersection keeps allOf in its export, then exported again",
    file: SCHEMA,
    from: "      return z.intersection(objectOf(looseness, platform), objectOf(looseness, install));",
    to: '      return z.intersection(objectOf(looseness, platform).describe("the platform half of the device"), objectOf(looseness, install));',
    exportAgain: true,
    want: { validator: { ...BASE, intersection: "MISSED" }, typify: { ...BASE, intersection: "MISSED" }, flagged: true },
  },
  {
    name: "the closed check reports allOf at the root of a schema only",
    file: CLOSED,
    from: '    if ("allOf" in schema) findings.push(',
    to: '    if ("allOf" in schema && at === "") findings.push(',
    want: { validator: BASE, typify: BASE, flagged: false },
  },
  {
    name: "the closed check reports allOf only where a branch describes an object",
    file: CLOSED,
    from: '    if ("allOf" in schema) findings.push(',
    to: "    if (Array.isArray(schema.allOf) && schema.allOf.some((branch) => isJson(branch) && describesObject(branch))) findings.push(",
    want: { validator: BASE, typify: BASE, flagged: false },
  },
  {
    name: "the Worker rejects every body of the form extend",
    file: WORKER,
    from: '    if (!parsed.success) return reject(variant, "schema", bytes);',
    to: '    if (!parsed.success || variant.form === "extend") return reject(variant, "schema", bytes);',
    want: { validator: { ...BASE, extend: "MISSED" }, typify: { ...BASE, extend: "MISSED" }, flagged: true },
  },
  {
    name: "the closed check lets an object pass that unevaluatedProperties alone closes",
    file: CLOSED,
    from: "      if (schema.additionalProperties !== false) findings.push(",
    to: "      if (schema.additionalProperties !== false && schema.unevaluatedProperties !== false) findings.push(",
    want: RUN_MUST_NOT_COUNT,
    wholeFirstLine: `${NOT_COUNTED}the closed check does not flag its own fixtures.`,
  },
  {
    name: "no arbitrary name is sampled",
    file: "spikes/s3/test/s3.test.ts",
    from: "const ARBITRARY_NAMES = 1000;",
    to: "const ARBITRARY_NAMES = 0;",
    want: RUN_MUST_NOT_COUNT,
    wholeFirstLine: NO_ARBITRARY_NAME,
  },
];

const STRING_LITERAL = /^(["'`])([^"'`$]*)\1$/;
function specifiersImportedBy(source) {
  const imports = [...source.matchAll(/^\s*import\b[^"'`;]*(["'`][^"'`]*["'`])/gm)].map((match) => match[1]);
  const exportsFrom = [...source.matchAll(/^\s*export\b[^"'`;]*\bfrom\s*(["'`][^"'`]*["'`])/gm)].map((match) => match[1]);
  const calls = [...source.matchAll(/\b(?:import|require)\s*\(\s*([^)]*?)\s*\)/g)].map((match) => match[1]);
  return [...imports, ...exportsFrom, ...calls].map((written) => STRING_LITERAL.exec(written)?.[2] ?? written);
}

function judgeImportsOnlyFromNode(tree) {
  const specifiers = specifiersImportedBy(readFileSync(join(tree, JUDGE), "utf8"));
  const fromElsewhere = specifiers.filter((specifier) => !specifier.startsWith("node:"));
  if (specifiers.length > 0 && fromElsewhere.length === 0) return;
  const found = specifiers.length === 0 ? "the check found no import in it, so it checked nothing" : `it imports ${fromElsewhere.join(", ")}`;
  console.log(`NOT AS REQUIRED: ${JUDGE} imports from node: modules only, so that it names what it expects by itself: ${found}`);
  process.exit(1);
}

function run(command, args, cwd) {
  const done = spawnSync(command, args, { cwd, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  if (done.error) throw done.error;
  return { status: done.status, lines: (done.stdout ?? "").split("\n"), stderr: done.stderr ?? "" };
}

function must(command, args, cwd) {
  const done = run(command, args, cwd);
  if (done.status !== 0) throw new Error(`${command} ${args.join(" ")} exited ${done.status}: ${done.stderr}`);
}

function filesLeftByTheRunIn(dir) {
  try {
    return readdirSync(dir, { withFileTypes: true }).filter((entry) => entry.isFile());
  } catch {
    return [];
  }
}

function judgePrintsTheSameAtTwoPaths(tree) {
  const measured = join(tree, ".tmp", "s3");
  const measuredFiles = filesLeftByTheRunIn(measured);
  const outputs = ["s3-at-one-path", join("s3-at-another-path", "one-level-down")].map((copyName) => {
    const dir = join(tree, ".tmp", copyName);
    mkdirSync(dir, { recursive: true });
    for (const file of measuredFiles) copyFileSync(join(measured, file.name), join(dir, file.name));
    const done = run("node", [JUDGE, dir], tree);
    return `exit ${done.status}\n${done.lines.join("\n")}`;
  });
  if (outputs[0] === outputs[1]) return;
  failures += 1;
  console.log("NOT AS REQUIRED: the judge prints the same text for one DIR at two paths: the two outputs differ");
}

const verdicts = (line) => Object.fromEntries([...(line ?? "").matchAll(/(extend|intersection) (MET|MISSED):/g)].map((match) => [match[1], match[2]]));
const text = (byForm) => ALLOWED.map((form) => `${form} ${byForm[form] ?? "?"}`).join(", ");
const lineWord = (byForm, flagged = true) => (flagged && ALLOWED.every((form) => byForm[form] === "MET") ? "MET" : "MISSED");

mkdirSync(join(root, ".tmp"), { recursive: true });
const keptCopy = mkdtempSync(join(root, ".tmp", "s3-mutation."));
const commitPinnedAtStart = spawnSync("git", ["-C", root, "rev-parse", "HEAD"], { encoding: "utf8" }).stdout.trim();
if (!/^[0-9a-f]{40}$/.test(commitPinnedAtStart)) throw new Error("git rev-parse HEAD printed no commit");
const copyFromHead = (path) => {
  const archive = join(keptCopy, ".copy.tar");
  must("git", ["-C", root, "archive", "-o", archive, commitPinnedAtStart, "--", path], root);
  must("tar", ["-x", "-f", archive, "-C", keptCopy], root);
};
copyFromHead(".");
symlinkSync(join(root, "node_modules"), join(keptCopy, "node_modules"));
console.log(`mutation-control: a copy of ${commitPinnedAtStart.slice(0, 7)} is in .tmp/${keptCopy.slice(root.length + 6)}`);
judgeImportsOnlyFromNode(keptCopy);

let failures = 0;
const report = (name, done, want, wholeFirstLine) => {
  let ok;
  let got;
  if (want === RUN_MUST_NOT_COUNT) {
    ok = done.status === 2 && done.lines[0] === wholeFirstLine;
    got = `run.sh exited ${done.status} and printed "${(done.lines[0] ?? "").slice(0, 150)}"`;
  } else {
    const validator = verdicts(done.lines[1]);
    const typify = verdicts(done.lines[2]);
    const flagLine = done.lines[3] ?? "";
    const wantValidator = lineWord(want.validator, want.flagged);
    const wantTypify = lineWord(want.typify);
    const allMet = wantValidator === "MET" && wantTypify === "MET";
    ok =
      done.status === (allMet ? 0 : 1) &&
      done.lines[0] === `rule: ${allMet ? "MET" : "MISSED"} (validator ${wantValidator}, typify ${wantTypify})` &&
      (done.lines[1] ?? "").startsWith(`rule (validator): ${wantValidator}. `) &&
      (done.lines[2] ?? "").startsWith(`rule (typify): ${wantTypify}. `) &&
      ALLOWED.every((form) => validator[form] === want.validator[form] && typify[form] === want.typify[form]) &&
      flagLine.startsWith(want.flagged ? FLAGGED : NOT_FLAGGED);
    got = `run.sh exited ${done.status}; ${done.lines[0]}; validator: ${text(validator)}; typify: ${text(typify)}; ${flagLine.startsWith(FLAGGED) ? FLAGGED : flagLine.startsWith(NOT_FLAGGED) ? NOT_FLAGGED : "no line for allof"}`;
  }
  if (!ok) failures += 1;
  console.log(`${ok ? "as required" : "NOT AS REQUIRED"}: ${name}: ${got}`);
};

report("the unchanged copy", run("sh", ["spikes/s3/run.sh"], keptCopy), { validator: BASE, typify: BASE, flagged: true });
judgePrintsTheSameAtTwoPaths(keptCopy);
for (const mutation of MUTATIONS) {
  copyFromHead("spikes/s3");
  const path = join(keptCopy, mutation.file);
  const before = readFileSync(path, "utf8");
  if (before.split(mutation.from).length !== 2) {
    failures += 1;
    console.log(`NOT AS REQUIRED: ${mutation.name}: the text to change is not in ${mutation.file} exactly once`);
    continue;
  }
  writeFileSync(path, before.replace(mutation.from, () => mutation.to));
  if (mutation.exportAgain) must("node", ["spikes/s3/export.mjs"], keptCopy);
  report(mutation.name, run("sh", ["spikes/s3/run.sh"], keptCopy), mutation.want, mutation.wholeFirstLine);
}
process.exit(failures === 0 ? 0 : 1);
