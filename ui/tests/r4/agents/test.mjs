import { mkdirSync, mkdtempSync, readFileSync, readdirSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '..', '..', '..', '..');
const FIXTURES = join(REPO, 'ui', 'tests', 'fixtures', 'coord');
const STRUCTURE = join(HERE, 'structure');
const VIEW = '/app/views/agents.js';
const NEEDS = '/app/views/needs.js';
const CONFIRM = '/app/views/confirm.js';
const ORDER = ['needs_you', 'problem', 'working', 'waiting_turn', 'all_set', 'off', 'not_yet'];
const ACTING = ['run_now', 'stop', 'take_back', 'switch_off', 'switch_on'];
const DRAWN = ['run_now', 'stop', 'take_back', 'switch_off', 'switch_on', 'stop_sign_in', 'delete'];

const { serve, open, act, check, settled } = await import(pathToFileURL(join(REPO, 'tools', 'r4-page', 'index.mjs')).href);

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
const want = (s) => only.length === 0 || only.includes(s);

let failed = 0;
let passed = 0;
function result(name, ok, detail) {
  if (ok) passed++;
  else failed++;
  console.log('agents test=' + name + ' ' + (ok ? 'ok' : 'fail') + (ok || !detail ? '' : ' detail=' + JSON.stringify(detail)));
}

async function test(name, fn) {
  try {
    const [ok, detail] = await fn();
    result(name, ok, detail);
  } catch (err) {
    result(name, false, { threw: String((err && err.message) || err).split('\n')[0] });
  }
}

