(function () {
  "use strict";
  var text;

  Halvo.views.add({
    id: "keeps",
    order: 2,
    heading: "What Halvo keeps",
    mount: function (el) {
      text = Halvo.el("p", {
        id: "window-text",
        class: "muted",
        text: "Halvo keeps a rolling record of your work on this computer, encrypted, and deletes older capture automatically.",
      });
      el.append(
        text,
        Halvo.el("p", {
          class: "muted",
          text: "Screen, typing, mouse, the app in front and window titles are recorded. Microphone, camera and system audio are off and are never requested in this version. Passwords are never recorded.",
        }),
      );
      Halvo.feed("status").on(function (s) {
        text.textContent =
          "Halvo keeps the last " +
          s.window.rolling_days +
          " days of your work on this computer, encrypted. " +
          "Anything older is deleted automatically. Your first workflow report arrives after day " +
          s.window.first_report_day +
          ".";
      });
    },
    show: function () {},
    hide: function () {},
  });
})();
