// The packages that adding psl brings into the graph of halvo-capture, each licence expression
// checked against the allow list that gate-2 plans for deny.toml: the permissive list of
// static-analyzer-rr-1 plus MPL-2.0, blessing and WTFPL (units/gate-2.md, RR-gate-ci-27).
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, dirname } from "node:path";

const allow = new Set([
  "MIT", "MIT-0", "Apache-2.0", "Apache-2.0 WITH LLVM-exception", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Zlib",
  "0BSD", "Unlicense", "CC0-1.0", "BSL-1.0", "Unicode-3.0", "Unicode-DFS-2016", "CDLA-Permissive-2.0",
  "MPL-2.0", "blessing", "WTFPL",
]);
const permissiveOnly = new Set([...allow].filter((l) => !["MPL-2.0", "blessing", "WTFPL"].includes(l)));

function tokens(expr) {
  return expr.replace(/\//g, " OR ").replace(/\(/g, " ( ").replace(/\)/g, " ) ").split(/\s+/).filter(Boolean);
}
// expr := term (OR term)*; term := factor (AND factor)*; factor := ID [WITH ID] | ( expr )
function evaluate(expr, set) {
  const t = tokens(expr);
  let i = 0;
  const factor = () => {
    if (t[i] === "(") { i++; const v = orExpr(); i++; return v; }
    let id = t[i++];
    if (t[i] === "WITH") { id = `${id} WITH ${t[i + 1]}`; i += 2; }
    return set.has(id);
  };
  const andExpr = () => { let v = factor(); while (t[i] === "AND") { i++; const w = factor(); v = v && w; } return v; };
  const orExpr = () => { let v = andExpr(); while (t[i] === "OR") { i++; const w = andExpr(); v = v || w; } return v; };
  return orExpr();
}

function mentionsMpl(dir) {
  let n = 0;
  const walk = (d, depth) => {
    for (const e of readdirSync(d)) {
      const p = join(d, e);
      const s = statSync(p);
      if (s.isDirectory()) { if (depth < 3) walk(p, depth + 1); continue; }
      if (s.size > 4_000_000) continue;
      const txt = readFileSync(p, "latin1");
      if (txt.includes("Mozilla Public License")) n++;
    }
  };
  try { walk(dir, 0); } catch { return -1; }
  return n;
}

const [baseFile, newFile] = process.argv.slice(2);
const base = JSON.parse(readFileSync(baseFile, "utf8"));
const next = JSON.parse(readFileSync(newFile, "utf8"));
const have = new Set(base.packages.map((p) => `${p.name} ${p.version}`));
const added = next.packages.filter((p) => !have.has(`${p.name} ${p.version}`));
console.log(`S8 licences packages_before=${base.packages.length} packages_after=${next.packages.length} added=${added.length}`);
for (const p of added) {
  const lic = p.license || "none";
  console.log(
    `S8 licence package=${p.name} version=${p.version} license_expression=${lic.replace(/ /g, "_")} in_planned_allow_list=${lic !== "none" && evaluate(lic, allow)} in_permissive_only_list=${lic !== "none" && evaluate(lic, permissiveOnly)} files_mentioning_mpl=${mentionsMpl(dirname(p.manifest_path))}`,
  );
}
