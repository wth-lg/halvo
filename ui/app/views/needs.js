(function () {
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
  let pointedAt = null;
  let say = null;
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

  function limit(typed, full, tooLong, hintId, notTaken) {
    const line = { kept: '', composing: false, ended: false, held: false, stored: false, field: typed, sent: null, text: null };
    let mark = false;
    let shown = null;
    let note = null;
    let before = null;
    let start = null;
    let refused = notTaken || '';
    function textOf(which) {
      return which === 'full' ? full : which === 'too_long' ? tooLong : which === 'not_taken' ? refused : '';
    }
    function rest() {
      return refused ? 'not_taken' : null;
    }
    function describe() {
      const ids = [hintId, note ? note.id : ''].filter(function (id) {
        return id;
      });
      if (ids.length > 0) typed.setAttribute('aria-describedby', ids.join(' '));
      else typed.removeAttribute('aria-describedby');
    }
    function show(which) {
      if (shown === which) return;
      shown = which;
      if (note) {
        note.remove();
        note = null;
      }
      const text = textOf(which);
      if (text) {
        note = el('p', 'hn-full', text);
        note.id = fresh();
        typed.parentNode.insertBefore(note, typed.nextSibling);
      }
      describe();
      if (text) tell(text, line);
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
        show(rest());
      }
      return true;
    }
    line.sent = function () {
      if (shown === 'too_long') show(rest());
    };
    line.restore = function (text, stored) {
      typed.value = text;
      line.kept = text;
      line.stored = stored;
    };
    line.text = function () {
      return textOf(shown);
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
    describe();
    show(rest());
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

  function addParams(box, item, answers, carry) {
    item.params.forEach(function (p) {
      const set = el('fieldset', 'hn-param', '');
      const legend = el('legend', null, p.label);
      legend.id = fresh();
      const group = fresh();
      set.appendChild(legend);
      let hint = null;
      if (p.hint) {
        hint = el('p', 'hn-hint', p.hint);
        hint.id = fresh();
        set.appendChild(hint);
      }
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
        typed.setAttribute('autocomplete', 'off');
        typed.setAttribute('autocorrect', 'off');
        typed.setAttribute('autocapitalize', 'off');
        typed.setAttribute('spellcheck', 'false');
        set.appendChild(typed);
        line = limit(typed, p.full, p.too_long, hint ? hint.id : '', p.not_taken);
        const was = carry && carry.step === item.step ? carry.lines.get(p.name) : null;
        if (was) line.restore(was.text, was.stored && !p.not_taken);
      }
      box.appendChild(set);
      answers.push({ param: p.name, radios: radios, typed: typed, line: line });
    });
  }

  function chosen(item, answers) {
    const first = [];
    const calls = [];
    answers.forEach(function (a) {
      const picked = a.radios.filter(function (r) {
        return r.input.checked;
      })[0];
      if (picked) {
        calls.push(['agents_answer', { item: item.item, step: item.step, choice: 'choose', param: a.param, n: picked.n }]);
      } else if (a.line && a.line.kept.trim() !== '') {
        (a.line.stored ? first : calls).push(['agents_setup_typed', { item: item.item, step: item.step, param: a.param, text: a.line.kept }]);
      }
    });
    return first.concat(calls);
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
      const emptied = card.answers.filter(function (a) {
        return a.line && a.line.stored && a.line.kept.trim() === '';
      })[0];
      if (emptied) {
        emptied.line.field.focus();
        return;
      }
      const calls = chosen(item, card.answers);
      if (calls.length === 0) return;
      const key = card.key;
      const went = [];
      held(card, calls.reduce(function (done, c) {
        return done.then(function (reply) {
          if (reply === null || reply.error) return reply;
          return call(c[0], c[1]).then(function (r) {
            if (r && !r.error && c[0] === 'agents_setup_typed') went.push(c[1].param);
            return r;
          });
        });
      }, Promise.resolve({})).then(function (reply) {
        settle(key, item, went);
        return reply;
      }));
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
    held(card, call('agents_answer', { item: item.item, step: item.step, choice: choice.choice })).then(function (reply) {
      if ((!reply || reply.error) && answered.delete(item.item) && last) apply(last);
    });
  }

  function settle(key, item, went) {
    const card = cards.get(key);
    if (!card || card.item.step !== item.step) return;
    card.answers.forEach(function (a) {
      if (!a.line || went.indexOf(a.param) < 0) return;
      const entry = card.item.params.filter(function (p) {
        return p.name === a.param;
      })[0];
      a.line.stored = !(entry && entry.not_taken);
    });
  }

  function kept(card) {
    const lines = new Map();
    card.answers.forEach(function (a) {
      if (a.line) lines.set(a.param, { text: a.line.kept, stored: a.line.stored });
    });
    return { step: card.step, lines: lines };
  }

  function input(event) {
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
    } else if (type === 'pointerdown') {
      pointedAt = controlName(event.target);
    }
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

  function asHeld(item) {
    return JSON.stringify(item, function (key, value) {
      return this === item && key === 'armed' ? undefined : value;
    });
  }

  function build(item, key, carry) {
    const card = { id: item.item, item: item, step: item.step, key: key, json: asHeld(item), li: el('li', 'hn-item', ''), dialog: null, handle: null, answers: [], gone: false, busy: false };
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
    addParams(box, item, card.answers, carry);
    const controls = el('div', 'hn-controls', '');
    const buttons = [];
    item.choices.forEach(function (c) {
      const b = el('button', 'hn-button', c.text);
      b.type = 'button';
      b.setAttribute('data-act', c.choice);
      b.setAttribute('data-param', item.item);
      b.addEventListener('click', function (event) {
        if (card.handle && !Halvo.confirm.takes(card.handle, event)) return;
        if (event.detail > 1) return;
        if (!card.dialog && event.detail > 0 && !pointedOn(b)) return;
        answer(item, c, card, event);
      });
      controls.appendChild(b);
      buttons.push(b);
    });
    box.appendChild(controls);
    if (page) {
      card.handle = Halvo.confirm.dress(box, item.title ? box.firstChild.id : null, about.id, release);
      card.handle.again = function () {
        modal(card);
      };
      card.handle.hold = buttons;
      card.dialog = box;
      card.li.appendChild(box);
    }
    return card;
  }

  function modal(card) {
    opened.add(card.id);
    empty();
    if (Halvo.confirm.show(card.handle)) report(card);
  }

  function report(card) {
    if (shown) call('agents_shown', { item: card.id, step: card.step });
  }

  function drop(key) {
    const card = cards.get(key);
    if (!card) return;
    card.gone = true;
    if (card.dialog && card.dialog.open) closing = true;
    if (card.handle) Halvo.confirm.drop(card.handle);
    card.li.remove();
    cards.delete(key);
  }

  function openNext() {
    if (Halvo.confirm.busy()) return;
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
      now.set(keys[i], asHeld(item));
    });
    const carried = new Map();
    Array.from(cards.keys()).forEach(function (key) {
      if (now.get(key) === cards.get(key).json) return;
      if (now.has(key)) carried.set(key, kept(cards.get(key)));
      drop(key);
    });
    const next = new Map();
    let ref = list.firstChild;
    items.forEach(function (item, i) {
      let card = cards.get(keys[i]);
      if (card && card.li === ref) {
        if (newer) card.busy = false;
        ref = ref.nextSibling;
      } else {
        const carry = card ? kept(card) : carried.get(keys[i]);
        if (card) drop(keys[i]);
        card = build(item, keys[i], carry);
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
    if (!say) return;
    say.empty();
  }

  function addition(sentences) {
    if (!say) return;
    say.add(sentences);
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
      if (typeof reply.seq === 'number' && reply.seq !== seq) take(reply);
      wait(mine);
    });
  }

  Halvo.views.add({
    id: 'needs',
    order: 10,
    heading: "What needs you",
    mount: function (element) {
      root = element;
      list = el('ul', 'hn-items', '');
      element.appendChild(list);
      ['keydown', 'keyup', 'pointerdown', 'pointerup', 'pointercancel'].forEach(function (type) {
        window.addEventListener(type, input, true);
      });
      Halvo.confirm.whenFree(function () {
        openNext();
        release();
      });
    },
    show: function () {
      if (shown) return;
      shown = true;
      cards.forEach(function (card) {
        if (card.dialog && card.dialog.open) report(card);
      });
      if (!region) {
        say = Halvo.confirm.region('hn-say');
        region = say.element;
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
