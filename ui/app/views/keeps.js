(function () {
  "use strict";
  var w = Halvo.words;
  var text;
  var recorded;
  var others;
  var typingBox;
  var typingToggle;
  var ifOn;
  var ifOnLead;
  var ifOnRow;
  var view;
  var turning = false;
  var shownOn = null;

  function renderTyping(control) {
    shownOn = control ? control.on === true : null;
    if (!control) {
      typingBox.classList.add("hidden");
      return;
    }
    typingToggle.textContent = control.label;
    typingToggle.setAttribute("aria-checked", shownOn ? "true" : "false");
    typingToggle.classList.toggle("on", shownOn);
    typingToggle.disabled = turning;
    var lead = typeof control.if_on === "string" ? control.if_on : "";
    var row = typeof control.row === "string" ? control.row : "";
    ifOnLead.textContent = lead;
    ifOnRow.textContent = row;
    ifOn.classList.toggle("hidden", shownOn || lead === "" || row === "");
    typingBox.classList.remove("hidden");
  }

  function render(s) {
    var t = s.service === "running" ? s.texts : null;
    typingBox.remove();
    if (!t) {
      text.textContent = w.KEEPS_UNKNOWN;
      recorded.replaceChildren();
      others.replaceChildren();
      view.insertBefore(typingBox, others);
      renderTyping(null);
      return;
    }
    text.textContent = w.KEEPS_WINDOW.replace("{days}", String(s.window.rolling_days));
    var control = s.typing_switch && typeof s.typing_switch === "object" ? s.typing_switch : null;
    var rows = [];
    (t.recorded || []).forEach(function (r) {
      var li = Halvo.el("li", { text: r.text });
      if (typeof r.switch === "string") li.setAttribute("data-switch", r.switch);
      rows.push(li);
      if (r.switch === "key_class" && control) rows.push(typingBox);
    });
    recorded.replaceChildren.apply(recorded, rows);
    if (rows.indexOf(typingBox) === -1) {
      view.insertBefore(typingBox, others);
      renderTyping(null);
    } else {
      renderTyping(control);
    }
    others.replaceChildren.apply(
      others,
      [t.passwords, t.typed_ahead, t.keys_withheld, t.backup_limit, t.limits]
        .filter(function (line) {
          return typeof line === "string" && line !== "";
        })
        .map(function (line) {
          return Halvo.el("p", { class: "muted", text: line });
        }),
    );
  }

  function onTurn(event) {
    if (turning || shownOn === null) return Promise.resolve();
    if (event && event.detail > 1) return Promise.resolve();
    turning = true;
    typingToggle.disabled = true;
    Halvo.showError("");
    return Halvo.ipc
      .call("set_typing", { on: !shownOn })
      .then(function (view) {
        Halvo.takeMutationReply(view);
      })
      .catch(function (e) {
        Halvo.showError(e);
      })
      .then(function () {
        turning = false;
        typingToggle.disabled = false;
      });
  }

  Halvo.views.add({
    id: "keeps",
    order: 2,
    heading: w.KEEPS_HEADING,
    mount: function (el) {
      text = Halvo.el("p", { id: "window-text", class: "muted", text: w.KEEPS_UNKNOWN });
      recorded = Halvo.el("ul", { id: "recorded" });
      others = Halvo.el("div", { id: "keeps-limits" });
      typingToggle = Halvo.el("button", { id: "record-typing", role: "switch" });
      typingToggle.onclick = onTurn;
      ifOnLead = Halvo.el("p", { id: "typing-if-on-lead", class: "muted" });
      ifOnRow = Halvo.el("p", { id: "typing-if-on-row", class: "muted" });
      ifOn = Halvo.el("div", { id: "typing-if-on", class: "hidden" }, [ifOnLead, ifOnRow]);
      typingBox = Halvo.el("li", { id: "typing-control", class: "hidden" }, [typingToggle, ifOn]);
      view = el;
      el.append(text, recorded, typingBox, others);
      Halvo.feed("status").on(render);
    },
    show: function () {},
    hide: function () {},
  });
})();
