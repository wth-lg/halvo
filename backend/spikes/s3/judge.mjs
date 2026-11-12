import { readFileSync } from "node:fs";
import { join } from "node:path";

const ALLOWED = ["extend", "intersection"];
const FORBIDDEN = "allof";
const FORMS = [...ALLOWED, FORBIDDEN];
const POSITIONS = ["root", "cap", "cap.period", "device"];
const SENTINEL = "S3-SENTINEL-not-real-data";
const UNKNOWN_CLASSES = [
  "forbidden names",
  "Object.prototype names",
  "__proto__, constructor, prototype with an object value",
  "variants of declared keys",
  "variants of forbidden names",
  "arbitrary names",
];
const ROUTER_CLASSES = ["repeat, sentinel first", "repeat, sentinel last", "padding", "trailing data", "encoding", "not an object"];
const TYPIFY_NAMES = [...FORMS, "control_open", "control_map", "probe_union"];
const HARNESS_FIXTURES = ["closedByUnevaluatedPropertiesOnly", "nestedOpenObject", "patternProperties", "trueSchema", "emptySchema", "openBranchOfAnyOf", "openDefinition"];
const ALLOF_FIXTURES = { allOfOfObjects: "an allOf of two closed objects", allOfWithUnevaluatedPropertiesFalse: "an allOf with unevaluatedProperties false", allOfOfStrings: "an allOf of two strings" };
const EXIT_MET = 0;
const EXIT_MISSED = 1;
const EXIT_RUN_DOES_NOT_COUNT = 2;
const EXIT_USAGE = 64;
const SHELL_COULD_NOT_EXECUTE_THE_COMMAND = "126";
const SHELL_DID_NOT_FIND_THE_COMMAND = "127";
const cargoDidNotStart = (exit) => !/^[0-9]+$/.test(exit) || exit === SHELL_COULD_NOT_EXECUTE_THE_COMMAND || exit === SHELL_DID_NOT_FIND_THE_COMMAND;

process.on("uncaughtException", (error) => {
  console.log(`rule: not decided. The run does not count: the judge could not read the measurements (${error.message}).`);
  process.exit(EXIT_RUN_DOES_NOT_COUNT);
});

const dir = process.argv[2];
if (dir === undefined) {
  console.error("usage: node spikes/s3/judge.mjs DIR");
  process.exit(EXIT_USAGE);
}

const read = (name) => {
  try {
    return readFileSync(join(dir, name), "utf8");
  } catch {
    return null;
  }
};
const linesOf = (text, prefix) =>
  (text ?? "")
    .split("\n")
    .filter((candidate) => candidate.startsWith(prefix))
    .map((line) => JSON.parse(line.slice(prefix.length)));
const sum = (numbers) => numbers.reduce((total, number) => total + number, 0);
const repo = (path) => {
  try {
    return readFileSync(new URL(`../../${path}`, import.meta.url), "utf8");
  } catch (error) {
    throw new Error(`${path} of the repository: ${error.code ?? "unreadable"}`);
  }
};
const versionOf = (name) => JSON.parse(repo(`node_modules/${name}/package.json`)).version;
const word = (met) => (met ? "MET" : "MISSED");

const whyRunDoesNotCount = [];
const vitestLog = read("vitest.log");
const vitestExit = (read("vitest.exit") ?? "missing").trim();
const results = linesOf(vitestLog, "S3-RESULT ");
if (vitestExit !== "0") whyRunDoesNotCount.push(`the test run under workerd exited ${vitestExit || "without a code"}`);
if (results.length !== 1) whyRunDoesNotCount.push(`the test run printed ${results.length} S3-RESULT lines, not 1`);
const result = results[0];
if (result === undefined) {
  console.log(`rule: not decided. The run does not count: ${whyRunDoesNotCount.join("; ")}.`);
  process.exit(EXIT_RUN_DOES_NOT_COUNT);
}

