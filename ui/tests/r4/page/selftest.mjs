import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, renameSync, statSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { ACTS_FILE, FIXTURES_DIR, RUN_VIEWS, SNAPSHOTS_DIR, VIEWS_DIR, denyLines, fileRules, spreadCount, statesOf, viewRules } from './files.mjs';
import { RECORD_FILE, STAMPED, pushLine, readRecord, recordText, rotation, stampOf } from './record.mjs';
import { loadHarness, pageConfirmFirst, plan, runStates, sayRule, verdict } from './run.mjs';

const COPIED = [
  VIEWS_DIR,
  FIXTURES_DIR,
  SNAPSHOTS_DIR,
  ACTS_FILE,
  join('docs', 'r4', 'page-features.txt'),
  join('tools', 'r4-page', 'package.json'),
  join('ui', 'tests', 'r4', 'needs', 'structure'),
  join('ui', 'tests', 'r4', 'agents', 'structure'),
  join('tools', 'r4-page', 'sample'),
];

const VIEW_CASES = [
  ['names-innerhtml', 'names', (t) => t + '\nvar r4x = document.body.innerHTML;\n'],
  ['names-escaped-identifier', 'names', (t) => t + '\nvar r4x = document.body.inner\\u0048TML;\n'],
  ['names-computed-key', 'names', (t) => t + "\nvar r4x = window['localStorage'];\n"],
  ['names-document-write-key', 'names', (t) => t + "\ndocument['write']('x');\n"],
  ['names-set-timeout', 'names', (t) => t + '\nsetTimeout(function () {}, 1);\n'],
  ['names-fetch-call', 'names', (t) => t + "\nwindow.fetch('/x');\n"],
  ['names-harness-path', 'names', (t) => t + "\nvar r4x = '/__r4/frame/';\n"],
  ['names-driver-global', 'names', (t) => t + '\nvar r4x = window.__playwright__binding__;\n'],
  ['names-attach-shadow', 'names', (t) => t + '\ndocument.body.attachShadow({ mode: "closed" });\n'],
  ['template-string', 'template', (t) => t + '\nvar r4x = `a`;\n'],
  ['syntax-error', 'syntax', (t) => t + '\nvar = ;\n'],
  ['syntax-after-2022', 'syntax', (t) => t + '\nvar r4x = [1].at(-1) ?? 0; using r4y = null;\n'],
  ['syntax-module-grammar', 'syntax', (t) => t + "\nimport x from './x.js';\n"],
];

const DENY = '# a planted deny line of the self-test\ndeny\tplanted_feature\t18.0\t\\.r4plantedFeature\\(\n';

const FILE_CASES = [
  ['a-denied-feature', 'deny', (d) => {
    append(d, join('docs', 'r4', 'page-features.txt'), DENY);
    append(d, join(VIEWS_DIR, 'agents.js'), '\ndocument.body.r4plantedFeature(1);\n');
  }],
  ['a-malformed-feature-line', 'features', (d) => append(d, join('docs', 'r4', 'page-features.txt'), 'deny\tonly-three\t18.0\n')],
  ['a-bad-deny-pattern', 'features', (d) => append(d, join('docs', 'r4', 'page-features.txt'), 'deny\tbad\t18.0\t(\n')],
  ['a-view-no-run-loads', 'unrun', (d) => writeFileSync(join(d, VIEWS_DIR, 'third.js'), '(function () {})();\n')],
  ['a-state-without-structure-file', 'structure', (d) => cpSync(join(d, FIXTURES_DIR, 'state-ten.json'), join(d, FIXTURES_DIR, 'state-ten_again.json'))],
  ['a-structure-file-without-state', 'structure', (d) => writeFileSync(join(d, SNAPSHOTS_DIR, 'gone.txt'), '- {"role":"main"}\n')],
  ['a-structure-file-of-another-name', 'structure', (d) => renameSync(join(d, SNAPSHOTS_DIR, 'ten.txt'), join(d, SNAPSHOTS_DIR, 'ten.text'))],
  ['a-spread-text-node', 'spread', (d) => append(d, join(SNAPSHOTS_DIR, 'ten.txt'), '- {"0":"W","1":"h"}\n')],
  ['a-spread-text-node-of-a-view', 'spread', (d) => append(d, join('ui', 'tests', 'r4', 'agents', 'structure', 'ten.txt'), '  - {"0":"x"}\n')],
  ['acts-of-an-unknown-state', 'acts', (d) => writeFileSync(join(d, ACTS_FILE), '{"no_such_state":[{"act":"open_card","param":"1"}]}\n')],
  ['acts-with-an-unknown-key', 'acts', (d) => writeFileSync(join(d, ACTS_FILE), '{"card":[{"act":"open_card","agent":"1"}]}\n')],
  ['acts-not-an-object', 'acts', (d) => writeFileSync(join(d, ACTS_FILE), '[]\n')],
  ['no-states', 'states', (d) => {
    for (const s of statesOf(join(d, FIXTURES_DIR))) renameSync(join(d, FIXTURES_DIR, 'state-' + s + '.json'), join(d, FIXTURES_DIR, 'gone-' + s + '.json'));
  }],
  ['a-playwright-pin-that-differs', 'pin', (d) => {
    mkdirSync(join(d, 'tools', 'fixture-server'), { recursive: true });
    writeFileSync(join(d, 'tools', 'fixture-server', 'package.json'), '{"dependencies":{"playwright":"1.64.0"}}\n');
  }],
];

