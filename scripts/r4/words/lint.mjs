import fs from 'node:fs';
import path from 'node:path';

const NAME = '45-r4-words';
const CLASSES = ['model', 'tier', 'count', 'tool', 'bundle', 'path', 'address', 'enum', 'code', 'trace', 'prompt'];
const RUNS = ['capture', 'workflow', 'agents', 'interface', 'billing'];
const THIS_RUN = 'interface';
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
];
const FIXTURES = 'ui/tests/fixtures/coord';
const WORDS = `${FIXTURES}/words.json`;
const VIEWS = ['ui/app/views/needs.js', 'ui/app/views/agents.js'];
const APP = 'ui/app';
const LIST = 'scripts/r4/words/never-read.txt';
const CASES = 'scripts/r4/words/never-read-cases.txt';
const ALLOWED = 'scripts/r4/words/allowed.txt';
const GLOSSARY = 'docs/r4/words.md';
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
    if (!c) break;
    const [term, meaning, run, reserved] = c;
    if (c.length !== 4 || term === '' || term.includes('`') || meaning === '') throw new Stop('glossary', GLOSSARY);
    if (!RUNS.includes(run) || (reserved !== 'yes' && reserved !== 'no')) throw new Stop('glossary', GLOSSARY);
    if (seen.has(term.toLowerCase())) throw new Stop('glossary', GLOSSARY);
    seen.add(term.toLowerCase());
    if (reserved === 'yes') {
      terms.push({
        term,
        run,
        re: new RegExp(`(?<![\\p{L}\\p{N}_])${escapeTerm(term)}(?![\\p{L}\\p{N}_])`, 'iu'),
      });
    }
  }
  if (state !== 'rows' || seen.size === 0) throw new Stop('glossary', GLOSSARY);
  return terms;
}

function parseAllowed(text, terms) {
  const pairs = dataLines(text).map((line) => line.split('\t'));
  if (pairs.length !== 1) throw new Stop('allowed', ALLOWED);
  for (const pair of pairs) {
    if (pair.length !== 2 || pair[0] === '' || !terms.some((t) => t.term === pair[1] && t.run !== THIS_RUN)) {
      throw new Stop('allowed', ALLOWED);
    }
  }
  return pairs.map(([key, term]) => `${key}\t${term}`);
}