const typify = Object.fromEntries(
  TYPIFY_NAMES.map((name) => {
    const exit = (read(`cargo-${name}.exit`) ?? "missing").trim();
    const measured = linesOf(read(`cargo-${name}.log`), "S3-TYPIFY ");
    if (cargoDidNotStart(exit)) whyRunDoesNotCount.push(`cargo did not run for the export ${name} (exit ${exit || "none"})`);
    if (measured.length > 1 || (measured[0] !== undefined && measured[0].schema !== name)) whyRunDoesNotCount.push(`the checker's line for the export ${name} is not its own`);
    return [name, { exit, measured: exit === "0" ? (measured[0] ?? null) : null }];
  }),
);

const POINTER = { root: "", cap: "/properties/cap", "cap.period": "/properties/cap/properties/period", device: "/properties/device" };
const positionHoldingPointer = (pointer) =>
  POSITIONS.filter((position) => pointer === POINTER[position] || pointer.startsWith(`${POINTER[position]}/`))
    .filter((position) => !pointer.slice(POINTER[position].length).startsWith("/properties/"))
    .sort((a, b) => POINTER[b].length - POINTER[a].length)[0];
const perPosition = (record, field) => POSITIONS.map((position) => record[position][field]).join(" / ");
const findingText = (findings) => findings.map((finding) => `${finding.at || "/"}: ${finding.why}`).join("; ");

const usable = [];
const unusable = [];
for (const form of FORMS) {
  const positive = result.forms[form].positive;
  if (positive.bodies > 0 && positive.accepted === positive.bodies && positive.stateRows === positive.bodies) usable.push(form);
  else if (positive.bodies > 0 && positive.schema === positive.bodies) unusable.push(form);
  else whyRunDoesNotCount.push(`the unmodified bodies of the form ${form} were neither all accepted nor all rejected by the validator`);
}
if (usable.length === 0) whyRunDoesNotCount.push("the validator accepted the unmodified bodies in no form");
if (unusable.includes(FORBIDDEN)) whyRunDoesNotCount.push(`the form ${FORBIDDEN} was not measured: the validator rejects its unmodified bodies`);

for (const form of usable) {
  const measured = result.forms[form];
  for (const klass of UNKNOWN_CLASSES) {
    for (const position of POSITIONS) {
      const tally = measured.unknown[klass][position];
      if (tally.cases === 0) whyRunDoesNotCount.push(`${form}: no body of "${klass}" at ${position}`);
      if (tally.canonical + tally.json > 0) whyRunDoesNotCount.push(`${form}: ${tally.canonical + tally.json} bodies of "${klass}" at ${position} did not reach the validator`);
    }
  }
  for (const klass of ROUTER_CLASSES) {
    if (measured.router[klass].cases === 0) whyRunDoesNotCount.push(`${form}: no body of "${klass}"`);
  }
}

const controls = [];
const control = (name, caught, detail) => {
  controls.push({ name, caught, detail });
  if (!caught) whyRunDoesNotCount.push(`control ${name} was not caught (${detail})`);
};

