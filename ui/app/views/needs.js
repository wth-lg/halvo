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
    '.hn-dialog::backdrop{background-color:rgba(0,0,0,0.4)}'
  ];

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

  const letters = new Intl.Segmenter(undefined, { granularity: 'grapheme' });

  function bytes(text) {
    let size = 0;
    for (const ch of text) {
      const code = ch.codePointAt(0);
      size += code < 128 ? 1 : code < 2048 ? 2 : code < 65536 ? 3 : 4;
    }
    return size;
  }

  function bounded(text) {
    let size = 0;
    let out = '';
    for (const part of letters.segment(text)) {
      size += bytes(part.segment);
      if (size > 1024) break;
      out += part.segment;
    }
    return out;
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
      if (p.typed === 'yes') {
        typed = document.createElement('input');
        typed.type = 'text';
        typed.className = 'hn-typed';
        typed.setAttribute('data-act', 'typed');
        typed.setAttribute('data-param', p.name);
        typed.setAttribute('aria-labelledby', legend.id);
        typed.addEventListener('input', function () {
          const kept = bounded(typed.value);
          if (kept !== typed.value) typed.value = kept;
        });
        set.appendChild(typed);
      }
      box.appendChild(set);
      answers.push({ param: p.name, radios: radios, typed: typed });
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
      } else if (a.typed && a.typed.value.trim() !== '') {
        calls.push(['agents_setup_typed', { item: item.item, param: a.param, text: bounded(a.typed.value) }]);
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

  function answer(item, choice, card) {
    if (card.busy) return;
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
    const card = { key: key, json: JSON.stringify(item), li: el('li', 'hn-item', ''), dialog: null, answers: [], gone: false, busy: false, opened: 0 };
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
        answer(item, c, card);
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
      });
      card.dialog = box;
      card.li.appendChild(box);
    }
    return card;
  }

  function modal(card) {
    card.opened = tick;
    card.dialog.showModal();
    card.dialog.scrollTop = 0;
    card.dialog.focus();
  }

  function drop(key) {
    const card = cards.get(key);
    if (!card) return;
    card.gone = true;
    if (card.dialog && card.dialog.open) card.dialog.close();
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

  function take(reply) {
    if (!reply || !Array.isArray(reply.items)) return;
    seq = reply.seq;
    last = reply;
    apply(reply, true);
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
    }
  });
})();
