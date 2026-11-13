import fs from 'node:fs';
import path from 'node:path';

const NAME = '45-r4-words';
const CLASSES = ['model', 'tier', 'count', 'tool', 'bundle', 'path', 'address', 'enum', 'code', 'trace', 'prompt'];
const RUNS = ['capture', 'workflow', 'agents', 'interface', 'billing'];
const THIS_RUN = 'interface';
const FIXED_TERMS = ['paused|capture|yes', 'cap|capture|yes', 'delete|workflow|yes', 'approve|workflow|yes'];
const STATES = [
  'empty',
  'ten',
  'needs_confirm_page',
  'needs_confirm_native',
  'needs_sign_in',
  'needs_link_account',
  'needs_stuck',
  'needs_check',
  'needs_setup',
  'needs_ready',
  'needs_show',
  'setup_three',
  'card',
  'problem',
  'off',
  'working',
  'queue',
  'no_service',
  'needs_sign_in_bind',
  'needs_confirm_mixed',
  'needs_confirm_changed',
];
const FIXTURES = 'ui/tests/fixtures/coord';
const WORDS = `${FIXTURES}/words.json`;
const VIEWS = ['ui/app/views/needs.js', 'ui/app/views/agents.js'];
const APP = 'ui/app';
const LIST = 'scripts/r4/words/never-read.txt';
const CASES = 'scripts/r4/words/never-read-cases.txt';
const ALLOWED = 'scripts/r4/words/allowed.txt';
const GLOSSARY = 'docs/r4/words.md';
const NAMES = ['crates/halvo-coord/src/types/names.rs', 'crates/halvo-coord/src/types/cause.rs'];
const MAX_WORDS = 20;
const HEADING = /^\s*heading: "([^"\\]*)",$/u;

const FIXTURE_KEYS = [
  'snapshot.rows.*.name',
  'snapshot.rows.*.word',
  'snapshot.rows.*.line',
  'snapshot.rows.*.say',
  'snapshot.rows.*.level',
  'snapshot.rows.*.next',
  'snapshot.rows.*.controls.*.text',
  'snapshot.items.*.title',
  'snapshot.items.*.text',
  'snapshot.items.*.fields.*.label',
  'snapshot.items.*.fields.*.from_text',
  'snapshot.items.*.params.*.label',
  'snapshot.items.*.choices.*.text',
  'snapshot.note',
  'card.name',
  'card.lines.*.text',
  'card.runs.*.day',
  'card.runs.*.text',
  'card.runs.*.effects.*.text',
  'card.runs.*.effects.*.fields.*.label',
  'card.runs.*.effects.*.fields.*.from_text',
];
const CONTROL_TEXT = 'snapshot.rows.*.controls.*.text';

class Stop extends Error {
  constructor(why, file) {
    super(`${why} ${file}`);
    this.why = why;
    this.file = file;
  }
}

function splitLines(text) {
  return text.split('\n').map((l) => (l.endsWith('\r') ? l.slice(0, -1) : l));
}

function dataLines(text) {
  return splitLines(text).filter((l) => l !== '' && !l.startsWith('#'));
}

function readText(repo, rel, why) {
  try {
    return fs.readFileSync(path.join(repo, rel), 'utf8');
  } catch {
    throw new Stop(why, rel);
  }
}

function parseList(text) {
  const patterns = [];
  for (const line of dataLines(text)) {
    const tab = line.indexOf('\t');
    const cls = tab < 0 ? '' : line.slice(0, tab);
    const source = tab < 0 ? '' : line.slice(tab + 1);
    if (!CLASSES.includes(cls) || source === '') throw new Stop('list', LIST);
    let re;
    try {
      re = new RegExp(source, 'u');
    } catch {
      throw new Stop('list', LIST);
    }
    patterns.push({ cls, re });
  }
  for (const cls of CLASSES) {
    if (!patterns.some((p) => p.cls === cls)) throw new Stop('list', LIST);
  }
  return patterns;
}

function closedNames(repo) {
  const names = new Set();
  for (const rel of NAMES) {
    const found = splitLines(readText(repo, rel, 'input'))
      .map((l) => /^\s*[A-Z][A-Za-z0-9]* = "([a-z][a-z0-9_]*)",$/u.exec(l))
      .filter(Boolean);
    if (found.length === 0) throw new Stop('input', rel);
    for (const m of found) if (m[1].includes('_')) names.add(m[1]);
  }
  if (names.size === 0) throw new Stop('input', NAMES[0]);
  const alt = [...names].sort().map((n) => n.split('_').join('[-_]')).join('|');
  return { cls: 'enum', re: new RegExp(`(?<![\\p{L}\\p{N}])(?:${alt})(?![\\p{L}\\p{N}])`, 'iu') };
}

function cells(line) {
  const t = line.trim();
  if (!t.startsWith('|') || !t.endsWith('|') || t.length < 2) return null;
  return t
    .slice(1, -1)
    .split('|')
    .map((c) => c.trim());
}

function escapeTerm(term) {
  return term.replace(/[\^$\\.*+?()[\]{}|/]/gu, '\\$&');
}

function parseGlossary(text) {
  const terms = [];
  const rows = [];
  const seen = new Set();
  let state = 'before';
  for (const line of splitLines(text)) {
    const c = cells(line);
    if (state === 'before') {
      if (c && c.join('|') === 'term|meaning|run|reserved') state = 'rule';
      continue;
    }
    if (state === 'rule') {
      if (!c || c.length !== 4 || !c.every((x) => /^:?-+:?$/u.test(x))) throw new Stop('glossary', GLOSSARY);
      state = 'rows';
      continue;
    }
    const tableLine = line.trim().startsWith('|');
    if (state === 'after') {
      if (tableLine) throw new Stop('glossary', GLOSSARY);
      continue;
    }
    if (!c) {
      if (tableLine) throw new Stop('glossary', GLOSSARY);
      state = 'after';
      continue;
    }
    const [term, meaning, run, reserved] = c;
    if (c.length !== 4 || term === '' || term.includes('`') || meaning === '') throw new Stop('glossary', GLOSSARY);
    if (!RUNS.includes(run) || (reserved !== 'yes' && reserved !== 'no')) throw new Stop('glossary', GLOSSARY);
    if (seen.has(term.toLowerCase())) throw new Stop('glossary', GLOSSARY);
    seen.add(term.toLowerCase());
    rows.push(`${term}|${run}|${reserved}`);
    if (reserved === 'yes') {
      terms.push({
        term,
        run,
        re: new RegExp(`(?<![\\p{L}\\p{N}_])${escapeTerm(term)}(?![\\p{L}\\p{N}_])`, 'iu'),
      });
    }
  }
  if (state === 'before' || state === 'rule' || seen.size === 0) throw new Stop('glossary', GLOSSARY);
  if (!FIXED_TERMS.every((row) => rows.includes(row))) throw new Stop('glossary', GLOSSARY);
  return terms;
}