for (const form of usable) {
  const measured = result.forms[form];
  const schemaUnderTest = measured.unknown["forbidden names"];
  const acceptsAllThereAndNoMoreElsewhere = (opened) =>
    POSITIONS.every((at) => {
      const row = opened[at];
      return row.cases[at] > 0 && POSITIONS.every((position) => row.accepted[position] === (position === at ? row.cases[position] : schemaUnderTest[position].accepted));
    });
  const beyond = (opened) =>
    sum(POSITIONS.flatMap((at) => POSITIONS.filter((position) => position !== at).map((position) => opened[at].accepted[position] - schemaUnderTest[position].accepted)));
  control(`K1 ${form}`, acceptsAllThereAndNoMoreElsewhere(measured.K1), `z.object at one position: accepted at root / cap / cap.period / device when that position is open: ${POSITIONS.map((at) => `${measured.K1[at].accepted[at]} of ${measured.K1[at].cases[at]}`).join(" / ")}; accepted at the other positions, beyond what the schema under test accepts there: ${beyond(measured.K1)}`);
  control(`K2 ${form}`, acceptsAllThereAndNoMoreElsewhere(measured.K2) && POSITIONS.every((at) => measured.K2[at].stateRowsWithSentinel > 0), `z.looseObject at one position: state rows that hold the sentinel: ${POSITIONS.map((at) => measured.K2[at].stateRowsWithSentinel).join(" / ")}`);
  const valid = (klass) => measured.router[klass].parsedValid;
  const k3 = ROUTER_CLASSES.every((klass) => measured.K3[klass].accepted === valid(klass)) && ["repeat, sentinel first", "padding", "encoding"].every((klass) => valid(klass) > 0) && valid("repeat, sentinel first") === measured.router["repeat, sentinel first"].cases;
  control(`K3 ${form}`, k3, `router rule off: accepted, of the bodies whose parsed value is valid, ${ROUTER_CLASSES.map((klass) => `${measured.K3[klass].accepted} of ${valid(klass)} in "${klass}"`).join(", ")}`);
  const rejectedByLeakyHandler = measured.K4.cases - measured.K4.accepted;
  control(`K4 ${form}`, rejectedByLeakyHandler > 0 && measured.K4.responsesWithSentinel === rejectedByLeakyHandler && measured.K4.consoleLinesWithSentinel === rejectedByLeakyHandler, `a handler that logs the body inside an object and returns it: the sentinel in ${measured.K4.responsesWithSentinel} of ${rejectedByLeakyHandler} rejected responses and in ${measured.K4.consoleLinesWithSentinel} console lines`);
  const known = new Set(measured.export.findings.map((finding) => `${finding.at} ${finding.why}`));
  const named = POSITIONS.every((at) =>
    ["strip", "loose"].every((opening) => {
      const findings = measured.export.K5[at][opening];
      const elsewhere = findings.filter((finding) => positionHoldingPointer(finding.at) !== at);
      return findings.length > elsewhere.length && elsewhere.every((finding) => known.has(`${finding.at} ${finding.why}`));
    }),
  );
  control(`K5 ${form}`, named, `the closed check on the export of each K1 and K2 schema: findings at the changed position, strip / loose: ${POSITIONS.map((at) => ["strip", "loose"].map((opening) => measured.export.K5[at][opening].filter((finding) => positionHoldingPointer(finding.at) === at).length).join(" / ")).join(", ")}`);
}
const open = typify.control_open;
const map = typify.control_map;
control("K6 open object", open.measured !== null && open.measured.not_denying.length > 0 && open.measured.serde_read_unknown.length > 0 && result.typifyControls.openEqualsCommitted === true, open.measured === null ? `no measurement, cargo exited ${open.exit}` : `typify on an export with z.object at cap: structs that do not deny unknown fields: ${open.measured.not_denying.join(", ") || "none"}; serde_json read ${open.measured.serde_read_unknown.length} of ${open.measured.unknown_key_cases} unknown keys`);
control("K6 map", map.measured !== null && map.measured.map_fields.length > 0 && result.typifyControls.mapEqualsCommitted === true, map.measured === null ? `no measurement, cargo exited ${map.exit}` : `typify on a schema-valued additionalProperties: map fields: ${map.measured.map_fields.join(", ") || "none"}; serde_json read ${map.measured.serde_read_unknown.length} of ${map.measured.unknown_key_cases} unknown keys`);

const walker = result.walker;
const harnessFixturesFlagged = walker.closedObject.length === 0 && HARNESS_FIXTURES.every((name) => walker[name].length > 0);
if (!harnessFixturesFlagged) whyRunDoesNotCount.push("the closed check does not flag its own fixtures");
if (result.runtime.userAgent !== "Cloudflare-Workers") whyRunDoesNotCount.push(`the tests did not run in workerd (user agent ${result.runtime.userAgent})`);
if (result.bodiesEqualCommitted !== true) whyRunDoesNotCount.push("the bodies that the Rust checker reads are not the Worker's unmodified bodies");

const logSentinel = (vitestLog ?? "").split(SENTINEL).length - 1;
const prototypeChanged = result.pollution.namesBefore !== result.pollution.namesAfter || result.pollution.pollutedProbe !== false;

