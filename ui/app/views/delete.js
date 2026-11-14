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
    start.disabled = busy || armedAt !== null || !offered(current);
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

  function deletedWords(r) {
    if (r.nothing_left === true) return [w.DELETE_DONE];
    var lines = [];
    if (r.not_followed > 0) lines.push(w.DELETE_NOT_FOLLOWED);
    if (r.root_destroyed === "no") lines.push(w.DELETE_ROOT_LEFT);
    if (r.root_destroyed === "unknown") lines.push(w.DELETE_ROOT_UNKNOWN);
    if (r.files_left > 0) lines.push(w.DELETE_FILES_LEFT.replace("{count}", String(r.files_left)));
    if (r.database_left === true) lines.push(w.DELETE_DATABASE_LEFT);
    return lines.length > 0 ? lines : [w.DELETE_ROOT_UNKNOWN];
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
        if (r.outcome === "deleted") {
          Halvo.clearStored();
          result.textContent = deletedWords(r).join(" ");
          showCleanup(r.root_destroyed === "no" ? r.cleanup_command : null);
        } else if (r.outcome === "already_deleted") {
          Halvo.clearStored();
          result.textContent = w.ALREADY_DELETED;
        } else if (r.outcome === "delete_refused") {
          result.textContent = w.DELETE_REFUSED;
        } else {
          result.textContent = "";
          Halvo.showError(w.ERROR_BROKEN);
        }
        return refreshAll();
      })
      .catch(function (e) {
        result.textContent = "";
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
      Halvo.feed("status").on(function (s) {
        current = s;
        renderOffer();
      });
    },
    show: function () {},
    hide: disarm,
  });
})();
