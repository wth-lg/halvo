(function () {
  var script = document.currentScript;
  var fault = script ? new URL(script.src).searchParams.get('fault') || '' : '';
  var CSS = [
    '.r4s-list { list-style: none; margin: 0; padding: 0; }',
    '.r4s-row { border-bottom: 1px solid #6b6b6b; padding: 8px 0; }',
    '.r4s-word { font-weight: bold; margin-right: 8px; }',
    '.r4s-line { margin: 4px 0; }',
    '.r4s-controls { display: flex; flex-wrap: wrap; gap: 8px; }',
    '.r4s button { -webkit-appearance: none; appearance: none; min-width: 44px; min-height: 44px; padding: 8px 12px; border: 2px solid #1a1a1a; border-radius: 4px; background: #f2f2f2; color: #1a1a1a; font: inherit; }',
    '.r4s input[type="radio"] { -webkit-appearance: none; appearance: none; width: 24px; height: 24px; margin: 0 8px 0 0; border: 2px solid #1a1a1a; border-radius: 50%; background: #ffffff; vertical-align: middle; }',
    '.r4s input[type="radio"]:checked { border-width: 8px; }',
    '.r4s input[type="text"] { min-height: 32px; border: 2px solid #1a1a1a; border-radius: 4px; padding: 4px 8px; font: inherit; }',
    '.r4s button:focus, .r4s input:focus { outline: 3px solid #0b57d0; outline-offset: 2px; }',
    '.r4s fieldset { border: 1px solid #6b6b6b; margin: 8px 0; padding: 8px; }',
    '.r4s label { display: block; margin: 4px 0; }',
    '.r4s-dialog { border: 2px solid #1a1a1a; padding: 16px; max-width: 28rem; background: #ffffff; color: #1a1a1a; }',
    '.r4s-dialog dt { font-weight: bold; }',
    '.r4s-dialog dd { margin: 0 0 8px; }'
  ];
  if (fault === 'motion_hidden') CSS.push('@media (prefers-reduced-motion: reduce) { .r4s [data-act="delete"] { display: none; } }');
  if (fault === 'forced_hidden') CSS.push('@media (forced-colors: active) { .r4s [data-act="switch_off"] { display: none; } }');
  if (fault === 'narrow_scroll') CSS.push('.r4s-line { min-width: 400px; }');

  var root = null;
  var status = null;
  var list = null;
  var items = null;
  var shown = false;
  var seq = 0;
  var statuses = {};
  var answered = {};

  function el(tag, cls, text) {
    var node = document.createElement(tag);
    if (cls) node.className = cls;
    if (text !== undefined && text !== null) node.appendChild(document.createTextNode(text));
    return node;
  }

  function clear(node) {
    while (node.firstChild) node.removeChild(node.firstChild);
  }

  function call(name, args) {
    return Halvo.ipc.call(name, args);
  }

  function row(r, first) {
    var li = el('li', 'r4s-row');
    var head = el('p', null, null);
    head.appendChild(el('span', 'r4s-word', r.word));
    head.appendChild(document.createTextNode(' '));
    head.appendChild(document.createTextNode(r.name));
    li.appendChild(head);
    var line = el('p', 'r4s-line', null);
    if (first && fault === 'inner_html') line.innerHTML = r.line;
    else line.appendChild(document.createTextNode(r.line));
    if (first && fault === 'contrast') line.style.color = '#c4c4c4';
    li.appendChild(line);
    var controls = el('div', 'r4s-controls', null);
    r.controls.forEach(function (c, i) {
      var text = c.text;
      if (first && i === 0 && fault === 'renamed') text = text + ' today';
      var b = el('button', null, text);
      b.type = 'button';
      b.setAttribute('data-act', c.control);
      if (first && i === 0 && fault === 'unreachable') b.tabIndex = -1;
      if (first && i === 0 && fault === 'focus_unseen') b.style.setProperty('outline', 'none', 'important');
      if (first && c.control === 'delete' && fault === 'small') {
        b.style.minWidth = '0';
        b.style.minHeight = '0';
        b.style.padding = '0 2px';
        b.style.height = '20px';
        b.style.fontSize = '12px';
      }
      b.addEventListener('click', function () {
        call('agents_control', { agent: r.agent, action: c.control });
      });
      controls.appendChild(b);
    });
    li.appendChild(controls);
    return li;
  }

  function setup(item) {
    var box = el('div', 'r4s-item', null);
    box.appendChild(el('h3', null, item.title));
    box.appendChild(el('p', null, item.text));
    item.params.forEach(function (p) {
      var set = el('fieldset', null, null);
      set.appendChild(el('legend', null, p.label));
      p.candidates.forEach(function (c) {
        var label = el('label', null, null);
        var input = document.createElement('input');
        input.type = 'radio';
        input.name = 'r4s-' + item.item + '-' + p.name;
        input.setAttribute('data-act', 'candidate');
        input.setAttribute('data-param', p.name);
        input.setAttribute('data-n', String(c.n));
        input.addEventListener('change', function () {
          call('agents_answer', { item: item.item, choice: 'choose', param: p.name, n: c.n });
        });
        label.appendChild(input);
        label.appendChild(document.createTextNode(c.value.join(' ')));
        set.appendChild(label);
      });
      if (p.typed === 'yes') {
        var tl = el('label', null, 'Type it here');
        var t = document.createElement('input');
        t.type = 'text';
        t.setAttribute('data-act', 'typed');
        t.setAttribute('data-param', p.name);
        tl.appendChild(t);
        set.appendChild(tl);
      }
      box.appendChild(set);
    });
    var controls = el('div', 'r4s-controls', null);
    item.choices.forEach(function (c) {
      var b = el('button', null, c.text);
      b.type = 'button';
      b.setAttribute('data-act', c.choice);
      b.addEventListener('click', function () {
        call('agents_answer', { item: item.item, choice: c.choice });
      });
      controls.appendChild(b);
    });
    box.appendChild(controls);
    return box;
  }

  function confirm(item) {
    var d = document.createElement('dialog');
    d.className = 'r4s-dialog';
    d.setAttribute('role', 'alertdialog');
    d.setAttribute('aria-modal', 'true');
    d.setAttribute('aria-labelledby', 'r4s-title-' + item.item);
    d.setAttribute('aria-describedby', 'r4s-fields-' + item.item);
    var title = el('h3', null, item.title);
    title.id = 'r4s-title-' + item.item;
    d.appendChild(title);
    var fields = el('dl', null, null);
    fields.id = 'r4s-fields-' + item.item;
    item.fields.forEach(function (f) {
      var dt = el('dt', null, f.label);
      dt.appendChild(document.createTextNode(' (' ));
      dt.appendChild(document.createTextNode(f.from_text));
      dt.appendChild(document.createTextNode(')'));
      fields.appendChild(dt);
      var dd = el('dd', null, null);
      f.value.forEach(function (piece, i) {
        if (i > 0) dd.appendChild(document.createTextNode(' '));
        dd.appendChild(document.createTextNode(piece));
      });
      fields.appendChild(dd);
    });
    d.appendChild(fields);
    var controls = el('div', 'r4s-controls', null);
    item.choices.forEach(function (c) {
      var b = el('button', null, c.text);
      b.type = 'button';
      b.setAttribute('data-act', c.choice);
      if (c.safe === 'yes') b.autofocus = true;
      b.addEventListener('click', function () {
        answered[item.item] = true;
        call('agents_answer', { item: item.item, choice: c.choice }).then(function () {
          if (d.open) d.close();
          if (d.parentNode) d.parentNode.removeChild(d);
        });
      });
      controls.appendChild(b);
    });
    d.appendChild(controls);
    d.addEventListener('cancel', function (event) {
      event.preventDefault();
    });
    return d;
  }

  function apply(snapshot) {
    seq = snapshot.seq;
    clear(list);
    snapshot.rows.forEach(function (r, i) {
      if (statuses[r.agent] && statuses[r.agent] !== r.status) {
        clear(status);
        status.appendChild(document.createTextNode(r.say));
      }
      statuses[r.agent] = r.status;
      list.appendChild(row(r, i === 0));
    });
    clear(items);
    snapshot.items.forEach(function (item) {
      if (answered[item.item]) return;
      if (item.kind === 'confirm' && item.presenter === 'page') {
        var d = confirm(item);
        items.appendChild(d);
        if (fault === 'focus_escapes') {
          d.show();
          d.querySelector('button').focus();
        } else {
          d.showModal();
        }
      } else {
        items.appendChild(setup(item));
      }
    });
  }

  function next() {
    if (!shown) return;
    call('agents_next', { after: seq }).then(function (reply) {
      if (!reply || reply.closed === 'yes') return;
      if (reply.seq > seq) apply(reply);
      next();
    });
  }

  Halvo.views.add({
    id: 'sample',
    order: 10,
    heading: 'Your agents',
    mount: function (element) {
      root = element;
      root.className = 'r4s';
      var style = document.createElement('style');
      style.appendChild(document.createTextNode(CSS.join('\n')));
      root.appendChild(style);
      status = el('p', null, null);
      status.setAttribute('role', 'status');
      root.appendChild(status);
      list = el('ul', 'r4s-list', null);
      root.appendChild(list);
      items = el('div', null, null);
      root.appendChild(items);
    },
    show: function () {
      if (shown) return;
      shown = true;
      call('agents_snapshot', null).then(function (snapshot) {
        apply(snapshot);
        next();
      });
    },
    hide: function () {
      shown = false;
    }
  });
})();
