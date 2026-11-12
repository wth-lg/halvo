(function () {
  "use strict";
  var Halvo = window.Halvo || (window.Halvo = {});

  /** Halvo.ipc.call(name, args): a promise of the reply of the command `name`, with no deadline of its own. */
  Halvo.ipc = {
    call: function (name, args) {
      return window.__TAURI__.core.invoke(name, args);
    },
  };
})();