async function withPage(options, fn) {
  const h = await open(options);
  try {
    return await fn(h);
  } finally {
    await h.close();
  }
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

const sel = (a, p) => '[data-act="' + a + '"][data-param="' + p + '"]';
const frames = (h) => h.page.evaluate(() => new Promise((d) => requestAnimationFrame(() => requestAnimationFrame(d))));
const settleCalls = async (h) => {
  await settled(h);
  await frames(h);
};
const callOf = (row, control) => ({ call: 'agents_control', args: { agent: row.agent, action: control } });
const focusOf = (h) => h.page.evaluate(() => [document.activeElement.getAttribute('data-act'), document.activeElement.getAttribute('data-param')]);
const cardsOf = (h) => h.page.evaluate(() => [...document.querySelectorAll('.r4a-card')].map((c) => c.id));

async function press(h, name, param) {
  await h.page.focus(sel(name, param));
  await h.page.keyboard.press('Enter');
  await settleCalls(h);
}

async function pointAt(h, name, param) {
  const box = await h.page.locator(sel(name, param)).boundingBox();
  const x = box.x + box.width / 2;
  const y = box.y + box.height / 2;
  await h.page.mouse.move(x, y);
  return [x, y];
}

async function clickAt(h, count) {
  await h.page.mouse.down({ clickCount: count });
  await h.page.mouse.up({ clickCount: count });
}

const underPointer = (h, [x, y]) =>
  h.page.evaluate(([px, py]) => {
    const b = document.elementFromPoint(px, py);
    const c = b && b.closest('button');
    return c ? c.getAttribute('data-act') + ':' + c.getAttribute('data-param') : null;
  }, [x, y]);

function cardActs(state) {
  return state.card ? [{ act: 'open_card', param: state.card.agent }] : [];
}

function pageConfirmFirst(snapshot) {
  const first = snapshot.items[0];
  return Boolean(first && first.kind === 'confirm' && first.presenter === 'page');
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
          level: r.level,
          controls: r.controls.filter((c) => DRAWN.includes(c.control)).map((c) => ({ act: c.control, param: r.agent, text: c.text })),
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
  const attr = (el, name) => (el ? el.getAttribute(name) : null);
  const groups = [...root.querySelectorAll('.r4a-group')].map((g) => ({
    status: g.getAttribute('data-status'),
    heading: text(g.querySelector('h3')),
    rows: [...g.querySelectorAll('.r4a-row')].map((li) => {
      const toggle = li.querySelector('.r4a-name button');
      const card = li.querySelector('.r4a-card');
      return {
        agent: attr(toggle, 'data-param'),
        name: text(toggle),
        toggle: attr(toggle, 'data-act'),
        expanded: attr(toggle, 'aria-expanded'),
        word: text(li.querySelector('.r4a-word')),
        line: text(li.querySelector('.r4a-line')),
        level: text(li.querySelector('.r4a-level')),
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

async function harness(name, options, snapshotFile) {
  await test('harness_' + name, async () => {
    const line = {};
    const names = [];
    await withPage(options, async (h) => {
      for (const n of [1, 2, 3, 4, 5, 6]) {
        const r = n === 3 && write ? await check(h, 3, { snapshot: snapshotFile, write: true }) : await check(h, n, { snapshot: snapshotFile });
        Object.assign(line, r.counts);
        for (const x of r.names) names.push(n + ':' + x);
      }
    });
    console.log(
      'agents state=' + name + ' violations=' + line.violations + ' review=' + line.review + ' keyboard=' + line.keyboard +
        ' structure=' + line.structure + ' size=' + line.size + ' modes=' + line.modes + ' modes_off=' + line.modes_off +
        ' markup=' + line.markup + ' names=' + (names.length ? names.join(',') : '-'),
    );
    const ok =
      line.violations === 0 &&
      line.keyboard === 'ok' &&
      line.structure === 'same' &&
      line.size === 'ok' &&
      line.modes === 'ok' &&
      line.modes_off === '-' &&
      line.markup === 'ok' &&
      names.length === 0;
    return [ok, { line, names }];
  });
}

const deletes = { inReplies: 0, shown: 0 };
for (const s of STATES) {
  const state = json('state-' + s + '.json');
  const acts = cardActs(state);
  const out = outDir();
  await test('render_' + s, async () =>
    withPage({ origin, state: s, views: [CONFIRM, VIEW], acts, out }, async (h) => {
      deletes.inReplies += state.snapshot.rows.filter((r) => r.controls.some((c) => c.control === 'delete')).length;
      deletes.shown += await h.page.evaluate(() => document.querySelectorAll('[data-act="delete"]').length);
      const got = await h.page.evaluate(MODEL);
      return [same(got, expectedModel(state.snapshot, state.card)), { got }];
    }),
  );
  result('card_call_' + s, same(calls(out), acts.map((a) => ({ call: 'agents_card', args: { agent: a.param } }))), calls(out));
  const views = pageConfirmFirst(state.snapshot) ? [CONFIRM, NEEDS, VIEW] : [CONFIRM, VIEW];
  await harness(s + (views.length > 2 ? '_with_both_views' : ''), { origin, state: s, views, acts }, join(STRUCTURE, s + '.txt'));
}
if (STATES.length > 0) {
  result('every_row_that_the_core_gives_delete_shows_delete', deletes.inReplies > 0 && deletes.shown === deletes.inReplies, deletes);
}

for (const s of STATES) {
  const state = json('state-' + s + '.json');
  const out = outDir();
  const wanted = [];
  await test('actions_' + s, async () => {
    await withPage({ origin, state: s, views: [CONFIRM, VIEW], out }, async (h) => {
      for (const r of state.snapshot.rows) {
        for (const c of r.controls.filter((x) => ACTING.includes(x.control))) {
          await act(h, { act: c.control, param: r.agent });
          wanted.push(callOf(r, c.control));
          await settleCalls(h);
        }
        await act(h, { act: 'open_card', param: r.agent });
        wanted.push({ call: 'agents_card', args: { agent: r.agent } });
        await settleCalls(h);
        if (state.card && state.card.agent === r.agent) await act(h, { act: 'close_card', param: r.agent });
      }
    });
    return [same(calls(out), wanted), { got: calls(out), wanted }];
  });
  const outside = calls(out).filter((c) => {
    const names = COMMANDS.get(c.call);
    return !names || !Object.keys(c.args).every((k) => names.includes(k));
  });
  result('calls_in_the_command_list_' + s, outside.length === 0, outside);
}

if (want('card')) {
  const state = json('state-card.json');
  const A = state.card.agent;
  const rows = state.snapshot.rows;
  const B = rows.find((r) => r.agent !== A).agent;
  const cardB = { ...state.card, agent: B, name: rows.find((r) => r.agent === B).name };

  await test('card_effects_one_line_each_values_in_fields_alone', async () =>
    withPage({ origin, state: 'card', views: [CONFIRM, VIEW], acts: cardActs(state) }, async (h) => {
      const sentences = await h.page.evaluate(() =>
        [...document.querySelectorAll('.r4a-effects > li')].map((e) => ({
          did: e.querySelector('.r4a-did').textContent,
          values: [...e.querySelectorAll('.r4a-value')].map((v) => v.textContent),
          pieces: [...e.querySelectorAll('.r4a-value')].map((v) => v.childNodes.length),
        })),
      );
      const effects = state.card.runs.flatMap((r) => r.effects);
      const ok =
        effects.length > 0 &&
        sentences.length === effects.length &&
        sentences.every((s, i) => s.did === effects[i].text && s.values.every((v) => !s.did.includes(v)) && same(s.values, effects[i].fields.map((f) => f.value.join(''))) && same(s.pieces, effects[i].fields.map((f) => f.value.length)));
      return [ok, { sentences }];
    }),
  );

  await test('a_value_of_two_pieces_shows_as_two_text_nodes', async () => {
    const two = JSON.parse(JSON.stringify(state.card));
    two.runs[0].effects[0].fields[0].value = ['City ', 'Power'];
    const replies = [{ state: state.snapshot }, { call: 'agents_card', args: { agent: A }, reply: two }];
    return withPage({ origin, replies, views: [CONFIRM, VIEW] }, async (h) => {
      await act(h, { act: 'open_card', param: A });
      await settleCalls(h);
      const value = await h.page.evaluate(() => {
        const v = document.querySelector('.r4a-value');
        return v ? { text: v.textContent, nodes: [...v.childNodes].map((n) => n.nodeType + ':' + n.textContent) } : null;
      });
      return [same(value, { text: 'City Power', nodes: ['3:City ', '3:Power'] }), value];
    });
  });

  await test('a_card_of_another_agent_is_not_shown', async () =>
    withPage({ origin, state: 'card', views: [CONFIRM, VIEW] }, async (h) => {
      await act(h, { act: 'open_card', param: B });
      await settleCalls(h);
      const cards = await cardsOf(h);
      const toggles = await h.page.evaluate(() => [...document.querySelectorAll('.r4a-name button')].map((b) => b.getAttribute('data-act')));
      return [same(cards, []) && toggles.every((t) => t === 'open_card'), { cards, toggles }];
    }),
  );

  await test('a_card_reply_without_effects_opens_no_card_and_keeps_the_list', async () => {
    const bad = JSON.parse(JSON.stringify(state.card));
    delete bad.runs[0].effects;
    const replies = [{ state: state.snapshot }, { call: 'agents_card', args: { agent: A }, reply: bad }];
    return withPage({ origin, replies, views: [CONFIRM, VIEW] }, async (h) => {
      await act(h, { act: 'open_card', param: A });
      await settleCalls(h);
      const cards = await cardsOf(h);
      const model = await h.page.evaluate(MODEL);
      const ok = same(cards, []) && same(model, expectedModel(state.snapshot, null)) && h.errors === 0;
      return [ok, { cards, errors: h.errors }];
    });
  });

  await test('close_card_sends_nothing_closes_and_keeps_the_focus', async () => {
    const out = outDir();
    return withPage({ origin, state: 'card', views: [CONFIRM, VIEW], acts: cardActs(state), out }, async (h) => {
      await press(h, 'close_card', A);
      const closed = await h.page.evaluate(MODEL);
      const focus = await focusOf(h);
      const ok = same(closed, expectedModel(state.snapshot, null)) && calls(out).length === 1 && same(focus, ['open_card', A]);
      return [ok, { calls: calls(out), focus }];
    });
  });

  await test('the_focus_goes_back_to_the_toggle_after_a_card_opens', async () =>
    withPage({ origin, state: 'card', views: [CONFIRM, VIEW] }, async (h) => {
      await press(h, 'open_card', A);
      const focus = await focusOf(h);
      return [same(focus, ['close_card', A]), focus];
    }),
  );

  await test('an_open_card_whose_agent_left_is_dropped', async () => {
    const snap = state.snapshot;
    const left = { ...snap, seq: 2, rows: snap.rows.filter((r) => r.agent !== A) };
    const back = { ...snap, seq: 3 };
    const replies = [{ state: snap }, { call: 'agents_card', args: { agent: A }, reply: state.card }, { state: left }, { state: back }];
    return withPage({ origin, replies, views: [CONFIRM, VIEW] }, async (h) => {
      await act(h, { act: 'open_card', param: A });
      await settleCalls(h);
      const cards = await cardsOf(h);
      const toggle = await h.page.evaluate((a) => [...document.querySelectorAll('[data-param="' + a + '"]')].map((b) => b.getAttribute('data-act'))[0], A);
      return [same(cards, []) && toggle === 'open_card', { cards, toggle }];
    });
  });

  await test('a_close_clears_a_card_asked_for_another_agent', async () => {
    const out = outDir();
    const replies = [{ state: state.snapshot }, { call: 'agents_card', args: { agent: A }, reply: state.card }, { call: 'agents_card', args: { agent: B }, reply: cardB }, { call: 'agents_card', args: { agent: B }, reply: cardB }];
    return withPage({ origin, replies, views: [CONFIRM, VIEW], out }, async (h) => {
      await act(h, { act: 'open_card', param: A });
      await h.page.evaluate(([a, b]) => {
        const pick = (act, p) => [...document.querySelectorAll('button')].find((x) => x.getAttribute('data-act') === act && x.getAttribute('data-param') === p);
        pick('open_card', b).click();
        pick('close_card', a).click();
      }, [A, B]);
      await settleCalls(h);
      const after = await cardsOf(h);
      await act(h, { act: 'open_card', param: B });
      await settleCalls(h);
      const again = await cardsOf(h);
      return [same(after, []) && same(again, ['r4a-card-' + B]) && calls(out).length === 3, { after, again, calls: calls(out) }];
    });
  });

  await test('a_card_call_that_rejects_gives_the_name_back', async () => {
    const out = outDir();
    const replies = [{ state: state.snapshot }, { call: 'agents_card', args: { agent: A }, reject: true }, { call: 'agents_card', args: { agent: A }, reply: state.card }];
    return withPage({ origin, replies, views: [CONFIRM, VIEW], out }, async (h) => {
      await act(h, { act: 'open_card', param: A });
      await settleCalls(h);
      await act(h, { act: 'open_card', param: A });
      await settleCalls(h);
      const cards = await cardsOf(h);
      return [same(cards, ['r4a-card-' + A]) && calls(out).length === 2, { cards, calls: calls(out) }];
    });
  });

  await test('one_call_and_one_card_for_a_double_click_on_a_name', async () => {
    const out = outDir();
    return withPage({ origin, state: 'card', views: [CONFIRM, VIEW], out }, async (h) => {
      await h.page.dblclick(sel('open_card', A));
      await settleCalls(h);
      const cards = await cardsOf(h);
      return [same(calls(out), [{ call: 'agents_card', args: { agent: A } }]) && same(cards, ['r4a-card-' + A]), { got: calls(out), cards }];
    });
  });

  await test('a_second_click_on_a_name_after_its_card_opened_keeps_it_open', async () => {
    const out = outDir();
    return withPage({ origin, state: 'card', views: [CONFIRM, VIEW], out }, async (h) => {
      const at = await pointAt(h, 'open_card', A);
      await clickAt(h, 1);
      await settleCalls(h);
      const under = await underPointer(h, at);
      await clickAt(h, 2);
      await settleCalls(h);
      const cards = await cardsOf(h);
      return [under === 'close_card:' + A && same(cards, ['r4a-card-' + A]) && calls(out).length === 1, { under, cards, calls: calls(out) }];
    });
  });

  await test('a_second_press_while_a_card_is_asked_for_sends_nothing_and_a_close_holds', async () => {
    const out = outDir();
    return withPage({ origin, state: 'card', views: [CONFIRM, VIEW], out }, async (h) => {
      const seen = await h.page.evaluate(
        (agent) =>
          new Promise((done) => {
            const pick = (a) => [...document.querySelectorAll('button')].find((x) => x.getAttribute('data-act') === a && x.getAttribute('data-param') === agent);
            let closed = false;
            const watch = new MutationObserver(() => {
              const c = pick('close_card');
              if (!closed && c) {
                closed = true;
                c.click();
              }
            });
            watch.observe(document.body, { childList: true, subtree: true });
            const b = pick('open_card');
            b.click();
            b.click();
            setTimeout(() => {
              watch.disconnect();
              done({ closed, shown: document.getElementById('r4a-card-' + agent) !== null });
            }, 1000);
          }),
        A,
      );
      const asked = calls(out).filter((c) => c.call === 'agents_card').length;
      return [seen.closed && !seen.shown && asked === 1, { seen, asked }];
    });
  });
}

for (const [s, control] of [['ten', 'run_now'], ['ten', 'switch_off'], ['working', 'stop'], ['off', 'switch_on']]) {
  if (!want(s)) continue;
  const row = json('state-' + s + '.json').snapshot.rows.find((r) => r.controls.some((c) => c.control === control));
  for (const how of ['held_enter', 'double_click', 'held_space']) {
    if (how === 'held_space' && control !== 'run_now') continue;
    const out = outDir();
    await test('one_call_for_' + how + '_on_' + control, async () => {
      await withPage({ origin, state: s, views: [CONFIRM, VIEW], out }, async (h) => {
        if (how === 'double_click') {
          await h.page.dblclick(sel(control, row.agent));
        } else {
          const key = how === 'held_enter' ? 'Enter' : ' ';
          await h.page.focus(sel(control, row.agent));
          for (let i = 0; i < 3; i++) {
            await h.page.keyboard.down(key);
            await settleCalls(h);
          }
          await h.page.keyboard.up(key);
        }
        await settleCalls(h);
      });
      return [same(calls(out), [callOf(row, control)]), calls(out)];
    });
  }
}

if (want('ten')) {
  const ten = json('state-ten.json').snapshot;
  const [A, B] = ten.rows;

  await test('an_answered_call_gives_the_controls_back', async () => {
    const out = outDir();
    await withPage({ origin, state: 'ten', views: [CONFIRM, VIEW], out }, async (h) => {
      await act(h, { act: 'run_now', param: A.agent });
      await settleCalls(h);
      await act(h, { act: 'run_now', param: A.agent });
      await settleCalls(h);
    });
    return [same(calls(out), [callOf(A, 'run_now'), callOf(A, 'run_now')]), calls(out)];
  });

  await test('a_call_on_its_way_holds_its_row_and_no_other', async () => {
    const out = outDir();
    const replies = [{ state: ten }, { call: 'agents_control', args: { agent: A.agent, action: 'run_now' }, hold: true }, { call: 'agents_control', args: { agent: B.agent, action: 'run_now' }, reply: { ok: 'yes' } }];
    await withPage({ origin, replies, views: [CONFIRM, VIEW], out }, async (h) => {
      await act(h, { act: 'run_now', param: A.agent });
      await settleCalls(h);
      await act(h, { act: 'run_now', param: A.agent });
      await act(h, { act: 'switch_off', param: A.agent });
      await act(h, { act: 'open_card', param: A.agent });
      await settleCalls(h);
      await act(h, { act: 'run_now', param: B.agent });
      await settleCalls(h);
    });
    return [same(calls(out), [callOf(A, 'run_now'), callOf(B, 'run_now')]), calls(out)];
  });

  await test('a_newer_reply_gives_the_controls_back', async () => {
    const out = outDir();
    const replies = [{ state: ten }, { call: 'agents_control', args: { agent: A.agent, action: 'run_now' }, hold: true }, { state: { ...ten, seq: 2 } }, { call: 'agents_control', args: { agent: A.agent, action: 'run_now' }, reply: { ok: 'yes' } }];
    await withPage({ origin, replies, views: [CONFIRM, VIEW], out }, async (h) => {
      await act(h, { act: 'run_now', param: A.agent });
      await settleCalls(h);
      await act(h, { act: 'run_now', param: A.agent });
      await settleCalls(h);
    });
    return [same(calls(out), [callOf(A, 'run_now'), callOf(A, 'run_now')]), calls(out)];
  });

  await test('a_rejected_call_gives_the_controls_back', async () => {
    const out = outDir();
    const replies = [{ state: ten }, { call: 'agents_control', args: { agent: A.agent, action: 'run_now' }, reject: true }, { call: 'agents_control', args: { agent: A.agent, action: 'run_now' }, reply: { ok: 'yes' } }];
    await withPage({ origin, replies, views: [CONFIRM, VIEW], out }, async (h) => {
      await act(h, { act: 'run_now', param: A.agent });
      await settleCalls(h);
      await act(h, { act: 'run_now', param: A.agent });
      await settleCalls(h);
    });
    return [same(calls(out), [callOf(A, 'run_now'), callOf(A, 'run_now')]), calls(out)];
  });

  await test('a_double_click_across_a_reply_starts_the_first_agent_alone', async () => {
    const out = outDir();
    const gone = { ...ten, seq: 2, rows: ten.rows.filter((r) => r.agent !== A.agent) };
    const replies = [{ state: ten }, { call: 'agents_control', args: { agent: A.agent, action: 'run_now' }, reply: { ok: 'yes' } }, { state: gone }, { call: 'agents_control', args: { agent: B.agent, action: 'run_now' }, reply: { ok: 'yes' } }];
    let under = null;
    await withPage({ origin, replies, views: [CONFIRM, VIEW], out }, async (h) => {
      const at = await pointAt(h, 'run_now', A.agent);
      await clickAt(h, 1);
      await h.page.waitForFunction((a) => document.querySelector('[data-param="' + a + '"]') === null, A.agent, { timeout: 3000 });
      await frames(h);
      under = await underPointer(h, at);
      await clickAt(h, 2);
      await settleCalls(h);
    });
    return [under === 'run_now:' + B.agent && same(calls(out), [callOf(A, 'run_now')]), { under, got: calls(out) }];
  });

  const touchCase = async (act, sent) => {
    const out = outDir();
    const C = ten.rows[2];
    const gone = { ...ten, seq: 2, rows: ten.rows.filter((r) => r.agent !== A.agent) };
    const replies = [{ state: ten }, { call: 'agents_card', args: { agent: C.agent }, reply: { error: 'busy' } }, { state: gone }, { ...sent, reply: act === 'open_card' ? { error: 'busy' } : { ok: 'yes' } }];
    return withPage({ origin, replies, views: [CONFIRM, VIEW], out }, async (h) => {
      const box = await h.page.locator(sel(act, A.agent)).boundingBox();
      const point = { x: box.x + box.width / 2, y: box.y + box.height / 2 };
      const cdp = await h.page.context().newCDPSession(h.page);
      await cdp.send('Emulation.setTouchEmulationEnabled', { enabled: true, maxTouchPoints: 1 });
      const touch = (kind) => cdp.send('Input.dispatchTouchEvent', kind === 'start' ? { type: 'touchStart', touchPoints: [point] } : { type: 'touchEnd', touchPoints: [] });
      await touch('start');
      await h.page.evaluate((c) => window.Halvo.ipc.call('agents_card', { agent: c }), C.agent);
      await h.page.waitForFunction((a) => document.querySelector('[data-param="' + a + '"]') === null, A.agent, { timeout: 3000 });
      await settleCalls(h);
      const under = await underPointer(h, [point.x, point.y]);
      await touch('end');
      await settleCalls(h);
      const afterBegunBefore = calls(out);
      await h.page.waitForTimeout(800);
      await touch('start');
      await touch('end');
      await settleCalls(h);
      const afterFresh = calls(out);
      const gate = { call: 'agents_card', args: { agent: C.agent } };
      const ok = under === act + ':' + B.agent && same(afterBegunBefore, [gate]) && same(afterFresh, [gate, { call: sent.call, args: sent.args }]);
      return [ok, { under, afterBegunBefore, afterFresh }];
    });
  };

  const engine = await withPage({ origin, state: 'empty', views: [CONFIRM, VIEW] }, async (h) => h.page.context().browser().browserType().name());
  if (engine === 'chromium') {
    await test('a_touch_begun_before_a_reply_moved_another_control_under_it_sends_nothing', () => touchCase('run_now', callOf(B, 'run_now')));
    await test('a_touch_begun_before_a_reply_moved_another_name_under_it_opens_nothing', () => touchCase('open_card', { call: 'agents_card', args: { agent: B.agent } }));
  } else {
    console.log('agents touch: not run on ' + engine + ', which takes no touch input through the browser\'s protocol in this harness');
  }

  await test('status_region_kept_says_a_change_and_the_focus_stays_in_the_row', async () => {
    const second = {
      ...ten,
      seq: 2,
      rows: ten.rows.map((r) => (r.agent === A.agent ? { ...r, status: 'working', word: 'Working', line: 'It is working on its task.', say: 'It changed.', controls: [{ control: 'stop', text: 'Stop' }, { control: 'switch_off', text: 'Switch off' }, { control: 'delete', text: 'Delete' }] } : r)),
    };
    const replies = [{ state: ten }, { call: 'agents_control', args: { agent: A.agent, action: 'run_now' }, reply: { ok: 'yes' } }, { state: second }];
    return withPage({ origin, replies, views: [CONFIRM, VIEW] }, async (h) => {
      const before = await h.page.evaluate(() => {
        const s = document.querySelectorAll('[role="status"]');
        s[0].r4aMark = 'kept';
        return { count: s.length, text: s[0].textContent };
      });
      await press(h, 'run_now', A.agent);
      const after = await h.page.evaluate(() => {
        const s = document.querySelectorAll('[role="status"]');
        return { count: s.length, mark: s[0].r4aMark || null, text: s[0].textContent };
      });
      const focus = await focusOf(h);
      const model = await h.page.evaluate(MODEL);
      const ok = before.count === 1 && before.text === '' && after.count === 1 && after.mark === 'kept' && after.text === 'It changed.' && same(focus, ['open_card', A.agent]) && same(model, expectedModel(second, null));
      return [ok, { before, after, focus }];
    });
  });

  await test('rd76_each_addition_takes_out_every_sentence_that_the_status_element_holds', async () => {
    const flip = (snap, seq, row, say) => ({ ...snap, seq, rows: snap.rows.map((r) => (r.agent === row.agent ? { ...r, status: 'off', word: 'Off', say } : r)) });
    const second = flip(ten, 2, A, 'First change.');
    const third = flip(second, 3, B, 'Second change.');
    return withPage({ origin, replies: [{ state: ten }, { state: second }, { state: third }], views: [CONFIRM, VIEW] }, async (h) => {
      const seen = await h.page.evaluate(() => {
        const s = document.querySelector('[role="status"]');
        return { says: [...s.children].map((p) => p.textContent), live: s.getAttribute('aria-live'), atomic: s.getAttribute('aria-atomic') };
      });
      return [same(seen, { says: ['Second change.'], live: 'polite', atomic: 'true' }), seen];
    });
  });

  await test('a_hidden_view_calls_nothing_and_a_shown_one_reads_again', async () =>
    withPage({ origin, state: 'ten', views: [CONFIRM, VIEW] }, async (h) => {
      const count = () => ({ all: h.started, next: h.nexts });
      const start = count();
      await h.page.evaluate(() => window.Halvo.views.window('hidden'));
      await h.page.waitForTimeout(300);
      const hidden = count();
      await h.page.evaluate(() => window.Halvo.views.window('shown'));
      await h.page.waitForTimeout(300);
      const shown = count();
      const snapshots = shown.all - hidden.all - (shown.next - hidden.next);
      return [same(start, hidden) && snapshots === 1 && shown.next - hidden.next === 1, { start, hidden, shown }];
    }),
  );

  await test('a_second_show_while_shown_starts_no_second_reader', async () =>
    withPage({ origin, state: 'ten', views: [CONFIRM, VIEW] }, async (h) => {
      const before = h.started;
      await h.page.evaluate(() => window.Halvo.views.window('shown'));
      await h.page.waitForTimeout(300);
      return [h.started === before, { before, after: h.started }];
    }),
  );

  await test('a_closed_reply_ends_the_calls', async () =>
    withPage({ origin, replies: [{ state: ten }, { state: { closed: 'yes' } }], views: [CONFIRM, VIEW] }, async (h) => {
      await h.page.waitForTimeout(500);
      return [h.started === 2, { started: h.started }];
    }),
  );

  await test('a_waiting_call_that_answers_without_a_state_ends_until_the_next_show', async () =>
    withPage({ origin, replies: [{ state: ten }, { state: { error: 'busy' } }], views: [CONFIRM, VIEW] }, async (h) => {
      await h.page.waitForTimeout(500);
      const ended = h.started;
      await h.page.evaluate(() => window.Halvo.views.window('hidden'));
      await h.page.evaluate(() => window.Halvo.views.window('shown'));
      await h.page.waitForTimeout(300);
      return [ended === 2 && same(h.mismatch, ['agents_snapshot']), { ended, mismatch: h.mismatch }];
    }),
  );

  await test('a_status_or_a_control_outside_the_closed_lists_is_not_shown', async () => {
    const odd = {
      ...ten,
      seq: 2,
      rows: ten.rows.map((r) => {
        if (r.agent === A.agent) return { ...r, status: 'paused', word: 'Paused', say: 'It paused.' };
        if (r.agent === B.agent) return { ...r, controls: [...r.controls, { control: 'switch_offf', text: 'Switch offf' }, { control: 'open_card', text: 'Open' }] };
        return r;
      }),
    };
    return withPage({ origin, replies: [{ state: ten }, { state: odd }], views: [CONFIRM, VIEW] }, async (h) => {
      const seen = await h.page.evaluate(([a, b]) => ({
        says: document.querySelector('[role="status"]').textContent,
        rowA: document.querySelectorAll('[data-param="' + a + '"]').length,
        actsB: [...document.querySelectorAll('[data-param="' + b + '"]')].map((x) => x.getAttribute('data-act')),
      }), [A.agent, B.agent]);
      const wantB = ['open_card', ...B.controls.filter((c) => DRAWN.includes(c.control)).map((c) => c.control)];
      return [seen.says === '' && seen.rowA === 0 && same(seen.actsB, wantB), seen];
    });
  });

  await test('each_control_is_described_by_its_agents_name', async () =>
    withPage({ origin, state: 'ten', views: [CONFIRM, VIEW] }, async (h) => {
      const all = await h.page.evaluate(() =>
        [...document.querySelectorAll('.r4a-controls button')].map((b) => {
          const d = document.getElementById(b.getAttribute('aria-describedby') || '');
          return d && d.getAttribute('data-param') === b.getAttribute('data-param') && /_card$/.test(d.getAttribute('data-act'));
        }),
      );
      return [all.length > 0 && all.every(Boolean), all];
    }),
  );
}


async function walkTo(h, name, param) {
  for (let i = 0; i < 60; i++) {
    const at = await h.page.evaluate(() => (document.activeElement ? document.activeElement.getAttribute('data-act') + ':' + document.activeElement.getAttribute('data-param') : ''));
    if (at === name + ':' + param) return i;
    await h.page.keyboard.press(h.build.walkKey);
  }
  throw new Error('the walk key does not reach ' + name);
}

const clickWithoutPress = (h) => h.page.evaluate(() => document.activeElement.click());

if (want('ten')) {
  const ten = json('state-ten.json').snapshot;
  const [A, B, C] = ten.rows;
  const gate = { call: 'agents_card', args: { agent: C.agent }, reply: { error: 'busy' } };

  await test('a_click_with_no_press_after_the_walk_key_acts_once_on_a_row_control', async () => {
    const out = outDir();
    const replies = [{ state: ten }, { ...callOf(C, 'switch_off'), reply: { ok: 'yes' } }, { ...callOf(A, 'run_now'), reply: { ok: 'yes' } }, { ...callOf(A, 'run_now'), reply: { ok: 'yes' } }];
    return withPage({ origin, replies, views: [CONFIRM, VIEW], out }, async (h) => {
      await h.page.focus(sel('switch_off', C.agent));
      await h.page.keyboard.press('Enter');
      await settleCalls(h);
      const presses = await walkTo(h, 'run_now', A.agent);
      await clickWithoutPress(h);
      await settleCalls(h);
      const got = calls(out);
      return [same(got, [callOf(C, 'switch_off'), callOf(A, 'run_now')]), { presses, got }];
    });
  });

  await test('a_click_with_no_press_after_a_mouse_released_outside_acts_once', async () => {
    const out = outDir();
    const aGone = { ...ten, seq: 2, rows: ten.rows.filter((r) => r.agent !== A.agent) };
    const replies = [{ state: ten }, gate, { state: aGone }, { ...callOf(B, 'run_now'), reply: { ok: 'yes' } }, { ...callOf(B, 'run_now'), reply: { ok: 'yes' } }];
    return withPage({ origin, replies, views: [CONFIRM, VIEW], out }, async (h) => {
      await pointAt(h, 'run_now', A.agent);
      await h.page.mouse.down();
      await h.page.evaluate((c) => window.Halvo.ipc.call('agents_card', { agent: c }), C.agent);
      await h.page.waitForFunction((a) => document.querySelector('[data-param="' + a + '"]') === null, A.agent, { timeout: 3000 });
      await h.page.mouse.move(-30, -30);
      await h.page.mouse.up();
      await settleCalls(h);
      const afterRelease = calls(out).length;
      await h.page.evaluate((s) => document.querySelector(s).click(), sel('run_now', B.agent));
      await settleCalls(h);
      const got = calls(out);
      return [afterRelease === 1 && same(got, [{ call: gate.call, args: gate.args }, callOf(B, 'run_now')]), { afterRelease, got }];
    });
  });

  await test('a_pointer_click_counts_only_for_a_press_begun_on_its_own_control', async () => {
    const out = outDir();
    return withPage({ origin, state: 'ten', views: [CONFIRM, VIEW], out }, async (h) => {
      const pointerClick = (from, to) =>
        h.page.evaluate(
          ([f, t]) => {
            document.querySelector(f).dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, isPrimary: true }));
            document.querySelector(t).dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, detail: 1 }));
          },
          [from, to],
        );
      await pointerClick(sel('run_now', A.agent), sel('run_now', B.agent));
      await settleCalls(h);
      const elsewhere = calls(out).length;
      await pointerClick(sel('open_card', A.agent), sel('open_card', B.agent));
      await settleCalls(h);
      const elsewhereName = calls(out).length;
      await pointerClick(sel('run_now', B.agent), sel('run_now', B.agent));
      await settleCalls(h);
      const got = calls(out);
      return [elsewhere === 0 && elsewhereName === 0 && same(got, [callOf(B, 'run_now')]), { elsewhere, elsewhereName, got }];
    });
  });

  await test('a_key_click_counts_only_for_a_key_pressed_on_its_own_control', async () => {
    const out = outDir();
    return withPage({ origin, state: 'ten', views: [CONFIRM, VIEW], out }, async (h) => {
      const keyClick = (from, to) =>
        h.page.evaluate(
          ([f, t]) => {
            document.querySelector(f).dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }));
            document.querySelector(t).dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, detail: 0 }));
          },
          [from, to],
        );
      await keyClick(sel('run_now', A.agent), sel('run_now', B.agent));
      await settleCalls(h);
      const elsewhere = calls(out).length;
      await keyClick(sel('open_card', A.agent), sel('open_card', B.agent));
      await settleCalls(h);
      const elsewhereName = calls(out).length;
      await keyClick(sel('run_now', B.agent), sel('run_now', B.agent));
      await settleCalls(h);
      const got = calls(out);
      return [elsewhere === 0 && elsewhereName === 0 && same(got, [callOf(B, 'run_now')]), { elsewhere, elsewhereName, got }];
    });
  });
}