function parseAllowed(text, terms) {
  const pairs = dataLines(text).map((line) => line.split('\t'));
  if (pairs.length !== 1) throw new Stop('allowed', ALLOWED);
  for (const pair of pairs) {
    if (pair.length !== 2 || pair[0] === '' || (terms && !terms.some((t) => t.term === pair[1] && t.run !== THIS_RUN))) {
      throw new Stop('allowed', ALLOWED);
    }
  }
  return pairs.map(([key, term]) => `${key}\t${term}`);
}

function countWords(text) {
  return text.split(/\p{White_Space}+/u).filter((w) => w !== '').length;
}

function judge(text, wordsKey, ctx, terms) {
  const found = [];
  for (const cls of CLASSES) {
    if (ctx.patterns.some((p) => p.cls === cls && p.re.test(text))) found.push({ rule: 'never_read', cls });
  }
  if (countWords(text) > MAX_WORDS) found.push({ rule: 'long', cls: '-' });
  for (const t of terms) {
    if (t.re.test(text) && !ctx.allowed.includes(`${wordsKey}\t${t.term}`)) found.push({ rule: 'clash', cls: '-' });
  }
  return found;
}

function parseJson(repo, rel) {
  try {
    return JSON.parse(readText(repo, rel, 'input'));
  } catch (e) {
    if (e instanceof Stop) throw e;
    throw new Stop('input', rel);
  }
}

function isObject(v) {
  return typeof v === 'object' && v !== null && !Array.isArray(v);
}

function fixtureSentences(doc, file) {
  if (!isObject(doc) || !isObject(doc.snapshot)) throw new Stop('input', file);
  if (!Array.isArray(doc.snapshot.rows) || !Array.isArray(doc.snapshot.items)) throw new Stop('input', file);
  if (doc.card !== null && !isObject(doc.card)) throw new Stop('input', file);
  const out = [];
  for (const spec of FIXTURE_KEYS) {
    const parts = spec.split('.');
    const visit = (node, i, place, holder) => {
      if (i === parts.length) {
        if (node === null || node === undefined) return;
        if (typeof node !== 'string') throw new Stop('input', file);
        const control = spec === CONTROL_TEXT && typeof holder.control === 'string' ? `control.${holder.control}` : '-';
        out.push({ text: node, key: place.join('.'), wordsKey: control });
        return;
      }
      if (node === null || node === undefined) return;
      if (parts[i] === '*') {
        if (!Array.isArray(node)) throw new Stop('input', file);
        node.forEach((child, j) => visit(child, i + 1, [...place, String(j + 1)], node));
        return;
      }
      if (!isObject(node)) throw new Stop('input', file);
      visit(node[parts[i]], i + 1, [...place, parts[i]], node);
    };
    visit(doc, 0, [], null);
  }
  return out;
}

function quotedStrings(text) {
  const out = [];
  let line = 1;
  let mode = 'code';
  let i = 0;
  while (i < text.length) {
    const c = text[i];
    if (c === '\n') {
      line += 1;
      i += 1;
      if (mode === 'line') mode = 'code';
      continue;
    }
    if (mode === 'line') {
      i += 1;
      continue;
    }
    if (mode === 'block') {
      i += c === '*' && text[i + 1] === '/' ? 2 : 1;
      if (c === '*' && text[i - 1] === '/') mode = 'code';
      continue;
    }
    if (mode === 'template') {
      if (c === '\\' && text[i + 1] !== '\n') {
        i += 2;
        continue;
      }
      if (c === '`') mode = 'code';
      i += 1;
      continue;
    }
    if (c === '/' && text[i + 1] === '/') {
      mode = 'line';
      i += 2;
      continue;
    }
    if (c === '/' && text[i + 1] === '*') {
      mode = 'block';
      i += 2;
      continue;
    }
    if (c === '`') {
      mode = 'template';
      i += 1;
      continue;
    }
    if (c === '"' || c === "'") {
      let j = i + 1;
      let closed = false;
      while (j < text.length && text[j] !== '\n' && text[j] !== '\r') {
        if (text[j] === '\\') {
          if (text[j + 1] === '\n' || text[j + 1] === '\r' || j + 1 >= text.length) break;
          j += 2;
          continue;
        }
        if (text[j] === c) {
          closed = true;
          break;
        }
        j += 1;
      }
      if (closed) {
        out.push({ text: text.slice(i + 1, j), line, quote: c });
        i = j + 1;
      } else {
        i = j;
      }
      continue;
    }
    i += 1;
  }
  return out;
}

function listViews(repo) {
  const files = [];
  try {
    fs.lstatSync(path.join(repo, APP));
  } catch (e) {
    if (e.code === 'ENOENT') return files;
    throw new Stop('input', APP);
  }
  const walk = (rel) => {
    let entries;
    try {
      entries = fs.readdirSync(path.join(repo, rel), { withFileTypes: true });
    } catch {
      throw new Stop('input', rel);
    }
    const names = entries.map((e) => e.name).sort((a, b) => Buffer.compare(Buffer.from(a), Buffer.from(b)));
    for (const name of names) {
      const entry = entries.find((e) => e.name === name);
      const child = `${rel}/${name}`;
      if (entry.isDirectory()) walk(child);
      else if (entry.isFile() && /\.(?:js|html)$/u.test(name) && !VIEWS.includes(child)) files.push(child);
    }
  };
  walk(APP);
  return files;
}

function hasSpace(text) {
  return /\p{White_Space}/u.test(text);
}