function countWords(text) {
  return text.split(' ').filter((w) => w !== '').length;
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
        const s = text.slice(i + 1, j);
        if (s.includes(' ')) out.push({ text: s, line });
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
  const walk = (rel) => {
    let entries;
    try {
      entries = fs.readdirSync(path.join(repo, rel), { withFileTypes: true });
    } catch (e) {
      if (e.code === 'ENOENT' && rel === APP) return;
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

function check(repo) {
  const patterns = parseList(readText(repo, LIST, 'list'));
  const terms = parseGlossary(readText(repo, GLOSSARY, 'glossary'));
  const allowed = parseAllowed(readText(repo, ALLOWED, 'allowed'), terms);
  const ctx = { patterns, allowed };
  const others = terms.filter((t) => t.run !== THIS_RUN);
  const sentences = [];
  const words = parseJson(repo, WORDS);
  if (!isObject(words)) throw new Stop('input', WORDS);
  for (const [key, text] of Object.entries(words)) {
    if (typeof text !== 'string') throw new Stop('input', WORDS);
    sentences.push({ file: WORDS, line: 1, key, wordsKey: key, text });
  }
  for (const state of STATES) {
    const file = `${FIXTURES}/state-${state}.json`;
    for (const s of fixtureSentences(parseJson(repo, file), file)) sentences.push({ file, line: 1, ...s });
  }
  let heading = false;
  for (const file of VIEWS) {
    let text;
    try {
      text = fs.readFileSync(path.join(repo, file), 'utf8');
    } catch (e) {
      if (e.code === 'ENOENT') continue;
      throw new Stop('input', file);
    }
    const found = [];
    splitLines(text).forEach((l, n) => {
      const m = HEADING.exec(l);
      if (m) found.push({ file, line: n + 1, key: '-', wordsKey: '-', text: m[1] });
    });
    if (found.length === 0) heading = true;
    sentences.push(...found);
  }
  const findings = [];
  const counts = { never_read: 0, long: 0, clash: 0 };
  for (const s of sentences) {
    for (const f of judge(s.text, s.wordsKey, ctx, others)) {
      counts[f.rule] += 1;
      findings.push(`finding file=${s.file} line=${s.line} rule=${f.rule} class=${f.cls} key=${s.key}`);
    }
  }
  const views = listViews(repo);
  let viewFindings = 0;
  for (const file of views) {
    for (const s of quotedStrings(readText(repo, file, 'input'))) {
      for (const f of judge(s.text, '-', ctx, terms)) {
        viewFindings += 1;
        findings.push(`finding file=${file} line=${s.line} rule=${f.rule} class=${f.cls} key=-`);
      }
    }
  }
  const out = [
    `words sentences=${sentences.length} never_read=${counts.never_read} long=${counts.long} clash=${counts.clash}`,
    `words other_views files=${views.length} findings=${viewFindings}`,
  ];
  let cause = null;
  if (heading) cause = 'heading';
  else cause = ['never_read', 'long', 'clash'].find((r) => counts[r] > 0) ?? null;
  return { out, findings, cause };
}

function run(repo, artifacts) {
  fs.mkdirSync(artifacts, { recursive: true });
  const findingsFile = path.join(artifacts, 'findings.txt');
  fs.writeFileSync(findingsFile, '');
  try {
    const r = check(repo);
    fs.writeFileSync(findingsFile, r.findings.map((l) => `${l}\n`).join(''));
    const last = r.cause ? `FAIL ${NAME} (${r.cause})` : `PASS ${NAME}`;
    return { code: r.cause ? 1 : 0, out: [...r.out, last] };
  } catch (e) {
    if (!(e instanceof Stop)) throw e;
    return { code: 1, out: [`words problem cause=${e.why} file=${e.file}`, `FAIL ${NAME} (${e.why})`] };
  }
}

const ID = (n) => n.toString(16).padStart(32, '0');
const NEVER = 'Ask the model now.';

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
      runs: [{ day: '3 November 2026', text: 'It finished the task.' }],
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
  for (const rel of [LIST, CASES, ALLOWED, GLOSSARY]) copies[rel] = readText(repo, rel, 'input');
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
    const root = path.join(dir, `case-${String(n).padStart(3, '0')}`);
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
    const artifacts = path.join(fx.root, 'artifacts');
    const r = run(fx.root, artifacts);
    const last = r.out[r.out.length - 1];
    const findings = fs.readFileSync(path.join(artifacts, 'findings.txt'), 'utf8');
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
      `words sentences=${5 + 18 * 19} never_read=0 long=0 clash=0`,
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
        f[`${FIXTURES}/state-extra.json`] = `${JSON.stringify({ snapshot: { note: NEVER } })}\n`;
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
      lines: [`words sentences=${5 + 18 * 19 + 2} never_read=1 long=0 clash=0`],
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
          '',
        ].join('\n');
        f['ui/app/index.html'] = `<p title='${NEVER}'>Hello</p>\n`;
        f['ui/app/notes.txt'] = `"${NEVER}"\n`;
      },
    }),
    'PASS',
    {
      lines: ['words other_views files=2 findings=2'],
      findings: [
        'finding file=ui/app/index.html line=1 rule=never_read class=model key=-',
        'finding file=ui/app/main.js line=1 rule=never_read class=model key=-',
      ],
    },
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
  expect(fixture('letter-case-counts', { words: (w) => (w['line.time'] = 'The MODEL and the TIER are here.') }), 'PASS');

  const cases = dataLines(copies[CASES]).map((l) => l.split('\t'));
  const matches = cases.filter((c) => c[1] === 'match');
  const misses = cases.filter((c) => c[1] === 'miss');
  if (matches.length !== patterns.length || misses.length !== patterns.length || cases.length !== 2 * patterns.length) {
    wrong.push('cases-pair-each-line');
    lines.push('case cases-pair-each-line want=yes got=no wrong');
  }
  patterns.forEach((p, i) => {
    const [cls, , sentence] = matches[i] ?? ['-', '-', ''];
    const miss = misses[i] ?? ['-', '-', ''];
    const own = cls === p.cls && miss[0] === p.cls && p.re.test(sentence);
    const missed = patterns.every((q) => !q.re.test(miss[2]));
    lines.push(`case pattern-${i + 1}-${p.cls} match=${own ? 'yes' : 'no'} miss=${missed ? 'yes' : 'no'}`);
    if (!own || !missed) wrong.push(`pattern-${i + 1}`);
    expect(
      fixture(`pattern-${i + 1}-${p.cls}-fails-the-check`, { words: (w) => (w['line.time'] = sentence) }),
      'FAIL:never_read',
      { findings: [wordsFinding('never_read', p.cls, 'line.time')] },
    );
  });
  expect(
    fixture('every-miss-sentence-passes', {
      words: (w) => {
        misses.forEach((m, i) => (w[`miss.${i + 1}`] = m[2]));
      },
    }),
    'PASS',
  );

  expect(fixture('state-file-missing', { files: (f) => (f[`${FIXTURES}/state-queue.json`] = null) }), 'FAIL:input');
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