if (want('card')) {
  const state = json('state-card.json');
  const A = state.card.agent;

  await test('a_click_with_no_press_after_the_walk_key_acts_once_on_a_name', async () => {
    const out = outDir();
    const replies = [{ state: state.snapshot }, { call: 'agents_card', args: { agent: A }, reply: state.card }, { call: 'agents_card', args: { agent: A }, reply: state.card }];
    return withPage({ origin, replies, views: [CONFIRM, VIEW], out }, async (h) => {
      const presses = await walkTo(h, 'open_card', A);
      await clickWithoutPress(h);
      await settleCalls(h);
      const cards = await cardsOf(h);
      return [same(cards, ['r4a-card-' + A]) && calls(out).length === 1, { presses, cards, calls: calls(out) }];
    });
  });

  await test('a_card_reply_whose_field_value_is_no_list_opens_no_card', async () => {
    const bad = JSON.parse(JSON.stringify(state.card));
    bad.runs[0].effects[0].fields[0].value = 'City Power';
    const replies = [{ state: state.snapshot }, { call: 'agents_card', args: { agent: A }, reply: bad }];
    return withPage({ origin, replies, views: [CONFIRM, VIEW] }, async (h) => {
      await act(h, { act: 'open_card', param: A });
      await settleCalls(h);
      const cards = await cardsOf(h);
      const model = await h.page.evaluate(MODEL);
      return [same(cards, []) && same(model, expectedModel(state.snapshot, null)) && h.errors === 0, { cards, errors: h.errors }];
    });
  });
}


