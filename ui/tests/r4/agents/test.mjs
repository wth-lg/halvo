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
const ORDER = ['needs_you', 'problem', 'working', 'waiting_turn', 'all_set', 'off', 'not_yet'];
const ACTING = ['run_now', 'stop', 'switch_off', 'switch_on'];

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
          controls: r.controls.filter((c) => ACTING.includes(c.control)).map((c) => ({ act: c.control, param: r.agent, text: c.text })),
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
    withPage({ origin, state: s, views: [VIEW], acts, out }, async (h) => {
      deletes.inReplies += state.snapshot.rows.filter((r) => r.controls.some((c) => c.control === 'delete')).length;
      deletes.shown += await h.page.evaluate(() => document.querySelectorAll('[data-act="delete"]').length);
      const got = await h.page.evaluate(MODEL);
      return [same(got, expectedModel(state.snapshot, state.card)), { got }];
    }),
  );
  result('card_call_' + s, same(calls(out), acts.map((a) => ({ call: 'agents_card', args: { agent: a.param } }))), calls(out));
  const views = pageConfirmFirst(state.snapshot) ? [NEEDS, VIEW] : [VIEW];
  await harness(s + (views.length > 1 ? '_with_both_views' : ''), { origin, state: s, views, acts }, join(STRUCTURE, s + '.txt'));
}
if (STATES.length > 0) {
  result('a_row_shows_no_element_for_delete', deletes.inReplies > 0 && deletes.shown === 0, deletes);
}

