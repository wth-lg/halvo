import { existsSync, readFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { join } from 'node:path';
import { HERE, Refusal, refuse, installed } from './machine.mjs';
import { serve, open, check } from './index.mjs';

const SAMPLE = join(HERE, 'sample');
const STATES = ['a', 'b'];
const FILES = ['state-a.json', 'state-b.json', 'reply-ok.json', 'reply-next-same.json'];
const MODES = ['reduced_motion', 'forced_colors', 'width_320'];

const FAULTS = {
  1: [
    { fault: 'contrast', state: 'a', want: (r) => r.counts.violations > 0 },
    { fault: 'late_next', state: 'a', want: (r) => r.counts.violations > 0 },
    { fault: 'late_css', state: 'a', want: (r) => r.counts.violations > 0 },
    { fault: 'outside_request', state: 'a', want: (r) => r.names.includes('outside_request') },
  ],
  2: [
    { fault: 'unreachable', state: 'a', want: (r) => r.names.includes('unreached') },
    { fault: 'focus_unseen', state: 'a', want: (r) => r.names.includes('focus_unseen') },
    { fault: 'focus_escapes', state: 'b', want: (r) => r.names.includes('dialog_focus_lost') },
    { fault: 'escape_row', state: 'b', want: (r) => r.names.includes('dialog_focus_lost') },
    { fault: 'dialog_blur', state: 'b', want: (r) => r.names.includes('dialog_not_taken') },
    { fault: 'dialog_noclose', state: 'b', want: (r) => r.names.includes('dialog_not_closed') },
    { fault: 'dialog_role', state: 'b', want: (r) => r.names.includes('dialog_not_closed') },
    { fault: 'inert_left', state: 'b', want: (r) => r.names.includes('dialog_not_closed') },
    { fault: 'safe_second', state: 'b', want: (r) => r.names.includes('dialog_safe_not_first') },
    { fault: 'body_stays', state: 'b', want: (r) => r.names.includes('dialog_focus_lost') },
    { fault: 'dialog_plain', state: 'b', want: (r) => r.names.includes('dialog_missing') },
    { fault: 'dialog_no_modal', state: 'b', want: (r) => r.names.includes('dialog_not_modal') },
    { fault: 'dialog_no_label', state: 'b', want: (r) => r.names.includes('dialog_no_label') },
    { fault: 'dialog_no_description', state: 'b', want: (r) => r.names.includes('dialog_no_description') },
    { fault: 'dialog_not_inert', state: 'b', want: (r) => r.names.includes('dialog_not_inert') },
    { fault: 'focus_pale', state: 'a', want: (r) => r.names.includes('focus_contrast') },
    { fault: 'focus_behind_image', state: 'a', want: (r) => r.names.includes('focus_contrast') },
    { fault: 'radio_forms', state: 'a', want: (r) => r.names.includes('unreached') },
    { fault: 'radio_arrows', state: 'a', want: (r) => r.names.includes('radio_arrows') },
    { fault: 'shadow_script', state: 'a', want: (r) => r.names.includes('opaque') },
    { fault: 'key_error', state: 'a', want: (r) => r.names.includes('page_error') },
    { fault: 'plaintext_only', state: 'a', want: (r) => r.names.includes('unreached') },
  ],
  3: [
    { fault: 'renamed', state: 'a', want: (r) => r.counts.structure === 'differ' },
    { fault: 'frame_file', state: 'a', frame: '/frame-shadow.html', want: (r) => r.names.includes('opaque') },
  ],
  4: [
    { fault: 'small', state: 'a', want: (r) => r.names.includes('delete') },
    { fault: 'shadow_markup', state: 'a', want: (r) => r.names.includes('opaque') },
    { fault: 'never_quiet', state: 'a', want: (r) => r.names.includes('unsettled') },
    { fault: 'dialog_30', state: 'b', want: (r) => r.names.includes('reject') },
    { fault: 'clipped', state: 'a', want: (r) => r.names.includes('more') },
    { fault: 'wrapped', state: 'a', want: (r) => r.names.includes('span') },
    { fault: 'act_kind', state: 'a', want: (r) => r.names.includes('more') },
  ],
  5: [
    { fault: 'motion_hidden', state: 'a', mode: 'reduced_motion' },
    { fault: 'forced_hidden', state: 'a', mode: 'forced_colors' },
    { fault: 'narrow_scroll', state: 'a', mode: 'width_320' },
    { fault: 'outside_mode', state: 'a', mode: 'reduced_motion', engine: true },
    { fault: 'inner_scroll', state: 'a', mode: 'width_320' },
    { fault: 'engine_mode', state: 'a', mode: 'forced_colors', engine: true },
    { fault: 'shadow_parse', state: 'a', want: (r) => r.names.includes('opaque') && r.counts.modes_off === MODES.join(',') },
  ],
  6: [
    { fault: 'inner_html', state: 'a', want: (r) => r.counts.markup === 'miss' },
    { fault: 'day_html', state: 'a', want: (r) => r.counts.markup === 'miss' },
    { fault: 'count_only', state: 'a', want: (r) => r.counts.markup === 'miss' },
    { fault: 'marker_only', state: 'a', want: (r) => r.counts.markup === 'miss' },
    { fault: 'frame', state: 'a', want: (r) => r.names.includes('opaque') },
  ],
};

function structure(state) {
  return join(SAMPLE, 'structure-' + state + '.txt');
}

const reached = { count: 0, port: 0 };

async function listen() {
  const server = createServer((req, res) => {
    reached.count++;
    res.writeHead(404).end();
  });
  server.on('upgrade', (req, socket) => {
    reached.count++;
    socket.destroy();
  });
  server.on('connection', (socket) => socket.unref());
  await new Promise((done, fail) => {
    server.once('error', fail);
    server.listen(0, '127.0.0.1', done);
  });
  server.unref();
  reached.port = server.address().port;
}

async function run(origin, n, state, fault, frame) {
  const view = fault ? '/view.js?fault=' + fault + '&port=' + reached.port : '/view.js';
  const page = await open({ origin, state, views: [view], frame });
  try {
    const result = await check(page, n, { snapshot: structure(state) });
    return { ...result, modes: page.modes };
  } finally {
    await page.close();
  }
}

function caught(f, r, before) {
  if (r.ok || reached.count !== before) return 'missed';
  if (f.mode && !f.want) {
    for (const m of MODES) {
      const want = m === f.mode ? 'miss' : r.modes[m] === 'off' ? 'off' : 'ok';
      if (r.modes[m] !== want) return 'missed';
    }
    return 'caught';
  }
  return f.want(r) ? 'caught' : 'missed';
}

async function engine(origin) {
  const page = await open({ origin, state: 'a', views: ['/view.js'] });
  try {
    await check(page, 1);
    return null;
  } catch (err) {
    if (err && err.code === 'engine') return err.reason;
    throw err;
  } finally {
    await page.close();
  }
}

async function main() {
  if (process.argv.length > 2) throw new Refusal('usage', 'node tools/r4-page/selftest.mjs');
  installed();
  for (const name of [...FILES, ...STATES.map((s) => 'structure-' + s + '.txt')]) {
    const path = join(SAMPLE, name);
    if (!existsSync(path)) throw new Refusal('fixture', 'tools/r4-page/sample/' + name + ' is missing');
    if (name.endsWith('.json')) {
      try {
        JSON.parse(readFileSync(path, 'utf8'));
      } catch {
        throw new Refusal('fixture', 'tools/r4-page/sample/' + name + ' is no JSON');
      }
    }
  }
  await listen();
  const origin = await serve({ ui: SAMPLE, fixtures: SAMPLE });
  const none = await engine(origin);
  const checks = none ? [2, 3, 4, 5, 6] : [1, 2, 3, 4, 5, 6];
  if (none) process.stderr.write('engine none reason=' + none + '\n');
  let passed = 0;
  for (const n of checks) {
    let sample = 'ok';
    for (const state of STATES) {
      const r = await run(origin, n, state, null);
      if (!r.ok) sample = 'miss';
    }
    let fault = 'caught';
    let ran = 0;
    for (const f of FAULTS[n]) {
      if (none && f.engine) continue;
      const before = reached.count;
      const r = await run(origin, n, f.state, f.fault, f.frame);
      if (f.mode && r.modes[f.mode] === 'off') continue;
      ran++;
      if (caught(f, r, before) !== 'caught') fault = 'missed';
    }
    if (ran === 0) fault = 'missed';
    if (sample === 'ok' && fault === 'caught') passed++;
    process.stdout.write('selftest check=' + n + ' sample=' + sample + ' fault=' + fault + '\n');
  }
  process.stdout.write('page self-test: ' + passed + ' of ' + checks.length + ' ok engine=' + (none ? 'none' : 'offline') + '\n');
  process.exitCode = passed === checks.length ? 0 : 1;
}

main().catch((err) => {
  if (refuse(err)) return;
  process.stdout.write('page self-test: failed error=' + String(err && err.name ? err.name : 'unknown') + '\n');
  process.stderr.write(String(err && err.stack ? err.stack.split('\n')[0] : err) + '\n');
  process.exitCode = 1;
});