const CSS_END = "    '.r4a-from{display:block;font-size:0.875rem}'\n";
const css = (rule) => (t) => swap(t, CSS_END, CSS_END.replace("'\n", "',\n") + "    '" + rule + "'\n");

const PAGE_CASES = [
  { name: 'check-1-low-contrast', checks: [1], plant: css('.r4a-word{color:#f7f7f7}'), want: (r) => r.line.violations > 0 },
  {
    name: 'check-2-focus-unseen',
    checks: [2],
    plant: (t) => swap(t, '.r4a-button:focus{outline-width:3px;outline-style:solid;outline-color:#0b57d0;outline-offset:2px}', '.r4a-button:focus{outline-style:none}'),
    want: (r) => r.line.keyboard === 'miss' && r.names.join().includes('focus_unseen'),
  },
  { name: 'check-3-another-heading', checks: [3], plant: (t) => swap(t, 'heading: "Your agents",', 'heading: "Your agent list",'), want: (r) => r.line.structure === 'differ' },
  {
    name: 'check-4-a-control-of-20-pixels',
    checks: [4],
    plant: css('.r4a-controls>.r4a-button{min-width:0;min-height:0;width:20px;height:20px;padding:0;overflow:hidden}'),
    want: (r) => r.line.size === 'miss',
  },
  { name: 'check-5-wider-than-320', checks: [5], plant: css('.r4a{min-width:400px}'), want: (r) => r.line.modes === 'miss' },
  {
    name: 'check-6-markup-from-a-text',
    checks: [6],
    plant: (t) => swap(t, "if (typeof text === 'string') node.appendChild(document.createTextNode(text));", "if (typeof text === 'string') node['inner' + 'HTML'] = text;"),
    want: (r) => r.line.markup === 'miss',
  },
  {
    name: 'a-status-region-that-is-rebuilt',
    checks: [],
    region: true,
    plant: (t) =>
      swap(
        t,
        '    seq = reply.seq;\n',
        "    var fresh = el('div', 'r4a-say', null);\n    fresh.setAttribute('role', 'status');\n    status.parentNode.replaceChild(fresh, status);\n    status = fresh;\n    seq = reply.seq;\n",
      ),
    want: (r) => r.region.misses.includes('agents:rebuilt'),
  },
  {
    name: 'no-status-region',
    checks: [],
    region: true,
    plant: (t) => swap(t, "      status.setAttribute('role', 'status');\n", ''),
    want: (r) => r.region.misses.includes('agents:count'),
  },
  {
    name: 'a-status-region-that-is-assertive',
    checks: [],
    region: true,
    plant: (t) => swap(t, "      status.setAttribute('role', 'status');\n", "      status.setAttribute('role', 'status');\n      status.setAttribute('aria-live', 'assertive');\n"),
    want: (r) => r.region.misses.includes('agents:not_polite'),
  },
];

function swap(text, from, to) {
  const i = text.indexOf(from);
  if (i < 0 || text.indexOf(from, i + 1) >= 0) throw new Error('planted fault does not apply once');
  return text.slice(0, i) + to + text.slice(i + from.length);
}

function append(dir, rel, text) {
  writeFileSync(join(dir, rel), readFileSync(join(dir, rel), 'utf8') + text);
}