const holdersOfAllOf = (value, at = "") =>
  typeof value !== "object" || value === null ? [] : [...(Array.isArray(value.allOf) ? [at] : []), ...Object.entries(value).flatMap(([key, inner]) => holdersOfAllOf(inner, `${at}/${key}`))];

const validator = usable.map((form) => {
  const measured = result.forms[form];
  const unknown = UNKNOWN_CLASSES.flatMap((klass) => POSITIONS.map((position) => measured.unknown[klass][position]));
  const router = ROUTER_CLASSES.map((klass) => measured.router[klass]);
  const facts = {
    form,
    unknownCases: sum(unknown.map((tally) => tally.cases)),
    unknownRejected: sum(unknown.map((tally) => tally.schema)),
    unknownAccepted: sum(unknown.map((tally) => tally.accepted)),
    routerCases: sum(router.map((tally) => tally.cases)),
    routerAccepted: sum(router.map((tally) => tally.accepted)),
    other: sum([...unknown, ...router].map((tally) => tally.other)),
    rowsAdded: measured.state.rowsAddedByCases,
    sentinelResponses: sum([...unknown, ...router].map((tally) => tally.sentinelInResponse)),
    sentinelConsole: measured.console.linesWithSentinel,
    sentinelState: measured.state.rowsWithSentinel,
    exportEqual: measured.export.equalsCommitted === true,
    findings: measured.export.findings,
    objects: measured.export.objects,
    allOfFoundByJudge: holdersOfAllOf(JSON.parse(repo(`spikes/s3/crate/schemas/${form}.json`))),
  };
  const closed = facts.exportEqual && facts.findings.length === 0 && facts.objects > 0 && facts.allOfFoundByJudge.length === 0;
  const met =
    facts.unknownAccepted === 0 && facts.unknownRejected === facts.unknownCases && facts.routerAccepted === 0 && facts.other === 0 && facts.rowsAdded === 0 &&
    facts.sentinelResponses === 0 && facts.sentinelConsole === 0 && facts.sentinelState === 0 && logSentinel === 0 && !prototypeChanged && closed;
  return { ...facts, closed, met };
});

const typed = usable.map((form) => {
  const { exit, measured } = typify[form];
  const met =
    measured !== null && result.forms[form].export.equalsCommitted === true &&
    measured.named_structs.length > 0 && measured.not_denying.length === 0 && measured.flatten_fields.length === 0 &&
    measured.map_fields.length === 0 && measured.serde_read_valid === measured.bodies && measured.bodies > 0 &&
    measured.unknown_key_cases > 0 && measured.serde_refused === measured.unknown_key_cases;
  return { form, exit, measured, met };
});

const ALLOF_REASON = "allOf, which the contract forbids";
const aboutAllOf = (why) => why === ALLOF_REASON;
const forbiddenExport = result.forms[FORBIDDEN].export;
const allOfFoundByJudge = holdersOfAllOf(JSON.parse(repo(`spikes/s3/crate/schemas/${FORBIDDEN}.json`)));
const whyNotFlagged = [
  ...(forbiddenExport.equalsCommitted === true ? [] : ["its export under workerd differs from the committed file"]),
  ...(allOfFoundByJudge.length > 0 ? [] : ["its committed export holds no allOf"]),
  ...allOfFoundByJudge.filter((at) => !forbiddenExport.findings.some((finding) => finding.at === at && aboutAllOf(finding.why))).map((at) => `the closed check reports no finding for allOf at ${at || "/"}`),
  ...Object.keys(ALLOF_FIXTURES).filter((name) => !walker[name].some(aboutAllOf)).map((name) => `the closed check reports no finding for allOf on ${ALLOF_FIXTURES[name]}`),
];
const flagged = whyNotFlagged.length === 0;