const AGENTS_SRC = readFileSync(join(REPO, 'ui', 'app', 'views', 'agents.js'), 'utf8');
const NEEDS_SRC = readFileSync(join(REPO, 'ui', 'app', 'views', 'needs.js'), 'utf8');

/** Runs the sources of views in the page under a stand-in `Halvo` whose calls the test answers:
 * the real helper serves as `Halvo.confirm`, `agents_snapshot` answers `first`, a call named in
 * `oks` answers `{"ok":"yes"}`, and every other call waits in `window.r4boot.pending`. Each call
 * stands in `window.r4boot.calls`. The harness freezes its own call function, so this is the one
 * way that a test holds and orders the replies of `agents_next`. */
const boot = (h, sources, first, oks) =>
  h.page.evaluate(
    ([srcs, snap, names]) => {
      const r = { calls: [], pending: [], views: [] };
      window.r4boot = r;
      const fake = {
        views: { add: (v) => r.views.push(v) },
        confirm: window.Halvo.confirm,
        ipc: {
          call: (name, args) => {
            r.calls.push({ call: name, args: args });
            if (name === 'agents_snapshot') return Promise.resolve(JSON.parse(JSON.stringify(snap)));
            if (names.includes(name)) return Promise.resolve({ ok: 'yes' });
            return new Promise((resolve, reject) => r.pending.push({ call: name, args: args, resolve: resolve, reject: reject }));
          },
        },
      };
      for (const src of srcs) new Function('Halvo', src)(fake);
      const main = document.getElementById('views');
      r.views
        .sort((a, b) => a.order - b.order)
        .forEach((v) => {
          const section = document.createElement('section');
          const body = document.createElement('div');
          section.appendChild(body);
          main.appendChild(section);
          v.mount(body);
        });
      r.views.forEach((v) => v.show());
    },
    [sources, first, oks],
  );
