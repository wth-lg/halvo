import { mkdirSync, mkdtempSync, readFileSync, readdirSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '..', '..', '..', '..');
const FIXTURES = join(REPO, 'ui', 'tests', 'fixtures', 'coord');
const STRUCTURE = join(HERE, 'structure');
const VIEW = '/app/views/agents.js';
const ORDER = ['needs_you', 'problem', 'working', 'waiting_turn', 'all_set', 'off', 'not_yet'];
const DELETE = { call: 'interface_decision', args: ['decision', 'workflow'] };

const { serve, open, act, check } = await import(pathToFileURL(join(REPO, 'tools', 'r4-page', 'index.mjs')).href);

const argv = process.argv.slice(2);
const write = argv.includes('--write');
if (write) mkdirSync(STRUCTURE, { recursive: true });
const only = argv.filter((a) => a !== '--write');
if (only.some((a) => a.startsWith('-'))) {
  process.stderr.write('REFUSED usage: node ui/tests/r4/agents/test.mjs [--write] [state...]\n');
  process.exit(2);
}

const json = (name) => JSON.parse(readFileSync(join(FIXTURES, name), 'utf8'));
const STATES = readdirSync(FIXTURES)
  .map((f) => /^state-(.+)\.json$/.exec(f))
  .filter(Boolean)
  .map((m) => m[1])
  .sort()
  .filter((s) => only.length === 0 || only.includes(s));
const COMMANDS = new Map(json('commands.json').map((c) => [c.command, c.args.map((a) => a.name)]));

let failed = 0;
let passed = 0;
function result(name, ok, detail) {
  if (ok) passed++;
  else failed++;
  console.log('agents test=' + name + ' ' + (ok ? 'ok' : 'fail') + (ok || !detail ? '' : ' detail=' + JSON.stringify(detail)));
}

function same(a, b) {
  return JSON.stringify(a) === JSON.stringify(b);
}

function outDir() {
  return mkdtempSync(join(tmpdir(), 'r4-agents-test-'));
}

function calls(out) {
  const file = join(out, 'calls.jsonl');
  if (!existsSync(file)) return [];
  return readFileSync(file, 'utf8')
    .split('\n')
    .filter(Boolean)
    .map((l) => JSON.parse(l))
    .map((c) => ({ call: c.call, args: c.args }));
}

async function press(h, name, param) {
  await h.page.evaluate(
    ([a, p]) => [...document.querySelectorAll('[data-act]')].find((e) => e.getAttribute('data-act') === a && e.getAttribute('data-param') === p).focus(),
    [name, param],
  );
  await h.page.keyboard.press('Enter');
  await h.page.waitForTimeout(300);
}

function cardActs(state) {
  return state.card ? [{ act: 'open_card', param: state.card.agent }] : [];
}

function pageConfirms(snapshot) {
  return snapshot.items.some((i) => i.kind === 'confirm' && i.presenter === 'page');
}

function expectedModel(snapshot, card) {
  const groups = [];
  for (const status of ORDER) {
    const rows = snapshot.rows.filter((r) => r.status === status);
    if (rows.length === 0) continue;
    groups.push({
      status,
      heading: rows[0].word,
      rows: rows.map((r) => {
        const open = card !== null && card.agent === r.agent;
        return {
          agent: r.agent,
          name: r.name,
          toggle: open ? 'close_card' : 'open_card',
          expanded: open ? 'true' : 'false',
          word: r.word,
          line: r.line,
          controls: r.controls.map((c) => ({ act: c.control, param: r.agent, text: c.text })),
          card: open
            ? {
                label: card.name,
                lines: card.lines.map((l) => l.text),
                next: r.next,
                runs: card.runs.map((run) => ({
                  day: run.day,
                  text: run.text,
                  effects: run.effects.map((e) => ({
                    text: e.text,
                    fields: e.fields.map((f) => ({ label: f.label, value: f.value.join(''), from: f.from_text })),
                  })),
                })),
              }
            : null,
        };
      }),
    });
  }
  return { status: 1, note: snapshot.note, groups };
}

