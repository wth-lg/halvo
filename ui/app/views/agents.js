(function () {
  var ORDER = ['needs_you', 'problem', 'working', 'waiting_turn', 'all_set', 'off', 'not_yet'];
  var CONTROL = { run_now: true, stop: true, take_back: true, switch_off: true, switch_on: true, stop_sign_in: true, delete: true };
  var ASKED = { delete: 'ask', stop_sign_in: 'ask_stop_sign_in' };
  var CARD_TRIES = 3;

  var root = null;
  var status = null;
  var say = null;
  var held = [];
  var asks = null;
  var question = null;
  var note = null;
  var groups = null;
  var shown = false;
  var turn = 0;
  var reads = 0;
  var brought = 0;
  var seq = 0;
  var snapshot = null;
  var statuses = {};
  var card = null;
  var again = null;
  var asking = 0;
  var busy = {};
  var answers = [];
  var pointerFrom = null;
  var keyFrom = null;
  var SPACE = String.fromCharCode(32);
  var ASKS = { reject: true, allow: true, stop: true, done: true };

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

  function keep(stays) {
    var before = answers.length;
    answers = answers.filter(function (a) {
      if (stays(a)) return true;
      if (a.said) a.said.remove();
      return false;
    });
    return answers.length !== before;
  }

  function drop(agent) {
    return keep(function (a) {
      return a.agent !== agent;
    });
  }

  function answer(agent, act, text, rereads) {
    var row = null;
    if (snapshot !== null) {
      snapshot.rows.forEach(function (r) {
        if (r.agent === agent) row = r;
      });
    }
    if (row === null) return;
    drop(agent);
    var said = null;
    if (Halvo.confirm.busy()) {
      held = held.concat([text]);
    } else {
      say.add([text]);
      said = status.firstChild;
    }
    answers.push({ agent: agent, act: act, line: rereads ? null : row.line, text: text, said: said });
    render();
  }

  function press(row, act, name, args, rereads) {
    var drawn = turn;
    if (drop(row.agent)) render();
    return send(row.agent, name, args).then(function (reply) {
      if (drawn === turn && reply && typeof reply.error === 'string' && isText(reply.text) && reply.text !== '') answer(row.agent, act, reply.text, rereads);
      return reply;
    });
  }

  function beside(b, row) {
    var a = null;
    answers.forEach(function (x) {
      if (x.agent === row.agent && x.act === b.getAttribute('data-act')) a = x;
    });
    if (a === null) return null;
    var p = el('p', 'r4a-answer', a.text);
    p.id = 'r4a-answer-' + row.agent;
    var by = b.getAttribute('aria-describedby');
    b.setAttribute('aria-describedby', by ? by + SPACE + p.id : p.id);
    return p;
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
      if (ASKED[c.control]) {
        if (drop(row.agent)) render();
        ask(row, c.control);
        return;
      }
      press(row, c.control, 'agents_control', { agent: row.agent, action: c.control }, true).then(reread);
    });
    return b;
  }

  function isAsk(value) {
    return (
      value !== null &&
      typeof value === 'object' &&
      isText(value.title) &&
      listOf(value.text, isText) &&
      listOf(value.effects, function (e) {
        return (
          e !== null &&
          isText(e.text) &&
          listOf(e.fields, function (f) {
            return f !== null && isText(f.label) && isText(f.from_text) && listOf(f.value, isText);
          })
        );
      }) &&
      (value.record === null || isText(value.record)) &&
      listOf(value.controls, function (c) {
        return c !== null && isText(c.text) && ASKS[c.act] === true;
      })
    );
  }

  function ask(row, control) {
    var key = ASKED[control];
    var asked = row[key];
    if (question !== null || Halvo.confirm.busy() || !isAsk(asked)) return;
    var agent = row.agent;
    var mine = { agent: agent, key: key, handle: null };
    question = mine;
    say.empty();
    mine.handle = Halvo.confirm.ask(asks, {
      title: asked.title,
      text: asked.text,
      effects: asked.effects,
      record: asked.record,
      controls: asked.controls,
      param: agent,
      answer: function (act) {
        if (act === 'allow' && control === 'stop_sign_in') press(row, control, 'agents_control', { agent: agent, action: control }, true).then(reread);
        else if (act === 'allow') send(agent, 'agents_delete', { agent: agent }).then(reread);
        else if (act === 'stop') send(agent, 'agents_control', { agent: agent, action: 'stop' }).then(reread);
      },
      closed: function () {
        if (question === mine) question = null;
      }
    });
  }

  function release() {
    if (Halvo.confirm.busy() || held.length === 0) return;
    var out = held;
    held = [];
    say.add(out);
  }

  function reread() {
    if (!shown) return;
    reads += 1;
    next({ life: turn, read: reads }, 0);
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
    if (c.history === 'busy' && (again === null || (again.left < 1 && !again.flying))) {
      box.appendChild(el('p', 'r4a-unread', c.unread.text));
      var read = button('read_card', row.agent, c.unread.again);
      read.addEventListener('click', function (event) {
        if (event.detail > 1 || !pressedOn(read, event) || busy[row.agent]) return;
        askCard(row, 'read_card');
      });
      box.appendChild(read);
      var told = beside(read, row);
      if (told) box.appendChild(told);
    }
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
      (reply.history !== 'busy' || (reply.unread !== null && typeof reply.unread === 'object' && isText(reply.unread.text) && isText(reply.unread.again))) &&
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
        drop(row.agent);
        asking += 1;
        card = null;
        again = null;
        render();
        return;
      }
      if (busy[row.agent]) return;
      askCard(row, 'open_card');
    });
    return b;
  }

  function askCard(row, act) {
    asking += 1;
    var mine = asking;
    var asked = turn;
    press(row, act, 'agents_card', { agent: row.agent }, false).then(function (reply) {
      if (mine !== asking || asked !== turn || !isCard(reply, row.agent)) return;
      card = reply;
      again = reply.history === 'busy' ? { agent: row.agent, left: CARD_TRIES } : null;
      render();
    });
  }

  function askAgain() {
    if (again === null || again.flying || card === null || card.agent !== again.agent || again.left < 1 || busy[again.agent]) return;
    again.left -= 1;
    again.flying = true;
    var mine = again;
    var agent = again.agent;
    var asked = turn;
    call('agents_card', { agent: agent })
      .then(
        function (reply) {
          return reply;
        },
        function () {
          return null;
        }
      )
      .then(function (reply) {
        mine.flying = false;
        if (mine !== again || asked !== turn || card === null || card.agent !== agent) return;
        if (!isCard(reply, agent)) {
          again = null;
          render();
          return;
        }
        card = reply;
        if (reply.history !== 'busy') again = null;
        render();
      });
  }

  function rowOf(row) {
    var li = el('li', 'r4a-row', null);
    var name = el('h4', 'r4a-name', null);
    var t = toggle(row);
    name.appendChild(t);
    li.appendChild(name);
    var named = beside(t, row);
    if (named) li.appendChild(named);
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
    var after = null;
    row.controls.forEach(function (c) {
      if (!CONTROL[c.control]) return;
      var b = control(row, c);
      controls.appendChild(b);
      after = after || beside(b, row);
    });
    li.appendChild(controls);
    if (after) li.appendChild(after);
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
      if (statuses[r.agent] && statuses[r.agent] !== r.status && ORDER.indexOf(r.status) >= 0 && isText(r.say) && r.say !== '') says.push(r.say);
    });
    statuses = {};
    reply.rows.forEach(function (r) {
      statuses[r.agent] = r.status;
    });
    seq = reply.seq;
    snapshot = reply;
    busy = {};
    keep(function (a) {
      var row = null;
      reply.rows.forEach(function (r) {
        if (r.agent === a.agent) row = r;
      });
      if (row === null) return false;
      if (a.line === null) a.line = row.line;
      return row.line === a.line;
    });
    if (question !== null) {
      var asked = question;
      var still = reply.rows.some(function (r) {
        return r.agent === asked.agent && isAsk(r[asked.key]);
      });
      if (!still) Halvo.confirm.drop(asked.handle);
    }
    if (says.length > 0) {
      if (Halvo.confirm.busy()) held = held.concat(says);
      else say.add(says);
    }
    render();
  }

  function next(mine, after) {
    call('agents_next', { after: after })
      .then(function (reply) {
        if (mine.life !== turn || !shown || !reply || reply.closed === 'yes' || typeof reply.seq !== 'number') return;
        if (mine.read !== reads) {
          if (brought < reads && reply.seq !== seq) take(reply);
          return;
        }
        brought = reads;
        if (reply.seq !== seq) take(reply);
        askAgain();
        next(mine, seq);
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
      say = Halvo.confirm.region('r4a-say');
      status = say.element;
      root.appendChild(status);
      note = el('div', null, null);
      root.appendChild(note);
      asks = el('div', null, null);
      root.appendChild(asks);
      Halvo.confirm.whenFree(release);
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
      var dropped = keep(function () {
        return false;
      });
      if (dropped) render();
      var mine = turn;
      call('agents_snapshot', {})
        .then(function (reply) {
          if (mine !== turn || !shown) return;
          brought = reads;
          take(reply);
          next({ life: mine, read: reads }, seq);
        })
        .catch(function () {});
    },
    hide: function () {
      shown = false;
      turn += 1;
      held = [];
      if (say) say.empty();
    }
  });
})();
