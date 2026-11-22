// Writes THIRD-PARTY-NOTICES.txt for one target triple: the software of others that a build of
// Halvo holds, and the licence texts of each (packaging/README.md, Notices).
//
//   node scripts/notices/generate.js --triple <triple> [--out <file>] [--offline] [--repo <dir>] [--config <file>]
//
// It reads `cargo metadata --locked --format-version 1 --filter-platform <triple>` once and walks
// from the app and from each helper of packaging/helpers.d over normal edges, past proc-macro
// crates, and lists every crate that is no member of the workspace. scripts/notices/config.json
// names what no such edge gives. With --out it writes the file only when its bytes differ, and
// prints the counts on standard error; without --out it writes the file to standard output.
"use strict";
const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

// The head of the file: the three sentences that a person reads first, in this one place.
const HEAD = [
  "THIRD-PARTY NOTICES",
  "",
  "This file lists the software of others inside this build of Halvo: each piece by its name, its version and the licence that it names, and after the list the text of each licence. It says nothing of the licence of Halvo itself.",
  "",
  "Where a piece ships no licence text of its own, this file gives the standard text of each licence that it names, with its authors as the holder.",
];

// A file at the root of a crate whose name says that it holds a licence text.
const TEXT_NAME = /^(licen[cs]e|copying|copyright|notice|unlicense)([-._].*)?$/i;
// The line of a standard text that names the holder; the generator fills it in.
const HOLDER_LINE = "Copyright (c) <holder>";

function fail(message) {
  console.error(`scripts/notices/generate.js: ${message}`);
  process.exit(1);
}

function options(argv) {
  const known = { "--triple": "triple", "--out": "out", "--repo": "repo", "--config": "config" };
  const given = { offline: false };
  for (let at = 0; at < argv.length; at += 1) {
    if (argv[at] === "--offline") {
      given.offline = true;
    } else if (argv[at] in known && at + 1 < argv.length && !(known[argv[at]] in given)) {
      given[known[argv[at]]] = argv[at + 1];
      at += 1;
    } else {
      fail(`the argument "${argv[at]}" is not one of --triple <triple>, --out <file>, --offline, --repo <dir>, --config <file>`);
    }
  }
  if (!given.triple) fail("--triple <triple> is required");
  given.repo = path.resolve(given.repo || path.join(__dirname, "..", ".."));
  given.config = path.resolve(given.config || path.join(__dirname, "config.json"));
  return given;
}

function readJson(file, what) {
  let text;
  try {
    text = fs.readFileSync(file, "utf8");
  } catch (error) {
    fail(`${what} cannot be read: ${error.code || error.message}`);
  }
  try {
    return JSON.parse(text);
  } catch (error) {
    fail(`${what} is no JSON: ${error.message.split("\n")[0]}`);
  }
}

// The same file, by its real path where it has one.
function same(a, b) {
  const real = (file) => {
    try {
      return fs.realpathSync(file);
    } catch {
      return path.resolve(file);
    }
  };
  return real(a) === real(b);
}

// A text as the file shows it: line ends as LF, no blank lines before it, one line end after it.
function normal(text) {
  return `${text.replace(/\r\n/g, "\n").replace(/^\n+/, "").replace(/\s+$/, "")}\n`;
}

function helpers(repo) {
  const directory = path.join(repo, "packaging", "helpers.d");
  let names;
  try {
    names = fs.readdirSync(directory).filter((name) => !name.startsWith(".")).sort();
  } catch (error) {
    fail(`packaging/helpers.d cannot be read: ${error.code || error.message}`);
  }
  return names.map((name) => {
    const fields = {};
    for (const line of fs.readFileSync(path.join(directory, name), "utf8").split(/\r?\n/)) {
      const match = /^(package|manifest)=(\S+)$/.exec(line);
      if (match) fields[match[1]] = match[2];
    }
    if (!fields.package || !fields.manifest) fail(`packaging/helpers.d/${name} has no line package= or manifest=`);
    return { name, ...fields };
  });
}

function metadata(repo, triple, offline) {
  const args = ["metadata", "--locked", ...(offline ? ["--offline"] : []), "--format-version", "1", "--filter-platform", triple];
  const result = spawnSync("cargo", args, { cwd: repo, encoding: "utf8", maxBuffer: 512 * 1024 * 1024, stdio: ["ignore", "pipe", "pipe"] });
  if (result.error) fail(`cargo: ${result.error.message}`);
  if (result.status !== 0) {
    const cause = (result.stderr || "").split(/\r?\n/).find((line) => line.startsWith("error")) || `exit ${result.status}`;
    fail(`cargo metadata for ${triple} failed: ${cause}`);
  }
  try {
    return JSON.parse(result.stdout);
  } catch (error) {
    fail(`cargo metadata for ${triple} printed no JSON: ${error.message.split("\n")[0]}`);
  }
}

