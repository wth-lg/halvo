(function () {
  'use strict';
  var internals = window.__TAURI_INTERNALS__;
  var counts = { ticks: 0, frames: 0 };

  function call(command, args) {
    return internals.invoke(command, args || {});
  }

  function report(line) {
    return call('report', { line: String(line) });
  }

  function listen(event, handler) {
    var id = internals.transformCallback(function (message) {
      handler(message.payload);
    });
    return call('plugin:event|listen', { event: event, target: { kind: 'Any' }, handler: id });
  }

  function channel(onmessage) {
    var next = 0;
    var pending = new Map();
    var id = internals.transformCallback(function (raw) {
      if ('end' in raw) {
        return;
      }
      pending.set(raw.index, raw.message);
      while (pending.has(next)) {
        var message = pending.get(next);
        pending.delete(next);
        next += 1;
        onmessage(message);
      }
    });
    return {
      toJSON: function () {
        return '__CHANNEL__:' + id;
      }
    };
  }

  window.__r4AppFrame = function (gen) {
    requestAnimationFrame(function () {
      call('frame', { gen: gen });
    });
  };

  window.__r4AppCounts = function (token) {
    call('counts', { token: token, ticks: counts.ticks, frames: counts.frames });
  };

  window.__r4Window = function (state) {
    window.dispatchEvent(new CustomEvent('r4-window', { detail: state }));
  };

  window.r4app = Object.freeze({
    call: call,
    report: report,
    listen: listen,
    channel: channel,
    counts: counts
  });
})();
