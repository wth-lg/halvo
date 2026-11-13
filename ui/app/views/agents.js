(function () {
  var ORDER = ['needs_you', 'problem', 'working', 'waiting_turn', 'all_set', 'off', 'not_yet'];
  var CONTROL = { run_now: true, stop: true, switch_off: true, switch_on: true };
  var CSS = [
    '.r4a{overflow-wrap:anywhere}',
    '.r4a-group{margin-top:16px}',
    '.r4a-group>h3{font-size:1.1rem;margin-top:0;margin-bottom:8px}',
    '.r4a-list{list-style:none;margin:0;padding:0}',
    '.r4a-row{padding-top:8px;padding-bottom:8px;border-bottom-width:1px;border-bottom-style:solid;border-bottom-color:#6b6b6b}',
    '.r4a-name{margin:0;font-size:1rem}',
    '.r4a-status,.r4a-level,.r4a-line,.r4a-next,.r4a-note,.r4a-did{margin-top:4px;margin-bottom:4px}',
    '.r4a-word{font-weight:bold}',
    '.r4a-icon{display:inline-block;width:10px;height:10px;margin-right:8px;border-radius:50%;border-width:2px;border-style:solid;border-color:#1a1a1a;vertical-align:middle}',
    '.r4a-icon-needs_you{background-color:#b3261e}',
    '.r4a-icon-problem{background-color:#8c1d18;border-radius:0}',
    '.r4a-icon-working{background-color:#0b57d0}',
    '.r4a-icon-waiting_turn{background-color:#ffffff}',
    '.r4a-icon-all_set{background-color:#146c2e}',
    '.r4a-icon-off{background-color:#6b6b6b;border-radius:0}',
    '.r4a-icon-not_yet{background-color:#ffffff;border-style:dashed}',
    '.r4a-controls{display:flex;flex-wrap:wrap;gap:8px;margin-top:8px}',
    '.r4a-button{-webkit-appearance:none;appearance:none;min-width:44px;min-height:44px;padding:8px;border-width:2px;border-style:solid;border-color:#1a1a1a;border-radius:4px;background-color:#f2f2f2;color:#1a1a1a;font:inherit;text-align:left;overflow-wrap:anywhere}',
    '.r4a-name>.r4a-button{width:100%;font-weight:bold}',
    '.r4a-button:focus{outline-width:3px;outline-style:solid;outline-color:#0b57d0;outline-offset:2px}',
    '.r4a-card{margin-top:8px;padding-left:12px;border-left-width:4px;border-left-style:solid;border-left-color:#6b6b6b}',
    '.r4a-lines,.r4a-runs,.r4a-effects{margin-top:4px;margin-bottom:4px;padding-left:16px}',
    '.r4a-day{font-weight:bold;margin-top:8px;margin-bottom:0}',
    '.r4a-ran{margin-top:0;margin-bottom:4px}',
    '.r4a-fields{margin-top:4px;margin-bottom:4px}',
    '.r4a-fields>div{margin-bottom:4px}',
    '.r4a-fields>div>dt{font-weight:bold}',
    '.r4a-fields>div>dd{margin-left:0}',
    '.r4a-from{display:block;font-size:0.875rem}'
  ];

  var root = null;
  var status = null;
  var note = null;
  var groups = null;
  var shown = false;
  var turn = 0;
  var seq = 0;
  var snapshot = null;
  var statuses = {};
  var card = null;
  var asking = 0;
  var busy = {};
  var pointerFrom = null;
  var keyFrom = null;
  var SPACE = String.fromCharCode(32);

  function el(tag, cls, text) {
    var node = document.createElement(tag);
    if (cls) node.className = cls;
    if (typeof text === 'string') node.appendChild(document.createTextNode(text));
    return node;
  }

  function clear(node) {
    while (node.firstChild) node.removeChild(node.firstChild);
  }

  function call(name, args) {
    return Halvo.ipc.call(name, args);
  }

  function send(agent, name, args) {
    var mark = {};
    busy[agent] = mark;
    var free = function () {
      if (busy[agent] === mark) delete busy[agent];
    };
    return call(name, args).then(
      function (reply) {
        free();
        return reply;
      },
      function () {
        free();
        return null;
      }
    );
  }

  function controlOf(node) {
    var mine = node && node.tagName === 'BUTTON' && node.hasAttribute('data-act') && groups.contains(node);
    return mine ? node.getAttribute('data-act') + ':' + node.getAttribute('data-param') : '';
  }

  function input(event) {
    if (event.type === 'pointerdown') {
      pointerFrom = controlOf(event.target);
    } else if (event.type === 'keydown' && !event.repeat && (event.key === 'Enter' || event.key === SPACE)) {
      keyFrom = controlOf(event.target);
    }
  }

  function pressedOn(b, event) {
    var own = b.getAttribute('data-act') + ':' + b.getAttribute('data-param');
    if (event.detail > 0) {
      var began = pointerFrom;
      pointerFrom = null;
      return began === own;
    }
    var key = keyFrom;
    keyFrom = null;
    return key === null || key === '' || key === own;
  }

  function button(act, param, text) {
    var b = el('button', 'r4a-button', text);
    b.type = 'button';
    b.setAttribute('data-act', act);
    b.setAttribute('data-param', param);
    return b;
  }

  function find(act, param) {
    var all = groups.querySelectorAll('button');
    for (var i = 0; i < all.length; i++) {
      if (all[i].getAttribute('data-act') === act && all[i].getAttribute('data-param') === param) return all[i];
    }
    return null;
  }

  function control(row, c) {
    var b = button(c.control, row.agent, c.text);
    b.setAttribute('aria-describedby', 'r4a-name-' + row.agent);
    b.addEventListener('click', function (event) {
      if (event.detail > 1 || !pressedOn(b, event) || busy[row.agent]) return;
      send(row.agent, 'agents_control', { agent: row.agent, action: c.control });
    });
    return b;
  }

  function fields(list) {
    var dl = el('dl', 'r4a-fields', null);
    list.forEach(function (f) {
      var pair = el('div', null, null);
      pair.appendChild(el('dt', null, f.label));
      var dd = el('dd', null, null);
      var value = el('span', 'r4a-value', null);
      f.value.forEach(function (piece) {
        value.appendChild(document.createTextNode(piece));
      });
      dd.appendChild(value);
      dd.appendChild(el('span', 'r4a-from', f.from_text));
      pair.appendChild(dd);
      dl.appendChild(pair);
    });
    return dl;
  }

  function cardOf(row, c) {
    var box = el('div', 'r4a-card', null);
    box.id = 'r4a-card-' + row.agent;
    box.setAttribute('role', 'group');
    box.setAttribute('aria-label', c.name);
    var lines = el('ul', 'r4a-lines', null);
    c.lines.forEach(function (l) {
      lines.appendChild(el('li', null, l.text));
    });
    box.appendChild(lines);
    if (typeof row.next === 'string' && row.next !== '') box.appendChild(el('p', 'r4a-next', row.next));
    var runs = el('ul', 'r4a-runs', null);
    c.runs.forEach(function (run) {
      var item = el('li', null, null);
      item.appendChild(el('p', 'r4a-day', run.day));
      item.appendChild(el('p', 'r4a-ran', run.text));
      if (run.effects.length > 0) {
        var effects = el('ul', 'r4a-effects', null);
        run.effects.forEach(function (e) {
          var entry = el('li', null, null);
          entry.appendChild(el('p', 'r4a-did', e.text));
          entry.appendChild(fields(e.fields));
          effects.appendChild(entry);
        });
        item.appendChild(effects);
      }
      runs.appendChild(item);
    });
    box.appendChild(runs);
    return box;
  }

  function listOf(value, each) {
    return Array.isArray(value) && value.every(each);
  }

  function isText(value) {
    return typeof value === 'string';
  }

  function isCard(reply, agent) {
    return (
      reply !== null &&
      typeof reply === 'object' &&
      reply.agent === agent &&
      isText(reply.name) &&
      listOf(reply.lines, function (l) {
        return l !== null && isText(l.text);
      }) &&
      listOf(reply.runs, function (run) {
        return (
          run !== null &&
          isText(run.day) &&
          isText(run.text) &&
          listOf(run.effects, function (e) {
            return (
              e !== null &&
              isText(e.text) &&
              listOf(e.fields, function (f) {
                return f !== null && isText(f.label) && isText(f.from_text) && listOf(f.value, isText);
              })
            );
          })
        );
      })
    );
  }

  function toggle(row) {
    var open = card !== null && card.agent === row.agent;
    var b = button(open ? 'close_card' : 'open_card', row.agent, row.name);
    b.id = 'r4a-name-' + row.agent;
    b.setAttribute('aria-expanded', open ? 'true' : 'false');
    if (open) b.setAttribute('aria-controls', 'r4a-card-' + row.agent);
    b.addEventListener('click', function (event) {
      if (event.detail > 1 || !pressedOn(b, event)) return;
      if (card !== null && card.agent === row.agent) {
        asking += 1;
        card = null;
        render();
        return;
      }
      if (busy[row.agent]) return;
      asking += 1;
      var mine = asking;
      var asked = turn;
      send(row.agent, 'agents_card', { agent: row.agent }).then(function (reply) {
        if (mine !== asking || asked !== turn || !isCard(reply, row.agent)) return;
        card = reply;
        render();
      });
    });
    return b;
  }

  function rowOf(row) {
    var li = el('li', 'r4a-row', null);
    var name = el('h4', 'r4a-name', null);
    name.appendChild(toggle(row));
    li.appendChild(name);
    var word = el('p', 'r4a-status', null);
    var icon = el('span', 'r4a-icon', null);
    icon.classList.add('r4a-icon-' + row.status);
    icon.setAttribute('aria-hidden', 'true');
    word.appendChild(icon);
    word.appendChild(el('span', 'r4a-word', row.word));
    li.appendChild(word);
    li.appendChild(el('p', 'r4a-level', row.level));
    li.appendChild(el('p', 'r4a-line', row.line));
    var controls = el('div', 'r4a-controls', null);
    row.controls.forEach(function (c) {
      if (CONTROL[c.control]) controls.appendChild(control(row, c));
    });
    li.appendChild(controls);
    if (card !== null && card.agent === row.agent) li.appendChild(cardOf(row, card));
    return li;
  }

  function render() {
    var active = document.activeElement;
    var was = null;
    if (active && groups.contains(active) && active.hasAttribute('data-act')) {
      was = { act: active.getAttribute('data-act'), param: active.getAttribute('data-param') };
    }
    clear(groups);
    clear(note);
    if (snapshot === null) return;
    if (typeof snapshot.note === 'string' && snapshot.note !== '') note.appendChild(el('p', 'r4a-note', snapshot.note));
    if (card !== null && !snapshot.rows.some(function (r) { return r.agent === card.agent; })) card = null;
    ORDER.forEach(function (key) {
      var rows = snapshot.rows.filter(function (r) { return r.status === key; });
      if (rows.length === 0) return;
      var group = el('section', 'r4a-group', null);
      group.setAttribute('data-status', key);
      var heading = el('h3', null, rows[0].word);
      heading.id = 'r4a-group-' + key;
      group.setAttribute('aria-labelledby', heading.id);
      group.appendChild(heading);
      var list = el('ul', 'r4a-list', null);
      rows.forEach(function (r) {
        list.appendChild(rowOf(r));
      });
      group.appendChild(list);
      groups.appendChild(group);
    });
    if (was !== null) {
      var back = find(was.act, was.param) || find('open_card', was.param) || find('close_card', was.param);
      if (back) back.focus();
    }
  }

  function take(reply) {
    if (!reply || !Array.isArray(reply.rows) || typeof reply.seq !== 'number') return;
    var says = [];
    reply.rows.forEach(function (r) {
      if (statuses[r.agent] && statuses[r.agent] !== r.status && ORDER.indexOf(r.status) >= 0) says.push(r.say);
    });
    statuses = {};
    reply.rows.forEach(function (r) {
      statuses[r.agent] = r.status;
    });
    seq = reply.seq;
    snapshot = reply;
    busy = {};
    says.forEach(function (s) {
      status.appendChild(el('p', null, s));
    });
    render();
  }

  function next(mine) {
    call('agents_next', { after: seq })
      .then(function (reply) {
        if (mine !== turn || !shown || !reply || reply.closed === 'yes' || typeof reply.seq !== 'number') return;
        if (reply.seq > seq) take(reply);
        next(mine);
      })
      .catch(function () {});
  }

  Halvo.views.add({
    id: 'agents',
    order: 20,
    heading: "Your agents",
    mount: function (element) {
      root = element;
      root.className = 'r4a';
      var style = document.createElement('style');
      style.appendChild(document.createTextNode(CSS.join('')));
      root.appendChild(style);
      status = el('div', 'r4a-say', null);
      status.setAttribute('role', 'status');
      root.appendChild(status);
      note = el('div', null, null);
      root.appendChild(note);
      groups = el('div', null, null);
      root.appendChild(groups);
      root.addEventListener('keydown', function (event) {
        if (event.repeat && event.key === 'Enter' && event.target.tagName === 'BUTTON') event.preventDefault();
      });
      ['keydown', 'pointerdown'].forEach(function (type) {
        window.addEventListener(type, input, true);
      });
    },
    show: function () {
      if (shown) return;
      shown = true;
      turn += 1;
      var mine = turn;
      call('agents_snapshot', {})
        .then(function (reply) {
          if (mine !== turn || !shown) return;
          take(reply);
          next(mine);
        })
        .catch(function () {});
    },
    hide: function () {
      shown = false;
      turn += 1;
    }
  });
})();