const answerFirst = (h, call, match, value, how) =>
  h.page.evaluate(
    ([c, m, v, w]) => {
      const p = window.r4boot.pending.find((x) => x.call === c && (m === null || JSON.stringify(x.args) === JSON.stringify(m)));
      if (!p) return false;
      window.r4boot.pending.splice(window.r4boot.pending.indexOf(p), 1);
      if (w === 'reject') p.reject(new Error('rejected'));
      else p.resolve(v);
      return true;
    },
    [call, match, value, how || 'resolve'],
  );
const answerAll = (h, call, value) =>
  h.page.evaluate(
    ([c, v]) => {
      const all = window.r4boot.pending.filter((x) => x.call === c);
      for (const p of all) {
        window.r4boot.pending.splice(window.r4boot.pending.indexOf(p), 1);
        p.resolve(JSON.parse(JSON.stringify(v)));
      }
      return all.length;
    },
    [call, value],
  );
const bootCalls = (h) => h.page.evaluate(() => window.r4boot.calls.filter((c) => c.call !== 'agents_snapshot' && c.call !== 'agents_next').map((c) => ({ call: c.call, args: c.args })));
const nextsOf = (h) => h.page.evaluate(() => window.r4boot.calls.filter((c) => c.call === 'agents_next').map((c) => c.args.after));

const questionOf = (h) =>
  h.page.evaluate(() => {
    const all = [...document.querySelectorAll('[role="alertdialog"]')];
    const d = all.find((x) => x.matches(':modal'));
    if (!d) return { open: false, dialogs: all.length };
    return {
      open: true,
      dialogs: all.length,
      title: d.querySelector('h3') ? d.querySelector('h3').textContent : null,
      text: [...d.querySelectorAll('.hc-text')].map((p) => p.textContent),
      did: [...d.querySelectorAll('.hc-did')].map((p) => p.textContent),
      labels: [...d.querySelectorAll('.hc-label')].map((p) => p.textContent),
      values: [...d.querySelectorAll('.hc-value')].map((p) => p.textContent),
      controls: [...d.querySelectorAll('button')].map((b) => ({ act: b.getAttribute('data-act'), param: b.getAttribute('data-param'), text: b.textContent })),
      focused: document.activeElement === d,
      labelled: Boolean(d.getAttribute('aria-labelledby')) && Boolean(d.getAttribute('aria-describedby')),
    };
  });

async function pressIn(h, name) {
  await h.page.evaluate((n) => {
    const d = [...document.querySelectorAll('[role="alertdialog"]')].find((x) => x.matches(':modal'));
    d.querySelector('[data-act="' + n + '"]').focus();
  }, name);
  await h.page.keyboard.press('Enter');
  await settleCalls(h);
}

function questionWanted(row) {
  return {
    title: row.ask.title,
    text: [...row.ask.text, ...(row.ask.record ? [row.ask.record] : [])],
    did: row.ask.effects.map((e) => e.text),
    controls: row.ask.controls.map((c) => ({ act: c.act, param: row.agent, text: c.text })),
  };
}

const STATUSES = ['needs_you', 'problem', 'working', 'waiting_turn', 'all_set', 'off', 'not_yet'];
const CONTROLS = ['run_now', 'stop', 'take_back', 'switch_off', 'switch_on', 'stop_sign_in', 'delete'];

function outsideTheClosedLists(snapshot) {
  const out = [];
  for (const r of snapshot.rows) {
    if (!STATUSES.includes(r.status)) out.push(r.agent + ':status=' + r.status);
    for (const c of r.controls) if (!CONTROLS.includes(c.control)) out.push(r.agent + ':control=' + c.control);
  }
  return out;
}

if (STATES.length > 0) {
  const faults = [];
  for (const s of STATES) for (const f of outsideTheClosedLists(json('state-' + s + '.json').snapshot)) faults.push(s + ':' + f);
  const ten = json('state-ten.json').snapshot;
  const odd = { ...ten, rows: [{ ...ten.rows[0], status: 'paused' }, { ...ten.rows[1], controls: [...ten.rows[1].controls, { control: 'switch_offf', text: 'x' }] }] };
  const caught = outsideTheClosedLists(odd).length === 2;
  result('point10_every_row_of_every_state_has_a_status_of_the_seven_and_controls_of_the_closed_list', faults.length === 0 && caught, { faults, caught });
}

