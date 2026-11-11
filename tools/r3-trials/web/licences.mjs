import { execFileSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ALLOWED = ['MIT', 'Apache-2.0', 'BSD-2-Clause', 'BSD-3-Clause', 'ISC', 'Zlib', 'Unicode-3.0'];

function refuse(detail) {
  process.stderr.write(`REFUSED usage: ${detail}\n`);
  process.exit(2);
}

const [flag, date] = process.argv.slice(2);
if (flag !== '--read' || !/^\d{4}-\d{2}-\d{2}$/.test(date || '')) {
  refuse('node tools/r3-trials/web/licences.mjs --read YYYY-MM-DD');
}

const manifest = join(dirname(fileURLToPath(import.meta.url)), 'Cargo.toml');
const meta = JSON.parse(
  execFileSync('cargo', ['metadata', '--locked', '--offline', '--format-version', '1', '--manifest-path', manifest], {
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
  }),
);

function terms(expr) {
  return expr
    .replace(/\//g, ' OR ')
    .replace(/[()]/g, ' ')
    .split(/\s+AND\s+/)
    .map((t) => t.split(/\s+OR\s+/).map((s) => s.trim()).filter(Boolean));
}

let lines = 0;
let refused = 0;
const packages = meta.packages.filter((p) => p.source !== null).sort((a, b) => (a.name + a.version).localeCompare(b.name + b.version));
for (const p of packages) {
  const expr = p.license || 'none';
  for (const choices of terms(expr)) {
    const pick = ALLOWED.find((a) => choices.includes(a));
    const ok = pick ? 'yes' : 'no';
    if (!pick) refused += 1;
    lines += 1;
    process.stdout.write(`licence item=${p.name}@${p.version} spdx=${pick || choices.join('_OR_')} read=${date} ok=${ok}\n`);
  }
}
process.stderr.write(`licences packages=${packages.length} lines=${lines} ok_no=${refused}\n`);
process.exit(refused === 0 ? 0 : 1);
