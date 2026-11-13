(function () {
  "use strict";
  var Halvo = window.Halvo || (window.Halvo = {});
  var replied = {};

  /**
   * With the global API off, the call goes through window.__TAURI_INTERNALS__,
   * which Tauri injects into every page it loads.
   */
  Halvo.ipc = {
    call: function (name, args) {
      return window.__TAURI_INTERNALS__.invoke(name, args).then(function (reply) {
        replied[name] = true;
        return reply;
      });
    },
    replied: function () {
      return Object.keys(replied);
    },
  };
})();
