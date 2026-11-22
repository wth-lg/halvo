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
  var question;
  var offerRow;
  var offerButton;
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
  var loading = false;
  var keptInFlight = false;
  var refusedInARow = 0;
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
    if (s && s.service === "not_running") return true;
    if (!s || s.service !== "running" || !s.store) return false;
    var store = s.store;
    if (store.state === "deleting") return false;
    if (store.error === "locked") return false;
    if (store.state === "opening" && !store.id) return false;
    return true;
  }

  function renderOffer() {
    start.disabled = busy || loading || keptInFlight || armedAt !== null || !offered(current);
    offerRow.classList.toggle("hidden", refusedInARow < 2);
    offerButton.disabled = busy || loading || keptInFlight || armedAt !== null || refusedInARow < 2;
  }

  function withFolder(text, folder) {
    return text.replace(", {folder}", typeof folder === "string" && folder !== "" ? ", " + folder : "");
  }

  function questionOf(asked, s) {
    if (!asked.offline) return w.DELETE_QUESTION;
    return withFolder(asked.stopFirst ? w.DELETE_STOP_QUESTION : w.DELETE_OFFLINE_QUESTION, s.data_folder);
  }

  function localOf(ms) {
    return new Date(ms - new Date(ms).getTimezoneOffset() * 60000);
  }

  function clockOf(ms) {
    var local = localOf(ms);
    var hh = local.getUTCHours();
    var mm = local.getUTCMinutes();
    return (hh < 10 ? "0" : "") + hh + ":" + (mm < 10 ? "0" : "") + mm;
  }

  function dayOf(ms) {
    var local = localOf(ms);
    return local.getUTCFullYear() * 10000 + local.getUTCMonth() * 100 + local.getUTCDate();
  }

  function dateOf(ms) {
    return localOf(ms).toLocaleDateString(undefined, { timeZone: "UTC", day: "numeric", month: "long", year: "numeric" });
  }

  function whenOf(ms) {
    if (typeof ms !== "number" || !isFinite(ms)) return "";
    if (dayOf(ms) === dayOf(Date.now())) return w.DELETE_AT.replace("{time}", clockOf(ms));
    return w.DELETE_ON_DAY.replace("{date}", dateOf(ms)).replace("{time}", clockOf(ms));
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
    armAt(event, start, false);
  }

  function armOffline(event) {
    armAt(event, offerButton, true);
  }

  Halvo.armDelete = function (event, control) {
    armAt(event, control || start, false);
  };

  function armAt(event, control, stopFirst) {
    if (armedAt !== null || busy || !current) return;
    if (stopFirst ? refusedInARow < 2 : !offered(current)) return;
    var offline = stopFirst || current.service === "not_running";
    armed = { id: offline ? null : current.store.id || null, service: current.service, offline: offline, stopFirst: stopFirst };
    question.textContent = questionOf(armed, current);
    var box = control.getBoundingClientRect();
    keptBox = { left: box.left, top: box.top, right: box.right, bottom: box.bottom, width: box.width, height: box.height };
    keptPoint = hasPoint(event) ? { x: event.clientX, y: event.clientY } : null;
    pressed = false;
    Halvo.views.hold();
    armedAt = performance.now();
    start.disabled = true;
    offerButton.disabled = true;
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
    var inARow = Number(r.unfinished_in_a_row) || 0;
    var unfinished = inARow >= 2 ? [withFolder(w.DELETE_NOT_FINISHED_AGAIN, r.looked_in)] : inARow === 1 ? [w.DELETE_NOT_FINISHED] : [];
    var lines = lineWords(r).concat(unfinished);
    if (lines.length === 0) lines = [w.DELETE_RESULT_UNREADABLE];
    return lines.concat(typeof r.looked_in === "string" ? [withFolder(w.DELETE_LOOKED_IN, r.looked_in)] : []);
  }

  function lineWords(r) {
    if (r.nothing_left === true && leftNothing(r)) return [w.DELETE_DONE, w.DELETE_COPIES];
    var lines = [];
    var keysDestroyed = r.root_destroyed === "yes";
    if (r.not_followed > 0) {
      if (keysDestroyed) lines.push(w.DELETE_KEYS_DESTROYED, w.DELETE_COPIES, w.DELETE_NOT_FOLLOWED);
      else lines.push(w.DELETE_NOT_FOLLOWED_KEY_LEFT);
    }
    if (r.root_destroyed === "no") lines.push(r.cleanup_command ? w.DELETE_ROOT_LEFT : w.DELETE_ROOT_UNNAMED);
    if (r.root_destroyed === "unknown") lines.push(w.DELETE_ROOT_UNKNOWN);
    if (r.files_left > 0) lines.push(counted(keysDestroyed ? w.DELETE_FILES_LEFT : w.DELETE_FILES_COUNT, r.files_left));
    if (r.database_left === true) lines.push(keysDestroyed ? w.DELETE_DATABASE_LEFT : w.DELETE_DATABASE_ALONE);
    if (r.entries_left > 0) lines.push(counted(w.DELETE_ENTRIES_LEFT, r.entries_left));
    if (r.unknown_entries > 0) lines.push(counted(w.DELETE_UNKNOWN_ENTRIES, r.unknown_entries));
    return lines;
  }

  function answerLost(e) {
    return Boolean(e) && typeof e === "object" && e.outcome_not_known === true;
  }

  function ownWords(e) {
    var code = e && typeof e === "object" ? e.error : null;
    if (code === "busy") return w.DELETE_BUSY;
    if (code === "service_ending") return w.DELETE_SERVICE_ENDING;
    if (code === "nothing_removed") return w.DELETE_NOTHING_REMOVED;
    if (code === "service_not_back") return answerLost(e) ? w.DELETE_SERVICE_NOT_BACK + " " + w.DELETE_UNKNOWN_OUTCOME : w.DELETE_SERVICE_NOT_BACK;
    if (code === "delete_held") return withFolder(w.DELETE_HELD, e.looked_in);
    if (code === "delete_not_started") return w.DELETE_NOT_STARTED;
    if (code === "delete_not_known") return w.DELETE_UNKNOWN_OUTCOME;
    return null;
  }

  function showKept(kept, afterDelete) {
    keptInFlight = Boolean(kept) && kept.state === "in_flight";
    var answer = kept && kept.state === "answered" ? kept.answer : null;
    if (answer && (answer.outcome === "deleted" || answer.outcome === "already_deleted" || answer.outcome === "no_store")) {
      var words =
        answer.outcome === "deleted" ? deletedWords(answer) : answer.outcome === "no_store" ? [withFolder(w.DELETE_NO_STORE, answer.looked_in)] : [w.ALREADY_DELETED];
      result.textContent = whenOf(kept.at_ms) + words.join(" ");
      showCleanup(answer.outcome === "deleted" && answer.root_destroyed === "no" ? answer.cleanup_command : null);
      if (afterDelete) {
        result.setAttribute("tabindex", "-1");
        if (typeof result.scrollIntoView === "function") result.scrollIntoView();
        result.focus();
      }
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
    var sent = asked.offline ? Halvo.ipc.call("delete_offline") : Halvo.ipc.call("delete_all", { storeId: asked.id });
    return sent
      .then(function (r) {
        Halvo.markAnswer();
        if (r.outcome === "deleted" || r.outcome === "already_deleted" || r.outcome === "no_store") {
          refusedInARow = 0;
          loading = true;
          Halvo.loadAgain(AFTER_DELETE);
          return null;
        } else if (r.outcome === "delete_refused" && asked.offline) {
          result.textContent = w.DELETE_REFUSED;
        } else if (r.outcome === "delete_refused") {
          refusedInARow += 1;
          result.textContent = refusedInARow >= 2 ? w.DELETE_REFUSED + " " + w.DELETE_REFUSED_TWICE : w.DELETE_REFUSED;
        } else {
          refusedInARow = 0;
          result.textContent = "";
          Halvo.showError(w.ERROR_BROKEN);
        }
        return refreshAll();
      })
      .catch(function (e) {
        refusedInARow = 0;
        var own = ownWords(e);
        if (own !== null) {
          result.textContent = own;
          return;
        }
        result.textContent = answerLost(e) ? w.DELETE_UNKNOWN_OUTCOME : "";
        Halvo.showError(e);
      })
      .then(function () {
        if (loading) return;
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
      offerButton = Halvo.el("button", { id: "delete-offline", class: "danger", text: w.DELETE_STOP_AND_DELETE });
      offerRow = Halvo.el("div", { id: "delete-offer", class: "row" }, [Halvo.el("p", { id: "delete-offer-line", text: w.DELETE_OFFER_OFFLINE }), offerButton]);
      offerRow.classList.add("hidden");
      question = Halvo.el("p", {
        id: "delete-question",
        text: w.DELETE_QUESTION,
      });
      cleanup = Halvo.el("code", { id: "delete-cleanup", class: "hidden" });
      cleanupNote = Halvo.el("p", { id: "delete-cleanup-note", class: "muted hidden", text: w.CLEANUP_NAMES_ACCOUNT });
      confirmRow = Halvo.el("div", { id: "delete-confirm-row", class: "confirm-row hidden" }, [question, Halvo.el("div", { class: "row" }, [confirm, cancel])]);
      start.onclick = arm;
      offerButton.onclick = armOffline;
      cancel.onclick = disarm;
      confirm.onclick = onConfirm;
      confirm.addEventListener("pointerdown", onPress);
      confirm.addEventListener("mousedown", onPress);
      el.append(
        Halvo.el("p", { class: "muted", text: w.DELETE_ABOUT }),
        Halvo.el("div", { class: "row" }, [start, result]),
        offerRow,
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
