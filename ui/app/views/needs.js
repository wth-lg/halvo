(function () {
  const CSS = [
    '.hn-items{list-style:none;margin:0;padding:0;overflow-wrap:anywhere}',
    '.hn-item{border-bottom-width:1px;border-bottom-style:solid;border-bottom-color:#6b6b6b;padding-top:8px;padding-bottom:8px}',
    '.hn-title{font-size:1.1rem;margin-top:8px;margin-bottom:8px}',
    '.hn-text{margin-top:4px;margin-bottom:8px}',
    '.hn-line{font-weight:bold;margin-top:8px;margin-bottom:4px}',
    '.hn-fields{margin-top:4px;margin-bottom:8px}',
    '.hn-label{font-weight:bold}',
    '.hn-value{margin-left:0;white-space:pre-wrap;overflow-wrap:anywhere}',
    '.hn-from{margin-left:0;margin-bottom:8px;font-style:italic}',
    '.hn-param{border-width:1px;border-style:solid;border-color:#6b6b6b;margin-top:8px;margin-bottom:8px;padding:8px;min-width:0}',
    '.hn-choice{display:flex;align-items:center;gap:8px;margin-top:4px;margin-bottom:4px;overflow-wrap:anywhere}',
    '.hn-radio{-webkit-appearance:none;appearance:none;flex:none;width:24px;height:24px;margin:0;border-width:2px;border-style:solid;border-color:#1a1a1a;border-radius:50%;background-color:#ffffff}',
    '.hn-radio:checked{border-width:8px}',
    '.hn-typed{box-sizing:border-box;width:100%;max-width:24rem;min-height:44px;border-width:2px;border-style:solid;border-color:#1a1a1a;border-radius:4px;padding:4px;font:inherit;color:#1a1a1a;background-color:#ffffff}',
    '.hn-controls{display:flex;flex-wrap:wrap;gap:8px;margin-top:8px}',
    '.hn-button{-webkit-appearance:none;appearance:none;min-width:44px;min-height:44px;padding:8px;border-width:2px;border-style:solid;border-color:#1a1a1a;border-radius:4px;background-color:#f2f2f2;color:#1a1a1a;font:inherit;overflow-wrap:anywhere}',
    '.hn-button:focus,.hn-radio:focus,.hn-typed:focus{outline-width:3px;outline-style:solid;outline-color:#0b57d0;outline-offset:2px}',
    '.hn-dialog{border-width:2px;border-style:solid;border-color:#1a1a1a;padding:16px;max-width:28rem;background-color:#ffffff;color:#1a1a1a}',
    '.hn-dialog:focus{outline-width:3px;outline-style:solid;outline-color:#0b57d0;outline-offset:2px}',
    '.hn-dialog::backdrop{background-color:rgba(0,0,0,0.4)}',
    '.hn-say{position:absolute;width:1px;height:1px;margin:-1px;padding:0;border-width:0;overflow:hidden;clip-path:inset(50%);white-space:nowrap}'
  ];

  let root = null;
  let region = null;
  let list = null;
  let shown = false;
  let turn = 0;
  let seq = 0;
  let last = null;
  let cards = new Map();
  const answered = new Set();
  let serial = 0;
  let tick = 0;
  let press = null;
  let pointer = null;
  let pointedAt = null;
  let raising = false;
  const keys = new Map();
  let holdBase = null;
  const heldGone = new Map();
  const answeredGone = new Set();
  const opened = new Set();
  let pointerRead = [];
  let closing = false;

  function fresh() {
    serial += 1;
    return 'hn-' + serial;
  }

  function el(tag, cls, text) {
    const node = document.createElement(tag);
    if (cls) node.className = cls;
    if (text) node.appendChild(document.createTextNode(text));
    return node;
  }

  function bytes(text) {
    let size = 0;
    for (const ch of text) {
      const code = ch.codePointAt(0);
      size += code < 128 ? 1 : code < 2048 ? 2 : code < 65536 ? 3 : 4;
    }
    return size;
  }

  function changed(before, after) {
    let start = 0;
    while (start < before.length && start < after.length && before[start] === after[start]) start += 1;
    let end = 0;
    while (end < before.length - start && end < after.length - start && before[before.length - 1 - end] === after[after.length - 1 - end]) end += 1;
    return { start: start, gone: before.length - start - end, added: after.length - start - end };
  }

  function limit(typed, full, tooLong) {
    const line = { kept: '', composing: false, ended: false, held: false, field: typed, sent: null, text: null };
    let mark = false;
    let shown = null;
    let note = null;
    let before = null;
    let start = null;
    function show(which) {
      if (shown === which) return;
      shown = which;
      if (note) {
        note.remove();
        note = null;
        typed.removeAttribute('aria-describedby');
      }
      const text = which === 'full' ? full : which === 'too_long' ? tooLong : '';
      if (!text) return;
      note = el('p', 'hn-full', text);
      note.id = fresh();
      typed.parentNode.insertBefore(note, typed.nextSibling);
      typed.setAttribute('aria-describedby', note.id);
      tell(text, line);
    }
    function atBound() {
      return mark || bytes(line.kept) >= 1024;
    }
    function judge(now, was) {
      const change = changed(line.kept, now);
      if (change.gone === 0 && change.added === 0) return true;
      if (bytes(now) > 1024 || (atBound() && change.added > 0)) {
        typed.value = line.kept;
        if (was) typed.setSelectionRange(was[0], was[1]);
        else typed.setSelectionRange(change.start, change.start + change.gone);
        if (bytes(line.kept) > 1020) mark = true;
        show(atBound() ? 'full' : 'too_long');
        return false;
      }
      line.kept = now;
      if (change.added === 0) {
        mark = false;
        show(null);
      }
      return true;
    }
    line.sent = function () {
      if (shown === 'too_long') show(null);
    };
    line.text = function () {
      return shown === 'full' ? full : shown === 'too_long' ? tooLong : '';
    };
    typed.addEventListener('beforeinput', function () {
      before = [typed.selectionStart, typed.selectionEnd];
    });
    typed.addEventListener('compositionstart', function () {
      line.composing = true;
      start = [typed.selectionStart, typed.selectionEnd];
    });
    typed.addEventListener('compositionend', function () {
      line.composing = false;
      const was = start;
      start = null;
      before = null;
      line.ended = true;
      judge(typed.value, was);
    });
    typed.addEventListener('input', function () {
      if (line.composing) return;
      const was = before;
      before = null;
      judge(typed.value, was);
    });
    return line;
  }

  function pieces(tag, cls, value) {
    const node = el(tag, cls, '');
    value.forEach(function (piece) {
      node.appendChild(document.createTextNode(piece));
    });
    return node;
  }

  function call(name, args) {
    return Halvo.ipc.call(name, args).catch(function () {
      return null;
    });
  }

  function isPage(item) {
    return item.kind === 'confirm' && item.presenter === 'page';
  }

  function addFields(box, fields) {
    let dl = null;
    fields.forEach(function (f) {
      if (f.from === 'halvo' && f.value.join('') === '') {
        if (f.label) box.appendChild(el('p', 'hn-line', f.label));
        dl = null;
        return;
      }
      if (!dl) {
        dl = el('dl', 'hn-fields', '');
        box.appendChild(dl);
      }
      dl.appendChild(el('dt', 'hn-label', f.label));
      dl.appendChild(pieces('dd', 'hn-value', f.value));
      if (f.from_text) dl.appendChild(el('dd', 'hn-from', f.from_text));
    });
  }

  function addParams(box, item, answers) {
    item.params.forEach(function (p) {
      const set = el('fieldset', 'hn-param', '');
      const legend = el('legend', null, p.label);
      legend.id = fresh();
      const group = fresh();
      set.appendChild(legend);
      const radios = [];
      p.candidates.forEach(function (c) {
        const label = el('label', 'hn-choice', '');
        const input = document.createElement('input');
        input.type = 'radio';
        input.className = 'hn-radio';
        input.name = group;
        input.setAttribute('data-act', 'candidate');
        input.setAttribute('data-param', p.name);
        input.setAttribute('data-n', String(c.n));
        label.appendChild(input);
        c.value.forEach(function (piece) {
          label.appendChild(document.createTextNode(piece));
        });
        set.appendChild(label);
        radios.push({ input: input, n: c.n });
      });
      let typed = null;
      let line = null;
      if (p.typed === 'yes') {
        typed = document.createElement('input');
        typed.type = 'text';
        typed.className = 'hn-typed';
        typed.setAttribute('data-act', 'typed');
        typed.setAttribute('data-param', p.name);
        typed.setAttribute('aria-labelledby', legend.id);
        set.appendChild(typed);
        line = limit(typed, p.full, p.too_long);
      }
      box.appendChild(set);
      answers.push({ param: p.name, radios: radios, typed: typed, line: line });
    });
  }

  function chosen(item, answers) {
    const calls = [];
    answers.forEach(function (a) {
      const picked = a.radios.filter(function (r) {
        return r.input.checked;
      })[0];
      if (picked) {
        calls.push(['agents_answer', { item: item.item, choice: 'choose', param: a.param, n: picked.n }]);
      } else if (a.line && a.line.kept.trim() !== '') {
        calls.push(['agents_setup_typed', { item: item.item, param: a.param, text: a.line.kept }]);
      }
    });
    return calls;
  }

  function held(card, sent) {
    card.busy = true;
    return sent.then(function (reply) {
      card.busy = false;
      return reply;
    });
  }

  function linesOf(card) {
    return card.answers
      .map(function (a) {
        return a.line;
      })
      .filter(function (l) {
        return l !== null;
      });
  }

  function allLines() {
    let out = [];
    cards.forEach(function (card) {
      out = out.concat(linesOf(card));
    });
    return out;
  }

  function unsettled(l) {
    return l.composing || l.ended;
  }

  function stopped(card, event) {
    const read = event.detail > 0 ? pointerRead : [];
    const lines = linesOf(card).filter(function (l) {
      return read.indexOf(l) >= 0 || unsettled(l);
    });
    if (lines.length === 0) return false;
    linesOf(card).forEach(function (l) {
      l.ended = false;
    });
    const typed = lines[0].field;
    if (document.activeElement !== typed) {
      const sel = [typed.selectionStart, typed.selectionEnd];
      typed.focus();
      if (!lines[0].composing && (typed.selectionStart !== sel[0] || typed.selectionEnd !== sel[1])) typed.setSelectionRange(sel[0], sel[1]);
    }
    return true;
  }

  function answer(item, choice, card, event) {
    if (card.busy || stopped(card, event)) return;
    if (choice.choice === 'present') {
      held(card, call('agents_present', { item: item.item }));
      return;
    }
    if (choice.choice === 'choose' && item.params.length > 0) {
      const calls = chosen(item, card.answers);
      if (calls.length === 0) return;
      held(card, calls.reduce(function (done, c) {
        return done.then(function (reply) {
          return reply === null ? null : call(c[0], c[1]);
        });
      }, Promise.resolve({})));
      card.answers.forEach(function (a) {
        if (a.line) a.line.sent();
      });
      return;
    }
    if (isPage(item)) {
      answered.add(item.item);
      drop(card.key);
      openNext();
    }
    held(card, call('agents_answer', { item: item.item, choice: choice.choice })).then(function (reply) {
      if ((!reply || reply.error) && answered.delete(item.item) && last) apply(last);
    });
  }

  function input(event) {
    tick += 1;
    const type = event.type;
    if (type === 'pointerdown') {
      pointerRead = allLines().filter(unsettled);
    }
    if (type !== 'keydown' || !event.repeat) {
      allLines().forEach(function (l) {
        l.ended = false;
      });
    }
    if (type === 'keydown') {
      if (event.repeat && event.key === 'Enter' && event.target.tagName === 'BUTTON' && list.contains(event.target)) event.preventDefault();
      if (!event.repeat) keys.set(event.key, { at: tick, target: event.target });
      press = { target: event.target, repeat: event.repeat, at: event.repeat ? null : tick, from: event.target, raise: false, end: false };
    } else if (type === 'keyup') {
      const down = keys.get(event.key) || null;
      keys.delete(event.key);
      press = { target: event.target, repeat: false, at: down ? down.at : null, from: down ? down.target : null, raise: false, end: true };
    } else if (type === 'pointerdown') {
      pointedAt = controlName(event.target);
      pointer = { at: tick, target: event.target, raise: raising };
      raising = false;
      press = { target: event.target, repeat: false, at: tick, from: event.target, raise: pointer.raise, end: false };
    } else {
      press = { target: event.target, repeat: false, at: pointer ? pointer.at : null, from: pointer ? pointer.target : null, raise: pointer ? pointer.raise : false, end: true };
    }
  }

  function raised(event) {
    if (event.target === window) raising = true;
  }

  function controlName(node) {
    const mine = node && node.tagName === 'BUTTON' && node.hasAttribute('data-act') && list.contains(node);
    return mine ? node.getAttribute('data-act') + ':' + node.getAttribute('data-param') : '';
  }

  function pointedOn(b) {
    const began = pointedAt;
    pointedAt = null;
    return began === b.getAttribute('data-act') + ':' + b.getAttribute('data-param');
  }

  function freshFor(card, event) {
    const p = press;
    press = null;
    if (!p) return true;
    if (p.target !== event.currentTarget) return !(p.end && (p.at === null || p.at <= card.opened));
    if (p.repeat || p.raise || p.at === null || p.at <= card.opened) return false;
    return Boolean(p.from && card.dialog.contains(p.from));
  }

  function build(item, key) {
    const card = { id: item.item, key: key, json: JSON.stringify(item), li: el('li', 'hn-item', ''), dialog: null, answers: [], gone: false, busy: false, opened: 0 };
    const page = isPage(item);
    const box = page ? el('dialog', 'hn-dialog', '') : card.li;
    if (item.title) {
      const title = el('h3', 'hn-title', item.title);
      title.id = fresh();
      box.appendChild(title);
    }
    const about = el('div', null, '');
    about.id = fresh();
    if (item.text) about.appendChild(el('p', 'hn-text', item.text));
    addFields(about, item.fields);
    box.appendChild(about);
    addParams(box, item, card.answers);
    const controls = el('div', 'hn-controls', '');
    item.choices.forEach(function (c) {
      const b = el('button', 'hn-button', c.text);
      b.type = 'button';
      b.setAttribute('data-act', c.choice);
      b.setAttribute('data-param', item.item);
      b.addEventListener('click', function (event) {
        if (card.dialog && (event.detail > 1 || !freshFor(card, event))) {
          card.dialog.focus();
          return;
        }
        if (event.detail > 1) return;
        if (!card.dialog && event.detail > 0 && !pointedOn(b)) return;
        answer(item, c, card, event);
      });
      controls.appendChild(b);
    });
    box.appendChild(controls);
    if (page) {
      box.setAttribute('role', 'alertdialog');
      box.setAttribute('tabindex', '-1');
      box.autofocus = true;
      box.setAttribute('aria-modal', 'true');
      if (item.title) box.setAttribute('aria-labelledby', box.firstChild.id);
      box.setAttribute('aria-describedby', about.id);
      box.addEventListener('cancel', function (event) {
        event.preventDefault();
      });
      box.addEventListener('close', function () {
        if (!card.gone && box.isConnected) modal(card);
        else release();
      });
      card.dialog = box;
      card.li.appendChild(box);
    }
    return card;
  }

  function modal(card) {
    card.opened = tick;
    opened.add(card.id);
    empty();
    card.dialog.showModal();
    card.dialog.scrollTop = 0;
    card.dialog.focus();
  }

  function drop(key) {
    const card = cards.get(key);
    if (!card) return;
    card.gone = true;
    if (card.dialog && card.dialog.open) {
      closing = true;
      card.dialog.close();
    }
    card.li.remove();
    cards.delete(key);
  }

  function openNext() {
    let waiting = null;
    for (const card of cards.values()) {
      if (card.dialog && card.dialog.open) return;
      if (card.dialog && !waiting) waiting = card;
    }
    if (waiting) modal(waiting);
  }

  function apply(snapshot, newer) {
    const items = snapshot.items.filter(function (item) {
      return !answered.has(item.item);
    });
    const ids = new Set(snapshot.items.map(function (item) {
      return item.item;
    }));
    answered.forEach(function (id) {
      if (!ids.has(id)) answered.delete(id);
    });
    const seen = new Map();
    const keys = items.map(function (item) {
      const n = (seen.get(item.item) || 0) + 1;
      seen.set(item.item, n);
      return item.item + '#' + n;
    });
    const now = new Map();
    items.forEach(function (item, i) {
      now.set(keys[i], JSON.stringify(item));
    });
    Array.from(cards.keys()).forEach(function (key) {
      if (now.get(key) !== cards.get(key).json) drop(key);
    });
    const next = new Map();
    let ref = list.firstChild;
    items.forEach(function (item, i) {
      let card = cards.get(keys[i]);
      if (card && card.li === ref) {
        if (newer) card.busy = false;
        ref = ref.nextSibling;
      } else {
        if (card) drop(keys[i]);
        card = build(item, keys[i]);
        list.insertBefore(card.li, ref);
      }
      next.set(keys[i], card);
    });
    cards = next;
    openNext();
  }

  function cardsOf(id) {
    return Array.from(cards.entries())
      .filter(function (entry) {
        return entry[0].slice(0, entry[0].lastIndexOf('#')) === id;
      })
      .map(function (entry) {
        return entry[1];
      });
  }

  function holdsFocus(id) {
    return cardsOf(id).some(function (card) {
      return card.li.contains(document.activeElement);
    });
  }

  function empty() {
    if (!region) return;
    while (region.firstChild) region.removeChild(region.firstChild);
  }

  function addition(sentences) {
    empty();
    sentences.forEach(function (text) {
      if (text) region.appendChild(el('p', null, text));
    });
  }

  function pageWaiting() {
    return Array.from(cards.values()).some(function (card) {
      return card.dialog !== null;
    });
  }

  function holding() {
    return holdBase !== null || closing || pageWaiting();
  }

  function tell(text, line) {
    if (!region) return;
    if (holding()) line.held = true;
    else addition([text]);
  }

  function heldLines() {
    const out = [];
    allLines().forEach(function (l) {
      const text = l.held ? l.text() : '';
      if (text && out.indexOf(text) < 0) out.push(text);
    });
    return out;
  }

  function firsts(items) {
    const byId = new Map();
    items.forEach(function (item) {
      if (!byId.has(item.item)) byId.set(item.item, item);
    });
    return byId;
  }

  function release() {
    if (pageWaiting()) return;
    closing = false;
    const out = heldLines();
    allLines().forEach(function (l) {
      l.held = false;
    });
    if (!holdBase) {
      if (out.length > 0) addition(out);
      return;
    }
    const base = holdBase;
    holdBase = null;
    const now = last ? firsts(last.items) : new Map();
    base.forEach(function (item, id) {
      if (!now.has(id) && !(isPage(item) && answeredGone.has(id))) out.push(item.say_gone);
    });
    heldGone.forEach(function (text) {
      out.push(text);
    });
    now.forEach(function (item, id) {
      if (!base.has(id) && !isPage(item) && !holdsFocus(id)) out.push(item.say_new);
    });
    heldGone.clear();
    answeredGone.clear();
    addition(out);
  }

  function take(reply) {
    if (!reply || !Array.isArray(reply.items)) return;
    const before = last ? firsts(last.items) : null;
    const after = firsts(reply.items);
    const answeredNow = new Set(answered);
    seq = reply.seq;
    last = reply;
    apply(reply, true);
    const gone = before ? Array.from(before.entries()).filter(function (e) {
      return !after.has(e[0]);
    }) : [];
    const came = before ? Array.from(after.entries()).filter(function (e) {
      return !before.has(e[0]);
    }) : [];
    if (holding()) {
      if (!holdBase) holdBase = before || after;
      gone.forEach(function (e) {
        if (!isPage(e[1])) return;
        if (answeredNow.has(e[0])) answeredGone.add(e[0]);
        else if (!holdBase.has(e[0]) && opened.has(e[0])) heldGone.set(e[0], e[1].say_gone);
      });
      if (gone.length + came.length > 0) empty();
      return;
    }
    if (gone.length + came.length === 0) return;
    if (came.length === 0 && gone.every(function (e) {
      return isPage(e[1]) && answeredNow.has(e[0]);
    })) return;
    const out = [];
    gone.forEach(function (e) {
      if (!(isPage(e[1]) && answeredNow.has(e[0]))) out.push(e[1].say_gone);
    });
    came.forEach(function (e) {
      if (!isPage(e[1]) && !holdsFocus(e[0])) out.push(e[1].say_new);
    });
    addition(out);
  }

  function wait(mine) {
    call('agents_next', { after: seq }).then(function (reply) {
      if (mine !== turn || !reply || reply.closed === 'yes' || reply.error) return;
      if (reply.seq > seq) take(reply);
      wait(mine);
    });
  }

  Halvo.views.add({
    id: 'needs',
    order: 10,
    heading: "What needs you",
    mount: function (element) {
      root = element;
      const style = document.createElement('style');
      style.appendChild(document.createTextNode(CSS.join('')));
      element.appendChild(style);
      list = el('ul', 'hn-items', '');
      element.appendChild(list);
      ['keydown', 'keyup', 'pointerdown', 'pointerup', 'pointercancel'].forEach(function (type) {
        window.addEventListener(type, input, true);
      });
      window.addEventListener('focus', raised);
    },
    show: function () {
      if (shown) return;
      shown = true;
      if (!region) {
        region = el('div', 'hn-say', '');
        region.setAttribute('role', 'status');
        region.setAttribute('aria-live', 'polite');
        region.setAttribute('aria-atomic', 'true');
        root.insertBefore(region, list);
      }
      turn += 1;
      const mine = turn;
      call('agents_snapshot', {}).then(function (reply) {
        if (mine !== turn) return;
        take(reply);
        wait(mine);
      });
    },
    hide: function () {
      shown = false;
      turn += 1;
      empty();
    }
  });
})();