const allowedUsable = ALLOWED.every((form) => usable.includes(form));
const allowedOf = (lines) => lines.filter((line) => ALLOWED.includes(line.form));
const validatorMet = allowedUsable && allowedOf(validator).every((line) => line.met) && flagged;
const typifyMet = allowedUsable && allowedOf(typed).every((line) => line.met);
const validatorMetCountingEveryForm = validator.length > 0 && validator.every((line) => line.met);
const typifyMetCountingEveryForm = typed.length > 0 && typed.every((line) => line.met);
const lockedVersion = (name) => /name = "([^"]+)"\nversion = "([^"]+)"/g[Symbol.matchAll](repo("spikes/s3/crate/Cargo.lock")).find((match) => match[1] === name)?.[2] ?? "?";

const out = [];
if (whyRunDoesNotCount.length > 0) {
  out.push(`rule: not decided. The run does not count: ${whyRunDoesNotCount.join("; ")}.`);
} else {
  out.push(`rule: ${word(validatorMet && typifyMet)} (validator ${word(validatorMet)}, typify ${word(typifyMet)})`);
}
const validatorText = (line) => `of ${line.unknownCases} bodies with an unknown key Zod rejected ${line.unknownRejected} and the Worker accepted ${line.unknownAccepted}; of ${line.routerCases} bodies for the router rule the Worker accepted ${line.routerAccepted}; state gained ${line.rowsAdded} rows; the sentinel was found in ${line.sentinelResponses} responses, ${line.sentinelConsole} console lines and ${line.sentinelState} state rows; the export ${line.closed ? "is closed" : "is not closed"}, with ${line.findings.length} findings at ${line.objects} object subschemas${line.findings.length > 0 ? ` (${findingText(line.findings)})` : ""}${ALLOWED.includes(line.form) && line.allOfFoundByJudge.length > 0 ? `; the judge finds allOf in the committed export at ${line.allOfFoundByJudge.map((at) => at || "/").join(", ")}` : ""}`;
const typifyText = (line) => (line.measured !== null ? `the crate builds, ${line.measured.named_structs.length - line.measured.not_denying.length} of ${line.measured.named_structs.length} structs deny unknown fields, ${line.measured.flatten_fields.length} flatten fields, ${line.measured.map_fields.length} map fields, serde_json read ${line.measured.serde_read_valid} of ${line.measured.bodies} unmodified bodies and refused ${line.measured.serde_refused} of ${line.measured.unknown_key_cases} unknown keys` : `the crate does not build (cargo exited ${line.exit})`);
const allowedText = (lines, textOf) =>
  ALLOWED.map((form) => {
    const line = lines.find((candidate) => candidate.form === form);
    return line === undefined ? `${form} MISSED: the validator rejects its unmodified bodies, so the form cannot be used` : `${form} ${word(line.met)}: ${textOf(line)}`;
  }).join(". ");
out.push(`rule (validator): ${word(validatorMet)}. ${allowedText(validator, validatorText)}. The closed check ${flagged ? "flags" : "does not flag"} the forbidden form ${FORBIDDEN}.`);
out.push(`rule (typify): ${word(typifyMet)}. ${allowedText(typed, typifyText)}.`);
const forbiddenValidator = validator.find((line) => line.form === FORBIDDEN);
const forbiddenTyped = typed.find((line) => line.form === FORBIDDEN);
out.push(
  `${FORBIDDEN}: forbidden by the contract, ${flagged ? `flagged by the walk. Its export holds allOf at ${allOfFoundByJudge.map((at) => at || "/").join(", ")}; the closed check reports allOf there and on its ${Object.keys(ALLOF_FIXTURES).length} allOf fixtures` : `not flagged: ${whyNotFlagged.join("; ")}`}. ` +
    (forbiddenValidator === undefined ? "Its cases were not measured: the validator rejects its unmodified bodies." : `Its measured values decide neither line: ${validatorText(forbiddenValidator)}; ${typifyText(forbiddenTyped)}.`),
);
out.push(`as the first statement counts, with ${FORBIDDEN} as a third form: rule: ${word(validatorMetCountingEveryForm && typifyMetCountingEveryForm)} (validator ${word(validatorMetCountingEveryForm)}, typify ${word(typifyMetCountingEveryForm)})`);
if (unusable.length > 0) out.push(`unusable forms, not measured: ${unusable.join(", ")} (the validator rejects the unmodified bodies).`);
out.push("");
out.push(`versions: zod ${versionOf("zod")}, @cloudflare/vitest-plugin ${versionOf("@cloudflare/vitest-plugin")}, workerd ${/^ +'?workerd@([0-9][^':]*)'?:/m.exec(repo("pnpm-lock.yaml"))?.[1] ?? "?"}, vitest ${versionOf("vitest")}, fast-check ${versionOf("fast-check")}, typify ${lockedVersion("typify")}, jsonschema ${lockedVersion("jsonschema")}, ${(read("cargo.version") ?? "").trim() || "cargo ?"}`);
out.push(`runtime: user agent ${result.runtime.userAgent}; Zod's allowsEval ${result.runtime.zodAllowsEval}; new Function ${result.runtime.newFunction}; Object.prototype has ${result.runtime.objectPrototypeNames.length} own names before the cases and ${result.pollution.namesAfter} after; "polluted" in {} is ${result.pollution.pollutedProbe}`);
out.push(`seed of the arbitrary names: ${result.seed}; sentinel occurrences in the whole log of the test run: ${logSentinel}`);
const exportText = Object.fromEntries(FORMS.map((form) => [form, repo(`spikes/s3/crate/schemas/${form}.json`)]));
out.push(`exports: ${FORMS.map((form) => `${form} holds ${result.forms[form].export.composition.join(", ") || "no composition keyword"}`).join("; ")}; ${FORMS.flatMap((form, index) => FORMS.slice(index + 1).map((other) => `${form} and ${other} are ${exportText[form] === exportText[other] ? "byte-identical" : "different"}`)).join("; ")}`);

const sorted = (value) => (Array.isArray(value) ? value.map(sorted) : typeof value === "object" && value !== null ? Object.fromEntries(Object.keys(value).sort().map((key) => [key, sorted(value[key])])) : value);
const committedBodies = JSON.parse(repo("spikes/s3/crate/bodies/valid.json"));
const passesRouterRuleUnderNode = (text) => JSON.stringify(JSON.parse(text)) === text;

for (const form of usable) {
  const measured = result.forms[form];
  out.push("");
  out.push(`### ${form}`);
  out.push("");
  out.push(`Unmodified bodies: ${measured.positive.accepted} of ${measured.positive.bodies} accepted, ${measured.positive.stateRows} rows in state. Arbitrary names: ${measured.arbitrary.sampled} sampled, ${measured.arbitrary.distinct} distinct and sent, the longest ${measured.arbitrary.longest} UTF-16 units. After all cases state holds ${measured.state.rows} rows; the dump read ${measured.state.instances} instances of the Durable Object. The Worker wrote ${measured.console.lines} console lines, ${measured.console.distinctLines.length} distinct.`);
  out.push("");
  out.push("| unknown key, one per body | bodies at root / cap / cap.period / device | accepted | rejected by the validator | rejected by the router rule | not JSON | other |");
  out.push("|---|---|---|---|---|---|---|");
  for (const klass of UNKNOWN_CLASSES) {
    const byPosition = measured.unknown[klass];
    out.push(`| ${klass} | ${perPosition(byPosition, "cases")} | ${perPosition(byPosition, "accepted")} | ${perPosition(byPosition, "schema")} | ${perPosition(byPosition, "canonical")} | ${perPosition(byPosition, "json")} | ${perPosition(byPosition, "other")} |`);
  }
  out.push("");
  out.push("| router rule and body shape | bodies | accepted | rejected by the router rule | not JSON | rejected by the validator | other | parsed value valid | accepted with the rule off (K3) |");
  out.push("|---|---|---|---|---|---|---|---|---|");
  for (const klass of ROUTER_CLASSES) {
    const tally = measured.router[klass];
    out.push(`| ${klass} | ${tally.cases} | ${tally.accepted} | ${tally.canonical} | ${tally.json} | ${tally.schema} | ${tally.other} | ${tally.parsedValid} | ${measured.K3[klass].accepted} |`);
  }
  out.push("");
  out.push(`Export: equal to the committed file: ${measured.export.equalsCommitted}; ${measured.export.objects} object subschemas, ${measured.export.withoutAdditionalPropertiesFalse} without additionalProperties false, ${measured.export.findings.length} findings${measured.export.findings.length > 0 ? ` (${findingText(measured.export.findings)})` : ""}. In output mode the export of z.object at cap has ${measured.export.outputModeFindingsForStripAtCap.filter((finding) => positionHoldingPointer(finding.at) === "cap").length} findings at cap.`);
  const { measured: rust } = typify[form];
  if (rust !== null) {
    const textsPassingRouterRuleUnderNode = rust.serde_written.filter((text) => passesRouterRuleUnderNode(text)).length;
    const sameValue = rust.serde_written.filter((text, index) => JSON.stringify(sorted(JSON.parse(text))) === JSON.stringify(sorted(committedBodies[index] ?? null))).length;
    out.push(`typify: ${rust.generated_lines} generated lines; structs ${rust.named_structs.join(", ")}; enums ${rust.enums.join(", ")}; tuple structs ${rust.tuple_structs.join(", ")}; aliases ${rust.aliases.length}. The jsonschema crate on the same export: ${rust.jsonschema_valid} of ${rust.bodies} unmodified bodies valid, ${rust.jsonschema_refused} of ${rust.unknown_key_cases} unknown keys refused. serde_json wrote ${rust.serde_written.length} unmodified bodies back from the generated types: ${textsPassingRouterRuleUnderNode} texts equal JSON.stringify of their parsed value and ${sameValue} hold the value they were read from.`);
  }
}

const probe = result.probe;
const probed = typify.probe_union;
out.push("");
out.push("### observations outside the rule");
out.push("");
out.push("A probe with what the unit's schema does not hold: a discriminated union of three strict objects, one of which holds only the tag, a nullable enum and an array of strict objects (buildProbe in src/schema.ts).");
out.push("");
out.push(`- Zod under workerd, called without the Worker: accepted ${probe.accepted} of ${probe.bodies} probe bodies and refused ${probe.refused} of ${probe.unknownKeyCases} bodies with an unknown key, at the positions ${probe.positions.join(", ")}.`);
out.push(`- Export: equal to the committed file: ${probe.exportEqualsCommitted}; composition keywords in it: ${probe.composition.join(", ") || "none"}; findings of the closed check: ${probe.findings.length}.`);
out.push(probed.measured === null ? `- typify: the crate does not build for this export (cargo exited ${probed.exit}).` : `- typify: the crate builds; structs and struct variants ${probed.measured.named_structs.join(", ")}; of them ${probed.measured.not_denying.length} do not deny unknown fields; ${probed.measured.flatten_fields.length} flatten fields; ${probed.measured.map_fields.length} map fields; serde_json read ${probed.measured.serde_read_valid} of ${probed.measured.bodies} probe bodies and refused ${probed.measured.serde_refused} of ${probed.measured.unknown_key_cases} unknown keys; it read an unknown key at: ${[...new Set(probed.measured.serde_read_unknown.map((entry) => entry.split(" ")[0]))].join(", ") || "no position"}; the jsonschema crate found ${probed.measured.jsonschema_valid} of ${probed.measured.bodies} bodies valid and refused ${probed.measured.jsonschema_refused} of ${probed.measured.unknown_key_cases}.`);

out.push("");
out.push("### controls");
out.push("");
for (const { name, caught, detail } of controls) out.push(`- ${name}: ${caught ? "caught" : "NOT CAUGHT"}. ${detail}.`);
console.log(out.join("\n"));
process.exit(whyRunDoesNotCount.length > 0 ? EXIT_RUN_DOES_NOT_COUNT : validatorMet && typifyMet ? EXIT_MET : EXIT_MISSED);