for (const s of STATES) {
  const state = json('state-' + s + '.json');
  const out = outDir();
  const wanted = [];
  await test('actions_' + s, async () => {
    await withPage({ origin, state: s, views: [VIEW], out }, async (h) => {
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
    withPage({ origin, state: 'card', views: [VIEW], acts: cardActs(state) }, async (h) => {
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
    return withPage({ origin, replies, views: [VIEW] }, async (h) => {
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
    withPage({ origin, state: 'card', views: [VIEW] }, async (h) => {
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
    return withPage({ origin, replies, views: [VIEW] }, async (h) => {
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
    return withPage({ origin, state: 'card', views: [VIEW], acts: cardActs(state), out }, async (h) => {
      await press(h, 'close_card', A);
      const closed = await h.page.evaluate(MODEL);
      const focus = await focusOf(h);
      const ok = same(closed, expectedModel(state.snapshot, null)) && calls(out).length === 1 && same(focus, ['open_card', A]);
      return [ok, { calls: calls(out), focus }];
    });
  });

  await test('the_focus_goes_back_to_the_toggle_after_a_card_opens', async () =>
    withPage({ origin, state: 'card', views: [VIEW] }, async (h) => {
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
    return withPage({ origin, replies, views: [VIEW] }, async (h) => {
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
    return withPage({ origin, replies, views: [VIEW], out }, async (h) => {
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
    return withPage({ origin, replies, views: [VIEW], out }, async (h) => {
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
    return withPage({ origin, state: 'card', views: [VIEW], out }, async (h) => {
      await h.page.dblclick(sel('open_card', A));
      await settleCalls(h);
      const cards = await cardsOf(h);
      return [same(calls(out), [{ call: 'agents_card', args: { agent: A } }]) && same(cards, ['r4a-card-' + A]), { got: calls(out), cards }];
    });
  });

  await test('a_second_click_on_a_name_after_its_card_opened_keeps_it_open', async () => {
    const out = outDir();
    return withPage({ origin, state: 'card', views: [VIEW], out }, async (h) => {
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
    return withPage({ origin, state: 'card', views: [VIEW], out }, async (h) => {
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
      await withPage({ origin, state: s, views: [VIEW], out }, async (h) => {
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
    await withPage({ origin, state: 'ten', views: [VIEW], out }, async (h) => {
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
    await withPage({ origin, replies, views: [VIEW], out }, async (h) => {
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
    await withPage({ origin, replies, views: [VIEW], out }, async (h) => {
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
    await withPage({ origin, replies, views: [VIEW], out }, async (h) => {
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
    await withPage({ origin, replies, views: [VIEW], out }, async (h) => {
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
    return withPage({ origin, replies, views: [VIEW], out }, async (h) => {
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

  const engine = await withPage({ origin, state: 'empty', views: [VIEW] }, async (h) => h.page.context().browser().browserType().name());
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
    return withPage({ origin, replies, views: [VIEW] }, async (h) => {
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

  await test('each_change_adds_its_say_to_the_status_region', async () => {
    const flip = (snap, seq, row, say) => ({ ...snap, seq, rows: snap.rows.map((r) => (r.agent === row.agent ? { ...r, status: 'off', word: 'Off', say } : r)) });
    const second = flip(ten, 2, A, 'First change.');
    const third = flip(second, 3, B, 'Second change.');
    return withPage({ origin, replies: [{ state: ten }, { state: second }, { state: third }], views: [VIEW] }, async (h) => {
      const says = await h.page.evaluate(() => [...document.querySelector('[role="status"]').children].map((p) => p.textContent));
      return [same(says, ['First change.', 'Second change.']), says];
    });
  });

  await test('a_hidden_view_calls_nothing_and_a_shown_one_reads_again', async () =>
    withPage({ origin, state: 'ten', views: [VIEW] }, async (h) => {
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
    withPage({ origin, state: 'ten', views: [VIEW] }, async (h) => {
      const before = h.started;
      await h.page.evaluate(() => window.Halvo.views.window('shown'));
      await h.page.waitForTimeout(300);
      return [h.started === before, { before, after: h.started }];
    }),
  );

  await test('a_closed_reply_ends_the_calls', async () =>
    withPage({ origin, replies: [{ state: ten }, { state: { closed: 'yes' } }], views: [VIEW] }, async (h) => {
      await h.page.waitForTimeout(500);
      return [h.started === 2, { started: h.started }];
    }),
  );

  await test('a_waiting_call_that_answers_without_a_state_ends_until_the_next_show', async () =>
    withPage({ origin, replies: [{ state: ten }, { state: { error: 'busy' } }], views: [VIEW] }, async (h) => {
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
    return withPage({ origin, replies: [{ state: ten }, { state: odd }], views: [VIEW] }, async (h) => {
      const seen = await h.page.evaluate(([a, b]) => ({
        says: document.querySelector('[role="status"]').textContent,
        rowA: document.querySelectorAll('[data-param="' + a + '"]').length,
        actsB: [...document.querySelectorAll('[data-param="' + b + '"]')].map((x) => x.getAttribute('data-act')),
      }), [A.agent, B.agent]);
      const wantB = ['open_card', ...B.controls.filter((c) => ACTING.includes(c.control)).map((c) => c.control)];
      return [seen.says === '' && seen.rowA === 0 && same(seen.actsB, wantB), seen];
    });
  });

  await test('each_control_is_described_by_its_agents_name', async () =>
    withPage({ origin, state: 'ten', views: [VIEW] }, async (h) => {
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
    return withPage({ origin, replies, views: [VIEW], out }, async (h) => {
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
    return withPage({ origin, replies, views: [VIEW], out }, async (h) => {
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
    return withPage({ origin, state: 'ten', views: [VIEW], out }, async (h) => {
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
}

if (want('card')) {
  const state = json('state-card.json');
  const A = state.card.agent;

  await test('a_click_with_no_press_after_the_walk_key_acts_once_on_a_name', async () => {
    const out = outDir();
    const replies = [{ state: state.snapshot }, { call: 'agents_card', args: { agent: A }, reply: state.card }, { call: 'agents_card', args: { agent: A }, reply: state.card }];
    return withPage({ origin, replies, views: [VIEW], out }, async (h) => {
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
    return withPage({ origin, replies, views: [VIEW] }, async (h) => {
      await act(h, { act: 'open_card', param: A });
      await settleCalls(h);
      const cards = await cardsOf(h);
      const model = await h.page.evaluate(MODEL);
      return [same(cards, []) && same(model, expectedModel(state.snapshot, null)) && h.errors === 0, { cards, errors: h.errors }];
    });
  });
}

console.log('agents tests passed=' + passed + ' failed=' + failed + ' states=' + STATES.length);
process.exit(failed === 0 ? 0 : 1);
