(function () {
  const CSS = [
    '.hn-items{list-style:none;margin:0;padding:0}',
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
    '.hn-dialog::backdrop{background-color:rgba(0,0,0,0.4)}'
  ];

  let list = null;
  let shown = false;
  let turn = 0;
  let seq = 0;
  let last = null;
  let cards = new Map();
  const answered = new Set();

  function el(tag, cls, text) {
    const node = document.createElement(tag);
    if (cls) node.className = cls;
    if (text) node.appendChild(document.createTextNode(text));
    return node;
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
      if (f.value.join('') === '') {
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
      legend.id = 'hn-param-' + item.item + '-' + p.name;
      set.appendChild(legend);
      const radios = [];
      p.candidates.forEach(function (c) {
        const label = el('label', 'hn-choice', '');
        const input = document.createElement('input');
        input.type = 'radio';
        input.className = 'hn-radio';
        input.name = 'hn-' + item.item + '-' + p.name;
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
        calls.push(['agents_setup_typed', { item: item.item, param: a.param, text: a.typed.value }]);
      }
    });
    return calls;
  }

  function answer(item, choice, card) {
    if (choice.choice === 'present') {
      call('agents_present', { item: item.item });
      return;
    }
    if (choice.choice === 'choose' && item.params.length > 0) {
      chosen(item, card.answers).reduce(function (done, c) {
        return done.then(function (reply) {
          return reply === null ? null : call(c[0], c[1]);
        });
      }, Promise.resolve({}));
      return;
    }
    if (isPage(item)) {
      answered.add(item.item);
      drop(item.item);
      openNext();
    }
    call('agents_answer', { item: item.item, choice: choice.choice }).then(function (reply) {
      if (reply && reply.error && answered.delete(item.item) && last) apply(last);
    });
  }

  function build(item) {
    const card = { json: JSON.stringify(item), li: el('li', 'hn-item', ''), dialog: null, answers: [], gone: false };
    const page = isPage(item);
    const box = page ? el('dialog', 'hn-dialog', '') : card.li;
    if (item.title) {
      const title = el('h3', 'hn-title', item.title);
      title.id = 'hn-title-' + item.item;
      box.appendChild(title);
    }
    const about = el('div', null, '');
    about.id = 'hn-about-' + item.item;
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
      if (page && c.safe === 'yes') b.autofocus = true;
      b.addEventListener('click', function () {
        answer(item, c, card);
      });
      controls.appendChild(b);
    });
    box.appendChild(controls);
    if (page) {
      box.setAttribute('role', 'alertdialog');
      box.setAttribute('aria-modal', 'true');
      if (item.title) box.setAttribute('aria-labelledby', 'hn-title-' + item.item);
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
    card.dialog.showModal();
    const safe = card.dialog.querySelector('[autofocus]');
    if (safe) safe.focus();
  }

  function drop(id) {
    const card = cards.get(id);
    if (!card) return;
    card.gone = true;
    if (card.dialog && card.dialog.open) card.dialog.close();
    card.li.remove();
    cards.delete(id);
  }

  function openNext() {
    let waiting = null;
    for (const card of cards.values()) {
      if (card.dialog && card.dialog.open) return;
      if (card.dialog && !waiting) waiting = card;
    }
    if (waiting) modal(waiting);
  }

  function apply(snapshot) {
    const items = snapshot.items.filter(function (item) {
      return !answered.has(item.item);
    });
    const ids = new Set(snapshot.items.map(function (item) {
      return item.item;
    }));
    answered.forEach(function (id) {
      if (!ids.has(id)) answered.delete(id);
    });
    const now = new Map();
    items.forEach(function (item) {
      now.set(item.item, JSON.stringify(item));
    });
    Array.from(cards.keys()).forEach(function (id) {
      if (now.get(id) !== cards.get(id).json) drop(id);
    });
    const next = new Map();
    let ref = list.firstChild;
    items.forEach(function (item) {
      let card = cards.get(item.item);
      if (card && card.li === ref) {
        ref = ref.nextSibling;
      } else {
        if (card) drop(item.item);
        card = build(item);
        list.insertBefore(card.li, ref);
      }
      next.set(item.item, card);
    });
    cards = next;
    openNext();
  }

  function take(reply) {
    if (!reply || !Array.isArray(reply.items)) return;
    seq = reply.seq;
    last = reply;
    apply(reply);
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
