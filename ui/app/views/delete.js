(function () {
  "use strict";
  var w = Halvo.words;
  var ARM_DELAY_MS = 500;
  var DISARM_AFTER_MS = 10000;
  var start;
  var confirmRow;
  var confirm;
  var cancel;
  var result;
  var armedAt = null;
  var disarmTimer = null;
  var keptBox = null;
  var keptPoint = null;
  var pressed = false;
  var cleanup;
  var cleanupNote;
  var current = null;
  var armed = null;
  var busy = false;
  var keptInFlight = false;
  var AFTER_DELETE = "after-delete";

  function hasPoint(event) {
    return Boolean(event) && event.detail >= 1;
  }

  function atKeptPlace(event) {
    var x = event.clientX;
    var y = event.clientY;
    if (keptPoint !== null && Math.abs(x - keptPoint.x) <= 2 && Math.abs(y - keptPoint.y) <= 2) return true;
    return keptBox !== null && keptBox.width > 0 && keptBox.height > 0 && x >= keptBox.left && x <= keptBox.right && y >= keptBox.top && y <= keptBox.bottom;
  }

  function onPress() {
    if (armedAt !== null) pressed = true;
  }

  function offered(s) {
    if (!s || s.service !== "running" || !s.store) return false;
    var store = s.store;
    if (store.state === "deleting") return false;
    if (store.error === "locked") return false;
    if (store.state === "opening" && !store.id) return false;
    return true;
  }

  function renderOffer() {
    start.disabled = busy || keptInFlight || armedAt !== null || !offered(current);
  }

  function clockOf(ms) {
    var local = new Date(ms - new Date(ms).getTimezoneOffset() * 60000);
    var hh = local.getUTCHours();
    var mm = local.getUTCMinutes();
    return (hh < 10 ? "0" : "") + hh + ":" + (mm < 10 ? "0" : "") + mm;
  }

  function disarm() {
    if (disarmTimer !== null) clearTimeout(disarmTimer);
    disarmTimer = null;
    armedAt = null;
    keptBox = null;
    keptPoint = null;
    pressed = false;
    renderOffer();
    confirmRow.classList.add("hidden");
    Halvo.views.release();
  }

  function arm(event) {
    if (armedAt !== null || busy || !offered(current)) return;
    armed = { id: current.store.id || null, service: current.service };
    var box = start.getBoundingClientRect();
    keptBox = { left: box.left, top: box.top, right: box.right, bottom: box.bottom, width: box.width, height: box.height };
    keptPoint = hasPoint(event) ? { x: event.clientX, y: event.clientY } : null;
    pressed = false;
    Halvo.views.hold();
    armedAt = performance.now();
    start.disabled = true;
    confirmRow.classList.remove("hidden");
    result.textContent = "";
    showCleanup(null);
    disarmTimer = setTimeout(disarm, DISARM_AFTER_MS);
  }

  function showCleanup(command) {
    cleanup.textContent = command || "";
    cleanup.classList.toggle("hidden", !command);
    cleanupNote.classList.toggle("hidden", !command);
  }

  function leftNothing(r) {
    return r.root_destroyed === "yes" && r.not_followed === 0 && r.files_left === 0 && r.database_left === false && r.entries_left === 0 && r.unknown_entries === 0;
  }

  function counted(text, count) {
    return text.replace("{count}", String(count));
  }

  function deletedWords(r) {
    if (r.nothing_left === true && leftNothing(r)) return [w.DELETE_DONE, w.DELETE_COPIES];
    var lines = [];
    if (r.not_followed > 0) {
      if (r.root_destroyed === "yes") lines.push(w.DELETE_KEYS_DESTROYED, w.DELETE_COPIES);
      lines.push(w.DELETE_NOT_FOLLOWED);
    }
    if (r.root_destroyed === "no") lines.push(r.cleanup_command ? w.DELETE_ROOT_LEFT : w.DELETE_ROOT_UNNAMED);
    if (r.root_destroyed === "unknown") lines.push(w.DELETE_ROOT_UNKNOWN);
    if (r.files_left > 0) lines.push(counted(w.DELETE_FILES_LEFT, r.files_left));
    if (r.database_left === true) lines.push(w.DELETE_DATABASE_LEFT);
    if (r.entries_left > 0) lines.push(counted(w.DELETE_ENTRIES_LEFT, r.entries_left));
    if (r.unknown_entries > 0) lines.push(counted(w.DELETE_UNKNOWN_ENTRIES, r.unknown_entries));
    return lines.length > 0 ? lines : [w.DELETE_RESULT_UNREADABLE];
  }

  function answerLost(e) {
    return Boolean(e) && typeof e === "object" && (e.error === "deadline" || e.error === "channel_failed" || e.error === "broken_reply");
  }

  function showKept(kept, afterDelete) {
    keptInFlight = Boolean(kept) && kept.state === "in_flight";
    var answer = kept && kept.state === "answered" ? kept.answer : null;
    if (answer && (answer.outcome === "deleted" || answer.outcome === "already_deleted")) {
      var words = answer.outcome === "deleted" ? deletedWords(answer) : [w.ALREADY_DELETED];
      result.textContent = w.DELETE_AT.replace("{time}", clockOf(kept.at_ms)) + words.join(" ");
      showCleanup(answer.outcome === "deleted" && answer.root_destroyed === "no" ? answer.cleanup_command : null);
      result.setAttribute("tabindex", "-1");
      if (typeof result.scrollIntoView === "function") result.scrollIntoView();
      result.focus();
    } else if (afterDelete && !keptInFlight) {
      result.textContent = w.DELETE_RESULT_UNREADABLE;
    }
    renderOffer();
  }

  function readKept(afterDelete) {
    return Halvo.ipc.call("last_delete").then(
      function (kept) {
        showKept(kept, afterDelete);
      },
      function () {
        keptInFlight = false;
        if (afterDelete) result.textContent = w.DELETE_RESULT_UNREADABLE;
        renderOffer();
      },
    );
  }

  function refreshAll() {
    return Halvo.feed("status")
      .refresh()
      .then(function () {
        return Halvo.feed("inventory").refresh();
      });
  }

  function remove() {
    var asked = armed;
    armed = null;
    if (!asked || !current || current.service !== asked.service) {
      result.textContent = w.DELETE_REFUSED;
      return refreshAll();
    }
    busy = true;
    renderOffer();
    confirm.disabled = true;
    start.textContent = w.DELETING;
    Halvo.showError("");
    return Halvo.ipc
      .call("delete_all", { storeId: asked.id })
      .then(function (r) {
        Halvo.markAnswer();
        if (r.outcome === "deleted" || r.outcome === "already_deleted") {
          Halvo.loadAgain(AFTER_DELETE);
          return null;
        } else if (r.outcome === "delete_refused") {
          result.textContent = w.DELETE_REFUSED;
        } else {
          result.textContent = "";
          Halvo.showError(w.ERROR_BROKEN);
        }
        return refreshAll();
      })
      .catch(function (e) {
        if (e && typeof e === "object" && e.error === "busy") {
          result.textContent = w.DELETE_BUSY;
          return;
        }
        result.textContent = answerLost(e) ? w.DELETE_UNKNOWN_OUTCOME : "";
        Halvo.showError(e);
      })
      .then(function () {
        busy = false;
        start.textContent = w.DELETE_EVERYTHING;
        confirm.disabled = false;
        renderOffer();
      });
  }

  function onConfirm(event) {
    if (armedAt === null) return Promise.resolve();
    if (event && event.detail > 1) return Promise.resolve();
    if (hasPoint(event)) {
      var fromPress = pressed;
      pressed = false;
      if (!fromPress || atKeptPlace(event)) return Promise.resolve();
    }
    var since = performance.now() - armedAt;
    if (since < ARM_DELAY_MS) return Promise.resolve();
    if (since >= DISARM_AFTER_MS) {
      disarm();
      return Promise.resolve();
    }
    disarm();
    return remove();
  }

  Halvo.views.add({
    id: "delete-everything",
    order: 6,
    heading: w.DELETE_EVERYTHING,
    mount: function (el) {
      start = Halvo.el("button", { id: "delete", class: "danger", text: w.DELETE_EVERYTHING });
      confirm = Halvo.el("button", { id: "confirm-delete", class: "danger", text: w.DELETE_YES });
      cancel = Halvo.el("button", { id: "cancel-delete", text: w.CANCEL });
      result = Halvo.el("span", { id: "delete-result", class: "muted" });
      cleanup = Halvo.el("code", { id: "delete-cleanup", class: "hidden" });
      cleanupNote = Halvo.el("p", { id: "delete-cleanup-note", class: "muted hidden", text: w.CLEANUP_NAMES_ACCOUNT });
      confirmRow = Halvo.el("div", { id: "delete-confirm-row", class: "confirm-row hidden" }, [
        Halvo.el("p", {
          id: "delete-question",
          text: w.DELETE_QUESTION,
        }),
        Halvo.el("div", { class: "row" }, [confirm, cancel]),
      ]);
      start.onclick = arm;
      cancel.onclick = disarm;
      confirm.onclick = onConfirm;
      confirm.addEventListener("pointerdown", onPress);
      confirm.addEventListener("mousedown", onPress);
      el.append(
        Halvo.el("p", { class: "muted", text: w.DELETE_ABOUT }),
        Halvo.el("div", { class: "row" }, [start, result]),
        cleanup,
        cleanupNote,
        confirmRow,
      );
      start.disabled = true;
      var afterDelete = Halvo.takeLoadMark() === AFTER_DELETE;
      Halvo.feed("status").on(function (s) {
        current = s;
        if (keptInFlight) readKept(false);
        renderOffer();
      });
      readKept(afterDelete);
    },
    show: function () {},
    hide: disarm,
  });
})();