if (want('ten')) {
  const ten = json('state-ten.json').snapshot;
  const [A, B, C] = ten.rows;

  await test('rule2_a_press_on_delete_sends_nothing_and_opens_the_question_whose_words_come_in_the_rows_reply', async () => {
    const out = outDir();
    return withPage({ origin, state: 'ten', views: [CONFIRM, VIEW], out }, async (h) => {
      await act(h, { act: 'delete', param: A.agent });
      await settleCalls(h);
      const q = await questionOf(h);
      const wantQ = questionWanted(A);
      const got = { title: q.title, text: q.text, did: q.did, controls: q.controls };
      const ok = A.ask.kind === 'delete' && q.open && q.dialogs === 1 && q.focused && q.labelled && same(got, wantQ) && calls(out).length === 0;
      return [ok, { q, wantQ, calls: calls(out) }];
    });
  });

  await test('rule2_the_safe_control_first_closes_the_question_sends_nothing_and_gives_the_focus_back', async () => {
    const out = outDir();
    return withPage({ origin, state: 'ten', views: [CONFIRM, VIEW], out }, async (h) => {
      await press(h, 'delete', A.agent);
      const first = await h.page.evaluate(() => {
        const d = [...document.querySelectorAll('[role="alertdialog"]')].find((x) => x.matches(':modal'));
        return d ? d.querySelector('button').getAttribute('data-act') : null;
      });
      await pressIn(h, 'reject');
      const q = await questionOf(h);
      const focus = await focusOf(h);
      return [first === 'reject' && !q.open && q.dialogs === 0 && calls(out).length === 0 && same(focus, ['delete', A.agent]), { first, q, focus, calls: calls(out) }];
    });
  });

  await test('rule7_the_unsafe_control_alone_sends_agents_delete_once_and_the_view_reads_the_state_anew', async () => {
    const gone = { ...ten, seq: 2, rows: ten.rows.filter((r) => r.agent !== A.agent) };
    return withPage({ origin, state: 'empty', views: [CONFIRM] }, async (h) => {
      await boot(h, [AGENTS_SRC], ten, ['agents_delete']);
      await settleCalls(h);
      await press(h, 'delete', A.agent);
      await h.page.focus('[role="alertdialog"] [data-act="allow"]');
      await h.page.keyboard.press('Enter');
      await h.page.keyboard.press('Enter');
      await settleCalls(h);
      const afters = await nextsOf(h);
      const answered = await answerFirst(h, 'agents_next', { after: 0 }, gone);
      await settleCalls(h);
      const model = await h.page.evaluate(MODEL);
      const sent = await bootCalls(h);
      const ok = same(sent, [{ call: 'agents_delete', args: { agent: A.agent } }]) && same(afters, [ten.seq, 0]) && answered && same(model, expectedModel(gone, null));
      return [ok, { sent, afters, answered }];
    });
  });

  await test('u24y_a_press_on_stop_sign_in_opens_its_question_the_safe_control_sends_nothing_and_the_other_sends_agents_control_once', async () => {
    const question = {
      kind: 'stop_sign_in',
      title: 'Stop using the sign-in of “' + A.name + '”?',
      text: [
        'Halvo will stop using the sign-in that it keeps for this agent, and removes it within 35 days.',
        'The next time the agent runs, it asks you to sign in again.',
      ],
      effects: [],
      record: null,
      controls: [
        { act: 'reject', text: 'Keep the sign-in' },
        { act: 'allow', text: 'Stop using its sign-in' },
      ],
    };
    const at = A.controls.findIndex((c) => c.control === 'delete');
    const control = { control: 'stop_sign_in', text: 'Stop using its sign-in' };
    const kept = { ...A, controls: [...A.controls.slice(0, at), control, ...A.controls.slice(at)], ask_stop_sign_in: question };
    const state = { ...ten, rows: ten.rows.map((r) => (r.agent === A.agent ? kept : r)) };
    const after = { ...ten, seq: ten.seq + 1 };
    return withPage({ origin, state: 'empty', views: [CONFIRM] }, async (h) => {
      await boot(h, [AGENTS_SRC], state, ['agents_control']);
      await settleCalls(h);
      await press(h, 'stop_sign_in', A.agent);
      const q = await questionOf(h);
      const wantQ = { title: question.title, text: question.text, controls: question.controls.map((c) => ({ act: c.act, param: A.agent, text: c.text })) };
      const asked = q.open && q.dialogs === 1 && same({ title: q.title, text: q.text, controls: q.controls }, wantQ);
      const atPress = await bootCalls(h);
      await pressIn(h, 'reject');
      const safe = await questionOf(h);
      const atSafe = await bootCalls(h);
      await press(h, 'stop_sign_in', A.agent);
      await h.page.focus('[role="alertdialog"] [data-act="allow"]');
      await h.page.keyboard.press('Enter');
      await h.page.keyboard.press('Enter');
      await settleCalls(h);
      const afters = await nextsOf(h);
      const answered = await answerFirst(h, 'agents_next', { after: 0 }, after);
      await settleCalls(h);
      const model = await h.page.evaluate(MODEL);
      const sent = await bootCalls(h);
      const ok =
        asked &&
        same(atPress, []) &&
        !safe.open &&
        safe.dialogs === 0 &&
        same(atSafe, []) &&
        same(sent, [callOf(A, 'stop_sign_in')]) &&
        same(afters, [ten.seq, 0]) &&
        answered &&
        same(model, expectedModel(after, null));
      return [ok, { q, wantQ, atPress, safe, atSafe, sent, afters, answered }];
    });
  });

  await test('rule7_a_double_click_on_the_unsafe_control_sends_one_delete', async () => {
    const out = outDir();
    const replies = [{ state: ten }, { call: 'agents_delete', args: { agent: A.agent }, reply: { ok: 'yes' } }];
    return withPage({ origin, replies, views: [CONFIRM, VIEW], out }, async (h) => {
      await press(h, 'delete', A.agent);
      await h.page.waitForTimeout(100);
      await h.page.dblclick('[role="alertdialog"] [data-act="allow"]');
      await settleCalls(h);
      const sent = calls(out).filter((c) => c.call === 'agents_delete').length;
      return [sent <= 1 && h.mismatch.length === 0, { calls: calls(out), mismatch: h.mismatch }];
    });
  });

  await test('rule4_the_question_shows_each_effect_of_unknown_outcome_with_its_kept_fields_above_the_controls_and_the_record_sentence', async () => {
    const effects = [
      {
        text: 'Halvo does not know whether it made this payment.',
        fields: [
          { label: 'Pay to', value: ['City ', 'Power'], from: 'you', from_text: 'You set this' },
          { label: 'Site', value: ['pay.site1.example'], from: 'halvo', from_text: '' },
        ],
      },
    ];
    const row = { ...A, ask: { ...A.ask, effects, record: 'Halvo keeps no record of this once the agent is removed.' } };
    const snap = { ...ten, rows: ten.rows.map((r) => (r.agent === A.agent ? row : r)) };
    return withPage({ origin, replies: [{ state: snap }], views: [CONFIRM, VIEW] }, async (h) => {
      await press(h, 'delete', A.agent);
      const q = await questionOf(h);
      const order = await h.page.evaluate(() => {
        const d = [...document.querySelectorAll('[role="alertdialog"]')].find((x) => x.matches(':modal'));
        const did = d.querySelector('.hc-did');
        const controls = d.querySelector('.hc-controls');
        return Boolean(did && controls && did.compareDocumentPosition(controls) & Node.DOCUMENT_POSITION_FOLLOWING);
      });
      const ok = same(q.did, [effects[0].text]) && same(q.labels, ['Pay to', 'Site']) && same(q.values, ['City Power', 'pay.site1.example']) && q.text[q.text.length - 1] === row.ask.record && order;
      return [ok, { q, order }];
    });
  });

  await test('rule6_a_page_confirmation_that_arrives_while_the_question_is_open_waits_behind_it', async () => {
    const page = json('state-needs_confirm_page.json').snapshot;
    const mixed = { ...page, seq: 2, rows: [...ten.rows.filter((r) => !page.rows.some((p) => p.agent === r.agent)), ...page.rows] };
    return withPage({ origin, state: 'empty', views: [CONFIRM] }, async (h) => {
      await boot(h, [NEEDS_SRC, AGENTS_SRC], ten, []);
      await settleCalls(h);
      await press(h, 'delete', A.agent);
      const delivered = await answerAll(h, 'agents_next', mixed);
      await settleCalls(h);
      const kinds = () =>
        h.page.evaluate(() => [...document.querySelectorAll('[role="alertdialog"]')].map((d) => (d.matches(':modal') ? 'modal:' : 'shut:') + (d.classList.contains('hc-dialog') ? 'question' : 'item')));
      const during = await kinds();
      await pressIn(h, 'reject');
      await settleCalls(h);
      const after = (await kinds()).filter((d) => d.startsWith('modal:'));
      const ok = delivered === 2 && same(during, ['shut:item', 'modal:question']) && same(after, ['modal:item']);
      return [ok, { delivered, during, after }];
    });
  });

  await test('rule6_a_page_confirmation_that_left_while_it_waited_behind_the_question_is_not_said_and_the_needs_view_adds_again', async () => {
    const page = json('state-needs_confirm_page.json').snapshot;
    const stuck = json('state-needs_stuck.json').snapshot;
    const join = (snap, seq) => ({ ...snap, seq, rows: [...ten.rows.filter((r) => !snap.rows.some((p) => p.agent === r.agent)), ...snap.rows] });
    return withPage({ origin, state: 'empty', views: [CONFIRM] }, async (h) => {
      await boot(h, [NEEDS_SRC, AGENTS_SRC], ten, []);
      await settleCalls(h);
      await press(h, 'delete', A.agent);
      await answerAll(h, 'agents_next', join(page, 2));
      await settleCalls(h);
      await answerAll(h, 'agents_next', join(stuck, 3));
      await settleCalls(h);
      await pressIn(h, 'reject');
      await settleCalls(h);
      const said = await h.page.evaluate(() => {
        const s = document.querySelector('.hn-say');
        return s ? [...s.children].map((p) => p.textContent) : null;
      });
      const want = [stuck.items[0].say_new];
      return [same(said, want), { said, want }];
    });
  });

  await test('rg1_after_every_reply_of_agents_control_whatever_it_holds_the_view_reads_the_state_anew', async () =>
    withPage({ origin, state: 'empty', views: [CONFIRM] }, async (h) => {
      await boot(h, [AGENTS_SRC], ten, []);
      await settleCalls(h);
      await act(h, { act: 'run_now', param: A.agent });
      await answerFirst(h, 'agents_control', null, { error: 'busy' });
      await settleCalls(h);
      const once = await nextsOf(h);
      await act(h, { act: 'run_now', param: A.agent });
      await answerFirst(h, 'agents_control', null, null, 'reject');
      await settleCalls(h);
      const twice = await nextsOf(h);
      return [same(once, [ten.seq, 0]) && same(twice, [ten.seq, 0, 0]), { once, twice }];
    }),
  );

  for (const order of ['new_first', 'old_first'])
    await test('a_reply_of_the_loop_before_a_control_reply_is_never_drawn_over_the_newer_state_' + order, async () =>
      withPage({ origin, state: 'empty', views: [CONFIRM] }, async (h) => {
        await boot(h, [AGENTS_SRC], ten, ['agents_control']);
        await settleCalls(h);
        await act(h, { act: 'run_now', param: A.agent });
        await settleCalls(h);
        const afters = await nextsOf(h);
        const newer = { ...ten, seq: 7, rows: ten.rows.filter((r) => r.agent !== B.agent) };
        const older = { ...ten, seq: 5, rows: ten.rows.filter((r) => r.agent !== C.agent) };
        if (order === 'new_first') {
          await answerFirst(h, 'agents_next', { after: 0 }, newer);
          await settleCalls(h);
          await answerFirst(h, 'agents_next', { after: ten.seq }, older);
        } else {
          await answerFirst(h, 'agents_next', { after: ten.seq }, older);
          await settleCalls(h);
          await answerFirst(h, 'agents_next', { after: 0 }, newer);
        }
        await settleCalls(h);
        const model = await h.page.evaluate(MODEL);
        const left = await nextsOf(h);
        return [same(afters, [ten.seq, 0]) && same(model, expectedModel(newer, null)) && left[left.length - 1] === 7, { afters, left }];
      }),
    );

  await test('rd74_a_reply_with_a_lower_seq_is_taken_by_the_transcript', async () => {
    const high = { ...ten, seq: 9 };
    const low = { ...ten, seq: 4, rows: ten.rows.filter((r) => r.agent !== A.agent) };
    const later = { ...low, seq: 5, rows: low.rows.filter((r) => r.agent !== B.agent) };
    return withPage({ origin, replies: [{ state: high }, { state: low }, { state: later }], views: [CONFIRM, VIEW] }, async (h) => {
      await h.page.waitForFunction((b) => !document.querySelector('[data-param="' + b + '"]'), B.agent, { timeout: 5000 }).catch(() => {});
      await settleCalls(h);
      const model = await h.page.evaluate(MODEL);
      return [same(model, expectedModel(later, null)), {}];
    });
  });

  await test('rd74_the_next_call_after_a_lower_seq_carries_that_seq', async () => {
    const high = { ...ten, seq: 9 };
    const low = { ...ten, seq: 4, rows: ten.rows.filter((r) => r.agent !== A.agent) };
    return withPage({ origin, state: 'empty', views: [CONFIRM] }, async (h) => {
      await boot(h, [AGENTS_SRC], high, []);
      await settleCalls(h);
      await answerFirst(h, 'agents_next', { after: 9 }, low);
      await settleCalls(h);
      const afters = await nextsOf(h);
      const model = await h.page.evaluate(MODEL);
      return [same(afters, [9, 4]) && same(model, expectedModel(low, null)), { afters }];
    });
  });

  await test('rd74_a_reply_with_seq_0_and_the_note_is_taken_and_then_a_state', async () => {
    const stand = { seq: 0, rows: [], items: [], note: 'Halvo cannot reach its service right now.' };
    const back = { ...ten, seq: 3 };
    return withPage({ origin, replies: [{ state: ten }, { state: stand }, { state: back }], views: [CONFIRM, VIEW] }, async (h) => {
      await settleCalls(h);
      const model = await h.page.evaluate(MODEL);
      return [same(model, expectedModel(back, null)), {}];
    });
  });

  await test('rd74_a_reply_with_seq_0_and_the_note_alone_shows_the_note_and_no_row', async () => {
    const stand = { seq: 0, rows: [], items: [], note: 'Halvo cannot reach its service right now.' };
    return withPage({ origin, replies: [{ state: ten }, { state: stand }], views: [CONFIRM, VIEW] }, async (h) => {
      await settleCalls(h);
      const model = await h.page.evaluate(MODEL);
      return [same(model, expectedModel(stand, null)), { model }];
    });
  });

  await test('rd74_a_reply_after_next_wait_carries_the_views_own_seq_and_is_not_taken', async () => {
    const same0 = { seq: ten.seq, rows: [], items: [], note: null };
    return withPage({ origin, replies: [{ state: ten }, { state: same0 }], views: [CONFIRM, VIEW] }, async (h) => {
      await settleCalls(h);
      const model = await h.page.evaluate(MODEL);
      return [same(model, expectedModel(ten, null)), {}];
    });
  });

  await test('rd77_a_change_of_status_whose_say_is_null_adds_nothing_to_the_status_element', async () => {
    const second = { ...ten, seq: 2, rows: ten.rows.map((r) => (r.agent === A.agent ? { ...r, status: 'off', word: 'Off', say: null } : r)) };
    return withPage({ origin, replies: [{ state: ten }, { state: second }], views: [CONFIRM, VIEW] }, async (h) => {
      await settleCalls(h);
      const seen = await h.page.evaluate(() => {
        const s = document.querySelector('[role="status"]');
        return { nodes: s.childNodes.length, text: s.textContent };
      });
      return [same(seen, { nodes: 0, text: '' }), seen];
    });
  });

  await test('rd76_r4_no_sentence_is_added_while_a_confirmation_is_open_and_it_comes_once_it_has_closed', async () => {
    const second = { ...ten, seq: 2, rows: ten.rows.map((r) => (r.agent === B.agent ? { ...r, status: 'off', word: 'Off', say: 'It is off.' } : r)) };
    return withPage({ origin, state: 'empty', views: [CONFIRM] }, async (h) => {
      await boot(h, [AGENTS_SRC], ten, []);
      await settleCalls(h);
      await press(h, 'delete', A.agent);
      await answerAll(h, 'agents_next', second);
      await settleCalls(h);
      const statusText = () => h.page.evaluate(() => document.querySelector('.r4a [role="status"]').textContent);
      const during = await statusText();
      await pressIn(h, 'reject');
      await settleCalls(h);
      const after = await statusText();
      return [during === '' && after === 'It is off.', { during, after }];
    });
  });

  await test('rg4_a_card_effect_with_empty_fields_draws_its_sentence', async () => {
    const card = json('state-card.json');
    const bare = JSON.parse(JSON.stringify(card.card));
    bare.runs[0].effects[0].fields = [];
    const agentA = card.card.agent;
    const replies = [{ state: card.snapshot }, { call: 'agents_card', args: { agent: agentA }, reply: bare }];
    return withPage({ origin, replies, views: [CONFIRM, VIEW] }, async (h) => {
      await act(h, { act: 'open_card', param: agentA });
      await settleCalls(h);
      const did = await h.page.evaluate(() => [...document.querySelectorAll('.r4a-did')].map((p) => p.textContent));
      return [did.length > 0 && did[0] === bare.runs[0].effects[0].text && h.errors === 0, { did }];
    });
  });

  await test('rd54g_the_row_of_a_gone_workflow_draws_its_line_and_no_control', async () => {
    const row = { ...A, status: 'working', word: 'Working', line: 'It is stopping.', controls: [], ask: null };
    const snap = { ...ten, seq: 2, rows: ten.rows.map((r) => (r.agent === A.agent ? row : r)) };
    return withPage({ origin, replies: [{ state: snap }], views: [CONFIRM, VIEW] }, async (h) => {
      const model = await h.page.evaluate(MODEL);
      const acts = await h.page.evaluate((a) => [...document.querySelectorAll('[data-param="' + a + '"]')].map((b) => b.getAttribute('data-act')), A.agent);
      return [same(model, expectedModel(snap, null)) && same(acts, ['open_card']), { acts }];
    });
  });
}

