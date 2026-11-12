import { readFileSync } from 'node:fs';
import { CATEGORIES, loadSites, references } from './sites.mjs';

function refuse(cause, detail) {
  process.stderr.write(`REFUSED ${cause}: ${detail}\n`);
  process.exit(2);
}

const USAGE = 'usage: node check-sites.mjs [--parameters FILE] SITE_DATA...';

function parseArgs(argv) {
  const out = { parameters: null, files: [] };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--parameters') {
      if (out.parameters !== null || i + 1 >= argv.length) refuse('usage', USAGE);
      out.parameters = argv[i + 1];
      i += 1;
    } else if (arg.startsWith('--')) {
      refuse('usage', USAGE);
    } else {
      out.files.push(arg);
    }
  }
  if (out.files.length === 0) refuse('usage', USAGE);
  return out;
}

function readRows(path) {
  let text;
  try {
    text = readFileSync(path, 'utf8');
  } catch {
    refuse('parameters', path);
  }
  const rows = new Set();
  for (const line of text.split('\n')) {
    const cells = /^\| ([^ |]+) \| ([^ |]+) \| ([^ |]+) \|$/.exec(line);
    if (cells) rows.add(`${cells[1]} ${cells[2]}`);
  }
  return rows;
}

const args = parseArgs(process.argv.slice(2));
const loaded = loadSites(args.files);
if (loaded.refused !== undefined) refuse('site-data', loaded.refused);
const rows = args.parameters === null ? null : readRows(args.parameters);

const lines = [`checked files=${args.files.length}`];
for (const category of CATEGORIES) {
  lines.push(`services ${category} ${loaded.sites.filter((site) => site.category === category).length}`);
}
loaded.sites.forEach((site, i) => {
  for (const page of site.pages) {
    if (page.password_input) lines.push(`password ${args.files[i]} ${page.page}`);
  }
  for (const page of site.pages) {
    if (page.card_title) lines.push(`card ${args.files[i]} ${page.page}`);
  }
});
let missing = 0;
if (rows !== null) {
  const names = new Set();
  const absent = [];
  loaded.sites.forEach((site, i) => {
    const used = new Set(site.pages.flatMap((page) => [...references(page.title), ...references(page.text)]));
    for (const name of [...site.parameters, ...used]) {
      const key = `${site.category} ${name}`;
      if (!names.has(key) && !rows.has(key)) absent.push(`missing ${args.files[i]} ${key}`);
      names.add(key);
    }
  });
  missing = absent.length;
  lines.push(`parameters names=${names.size} missing=${missing}`, ...absent);
}
process.stdout.write(`${lines.join('\n')}\n`);
process.exitCode = missing === 0 ? 0 : 1;