function listOf(dir) {
  const out = [];
  const visit = (rel) => {
    const full = join(dir, rel);
    if (!existsSync(full)) return;
    if (statSync(full).isDirectory()) for (const f of readdirSync(full)) visit(rel + '/' + f);
    else out.push(rel);
  };
  for (const s of STAMPED) visit(s);
  return out;
}

const OK_LINE = ' violations=0 review=0 keyboard=ok structure=same size=ok modes=ok modes_off=- markup=ok';

function writeRecord(dir, change = (l) => l, cause = null) {
  const states = statesOf(join(dir, FIXTURES_DIR));
  const lines = [
    ...states.map((s) => 'page state=' + s + ' views=agents' + OK_LINE + ' region=ok'),
    ...states.map((s) => 'needs page state=' + s + OK_LINE),
    'page states=' + states.length + OK_LINE,
  ];
  writeFileSync(join(dir, RECORD_FILE), recordText({ ...stampOf(dir, listOf(dir)), browser: 'browser name=webkit version=27.2', lines: change(lines), cause }));
}

function recordFault(dir) {
  return readRecord(dir, statesOf(join(dir, FIXTURES_DIR)), stampOf(dir, listOf(dir)).stamp).fault || 'pass';
}

const RECORD_CASES = [
  ['record-as-written', 'pass', (d) => writeRecord(d)],
  ['record-missing', 'missing', () => {}],
  ['record-then-a-view-changed', 'stamp', (d) => {
    writeRecord(d);
    append(d, join(VIEWS_DIR, 'agents.js'), '\n');
  }],
  ['record-then-a-state-added', 'stamp', (d) => {
    writeRecord(d);
    cpSync(join(d, FIXTURES_DIR, 'state-ten.json'), join(d, FIXTURES_DIR, 'state-ten_again.json'));
  }],
  ['record-then-a-structure-file-changed', 'stamp', (d) => {
    writeRecord(d);
    append(d, join(SNAPSHOTS_DIR, 'ten.txt'), '- {"role":"note"}\n');
  }],
  ['record-then-the-views-checked-out-with-crlf', 'pass', (d) => {
    writeRecord(d);
    for (const f of ['agents.js', 'needs.js']) writeFileSync(join(d, VIEWS_DIR, f), readFileSync(join(d, VIEWS_DIR, f), 'utf8').replace(/\n/g, '\r\n'));
  }],
  ['record-of-a-run-that-failed', 'result', (d) => writeRecord(d, (l) => l, 'page')],
  ['record-without-a-run-of-a-state', 'states', (d) => writeRecord(d, (l) => l.slice(1))],
  ['record-without-a-needs-run-of-a-state', 'states', (d) => writeRecord(d, (l) => l.filter((x) => !x.startsWith('needs page state=ten ')))],
  ['record-with-a-run-that-missed', 'result', (d) => writeRecord(d, (l) => l.map((x) => (x.startsWith('page state=ten ') ? x + ' misses=2:focus_unseen' : x)))],
  ['record-with-a-region-that-missed', 'result', (d) => writeRecord(d, (l) => l.map((x) => (x.startsWith('page state=ten ') ? x.replace('region=ok', 'region=miss') : x)))],
  ['record-with-a-second-stamp', 'no_stamp', (d) => writeRecord(d, (l) => [...l, 'stamp=' + '0'.repeat(64)])],
  ['record-whose-line-counts-fewer-states', 'states', (d) => writeRecord(d, (l) => l.map((x) => (x.startsWith('page states=') ? x.replace(/states=\d+/, 'states=1') : x)))],
];

function copyRepo(repo, dir) {
  for (const rel of COPIED) {
    mkdirSync(dirname(join(dir, rel)), { recursive: true });
    cpSync(join(repo, rel), join(dir, rel), { recursive: true });
  }
  return dir;
}

function reporter(log) {
  const r = { rejected: 0, wrong: 0 };
  r.report = (name, want, got) => {
    const ok = want === got;
    log('case ' + name + ' want=' + want + ' got=' + got + (ok ? '' : ' wrong'));
    if (!ok) r.wrong++;
    else if (want !== 'pass') r.rejected++;
  };
  return r;
}

