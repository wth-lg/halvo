import { readFileSync } from 'node:fs';
import { CATEGORIES, loadSites, readArgs, references } from './sites.mjs';

function refuse(cause, detail) {
  process.stderr.write(`REFUSED ${cause}: ${detail}\n`);
  process.exit(2);
}

const USAGE = 'usage: node check-sites.mjs [--parameters FILE] SITE_DATA...';

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

const args = readArgs(process.argv.slice(2), '--parameters', () => true);
if (args === null) refuse('usage', USAGE);
const loaded = loadSites(args.files);
if (loaded.refused !== undefined) refuse('site-data', loaded.refused);
const rows = args.value === null ? null : readRows(args.value);

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
