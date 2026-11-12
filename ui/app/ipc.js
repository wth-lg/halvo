(function () {
  "use strict";
  var Halvo = window.Halvo || (window.Halvo = {});

  /**
   * Halvo.ipc.call(name, args): a promise of the reply of the command `name`,
   * with no deadline of its own. With the global API off, the call goes through
   * window.__TAURI_INTERNALS__, which Tauri injects into every page it loads.
   */
  Halvo.ipc = {
    call: function (name, args) {
      return window.__TAURI_INTERNALS__.invoke(name, args);
    },
  };
})();