/** The cases that need no browser: the rules of a view file, the files of the check and the verdict of a whole run. */
export function fileCases({ repo, acorn: acornDir, work, report }) {
  const acorn = createRequire(join(acornDir, 'package.json'))('acorn');
  const views = join(repo, VIEWS_DIR);
  const denies = denyLines(readFileSync(join(repo, 'docs', 'r4', 'page-features.txt'), 'utf8'));
  for (const file of ['needs.js', 'agents.js']) {
    const text = readFileSync(join(views, file), 'utf8');
    const clean = viewRules(acorn, text, denies);
    report('view-' + file + '-as-it-is', 'pass', Object.values(clean).some((v) => v.length) ? 'fail' : 'pass');
    for (const [name, rule, plant] of VIEW_CASES) {
      const found = viewRules(acorn, plant(text), denies);
      const got = Object.entries(found).filter(([, v]) => v.length).map(([k]) => k);
      report(name + '-in-' + file, rule, got.length === 1 ? got[0] : got.length ? got.join('+') : 'pass');
    }
  }
  report('spread-count-of-a-bare-text-line', 'pass', spreadCount('- "Shop"\n- {"name":"Shop","role":"radio"}\n') === 0 ? 'pass' : 'fail');
  const base = copyRepo(repo, join(work, 'files-base'));
  report('files-of-the-repository', 'pass', fileRules(base, acornDir).cause || 'pass');
  for (const [name, cause, plant] of FILE_CASES) {
    const dir = copyRepo(repo, join(work, 'files-' + name));
    plant(dir);
    report(name, cause, fileRules(dir, acornDir).cause || 'pass');
  }
  const fx = join(repo, FIXTURES_DIR);
  const states = statesOf(fx);
  const runs = plan(fx, states);
  const firsts = states.filter((s) => pageConfirmFirst(JSON.parse(readFileSync(join(fx, 'state-' + s + '.json'), 'utf8')).snapshot));
  report('plan-both-views-where-a-page-confirmation-is-first', 'pass', firsts.length > 0 && runs.every((r) => (r.views.length === 2) === firsts.includes(r.state)) ? 'pass' : 'fail');
  const one = { line: {}, names: [], region: { misses: [] } };
  const needs = { parsed: true, states: states.length, code: 0, itemsFailed: 0 };
  const results = states.map((state) => ({ ...one, state }));
  const say = { misses: [] };
  report('verdict-of-a-whole-run', 'pass', verdict(states, results, needs, say) || 'pass');
  report('verdict-a-state-with-no-run', 'states', verdict(states, results.slice(1), needs, say) || 'pass');
  report('verdict-a-run-of-another-state', 'states', verdict(states, [...results.slice(1), { ...one, state: 'other' }], needs, say) || 'pass');
  report('verdict-needs-alone-read-fewer-states', 'states', verdict(states, results, { ...needs, states: states.length - 1 }, say) || 'pass');
  report('verdict-needs-alone-printed-no-line', 'states', verdict(states, results, { ...needs, parsed: false }, say) || 'pass');
  report('verdict-needs-alone-exit-1', 'page', verdict(states, results, { ...needs, code: 1 }, say) || 'pass');
  report('verdict-a-test-item-failed', 'page', verdict(states, results, { ...needs, itemsFailed: 1 }, say) || 'pass');
  report('verdict-no-states', 'states', verdict([], [], needs, say) || 'pass');
  report('verdict-a-say-missed', 'status', verdict(states, results, needs, { misses: ['agents:say'] }) || 'pass');
  for (const [name, fault, plant] of RECORD_CASES) {
    const dir = copyRepo(repo, join(work, name));
    plant(dir);
    report(name, fault, recordFault(dir));
  }
  const a = rotation('0'.repeat(40), states);
  const b = rotation('1'.repeat(40), states);
  report('rotation-reads-every-state-once', 'pass', a.slice().sort().join() === states.join() ? 'pass' : 'fail');
  report('rotation-of-one-commit-is-the-same', 'pass', rotation('0'.repeat(40), states).join() === a.join() ? 'pass' : 'fail');
  report('rotation-of-another-commit-reads-others-first', 'pass', a.slice(0, 3).join() !== b.slice(0, 3).join() ? 'pass' : 'fail');
  const okLine = 'page states=' + states.length + OK_LINE;
  const missed = [{ line: { ...one.line, violations: 0, review: 0, keyboard: 'miss', structure: 'same', size: 'ok', modes: 'ok', modes_off: '-', markup: 'ok' } }];
  report('push-line-shows-a-miss-of-the-push', 'pass', / keyboard=miss /.test(pushLine(states.length, okLine, missed, null)) ? 'pass' : 'fail');
  report('push-line-of-a-clean-push', 'pass', pushLine(states.length, okLine, [], null) === okLine ? 'pass' : 'fail');
}