function readSentences(repo) {
  const sentences = [];
  const words = parseJson(repo, WORDS);
  if (!isObject(words)) throw new Stop('input', WORDS);
  for (const [key, text] of Object.entries(words)) {
    if (typeof text !== 'string') throw new Stop('input', WORDS);
    sentences.push({ file: WORDS, line: 1, key, wordsKey: key, text });
  }
  let names;
  try {
    names = fs.readdirSync(path.join(repo, FIXTURES));
  } catch {
    throw new Stop('input', FIXTURES);
  }
  const filed = names.map((n) => /^state-(.+)\.json$/u.exec(n)).filter(Boolean).map((m) => m[1]);
  for (const state of [...STATES.filter((s) => !filed.includes(s)), ...filed.filter((s) => !STATES.includes(s))]) {
    throw new Stop('input', `${FIXTURES}/state-${state}.json`);
  }
  for (const state of STATES) {
    const file = `${FIXTURES}/state-${state}.json`;
    for (const s of fixtureSentences(parseJson(repo, file), file)) sentences.push({ file, line: 1, ...s });
  }
  let headingMissing = false;
  const viewStrings = [];
  for (const file of VIEWS) {
    let text;
    try {
      text = fs.readFileSync(path.join(repo, file), 'utf8');
    } catch (e) {
      if (e.code === 'ENOENT') continue;
      throw new Stop('input', file);
    }
    const lines = splitLines(text);
    let headings = 0;
    for (const s of quotedStrings(text)) {
      const m = HEADING.exec(lines[s.line - 1]);
      if (m && s.quote === '"' && m[1] === s.text) {
        headings += 1;
        sentences.push({ file, line: s.line, key: '-', wordsKey: '-', text: s.text });
      } else if (hasSpace(s.text)) {
        viewStrings.push({ file, line: s.line });
      }
    }
    if (headings === 0) headingMissing = true;
  }
  const hidden = sentences.find((s) => /[\p{Cf}\u00ad]/u.test(s.text));
  if (hidden) throw new Stop('input', hidden.file);
  return { sentences, headingMissing, viewStrings };
}

const ORDER = ['usage', 'setup', 'node', 'list', 'allowed', 'input', 'glossary', 'heading', 'view', 'never_read', 'long', 'clash'];

function firstCause(causes) {
  return ORDER.find((c) => causes.has(c)) ?? null;
}

function check(repo) {
  const causes = new Set();
  const problems = [];
  const attempt = (fn) => {
    try {
      return fn();
    } catch (e) {
      if (!(e instanceof Stop)) throw e;
      causes.add(e.why);
      problems.push(`words problem cause=${e.why} file=${e.file}`);
      return null;
    }
  };
  const listed = attempt(() => parseList(readText(repo, LIST, 'list')));
  const closed = attempt(() => closedNames(repo));
  const terms = attempt(() => parseGlossary(readText(repo, GLOSSARY, 'glossary')));
  const allowed = attempt(() => parseAllowed(readText(repo, ALLOWED, 'allowed'), terms));
  const read = attempt(() => readSentences(repo));
  const views = attempt(() => listViews(repo));
  if (causes.size > 0) return { out: problems, findings: [], cause: firstCause(causes) };
  const ctx = { patterns: [...listed, closed], allowed };
  const others = terms.filter((t) => t.run !== THIS_RUN);
  const { sentences, headingMissing, viewStrings } = read;
  if (headingMissing) causes.add('heading');
  if (viewStrings.length > 0) causes.add('view');
  const findings = [];
  const counts = { never_read: 0, long: 0, clash: 0 };
  for (const s of sentences) {
    for (const f of judge(s.text, s.wordsKey, ctx, others)) {
      counts[f.rule] += 1;
      findings.push(`finding file=${s.file} line=${s.line} rule=${f.rule} class=${f.cls} key=${s.key}`);
    }
  }
  let viewFindings = 0;
  for (const file of views) {
    for (const s of quotedStrings(readText(repo, file, 'input'))) {
      if (!hasSpace(s.text)) continue;
      for (const f of judge(s.text, '-', ctx, terms)) {
        viewFindings += 1;
        findings.push(`finding file=${file} line=${s.line} rule=${f.rule} class=${f.cls} key=-`);
      }
    }
  }
  for (const rule of ['never_read', 'long', 'clash']) if (counts[rule] > 0) causes.add(rule);
  const out = [
    `words sentences=${sentences.length} never_read=${counts.never_read} long=${counts.long} clash=${counts.clash}`,
    `words other_views files=${views.length} findings=${viewFindings}`,
    ...viewStrings.map((v) => `words view file=${v.file} line=${v.line}`),
  ];
  return { out, findings, cause: firstCause(causes) };
}

function run(repo, artifacts) {
  const findingsFile = path.join(artifacts, 'findings.txt');
  const setup = () => ({ code: 1, out: [`words problem cause=setup file=-`, `FAIL ${NAME} (setup)`] });
  try {
    fs.mkdirSync(artifacts, { recursive: true });
    fs.writeFileSync(findingsFile, '');
  } catch {
    return setup();
  }
  try {
    const r = check(repo);
    try {
      fs.writeFileSync(findingsFile, r.findings.map((l) => `${l}\n`).join(''));
    } catch {
      return setup();
    }
    const last = r.cause ? `FAIL ${NAME} (${r.cause})` : `PASS ${NAME}`;
    return { code: r.cause ? 1 : 0, out: [...r.out, last] };
  } catch (e) {
    if (!(e instanceof Stop)) throw e;
    return { code: 1, out: [`words problem cause=${e.why} file=${e.file}`, `FAIL ${NAME} (${e.why})`] };
  }
}

const ID = (n) => n.toString(16).padStart(32, '0');
const NEVER = 'Ask Claude now.';

function baseState() {
  return {
    snapshot: {
      seq: 1,
      rows: [
        {
          agent: ID(2),
          workflow: ID(1),
          name: 'Paying a bill on Site1',
          status: 'needs_you',
          word: 'Needs you',
          line: 'It needs your answer.',
          say: 'Paying a bill on Site1 needs you.',
          level: 'Standard',
          next: '3 November 2026',
          controls: [
            { control: 'stop', text: 'Stop' },
            { control: 'delete', text: 'Delete' },
          ],
        },
      ],
      items: [
        {
          item: ID(3),
          agent: ID(2),
          kind: 'confirm',
          presenter: 'page',
          armed: 'yes',
          title: 'Pay this?',
          text: 'Look at it first.',
          fields: [{ label: 'Amount', value: ['42.10'], from: 'you', from_text: 'You gave this' }],
          params: [
            {
              name: 'payee',
              label: 'Who to pay',
              candidates: [{ n: 1, value: ['Site1'], from: 'records' }],
              typed: 'no',
            },
          ],
          choices: [{ choice: 'reject', text: "Don't pay", safe: 'yes' }],
        },
      ],
      note: 'Halvo shows your agents.',
    },
    card: {
      agent: ID(2),
      name: 'Paying a bill on Site1',
      lines: [{ key: 'level', text: 'Standard' }],
      runs: [
        {
          day: '3 November 2026',
          text: 'It finished the task.',
          effects: [
            {
              key: 'run.effect.payment',
              text: 'It made this payment.',
              fields: [{ label: 'Pay to', value: ['City Power'], from: 'you', from_text: 'You gave this' }],
            },
          ],
        },
      ],
    },
  };
}

