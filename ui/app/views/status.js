(function () {
  "use strict";
  var HOLD_MS = 1000;
  var EXIT_FAULT_REPORTS_ON = 6;
  var w = Halvo.words;
  var status = Halvo.feed("status");
  var dot;
  var state;
  var reason;
  var toggle;
  var failed;
  var drain;
  var expiry;
  var loginLine;
  var firstRun;
  var settingsLine;
  var settingsReset;
  var settingsDelete;
  var resetRow;
  var resetRecorded;
  var asked = { armedAt: null, timer: null, box: null, point: null, pressed: false };
  var resetting = false;
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
    unsupported: w.NOT_RUNNING,
    another_version: w.ERROR_UPDATE,
    not_started: w.FIRST_RUN_HEADING,
    data_dir_unusable: w.DATA_DIR_UNUSABLE,
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
  var SETTINGS = { corrupt: w.SETTINGS_UNREADABLE, newer_version: w.SETTINGS_NEWER };
  var FAULT = { store: w.FAULT_STORE, drain: w.FAULT_DRAIN, no_source: w.FAULT_NO_SOURCE, permission: w.FAULT_PERMISSION };
  var DRAIN = { no_space: w.CAUSE_NO_SPACE, io: w.CAUSE_IO, db: w.CAUSE_DB, rejected: w.CAUSE_REJECTED };
  var EXPIRY = { io: w.CAUSE_IO, no_space: w.CAUSE_NO_SPACE, db: w.CAUSE_DB };
  var SOURCE = {
    foreground: w.SOURCE_FOREGROUND,
    screen: w.SCREEN_ACTIVITY,
    input: w.SOURCE_INPUT,
    system_audio: w.KIND_SYSTEM_AUDIO,
    microphone: w.KIND_MICROPHONE,
    keyboard: w.KIND_TYPING,
    pointer: w.SOURCE_POINTER,
    window_title: w.KIND_WINDOW_TITLES,
  };

  Halvo.service = function () {
    return shownService;
  };

  Halvo.takeMutationReply = function (view) {
    Halvo.markAnswer();
    readsSentBeforeTheLastMutation = sent;
    status.set(view);
  };

  function pressOf(s) {
    return s.hold ? s.hold.press : null;
  }

  function heading(s) {
    if (s.hold) return w.CAPTURE_HELD;
    if (s.service === "not_running" && s.last_exit === EXIT_FAULT_REPORTS_ON) return w.FAULT_REPORTS_ON;
    if (s.service === "not_running" && s.platform === "windows") return w.NOT_RECORDING_ON_WINDOWS;
    if (s.service !== "running") return SERVICE[s.service] || w.STATUS_REFUSED;
    return CAPTURE[s.capture] || w.CAPTURE_UNKNOWN;
  }

  function heldReason(s) {
    var press = pressOf(s);
    if (press === "start") return w.HELD_START;
    if (press === "resume" && s.service === "running") return w.HELD_RESUME;
    return "";
  }

  function renderToggle(s) {
    var press = pressOf(s);
    var notRunning = s.service === "not_running";
    if (s.service === "not_started") action = "start";
    else if (press === "start" || notRunning) action = "running";
    else if (press === "resume") action = s.service === "running" ? "running" : null;
    else if (s.capture === "awaiting_resume") action = "running";
    else action = s.desired === "running" ? "paused" : s.desired === "paused" ? "running" : null;
    if (pending !== null) {
      toggle.textContent = pending === "paused" ? w.PAUSING : w.STARTING;
      toggle.disabled = true;
      return;
    }
    if (press === "start" || action === "start" || notRunning) toggle.textContent = w.START;
    else toggle.textContent = press === "resume" || action === "running" ? w.RESUME : w.PAUSE;
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

  function renderFirstRun(s) {
    var t = s.service === "not_started" ? s.texts : null;
    firstRun.classList.toggle("hidden", !t);
    if (!t) {
      firstRun.replaceChildren();
      return;
    }
    var lines = [t.repeated_tasks, t.passwords, t.typed_ahead, t.backup_limit, t.storage, t.limits, t.custody].filter(function (line) {
      return typeof line === "string" && line !== "";
    });
    firstRun.replaceChildren.apply(
      firstRun,
      [
        Halvo.el("p", { text: w.FIRST_RUN_INTRO }),
        Halvo.el(
          "ul",
          { id: "first-run-recorded" },
          (t.recorded || []).map(function (r) {
            return Halvo.el("li", { text: r.text });
          }),
        ),
      ].concat(
        lines.map(function (line) {
          return Halvo.el("p", { class: "muted", text: line });
        }),
      ),
    );
  }

  function render(s) {
    var key = s.service + ":" + s.capture + ":" + s.desired + ":" + pressOf(s);
    if (shown !== null && key !== shown) heldUntil = performance.now() + HOLD_MS;
    shown = key;
    shownService = s.service;
    dot.classList.toggle("on", !s.hold && s.service === "running" && (s.capture === "running" || s.capture === "partial"));
    state.textContent = heading(s);
    if (s.hold) reason.textContent = heldReason(s);
    else reason.textContent = s.service === "running" && s.capture === "faulted" ? FAULT[s.capture_reason] || "" : "";
    renderToggle(s);
    renderSources(s);
    var d = s.drain;
    drain.textContent = d && d.state === "failing" ? w.NOT_SAVING + " " + (DRAIN[d.error] || w.CAUSE_IO) : "";
    var x = s.expiry;
    expiry.textContent =
      x && x.state === "failing" ? w.EXPIRY_FAILING + " " + (EXPIRY[x.error] || w.CAUSE_IO) + (x.last_at ? " " + w.EXPIRY_SINCE + " " + x.last_at + "." : "") : "";
    loginLine.textContent = s.login_item_written === false ? w.LOGIN_ITEM_NOT_WRITTEN : s.login_item_loaded === false ? w.LOGIN_ITEM_OFF : "";
    settingsLine.textContent = SETTINGS[s.state_file] || "";
    settingsReset.classList.toggle("hidden", !resetOffered(s));
    settingsDelete.classList.toggle("hidden", s.state_file !== "newer_version");
    renderFirstRun(s);
  }

  function resetOffered(s) {
    return s.state_file === "corrupt" && !(s.store && s.store.state === "deleting");
  }

  function boxOf(control) {
    var box = control.getBoundingClientRect();
    return { left: box.left, top: box.top, right: box.right, bottom: box.bottom, width: box.width, height: box.height };
  }

  function disarmReset() {
    var held = asked.armedAt !== null;
    if (asked.timer !== null) clearTimeout(asked.timer);
    asked = { armedAt: null, timer: null, box: null, point: null, pressed: false };
    resetRow.classList.add("hidden");
    settingsReset.disabled = resetting;
    if (held) Halvo.views.release();
  }

  function armReset(event) {
    var guard = Halvo.pressGuard;
    if (asked.armedAt !== null || resetting || !resetOffered(last)) return;
    var rows = Array.isArray(last.reset_recorded) ? last.reset_recorded : [];
    resetRecorded.replaceChildren.apply(
      resetRecorded,
      rows.map(function (r) {
        return Halvo.el("li", { text: r.text });
      }),
    );
    asked = {
      armedAt: performance.now(),
      timer: setTimeout(disarmReset, guard.disarmAfterMs),
      box: boxOf(settingsReset),
      point: guard.hasPoint(event) ? { x: event.clientX, y: event.clientY } : null,
      pressed: false,
    };
    Halvo.views.hold();
    settingsReset.disabled = true;
    resetRow.classList.remove("hidden");
  }

  function onResetPress() {
    if (asked.armedAt !== null) asked.pressed = true;
  }

  function onResetYes(event) {
    var guard = Halvo.pressGuard;
    if (asked.armedAt === null) return Promise.resolve();
    if (event && event.detail > 1) return Promise.resolve();
    if (guard.hasPoint(event)) {
      var fromPress = asked.pressed;
      asked.pressed = false;
      if (!fromPress || guard.atPlace(event, asked.box, asked.point)) return Promise.resolve();
    }
    var since = performance.now() - asked.armedAt;
    if (since < guard.armDelayMs) return Promise.resolve();
    disarmReset();
    if (since >= guard.disarmAfterMs) return Promise.resolve();
    return sendReset();
  }

  function sendReset() {
    resetting = true;
    settingsReset.disabled = true;
    Halvo.showError("");
    return Halvo.ipc
      .call("reset_settings")
      .then(function (view) {
        Halvo.takeMutationReply(view);
      })
      .catch(function (e) {
        Halvo.showError(e);
      })
      .then(function () {
        resetting = false;
        settingsReset.disabled = false;
      });
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
    if (asked === "start") return consent();
    return Halvo.ipc
      .call("set_capture_state", { capture: asked })
      .then(function (reply) {
        pending = null;
        Halvo.takeMutationReply(reply);
        if (reply.reached === true) return;
        if (asked === "paused") Halvo.showError(w.PAUSE_NOT_REACHED);
        else if (reply.service === "running") Halvo.showError(w.RESUME_NOT_REACHED);
      })
      .catch(function (e) {
        pending = null;
        Halvo.showError(asked === "paused" ? w.PAUSE_NOT_REACHED : e);
        renderToggle(last);
      });
  }

  function consent() {
    return Halvo.ipc
      .call("start_capture")
      .then(function (reply) {
        pending = null;
        Halvo.takeMutationReply(reply);
        if (reply.service === "running" && reply.desired !== "running") Halvo.showError(w.START_STEPS_OPEN);
      })
      .catch(function (e) {
        pending = null;
        Halvo.showError(e && e.error === "deadline" ? w.START_NO_ANSWER.replace("{seconds}", String(Number(e.seconds) || 0)) : e);
        renderToggle(last);
      });
  }

  Halvo.views.add({
    id: "status",
    order: 1,
    heading: w.CAPTURE_HEADING,
    mount: function (el) {
      dot = Halvo.el("div", { id: "dot", class: "dot" });
      state = Halvo.el("span", { id: "state", text: w.CONNECTING });
      reason = Halvo.el("span", { id: "capture-reason", class: "muted" });
      toggle = Halvo.el("button", { id: "toggle", text: w.PAUSE, disabled: true });
      toggle.onclick = onToggle;
      failed = Halvo.el("ul", { id: "failed-sources", class: "hidden" });
      drain = Halvo.el("p", { id: "drain-line" });
      expiry = Halvo.el("p", { id: "expiry-line" });
      loginLine = Halvo.el("p", { id: "login-line" });
      firstRun = Halvo.el("div", { id: "first-run", class: "hidden" });
      settingsLine = Halvo.el("span", { id: "settings-line" });
      settingsReset = Halvo.el("button", { id: "settings-reset", text: w.SETTINGS_RESET });
      settingsReset.classList.add("hidden");
      settingsReset.onclick = armReset;
      settingsDelete = Halvo.el("button", { id: "settings-delete", class: "danger", text: w.DELETE_EVERYTHING });
      settingsDelete.classList.add("hidden");
      settingsDelete.onclick = function (event) {
        Halvo.armDelete(event, settingsDelete);
      };
      resetRecorded = Halvo.el("ul", { id: "settings-reset-recorded" });
      var resetYes = Halvo.el("button", { id: "confirm-settings-reset", text: w.SETTINGS_RESET_YES });
      var resetNo = Halvo.el("button", { id: "cancel-settings-reset", text: w.CANCEL });
      resetYes.onclick = onResetYes;
      resetYes.addEventListener("pointerdown", onResetPress);
      resetYes.addEventListener("mousedown", onResetPress);
      resetNo.onclick = disarmReset;
      resetRow = Halvo.el("div", { id: "settings-reset-row", class: "confirm-row" }, [
        Halvo.el("p", { id: "settings-reset-question", text: w.SETTINGS_RESET_QUESTION }),
        resetRecorded,
        Halvo.el("div", { class: "row" }, [resetYes, resetNo]),
      ]);
      resetRow.classList.add("hidden");
      el.append(
        Halvo.el("div", { class: "row status-row" }, [dot, Halvo.el("p", { class: "state-line" }, [state, " ", reason]), toggle]),
        failed,
        drain,
        expiry,
        loginLine,
        Halvo.el("div", { class: "row" }, [settingsLine, settingsReset, settingsDelete]),
        resetRow,
        firstRun,
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