/** The cases that run the views in the browser of the machine: an unchanged control and one planted fault per check and per region rule. */
export async function pageCases({ repo, work, report, log }) {
  const fx = join(repo, FIXTURES_DIR);
  const views = join(repo, VIEWS_DIR);
  const harness = await loadHarness(repo);
  const agents = readFileSync(join(views, 'agents.js'), 'utf8').replace(/\r\n/g, '\n');
  const acts = JSON.parse(readFileSync(join(repo, ACTS_FILE), 'utf8'));
  const snapshots = join(repo, SNAPSHOTS_DIR);
  const state = 'ten';
  const control = join(work, 'ui-control');
  cpSync(join(repo, 'ui', 'app'), join(control, 'app'), { recursive: true });
  const controlOrigin = await harness.serve({ ui: control, fixtures: fx });
  const [clean] = await runStates(harness, { origin: controlOrigin, runs: plan(fx, [state]), snapshots, acts, write: false, log });
  report('page-control-' + state + '-all-six-and-the-region', 'pass', clean.ok ? 'pass' : 'fail');
  const setup = statesOf(fx).find((s) => s.startsWith('setup'));
  const written = join(work, 'structure-' + setup + '.txt');
  const page = await harness.open({ origin: controlOrigin, state: setup, views: [RUN_VIEWS['needs.js']] });
  try {
    await harness.check(page, 3, { snapshot: written, write: true });
  } finally {
    await page.close();
  }
  const text = readFileSync(written, 'utf8');
  const whole = text.split('\n').filter((l) => /^ *- "/.test(l)).length;
  report('structure-writer-keeps-a-text-node-whole', 'pass', whole > 0 && spreadCount(text) === 0 ? 'pass' : 'fail');
  for (const c of PAGE_CASES) {
    const ui = join(work, 'ui-' + c.name);
    let got;
    try {
      cpSync(join(repo, 'ui', 'app'), join(ui, 'app'), { recursive: true });
      writeFileSync(join(ui, 'app', 'views', 'agents.js'), c.plant(agents));
      const origin = await harness.serve({ ui, fixtures: fx });
      const [r] = await runStates(harness, { origin, runs: plan(fx, [state]), snapshots, acts, write: false, log, checks: c.checks, region: Boolean(c.region) });
      got = !r.ok && c.want(r) ? 'caught' : 'missed';
    } catch (err) {
      got = 'error_' + String((err && (err.code || err.name)) || 'unknown').replace(/[^A-Za-z0-9_]/g, '_');
    }
    report(c.name, 'caught', got);
  }
  const quiet = join(work, 'ui-a-status-change-that-adds-no-say');
  let said;
  try {
    cpSync(join(repo, 'ui', 'app'), join(quiet, 'app'), { recursive: true });
    writeFileSync(join(quiet, 'app', 'views', 'agents.js'), swap(agents, "      status.appendChild(el('p', null, s));\n", ''));
    const r = await sayRule(harness, await harness.serve({ ui: quiet, fixtures: fx }), fx, statesOf(fx));
    said = r.misses.includes('agents:say') ? 'caught' : 'missed';
  } catch (err) {
    said = 'error_' + String((err && (err.code || err.name)) || 'unknown').replace(/[^A-Za-z0-9_]/g, '_');
  }
  report('a-status-change-that-adds-no-say', 'caught', said);
  const kept = await sayRule(harness, controlOrigin, fx, statesOf(fx));
  report('say-control-of-the-unchanged-view', 'pass', kept.read === 1 && kept.misses.length === 0 ? 'pass' : 'fail');
}

/** The self-test of 40-r4-page: every planted fault is rejected by its rule or check, and the unchanged copies pass. */
export async function selfTest({ repo, acorn, log }) {
  const work = mkdtempSync(join(tmpdir(), 'r4-page-selftest-'));
  const r = reporter(log);
  fileCases({ repo, acorn, work, report: r.report });
  await pageCases({ repo, work, report: r.report, log });
  log('self-test rejected=' + r.rejected + ' wrong=' + r.wrong);
  return r.wrong === 0 && r.rejected > 0;
}

export { reporter };
