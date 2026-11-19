import { readFileSync, statSync } from 'node:fs';
import { CATEGORIES, fileNumber, loadSites, readArgs, references } from './sites.mjs';

function refuse(cause, detail) {
  process.stderr.write(`REFUSED ${cause}: ${detail}\n`);
  process.exit(2);
}

process.on('uncaughtException', () => refuse('internal', 'the checker stopped at a fault of its own'));

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

const SIZE_MAX = 262_144;

const args = readArgs(process.argv.slice(2), '--parameters', () => true);
if (args === null) refuse('usage', USAGE);
for (const [i, path] of args.files.entries()) {
  let size = 0;
  try {
    size = statSync(path).size;
  } catch {
    continue;
  }
  if (size > SIZE_MAX) refuse('too-large', `SITE_DATA ${fileNumber(args.files, i)}`);
}
const loaded = loadSites(args.files);
if (loaded.refused !== undefined) refuse('site-data', `SITE_DATA ${loaded.number}`);
const rows = args.value === null ? null : readRows(args.value);

const lines = [`checked files=${args.files.length}`];
for (const category of CATEGORIES) {
  lines.push(`services ${category} ${loaded.sites.filter((site) => site.category === category).length}`);
}
const number = (i) => `SITE_DATA ${fileNumber(args.files, i)}`;
loaded.sites.forEach((site, i) => {
  site.pages.forEach((page, k) => {
    if (page.password_input) lines.push(`password ${number(i)} page ${k + 1}`);
  });
  site.pages.forEach((page, k) => {
    if (page.card_title) lines.push(`card ${number(i)} page ${k + 1}`);
  });
});
let missing = 0;
if (rows !== null) {
  const names = new Set();
  const absent = [];
  loaded.sites.forEach((site, i) => {
    const used = new Set(site.pages.flatMap((page) => [...references(page.title), ...references(page.text)]));
    for (const name of [...site.parameters, ...used]) {
      const key = `${site.category} ${name}`;
      const place = site.parameters.indexOf(name) + 1;
      if (!names.has(key) && !rows.has(key)) absent.push(`missing ${number(i)} ${site.category} parameter ${place}`);
      names.add(key);
    }
  });
  missing = absent.length;
  lines.push(`parameters names=${names.size} missing=${missing}`, ...absent);
}
process.stdout.write(`${lines.join('\n')}\n`);
process.exitCode = missing === 0 ? 0 : 1;
