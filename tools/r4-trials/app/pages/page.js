(function () {
  'use strict';
  var app = window.r4app;
  var counts = app.counts;
  var interval = null;
  var loop = null;
  var hidden = false;
  var windowHidden = false;
  var viewHidden = false;
  var way = null;
  var waiting = false;
  var closedSeen = false;
  var STALL_TICKS = 150;
  var quietTicks = 0;
  var sync = { last: 0, order: 'ok', afterHide: 0, hiddenTicks: 0, mark: 0, ended: false };

  function tick() {
    counts.ticks += 1;
    if (hidden) {
      sync.hiddenTicks += 1;
    }
    if (way !== null && !sync.ended) {
      quietTicks += 1;
      if (quietTicks === STALL_TICKS) {
        app.report('sync stalled last=' + sync.last + ' order=' + sync.order +
          ' after_hide=' + sync.afterHide + ' hidden_ticks=' + sync.hiddenTicks);
      }
    }
  }

  function frame() {
    counts.frames += 1;
    loop = requestAnimationFrame(frame);
  }

  function start() {
    if (interval === null) {
      interval = setInterval(tick, 100);
    }
    if (loop === null) {
      loop = requestAnimationFrame(frame);
    }
  }

  function stop() {
    if (interval !== null) {
      clearInterval(interval);
      interval = null;
    }
    if (loop !== null) {
      cancelAnimationFrame(loop);
      loop = null;
    }
  }

  function accept(change) {
    quietTicks = 0;
    if (change.seq <= sync.last) {
      if (sync.order === 'ok') {
        app.report('sync order_miss seq=' + change.seq + ' last=' + sync.last);
      }
      sync.order = 'miss';
    } else {
      sync.last = change.seq;
    }
    if (change.hidden === true) {
      sync.afterHide += 1;
    }
    var mark = Math.floor(sync.last / 10000) * 10000;
    if (mark > sync.mark) {
      sync.mark = mark;
      app.report('sync mark=' + mark);
    }
    if (sync.ended) {
      app.report('sync after_end seq=' + change.seq + ' last=' + sync.last + ' order=' + sync.order);
    } else if (change.seq === change.of) {
      sync.ended = true;
      app.report('sync end last=' + sync.last + ' order=' + sync.order +
        ' after_hide=' + sync.afterHide + ' hidden_ticks=' + sync.hiddenTicks);
    }
  }

  function setHidden() {
    hidden = windowHidden || viewHidden;
  }

  function getNext() {
    if (way !== 'get' || waiting) {
      return;
    }
    if (hidden && !(windowHidden && !closedSeen)) {
      return;
    }
    waiting = true;
    app.call('next', { seq: sync.last }).then(function (reply) {
      waiting = false;
      if (reply.closed === 'yes') {
        closedSeen = true;
        app.report('sync closed last=' + sync.last);
        return;
      }
      accept(reply);
      getNext();
    }, function () {
      waiting = false;
    });
  }

  function startSync(name) {
    way = name;
    if (way === 'get') {
      getNext();
    } else if (way === 'channel') {
      app.call('subscribe', { channel: app.channel(accept) });
    } else if (way === 'events') {
      app.listen('r4-change', accept);
    }
  }

  window.addEventListener('r4-window', function (event) {
    if (event.detail === 'hide') {
      windowHidden = true;
      setHidden();
      stop();
      getNext();
    } else if (event.detail === 'show') {
      var seen = windowHidden;
      windowHidden = false;
      closedSeen = false;
      setHidden();
      start();
      app.report('window shown after_hide=' + sync.afterHide + ' hidden_ticks=' + sync.hiddenTicks +
        ' hide_seen=' + (seen ? 'yes' : 'no'));
      getNext();
    }
  });

  document.addEventListener('visibilitychange', function () {
    viewHidden = document.visibilityState === 'hidden';
    setHidden();
    getNext();
  });

  document.addEventListener('DOMContentLoaded', function () {
    var count = document.getElementById('count');
    document.getElementById('act').addEventListener('click', function () {
      app.call('bump').then(function (n) {
        count.textContent = String(n);
      });
    });
    start();
    app.call('config').then(function (config) {
      if (config.sync) {
        startSync(config.sync);
      }
    });
  });
})();
