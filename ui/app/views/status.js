(function () {
  "use strict";
  var HOLD_MS = 1000;
  var status = Halvo.feed("status");
  var dot;
  var state;
  var toggle;
  var action = null;
  var shown = null;
  var heldUntil = 0;

  function render(s) {
    var running = s.state === "running";
    if (shown !== null && s.state !== shown) heldUntil = performance.now() + HOLD_MS;
    shown = s.state;
    action = running ? "paused" : "running";
    dot.classList.toggle("on", running);
    state.textContent = running ? "capturing" : "paused";
    toggle.textContent = running ? "Pause" : "Resume";
    toggle.disabled = false;
  }

  function refresh() {
    return Halvo.ipc
      .call("capture_status")
      .then(function (s) {
        status.set(s);
        Halvo.showError("");
      })
      .catch(Halvo.showError);
  }

  function onToggle(event) {
    if (action === null) return Promise.resolve();
    if (event && event.detail > 1) return Promise.resolve();
    if (performance.now() < heldUntil) return Promise.resolve();
    toggle.disabled = true;
    return Halvo.ipc
      .call("set_capture_state", { capture: action })
      .then(function (s) {
        status.set(s);
        Halvo.showError("");
      })
      .catch(function (e) {
        Halvo.showError(e);
        toggle.disabled = false;
      });
  }

  Halvo.views.add({
    id: "status",
    order: 1,
    heading: "Capture",
    mount: function (el) {
      dot = Halvo.el("div", { id: "dot", class: "dot" });
      state = Halvo.el("span", { id: "state", text: "starting" });
      toggle = Halvo.el("button", { id: "toggle", text: "Pause", disabled: true });
      toggle.onclick = onToggle;
      el.append(Halvo.el("div", { class: "row status-row" }, [dot, Halvo.el("p", { class: "state-line" }, ["Halvo is ", state]), toggle]));
      status.on(render);
      status.source(refresh);
      refresh();
      setInterval(refresh, 3000);
    },
    show: function () {},
    hide: function () {},
  });
})();