const MODEL = () => {
  const root = document.querySelector('.r4a');
  const text = (el) => (el ? el.textContent : null);
  const groups = [...root.querySelectorAll('.r4a-group')].map((g) => ({
    status: g.getAttribute('data-status'),
    heading: text(g.querySelector('h3')),
    rows: [...g.querySelectorAll('.r4a-row')].map((li) => {
      const toggle = li.querySelector('.r4a-name button');
      const card = li.querySelector('.r4a-card');
      return {
        agent: toggle.getAttribute('data-param'),
        name: toggle.textContent,
        toggle: toggle.getAttribute('data-act'),
        expanded: toggle.getAttribute('aria-expanded'),
        word: text(li.querySelector('.r4a-word')),
        line: text(li.querySelector('.r4a-line')),
        controls: [...li.querySelectorAll('.r4a-controls button')].map((b) => ({
          act: b.getAttribute('data-act'),
          param: b.getAttribute('data-param'),
          text: b.textContent,
        })),
        card: card
          ? {
              label: card.getAttribute('aria-label'),
              lines: [...card.querySelectorAll('.r4a-lines > li')].map((l) => l.textContent),
              next: text(card.querySelector('.r4a-next')),
              runs: [...card.querySelectorAll('.r4a-runs > li')].map((run) => ({
                day: text(run.querySelector('.r4a-day')),
                text: text(run.querySelector('.r4a-ran')),
                effects: [...run.querySelectorAll('.r4a-effects > li')].map((e) => ({
                  text: text(e.querySelector('.r4a-did')),
                  fields: [...e.querySelectorAll('.r4a-fields > div')].map((f) => ({
                    label: text(f.querySelector('dt')),
                    value: text(f.querySelector('.r4a-value')),
                    from: text(f.querySelector('.r4a-from')),
                  })),
                })),
              })),
            }
          : null,
      };
    }),
  }));
  return {
    status: document.querySelectorAll('[role="status"]').length,
    note: text(root.querySelector('.r4a-note')),
    groups,
  };
};

const origin = await serve({ ui: join(REPO, 'ui'), fixtures: FIXTURES });

async function harness(name, options, snapshotFile, expectKeyboard) {
  const h = await open(options);
  const line = {};
  const names = [];
  try {
    for (const n of [1, 2, 3, 4, 5, 6]) {
      const r = n === 3 && write ? await check(h, 3, { snapshot: snapshotFile, write: true }) : await check(h, n, { snapshot: snapshotFile });
      Object.assign(line, r.counts);
      for (const x of r.names) names.push(n + ':' + x);
    }
  } finally {
    await h.close();
  }
  console.log(
    'agents state=' + name + ' violations=' + line.violations + ' review=' + line.review + ' keyboard=' + line.keyboard +
      ' structure=' + line.structure + ' size=' + line.size + ' modes=' + line.modes + ' modes_off=' + line.modes_off +
      ' markup=' + line.markup + ' names=' + (names.length ? names.join(',') : '-'),
  );
  const ok =
    line.violations === 0 &&
    line.structure === 'same' &&
    line.size === 'ok' &&
    line.markup === 'ok' &&
    line.modes_off === '-' &&
    (expectKeyboard
      ? line.keyboard === 'ok' && line.modes === 'ok' && names.length === 0
      : line.keyboard === 'miss' && line.modes === 'miss' && same(names, ['2:dialog_missing']));
  result('harness_' + name, ok, ok ? null : { line, names });
}

for (const s of STATES) {
  const state = json('state-' + s + '.json');
  const acts = cardActs(state);
  const out = outDir();
  const h = await open({ origin, state: s, views: [VIEW], acts, out });
  try {
    const got = await h.page.evaluate(MODEL);
    const want = expectedModel(state.snapshot, state.card);
    result('render_' + s, same(got, want), { got, want });
    const want_calls = acts.map((a) => ({ call: 'agents_card', args: { agent: a.param } }));
    result('card_call_' + s, same(calls(out), want_calls), calls(out));
  } finally {
    await h.close();
  }
  const confirms = pageConfirms(state.snapshot);
  await harness(s, { origin, state: s, views: [VIEW], acts }, join(STRUCTURE, s + '.txt'), !confirms);
  if (confirms) {
    const bare = { ...state.snapshot, items: state.snapshot.items.filter((i) => !(i.kind === 'confirm' && i.presenter === 'page')) };
    await harness(s + '_without_page_confirm', { origin, replies: [{ state: bare }], views: [VIEW] }, join(STRUCTURE, s + '.txt'), true);
  }
}

if (only.length === 0 || only.includes('card')) {
  const state = json('state-card.json');
  const agent = state.card.agent;
  const out = outDir();
  const h = await open({ origin, state: 'card', views: [VIEW], acts: cardActs(state), out });
  try {
    const sentences = await h.page.evaluate(() =>
      [...document.querySelectorAll('.r4a-effects > li')].map((e) => ({
        did: e.querySelector('.r4a-did').textContent,
        values: [...e.querySelectorAll('.r4a-value')].map((v) => v.textContent),
        pieces: [...e.querySelectorAll('.r4a-value')].map((v) => v.childNodes.length),
      })),
    );
    const effects = state.card.runs.flatMap((r) => r.effects);
    result(
      'card_effects_one_line_each_values_in_fields_alone',
      effects.length > 0 &&
        sentences.length === effects.length &&
        sentences.every((s, i) => s.did === effects[i].text && s.values.every((v) => !s.did.includes(v)) && same(s.values, effects[i].fields.map((f) => f.value.join(''))) && same(s.pieces, effects[i].fields.map((f) => f.value.length))),
      { sentences, effects },
    );
    await press(h, 'close_card', agent);
    const closed = await h.page.evaluate(MODEL);
    const want = expectedModel(state.snapshot, null);
    result('close_card_sends_nothing_and_closes', same(closed, want) && calls(out).length === 1, { calls: calls(out) });
    const focus = await h.page.evaluate(() => [document.activeElement.getAttribute('data-act'), document.activeElement.getAttribute('data-param')]);
    result('close_card_keeps_the_focus_on_its_toggle', same(focus, ['open_card', agent]), focus);
  } finally {
    await h.close();
  }
}

