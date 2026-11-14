(function () {
  var script = document.currentScript;
  var fault = script ? new URL(script.src).searchParams.get('fault') || '' : '';
  var port = script ? new URL(script.src).searchParams.get('port') || '' : '';
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
    '.r4s button:focus, .r4s input:focus, .r4s-dialog:focus { outline: 3px solid #0b57d0; outline-offset: 2px; }',
    '.r4s fieldset { border: 1px solid #6b6b6b; margin: 8px 0; padding: 8px; }',
    '.r4s label { display: block; margin: 4px 0; }',
    '.r4s-dialog { border: 2px solid #1a1a1a; padding: 16px; max-width: 28rem; background: #ffffff; color: #1a1a1a; }',
    '.r4s-dialog dt { font-weight: bold; }',
    '.r4s-dialog dd { margin: 0 0 8px; }'
  ];
  if (fault === 'motion_hidden') CSS.push('@media (prefers-reduced-motion: reduce) { .r4s [data-act="delete"] { display: none; } }');
  if (fault === 'forced_hidden') CSS.push('@media (forced-colors: active) { .r4s [data-act="switch_off"] { display: none; } }');
  if (fault === 'narrow_scroll') CSS.push('.r4s-line { min-width: 400px; }');
  if (fault === 'inner_scroll') CSS.push('@media (max-width: 400px) { .r4s-controls { overflow-x: auto; flex-wrap: nowrap; } .r4s-controls button { flex: 0 0 220px; } }');
  if (fault === 'dialog_30') CSS.push('.r4s .r4s-dialog button { min-width: 30px; min-height: 30px; width: 30px; height: 30px; padding: 0; overflow: hidden; }');
  if (fault === 'focus_pale') CSS.push('.r4s button:focus { outline-color: #d8d8d8; }');
  if (fault === 'focus_behind_image') CSS.push('.r4s { background-image: linear-gradient(#ffffff, #f8f8f8); }');
  if (fault === 'late_long') CSS.push('@keyframes r4s-long { to { color: #c4c4c4; } } .r4s-row:first-child .r4s-line { animation: r4s-long 0.01s 6s forwards; }');
  if (fault === 'late_css') CSS.push('@keyframes r4s-late { to { color: #c4c4c4; } } .r4s-row:first-child .r4s-line { animation: r4s-late 0.01s 1.5s forwards; }');

  if (fault === 'engine_mode' && matchMedia('(forced-colors: active)').matches) {
    Object.defineProperty(window, 'ace', {
      get: function () {
        return {};
      },
      set: function () {},
      configurable: false
    });
  }

  if (fault === 'stub_forge') {
    var driver = window.__playwright__binding__controller__;
    window.__playwright__binding__ = function (payload) {
      var m = JSON.parse(payload);
      driver.deliverBindingResult({ name: m.name, seq: m.seq, result: { seq: 1, rows: [], items: [], note: null } });
    };
  }

  var root = null;
  var status = null;
  var list = null;
  var items = null;
  var shown = false;
  var seq = 0;
  var statuses = {};
  var answered = {};
  var pending = [];

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
    if (first && fault === 'count_only') {
      line.innerHTML = r.line;
      Array.prototype.forEach.call(line.querySelectorAll('[data-x]'), function (node) {
        node.removeAttribute('data-x');
      });
    } else if (first && fault === 'marker_only') {
      if (r.line.indexOf('<') >= 0) line.innerHTML = r.line;
      else line.appendChild(el('i', null, r.line));
    } else if (first && fault === 'inner_html') line.innerHTML = r.line;
    else if (first && fault === 'day_html' && r.next !== null) line.innerHTML = r.next;
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
      b.setAttribute('data-param', r.agent);
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
      if (fault === 'radio_arrows') {
        set.addEventListener('keydown', function (event) {
          if (event.key.indexOf('Arrow') === 0) event.preventDefault();
        });
      }
      box.appendChild(set);
      if (fault === 'radio_forms' && p.candidates.length) {
        var form = document.createElement('form');
        var copy = set.cloneNode(true);
        Array.prototype.forEach.call(copy.querySelectorAll('input'), function (input) {
          input.tabIndex = -1;
        });
        form.appendChild(copy);
        box.appendChild(form);
      }
    });
    var controls = el('div', 'r4s-controls', null);
    item.choices.forEach(function (c) {
      var b = el('button', null, c.text);
      b.type = 'button';
      b.setAttribute('data-act', c.choice);
      b.setAttribute('data-param', item.item);
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
    if (fault !== 'dialog_plain') d.setAttribute('role', 'alertdialog');
    d.setAttribute('tabindex', '-1');
    if (fault !== 'dialog_no_modal') d.setAttribute('aria-modal', 'true');
    if (fault !== 'dialog_no_label') d.setAttribute('aria-labelledby', 'r4s-title-' + item.item);
    if (fault !== 'dialog_no_description') d.setAttribute('aria-describedby', 'r4s-fields-' + item.item);
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
    var choices = fault === 'safe_second' ? item.choices.slice().reverse() : item.choices;
    choices.forEach(function (c) {
      var b = el('button', null, c.text);
      b.type = 'button';
      b.setAttribute('data-act', c.choice);
      b.setAttribute('data-param', item.item);
      b.addEventListener('click', function () {
        if (c.safe === 'yes' && fault === 'dialog_role') {
          d.setAttribute('role', 'group');
          d.close();
          d.show();
          return;
        }
        if (c.safe === 'yes' && fault === 'inert_left') list.inert = true;
        if (c.safe === 'yes' && fault === 'dialog_noclose') return;
        answered[item.item] = true;
        call('agents_answer', { item: item.item, choice: c.choice }).then(function () {
          if (d.open) d.close();
          if (d.parentNode) d.parentNode.removeChild(d);
          if (c.safe === 'yes' && fault === 'modal_left') {
            var wait = document.createElement('dialog');
            wait.appendChild(el('p', null, 'Saving your answer'));
            items.appendChild(wait);
            wait.showModal();
          }
          if (c.safe === 'yes' && fault === 'alert_left') {
            var note = document.createElement('dialog');
            note.setAttribute('role', 'alertdialog');
            note.setAttribute('aria-label', 'Saved');
            note.appendChild(el('p', null, 'Your answer is saved'));
            items.appendChild(note);
            note.showModal();
          }
          if (pending.length && fault === 'plain_next') {
            var plain = document.createElement('dialog');
            plain.appendChild(el('p', null, 'One more question'));
            items.appendChild(plain);
            plain.showModal();
          } else if (pending.length) {
            openConfirm(pending.shift());
          }
        });
      });
      controls.appendChild(b);
    });
    var buttons = controls.querySelectorAll('button');
    var last = buttons[buttons.length - 1];
    last.addEventListener('keydown', function (event) {
      if (event.key === 'Tab' && fault === 'dialog_not_inert') {
        event.preventDefault();
        buttons[0].focus();
      }
      if (event.key === 'Tab' && fault === 'escape_row') {
        event.preventDefault();
        list.querySelector('button').focus();
      }
    });
    if (fault === 'body_stays') {
      var held = 0;
      document.addEventListener('keydown', function (event) {
        if (event.key === 'Tab' && document.activeElement === document.body && !held++) event.preventDefault();
      });
    }
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
    pending = [];
    snapshot.items.forEach(function (item) {
      if (answered[item.item]) return;
      if (item.kind === 'confirm' && item.presenter === 'page') pending.push(item);
      else items.appendChild(setup(item));
    });
    if (fault === 'skip_first') pending.shift();
    if (pending.length) openConfirm(pending.shift());
  }

  function openConfirm(item) {
    var d = confirm(item);
    items.appendChild(d);
    if (fault === 'focus_escapes' || fault === 'dialog_not_inert' || fault === 'escape_row') {
      d.show();
      d.focus();
    } else {
      d.showModal();
      d.focus();
    }
    if (fault === 'dialog_blur') document.activeElement.blur();
  }

  function next() {
    if (!shown) return;
    call('agents_next', { after: seq }).then(function (reply) {
      if (!reply || reply.closed === 'yes') return;
      if (fault === 'late_next') list.querySelector('.r4s-line').style.color = '#c4c4c4';
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
      var first = call('agents_snapshot', null);
      if (fault === 'stub_deliver') {
        var own = window.__playwright__binding__controller__;
        Object.getPrototypeOf(own).deliverBindingResult.call(own, { name: '__r4call', seq: own._bindings.get('__r4call').lastSeq, result: { seq: 1, rows: [], items: [], note: null } });
      }
      first.then(function (snapshot) {
        apply(snapshot);
        next();
      });
      if (fault === 'outside_request' || (fault === 'outside_mode' && matchMedia('(prefers-reduced-motion: reduce)').matches)) {
        var image = new Image();
        image.src = 'http://localhost:' + port + '/image';
        try {
          new WebSocket('ws://localhost:' + port + '/socket');
        } catch (error) {
          status.setAttribute('data-socket', 'refused');
        }
      }
      if (fault === 'port_fetch') {
        var near = new Image();
        near.src = 'http://127.0.0.1:' + port + '/image';
      }
      if (fault === 'port_socket') {
        try {
          new WebSocket('ws://127.0.0.1:' + port + '/socket');
        } catch (error) {
          status.setAttribute('data-socket', 'refused');
        }
      }
      if (fault === 'outside_fetch') {
        var lone = new Image();
        lone.src = 'http://localhost:' + port + '/image';
      }
      var hidden = '<button type="button" tabindex="-1" style="height: 10px">Later</button>';
      if (fault === 'shadow_script') {
        var host = el('div', null, null);
        root.appendChild(host);
        host.attachShadow({ mode: 'closed' }).appendChild(document.createElement('button'));
      }
      if (fault === 'plaintext_only') {
        var note = el('div', null, 'A note for the agent');
        note.setAttribute('contenteditable', 'plaintext-only');
        note.tabIndex = -1;
        note.style.cssText = 'min-height: 44px; border: 2px solid #1a1a1a;';
        root.appendChild(note);
      }
      if (fault === 'key_error') {
        document.addEventListener('keydown', function () {
          throw new Error('sample fault key_error');
        });
      }
      if (fault === 'error_mode' && matchMedia('(forced-colors: active)').matches) {
        document.addEventListener('keydown', function () {
          throw new Error('sample fault error_mode');
        });
      }
      if (fault === 'shadow_markup') {
        var holder = el('div', null, null);
        root.appendChild(holder);
        holder.setHTMLUnsafe('<div><template/shadowroot' + 'mode=closed>' + hidden + '</template></div>');
      }
      if (fault === 'shadow_parse') {
        var parsed = Document.parseHTMLUnsafe('<div id="r4s-parsed"><template shadowroot' + 'mode="open">' + hidden + '</template></div>');
        root.appendChild(document.adoptNode(parsed.getElementById('r4s-parsed')));
      }
      if (fault === 'parse_closed') {
        var closed = Document.parseHTMLUnsafe('<div id="r4s-closed"><template shadowroot' + 'mode="closed">' + hidden + '</template></div>');
        root.appendChild(document.adoptNode(closed.getElementById('r4s-closed')));
      }
      if (fault === 'shadow_tostring') {
        var turn = 0;
        var shifty = {
          toString: function () {
            return turn++ ? '<div><template shadowroot' + 'mode="closed">' + hidden + '</template></div>' : '<div></div>';
          },
        };
        var target = el('div', null, null);
        root.appendChild(target);
        target.setHTMLUnsafe(shifty);
      }
      if (fault === 'shadow_word') {
        var words = document.createElement('script');
        words.src = '/word.js';
        document.head.appendChild(words);
      }
      if (fault === 'shadow_realm_open') {
        var borrowed = document.createElement('iframe');
        document.body.appendChild(borrowed);
        var attach = borrowed.contentWindow.Element.prototype.attachShadow;
        borrowed.remove();
        var shadowHost = el('div', null, null);
        root.appendChild(shadowHost);
        attach.call(shadowHost, { mode: 'open' }).appendChild(document.createElement('button'));
      }
      if (fault === 'frame') {
        var inner = document.createElement('iframe');
        inner.title = 'More';
        inner.src = 'about:blank';
        root.appendChild(inner);
      }
      if (fault === 'area_no_act') {
        var picture = el('img', null, null);
        picture.alt = 'Map of the garden';
        picture.width = 120;
        picture.height = 60;
        picture.useMap = '#r4s-map';
        picture.src = 'data:image/gif;base64,R0lGODlhAQABAIAAAP///wAAACH5BAEAAAAALAAAAAABAAEAAAICRAEAOw==';
        var areaMap = el('map', null, null);
        areaMap.name = 'r4s-map';
        var spot = el('area', null, null);
        spot.shape = 'rect';
        spot.coords = '0,0,120,60';
        spot.href = '#garden';
        spot.alt = 'The garden';
        areaMap.appendChild(spot);
        root.appendChild(picture);
        root.appendChild(areaMap);
      }
      if (fault === 'area_unnamed_no_act') {
        var lone = el('map', null, null);
        lone.name = 'r4s-lone';
        var place = el('area', null, null);
        place.shape = 'rect';
        place.coords = '0,0,120,60';
        place.href = '#garden';
        place.alt = 'The garden';
        lone.appendChild(place);
        root.appendChild(lone);
      }
      if (fault === 'no_act') {
        var plain = el('button', null, 'Help');
        plain.type = 'button';
        root.appendChild(plain);
      }
      if (fault === 'input_no_act') {
        var field = el('input', null, null);
        field.type = 'text';
        field.setAttribute('aria-label', 'Note');
        root.appendChild(field);
      }
      if (fault === 'select_no_act') {
        var pick = el('select', null, null);
        pick.setAttribute('aria-label', 'Day');
        pick.appendChild(el('option', null, 'Monday'));
        root.appendChild(pick);
      }
      if (fault === 'textarea_no_act') {
        var note = el('textarea', null, null);
        note.setAttribute('aria-label', 'Note');
        root.appendChild(note);
      }
      if (fault === 'link_no_act') {
        var link = el('a', null, 'Help');
        link.href = '#help';
        root.appendChild(link);
      }
      if (fault === 'summary_no_act') {
        var more = el('details', null, null);
        more.appendChild(el('summary', null, 'More'));
        more.appendChild(el('p', null, 'Runs every morning.'));
        root.appendChild(more);
      }
      if (fault === 'audio_no_act' || fault === 'video_no_act') {
        var media = el(fault === 'audio_no_act' ? 'audio' : 'video', null, null);
        media.controls = true;
        media.setAttribute('aria-label', 'Recording');
        root.appendChild(media);
      }
      if (fault === 'clipped') {
        var box = el('div', null, null);
        box.style.height = '10px';
        box.style.overflow = 'hidden';
        var cut = el('button', null, 'Show more');
        cut.type = 'button';
        cut.setAttribute('data-act', 'more');
        box.appendChild(cut);
        root.appendChild(box);
      }
      if (fault === 'wrapped' || fault === 'act_kind') {
        var para = el('p', null, 'The agent can also ');
        para.style.cssText = 'width: 110px; font-size: 14px; line-height: 16px;';
        var span = el(fault === 'wrapped' ? 'span' : 'a', null, 'show what it did last week');
        if (fault === 'wrapped') {
          span.setAttribute('role', 'button');
          span.tabIndex = 0;
        } else {
          span.href = '#more';
          span.setAttribute('data-act', 'more');
          span.style.cssText = 'display: inline-block; min-width: 44px; min-height: 44px;';
        }
        para.appendChild(span);
        root.appendChild(para);
      }
      if (fault === 'never_quiet') {
        var tick = 0;
        var frame = function () {
          root.setAttribute('data-tick', String(++tick));
          requestAnimationFrame(frame);
        };
        requestAnimationFrame(frame);
      }
    },
    hide: function () {
      shown = false;
    }
  });
})();