if (want('working')) {
  const working = json('state-working.json').snapshot;
  const live = working.rows.find((r) => r.ask && r.ask.kind === 'working');

  await test('rule3_with_a_live_run_the_question_is_the_notice_with_the_safe_control_first_and_the_rows_stop', async () =>
    withPage({ origin, state: 'empty', views: [CONFIRM] }, async (h) => {
      await boot(h, [AGENTS_SRC], working, ['agents_control']);
      await settleCalls(h);
      await press(h, 'delete', live.agent);
      const q = await questionOf(h);
      const got = { title: q.title, text: q.text, did: q.did, controls: q.controls };
      await pressIn(h, 'stop');
      const sent = await bootCalls(h);
      const afters = await nextsOf(h);
      const ok = same(got, questionWanted(live)) && same(q.controls.map((c) => c.act), ['reject', 'stop']) && same(sent, [callOf(live, 'stop')]) && same(afters, [working.seq, 0]);
      return [ok, { q, sent, afters }];
    }),
  );

  await test('rule3_while_the_run_stops_the_notice_has_one_control_that_sends_nothing', async () => {
    const out = outDir();
    const stopping = {
      ...live,
      ask: { kind: 'stopping', title: 'It is stopping', text: ['You can remove it once it has stopped.'], effects: [], record: null, controls: [{ act: 'done', text: 'OK' }] },
    };
    const snap = { ...working, rows: working.rows.map((r) => (r.agent === live.agent ? stopping : r)) };
    return withPage({ origin, replies: [{ state: snap }], views: [CONFIRM, VIEW], out }, async (h) => {
      await press(h, 'delete', live.agent);
      const q = await questionOf(h);
      await pressIn(h, 'done');
      const after = await questionOf(h);
      return [same(q.controls.map((c) => c.act), ['done']) && !after.open && calls(out).length === 0, { q, after, calls: calls(out) }];
    });
  });
}

if (want('queue')) {
  const queue = json('state-queue.json').snapshot;
  const waiting = queue.rows.find((r) => r.controls.some((c) => c.control === 'take_back'));

  await test('u24e_the_row_draws_cancel_this_start_from_its_controls_and_reads_the_state_again_after_an_answer_ok', async () => {
    const back = { ...queue, seq: 2, rows: queue.rows.map((r) => (r.agent === waiting.agent ? { ...r, controls: r.controls.filter((c) => c.control !== 'take_back') } : r)) };
    return withPage({ origin, state: 'empty', views: [CONFIRM] }, async (h) => {
      await boot(h, [AGENTS_SRC], queue, ['agents_control']);
      await settleCalls(h);
      const text = await h.page.evaluate((s) => (document.querySelector(s) ? document.querySelector(s).textContent : null), sel('take_back', waiting.agent));
      await act(h, { act: 'take_back', param: waiting.agent });
      await settleCalls(h);
      const afters = await nextsOf(h);
      await answerFirst(h, 'agents_next', { after: 0 }, back);
      await settleCalls(h);
      const model = await h.page.evaluate(MODEL);
      const sent = await bootCalls(h);
      const wantText = waiting.controls.find((c) => c.control === 'take_back').text;
      const ok = text === wantText && same(sent, [callOf(waiting, 'take_back')]) && same(afters, [queue.seq, 0]) && same(model, expectedModel(back, null));
      return [ok, { text, sent, afters }];
    });
  });

}