function baseWords() {
  return {
    'control.delete': 'Delete',
    'control.stop': 'Stop',
    'line.time': 'It stops after {minutes} minutes.',
    'say.needs_you': '{name} needs you.',
    'status.needs_you': 'Needs you',
  };
}

function setAt(doc, spec, value) {
  const parts = spec.split('.');
  let node = doc;
  for (let i = 0; i < parts.length - 1; i += 1) node = node[parts[i] === '*' ? 0 : parts[i]];
  node[parts[parts.length - 1]] = value;
}

function selfTest(dir, repo) {
  const copies = {};
  for (const rel of [LIST, CASES, ALLOWED, GLOSSARY, ...NAMES]) copies[rel] = readText(repo, rel, 'input');
  const patterns = parseList(copies[LIST]);
  const lines = [];
  const wrong = [];
  let rejected = 0;
  let n = 0;
  const write = (root, rel, text) => {
    fs.mkdirSync(path.dirname(path.join(root, rel)), { recursive: true });
    fs.writeFileSync(path.join(root, rel), text);
  };
  const fixture = (name, edit = {}) => {
    n += 1;
    const root = path.join(dir, name);
    if (fs.existsSync(root)) throw new Error(`two fixtures are named ${name}`);
    const files = { ...copies };
    const words = baseWords();
    if (edit.words) edit.words(words);
    files[WORDS] = `${JSON.stringify(words)}\n`;
    for (const state of STATES) {
      const doc = baseState();
      if (edit.state) edit.state(state, doc);
      files[`${FIXTURES}/state-${state}.json`] = `${JSON.stringify(doc)}\n`;
    }
    if (edit.files) edit.files(files);
    for (const [rel, text] of Object.entries(files)) if (text !== null) write(root, rel, text);
    return { name, root };
  };
  const expect = (fx, want, opts = {}) => {
    const artifacts = path.join(fx.root, opts.artifacts ?? 'artifacts');
    const r = run(fx.root, artifacts);
    const last = r.out[r.out.length - 1];
    let findings = '';
    try {
      findings = fs.readFileSync(path.join(artifacts, 'findings.txt'), 'utf8');
    } catch {
      findings = '';
    }
    let got = r.code === 0 && last === `PASS ${NAME}` ? 'PASS' : `exit-${r.code}`;
    const m = /^FAIL 45-r4-words \(([a-z_]+)\)$/u.exec(last);
    if (r.code === 1 && m) got = `FAIL:${m[1]}`;
    let ok = got === want;
    for (const line of opts.lines ?? []) if (!r.out.includes(line)) ok = false;
    for (const f of opts.findings ?? []) if (!findings.split('\n').includes(f)) ok = false;
    for (const t of opts.secret ?? []) if (findings.includes(t)) ok = false;
    for (const l of findings.split('\n').filter((x) => x !== '')) {
      if (!/^finding file=\S+ line=[1-9][0-9]* rule=(?:never_read|long|clash) class=\S+ key=\S+$/u.test(l)) ok = false;
    }
    lines.push(`case ${fx.name} want=${want} got=${got}${ok ? '' : ' wrong'}`);
    if (!ok) wrong.push(fx.name);
    else if (want !== 'PASS') rejected += 1;
  };
  const wordsFinding = (rule, cls, key) => `finding file=${WORDS} line=1 rule=${rule} class=${cls} key=${key}`;

  const base = fixture('base');
  expect(base, 'PASS', {
    lines: [
      `words sentences=${5 + STATES.length * 22} never_read=0 long=0 clash=0`,
      'words other_views files=0 findings=0',
    ],
  });

  FIXTURE_KEYS.forEach((spec, i) => {
    const state = STATES[i % STATES.length];
    const place = spec.replaceAll('*', '1');
    const fx = fixture(`never-read-under-${place}`, {
      state: (s, doc) => {
        if (s === state) setAt(doc, spec, NEVER);
      },
    });
    expect(fx, 'FAIL:never_read', {
      findings: [`finding file=${FIXTURES}/state-${state}.json line=1 rule=never_read class=model key=${place}`],
      secret: [NEVER],
    });
  });

  expect(
    fixture('never-read-in-the-table', { words: (w) => (w['line.time'] = NEVER) }),
    'FAIL:never_read',
    { findings: [wordsFinding('never_read', 'model', 'line.time')], secret: [NEVER] },
  );

  const unread = [
    'snapshot.rows.*.agent',
    'snapshot.rows.*.workflow',
    'snapshot.rows.*.status',
    'snapshot.rows.*.controls.*.control',
    'snapshot.items.*.item',
    'snapshot.items.*.kind',
    'snapshot.items.*.presenter',
    'snapshot.items.*.armed',
    'snapshot.items.*.fields.*.from',
    'snapshot.items.*.params.*.name',
    'snapshot.items.*.params.*.typed',
    'snapshot.items.*.choices.*.choice',
    'snapshot.items.*.choices.*.safe',
    'card.agent',
    'card.lines.*.key',
    'card.runs.*.effects.*.key',
    'card.runs.*.effects.*.fields.*.from',
  ];
  const longText = Array.from({ length: 21 }, () => 'word').join(' ');
  expect(
    fixture('values-and-other-keys-are-not-read', {
      state: (s, doc) => {
        for (const spec of unread) setAt(doc, spec, NEVER);
        doc.snapshot.items[0].fields[0].value = [NEVER, longText, 'It is paused.'];
        doc.snapshot.items[0].params[0].candidates[0].value = [NEVER];
        doc.snapshot.seq = NEVER;
        doc.extra = NEVER;
      },
      files: (f) => {
        f[`${FIXTURES}/reply-ok.json`] = `${JSON.stringify({ note: NEVER })}\n`;
      },
    }),
    'PASS',
  );

  expect(
    fixture('long-in-the-table', { words: (w) => (w['line.time'] = longText) }),
    'FAIL:long',
    { findings: [wordsFinding('long', '-', 'line.time')], secret: [longText] },
  );
  expect(
    fixture('long-in-a-fixture', {
      state: (s, doc) => {
        if (s === 'card') doc.card.runs[0].text = longText;
      },
    }),
    'FAIL:long',
    {
      findings: [`finding file=${FIXTURES}/state-card.json line=1 rule=long class=- key=card.runs.1.text`],
    },
  );
  const twenty = `{name} ${Array.from({ length: 19 }, () => 'word').join('  ')}`;
  expect(fixture('twenty-words-with-a-placeholder', { words: (w) => (w['line.time'] = twenty) }), 'PASS');

  expect(
    fixture('clash-in-the-table', { words: (w) => (w['line.time'] = 'It is paused now.') }),
    'FAIL:clash',
    { findings: [wordsFinding('clash', '-', 'line.time')], secret: ['paused'] },
  );
  expect(
    fixture('clash-in-another-letter-case', { words: (w) => (w['line.time'] = 'PAUSED for now.') }),
    'FAIL:clash',
  );
  expect(
    fixture('clash-of-the-delete-term-under-another-key', { words: (w) => (w['control.stop'] = 'Delete') }),
    'FAIL:clash',
    { findings: [wordsFinding('clash', '-', 'control.stop')] },
  );
  expect(
    fixture('clash-of-the-delete-term-on-another-control', {
      state: (s, doc) => {
        if (s === 'off') doc.snapshot.rows[0].controls[0].text = 'Delete';
      },
    }),
    'FAIL:clash',
    {
      findings: [`finding file=${FIXTURES}/state-off.json line=1 rule=clash class=- key=snapshot.rows.1.controls.1.text`],
    },
  );
  expect(
    fixture('another-form-and-this-runs-own-terms-pass', {
      words: (w) => {
        w['line.time'] = 'It will pause, and its capture needs you.';
        w['say.needs_you'] = 'It is switched off and waiting its turn.';
      },
    }),
    'PASS',
  );

  expect(
    fixture('heading-line-missing', {
      files: (f) => {
        f['ui/app/views/needs.js'] = 'export const view = {\n  title: "What needs you",\n};\n';
      },
    }),
    'FAIL:heading',
  );
  expect(
    fixture('never-read-in-a-heading', {
      files: (f) => {
        f['ui/app/views/agents.js'] = `export const view = {\n  heading: "${NEVER}",\n};\n`;
        f['ui/app/views/needs.js'] = 'export const view = {\n  heading: "What needs you",\n};\n';
      },
    }),
    'FAIL:never_read',
    {
      findings: ['finding file=ui/app/views/agents.js line=2 rule=never_read class=model key=-'],
      lines: [`words sentences=${5 + STATES.length * 22 + 2} never_read=1 long=0 clash=0`],
      secret: [NEVER],
    },
  );
  expect(
    fixture('other-views-are-reported-and-never-fail', {
      files: (f) => {
        f['ui/app/main.js'] = [
          `const a = "${NEVER}";`,
          'const b = `Ask the model ${a} now`;',
          "const c = 'model';",
          `// "${NEVER}"`,
          `const d = \`say "${NEVER}" here\`;`,
          `/* "${NEVER}" */ const e = "It is paused here";`,
          'const g = "The agent needs you now";',
          '',
        ].join('\n');
        f['ui/app/index.html'] = `<p title='${NEVER}'>Hello</p>\n`;
        f['ui/app/notes.txt'] = `"${NEVER}"\n`;
      },
    }),
    'PASS',
    {
      lines: ['words other_views files=2 findings=4'],
      findings: [
        'finding file=ui/app/index.html line=1 rule=never_read class=model key=-',
        'finding file=ui/app/main.js line=1 rule=never_read class=model key=-',
        'finding file=ui/app/main.js line=6 rule=clash class=- key=-',
        'finding file=ui/app/main.js line=7 rule=clash class=- key=-',
      ],
    },
  );
  expect(
    fixture('the-two-views-are-not-other-views', {
      files: (f) => {
        f['ui/app/views/needs.js'] = `export const view = {\n  heading: "What needs you",\n  note: "${NEVER}",\n};\n`;
      },
    }),
    'FAIL:view',
    {
      lines: [
        `words sentences=${5 + STATES.length * 22 + 1} never_read=0 long=0 clash=0`,
        'words other_views files=0 findings=0',
        'words view file=ui/app/views/needs.js line=3',
      ],
      secret: [NEVER],
    },
  );
  const view = (body) => `export const view = {\n  heading: "What needs you",\n${body}};\n`;
  expect(
    fixture('a-view-string-without-white-space-passes', {
      files: (f) => (f['ui/app/views/needs.js'] = view("  id: 'needs',\n  // \"Ask the model now\"\n")),
    }),
    'PASS',
    { lines: [`words sentences=${5 + STATES.length * 22 + 1} never_read=0 long=0 clash=0`] },
  );
  expect(
    fixture('a-view-string-with-a-no-break-space-ends-with-view', {
      files: (f) => (f['ui/app/views/agents.js'] = view("  note: 'Two\u00a0words',\n")),
    }),
    'FAIL:view',
  );
  expect(
    fixture('a-spaced-view-string-after-a-template-literal-ends-with-view', {
      files: (f) => (f['ui/app/views/needs.js'] = view('  id: `x`,\n  note: "two words",\n')),
    }),
    'FAIL:view',
    { lines: ['words view file=ui/app/views/needs.js line=4'] },
  );
  expect(
    fixture('other-view-strings-with-no-space-are-not-judged', {
      files: (f) => (f['ui/app/main.js'] = "const a = 'Claude';\n"),
    }),
    'PASS',
    { lines: ['words other_views files=1 findings=0'] },
  );
  expect(
    fixture('a-heading-inside-a-comment-does-not-count', {
      files: (f) => (f['ui/app/views/needs.js'] = 'export const view = {\n  // heading: "What needs you",\n  id: `x`,\n};\n'),
    }),
    'FAIL:heading',
  );
  expect(
    fixture('a-heading-inside-a-block-comment-does-not-count', {
      files: (f) => (f['ui/app/views/needs.js'] = '/*\n  heading: "What needs you",\n*/\nexport const view = {};\n'),
    }),
    'FAIL:heading',
  );
  expect(
    fixture('a-second-heading-line-is-read', {
      files: (f) => (f['ui/app/views/needs.js'] = view(`  heading: "${NEVER}",\n`)),
    }),
    'FAIL:never_read',
    { findings: ['finding file=ui/app/views/needs.js line=3 rule=never_read class=model key=-'] },
  );
  expect(
    fixture('a-heading-with-an-escape-is-no-heading', {
      files: (f) => (f['ui/app/views/needs.js'] = 'export const view = {\n  heading: "What\\\\ needs you",\n};\n'),
    }),
    'FAIL:heading',
  );
  const twoFaults = [
    ['list', 'allowed', { files: (f) => { f[LIST] = `${copies[LIST]}model\t(unclosed\n`; f[ALLOWED] = ''; } }],
    ['allowed', 'input', { files: (f) => { f[ALLOWED] = ''; f[WORDS] = null; } }],
    ['input', 'glossary', { files: (f) => { f[WORDS] = null; f[GLOSSARY] = '# Glossary\n'; } }],
    ['glossary', 'heading', { files: (f) => { f[GLOSSARY] = '# Glossary\n'; f['ui/app/views/needs.js'] = 'export const view = {};\n'; } }],
    ['glossary', 'view', { files: (f) => { f[GLOSSARY] = '# Glossary\n'; f['ui/app/views/agents.js'] = view(`  note: "${NEVER}",\n`); } }],
    ['heading', 'view', { files: (f) => { f['ui/app/views/needs.js'] = 'export const view = {};\n'; f['ui/app/views/agents.js'] = view(`  note: "${NEVER}",\n`); } }],
    ['view', 'never_read', { words: (w) => (w['line.time'] = NEVER), files: (f) => (f['ui/app/views/agents.js'] = view("  note: 'two words',\n")) }],
  ];
  for (const [a, b, edit] of twoFaults) expect(fixture(`order-${a}-before-${b}`, edit), `FAIL:${a}`);
  for (const [name, mark] of [
    ['zero-width-space', '\u200b'],
    ['soft-hyphen', '\u00ad'],
    ['word-joiner', '\u2060'],
    ['left-to-right-mark', '\u200e'],
  ]) {
    expect(
      fixture(`a-${name}-in-the-table-is-input`, { words: (w) => (w['line.time'] = `It is pau${mark}sed now.`) }),
      'FAIL:input',
    );
  }
  expect(
    fixture('a-hidden-character-in-a-fixture-is-input', {
      state: (s, doc) => {
        if (s === 'queue') doc.snapshot.rows[0].line = 'It is wait\u200bing.';
      },
    }),
    'FAIL:input',
  );
  expect(
    fixture('a-hidden-character-in-a-heading-is-input', {
      files: (f) => (f['ui/app/views/needs.js'] = 'export const view = {\n  heading: "What\u200b needs you",\n};\n'),
    }),
    'FAIL:input',
  );
  const blocked = { artifacts: 'blocker/artifacts' };
  expect(fixture('artifacts-under-a-file-is-setup', { files: (f) => (f.blocker = 'x\n') }), 'FAIL:setup', blocked);
  expect(
    fixture('order-setup-before-list', {
      files: (f) => {
        f.blocker = 'x\n';
        f[LIST] = `${copies[LIST]}model\t(unclosed\n`;
      },
    }),
    'FAIL:setup',
    blocked,
  );
  expect(
    fixture('order-setup-before-input', {
      files: (f) => {
        f.blocker = 'x\n';
        f[WORDS] = null;
      },
    }),
    'FAIL:setup',
    blocked,
  );
  const row = (r) => (f) => (f[GLOSSARY] = copies[GLOSSARY].replace('| paused |', `${r}\n| paused |`));
  const say = (text) => (w) => (w['line.time'] = text);
  expect(fixture('list-with-an-empty-pattern', { files: (f) => (f[LIST] = `${copies[LIST]}model\t\n`) }), 'FAIL:list');
  expect(fixture('a-term-with-a-point-matches-only-itself', { files: row('| a.m. | A test term. | billing | yes |'), words: say('Take aim. Then go.') }), 'PASS');
  expect(fixture('a-term-with-a-point-clashes', { files: row('| a.m. | A test term. | billing | yes |'), words: say('It opens at 9 a.m. today.') }), 'FAIL:clash');
  expect(fixture('a-digit-before-a-term-is-no-boundary', { words: say('Ref 1paused now.') }), 'PASS');
  expect(fixture('an-underscore-after-a-term-is-no-boundary', { words: say('It is paused_ now.') }), 'PASS');
  expect(fixture('a-duplicate-glossary-term', { files: row('| Paused | Twice. | capture | yes |') }), 'FAIL:glossary');
  expect(fixture('a-glossary-term-in-code-font', { files: row('| `zzz` | A test term. | capture | yes |') }), 'FAIL:glossary');
  expect(fixture('a-glossary-run-outside-the-list', { files: row('| zzz | A test term. | captur | yes |') }), 'FAIL:glossary');
  expect(fixture('a-glossary-reserved-value-outside-yes-and-no', { files: row('| zzz | A test term. | capture | Yes |') }), 'FAIL:glossary');
  expect(
    fixture('a-table-line-after-the-table-is-glossary', { files: (f) => (f[GLOSSARY] = `${copies[GLOSSARY]}\nNotes.\n\n| a | b |\n`) }),
    'FAIL:glossary',
  );
  expect(
    fixture('an-indented-table-line-after-the-table-is-glossary', { files: (f) => (f[GLOSSARY] = `${copies[GLOSSARY]}\nNotes.\n   | a | b |\n`) }),
    'FAIL:glossary',
  );
  expect(fixture('text-after-the-table-passes',{ files: (f) => (f[GLOSSARY] = `${copies[GLOSSARY]}\nNotes.\n`) }), 'PASS');
  const glossaryLines = (edit) => (f) => (f[GLOSSARY] = splitLines(copies[GLOSSARY]).flatMap(edit).join('\n'));
  for (const term of ['paused', 'cap', 'delete', 'approve']) {
    expect(
      fixture(`the-fixed-term-${term}-missing`, { files: glossaryLines((l) => (l.startsWith(`| ${term} |`) ? [] : [l])) }),
      'FAIL:glossary',
    );
  }
  expect(
    fixture('a-typo-in-the-fixed-term-paused', {
      files: (f) => (f[GLOSSARY] = copies[GLOSSARY].replace('| paused |', '| pasued |')),
      words: say('It is paused.'),
    }),
    'FAIL:glossary',
  );
  expect(
    fixture('a-blank-line-before-the-approve-row', {
      files: (f) => (f[GLOSSARY] = copies[GLOSSARY].replace('| approve |', '\n| approve |')),
      words: say('You approve it.'),
    }),
    'FAIL:glossary',
  );
  expect(
    fixture('the-approve-row-without-its-end-pipe', {
      files: glossaryLines((l) => (l.startsWith('| approve |') ? [l.replace(/ \|$/u, '')] : [l])),
      words: say('You approve it.'),
    }),
    'FAIL:glossary',
  );
  expect(
    fixture('the-last-row-without-its-end-pipe', {
      files: glossaryLines((l) => (l.startsWith('| Thorough |') ? [l.replace(/ \|$/u, '')] : [l])),
    }),
    'FAIL:glossary',
  );
  expect(
    fixture('the-fixed-term-paused-in-capitals', {
      files: (f) => (f[GLOSSARY] = copies[GLOSSARY].replace('| paused |', '| Paused |')),
    }),
    'FAIL:glossary',
  );
  expect(
    fixture('the-fixed-term-approve-not-reserved', {
      files: glossaryLines((l) => (l.startsWith('| approve |') ? [l.replace(/\| yes \|$/u, '| no |')] : [l])),
    }),
    'FAIL:glossary',
  );
  expect(
    fixture('the-fixed-term-cap-under-another-run', {
      files: glossaryLines((l) => (l.startsWith('| cap |') ? [l.replace('| capture |', '| billing |')] : [l])),
    }),
    'FAIL:glossary',
  );
  expect(fixture('an-empty-allowed-list', { files: (f) => (f[ALLOWED] = '') }), 'FAIL:allowed');
  expect(fixture('an-allowed-term-of-this-run', { files: (f) => (f[ALLOWED] = 'control.delete\tNeeds you\n') }), 'FAIL:allowed');
  expect(
    fixture('a-number-where-a-fixture-sentence-stands', {
      state: (s, doc) => {
        if (s === 'ten') doc.snapshot.rows[0].line = 5;
      },
    }),
    'FAIL:input',
  );
  expect(
    fixture('a-list-where-a-fixture-sentence-stands', {
      state: (s, doc) => {
        if (s === 'ten') doc.snapshot.rows[0].name = [NEVER];
      },
    }),
    'FAIL:input',
  );
  expect(fixture('a-number-in-the-table', { words: (w) => (w['line.time'] = 5) }), 'FAIL:input');
  expect(
    fixture('an-object-where-a-list-stands', {
      state: (s, doc) => {
        if (s === 'ten') doc.snapshot.rows[0].controls = {};
      },
    }),
    'FAIL:input',
  );
  const crlf = (t) => t.replaceAll('\r\n', '\n').replaceAll('\n', '\r\n');
  expect(
    fixture('lists-and-a-view-with-crlf-line-ends-pass', {
      files: (f) => {
        for (const rel of [LIST, ALLOWED, GLOSSARY]) f[rel] = crlf(copies[rel]);
        f['ui/app/views/needs.js'] = crlf(view(''));
      },
    }),
    'PASS',
    { lines: [`words sentences=${5 + STATES.length * 22 + 1} never_read=0 long=0 clash=0`] },
  );
  expect(
    fixture('two-classes-in-one-sentence-count-twice', { words: say('Claude saved it to /Users/ann/x.') }),
    'FAIL:never_read',
    { lines: [`words sentences=${5 + STATES.length * 22} never_read=2 long=0 clash=0`] },
  );
  expect(fixture('ui-app-that-is-a-file', { files: (f) => (f['ui/app'] = 'x\n') }), 'FAIL:input');
  if (process.platform !== 'win32') {
    const locked = fixture('a-directory-under-ui-app-that-cannot-be-read', { files: (f) => (f['ui/app/sub/x.js'] = 'const a = 1;\n') });
    fs.chmodSync(path.join(locked.root, 'ui/app/sub'), 0o000);
    expect(locked, 'FAIL:input');
    const dangling = fixture('ui-app-that-points-nowhere');
    fs.symlinkSync(path.join(dangling.root, 'no-such-directory'), path.join(dangling.root, 'ui/app'));
    expect(dangling, 'FAIL:input');
  }
  expect(
    fixture('delete-in-a-row-line-clashes', {
      state: (s, doc) => {
        if (s === 'off') doc.snapshot.rows[0].line = 'It will delete your workflow.';
      },
    }),
    'FAIL:clash',
  );
  expect(fixture('a-lone-point-is-a-word', { words: say(`${Array.from({ length: 20 }, () => 'word').join(' ')} .`) }), 'FAIL:long');
  expect(
    fixture('heading-line-without-its-comma', {
      files: (f) => {
        f['ui/app/views/needs.js'] = 'export const view = {\n  heading: "What needs you"\n};\n';
      },
    }),
    'FAIL:heading',
  );
  expect(
    fixture('heading-comes-before-the-rules', {
      words: (w) => (w['line.time'] = NEVER),
      files: (f) => {
        f['ui/app/views/agents.js'] = 'export const view = {};\n';
      },
    }),
    'FAIL:heading',
  );
  expect(
    fixture('never-read-comes-before-long', { words: (w) => (w['line.time'] = `${NEVER} ${longText}`) }),
    'FAIL:never_read',
    { lines: [`words sentences=${5 + STATES.length * 22} never_read=1 long=1 clash=0`] },
  );
  expect(
    fixture('long-comes-before-clash', { words: (w) => (w['line.time'] = `It is paused ${longText}`) }),
    'FAIL:long',
    { lines: [`words sentences=${5 + STATES.length * 22} never_read=0 long=1 clash=1`] },
  );
  for (const [name, space] of [
    ['a-tab', '\t'],
    ['a-no-break-space', '\u00a0'],
    ['an-em-space', '\u2003'],
    ['a-line-break', '\n'],
    ['a-next-line', '\u0085'],
  ]) {
    expect(
      fixture(`${name}-parts-words`, {
        words: (w) => (w['line.time'] = `${Array.from({ length: 19 }, () => 'word').join(' ')} two${space}words`),
      }),
      'FAIL:long',
      { findings: [wordsFinding('long', '-', 'line.time')] },
    );
  }
  expect(
    fixture('a-term-inside-a-longer-word', {
      words: (w) => {
        w['line.time'] = 'It is unpaused after a short recap.';
        w['say.needs_you'] = 'It is undeleted and disapproved.';
      },
    }),
    'PASS',
  );

  const listWithout = (cls) => dataLines(copies[LIST]).filter((l) => !l.startsWith(`${cls}\t`)).join('\n');
  for (const cls of CLASSES) {
    expect(fixture(`list-without-${cls}`, { files: (f) => (f[LIST] = `${listWithout(cls)}\n`) }), 'FAIL:list');
  }
  expect(
    fixture('list-with-a-broken-pattern', { files: (f) => (f[LIST] = `${copies[LIST]}model\t(unclosed\n`) }),
    'FAIL:list',
  );
  expect(
    fixture('list-with-an-unknown-class', { files: (f) => (f[LIST] = `${copies[LIST]}vendor\tAcme\n`) }),
    'FAIL:list',
  );
  expect(
    fixture('list-with-a-pattern-that-flag-u-refuses', { files: (f) => (f[LIST] = `${copies[LIST]}code\ta\\:b\n`) }),
    'FAIL:list',
  );
  expect(
    fixture('a-closed-name-with-a-hyphen-in-the-table', { words: say('It needs-you now.') }),
    'FAIL:never_read',
    { findings: [wordsFinding('never_read', 'enum', 'line.time')] },
  );
  expect(
    fixture('a-closed-name-with-an-underscore-in-a-fixture', {
      state: (s, doc) => {
        if (s === 'queue') doc.snapshot.rows[0].line = 'It is waiting_turn.';
      },
    }),
    'FAIL:never_read',
    { findings: [`finding file=${FIXTURES}/state-queue.json line=1 rule=never_read class=enum key=snapshot.rows.1.line`] },
  );
  expect(fixture('a-closed-name-with-an-underscore-before-a-file-suffix', { words: say('It saved needs_you.pdf today.') }), 'FAIL:never_read');
  expect(fixture('a-cause-with-a-hyphen',{ words: say('It stopped: queue-full.') }), 'FAIL:never_read');
  expect(fixture('a-closed-name-with-a-hyphen-in-capitals', { words: say('It is ALL-SET now.') }), 'FAIL:never_read');
  expect(
    fixture('a-closed-name-of-three-parts-with-hyphens', { words: say('It paid the utility-bill-payment today.') }),
    'FAIL:never_read',
    { findings: [wordsFinding('never_read', 'enum', 'line.time')] },
  );
  expect(fixture('a-closed-name-inside-a-longer-word-passes', { words: say('It needs-your answer on the sign-up page of a non-device plan.') }), 'PASS');
  expect(
    fixture('names-files-with-no-name-that-holds-an-underscore', {
      files: (f) => {
        f[NAMES[0]] = '    Status = "off",\n';
        f[NAMES[1]] = '    Busy = "busy",\n';
      },
    }),
    'FAIL:input',
  );
  expect(fixture('a-names-file-missing', { files: (f) => (f[NAMES[0]] = null) }), 'FAIL:input');
  expect(fixture('a-names-file-with-no-names', { files: (f) => (f[NAMES[1]] = 'pub enum Cause {}\n') }), 'FAIL:input');
  expect(fixture('letter-case-counts',{ words: (w) => (w['line.time'] = 'The CLAUDE and the TIER are here.') }), 'PASS');

  const cases = dataLines(copies[CASES]).map((l) => l.split('\t'));
  const misses = [];
  const wellFormed = cases.every(
    (c) => c.length === 4 && /^[1-9][0-9]*$/u.test(c[0]) && Number(c[0]) <= patterns.length && ['match', 'miss'].includes(c[2]),
  );
  if (!wellFormed) {
    wrong.push('cases-name-a-line-each');
    lines.push('case cases-name-a-line-each want=yes got=no wrong');
  }
  patterns.forEach((p, i) => {
    const own = cases.filter((c) => Number(c[0]) === i + 1);
    const hits = own.filter((c) => c[2] === 'match');
    const away = own.filter((c) => c[2] === 'miss');
    misses.push(...away.map((c) => c[3]));
    const matched = hits.length > 0 && hits.every((c) => c[1] === p.cls && p.re.test(c[3]));
    const missed = away.length > 0 && away.every((c) => c[1] === p.cls && patterns.every((q) => !q.re.test(c[3])));
    lines.push(`case pattern-${i + 1}-${p.cls} match=${matched ? 'yes' : 'no'} miss=${missed ? 'yes' : 'no'}`);
    if (!matched || !missed) wrong.push(`pattern-${i + 1}`);
    expect(
      fixture(`pattern-${i + 1}-${p.cls}-fails-the-check`, { words: (w) => (w['line.time'] = hits[0]?.[3] ?? '') }),
      'FAIL:never_read',
      { findings: [wordsFinding('never_read', p.cls, 'line.time')] },
    );
  });
  expect(
    fixture('every-miss-sentence-passes', {
      words: (w) => {
        misses.forEach((m, i) => (w[`miss.${i + 1}`] = m));
      },
    }),
    'PASS',
  );

  expect(fixture('state-file-missing', { files: (f) => (f[`${FIXTURES}/state-queue.json`] = null) }), 'FAIL:input');
  expect(
    fixture('a-state-file-that-the-list-does-not-name', {
      files: (f) => (f[`${FIXTURES}/state-extra.json`] = `${JSON.stringify(baseState())}\n`),
    }),
    'FAIL:input',
  );
  expect(fixture('words-file-missing', { files: (f) => (f[WORDS] = null) }), 'FAIL:input');
  expect(fixture('state-file-not-json', { files: (f) => (f[`${FIXTURES}/state-ten.json`] = '{"snapshot":\n') }), 'FAIL:input');
  expect(
    fixture('glossary-without-its-table', { files: (f) => (f[GLOSSARY] = '# Glossary\n\nNo table.\n') }),
    'FAIL:glossary',
  );
  expect(
    fixture('allowed-with-a-second-pair', { files: (f) => (f[ALLOWED] = `${copies[ALLOWED]}control.stop\tpaused\n`) }),
    'FAIL:allowed',
  );

  return { lines, wrong, rejected };
}

