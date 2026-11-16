(function () {
  "use strict";
  var w = Halvo.words;
  var text;
  var recorded;
  var others;

  function render(s) {
    var t = s.service === "running" ? s.texts : null;
    if (!t) {
      text.textContent = w.KEEPS_UNKNOWN;
      recorded.replaceChildren();
      others.replaceChildren();
      return;
    }
    text.textContent = w.KEEPS_WINDOW.replace("{days}", String(s.window.rolling_days));
    recorded.replaceChildren.apply(
      recorded,
      (t.recorded || []).map(function (r) {
        return Halvo.el("li", { text: r.text });
      }),
    );
    others.replaceChildren.apply(
      others,
      [t.passwords, t.typed_ahead, t.backup_limit, t.limits]
        .filter(function (line) {
          return typeof line === "string" && line !== "";
        })
        .map(function (line) {
          return Halvo.el("p", { class: "muted", text: line });
        }),
    );
  }

  Halvo.views.add({
    id: "keeps",
    order: 2,
    heading: w.KEEPS_HEADING,
    mount: function (el) {
      text = Halvo.el("p", { id: "window-text", class: "muted", text: w.KEEPS_UNKNOWN });
      recorded = Halvo.el("ul", { id: "recorded" });
      others = Halvo.el("div", { id: "keeps-limits" });
      el.append(text, recorded, others);
      Halvo.feed("status").on(render);
    },
    show: function () {},
    hide: function () {},
  });
})();
