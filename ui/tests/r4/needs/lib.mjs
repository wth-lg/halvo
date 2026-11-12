import { copyFileSync, existsSync, mkdtempSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

export const HERE = dirname(fileURLToPath(import.meta.url));
export const ROOT = resolve(HERE, '..', '..', '..', '..');
export const TRACKED = join(ROOT, 'ui', 'tests', 'fixtures', 'coord');
export const VIEW_FILE = join(ROOT, 'ui', 'app', 'views', 'needs.js');
export const VIEW = '/app/views/needs.js';

const harness = await import(pathToFileURL(join(ROOT, 'tools', 'r4-page', 'index.mjs')).href);
export const { serve, open, act, check, trail, settled } = harness;

/** The states of the fixture directory, read from its file names, and the test items of items.json. */
export function fixtures() {
  const dir = mkdtempSync(join(tmpdir(), 'r4-needs-'));
  for (const name of readdirSync(TRACKED)) {
    if (name.endsWith('.json')) copyFileSync(join(TRACKED, name), join(dir, name));
  }
  const tests = JSON.parse(readFileSync(join(HERE, 'items.json'), 'utf8'));
  for (const [state, file] of Object.entries(tests)) {
    writeFileSync(join(dir, 'state-' + state + '.json'), JSON.stringify(file) + '\n');
  }
  return { dir, states: statesOf(TRACKED), tests: Object.keys(tests) };
}

export function statesOf(dir) {
  return readdirSync(dir)
    .map((name) => /^state-(.+)\.json$/.exec(name))
    .filter(Boolean)
    .map((m) => m[1])
    .sort();
}

export function stateFile(dir, state) {
  return JSON.parse(readFileSync(join(dir, 'state-' + state + '.json'), 'utf8'));
}

export function commands() {
  return JSON.parse(readFileSync(join(TRACKED, 'commands.json'), 'utf8'));
}

export function words() {
  return JSON.parse(readFileSync(join(TRACKED, 'words.json'), 'utf8'));
}

const joined = (value) => value.join('');

/** Whether a field is a line of Halvo with no value, which the card shows as its label alone. */
export function isLine(field) {
  return field.from === 'halvo' && joined(field.value) === '';
}

function texts(items, value) {
  const out = [];
  const add = (text) => {
    if (text !== '') out.push(text);
  };
  for (const item of items) {
    add(item.title);
    add(item.text);
    for (const f of item.fields) {
      add(f.label);
      if (isLine(f)) continue;
      value(f.value).forEach(add);
      add(f.from_text);
    }
    for (const p of item.params) {
      add(p.label);
      for (const c of p.candidates) value(c.value).forEach(add);
    }
    for (const c of item.choices) add(c.text);
  }
  return out;
}

/** The texts that a card of each item shows, one per element in document order: what the reply holds and nothing else. */
export function expectedUnits(items) {
  return texts(items, (value) => [joined(value)]);
}

/** The text nodes of the cards in document order: each piece of a value as one text node (I3.4). */
export function expectedNodes(items) {
  return texts(items, (value) => value);
}

/** The controls of each item in document order: [data-act, data-param, data-n, kind]. */
export function expectedControls(items) {
  const controls = [];
  for (const item of items) {
    for (const p of item.params) {
      for (const c of p.candidates) controls.push(['candidate', p.name, String(c.n), 'radio']);
      if (p.typed === 'yes') controls.push(['typed', p.name, null, 'text']);
    }
    for (const c of item.choices) controls.push([c.choice, item.item, null, 'button']);
  }
  return controls;
}

export function pageConfirms(items) {
  return items.filter((i) => i.kind === 'confirm' && i.presenter === 'page');
}

/** What the section of the view shows: its texts, its controls and its confirmations. */
export async function read(h) {
  return h.page.evaluate(() => {
    const heading = document.getElementById('view-needs');
    if (!heading) return null;
    const section = heading.parentNode;
    const units = [];
    const nodes = [];
    const walk = (el) => {
      for (const node of el.children) {
        if (node === heading || node.tagName === 'STYLE') continue;
        let text = '';
        for (const t of node.childNodes) {
          if (t.nodeType !== 3) continue;
          text += t.data;
          if (t.data !== '') nodes.push(t.data);
        }
        if (text !== '') units.push(text);
        walk(node);
      }
    };
    walk(section);
    const controls = [...section.querySelectorAll('[data-act]')].map((e) => [
      e.getAttribute('data-act'),
      e.getAttribute('data-param'),
      e.getAttribute('data-n'),
      e.tagName === 'INPUT' ? e.type : e.tagName.toLowerCase(),
    ]);
    const dialogs = [...section.querySelectorAll('[role="alertdialog"]')].map((d) => ({
      rendered: d.getClientRects().length > 0,
      modal: d.matches(':modal'),
      item: (d.querySelector('[data-param]') || d).getAttribute('data-param'),
    }));
    return { units, nodes, controls, dialogs };
  });
}

export function callsOf(out) {
  const file = join(out, 'calls.jsonl');
  if (!existsSync(file)) return [];
  return readFileSync(file, 'utf8')
    .split('\n')
    .filter(Boolean)
    .map((line) => JSON.parse(line));
}

export function outDir() {
  return mkdtempSync(join(tmpdir(), 'r4-needs-out-'));
}

/** The causes for which a call breaks the command list of commands.json; none for a call that it holds. */
export function callFaults(list, call) {
  const command = list.find((c) => c.command === call.call);
  if (!command) return ['command'];
  const faults = [];
  const args = call.args || {};
  const names = new Set(command.args.map((a) => a.name));
  for (const key of Object.keys(args)) if (!names.has(key)) faults.push('arg_' + key);
  for (const a of command.args) {
    const wanted = a.when === null || args.choice === a.when;
    const given = Object.prototype.hasOwnProperty.call(args, a.name);
    if (wanted !== given) {
      faults.push((wanted ? 'missing_' : 'extra_') + a.name);
      continue;
    }
    if (!given) continue;
    const v = args[a.name];
    if (a.type === 'id' && !(typeof v === 'string' && /^[0-9a-f]{32}$/.test(v))) faults.push('type_' + a.name);
    if (a.type === 'count' && !(Number.isInteger(v) && v >= 0)) faults.push('type_' + a.name);
    if (a.type === 'name' && !(typeof v === 'string' && (a.values === null || a.values.includes(v)))) faults.push('value_' + a.name);
    if (a.type === 'text' && !(typeof v === 'string' && Buffer.byteLength(v) <= 1024)) faults.push('type_' + a.name);
  }
  return faults;
}

/** The index of the first difference of two lists, or -1. */
export function firstDiff(a, b) {
  const n = Math.max(a.length, b.length);
  for (let i = 0; i < n; i++) {
    if (JSON.stringify(a[i]) !== JSON.stringify(b[i])) return i;
  }
  return -1;
}