for (const s of STATES) {
  const state = json('state-' + s + '.json');
  const out = outDir();
  const h = await open({ origin, state: s, views: [VIEW], out });
  const want = [];
  try {
    for (const r of state.snapshot.rows) {
      for (const c of r.controls) {
        await act(h, { act: c.control, param: r.agent });
        want.push(c.control === 'delete' ? { call: DELETE.call, args: { decision: 'delete', workflow: r.workflow } } : { call: 'agents_control', args: { agent: r.agent, action: c.control } });
      }
      await act(h, { act: 'open_card', param: r.agent });
      want.push({ call: 'agents_card', args: { agent: r.agent } });
      if (state.card && state.card.agent === r.agent) await act(h, { act: 'close_card', param: r.agent });
    }
  } finally {
    await h.close();
  }
  const got = calls(out);
  result('actions_' + s, same(got, want), { got, want });
  const outside = got.filter((c) => {
    const names = c.call === DELETE.call ? DELETE.args : COMMANDS.get(c.call);
    return !names || !Object.keys(c.args).every((k) => names.includes(k));
  });
  result('calls_in_the_command_list_' + s, outside.length === 0, outside);
}

if (only.length === 0 || only.includes('ten')) {
  const first = json('state-ten.json').snapshot;
  const row = first.rows.find((r) => r.controls.some((c) => c.control === 'run_now'));
  const second = {
    ...first,
    seq: 2,
    rows: first.rows.map((r) =>
      r.agent === row.agent
        ? { ...r, status: 'working', word: 'Working', line: 'It is working on its task.', say: 'It changed.', controls: [{ control: 'stop', text: 'Stop' }, { control: 'switch_off', text: 'Switch off' }, { control: 'delete', text: 'Delete' }] }
        : r,
    ),
  };
  const replies = [{ state: first }, { call: 'agents_control', args: { agent: row.agent, action: 'run_now' }, reply: { ok: 'yes' } }, { state: second }];
  const h = await open({ origin, replies, views: [VIEW] });
  try {
    const before = await h.page.evaluate(() => {
      const s = document.querySelectorAll('[role="status"]');
      s[0].r4aMark = 'kept';
      return { count: s.length, text: s[0].textContent };
    });
    await press(h, 'run_now', row.agent);
    const after = await h.page.evaluate(() => {
      const s = document.querySelectorAll('[role="status"]');
      return { count: s.length, mark: s[0].r4aMark || null, text: s[0].textContent, focus: [document.activeElement.getAttribute('data-act'), document.activeElement.getAttribute('data-param')] };
    });
    result('status_region_kept_and_says_a_change', before.count === 1 && before.text === '' && after.count === 1 && after.mark === 'kept' && after.text === 'It changed.', { before, after });
    result('focus_stays_in_the_row_after_a_new_state', same(after.focus, ['open_card', row.agent]), after.focus);
    const model = await h.page.evaluate(MODEL);
    result('a_new_state_is_rendered', same(model, { ...expectedModel(second, null), status: 1 }), model);
  } finally {
    await h.close();
  }
}

if (only.length === 0 || only.includes('ten')) {
  const h = await open({ origin, state: 'ten', views: [VIEW] });
  try {
    const count = () => ({ all: h.started, next: h.nexts });
    const start = count();
    await h.page.evaluate(() => window.Halvo.views.window('hidden'));
    await h.page.waitForTimeout(300);
    const hidden = count();
    await h.page.evaluate(() => window.Halvo.views.window('shown'));
    await h.page.waitForTimeout(300);
    const shown = count();
    const snapshots = shown.all - hidden.all - (shown.next - hidden.next);
    const seen = { start, hidden, shown, snapshots };
    result('a_hidden_view_calls_nothing_and_a_shown_one_reads_again', same(start, hidden) && snapshots === 1 && shown.next - hidden.next === 1, seen);
  } finally {
    await h.close();
  }
}

console.log('agents tests passed=' + passed + ' failed=' + failed + ' states=' + STATES.length);
process.exit(failed === 0 ? 0 : 1);
