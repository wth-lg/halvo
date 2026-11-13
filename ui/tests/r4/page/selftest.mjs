import { execFileSync, spawnSync } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { ACTS_FILE, FIXTURES_DIR, NAMES, RUN_VIEWS, SNAPSHOTS_DIR, VIEWS_DIR, denyLines, fileRules, spreadCount, statesOf, viewRules } from './files.mjs';
import { RECORD_FILE, committedRecord, factsOf, packagesCheck, readRecord, recordForm, stampOf, treeBeside } from './record.mjs';
import { loadHarness, pageConfirmFirst, plan, runStates, sayRule, verdict } from './run.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));

const COPIED = [
  VIEWS_DIR,
  FIXTURES_DIR,
  SNAPSHOTS_DIR,
  ACTS_FILE,
  join('docs', 'r4', 'page-features.txt'),
  join('tools', 'r4-page', 'package.json'),
  join('tools', 'r4-page', 'package-lock.json'),
  join('tools', 'r4-page', 'browser.txt'),
  join('ui', 'index.html'),
  join('ui', 'tests', 'r4', 'needs', 'structure'),
  join('ui', 'tests', 'r4', 'agents', 'structure'),
  join('ui', 'tests', 'r4', 'needs', 'items.json'),
  join('tools', 'r4-page', 'sample'),
];

const I8_4_NAMES = [
  'setTimeout', 'setInterval', 'requestAnimationFrame', 'requestIdleCallback', 'localStorage', 'sessionStorage', 'indexedDB',
  'fetch', 'XMLHttpRequest', 'WebSocket', 'EventSource', 'sendBeacon', 'innerHTML', 'outerHTML', 'insertAdjacentHTML',
  'setHTMLUnsafe', 'parseHTMLUnsafe', 'createContextualFragment', 'DOMParser', 'document.write', 'attachShadow',
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
  ['syntax-after-2022', 'syntax', (t) => t + '\nvar r4x = /[a]/v;\n'],
  ['names-escaped-global-call', 'names', (t) => t + '\n\\u0073etTimeout(function () {}, 1);\n'],
  ['syntax-module-grammar', 'syntax', (t) => t + "\nimport x from './x.js';\n"],
  ['names-write-of-an-alias', 'names', (t) => t + '\nfunction r4z() { var d = document; d.write("<p>x</p>"); }\n'],
  ['names-a-with-statement', 'names', (t) => t + '\nfunction r4z() { with (document) body; }\n'],
  ['names-a-joined-key-set-timeout', 'names', (t) => t + '\nwindow["set" + "Timeout"](function () {}, 0);\n'],
  ['names-a-joined-key-fetch', 'names', (t) => t + '\nself["fe" + "tch"]("/x");\n'],
  ['names-a-decoded-string-carried-to-a-key', 'names', (t) => t + '\nvar r4k = "\\u0069nnerHTML"; document.body[r4k] = "x";\n'],
  ['a-name-that-ends-in-a-forbidden-name', 'pass', (t) => t + '\nvar prefetch = 1;\n'],
  ['a-name-that-starts-with-a-forbidden-name', 'pass', (t) => t + '\nvar fetcher = 1;\n'],
  ...I8_4_NAMES.map((n) => ['names-each-' + n.replace('.', '-'), 'names', (t) => t + '\n' + (n === 'document.write' ? "document.write('x');" : 'var r4n = window.' + n + ';') + '\n']),
];

const DENY = '# a planted deny line of the self-test\ndeny\tplanted_feature\t18.0\t\\.r4plantedFeature\\(\n';

