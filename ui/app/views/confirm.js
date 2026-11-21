(function () {
  let tick = 0;
  let press = null;
  const pointers = new Map();
  let raising = false;
  const keys = new Map();
  let shown = null;
  let serial = 0;
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
      press = { target: event.target, repeat: event.repeat, at: event.repeat ? null : tick, from: event.target, raise: false, held: false, end: false };
    } else if (type === 'keyup') {
      const down = keys.get(event.key) || null;
      keys.delete(event.key);
      press = { target: event.target, repeat: false, at: down ? down.at : null, from: down ? down.target : null, raise: false, held: false, end: true };
    } else if (type === 'pointerdown') {
      const pointer = { at: tick, target: event.target, raise: raising, held: holding(event.target) };
      pointers.set(event.pointerId, pointer);
      raising = false;
      press = { target: event.target, repeat: false, at: tick, from: event.target, raise: pointer.raise, held: pointer.held, end: false };
    } else {
      const down = pointers.get(event.pointerId) || null;
      pointers.delete(event.pointerId);
      press = { target: event.target, repeat: false, at: down ? down.at : null, from: down ? down.target : null, raise: down ? down.raise : false, held: down ? down.held : false, end: true };
    }
  }

  // The hold of a control is the CSS animation hc-hold of confirm.css, which also draws its look.
  // The engine alone keeps its time: a pointer press refuses its click when the animation was
  // running at the press, read here at pointerdown, and nothing waits for the animation to end.
  function holding(node) {
    if (!node || typeof node.getAnimations !== 'function') return false;
    return node.getAnimations().some(function (a) {
      return a.animationName === 'hc-hold' && a.playState === 'running';
    });
  }

  // Starts the hold of the handle's controls anew: the class goes and comes back across the style
  // change that getAnimations() makes, so the engine starts the animation from its beginning.
  function hold(handle) {
    if (!handle.hold) return;
    handle.hold.forEach(function (control) {
      control.classList.remove('hc-hold');
      control.getAnimations();
      control.classList.add('hc-hold');
    });
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
    if (p.repeat || p.raise || p.held || p.at === null || p.at <= handle.opened) return false;
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
    const handle = { dialog: box, opened: 0, gone: false, closed: closed, again: null, hold: null };
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
    shown = handle;
    handle.opened = tick;
    hold(handle);
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