function main(argv) {
  const usage = () => {
    process.stdout.write(`FAIL ${NAME} (usage)\n`);
    return 1;
  };
  const opts = {};
  for (let i = 0; i < argv.length; i += 2) {
    if (!['--repo', '--artifacts', '--self-test'].includes(argv[i]) || argv[i + 1] === undefined) return usage();
    if (opts[argv[i]] !== undefined) return usage();
    opts[argv[i]] = argv[i + 1];
  }
  if (opts['--repo'] === undefined) return usage();
  if (opts['--self-test'] !== undefined) {
    if (opts['--artifacts'] !== undefined) return usage();
    let r;
    try {
      r = selfTest(opts['--self-test'], opts['--repo']);
    } catch (e) {
      if (!(e instanceof Stop)) throw e;
      process.stdout.write(`FAIL self-test:${NAME} (${e.why})\n`);
      return 1;
    }
    process.stdout.write(r.lines.map((l) => `${l}\n`).join(''));
    if (r.wrong.length > 0) {
      process.stdout.write(`FAIL self-test:${NAME} (wrong verdict in ${r.wrong.length} cases)\n`);
      return 1;
    }
    process.stdout.write(`PASS self-test:${NAME} (${r.rejected} failing fixtures rejected)\n`);
    return 0;
  }
  if (opts['--artifacts'] === undefined) return usage();
  const r = run(opts['--repo'], opts['--artifacts']);
  process.stdout.write(r.out.map((l) => `${l}\n`).join(''));
  return r.code;
}

process.exitCode = main(process.argv.slice(2));