const FILE_CASES = [
  ['a-denied-feature', 'deny', (d) => {
    append(d, join('docs', 'r4', 'page-features.txt'), DENY);
    append(d, join(VIEWS_DIR, 'agents.js'), '\ndocument.body.r4plantedFeature(1);\n');
  }],
  ['a-deny-pattern-that-needs-the-u-flag', 'deny', (d) => {
    append(d, join('docs', 'r4', 'page-features.txt'), '# a planted deny line of the self-test\ndeny\tplanted_unicode\t18.0\t\\.r4\\p{Lu}planted\\(\n');
    append(d, join(VIEWS_DIR, 'agents.js'), '\ndocument.body.r4Xplanted(1);\n');
  }],
  ['no-view-file', 'views', (d) => {
    for (const f of ['agents.js', 'needs.js']) renameSync(join(d, VIEWS_DIR, f), join(d, f));
  }],
  ['a-malformed-feature-line', 'features', (d) => append(d, join('docs', 'r4', 'page-features.txt'), 'deny\tonly-three\t18.0\n')],
  ['a-bad-deny-pattern', 'features', (d) => append(d, join('docs', 'r4', 'page-features.txt'), 'deny\tbad\t18.0\t(\n')],
  ['a-view-no-run-loads', 'unrun', (d) => writeFileSync(join(d, VIEWS_DIR, 'third.js'), '(function () {})();\n')],
  ['a-state-without-structure-file', 'structure', (d) => cpSync(join(d, FIXTURES_DIR, 'state-ten.json'), join(d, FIXTURES_DIR, 'state-ten_again.json'))],
  ['a-structure-file-without-state', 'structure', (d) => writeFileSync(join(d, SNAPSHOTS_DIR, 'gone.txt'), '- {"role":"main"}\n')],
  ['a-structure-file-removed', 'structure', (d) => renameSync(join(d, SNAPSHOTS_DIR, 'ten.txt'), join(d, 'ten.txt'))],
  ['a-structure-file-of-another-name', 'structure', (d) => renameSync(join(d, SNAPSHOTS_DIR, 'ten.txt'), join(d, SNAPSHOTS_DIR, 'ten.text'))],
  ['a-view-file-in-a-subdirectory', 'unrun', (d) => {
    mkdirSync(join(d, VIEWS_DIR, 'parts'), { recursive: true });
    writeFileSync(join(d, VIEWS_DIR, 'parts', 'helper.js'), '(function () {})();\n');
  }],
  ['a-view-file-of-another-kind', 'unrun', (d) => writeFileSync(join(d, VIEWS_DIR, 'extra.mjs'), '(function () {})();\n')],
  ['words-in-a-file-in-a-subdirectory', 'names', (d) => {
    mkdirSync(join(d, VIEWS_DIR, 'parts'), { recursive: true });
    writeFileSync(join(d, VIEWS_DIR, 'parts', 'helper.js'), 'document.body.innerHTML = 1;\n');
  }],
  ['a-needs-structure-file-missing', 'structure', (d) => renameSync(join(d, 'ui', 'tests', 'r4', 'needs', 'structure', 'card.txt'), join(d, 'ui', 'tests', 'r4', 'needs', 'card.txt'))],
  ['a-stale-agents-structure-file', 'structure', (d) => writeFileSync(join(d, 'ui', 'tests', 'r4', 'agents', 'structure', 'ghost.txt'), '- {"role":"main"}\n')],
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
    name: 'a-status-region-that-is-hidden',
    checks: [],
    region: true,
    plant: css('.r4a-say{display:none}'),
    want: (r) => r.region.misses.includes('agents:hidden'),
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

function git(dir, ...args) {
  return execFileSync('git', ['-C', dir, ...args], { stdio: ['ignore', 'pipe', 'ignore'] }).toString('utf8');
}

/** A fixture repository whose index holds its files, with no commit. */
function indexed(dir, crlf = false) {
  git(dir, 'init', '-q');
  if (crlf) git(dir, 'config', 'core.autocrlf', 'true');
  git(dir, 'add', '-A');
  return dir;
}

function counted(names) {
  return new Map(names.map((n, i) => [n, (i % 9) + 1]));
}

/** A record of a passing run of the fixture, as the writer writes it, through one change of its text. */
function writeRecord(dir, change = (x) => x) {
  git(dir, 'add', '-A');
  const f = factsOf(dir);
  const text = recordForm({ ...f, version: '27.2', own: counted(f.all), needs: counted([...f.all, ...f.items]) });
  writeFileSync(join(dir, RECORD_FILE), change(text));
}

function recordFault(dir) {
  git(dir, 'add', '-A');
  return readRecord(committedRecord(dir), factsOf(dir)).fault || 'pass';
}

const line = (start, change) => (text) => text.split('\n').map((l) => (l.startsWith(start) ? change(l) : l)).join('\n');
const drop = (start) => (text) => text.split('\n').filter((l) => !l.startsWith(start)).join('\n');

const RECORD_CASES = [
  ['record-as-written', 'pass', (d) => writeRecord(d)],
  ['record-missing', 'missing', () => {}],
  ['record-then-a-view-changed', 'stamp', (d) => {
    writeRecord(d);
    append(d, join(VIEWS_DIR, 'agents.js'), '\n');
  }],
  ['record-then-a-lone-carriage-return-in-a-view', 'stamp', (d) => {
    writeRecord(d);
    const p = join(d, VIEWS_DIR, 'agents.js');
    writeFileSync(p, readFileSync(p, 'utf8').replace('\n', '\r\n'));
  }],
  ['record-then-a-file-under-ui-outside-the-views-changed', 'stamp', (d) => {
    writeRecord(d);
    append(d, join('ui', 'index.html'), '\n');
  }],
  ['record-then-a-line-of-the-harness-lock-changed', 'stamp', (d) => {
    writeRecord(d);
    const p = join(d, 'tools', 'r4-page', 'package-lock.json');
    writeFileSync(p, readFileSync(p, 'utf8').replace('"lockfileVersion": 3', '"lockfileVersion": 3 '));
  }],
  ['record-then-a-state-added', 'stamp', (d) => {
    writeRecord(d);
    cpSync(join(d, FIXTURES_DIR, 'state-ten.json'), join(d, FIXTURES_DIR, 'state-ten_again.json'));
  }],
  ['record-then-a-structure-file-changed', 'stamp', (d) => {
    writeRecord(d);
    append(d, join(SNAPSHOTS_DIR, 'ten.txt'), '- {"role":"note"}\n');
  }],
  ['record-with-files-999', 'stamp', (d) => writeRecord(d, line('stamp=', (l) => l.replace(/files=\d+/, 'files=999')))],
  ['record-with-violations-1-on-a-line', 'form', (d) => writeRecord(d, line('page state=ten ', (l) => l.replace('violations=0', 'violations=1')))],
  ['record-with-markup-miss-and-no-miss-name', 'form', (d) => writeRecord(d, line('page state=ten ', (l) => l.replace('markup=ok', 'markup=miss')))],
  ['record-with-region-not-read-on-a-line', 'form', (d) => writeRecord(d, line('page state=ten ', (l) => l.replace('region=ok', 'region=-')))],
  ['record-whose-regions-line-says-say-miss', 'form', (d) => writeRecord(d, line('page regions ', (l) => l.replace('say=ok', 'say=miss')))],
  ['record-without-its-regions-line', 'form', (d) => writeRecord(d, drop('page regions '))],
  ['record-with-failed-test-items', 'form', (d) => writeRecord(d, line('needs page test_items=', (l) => l.replace('failed=0', 'failed=7')))],
  ['record-whose-needs-run-exited-1', 'form', (d) => writeRecord(d, line('page needs_alone ', (l) => l.replace('exit=0', 'exit=1')))],
  ['record-without-the-line-of-a-test-item', 'form', (d) => writeRecord(d, (x) => drop('needs page state=' + Object.keys(JSON.parse(readFileSync(join(d, 'ui', 'tests', 'r4', 'needs', 'items.json'), 'utf8')))[0] + ' ')(x))],
  ['record-with-a-key-twice-on-a-line', 'form', (d) => writeRecord(d, line('page state=ten ', (l) => l.replace('keyboard=ok', 'keyboard=miss keyboard=ok')))],
  ['record-of-another-engine', 'form', (d) => writeRecord(d, line('browser name=', (l) => l.replace('name=webkit', 'name=chromium')))],
  ['record-without-a-browser-line', 'form', (d) => writeRecord(d, drop('browser name='))],
  ['record-with-a-line-after-the-result', 'form', (d) => writeRecord(d, (x) => x + 'page state=ten\n')],
  ['record-with-another-first-line', 'form', (d) => writeRecord(d, (x) => x.replace('page record: ', 'page record by hand: '))],
  ['record-of-a-run-that-failed', 'form', (d) => writeRecord(d, (x) => x.replace('result=ok', 'result=page'))],
  ['record-without-a-run-of-a-state', 'form', (d) => writeRecord(d, drop('page state=ten '))],
  ['record-without-a-needs-run-of-a-state', 'form', (d) => writeRecord(d, drop('needs page state=ten '))],
  ['record-with-a-run-that-missed', 'form', (d) => writeRecord(d, line('page state=ten ', (l) => l + ' misses=2:focus_unseen'))],
  ['record-of-a-confirmation-state-with-one-view', 'form', (d) => writeRecord(d, line('page state=needs_confirm_page ', (l) => l.replace('views=both', 'views=agents')))],
  ['record-whose-joined-line-shows-a-miss', 'form', (d) => writeRecord(d, line('page states=', (l) => l.replace('structure=same', 'structure=differ')))],
  ['record-whose-joined-line-counts-another-review', 'form', (d) => writeRecord(d, line('page states=', (l) => l.replace(/review=\d+/, 'review=1')))],
  ['record-with-a-second-stamp', 'form', (d) => writeRecord(d, (x) => x.replace('result=ok\n', 'stamp=' + '0'.repeat(64) + '\nresult=ok\n'))],
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
  report('the-21-names-of-i8-4', 'pass', I8_4_NAMES.length === 21 && NAMES.length === 21 && I8_4_NAMES.every((n) => NAMES.includes(n)) ? 'pass' : 'fail');
  report('spread-count-of-a-bare-text-line', 'pass', spreadCount('- "Shop"\n- {"name":"Shop","role":"radio"}\n') === 0 ? 'pass' : 'fail');
  const base = copyRepo(repo, join(work, 'files-base'));
  report('files-of-the-repository', 'pass', fileRules(base, acornDir).cause || 'pass');
  for (const [name, cause, plant] of FILE_CASES) {
    const dir = copyRepo(repo, join(work, 'files-' + name));
    plant(dir);
    report(name, cause, fileRules(dir, acornDir).cause || 'pass');
  }
  const lacking = indexed(copyRepo(repo, join(work, 'a-view-that-the-commit-holds-and-the-tree-lacks')));
  renameSync(join(lacking, VIEWS_DIR, 'agents.js'), join(lacking, 'agents.js'));
  report('a-view-that-the-commit-holds-and-the-tree-lacks', 'views', fileRules(lacking, acornDir).cause || 'pass');
  const beside = indexed(copyRepo(repo, join(work, 'tree-beside')));
  report('tree-beside-a-clean-index', 'pass', treeBeside(beside).length === 0 ? 'pass' : 'fail');
  writeFileSync(join(beside, RECORD_FILE), 'x\n');
  report('tree-beside-leaves-the-record-out', 'pass', treeBeside(beside).length === 0 ? 'pass' : 'fail');
  writeFileSync(join(beside, 'ui', 'extra.txt'), 'x\n');
  report('tree-beside-an-untracked-file', 'listed', treeBeside(beside).join() === 'ui/extra.txt' ? 'listed' : 'pass');
  append(beside, join(VIEWS_DIR, 'agents.js'), '\n');
  report('tree-beside-a-changed-tracked-file', 'listed', treeBeside(beside).includes('ui/app/views/agents.js') ? 'listed' : 'pass');
  const fx = join(repo, FIXTURES_DIR);
  const states = statesOf(fx);
  const runs = plan(fx, states);
  const firstItem = (s) => (JSON.parse(readFileSync(join(fx, 'state-' + s + '.json'), 'utf8')).snapshot.items || [])[0] || {};
  const firsts = states.filter((s) => firstItem(s).kind === 'confirm' && firstItem(s).presenter === 'page');
  const otherConfirms = states.filter((s) => firstItem(s).kind === 'confirm' && firstItem(s).presenter !== 'page');
  report(
    'plan-both-views-where-a-page-confirmation-is-first',
    'pass',
    firsts.length > 0 && otherConfirms.length > 0 && runs.every((r) => (r.views.length === 2) === firsts.includes(r.state) && r.label === (r.views.length === 2 ? 'both' : 'agents')) ? 'pass' : 'fail',
  );
  report('page-confirm-first-of-a-native-confirmation', 'pass', otherConfirms.every((s) => !pageConfirmFirst(JSON.parse(readFileSync(join(fx, 'state-' + s + '.json'), 'utf8')).snapshot)) ? 'pass' : 'fail');
  const one = { line: {}, names: [], region: { read: 1, misses: [] }, ok: true };
  const needs = { parsed: true, states: states.length, code: 0, itemsFailed: 0 };
  const results = runs.map((r) => ({ ...r, ...one }));
  const say = { read: 1, misses: [] };
  report('verdict-of-a-whole-run', 'pass', verdict(states, results, needs, say) || 'pass');
  report('verdict-a-state-with-no-run', 'states', verdict(states, results.slice(1), needs, say) || 'pass');
  report('verdict-a-run-of-another-state', 'states', verdict(states, [...results.slice(1), { ...one, state: 'other' }], needs, say) || 'pass');
  report('verdict-needs-alone-read-fewer-states', 'states', verdict(states, results, { ...needs, states: states.length - 1 }, say) || 'pass');
  report('verdict-needs-alone-printed-no-line', 'states', verdict(states, results, { ...needs, parsed: false }, say) || 'pass');
  report('verdict-needs-alone-exit-1', 'page', verdict(states, results, { ...needs, code: 1 }, say) || 'pass');
  report('verdict-a-test-item-failed', 'page', verdict(states, results, { ...needs, itemsFailed: 1 }, say) || 'pass');
  report('verdict-no-states', 'states', verdict([], [], needs, say) || 'pass');
  report('verdict-a-say-missed', 'status', verdict(states, results, needs, { read: 1, misses: ['agents:say'] }) || 'pass');
  const missedCheck = results.map((r, i) => (i === 0 ? { ...r, names: ['2:focus_unseen'], ok: false } : r));
  report('verdict-a-check-that-missed', 'page', verdict(states, missedCheck, needs, say) || 'pass');
  const missedRegion = results.map((r, i) => (i === 0 ? { ...r, region: { read: 1, misses: ['agents:rebuilt'] }, ok: false } : r));
  report('verdict-a-region-that-missed', 'status', verdict(states, missedRegion, needs, say) || 'pass');
  for (const [name, fault, plant] of RECORD_CASES) {
    const dir = indexed(copyRepo(repo, join(work, name)));
    plant(dir);
    report(name, fault, recordFault(dir));
  }
  const crlf = indexed(copyRepo(repo, join(work, 'record-then-the-views-checked-out-with-crlf')), true);
  writeRecord(crlf);
  for (const f of ['agents.js', 'needs.js']) writeFileSync(join(crlf, VIEWS_DIR, f), readFileSync(join(crlf, VIEWS_DIR, f), 'utf8').replace(/\n/g, '\r\n'));
  report('record-then-the-views-checked-out-with-crlf', 'pass', recordFault(crlf));
  const real = process.env.R4_PAGE_BROWSERS || join(repo, 'tools', 'r4-page', '.browsers');
  report('packages-of-the-browsers-directory', 'pass', packagesCheck(repo, real).cause || 'pass');
  const lockDiffers = join(work, 'packages-lock-differs');
  mkdirSync(lockDiffers, { recursive: true });
  const lock = JSON.parse(readFileSync(join(repo, 'tools', 'r4-page', 'package-lock.json'), 'utf8'));
  for (const key of Object.keys(lock.packages || {}).filter((k) => k.startsWith('node_modules/'))) {
    mkdirSync(join(lockDiffers, key), { recursive: true });
    if (existsSync(join(real, key, 'package.json'))) cpSync(join(real, key, 'package.json'), join(lockDiffers, key, 'package.json'));
  }
  cpSync(join(repo, 'tools', 'r4-page', 'package.json'), join(lockDiffers, 'package.json'));
  writeFileSync(join(lockDiffers, 'package-lock.json'), readFileSync(join(repo, 'tools', 'r4-page', 'package-lock.json'), 'utf8') + ' ');
  report('packages-a-copy-of-the-lock-differs', 'browser', packagesCheck(repo, lockDiffers).cause || 'pass');
  const versionDiffers = join(work, 'packages-a-version-differs');
  mkdirSync(versionDiffers, { recursive: true });
  cpSync(join(lockDiffers, 'node_modules'), join(versionDiffers, 'node_modules'), { recursive: true });
  for (const f of ['package.json', 'package-lock.json']) cpSync(join(repo, 'tools', 'r4-page', f), join(versionDiffers, f));
  report('packages-copies-as-committed', 'pass', packagesCheck(repo, versionDiffers).cause || 'pass');
  const pw = join(versionDiffers, 'node_modules', 'playwright', 'package.json');
  writeFileSync(pw, JSON.stringify({ ...JSON.parse(readFileSync(pw, 'utf8')), version: '0.0.1' }));
  report('packages-a-version-differs', 'browser', packagesCheck(repo, versionDiffers).cause || 'pass');
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
  const SAY_CASES = [
    ['a-region-rebuilt-at-a-status-change', 'agents:rebuilt', (t) => swap(t, '    says.forEach(function (s) {\n', "    if (says.length) {\n      var fresh = el('div', 'r4a-say', null);\n      fresh.setAttribute('role', 'status');\n      status.parentNode.replaceChild(fresh, status);\n      status = fresh;\n    }\n    says.forEach(function (s) {\n")],
    ['a-region-hidden-once-it-holds-a-say', 'agents:hidden', css('.r4a-say:not(:empty){display:none}')],
    ['a-row-that-takes-no-press', 'agents:press', (t) => swap(t, "    var li = el('li', 'r4a-row', null);\n", "    var li = el('li', 'r4a-row', null);\n    li.inert = true;\n")],
    ['a-say-set-as-markup', 'agents:markup', (t) => swap(t, "      status.appendChild(el('p', null, s));\n", "      var said = el('p', null, null);\n      said['inner' + 'HTML'] = s;\n      status.appendChild(said);\n")],
  ];
  for (const [name, miss, plant] of SAY_CASES) {
    const ui = join(work, 'ui-' + name);
    let got;
    try {
      cpSync(join(repo, 'ui', 'app'), join(ui, 'app'), { recursive: true });
      writeFileSync(join(ui, 'app', 'views', 'agents.js'), plant(agents));
      const r = await sayRule(harness, await harness.serve({ ui, fixtures: fx }), fx, statesOf(fx));
      got = r.misses.includes(miss) ? 'caught' : 'missed';
    } catch (err) {
      got = 'error_' + String((err && (err.code || err.name)) || 'unknown').replace(/[^A-Za-z0-9_]/g, '_');
    }
    report(name, 'caught', got);
  }
  const kept = await sayRule(harness, controlOrigin, fx, statesOf(fx));
  report('say-control-of-the-unchanged-view', 'pass', kept.read === 1 && kept.misses.length === 0 ? 'pass' : 'fail');
  await wholeCases({ repo, work, report });
}

const SMALL_STATES = ['empty', 'ten'];
const SMALL_ITEM = 'test_changed';

/** A small fixture repository: the stamped files of the repository with two states and one test item, its index holding them. */
function smallRepo(repo, dir) {
  const files = git(repo, 'ls-files', '-z', '--', 'ui', 'tools/r4-page', 'docs/r4/page-features.txt', 'scripts/r4/ci.d/40-r4-page.sh').split('\0').filter(Boolean);
  for (const f of files) {
    if (f === 'ui/tests/r4/page/record.txt') continue;
    const state = /^ui\/tests\/fixtures\/coord\/state-(.+)\.json$/.exec(f);
    if (state && !SMALL_STATES.includes(state[1])) continue;
    const structure = /^ui\/tests\/r4\/(?:snapshots|agents\/structure|needs\/structure)\/(.+)\.txt$/.exec(f);
    if (structure && !SMALL_STATES.includes(structure[1]) && structure[1] !== SMALL_ITEM) continue;
    mkdirSync(dirname(join(dir, f)), { recursive: true });
    cpSync(join(repo, f), join(dir, f));
  }
  const items = JSON.parse(readFileSync(join(repo, 'ui', 'tests', 'r4', 'needs', 'items.json'), 'utf8'));
  writeFileSync(join(dir, 'ui', 'tests', 'r4', 'needs', 'items.json'), JSON.stringify({ [SMALL_ITEM]: items[SMALL_ITEM] }) + '\n');
  return indexed(dir);
}

function runCheck(repo, args) {
  const env = { ...process.env, R4_PAGE_BROWSERS: process.env.R4_PAGE_BROWSERS || join(repo, 'tools', 'r4-page', '.browsers') };
  const r = spawnSync(process.execPath, [join(HERE, 'check.mjs'), ...args], { env, encoding: 'utf8' });
  const lines = (r.stdout || '').split('\n').filter(Boolean);
  const last = lines.length ? lines[lines.length - 1] : '';
  return { status: r.status, lines, cause: last.startsWith('check result=') ? last.slice('check result='.length) : 'exit-' + r.status };
}

/** The whole run of the macos job on a small fixture: --record writes a record that passes; a hand record over a wrong structure file and a record one byte away from the fresh one fail. */
export async function wholeCases({ repo, work, report }) {
  const fx = smallRepo(repo, join(work, 'whole-small'));
  const written = runCheck(repo, ['record', '--repo', fx]);
  report('whole-record-of-a-small-fixture', 'pass', written.status === 0 && written.cause === 'ok' ? 'pass' : written.cause);
  const text = committedRecord(fx) || '';
  const byHand = join(work, 'whole-a-hand-record-over-a-wrong-structure-file');
  cpSync(fx, byHand, { recursive: true });
  const p = join(byHand, SNAPSHOTS_DIR, 'ten.txt');
  writeFileSync(p, readFileSync(p, 'utf8').replace('"name":"Your agents"', '"name":"Your agent list"'));
  git(byHand, 'add', '-A');
  const f = stampOf(byHand);
  writeFileSync(join(byHand, RECORD_FILE), text.replace(/^stamp=\S+ files=\d+$/m, 'stamp=' + f.stamp + ' files=' + f.files));
  const windows = runCheck(repo, ['page', '--repo', byHand]);
  const ran = windows.lines.some((l) => l === 'page ran states=' + SMALL_STATES.length + ' of ' + SMALL_STATES.length + ' needs_states=' + SMALL_STATES.length + ' test_items=1');
  report('windows-form-runs-every-state-and-fails-on-a-broken-one', 'page', ran ? windows.cause : 'ran-' + windows.cause);
  report('whole-a-hand-record-over-a-wrong-structure-file', 'page', runCheck(repo, ['whole', '--repo', byHand, '--out', join(work, 'fresh-a.txt')]).cause);
  const stale = join(work, 'whole-a-stale-record-fails-before-the-run');
  cpSync(fx, stale, { recursive: true });
  append(stale, join('ui', 'index.html'), '\n');
  git(stale, 'add', '-A');
  const s = runCheck(repo, ['whole', '--repo', stale, '--out', join(work, 'fresh-s.txt')]);
  const before = s.lines.some((l) => l.startsWith('page record fault=stamp')) && !s.lines.some((l) => l.startsWith('page state='));
  report('whole-a-stale-record-fails-before-the-run', 'record', before ? s.cause : 'ran-' + s.cause);
  const oneByte = join(work, 'whole-a-record-one-byte-away');
  cpSync(fx, oneByte, { recursive: true });
  writeFileSync(join(oneByte, RECORD_FILE), text.replace(/^(page state=ten .* review=)(\d+)/m, (m, a, n) => a + (Number(n) + 1)));
  report('whole-a-record-one-byte-away', 'record', runCheck(repo, ['whole', '--repo', oneByte, '--out', join(work, 'fresh-b.txt')]).cause);
}

/** The self-test of 40-r4-page: every planted fault is rejected by its rule or check, and the unchanged copies pass. */
export async function selfTest({ repo, acorn, work, log }) {
  mkdirSync(work, { recursive: true });
  const r = reporter(log);
  fileCases({ repo, acorn, work, report: r.report });
  await pageCases({ repo, work, report: r.report, log });
  log('self-test rejected=' + r.rejected + ' wrong=' + r.wrong);
  return r.wrong === 0 && r.rejected > 0;
}

export { reporter };
