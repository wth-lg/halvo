import { existsSync, mkdtempSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import vm from 'node:vm';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '..', '..', '..', '..');
const FIXTURES = join(REPO, 'ui', 'tests', 'fixtures', 'coord');
const CONFIRM = '/app/views/confirm.js';
const NEEDS = '/app/views/needs.js';
const AGENTS = '/app/views/agents.js';
const DENIED = [
  'setTimeout',
  'setInterval',
  'requestAnimationFrame',
  'requestIdleCallback',
  'localStorage',
  'sessionStorage',
  'indexedDB',
  'fetch',
  'XMLHttpRequest',
  'WebSocket',
  'EventSource',
  'sendBeacon',
  'innerHTML',
  'outerHTML',
  'insertAdjacentHTML',
  'setHTMLUnsafe',
  'parseHTMLUnsafe',
  'createContextualFragment',
  'DOMParser',
  'document.write',
  'attachShadow',
  'Date',
  'performance.now',
  'document.cookie',
  'Worker',
  'eval',
];

const { serve, open, settled } = await import(pathToFileURL(join(REPO, 'tools', 'r4-page', 'index.mjs')).href);

if (process.argv.length > 2) {
  process.stderr.write('REFUSED usage: node ui/tests/r4/confirm/test.mjs\n');
  process.exit(2);
}

let failed = 0;
let passed = 0;
function result(name, ok, detail) {
  if (ok) passed++;
  else failed++;
  console.log('confirm test=' + name + ' ' + (ok ? 'ok' : 'fail') + (ok || !detail ? '' : ' detail=' + JSON.stringify(detail)));
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

const json = (name) => JSON.parse(readFileSync(join(FIXTURES, name), 'utf8'));
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
const frames = (h) => h.page.evaluate(() => new Promise((d) => requestAnimationFrame(() => requestAnimationFrame(d))));
const settle = async (h) => {
  await settled(h);
  await frames(h);
};
function outDir() {
  return mkdtempSync(join(tmpdir(), 'r4-confirm-test-'));
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
const modal = (h) =>
  h.page.evaluate(() => {
    const d = [...document.querySelectorAll('[role="alertdialog"]')].find((x) => x.matches(':modal'));
    return d ? { focused: document.activeElement === d, inQuestion: Boolean(d.closest('.r4a')), controls: [...d.querySelectorAll('button')].map((b) => b.getAttribute('data-act')) } : null;
  });

function lint(file) {
  const src = readFileSync(join(REPO, 'ui', 'app', 'views', file), 'utf8');
  const faults = [];
  for (const name of DENIED) {
    const re = new RegExp('(^|[^A-Za-z0-9_$.])' + name.replace('.', '\\s*\\.\\s*') + '($|[^A-Za-z0-9_$])');
    if (re.test(src)) faults.push('denied=' + name);
  }
  if (/__playwright|__r4/.test(src)) faults.push('denied=prefix');
  if (src.includes('`')) faults.push('template');
  if (/^\s*(import|export)\b/m.test(src)) faults.push('module');
  if (/r4a-|hn-/.test(src) && file === 'confirm.js') faults.push('prefix_of_a_view');
  try {
    new vm.Script(src, { filename: file });
  } catch {
    faults.push('syntax');
  }
  return faults;
}

for (const file of ['confirm.js', 'agents.js', 'needs.js']) {
  const faults = lint(file);
  result('lint_' + file.replace('.js', ''), faults.length === 0, faults);
}

const origin = await serve({ ui: join(REPO, 'ui'), fixtures: FIXTURES });
const ten = json('state-ten.json').snapshot;
const A = ten.rows[0];
const sel = (a, p) => '[data-act="' + a + '"][data-param="' + p + '"]';

await test('the_helper_is_one_object_that_both_views_reach_and_it_makes_no_id_or_class_of_a_view', async () =>
  withPage({ origin, state: 'ten', views: [CONFIRM, NEEDS, AGENTS] }, async (h) => {
    const seen = await h.page.evaluate(() => ({
      keys: Object.keys(window.Halvo.confirm).sort(),
      groups: [...document.querySelectorAll('[id^="r4a-group-"]')].length,
    }));
    return [same(seen.keys, ['ask', 'busy', 'dress', 'drop', 'region', 'show', 'takes', 'whenFree']) && seen.groups > 0, seen];
  }),
);

await test('r1_r2_the_status_element_of_the_helper_is_written_out_and_each_addition_takes_out_what_it_held', async () =>
  withPage({ origin, state: 'empty', views: [CONFIRM] }, async (h) => {
    const seen = await h.page.evaluate(() => {
      const r = window.Halvo.confirm.region('x-say');
      document.body.appendChild(r.element);
      r.add(['One.', 'Two.']);
      const first = [...r.element.children].map((p) => p.textContent);
      r.add(['Three.']);
      const second = [...r.element.children].map((p) => p.textContent);
      r.add([null, '']);
      const third = r.element.childNodes.length;
      return {
        role: r.element.getAttribute('role'),
        live: r.element.getAttribute('aria-live'),
        atomic: r.element.getAttribute('aria-atomic'),
        first,
        second,
        third,
      };
    });
    return [same(seen, { role: 'status', live: 'polite', atomic: 'true', first: ['One.', 'Two.'], second: ['Three.'], third: 0 }), seen];
  }),
);

await test('the_question_holds_the_focus_itself_and_enter_on_it_answers_nothing', async () => {
  const out = outDir();
  return withPage({ origin, state: 'ten', views: [CONFIRM, AGENTS], out }, async (h) => {
    await h.page.focus(sel('delete', A.agent));
    await h.page.keyboard.press('Enter');
    await settle(h);
    const opened = await modal(h);
    await h.page.keyboard.press('Enter');
    await h.page.keyboard.press(' ');
    await settle(h);
    const still = await modal(h);
    return [Boolean(opened && opened.focused && opened.inQuestion && still && still.focused) && calls(out).length === 0, { opened, still, calls: calls(out) }];
  });
});

await test('a_held_key_begun_before_the_question_opened_answers_nothing_in_it', async () => {
  const out = outDir();
  return withPage({ origin, state: 'ten', views: [CONFIRM, AGENTS], out }, async (h) => {
    await h.page.focus(sel('delete', A.agent));
    await h.page.keyboard.down('Enter');
    await settle(h);
    await h.page.evaluate(() => {
      const d = [...document.querySelectorAll('[role="alertdialog"]')].find((x) => x.matches(':modal'));
      d.querySelector('[data-act="allow"]').focus();
    });
    for (let i = 0; i < 3; i++) {
      await h.page.keyboard.down('Enter');
      await settle(h);
    }
    await h.page.keyboard.up('Enter');
    await settle(h);
    const still = await modal(h);
    return [Boolean(still) && calls(out).length === 0, { still, calls: calls(out) }];
  });
});

await test('the_press_rule_decides_on_the_last_event_alone', async () => {
  const out = outDir();
  const replies = [{ state: ten }, { call: 'agents_delete', args: { agent: A.agent }, reply: { ok: 'yes' } }];
  return withPage({ origin, replies, views: [CONFIRM, AGENTS], out }, async (h) => {
    await h.page.focus(sel('delete', A.agent));
    await h.page.keyboard.press('Enter');
    await settle(h);
    const fire = (first, last) =>
      h.page.evaluate(
        ([a, b]) => {
          const d = [...document.querySelectorAll('[role="alertdialog"]')].find((x) => x.matches(':modal'));
          if (!d) return 'none';
          const allow = d.querySelector('[data-act="allow"]');
          allow.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', repeat: a, bubbles: true, cancelable: true }));
          allow.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', repeat: b, bubbles: true, cancelable: true }));
          allow.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, detail: 0 }));
          return [...document.querySelectorAll('[role="alertdialog"]')].some((x) => x.matches(':modal')) ? 'open' : 'closed';
        },
        [first, last],
      );
    const fresh_then_repeat = await fire(false, true);
    await settle(h);
    const before = calls(out).length;
    const repeat_then_fresh = await fire(true, false);
    await settle(h);
    const sent = calls(out);
    const ok = fresh_then_repeat === 'open' && before === 0 && repeat_then_fresh === 'closed' && same(sent, [{ call: 'agents_delete', args: { agent: A.agent } }]);
    return [ok, { fresh_then_repeat, before, repeat_then_fresh, sent }];
  });
});

