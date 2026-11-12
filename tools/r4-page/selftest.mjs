import { copyFileSync, existsSync, mkdtempSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { createServer } from 'node:http';
import { join } from 'node:path';
import { HERE, Refusal, refuse, installed } from './machine.mjs';
import { serve, open, check, act, trail } from './index.mjs';

const SAMPLE = join(HERE, 'sample');
const STATES = ['a', 'b', 'c'];
const FILES = ['state-a.json', 'state-b.json', 'state-c.json', 'reply-ok.json', 'reply-next-same.json'];
const MODES = ['reduced_motion', 'forced_colors', 'width_320'];

const FAULTS = {
  1: [
    { fault: 'contrast', state: 'a', want: (r) => r.counts.violations > 0 },
    { fault: 'late_next', state: 'a', want: (r) => r.counts.violations > 0 },
    { fault: 'late_css', state: 'a', want: (r) => r.counts.violations > 0 },
    { fault: 'late_long', state: 'a', want: (r) => r.counts.violations > 0 },
    { fault: 'outside_request', state: 'a', want: (r) => r.names.includes('outside_request') },
    { fault: 'outside_fetch', state: 'a', want: (r) => r.names.includes('outside_request') },
    { fault: 'port_fetch', state: 'a', want: (r) => r.names.includes('outside_request') },
    { fault: 'port_socket', state: 'a', want: (r) => r.names.includes('outside_request') },
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
    { fault: 'modal_left', state: 'b', want: (r) => r.names.includes('dialog_not_closed') },
    { fault: 'alert_left', state: 'b', want: (r) => r.names.includes('dialog_not_closed') },
    { fault: 'plain_next', state: 'c', want: (r) => r.names.includes('dialog_not_closed') },
    { fault: 'skip_first', state: 'c', want: (r) => r.names.includes('dialog_safe_not_first') },
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
    { fault: 'frame_file', state: 'a', frame: '/frame-shadow.html', refused: true },
    { fault: 'frame_slash', state: 'a', frame: '/frame-shadow-slash.html', refused: true },
  ],
  4: [
    { fault: 'small', state: 'a', want: (r) => r.names.includes('delete') },
    { fault: 'shadow_markup', state: 'a', want: (r) => r.names.includes('opaque') },
    { fault: 'shadow_word', state: 'a', want: (r) => r.names.includes('opaque') },
    { fault: 'parse_closed', state: 'a', want: (r) => r.names.includes('opaque') },
    { fault: 'shadow_tostring', state: 'a', want: (r) => r.names.includes('opaque') },
    { fault: 'never_quiet', state: 'a', want: (r) => r.names.includes('unsettled') },
    { fault: 'dialog_30', state: 'b', want: (r) => r.names.includes('reject') },
    { fault: 'clipped', state: 'a', want: (r) => r.names.includes('more') },
    { fault: 'wrapped', state: 'a', want: (r) => r.names.includes('span') },
    { fault: 'act_kind', state: 'a', want: (r) => r.names.includes('more') },
    { fault: 'no_act', state: 'a', want: (r) => r.names.includes('no_act') },
    { fault: 'area_no_act', state: 'a', want: (r) => r.names.includes('no_act') },
    { fault: 'area_unnamed_no_act', state: 'a', want: (r) => r.names.includes('no_act') },
    { fault: 'input_no_act', state: 'a', want: (r) => r.names.includes('no_act') },
    { fault: 'select_no_act', state: 'a', want: (r) => r.names.includes('no_act') },
    { fault: 'textarea_no_act', state: 'a', want: (r) => r.names.includes('no_act') },
    { fault: 'link_no_act', state: 'a', want: (r) => r.names.includes('no_act') },
    { fault: 'summary_no_act', state: 'a', want: (r) => r.names.includes('no_act') },
    { fault: 'audio_no_act', state: 'a', want: (r) => r.names.includes('no_act') },
    { fault: 'video_no_act', state: 'a', want: (r) => r.names.includes('no_act') },
  ],
  5: [
    { fault: 'motion_hidden', state: 'a', mode: 'reduced_motion' },
    { fault: 'forced_hidden', state: 'a', mode: 'forced_colors' },
    { fault: 'narrow_scroll', state: 'a', mode: 'width_320' },
    { fault: 'outside_mode', state: 'a', mode: 'reduced_motion', engine: true },
    { fault: 'inner_scroll', state: 'a', mode: 'width_320' },
    { fault: 'engine_mode', state: 'a', mode: 'forced_colors', engine: true },
    { fault: 'error_mode', state: 'a', mode: 'forced_colors' },
    { fault: 'shadow_parse', state: 'a', want: (r) => r.names.includes('opaque') && r.counts.modes_off === MODES.join(',') },
  ],
  6: [
    { fault: 'inner_html', state: 'a', want: (r) => r.counts.markup === 'miss' },
    { fault: 'day_html', state: 'a', want: (r) => r.counts.markup === 'miss' },
    { fault: 'count_only', state: 'a', want: (r) => r.counts.markup === 'miss' },
    { fault: 'marker_only', state: 'a', want: (r) => r.counts.markup === 'miss' },
    { fault: 'frame', state: 'a', want: (r) => r.names.includes('opaque') },
    { fault: 'shadow_realm_open', state: 'a', want: (r) => r.names.includes('opaque') },
    { fault: 'stub_forge', state: 'a', want: (r) => r.names.includes('stub_tampered') },
    { fault: 'stub_deliver', state: 'a', want: (r) => r.names.includes('stub_tampered') },
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
  if (f.refused) return r.refused && reached.count === before ? 'caught' : 'missed';
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

async function stubLines(origin) {
  const agent = JSON.parse(readFileSync(join(SAMPLE, 'state-a.json'), 'utf8')).snapshot.rows[0].agent;
  const first = { state: JSON.parse(readFileSync(join(SAMPLE, 'state-a.json'), 'utf8')).snapshot };
  const has = async (o, word, how) => {
    const page = await open({ origin: o, views: ['/view.js'], ...how });
    try {
      await act(page, { act: 'delete', param: agent }).catch(() => {});
      return trail(page).some((l) => l.includes(word));
    } finally {
      await page.close();
    }
  };
  const rejected = await has(origin, 'rejected by=transcript', { replies: [first, { call: 'agents_control', args: { agent, action: 'delete' }, reject: true }] });
  const refused = await has(origin, 'refused cause=not_next_in_transcript', { replies: [first, { call: 'agents_control', args: { agent, action: 'stop' }, reply: { ok: 'yes' } }] });
  const dir = mkdtempSync(join(tmpdir(), 'r4-selftest-'));
  for (const name of ['state-a.json', 'reply-next-same.json']) copyFileSync(join(SAMPLE, name), join(dir, name));
  const bare = await serve({ ui: SAMPLE, fixtures: dir });
  const unread = await has(bare, 'stub fixture=reply-ok.json cause=status_404', { state: 'a' });
  const unanswered = await has(bare, 'stub call=agents_control unanswered', { state: 'a' });
  return rejected && refused && unread && unanswered;
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
      const r = await run(origin, n, f.state, f.fault, f.frame).catch((err) => {
        if (f.refused && err instanceof Refusal && err.code === 'fixture') return { refused: true };
        throw err;
      });
      if (f.mode && r.modes[f.mode] === 'off') continue;
      ran++;
      if (caught(f, r, before) !== 'caught') fault = 'missed';
    }
    if (ran === 0) fault = 'missed';
    if (sample === 'ok' && fault === 'caught') passed++;
    process.stdout.write('selftest check=' + n + ' sample=' + sample + ' fault=' + fault + '\n');
  }
  const lines = await stubLines(origin);
  process.stdout.write('selftest stub_lines=' + (lines ? 'ok' : 'miss') + '\n');
  process.stdout.write('page self-test: ' + passed + ' of ' + checks.length + ' ok engine=' + (none ? 'none' : 'offline') + '\n');
  process.exitCode = passed === checks.length && lines ? 0 : 1;
}

main().catch((err) => {
  if (refuse(err)) return;
  process.stdout.write('page self-test: failed error=' + String(err && err.name ? err.name : 'unknown') + '\n');
  process.stderr.write(String(err && err.stack ? err.stack.split('\n')[0] : err) + '\n');
  process.exitCode = 1;
});
