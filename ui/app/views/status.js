(function () {
  "use strict";
  var HOLD_MS = 1000;
  var w = Halvo.words;
  var status = Halvo.feed("status");
  var dot;
  var state;
  var reason;
  var toggle;
  var failed;
  var drain;
  var expiry;
  var action = null;
  var pending = null;
  var shown = null;
  var shownService = "connecting";
  var heldUntil = 0;
  var sent = 0;
  var last = {};
  var readsSentBeforeTheLastMutation = 0;

  var SERVICE = {
    connecting: w.CONNECTING,
    not_running: w.NOT_RUNNING,
    held_unreachable: w.HELD_UNREACHABLE,
    status_refused: w.STATUS_REFUSED,
    unsupported: w.UNSUPPORTED,
    another_version: w.ERROR_UPDATE,
  };
  var CAPTURE = {
    running: w.CAPTURE_RUNNING,
    partial: w.CAPTURE_PARTIAL,
    starting: w.CAPTURE_STARTING,
    pausing: w.CAPTURE_PAUSING,
    paused: w.CAPTURE_PAUSED,
    no_indicator: w.CAPTURE_NO_INDICATOR,
    awaiting_resume: w.CAPTURE_AWAITING_RESUME,
    faulted: w.CAPTURE_FAULTED,
  };
  var FAULT = { store: w.FAULT_STORE, drain: w.FAULT_DRAIN, no_source: w.FAULT_NO_SOURCE, permission: w.FAULT_PERMISSION };
  var DRAIN = { no_space: w.CAUSE_NO_SPACE, io: w.CAUSE_IO, db: w.CAUSE_DB, rejected: w.CAUSE_REJECTED };
  var EXPIRY = { io: w.CAUSE_IO, no_space: w.CAUSE_NO_SPACE, db: w.CAUSE_DB };
  var SOURCE = {
    foreground: "The app in front",
    screen: w.SCREEN_ACTIVITY,
    input: "Typing and the pointer",
    system_audio: "System audio",
    microphone: "Microphone",
    keyboard: "Typing",
    pointer: "The pointer",
    window_title: "Window titles",
  };

  Halvo.service = function () {
    return shownService;
  };

  Halvo.takeMutationReply = function (view) {
    readsSentBeforeTheLastMutation = sent;
    status.set(view);
  };

  function heading(s) {
    if (s.service !== "running") return SERVICE[s.service] || w.STATUS_REFUSED;
    return CAPTURE[s.capture] || w.CAPTURE_UNKNOWN;
  }

  function renderToggle(s) {
    action = s.desired === "running" ? "paused" : s.desired === "paused" ? "running" : null;
    if (pending !== null) {
      toggle.textContent = pending === "paused" ? w.PAUSING : w.STARTING;
      toggle.disabled = true;
      return;
    }
    toggle.textContent = s.desired === "paused" ? w.RESUME : w.PAUSE;
    toggle.disabled = action === null;
  }

  function renderSources(s) {
    var rows = (s.sources || []).filter(function (src) {
      return src.state === "failed";
    });
    failed.replaceChildren.apply(
      failed,
      rows.map(function (src) {
        var text = (SOURCE[src.name] || src.name) + " " + w.SOURCE_FAILED + ".";
        if (src.reason === "permission") text += " " + w.SOURCE_WALKBACK;
        return Halvo.el("li", { text: text });
      }),
    );
    failed.classList.toggle("hidden", rows.length === 0);
  }

  function render(s) {
    var key = s.service + ":" + s.capture + ":" + s.desired;
    if (shown !== null && key !== shown) heldUntil = performance.now() + HOLD_MS;
    shown = key;
    shownService = s.service;
    dot.classList.toggle("on", s.service === "running" && (s.capture === "running" || s.capture === "partial"));
    state.textContent = heading(s);
    reason.textContent = s.service === "running" && s.capture === "faulted" ? FAULT[s.capture_reason] || "" : "";
    renderToggle(s);
    renderSources(s);
    var d = s.drain;
    drain.textContent = d && d.state === "failing" ? w.NOT_SAVING + " " + (DRAIN[d.error] || w.CAUSE_IO) : "";
    var x = s.expiry;
    expiry.textContent =
      x && x.state === "failing" ? w.EXPIRY_FAILING + " " + (EXPIRY[x.error] || w.CAUSE_IO) + (x.last_at ? " " + w.EXPIRY_SINCE + " " + x.last_at + "." : "") : "";
  }

  function refresh() {
    var mine = ++sent;
    return Halvo.ipc
      .call("capture_status")
      .then(function (s) {
        Halvo.showConnection("");
        if (mine <= readsSentBeforeTheLastMutation) return;
        status.set(s);
      })
      .catch(function () {
        Halvo.showConnection(w.READ_FAILED);
      });
  }

  function onToggle(event) {
    if (action === null || pending !== null) return Promise.resolve();
    if (event && event.detail > 1) return Promise.resolve();
    if (performance.now() < heldUntil) return Promise.resolve();
    var asked = action;
    pending = asked;
    toggle.disabled = true;
    toggle.textContent = asked === "paused" ? w.PAUSING : w.STARTING;
    Halvo.showError("");
    return Halvo.ipc
      .call("set_capture_state", { capture: asked })
      .then(function (reply) {
        pending = null;
        Halvo.takeMutationReply(reply);
        if (reply.service === "running" && reply.reached !== true) {
          Halvo.showError(asked === "paused" ? w.PAUSE_NOT_REACHED : w.RESUME_NOT_REACHED);
        }
      })
      .catch(function (e) {
        pending = null;
        Halvo.showError(e);
        renderToggle(last);
      });
  }

  Halvo.views.add({
    id: "status",
    order: 1,
    heading: "Capture",
    mount: function (el) {
      dot = Halvo.el("div", { id: "dot", class: "dot" });
      state = Halvo.el("span", { id: "state", text: w.CONNECTING });
      reason = Halvo.el("span", { id: "capture-reason", class: "muted" });
      toggle = Halvo.el("button", { id: "toggle", text: w.PAUSE, disabled: true });
      toggle.onclick = onToggle;
      failed = Halvo.el("ul", { id: "failed-sources", class: "hidden" });
      drain = Halvo.el("p", { id: "drain-line" });
      expiry = Halvo.el("p", { id: "expiry-line" });
      el.append(
        Halvo.el("div", { class: "row status-row" }, [dot, Halvo.el("p", { class: "state-line" }, [state, " ", reason]), toggle]),
        failed,
        drain,
        expiry,
      );
      status.on(function (s) {
        last = s;
      });
      status.on(render);
      status.source(refresh);
      refresh();
      setInterval(refresh, 3000);
    },
    show: function () {},
    hide: function () {},
  });
})();
