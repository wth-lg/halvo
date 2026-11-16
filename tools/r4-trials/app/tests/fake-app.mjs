// A stand-in for the trial app in tests: it opens no window and makes no tray item. It speaks the
// control lines of I11.3, prints the core's and the page's lines as the app does, and writes the
// state of its window (onscreen, hidden or absent) to the file FAKE_STATE, which the readers of
// fake-readers.mjs read. FAKE_FAULT names faults, comma-separated: show_kept, show_kept_first,
// show_kept_later, hide_kept, destroy_kept, quit_stays, wrong_pid, counts_frozen, hidden_unmarked,
// get_open, lose_last, repeat_last, hide_unseen, shows_differ (each show answers its own show_ms),
// child_alive (a child that outlives the app, whose pid the state file holds third), exit_one (quit
// ends with 1), repeat_early (change 4,999 again after 5,000) and closed_early (a closed line at the
// first show, before any hide).
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import net from 'node:net';

const args = process.argv.slice(2);
const flag = (name, fallback) => {
  const at = args.indexOf(name);
  return at >= 0 ? args[at + 1] : fallback;
};
const arm = flag('--arm', 'main');
const way = flag('--sync', null);
const faults = new Set((process.env.FAKE_FAULT ?? '').split(',').filter(Boolean));
const stateFile = process.env.FAKE_STATE;
const child = faults.has('child_alive') ? spawn(process.execPath, ['-e', 'setTimeout(() => {}, 120000)'], { stdio: 'ignore', detached: true }) : null;
const system = process.platform === 'win32' ? 'windows' : 'macos';

let windowState = 'hidden';
let page = null;
let latest = 0;
let hiddenMark = false;
let running = 0;
let runningSince = null;
let slowNext = false;
let sending = false;
let sendOf = 0;
let shows = 0;

function print(line) {
  process.stdout.write(`${line}\n`);
}

function setWindow(state) {
  windowState = state;
  if (stateFile) fs.writeFileSync(stateFile, `${state} ${process.pid}${child ? ` ${child.pid}` : ''}\n`);
}

function newPage() {
  page = { last: 0, order: 'ok', afterHide: 0, hiddenTicks: 0, mark: 0, ended: false, hidden: false };
}

function quietLines() {
  if (arm !== 'a2' || system !== 'windows') return;
  print('core quiet system=windows call=set_is_visible result=ok');
  print('core quiet system=windows call=set_memory_usage_target_level result=ok');
}

function counting() {
  return page && !(page.hidden && (arm === 'a' || arm === 'a2'));
}

function startCount() {
  if (runningSince === null && counting()) runningSince = Date.now();
}

function stopCount() {
  if (runningSince !== null) running += Date.now() - runningSince;
  runningSince = null;
}

function elapsed() {
  return running + (runningSince === null ? 0 : Date.now() - runningSince);
}

function accept(seq, of, hidden) {
  if (!page) return;
  if (seq <= page.last) {
    if (page.order === 'ok') print(`page sync order_miss seq=${seq} last=${page.last}`);
    page.order = 'miss';
  } else {
    page.last = seq;
  }
  if (hidden) page.afterHide += 1;
  const mark = Math.floor(page.last / 10000) * 10000;
  if (mark > page.mark) {
    page.mark = mark;
    print(`page sync mark=${mark}`);
  }
  if (page.ended) {
    print(`page sync after_end seq=${seq} last=${page.last} order=${page.order}`);
  } else if (seq === of) {
    page.ended = true;
    print(`page sync end last=${page.last} order=${page.order} after_hide=${page.afterHide} hidden_ticks=${page.hiddenTicks}`);
  }
}

function deliver(seq, of) {
  latest = seq;
  if (faults.has('lose_last') && seq === of) return;
  if (!page) return;
  if (way === 'get') {
    if (!page.hidden || faults.has('get_open')) accept(seq, of, hiddenMark && !faults.has('get_open') ? true : false);
    return;
  }
  accept(seq, of, hiddenMark && !faults.has('hidden_unmarked'));
  if (faults.has('repeat_early') && seq === 5000) accept(4999, of, false);
  if (faults.has('repeat_last') && seq === of) accept(seq, of, false);
}