if (want('card')) {
  const { trail } = await import(pathToFileURL(join(REPO, 'tools', 'r4-page', 'index.mjs')).href);
  const state = json('state-card.json');
  const A = state.card.agent;
  const snap = state.snapshot;
  const B = snap.rows.find((r) => r.agent !== A).agent;
  const row = snap.rows.find((r) => r.agent === A);
  const tries = /^ {2}var CARD_TRIES = (\d+);$/m.exec(readFileSync(join(REPO, 'ui', 'app', 'views', 'agents.js'), 'utf8'));
  const N = tries ? Number(tries[1]) : 0;
  const count = { key: 'records_dropped', text: 'One record went unsaved.' };
  const words = json('words.json');
  const unread = { text: words['card.history_busy'], again: words['card.read_again'] };
  const busyCard = { agent: A, name: state.card.name, lines: [count], runs: [], history: 'busy', unread };
  const same0 = { state: { ...snap } };
  const cardCall = (reply) => ({ call: 'agents_card', args: { agent: A }, reply });
  const syncLine = { call: 'agents_control', args: { agent: B, action: 'run_now' }, reply: { ok: 'yes' } };
  const sync = async (h) => {
    await h.page.evaluate((b) => window.Halvo.ipc.call('agents_control', { agent: b, action: 'run_now' }), B);
    await settleCalls(h);
    await settleCalls(h);
  };
  const strays = (h) => trail(h).filter((l) => l.includes('not_next_in_transcript')).length;
  const cardsAsked = (out) => calls(out).filter((c) => c.call === 'agents_card').length;
  const cardOf = (h) =>
    h.page.evaluate((a) => {
      const c = document.getElementById('r4a-card-' + a);
      if (c === null) return null;
      return {
        runs: c.querySelectorAll('.r4a-runs > li').length,
        texts: [...c.querySelectorAll('*')].filter((e) => e.children.length === 0 && e.textContent !== '').map((e) => e.textContent),
      };
    }, A);
  const busyTexts = [count.text].concat(typeof row.next === 'string' && row.next !== '' ? [row.next] : []);
  const unreadTexts = busyTexts.concat([unread.text, unread.again]);

  await test('a_busy_card_is_asked_again_at_the_next_reply_and_shows_its_runs_without_a_close', async () => {
    const out = outDir();
    const replies = [{ state: snap }, cardCall(busyCard), syncLine, same0, cardCall(state.card), syncLine, same0];
    return withPage({ origin, replies, views: [CONFIRM, VIEW], out }, async (h) => {
      await act(h, { act: 'open_card', param: A });
      await settleCalls(h);
      await sync(h);
      const shown = await cardOf(h);
      await sync(h);
      const asked = cardsAsked(out);
      return [N >= 1 && shown !== null && shown.runs === state.card.runs.length && asked === 2 && strays(h) === 0, { N, shown, asked, trail: trail(h) }];
    });
  });

  await test('a_busy_card_whose_next_ask_is_refused_is_not_asked_again_and_keeps_its_count_line', async () => {
    const out = outDir();
    const replies = [{ state: snap }, cardCall(busyCard), syncLine, same0, cardCall({ error: 'no_service' }), syncLine, same0];
    return withPage({ origin, replies, views: [CONFIRM, VIEW], out }, async (h) => {
      await act(h, { act: 'open_card', param: A });
      await settleCalls(h);
      await sync(h);
      await sync(h);
      const shown = await cardOf(h);
      const asked = cardsAsked(out);
      return [N >= 2 && asked === 2 && strays(h) === 0 && shown !== null && same(shown.texts, unreadTexts), { N, asked, shown, trail: trail(h) }];
    });
  });

  await test('a_busy_card_asked_no_more_after_CARD_TRIES_says_so_and_its_control_reads_again', async () => {
    const out = outDir();
    const replies = [{ state: snap }, cardCall(busyCard)];
    for (let k = 1; k <= N; k++) replies.push(syncLine, same0, cardCall(busyCard));
    replies.push(syncLine, same0, cardCall(state.card));
    return withPage({ origin, replies, views: [CONFIRM, VIEW], out }, async (h) => {
      await act(h, { act: 'open_card', param: A });
      await settleCalls(h);
      for (let k = 0; k <= N; k++) await sync(h);
      const shown = await cardOf(h);
      const asked = cardsAsked(out);
      await act(h, { act: 'read_card', param: A });
      await settleCalls(h);
      const read = await cardOf(h);
      const ok =
        N >= 1 && asked === N + 1 && shown !== null && shown.runs === 0 && same(shown.texts, unreadTexts) &&
        cardsAsked(out) === N + 2 && read !== null && read.runs === state.card.runs.length && strays(h) === 0;
      return [ok, { N, asked, shown, unreadTexts, read, trail: trail(h) }];
    });
  });

  await test('no_card_is_asked_for_behind_a_busy_card_that_was_closed', async () => {
    const out = outDir();
    const replies = [{ state: snap }, cardCall(busyCard), syncLine, same0];
    return withPage({ origin, replies, views: [CONFIRM, VIEW], out }, async (h) => {
      await act(h, { act: 'open_card', param: A });
      await settleCalls(h);
      await act(h, { act: 'close_card', param: A });
      await settleCalls(h);
      await sync(h);
      const asked = cardsAsked(out);
      const cards = await cardsOf(h);
      return [asked === 1 && strays(h) === 0 && same(cards, []), { asked, cards, trail: trail(h) }];
    });
  });

  await test('only_the_busy_key_is_asked_again_and_a_busy_refusal_or_a_card_that_answered_is_not', async () => {
    const seen = [];
    for (const first of [{ error: 'busy' }, state.card]) {
      const out = outDir();
      const replies = [{ state: snap }, cardCall(first), syncLine, same0];
      const one = await withPage({ origin, replies, views: [CONFIRM, VIEW], out }, async (h) => {
        await act(h, { act: 'open_card', param: A });
        await settleCalls(h);
        await sync(h);
        return { asked: cardsAsked(out), strays: strays(h), cards: await cardsOf(h) };
      });
      seen.push(one);
    }
    const ok = same(seen, [
      { asked: 1, strays: 0, cards: [] },
      { asked: 1, strays: 0, cards: ['r4a-card-' + A] },
    ]);
    return [ok, seen];
  });

  await test('a_busy_card_shows_the_count_line_and_no_words_of_a_card_with_no_history', async () => {
    const replies = [{ state: snap }, cardCall(busyCard)];
    return withPage({ origin, replies, views: [CONFIRM, VIEW] }, async (h) => {
      await act(h, { act: 'open_card', param: A });
      await settleCalls(h);
      const shown = await cardOf(h);
      return [shown !== null && shown.runs === 0 && same(shown.texts, busyTexts) && h.errors === 0, { shown, busyTexts }];
    });
  });

  await test('a_stop_pressed_while_the_re_ask_of_a_busy_card_is_in_flight_is_sent', async () => {
    const out = outDir();
    const work = { ...snap, rows: snap.rows.map((r) => (r.agent === A ? { ...r, controls: [{ control: 'stop', text: 'Stop' }] } : r)) };
    const stopA = { call: 'agents_control', args: { agent: A, action: 'stop' }, reply: { ok: 'yes' } };
    const replies = [{ state: work }, cardCall(busyCard), syncLine, { state: { ...work } }, { call: 'agents_card', args: { agent: A }, hold: true }, stopA];
    return withPage({ origin, replies, views: [CONFIRM, VIEW], out }, async (h) => {
      await act(h, { act: 'open_card', param: A });
      await settleCalls(h);
      await sync(h);
      const reAsked = cardsAsked(out);
      await act(h, { act: 'stop', param: A }).catch(() => {});
      await settleCalls(h);
      const stops = calls(out).filter((c) => c.call === 'agents_control' && c.args.agent === A && c.args.action === 'stop').length;
      return [reAsked === 2 && stops === 1 && strays(h) === 0, { reAsked, stops, calls: calls(out), trail: trail(h) }];
    });
  });

  await test('a_card_that_the_person_opens_while_a_busy_card_is_asked_again_is_the_one_shown', async () => {
    const out = outDir();
    const cardB = { agent: B, name: 'Card of B', lines: [{ key: 'x', text: 'A line of B' }], runs: [] };
    const replies = [{ state: snap }, cardCall(busyCard), { call: 'agents_card', args: { agent: B }, reply: cardB }, same0, cardCall(busyCard)];
    return withPage({ origin, replies, views: [CONFIRM, VIEW], out }, async (h) => {
      await act(h, { act: 'open_card', param: A });
      await settleCalls(h);
      await act(h, { act: 'open_card', param: B });
      await settleCalls(h);
      await settleCalls(h);
      const cards = await cardsOf(h);
      return [same(cards, ['r4a-card-' + B]) && strays(h) === 0, { cards, calls: calls(out), trail: trail(h) }];
    });
  });
}

if (want('ten')) {
  const ten = json('state-ten.json').snapshot;
  const [A, , C] = ten.rows;
  const words = json('words.json');
  const CAUSES = readdirSync(FIXTURES)
    .map((f) => /^reply-error-(.+)\.json$/.exec(f))
    .filter(Boolean)
    .map((m) => m[1])
    .sort();
  const refused = (cause) => ({ ...callOf(A, 'run_now'), reply: json('reply-error-' + cause + '.json') });
  const answerOf = (h, control) =>
    h.page.evaluate(
      ([a, c]) => {
        const b = document.querySelector('[data-act="' + c + '"][data-param="' + a + '"]');
        const p = b ? b.closest('li').querySelector('.r4a-answer') : null;
        return {
          text: p ? p.textContent : null,
          beside: Boolean(p && p.previousElementSibling === b.parentElement),
          described: Boolean(p && (b.getAttribute('aria-describedby') || '').split(' ').includes(p.id)),
          said: [...document.querySelectorAll('[role="status"]')].map((s) => [...s.children].map((x) => x.textContent)),
        };
      },
      [A.agent, control],
    );

  await test('a_refused_press_shows_the_sentence_of_each_cause_beside_the_control_and_the_status_region_says_it', async () => {
    const seen = [];
    await withPage({ origin, replies: [{ state: ten }, ...CAUSES.map(refused)], views: [CONFIRM, VIEW] }, async (h) => {
      for (const cause of CAUSES) {
        await press(h, 'run_now', A.agent);
        seen.push({ cause, ...(await answerOf(h, 'run_now')) });
      }
    });
    const ok =
      CAUSES.length === 13 &&
      seen.every((s) => typeof words['refused.' + s.cause] === 'string' && s.text === words['refused.' + s.cause] && s.beside && s.described && same(s.said, [[s.text]]));
    return [ok, seen];
  });

  await test('a_refused_press_answer_stands_through_a_state_that_keeps_its_row_and_goes_at_each_end', async () => {
    const line = [words['line.all_set.done'], words['line.all_set.new']].find((l) => typeof l === 'string' && l !== A.line);
    const changed = { ...ten, seq: 4, rows: ten.rows.map((r) => (r.agent === A.agent ? { ...r, line } : r)) };
    const syncCall = { ...callOf(C, 'run_now'), reply: { ok: 'yes' } };
    const replies = [
      { state: ten },
      refused('busy'),
      syncCall,
      { state: { ...ten, seq: 2 } },
      { ...callOf(A, 'switch_off'), reply: { ok: 'yes' } },
      refused('busy'),
      syncCall,
      { state: { ...ten, seq: 3 } },
      syncCall,
      { state: changed },
      refused('busy'),
    ];
    const sync = async (h) => {
      await h.page.evaluate((c) => window.Halvo.ipc.call('agents_control', { agent: c, action: 'run_now' }), C.agent);
      await settleCalls(h);
      await settleCalls(h);
    };
    const seen = {};
    await withPage({ origin, replies, views: [CONFIRM, VIEW] }, async (h) => {
      await press(h, 'run_now', A.agent);
      await sync(h);
      seen.kept = await answerOf(h, 'run_now');
      await press(h, 'switch_off', A.agent);
      seen.pressed = await answerOf(h, 'run_now');
      await press(h, 'run_now', A.agent);
      seen.before_line = await answerOf(h, 'run_now');
      await sync(h);
      seen.after_reread = await answerOf(h, 'run_now');
      await sync(h);
      seen.line = await answerOf(h, 'run_now');
      await press(h, 'run_now', A.agent);
      seen.before_drawn = await answerOf(h, 'run_now');
      await h.page.evaluate(() => window.Halvo.views.window('hidden'));
      await h.page.evaluate(() => window.Halvo.views.window('shown'));
      await settleCalls(h);
      seen.drawn = await answerOf(h, 'run_now');
    });
    const sentence = words['refused.busy'];
    const stands = (s) => typeof sentence === 'string' && s.text === sentence && s.beside && same(s.said, [[sentence]]);
    const gone = (s) => s.text === null && same(s.said, [[]]);
    const ok =
      typeof line === 'string' && stands(seen.kept) && gone(seen.pressed) && stands(seen.before_line) && stands(seen.after_reread) && gone(seen.line) && stands(seen.before_drawn) && gone(seen.drawn);
    return [ok, seen];
  });
}

console.log('agents tests passed=' + passed + ' failed=' + failed + ' states=' + STATES.length);
process.exit(failed === 0 ? 0 : 1);