function standardText(id) {
  if (!/^[A-Za-z0-9.+-]+$/.test(id)) fail(`the licence id "${id}" is no name of a file of scripts/notices/licences`);
  const file = path.join(__dirname, "licences", `${id}.txt`);
  if (!fs.existsSync(file)) return null;
  return fs.readFileSync(file, "utf8");
}

// The ids of a licence field, in the order in which it names them: "MIT OR Apache-2.0" and the
// older "MIT/Apache-2.0" both give MIT and Apache-2.0.
function licenceIds(field) {
  const ids = [];
  for (const word of field.split(/[\s()/]+/)) {
    if (word && !["AND", "OR", "WITH"].includes(word) && !ids.includes(word)) ids.push(word);
  }
  return ids;
}

// The lines of a file from the first line that holds `from` through the first line after it that
// holds `through`.
function part(text, from, through, where) {
  const lines = text.replace(/\r\n/g, "\n").split("\n");
  const start = lines.findIndex((line) => line.includes(from));
  if (start === -1) fail(`${where} holds no line with "${from}"`);
  const end = lines.findIndex((line, at) => at >= start && line.includes(through));
  if (end === -1) fail(`${where} holds no line with "${through}" after the line with "${from}"`);
  return lines.slice(start, end + 1).join("\n");
}

