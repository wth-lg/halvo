(function () {
  "use strict";
  var status = Halvo.feed("status");
  var dot;
  var state;
  var toggle;

  function render(s) {
    var running = s.state === "running";
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

  function onToggle() {
    var current = status.latest();
    if (!current) return Promise.resolve();
    var next = current.state === "running" ? "paused" : "running";
    toggle.disabled = true;
    return Halvo.ipc
      .call("set_capture_state", { capture: next })
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
