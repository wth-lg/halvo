(function () {
  'use strict';
  window.requestAnimationFrame = function (fn) {
    return setTimeout(function () {
      fn(Date.now());
    }, 1500);
  };
})();