function generate(given) {
  const config = readJson(given.config, "scripts/notices/config.json");
  const meta = metadata(given.repo, given.triple, given.offline);
  const packages = new Map(meta.packages.map((p) => [p.id, p]));
  const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]));
  const members = new Set(meta.workspace_members);
  const memberNamed = (name) => meta.packages.find((p) => members.has(p.id) && p.name === name);
  const procMacro = (p) => p.targets.some((t) => t.kind.includes("proc-macro"));

  const app = meta.packages.find((p) => members.has(p.id) && same(p.manifest_path, path.join(given.repo, config.app)));
  if (!app) fail(`config.json names the app ${config.app}, which is no member of the workspace that cargo metadata reads`);
  const roots = [{ name: app.name, id: app.id }];
  for (const helper of helpers(given.repo)) {
    const p = memberNamed(helper.package);
    if (!p || !same(p.manifest_path, path.join(given.repo, helper.manifest))) {
      fail(`packaging/helpers.d/${helper.name} names the package ${helper.package} of ${helper.manifest}, which is no member of the workspace that cargo metadata reads`);
    }
    roots.push({ name: helper.name, id: p.id });
  }

  // Every crate that a root reaches over normal edges, past proc-macro crates.
  const reach = (start) => {
    const seen = new Set();
    const stack = [start];
    while (stack.length > 0) {
      const id = stack.pop();
      if (seen.has(id)) continue;
      seen.add(id);
      for (const dep of nodes.get(id).deps) {
        if (!dep.dep_kinds.some((kind) => kind.kind === null)) continue;
        if (procMacro(packages.get(dep.pkg))) continue;
        stack.push(dep.pkg);
      }
    }
    return seen;
  };

  // The pieces that one set of reached crates ships: crates of others by their id, the build
  // dependencies that config.json names as compiled in, and the tracked texts of config.json.
  const pieces = (reached) => {
    const crates = new Set([...reached].filter((id) => !members.has(id)));
    const compiled = [];
    for (const rule of config.compiled || []) {
      const by = memberNamed(rule.by) || meta.packages.find((p) => p.name === rule.by);
      if (!by || !reached.has(by.id)) continue;
      const dep = nodes.get(by.id).deps.find((d) => packages.get(d.pkg).name === rule.crate && d.dep_kinds.some((kind) => kind.kind === "build"));
      if (!dep) fail(`config.json names ${rule.crate} as compiled into ${rule.by}, which has no build-dependency of that name`);
      compiled.push({ id: dep.pkg, rule });
      crates.add(dep.pkg);
    }
    const tracked = (config.tracked || []).filter((entry) => {
      const holder = memberNamed(entry.in);
      if (!holder) fail(`config.json names the text of ${entry.name} in ${entry.in}, which is no member of the workspace`);
      return reached.has(holder.id);
    });
    return { crates, compiled, tracked };
  };

  const all = new Set();
  const perRoot = [];
  for (const root of roots) {
    const reached = reach(root.id);
    for (const id of reached) all.add(id);
    const own = pieces(reached);
    perRoot.push(`${root.name} ${own.crates.size + own.tracked.length}`);
  }
  // A build-dependency whose name ends in -src carries sources that a build script may compile in,
  // as sqlcipher-src does for halvo-sqlcipher: config.json names each such edge under "compiled".
  for (const id of all) {
    const by = packages.get(id);
    for (const dep of nodes.get(id).deps) {
      const p = packages.get(dep.pkg);
      if (!p.name.endsWith("-src") || !dep.dep_kinds.some((kind) => kind.kind === "build")) continue;
      if ((config.compiled || []).some((rule) => rule.crate === p.name && rule.by === by.name)) continue;
      const about = p.description ? ` (${p.description})` : "";
      fail(`${by.name} build-depends on ${p.name} ${p.version}${about}, whose name ends in -src, so its build may compile its sources in: name it in scripts/notices/config.json under "compiled", with its texts`);
    }
  }
  const { crates, compiled, tracked } = pieces(all);

  const software = [];
  const texts = new Map();
  const addText = (text, line) => {
    const key = normal(text);
    if (!texts.has(key)) texts.set(key, []);
    if (!texts.get(key).includes(line)) texts.get(key).push(line);
  };
  const standard = [];
  for (const id of crates) {
    const p = packages.get(id);
    const label = `${p.name} ${p.version}`;
    const dir = path.dirname(p.manifest_path);
    const files = fs
      .readdirSync(dir)
      .filter((name) => TEXT_NAME.test(name) && fs.statSync(path.join(dir, name)).isFile())
      .sort();
    if (p.license_file && !files.includes(p.license_file)) files.push(p.license_file.split(path.sep).join("/"));
    for (const file of files) {
      let body;
      try {
        body = fs.readFileSync(path.join(dir, file), "utf8");
      } catch (error) {
        fail(`the file ${file} of ${label} cannot be read: ${error.code || error.message}`);
      }
      addText(body, `${label}: its file ${file}`);
    }
    const rule = compiled.find((c) => c.id === id);
    for (const text of rule ? rule.rule.texts : []) {
      const where = `the file ${text.file} of ${label}`;
      let body;
      try {
        body = fs.readFileSync(path.join(dir, text.file), "utf8");
      } catch (error) {
        fail(`${where}, which config.json names, cannot be read: ${error.code || error.message}`);
      }
      if (text.from) {
        addText(part(body, text.from, text.through, where), `${label}: a part of its file ${text.file}`);
      } else {
        addText(body, `${label}: its file ${text.file}`);
      }
    }
    if (files.length === 0 && !rule) {
      const ids = licenceIds(p.license || "");
      if (ids.length === 0) fail(`${label} ships no licence text file and names no licence`);
      standard.push(label);
      for (const licence of ids) {
        const body = standardText(licence);
        if (body === null) fail(`${label} ships no licence text file, and scripts/notices/licences holds no standard text of ${licence}`);
        const holder = p.authors && p.authors.length > 0 ? `the authors of ${p.name}: ${p.authors.join(", ")}` : `the authors of ${p.name}`;
        const filled = body
          .replace(/\r\n/g, "\n")
          .split("\n")
          .map((line) => (line === HOLDER_LINE ? `Copyright (c) ${holder}` : line))
          .join("\n");
        addText(filled, `${label}: the standard text of ${licence}, for it ships no text of its own`);
      }
    }
    software.push({ name: p.name, version: p.version, line: `${label}  ${p.license || p.license_file}` });
  }
  for (const entry of tracked) {
    for (const text of entry.texts) {
      let body;
      try {
        body = fs.readFileSync(path.join(given.repo, text.path), "utf8");
      } catch (error) {
        fail(`the file ${text.path}, which config.json names for ${entry.name}, cannot be read: ${error.code || error.message}`);
      }
      addText(body, `${entry.name}: the file ${text.path} of the source of Halvo`);
    }
    software.push({ name: entry.name, version: "", line: `${entry.name}  ${entry.licence}` });
  }

  const byBytes = (a, b) => (a < b ? -1 : a > b ? 1 : 0);
  software.sort((a, b) => byBytes(a.name, b.name) || byBytes(a.version, b.version));
  const blocks = [...texts].map(([text, lines]) => ({ text, lines: lines.sort(byBytes) }));
  blocks.sort((a, b) => byBytes(a.lines[0], b.lines[0]) || byBytes(a.text, b.text));

  const out = [...HEAD, "", `SOFTWARE OF OTHERS: ${software.length}`, "", ...software.map((s) => s.line), "", `LICENCE TEXTS: ${blocks.length}`];
  blocks.forEach((block, at) => {
    out.push("", "=".repeat(80), `Text ${at + 1} of ${blocks.length}, for:`, ...block.lines, "-".repeat(80), block.text.replace(/\n$/, ""));
  });
  const summary = [
    `${given.triple}: ${software.length} pieces of software of others, ${blocks.length} licence texts; from each root: ${perRoot.join(", ")}`,
    `${given.triple}: ${standard.length} crates ship no licence text file, so the standard text of their licence field stands for each`,
  ];
  if (standard.length > 0) summary.push(`${given.triple}: those crates: ${standard.sort(byBytes).join(", ")}`);
  return { text: `${out.join("\n")}\n`, summary };
}

const given = options(process.argv.slice(2));
const { text, summary } = generate(given);
for (const line of summary) console.error(`scripts/notices/generate.js: ${line}`);
if (given.out) {
  const bytes = Buffer.from(text, "utf8");
  const file = path.resolve(given.out);
  let written = false;
  if (!fs.existsSync(file) || !fs.readFileSync(file).equals(bytes)) {
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, bytes);
    written = true;
  }
  console.error(`scripts/notices/generate.js: the notices file: ${written ? "written" : "unchanged"}, ${bytes.length} bytes`);
} else {
  process.stdout.write(text);
}