await test('one_confirmation_is_open_at_a_time_for_the_whole_page', async () =>
  withPage({ origin, state: 'needs_confirm_page', views: [CONFIRM, NEEDS, AGENTS] }, async (h) => {
    await settle(h);
    const first = await modal(h);
    const pressed = await h.page.evaluate((s) => {
      const b = document.querySelector(s);
      if (!b) return 'none';
      b.click();
      return 'clicked';
    }, sel('delete', json('state-needs_confirm_page.json').snapshot.rows[0].agent));
    await settle(h);
    const dialogs = await h.page.evaluate(() => [...document.querySelectorAll('[role="alertdialog"]')].filter((d) => d.matches(':modal')).length);
    const busy = await h.page.evaluate(() => window.Halvo.confirm.busy());
    return [Boolean(first) && !first.inQuestion && dialogs === 1 && busy === true, { first, pressed, dialogs, busy }];
  }),
);

const engine = await withPage({ origin, state: 'empty', views: [CONFIRM] }, async (h) => h.page.context().browser().browserType().name());
if (engine === 'chromium') {
  await test('a_tap_at_once_after_a_refused_early_touch_counts_as_a_double_tap_and_needs_a_second_tap', async () => {
    const out = outDir();
    const replies = [{ state: ten }, { call: 'agents_delete', args: { agent: A.agent }, reply: { ok: 'yes' } }];
    return withPage({ origin, replies, views: [CONFIRM, AGENTS], out }, async (h) => {
      const cdp = await h.page.context().newCDPSession(h.page);
      await cdp.send('Emulation.setTouchEmulationEnabled', { enabled: true, maxTouchPoints: 1 });
      const at = async (s) => {
        const box = await h.page.locator(s).boundingBox();
        return { x: box.x + box.width / 2, y: box.y + box.height / 2 };
      };
      const tap = async (p) => {
        await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [p] });
        await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
      };
      await h.page.focus(sel('delete', A.agent));
      await h.page.keyboard.press('Enter');
      await settle(h);
      const p = await at('[role="alertdialog"] [data-act="allow"]');
      await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [p] });
      await h.page.evaluate(() => {
        const d = [...document.querySelectorAll('[role="alertdialog"]')].find((x) => x.matches(':modal'));
        d.close();
      });
      await settle(h);
      await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
      await tap(p);
      await settle(h);
      const early = calls(out).length;
      await h.page.waitForTimeout(800);
      await tap(p);
      await settle(h);
      return [early === 0 && calls(out).length === 1, { early, calls: calls(out) }];
    });
  });
} else {
  console.log('confirm touch: not run on ' + engine + ', which takes no touch input through the browser\'s protocol in this harness');
}

console.log('confirm tests passed=' + passed + ' failed=' + failed);
process.exit(failed === 0 ? 0 : 1);