function send(count, rate) {
  if (sending) return 'err busy';
  sending = true;
  sendOf = count;
  const started = Date.now();
  let sent = 0;
  const timer = setInterval(() => {
    const due = Math.min(count, Math.floor(((Date.now() - started) * rate) / 1000));
    while (sent < due) {
      sent += 1;
      deliver(sent, count);
    }
    if (sent >= count) {
      clearInterval(timer);
      sending = false;
      print(`core send_done sent=${sent}`);
      setTimeout(() => {
        if (page && !page.ended) {
          print(`page sync stalled last=${page.last} order=${page.order} after_hide=${page.afterHide} hidden_ticks=${page.hiddenTicks}`);
        }
      }, 200);
    }
  }, 5);
  return 'ok';
}

function answer(line) {
  if (line === 'show') {
    if (arm === 'b' && windowState === 'absent') {
      newPage();
      if (way === 'get' && latest > 0) accept(latest, sendOf, false);
    }
    if (!page) newPage();
    hiddenMark = false;
    shows += 1;
    const kept = faults.has('show_kept') || (faults.has('show_kept_first') && shows === 1) || (faults.has('show_kept_later') && shows > 1);
    if (!kept) setWindow('onscreen');
    quietLines();
    const seen = page.hidden;
    page.hidden = false;
    if (arm === 'a' || arm === 'a2') {
      print(`page window shown after_hide=${page.afterHide} hidden_ticks=${page.hiddenTicks} hide_seen=${seen ? 'yes' : 'no'}`);
    }
    if (seen && way === 'get' && latest > page.last) accept(latest, sendOf, false);
    startCount();
    if (faults.has('closed_early') && shows === 1 && way === 'get') print(`page sync closed last=${page.last}`);
    const ms = slowNext ? 1500 : faults.has('shows_differ') ? 10 * shows + 2 : 12;
    slowNext = false;
    return `ok show_ms=${ms}`;
  }
  if (line === 'hide') {
    if (windowState === 'absent') return 'err no_window';
    hiddenMark = true;
    if (page && !faults.has('hide_unseen')) {
      page.hidden = true;
      if (way === 'get' && !faults.has('get_open')) print(`page sync closed last=${page.last}`);
    }
    quietLines();
    stopCount();
    startCount();
    if (!faults.has('hide_kept')) setWindow('hidden');
    return 'ok';
  }
  if (line === 'destroy') {
    if (windowState === 'absent') return 'err no_window';
    hiddenMark = true;
    stopCount();
    page = null;
    if (!faults.has('destroy_kept')) setWindow('absent');
    return 'ok';
  }
  if (line === 'sample') {
    const pid = faults.has('wrong_pid') ? process.pid + 1 : process.pid;
    if (!page) return `ok pid=${pid} ticks=- frames=-`;
    const ms = faults.has('counts_frozen') ? 0 : elapsed();
    return `ok pid=${pid} ticks=${Math.floor(ms / 100)} frames=${Math.floor(ms / 16)}`;
  }
  if (line.startsWith('load ')) {
    slowNext = line === 'load slow.html';
    return 'ok';
  }
  const sent = /^send (\d+) (\d+)$/.exec(line);
  if (sent) return send(Number(sent[1]), Number(sent[2]));
  return 'err unknown_line';
}

const server = net.createServer((socket) => {
  let buffer = '';
  socket.setEncoding('utf8');
  socket.on('error', () => {});
  socket.on('data', (chunk) => {
    buffer += chunk;
    let at;
    while ((at = buffer.indexOf('\n')) >= 0) {
      const line = buffer.slice(0, at).replace(/\r$/, '');
      buffer = buffer.slice(at + 1);
      if (line === 'quit') {
        socket.write('ok\n');
        if (!faults.has('quit_stays')) setTimeout(() => process.exit(faults.has('exit_one') ? 1 : 0), 50);
        continue;
      }
      socket.write(`${answer(line)}\n`);
    }
  });
});

server.listen(0, '127.0.0.1', () => {
  setWindow('hidden');
  newPage();
  print(`control port=${server.address().port}`);
  if (arm === 'a2' && system === 'macos') print('core quiet system=macos call=background_throttling result=ok');
});
