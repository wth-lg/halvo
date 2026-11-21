(function () {
  const CSS = [
    '.hc-dialog{border-width:2px;border-style:solid;border-color:#1a1a1a;padding:16px;max-width:28rem;background-color:#ffffff;color:#1a1a1a;overflow-wrap:anywhere}',
    '.hc-dialog:focus{outline-width:3px;outline-style:solid;outline-color:#0b57d0;outline-offset:2px}',
    '.hc-dialog::backdrop{background-color:rgba(0,0,0,0.4)}',
    '.hc-title{font-size:1.1rem;margin-top:0;margin-bottom:8px}',
    '.hc-text{margin-top:4px;margin-bottom:8px}',
    '.hc-did{font-weight:bold;margin-top:8px;margin-bottom:4px}',
    '.hc-fields{margin-top:4px;margin-bottom:8px}',
    '.hc-label{font-weight:bold}',
    '.hc-value{margin-left:0;white-space:pre-wrap;overflow-wrap:anywhere}',
    '.hc-from{margin-left:0;margin-bottom:8px;font-style:italic}',
    '.hc-controls{display:flex;flex-wrap:wrap;gap:8px;margin-top:8px}',
    '.hc-button{-webkit-appearance:none;appearance:none;min-width:44px;min-height:44px;padding:8px;border-width:2px;border-style:solid;border-color:#1a1a1a;border-radius:4px;background-color:#f2f2f2;color:#1a1a1a;font:inherit;overflow-wrap:anywhere}',
    '.hc-button:focus{outline-width:3px;outline-style:solid;outline-color:#0b57d0;outline-offset:2px}'
  ];

  let tick = 0;
  let press = null;
  let pointer = null;
  let raising = false;
  const keys = new Map();
  let shown = null;
  let serial = 0;
  let styled = false;
  const free = [];

  function fresh() {
    serial += 1;
    return 'hc-' + serial;
  }

  function el(tag, cls, text) {
    const node = document.createElement(tag);
    if (cls) node.className = cls;
    if (text) node.appendChild(document.createTextNode(text));
    return node;
  }

  function input(event) {
    tick += 1;
    const type = event.type;
    if (type === 'keydown') {
      if (!event.repeat) keys.set(event.key, { at: tick, target: event.target });
      press = { target: event.target, repeat: event.repeat, at: event.repeat ? null : tick, from: event.target, raise: false, end: false };
    } else if (type === 'keyup') {
      const down = keys.get(event.key) || null;
      keys.delete(event.key);
      press = { target: event.target, repeat: false, at: down ? down.at : null, from: down ? down.target : null, raise: false, end: true };
    } else if (type === 'pointerdown') {
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

  ['keydown', 'keyup', 'pointerdown', 'pointerup', 'pointercancel'].forEach(function (type) {
    window.addEventListener(type, input, true);
  });
  window.addEventListener('focus', raised);

  function freshFor(handle, event) {
    const p = press;
    press = null;
    if (!p) return true;
    if (p.target !== event.currentTarget) return !(p.end && (p.at === null || p.at <= handle.opened));
    if (p.repeat || p.raise || p.at === null || p.at <= handle.opened) return false;
    return Boolean(p.from && handle.dialog.contains(p.from));
  }

  function open() {
    return shown !== null && shown.dialog.open;
  }

  function settle() {
    if (open()) return;
    for (let i = 0; i < free.length; i++) {
      free[i]();
      if (open()) return;
    }
  }

  function dress(box, labelledby, describedby, closed) {
    const handle = { dialog: box, opened: 0, gone: false, closed: closed, before: null, again: null };
    box.setAttribute('role', 'alertdialog');
    box.setAttribute('tabindex', '-1');
    box.autofocus = true;
    box.setAttribute('aria-modal', 'true');
    if (labelledby) box.setAttribute('aria-labelledby', labelledby);
    if (describedby) box.setAttribute('aria-describedby', describedby);
    box.addEventListener('cancel', function (event) {
      event.preventDefault();
    });
    box.addEventListener('close', function () {
      if (!handle.gone && box.isConnected) {
        if (handle.again) handle.again();
        else show(handle);
        return;
      }
      if (shown === handle) shown = null;
      if (handle.closed) handle.closed();
      settle();
    });
    return handle;
  }

  function show(handle) {
    if (shown !== null && shown !== handle && shown.dialog.open) return false;
    if (handle.before === null) handle.before = document.activeElement;
    shown = handle;
    handle.opened = tick;
    handle.dialog.showModal();
    handle.dialog.scrollTop = 0;
    handle.dialog.focus();
    return true;
  }

  function drop(handle) {
    handle.gone = true;
    if (handle.dialog.open) handle.dialog.close();
  }

  function takes(handle, event) {
    if (event.detail > 1 || !freshFor(handle, event)) {
      handle.dialog.focus();
      return false;
    }
    return true;
  }

  function region(cls) {
    const node = el('div', cls, '');
    node.setAttribute('role', 'status');
    node.setAttribute('aria-live', 'polite');
    node.setAttribute('aria-atomic', 'true');
    function empty() {
      while (node.firstChild) node.removeChild(node.firstChild);
    }
    return {
      element: node,
      empty: empty,
      add: function (sentences) {
        empty();
        sentences.forEach(function (text) {
          if (typeof text === 'string' && text !== '') node.appendChild(el('p', null, text));
        });
      }
    };
  }

  function style() {
    if (styled) return;
    styled = true;
    const node = document.createElement('style');
    node.appendChild(document.createTextNode(CSS.join('')));
    document.head.appendChild(node);
  }

  function fields(box, list) {
    if (list.length === 0) return;
    const dl = el('dl', 'hc-fields', '');
    list.forEach(function (f) {
      dl.appendChild(el('dt', 'hc-label', f.label));
      const dd = el('dd', 'hc-value', '');
      f.value.forEach(function (piece) {
        dd.appendChild(document.createTextNode(piece));
      });
      dl.appendChild(dd);
      if (f.from_text) dl.appendChild(el('dd', 'hc-from', f.from_text));
    });
    box.appendChild(dl);
  }

  function ask(parent, o) {
    style();
    const box = el('dialog', 'hc-dialog', '');
    const title = el('h3', 'hc-title', o.title);
    title.id = fresh();
    box.appendChild(title);
    const about = el('div', null, '');
    about.id = fresh();
    o.text.forEach(function (t) {
      about.appendChild(el('p', 'hc-text', t));
    });
    o.effects.forEach(function (e) {
      about.appendChild(el('p', 'hc-did', e.text));
      fields(about, e.fields);
    });
    if (typeof o.record === 'string' && o.record !== '') about.appendChild(el('p', 'hc-text', o.record));
    box.appendChild(about);
    const controls = el('div', 'hc-controls', '');
    let handle = null;
    o.controls.forEach(function (c) {
      const b = el('button', 'hc-button', c.text);
      b.type = 'button';
      b.setAttribute('data-act', c.act);
      b.setAttribute('data-param', o.param);
      b.addEventListener('click', function (event) {
        if (!handle || handle.gone || !takes(handle, event)) return;
        drop(handle);
        o.answer(c.act);
      });
      controls.appendChild(b);
    });
    box.appendChild(controls);
    parent.appendChild(box);
    handle = dress(box, title.id, about.id, function () {
      box.remove();
      const back = handle.before;
      if (back && back.isConnected && !open()) back.focus();
      if (o.closed) o.closed();
    });
    show(handle);
    return handle;
  }

  window.Halvo = window.Halvo || {};
  window.Halvo.confirm = {
    busy: open,
    dress: dress,
    show: show,
    drop: drop,
    takes: takes,
    region: region,
    ask: ask,
    whenFree: function (fn) {
      free.push(fn);
    }
  };
})();
